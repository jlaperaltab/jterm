//! Adaptador de sincronización: lee y escribe los archivos de datos de jTerm (servidores, cuentas AWS, comandos y
//! ajustes) y los secretos del Keychain. Trabaja siempre sobre los archivos, así que puede ejecutarse en un hilo
//! de fondo; la aplicación recarga sus listas al terminar.

use crate::aws::AwsAccount;
use crate::models::{AuthMethod, Server, ServerEnv, Snippet};
use crate::settings::Settings;
use crate::store::{write_atomic, JsonList, SecretStore};
use crate::sync::{kind, command_natural_key, server_natural_key, JsonValue, SyncItem, SyncLocalAdapter};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

/// Ajustes propios de cada Mac: no viajan (shell, carpetas iniciales…).
pub const LOCAL_ONLY_SETTINGS: [&str; 5] = ["shell", "startDir", "filesLocalStart", "recordingDir", "language"];

pub struct FileAdapter {
    pub dir: PathBuf,
    pub secrets: Arc<dyn SecretStore>,
}

impl FileAdapter {
    pub fn new(dir: impl Into<PathBuf>, secrets: Arc<dyn SecretStore>) -> Self {
        FileAdapter { dir: dir.into(), secrets }
    }

    fn servers(&self) -> JsonList<Server> {
        JsonList::open(self.dir.join("servers.json"))
    }
    fn aws(&self) -> JsonList<AwsAccount> {
        JsonList::open(self.dir.join("aws.json"))
    }
    fn snippets(&self) -> JsonList<Snippet> {
        JsonList::open(self.dir.join("commands.json"))
    }
    fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    fn load_settings(&self) -> Settings {
        std::fs::read(self.settings_path()).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
    }
}

fn id_of(key: &str) -> Option<Uuid> {
    Uuid::parse_str(key.split_once(':')?.1).ok()
}

fn to_json_value(v: &Value) -> Option<JsonValue> {
    Some(match v {
        Value::Bool(b) => JsonValue::Bool(*b),
        Value::Number(n) => n.as_i64().map(JsonValue::Int).or_else(|| n.as_f64().map(JsonValue::Double))?,
        Value::String(s) => JsonValue::String(s.clone()),
        Value::Array(a) => JsonValue::Array(a.iter().filter_map(to_json_value).collect()),
        _ => return None,
    })
}

fn from_json_value(v: &JsonValue) -> Value {
    match v {
        JsonValue::Bool(b) => Value::Bool(*b),
        JsonValue::Int(i) => Value::from(*i),
        JsonValue::Double(d) => Value::from(*d),
        JsonValue::String(s) => Value::String(s.clone()),
        JsonValue::Array(a) => Value::Array(a.iter().map(from_json_value).collect()),
    }
}

fn s(v: &str) -> JsonValue {
    JsonValue::String(v.into())
}

/// Campos sincronizables de los ajustes (todos menos los propios de cada Mac).
pub fn settings_fields(settings: &Settings) -> BTreeMap<String, JsonValue> {
    let Value::Object(map) = serde_json::to_value(settings).unwrap_or(Value::Null) else { return BTreeMap::new() };
    map.iter().filter(|(k, _)| !LOCAL_ONLY_SETTINGS.contains(&k.as_str())).filter_map(|(k, v)| Some((k.clone(), to_json_value(v)?))).collect()
}

/// Ajustes locales con los campos recibidos encima (lo propio de cada Mac se conserva).
pub fn merge_settings(local: &Settings, fields: &BTreeMap<String, JsonValue>) -> Settings {
    let Value::Object(mut map) = serde_json::to_value(local).unwrap_or(Value::Null) else { return local.clone() };
    for (k, v) in fields {
        if !LOCAL_ONLY_SETTINGS.contains(&k.as_str()) && map.contains_key(k) {
            map.insert(k.clone(), from_json_value(v));
        }
    }
    serde_json::from_value(Value::Object(map)).unwrap_or_else(|_| local.clone())
}

impl SyncLocalAdapter for FileAdapter {
    fn snapshot(&mut self, enabled: &HashSet<String>, include_secrets: bool) -> BTreeMap<String, SyncItem> {
        let mut out = BTreeMap::new();
        let secret = |id: Uuid| if include_secrets { self.secrets.get(id).filter(|x| !x.is_empty()) } else { None };
        if enabled.contains(kind::SERVER) {
            for sv in self.servers().items {
                let mut f: BTreeMap<String, JsonValue> = BTreeMap::new();
                f.insert("name".into(), s(&sv.name));
                f.insert("host".into(), s(&sv.host));
                f.insert("port".into(), JsonValue::Int(sv.port as i64));
                f.insert("user".into(), s(&sv.user));
                f.insert("auth".into(), s(if sv.auth == AuthMethod::Key { "key" } else { "password" }));
                f.insert("keyPath".into(), s(&sv.key_path));
                if !sv.start_dir.is_empty() {
                    f.insert("startDir".into(), s(&sv.start_dir));
                }
                if let Some(e) = sv.env {
                    f.insert("env".into(), s(serde_json::to_value(e).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default().as_str()));
                }
                if let Some(j) = sv.jump {
                    f.insert("jump".into(), s(&j.to_string().to_uppercase()));
                }
                out.insert(
                    format!("server:{}", sv.id.to_string().to_uppercase()),
                    SyncItem { fields: f, secret: secret(sv.id), natural_key: Some(server_natural_key(&sv.host, &sv.user, sv.port)) },
                );
            }
        }
        if enabled.contains(kind::AWS) {
            for a in self.aws().items {
                let mut f: BTreeMap<String, JsonValue> = BTreeMap::new();
                f.insert("name".into(), s(&a.name));
                f.insert("accessKeyId".into(), s(&a.access_key_id));
                f.insert("region".into(), s(&a.region));
                if let Some(e) = a.env {
                    f.insert("env".into(), s(serde_json::to_value(e).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default().as_str()));
                }
                out.insert(format!("aws:{}", a.id.to_string().to_uppercase()), SyncItem { fields: f, secret: secret(a.id), natural_key: Some(a.access_key_id.clone()) });
            }
        }
        if enabled.contains(kind::COMMAND) {
            for c in self.snippets().items {
                let f = BTreeMap::from([("name".to_string(), s(&c.name)), ("command".to_string(), s(&c.command)), ("autoRun".to_string(), JsonValue::Bool(c.auto_run))]);
                out.insert(format!("cmd:{}", c.id.to_string().to_uppercase()), SyncItem { fields: f, secret: None, natural_key: Some(command_natural_key(&c.name, &c.command)) });
            }
        }
        if enabled.contains(kind::SETTINGS) {
            let fields = settings_fields(&self.load_settings());
            if !fields.is_empty() {
                out.insert("settings:main".into(), SyncItem { fields, secret: None, natural_key: None });
            }
        }
        out
    }

    fn rekey(&mut self, map: &BTreeMap<String, String>) {
        let (mut servers, mut aws, mut snippets) = (self.servers(), self.aws(), self.snippets());
        let (mut ds, mut da, mut dc) = (false, false, false);
        for (old, new) in map {
            let (Some(o), Some(n)) = (id_of(old), id_of(new)) else { continue };
            match kind::of(old) {
                kind::SERVER => {
                    if let Some(i) = servers.items.iter().position(|x| x.id == o) {
                        if let Some(sec) = self.secrets.get(o) {
                            self.secrets.set(n, &sec);
                            self.secrets.delete(o);
                        }
                        servers.items[i].id = n;
                        for x in servers.items.iter_mut().filter(|x| x.jump == Some(o)) {
                            x.jump = Some(n);
                        }
                        ds = true;
                    }
                }
                kind::AWS => {
                    if let Some(i) = aws.items.iter().position(|x| x.id == o) {
                        if let Some(sec) = self.secrets.get(o) {
                            self.secrets.set(n, &sec);
                            self.secrets.delete(o);
                        }
                        aws.items[i].id = n;
                        da = true;
                    }
                }
                kind::COMMAND => {
                    if let Some(i) = snippets.items.iter().position(|x| x.id == o) {
                        snippets.items[i].id = n;
                        dc = true;
                    }
                }
                _ => {}
            }
        }
        if ds {
            let _ = servers.save();
        }
        if da {
            let _ = aws.save();
        }
        if dc {
            let _ = snippets.save();
        }
    }

    fn apply(&mut self, upserts: &BTreeMap<String, SyncItem>, deletes: &[String], manage_secrets: bool) {
        if upserts.is_empty() && deletes.is_empty() {
            return;
        }
        let (mut servers, mut aws, mut snippets) = (self.servers(), self.aws(), self.snippets());
        let (mut ds, mut da, mut dc) = (false, false, false);
        let text = |f: &BTreeMap<String, JsonValue>, k: &str| f.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        for (key, item) in upserts {
            let f = &item.fields;
            let secret_for = |id: Uuid| {
                if manage_secrets {
                    self.secrets.set(id, item.secret.as_deref().unwrap_or(""));
                }
            };
            match kind::of(key) {
                kind::SERVER => {
                    let Some(id) = id_of(key) else { continue };
                    let mut sv = servers.items.iter().find(|x| x.id == id).cloned().unwrap_or_default();
                    sv.id = id;
                    sv.name = text(f, "name");
                    sv.host = text(f, "host");
                    sv.port = f.get("port").and_then(|v| v.as_int()).and_then(|p| u16::try_from(p).ok()).unwrap_or(22);
                    sv.user = text(f, "user");
                    sv.auth = if text(f, "auth") == "key" { AuthMethod::Key } else { AuthMethod::Password };
                    sv.key_path = text(f, "keyPath");
                    sv.start_dir = text(f, "startDir");
                    sv.env = f.get("env").and_then(|v| v.as_str()).and_then(|e| serde_json::from_value::<ServerEnv>(Value::String(e.into())).ok());
                    sv.jump = f.get("jump").and_then(|v| v.as_str()).and_then(|j| Uuid::parse_str(j).ok());
                    match servers.items.iter().position(|x| x.id == id) {
                        Some(i) => servers.items[i] = sv,
                        None => servers.items.push(sv),
                    }
                    secret_for(id);
                    ds = true;
                }
                kind::AWS => {
                    let Some(id) = id_of(key) else { continue };
                    let a = AwsAccount {
                        id,
                        name: text(f, "name"),
                        access_key_id: text(f, "accessKeyId"),
                        region: f.get("region").and_then(|v| v.as_str()).unwrap_or("us-east-1").to_string(),
                        env: f.get("env").and_then(|v| v.as_str()).and_then(|e| serde_json::from_value::<ServerEnv>(Value::String(e.into())).ok()),
                    };
                    match aws.items.iter().position(|x| x.id == id) {
                        Some(i) => aws.items[i] = a,
                        None => aws.items.push(a),
                    }
                    secret_for(id);
                    da = true;
                }
                kind::COMMAND => {
                    let Some(id) = id_of(key) else { continue };
                    let c = Snippet { id, name: text(f, "name"), command: text(f, "command"), auto_run: f.get("autoRun").and_then(|v| v.as_bool()).unwrap_or(false) };
                    match snippets.items.iter().position(|x| x.id == id) {
                        Some(i) => snippets.items[i] = c,
                        None => snippets.items.push(c),
                    }
                    dc = true;
                }
                kind::SETTINGS => {
                    let merged = merge_settings(&self.load_settings(), f);
                    if let Ok(d) = serde_json::to_vec_pretty(&merged) {
                        let _ = write_atomic(&self.settings_path(), &d);
                    }
                }
                _ => {}
            }
        }
        for key in deletes {
            let Some(id) = id_of(key) else { continue };
            match kind::of(key) {
                kind::SERVER => {
                    servers.items.retain(|x| x.id != id);
                    self.secrets.delete(id);
                    ds = true;
                }
                kind::AWS => {
                    aws.items.retain(|x| x.id != id);
                    self.secrets.delete(id);
                    da = true;
                }
                kind::COMMAND => {
                    snippets.items.retain(|x| x.id != id);
                    dc = true;
                }
                _ => {}
            }
        }
        if ds {
            let _ = servers.save();
        }
        if da {
            let _ = aws.save();
        }
        if dc {
            let _ = snippets.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemorySecrets;
    use crate::sync::{run_with, SyncState, SyncStorage};
    use chrono::{DateTime, Duration, TimeZone, Utc};

    const IT: u32 = 1000;

    fn t(min: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap() + Duration::minutes(min)
    }

    fn all() -> HashSet<String> {
        [kind::SERVER, kind::AWS, kind::COMMAND, kind::SETTINGS].iter().map(|k| k.to_string()).collect()
    }

    struct Mac {
        dir: tempfile::TempDir,
        secrets: Arc<MemorySecrets>,
        state: SyncState,
    }

    impl Mac {
        fn new() -> Self {
            Mac { dir: tempfile::tempdir().unwrap(), secrets: Arc::new(MemorySecrets::default()), state: SyncState::default() }
        }
        fn adapter(&self) -> FileAdapter {
            FileAdapter::new(self.dir.path(), self.secrets.clone())
        }
        fn sync(&mut self, st: &SyncStorage, secrets: bool, now: DateTime<Utc>) -> crate::sync::SyncResult {
            let mut a = self.adapter();
            let (r, ns) = run_with(&mut a, st, "clave", &self.state, &all(), secrets, now, IT).unwrap();
            self.state = ns;
            r
        }
    }

    #[test]
    fn two_macs_share_servers_aws_commands_and_settings() {
        let cloud = tempfile::tempdir().unwrap();
        let st = SyncStorage::new(cloud.path().join("jTerm"));
        let (mut a, mut b) = (Mac::new(), Mac::new());

        let jump = Server { name: "bastion".into(), host: "Bastion.X".into(), user: "ops".into(), env: Some(ServerEnv::Prod), ..Server::default() };
        let web = Server { name: "web".into(), host: "web.x".into(), auth: AuthMethod::Key, key_path: "~/.ssh/id".into(), jump: Some(jump.id), ..Server::default() };
        let mut list = JsonList::<Server>::open(a.dir.path().join("servers.json"));
        list.items = vec![jump.clone(), web.clone()];
        list.save().unwrap();
        let acct = AwsAccount { id: Uuid::new_v4(), name: "prod".into(), access_key_id: "AKIA1".into(), region: "eu-west-1".into(), env: Some(ServerEnv::Staging) };
        let mut al = JsonList::<AwsAccount>::open(a.dir.path().join("aws.json"));
        al.items = vec![acct.clone()];
        al.save().unwrap();
        let mut cl = JsonList::<Snippet>::open(a.dir.path().join("commands.json"));
        cl.items = vec![Snippet { auto_run: true, ..Snippet::new("reiniciar", "sudo reboot") }];
        cl.save().unwrap();
        let mut cfg = Settings::default();
        cfg.theme = "Nord".into();
        cfg.shell = "/bin/bash".into(); // propio de este Mac
        cfg.start_dir = "/Users/a".into();
        write_atomic(&a.dir.path().join("settings.json"), &serde_json::to_vec(&cfg).unwrap()).unwrap();
        a.secrets.set(web.id, "pw-web");
        a.secrets.set(acct.id, "aws-secret");

        // Sin secretos: llegan los datos pero no las contraseñas.
        let r = a.sync(&st, false, t(0));
        assert!(r.uploaded >= 5, "{r:?}");
        let r = b.sync(&st, false, t(1));
        assert!(r.downloaded >= 5, "{r:?}");
        let servers = JsonList::<Server>::open(b.dir.path().join("servers.json")).items;
        let got_web = servers.iter().find(|s| s.id == web.id).unwrap();
        assert_eq!((got_web.auth, got_web.key_path.as_str(), got_web.jump), (AuthMethod::Key, "~/.ssh/id", Some(jump.id)));
        assert_eq!(servers.iter().find(|s| s.id == jump.id).unwrap().env, Some(ServerEnv::Prod));
        assert_eq!(JsonList::<AwsAccount>::open(b.dir.path().join("aws.json")).items, [acct.clone()]);
        let cmds = JsonList::<Snippet>::open(b.dir.path().join("commands.json")).items;
        assert_eq!((cmds[0].command.as_str(), cmds[0].auto_run), ("sudo reboot", true));
        assert!(b.secrets.get(web.id).is_none(), "las contraseñas no viajan si no se pidió");
        // Los ajustes llegan, salvo lo propio de cada Mac.
        let bs: Settings = serde_json::from_slice(&std::fs::read(b.dir.path().join("settings.json")).unwrap()).unwrap();
        assert_eq!((bs.theme.as_str(), bs.shell.as_str(), bs.start_dir.as_str()), ("Nord", "", ""));

        // Con secretos en ambos Macs, la contraseña también llega.
        a.sync(&st, true, t(2));
        b.sync(&st, true, t(3));
        assert_eq!(b.secrets.get(web.id).as_deref(), Some("pw-web"));
        assert_eq!(b.secrets.get(acct.id).as_deref(), Some("aws-secret"));

        // Un borrado en B llega a A, con su secreto.
        let mut bl = JsonList::<Server>::open(b.dir.path().join("servers.json"));
        bl.items.retain(|s| s.id != web.id);
        bl.save().unwrap();
        b.sync(&st, true, t(4));
        a.sync(&st, true, t(5));
        let left: Vec<String> = JsonList::<Server>::open(a.dir.path().join("servers.json")).items.into_iter().map(|s| s.name).collect();
        assert_eq!(left, ["bastion"]);
        assert!(a.secrets.get(web.id).is_none());
    }

    #[test]
    fn same_server_created_on_both_macs_is_not_duplicated_and_keeps_its_password() {
        let cloud = tempfile::tempdir().unwrap();
        let st = SyncStorage::new(cloud.path().join("jTerm"));
        let (mut a, mut b) = (Mac::new(), Mac::new());
        let sa = Server { name: "db".into(), host: "DB.interna".into(), user: "root".into(), ..Server::default() };
        let sb = Server { name: "db".into(), host: "db.interna".into(), user: "root".into(), ..Server::default() };
        for (m, sv) in [(&a, &sa), (&b, &sb)] {
            let mut l = JsonList::<Server>::open(m.dir.path().join("servers.json"));
            l.items = vec![sv.clone()];
            l.save().unwrap();
            m.secrets.set(sv.id, "pw");
        }
        a.sync(&st, true, t(0));
        b.sync(&st, true, t(1));
        let after = JsonList::<Server>::open(b.dir.path().join("servers.json")).items;
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, sa.id, "B adopta la identidad del servidor de A");
        assert_eq!(b.secrets.get(sa.id).as_deref(), Some("pw"), "la contraseña se movió a la nueva identidad");
        assert!(b.secrets.get(sb.id).is_none());
    }

    #[test]
    fn settings_helpers() {
        let mut s = Settings::default();
        s.shell = "/bin/zsh".into();
        s.font_size = 15;
        let f = settings_fields(&s);
        assert!(!f.contains_key("shell") && !f.contains_key("startDir"));
        assert_eq!(f["fontSize"], JsonValue::Int(15));
        let mut other = Settings::default();
        other.shell = "/opt/x".into();
        let merged = merge_settings(&other, &f);
        assert_eq!((merged.font_size, merged.shell.as_str()), (15, "/opt/x"));
        // Campos desconocidos o de tipo equivocado no rompen nada.
        let weird = BTreeMap::from([("nope".to_string(), JsonValue::Int(1)), ("theme".to_string(), JsonValue::Int(3))]);
        assert_eq!(merge_settings(&other, &weird), other);
    }
}
