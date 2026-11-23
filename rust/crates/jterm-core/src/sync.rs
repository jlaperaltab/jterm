//! Sincronización entre equipos mediante un archivo cifrado en una carpeta (p. ej. iCloud Drive).
//!
//! Port del motor de combinación de la versión Swift. Las fechas se guardan como segundos desde 2001-01-01
//! (el formato por defecto de `JSONEncoder` en Swift), así que ambas versiones leen el mismo archivo.
//! Los hashes de `SyncState` NO son compatibles byte a byte con los de Swift: la primera sincronización tras
//! migrar tratará los elementos como "editados" y se resolverá por fecha (sin pérdida de datos).

use crate::crypto::{self, CryptoError};
use crate::store::write_atomic;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

// MARK: Fechas al estilo Swift

mod swift_date {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    const REF: f64 = 978_307_200.0; // 2001-01-01 en tiempo Unix

    pub fn serialize<S: Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(d.timestamp_micros() as f64 / 1e6 - REF)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let x = f64::deserialize(d)?;
        Ok(DateTime::from_timestamp_micros(((x + REF) * 1e6).round() as i64).unwrap_or_default())
    }
}

// MARK: Modelo

/// Valor JSON simple para guardar campos de cualquier tipo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonValue {
    Bool(bool),
    Int(i64),
    Double(f64),
    String(String),
    Array(Vec<JsonValue>),
}

impl JsonValue {
    pub fn as_str(&self) -> Option<&str> {
        if let JsonValue::String(s) = self { Some(s) } else { None }
    }
    pub fn as_int(&self) -> Option<i64> {
        match self { JsonValue::Int(i) => Some(*i), JsonValue::Double(d) => Some(*d as i64), _ => None }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let JsonValue::Bool(b) = self { Some(*b) } else { None }
    }
}

/// Un elemento sincronizable (servidor, cuenta AWS, comando o ajustes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncItem {
    pub fields: BTreeMap<String, JsonValue>,
    /// Contraseña / passphrase / clave secreta (solo si el usuario eligió sincronizarlas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    /// Identifica "lo mismo" entre equipos que lo crearon por separado.
    #[serde(default, rename = "naturalKey", skip_serializing_if = "Option::is_none")]
    pub natural_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<SyncItem>,
    #[serde(rename = "updatedAt", with = "swift_date")]
    pub updated_at: DateTime<Utc>,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncDocument {
    pub format: String,
    pub version: u32,
    pub records: BTreeMap<String, SyncRecord>,
}

impl Default for SyncDocument {
    fn default() -> Self {
        SyncDocument { format: "jterm-sync".into(), version: 1, records: BTreeMap::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateEntry {
    /// Contenido sin secreto.
    pub hash: String,
    /// Contenido con secreto, solo si ese equipo sincronizaba secretos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,
    #[serde(rename = "updatedAt", with = "swift_date")]
    pub updated_at: DateTime<Utc>,
    pub deleted: bool,
}

/// Lo que este equipo recuerda de la última sincronización.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncState {
    pub entries: BTreeMap<String, StateEntry>,
}

pub mod kind {
    pub const SERVER: &str = "server";
    pub const AWS: &str = "aws";
    pub const COMMAND: &str = "cmd";
    pub const SETTINGS: &str = "settings";

    /// Tipo de una clave "tipo:id".
    pub fn of(key: &str) -> &str {
        key.split(':').next().unwrap_or("")
    }
}

/// Claves naturales (para no duplicar lo que dos equipos crearon por separado).
pub fn server_natural_key(host: &str, user: &str, port: u16) -> String {
    format!("{}|{user}|{port}", host.to_lowercase())
}
pub fn command_natural_key(name: &str, command: &str) -> String {
    format!("{name}|{command}")
}

// MARK: Motor de combinación (sin entrada/salida)

#[derive(Debug, Clone)]
pub struct MergeOutcome {
    pub document: SyncDocument,
    /// A aplicar en este equipo.
    pub upserts: BTreeMap<String, SyncItem>,
    /// A borrar en este equipo.
    pub deletes: Vec<String>,
    pub state: SyncState,
}

pub fn hash(item: &SyncItem, include_secret: bool) -> String {
    let mut i = item.clone();
    if !include_secret {
        i.secret = None;
    }
    // `fields` es un BTreeMap: las claves salen ordenadas, así que el JSON es determinista.
    let data = serde_json::to_vec(&i).unwrap_or_default();
    Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Elementos creados por separado en dos equipos que son "lo mismo" (misma clave natural): el local adopta
/// la identidad del remoto. Devuelve `{claveLocal: claveRemota}`.
pub fn find_rekeys(
    local: &BTreeMap<String, SyncItem>, remote: Option<&SyncDocument>, state: &SyncState, enabled: &HashSet<String>,
) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(remote) = remote else { return map };
    let mut taken: HashSet<&str> = HashSet::new();
    for (rkey, rec) in &remote.records {
        let Some(nk) = rec.item.as_ref().and_then(|i| i.natural_key.as_ref()).filter(|_| !rec.deleted) else { continue };
        if !enabled.contains(kind::of(rkey)) || local.contains_key(rkey) || state.entries.contains_key(rkey) {
            continue;
        }
        let found = local.iter().find(|(lk, li)| {
            kind::of(lk) == kind::of(rkey)
                && li.natural_key.as_ref() == Some(nk)
                && !remote.records.contains_key(*lk)
                && !state.entries.contains_key(*lk)
                && !taken.contains(lk.as_str())
        });
        if let Some((lkey, _)) = found {
            map.insert(lkey.clone(), rkey.clone());
            taken.insert(lkey);
        }
    }
    map
}

pub const TOMBSTONE_TTL_DAYS: i64 = 90;

pub fn reconcile(
    local: &BTreeMap<String, SyncItem>, remote: Option<&SyncDocument>, state: &SyncState, enabled: &HashSet<String>,
    manage_secrets: bool, now: DateTime<Utc>,
) -> MergeOutcome {
    let empty = BTreeMap::new();
    let remote_recs = remote.map(|r| &r.records).unwrap_or(&empty);
    let h = |i: &SyncItem| hash(i, manage_secrets);
    let entry_for = |r: &SyncRecord| match &r.item {
        None => StateEntry { hash: String::new(), full: None, updated_at: r.updated_at, deleted: true },
        Some(it) => StateEntry {
            hash: hash(it, false),
            full: manage_secrets.then(|| hash(it, true)),
            updated_at: r.updated_at,
            deleted: false,
        },
    };
    // ¿Este elemento local sigue igual que en la última sincronización?
    let unchanged = |key: &str, item: &SyncItem| -> bool {
        let Some(st) = state.entries.get(key).filter(|s| !s.deleted) else { return false };
        if manage_secrets {
            if let Some(full) = &st.full {
                return &hash(item, true) == full;
            }
            return item.secret.is_none() && hash(item, false) == st.hash; // antes no se seguían los secretos
        }
        hash(item, false) == st.hash
    };

    // 1) Estado local como registros con fecha de modificación
    let mut local_recs: BTreeMap<String, SyncRecord> = BTreeMap::new();
    let mut local_unchanged: HashSet<&str> = HashSet::new();
    for (key, item) in local {
        if !enabled.contains(kind::of(key)) {
            continue;
        }
        match state.entries.get(key) {
            Some(st) if unchanged(key, item) => {
                local_recs.insert(key.clone(), SyncRecord { item: Some(item.clone()), updated_at: st.updated_at, deleted: false });
                local_unchanged.insert(key);
            }
            _ => {
                local_recs.insert(key.clone(), SyncRecord { item: Some(item.clone()), updated_at: now, deleted: false });
            }
        }
    }
    for (key, st) in &state.entries {
        if enabled.contains(kind::of(key)) && !local.contains_key(key) {
            let at = if st.deleted { st.updated_at } else { now };
            local_recs.insert(key.clone(), SyncRecord { item: None, updated_at: at, deleted: true }); // borrado
        }
    }

    // 2) Combinar
    let mut out = MergeOutcome { document: SyncDocument::default(), upserts: BTreeMap::new(), deletes: vec![], state: SyncState::default() };
    let keys: BTreeSet<&String> = local_recs.keys().chain(remote_recs.keys()).collect();
    for key in keys {
        let k = kind::of(key);
        if !enabled.contains(k) {
            // Tipos que este equipo no sincroniza: se respetan.
            if let Some(r) = remote_recs.get(key) {
                out.document.records.insert(key.clone(), r.clone());
            }
            if let Some(st) = state.entries.get(key) {
                out.state.entries.insert(key.clone(), st.clone());
            }
            continue;
        }
        let (l, r) = (local_recs.get(key), remote_recs.get(key));
        let mut from_remote = false;
        let mut winner: SyncRecord = match (l, r) {
            (Some(l), None) => l.clone(),
            (None, Some(r)) => { from_remote = true; r.clone() }
            (Some(l), Some(r)) => {
                let same = (l.deleted && r.deleted)
                    || (!l.deleted && !r.deleted && l.item.as_ref().map(h) == r.item.as_ref().map(h));
                let first_sync = !state.entries.contains_key(key);
                if same {
                    if l.updated_at >= r.updated_at { l.clone() } else { r.clone() }
                } else if k == kind::SETTINGS && first_sync {
                    from_remote = true; // un equipo nuevo no pisa los ajustes
                    r.clone()
                } else if local_unchanged.contains(key.as_str()) {
                    // Lo que no cambió aquí nunca pisa lo de otro equipo.
                    if r.updated_at >= l.updated_at { from_remote = true; r.clone() } else { l.clone() }
                } else if r.updated_at > l.updated_at {
                    from_remote = true;
                    r.clone()
                } else {
                    l.clone()
                }
            }
            (None, None) => continue,
        };
        if winner.deleted && now - winner.updated_at > Duration::days(TOMBSTONE_TTL_DAYS) {
            continue; // tumba antigua: se descarta
        }
        if !from_remote && !manage_secrets {
            // No pisar secretos que este equipo no gestiona.
            if let (Some(it), Some(rs)) = (winner.item.as_mut(), r.and_then(|r| r.item.as_ref()).and_then(|i| i.secret.clone())) {
                it.secret = Some(rs);
            }
        }
        out.state.entries.insert(key.clone(), entry_for(&winner));
        if from_remote {
            if winner.deleted {
                if local.contains_key(key) {
                    out.deletes.push(key.clone());
                }
            } else if let Some(it) = &winner.item {
                if local.get(key).map(h).as_deref() != Some(h(it).as_str()) {
                    out.upserts.insert(key.clone(), it.clone());
                }
            }
        }
        out.document.records.insert(key.clone(), winner);
    }
    out.deletes.sort();
    out
}

/// Une dos documentos (p. ej. una copia en conflicto de iCloud): gana la fecha más reciente de cada elemento.
pub fn merge_documents(a: &SyncDocument, b: &SyncDocument) -> SyncDocument {
    let mut out = a.clone();
    for (k, r) in &b.records {
        if out.records.get(k).map(|x| x.updated_at < r.updated_at).unwrap_or(true) {
            out.records.insert(k.clone(), r.clone());
        }
    }
    out
}

// MARK: Almacenamiento (archivo cifrado en una carpeta)

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("La contraseña de sincronización no coincide con la del archivo.")]
    BadPassphrase,
    #[error("iCloud todavía está descargando el archivo de sincronización. Se reintentará en un momento.")]
    Downloading,
    #[error("El archivo de sincronización no se puede leer.")]
    Unreadable,
    #[error("No se encontró la carpeta de iCloud Drive.")]
    NoFolder,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub const FILE_NAME: &str = "sync.jterm";

/// `~/Library/Mobile Documents/com~apple~CloudDocs/jTerm` si iCloud Drive existe.
pub fn icloud_folder() -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("HOME")?).join("Library/Mobile Documents/com~apple~CloudDocs");
    base.is_dir().then(|| base.join("jTerm"))
}

pub struct SyncStorage {
    pub folder: PathBuf,
}

impl SyncStorage {
    pub fn new(folder: impl Into<PathBuf>) -> Self {
        SyncStorage { folder: folder.into() }
    }

    pub fn main_path(&self) -> PathBuf {
        self.folder.join(FILE_NAME)
    }

    fn placeholder(&self) -> PathBuf {
        self.folder.join(format!(".{FILE_NAME}.icloud"))
    }

    /// Archivo principal presente (o su marcador de "aún no descargado").
    pub fn has_data(&self) -> bool {
        self.main_path().exists() || self.placeholder().exists()
    }

    /// Copias en conflicto que crea iCloud: "sync 2.jterm", "sync 3.jterm"…
    fn conflicts(&self) -> Vec<PathBuf> {
        let mut v: Vec<String> = std::fs::read_dir(&self.folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| {
                n.strip_prefix("sync ").and_then(|r| r.strip_suffix(".jterm")).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
            })
            .collect();
        v.sort();
        v.into_iter().map(|n| self.folder.join(n)).collect()
    }

    fn decode(path: &Path, passphrase: &str) -> Result<SyncDocument, SyncError> {
        let data = std::fs::read(path)?;
        let env = crypto::envelope(&data).ok_or(SyncError::Unreadable)?;
        let plain = crypto::decrypt(&env, passphrase).map_err(|e| match e {
            CryptoError::Unsupported => SyncError::Unreadable,
            _ => SyncError::BadPassphrase,
        })?;
        let doc: SyncDocument = serde_json::from_slice(&plain).map_err(|_| SyncError::Unreadable)?;
        if doc.format != "jterm-sync" {
            return Err(SyncError::Unreadable);
        }
        Ok(doc)
    }

    /// Lee el documento y combina las copias en conflicto. `None` si todavía no existe.
    pub fn read(&self, passphrase: &str) -> Result<(Option<SyncDocument>, Vec<PathBuf>), SyncError> {
        if !self.main_path().exists() {
            if self.placeholder().exists() {
                return Err(SyncError::Downloading);
            }
            return Ok((None, vec![]));
        }
        let mut doc = Self::decode(&self.main_path(), passphrase)?;
        let conflicts = self.conflicts();
        for u in &conflicts {
            if let Ok(d) = Self::decode(u, passphrase) {
                doc = merge_documents(&doc, &d);
            }
        }
        Ok((Some(doc), conflicts))
    }

    pub fn write(&self, doc: &SyncDocument, passphrase: &str, remove: &[PathBuf]) -> Result<(), SyncError> {
        self.write_with(doc, passphrase, remove, crypto::ITERATIONS)
    }

    pub fn write_with(&self, doc: &SyncDocument, passphrase: &str, remove: &[PathBuf], iterations: u32) -> Result<(), SyncError> {
        std::fs::create_dir_all(&self.folder)?;
        let json = serde_json::to_vec(doc).map_err(std::io::Error::other)?;
        let data = crypto::encrypt_with(&json, passphrase, iterations).map_err(|_| SyncError::Unreadable)?;
        write_atomic(&self.main_path(), &data)?;
        for u in remove {
            let _ = std::fs::remove_file(u);
        }
        Ok(())
    }

    /// Comprueba que la contraseña sirve para el archivo existente.
    pub fn verify(&self, passphrase: &str) -> Result<(), SyncError> {
        self.read(passphrase).map(|_| ())
    }

    pub fn delete(&self) {
        let _ = std::fs::remove_file(self.main_path());
        for u in self.conflicts() {
            let _ = std::fs::remove_file(u);
        }
    }
}

// MARK: Ejecución de una sincronización

pub trait SyncLocalAdapter {
    fn snapshot(&mut self, enabled: &HashSet<String>, include_secrets: bool) -> BTreeMap<String, SyncItem>;
    fn rekey(&mut self, map: &BTreeMap<String, String>);
    fn apply(&mut self, upserts: &BTreeMap<String, SyncItem>, deletes: &[String], manage_secrets: bool);
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncResult {
    pub uploaded: usize,
    pub downloaded: usize,
    pub deleted: usize,
}

/// Una sincronización completa y síncrona (llamarla fuera del hilo de la interfaz).
pub fn run(
    adapter: &mut dyn SyncLocalAdapter, storage: &SyncStorage, passphrase: &str, state: &SyncState,
    enabled: &HashSet<String>, manage_secrets: bool, now: DateTime<Utc>,
) -> Result<(SyncResult, SyncState), SyncError> {
    run_with(adapter, storage, passphrase, state, enabled, manage_secrets, now, crypto::ITERATIONS)
}

#[allow(clippy::too_many_arguments)]
pub fn run_with(
    adapter: &mut dyn SyncLocalAdapter, storage: &SyncStorage, passphrase: &str, state: &SyncState,
    enabled: &HashSet<String>, manage_secrets: bool, now: DateTime<Utc>, iterations: u32,
) -> Result<(SyncResult, SyncState), SyncError> {
    let (remote, conflicts) = storage.read(passphrase)?;
    let mut local = adapter.snapshot(enabled, manage_secrets);
    let rekeys = find_rekeys(&local, remote.as_ref(), state, enabled);
    if !rekeys.is_empty() {
        adapter.rekey(&rekeys);
        local = adapter.snapshot(enabled, manage_secrets);
    }
    // Un cambio hecho después de ver los de otro equipo debe ganar aunque el reloj de este vaya atrasado.
    let latest_seen = remote.as_ref().and_then(|r| r.records.values().map(|x| x.updated_at).max());
    let stamp = latest_seen.map_or(now, |l| now.max(l + Duration::milliseconds(1)));
    let outcome = reconcile(&local, remote.as_ref(), state, enabled, manage_secrets, stamp);
    adapter.apply(&outcome.upserts, &outcome.deletes, manage_secrets);

    let mut result = SyncResult { downloaded: outcome.upserts.len(), deleted: outcome.deletes.len(), uploaded: 0 };
    if Some(&outcome.document) != remote.as_ref() || !conflicts.is_empty() {
        result.uploaded = outcome
            .document
            .records
            .iter()
            .filter(|(k, r)| {
                enabled.contains(kind::of(k))
                    && remote.as_ref().and_then(|d| d.records.get(*k)) != Some(*r)
                    && !(outcome.upserts.contains_key(*k) || (r.deleted && outcome.deletes.contains(k)))
            })
            .count();
        storage.write_with(&outcome.document, passphrase, &conflicts, iterations)?;
    }
    Ok((result, outcome.state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const IT: u32 = 1000; // PBKDF2 rápido en tests

    fn t(min: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + Duration::minutes(min)
    }

    fn item(name: &str, nk: Option<&str>) -> SyncItem {
        SyncItem {
            fields: BTreeMap::from([("name".to_string(), JsonValue::String(name.into()))]),
            secret: None,
            natural_key: nk.map(String::from),
        }
    }

    fn all() -> HashSet<String> {
        [kind::SERVER, kind::AWS, kind::COMMAND, kind::SETTINGS].iter().map(|s| s.to_string()).collect()
    }

    /// Un "equipo": datos locales y estado de sincronización.
    #[derive(Default)]
    struct Device {
        items: BTreeMap<String, SyncItem>,
        state: SyncState,
    }

    impl SyncLocalAdapter for Device {
        fn snapshot(&mut self, enabled: &HashSet<String>, _s: bool) -> BTreeMap<String, SyncItem> {
            self.items.iter().filter(|(k, _)| enabled.contains(kind::of(k))).map(|(k, v)| (k.clone(), v.clone())).collect()
        }
        fn rekey(&mut self, map: &BTreeMap<String, String>) {
            for (old, new) in map {
                if let Some(v) = self.items.remove(old) {
                    self.items.insert(new.clone(), v);
                }
            }
        }
        fn apply(&mut self, upserts: &BTreeMap<String, SyncItem>, deletes: &[String], _m: bool) {
            for (k, v) in upserts {
                self.items.insert(k.clone(), v.clone());
            }
            for k in deletes {
                self.items.remove(k);
            }
        }
    }

    fn sync(d: &mut Device, st: &SyncStorage, now: DateTime<Utc>) -> SyncResult {
        let state = d.state.clone();
        let (res, new_state) = run_with(d, st, "clave", &state, &all(), false, now, IT).unwrap();
        d.state = new_state;
        res
    }

    fn storage() -> (tempfile::TempDir, SyncStorage) {
        let dir = tempfile::tempdir().unwrap();
        let st = SyncStorage::new(dir.path().join("jTerm"));
        (dir, st)
    }

    #[test]
    fn two_devices_converge() {
        let (_d, st) = storage();
        let (mut a, mut b) = (Device::default(), Device::default());
        a.items.insert("server:1".into(), item("web", None));
        let r = sync(&mut a, &st, t(0));
        assert_eq!(r, SyncResult { uploaded: 1, downloaded: 0, deleted: 0 });

        let r = sync(&mut b, &st, t(1));
        assert_eq!(r.downloaded, 1);
        assert_eq!(b.items["server:1"], item("web", None));

        // B edita; A lo recibe.
        b.items.insert("server:1".into(), item("web2", None));
        sync(&mut b, &st, t(2));
        sync(&mut a, &st, t(3));
        assert_eq!(a.items["server:1"], item("web2", None));

        // A borra; B lo recibe.
        a.items.remove("server:1");
        sync(&mut a, &st, t(4));
        let r = sync(&mut b, &st, t(5));
        assert_eq!(r.deleted, 1);
        assert!(b.items.is_empty());
    }

    #[test]
    fn newest_edit_wins_and_clock_skew_is_handled() {
        let (_d, st) = storage();
        let (mut a, mut b) = (Device::default(), Device::default());
        a.items.insert("cmd:1".into(), item("x", None));
        sync(&mut a, &st, t(100));
        sync(&mut b, &st, t(101));
        // El reloj de B va atrasado, pero su edición es posterior a lo que vio.
        b.items.insert("cmd:1".into(), item("editado en B", None));
        sync(&mut b, &st, t(0));
        sync(&mut a, &st, t(102));
        assert_eq!(a.items["cmd:1"], item("editado en B", None));
    }

    #[test]
    fn natural_key_avoids_duplicates() {
        let (_d, st) = storage();
        let (mut a, mut b) = (Device::default(), Device::default());
        a.items.insert("server:A".into(), item("web", Some("h|u|22")));
        sync(&mut a, &st, t(0));
        b.items.insert("server:B".into(), item("web", Some("h|u|22")));
        sync(&mut b, &st, t(1));
        assert_eq!(b.items.keys().collect::<Vec<_>>(), ["server:A"]);
        sync(&mut a, &st, t(2));
        assert_eq!(a.items.len(), 1);
        assert_eq!(server_natural_key("Host.COM", "u", 22), "host.com|u|22");
    }

    #[test]
    fn new_device_does_not_override_settings() {
        let (_d, st) = storage();
        let (mut a, mut b) = (Device::default(), Device::default());
        a.items.insert("settings:main".into(), item("tema A", None));
        sync(&mut a, &st, t(0));
        b.items.insert("settings:main".into(), item("tema B (por defecto)", None));
        sync(&mut b, &st, t(5));
        assert_eq!(b.items["settings:main"], item("tema A", None));
    }

    #[test]
    fn wrong_passphrase_and_unreadable() {
        let (_d, st) = storage();
        let mut a = Device::default();
        a.items.insert("cmd:1".into(), item("x", None));
        sync(&mut a, &st, t(0));
        assert!(matches!(st.read("otra"), Err(SyncError::BadPassphrase)));
        assert!(st.verify("clave").is_ok());
        std::fs::write(st.main_path(), "basura").unwrap();
        assert!(matches!(st.read("clave"), Err(SyncError::Unreadable)));
        st.delete();
        assert!(!st.has_data());
    }

    #[test]
    fn conflict_copies_are_merged_and_removed() {
        let (_d, st) = storage();
        let mut a = Device::default();
        a.items.insert("cmd:1".into(), item("uno", None));
        sync(&mut a, &st, t(0));
        // Una copia en conflicto con otro elemento más reciente.
        let mut doc = SyncDocument::default();
        doc.records.insert("cmd:2".into(), SyncRecord { item: Some(item("dos", None)), updated_at: t(10), deleted: false });
        let other = SyncStorage::new(st.folder.clone());
        let conflict = st.folder.join("sync 2.jterm");
        let data = crypto::encrypt_with(&serde_json::to_vec(&doc).unwrap(), "clave", IT).unwrap();
        std::fs::write(&conflict, data).unwrap();
        let (merged, conflicts) = other.read("clave").unwrap();
        assert_eq!(conflicts, [conflict.clone()]);
        assert_eq!(merged.unwrap().records.len(), 2);
        sync(&mut a, &st, t(11));
        assert!(!conflict.exists());
        assert_eq!(a.items.len(), 2);
    }

    #[test]
    fn old_tombstones_are_dropped() {
        let state = SyncState {
            entries: BTreeMap::from([(
                "cmd:1".to_string(),
                StateEntry { hash: String::new(), full: None, updated_at: t(0), deleted: true },
            )]),
        };
        let now = t(0) + Duration::days(91);
        let out = reconcile(&BTreeMap::new(), None, &state, &all(), false, now);
        assert!(out.document.records.is_empty() && out.state.entries.is_empty());
    }

    #[test]
    fn disabled_kinds_are_preserved() {
        let mut doc = SyncDocument::default();
        doc.records.insert("aws:1".into(), SyncRecord { item: Some(item("cuenta", None)), updated_at: t(0), deleted: false });
        let enabled: HashSet<String> = [kind::SERVER.to_string()].into();
        let out = reconcile(&BTreeMap::new(), Some(&doc), &SyncState::default(), &enabled, false, t(1));
        assert!(out.document.records.contains_key("aws:1"));
        assert!(out.upserts.is_empty());
    }

    #[test]
    fn dates_use_swift_reference() {
        let r = SyncRecord { item: None, updated_at: Utc.with_ymd_and_hms(2001, 1, 1, 0, 0, 10).unwrap(), deleted: true };
        let j = serde_json::to_string(&r).unwrap();
        assert_eq!(j, r#"{"updatedAt":10.0,"deleted":true}"#);
        assert_eq!(serde_json::from_str::<SyncRecord>(&j).unwrap(), r);
    }
}
