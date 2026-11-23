use regex::{Regex, RegexBuilder};
use std::sync::OnceLock;

const PATTERNS: &[(&str, &str)] = &[
    (r"(?:^|[\s;&|(`])(?:sudo\s+)?rm\s+\S", "rm (borra archivos)"),
    (r"\bdrop\s+(?:table|database|schema|index|view|user|role|column|materialized)\b", "DROP (borra objetos de la base de datos)"),
    (r"\btruncate\s+table\b", "TRUNCATE TABLE (vacía una tabla)"),
    // Sin lookahead (regex no lo soporta): se comprueba aparte.
    (r"\bdelete\s+from\s+\w+", "DELETE sin WHERE (borra todas las filas)"),
    (r"\bmkfs(?:\.\w+)?\b", "mkfs (formatea un disco)"),
    (r"\bdd\s+[^;|]*\bof=", "dd (escribe sobre un disco o archivo)"),
    (r"(?:^|[\s;&|(`])(?:sudo\s+)?(?:shutdown|reboot|halt|poweroff)\b", "apagado o reinicio del servidor"),
    (r"\bkubectl\s+delete\b", "kubectl delete"),
    (r"\bdocker\s+(?:rm|rmi|system\s+prune|volume\s+(?:rm|prune)|container\s+prune)\b", "docker (borra contenedores o imágenes)"),
];

fn compiled() -> &'static Vec<(Regex, &'static str)> {
    static C: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    C.get_or_init(|| {
        PATTERNS
            .iter()
            .map(|(p, l)| (RegexBuilder::new(p).case_insensitive(true).build().unwrap(), *l))
            .collect()
    })
}

fn git_rm() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| RegexBuilder::new(r"\bgit\s+rm\b").case_insensitive(true).build().unwrap())
}

fn where_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| RegexBuilder::new(r"\bwhere\b").case_insensitive(true).build().unwrap())
}

/// Descripción del primer patrón delicado que aparece en el texto, o `None` si no hay ninguno.
pub fn risk(raw: &str) -> Option<&'static str> {
    // "git rm" solo quita archivos del repositorio: no es el rm del sistema.
    let text = git_rm().replace_all(raw, "git-rm");
    for (re, label) in compiled() {
        let Some(m) = re.find(&text) else { continue };
        if label.starts_with("DELETE") {
            // DELETE ... sin WHERE antes del siguiente ';'.
            let rest = &text[m.end()..];
            let stmt = rest.split(';').next().unwrap_or("");
            if where_re().is_match(stmt) {
                continue;
            }
        }
        return Some(label);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects() {
        assert!(risk("sudo rm -rf /tmp/x").is_some());
        assert!(risk("psql -c 'DROP TABLE users'").is_some());
        assert!(risk("delete from users").is_some());
        assert!(risk("kubectl delete pod x").is_some());
        assert!(risk("reboot").is_some());
    }

    #[test]
    fn ignores_safe() {
        assert!(risk("git rm file.txt").is_none());
        assert!(risk("delete from users where id = 1").is_none());
        assert!(risk("ls -la").is_none());
        assert!(risk("echo confirm").is_none());
    }
}
