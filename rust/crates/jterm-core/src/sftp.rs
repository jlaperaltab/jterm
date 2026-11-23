//! SFTP y transferencias sobre una conexión SSH maestra (ControlMaster), usando el `ssh`/`sftp`/`scp` del sistema.
//!
//! La contraseña o la llave temporal se usan una sola vez: todas las operaciones reutilizan el socket de control.
//! Las partes puras (comillas, análisis de `ls`, progreso, argumentos) están separadas de la ejecución para
//! poder probarlas sin abrir ninguna conexión.

use crate::models::{AuthMethod, Server};
use crate::settings::Settings;
use crate::ssh_jump::{self, ASKPASS_BODY};
use crate::store::SecretStore;
use regex::Regex;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CmdError(pub String);

pub type Result<T> = std::result::Result<T, CmdError>;

// MARK: Destino SSH

/// Destino SSH ya resuelto (opciones, entorno con askpass, etc.).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SshTarget {
    pub title: String,
    pub user: String,
    pub host: String,
    pub port: u16,
    pub options: Vec<String>,
    pub env: HashMap<String, String>,
    /// Directorio donde abrir la sesión (opcional).
    pub start_dir: Option<String>,
}

impl SshTarget {
    pub fn destination(&self) -> String {
        if self.user.is_empty() { self.host.clone() } else { format!("{}@{}", self.user, self.host) }
    }
}

/// Destino para un servidor guardado (contraseña o llave, con sus saltos). Lee secretos: no llamarlo en el hilo de la UI.
pub fn target_for_server(
    s: &Server, servers: &[Server], settings: &Settings, secrets: &dyn SecretStore, askpass_path: &str, ssh_path: &str,
) -> SshTarget {
    let mut t = SshTarget {
        title: if s.name.is_empty() { s.host.clone() } else { s.name.clone() },
        user: s.user.clone(),
        host: s.host.clone(),
        port: s.port,
        start_dir: Some(s.start_dir.trim().to_string()).filter(|d| !d.is_empty()),
        ..Default::default()
    };
    if s.auth == AuthMethod::Key && !s.key_path.is_empty() {
        let path = ssh_jump::expand_tilde(&s.key_path);
        t.options.extend(["-i".into(), path.clone(), "-o".into(), "IdentitiesOnly=yes".into()]);
        t.env.insert("TERMINAL_KEY".into(), path);
    }
    let jump = ssh_jump::setup(s, servers, ssh_path, settings, askpass_path, |id: Uuid| secrets.get(id));
    t.options.extend(jump.options);
    t.env.extend(jump.env);
    if let Some(secret) = secrets.get(s.id) {
        t.env.insert("SSH_ASKPASS".into(), askpass_path.into());
        t.env.insert("SSH_ASKPASS_REQUIRE".into(), "force".into());
        t.env.insert("TERMINAL_SECRET".into(), secret);
    }
    t
}

/// Escribe el script askpass (0700) si cambió y devuelve su ruta.
pub fn ensure_askpass(dir: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("askpass.sh");
    if std::fs::read_to_string(&p).ok().as_deref() != Some(ASKPASS_BODY) {
        std::fs::write(&p, ASKPASS_BODY)?;
    }
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700))?;
    Ok(p)
}

/// Comando remoto que entra en `dir` y deja un shell de inicio de sesión. Si la carpeta no existe, el shell abre
/// en la carpeta personal. Sirve en sh, bash, zsh y fish (no usa `${…}` ni `&&`).
pub fn remote_cd_command(dir: &str) -> String {
    let d = dir.trim();
    let cd = match d {
        "" | "~" => "cd".to_string(),
        _ => match d.strip_prefix("~/") {
            // La tilde dentro de comillas no se expande: se compone con $HOME.
            Some(rest) => format!("cd \"$HOME\"{}", quote_shell(&format!("/{rest}"))),
            None => format!("cd {}", quote_shell(d)),
        },
    };
    format!("{cd} 2>/dev/null; exec $SHELL -l")
}

/// Argumentos de `ssh` para una sesión interactiva (se ejecuta en un PTY). Con directorio inicial se fuerza el
/// pseudo-terminal (`-t`) porque se ejecuta un comando remoto.
pub fn interactive_args(t: &SshTarget, settings: &Settings) -> Vec<String> {
    let mut a = vec!["-p".to_string(), t.port.to_string()];
    a.extend(settings.ssh_args());
    a.extend(t.options.iter().cloned());
    let dir = t.start_dir.as_deref().filter(|d| !d.trim().is_empty());
    if dir.is_some() {
        a.push("-t".into());
    }
    a.push(t.destination());
    if let Some(d) = dir {
        a.push(remote_cd_command(d));
    }
    a
}

// MARK: Comillas

/// Comillas para el modo batch de sftp: solo se escapan `\` y `"`.
pub fn quote_arg(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Comillas simples para un comando de shell remoto.
pub fn quote_shell(s: &str) -> String {
    ssh_jump::shq(s)
}

// MARK: Análisis de `ls -la`

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    pub modified: String,
}

fn ls_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^([\-dlcbps])[rwxsStT\-]{9}[+@.]?\s+\S+\s+(\S+)\s+(\S+)\s+(\d+)\s+([A-Za-z]{3})\s+(\d{1,2})\s+(\d{1,2}:\d{2}|\d{4})\s+(.+)$").unwrap()
    })
}

pub fn parse_ls(text: &str) -> Vec<RemoteEntry> {
    let mut out = Vec::new();
    for line in text.split(['\n', '\r']) {
        let Some(m) = ls_re().captures(line) else { continue };
        let kind = &m[1];
        let mut name = m[8].to_string();
        let is_link = kind == "l";
        if is_link {
            if let Some(i) = name.find(" -> ") {
                name.truncate(i);
            }
        }
        // Algunas versiones imprimen la ruta completa: nos quedamos con el último componente.
        if let Some(i) = name.rfind('/') {
            name = name[i + 1..].to_string();
        }
        if name == "." || name == ".." || name.is_empty() {
            continue;
        }
        out.push(RemoteEntry {
            name,
            is_dir: kind == "d",
            is_link,
            size: m[4].parse().unwrap_or(0),
            modified: format!("{} {} {}", &m[5], &m[6], &m[7]),
        });
    }
    out
}

// MARK: Salida de los procesos

fn strip_ansi(s: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new("\u{1b}\\[[0-9;?]*[A-Za-z]").unwrap()).replace_all(s, "").into_owned()
}

/// Deja las últimas líneas útiles de un error de sftp/scp.
pub fn clean_output(s: &str) -> String {
    let s = s.replace("^D", "").replace(['\u{04}', '\u{08}'], "");
    let lines: Vec<String> = strip_ansi(&s)
        .split(['\n', '\r'])
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with("sftp>") && !l.contains("Permanently added"))
        .collect();
    lines[lines.len().saturating_sub(3)..].join("\n")
}

/// Último avance de scp ("archivo   45%  1MB  2.0MB/s") → (porcentaje, "archivo · velocidad").
pub fn parse_progress(text: &str) -> Option<(u32, String)> {
    static R: OnceLock<Regex> = OnceLock::new();
    let re = R.get_or_init(|| Regex::new(r"^(.*?)\s+(\d{1,3})%\s+\S+\s+(\S+/s)").unwrap());
    let plain = strip_ansi(text);
    let last = plain.split(['\n', '\r']).rfind(|l| l.contains('%'))?;
    let m = re.captures(last)?;
    Some((m[2].parse().unwrap_or(0), format!("{} · {}", m[1].trim(), &m[3])))
}

pub struct ProcResult {
    pub status: i32,
    pub out: String,
    pub err: String,
}

/// Ejecuta un proceso y recoge su salida (stdin cerrado). `status` es -1 si no pudo arrancar.
pub fn run_process(exe: &str, args: &[String], env: &HashMap<String, String>) -> ProcResult {
    let child = Command::new(exe)
        .args(args)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match child {
        Ok(o) => ProcResult {
            status: o.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&o.stdout).into_owned(),
            err: String::from_utf8_lossy(&o.stderr).into_owned(),
        },
        Err(e) => ProcResult { status: -1, out: String::new(), err: e.to_string() },
    }
}

// MARK: Argumentos (puros)

pub fn master_args(sock: &Path, t: &SshTarget, settings: &Settings) -> Vec<String> {
    let mut a: Vec<String> = ["-M", "-N", "-S"].iter().map(|s| s.to_string()).collect();
    a.push(sock.display().to_string());
    a.extend(["-o", "ControlPersist=no", "-o", "ConnectTimeout=15", "-p"].map(String::from));
    a.push(t.port.to_string());
    a.extend(settings.ssh_args());
    a.extend(t.options.iter().cloned());
    a.push(t.destination());
    a
}

pub fn sftp_args(sock: &Path, t: &SshTarget, batch: &Path) -> Vec<String> {
    vec![
        "-q".into(), "-o".into(), format!("ControlPath={}", sock.display()), "-P".into(), t.port.to_string(),
        "-b".into(), batch.display().to_string(), t.destination(),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Upload,
    Download,
}

/// Argumentos de `script` para ejecutar scp dentro de un pty (así muestra su barra de progreso).
/// Subida: `sources` locales y `dest_dir` remoto; descarga: al revés.
pub fn transfer_args(sock: &Path, t: &SshTarget, dir: Direction, sources: &[String], dest_dir: &str) -> Vec<String> {
    let dest = t.destination();
    let mut a: Vec<String> = ["-q", "/dev/null", "/usr/bin/scp", "-r", "-o"].map(String::from).into();
    a.push(format!("ControlPath={}", sock.display()));
    a.extend(["-P".into(), t.port.to_string()]);
    match dir {
        Direction::Upload => {
            a.extend(sources.iter().cloned());
            a.push(format!("{dest}:{dest_dir}"));
        }
        Direction::Download => {
            a.extend(sources.iter().map(|s| format!("{dest}:{s}")));
            a.push(dest_dir.into());
        }
    }
    a
}

// MARK: Sesión

/// Para cancelar una transferencia desde otro hilo.
#[derive(Clone, Default)]
pub struct TransferJob {
    cancelled: Arc<AtomicBool>,
}

impl TransferJob {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Carpeta de sockets y archivos temporales (`/tmp/jterm-<uid>`, 0700).
pub fn ctl_dir() -> PathBuf {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let uid = std::fs::metadata(std::env::var_os("HOME").unwrap_or_else(|| "/".into())).map(|m| m.uid()).unwrap_or(0);
    let d = PathBuf::from(format!("/tmp/jterm-{uid}"));
    let _ = std::fs::create_dir_all(&d);
    let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    d
}

fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..8].to_string()
}

pub struct SftpSession {
    pub target: SshTarget,
    pub settings: Settings,
    sock: PathBuf,
    err_file: PathBuf,
    master: Option<std::process::Child>,
    pub home: String,
    closed: bool,
    pub ssh_path: String,
}

impl SftpSession {
    pub fn new(target: SshTarget, settings: Settings) -> Self {
        let dir = ctl_dir();
        let id = short_id();
        SftpSession {
            target,
            settings,
            sock: dir.join(format!("{id}.sock")),
            err_file: dir.join(format!("{id}.err")),
            master: None,
            home: "/".into(),
            closed: false,
            ssh_path: "/usr/bin/ssh".into(),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.sock
    }

    pub fn is_alive(&mut self) -> bool {
        !self.closed && self.master.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    /// Abre la conexión maestra y espera a que esté lista (hasta ~20 s).
    pub fn connect(&mut self) -> Result<()> {
        let err = std::fs::File::create(&self.err_file).map_err(|e| CmdError(e.to_string()))?;
        let child = Command::new(&self.ssh_path)
            .args(master_args(&self.sock, &self.target, &self.settings))
            .envs(&self.target.env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(err)
            .spawn()
            .map_err(|e| CmdError(e.to_string()))?;
        self.master = Some(child);

        let mut ready = false;
        for _ in 0..100 {
            if !self.is_alive() {
                let msg = std::fs::read_to_string(&self.err_file).unwrap_or_default();
                let msg = msg.trim();
                self.cleanup_files();
                return Err(CmdError(if msg.is_empty() { "La conexión SSH terminó inesperadamente.".into() } else { msg.into() }));
            }
            if self.sock.exists() {
                let args = ["-S".to_string(), self.sock.display().to_string(), "-O".into(), "check".into(), self.target.destination()];
                if run_process(&self.ssh_path, &args, &HashMap::new()).status == 0 {
                    ready = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        if !ready {
            self.close();
            return Err(CmdError(format!("Tiempo de espera agotado al conectar con {}.", self.target.host)));
        }
        self.home = self.pwd().unwrap_or_else(|_| "/".into());
        Ok(())
    }

    fn cleanup_files(&self) {
        let _ = std::fs::remove_file(&self.sock);
        let _ = std::fs::remove_file(&self.err_file);
    }

    pub fn close(&mut self) {
        self.closed = true;
        if let Some(mut c) = self.master.take() {
            if matches!(c.try_wait(), Ok(None)) {
                let args = ["-S".to_string(), self.sock.display().to_string(), "-O".into(), "exit".into(), self.target.destination()];
                run_process(&self.ssh_path, &args, &HashMap::new());
                let _ = c.kill();
            }
            let _ = c.wait();
        }
        self.cleanup_files();
    }

    /// Ejecuta comandos de sftp en modo batch y devuelve su salida.
    pub fn sftp(&self, commands: &[String]) -> Result<String> {
        let batch = ctl_dir().join(format!("b-{}.txt", short_id()));
        std::fs::write(&batch, commands.join("\n") + "\n").map_err(|e| CmdError(e.to_string()))?;
        let r = run_process("/usr/bin/sftp", &sftp_args(&self.sock, &self.target, &batch), &self.target.env);
        let _ = std::fs::remove_file(&batch);
        if r.status != 0 {
            let msg = clean_output(&format!("{}\n{}", r.err, r.out));
            return Err(CmdError(if msg.is_empty() { format!("sftp terminó con código {}.", r.status) } else { msg }));
        }
        Ok(r.out)
    }

    pub fn pwd(&self) -> Result<String> {
        let out = self.sftp(&["pwd".into()])?;
        for line in out.lines() {
            if let Some((_, p)) = line.split_once("Remote working directory:") {
                return Ok(p.trim().to_string());
            }
        }
        Ok("/".into())
    }

    pub fn list(&self, path: &str) -> Result<Vec<RemoteEntry>> {
        Ok(parse_ls(&self.sftp(&[format!("ls -la {}", quote_arg(path))])?))
    }

    pub fn mkdir(&self, path: &str) -> Result<()> {
        self.sftp(&[format!("mkdir {}", quote_arg(path))]).map(|_| ())
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.sftp(&[format!("rename {} {}", quote_arg(from), quote_arg(to))]).map(|_| ())
    }

    pub fn remove_remote(&self, paths: &[String]) -> Result<()> {
        let cmd = format!("rm -rf -- {}", paths.iter().map(|p| quote_shell(p)).collect::<Vec<_>>().join(" "));
        let args = ["-S".to_string(), self.sock.display().to_string(), self.target.destination(), cmd];
        let r = run_process(&self.ssh_path, &args, &self.target.env);
        if r.status != 0 {
            let e = r.err.trim();
            return Err(CmdError(if e.is_empty() { "No se pudo eliminar.".into() } else { e.into() }));
        }
        Ok(())
    }

    /// scp con progreso (bloqueante; llamarlo desde un hilo de fondo). `progress(porcentaje, texto)`.
    pub fn run_transfer(
        &self, job: &TransferJob, dir: Direction, sources: &[String], dest_dir: &str, mut progress: impl FnMut(u32, &str),
    ) -> Result<()> {
        let args = transfer_args(&self.sock, &self.target, dir, sources, dest_dir);
        let mut child = Command::new("/usr/bin/script")
            .args(&args)
            .envs(&self.target.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CmdError(e.to_string()))?;
        let mut stdout = child.stdout.take().expect("stdout canalizado");
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });

        let mut text = String::new();
        let status = loop {
            while let Ok(chunk) = rx.try_recv() {
                text.push_str(&String::from_utf8_lossy(&chunk));
                if text.len() > 4000 {
                    let cut = text.char_indices().map(|(i, _)| i).find(|&i| i >= text.len() - 2000).unwrap_or(0);
                    text.drain(..cut);
                }
                if let Some((pct, label)) = parse_progress(&text) {
                    progress(pct, &label);
                }
            }
            if job.is_cancelled() {
                let _ = child.kill();
            }
            match child.try_wait() {
                Ok(Some(st)) => break st,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(CmdError(e.to_string())),
            }
        };
        let _ = reader.join();
        while let Ok(chunk) = rx.try_recv() {
            text.push_str(&String::from_utf8_lossy(&chunk));
        }
        if job.is_cancelled() {
            return Err(CmdError("Cancelado".into()));
        }
        if !status.success() {
            let msg = clean_output(&text);
            return Err(CmdError(if msg.is_empty() {
                format!("La transferencia falló (código {}).", status.code().unwrap_or(-1))
            } else {
                msg
            }));
        }
        Ok(())
    }
}

impl Drop for SftpSession {
    fn drop(&mut self) {
        if !self.closed {
            self.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemorySecrets;

    #[test]
    fn quoting() {
        assert_eq!(quote_arg(r#"a "b" \c"#), r#""a \"b\" \\c""#);
        assert_eq!(quote_shell("it's"), r"'it'\''s'");
    }

    #[test]
    fn ls_parsing() {
        let text = "drwxr-xr-x    5 root     root         4096 Jan  3 12:00 .\n\
                    drwxr-xr-x   20 root     root         4096 Jan  3 12:00 ..\n\
                    drwxr-xr-x    2 ana      ana          4096 Mar 14 09:30 docs\n\
                    -rw-r--r--    1 ana      ana          1234 Dec 25  2023 notas con espacios.txt\n\
                    lrwxrwxrwx    1 ana      ana             9 Jan  1 10:00 enlace -> /etc/hosts\n\
                    -rw-r--r--@   1 ana      ana             0 Jan  1 10:00 /home/ana/ruta_completa\n\
                    sftp> ruido";
        let v = parse_ls(text);
        assert_eq!(v.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["docs", "notas con espacios.txt", "enlace", "ruta_completa"]);
        assert!(v[0].is_dir && !v[0].is_link);
        assert_eq!((v[1].size, v[1].modified.as_str()), (1234, "Dec 25 2023"));
        assert!(v[2].is_link);
    }

    #[test]
    fn progress_and_clean() {
        let t = "\u{1b}[2Kfoo.txt   12%  100KB  1.5MB/s   00:03\rfoo.txt   45%  400KB  2.0MB/s   00:01";
        assert_eq!(parse_progress(t), Some((45, "foo.txt · 2.0MB/s".into())));
        assert_eq!(parse_progress("sin porcentaje"), None);
        let e = "sftp> ls\nWarning: Permanently added 'x'\n\n1\n2\n3\n4\n^D";
        assert_eq!(clean_output(e), "2\n3\n4");
    }

    #[test]
    fn argument_builders() {
        let t = SshTarget { user: "u".into(), host: "h".into(), port: 2200, options: vec!["-i".into(), "k".into()], ..Default::default() };
        let sock = Path::new("/tmp/s.sock");
        let m = master_args(sock, &t, &Settings { ssh_keep_alive: 0, ..Settings::default() });
        assert_eq!(&m[..3], ["-M", "-N", "-S"]);
        assert!(m.windows(2).any(|w| w == ["-p", "2200"]));
        assert_eq!(m.last().unwrap(), "u@h");
        assert!(m.windows(2).any(|w| w == ["-i", "k"]));

        let up = transfer_args(sock, &t, Direction::Upload, &["/a".into(), "/b".into()], "/srv");
        assert_eq!(&up[up.len() - 3..], ["/a", "/b", "u@h:/srv"]);
        let down = transfer_args(sock, &t, Direction::Download, &["/r1".into()], "/local");
        assert_eq!(&down[down.len() - 2..], ["u@h:/r1", "/local"]);

        let s = sftp_args(sock, &t, Path::new("/tmp/b.txt"));
        assert!(s.contains(&"ControlPath=/tmp/s.sock".to_string()) && s.contains(&"-P".to_string()));
    }

    #[test]
    fn interactive_args_order() {
        let t = SshTarget { user: "u".into(), host: "h".into(), port: 2200, options: vec!["-i".into(), "k".into()], ..Default::default() };
        let a = interactive_args(&t, &Settings { ssh_keep_alive: 0, ..Settings::default() });
        assert_eq!(a, ["-p", "2200", "-o", "StrictHostKeyChecking=accept-new", "-i", "k", "u@h"]);
    }

    #[test]
    fn start_directory_adds_a_remote_command() {
        assert_eq!(remote_cd_command("/var/www/app"), "cd '/var/www/app' 2>/dev/null; exec $SHELL -l");
        assert_eq!(remote_cd_command("~"), "cd 2>/dev/null; exec $SHELL -l");
        assert_eq!(remote_cd_command("~/mi proyecto"), "cd \"$HOME\"'/mi proyecto' 2>/dev/null; exec $SHELL -l");
        assert_eq!(remote_cd_command("it's"), "cd 'it'\\''s' 2>/dev/null; exec $SHELL -l");
        // Una inyección en el nombre queda como texto entrecomillado, no como otro comando.
        assert!(remote_cd_command("/x'; rm -rf /; '").contains("'\\''; rm -rf /; '\\'''"));
        let mut t = SshTarget { user: "u".into(), host: "h".into(), port: 22, ..Default::default() };
        let plain = interactive_args(&t, &Settings { ssh_keep_alive: 0, ..Settings::default() });
        assert!(!plain.contains(&"-t".to_string()) && plain.last().unwrap() == "u@h");
        t.start_dir = Some("/srv".into());
        let a = interactive_args(&t, &Settings { ssh_keep_alive: 0, ..Settings::default() });
        let n = a.len();
        assert_eq!(&a[n - 3..], ["-t", "u@h", "cd '/srv' 2>/dev/null; exec $SHELL -l"]);
        t.start_dir = Some("   ".into());
        assert!(!interactive_args(&t, &Settings::default()).contains(&"-t".to_string()));
    }

    #[test]
    fn target_uses_secret_and_key() {
        let secrets = MemorySecrets::default();
        let s = Server {
            name: "web".into(), host: "h".into(), user: "u".into(), auth: AuthMethod::Key, key_path: "/k/id".into(),
            ..Server::default()
        };
        secrets.set(s.id, "pw");
        let t = target_for_server(&s, std::slice::from_ref(&s), &Settings::default(), &secrets, "/x/askpass.sh", "/usr/bin/ssh");
        assert_eq!(t.title, "web");
        assert_eq!(t.env["TERMINAL_KEY"], "/k/id");
        assert_eq!(t.env["TERMINAL_SECRET"], "pw");
        assert_eq!(t.env["SSH_ASKPASS_REQUIRE"], "force");
        assert!(t.options.contains(&"IdentitiesOnly=yes".to_string()));
    }

    #[test]
    fn askpass_file_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = ensure_askpass(d.path()).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), ASKPASS_BODY);
    }

    #[test]
    fn run_process_captures_and_reports_missing() {
        let r = run_process("/bin/echo", &["hola".into()], &HashMap::new());
        assert_eq!((r.status, r.out.trim()), (0, "hola"));
        assert_eq!(run_process("/no/existe", &[], &HashMap::new()).status, -1);
    }

    #[test]
    fn job_cancel_flag() {
        let j = TransferJob::default();
        let j2 = j.clone();
        assert!(!j.is_cancelled());
        j2.cancel();
        assert!(j.is_cancelled());
    }
}
