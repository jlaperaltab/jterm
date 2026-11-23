//! Diálogos del actualizador.

use crate::colors::Palette;
use crate::editor::{action_button, dialog};
use crate::i18n::tr;
use crate::Message;
use iced::widget::{column, row, space, text};
use iced::{Color, Element};
use jterm_net::updater::Release;
use std::sync::{Arc, Mutex};

pub enum UpdateDialog {
    /// Hay una versión nueva.
    Found { rel: Release, current: String },
    /// Aviso simple (al día, error…).
    Info { title: String, text: String },
    /// Se cambió el idioma: ofrece reiniciar.
    Relaunch,
    /// Descargando e instalando: el texto lo actualiza la tarea.
    Installing { progress: Arc<Mutex<String>> },
}

/// Segundos entre comprobaciones silenciosas al abrir la app.
pub const CHECK_EVERY_SECS: u64 = 24 * 3600;

/// ¿Toca comprobar al abrir? (`last` = instante Unix de la última comprobación; 0 = nunca)
pub fn due_for_launch_check(now_secs: u64, last: u64) -> bool {
    now_secs.saturating_sub(last) > CHECK_EVERY_SECS
}

pub fn view<'a>(d: &'a UpdateDialog, p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
    match d {
        UpdateDialog::Found { rel, current } => dialog(
            column![
                text(tr(&format!("Hay una versión nueva: jTerm {}", rel.version))).size(16).color(fg),
                text(tr(&format!("Tienes la {current}."))).size(13).color(dim),
                row![space::horizontal(), action_button("Más tarde", fg, fg, false).on_press(Message::UpdateLater), action_button("Actualizar ahora", accent, fg, true).on_press(Message::UpdateInstall)].spacing(8),
            ]
            .spacing(12)
            .into(),
            p,
            440.0,
        ),
        UpdateDialog::Info { title, text: body } => dialog(
            column![
                text(tr(title)).size(16).color(fg),
                text(tr(body)).size(13).color(dim),
                row![space::horizontal(), action_button("OK", accent, fg, true).on_press(Message::UpdateLater)],
            ]
            .spacing(12)
            .into(),
            p,
            440.0,
        ),
        UpdateDialog::Relaunch => dialog(
            column![
                text(tr("Idioma cambiado")).size(16).color(fg),
                text(tr("Reinicia jTerm para aplicar el nuevo idioma.")).size(13).color(dim),
                row![space::horizontal(), action_button("Más tarde", fg, fg, false).on_press(Message::UpdateLater), action_button("Reiniciar ahora", accent, fg, true).on_press(Message::Relaunch)].spacing(8),
            ]
            .spacing(12)
            .into(),
            p,
            420.0,
        ),
        UpdateDialog::Installing { progress } => dialog(
            column![
                text(tr("Actualizando jTerm")).size(16).color(fg),
                text(tr(&progress.lock().unwrap())).size(13).color(dim),
                iced::widget::progress_bar(0.0..=1.0, 0.0).girth(4),
            ]
            .spacing(12)
            .into(),
            p,
            380.0,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_check_is_at_most_daily() {
        assert!(due_for_launch_check(1_000_000, 0));
        assert!(!due_for_launch_check(1_000_000, 1_000_000 - 3600));
        assert!(due_for_launch_check(1_000_000, 1_000_000 - CHECK_EVERY_SECS - 1));
        assert!(!due_for_launch_check(10, 100), "un reloj atrasado no provoca comprobaciones");
    }
}
