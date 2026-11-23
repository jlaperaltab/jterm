//! Gestor de archivos de dos paneles: este Mac ↔ servidor (SFTP sobre una conexión SSH maestra).
//! Las operaciones remotas solo se lanzan desde los botones de la vista.

use crate::colors::Palette;
use crate::editor::{action_button, dialog};
use crate::s3_ui::human_size;
use crate::ui::{self, caption, input as text_input};
use crate::i18n::tr;
use crate::Message;
use chrono::{DateTime, Local};
use iced::widget::{column, container, row, scrollable, space, text, Column};
use iced::{Background, Border, Color, Element, Length};
use jterm_core::sftp::{RemoteEntry, SftpSession, TransferJob};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneId {
    Local,
    Remote,
}

/// Sesión SFTP compartida (con `Debug` para poder viajar en los mensajes).
#[derive(Clone)]
pub struct Session(pub Arc<SftpSession>);

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    pub modified: String,
}

/// Carpetas primero y luego archivos, cada grupo por nombre sin distinguir mayúsculas.
pub fn sort_entries(v: &mut [Entry]) {
    v.sort_by_key(|e| (!e.is_dir, e.name.to_lowercase()));
}

pub fn list_local(dir: &Path, show_hidden: bool) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let meta = e.metadata();
        let is_link = e.file_type().map(|t| t.is_symlink()).unwrap_or(false);
        // Un enlace a una carpeta se trata como carpeta.
        let is_dir = if is_link { std::fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false) } else { meta.as_ref().map(|m| m.is_dir()).unwrap_or(false) };
        let modified = meta.as_ref().ok().and_then(|m| m.modified().ok()).map(|t| DateTime::<Local>::from(t).format("%d %b %Y %H:%M").to_string()).unwrap_or_default();
        out.push(Entry { name, is_dir, is_link, size: meta.map(|m| m.len()).unwrap_or(0), modified });
    }
    sort_entries(&mut out);
    Ok(out)
}

pub fn remote_entries(v: Vec<RemoteEntry>, show_hidden: bool) -> Vec<Entry> {
    let mut out: Vec<Entry> = v
        .into_iter()
        .filter(|e| show_hidden || !e.name.starts_with('.'))
        .map(|e| Entry { name: e.name, is_dir: e.is_dir, is_link: e.is_link, size: e.size, modified: e.modified })
        .collect();
    sort_entries(&mut out);
    out
}

pub fn join_remote(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") }
}

pub fn parent_remote(dir: &str) -> String {
    let t = dir.trim_end_matches('/');
    match t.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => t[..i].to_string(),
    }
}

/// Carpeta con la que abre el panel remoto: la inicial del servidor (resolviendo `~`) o, si no hay, la personal.
pub fn resolve_remote_start(home: &str, start: Option<&str>) -> String {
    match start.map(str::trim).filter(|d| !d.is_empty()) {
        None | Some("~") => home.to_string(),
        Some(d) => match d.strip_prefix("~/") {
            Some(rest) => join_remote(home, rest),
            None if d.starts_with('/') => d.to_string(),
            None => join_remote(home, d),
        },
    }
}

pub fn valid_name(n: &str) -> bool {
    let t = n.trim();
    !t.is_empty() && !t.contains('/') && t != "." && t != ".."
}

pub struct Pane {
    pub path: String,
    pub entries: Vec<Entry>,
    pub selected: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub hidden: bool,
    pub generation: u64,
}

impl Pane {
    pub fn new(path: String, hidden: bool) -> Self {
        Pane { path, entries: vec![], selected: None, loading: false, error: None, hidden, generation: 0 }
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.selected.as_ref().and_then(|n| self.entries.iter().find(|e| &e.name == n))
    }
}

pub struct FileTransfer {
    pub id: u64,
    pub title: String,
    pub percent: Arc<AtomicU32>,
    pub label: Arc<Mutex<String>>,
    pub job: TransferJob,
    /// Panel que se recarga al terminar (el de destino).
    pub refresh: PaneId,
    pub finished: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameKind {
    NewFolder,
    Rename(String),
}

pub struct NameDialog {
    pub pane: PaneId,
    pub kind: NameKind,
    pub text: String,
}

pub struct FilesState {
    pub title: String,
    pub session: Option<Session>,
    pub connecting: bool,
    pub local: Pane,
    pub remote: Pane,
    pub transfers: Vec<FileTransfer>,
    pub next_transfer: u64,
    pub status: String,
    pub is_error: bool,
    pub confirm_delete: Option<(PaneId, Entry)>,
    pub name_dialog: Option<NameDialog>,
    /// Directorio inicial del servidor (si tiene uno): ahí abre el panel remoto.
    pub remote_start: Option<String>,
}

impl FilesState {
    pub fn new(title: String, local_start: PathBuf, hidden_local: bool, hidden_remote: bool) -> Self {
        FilesState {
            title, session: None, connecting: true, local: Pane::new(local_start.display().to_string(), hidden_local), remote: Pane::new("/".into(), hidden_remote),
            transfers: vec![], next_transfer: 1, status: "Conectando…".into(), is_error: false, confirm_delete: None, name_dialog: None, remote_start: None,
        }
    }

    pub fn pane(&self, p: PaneId) -> &Pane {
        if p == PaneId::Local { &self.local } else { &self.remote }
    }

    pub fn pane_mut(&mut self, p: PaneId) -> &mut Pane {
        if p == PaneId::Local { &mut self.local } else { &mut self.remote }
    }

    pub fn new_transfer(&mut self, title: String, refresh: PaneId) -> (u64, TransferJob, Arc<AtomicU32>, Arc<Mutex<String>>) {
        let id = self.next_transfer;
        self.next_transfer += 1;
        let (job, percent, label) = (TransferJob::default(), Arc::new(AtomicU32::new(0)), Arc::new(Mutex::new("Iniciando…".to_string())));
        self.transfers.push(FileTransfer { id, title, percent: percent.clone(), label: label.clone(), job: job.clone(), refresh, finished: false, error: None });
        (id, job, percent, label)
    }
}

// MARK: Vista

pub fn view<'a>(st: &'a FilesState, p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim, bg) = (p.fg, Color { a: 0.55, ..p.fg }, p.bg);
    let can_up = st.local.selected_entry().is_some() && st.session.is_some();
    let can_down = st.remote.selected_entry().is_some();

    let header = row![
        ui::icon(ui::Icon::FolderOpen, 15.0, accent),
        text(tr(&format!("Archivos · {}", st.title))).size(13).color(fg),
        space::horizontal(),
        action_button("✕ Cerrar", fg, fg, false).on_press(Message::FilesClose),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let middle = column![
        ui::compact("→", fg, fg).on_press_maybe(can_up.then_some(Message::FilesUpload)),
        ui::compact("←", fg, fg).on_press_maybe(can_down.then_some(Message::FilesDownload)),
    ]
    .spacing(8)
    .align_x(iced::Alignment::Center);

    let panes = row![
        pane_view(st, PaneId::Local, p, accent),
        container(middle).center_y(Length::Fill).padding([0, 2]),
        pane_view(st, PaneId::Remote, p, accent)
    ]
    .spacing(8)
    .height(Length::Fill);

    let mut transfers: Column<Message> = Column::new().spacing(4);
    for t in st.transfers.iter().rev().take(4) {
        let pct = if t.finished { 100 } else { t.percent.load(Ordering::Relaxed) };
        let detail = match &t.error {
            Some(e) => e.clone(),
            None if t.finished => "Completado".into(),
            None => format!("{pct}% · {}", t.label.lock().unwrap()),
        };
        let color = if t.error.is_some() { Color::from_rgb8(0xff, 0x6b, 0x6b) } else { dim };
        let bar = iced::widget::progress_bar(0.0..=100.0, pct as f32).girth(4).length(Length::Fill);
        let action = if t.finished || t.error.is_some() { ("×", Message::FilesDismiss(t.id)) } else { ("×", Message::FilesCancel(t.id)) };
        transfers = transfers.push(
            row![
                column![text(tr(&t.title)).size(12).color(fg), bar, text(tr(&detail)).size(11).color(color)].spacing(3).width(Length::Fill),
                ui::icon_button(action.0, dim, fg).on_press(action.1)
            ]
            .align_y(iced::Alignment::Center),
        );
    }
    let status = text(tr(&st.status)).size(11).color(if st.is_error { Color::from_rgb8(0xff, 0x6b, 0x6b) } else { dim });
    container(column![header, panes, transfers, status].spacing(10).padding(12))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style { background: Some(Background::Color(bg)), ..Default::default() })
        .into()
}

fn pane_view<'a>(st: &'a FilesState, id: PaneId, p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
    let pane = st.pane(id);
    let ready = id == PaneId::Local || st.session.is_some();
    let sel = pane.selected_entry();
    let tools = row![
        ui::icon_button("←", dim, fg).on_press_maybe(ready.then_some(Message::FilesUp(id))),
        text(pane.path.clone()).size(12).font(crate::term_view::FONT).color(fg).width(Length::Fill).wrapping(iced::widget::text::Wrapping::None),
        ui::icon_button(if pane.hidden { "▾" } else { "▸" }, dim, fg).on_press(Message::FilesHidden(id)),
        ui::icon_button("↻", dim, fg).on_press_maybe(ready.then_some(Message::FilesRefresh(id))),
        ui::icon_button("+", dim, fg).on_press_maybe(ready.then_some(Message::FilesNewFolder(id))),
        ui::icon_button("✎", dim, fg).on_press_maybe(sel.map(|_| Message::FilesRename(id))),
        ui::icon_button("🗑", dim, fg).on_press_maybe(sel.map(|_| Message::FilesDelete(id))),
    ]
    .spacing(2)
    .align_y(iced::Alignment::Center);

    let mut list: Column<Message> = Column::new().spacing(1);
    for e in &pane.entries {
        let selected = pane.selected.as_deref() == Some(e.name.as_str());
        let size = if e.is_dir { String::new() } else { human_size(e.size) };
        let line = row![
            ui::label(if e.is_dir { "📁" } else { "📄" }, 13.0, if e.is_dir { accent } else { dim }),
            text(e.name.clone()).size(13).color(fg).width(Length::Fill).wrapping(iced::widget::text::Wrapping::None),
            text(size).size(11).color(dim).width(Length::Fixed(64.0)),
            text(e.modified.clone()).size(11).color(dim).width(Length::Fixed(110.0)).wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let open = e.is_dir.then(|| Message::FilesEnter(id, e.name.clone()));
        list = list.push(ui::selectable_row(line, selected, fg, Message::FilesSelect(id, Some(e.name.clone())), open));
    }
    let note = if pane.loading {
        Some("Cargando…".to_string())
    } else if let Some(e) = &pane.error {
        Some(e.clone())
    } else if !ready {
        Some(if st.connecting { "Conectando…".to_string() } else { "Sin conexión.".to_string() })
    } else if pane.entries.is_empty() {
        Some("Carpeta vacía.".to_string())
    } else {
        None
    };
    if let Some(n) = note {
        list = list.push(container(text(tr(&n)).size(12).color(if pane.error.is_some() { Color::from_rgb8(0xff, 0x6b, 0x6b) } else { dim })).padding(12));
    }
    container(column![caption(if id == PaneId::Local { "Este Mac" } else { "Servidor" }, fg), tools, scrollable(list).height(Length::Fill)].spacing(6))
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .padding(10)
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.04, ..Color::WHITE })),
            border: Border { radius: 10.0.into(), width: 1.0, color: Color { a: 0.1, ..fg } },
            ..Default::default()
        })
        .into()
}

pub fn confirm_delete_view<'a>(pane: PaneId, e: &'a Entry, p: &Palette) -> Element<'a, Message> {
    let fg = p.fg;
    let red = Color::from_rgb8(0xff, 0x45, 0x3b);
    let note = if pane == PaneId::Local { "Se moverá a la papelera." } else { "Se eliminará del servidor y no se puede recuperar." };
    dialog(
        column![
            text(tr(&format!("¿Eliminar “{}”?", e.name))).size(16).color(red),
            text(tr(note)).size(13).color(fg),
            row![space::horizontal(), action_button("Cancelar", fg, fg, false).on_press(Message::FilesDeleteCancel), action_button("Eliminar", red, fg, true).on_press(Message::FilesDeleteOk)].spacing(8),
        ]
        .spacing(12)
        .into(),
        p,
        420.0,
    )
}

pub fn name_dialog_view<'a>(d: &'a NameDialog, p: &Palette, accent: Color) -> Element<'a, Message> {
    let fg = p.fg;
    let (title, ok) = match &d.kind {
        NameKind::NewFolder => ("Nueva carpeta".to_string(), "Crear"),
        NameKind::Rename(old) => (format!("Renombrar “{old}”"), "Renombrar"),
    };
    dialog(
        column![
            text(tr(&title)).size(16).color(fg),
            text_input("Nombre", &d.text).id("files-name").on_input(Message::FilesName).on_submit(Message::FilesNameOk),
            row![space::horizontal(), action_button("Cancelar", fg, fg, false).on_press(Message::FilesNameCancel), action_button(ok, accent, fg, true).on_press(Message::FilesNameOk)].spacing(8),
        ]
        .spacing(12)
        .into(),
        p,
        400.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, dir: bool) -> Entry {
        Entry { name: name.into(), is_dir: dir, is_link: false, size: 0, modified: String::new() }
    }

    #[test]
    fn sorts_folders_first_case_insensitively() {
        let mut v = vec![e("b.txt", false), e("Zeta", true), e("A.txt", false), e("alfa", true)];
        sort_entries(&mut v);
        assert_eq!(v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["alfa", "Zeta", "A.txt", "b.txt"]);
    }

    #[test]
    fn remote_start_directory() {
        assert_eq!(resolve_remote_start("/home/u", None), "/home/u");
        assert_eq!(resolve_remote_start("/home/u", Some("  ")), "/home/u");
        assert_eq!(resolve_remote_start("/home/u", Some("~")), "/home/u");
        assert_eq!(resolve_remote_start("/home/u", Some("~/app")), "/home/u/app");
        assert_eq!(resolve_remote_start("/home/u", Some("/var/www")), "/var/www");
        assert_eq!(resolve_remote_start("/home/u", Some("app/src")), "/home/u/app/src");
    }

    #[test]
    fn remote_paths() {
        assert_eq!(join_remote("/", "a"), "/a");
        assert_eq!(join_remote("/home/u", "a b"), "/home/u/a b");
        assert_eq!(parent_remote("/home/u/"), "/home");
        assert_eq!(parent_remote("/home"), "/");
        assert_eq!(parent_remote("/"), "/");
        assert!(valid_name("ok.txt") && !valid_name("a/b") && !valid_name("..") && !valid_name("  "));
    }

    #[test]
    fn remote_entries_hide_dotfiles_unless_asked() {
        let raw = vec![
            RemoteEntry { name: ".env".into(), is_dir: false, is_link: false, size: 1, modified: String::new() },
            RemoteEntry { name: "docs".into(), is_dir: true, is_link: false, size: 0, modified: String::new() },
        ];
        assert_eq!(remote_entries(raw.clone(), false).len(), 1);
        let all = remote_entries(raw, true);
        assert_eq!(all.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["docs", ".env"]);
    }

    #[test]
    fn local_listing_reads_a_real_folder() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("a.txt"), "hola").unwrap();
        std::fs::write(d.path().join(".oculto"), "x").unwrap();
        std::os::unix::fs::symlink(d.path().join("sub"), d.path().join("enlace")).unwrap();
        let v = list_local(d.path(), false).unwrap();
        assert_eq!(v.iter().map(|x| (x.name.as_str(), x.is_dir)).collect::<Vec<_>>(), [("enlace", true), ("sub", true), ("a.txt", false)]);
        assert_eq!(v[2].size, 4);
        assert_eq!(list_local(d.path(), true).unwrap().len(), 4);
        assert!(list_local(&d.path().join("no-existe"), false).is_err());
    }

    #[test]
    fn selection_follows_names() {
        let mut p = Pane::new("/".into(), false);
        p.entries = vec![e("a", false)];
        assert!(p.selected_entry().is_none());
        p.selected = Some("a".into());
        assert_eq!(p.selected_entry().unwrap().name, "a");
    }
}
