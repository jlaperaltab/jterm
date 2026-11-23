//! Detección de URLs y rutas de archivo en el texto del terminal.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Url,
    Path,
}

/// Enlace hallado: rango en caracteres (fin exclusivo) y su texto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub kind: LinkKind,
    pub text: String,
}

fn url_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?i)\b(?:https?|ftp)://[^\s<>"'`]+"#).unwrap())
}

fn path_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?:^|[\s"'(=:])((?:~|\.{1,2})?/[\w@%+=:,.~\-]+(?:/[\w@%+=:,.~\-]*)*)"#).unwrap())
}

/// Quita la puntuación final que casi nunca forma parte del enlace.
fn trim_end(s: &str, is_url: bool) -> &str {
    let mut s = s;
    loop {
        let Some(last) = s.chars().last() else { return s };
        let strip = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' => true,
            // Un paréntesis o corchete final solo sobra si no hay uno que lo abra dentro del enlace.
            ')' => is_url && !s.contains('(') || !is_url,
            ']' => !s.contains('['),
            '}' => !s.contains('{'),
            _ => false,
        };
        if !strip {
            return s;
        }
        s = &s[..s.len() - last.len_utf8()];
    }
}

/// Todos los enlaces de una línea. Las rutas solo se buscan con `paths` (y no dentro de una URL).
pub fn find(text: &str, paths: bool) -> Vec<Found> {
    // índice de byte → índice de carácter
    let char_at: Vec<usize> = {
        let mut v = vec![0; text.len() + 1];
        for (ci, (bi, c)) in text.char_indices().enumerate() {
            for k in 0..c.len_utf8() {
                v[bi + k] = ci;
            }
            v[bi + c.len_utf8()] = ci + 1;
        }
        v
    };
    let mut out: Vec<Found> = Vec::new();
    for m in url_re().find_iter(text) {
        let t = trim_end(m.as_str(), true);
        if t.len() > "http://".len() {
            out.push(Found { start: char_at[m.start()], end: char_at[m.start() + t.len()], kind: LinkKind::Url, text: t.to_string() });
        }
    }
    if paths {
        for c in path_re().captures_iter(text) {
            let g = c.get(1).unwrap();
            let t = trim_end(g.as_str(), false);
            let (start, end) = (char_at[g.start()], char_at[g.start() + t.len()]);
            let overlaps = out.iter().any(|u| u.kind == LinkKind::Url && start < u.end && u.start < end);
            if t.chars().count() >= 3 && !overlaps {
                out.push(Found { start, end, kind: LinkKind::Path, text: t.to_string() });
            }
        }
    }
    out.sort_by_key(|f| f.start);
    out
}

/// Ruta absoluta de una ruta con `~` (las relativas no se resuelven: no se conoce la carpeta del shell).
pub fn resolve_path(p: &str) -> Option<std::path::PathBuf> {
    if p.starts_with("./") || p.starts_with("../") {
        return None;
    }
    let full = jterm_core::ssh_jump::expand_tilde(p);
    full.starts_with('/').then(|| full.into())
}

/// ¿Se puede abrir sin riesgo? Los ejecutables y las apps se muestran en el Finder en vez de lanzarse.
pub fn is_safe_to_open(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => p.extension().is_none_or(|e| e != "app"),
        Ok(m) => m.permissions().mode() & 0o111 == 0,
        Err(_) => false,
    }
}

/// Abre una URL (solo http, https y ftp) o una ruta existente. Devuelve el motivo si no se pudo.
pub fn open(kind: LinkKind, text: &str) -> Result<(), String> {
    use std::process::Command;
    let status = match kind {
        LinkKind::Url => {
            let lower = text.to_lowercase();
            if !(lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("ftp://")) {
                return Err("Solo se abren enlaces http, https y ftp.".into());
            }
            Command::new("open").arg(text).status()
        }
        LinkKind::Path => {
            let p = resolve_path(text).filter(|p| p.exists()).ok_or("La ruta no existe.")?;
            if is_safe_to_open(&p) {
                Command::new("open").arg(&p).status()
            } else {
                Command::new("open").arg("-R").arg(&p).status()
            }
        }
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err("No se pudo abrir el enlace.".into()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(s: &str, paths: bool) -> Vec<(String, LinkKind)> {
        find(s, paths).into_iter().map(|f| (f.text, f.kind)).collect()
    }

    #[test]
    fn urls_trim_punctuation() {
        assert_eq!(texts("ver https://ejemplo.com/a?b=1&c=2.", false), [("https://ejemplo.com/a?b=1&c=2".into(), LinkKind::Url)]);
        assert_eq!(texts("(https://ejemplo.com/x)", false)[0].0, "https://ejemplo.com/x");
        assert_eq!(texts("https://es.wikipedia.org/wiki/Rust_(lenguaje)", false)[0].0, "https://es.wikipedia.org/wiki/Rust_(lenguaje)");
        assert_eq!(texts("HTTP://A.COM/x, y", false)[0].0, "HTTP://A.COM/x");
        assert!(texts("http:// nada", false).is_empty());
        assert!(texts("javascript://x\nfile:///etc/passwd", false).is_empty());
    }

    #[test]
    fn paths_only_when_asked_and_not_inside_urls() {
        assert!(texts("cd /var/log/nginx", false).is_empty());
        assert_eq!(texts("cd /var/log/nginx", true), [("/var/log/nginx".into(), LinkKind::Path)]);
        assert_eq!(texts("vim ~/.zshrc: error", true)[0].0, "~/.zshrc");
        assert_eq!(texts("ls ./src/main.rs", true)[0].0, "./src/main.rs");
        assert_eq!(texts("log=/tmp/x.log.", true)[0].0, "/tmp/x.log");
        let v = texts("https://a.com/ruta/larga y /etc/hosts", true);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1], ("/etc/hosts".into(), LinkKind::Path));
        // Fracciones y operadores no son rutas.
        assert!(texts("1/2 y a / b", true).is_empty());
    }

    #[test]
    fn ranges_are_in_characters_not_bytes() {
        let f = &find("ñandú → https://x.org/ñ ok", false)[0];
        assert_eq!((f.start, f.end), (8, 23));
        assert_eq!(f.text, "https://x.org/ñ");
    }

    #[test]
    fn open_rejects_dangerous_schemes_and_missing_paths() {
        assert!(open(LinkKind::Url, "file:///Applications/Calculator.app").is_err());
        assert!(open(LinkKind::Url, "javascript:alert(1)").is_err());
        assert!(open(LinkKind::Path, "/no/existe/xyz").is_err());
        assert!(open(LinkKind::Path, "./relativa").is_err());
    }

    #[test]
    fn safe_to_open() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("nota.txt");
        std::fs::write(&f, "x").unwrap();
        assert!(is_safe_to_open(&f));
        assert!(is_safe_to_open(d.path()));
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!is_safe_to_open(&f));
        let app = d.path().join("X.app");
        std::fs::create_dir(&app).unwrap();
        assert!(!is_safe_to_open(&app));
        assert!(!is_safe_to_open(&d.path().join("no-existe")));
    }
}
