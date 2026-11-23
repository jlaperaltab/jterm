//! Explorador de S3: estado, utilidades puras, transferencias y vista.
//! No llama a AWS por sí mismo: las operaciones se lanzan desde los botones de la vista.

use crate::colors::Palette;
use crate::editor::{action_button, dialog};
use crate::i18n::tr;
use crate::Message;
use iced::widget::{button, column, container, row, scrollable, space, text, Column};
use crate::ui::{input as text_input};
use iced::{Background, Border, Color, Element, Length};
use jterm_core::aws::{AwsAccount, AwsError};
use jterm_core::s3::{self, Bucket, Listing};
use jterm_net::s3_client::S3Client;
use jterm_net::transport::{Progress, ReqwestTransport, Transport};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub type Client = S3Client<ReqwestTransport>;

/// Una fila del explorador.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Clave completa (las carpetas terminan en "/").
    pub key: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
}

/// Filas de un listado: carpetas primero, luego archivos, cada grupo por nombre. El marcador de la carpeta actual no se muestra.
pub fn entries(l: &Listing, prefix: &str) -> Vec<Entry> {
    let mut dirs: Vec<Entry> = l
        .prefixes
        .iter()
        .map(|p| Entry { key: p.clone(), name: p[prefix.len().min(p.len())..].trim_end_matches('/').to_string(), is_dir: true, size: 0, modified: String::new() })
        .collect();
    let mut files: Vec<Entry> = l
        .objects
        .iter()
        .filter(|o| o.key != prefix && !o.key.ends_with('/'))
        .map(|o| Entry {
            key: o.key.clone(),
            name: o.key[prefix.len().min(o.key.len())..].to_string(),
            is_dir: false,
            size: o.size,
            modified: o.modified.map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default(),
        })
        .collect();
    dirs.sort_by_key(|e| e.name.to_lowercase());
    files.sort_by_key(|e| e.name.to_lowercase());
    dirs.extend(files);
    dirs
}

pub fn human_size(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", U[i]) }
}

/// Carpeta padre de un prefijo ("a/b/" → "a/"; "a/" → "").
pub fn parent(prefix: &str) -> String {
    let t = prefix.trim_end_matches('/');
    match t.rfind('/') {
        Some(i) => t[..=i].to_string(),
        None => String::new(),
    }
}

/// Migas de pan: (texto, prefijo al que lleva).
pub fn crumbs(prefix: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut acc = String::new();
    for part in prefix.split('/').filter(|p| !p.is_empty()) {
        acc.push_str(part);
        acc.push('/');
        out.push((part.to_string(), acc.clone()));
    }
    out
}

#[derive(Clone)]
pub struct Transfer {
    pub id: u64,
    pub name: String,
    pub done: Arc<AtomicU64>,
    /// Bytes totales; se conocen al empezar o, en descargas de carpetas, tras listar su contenido.
    pub total: Arc<AtomicU64>,
    pub finished: bool,
    pub error: Option<String>,
}

impl Transfer {
    pub fn percent(&self) -> u32 {
        let total = self.total.load(Ordering::Relaxed);
        if self.finished {
            100
        } else if total == 0 {
            0
        } else {
            ((self.done.load(Ordering::Relaxed) * 100) / total).min(99) as u32
        }
    }
}

pub struct S3State {
    pub account: AwsAccount,
    pub client: Arc<Client>,
    pub bucket: Option<String>,
    /// Prefijo actual con "/" final (vacío = raíz del bucket).
    pub prefix: String,
    pub buckets: Vec<Bucket>,
    pub entries: Vec<Entry>,
    pub selected: Option<String>,
    pub loading: bool,
    pub status: String,
    pub is_error: bool,
    pub generation: u64,
    pub transfers: Vec<Transfer>,
    pub next_transfer: u64,
    pub confirm_delete: Option<Vec<Entry>>,
    pub new_folder: Option<String>,
}

impl S3State {
    pub fn new(account: AwsAccount, client: Arc<Client>) -> Self {
        S3State {
            account, client, bucket: None, prefix: String::new(), buckets: vec![], entries: vec![], selected: None, loading: false,
            status: "Pulsa ↻ para listar los buckets.".into(), is_error: false, generation: 0, transfers: vec![], next_transfer: 1,
            confirm_delete: None, new_folder: None,
        }
    }

    pub fn path(&self) -> String {
        match &self.bucket {
            None => "/".into(),
            Some(b) => s3::path::join(b, self.prefix.trim_end_matches('/')),
        }
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.selected.as_ref().and_then(|k| self.entries.iter().find(|e| &e.key == k))
    }

    /// Registra una transferencia y devuelve su id y los contadores (hechos, totales) que actualiza la tarea.
    pub fn new_transfer(&mut self, name: String, total: u64) -> (u64, Arc<AtomicU64>, Arc<AtomicU64>) {
        let id = self.next_transfer;
        self.next_transfer += 1;
        let (done, total) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(total)));
        self.transfers.push(Transfer { id, name, done: done.clone(), total: total.clone(), finished: false, error: None });
        (id, done, total)
    }
}

fn counter(done: &Arc<AtomicU64>, base: u64) -> Progress {
    let d = done.clone();
    Arc::new(move |n| d.store(base + n, Ordering::Relaxed))
}

/// Sube archivos a `bucket/prefix`. Devuelve el primer error.
pub async fn upload_files<T: Transport>(client: Arc<S3Client<T>>, bucket: String, prefix: String, files: Vec<PathBuf>, done: Arc<AtomicU64>) -> Result<(), String> {
    let mut base = 0u64;
    for f in files {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("Nombre de archivo no válido.")?;
        let size = tokio::fs::metadata(&f).await.map_err(|e| e.to_string())?.len();
        client.upload(&f, &bucket, &format!("{prefix}{name}"), counter(&done, base)).await.map_err(|e| e.0)?;
        base += size;
        done.store(base, Ordering::Relaxed);
    }
    Ok(())
}

/// Objetos (clave, tamaño) que hay que bajar para una selección: el archivo, o todo lo que cuelga de la carpeta.
pub async fn expand_keys<T: Transport>(client: &S3Client<T>, bucket: &str, e: &Entry) -> Result<Vec<(String, u64)>, AwsError> {
    if !e.is_dir {
        return Ok(vec![(e.key.clone(), e.size)]);
    }
    let l = client.list(bucket, &e.key, true).await?;
    Ok(l.objects.into_iter().filter(|o| !o.key.ends_with('/')).map(|o| (o.key, o.size)).collect())
}

/// Descarga `items` dentro de `dest`, conservando la estructura relativa a `strip` (el prefijo del padre).
pub async fn download_items<T: Transport>(client: Arc<S3Client<T>>, bucket: String, items: Vec<(String, u64)>, strip: String, dest: PathBuf, done: Arc<AtomicU64>) -> Result<usize, String> {
    let mut base = 0u64;
    let mut n = 0;
    for (key, size) in items {
        let rel = key.strip_prefix(strip.as_str()).unwrap_or(&key);
        // Un bucket ajeno podría traer claves con ".." para escribir fuera de la carpeta elegida.
        let Some(target) = s3::path::safe_destination(&dest, rel) else { continue };
        client.download(&bucket, &key, &target, counter(&done, base)).await.map_err(|e| e.0)?;
        base += size;
        done.store(base, Ordering::Relaxed);
        n += 1;
    }
    Ok(n)
}

/// Claves a borrar para una selección (las carpetas incluyen todo su contenido y su marcador).
pub async fn delete_keys<T: Transport>(client: &S3Client<T>, bucket: &str, es: &[Entry]) -> Result<Vec<String>, AwsError> {
    let mut keys = Vec::new();
    for e in es {
        if e.is_dir {
            let l = client.list(bucket, &e.key, true).await?;
            keys.extend(l.objects.into_iter().map(|o| o.key));
            if !keys.contains(&e.key) {
                keys.push(e.key.clone());
            }
        } else {
            keys.push(e.key.clone());
        }
    }
    Ok(keys)
}

// MARK: Vista

pub fn view<'a>(st: &'a S3State, p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim, bg) = (p.fg, Color { a: 0.55, ..p.fg }, p.bg);
    let btn = |label: &'static str, msg: Option<Message>| {
        let b = action_button(label, fg, fg, false);
        match msg {
            Some(m) => b.on_press(m),
            None => b,
        }
    };
    let sel = st.selected_entry();
    let has_bucket = st.bucket.is_some();

    let mut crumbs_row = row![container(text(tr("S3")).size(13).color(accent)).padding([0, 4])].spacing(2).align_y(iced::Alignment::Center);
    crumbs_row = crumbs_row.push(
        button(text(tr(if st.bucket.is_some() { "Buckets" } else { "Buckets ▸" })).size(13).color(fg)).padding([2, 6]).on_press(Message::S3Home).style(link_style(fg)),
    );
    if let Some(b) = &st.bucket {
        crumbs_row = crumbs_row.push(text(tr("›")).color(dim));
        crumbs_row = crumbs_row.push(button(text(b.clone()).size(13).color(fg)).padding([2, 6]).on_press(Message::S3Cd(String::new())).style(link_style(fg)));
        for (name, pfx) in crumbs(&st.prefix) {
            crumbs_row = crumbs_row.push(text(tr("›")).color(dim));
            crumbs_row = crumbs_row.push(button(text(name).size(13).color(fg)).padding([2, 6]).on_press(Message::S3Cd(pfx)).style(link_style(fg)));
        }
    }

    let toolbar = row![
        btn("←", has_bucket.then_some(Message::S3Up)),
        crumbs_row,
        btn(if st.loading { "…" } else { "↻" }, Some(Message::S3Refresh)),
        btn("⬆ Subir", has_bucket.then_some(Message::S3Upload)),
        btn("⬇ Descargar", sel.map(|_| Message::S3Download)),
        btn("+ Carpeta", has_bucket.then_some(Message::S3NewFolder)),
        btn("🔗 URL", sel.filter(|e| !e.is_dir).map(|_| Message::S3Presign)),
        btn("🗑", sel.map(|_| Message::S3Delete)),
        btn("✕ Cerrar", Some(Message::S3Close)),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .wrap()
    .vertical_spacing(6);

    let mut list: Column<Message> = Column::new().spacing(1);
    if st.bucket.is_none() {
        for b in &st.buckets {
            let row_ = row![crate::ui::label("🪣", 13.0, dim), text(b.name.clone()).size(13).color(fg).width(Length::Fill), text(b.created.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()).size(11).color(dim)]
                .spacing(10)
                .align_y(iced::Alignment::Center);
            list = list.push(button(row_).width(Length::Fill).padding([6, 10]).on_press(Message::S3Enter(b.name.clone())).style(row_style(fg, false)));
        }
    } else {
        for e in &st.entries {
            let selected = st.selected.as_deref() == Some(e.key.as_str());
            let size = if e.is_dir { String::new() } else { human_size(e.size) };
            let row_ = row![
                crate::ui::label(if e.is_dir { "📁" } else { "📄" }, 13.0, if e.is_dir { accent } else { dim }),
                text(e.name.clone()).size(13).color(fg).width(Length::Fill),
                text(size).size(11).color(dim).width(Length::Fixed(80.0)),
                text(e.modified.clone()).size(11).color(dim).width(Length::Fixed(130.0)),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center);
            let open = e.is_dir.then(|| Message::S3Cd(e.key.clone()));
            list = list.push(crate::ui::selectable_row(row_, selected, fg, Message::S3Select(Some(e.key.clone())), open));
        }
    }
    if !st.loading && ((st.bucket.is_none() && st.buckets.is_empty()) || (st.bucket.is_some() && st.entries.is_empty())) && !st.is_error {
        list = list.push(container(text(tr(if st.bucket.is_none() { "Sin buckets cargados." } else { "Carpeta vacía." })).size(12).color(dim)).padding(14));
    }

    let mut transfers: Column<Message> = Column::new().spacing(4);
    for t in st.transfers.iter().rev().take(4) {
        let label = match &t.error {
            Some(e) => format!("✕ {} — {e}", t.name),
            None if t.finished => format!("✓ {}", t.name),
            None => format!("{} {}%", t.name, t.percent()),
        };
        let color = if t.error.is_some() { Color::from_rgb8(0xff, 0x6b, 0x6b) } else { dim };
        transfers = transfers.push(row![text(tr(&label)).size(11).color(color).width(Length::Fill), action_button("×", dim, fg, false).on_press(Message::S3DismissTransfer(t.id))].align_y(iced::Alignment::Center));
    }

    let status = text(tr(&st.status)).size(11).color(if st.is_error { Color::from_rgb8(0xff, 0x6b, 0x6b) } else { dim });
    container(column![toolbar, container(scrollable(list)).height(Length::Fill), transfers, status].spacing(8).padding(12))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style { background: Some(Background::Color(bg)), ..Default::default() })
        .into()
}

fn link_style(fg: Color) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, st| button::Style {
        background: Some(Background::Color(if matches!(st, button::Status::Hovered) { Color { a: 0.12, ..fg } } else { Color::TRANSPARENT })),
        text_color: fg,
        border: Border { radius: 5.0.into(), ..Default::default() },
        ..Default::default()
    }
}

fn row_style(fg: Color, selected: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, st| button::Style {
        background: Some(Background::Color(if selected { Color { a: 0.2, ..fg } } else if matches!(st, button::Status::Hovered) { Color { a: 0.08, ..fg } } else { Color::TRANSPARENT })),
        text_color: fg,
        border: Border { radius: 6.0.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn confirm_delete_view<'a>(items: &'a [Entry], p: &Palette) -> Element<'a, Message> {
    let fg = p.fg;
    let red = Color::from_rgb8(0xff, 0x45, 0x3b);
    let what = if items.len() == 1 { format!("“{}”", items[0].name) } else { format!("{} elementos", items.len()) };
    let extra = if items.iter().any(|e| e.is_dir) { " Las carpetas se borran con todo su contenido." } else { "" };
    dialog(
        column![
            text(tr("¿Eliminar de S3?")).size(16).color(red),
            text(tr(&format!("Se eliminará {what} del bucket. No se puede deshacer.{extra}"))).size(13).color(fg),
            row![space::horizontal(), action_button("Cancelar", fg, fg, false).on_press(Message::S3DeleteCancel), action_button("Eliminar", red, fg, true).on_press(Message::S3DeleteConfirm)].spacing(8),
        ]
        .spacing(12)
        .into(),
        p,
        420.0,
    )
}

pub fn new_folder_view<'a>(name: &'a str, p: &Palette, accent: Color) -> Element<'a, Message> {
    let fg = p.fg;
    dialog(
        column![
            text(tr("Nueva carpeta")).size(16).color(fg),
            text_input("nombre", name).on_input(Message::S3FolderName).on_submit(Message::S3FolderOk).padding(7).size(13),
            row![space::horizontal(), action_button("Cancelar", fg, fg, false).on_press(Message::S3FolderCancel), action_button("Crear", accent, fg, true).on_press(Message::S3FolderOk)].spacing(8),
        ]
        .spacing(12)
        .into(),
        p,
        380.0,
    )
}

/// ¿Es un nombre de carpeta aceptable? (sin "/" ni vacío)
pub fn valid_folder_name(n: &str) -> bool {
    let t = n.trim();
    !t.is_empty() && !t.contains('/') && t != "." && t != ".."
}

/// Nombre de un archivo temporal de descarga en `dir` (para pruebas y avisos).
pub fn display_path(p: &Path) -> String {
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jterm_core::s3::Object;

    fn obj(k: &str, size: u64) -> Object {
        Object { key: k.into(), size, modified: None }
    }

    #[test]
    fn entries_sorted_folders_first_without_the_folder_marker() {
        let l = Listing {
            prefixes: vec!["a/zeta/".into(), "a/Beta/".into()],
            objects: vec![obj("a/", 0), obj("a/b.txt", 5), obj("a/A.txt", 3), obj("a/sub/", 0)],
        };
        let e = entries(&l, "a/");
        assert_eq!(e.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Beta", "zeta", "A.txt", "b.txt"]);
        assert!(e[0].is_dir && !e[2].is_dir);
        assert_eq!(e[0].key, "a/Beta/");
    }

    #[test]
    fn sizes_parent_and_crumbs() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0 GB");
        assert_eq!(parent("a/b/"), "a/");
        assert_eq!(parent("a/"), "");
        assert_eq!(parent(""), "");
        assert_eq!(crumbs("a/b/"), [("a".to_string(), "a/".to_string()), ("b".to_string(), "a/b/".to_string())]);
        assert!(crumbs("").is_empty());
    }

    fn fake_client(handler: impl Fn(&jterm_core::sigv4::SignedRequest) -> jterm_net::transport::HttpResponse + Send + Sync + 'static) -> Arc<S3Client<jterm_net::transport::FakeTransport>> {
        use jterm_net::transport::FakeTransport;
        Arc::new(S3Client::new(FakeTransport::new(handler), "us-east-1", || Ok(jterm_core::sigv4::Credentials { access_key: "AK".into(), secret: "SK".into() })))
    }

    fn region_ok(req: &jterm_core::sigv4::SignedRequest) -> Option<jterm_net::transport::HttpResponse> {
        (req.method == "HEAD").then(|| jterm_net::transport::response(200, ""))
    }

    #[tokio::test]
    async fn downloads_keep_structure_and_skip_path_traversal() {
        use jterm_net::transport::response;
        let c = fake_client(|req| region_ok(req).unwrap_or_else(|| response(200, "datos")));
        let dir = tempfile::tempdir().unwrap();
        let items = vec![("carpeta/a.txt".to_string(), 5), ("carpeta/sub/b.txt".to_string(), 5), ("carpeta/../../evil.txt".to_string(), 5)];
        let done = Arc::new(AtomicU64::new(0));
        let n = download_items(c, "b".into(), items, String::new(), dir.path().to_path_buf(), done.clone()).await.unwrap();
        assert_eq!(n, 2, "la clave con .. se descarta");
        assert_eq!(std::fs::read_to_string(dir.path().join("carpeta/sub/b.txt")).unwrap(), "datos");
        assert!(!dir.path().parent().unwrap().join("evil.txt").exists());
        assert_eq!(done.load(Ordering::Relaxed), 10);
    }

    #[tokio::test]
    async fn expand_and_delete_folders_recursively() {
        use jterm_net::transport::response;
        let listing = "<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>d/x</Key><Size>3</Size></Contents><Contents><Key>d/y/z</Key><Size>4</Size></Contents></ListBucketResult>";
        let c = fake_client(move |req| region_ok(req).unwrap_or_else(|| response(200, listing)));
        let folder = Entry { key: "d/".into(), name: "d".into(), is_dir: true, size: 0, modified: String::new() };
        let items = expand_keys(&c, "b", &folder).await.unwrap();
        assert_eq!(items, [("d/x".to_string(), 3), ("d/y/z".to_string(), 4)]);
        let keys = delete_keys(&c, "b", std::slice::from_ref(&folder)).await.unwrap();
        assert_eq!(keys, ["d/x", "d/y/z", "d/"]);
        let file = Entry { key: "f.txt".into(), name: "f.txt".into(), is_dir: false, size: 1, modified: String::new() };
        assert_eq!(expand_keys(&c, "b", &file).await.unwrap(), [("f.txt".to_string(), 1)]);
    }

    #[tokio::test]
    async fn uploads_put_files_under_the_prefix() {
        use jterm_net::transport::response;
        let c = fake_client(|req| region_ok(req).unwrap_or_else(|| response(200, "")));
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.bin"));
        std::fs::write(&a, "12345").unwrap();
        std::fs::write(&b, "678").unwrap();
        let done = Arc::new(AtomicU64::new(0));
        upload_files(c.clone(), "bk".into(), "x/".into(), vec![a, b], done.clone()).await.unwrap();
        assert_eq!(done.load(Ordering::Relaxed), 8);
        let urls: Vec<String> = c.transport().recorded().into_iter().filter(|r| r.method == "PUT").map(|r| r.url).collect();
        assert_eq!(urls, ["https://s3.us-east-1.amazonaws.com/bk/x/a.txt", "https://s3.us-east-1.amazonaws.com/bk/x/b.bin"]);
    }

    #[test]
    fn folder_names() {
        assert!(valid_folder_name(" datos "));
        assert!(!valid_folder_name(""));
        assert!(!valid_folder_name("a/b"));
        assert!(!valid_folder_name(".."));
    }

    #[test]
    fn transfer_percent_is_capped_until_finished() {
        let t = Transfer { id: 1, name: "x".into(), done: Arc::new(AtomicU64::new(50)), total: Arc::new(AtomicU64::new(50)), finished: false, error: None };
        assert_eq!(t.percent(), 99);
        let mut t2 = t.clone();
        t2.finished = true;
        assert_eq!(t2.percent(), 100);
        let z = Transfer { total: Arc::new(AtomicU64::new(0)), ..t };
        assert_eq!(z.percent(), 0);
    }
}
