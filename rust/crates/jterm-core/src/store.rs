//! Persistencia: listas JSON (servidores, comandos, túneles) y secretos en el Keychain.

use crate::models::Server;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

/// Carpeta de datos (`~/Library/Application Support/jTerm` en macOS), creada con permisos 0700.
pub fn data_dir() -> std::io::Result<PathBuf> {
    let base = std::env::var_os("JTERM_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        if cfg!(target_os = "macos") { home.join("Library/Application Support/jTerm") } else { home.join(".local/share/jTerm") }
    });
    std::fs::create_dir_all(&base)?;
    set_mode(&base, 0o700);
    Ok(base)
}

fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
}

/// Escribe de forma atómica (archivo temporal + rename) con permisos 0600.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    set_mode(&tmp, 0o600);
    std::fs::rename(&tmp, path)
}

/// Lista de elementos guardada como JSON (mismo formato que la versión Swift).
pub struct JsonList<T> {
    path: PathBuf,
    pub items: Vec<T>,
}

impl<T: Serialize + DeserializeOwned> JsonList<T> {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let items = std::fs::read(&path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
        JsonList { path, items }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let data = serde_json::to_vec_pretty(&self.items).map_err(std::io::Error::other)?;
        write_atomic(&self.path, &data)
    }
}

pub type ServerStore = JsonList<Server>;

/// Almacén de secretos (contraseñas, passphrases, claves AWS) por identificador.
pub trait SecretStore: Send + Sync {
    fn get(&self, id: Uuid) -> Option<String>;
    fn set(&self, id: Uuid, secret: &str);
    fn delete(&self, id: Uuid) {
        self.set(id, "");
    }
}

/// Para pruebas.
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<Uuid, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, id: Uuid) -> Option<String> {
        self.0.lock().unwrap().get(&id).cloned()
    }
    fn set(&self, id: Uuid, secret: &str) {
        let mut m = self.0.lock().unwrap();
        if secret.is_empty() { m.remove(&id); } else { m.insert(id, secret.into()); }
    }
}

#[cfg(target_os = "macos")]
pub use keychain::KeychainVault;

#[cfg(target_os = "macos")]
mod keychain {
    use super::SecretStore;
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    const SERVICE: &str = "cl.enovus.terminal.servers";
    const VAULT: &str = "vault";

    /// Todos los secretos viven en UN solo elemento del Keychain (JSON `{id: secreto}`), igual que en Swift:
    /// así macOS pide la clave una sola vez tras cada actualización.
    #[derive(Default)]
    pub struct KeychainVault {
        cache: Mutex<Option<HashMap<String, String>>>,
    }

    impl KeychainVault {
        pub fn new() -> Self {
            Self::default()
        }

        fn load(&self) -> Option<HashMap<String, String>> {
            let mut c = self.cache.lock().unwrap();
            if c.is_none() {
                *c = Some(match get_generic_password(SERVICE, VAULT) {
                    Ok(d) => serde_json::from_slice(&d).unwrap_or_default(),
                    Err(e) if e.code() == -25300 => HashMap::new(), // errSecItemNotFound
                    Err(_) => return None,
                });
            }
            c.clone()
        }

        fn persist(map: &HashMap<String, String>) -> bool {
            let Ok(data) = serde_json::to_vec(map) else { return false };
            let _ = delete_generic_password(SERVICE, VAULT);
            set_generic_password(SERVICE, VAULT, &data).is_ok()
        }
    }

    impl SecretStore for KeychainVault {
        fn get(&self, id: Uuid) -> Option<String> {
            self.load()?.get(&id.to_string().to_uppercase()).cloned()
        }

        fn set(&self, id: Uuid, secret: &str) {
            let Some(mut map) = self.load() else { return };
            let key = id.to_string().to_uppercase();
            if secret.is_empty() { map.remove(&key); } else { map.insert(key, secret.into()); }
            if Self::persist(&map) {
                *self.cache.lock().unwrap() = Some(map);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Snippet;

    #[test]
    fn json_list_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("commands.json");
        let mut l: JsonList<Snippet> = JsonList::open(&p);
        assert!(l.items.is_empty());
        l.items.push(Snippet::new("a", "ls"));
        l.save().unwrap();
        let l2: JsonList<Snippet> = JsonList::open(&p);
        assert_eq!(l2.items, l.items);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn corrupt_file_gives_empty() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.json");
        std::fs::write(&p, "{no").unwrap();
        let l: JsonList<Snippet> = JsonList::open(&p);
        assert!(l.items.is_empty());
    }

    #[test]
    fn memory_secrets() {
        let s = MemorySecrets::default();
        let id = Uuid::new_v4();
        s.set(id, "pw");
        assert_eq!(s.get(id).as_deref(), Some("pw"));
        s.delete(id);
        assert_eq!(s.get(id), None);
    }
}
