//! Saltos SSH (ProxyJump): un servidor guardado llega a su destino a través de otro.
//!
//! Se usa `-o ProxyCommand="ssh -W %h:%p …"` en vez de `-J` porque ssh no pasa al salto las opciones de la
//! línea de comandos (`-i`, contraseña por askpass…): así cada salto usa su propia llave y contraseña.

use crate::models::{AuthMethod, Server};
use crate::settings::Settings;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const MAX_HOPS: usize = 4;

/// Script askpass: ssh lo invoca para obtener la contraseña desde variables de entorno, así el secreto
/// nunca aparece en la línea de comandos ni en disco. Idéntico al de la versión Swift.
pub const ASKPASS_BODY: &str = r#"#!/bin/sh
case "$1" in
  *passphrase*)
    if [ -n "$TERMINAL_KEY" ]; then
      case "$1" in *"$TERMINAL_KEY"*) printf '%s\n' "$TERMINAL_SECRET"; exit 0;; esac
    fi
    for i in 1 2 3 4; do
      eval "k=\${TERMINAL_JUMP_${i}_KEY-}"
      if [ -n "$k" ]; then
        case "$1" in *"$k"*) eval "printf '%s\n' \"\${TERMINAL_JUMP_${i}_SECRET-}\""; exit 0;; esac
      fi
    done
    exit 1 ;;
  *assword*)
    for i in 1 2 3 4; do
      eval "h=\${TERMINAL_JUMP_${i}_HOST-}"
      if [ -n "$h" ]; then
        case "$1" in *"$h's password"*) eval "printf '%s\n' \"\${TERMINAL_JUMP_${i}_SECRET-}\""; exit 0;; esac
      fi
    done
    printf '%s\n' "$TERMINAL_SECRET" ;;
  *) exit 1 ;;
esac
"#;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct JumpSetup {
    pub options: Vec<String>,
    pub env: HashMap<String, String>,
}

impl JumpSetup {
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }
}

/// Entrecomilla para el shell: el ProxyCommand lo ejecuta `sh -c`.
pub fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    } else if p == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return home;
        }
    }
    p.to_string()
}

fn key_path(s: &Server) -> Option<String> {
    (s.auth == AuthMethod::Key && !s.key_path.is_empty()).then(|| expand_tilde(&s.key_path))
}

/// Los saltos de un servidor, del más cercano al destino al más lejano. Corta si hay un ciclo (A → B → A).
pub fn hops<'a>(s: &'a Server, servers: &'a [Server]) -> Vec<&'a Server> {
    let mut out = Vec::new();
    let mut seen: HashSet<Uuid> = HashSet::from([s.id]);
    let mut cur = s;
    while out.len() < MAX_HOPS {
        let Some(id) = cur.jump else { break };
        if seen.contains(&id) {
            break;
        }
        let Some(j) = servers.iter().find(|x| x.id == id) else { break };
        out.push(j);
        seen.insert(id);
        cur = j;
    }
    out
}

/// El comando que usa ssh para llegar al destino pasando por `hops[0]` (que a su vez pasa por `hops[1]`…).
pub fn proxy_command(hops: &[&Server], ssh_path: &str, settings: &Settings) -> Option<String> {
    let first = hops.first()?;
    let mut args: Vec<String> = vec![ssh_path.into(), "-W".into(), "%h:%p".into(), "-p".into(), first.port.to_string()];
    args.extend(settings.ssh_args());
    if let Some(k) = key_path(first) {
        args.extend(["-i".into(), k, "-o".into(), "IdentitiesOnly=yes".into()]);
    }
    if let Some(nested) = proxy_command(&hops[1..], ssh_path, settings) {
        args.extend(["-o".into(), format!("ProxyCommand={nested}")]);
    }
    args.push(first.destination());
    Some(args.iter().map(|a| shq(a)).collect::<Vec<_>>().join(" "))
}

/// Opciones y entorno (con las contraseñas de los saltos para askpass) para conectar a `s`.
/// `secret_of` consulta el almacén de secretos (Keychain): llamarlo fuera del hilo de la interfaz.
pub fn setup(
    s: &Server,
    servers: &[Server],
    ssh_path: &str,
    settings: &Settings,
    askpass_path: &str,
    secret_of: impl Fn(Uuid) -> Option<String>,
) -> JumpSetup {
    let hs = hops(s, servers);
    let Some(cmd) = proxy_command(&hs, ssh_path, settings) else { return JumpSetup::default() };
    let mut env = HashMap::new();
    let mut any_secret = false;
    for (i, h) in hs.iter().enumerate() {
        let n = i + 1;
        env.insert(format!("TERMINAL_JUMP_{n}_HOST"), h.destination());
        if let Some(k) = key_path(h) {
            env.insert(format!("TERMINAL_JUMP_{n}_KEY"), k);
        }
        if let Some(secret) = secret_of(h.id).filter(|x| !x.is_empty()) {
            env.insert(format!("TERMINAL_JUMP_{n}_SECRET"), secret);
            any_secret = true;
        }
    }
    if any_secret {
        env.insert("SSH_ASKPASS".into(), askpass_path.into());
        env.insert("SSH_ASKPASS_REQUIRE".into(), "force".into());
    }
    JumpSetup { options: vec!["-o".into(), format!("ProxyCommand={cmd}")], env }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srv(name: &str, jump: Option<Uuid>) -> Server {
        Server { name: name.into(), host: format!("{name}.x"), user: "u".into(), jump, ..Server::default() }
    }

    #[test]
    fn quoting() {
        assert_eq!(shq("a'b"), r"'a'\''b'");
    }

    #[test]
    fn chain_and_cycle() {
        let c = srv("c", None);
        let b = srv("b", Some(c.id));
        let a = srv("a", Some(b.id));
        let all = vec![a.clone(), b.clone(), c.clone()];
        let h = hops(&a, &all);
        assert_eq!(h.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["b", "c"]);

        let mut c2 = c.clone();
        c2.jump = Some(a.id); // ciclo a → b → c → a
        let all = vec![a.clone(), b.clone(), c2];
        assert_eq!(hops(&a, &all).len(), 2);
    }

    #[test]
    fn proxy_command_nests() {
        let c = srv("c", None);
        let b = srv("b", Some(c.id));
        let mut s = Settings::default();
        s.ssh_keep_alive = 0;
        let cmd = proxy_command(&[&b, &c], "/usr/bin/ssh", &s).unwrap();
        assert!(cmd.starts_with("'/usr/bin/ssh' '-W' '%h:%p' '-p' '22'"));
        assert!(cmd.contains("'ProxyCommand='\\''/usr/bin/ssh'"));
        assert!(cmd.ends_with("'u@b.x'"));
    }

    #[test]
    fn setup_sets_askpass_only_with_secrets() {
        let b = srv("b", None);
        let a = srv("a", Some(b.id));
        let all = vec![a.clone(), b.clone()];
        let s = Settings::default();
        let none = setup(&a, &all, "ssh", &s, "/x/askpass.sh", |_| None);
        assert!(!none.env.contains_key("SSH_ASKPASS"));
        assert_eq!(none.env["TERMINAL_JUMP_1_HOST"], "u@b.x");
        let some = setup(&a, &all, "ssh", &s, "/x/askpass.sh", |_| Some("pw".into()));
        assert_eq!(some.env["TERMINAL_JUMP_1_SECRET"], "pw");
        assert_eq!(some.env["SSH_ASKPASS"], "/x/askpass.sh");
        assert!(setup(&b, &all, "ssh", &s, "x", |_| None).is_empty());
    }
}
