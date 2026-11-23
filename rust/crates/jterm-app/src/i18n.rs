//! Idioma de la interfaz: español (base) e inglés. La clave de cada traducción es el texto en español.
//!
//! El idioma se fija al arrancar (cambiarlo pide reiniciar, igual que en la versión Swift). Las traducciones salen de
//! `i18n_en.rs` (las de la versión Swift) más las de `EXTRA` (textos nuevos de la versión Rust) y unos patrones para
//! los mensajes con números o nombres ("12 instancias", "Siguiendo · 5 eventos…").

use crate::i18n_en::SWIFT;
use regex::Regex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_english(on: bool) {
    ENGLISH.store(on, Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// Idioma pedido en ajustes ("system", "es" o "en") → ¿inglés?
pub fn resolve(pref: &str, system_languages: &[String]) -> bool {
    match pref {
        "en" => true,
        "es" => false,
        _ => system_languages.first().map(|l| !l.to_lowercase().starts_with("es")).unwrap_or(false),
    }
}

/// Idiomas del sistema, del preferido al menos preferido (`defaults read -g AppleLanguages`).
pub fn system_languages() -> Vec<String> {
    let out = std::process::Command::new("defaults").args(["read", "-g", "AppleLanguages"]).output().ok();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let v: Vec<String> = text.lines().map(|l| l.trim().trim_matches(|c| c == '(' || c == ')' || c == ',' || c == '"').trim().to_string()).filter(|l| !l.is_empty()).collect();
    if v.is_empty() {
        std::env::var("LANG").ok().map(|l| vec![l.replace('_', "-")]).unwrap_or_default()
    } else {
        v
    }
}

/// Textos nuevos de la versión Rust (los de Swift ya están en `SWIFT`).
const EXTRA: &[(&str, &str)] = &[
    ("Terminal para macOS con SSH, AWS, archivos y comandos guardados.", "macOS terminal with SSH, AWS, files and saved commands."),
    ("Ocultar jTerm", "Hide jTerm"),
    ("Ocultar otros", "Hide Others"),
    ("Mostrar todo", "Show All"),
    ("Pantalla completa", "Enter Full Screen"),
    ("Minimizar", "Minimize"),
    ("Acerca de jTerm", "About jTerm"),
    ("Salir de jTerm", "Quit jTerm"),
    ("Ajustes…", "Settings…"),
    ("Buscar actualizaciones…", "Check for Updates…"),
    ("Archivo", "File"),
    ("Edición", "Edit"),
    ("Paneles", "Panes"),
    ("Comandos", "Commands"),
    ("↩ abrir     ⌥↩ pegar y ejecutar     ⌘P fijar     ↑↓ moverse     esc cerrar", "↩ open     ⌥↩ paste and run     ⌘P pin     ↑↓ move     esc close"),
    ("Pestañas", "Tabs"),
    ("Ventana", "Window"),
    ("Copiar", "Copy"),
    ("Pegar", "Paste"),
    ("Seleccionar todo", "Select All"),
    ("Nueva pestaña", "New Tab"),
    ("Cerrar panel o pestaña", "Close Pane or Tab"),
    ("Cerrar pestaña entera", "Close Entire Tab"),
    ("Buscar…", "Find…"),
    ("Buscar siguiente", "Find Next"),
    ("Buscar anterior", "Find Previous"),
    ("Prompt anterior", "Previous Prompt"),
    ("Prompt siguiente", "Next Prompt"),
    ("Dividir a la derecha", "Split Right"),
    ("Dividir hacia abajo", "Split Down"),
    ("Dividir a la derecha con terminal local", "Split Right with Local Terminal"),
    ("Dividir hacia abajo con terminal local", "Split Down with Local Terminal"),
    ("Ir al panel de la izquierda", "Go to Left Pane"),
    ("Ir al panel de la derecha", "Go to Right Pane"),
    ("Ir al panel de arriba", "Go to Pane Above"),
    ("Ir al panel de abajo", "Go to Pane Below"),
    ("Panel siguiente", "Next Pane"),
    ("Panel anterior", "Previous Pane"),
    ("Maximizar panel", "Maximize Pane"),
    ("Escribir en todos los paneles", "Type in All Panes"),
    ("Mostrar comandos guardados", "Show Saved Commands"),
    ("Buscar servidor, instancia o comando…", "Find Server, Instance or Command…"),
    ("Siguiente", "Next"),
    ("Anterior", "Previous"),
    ("Mostrar u ocultar la barra lateral", "Show or Hide Sidebar"),
    ("Indica el ID de clave de acceso (sin espacios).", "Enter the access key ID (no spaces)."),
    ("Indica la clave de acceso secreta.", "Enter the secret access key."),
    ("Se guarda en el Keychain de macOS, nunca en un archivo.", "It is stored in the macOS Keychain, never in a file."),
    ("Indica el host (sin espacios).", "Enter the host (no spaces)."),
    ("Indica la ruta de la llave privada.", "Enter the path to the private key."),
    ("🔎 Logs", "🔎 Logs"),
    ("✕ Cerrar", "✕ Close"),
    ("Cargando…", "Loading…"),
    ("Sin conexión.", "Not connected."),
    ("Carpeta vacía.", "Empty folder."),
    ("Se moverá a la papelera.", "It will be moved to the Trash."),
    ("Se eliminará del servidor y no se puede recuperar.", "It will be deleted from the server and cannot be recovered."),
    ("Grupo de logs", "Log group"),
    ("Filtro (sintaxis de CloudWatch)", "Filter (CloudWatch syntax)"),
    ("Guardar…", "Save…"),
    ("Mostrar anteriores ↑", "Show earlier ↑"),
    ("Sin eventos todavía.", "No events yet."),
    ("Ningún evento coincide con el filtro de nivel.", "No event matches the level filter."),
    ("Copiar evento", "Copy event"),
    ("No hay grupos cargados. Pulsa Recargar para consultarlos.", "No groups loaded. Press Reload to fetch them."),
    ("No se puede escribir en todos los paneles: hay uno de PRODUCCIÓN.", "Cannot type in all panes: one of them is PRODUCTION."),
    ("No se pudo iniciar el cliente de red.", "Could not start the network client."),
    ("Instancia sin IP pública: la conexión por SSM Session Manager aún no está disponible en esta versión.", "Instance without a public IP: connecting through SSM Session Manager is not available in this version yet."),
    ("Visor cerrado.", "Viewer closed."),
    ("Solo se pueden subir archivos (no carpetas).", "Only files can be uploaded (not folders)."),
    ("El nombre de la carpeta no puede estar vacío ni contener «/».", "The folder name cannot be empty or contain “/”."),
    ("Eliminando…", "Deleting…"),
    ("Buscar en el terminal", "Find in Terminal"),
    ("Buscar en el terminal…", "Find in terminal…"),
    ("Buscar actualizaciones", "Check for Updates"),
    ("No hay una sincronización activa que cambiar.", "There is no active sync to change."),
    ("El nombre no puede estar vacío ni contener «/».", "The name cannot be empty or contain “/”."),
    ("El túnel no tiene un servidor válido: edítalo y elige uno.", "The tunnel has no valid server: edit it and choose one."),
    ("Pulsa Buscar para ver los eventos.", "Press Search to see the events."),
    ("Guardar logs", "Save logs"),
    ("Subir a S3", "Upload to S3"),
    ("Descargar en…", "Download to…"),
    ("URL firmada copiada al portapapeles (válida 1 hora).", "Signed URL copied to the clipboard (valid for 1 hour)."),
    ("Escribiendo en todos los paneles", "Typing in all panes"),
    ("Sin servidores guardados.", "No saved servers."),
    ("Botón flotante en la esquina inferior derecha del terminal. Si lo ocultas, sigues pudiendo abrir la lista con ⇧⌘K.", "Floating button in the bottom-right corner of the terminal. If you hide it, you can still open the list with ⇧⌘K."),
    ("El entorno de cada servidor se elige al editarlo.", "Each server's environment is chosen when editing it."),
    ("Vuelve todos los ajustes a sus valores por defecto. No toca servidores ni comandos.", "Resets all settings to their defaults. Servers and commands are not touched."),
    ("Botones, selección y resaltados de la app.", "Buttons, selection and highlights of the app."),
    ("Solo fuentes monoespaciadas.", "Monospaced fonts only."),
    ("Texto más grueso", "Bolder text"),
    ("Usa el peso Medium. Ayuda si el texto se ve fino sobre fondos oscuros.", "Uses the Medium weight. Helps if text looks thin on dark backgrounds."),
    ("Las URLs se ven subrayadas; ⌘+clic las abre. Con ⌘ pulsado también se subrayan las rutas de archivo que existen (solo en pestañas locales).", "URLs are underlined; ⌘+click opens them. With ⌘ held, existing file paths are underlined too (local tabs only)."),
    ("Necesario para tmux, vim o htop con ratón. Si lo desactivas, el mouse solo selecciona texto. Con Mayús pulsado siempre selecciona.", "Needed for tmux, vim or htop with the mouse. If you turn it off, the mouse only selects text. Holding Shift always selects."),
    ("Permite atajos como Option+← para saltar palabras en el shell. AltGr sigue escribiendo @ | #.", "Enables shortcuts like Option+← to jump words in the shell. AltGr still types @ | #."),
    ("Los cambios de historial se aplican a las pestañas nuevas.", "History changes apply to new tabs."),
    ("Se aplica a las conexiones nuevas.", "Applies to new connections."),
    ("Ejecutar al insertar (pulsa Retorno)", "Run on insert (presses Return)"),
    ("Aún no hay comandos guardados.", "No saved commands yet."),
    ("Sin resultados.", "No results."),
    ("Sin resultados", "No results"),
    ("+ Nuevo", "+ New"),
    ("Buscar por nombre o comando…", "Search by name or command…"),
    ("Pulsa ↻ para listar los buckets.", "Press ↻ to list the buckets."),
    ("Nombre de archivo no válido.", "Invalid file name."),
    ("Buckets ▸", "Buckets ▸"),
    ("⬆ Subir", "⬆ Upload"),
    ("⬇ Descargar", "⬇ Download"),
    ("+ Carpeta", "+ Folder"),
    ("Sin buckets cargados.", "No buckets loaded."),
    ("¿Eliminar de S3?", "Delete from S3?"),
    (" Las carpetas se borran con todo su contenido.", " Folders are deleted with all their contents."),
    ("Escribe la contraseña de sincronización.", "Enter the sync password."),
    ("Las contraseñas no coinciden.", "The passwords do not match."),
    ("Se elimina el archivo de sincronización y se desactiva la sincronización aquí. Tus datos en este Mac no se tocan.", "The sync file is deleted and syncing is turned off here. Your data on this Mac is not touched."),
    ("Repite la contraseña", "Repeat the password"),
    ("Contraseña", "Password"),
    ("Local (-L)", "Local (-L)"),
    ("Remoto (-R)", "Remote (-R)"),
    ("Servidor por el que pasa", "Server it goes through"),
    ("Túneles SSH", "SSH tunnels"),
    ("Túneles", "Tunnels"),
    ("Servidores", "Servers"),
    ("Guardados", "Saved"),
    ("Conectando…", "Connecting…"),
    ("Activo", "Active"),
    ("Detener", "Stop"),
    ("Aún no hay túneles.\n\nUn túnel conecta un puerto de tu Mac con un servicio al que solo llega el servidor (una base de datos, un panel interno…). Guarda uno con + y actívalo con un clic.", "There are no tunnels yet.\n\nA tunnel connects a port on your Mac to a service only the server can reach (a database, an internal panel…). Save one with + and turn it on with a click."),
    ("Pestaña abierta", "Open tab"),
    ("Pestaña", "Tab"),
    ("Servidor", "Server"),
    ("Comando", "Command"),
    ("Acción", "Action"),
    ("Ajustes", "Settings"),
    ("Cerrar", "Close"),
    ("Cancelar", "Cancel"),
    ("Guardar", "Save"),
    ("Eliminar", "Delete"),
    ("Crear", "Create"),
    ("Renombrar", "Rename"),
    ("Nombre", "Name"),
    ("Nueva carpeta", "New folder"),
    ("Ejecutar de todos modos", "Run anyway"),
    ("Comando delicado en PRODUCCIÓN", "Sensitive command in PRODUCTION"),
    ("Actualizar ahora", "Update now"),
    ("Más tarde", "Later"),
    ("Actualizando jTerm", "Updating jTerm"),
    ("jTerm está al día", "jTerm is up to date"),
    ("No se pudo actualizar", "Could not update"),
    ("Terminal local", "Local terminal"),
    ("Subiendo", "Uploading"),
    ("Descargando", "Downloading"),
    ("Completado", "Completed"),
    ("Iniciando…", "Starting…"),
    ("Cancelado", "Cancelled"),
    ("justo ahora", "just now"),
    ("Todavía no se ha sincronizado.", "Not synced yet."),
    ("Sincronizando…", "Syncing…"),
    ("Sincronizar ahora", "Sync now"),
    ("Hay una versión nueva", "A new version is available"),
    ("Este Mac", "This Mac"),
    ("Buscando…", "Searching…"),
    ("Buscar", "Search"),
    ("Seguir", "Follow"),
    ("Formateado", "Formatted"),
    ("Texto", "Text"),
    ("Agrupar", "Group"),
    ("Copiar", "Copy"),
    ("Limpiar", "Clear"),
    ("Región", "Region"),
    ("Rango", "Range"),
    ("Stream", "Stream"),
    ("Cuenta", "Account"),
    ("Explorar S3", "Explore S3"),
    ("Recargar", "Reload"),
    ("Sin cuentas", "No accounts"),
    ("Todas las regiones", "All regions"),
    ("Conectado", "Connected"),
    ("Cierra y vuelve a abrir jTerm para aplicar el nuevo idioma.", "Close and reopen jTerm to apply the new language."),
    ("Reiniciar servicio", "Restart service"),
    ("Producción web", "Production web"),
    ("DIRECTORIO INICIAL (OPCIONAL)", "STARTING DIRECTORY (OPTIONAL)"),
    ("Carpeta donde abre la sesión SSH y el gestor de archivos.", "Folder where the SSH session and the file manager open."),
];

/// Plantillas para mensajes con valores: (expresión regular en español, texto en inglés con `$1`, `$2`…).
/// `$tN` traduce además el valor capturado (cuando es un texto de la interfaz y no un dato del usuario).
const PATTERNS: &[(&str, &str)] = &[
    (r"^Pestaña (\d+)$", "Tab $1"),
    (r"^No se pudo abrir el terminal: (.+)$", "Could not open the terminal: $1"),
    (r"^No se pudo preparar (.+): (.+)$", "Could not prepare $1: $2"),
    (r"^No se pudo guardar (.+): (.+)$", "Could not save $1: $2"),
    (r"^No se pudo guardar: (.+)$", "Could not save: $1"),
    (r"^(.+) · (\d+) región\(es\) con error$", "$t1 · $2 region(s) with errors"),
    (r"^Todos (\d+)$", "All $1"),
    (r"^Buscando… (\d+) eventos$", "Searching… $1 events"),
    (r"^Siguiendo · (\d+) eventos \(\+(\d+)\) · (.+)$", "Following · $1 events (+$2) · $3"),
    (r"^Copiado: (\d+) eventos\.$", "Copied: $1 events."),
    (r"^(\d+) eventos en (.+) \((.+)\)\.(.*)$", "$1 events in $2 ($3).$t4"),
    (r" Hay más eventos: acota el rango o el filtro para verlos\.", " There are more events: narrow the range or the filter to see them."),
    (r"^(\d+) grupos de logs en (.+)\. Elige uno y pulsa Buscar\.$", "$1 log groups in $2. Choose one and press Search."),
    (r"^Retención: (\d+) días$", "Retention: $1 days"),
    (r"^(\d+) líneas$", "$1 lines"),
    (r"^Subida de (\d+) archivos$", "Uploading $1 files"),
    (r"^Subida de (.+)$", "Upload of $1"),
    (r"^Descarga de (.+)$", "Download of $1"),
    (r"^Subiendo (.+)$", "Uploading $1"),
    (r"^Descargando (.+)$", "Downloading $1"),
    (r"^La instancia no está en ejecución \(estado: (.+)\)\.$", "The instance is not running (state: $1)."),
    (r"^Error: (.+)$", "Error: $t1"),
    (r"^Última sincronización: (.+)$", "Last sync: $t1"),
    (r"^hace (\d+) s$", "$1 s ago"),
    (r"^hace (\d+) min$", "$1 min ago"),
    (r"^hace (\d+) h$", "$1 h ago"),
    (r"^hace (\d+) días$", "$1 days ago"),
    (r"^Pestaña abierta · (\d+) paneles$", "Open tab · $1 panes"),
    (r"^Usa al menos (\d+) caracteres\.$", "Use at least $1 characters."),
    (r"^Ya existe “(.+)”\. Elige otro nombre\.$", "“$1” already exists. Choose another name."),
    (r"^Renombrar “(.+)”$", "Rename “$1”"),
    (r"^Se eliminará (.+) del bucket\. No se puede deshacer\.(.*)$", "$t1 will be deleted from the bucket. This cannot be undone.$t2"),
    (r"^(\d+) objeto\(s\) eliminados\.$", "$1 object(s) deleted."),
    (r"^(.+) · (\d+) elementos$", "$1 · $2 items"),
    (r"^(\d+) elementos?$", "$1 items"),
    (r"^(\d+) buckets?$", "$1 buckets"),
    (r"^(\d+) instancias?$", "$1 instances"),
    (r"^Guardado en (.+)\.$", "Saved to $1."),
    (r"^Hay una versión nueva: jTerm (.+)$", "A new version is available: jTerm $1"),
    (r"^Tienes la (.+)\.$", "You have $1."),
    (r"^Tienes la versión (.+), que es la más reciente\.$", "You have version $1, which is the latest."),
    (r"^Usuario para (.+)$", "User for $1"),
    (r"^Conectando a (.+)…$", "Connecting to $1…"),
    (r"^Archivos · (.+)$", "Files · $1"),
    (r"^Logs · (.+)$", "Logs · $1"),
    (r"^S3 · (.+)$", "S3 · $1"),
    (r"^(\d+) pt$", "$1 pt"),
    (r"^(\d+) s$", "$1 s"),
    (r"^¿Eliminar “(.+)”\?$", "Delete “$1”?"),
];

type Compiled = Vec<(Regex, String)>;

fn exact() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m: HashMap<&str, &str> = SWIFT.iter().copied().collect();
        // Lo de Rust manda sobre lo de Swift cuando coincide la clave.
        m.extend(EXTRA.iter().copied());
        m
    })
}

fn patterns() -> &'static Compiled {
    static P: OnceLock<Compiled> = OnceLock::new();
    P.get_or_init(|| {
        let mut v: Compiled = PATTERNS.iter().filter_map(|(re, en)| Regex::new(re).ok().map(|r| (r, en.to_string()))).collect();
        // Los textos de Swift con %d / %@ también valen como plantillas.
        for (es, en) in SWIFT.iter().filter(|(k, _)| k.contains('%')) {
            let mut regex = String::from("^");
            let mut rest = *es;
            while let Some(i) = rest.find('%') {
                regex.push_str(&regex::escape(&rest[..i]));
                match rest.as_bytes().get(i + 1) {
                    Some(b'd') => regex.push_str(r"(\d+)"),
                    Some(b'@') => regex.push_str("(.+)"),
                    _ => regex.push_str(&regex::escape(&rest[i..i + 1])),
                }
                rest = &rest[(i + 2).min(rest.len())..];
            }
            regex.push_str(&regex::escape(rest));
            regex.push('$');
            let (mut template, mut n, mut tail) = (String::new(), 0, *en);
            while let Some(i) = tail.find('%') {
                template.push_str(&tail[..i].replace('$', "$$"));
                if matches!(tail.as_bytes().get(i + 1), Some(b'd') | Some(b'@')) {
                    n += 1;
                    template.push_str(&format!("${{{n}}}"));
                    tail = &tail[i + 2..];
                } else {
                    template.push('%');
                    tail = &tail[i + 1..];
                }
            }
            template.push_str(&tail.replace('$', "$$"));
            if let Ok(r) = Regex::new(&regex) {
                v.push((r, template));
            }
        }
        v
    })
}

/// Traduce `s` según el idioma activo. Lo que no tiene traducción se devuelve igual (los datos del usuario, por ejemplo).
pub fn tr(s: &str) -> String {
    if !is_english() || s.is_empty() {
        return s.to_string();
    }
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap().get(s) {
        return hit.clone();
    }
    let out = translate(s, 0);
    let mut c = cache.lock().unwrap();
    if c.len() > 4000 {
        c.clear();
    }
    c.insert(s.to_string(), out.clone());
    out
}

fn translate(s: &str, depth: u8) -> String {
    if let Some(en) = exact().get(s) {
        return en.to_string();
    }
    for (re, template) in patterns() {
        if let Some(c) = re.captures(s) {
            // `$tN` → traducción del grupo N.
            let mut tpl = template.clone();
            if depth < 3 {
                for n in 1..c.len() {
                    let marker = format!("$t{n}");
                    if tpl.contains(&marker) {
                        let inner = c.get(n).map(|m| translate(m.as_str(), depth + 1)).unwrap_or_default();
                        tpl = tpl.replace(&marker, &inner.replace('$', "$$"));
                    }
                }
            }
            let mut out = String::new();
            c.expand(&tpl, &mut out);
            return out;
        }
    }
    s.to_string()
}

/// Como `iced::widget::text`, pero traduce el texto (para etiquetas escritas en el código, nunca para datos del usuario).
pub fn text<'a>(s: impl AsRef<str>) -> iced::widget::Text<'a> {
    iced::widget::text(tr(s.as_ref()))
}

/// Valor mostrado en un desplegable con el texto traducido (el valor interno sigue siendo el original).
#[derive(Clone, PartialEq)]
pub struct Tr<T>(pub T);

impl<T: ToString> std::fmt::Display for Tr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr(&self.0.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en<R>(f: impl FnOnce() -> R) -> R {
        // El idioma es global: las pruebas que lo usan van en un solo test para no pisarse.
        set_english(true);
        let r = f();
        set_english(false);
        r
    }

    #[test]
    fn language_resolution() {
        let es = vec!["es-CL".to_string(), "en-US".to_string()];
        let us = vec!["en-US".to_string()];
        assert!(!resolve("system", &es) && resolve("system", &us));
        assert!(resolve("en", &es) && !resolve("es", &us));
        assert!(!resolve("system", &[]), "sin información del sistema, español");
    }

    #[test]
    fn translations_patterns_and_fallbacks() {
        // Con el idioma en español, nada cambia.
        set_english(false);
        assert_eq!(tr("Ajustes"), "Ajustes");
        en(|| {
            assert_eq!(tr("Ajustes"), "Settings");
            assert_eq!(tr("Más tarde"), "Later");
            assert_eq!(tr("Nombre que nadie tradujo"), "Nombre que nadie tradujo", "lo desconocido queda igual");
            assert_eq!(tr(""), "");
            // Plantillas de Rust y de Swift (%d, %@).
            assert_eq!(tr("Buscando… 120 eventos"), "Searching… 120 events");
            assert_eq!(tr("Siguiendo · 5 eventos (+2) · 10:30:00"), "Following · 5 events (+2) · 10:30:00");
            assert_eq!(tr("12 instancias"), "12 instances");
            assert_eq!(tr("Retención: 7 días"), "Retention: 7 days");
            // Plantilla con parte traducible dentro.
            assert_eq!(tr("Última sincronización: hace 5 min"), "Last sync: 5 min ago");
            assert_eq!(tr("Última sincronización: justo ahora"), "Last sync: just now");
            assert_eq!(tr("Error: Todavía no se ha sincronizado."), "Error: Not synced yet.");
            // Los datos del usuario no se traducen aunque lleven símbolos especiales.
            assert_eq!(tr("Renombrar “Mi $5 servidor”"), "Rename “Mi $5 servidor”");
            assert_eq!(tr("Archivos · Producción"), "Files · Producción");
            assert_eq!(Tr("Todas las regiones".to_string()).to_string(), "All regions");
            assert_eq!(tr("No se pudo guardar servers.json: permiso denegado"), "Could not save servers.json: permiso denegado");
            assert_eq!(tr("3 instancias · 2 región(es) con error"), "3 instances · 2 region(s) with errors");
            assert_eq!(tr("Todos 12"), "All 12");
        });
    }

    #[test]
    fn every_pattern_compiles_and_swift_table_is_loaded() {
        assert!(patterns().len() >= PATTERNS.len(), "algún patrón no compiló");
        assert!(exact().len() > 700);
        // Ningún texto de EXTRA o de Swift queda con marcadores sin resolver.
        for (_, v) in exact() {
            assert!(!v.contains("$t"), "{v}");
        }
    }
}
