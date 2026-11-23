//! Actualizador: busca versiones nuevas en los Releases de GitHub y se instala solo.
//!
//! Flujo (igual que el de la versión Swift, para que las dos se actualicen entre sí):
//! `releases/latest` → compara con la versión instalada → descarga `jTerm-<versión>.dmg` → verifica su SHA-256
//! (`.dmg.sha256` del mismo release) → copia la .app nueva junto a la actual → un script auxiliar espera a que la
//! app se cierre, intercambia las dos y la vuelve a abrir.
//!
//! Toda la red pasa por el trait [`Fetcher`], así que las pruebas nunca hablan con GitHub.

use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub tag: String,
    pub dmg: String,
    pub sha: String,
}

/// Salida de red del actualizador.
pub trait Fetcher: Send + Sync {
    /// `HEAD https://github.com/<repo>/releases/latest` SIN seguir la redirección: (código, cabecera `Location`).
    fn latest_redirect(&self, repo: &str) -> impl Future<Output = Result<(u16, Option<String>), String>> + Send;
    /// Descarga `url` a `dest` (siguiendo redirecciones).
    fn download(&self, url: &str, dest: &Path) -> impl Future<Output = Result<(), String>> + Send;
}

/// Fetcher real contra github.com.
pub struct GithubFetcher {
    plain: reqwest::Client,
    no_redirect: reqwest::Client,
}

impl GithubFetcher {
    pub fn new() -> Result<Self, String> {
        let build = |policy| {
            reqwest::Client::builder().user_agent("jTerm-updater").redirect(policy).connect_timeout(std::time::Duration::from_secs(15)).build().map_err(|e| e.to_string())
        };
        Ok(GithubFetcher { plain: build(reqwest::redirect::Policy::limited(10))?, no_redirect: build(reqwest::redirect::Policy::none())? })
    }
}

impl Fetcher for GithubFetcher {
    async fn latest_redirect(&self, repo: &str) -> Result<(u16, Option<String>), String> {
        let r = self
            .no_redirect
            .head(format!("https://github.com/{repo}/releases/latest"))
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let loc = r.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).map(String::from);
        Ok((r.status().as_u16(), loc))
    }

    async fn download(&self, url: &str, dest: &Path) -> Result<(), String> {
        use tokio::io::AsyncWriteExt;
        let mut r = self.plain.get(url).timeout(std::time::Duration::from_secs(1800)).send().await.map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("GitHub respondió con el código {}.", r.status().as_u16()));
        }
        let mut f = tokio::fs::File::create(dest).await.map_err(|e| e.to_string())?;
        while let Some(chunk) = r.chunk().await.map_err(|e| e.to_string())? {
            f.write_all(&chunk).await.map_err(|e| e.to_string())?;
        }
        f.flush().await.map_err(|e| e.to_string())
    }
}

// MARK: Versiones y releases

/// ¿`a` es una versión más nueva que `b`? Compara números separados por puntos; lo que sigue a un "-" se ignora.
pub fn is_newer(a: &str, b: &str) -> bool {
    fn parts(s: &str) -> Vec<u64> {
        s.split('-').next().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect()
    }
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p > q;
        }
    }
    false
}

/// A qué release apunta la redirección de `/releases/latest` (`…/releases/tag/vX.Y.Z`).
pub fn release_from_redirect(repo: &str, status: u16, location: Option<&str>) -> Result<Release, String> {
    let tag_part = location.and_then(|l| l.split_once("/releases/tag/")).map(|(_, t)| t);
    let Some(raw) = tag_part.filter(|_| (300..400).contains(&status)) else {
        if status == 404 || (300..400).contains(&status) {
            return Err(format!("No hay ningún release publicado en {repo} (o el repositorio no es público)."));
        }
        return Err(format!("GitHub respondió con el código {status}."));
    };
    let tag = raw.split(['?', '#']).next().unwrap_or("").trim_end_matches('/').to_string();
    if tag.is_empty() || tag.contains('/') || tag.contains("..") {
        return Err("No se pudo leer la versión del release.".into());
    }
    let version = tag.strip_prefix(['v', 'V']).unwrap_or(&tag).to_string();
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        return Err(format!("Versión no válida: {tag}."));
    }
    let base = format!("https://github.com/{repo}/releases/download/{tag}/jTerm-{version}.dmg");
    Ok(Release { version, tag, dmg: base.clone(), sha: format!("{base}.sha256") })
}

/// Consulta el último release. `Some` solo si es más nuevo que `current`.
pub async fn check<F: Fetcher>(f: &F, repo: &str, current: &str) -> Result<Option<Release>, String> {
    let (status, loc) = f.latest_redirect(repo).await?;
    let rel = release_from_redirect(repo, status, loc.as_deref())?;
    Ok(is_newer(&rel.version, current).then_some(rel))
}

/// Primer token hexadecimal de 64 caracteres de un archivo `.sha256` (`<hash>  jTerm-1.2.3.dmg`).
pub fn parse_sha256(text: &str) -> Option<String> {
    let t = text.split_whitespace().next()?.to_lowercase();
    (t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit())).then_some(t)
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

// MARK: Información de la app instalada

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleInfo {
    pub path: PathBuf,
    pub identifier: String,
    pub version: String,
    /// "usuario/repo" (clave `JTermUpdateRepo` del Info.plist).
    pub repo: Option<String>,
}

pub fn read_plist(path: &Path) -> Option<plist::Dictionary> {
    plist::Value::from_file(path).ok()?.into_dictionary()
}

/// La .app desde la que se ejecuta el binario (`…/jTerm.app/Contents/MacOS/jterm`), si es el caso.
pub fn bundle_of_exe(exe: &Path) -> Option<BundleInfo> {
    let path = exe.ancestors().nth(3)?.to_path_buf();
    if path.extension()? != "app" {
        return None;
    }
    let d = read_plist(&path.join("Contents/Info.plist"))?;
    let s = |k: &str| d.get(k).and_then(|v| v.as_string()).map(str::to_string);
    Some(BundleInfo {
        identifier: s("CFBundleIdentifier")?,
        version: s("CFBundleShortVersionString")?,
        repo: s("JTermUpdateRepo").map(|r| r.trim().to_string()).filter(|r| !r.is_empty()),
        path,
    })
}

// MARK: Instalación

/// Lo que queda listo para el intercambio final.
#[derive(Debug, Clone)]
pub struct Staged {
    pub staged_app: PathBuf,
    pub work: PathBuf,
    pub script: PathBuf,
}

/// ¿Se puede escribir en la carpeta que contiene la app? (si no, hay que descargar el instalador a mano)
pub fn can_write_next_to(bundle: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    bundle.parent().and_then(|p| std::fs::metadata(p).ok()).is_some_and(|m| !m.permissions().readonly() && (m.permissions().mode() & 0o200 != 0)) && {
        let probe = bundle.parent().unwrap().join(format!(".jterm-write-test-{}", std::process::id()));
        let ok = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }
}

fn run(exe: &str, args: &[&str]) -> (i32, String) {
    match Command::new(exe).args(args).stdin(Stdio::null()).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
        Err(e) => (-1, e.to_string()),
    }
}

/// Dispositivo del disco entero que devolvió `hdiutil attach -plist` (p. ej. `/dev/disk16`).
pub fn device_from_attach_plist(xml: &str) -> Option<String> {
    let start = xml.find("<?xml")?;
    let v: plist::Value = plist::Value::from_reader_xml(std::io::Cursor::new(xml[start..].as_bytes())).ok()?;
    let entities = v.as_dictionary()?.get("system-entities")?.as_array()?;
    let dev = entities.iter().filter_map(|e| e.as_dictionary()?.get("dev-entry")?.as_string()).next()?;
    // `/dev/disk16s1` → `/dev/disk16`
    let base = dev.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('s');
    Some(if base.len() > "/dev/disk".len() { base.to_string() } else { dev.to_string() })
}

/// Expulsa la imagen montada: primero el disco entero y, si no se pudo, el punto de montaje.
fn detach_image(attach_output: &str, mount: &Path) {
    if let Some(dev) = device_from_attach_plist(attach_output) {
        if run("/usr/bin/hdiutil", &["detach", "-force", &dev]).0 == 0 {
            return;
        }
    }
    let _ = run("/usr/bin/hdiutil", &["detach", "-force", &mount.to_string_lossy()]);
}

/// Script que espera a que termine el proceso `$1`, intercambia `$2` (app instalada) por `$3` (nueva) y la reabre.
pub fn swap_script(open_after: bool) -> String {
    format!(
        r#"#!/bin/sh
PID="$1"; DEST="$2"; NEW="$3"; WORK="$4"
i=0
while kill -0 "$PID" 2>/dev/null; do
  i=$((i+1)); [ $i -gt 240 ] && {{ rm -rf "$NEW" "$WORK"; exit 1; }}   # el usuario canceló la salida
  sleep 0.5
done
OLD="$DEST.old"
rm -rf "$OLD"
mv "$DEST" "$OLD" && mv "$NEW" "$DEST" || {{ [ -e "$DEST" ] || mv "$OLD" "$DEST"; rm -rf "$NEW" "$WORK"; exit 1; }}
xattr -dr com.apple.quarantine "$DEST" 2>/dev/null
rm -rf "$OLD"
{open}
rm -rf "$WORK"
"#,
        open = if open_after { r#"open "$DEST""# } else { ":" }
    )
}

/// Descarga, verifica y deja preparada la versión nueva. No toca la app instalada.
pub async fn prepare<F: Fetcher>(f: &F, rel: &Release, bundle: &Path, bundle_id: &str, progress: &(dyn Fn(&str) + Send + Sync)) -> Result<Staged, String> {
    if bundle.extension().is_none_or(|e| e != "app") {
        return Err("jTerm no se está ejecutando desde una .app.".into());
    }
    let parent = bundle.parent().ok_or("Ruta de la app no válida.")?.to_path_buf();
    let work = std::env::temp_dir().join(format!("jterm-update-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let result = prepare_in(f, rel, &parent, bundle_id, &work, progress).await;
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&work);
    }
    result
}

async fn prepare_in<F: Fetcher>(f: &F, rel: &Release, parent: &Path, bundle_id: &str, work: &Path, progress: &(dyn Fn(&str) + Send + Sync)) -> Result<Staged, String> {
    progress(&format!("Descargando jTerm {}…", rel.version));
    let dmg = work.join("update.dmg");
    f.download(&rel.dmg, &dmg).await?;
    let sha_file = work.join("update.sha256");
    f.download(&rel.sha, &sha_file).await.map_err(|_| "El release no incluye el archivo .sha256; por seguridad no se instala.".to_string())?;
    let expected = parse_sha256(&std::fs::read_to_string(&sha_file).map_err(|e| e.to_string())?).unwrap_or_default();
    let dmg2 = dmg.clone();
    let actual = tokio::task::spawn_blocking(move || sha256_file(&dmg2)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    if expected.is_empty() || expected != actual {
        return Err("La descarga está dañada (el SHA-256 no coincide). No se instaló.".into());
    }

    progress("Preparando la actualización…");
    let (work2, parent2, version, id) = (work.to_path_buf(), parent.to_path_buf(), rel.version.clone(), bundle_id.to_string());
    tokio::task::spawn_blocking(move || -> Result<Staged, String> {
        let mount = work2.join("mnt");
        std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
        // Misma carpeta que la app instalada: el intercambio final es un `mv` atómico en el mismo volumen.
        let staged = parent2.join(".jTerm-update.app");
        let _ = std::fs::remove_dir_all(&staged);
        let (code, out) = run("/usr/bin/hdiutil", &["attach", "-plist", "-nobrowse", "-readonly", "-noverify", "-noautoopen", "-mountpoint", &mount.to_string_lossy(), &work2.join("update.dmg").to_string_lossy()]);
        if code != 0 {
            return Err("No se pudo abrir el instalador descargado.".into());
        }
        let (copy_code, _) = run("/usr/bin/ditto", &[&mount.join("jTerm.app").to_string_lossy(), &staged.to_string_lossy()]);
        // Se expulsa el disco entero (no solo el volumen): si no, la imagen puede quedar adjunta tras la actualización.
        detach_image(&out, &mount);
        if copy_code != 0 {
            let _ = std::fs::remove_dir_all(&staged);
            return Err("No se pudo copiar la nueva versión.".into());
        }
        // Lo copiado debe ser jTerm y la versión esperada.
        let info = read_plist(&staged.join("Contents/Info.plist"));
        let get = |k: &str| info.as_ref().and_then(|d| d.get(k)).and_then(|v| v.as_string()).map(str::to_string);
        if get("CFBundleIdentifier").as_deref() != Some(id.as_str()) || get("CFBundleShortVersionString").as_deref() != Some(version.as_str()) {
            let _ = std::fs::remove_dir_all(&staged);
            return Err(format!("El paquete descargado no corresponde a jTerm {version}."));
        }
        let script = work2.join("swap.sh");
        std::fs::write(&script, swap_script(true)).map_err(|e| e.to_string())?;
        Ok(Staged { staged_app: staged, work: work2, script })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Lanza el script de intercambio en segundo plano. Después hay que cerrar la app (`pid`).
pub fn launch_swap(staged: &Staged, pid: u32, dest: &Path) -> std::io::Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"nohup /bin/sh "$0" "$@" >/dev/null 2>&1 &"#)
        .arg(&staged.script)
        .arg(pid.to_string())
        .arg(dest)
        .arg(&staged.staged_app)
        .arg(&staged.work)
        .stdin(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"), "se compara por números, no por texto");
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("0.2.0-beta", "0.2.0"), "lo posterior al guion se ignora");
    }

    #[test]
    fn release_from_the_redirect() {
        let r = release_from_redirect("u/r", 302, Some("https://github.com/u/r/releases/tag/v0.3.0")).unwrap();
        assert_eq!((r.version.as_str(), r.tag.as_str()), ("0.3.0", "v0.3.0"));
        assert_eq!(r.dmg, "https://github.com/u/r/releases/download/v0.3.0/jTerm-0.3.0.dmg");
        assert_eq!(r.sha, format!("{}.sha256", r.dmg));
        assert!(release_from_redirect("u/r", 302, Some("https://github.com/u/r/releases")).unwrap_err().contains("No hay ningún release"));
        assert!(release_from_redirect("u/r", 404, None).unwrap_err().contains("No hay ningún release"));
        assert!(release_from_redirect("u/r", 500, None).unwrap_err().contains("500"));
        // Un tag con rutas o caracteres raros no se acepta (evita URLs manipuladas).
        assert!(release_from_redirect("u/r", 302, Some("https://github.com/u/r/releases/tag/v1/../../x")).is_err());
        assert!(release_from_redirect("u/r", 302, Some("https://github.com/u/r/releases/tag/v1;rm")).is_err());
    }

    #[test]
    fn sha_file_parsing() {
        let h = "a".repeat(64);
        assert_eq!(parse_sha256(&format!("{}  jTerm-1.dmg\n", h.to_uppercase())), Some(h.clone()));
        assert_eq!(parse_sha256("corto"), None);
        assert_eq!(parse_sha256(&format!("{}zz", "a".repeat(62))), None);
        assert_eq!(parse_sha256(""), None);
    }

    #[test]
    fn attach_output_gives_the_whole_disk_device() {
        let xml = r#"warning: algo
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>system-entities</key><array>
<dict><key>content-hint</key><string>GUID_partition_scheme</string><key>dev-entry</key><string>/dev/disk16</string></dict>
<dict><key>dev-entry</key><string>/dev/disk16s1</string><key>mount-point</key><string>/tmp/x</string></dict>
</array></dict></plist>"#;
        assert_eq!(device_from_attach_plist(xml).as_deref(), Some("/dev/disk16"));
        let only_slice = xml.replace("<string>/dev/disk16</string>", "<string>/dev/disk16s1</string>");
        assert_eq!(device_from_attach_plist(&only_slice).as_deref(), Some("/dev/disk16"));
        assert_eq!(device_from_attach_plist("sin plist"), None);
    }

    #[test]
    fn swap_script_has_the_safety_steps() {
        let s = swap_script(true);
        assert!(s.contains("kill -0") && s.contains("xattr -dr com.apple.quarantine") && s.contains(r#"open "$DEST""#));
        assert!(!swap_script(false).contains("open \"$DEST\""));
    }

    /// Fetcher de mentira: responde con valores fijos y copia archivos locales. No abre conexiones.
    struct Fake {
        redirect: (u16, Option<String>),
        files: Mutex<HashMap<String, PathBuf>>,
        texts: Mutex<HashMap<String, String>>,
    }

    impl Fake {
        fn new(redirect: (u16, Option<&str>)) -> Self {
            Fake { redirect: (redirect.0, redirect.1.map(String::from)), files: Default::default(), texts: Default::default() }
        }
    }

    impl Fetcher for Fake {
        async fn latest_redirect(&self, _repo: &str) -> Result<(u16, Option<String>), String> {
            Ok(self.redirect.clone())
        }
        async fn download(&self, url: &str, dest: &Path) -> Result<(), String> {
            if let Some(src) = self.files.lock().unwrap().get(url) {
                return std::fs::copy(src, dest).map(|_| ()).map_err(|e| e.to_string());
            }
            if let Some(t) = self.texts.lock().unwrap().get(url) {
                return std::fs::write(dest, t).map_err(|e| e.to_string());
            }
            Err("404".into())
        }
    }

    #[tokio::test]
    async fn check_reports_only_newer_releases() {
        let f = Fake::new((302, Some("https://github.com/u/r/releases/tag/v0.3.0")));
        assert_eq!(check(&f, "u/r", "0.2.0").await.unwrap().unwrap().version, "0.3.0");
        assert_eq!(check(&f, "u/r", "0.3.0").await.unwrap(), None);
        assert_eq!(check(&f, "u/r", "1.0.0").await.unwrap(), None);
        let none = Fake::new((404, None));
        assert!(check(&none, "u/r", "0.2.0").await.is_err());
    }

    #[test]
    fn bundle_detection_reads_the_info_plist() {
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("jTerm.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        let mut dict = plist::Dictionary::new();
        dict.insert("CFBundleIdentifier".into(), "cl.jotaele.jterm".into());
        dict.insert("CFBundleShortVersionString".into(), "0.3.0".into());
        dict.insert("JTermUpdateRepo".into(), "u/r".into());
        plist::Value::Dictionary(dict).to_file_xml(app.join("Contents/Info.plist")).unwrap();
        let info = bundle_of_exe(&app.join("Contents/MacOS/jTerm")).unwrap();
        assert_eq!((info.identifier.as_str(), info.version.as_str(), info.repo.as_deref()), ("cl.jotaele.jterm", "0.3.0", Some("u/r")));
        assert_eq!(info.path, app);
        assert!(bundle_of_exe(Path::new("/usr/bin/ssh")).is_none(), "fuera de una .app no hay actualizador");
        assert!(bundle_of_exe(&d.path().join("suelto/Contents/MacOS/x")).is_none());
        assert!(can_write_next_to(&app));
    }

    /// DMG de verdad (hdiutil) con una `jTerm.app` falsa dentro: ejercita descarga simulada, hash, montaje, copia y validación.
    fn make_dmg(dir: &Path, identifier: &str, version: &str) -> Option<PathBuf> {
        let src = dir.join("src");
        let app = src.join("jTerm.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/MacOS/jTerm"), "#!/bin/sh\n").unwrap();
        let mut dict = plist::Dictionary::new();
        dict.insert("CFBundleIdentifier".into(), identifier.into());
        dict.insert("CFBundleShortVersionString".into(), version.into());
        plist::Value::Dictionary(dict).to_file_xml(app.join("Contents/Info.plist")).unwrap();
        let out = dir.join(format!("{version}.dmg"));
        let (code, msg) = run("/usr/bin/hdiutil", &["create", "-volname", "jTerm", "-srcfolder", &src.to_string_lossy(), "-ov", "-format", "UDZO", &out.to_string_lossy()]);
        if code != 0 {
            // Algunos Macs no pueden crear imágenes en ese momento («Recurso ocupado»): la prueba se omite, no falla.
            eprintln!("OMITIDA: hdiutil no pudo crear la imagen de prueba: {msg}");
            return None;
        }
        Some(out)
    }

    fn release_for(version: &str) -> Release {
        release_from_redirect("u/r", 302, Some(&format!("https://github.com/u/r/releases/tag/v{version}"))).unwrap()
    }

    /// `hdiutil` falla con «Recurso ocupado» si varias pruebas crean y montan imágenes a la vez: se hacen de una en una.
    static HDIUTIL: Mutex<()> = Mutex::new(());

    async fn stage(version_in_dmg: &str, sha_override: Option<String>, id: &str) -> Option<(tempfile::TempDir, Result<Staged, String>)> {
        let _serial = HDIUTIL.lock().unwrap_or_else(|e| e.into_inner());
        let d = tempfile::tempdir().unwrap();
        let dmg = make_dmg(d.path(), "cl.jotaele.jterm", version_in_dmg)?;
        let installed = d.path().join("Apps");
        std::fs::create_dir_all(installed.join("jTerm.app")).unwrap();
        let rel = release_for("0.3.0");
        let f = Fake::new((302, None));
        let sha = sha_override.unwrap_or_else(|| sha256_file(&dmg).unwrap());
        f.files.lock().unwrap().insert(rel.dmg.clone(), dmg);
        f.texts.lock().unwrap().insert(rel.sha.clone(), format!("{sha}  jTerm-0.3.0.dmg\n"));
        let r = prepare(&f, &rel, &installed.join("jTerm.app"), id, &|_| {}).await;
        Some((d, r))
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn prepares_a_verified_update_without_touching_the_installed_app() {
        let Some((d, r)) = stage("0.3.0", None, "cl.jotaele.jterm").await else { return };
        let s = r.unwrap();
        assert!(s.staged_app.join("Contents/MacOS/jTerm").exists());
        assert!(s.script.exists());
        assert_eq!(s.staged_app, d.path().join("Apps/.jTerm-update.app"));
        // La app instalada sigue intacta.
        assert!(d.path().join("Apps/jTerm.app").exists());
        let _ = std::fs::remove_dir_all(&s.work);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn rejects_a_corrupt_download_a_wrong_version_and_a_foreign_app() {
        let Some((_d, r)) = stage("0.3.0", Some("0".repeat(64)), "cl.jotaele.jterm").await else { return };
        assert!(r.unwrap_err().contains("SHA-256"));
        let Some((d2, r)) = stage("0.2.5", None, "cl.jotaele.jterm").await else { return };
        assert!(r.unwrap_err().contains("no corresponde a jTerm 0.3.0"));
        assert!(!d2.path().join("Apps/.jTerm-update.app").exists(), "no deja restos");
        let Some((_d3, r)) = stage("0.3.0", None, "com.otra.app").await else { return };
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn refuses_to_install_without_a_sha_file_or_outside_an_app() {
        let d = tempfile::tempdir().unwrap();
        let rel = release_for("0.3.0");
        let f = Fake::new((302, None));
        std::fs::write(d.path().join("x.dmg"), "datos").unwrap();
        f.files.lock().unwrap().insert(rel.dmg.clone(), d.path().join("x.dmg"));
        let app = d.path().join("jTerm.app");
        std::fs::create_dir_all(&app).unwrap();
        assert!(prepare(&f, &rel, &app, "cl.jotaele.jterm", &|_| {}).await.unwrap_err().contains(".sha256"));
        assert!(prepare(&f, &rel, &d.path().join("jterm"), "cl.jotaele.jterm", &|_| {}).await.unwrap_err().contains("desde una .app"));
    }

    /// Ejecuta el script de intercambio de verdad, en una carpeta temporal, con un proceso que ya terminó y sin reabrir nada.
    #[test]
    fn the_swap_script_replaces_the_app_and_cleans_up() {
        let d = tempfile::tempdir().unwrap();
        let (dest, new, work) = (d.path().join("jTerm.app"), d.path().join(".jTerm-update.app"), d.path().join("work"));
        for (p, mark) in [(&dest, "vieja"), (&new, "nueva")] {
            std::fs::create_dir_all(p).unwrap();
            std::fs::write(p.join("version.txt"), mark).unwrap();
        }
        std::fs::create_dir_all(&work).unwrap();
        let script = work.join("swap.sh");
        std::fs::write(&script, swap_script(false)).unwrap();
        let mut dead = Command::new("/usr/bin/true").spawn().unwrap();
        let pid = dead.id();
        dead.wait().unwrap();
        let st = Command::new("/bin/sh").arg(&script).arg(pid.to_string()).arg(&dest).arg(&new).arg(&work).status().unwrap();
        assert!(st.success());
        assert_eq!(std::fs::read_to_string(dest.join("version.txt")).unwrap(), "nueva");
        assert!(!new.exists() && !work.exists() && !d.path().join("jTerm.app.old").exists());
    }
}
