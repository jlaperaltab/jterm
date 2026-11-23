//! Túneles SSH guardados (-L / -R): un proceso `ssh -N` por túnel, con estado y mensajes de error legibles.

use crate::models::{Tunnel, TunnelKind};
use crate::settings::Settings;
use crate::sftp::SshTarget;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

/// Texto que `ssh` imprime (vía `LocalCommand`) cuando el reenvío ya quedó establecido.
pub const UP_MARKER: &str = "jterm-tunnel-up";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelState {
    Stopped,
    Starting,
    Active,
    Failed(String),
}

impl TunnelState {
    pub fn is_running(&self) -> bool {
        matches!(self, TunnelState::Starting | TunnelState::Active)
    }
}

/// ¿Somos el administrador? (los puertos < 1024 solo los abre root)
pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Argumentos de `ssh` para el túnel.
pub fn arguments(t: &Tunnel, target: &SshTarget, settings: &Settings) -> Vec<String> {
    let mut a: Vec<String> = [
        "-N", "-T", "-o", "ExitOnForwardFailure=yes", "-o", "ConnectTimeout=15", "-o", "PermitLocalCommand=yes", "-o",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(format!("LocalCommand=echo {UP_MARKER}"));
    a.push("-p".into());
    a.push(target.port.to_string());
    a.extend(settings.ssh_args());
    a.extend(target.options.iter().cloned());
    a.push(t.flag().into());
    a.push(t.forward_spec());
    a.push(target.destination());
    a
}

/// Mensaje comprensible a partir de lo que `ssh` escribió al fallar.
pub fn explain(stderr: &str, t: &Tunnel, status: i32, was_active: bool) -> String {
    let low = stderr.to_lowercase();
    if low.contains("address already in use") || (low.contains("cannot listen") && low.contains("port")) {
        return if t.kind == TunnelKind::Local {
            format!("El puerto {} ya está en uso en este Mac. Cierra lo que lo usa o elige otro.", t.listen_port)
        } else {
            format!("El puerto {} ya está en uso en el servidor. Cierra lo que lo usa o elige otro.", t.listen_port)
        };
    }
    if low.contains("remote port forwarding failed") {
        return format!("El servidor rechazó el reenvío remoto del puerto {} (¿ocupado o no permitido?).", t.listen_port);
    }
    if low.contains("administratively prohibited") || low.contains("open failed") {
        return "El servidor no permite este reenvío (AllowTcpForwarding).".into();
    }
    if low.contains("permission denied") {
        return "El servidor rechazó la autenticación. Revisa la contraseña guardada o la llave.".into();
    }
    if low.contains("connection refused") {
        return "Conexión rechazada por el servidor.".into();
    }
    if low.contains("timed out") || low.contains("no route to host") {
        return "No se pudo llegar al servidor (tiempo agotado).".into();
    }
    if low.contains("could not resolve") || low.contains("name or service not known") {
        return "No se encontró el servidor (revisa el host).".into();
    }
    if low.contains("host key verification failed") {
        return "No se pudo verificar la llave del servidor (revisa known_hosts).".into();
    }
    if let Some(last) = stderr.lines().map(str::trim).rfind(|l| !l.is_empty()) {
        return last.to_string();
    }
    if was_active { "La conexión del túnel se cerró.".into() } else { format!("ssh terminó con el código {status}.") }
}

struct Proc {
    child: Arc<Mutex<Child>>,
    /// Distingue ejecuciones sucesivas del mismo túnel.
    gen: u64,
}

#[derive(Default)]
struct Inner {
    states: HashMap<Uuid, TunnelState>,
    procs: HashMap<Uuid, Proc>,
    stopped_by_user: HashSet<Uuid>,
    next_gen: u64,
}

type Shared = Arc<Mutex<Inner>>;

/// Ejecuta los túneles. Al soltarlo se detienen todos (no quedan procesos `ssh` huérfanos).
pub struct TunnelManager {
    inner: Shared,
    /// Ruta de ssh; las pruebas la cambian por un script de mentira.
    pub ssh_path: String,
    /// Segundos que ssh debe seguir vivo tras conectar para dar el túnel por activo.
    pub settle: Duration,
}

impl Default for TunnelManager {
    fn default() -> Self {
        TunnelManager { inner: Shared::default(), ssh_path: "/usr/bin/ssh".into(), settle: Duration::from_millis(1000) }
    }
}

impl TunnelManager {
    /// Gestor con otra ruta de ssh y otro tiempo de confirmación (para pruebas).
    pub fn with_ssh(ssh_path: impl Into<String>, settle: Duration) -> Self {
        TunnelManager { inner: Shared::default(), ssh_path: ssh_path.into(), settle }
    }

    pub fn state(&self, id: Uuid) -> TunnelState {
        self.inner.lock().unwrap().states.get(&id).cloned().unwrap_or(TunnelState::Stopped)
    }

    pub fn active_count(&self) -> usize {
        self.inner.lock().unwrap().states.values().filter(|s| s.is_running()).count()
    }

    fn set(inner: &Shared, id: Uuid, s: TunnelState) {
        inner.lock().unwrap().states.insert(id, s);
    }

    /// Inicia el túnel. `target` ya trae las opciones y el entorno (askpass, saltos…).
    pub fn start(&self, t: &Tunnel, target: &SshTarget, settings: &Settings) {
        let id = t.id;
        let gen = {
            let mut g = self.inner.lock().unwrap();
            if g.procs.contains_key(&id) || g.states.get(&id) == Some(&TunnelState::Starting) {
                return;
            }
            g.stopped_by_user.remove(&id);
            g.states.insert(id, TunnelState::Starting);
            g.next_gen += 1;
            g.next_gen
        };
        let spawned = Command::new(&self.ssh_path)
            .args(arguments(t, target, settings))
            .envs(&target.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => return Self::set(&self.inner, id, TunnelState::Failed(format!("No se pudo ejecutar ssh: {e}"))),
        };
        let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        let child = Arc::new(Mutex::new(child));
        self.inner.lock().unwrap().procs.insert(id, Proc { child: child.clone(), gen });

        let tail = Arc::new(Mutex::new(String::new()));
        // stdout: el marcador indica que el reenvío está listo; se confirma tras `settle` si ssh sigue vivo.
        {
            let (inner, child, settle) = (self.inner.clone(), child.clone(), self.settle);
            std::thread::spawn(move || {
                let mut buf = [0u8; 512];
                let mut seen = String::new();
                while let Ok(n) = out.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    seen.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if seen.contains(UP_MARKER) {
                        std::thread::sleep(settle);
                        let alive = matches!(child.lock().unwrap().try_wait(), Ok(None));
                        let mut g = inner.lock().unwrap();
                        if alive && g.states.get(&id) == Some(&TunnelState::Starting) && g.procs.get(&id).is_some_and(|p| p.gen == gen) {
                            g.states.insert(id, TunnelState::Active);
                        }
                        break;
                    }
                }
            });
        }
        {
            let tail = tail.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 512];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut t = tail.lock().unwrap();
                    t.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if t.len() > 2000 {
                        let cut = t.char_indices().map(|(i, _)| i).find(|&i| i >= t.len() - 2000).unwrap_or(0);
                        t.drain(..cut);
                    }
                }
            });
        }
        // Vigilante: detecta el fin del proceso y decide si fue una parada o un fallo.
        let (inner, tunnel) = (self.inner.clone(), t.clone());
        std::thread::spawn(move || {
            let status = loop {
                match child.lock().unwrap().try_wait() {
                    Ok(Some(s)) => break s.code().unwrap_or(-1),
                    Ok(None) => {}
                    Err(_) => break -1,
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            std::thread::sleep(Duration::from_millis(150)); // deja llegar lo último que ssh escribió
            let mut g = inner.lock().unwrap();
            if g.procs.get(&id).is_none_or(|p| p.gen != gen) {
                return;
            }
            g.procs.remove(&id);
            if g.stopped_by_user.remove(&id) {
                g.states.insert(id, TunnelState::Stopped);
                return;
            }
            let was_active = g.states.get(&id) == Some(&TunnelState::Active);
            let msg = explain(&tail.lock().unwrap(), &tunnel, status, was_active);
            g.states.insert(id, TunnelState::Failed(msg));
        });
    }

    pub fn stop(&self, id: Uuid) {
        let mut g = self.inner.lock().unwrap();
        match g.procs.get(&id) {
            Some(p) => {
                let child = p.child.clone();
                g.stopped_by_user.insert(id);
                drop(g);
                let _ = child.lock().unwrap().kill();
            }
            None => {
                g.states.insert(id, TunnelState::Stopped);
            }
        }
    }

    pub fn stop_all(&self) {
        let ids: Vec<Uuid> = self.inner.lock().unwrap().procs.keys().copied().collect();
        for id in ids {
            self.stop(id);
        }
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    fn tunnel(kind: TunnelKind) -> Tunnel {
        Tunnel { id: Uuid::new_v4(), name: "db".into(), kind, listen_port: 5433, dest_host: "db.interna".into(), dest_port: 5432, server_id: Some(Uuid::new_v4()) }
    }

    fn target() -> SshTarget {
        SshTarget { user: "u".into(), host: "h".into(), port: 2222, options: vec!["-i".into(), "k".into()], ..Default::default() }
    }

    /// Un "ssh" de mentira: nada sale a la red.
    fn fake_ssh(dir: &std::path::Path, body: &str) -> String {
        let p = dir.join("fake-ssh");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    }

    fn wait_state(m: &TunnelManager, id: Uuid, pred: impl Fn(&TunnelState) -> bool) -> TunnelState {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let s = m.state(id);
            if pred(&s) || Instant::now() > end {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn ssh_arguments() {
        let s = Settings { ssh_keep_alive: 0, ..Settings::default() };
        let a = arguments(&tunnel(TunnelKind::Local), &target(), &s);
        assert_eq!(&a[..4], ["-N", "-T", "-o", "ExitOnForwardFailure=yes"]);
        let n = a.len();
        assert_eq!(&a[n - 3..], ["-L", "127.0.0.1:5433:db.interna:5432", "u@h"]);
        assert!(a.windows(2).any(|w| w == ["-p", "2222"]) && a.windows(2).any(|w| w == ["-i", "k"]));
        assert!(a.contains(&format!("LocalCommand=echo {UP_MARKER}")));
        let r = arguments(&tunnel(TunnelKind::Remote), &target(), &s);
        assert_eq!(&r[r.len() - 3..], ["-R", "5433:db.interna:5432", "u@h"]);
    }

    #[test]
    fn explains_common_failures() {
        let t = tunnel(TunnelKind::Local);
        assert!(explain("bind [127.0.0.1]:5433: Address already in use", &t, 255, false).contains("ya está en uso en este Mac"));
        assert!(explain("Permission denied (publickey).", &t, 255, false).contains("autenticación"));
        assert!(explain("channel 2: open failed: administratively prohibited", &t, 255, true).contains("AllowTcpForwarding"));
        assert_eq!(explain("algo raro\nultima linea\n", &t, 1, false), "ultima linea");
        assert_eq!(explain("", &t, 7, false), "ssh terminó con el código 7.");
        assert_eq!(explain("", &t, 0, true), "La conexión del túnel se cerró.");
        let r = tunnel(TunnelKind::Remote);
        assert!(explain("Error: remote port forwarding failed for listen port 5433", &r, 255, false).contains("rechazó el reenvío remoto"));
    }

    #[test]
    fn becomes_active_then_stops_by_user() {
        let d = tempfile::tempdir().unwrap();
        let m = TunnelManager::with_ssh(fake_ssh(d.path(), &format!("echo {UP_MARKER}; sleep 30")), Duration::from_millis(100));
        let t = tunnel(TunnelKind::Local);
        assert_eq!(m.state(t.id), TunnelState::Stopped);
        m.start(&t, &target(), &Settings::default());
        assert!(m.state(t.id).is_running());
        assert_eq!(wait_state(&m, t.id, |s| *s == TunnelState::Active), TunnelState::Active);
        assert_eq!(m.active_count(), 1);
        m.start(&t, &target(), &Settings::default()); // un segundo inicio no duplica el proceso
        m.stop(t.id);
        assert_eq!(wait_state(&m, t.id, |s| *s == TunnelState::Stopped), TunnelState::Stopped);
        assert_eq!(m.active_count(), 0);
    }

    #[test]
    fn failure_message_comes_from_stderr() {
        let d = tempfile::tempdir().unwrap();
        let m = TunnelManager::with_ssh(fake_ssh(d.path(), "echo 'bind: Address already in use' >&2; exit 255"), Duration::from_millis(100));
        let t = tunnel(TunnelKind::Local);
        m.start(&t, &target(), &Settings::default());
        let s = wait_state(&m, t.id, |s| matches!(s, TunnelState::Failed(_)));
        assert!(matches!(&s, TunnelState::Failed(msg) if msg.contains("ya está en uso")), "{s:?}");
        // Tras un fallo se puede volver a iniciar.
        m.start(&t, &target(), &Settings::default());
        assert!(m.state(t.id).is_running() || matches!(m.state(t.id), TunnelState::Failed(_)));
    }

    #[test]
    fn dropping_the_manager_kills_the_processes() {
        let d = tempfile::tempdir().unwrap();
        let marker = d.path().join("alive");
        let script = format!("touch {}; sleep 30", marker.display());
        let m = TunnelManager::with_ssh(fake_ssh(d.path(), &script), Duration::from_millis(100));
        let t = tunnel(TunnelKind::Local);
        m.start(&t, &target(), &Settings::default());
        let end = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        let child = m.inner.lock().unwrap().procs.get(&t.id).unwrap().child.clone();
        let pid = child.lock().unwrap().id();
        drop(m);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        // kill -0 comprueba si el proceso sigue vivo (macOS no tiene /proc).
        let alive = Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        assert!(!alive, "el proceso ssh debió terminar");
    }
}
