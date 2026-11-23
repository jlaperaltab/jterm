use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuthMethod {
    #[default]
    #[serde(rename = "Contraseña")]
    Password,
    #[serde(rename = "Llave (key)")]
    Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerEnv {
    Prod,
    Staging,
    Dev,
}

impl ServerEnv {
    pub const ALL: [ServerEnv; 3] = [ServerEnv::Prod, ServerEnv::Staging, ServerEnv::Dev];

    pub fn tag(self) -> &'static str {
        match self {
            ServerEnv::Prod => "PROD",
            ServerEnv::Staging => "STAGING",
            ServerEnv::Dev => "DEV",
        }
    }

    /// Color RGB (0-255).
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            ServerEnv::Prod => (255, 69, 59),
            ServerEnv::Staging => (255, 204, 0),
            ServerEnv::Dev => (48, 209, 89),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub auth: AuthMethod,
    #[serde(default)]
    pub key_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<ServerEnv>,
    /// Servidor guardado por el que se pasa para llegar a este (ProxyJump).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<Uuid>,
    /// Directorio donde abre la sesión (vacío = el que decida el servidor, normalmente la carpeta personal).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start_dir: String,
}

fn default_port() -> u16 {
    22
}

impl Default for Server {
    fn default() -> Self {
        Server {
            id: Uuid::new_v4(),
            name: String::new(),
            host: String::new(),
            port: 22,
            user: String::new(),
            auth: AuthMethod::Password,
            key_path: String::new(),
            env: None,
            jump: None,
            start_dir: String::new(),
        }
    }
}

impl Server {
    /// `usuario@host` o solo `host`.
    pub fn destination(&self) -> String {
        if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }
}

/// Un comando guardado por el usuario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub command: String,
    /// Si es true, al insertarlo se pulsa Enter automáticamente.
    #[serde(default)]
    pub auto_run: bool,
}

impl Snippet {
    pub fn new(name: &str, command: &str) -> Self {
        Snippet { id: Uuid::new_v4(), name: name.into(), command: command.into(), auto_run: false }
    }

    pub fn first_line(&self) -> String {
        self.command.lines().find(|l| !l.is_empty()).map(|l| l.trim().to_string()).unwrap_or_default()
    }

    /// El nombre si existe; si no, la primera línea del comando.
    pub fn title(&self) -> String {
        let n = self.name.trim();
        if n.is_empty() { self.first_line() } else { n.to_string() }
    }

    pub fn line_count(&self) -> usize {
        self.command.lines().filter(|l| !l.is_empty()).count()
    }
}

/// Minúsculas y sin acentos (para buscar sin distinguir mayúsculas ni diacríticos).
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// Comandos cuyo nombre o texto contiene `q` (vacío = todos).
pub fn search_snippets<'a>(items: &'a [Snippet], q: &str) -> Vec<&'a Snippet> {
    let q = fold(q.trim());
    items.iter().filter(|s| q.is_empty() || fold(&s.name).contains(&q) || fold(&s.command).contains(&q)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TunnelKind {
    #[default]
    Local,
    Remote,
}

/// Túnel SSH guardado: reenvío de un puerto (-L) o inverso (-R) por un servidor guardado.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tunnel {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: TunnelKind,
    #[serde(default)]
    pub listen_port: u32,
    #[serde(default = "default_dest_host")]
    pub dest_host: String,
    #[serde(default)]
    pub dest_port: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<Uuid>,
}

fn default_dest_host() -> String {
    "localhost".into()
}

impl Tunnel {
    fn host(&self) -> String {
        if self.dest_host.contains(':') && !self.dest_host.starts_with('[') {
            format!("[{}]", self.dest_host)
        } else {
            self.dest_host.clone()
        }
    }

    /// Argumento de -L o -R. Local escucha solo en 127.0.0.1.
    pub fn forward_spec(&self) -> String {
        match self.kind {
            TunnelKind::Local => format!("127.0.0.1:{}:{}:{}", self.listen_port, self.host(), self.dest_port),
            TunnelKind::Remote => format!("{}:{}:{}", self.listen_port, self.host(), self.dest_port),
        }
    }

    pub fn flag(&self) -> &'static str {
        match self.kind {
            TunnelKind::Local => "-L",
            TunnelKind::Remote => "-R",
        }
    }

    /// Resumen corto: `localhost:5433 → db:5432` (local) o `servidor:5433 → localhost:3000` (remoto).
    pub fn summary(&self) -> String {
        match self.kind {
            TunnelKind::Local => format!("localhost:{} → {}:{}", self.listen_port, self.dest_host, self.dest_port),
            TunnelKind::Remote => format!("servidor:{} → {}:{}", self.listen_port, self.dest_host, self.dest_port),
        }
    }

    /// El nombre, o el resumen si no tiene.
    pub fn title(&self) -> String {
        if self.name.trim().is_empty() { self.summary() } else { self.name.clone() }
    }

    /// Texto del problema si los datos no sirven, o `None` si están bien.
    pub fn validation_error(&self, is_root: bool) -> Option<&'static str> {
        if !(1..=65535).contains(&self.listen_port) {
            return Some("El puerto debe estar entre 1 y 65535.");
        }
        if !(1..=65535).contains(&self.dest_port) {
            return Some("El puerto de destino debe estar entre 1 y 65535.");
        }
        if self.dest_host.trim().is_empty() || self.dest_host.contains(' ') {
            return Some("Indica el host de destino (sin espacios).");
        }
        if self.kind == TunnelKind::Local && self.listen_port < 1024 && !is_root {
            return Some("Los puertos menores a 1024 solo los puede abrir el administrador. Usa uno mayor.");
        }
        if self.server_id.is_none() {
            return Some("Elige el servidor por el que pasa el túnel.");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_reads_swift_json() {
        let j = r#"{"id":"3F2504E0-4F89-11D3-9A0C-0305E82C3301","name":"web","host":"h","port":2222,
            "user":"root","auth":"Llave (key)","keyPath":"~/.ssh/id","env":"prod"}"#;
        let s: Server = serde_json::from_str(j).unwrap();
        assert_eq!(s.auth, AuthMethod::Key);
        assert_eq!(s.env, Some(ServerEnv::Prod));
        assert_eq!(s.port, 2222);
        assert_eq!(s.destination(), "root@h");
        assert!(s.jump.is_none());
    }

    #[test]
    fn start_dir_is_optional_and_roundtrips() {
        let s: Server = serde_json::from_str(r#"{"id":"3F2504E0-4F89-11D3-9A0C-0305E82C3301","host":"h"}"#).unwrap();
        assert!(s.start_dir.is_empty());
        assert!(!serde_json::to_string(&s).unwrap().contains("startDir"), "vacío no se escribe (Swift no lo conoce)");
        let mut s2 = s.clone();
        s2.start_dir = "/var/www".into();
        let back: Server = serde_json::from_str(&serde_json::to_string(&s2).unwrap()).unwrap();
        assert_eq!(back.start_dir, "/var/www");
    }

    #[test]
    fn tunnel_specs() {
        let mut t = Tunnel {
            id: Uuid::new_v4(), name: String::new(), kind: TunnelKind::Local, listen_port: 5432,
            dest_host: "::1".into(), dest_port: 5432, server_id: Some(Uuid::new_v4()),
        };
        assert_eq!(t.forward_spec(), "127.0.0.1:5432:[::1]:5432");
        t.kind = TunnelKind::Remote;
        assert_eq!(t.forward_spec(), "5432:[::1]:5432");
        assert_eq!(t.flag(), "-R");
        assert_eq!(t.summary(), "servidor:5432 → ::1:5432");
        assert_eq!(t.title(), t.summary());
        t.name = "db".into();
        assert_eq!(t.title(), "db");
        t.listen_port = 0;
        assert!(t.validation_error(false).is_some());
    }

    #[test]
    fn snippet_search_ignores_case_and_accents() {
        let v = vec![Snippet::new("Reiniciar Ñandú", "systemctl restart x"), Snippet::new("logs", "tail -f /var/log/syslog")];
        assert_eq!(search_snippets(&v, "nandu").len(), 1);
        assert_eq!(search_snippets(&v, "SYSLOG")[0].name, "logs");
        assert_eq!(search_snippets(&v, "  ").len(), 2);
        assert!(search_snippets(&v, "zzz").is_empty());
    }

    #[test]
    fn snippet_title() {
        let s = Snippet::new("", "\n  ls -la\nwhoami");
        assert_eq!(s.title(), "ls -la");
        assert_eq!(s.line_count(), 2);
    }
}
