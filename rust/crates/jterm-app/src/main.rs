mod appmenu;
mod aws_ui;
mod backend;
mod colors;
mod editor;
mod files_ui;
mod i18n;
mod i18n_en;
mod finder;
mod keys;
mod links;
mod logs_ui;
mod mouse;
mod panels;
mod s3_ui;
mod sync_ui;
mod term_view;
mod update_ui;
mod tunnels_ui;
mod ui;

use crate::i18n::tr;
use alacritty_terminal::index::Side;
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::term::TermMode;
use backend::{Backend, SpawnOptions};
use colors::{rgb, Palette};
use iced::widget::canvas::Cache;
use iced::widget::{pane_grid, button, canvas, center, column, container, opaque, row, scrollable, space, stack, text, Column, Row};
use crate::ui::{input as text_input};
use iced::{keyboard, time, Background, Border, Color, Element, Length, Size, Subscription, Task, Theme};
use aws_ui::{AwsEditor, AwsField, AwsPanel, ALL_REGIONS};
use jterm_core::aws::{AwsAccount, Ec2Instance, REGIONS};
use jterm_core::awsstore::{self, AwsStore, KnownUsers};
use jterm_net::aws_client::AwsClient;
use jterm_net::transport::ReqwestTransport;
use jterm_core::s3::{Bucket, Listing};
use jterm_net::s3_client::S3Client;
use s3_ui::S3State;
use appmenu::{AppMenu, MenuAction};
use finder::{FinderState, Item as FinderItem, Kind as FinderKind, Target as FinderTarget};
use jterm_net::updater::{self, BundleInfo, GithubFetcher, Release, Staged};
use update_ui::UpdateDialog;
use sync_ui::{DialogKind, KindFlag, SyncCtl, SyncDialog, SyncPrefs};
use jterm_core::sync::{SyncResult, SyncState, SyncStorage};
use jterm_core::syncadapter::FileAdapter;
use files_ui::{FilesState, NameDialog, NameKind, PaneId, Session};
use jterm_core::sftp::{Direction, RemoteEntry};
use jterm_core::models::{Tunnel, TunnelKind};
use jterm_core::tunnels::TunnelManager;
use tunnels_ui::{TunField, TunnelEditor};
use jterm_core::aws::{LogEvent, LogGroup};
use jterm_core::logs::LogLevel;
use logs_ui::LogsState;
use editor::{action_button, dialog, Editor, Field, JumpOpt};
use jterm_core::command_guard;
use jterm_core::models::{search_snippets, AuthMethod, Server, ServerEnv, Snippet};
use panels::{Page, Setting, SnippetForm, SnippetState};
use jterm_core::settings::Settings;
use jterm_core::sftp::{self, SshTarget};
use jterm_core::store::{self, JsonList, SecretStore};
use jterm_core::themes;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use term_view::{Metrics, TermView};
use uuid::Uuid;

#[derive(Debug, Clone)]
enum Message {
    Tick(Instant),
    Key(keyboard::Event),
    NewLocal,
    Select(usize),
    Close(usize),
    ToggleSidebar,
    Connect(Uuid),
    /// (pestaña, columnas, filas)
    Grid(u64, u16, u16),
    SelStart(u64, usize, usize, Side),
    SelMove(u64, usize, usize, Side),
    Scroll(u64, f32),
    Paste(Option<String>),
    /// Bytes de un informe de ratón para la aplicación del terminal.
    Write(u64, Vec<u8>),
    Redraw(u64),
    OpenLink(links::LinkKind, String),
    OpenFind,
    FindQuery(String),
    FindNext,
    FindPrev,
    CloseFind,
    // Editor de servidores
    EditNew,
    EditServer(Uuid),
    EditField(Field, String),
    EditAuth(AuthMethod),
    EditEnv(Option<ServerEnv>),
    EditJump(Option<Uuid>),
    EditSave,
    EditCancel,
    EditDelete,
    // Ajustes y comandos guardados
    OpenSettings,
    OpenSnippets,
    ClosePanel,
    Setting(Setting),
    SettingsPage(Page),
    SnipQuery(String),
    SnipInsert(Uuid),
    SnipNew,
    SnipEdit(Uuid),
    SnipField(bool, String),
    SnipAutoRun(bool),
    SnipSave,
    SnipDelete(Uuid),
    SnipBack,
    // Archivos (SFTP)
    FilesOpen(Uuid),
    FilesClose,
    FilesConnected(Result<Session, String>),
    FilesLoaded(u64, Result<Vec<RemoteEntry>, String>),
    FilesEnter(PaneId, String),
    FilesUp(PaneId),
    FilesRefresh(PaneId),
    FilesSelect(PaneId, Option<String>),
    FilesHidden(PaneId),
    FilesUpload,
    FilesDownload,
    FilesTransferDone(u64, Result<(), String>),
    FilesCancel(u64),
    FilesDismiss(u64),
    FilesDelete(PaneId),
    FilesDeleteCancel,
    FilesDeleteOk,
    FilesNewFolder(PaneId),
    FilesRename(PaneId),
    FilesName(String),
    FilesNameOk,
    FilesNameCancel,
    FilesOpDone(PaneId, Result<(), String>),
    // Sincronización con iCloud
    SyncToggle(bool),
    SyncKind(KindFlag, bool),
    SyncSecrets(bool),
    SyncNow,
    SyncChangePass,
    SyncDelete,
    SyncReenter,
    SyncPass1(String),
    SyncPass2(String),
    SyncDialogOk,
    SyncDialogCancel,
    SyncDone(Result<(SyncResult, SyncState), (String, bool)>),
    // Túneles
    TunEdNew,
    TunEdEdit(Uuid),
    TunEdField(TunField, String),
    TunEdKind(TunnelKind),
    TunEdServer(Option<Uuid>),
    TunEdSave,
    TunEdCancel,
    TunEdDelete,
    TunToggle(Uuid),
    CloseRequested,
    // Buscador rápido (⌘K)
    FinderQuery(String),
    FinderRun(usize, bool),
    FinderClose,
    FinderPin(usize),
    Relaunch,
    // Actualizador
    UpdateCheck(bool),
    UpdateChecked(bool, Result<Option<Release>, String>),
    UpdateInstall,
    UpdateLater,
    UpdatePrepared(Result<Staged, String>),
    // Paneles
    PaneFocus(pane_grid::Pane),
    PaneResized(pane_grid::ResizeEvent),
    // Ventana
    ShowScreen(Screen),
    DragWindow,
    ZoomWindow,
    // Barra lateral y AWS
    SidebarMode(SideMode),
    AwsAccountPick(Uuid),
    AwsRegionPick(String),
    AwsRefresh,
    AwsLoaded(u64, Vec<Ec2Instance>, Vec<String>),
    AwsConnect(usize),
    AwsUserInput(String),
    AwsUserOk,
    AwsUserCancel,
    AwsConnected(Result<ConnectInfo, String>),
    AwsCopy(String),
    AwsEdNew,
    AwsEdEdit,
    AwsEdField(AwsField, String),
    AwsEdRegion(String),
    AwsEdEnv(Option<ServerEnv>),
    AwsEdSave,
    AwsEdCancel,
    AwsEdDelete,
    // Visor de logs de CloudWatch
    LogsOpen,
    LogsClose,
    LogsRegion(String),
    LogsPicker(bool),
    LogsPickerQuery(String),
    LogsLoadGroups,
    LogsGroups(String, Result<Vec<LogGroup>, String>),
    LogsPick(String),
    LogsRange(String),
    LogsFilter(String),
    LogsStream(String),
    LogsSearch,
    LogsPage(u64, Result<(Vec<LogEvent>, Option<String>), String>),
    LogsFollow,
    LogsFollowed(u64, Result<Vec<LogEvent>, String>),
    LogsLevel(Option<LogLevel>),
    LogsToggleGroup,
    LogsTogglePretty,
    LogsToggleMode,
    LogsMode(bool),
    LogsCollapse(String),
    LogsExpand(usize),
    LogsMore,
    LogsCopyAll,
    LogsCopyEvent(usize),
    LogsSave,
    LogsSaveTo(Option<PathBuf>),
    LogsClear,
    LogsScrolled(f32),
    // Explorador de S3
    S3Open,
    S3Close,
    S3Home,
    S3Enter(String),
    S3Cd(String),
    S3Up,
    S3Refresh,
    S3Buckets(u64, Result<Vec<Bucket>, String>),
    S3Listed(u64, Result<Listing, String>),
    S3Select(Option<String>),
    S3Upload,
    S3UploadPicked(Vec<PathBuf>),
    S3Download,
    S3DownloadPicked(Option<PathBuf>),
    S3TransferDone(u64, Result<String, String>),
    S3DismissTransfer(u64),
    S3Presign,
    S3Presigned(Result<String, String>),
    S3NewFolder,
    S3FolderName(String),
    S3FolderOk,
    S3FolderCancel,
    S3Delete,
    S3DeleteCancel,
    S3DeleteConfirm,
    S3Deleted(Result<usize, String>),
    S3FolderDone(Result<(), String>),
    // Aviso de comando delicado en producción
    DangerRun,
    DangerCancel,
}

/// Qué se muestra en el área principal: el terminal activo o una de las vistas de AWS abiertas como pestaña.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Term,
    Logs,
    S3,
    Files,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideMode {
    Servers,
    Aws,
    Tunnels,
}

/// Datos para abrir la sesión SSH de una instancia tras enviar la llave por Instance Connect.
#[derive(Debug, Clone)]
struct ConnectInfo {
    instance: Ec2Instance,
    user: String,
    options: Vec<String>,
    env: Option<ServerEnv>,
}

struct UserPrompt {
    instance: Ec2Instance,
    user: String,
}

/// Comando delicado pendiente de confirmar en una pestaña de producción.
struct Danger {
    tab: u64,
    what: &'static str,
    line: String,
}

/// Cómo se lanzó un panel (para duplicarlo al dividir: una sesión SSH vuelve al mismo servidor).
#[derive(Clone)]
struct SpawnSpec {
    title: String,
    program: Option<String>,
    args: Vec<String>,
    env: HashMap<String, String>,
    server: Option<Uuid>,
    tab_env: Option<ServerEnv>,
    remote: bool,
}

/// Un terminal (un panel dentro de una pestaña).
struct Pane {
    id: u64,
    title: String,
    server: Option<Uuid>,
    env: Option<ServerEnv>,
    backend: Arc<Backend>,
    cache: Cache,
    scroll_acc: f32,
    bell: bool,
    /// Sesión en otro equipo (SSH): sus rutas de archivo no son locales.
    remote: bool,
    spec: SpawnSpec,
}

impl Pane {
    fn alive(&self) -> bool {
        !self.backend.exited()
    }
}

/// Una pestaña: uno o varios paneles de terminal. Sus campos de solo lectura y escritura (título, backend…)
/// son los del panel con el foco.
struct Tab {
    grid: pane_grid::State<Pane>,
    focus: pane_grid::Pane,
    /// Lo que se escribe se envía a todos los paneles.
    broadcast: bool,
}

impl std::ops::Deref for Tab {
    type Target = Pane;
    fn deref(&self) -> &Pane {
        self.grid.get(self.focus).expect("el panel con foco existe")
    }
}

impl std::ops::DerefMut for Tab {
    fn deref_mut(&mut self) -> &mut Pane {
        self.grid.get_mut(self.focus).expect("el panel con foco existe")
    }
}

/// Paneles en orden espacial (izquierda→derecha, arriba→abajo), para recorrerlos con ⌘[ y ⌘].
fn leaves(n: &pane_grid::Node, out: &mut Vec<pane_grid::Pane>) {
    match n {
        pane_grid::Node::Pane(p) => out.push(*p),
        pane_grid::Node::Split { a, b, .. } => {
            leaves(a, out);
            leaves(b, out);
        }
    }
}

impl Tab {
    fn new(pane: Pane) -> Self {
        let (grid, focus) = pane_grid::State::new(pane);
        Tab { grid, focus, broadcast: false }
    }

    fn ordered(&self) -> Vec<pane_grid::Pane> {
        let mut v = Vec::new();
        leaves(self.grid.layout(), &mut v);
        v
    }

    fn panes(&self) -> impl Iterator<Item = &Pane> {
        self.grid.iter().map(|(_, p)| p)
    }

    fn clear_caches(&self) {
        self.panes().for_each(|p| p.cache.clear());
    }
}

enum Panel {
    Settings(Page),
    Snippets(SnippetState),
}

/// Barra de búsqueda (⌘F) de la pestaña activa.
struct FindState {
    query: String,
    /// (actual, total)
    info: (usize, usize),
}

struct App {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    servers: JsonList<Server>,
    settings: Settings,
    palette: Palette,
    metrics: Metrics,
    sidebar: bool,
    error: Option<String>,
    editor: Option<Editor>,
    danger: Option<Danger>,
    find: Option<FindState>,
    panel: Option<Panel>,
    snippets: JsonList<Snippet>,
    secrets: Arc<dyn SecretStore>,
    side: SideMode,
    aws: AwsPanel,
    aws_store: AwsStore,
    aws_users: KnownUsers,
    aws_editor: Option<AwsEditor>,
    user_prompt: Option<UserPrompt>,
    aws_net: Option<Arc<AwsClient<ReqwestTransport>>>,
    s3: Option<S3State>,
    logs: Option<LogsState>,
    screen: Screen,
    menu: Option<AppMenu>,
    menu_failed: bool,
    finder: Option<FinderState>,
    bundle: Option<BundleInfo>,
    update_dialog: Option<UpdateDialog>,
    update_busy: bool,
    update_found: Option<Release>,
    update_due: Option<Instant>,
    files: Option<FilesState>,
    sync: SyncCtl,
    tunnels: JsonList<Tunnel>,
    tunnel_mgr: TunnelManager,
    tunnel_editor: Option<TunnelEditor>,
    data_dir: PathBuf,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn load_settings(dir: &std::path::Path) -> Settings {
    std::fs::read(dir.join("settings.json")).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

fn boot() -> (App, Task<Message>) {
    let data_dir = store::data_dir().unwrap_or_else(|_| std::env::temp_dir());
    let settings = load_settings(&data_dir);
    // El idioma se fija al arrancar (cambiarlo pide reiniciar).
    i18n::set_english(i18n::resolve(&settings.language, &i18n::system_languages()));
    let palette = Palette::from_theme(themes::theme(&settings.theme));
    let metrics = Metrics::for_settings(&settings.font_family, settings.font_size as f32, settings.font_medium);
    #[cfg(target_os = "macos")]
    let secrets: Arc<dyn SecretStore> = Arc::new(store::KeychainVault::new());
    #[cfg(not(target_os = "macos"))]
    let secrets: Arc<dyn SecretStore> = Arc::new(store::MemorySecrets::default());
    let sync_prefs: SyncPrefs = std::fs::read(data_dir.join("sync-prefs.json")).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
    let mut app = App {
        tabs: vec![],
        active: 0,
        next_id: 1,
        servers: JsonList::open(data_dir.join("servers.json")),
        sidebar: settings.sidebar_at_launch,
        settings,
        palette,
        metrics,
        error: None,
        editor: None,
        danger: None,
        find: None,
        panel: None,
        snippets: JsonList::open(data_dir.join("commands.json")),
        secrets,
        side: SideMode::Servers,
        aws: AwsPanel::default(),
        aws_store: AwsStore::open(data_dir.join("aws.json")),
        aws_users: KnownUsers::open(data_dir.join("aws-users.json")),
        aws_editor: None,
        user_prompt: None,
        aws_net: ReqwestTransport::new().ok().map(|t| Arc::new(AwsClient::new(t))),
        s3: None,
        logs: None,
        screen: Screen::Term,
        menu: None,
        menu_failed: false,
        finder: None,
        bundle: std::env::current_exe().ok().and_then(|e| updater::bundle_of_exe(&e)),
        update_dialog: None,
        update_busy: false,
        update_found: None,
        update_due: None,
        files: None,
        sync: SyncCtl::new(sync_prefs),
        tunnels: JsonList::open(data_dir.join("tunnels.json")),
        tunnel_mgr: TunnelManager::default(),
        tunnel_editor: None,
        data_dir,
    };
    app.new_local();
    // Comprobación silenciosa de actualizaciones al abrir (como mucho una vez al día y solo desde la .app empaquetada).
    if app.bundle.as_ref().is_some_and(|b| b.repo.is_some()) && update_ui::due_for_launch_check(now_unix(), app.last_update_check()) {
        app.update_due = Some(Instant::now() + Duration::from_secs(4));
    }
    // Si la sincronización está activa, se hace una al abrir.
    if app.sync.prefs.enabled {
        app.sync.due = Some(Instant::now() + Duration::from_secs(2));
    }
    (app, Task::none())
}

impl App {
    fn env_for_pty(&self) -> HashMap<String, String> {
        HashMap::from([
            ("TERM".to_string(), self.settings.term.clone()),
            ("COLORTERM".to_string(), "truecolor".to_string()),
            ("TERM_PROGRAM".to_string(), "jTerm".to_string()),
        ])
    }

    fn spawn(&mut self, title: String, program: Option<String>, args: Vec<String>, env: HashMap<String, String>, server: Option<&Server>) {
        self.spawn_with(title, program, args, env, server.map(|s| s.id), server.and_then(|s| s.env), server.is_some());
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_with(&mut self, title: String, program: Option<String>, args: Vec<String>, env: HashMap<String, String>, server: Option<Uuid>, tab_env: Option<ServerEnv>, remote: bool) {
        let spec = SpawnSpec { title, program, args, env, server, tab_env, remote };
        if let Some(p) = self.build_pane(spec) {
            self.screen = Screen::Term;
            self.tabs.push(Tab::new(p));
            self.active = self.tabs.len() - 1;
            self.error = None;
        }
    }

    /// Lanza un terminal según `spec` (un PTY nuevo).
    fn build_pane(&mut self, spec: SpawnSpec) -> Option<Pane> {
        let id = self.next_id;
        self.next_id += 1;
        let mut env = spec.env.clone();
        env.extend(self.env_for_pty());
        let start = jterm_core::ssh_jump::expand_tilde(&self.settings.start_dir);
        let cwd = if !self.settings.start_dir.is_empty() && std::path::Path::new(&start).is_dir() {
            Some(PathBuf::from(start))
        } else {
            std::env::var_os("HOME").map(PathBuf::from)
        };
        let opts = SpawnOptions {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd,
            env,
            scrollback: self.settings.scrollback as usize,
            cols: 100,
            lines: 30,
            cell_width: self.metrics.cw.round() as u16,
            cell_height: self.metrics.ch as u16,
        };
        match Backend::spawn(id, opts) {
            Ok(b) => {
                b.set_underline_links(self.settings.underline_links);
                Some(Pane {
                    id,
                    title: spec.title.clone(),
                    server: spec.server,
                    env: spec.tab_env,
                    remote: spec.remote,
                    backend: Arc::new(b),
                    cache: Cache::new(),
                    scroll_acc: 0.0,
                    bell: false,
                    spec,
                })
            }
            Err(e) => {
                self.error = Some(format!("No se pudo abrir el terminal: {e}"));
                None
            }
        }
    }

    /// Divide el panel con foco (⌘D: a la derecha; ⇧⌘D: hacia abajo). El panel nuevo repite la sesión.
    fn split_focused(&mut self, side_by_side: bool) {
        let Some(spec) = self.tabs.get(self.active).map(|t| t.spec.clone()) else { return };
        let Some(pane) = self.build_pane(spec) else { return };
        let axis = if side_by_side { pane_grid::Axis::Vertical } else { pane_grid::Axis::Horizontal };
        let t = &mut self.tabs[self.active];
        if let Some((new, _)) = t.grid.split(axis, t.focus, pane) {
            t.focus = new;
        }
        t.clear_caches();
    }

    /// Cierra el panel con foco; si era el único, cierra la pestaña.
    fn close_focused_pane(&mut self) {
        let Some(t) = self.tabs.get_mut(self.active) else { return };
        let was = t.focus;
        match t.grid.close(was) {
            Some((_, sibling)) => {
                t.focus = sibling;
                t.clear_caches();
            }
            None => self.close(self.active),
        }
    }

    fn move_focus(&mut self, dir: pane_grid::Direction) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            if let Some(next) = t.grid.adjacent(t.focus, dir) {
                t.focus = next;
                t.clear_caches();
            }
        }
    }

    fn cycle_focus(&mut self, delta: i32) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            let order = t.ordered();
            if let Some(i) = order.iter().position(|p| *p == t.focus) {
                t.focus = order[(i as i32 + delta).rem_euclid(order.len() as i32) as usize];
                t.clear_caches();
            }
        }
    }

    fn toggle_zoom(&mut self) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            if t.grid.maximized().is_some() {
                t.grid.restore();
            } else if t.grid.len() > 1 {
                t.grid.maximize(t.focus);
            }
            t.clear_caches();
        }
    }

    /// Activa o desactiva escribir en todos los paneles. No se activa si hay un panel de producción (sus avisos no aplicarían).
    fn toggle_broadcast(&mut self) {
        let Some(t) = self.tabs.get_mut(self.active) else { return };
        if !t.broadcast && t.panes().any(|p| p.env == Some(ServerEnv::Prod)) {
            self.error = Some("No se puede escribir en todos los paneles: hay uno de PRODUCCIÓN.".into());
            return;
        }
        t.broadcast = !t.broadcast;
    }

    fn new_local(&mut self) {
        let shell = self.settings.resolved_shell();
        let args = if self.settings.login_shell { vec!["-l".to_string()] } else { vec![] };
        self.spawn("Terminal".into(), Some(shell), args, HashMap::new(), None);
    }

    /// Abre una sesión SSH con el `ssh` del sistema. Lee el Keychain (puede mostrar su diálogo).
    fn connect(&mut self, id: Uuid) {
        let Some(server) = self.servers.items.iter().find(|s| s.id == id).cloned() else { return };
        let askpass = match sftp::ensure_askpass(&self.data_dir) {
            Ok(p) => p.display().to_string(),
            Err(e) => {
                self.error = Some(format!("No se pudo preparar askpass: {e}"));
                return;
            }
        };
        let target: SshTarget = sftp::target_for_server(&server, &self.servers.items, &self.settings, &*self.secrets, &askpass, "/usr/bin/ssh");
        let args = sftp::interactive_args(&target, &self.settings);
        let title = target.title.clone();
        self.spawn(title, Some("/usr/bin/ssh".into()), args, target.env, Some(&server));
    }

    fn close(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        if self.tabs.is_empty() {
            self.new_local();
        } else {
            self.active = self.active.min(self.tabs.len() - 1);
        }
    }

    /// ¿Hay que frenar este Retorno por un comando delicado en producción? (abre el aviso)
    fn guard_enter(&mut self) -> bool {
        let Some(t) = self.active_tab() else { return false };
        if t.env != Some(ServerEnv::Prod) || !self.settings.warn_dangerous_prod || t.backend.alt_screen() {
            return false;
        }
        let line = t.backend.cursor_line_text();
        match command_guard::risk(&line) {
            Some(what) => {
                self.danger = Some(Danger { tab: t.id, what, line });
                true
            }
            None => false,
        }
    }

    /// ¿Hay un diálogo, el buscador o la barra de búsqueda abiertos? (el teclado y el portapapeles son suyos)
    fn overlay_open(&self) -> bool {
        self.finder.is_some()
            || self.find.is_some()
            || self.editor.is_some()
            || self.danger.is_some()
            || self.panel.is_some()
            || self.aws_editor.is_some()
            || self.user_prompt.is_some()
            || self.tunnel_editor.is_some()
            || self.sync.dialog.is_some()
            || self.update_dialog.is_some()
    }

    /// Acciones del menú superior (las mismas que los atajos de teclado).
    fn menu_action(&mut self, a: MenuAction) -> Task<Message> {
        use pane_grid::Direction as D;
        match a {
            MenuAction::Settings => self.panel = Some(Panel::Settings(Page::General)),
            MenuAction::Quit => {
                self.tunnel_mgr.stop_all();
                return iced::exit();
            }
            MenuAction::Copy => {
                if let Some(t) = self.active_tab().filter(|_| self.screen == Screen::Term && !self.overlay_open()) {
                    if let Some(s) = t.backend.selection_text() {
                        t.backend.clear_selection();
                        return iced::clipboard::write(s);
                    }
                }
            }
            MenuAction::Paste => {
                if self.screen == Screen::Term && !self.overlay_open() {
                    return iced::clipboard::read().map(Message::Paste);
                }
            }
            MenuAction::CheckUpdates => return self.update_check(true),
            MenuAction::Finder => return self.toggle_finder(),
            MenuAction::NewTab => self.new_local(),
            MenuAction::ClosePane => {
                if self.screen == Screen::Term {
                    self.close_focused_pane();
                }
            }
            MenuAction::CloseTab => self.close(self.active),
            MenuAction::Search => return self.open_find(),
            MenuAction::FindNext => self.find_step(true),
            MenuAction::FindPrev => self.find_step(false),
            MenuAction::PromptPrev | MenuAction::PromptNext => {
                if let Some(t) = self.active_tab() {
                    t.backend.jump_prompt(if a == MenuAction::PromptPrev { -1 } else { 1 });
                    t.cache.clear();
                }
            }
            MenuAction::SplitRight => self.split_focused(true),
            MenuAction::SplitDown => self.split_focused(false),
            MenuAction::SplitRightLocal => self.split_focused_local(true),
            MenuAction::SplitDownLocal => self.split_focused_local(false),
            MenuAction::FocusLeft => self.move_focus(D::Left),
            MenuAction::FocusRight => self.move_focus(D::Right),
            MenuAction::FocusUp => self.move_focus(D::Up),
            MenuAction::FocusDown => self.move_focus(D::Down),
            MenuAction::NextPane => self.cycle_focus(1),
            MenuAction::PrevPane => self.cycle_focus(-1),
            MenuAction::Zoom => self.toggle_zoom(),
            MenuAction::Broadcast => self.toggle_broadcast(),
            MenuAction::Snippets => return self.open_snippets(),
            MenuAction::NextTab | MenuAction::PrevTab => {
                if !self.tabs.is_empty() {
                    let n = self.tabs.len();
                    self.active = if a == MenuAction::NextTab { (self.active + 1) % n } else { (self.active + n - 1) % n };
                    self.screen = Screen::Term;
                }
            }
            MenuAction::Sidebar => self.sidebar = !self.sidebar,
            MenuAction::GoTab(n) => {
                if (n as usize) < self.tabs.len() {
                    self.active = n as usize;
                    self.screen = Screen::Term;
                }
            }
        }
        Task::none()
    }

    /// Como `split_focused`, pero el panel nuevo es un terminal local (sin repetir la sesión SSH).
    fn split_focused_local(&mut self, side_by_side: bool) {
        let shell = self.settings.resolved_shell();
        let spec = SpawnSpec {
            title: "Terminal".into(),
            program: Some(shell),
            args: if self.settings.login_shell { vec!["-l".into()] } else { vec![] },
            env: HashMap::new(),
            server: None,
            tab_env: None,
            remote: false,
        };
        let Some(pane) = self.build_pane(spec) else { return };
        let axis = if side_by_side { pane_grid::Axis::Vertical } else { pane_grid::Axis::Horizontal };
        if let Some(t) = self.tabs.get_mut(self.active) {
            if let Some((new, _)) = t.grid.split(axis, t.focus, pane) {
                t.focus = new;
            }
            t.clear_caches();
        }
    }

    /// Elige la primera cuenta si no hay ninguna seleccionada y deja el aviso inicial. No llama a AWS.
    fn aws_init(&mut self) {
        let valid = self.aws.account.is_some_and(|id| self.aws_store.items.iter().any(|a| a.id == id));
        if !valid {
            self.aws.account = self.aws_store.items.first().map(|a| a.id);
            self.aws.instances.clear();
        }
        if let Some(a) = self.aws.account.and_then(|id| self.aws_store.items.iter().find(|a| a.id == id)) {
            if self.aws.region.is_empty() && self.aws.instances.is_empty() {
                self.aws.region = a.region.clone();
            }
        }
        if self.aws.instances.is_empty() && !self.aws.loading {
            self.aws.is_error = false;
            self.aws.status = if self.aws.account.is_none() { "Agrega una cuenta de AWS con +.".into() } else { "Pulsa ↻ para listar las instancias.".into() };
        }
    }

    fn aws_account(&self) -> Option<AwsAccount> {
        self.aws.account.and_then(|id| self.aws_store.items.iter().find(|a| a.id == id)).cloned()
    }

    /// Lista las instancias (solo al pulsar ↻). Lee la clave secreta del Keychain.
    fn aws_refresh(&mut self) -> Task<Message> {
        let Some(acc) = self.aws_account() else {
            self.aws.status = "Agrega una cuenta de AWS con +.".into();
            return Task::none();
        };
        let Some(net) = self.aws_net.clone() else {
            self.aws.status = "No se pudo iniciar el cliente de red.".into();
            self.aws.is_error = true;
            return Task::none();
        };
        let creds = match awsstore::credentials(&acc, &*self.secrets) {
            Ok(c) => c,
            Err(e) => {
                self.aws.status = e.0;
                self.aws.is_error = true;
                return Task::none();
            }
        };
        let regions: Vec<String> = if self.aws.region.is_empty() { REGIONS.iter().map(|r| r.to_string()).collect() } else { vec![self.aws.region.clone()] };
        self.aws.generation += 1;
        self.aws.loading = true;
        self.aws.is_error = false;
        self.aws.status = "Cargando instancias…".into();
        let gen = self.aws.generation;
        Task::perform(async move { net.describe_instances_regions(&creds, &regions).await }, move |(found, errs)| Message::AwsLoaded(gen, found, errs))
    }

    fn aws_connect(&mut self, idx: usize) -> Task<Message> {
        let Some(i) = self.aws.instances.get(idx).cloned() else { return Task::none() };
        if i.state != "running" {
            return self.aws_fail(format!("La instancia no está en ejecución (estado: {}).", i.state));
        }
        if i.public_dns.is_empty() && i.public_ip.is_empty() {
            return self.aws_fail("Instancia sin IP pública: la conexión por SSM Session Manager aún no está disponible en esta versión.".into());
        }
        match self.aws_users.map.get(&i.id).cloned() {
            Some(user) => return self.aws_start_connect(i, user),
            None => {
                let user = if self.settings.aws_user.is_empty() { "ec2-user".to_string() } else { self.settings.aws_user.clone() };
                self.user_prompt = Some(UserPrompt { instance: i, user });
                iced::widget::operation::focus_next()
            }
        }
    }

    /// Envía la llave pública por Instance Connect (60 s de validez) y, al terminar, abre el SSH.
    fn aws_start_connect(&mut self, instance: Ec2Instance, user: String) -> Task<Message> {
        let Some(acc) = self.aws_account() else { return Task::none() };
        let Some(net) = self.aws_net.clone() else { return Task::none() };
        let creds = match awsstore::credentials(&acc, &*self.secrets) {
            Ok(c) => c,
            Err(e) => return self.aws_fail(e.0),
        };
        let key = match awsstore::ensure_eic_key(&self.data_dir) {
            Ok(k) => k,
            Err(e) => return self.aws_fail(e.0),
        };
        self.aws.status = format!("Conectando a {}…", instance.name);
        self.aws.is_error = false;
        let env = acc.env;
        Task::perform(
            async move {
                net.send_ssh_public_key(&creds, &instance.region, &instance.id, &instance.zone, &user, &key.public)
                    .await
                    .map_err(|e| e.0)?;
                Ok(ConnectInfo { options: awsstore::instance_ssh_options(&key, &instance.id), instance, user, env })
            },
            Message::AwsConnected,
        )
    }

    fn aws_fail(&mut self, msg: String) -> Task<Message> {
        self.aws.status = msg;
        self.aws.is_error = true;
        Task::none()
    }

    // MARK: CloudWatch Logs (las llamadas a AWS solo ocurren al pulsar los botones del visor)

    fn logs_open(&mut self) -> Task<Message> {
        let Some(acc) = self.aws_account() else {
            self.aws.status = "Agrega una cuenta de AWS con +.".into();
            self.aws.is_error = true;
            return Task::none();
        };
        let Some(net) = self.aws_net.clone() else {
            self.aws.status = "No se pudo iniciar el cliente de red.".into();
            self.aws.is_error = true;
            return Task::none();
        };
        let region = if self.aws.region.is_empty() { acc.region.clone() } else { self.aws.region.clone() };
        self.logs = Some(LogsState::new(acc, region, net));
        self.screen = Screen::Logs;
        Task::none()
    }

    /// Credenciales de la cuenta del visor (se leen del Keychain la primera vez).
    fn logs_creds(&mut self) -> Result<jterm_core::sigv4::Credentials, String> {
        let Some(st) = &mut self.logs else { return Err("Visor cerrado.".into()) };
        if st.creds.is_none() {
            st.creds = Some(awsstore::credentials(&st.account, &*self.secrets).map_err(|e| e.0)?);
        }
        Ok(st.creds.clone().unwrap())
    }

    fn logs_fail(&mut self, msg: String) -> Task<Message> {
        if let Some(st) = &mut self.logs {
            st.status = msg;
            st.is_error = true;
            st.searching = false;
            st.following = false;
            st.follow_busy = false;
            st.groups_loading = false;
        }
        Task::none()
    }

    fn logs_load_groups(&mut self) -> Task<Message> {
        let creds = match self.logs_creds() {
            Ok(c) => c,
            Err(e) => return self.logs_fail(e),
        };
        let Some(st) = &mut self.logs else { return Task::none() };
        if st.groups_loading {
            return Task::none();
        }
        st.groups_loading = true;
        st.status = "Cargando grupos de logs…".into();
        st.is_error = false;
        let (client, region) = (st.client.clone(), st.region.clone());
        let r2 = region.clone();
        Task::perform(async move { logs_ui::fetch_groups(&client, &creds, &region).await }, move |r| Message::LogsGroups(r2.clone(), r))
    }

    fn logs_search(&mut self) -> Task<Message> {
        if self.logs.as_ref().is_some_and(|s| s.group.is_none()) {
            if let Some(st) = &mut self.logs {
                st.status = "Primero elige un grupo de logs.".into();
                st.is_error = true;
                st.picker = Some(String::new());
            }
            return Task::none();
        }
        let creds = match self.logs_creds() {
            Ok(c) => c,
            Err(e) => return self.logs_fail(e),
        };
        let Some(st) = &mut self.logs else { return Task::none() };
        st.following = false;
        st.follow_busy = false;
        st.generation += 1;
        st.clear_results();
        st.searching = true;
        st.is_error = false;
        st.status = "Buscando…".into();
        st.at_bottom = true;
        st.follow_from = st.start_ms_for_range(logs_ui::now_ms());
        Self::logs_fetch_page(st, creds, None)
    }

    fn logs_fetch_page(st: &LogsState, creds: jterm_core::sigv4::Credentials, token: Option<String>) -> Task<Message> {
        let (client, region, group) = (st.client.clone(), st.region.clone(), st.group.clone().unwrap_or_default());
        let (start, pattern, prefix, gen) = (st.follow_from, st.filter.trim().to_string(), st.stream_prefix.trim().to_string(), st.generation);
        Task::perform(
            async move { logs_ui::fetch_page(&client, &creds, &region, &group, start, &pattern, &prefix, token.as_deref()).await },
            move |r| Message::LogsPage(gen, r),
        )
    }

    fn logs_snap(&self) -> Task<Message> {
        match &self.logs {
            Some(st) if st.at_bottom => iced::widget::operation::snap_to(logs_ui::SCROLL_ID, iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 }),
            _ => Task::none(),
        }
    }

    fn logs_page(&mut self, gen: u64, r: Result<(Vec<LogEvent>, Option<String>), String>) -> Task<Message> {
        match &self.logs {
            Some(st) if st.generation == gen => {}
            _ => return Task::none(),
        }
        let (events, token) = match r {
            Ok(v) => v,
            Err(e) => return self.logs_fail(e),
        };
        let creds = self.logs_creds().ok();
        let Some(st) = &mut self.logs else { return Task::none() };
        st.append(events);
        st.pages += 1;
        if let (Some(t), Some(c)) = (token.clone(), creds) {
            if st.event_count() < logs_ui::MAX_SEARCH_EVENTS && st.pages < 40 {
                st.status = format!("Buscando… {} eventos", st.event_count());
                return Task::batch([Self::logs_fetch_page(st, c, Some(t)), self.logs_snap()]);
            }
        }
        st.searching = false;
        let n = st.event_count();
        let more = if token.is_some() { " Hay más eventos: acota el rango o el filtro para verlos." } else { "" };
        st.status = if n == 0 {
            "Sin eventos en el rango y filtro elegidos.".into()
        } else {
            format!("{n} eventos en {} ({}).{more}", st.group.clone().unwrap_or_default(), st.region)
        };
        self.logs_snap()
    }

    fn logs_toggle_follow(&mut self) -> Task<Message> {
        let Some(st) = &mut self.logs else { return Task::none() };
        if st.following {
            st.following = false;
            st.follow_busy = false;
            st.generation += 1;
            return Task::none();
        }
        if st.group.is_none() {
            st.status = "Primero elige un grupo de logs.".into();
            st.is_error = true;
            st.picker = Some(String::new());
            return Task::none();
        }
        st.following = true;
        st.follow_from = st.parsed.last().map(|p| p.event.timestamp).unwrap_or_else(|| logs_ui::now_ms() - 60_000);
        st.last_poll = None;
        st.is_error = false;
        self.logs_poll()
    }

    /// Consulta los eventos nuevos si toca (cada pocos segundos). Se llama desde el temporizador.
    fn logs_poll(&mut self) -> Task<Message> {
        let due = self.logs.as_ref().is_some_and(|s| s.following && !s.follow_busy && s.last_poll.is_none_or(|t| t.elapsed().as_secs() >= logs_ui::FOLLOW_SECS));
        if !due {
            return Task::none();
        }
        let creds = match self.logs_creds() {
            Ok(c) => c,
            Err(e) => return self.logs_fail(e),
        };
        let Some(st) = &mut self.logs else { return Task::none() };
        st.follow_busy = true;
        st.last_poll = Some(Instant::now());
        let (client, region, group) = (st.client.clone(), st.region.clone(), st.group.clone().unwrap_or_default());
        let (start, pattern, prefix, gen) = (st.follow_from, st.filter.trim().to_string(), st.stream_prefix.trim().to_string(), st.generation);
        Task::perform(
            async move { logs_ui::fetch_follow(&client, &creds, &region, &group, start, &pattern, &prefix).await },
            move |r| Message::LogsFollowed(gen, r),
        )
    }

    fn logs_followed(&mut self, gen: u64, r: Result<Vec<LogEvent>, String>) -> Task<Message> {
        match &self.logs {
            Some(st) if st.following && st.generation == gen => {}
            _ => return Task::none(),
        }
        match r {
            Err(e) => self.logs_fail(e),
            Ok(events) => {
                let Some(st) = &mut self.logs else { return Task::none() };
                st.follow_busy = false;
                if let Some(last) = events.iter().map(|e| e.timestamp).max() {
                    st.follow_from = st.follow_from.max(last);
                }
                let added = st.append(events);
                let now = logs_ui::format_time(logs_ui::now_ms(), false);
                st.status = format!("Siguiendo · {} eventos (+{added}) · {}", st.event_count(), &now[..8.min(now.len())]);
                self.logs_snap()
            }
        }
    }

    // MARK: S3 (las llamadas a AWS solo ocurren al pulsar los botones del explorador)

    fn s3_open(&mut self) -> Task<Message> {
        let Some(acc) = self.aws_account() else {
            self.aws.status = "Agrega una cuenta de AWS con +.".into();
            self.aws.is_error = true;
            return Task::none();
        };
        let Ok(transport) = ReqwestTransport::new() else {
            self.aws.status = "No se pudo iniciar el cliente de red.".into();
            self.aws.is_error = true;
            return Task::none();
        };
        let secrets = self.secrets.clone();
        let acc2 = acc.clone();
        // Las credenciales se leen del Keychain solo cuando una petición las necesita.
        let client = Arc::new(S3Client::new(transport, &acc.region, move || awsstore::credentials(&acc2, &*secrets)));
        self.s3 = Some(S3State::new(acc, client));
        self.screen = Screen::S3;
        Task::none()
    }

    fn s3_refresh(&mut self) -> Task<Message> {
        let Some(st) = &mut self.s3 else { return Task::none() };
        st.generation += 1;
        st.loading = true;
        st.is_error = false;
        st.status = "Cargando…".into();
        let (gen, client) = (st.generation, st.client.clone());
        match st.bucket.clone() {
            None => Task::perform(async move { client.buckets().await.map_err(|e| e.0) }, move |r| Message::S3Buckets(gen, r)),
            Some(b) => {
                let prefix = st.prefix.clone();
                Task::perform(async move { client.list(&b, &prefix, false).await.map_err(|e| e.0) }, move |r| Message::S3Listed(gen, r))
            }
        }
    }

    fn s3_upload(&mut self, files: Vec<PathBuf>) -> Task<Message> {
        let Some(st) = &mut self.s3 else { return Task::none() };
        let Some(bucket) = st.bucket.clone() else { return Task::none() };
        if files.is_empty() {
            return Task::none();
        }
        let files: Vec<PathBuf> = files.into_iter().filter(|f| f.is_file()).collect();
        if files.is_empty() {
            st.status = "Solo se pueden subir archivos (no carpetas).".into();
            st.is_error = true;
            return Task::none();
        }
        let total: u64 = files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
        let name = if files.len() == 1 { format!("Subida de {}", files[0].file_name().unwrap_or_default().to_string_lossy()) } else { format!("Subida de {} archivos", files.len()) };
        let (id, done, _) = st.new_transfer(name, total);
        let (client, prefix) = (st.client.clone(), st.prefix.clone());
        Task::perform(async move { s3_ui::upload_files(client, bucket, prefix, files, done).await.map(|_| String::new()) }, move |r| Message::S3TransferDone(id, r))
    }

    fn s3_download(&mut self, dest: Option<PathBuf>) -> Task<Message> {
        let (Some(dest), Some(st)) = (dest, &mut self.s3) else { return Task::none() };
        let (Some(bucket), Some(entry)) = (st.bucket.clone(), st.selected_entry().cloned()) else { return Task::none() };
        let (id, done, total) = st.new_transfer(format!("Descarga de {}", entry.name), 0);
        let client = st.client.clone();
        let strip = s3_ui::parent(&entry.key);
        Task::perform(
            async move {
                let items = s3_ui::expand_keys(&client, &bucket, &entry).await.map_err(|e| e.0)?;
                total.store(items.iter().map(|i| i.1).sum(), std::sync::atomic::Ordering::Relaxed);
                let n = s3_ui::download_items(client, bucket, items, strip, dest, done).await?;
                Ok(format!("{n} archivo(s)"))
            },
            move |r| Message::S3TransferDone(id, r),
        )
    }

    fn s3_presign(&mut self) -> Task<Message> {
        let Some(st) = &mut self.s3 else { return Task::none() };
        let (Some(bucket), Some(e)) = (st.bucket.clone(), st.selected_entry().cloned()) else { return Task::none() };
        if e.is_dir {
            return Task::none();
        }
        let client = st.client.clone();
        Task::perform(async move { client.presigned_url(&bucket, &e.key, 3600).await.map_err(|e| e.0) }, Message::S3Presigned)
    }

    fn s3_create_folder(&mut self) -> Task<Message> {
        let Some(st) = &mut self.s3 else { return Task::none() };
        let Some(name) = st.new_folder.clone() else { return Task::none() };
        if !s3_ui::valid_folder_name(&name) {
            st.status = "El nombre de la carpeta no puede estar vacío ni contener «/».".into();
            st.is_error = true;
            return Task::none();
        }
        st.new_folder = None;
        let Some(bucket) = st.bucket.clone() else { return Task::none() };
        let (client, key) = (st.client.clone(), format!("{}{}", st.prefix, name.trim()));
        Task::perform(async move { client.create_folder(&bucket, &key).await.map_err(|e| e.0) }, Message::S3FolderDone)
    }

    fn s3_delete(&mut self) -> Task<Message> {
        let Some(st) = &mut self.s3 else { return Task::none() };
        let (Some(items), Some(bucket)) = (st.confirm_delete.take(), st.bucket.clone()) else { return Task::none() };
        st.status = "Eliminando…".into();
        let client = st.client.clone();
        Task::perform(
            async move {
                let keys = s3_ui::delete_keys(&client, &bucket, &items).await.map_err(|e| e.0)?;
                client.delete(&bucket, &keys).await.map_err(|e| e.0)?;
                Ok(keys.len())
            },
            Message::S3Deleted,
        )
    }

    // MARK: Buscador rápido

    fn toggle_finder(&mut self) -> Task<Message> {
        if self.finder.take().is_some() {
            return Task::none();
        }
        self.finder = Some(FinderState::new(self.finder_items()));
        iced::widget::operation::focus(finder::INPUT_ID)
    }

    /// Todo lo que se puede abrir desde el buscador: pestañas, servidores, instancias cargadas, comandos y acciones.
    fn finder_items(&self) -> Vec<FinderItem> {
        let mut v = Vec::new();
        for (i, t) in self.tabs.iter().enumerate() {
            let n = t.grid.len();
            v.push(FinderItem { pin: None, pinned: false,
                kind: FinderKind::Tab,
                title: t.title.clone(),
                subtitle: if n > 1 { format!("Pestaña abierta · {n} paneles") } else { "Pestaña abierta".into() },
                env: t.env,
                keywords: if t.remote { "ssh sesion".into() } else { "local terminal".into() },
                target: FinderTarget::Tab(i),
                icon: None,
            });
        }
        // Vistas de AWS y archivos abiertas como pestaña.
        let mut screens: Vec<(String, ui::Icon, Screen)> = vec![];
        if let Some(l) = &self.logs {
            screens.push((format!("Logs · {}", if l.account.name.is_empty() { "AWS" } else { &l.account.name }), ui::Icon::ScrollText, Screen::Logs));
        }
        if let Some(s) = &self.s3 {
            screens.push((format!("S3 · {}", if s.account.name.is_empty() { "AWS" } else { &s.account.name }), ui::Icon::Database, Screen::S3));
        }
        if let Some(f) = &self.files {
            screens.push((format!("Archivos · {}", f.title), ui::Icon::FolderOpen, Screen::Files));
        }
        for (title, icon, sc) in screens {
            v.push(FinderItem { pin: None, pinned: false, kind: FinderKind::Tab, title, subtitle: "Pestaña abierta".into(), env: None, keywords: String::new(), target: FinderTarget::Screen(sc), icon: Some(icon) });
        }
        for s in &self.servers.items {
            v.push(FinderItem { pin: None, pinned: false,
                kind: FinderKind::Server,
                title: if s.name.is_empty() { s.host.clone() } else { s.name.clone() },
                subtitle: s.destination(),
                env: s.env,
                keywords: s.host.clone(),
                target: FinderTarget::Server(s.id),
                icon: Some(if s.auth == AuthMethod::Key { ui::Icon::KeyRound } else { ui::Icon::Lock }),
            });
        }
        let aws_env = self.aws_account().and_then(|a| a.env);
        for (i, inst) in self.aws.instances.iter().enumerate() {
            v.push(FinderItem { pin: None, pinned: false,
                kind: FinderKind::Instance,
                title: inst.name.clone(),
                subtitle: format!("{} · {} · {}", inst.kind, inst.region, aws_ui::ip_label(inst)),
                env: aws_env,
                keywords: format!("{} {} {}", inst.id, inst.state, inst.public_ip),
                target: FinderTarget::Instance(i),
                icon: Some(ui::Icon::Cloud),
            });
        }
        for c in &self.snippets.items {
            v.push(FinderItem { pin: None, pinned: false,
                kind: FinderKind::Snippet,
                title: c.title(),
                subtitle: c.first_line(),
                env: None,
                keywords: c.command.clone(),
                target: FinderTarget::Snippet(c.id),
                icon: None,
            });
        }
        for (title, a) in [
            ("Nueva pestaña", MenuAction::NewTab),
            ("Dividir a la derecha", MenuAction::SplitRight),
            ("Dividir hacia abajo", MenuAction::SplitDown),
            ("Buscar en el terminal", MenuAction::Search),
            ("Mostrar comandos guardados", MenuAction::Snippets),
            ("Mostrar u ocultar la barra lateral", MenuAction::Sidebar),
            ("Ajustes", MenuAction::Settings),
            ("Buscar actualizaciones", MenuAction::CheckUpdates),
        ] {
            v.push(FinderItem { pin: None, pinned: false, kind: FinderKind::Action, title: tr(title), subtitle: String::new(), env: None, keywords: String::new(), target: FinderTarget::Action(a), icon: None });
        }
        // Claves estables para fijar (servidores, instancias, comandos y acciones) y estado de fijado.
        for it in &mut v {
            it.pin = match it.target {
                FinderTarget::Server(id) => Some(format!("server:{id}")),
                FinderTarget::Snippet(id) => Some(format!("snippet:{id}")),
                FinderTarget::Instance(i) => self.aws.instances.get(i).map(|x| format!("ec2:{}", x.id)),
                FinderTarget::Action(a) => Some(format!("action:{a:?}")),
                _ => None,
            };
            it.pinned = it.pin.as_ref().is_some_and(|k| self.settings.finder_pins.contains(k));
        }
        v
    }

    /// Fija o desfija el resultado `pos` y recalcula la lista sin perder la consulta ni la selección.
    fn toggle_finder_pin(&mut self, pos: usize) {
        let Some(f) = &self.finder else { return };
        let Some(key) = f.shown.get(pos).and_then(|&i| f.items[i].pin.clone()) else { return };
        let query = f.query.clone();
        if let Some(i) = self.settings.finder_pins.iter().position(|k| *k == key) {
            self.settings.finder_pins.remove(i);
        } else {
            self.settings.finder_pins.push(key.clone());
        }
        self.save_settings();
        let mut st = FinderState::new(self.finder_items());
        st.set_query(query);
        if let Some(p) = st.position_of(&key) {
            st.selected = p;
        }
        self.finder = Some(st);
    }

    /// Abre el resultado `pos` (índice en la lista filtrada). Con ⌥, un comando guardado se pega Y se ejecuta.
    fn run_finder(&mut self, pos: usize, alt: bool) -> Task<Message> {
        let Some(f) = self.finder.take() else { return Task::none() };
        let Some(&idx) = f.shown.get(pos) else { return Task::none() };
        match f.items[idx].target {
            FinderTarget::Tab(i) => {
                if i < self.tabs.len() {
                    self.active = i;
                    self.screen = Screen::Term;
                }
            }
            FinderTarget::Server(id) => {
                self.screen = Screen::Term;
                self.connect(id);
            }
            FinderTarget::Instance(i) => {
                self.side = SideMode::Aws;
                return self.aws_connect(i);
            }
            FinderTarget::Snippet(id) => {
                let Some(s) = self.snippets.items.iter().find(|s| s.id == id).cloned() else { return Task::none() };
                self.screen = Screen::Term;
                if let Some(t) = self.active_tab() {
                    t.backend.paste(&s.command);
                    t.cache.clear();
                }
                if (alt || s.auto_run) && !self.guard_enter() {
                    if let Some(t) = self.active_tab() {
                        t.backend.mark_prompt();
                        t.backend.write(&b"\r"[..]);
                    }
                }
            }
            FinderTarget::Screen(sc) => self.screen = sc,
            FinderTarget::Action(a) => return self.menu_action(a),
        }
        Task::none()
    }

    // MARK: Actualizador

    fn current_version(&self) -> String {
        self.bundle.as_ref().map(|b| b.version.clone()).unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
    }

    fn update_state_path(&self) -> PathBuf {
        self.data_dir.join("update.json")
    }

    fn last_update_check(&self) -> u64 {
        std::fs::read(self.update_state_path()).ok().and_then(|d| serde_json::from_slice::<serde_json::Value>(&d).ok()).and_then(|v| v["lastCheck"].as_u64()).unwrap_or(0)
    }

    fn save_last_update_check(&self) {
        let _ = store::write_atomic(&self.update_state_path(), serde_json::json!({ "lastCheck": now_unix() }).to_string().as_bytes());
    }

    /// Consulta el último release de GitHub. Solo se llama al abrir (una vez al día) o desde el menú.
    fn update_check(&mut self, manual: bool) -> Task<Message> {
        if self.update_busy {
            return Task::none();
        }
        let Some(repo) = self.bundle.as_ref().and_then(|b| b.repo.clone()) else {
            if manual {
                self.update_dialog = Some(UpdateDialog::Info {
                    title: "Las actualizaciones no están disponibles".into(),
                    text: "Esta compilación no tiene configurado el repositorio de GitHub (ejecútala desde la app empaquetada con build_app.sh).".into(),
                });
            }
            return Task::none();
        };
        let current = self.current_version();
        self.update_busy = true;
        Task::perform(
            async move {
                let f = GithubFetcher::new()?;
                updater::check(&f, &repo, &current).await
            },
            move |r| Message::UpdateChecked(manual, r),
        )
    }

    /// Descarga, verifica y prepara la versión nueva; al terminar se reemplaza la app y se reabre.
    fn update_install(&mut self) -> Task<Message> {
        let (Some(rel), Some(bundle)) = (self.update_found.clone(), self.bundle.clone()) else { return Task::none() };
        if !updater::can_write_next_to(&bundle.path) {
            // Sin permiso para modificar la carpeta (p. ej. instalada por otro usuario): descarga manual del instalador.
            let _ = std::process::Command::new("open").arg(&rel.dmg).spawn();
            self.update_dialog = Some(UpdateDialog::Info {
                title: "No se pudo actualizar".into(),
                text: format!("No tengo permiso para modificar {}. Se descargará el instalador para que lo arrastres tú.", bundle.path.parent().map(|p| p.display().to_string()).unwrap_or_default()),
            });
            return Task::none();
        }
        let progress = Arc::new(std::sync::Mutex::new(format!("Descargando jTerm {}…", rel.version)));
        self.update_dialog = Some(UpdateDialog::Installing { progress: progress.clone() });
        self.update_busy = true;
        Task::perform(
            async move {
                let f = GithubFetcher::new()?;
                let p2 = progress.clone();
                updater::prepare(&f, &rel, &bundle.path, &bundle.identifier, &move |t: &str| *p2.lock().unwrap() = t.to_string()).await
            },
            Message::UpdatePrepared,
        )
    }

    // MARK: Sincronización con iCloud

    fn save_sync_prefs(&self) {
        if let Ok(d) = serde_json::to_vec_pretty(&self.sync.prefs) {
            let _ = store::write_atomic(&self.data_dir.join("sync-prefs.json"), &d);
        }
    }

    fn sync_state_path(&self) -> PathBuf {
        self.data_dir.join("sync-state.json")
    }

    fn load_sync_state(&self) -> SyncState {
        std::fs::read(self.sync_state_path()).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
    }

    /// La contraseña se lee del Keychain la primera vez que hace falta.
    fn sync_passphrase(&mut self) -> Option<String> {
        if !self.sync.pass_loaded {
            self.sync.passphrase = self.secrets.get(sync_ui::PASSPHRASE_ID).filter(|p| !p.is_empty());
            self.sync.pass_loaded = true;
        }
        self.sync.passphrase.clone()
    }

    fn sync_enable_dialog(&mut self) {
        let Some(folder) = sync_ui::sync_folder() else {
            self.sync.last_error = Some("No se encontró la carpeta de iCloud Drive.".into());
            return;
        };
        let has_data = SyncStorage::new(folder).has_data();
        self.sync.dialog = Some(SyncDialog::new(DialogKind::Enable { has_data }));
    }

    /// Una sincronización en segundo plano. Los datos se leen y escriben en los archivos; al terminar se recargan las listas.
    fn sync_now(&mut self) -> Task<Message> {
        if !self.sync.prefs.enabled {
            return Task::none();
        }
        if self.sync.syncing {
            self.sync.pending_rerun = true;
            return Task::none();
        }
        self.sync.due = None;
        self.sync.last_periodic = Instant::now();
        let Some(folder) = sync_ui::sync_folder() else {
            self.sync.last_error = Some("No se encontró la carpeta de iCloud Drive.".into());
            return Task::none();
        };
        let Some(pass) = self.sync_passphrase() else {
            self.sync.needs_pass = true;
            self.sync.last_error = Some("Falta la contraseña de sincronización.".into());
            return Task::none();
        };
        self.sync.syncing = true;
        let (state, kinds, secrets_on) = (self.load_sync_state(), self.sync.prefs.kinds(), self.sync.prefs.secrets);
        let mut adapter = FileAdapter::new(self.data_dir.clone(), self.secrets.clone());
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    jterm_core::sync::run(&mut adapter, &SyncStorage::new(folder), &pass, &state, &kinds, secrets_on, chrono::Utc::now())
                        .map_err(|e| (e.to_string(), matches!(e, jterm_core::sync::SyncError::BadPassphrase)))
                })
                .await
                .map_err(|e| (e.to_string(), false))?
            },
            Message::SyncDone,
        )
    }

    fn sync_done(&mut self, r: Result<(SyncResult, SyncState), (String, bool)>) {
        self.sync.syncing = false;
        match r {
            Ok((res, state)) => {
                if let Ok(d) = serde_json::to_vec(&state) {
                    let _ = store::write_atomic(&self.sync_state_path(), &d);
                }
                self.sync.last_sync = Some(chrono::Utc::now());
                self.sync.last_error = None;
                self.sync.needs_pass = false;
                if res.downloaded > 0 || res.deleted > 0 {
                    self.reload_data_from_disk();
                }
            }
            Err((msg, bad_pass)) => {
                self.sync.last_error = Some(msg);
                self.sync.needs_pass = bad_pass;
            }
        }
        if std::mem::take(&mut self.sync.pending_rerun) {
            self.sync.due = Some(Instant::now() + Duration::from_secs(1));
        }
    }

    /// Recarga lo que la sincronización pudo cambiar en los archivos (sin volver a guardar ni a sincronizar).
    fn reload_data_from_disk(&mut self) {
        self.servers = JsonList::open(self.data_dir.join("servers.json"));
        self.aws_store = AwsStore::open(self.data_dir.join("aws.json"));
        self.snippets = JsonList::open(self.data_dir.join("commands.json"));
        self.settings = load_settings(&self.data_dir);
        self.palette = Palette::from_theme(themes::theme(&self.settings.theme));
        self.metrics = Metrics::for_settings(&self.settings.font_family, self.settings.font_size as f32, self.settings.font_medium);
        for t in &self.tabs {
            for p in t.panes() {
                p.backend.set_underline_links(self.settings.underline_links);
                p.cache.clear();
            }
        }
    }

    fn sync_dialog_ok(&mut self) -> Task<Message> {
        let Some(d) = &mut self.sync.dialog else { return Task::none() };
        if let Err(e) = d.check() {
            d.error = Some(e);
            return Task::none();
        }
        let kind = d.kind;
        let pass = d.pass1.clone();
        let storage = sync_ui::sync_folder().map(SyncStorage::new);
        let fail = |this: &mut Self, m: String| {
            if let Some(d) = &mut this.sync.dialog {
                d.error = Some(m);
            }
        };
        match kind {
            DialogKind::Enable { has_data } => {
                let Some(storage) = storage else {
                    fail(self, "No se encontró la carpeta de iCloud Drive.".into());
                    return Task::none();
                };
                if has_data {
                    if let Err(e) = storage.verify(&pass) {
                        fail(self, e.to_string());
                        return Task::none();
                    }
                }
                self.secrets.set(sync_ui::PASSPHRASE_ID, &pass);
                self.sync.passphrase = Some(pass);
                self.sync.pass_loaded = true;
                let _ = std::fs::remove_file(self.sync_state_path());
                self.sync.prefs.enabled = true;
                self.sync.needs_pass = false;
                self.sync.last_error = None;
                self.save_sync_prefs();
                self.sync.dialog = None;
                self.sync_now()
            }
            DialogKind::Reenter => {
                let Some(storage) = storage else {
                    fail(self, "No se encontró la carpeta de iCloud Drive.".into());
                    return Task::none();
                };
                if let Err(e) = storage.verify(&pass) {
                    fail(self, e.to_string());
                    return Task::none();
                }
                self.secrets.set(sync_ui::PASSPHRASE_ID, &pass);
                self.sync.passphrase = Some(pass);
                self.sync.pass_loaded = true;
                self.sync.needs_pass = false;
                self.sync.last_error = None;
                self.sync.dialog = None;
                self.sync_now()
            }
            DialogKind::Change => {
                let (Some(storage), Some(old)) = (storage, self.sync_passphrase()) else {
                    fail(self, "No hay una sincronización activa que cambiar.".into());
                    return Task::none();
                };
                let result = storage.read(&old).and_then(|(doc, _)| match doc {
                    Some(doc) => storage.write(&doc, &pass, &[]),
                    None => Ok(()),
                });
                if let Err(e) = result {
                    fail(self, e.to_string());
                    return Task::none();
                }
                self.secrets.set(sync_ui::PASSPHRASE_ID, &pass);
                self.sync.passphrase = Some(pass);
                self.sync.needs_pass = false;
                self.sync.last_error = None;
                self.sync.dialog = None;
                Task::none()
            }
            DialogKind::ConfirmSecrets => {
                self.sync.prefs.secrets = true;
                self.save_sync_prefs();
                self.sync.dialog = None;
                self.sync.due = Some(Instant::now() + Duration::from_secs(1));
                Task::none()
            }
            DialogKind::ConfirmDisable | DialogKind::ConfirmDelete => {
                if kind == DialogKind::ConfirmDelete {
                    if let Some(st) = storage {
                        st.delete();
                    }
                }
                self.sync_disable();
                Task::none()
            }
        }
    }

    /// Desactiva la sincronización en este Mac (los datos locales no se tocan).
    fn sync_disable(&mut self) {
        self.sync.prefs.enabled = false;
        self.sync.prefs.secrets = false;
        self.secrets.delete(sync_ui::PASSPHRASE_ID);
        self.sync.passphrase = None;
        self.sync.pass_loaded = true;
        let _ = std::fs::remove_file(self.sync_state_path());
        self.sync.last_error = None;
        self.sync.needs_pass = false;
        self.sync.last_sync = None;
        self.sync.due = None;
        self.sync.dialog = None;
        self.save_sync_prefs();
    }

    // MARK: Archivos (SFTP): la conexión solo se abre al pulsar el botón de carpeta de un servidor

    fn files_open(&mut self, id: Uuid) -> Task<Message> {
        let Some(server) = self.servers.items.iter().find(|s| s.id == id).cloned() else { return Task::none() };
        let askpass = match sftp::ensure_askpass(&self.data_dir) {
            Ok(p) => p.display().to_string(),
            Err(e) => {
                self.error = Some(format!("No se pudo preparar askpass: {e}"));
                return Task::none();
            }
        };
        let target = sftp::target_for_server(&server, &self.servers.items, &self.settings, &*self.secrets, &askpass, "/usr/bin/ssh");
        let start = {
            let p = jterm_core::ssh_jump::expand_tilde(&self.settings.files_local_start);
            if !self.settings.files_local_start.is_empty() && Path::new(&p).is_dir() { PathBuf::from(p) } else { PathBuf::from(std::env::var_os("HOME").unwrap_or_default()) }
        };
        let mut st = FilesState::new(target.title.clone(), start, self.settings.files_local_hidden, self.settings.files_remote_hidden);
        st.remote_start = target.start_dir.clone();
        self.files_load_local_into(&mut st);
        self.files = Some(st);
        self.screen = Screen::Files;
        let settings = self.settings.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let mut s = sftp::SftpSession::new(target, settings);
                    s.connect().map(|_| Session(Arc::new(s))).map_err(|e| e.0)
                })
                .await
                .map_err(|e| e.to_string())?
            },
            Message::FilesConnected,
        )
    }

    fn files_load_local_into(&self, f: &mut FilesState) {
        match files_ui::list_local(Path::new(&f.local.path), f.local.hidden) {
            Ok(v) => {
                f.local.entries = v;
                f.local.error = None;
            }
            Err(e) => {
                f.local.entries.clear();
                f.local.error = Some(e);
            }
        }
    }

    fn files_connected(&mut self, r: Result<Session, String>) -> Task<Message> {
        let Some(f) = &mut self.files else { return Task::none() };
        f.connecting = false;
        match r {
            Ok(sess) => {
                f.remote.path = files_ui::resolve_remote_start(&sess.0.home, f.remote_start.as_deref());
                f.session = Some(sess);
                f.is_error = false;
                f.status = "Conectado.".into();
                self.files_reload(PaneId::Remote)
            }
            Err(e) => {
                f.status = e;
                f.is_error = true;
                Task::none()
            }
        }
    }

    /// Vuelve a leer un panel (el local, al instante; el remoto, en segundo plano).
    fn files_reload(&mut self, id: PaneId) -> Task<Message> {
        let Some(mut f) = self.files.take() else { return Task::none() };
        let task = match id {
            PaneId::Local => {
                self.files_load_local_into(&mut f);
                if f.local.error.is_none() {
                    f.status = format!("{} · {} elementos", f.local.path, f.local.entries.len());
                    f.is_error = false;
                }
                Task::none()
            }
            PaneId::Remote => match f.session.clone() {
                None => Task::none(),
                Some(sess) => {
                    f.remote.loading = true;
                    f.remote.generation += 1;
                    let (gen, path) = (f.remote.generation, f.remote.path.clone());
                    Task::perform(
                        async move { tokio::task::spawn_blocking(move || sess.0.list(&path).map_err(|e| e.0)).await.map_err(|e| e.to_string())? },
                        move |r| Message::FilesLoaded(gen, r),
                    )
                }
            },
        };
        self.files = Some(f);
        task
    }

    fn files_transfer(&mut self, dir: Direction) -> Task<Message> {
        let Some(f) = &mut self.files else { return Task::none() };
        let Some(sess) = f.session.clone() else { return Task::none() };
        let (src_pane, dst_pane) = if dir == Direction::Upload { (PaneId::Local, PaneId::Remote) } else { (PaneId::Remote, PaneId::Local) };
        let Some(entry) = f.pane(src_pane).selected_entry().cloned() else { return Task::none() };
        let src_path = f.pane(src_pane).path.clone();
        let source = if src_pane == PaneId::Local { Path::new(&src_path).join(&entry.name).display().to_string() } else { files_ui::join_remote(&src_path, &entry.name) };
        let dest = f.pane(dst_pane).path.clone();
        let title = format!("{} {}", if dir == Direction::Upload { "Subiendo" } else { "Descargando" }, entry.name);
        let (id, job, percent, label) = f.new_transfer(title, dst_pane);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    sess.0.run_transfer(&job, dir, &[source], &dest, |pct, text| {
                        percent.store(pct, std::sync::atomic::Ordering::Relaxed);
                        *label.lock().unwrap() = text.to_string();
                    })
                    .map_err(|e| e.0)
                })
                .await
                .map_err(|e| e.to_string())?
            },
            move |r| Message::FilesTransferDone(id, r),
        )
    }

    fn files_delete(&mut self) -> Task<Message> {
        let Some(f) = &mut self.files else { return Task::none() };
        let Some((pane, entry)) = f.confirm_delete.take() else { return Task::none() };
        let path = f.pane(pane).path.clone();
        match pane {
            PaneId::Local => {
                let full = Path::new(&path).join(&entry.name);
                Task::perform(async move { trash::delete(&full).map_err(|e| e.to_string()) }, move |r| Message::FilesOpDone(pane, r))
            }
            PaneId::Remote => {
                let Some(sess) = f.session.clone() else { return Task::none() };
                let full = files_ui::join_remote(&path, &entry.name);
                Task::perform(
                    async move { tokio::task::spawn_blocking(move || sess.0.remove_remote(&[full]).map_err(|e| e.0)).await.map_err(|e| e.to_string())? },
                    move |r| Message::FilesOpDone(pane, r),
                )
            }
        }
    }

    fn files_name_ok(&mut self) -> Task<Message> {
        let Some(f) = &mut self.files else { return Task::none() };
        let Some(d) = f.name_dialog.take() else { return Task::none() };
        let name = d.text.trim().to_string();
        if !files_ui::valid_name(&name) {
            f.status = "El nombre no puede estar vacío ni contener «/».".into();
            f.is_error = true;
            return Task::none();
        }
        let base = f.pane(d.pane).path.clone();
        if f.pane(d.pane).entries.iter().any(|e| e.name == name && d.kind != NameKind::Rename(name.clone())) {
            f.status = format!("Ya existe “{name}”. Elige otro nombre.");
            f.is_error = true;
            return Task::none();
        }
        let pane = d.pane;
        match (pane, d.kind) {
            (PaneId::Local, kind) => {
                let dir = PathBuf::from(&base);
                let r = match kind {
                    NameKind::NewFolder => std::fs::create_dir(dir.join(&name)),
                    NameKind::Rename(old) => std::fs::rename(dir.join(old), dir.join(&name)),
                };
                if let Err(e) = r {
                    f.status = e.to_string();
                    f.is_error = true;
                }
                self.files_reload(PaneId::Local)
            }
            (PaneId::Remote, kind) => {
                let Some(sess) = f.session.clone() else { return Task::none() };
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || match kind {
                            NameKind::NewFolder => sess.0.mkdir(&files_ui::join_remote(&base, &name)).map_err(|e| e.0),
                            NameKind::Rename(old) => sess.0.rename(&files_ui::join_remote(&base, &old), &files_ui::join_remote(&base, &name)).map_err(|e| e.0),
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    move |r| Message::FilesOpDone(PaneId::Remote, r),
                )
            }
        }
    }

    /// Inicia o detiene un túnel. Iniciar lee el Keychain (puede mostrar su diálogo).
    fn tunnel_toggle(&mut self, id: Uuid) {
        if self.tunnel_mgr.state(id).is_running() {
            self.tunnel_mgr.stop(id);
            return;
        }
        let Some(t) = self.tunnels.items.iter().find(|t| t.id == id).cloned() else { return };
        let Some(server) = t.server_id.and_then(|sid| self.servers.items.iter().find(|s| s.id == sid)).cloned() else {
            self.error = Some("El túnel no tiene un servidor válido: edítalo y elige uno.".into());
            return;
        };
        let askpass = match sftp::ensure_askpass(&self.data_dir) {
            Ok(p) => p.display().to_string(),
            Err(e) => {
                self.error = Some(format!("No se pudo preparar askpass: {e}"));
                return;
            }
        };
        let target = sftp::target_for_server(&server, &self.servers.items, &self.settings, &*self.secrets, &askpass, &self.tunnel_mgr.ssh_path);
        self.tunnel_mgr.start(&t, &target, &self.settings);
    }

    fn open_find(&mut self) -> Task<Message> {
        let query = self.find.as_ref().map(|f| f.query.clone()).unwrap_or_default();
        let info = match self.active_tab() {
            Some(t) if !query.is_empty() => t.backend.set_search(&query),
            _ => (0, 0),
        };
        self.find = Some(FindState { query, info });
        iced::widget::operation::focus("find-input")
    }

    fn close_find(&mut self) {
        self.find = None;
        if let Some(t) = self.active_tab() {
            t.backend.clear_search();
            t.cache.clear();
        }
    }

    fn find_step(&mut self, forward: bool) {
        let Some(t) = self.tabs.get(self.active) else { return };
        let info = t.backend.search_step(forward);
        t.cache.clear();
        if let Some(f) = &mut self.find {
            f.info = info;
        }
    }

    fn open_snippets(&mut self) -> Task<Message> {
        self.panel = Some(Panel::Snippets(SnippetState::default()));
        iced::widget::operation::focus(panels::SEARCH_ID)
    }

    fn save_settings(&mut self) {
        self.sync.touch();
        if let Ok(data) = serde_json::to_vec_pretty(&self.settings) {
            if let Err(e) = store::write_atomic(&self.data_dir.join("settings.json"), &data) {
                eprintln!("jTerm: no se pudo guardar settings.json: {e}");
            }
        }
    }

    fn apply_settings(&mut self) {
        self.palette = Palette::from_theme(themes::theme(&self.settings.theme));
        self.metrics = Metrics::for_settings(&self.settings.font_family, self.settings.font_size as f32, self.settings.font_medium);
        for t in &self.tabs {
            for p in t.panes() {
                p.backend.set_underline_links(self.settings.underline_links);
                p.cache.clear();
            }
        }
        self.save_settings();
    }

    fn persist_servers(&mut self) {
        self.sync.touch();
        if let Err(e) = self.servers.save() {
            self.error = Some(format!("No se pudo guardar servers.json: {e}"));
        }
    }

    fn save_editor(&mut self) {
        let Some(ed) = &mut self.editor else { return };
        match ed.build() {
            Err(msg) => ed.error = Some(msg),
            Ok((server, secret)) => {
                let id = server.id;
                match self.servers.items.iter().position(|s| s.id == id) {
                    Some(i) => self.servers.items[i] = server,
                    None => self.servers.items.push(server),
                }
                if let Some(pw) = secret {
                    self.secrets.set(id, &pw);
                }
                self.editor = None;
                self.persist_servers();
            }
        }
    }

    fn tab_mut(&mut self, id: u64) -> Option<&mut Pane> {
        self.tabs.iter_mut().flat_map(|t| t.grid.iter_mut().map(|(_, p)| p)).find(|p| p.id == id)
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    fn handle_key(&mut self, e: keyboard::Event) -> Task<Message> {
        let keyboard::Event::KeyPressed { key, modified_key, modifiers, text, .. } = e else { return Task::none() };

        // Con el explorador de S3 abierto, el teclado no va al terminal (salvo los atajos con ⌘ y los diálogos).
        if self.screen != Screen::Term && !modifiers.logo() && self.editor.is_none() && self.danger.is_none() && self.panel.is_none() && self.aws_editor.is_none() && self.user_prompt.is_none() && self.tunnel_editor.is_none() && self.sync.dialog.is_none() && self.update_dialog.is_none() {
            if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                if let Some(s) = &mut self.s3 {
                    s.confirm_delete = None;
                    s.new_folder = None;
                }
                if let Some(l) = &mut self.logs {
                    l.picker = None;
                }
                if let Some(f) = &mut self.files {
                    f.confirm_delete = None;
                    f.name_dialog = None;
                }
            }
            return Task::none();
        }

        // ⌘P con el buscador abierto: fija o desfija el resultado seleccionado.
        if self.finder.is_some() && modifiers.logo() && matches!(&key, keyboard::Key::Character(c) if c.eq_ignore_ascii_case("p")) {
            let pos = self.finder.as_ref().unwrap().selected;
            self.toggle_finder_pin(pos);
            return Task::none();
        }

        // Con el buscador rápido abierto, el teclado es del buscador (escribir lo recibe el campo de texto).
        if self.finder.is_some() && !modifiers.logo() {
            use keyboard::key::Named as N;
            if let keyboard::Key::Named(n) = &key {
                match n {
                    N::Escape => self.finder = None,
                    N::ArrowUp => self.finder.as_mut().unwrap().move_selection(-1),
                    N::ArrowDown => self.finder.as_mut().unwrap().move_selection(1),
                    N::Tab => self.finder.as_mut().unwrap().move_selection(if modifiers.shift() { -1 } else { 1 }),
                    N::Enter => {
                        let pos = self.finder.as_ref().unwrap().selected;
                        return self.run_finder(pos, modifiers.alt());
                    }
                    _ => {}
                }
            }
            return Task::none();
        }

        // Con la barra de búsqueda abierta, el teclado es de la barra (salvo los atajos con ⌘).
        if self.find.is_some() && !modifiers.logo() && self.editor.is_none() && self.danger.is_none() && self.panel.is_none() && self.aws_editor.is_none() && self.user_prompt.is_none() && self.tunnel_editor.is_none() && self.sync.dialog.is_none() && self.update_dialog.is_none() {
            if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                self.close_find();
            }
            return Task::none();
        }

        // Con un diálogo abierto, el teclado es del diálogo.
        if self.editor.is_some() || self.danger.is_some() || self.panel.is_some() || self.aws_editor.is_some() || self.user_prompt.is_some() || self.tunnel_editor.is_some() || self.sync.dialog.is_some() || self.update_dialog.is_some() {
            match key {
                keyboard::Key::Named(keyboard::key::Named::Escape) => {
                    if let Some(Panel::Snippets(st)) = &mut self.panel {
                        if st.form.take().is_some() {
                            return Task::none();
                        }
                    }
                    self.editor = None;
                    self.danger = None;
                    self.panel = None;
                    self.aws_editor = None;
                    self.user_prompt = None;
                    self.tunnel_editor = None;
                    self.sync.dialog = None;
                }
                keyboard::Key::Named(keyboard::key::Named::Tab) => {
                    return if modifiers.shift() { iced::widget::operation::focus_previous() } else { iced::widget::operation::focus_next() };
                }
                _ => {}
            }
            return Task::none();
        }

        // Atajos de la aplicación (⌘).
        if modifiers.logo() {
            // ⌥⌘ + flechas: mover el foco entre paneles. ⇧⌘↩: ampliar o restaurar el panel.
            if let keyboard::Key::Named(n) = &key {
                use keyboard::key::Named as N;
                let dir = match n {
                    N::ArrowLeft if modifiers.alt() => Some(pane_grid::Direction::Left),
                    N::ArrowRight if modifiers.alt() => Some(pane_grid::Direction::Right),
                    N::ArrowUp if modifiers.alt() => Some(pane_grid::Direction::Up),
                    N::ArrowDown if modifiers.alt() => Some(pane_grid::Direction::Down),
                    _ => None,
                };
                if let Some(d) = dir {
                    self.move_focus(d);
                    return Task::none();
                }
                if matches!(n, N::Enter) && modifiers.shift() {
                    self.toggle_zoom();
                    return Task::none();
                }
            }
            if let keyboard::Key::Named(n @ (keyboard::key::Named::ArrowUp | keyboard::key::Named::ArrowDown)) = &key {
                let dir = if matches!(n, keyboard::key::Named::ArrowUp) { -1 } else { 1 };
                if let Some(t) = self.active_tab() {
                    t.backend.jump_prompt(dir);
                    t.cache.clear();
                }
                return Task::none();
            }
            if let keyboard::Key::Character(c) = &key {
                match (c.as_str().to_lowercase().as_str(), modifiers.shift()) {
                    ("t", _) => self.new_local(),
                    ("w", _) => self.close_focused_pane(),
                    ("d", shift) => self.split_focused(!shift),
                    ("i", _) if modifiers.alt() => self.toggle_broadcast(),
                    ("]", false) => self.cycle_focus(1),
                    ("[", false) => self.cycle_focus(-1),
                    ("b", _) => self.sidebar = !self.sidebar,
                    ("f", _) => return self.open_find(),
                    ("g", shift) => self.find_step(!shift),
                    (",", _) => self.panel = Some(Panel::Settings(Page::General)),
                    ("k", true) => return self.open_snippets(),
                    ("c", _) => {
                        if let Some(t) = self.active_tab() {
                            if let Some(s) = t.backend.selection_text() {
                                t.backend.clear_selection();
                                return iced::clipboard::write(s);
                            }
                        }
                    }
                    ("v", _) => return iced::clipboard::read().map(Message::Paste),
                    ("]", true) | ("}", true) => self.active = (self.active + 1) % self.tabs.len(),
                    ("[", true) | ("{", true) => self.active = (self.active + self.tabs.len() - 1) % self.tabs.len(),
                    (d, false) if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() && d != "0" => {
                        let n = (d.as_bytes()[0] - b'1') as usize;
                        if n < self.tabs.len() {
                            self.active = n;
                        }
                    }
                    _ => {}
                }
            }
            return Task::none();
        }

        let Some(t) = self.active_tab() else { return Task::none() };
        // Avisar antes de ejecutar un comando delicado en una pestaña de producción.
        if matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter)) && self.guard_enter() {
            return Task::none();
        }
        let Some(t) = self.active_tab() else { return Task::none() };
        if matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter)) && !t.backend.alt_screen() {
            t.backend.mark_prompt();
        }
        if self.settings.backspace_ctrl_h && matches!(key, keyboard::Key::Named(keyboard::key::Named::Backspace)) && !modifiers.alt() {
            t.backend.write(&b"\x08"[..]);
            return Task::none();
        }
        let app_cursor = t.backend.mode().contains(TermMode::APP_CURSOR);
        if let Some(bytes) = keys::encode(&key, &modified_key, text.as_deref(), modifiers, app_cursor, self.settings.option_as_meta) {
            if t.broadcast {
                for p in t.panes() {
                    p.backend.scroll_to_bottom();
                    p.backend.write(bytes.clone());
                    p.cache.clear();
                }
            } else {
                t.backend.scroll_to_bottom();
                t.backend.clear_selection();
                t.backend.write(bytes);
                t.cache.clear();
            }
        }
        Task::none()
    }

    fn update(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::Tick(_) => {
                let mut update_task: Option<Task<Message>> = None;
                // La barra de menús se instala en el primer ciclo (ya con la aplicación iniciada, en el hilo principal).
                if self.menu.is_none() && !self.menu_failed && cfg!(target_os = "macos") {
                    match AppMenu::install() {
                        Ok(m) => self.menu = Some(m),
                        Err(e) => {
                            self.menu_failed = true;
                            eprintln!("jTerm: no se pudo crear el menú: {e}");
                        }
                    }
                }
                if self.update_due.is_some_and(|d| Instant::now() >= d) {
                    self.update_due = None;
                    update_task = Some(self.update_check(false));
                }
                let many = self.tabs.get(self.active).is_some_and(|t| t.grid.len() > 1);
                let actions = match &mut self.menu {
                    Some(m) => {
                        m.set_many_panes(many);
                        m.poll()
                    }
                    None => vec![],
                };
                let menu_tasks: Vec<Task<Message>> = actions.into_iter().map(|a| self.menu_action(a)).collect();
                let menu_task = Task::batch(menu_tasks.into_iter().chain(update_task));
                let active = self.active;
                for (i, tab) in self.tabs.iter_mut().enumerate() {
                    for (_, p) in tab.grid.iter_mut() {
                        if p.backend.take_dirty() {
                            p.cache.clear();
                            if let Some(title) = p.backend.title() {
                                p.title = title;
                            }
                        }
                        if p.backend.take_bell() && i != active {
                            p.bell = true;
                        }
                    }
                }
                if let Some(tab) = self.tabs.get_mut(active) {
                    for (_, p) in tab.grid.iter_mut() {
                        p.bell = false;
                    }
                }
                if self.sync.prefs.enabled && !self.sync.syncing && !self.sync.needs_pass {
                    let due = self.sync.due.is_some_and(|d| Instant::now() >= d) || self.sync.last_periodic.elapsed().as_secs() >= sync_ui::PERIODIC_SECS;
                    if due {
                        return Task::batch([menu_task, self.sync_now(), self.logs_poll()]);
                    }
                }
                return Task::batch([menu_task, self.logs_poll()]);
            }
            Message::Key(e) => return self.handle_key(e),
            Message::NewLocal => self.new_local(),
            Message::Select(i) => {
                if i < self.tabs.len() {
                    self.screen = Screen::Term;
                    if i != self.active && self.find.is_some() {
                        self.close_find();
                    }
                    self.active = i;
                    self.tabs[i].cache.clear();
                }
            }
            Message::Close(i) => self.close(i),
            Message::ToggleSidebar => self.sidebar = !self.sidebar,
            Message::Connect(id) => self.connect(id),
            Message::Grid(id, cols, lines) => {
                let (cw, ch) = (self.metrics.cw.round() as u16, self.metrics.ch as u16);
                if let Some(t) = self.tab_mut(id) {
                    t.backend.resize(cols, lines, cw, ch);
                    t.cache.clear();
                }
            }
            Message::SelStart(id, r, c, side) => {
                if let Some(t) = self.tab_mut(id) {
                    t.backend.start_selection(r, c, side, SelectionType::Simple);
                    t.cache.clear();
                }
            }
            Message::SelMove(id, r, c, side) => {
                if let Some(t) = self.tab_mut(id) {
                    t.backend.update_selection(r, c, side);
                    t.cache.clear();
                }
            }
            Message::Scroll(id, lines) => {
                if let Some(t) = self.tab_mut(id) {
                    t.scroll_acc += lines;
                    let whole = t.scroll_acc.trunc();
                    if whole != 0.0 {
                        t.scroll_acc -= whole;
                        t.backend.scroll_lines(whole as i32);
                        t.cache.clear();
                    }
                }
            }
            Message::Paste(Some(s)) => {
                if let Some(t) = self.active_tab() {
                    t.backend.paste(&s);
                }
            }
            Message::Paste(None) => {}
            Message::EditNew => self.editor = Some(Editor::new(&self.settings.default_user, self.settings.default_port)),
            Message::EditServer(id) => {
                self.editor = self.servers.items.iter().find(|s| s.id == id).map(Editor::from_server);
            }
            Message::EditField(f, v) => {
                if let Some(e) = &mut self.editor { e.set(f, v) }
            }
            Message::EditAuth(a) => {
                if let Some(e) = &mut self.editor { e.auth = a }
            }
            Message::EditEnv(v) => {
                if let Some(e) = &mut self.editor { e.env = v }
            }
            Message::EditJump(v) => {
                if let Some(e) = &mut self.editor { e.jump = v }
            }
            Message::EditCancel => self.editor = None,
            Message::EditSave => self.save_editor(),
            Message::EditDelete => {
                if let Some(orig) = self.editor.take().and_then(|e| e.original) {
                    self.servers.items.retain(|s| s.id != orig.id);
                    for s in &mut self.servers.items {
                        if s.jump == Some(orig.id) { s.jump = None }
                    }
                    self.secrets.delete(orig.id);
                    self.persist_servers();
                }
            }
            Message::FinderQuery(q) => {
                if let Some(f) = &mut self.finder { f.set_query(q) }
            }
            Message::FinderRun(pos, alt) => return self.run_finder(pos, alt),
            Message::FinderClose => self.finder = None,
            Message::FinderPin(pos) => self.toggle_finder_pin(pos),
            Message::UpdateCheck(manual) => return self.update_check(manual),
            Message::UpdateChecked(manual, r) => {
                self.update_busy = false;
                match r {
                    Ok(Some(rel)) => {
                        self.save_last_update_check();
                        let current = self.current_version();
                        self.update_found = Some(rel.clone());
                        self.update_dialog = Some(UpdateDialog::Found { rel, current });
                    }
                    Ok(None) => {
                        self.save_last_update_check();
                        if manual {
                            let v = self.current_version();
                            self.update_dialog = Some(UpdateDialog::Info { title: "jTerm está al día".into(), text: format!("Tienes la versión {v}, que es la más reciente.") });
                        }
                    }
                    Err(e) => {
                        if manual {
                            self.update_dialog = Some(UpdateDialog::Info { title: "No se pudo actualizar".into(), text: e });
                        } else {
                            eprintln!("jTerm updater: {e}");
                        }
                    }
                }
            }
            Message::UpdateLater => self.update_dialog = None,
            Message::Relaunch => {
                self.update_dialog = None;
                match &self.bundle {
                    Some(b) => {
                        // Igual que en Swift: espera un momento y vuelve a abrir la app.
                        let _ = std::process::Command::new("/bin/sh").arg("-c").arg(r#"sleep 0.6; /usr/bin/open -n "$0""#).arg(&b.path).spawn();
                        self.tunnel_mgr.stop_all();
                        return iced::exit();
                    }
                    None => {
                        self.update_dialog = Some(UpdateDialog::Info { title: "Idioma cambiado".into(), text: "Cierra y vuelve a abrir jTerm para aplicar el nuevo idioma.".into() });
                    }
                }
            }
            Message::UpdateInstall => return self.update_install(),
            Message::UpdatePrepared(r) => {
                self.update_busy = false;
                match r {
                    Ok(staged) => {
                        let Some(bundle) = self.bundle.clone() else { return Task::none() };
                        match updater::launch_swap(&staged, std::process::id(), &bundle.path) {
                            Ok(()) => {
                                self.tunnel_mgr.stop_all();
                                return iced::exit();
                            }
                            Err(e) => {
                                self.update_dialog = Some(UpdateDialog::Info { title: "No se pudo actualizar".into(), text: format!("No se pudo preparar el reemplazo: {e}") });
                            }
                        }
                    }
                    Err(e) => self.update_dialog = Some(UpdateDialog::Info { title: "No se pudo actualizar".into(), text: e }),
                }
            }
            Message::SyncToggle(on) => {
                if on {
                    self.sync_enable_dialog();
                } else {
                    self.sync.dialog = Some(SyncDialog::new(DialogKind::ConfirmDisable));
                }
            }
            Message::SyncKind(k, on) => {
                match k {
                    KindFlag::Servers => self.sync.prefs.servers = on,
                    KindFlag::Aws => self.sync.prefs.aws = on,
                    KindFlag::Commands => self.sync.prefs.commands = on,
                    KindFlag::Settings => self.sync.prefs.settings = on,
                }
                self.save_sync_prefs();
                self.sync.due = Some(Instant::now() + Duration::from_secs(1));
            }
            Message::SyncSecrets(on) => {
                if on {
                    self.sync.dialog = Some(SyncDialog::new(DialogKind::ConfirmSecrets));
                } else {
                    self.sync.prefs.secrets = false;
                    self.save_sync_prefs();
                    self.sync.due = Some(Instant::now() + Duration::from_secs(1));
                }
            }
            Message::SyncNow => return self.sync_now(),
            Message::SyncChangePass => self.sync.dialog = Some(SyncDialog::new(DialogKind::Change)),
            Message::SyncDelete => self.sync.dialog = Some(SyncDialog::new(DialogKind::ConfirmDelete)),
            Message::SyncReenter => self.sync.dialog = Some(SyncDialog::new(DialogKind::Reenter)),
            Message::SyncPass1(v) => {
                if let Some(d) = &mut self.sync.dialog {
                    d.pass1 = v;
                    d.error = None;
                }
            }
            Message::SyncPass2(v) => {
                if let Some(d) = &mut self.sync.dialog {
                    d.pass2 = v;
                    d.error = None;
                }
            }
            Message::SyncDialogCancel => self.sync.dialog = None,
            Message::SyncDialogOk => return self.sync_dialog_ok(),
            Message::SyncDone(r) => self.sync_done(r),
            Message::FilesOpen(id) => return self.files_open(id),
            Message::FilesClose => {
                self.files = None;
                if self.screen == Screen::Files {
                    self.screen = self.fallback_screen();
                }
            }
            Message::FilesConnected(r) => return self.files_connected(r),
            Message::FilesLoaded(gen, r) => {
                if let Some(f) = self.files.as_mut().filter(|f| f.remote.generation == gen) {
                    f.remote.loading = false;
                    match r {
                        Ok(v) => {
                            f.remote.entries = files_ui::remote_entries(v, f.remote.hidden);
                            f.remote.error = None;
                            f.is_error = false;
                            f.status = format!("{} · {} elementos", f.remote.path, f.remote.entries.len());
                        }
                        Err(e) => {
                            f.remote.error = Some(e);
                            f.remote.entries.clear();
                        }
                    }
                }
            }
            Message::FilesEnter(id, name) => {
                if let Some(f) = &mut self.files {
                    let p = f.pane_mut(id);
                    p.path = if id == PaneId::Local { Path::new(&p.path).join(&name).display().to_string() } else { files_ui::join_remote(&p.path, &name) };
                    p.selected = None;
                }
                return self.files_reload(id);
            }
            Message::FilesUp(id) => {
                if let Some(f) = &mut self.files {
                    let p = f.pane_mut(id);
                    p.path = if id == PaneId::Local {
                        Path::new(&p.path).parent().map(|x| x.display().to_string()).unwrap_or_else(|| p.path.clone())
                    } else {
                        files_ui::parent_remote(&p.path)
                    };
                    p.selected = None;
                }
                return self.files_reload(id);
            }
            Message::FilesRefresh(id) => return self.files_reload(id),
            Message::FilesSelect(id, name) => {
                if let Some(f) = &mut self.files { f.pane_mut(id).selected = name }
            }
            Message::FilesHidden(id) => {
                if let Some(f) = &mut self.files {
                    let p = f.pane_mut(id);
                    p.hidden = !p.hidden;
                    let v = p.hidden;
                    if id == PaneId::Local { self.settings.files_local_hidden = v } else { self.settings.files_remote_hidden = v }
                    self.save_settings();
                }
                return self.files_reload(id);
            }
            Message::FilesUpload => return self.files_transfer(Direction::Upload),
            Message::FilesDownload => return self.files_transfer(Direction::Download),
            Message::FilesTransferDone(id, r) => {
                let mut reload = None;
                if let Some(f) = &mut self.files {
                    if let Some(t) = f.transfers.iter_mut().find(|t| t.id == id) {
                        match r {
                            Ok(()) => {
                                t.finished = true;
                                reload = Some(t.refresh);
                            }
                            Err(e) => t.error = Some(e),
                        }
                    }
                }
                if let Some(p) = reload {
                    return self.files_reload(p);
                }
            }
            Message::FilesCancel(id) => {
                if let Some(t) = self.files.as_ref().and_then(|f| f.transfers.iter().find(|t| t.id == id)) {
                    t.job.cancel();
                }
            }
            Message::FilesDismiss(id) => {
                if let Some(f) = &mut self.files { f.transfers.retain(|t| t.id != id) }
            }
            Message::FilesDelete(id) => {
                if let Some(f) = &mut self.files {
                    if let Some(e) = f.pane(id).selected_entry().cloned() {
                        f.confirm_delete = Some((id, e));
                    }
                }
            }
            Message::FilesDeleteCancel => {
                if let Some(f) = &mut self.files { f.confirm_delete = None }
            }
            Message::FilesDeleteOk => return self.files_delete(),
            Message::FilesNewFolder(id) => {
                if let Some(f) = &mut self.files {
                    f.name_dialog = Some(NameDialog { pane: id, kind: NameKind::NewFolder, text: String::new() });
                }
                return iced::widget::operation::focus("files-name");
            }
            Message::FilesRename(id) => {
                if let Some(f) = &mut self.files {
                    if let Some(e) = f.pane(id).selected_entry().cloned() {
                        f.name_dialog = Some(NameDialog { pane: id, kind: NameKind::Rename(e.name.clone()), text: e.name });
                    }
                }
                return iced::widget::operation::focus("files-name");
            }
            Message::FilesName(t) => {
                if let Some(d) = self.files.as_mut().and_then(|f| f.name_dialog.as_mut()) { d.text = t }
            }
            Message::FilesNameCancel => {
                if let Some(f) = &mut self.files { f.name_dialog = None }
            }
            Message::FilesNameOk => return self.files_name_ok(),
            Message::FilesOpDone(id, r) => {
                if let Some(f) = &mut self.files {
                    if let Err(e) = r {
                        f.status = e;
                        f.is_error = true;
                    }
                }
                return self.files_reload(id);
            }
            Message::TunEdNew => self.tunnel_editor = Some(TunnelEditor::new(self.servers.items.first().map(|s| s.id))),
            Message::TunEdEdit(id) => {
                self.tunnel_editor = self.tunnels.items.iter().find(|t| t.id == id).map(TunnelEditor::from_tunnel);
            }
            Message::TunEdField(f, v) => {
                if let Some(e) = &mut self.tunnel_editor { e.set(f, v) }
            }
            Message::TunEdKind(k) => {
                if let Some(e) = &mut self.tunnel_editor { e.kind = k }
            }
            Message::TunEdServer(sv) => {
                if let Some(e) = &mut self.tunnel_editor { e.server = sv }
            }
            Message::TunEdCancel => self.tunnel_editor = None,
            Message::TunEdSave => {
                let Some(ed) = &mut self.tunnel_editor else { return Task::none() };
                match ed.build(jterm_core::tunnels::is_root()) {
                    Err(m) => ed.error = Some(m),
                    Ok(t) => {
                        match self.tunnels.items.iter().position(|x| x.id == t.id) {
                            Some(i) => self.tunnels.items[i] = t,
                            None => self.tunnels.items.push(t),
                        }
                        self.tunnel_editor = None;
                        if let Err(e) = self.tunnels.save() {
                            self.error = Some(format!("No se pudo guardar tunnels.json: {e}"));
                        }
                    }
                }
            }
            Message::TunEdDelete => {
                if let Some(orig) = self.tunnel_editor.take().and_then(|e| e.original) {
                    self.tunnel_mgr.stop(orig.id);
                    self.tunnels.items.retain(|t| t.id != orig.id);
                    let _ = self.tunnels.save();
                }
            }
            Message::TunToggle(id) => self.tunnel_toggle(id),
            Message::CloseRequested => {
                self.tunnel_mgr.stop_all();
                return iced::exit();
            }
            Message::PaneFocus(p) => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    if t.grid.get(p).is_some() && t.focus != p {
                        t.focus = p;
                        t.clear_caches();
                    }
                }
            }
            Message::PaneResized(e) => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.grid.resize(e.split, e.ratio);
                    t.clear_caches();
                }
            }
            Message::DragWindow => return iced::window::latest().and_then(iced::window::drag),
            Message::ZoomWindow => return iced::window::latest().and_then(iced::window::toggle_maximize),
            Message::SidebarMode(m) => {
                self.side = m;
                if m == SideMode::Aws {
                    self.aws_init();
                }
            }
            Message::AwsAccountPick(id) => {
                self.aws.account = Some(id);
                if let Some(a) = self.aws_store.items.iter().find(|a| a.id == id) {
                    self.aws.region = a.region.clone();
                }
                self.aws.instances.clear();
                self.aws.generation += 1;
                self.aws.loading = false;
                self.aws.status = "Pulsa ↻ para listar las instancias.".into();
                self.aws.is_error = false;
            }
            Message::AwsRegionPick(r) => {
                self.aws.region = if r == ALL_REGIONS { String::new() } else { r };
                self.aws.instances.clear();
                self.aws.status = "Pulsa ↻ para listar las instancias.".into();
                self.aws.is_error = false;
            }
            Message::AwsRefresh => return self.aws_refresh(),
            Message::AwsLoaded(gen, found, errors) => {
                if gen == self.aws.generation {
                    self.aws.loading = false;
                    let all_failed = found.is_empty() && !errors.is_empty();
                    self.aws.instances = found;
                    aws_ui::sort_instances(&mut self.aws.instances);
                    self.aws.is_error = all_failed;
                    self.aws.status = if all_failed {
                        errors.join("\n")
                    } else {
                        let n = self.aws.instances.len();
                        let base = if n == 1 { "1 instancia".to_string() } else { format!("{n} instancias") };
                        if errors.is_empty() { base } else { format!("{base} · {} región(es) con error", errors.len()) }
                    };
                }
            }
            Message::AwsConnect(i) => return self.aws_connect(i),
            Message::AwsUserInput(u) => {
                if let Some(p) = &mut self.user_prompt { p.user = u }
            }
            Message::AwsUserCancel => self.user_prompt = None,
            Message::AwsUserOk => {
                if let Some(p) = self.user_prompt.take() {
                    let user = p.user.trim().to_string();
                    if !user.is_empty() {
                        self.aws_users.set(&p.instance.id, &user);
                        self.settings.aws_user = user.clone();
                        self.save_settings();
                        return self.aws_start_connect(p.instance, user);
                    }
                }
            }
            Message::AwsConnected(Ok(info)) => {
                let host = if info.instance.public_dns.is_empty() { info.instance.public_ip.clone() } else { info.instance.public_dns.clone() };
                let mut args = vec!["-p".to_string(), "22".to_string()];
                args.extend(self.settings.ssh_args());
                args.extend(info.options.clone());
                args.push(format!("{}@{host}", info.user));
                self.spawn_with(info.instance.name.clone(), Some("/usr/bin/ssh".into()), args, HashMap::new(), None, info.env, true);
                self.aws.status = format!("{} instancias", self.aws.instances.len());
                self.aws.is_error = false;
            }
            Message::AwsConnected(Err(e)) => {
                self.aws.status = e;
                self.aws.is_error = true;
            }
            Message::AwsCopy(t) => return iced::clipboard::write(t),
            Message::AwsEdNew => self.aws_editor = Some(AwsEditor::new(&self.settings.aws_region)),
            Message::AwsEdEdit => {
                self.aws_editor = self.aws.account.and_then(|id| self.aws_store.items.iter().find(|a| a.id == id)).map(AwsEditor::from_account);
            }
            Message::AwsEdField(f, v) => {
                if let Some(e) = &mut self.aws_editor { e.set(f, v) }
            }
            Message::AwsEdRegion(r) => {
                if let Some(e) = &mut self.aws_editor { e.region = r }
            }
            Message::AwsEdEnv(v) => {
                if let Some(e) = &mut self.aws_editor { e.env = v }
            }
            Message::AwsEdCancel => self.aws_editor = None,
            Message::AwsEdSave => {
                let Some(ed) = &mut self.aws_editor else { return Task::none() };
                match ed.build() {
                    Err(m) => ed.error = Some(m),
                    Ok((acct, secret)) => {
                        let id = acct.id;
                        match awsstore::upsert(&mut self.aws_store, acct, secret.as_deref(), &*self.secrets) {
                            Ok(()) => {
                                self.sync.touch();
                                self.aws_editor = None;
                                self.aws.account = Some(id);
                                self.aws_init();
                            }
                            Err(e) => ed.error = Some(format!("No se pudo guardar aws.json: {e}")),
                        }
                    }
                }
            }
            Message::AwsEdDelete => {
                if let Some(orig) = self.aws_editor.take().and_then(|e| e.original) {
                    let _ = awsstore::remove(&mut self.aws_store, orig.id, &*self.secrets);
                    self.sync.touch();
                    if self.aws.account == Some(orig.id) {
                        self.aws.account = None;
                        self.aws.instances.clear();
                    }
                    self.aws_init();
                }
            }
            Message::LogsOpen => return self.logs_open(),
            Message::LogsClose => {
                self.logs = None;
                if self.screen == Screen::Logs {
                    self.screen = self.fallback_screen();
                }
            }
            Message::ShowScreen(sc) => self.screen = sc,
            Message::LogsRegion(r) => {
                if let Some(st) = &mut self.logs {
                    st.region = r;
                    st.groups.clear();
                    st.group = None;
                    st.following = false;
                    st.generation += 1;
                    st.clear_results();
                    st.status = "Elige un grupo de logs y pulsa Buscar.".into();
                    st.is_error = false;
                }
            }
            Message::LogsPicker(open) => {
                let mut load = false;
                if let Some(st) = &mut self.logs {
                    st.picker = open.then(String::new);
                    load = open && st.groups.is_empty() && !st.groups_loading;
                }
                if open {
                    return if load { Task::batch([self.logs_load_groups(), iced::widget::operation::focus("logs-group-search")]) } else { iced::widget::operation::focus("logs-group-search") };
                }
            }
            Message::LogsPickerQuery(q) => {
                if let Some(st) = &mut self.logs { st.picker = Some(q) }
            }
            Message::LogsLoadGroups => return self.logs_load_groups(),
            Message::LogsGroups(region, r) => {
                if let Some(st) = self.logs.as_mut().filter(|s| s.region == region) {
                    st.groups_loading = false;
                    match r {
                        Ok(g) => {
                            st.status = format!("{} grupos de logs en {region}. Elige uno y pulsa Buscar.", g.len());
                            st.is_error = false;
                            st.groups = g;
                        }
                        Err(e) => {
                            st.status = e;
                            st.is_error = true;
                        }
                    }
                }
            }
            Message::LogsPick(g) => {
                if let Some(st) = &mut self.logs {
                    st.group = Some(g);
                    st.picker = None;
                    st.following = false;
                    st.generation += 1;
                    st.clear_results();
                    st.status = "Pulsa Buscar para ver los eventos.".into();
                    st.is_error = false;
                }
            }
            Message::LogsRange(r) => {
                if let Some(st) = &mut self.logs { st.range = r }
            }
            Message::LogsFilter(f) => {
                if let Some(st) = &mut self.logs { st.filter = f }
            }
            Message::LogsStream(f) => {
                if let Some(st) = &mut self.logs { st.stream_prefix = f }
            }
            Message::LogsSearch => return self.logs_search(),
            Message::LogsPage(gen, r) => return self.logs_page(gen, r),
            Message::LogsFollow => return self.logs_toggle_follow(),
            Message::LogsFollowed(gen, r) => return self.logs_followed(gen, r),
            Message::LogsLevel(l) => {
                if let Some(st) = &mut self.logs {
                    st.level = l;
                    st.window = logs_ui::WINDOW_STEP;
                }
            }
            Message::LogsToggleGroup => {
                if let Some(st) = &mut self.logs { st.by_invocation = !st.by_invocation }
            }
            Message::LogsTogglePretty => {
                if let Some(st) = &mut self.logs { st.pretty_json = !st.pretty_json }
            }
            Message::LogsMode(t) => {
                if let Some(st) = &mut self.logs { st.text_mode = t }
            }
            Message::LogsToggleMode => {
                if let Some(st) = &mut self.logs { st.text_mode = !st.text_mode }
            }
            Message::LogsCollapse(rid) => {
                if let Some(st) = &mut self.logs {
                    if !st.collapsed.remove(&rid) {
                        st.collapsed.insert(rid);
                    }
                }
            }
            Message::LogsExpand(i) => {
                if let Some(st) = &mut self.logs {
                    if !st.expanded.remove(&i) {
                        st.expanded.insert(i);
                    }
                }
            }
            Message::LogsMore => {
                if let Some(st) = &mut self.logs { st.window += logs_ui::WINDOW_STEP }
            }
            Message::LogsCopyAll => {
                if let Some(st) = &mut self.logs {
                    st.status = format!("Copiado: {} eventos.", st.event_count());
                    st.is_error = false;
                    return iced::clipboard::write(st.plain_text());
                }
            }
            Message::LogsCopyEvent(i) => {
                if let Some(p) = self.logs.as_ref().and_then(|s| s.parsed.get(i)) {
                    return iced::clipboard::write(p.event.message.clone());
                }
            }
            Message::LogsSave => {
                return Task::perform(
                    async { rfd::AsyncFileDialog::new().set_title("Guardar logs").set_file_name("logs.txt").save_file().await.map(|f| f.path().to_path_buf()) },
                    Message::LogsSaveTo,
                );
            }
            Message::LogsSaveTo(path) => {
                if let (Some(path), Some(st)) = (path, &mut self.logs) {
                    match std::fs::write(&path, st.plain_text()) {
                        Ok(()) => {
                            st.status = format!("Guardado en {}.", path.file_name().unwrap_or_default().to_string_lossy());
                            st.is_error = false;
                        }
                        Err(e) => {
                            st.status = format!("No se pudo guardar: {e}");
                            st.is_error = true;
                        }
                    }
                }
            }
            Message::LogsClear => {
                if let Some(st) = &mut self.logs {
                    st.following = false;
                    st.searching = false;
                    st.generation += 1;
                    st.clear_results();
                    st.status = "Pantalla limpia.".into();
                    st.is_error = false;
                }
            }
            Message::LogsScrolled(y) => {
                if let Some(st) = &mut self.logs { st.at_bottom = y > 0.97 }
            }
            Message::S3Open => return self.s3_open(),
            Message::S3Close => {
                self.s3 = None;
                if self.screen == Screen::S3 {
                    self.screen = self.fallback_screen();
                }
            }
            Message::S3Home => {
                if let Some(st) = &mut self.s3 {
                    st.bucket = None;
                    st.prefix.clear();
                    st.entries.clear();
                    st.selected = None;
                }
            }
            Message::S3Enter(b) => {
                if let Some(st) = &mut self.s3 {
                    st.bucket = Some(b);
                    st.prefix.clear();
                    st.entries.clear();
                    st.selected = None;
                }
                return self.s3_refresh();
            }
            Message::S3Cd(p) => {
                if let Some(st) = &mut self.s3 {
                    st.prefix = p;
                    st.selected = None;
                }
                return self.s3_refresh();
            }
            Message::S3Up => {
                if let Some(st) = &mut self.s3 {
                    if st.prefix.is_empty() {
                        st.bucket = None;
                        st.entries.clear();
                    } else {
                        st.prefix = s3_ui::parent(&st.prefix);
                    }
                    st.selected = None;
                    if st.bucket.is_none() {
                        return Task::none();
                    }
                }
                return self.s3_refresh();
            }
            Message::S3Refresh => return self.s3_refresh(),
            Message::S3Buckets(gen, r) => {
                if let Some(st) = self.s3.as_mut().filter(|s| s.generation == gen) {
                    st.loading = false;
                    match r {
                        Ok(b) => {
                            st.status = if b.len() == 1 { "1 bucket".into() } else { format!("{} buckets", b.len()) };
                            st.is_error = false;
                            st.buckets = b;
                        }
                        Err(e) => {
                            st.status = e;
                            st.is_error = true;
                        }
                    }
                }
            }
            Message::S3Listed(gen, r) => {
                if let Some(st) = self.s3.as_mut().filter(|s| s.generation == gen) {
                    st.loading = false;
                    match r {
                        Ok(l) => {
                            st.entries = s3_ui::entries(&l, &st.prefix);
                            let n = st.entries.len();
                            st.status = if n == 1 { "1 elemento".into() } else { format!("{n} elementos") };
                            st.is_error = false;
                        }
                        Err(e) => {
                            st.status = e;
                            st.is_error = true;
                            st.entries.clear();
                        }
                    }
                }
            }
            Message::S3Select(k) => {
                if let Some(st) = &mut self.s3 {
                    st.selected = k;
                }
            }
            Message::S3Upload => {
                return Task::perform(
                    async { rfd::AsyncFileDialog::new().set_title("Subir a S3").pick_files().await.map(|v| v.into_iter().map(|f| f.path().to_path_buf()).collect()).unwrap_or_default() },
                    Message::S3UploadPicked,
                );
            }
            Message::S3UploadPicked(files) => return self.s3_upload(files),
            Message::S3Download => {
                return Task::perform(
                    async { rfd::AsyncFileDialog::new().set_title("Descargar en…").pick_folder().await.map(|f| f.path().to_path_buf()) },
                    Message::S3DownloadPicked,
                );
            }
            Message::S3DownloadPicked(dir) => return self.s3_download(dir),
            Message::S3TransferDone(id, r) => {
                let mut refresh = false;
                if let Some(st) = &mut self.s3 {
                    if let Some(t) = st.transfers.iter_mut().find(|t| t.id == id) {
                        match r {
                            Ok(_) => {
                                t.finished = true;
                                refresh = t.name.starts_with("Subida");
                            }
                            Err(e) => t.error = Some(e),
                        }
                    }
                }
                if refresh {
                    return self.s3_refresh();
                }
            }
            Message::S3DismissTransfer(id) => {
                if let Some(st) = &mut self.s3 {
                    st.transfers.retain(|t| t.id != id);
                }
            }
            Message::S3Presign => return self.s3_presign(),
            Message::S3Presigned(r) => {
                if let Some(st) = &mut self.s3 {
                    match r {
                        Ok(url) => {
                            st.status = "URL firmada copiada al portapapeles (válida 1 hora).".into();
                            st.is_error = false;
                            return iced::clipboard::write(url);
                        }
                        Err(e) => {
                            st.status = e;
                            st.is_error = true;
                        }
                    }
                }
            }
            Message::S3NewFolder => {
                if let Some(st) = &mut self.s3 {
                    st.new_folder = Some(String::new());
                }
                return iced::widget::operation::focus_next();
            }
            Message::S3FolderName(n) => {
                if let Some(st) = &mut self.s3 {
                    st.new_folder = Some(n);
                }
            }
            Message::S3FolderCancel => {
                if let Some(st) = &mut self.s3 {
                    st.new_folder = None;
                }
            }
            Message::S3FolderOk => return self.s3_create_folder(),
            Message::S3FolderDone(r) => {
                if let Some(st) = &mut self.s3 {
                    if let Err(e) = r {
                        st.status = e;
                        st.is_error = true;
                        return Task::none();
                    }
                }
                return self.s3_refresh();
            }
            Message::S3Delete => {
                if let Some(st) = &mut self.s3 {
                    if let Some(e) = st.selected_entry().cloned() {
                        st.confirm_delete = Some(vec![e]);
                    }
                }
            }
            Message::S3DeleteCancel => {
                if let Some(st) = &mut self.s3 {
                    st.confirm_delete = None;
                }
            }
            Message::S3DeleteConfirm => return self.s3_delete(),
            Message::S3Deleted(r) => {
                if let Some(st) = &mut self.s3 {
                    match r {
                        Ok(n) => {
                            st.status = format!("{n} objeto(s) eliminados.");
                            st.is_error = false;
                            st.selected = None;
                        }
                        Err(e) => {
                            st.status = e;
                            st.is_error = true;
                        }
                    }
                }
                return self.s3_refresh();
            }
            Message::Redraw(id) => {
                if let Some(t) = self.tab_mut(id) {
                    t.cache.clear();
                }
            }
            Message::OpenLink(kind, text) => {
                if let Err(e) = links::open(kind, &text) {
                    self.error = Some(e);
                }
            }
            Message::Write(id, bytes) => {
                if let Some(t) = self.tab_mut(id) {
                    t.backend.write(bytes);
                }
            }
            Message::OpenFind => return self.open_find(),
            Message::FindQuery(q) => {
                let info = self.active_tab().map(|t| {
                    let i = t.backend.set_search(&q);
                    t.cache.clear();
                    i
                });
                if let Some(f) = &mut self.find {
                    f.query = q;
                    f.info = info.unwrap_or((0, 0));
                }
            }
            Message::FindNext => self.find_step(true),
            Message::FindPrev => self.find_step(false),
            Message::CloseFind => self.close_find(),
            Message::OpenSettings => self.panel = Some(Panel::Settings(Page::General)),
            Message::SettingsPage(p) => self.panel = Some(Panel::Settings(p)),
            Message::OpenSnippets => return self.open_snippets(),
            Message::ClosePanel => self.panel = None,
            Message::Setting(c) => {
                let old_language = self.settings.language.clone();
                panels::apply(&mut self.settings, c);
                if self.settings.language != old_language {
                    self.update_dialog = Some(UpdateDialog::Relaunch);
                }
                self.apply_settings();
            }
            Message::SnipQuery(q) => {
                if let Some(Panel::Snippets(st)) = &mut self.panel { st.query = q }
            }
            Message::SnipNew => {
                if let Some(Panel::Snippets(st)) = &mut self.panel { st.form = Some(SnippetForm::default()) }
            }
            Message::SnipEdit(id) => {
                let form = self.snippets.items.iter().find(|s| s.id == id).map(SnippetForm::from);
                if let Some(Panel::Snippets(st)) = &mut self.panel { st.form = form }
            }
            Message::SnipField(is_command, v) => {
                if let Some(Panel::Snippets(SnippetState { form: Some(f), .. })) = &mut self.panel {
                    if is_command { f.command = v } else { f.name = v }
                }
            }
            Message::SnipAutoRun(b) => {
                if let Some(Panel::Snippets(SnippetState { form: Some(f), .. })) = &mut self.panel { f.auto_run = b }
            }
            Message::SnipBack => {
                if let Some(Panel::Snippets(st)) = &mut self.panel { st.form = None }
            }
            Message::SnipSave => {
                let built = match &self.panel {
                    Some(Panel::Snippets(SnippetState { form: Some(f), .. })) => f.build(),
                    _ => None,
                };
                if let Some(s) = built {
                    match self.snippets.items.iter().position(|x| x.id == s.id) {
                        Some(i) => self.snippets.items[i] = s,
                        None => self.snippets.items.push(s),
                    }
                    let _ = self.snippets.save();
                self.sync.touch();
                    self.sync.touch();
                    if let Some(Panel::Snippets(st)) = &mut self.panel { st.form = None }
                }
            }
            Message::SnipDelete(id) => {
                self.snippets.items.retain(|s| s.id != id);
                let _ = self.snippets.save();
                self.sync.touch();
            }
            Message::SnipInsert(id) => {
                let Some(s) = self.snippets.items.iter().find(|s| s.id == id).cloned() else { return Task::none() };
                self.panel = None;
                if let Some(t) = self.active_tab() {
                    t.backend.paste(&s.command);
                    t.cache.clear();
                }
                if s.auto_run && !self.guard_enter() {
                    if let Some(t) = self.active_tab() {
                        t.backend.mark_prompt();
                        t.backend.write(&b"\r"[..]);
                    }
                }
            }
            Message::DangerCancel => self.danger = None,
            Message::DangerRun => {
                if let Some(d) = self.danger.take() {
                    if let Some(t) = self.tab_mut(d.tab) {
                        t.backend.mark_prompt();
                        t.backend.write(&b"\r"[..]);
                        t.cache.clear();
                    }
                }
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            keyboard::listen().map(Message::Key),
            time::every(Duration::from_millis(30)).map(Message::Tick),
            iced::window::close_requests().map(|_| Message::CloseRequested),
        ])
    }

    // MARK: Vista

    fn view(&self) -> Element<'_, Message> {
        let p = &self.palette;
        let bar_bg = shade(p.bg, -0.18);
        // La barra de pestañas ocupa todo el ancho (bajo los botones de la ventana) y la barra lateral queda debajo de ella.
        let body: Element<Message> = if self.sidebar {
            row![self.sidebar_view(), container(self.terminal()).width(Length::Fill).height(Length::Fill)].height(Length::Fill).into()
        } else {
            container(self.terminal()).width(Length::Fill).height(Length::Fill).into()
        };
        let content: Element<Message> = column![self.tab_bar(bar_bg), body].width(Length::Fill).height(Length::Fill).into();
        let base: Element<Message> = container(content).width(Length::Fill).height(Length::Fill).style(move |_| container::Style { background: Some(Background::Color(bar_bg)), ..Default::default() }).into();
        let overlay = if let Some(ed) = &self.editor {
            let me = ed.original.as_ref().map(|s| s.id);
            let mut jumps = vec![JumpOpt { id: None, label: "Ninguno (conexión directa)".into() }];
            jumps.extend(self.servers.items.iter().filter(|s| Some(s.id) != me).map(|s| JumpOpt {
                id: Some(s.id),
                label: if s.name.is_empty() { s.host.clone() } else { s.name.clone() },
            }));
            Some(ed.view(jumps, &self.palette, self.accent()))
        } else if let Some((st, q)) = self.logs.as_ref().and_then(|s| s.picker.as_deref().map(|q| (s, q))) {
            Some(logs_ui::picker_view(st, q, &self.palette, self.accent()))
        } else if let Some(items) = self.s3.as_ref().and_then(|s| s.confirm_delete.as_ref()) {
            Some(s3_ui::confirm_delete_view(items, &self.palette))
        } else if let Some(name) = self.s3.as_ref().and_then(|s| s.new_folder.as_ref()) {
            Some(s3_ui::new_folder_view(name, &self.palette, self.accent()))
        } else if let Some((pane, e)) = self.files.as_ref().and_then(|f| f.confirm_delete.as_ref()).map(|(p, e)| (*p, e)) {
            Some(files_ui::confirm_delete_view(pane, e, &self.palette))
        } else if let Some(d) = self.files.as_ref().and_then(|f| f.name_dialog.as_ref()) {
            Some(files_ui::name_dialog_view(d, &self.palette, self.accent()))
        } else if let Some(d) = &self.update_dialog {
            Some(update_ui::view(d, &self.palette, self.accent()))
        } else if let Some(d) = &self.sync.dialog {
            Some(sync_ui::dialog_view(d, &self.palette, self.accent()))
        } else if let Some(ed) = &self.tunnel_editor {
            Some(ed.view(&self.servers.items, &self.palette, self.accent()))
        } else if let Some(ed) = &self.aws_editor {
            Some(ed.view(&self.palette, self.accent()))
        } else if let Some(u) = &self.user_prompt {
            Some(aws_ui::user_prompt_view(&u.instance, &u.user, &self.palette, self.accent()))
        } else if let Some(d) = &self.danger {
            Some(self.danger_view(d))
        } else {
            match &self.panel {
                Some(Panel::Settings(page)) => Some(panels::settings_view(&self.settings, *page, &self.palette, self.accent(), &self.sync.view_model(sync_ui::sync_folder().is_some(), chrono::Utc::now()))),
                Some(Panel::Snippets(st)) => Some(panels::snippets_view(search_snippets(&self.snippets.items, &st.query), st, &self.palette, self.accent())),
                None => None,
            }
        };
        let result: Element<Message> = match overlay {
            None => base,
            Some(dlg) => stack![
                base,
                opaque(center(opaque(dlg)).style(|_| container::Style { background: Some(Background::Color(Color { a: 0.55, ..Color::BLACK })), ..Default::default() }))
            ]
            .into(),
        };
        match &self.finder {
            None => result,
            Some(f) => {
                let layer = container(opaque(finder::view(f, &self.palette, self.accent())))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Top)
                    .padding(iced::Padding { top: 90.0, ..Default::default() });
                stack![result, iced::widget::mouse_area(layer).on_press(Message::FinderClose)].into()
            }
        }
    }

    fn danger_view<'a>(&'a self, d: &'a Danger) -> Element<'a, Message> {
        let fg = self.palette.fg;
        let red = Color::from_rgb8(0xff, 0x45, 0x3b);
        let body = column![
            text(tr("Comando delicado en PRODUCCIÓN")).size(16).color(red),
            text(tr(d.what)).size(13).color(fg),
            container(text(d.line.clone()).size(12).font(term_view::FONT).color(fg))
                .padding(8)
                .width(Length::Fill)
                .style(|_| container::Style { background: Some(Background::Color(Color { a: 0.25, ..Color::BLACK })), border: Border { radius: 6.0.into(), ..Default::default() }, ..Default::default() }),
            row![
                space::horizontal(),
                action_button("Cancelar", fg, fg, false).on_press(Message::DangerCancel),
                action_button("Ejecutar de todos modos", red, fg, true).on_press(Message::DangerRun),
            ]
            .spacing(8),
        ]
        .spacing(12);
        dialog(body.into(), &self.palette, 460.0)
    }

    /// Tema de iced con los colores del tema de terminal elegido: los controles heredan fondo, texto y acento.
    fn theme(&self) -> Theme {
        let bg = shade(self.palette.bg, -0.1);
        Theme::custom(
            "jTerm".to_string(),
            iced::theme::Palette {
                background: bg,
                text: self.palette.fg,
                primary: self.accent(),
                success: Color::from_rgb8(0x4c, 0xc3, 0x8a),
                warning: Color::from_rgb8(0xf2, 0xc9, 0x7d),
                danger: Color::from_rgb8(0xff, 0x6b, 0x6b),
            },
        )
    }

    /// Vista a mostrar cuando se cierra la que estaba al frente: otra de AWS/archivos abierta o el terminal.
    fn fallback_screen(&self) -> Screen {
        if self.logs.is_some() {
            Screen::Logs
        } else if self.s3.is_some() {
            Screen::S3
        } else if self.files.is_some() {
            Screen::Files
        } else {
            Screen::Term
        }
    }

    fn accent(&self) -> Color {
        rgb(themes::accent_hex(&self.settings.accent))
    }

    /// Botón flotante (esquina inferior derecha del terminal) que abre los comandos guardados.
    fn snippets_fab(&self) -> Element<'_, Message> {
        let accent = self.accent();
        let fg = self.palette.fg;
        let base = self.palette.bg;
        let content = row![
            ui::icon(ui::Icon::SquareTerminal, 15.0, accent),
            text(tr("Comandos")).size(12).color(Color { a: 0.85, ..fg }),
            text("⇧⌘K").size(10).color(Color { a: 0.4, ..fg }),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let btn = button(content).padding([8, 14]).on_press(Message::OpenSnippets).style(move |_, st| {
            let hover = matches!(st, button::Status::Hovered | button::Status::Pressed);
            // Fondo opaco (el texto del terminal no debe verse detrás) con un velo del color de acento.
            let mix = |a: f32| Color { r: base.r + (accent.r - base.r) * a, g: base.g + (accent.g - base.g) * a, b: base.b + (accent.b - base.b) * a, a: 1.0 };
            button::Style {
                background: Some(Background::Color(mix(if hover { 0.26 } else { 0.14 }))),
                text_color: fg,
                border: Border { radius: 20.0.into(), width: 1.0, color: Color { a: if hover { 0.8 } else { 0.45 }, ..accent } },
                shadow: iced::Shadow { color: Color { a: 0.35, ..Color::BLACK }, offset: iced::Vector::new(0.0, 4.0), blur_radius: 12.0 },
                ..Default::default()
            }
        });
        container(btn)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(18)
            .into()
    }

    fn find_bar(&self, f: &FindState) -> Element<'_, Message> {
        let fg = self.palette.fg;
        let dim = Color { a: 0.6, ..fg };
        let count = if f.query.is_empty() {
            String::new()
        } else if f.info.1 == 0 {
            "Sin resultados".to_string()
        } else {
            format!("{} de {}", f.info.0, f.info.1)
        };
        let bar = row![
            text_input("Buscar en el terminal…", &f.query).id("find-input").on_input(Message::FindQuery).on_submit(Message::FindNext).padding(6).size(13).width(Length::Fixed(220.0)),
            text(count).size(12).color(dim).width(Length::Fixed(90.0)),
            flat_button("↑", fg).on_press(Message::FindPrev),
            flat_button("↓", fg).on_press(Message::FindNext),
            flat_button("×", fg).on_press(Message::CloseFind),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center);
        let bg = shade(self.palette.bg, -0.2);
        container(container(bar).padding([6, 10]).style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: 8.0.into(), width: 1.0, color: Color { a: 0.15, ..Color::WHITE } },
            ..Default::default()
        }))
        .width(Length::Fill)
        .padding(10)
        .align_x(iced::alignment::Horizontal::Right)
        .into()
    }

    fn terminal(&self) -> Element<'_, Message> {
        match (self.screen, &self.logs, &self.s3) {
            (Screen::Logs, Some(st), _) => return logs_ui::view(st, &self.palette, self.accent()),
            (Screen::S3, _, Some(st)) => return s3_ui::view(st, &self.palette, self.accent()),
            _ => {}
        }
        if let (Screen::Files, Some(st)) = (self.screen, &self.files) {
            return files_ui::view(st, &self.palette, self.accent());
        }
        let term = self.terminal_canvas();
        let mut layers: Vec<Element<Message>> = vec![term];
        if let Some(f) = &self.find {
            layers.push(self.find_bar(f));
        }
        if self.settings.show_snippets_button {
            layers.push(self.snippets_fab());
        }
        let area: Element<Message> = stack(layers).into();
        // Línea del color del entorno sobre el terminal (roja en producción), como en la versión Swift.
        match self.active_tab().and_then(|t| t.env) {
            Some(env) => {
                let (r8, g8, b8) = env.rgb();
                let c = Color::from_rgb8(r8, g8, b8);
                let line = container(space::vertical()).width(Length::Fill).height(Length::Fixed(2.0)).style(move |_| container::Style { background: Some(Background::Color(c)), ..Default::default() });
                column![line, area].into()
            }
            None => area,
        }
    }

    fn terminal_canvas(&self) -> Element<'_, Message> {
        let Some(tab) = self.active_tab() else { return space::vertical().into() };
        let multiple = tab.grid.len() > 1;
        let (accent, focus) = (self.accent(), tab.focus);
        let font = term_view::terminal_font(&self.settings.font_family);
        let grid = pane_grid::PaneGrid::new(&tab.grid, |handle, p, _maximized| {
            let focused = handle == focus;
            let view = canvas(TermView {
                tab: p.id,
                backend: &p.backend,
                palette: &self.palette,
                metrics: self.metrics,
                cache: &p.cache,
                cursor_color: self.palette.cursor,
                font,
                medium: self.settings.font_medium,
                cursor_shape: &self.settings.cursor_shape,
                mouse_reporting: self.settings.mouse_reporting,
                allow_paths: !p.remote,
                link_color: accent,
            })
            .width(Length::Fill)
            .height(Length::Fill);
            // Margen interior con el fondo del tema, para que el texto no quede pegado al borde.
            let term_bg = self.palette.bg;
            let view = container(view).padding(if multiple { 4.0 } else { 5.0 }).style(move |_| container::Style { background: Some(Background::Color(term_bg)), ..Default::default() });
            pane_grid::Content::new(view).style(move |_| container::Style {
                border: Border { radius: 0.0.into(), width: if multiple { 1.5 } else { 0.0 }, color: if focused { Color { a: 0.8, ..accent } } else { Color { a: 0.12, ..Color::WHITE } } },
                ..Default::default()
            })
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(if multiple { 3 } else { 0 })
        .on_click(Message::PaneFocus)
        .on_resize(60, Message::PaneResized);
        let mut e: Element<Message> = grid.into();
        if tab.broadcast {
            e = stack![e, container(text(tr("Escribiendo en todos los paneles")).size(11).color(Color::BLACK)).padding([3, 9]).style(move |_| container::Style {
                background: Some(Background::Color(Color::from_rgb8(0xf2, 0xc9, 0x7d))),
                border: Border { radius: 8.0.into(), ..Default::default() },
                ..Default::default()
            })]
            .into();
        }
        e
    }

    fn tab_bar(&self, bg: Color) -> Element<'_, Message> {
        let accent = self.accent();
        let fg = self.palette.fg;
        let dim = Color { a: 0.55, ..fg };
        let mut r: Row<Message> = Row::new().spacing(6).align_y(iced::Alignment::Center);
        // Botón de la barra lateral: con el contorno del acento mientras está visible.
        let side_on = self.sidebar;
        r = r.push(
            button(ui::icon(ui::Icon::PanelLeft, 15.0, if side_on { accent } else { dim })).padding([6, 8]).on_press(Message::ToggleSidebar).style(move |_, st| button::Style {
                background: Some(Background::Color(if matches!(st, button::Status::Hovered) { Color { a: 0.1, ..fg } } else { Color::TRANSPARENT })),
                text_color: fg,
                border: Border { radius: 8.0.into(), width: if side_on { 1.0 } else { 0.0 }, color: Color { a: 0.7, ..accent } },
                ..Default::default()
            }),
        );
        for (i, t) in self.tabs.iter().enumerate() {
            let selected = i == self.active && self.screen == Screen::Term;
            let mut label: Row<Message> = Row::new().spacing(7).align_y(iced::Alignment::Center);
            label = label.push(ui::icon(if t.remote { ui::Icon::Server } else { ui::Icon::Terminal }, 13.0, if selected { fg } else { dim }));
            if let Some(env) = t.env {
                let (r8, g8, b8) = env.rgb();
                let c = Color::from_rgb8(r8, g8, b8);
                label = label.push(
                    container(text(env.tag()).size(8).color(c)).padding([1, 5]).style(move |_| container::Style {
                        background: Some(Background::Color(Color { a: 0.2, ..c })),
                        border: Border { radius: 4.0.into(), ..Default::default() },
                        ..Default::default()
                    }),
                );
            }
            let mut title = t.title.clone();
            if !t.alive() {
                title.push_str(" (terminado)");
            }
            if title.chars().count() > 28 {
                title = title.chars().take(27).collect::<String>() + "…";
            }
            label = label.push(text(title).size(12).color(if selected { fg } else { dim }));
            if t.bell {
                label = label.push(text(tr("●")).size(8).color(accent));
            }
            let close = button(ui::icon(ui::Icon::X, 12.0, dim)).padding([2, 4]).on_press(Message::Close(i)).style(move |_, st| button::Style {
                background: Some(Background::Color(if matches!(st, button::Status::Hovered) { Color { a: 0.16, ..fg } } else { Color::TRANSPARENT })),
                text_color: fg,
                border: Border { radius: 5.0.into(), ..Default::default() },
                ..Default::default()
            });
            let inner = row![label, close].spacing(8).align_y(iced::Alignment::Center);
            // Las pestañas de un servidor con entorno (PROD, STAGING, DEV) se tiñen con su color.
            let env_c = t.env.map(|e| {
                let (r8, g8, b8) = e.rgb();
                Color::from_rgb8(r8, g8, b8)
            });
            r = r.push(button(inner).padding([5, 10]).on_press(Message::Select(i)).style(move |_, status| {
                let hover = matches!(status, button::Status::Hovered);
                match env_c {
                    Some(c) => button::Style {
                        background: Some(Background::Color(Color { a: if selected { 0.22 } else if hover { 0.16 } else { 0.10 }, ..c })),
                        text_color: fg,
                        border: Border { radius: 9.0.into(), width: 1.0, color: Color { a: if selected { 0.95 } else { 0.4 }, ..c } },
                        ..Default::default()
                    },
                    None => button::Style {
                        background: Some(Background::Color(if selected { Color { a: 0.11, ..fg } } else if hover { Color { a: 0.06, ..fg } } else { Color::TRANSPARENT })),
                        text_color: fg,
                        border: Border { radius: 9.0.into(), width: 1.0, color: if selected { Color { a: 0.16, ..fg } } else { Color::TRANSPARENT } },
                        ..Default::default()
                    },
                }
            }));
        }
        // Vistas de AWS abiertas (logs y S3): pestañas que se pueden alternar con las de terminal sin perder su estado.
        if let Some(l) = &self.logs {
            r = aws_tab(r, fg, dim, ui::Icon::ScrollText, format!("Logs · {}", if l.account.name.is_empty() { "AWS" } else { &l.account.name }), Screen::Logs, self.screen == Screen::Logs, Message::LogsClose);
        }
        if let Some(s3) = &self.s3 {
            r = aws_tab(r, fg, dim, ui::Icon::Database, format!("S3 · {}", if s3.account.name.is_empty() { "AWS" } else { &s3.account.name }), Screen::S3, self.screen == Screen::S3, Message::S3Close);
        }
        if let Some(f) = &self.files {
            r = aws_tab(r, fg, dim, ui::Icon::FolderOpen, format!("Archivos · {}", f.title), Screen::Files, self.screen == Screen::Files, Message::FilesClose);
        }
        r = r.push(flat_button("+", dim).on_press(Message::NewLocal));
        let tabs = scrollable(r).width(Length::Fill).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(0).scroller_width(0)));
        let tools = row![flat_button("🔍", dim).on_press(Message::OpenFind), flat_button("⚙", dim).on_press(Message::OpenSettings)].spacing(2);
        // En macOS el contenido llega hasta arriba (barra de título transparente): se deja sitio a los botones de la ventana.
        let left = if cfg!(target_os = "macos") { 82.0 } else { 8.0 };
        let bar = container(row![tabs, tools].spacing(8).align_y(iced::Alignment::Center))
            .padding(iced::Padding { top: 6.0, bottom: 6.0, left, right: 10.0 })
            .width(Length::Fill)
            .style(move |_| container::Style { background: Some(Background::Color(bg)), ..Default::default() });
        // Arrastrar la barra mueve la ventana; doble clic la amplía.
        iced::widget::mouse_area(bar).on_press(Message::DragWindow).on_double_click(Message::ZoomWindow).into()
    }

    fn sidebar_view(&self) -> Element<'_, Message> {
        let fg = self.palette.fg;
        let bg = shade(self.palette.bg, -0.1);
        let accent = self.accent();
        let idx = match self.side {
            SideMode::Servers => 0,
            SideMode::Aws => 1,
            SideMode::Tunnels => 2,
        };
        let switcher = container(ui::segmented(&["Servidores", "AWS", "Túneles"], idx, accent, fg, |i| {
            Message::SidebarMode(match i {
                0 => SideMode::Servers,
                1 => SideMode::Aws,
                _ => SideMode::Tunnels,
            })
        }))
        .padding([10, 10]);
        let body: Element<Message> = match self.side {
            SideMode::Servers => self.servers_view(),
            SideMode::Aws => aws_ui::panel_view(&self.aws, &self.aws_store.items, &self.palette, accent),
            SideMode::Tunnels => tunnels_ui::panel_view(&self.tunnels.items, &self.servers.items, &self.tunnel_mgr, &self.palette),
        };
        let content = column![switcher, body].height(Length::Fill);
        container(content)
            .width(Length::Fixed(240.0))
            .height(Length::Fill)
            .style(move |_| container::Style { background: Some(Background::Color(bg)), ..Default::default() })
            .into()
    }

    fn servers_view(&self) -> Element<'_, Message> {
        let fg = self.palette.fg;
        let dim = Color { a: 0.5, ..fg };
        let accent = self.accent();
        // Servidores con alguna sesión abierta (cualquier pestaña o panel que siga vivo).
        let connected: std::collections::HashSet<Uuid> = self.tabs.iter().flat_map(|t| t.panes()).filter(|p| p.alive()).filter_map(|p| p.server).collect();
        let mut col: Column<Message> = Column::new().spacing(3).padding([4, 10]).width(Length::Fill);
        col = col.push(
            row![ui::caption("Guardados", fg), space::horizontal(), flat_button("+", dim).on_press(Message::EditNew)].align_y(iced::Alignment::Center),
        );
        if self.servers.items.is_empty() {
            col = col.push(text(tr("Sin servidores guardados.")).size(12).color(dim));
        }
        for s in &self.servers.items {
            let name = if s.name.is_empty() { s.host.clone() } else { s.name.clone() };
            let green = Color::from_rgb8(0x30, 0xd1, 0x58);
            let is_conn = connected.contains(&s.id);
            let active = self.active_tab().and_then(|t| t.server) == Some(s.id);
            // Línea 1: punto de estado + nombre + etiqueta de entorno.
            let mut title: Row<Message> = Row::new().spacing(7).align_y(iced::Alignment::Center);
            if is_conn {
                title = title.push(text("●").size(9).color(green));
            }
            title = title.push(text(name).size(13).color(fg).wrapping(iced::widget::text::Wrapping::None));
            if let Some(env) = s.env {
                let (r, g, b) = env.rgb();
                let c = Color::from_rgb8(r, g, b);
                title = title.push(container(text(env.tag()).size(8).color(c)).padding([1, 5]).style(move |_| container::Style {
                    background: Some(Background::Color(Color { a: 0.2, ..c })),
                    border: Border { radius: 4.0.into(), ..Default::default() },
                    ..Default::default()
                }));
            }
            // Línea 2: método de autenticación + usuario@host.
            let sub = if s.user.is_empty() { s.host.clone() } else { format!("{}@{}", s.user, s.host) };
            let auth_icon = ui::icon(if s.auth == AuthMethod::Key { ui::Icon::KeyRound } else { ui::Icon::Lock }, 11.0, dim);
            let subline = row![auth_icon, text(sub).size(11).color(dim).wrapping(iced::widget::text::Wrapping::None)].spacing(5).align_y(iced::Alignment::Center);
            let mini = |i: ui::Icon, m: Message| {
                button(ui::icon(i, 13.0, dim)).padding([3, 5]).on_press(m).style(move |_, st| button::Style {
                    background: Some(Background::Color(if matches!(st, button::Status::Hovered | button::Status::Pressed) { Color { a: 0.14, ..fg } } else { Color::TRANSPARENT })),
                    text_color: dim,
                    border: Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                })
            };
            let acts = row![mini(ui::Icon::Folder, Message::FilesOpen(s.id)), mini(ui::Icon::Pencil, Message::EditServer(s.id))].spacing(2);
            let body = row![column![title, subline].spacing(3).width(Length::Fill), acts].spacing(4).align_y(iced::Alignment::Center);
            let card = container(body)
                .padding(iced::Padding { top: 7.0, bottom: 7.0, left: 10.0, right: 6.0 })
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color { a: if active { 0.1 } else { 0.04 }, ..fg })),
                    border: Border {
                        radius: 10.0.into(),
                        width: 1.0,
                        color: if active { Color { a: 0.6, ..accent } } else if is_conn { Color { a: 0.4, ..green } } else { Color { a: 0.08, ..fg } },
                    },
                    ..Default::default()
                });
            // Toda la tarjeta conecta; los botones de la cabecera tienen prioridad.
            let card = iced::widget::mouse_area(card).on_press(Message::Connect(s.id)).interaction(iced::mouse::Interaction::Pointer);
            col = col.push(card);
        }
        if let Some(e) = &self.error {
            col = col.push(text(tr(e)).size(11).color(rgb("#ff6b6b")));
        }
        scrollable(col).height(Length::Fill).into()
    }
}

/// Pestaña de una vista de AWS (logs o S3) en la barra de pestañas.
#[allow(clippy::too_many_arguments)]
fn aws_tab<'a>(r: Row<'a, Message>, fg: Color, dim: Color, icon: ui::Icon, title: String, sc: Screen, selected: bool, close: Message) -> Row<'a, Message> {
    let inner = row![
        ui::icon(icon, 13.0, if selected { fg } else { dim }),
        text(title).size(12).color(if selected { fg } else { dim }),
        button(ui::icon(ui::Icon::X, 12.0, dim)).padding([2, 4]).on_press(close).style(move |_, st| button::Style {
            background: Some(Background::Color(if matches!(st, button::Status::Hovered) { Color { a: 0.16, ..fg } } else { Color::TRANSPARENT })),
            text_color: fg,
            border: Border { radius: 5.0.into(), ..Default::default() },
            ..Default::default()
        }),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    r.push(button(inner).padding([5, 10]).on_press(Message::ShowScreen(sc)).style(move |_, status| {
        let hover = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if selected { Color { a: 0.11, ..fg } } else if hover { Color { a: 0.06, ..fg } } else { Color::TRANSPARENT })),
            text_color: fg,
            border: Border { radius: 9.0.into(), width: 1.0, color: if selected { Color { a: 0.16, ..fg } } else { Color::TRANSPARENT } },
            ..Default::default()
        }
    }))
}

fn flat_button<'a>(label: &'a str, fg: Color) -> button::Button<'a, Message> {
    ui::icon_button(label, fg, fg)
}

/// Aclara (`amount` > 0) u oscurece (< 0) un color.
fn shade(c: Color, amount: f32) -> Color {
    let f = |v: f32| if amount >= 0.0 { v + (1.0 - v) * amount } else { v * (1.0 + amount) };
    Color { r: f(c.r), g: f(c.g), b: f(c.b), a: c.a }
}

/// Tamaño mínimo de la ventana (ancho, alto): barra lateral de 240 + un área de trabajo que quepa.
const MIN_WINDOW: (f32, f32) = (900.0, 520.0);

/// ¿Se cargó la fuente de interfaz del sistema (SF Pro, que cosmic-text llama "System Font")?
static SYSTEM_UI_FONT_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// SF Mono viene con Terminal.app; se carga desde ahí (no se distribuye con jTerm).
fn load_system_fonts() -> Vec<Vec<u8>> {
    let dir = "/System/Applications/Utilities/Terminal.app/Contents/Resources/Fonts";
    let files = ["SF-Mono-Regular.otf", "SF-Mono-RegularItalic.otf", "SF-Mono-Medium.otf", "SF-Mono-MediumItalic.otf", "SF-Mono-Bold.otf", "SF-Mono-BoldItalic.otf"];
    let mut found: Vec<Vec<u8>> = files.iter().filter_map(|f| std::fs::read(format!("{dir}/{f}")).ok()).collect();
    if !found.is_empty() {
        term_view::SF_MONO_LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let Ok(sf) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        found.push(sf);
        SYSTEM_UI_FONT_LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    // Se cargan ya en el motor de texto compartido: así el ancho de los caracteres se puede medir al arrancar,
    // antes de que iced termine de registrar las fuentes de `.font(...)`.
    if let Ok(mut fs) = iced::advanced::graphics::text::font_system().write() {
        for f in &found {
            fs.raw().db_mut().load_font_data(f.clone());
        }
    }
    found
}

fn main() -> iced::Result {
    let mut app = iced::application(boot, App::update, App::view).font(ui::LUCIDE_FONT_BYTES);
    // Las fuentes del sistema ya quedan cargadas en el motor de texto compartido.
    let _ = load_system_fonts();
    let ui_font = if SYSTEM_UI_FONT_LOADED.load(std::sync::atomic::Ordering::Relaxed) { iced::Font::with_name("System Font") } else { iced::Font::DEFAULT };
    app
        .default_font(ui_font)
        .title("jTerm")
        .subscription(App::subscription)
        .theme(App::theme)
        .window(iced::window::Settings {
            size: Size::new(1100.0, 700.0),
            // Al cerrar la ventana se detienen los túneles antes de salir (no quedan procesos ssh huérfanos).
            exit_on_close_request: false,
            // Por debajo de este tamaño las barras de herramientas y los paneles dejan de verse bien.
            min_size: Some(Size::new(MIN_WINDOW.0, MIN_WINDOW.1)),
            // La barra de título se integra con la de pestañas (los botones de la ventana quedan sobre ella).
            #[cfg(target_os = "macos")]
            platform_specific: iced::window::settings::PlatformSpecific { title_hidden: true, titlebar_transparent: true, fullsize_content_view: true },
            ..Default::default()
        })
        .antialiasing(false)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_are_walked_in_spatial_order_and_neighbours_are_found() {
        let (mut g, a) = pane_grid::State::new("a");
        let (b, _) = g.split(pane_grid::Axis::Vertical, a, "b").unwrap(); // a | b
        let (c, _) = g.split(pane_grid::Axis::Horizontal, b, "c").unwrap(); // a | (b sobre c)
        let mut order = Vec::new();
        leaves(g.layout(), &mut order);
        assert_eq!(order.iter().map(|p| *g.get(*p).unwrap()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(g.adjacent(a, pane_grid::Direction::Right), Some(b));
        assert_eq!(g.adjacent(c, pane_grid::Direction::Up), Some(b));
        assert_eq!(g.adjacent(c, pane_grid::Direction::Left), Some(a));
        assert_eq!(g.adjacent(a, pane_grid::Direction::Left), None);
        // Cerrar un panel deja a su vecino con el foco.
        let (_, sibling) = g.close(c).unwrap();
        assert_eq!(sibling, b);
        assert_eq!(g.len(), 2);
        assert!(g.close(a).is_some() && g.close(b).is_none(), "el último panel no se puede cerrar: se cierra la pestaña");
    }

    #[test]
    fn shade_moves_toward_white_and_black() {
        let c = Color::from_rgb(0.5, 0.5, 0.5);
        assert!(shade(c, 0.5).r > 0.5 && shade(c, -0.5).r < 0.5);
        assert_eq!(shade(c, 0.0), c);
    }

    #[test]
    fn missing_settings_file_gives_defaults() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load_settings(d.path()), Settings::default());
        std::fs::write(d.path().join("settings.json"), r#"{"theme":"Nord"}"#).unwrap();
        assert_eq!(load_settings(d.path()).theme, "Nord");
    }
}
