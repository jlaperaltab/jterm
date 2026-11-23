//! Sincronización con iCloud Drive: preferencias, estado visible y diálogos.
//! La sincronización real corre en segundo plano (ver `App::sync_now`); aquí solo hay datos y vistas.

use crate::colors::Palette;
use crate::editor::{action_button, dialog};
use crate::ui::input as text_input;
use crate::i18n::tr;
use crate::Message;
use chrono::{DateTime, Utc};
use iced::widget::{column, row, space, text};
use iced::{Color, Element};
use jterm_core::sync::{self, kind};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;
use uuid::Uuid;

/// Mismo elemento del Keychain que usa la versión Swift para la contraseña de sincronización.
pub const PASSPHRASE_ID: Uuid = Uuid::from_u128(0x5E7C0000_0000_4000_8000_00000000A5C1);
pub const MIN_PASSPHRASE: usize = 8;
pub const PERIODIC_SECS: u64 = 120;
pub const DEBOUNCE_SECS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncPrefs {
    pub enabled: bool,
    pub servers: bool,
    pub aws: bool,
    pub commands: bool,
    pub settings: bool,
    pub secrets: bool,
}

impl Default for SyncPrefs {
    fn default() -> Self {
        SyncPrefs { enabled: false, servers: true, aws: true, commands: true, settings: true, secrets: false }
    }
}

impl SyncPrefs {
    pub fn kinds(&self) -> HashSet<String> {
        let mut s = HashSet::new();
        for (on, k) in [(self.servers, kind::SERVER), (self.aws, kind::AWS), (self.commands, kind::COMMAND), (self.settings, kind::SETTINGS)] {
            if on {
                s.insert(k.to_string());
            }
        }
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFlag {
    Servers,
    Aws,
    Commands,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// Activar: `has_data` = ya hay un archivo en iCloud (se pide su contraseña en vez de crear una).
    Enable { has_data: bool },
    Reenter,
    Change,
    ConfirmDisable,
    ConfirmSecrets,
    ConfirmDelete,
}

pub struct SyncDialog {
    pub kind: DialogKind,
    pub pass1: String,
    pub pass2: String,
    pub error: Option<String>,
}

impl SyncDialog {
    pub fn new(kind: DialogKind) -> Self {
        SyncDialog { kind, pass1: String::new(), pass2: String::new(), error: None }
    }

    pub fn asks_passphrase(&self) -> bool {
        matches!(self.kind, DialogKind::Enable { .. } | DialogKind::Reenter | DialogKind::Change)
    }

    /// ¿Se crea una contraseña nueva (hay que repetirla y debe ser larga)?
    pub fn creates_passphrase(&self) -> bool {
        matches!(self.kind, DialogKind::Enable { has_data: false } | DialogKind::Change)
    }

    /// Valida lo escrito; `Err` con el motivo.
    pub fn check(&self) -> Result<(), String> {
        if !self.asks_passphrase() {
            return Ok(());
        }
        if self.pass1.is_empty() {
            return Err("Escribe la contraseña de sincronización.".into());
        }
        if self.creates_passphrase() {
            if self.pass1.chars().count() < MIN_PASSPHRASE {
                return Err(format!("Usa al menos {MIN_PASSPHRASE} caracteres."));
            }
            if self.pass1 != self.pass2 {
                return Err("Las contraseñas no coinciden.".into());
            }
        }
        Ok(())
    }
}

/// Estado de la sincronización en la aplicación.
pub struct SyncCtl {
    pub prefs: SyncPrefs,
    /// Se lee del Keychain la primera vez que hace falta.
    pub passphrase: Option<String>,
    pub pass_loaded: bool,
    pub syncing: bool,
    pub pending_rerun: bool,
    pub last_sync: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub needs_pass: bool,
    pub due: Option<Instant>,
    pub last_periodic: Instant,
    pub dialog: Option<SyncDialog>,
}

impl SyncCtl {
    pub fn new(prefs: SyncPrefs) -> Self {
        SyncCtl {
            prefs, passphrase: None, pass_loaded: false, syncing: false, pending_rerun: false, last_sync: None, last_error: None, needs_pass: false,
            due: None, last_periodic: Instant::now(), dialog: None,
        }
    }

    /// Pide una sincronización pronto (un cambio local la dispara tras unos segundos).
    pub fn touch(&mut self) {
        if self.prefs.enabled && self.due.is_none() {
            self.due = Some(Instant::now() + std::time::Duration::from_secs(DEBOUNCE_SECS));
        }
    }

    pub fn view_model(&self, folder_ok: bool, now: DateTime<Utc>) -> SyncView {
        let status = if self.syncing {
            "Sincronizando…".to_string()
        } else if let Some(e) = &self.last_error {
            format!("Error: {e}")
        } else if let Some(l) = self.last_sync {
            format!("Última sincronización: {}", relative(now, l))
        } else {
            "Todavía no se ha sincronizado.".to_string()
        };
        SyncView { folder_ok, prefs: self.prefs, status, syncing: self.syncing, needs_pass: self.needs_pass, missing_pass: self.passphrase.is_none() }
    }
}

/// Lo que la página de ajustes necesita saber.
#[derive(Debug, Clone)]
pub struct SyncView {
    pub folder_ok: bool,
    pub prefs: SyncPrefs,
    pub status: String,
    pub syncing: bool,
    pub needs_pass: bool,
    pub missing_pass: bool,
}

/// «justo ahora», «hace 5 min»…
pub fn relative(now: DateTime<Utc>, then: DateTime<Utc>) -> String {
    let s = (now - then).num_seconds().max(0);
    match s {
        0..=9 => "justo ahora".into(),
        10..=59 => format!("hace {s} s"),
        60..=3599 => format!("hace {} min", s / 60),
        3600..=86_399 => format!("hace {} h", s / 3600),
        _ => format!("hace {} días", s / 86_400),
    }
}

/// Carpeta de sincronización: la de iCloud Drive, o la de `JTERM_SYNC_FOLDER` (para probar sin tocar iCloud).
pub fn sync_folder() -> Option<PathBuf> {
    std::env::var_os("JTERM_SYNC_FOLDER").map(PathBuf::from).or_else(sync::icloud_folder)
}

pub fn dialog_view<'a>(d: &'a SyncDialog, p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
    let red = Color::from_rgb8(0xff, 0x45, 0x3b);
    let (title, body, ok, danger): (&str, &str, &str, bool) = match d.kind {
        DialogKind::Enable { has_data: true } => ("Contraseña de sincronización", "Ya hay datos de jTerm en tu iCloud. Ingresa la contraseña de sincronización con la que se crearon.", "Activar", false),
        DialogKind::Enable { has_data: false } => ("Crea una contraseña de sincronización", "Cifra tus datos en iCloud. La necesitarás para activar la sincronización en tus otros Mac y no se puede recuperar.", "Activar", false),
        DialogKind::Reenter => ("Contraseña de sincronización", "Ingresa la contraseña con la que se cifraron los datos en iCloud.", "Continuar", false),
        DialogKind::Change => ("Nueva contraseña de sincronización", "Se volverá a cifrar el archivo de iCloud. Tendrás que ingresarla en tus otros Mac.", "Cambiar", false),
        DialogKind::ConfirmDisable => ("¿Dejar de sincronizar con iCloud?", "Tus datos en este Mac se conservan y los del iCloud también. Para volver a sincronizar tendrás que ingresar la contraseña de sincronización.", "Dejar de sincronizar", true),
        DialogKind::ConfirmSecrets => ("¿Sincronizar también contraseñas y claves?", "Se guardarán en tu iCloud cifradas con tu contraseña de sincronización. Si la olvidas, no hay forma de recuperarlas desde iCloud.", "Sincronizar", false),
        DialogKind::ConfirmDelete => ("¿Borrar los datos de iCloud?", "Se elimina el archivo de sincronización y se desactiva la sincronización aquí. Tus datos en este Mac no se tocan.", "Borrar", true),
    };
    let mut col = column![text(tr(title)).size(16).color(if danger { red } else { fg }), text(tr(body)).size(12).color(dim)].spacing(10);
    if d.asks_passphrase() {
        col = col.push(text_input("Contraseña", &d.pass1).secure(true).on_input(Message::SyncPass1).on_submit(Message::SyncDialogOk));
        if d.creates_passphrase() {
            col = col.push(text_input("Repite la contraseña", &d.pass2).secure(true).on_input(Message::SyncPass2).on_submit(Message::SyncDialogOk));
        }
    }
    if let Some(e) = &d.error {
        col = col.push(text(tr(e)).size(12).color(red));
    }
    col = col.push(
        row![
            space::horizontal(),
            action_button("Cancelar", fg, fg, false).on_press(Message::SyncDialogCancel),
            action_button(ok, if danger { red } else { accent }, fg, true).on_press(Message::SyncDialogOk),
        ]
        .spacing(8),
    );
    dialog(col.into(), p, 460.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn prefs_defaults_and_kinds() {
        let p = SyncPrefs::default();
        assert!(!p.enabled && !p.secrets);
        assert_eq!(p.kinds().len(), 4);
        let q = SyncPrefs { aws: false, settings: false, ..p };
        let k = q.kinds();
        assert!(k.contains("server") && k.contains("cmd") && !k.contains("aws") && !k.contains("settings"));
        // Un archivo parcial completa lo que falta con los valores por defecto.
        let r: SyncPrefs = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(r.enabled && r.servers);
    }

    #[test]
    fn passphrase_rules() {
        let mut d = SyncDialog::new(DialogKind::Enable { has_data: false });
        assert!(d.check().is_err());
        d.pass1 = "corta".into();
        d.pass2 = "corta".into();
        assert!(d.check().unwrap_err().contains("al menos"));
        d.pass1 = "una clave larga".into();
        assert!(d.check().unwrap_err().contains("no coinciden"));
        d.pass2 = d.pass1.clone();
        assert!(d.check().is_ok());
        // Con datos existentes no se exige longitud ni repetición: solo debe coincidir con la del archivo.
        let mut e = SyncDialog::new(DialogKind::Enable { has_data: true });
        e.pass1 = "x".into();
        assert!(e.check().is_ok());
        assert!(SyncDialog::new(DialogKind::ConfirmDelete).check().is_ok());
    }

    #[test]
    fn relative_times() {
        let now = Utc.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(relative(now, now - Duration::seconds(3)), "justo ahora");
        assert_eq!(relative(now, now - Duration::seconds(30)), "hace 30 s");
        assert_eq!(relative(now, now - Duration::minutes(5)), "hace 5 min");
        assert_eq!(relative(now, now - Duration::hours(3)), "hace 3 h");
        assert_eq!(relative(now, now - Duration::days(2)), "hace 2 días");
    }

    #[test]
    fn touch_only_schedules_when_enabled_and_status_text() {
        let mut c = SyncCtl::new(SyncPrefs::default());
        c.touch();
        assert!(c.due.is_none());
        c.prefs.enabled = true;
        c.touch();
        assert!(c.due.is_some());
        let now = Utc::now();
        assert_eq!(c.view_model(true, now).status, "Todavía no se ha sincronizado.");
        c.last_sync = Some(now);
        assert!(c.view_model(true, now).status.contains("justo ahora"));
        c.last_error = Some("boom".into());
        assert_eq!(c.view_model(true, now).status, "Error: boom");
        c.syncing = true;
        assert_eq!(c.view_model(true, now).status, "Sincronizando…");
    }

    #[test]
    fn passphrase_id_matches_swift() {
        assert_eq!(PASSPHRASE_ID.to_string().to_uppercase(), "5E7C0000-0000-4000-8000-00000000A5C1");
    }
}
