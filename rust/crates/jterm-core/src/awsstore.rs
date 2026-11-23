//! Cuentas AWS (metadatos en JSON, clave secreta en el Keychain) y la llave de EC2 Instance Connect.

use crate::aws::{AwsAccount, AwsError};
use crate::sigv4::Credentials;
use crate::store::{JsonList, SecretStore};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

pub type AwsStore = JsonList<AwsAccount>;

/// Credenciales de una cuenta: el id de clave va en el JSON y la clave secreta en el almacén de secretos.
pub fn credentials(a: &AwsAccount, secrets: &dyn SecretStore) -> Result<Credentials, AwsError> {
    match secrets.get(a.id).filter(|s| !s.is_empty()) {
        Some(secret) if !a.access_key_id.is_empty() => Ok(Credentials { access_key: a.access_key_id.clone(), secret }),
        Some(_) => Err(AwsError("La cuenta no tiene ID de clave de acceso.".into())),
        None => Err(AwsError("No se pudo leer la clave secreta del Keychain.".into())),
    }
}

/// Inserta o reemplaza una cuenta. Un `secret` vacío o `None` conserva la clave ya guardada.
pub fn upsert(store: &mut AwsStore, a: AwsAccount, secret: Option<&str>, secrets: &dyn SecretStore) -> std::io::Result<()> {
    match store.items.iter().position(|x| x.id == a.id) {
        Some(i) => store.items[i] = a.clone(),
        None => store.items.push(a.clone()),
    }
    if let Some(s) = secret.filter(|s| !s.is_empty()) {
        secrets.set(a.id, s);
    }
    store.save()
}

pub fn remove(store: &mut AwsStore, id: Uuid, secrets: &dyn SecretStore) -> std::io::Result<()> {
    store.items.retain(|a| a.id != id);
    secrets.delete(id);
    store.save()
}

/// Llave SSH propia de jTerm, usada solo con EC2 Instance Connect (AWS la acepta 60 s por conexión).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EicKey {
    pub path: PathBuf,
    pub public: String,
}

/// Devuelve la llave, generándola con `ssh-keygen` la primera vez.
pub fn ensure_eic_key(dir: &Path) -> Result<EicKey, AwsError> {
    let path = dir.join("eic_ed25519");
    if !path.exists() {
        let ok = Command::new("/usr/bin/ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "jterm-instance-connect", "-f"])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err(AwsError("No se pudo generar la llave SSH (ssh-keygen).".into()));
        }
    }
    let mut pub_path = path.clone().into_os_string();
    pub_path.push(".pub");
    let public = std::fs::read_to_string(&pub_path).map_err(|_| AwsError("No se pudo leer la llave pública.".into()))?.trim().to_string();
    Ok(EicKey { path, public })
}

/// Opciones de `ssh` para entrar a una instancia con la llave de Instance Connect.
pub fn instance_ssh_options(key: &EicKey, instance_id: &str) -> Vec<String> {
    vec!["-i".into(), key.path.display().to_string(), "-o".into(), "IdentitiesOnly=yes".into(), "-o".into(), format!("HostKeyAlias={instance_id}")]
}

/// Usuarios recordados por instancia (`aws-users.json`: `{ "i-123": "ubuntu" }`).
pub type KnownUsers = JsonMap;

pub struct JsonMap {
    path: PathBuf,
    pub map: std::collections::BTreeMap<String, String>,
}

impl JsonMap {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let map = std::fs::read(&path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
        JsonMap { path, map }
    }

    pub fn set(&mut self, k: &str, v: &str) {
        self.map.insert(k.into(), v.into());
        self.save();
    }

    pub fn forget(&mut self, k: &str) {
        self.map.remove(k);
        self.save();
    }

    fn save(&self) {
        if let Ok(d) = serde_json::to_vec_pretty(&self.map) {
            let _ = crate::store::write_atomic(&self.path, &d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemorySecrets;

    fn account() -> AwsAccount {
        AwsAccount { id: Uuid::new_v4(), name: "prod".into(), access_key_id: "AKIA123".into(), region: "us-east-1".into(), env: None }
    }

    #[test]
    fn credentials_need_a_stored_secret() {
        let s = MemorySecrets::default();
        let a = account();
        assert!(credentials(&a, &s).is_err());
        s.set(a.id, "sek");
        let c = credentials(&a, &s).unwrap();
        assert_eq!((c.access_key.as_str(), c.secret.as_str()), ("AKIA123", "sek"));
        let mut no_key = a.clone();
        no_key.access_key_id.clear();
        assert!(credentials(&no_key, &s).is_err());
    }

    #[test]
    fn upsert_keeps_secret_unless_new_one_given() {
        let dir = tempfile::tempdir().unwrap();
        let mut st = AwsStore::open(dir.path().join("aws.json"));
        let s = MemorySecrets::default();
        let mut a = account();
        upsert(&mut st, a.clone(), Some("uno"), &s).unwrap();
        a.name = "renombrada".into();
        upsert(&mut st, a.clone(), None, &s).unwrap();
        upsert(&mut st, a.clone(), Some(""), &s).unwrap();
        assert_eq!((st.items.len(), st.items[0].name.as_str()), (1, "renombrada"));
        assert_eq!(s.get(a.id).as_deref(), Some("uno"));
        upsert(&mut st, a.clone(), Some("dos"), &s).unwrap();
        assert_eq!(s.get(a.id).as_deref(), Some("dos"));
        // Se relee igual desde el disco.
        assert_eq!(AwsStore::open(dir.path().join("aws.json")).items, st.items);
        remove(&mut st, a.id, &s).unwrap();
        assert!(st.items.is_empty() && s.get(a.id).is_none());
    }

    #[test]
    fn eic_key_is_generated_once_locally() {
        let dir = tempfile::tempdir().unwrap();
        let k1 = ensure_eic_key(dir.path()).unwrap();
        assert!(k1.public.starts_with("ssh-ed25519 "));
        let k2 = ensure_eic_key(dir.path()).unwrap();
        assert_eq!(k1, k2, "la segunda vez reutiliza la llave");
        let o = instance_ssh_options(&k1, "i-1");
        assert!(o.contains(&"HostKeyAlias=i-1".to_string()) && o.contains(&"IdentitiesOnly=yes".to_string()));
    }

    #[test]
    fn known_users_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.json");
        let mut m = JsonMap::open(&p);
        m.set("i-1", "ubuntu");
        assert_eq!(JsonMap::open(&p).map["i-1"], "ubuntu");
        m.forget("i-1");
        assert!(JsonMap::open(&p).map.is_empty());
    }
}
