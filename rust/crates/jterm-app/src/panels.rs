//! Ajustes (ventana con páginas) y paleta de comandos guardados.

use crate::colors::Palette;
use crate::editor::{action_button, dialog};
use crate::i18n::tr;
use crate::Message;
use iced::widget::{button, checkbox, column, container, row, scrollable, space, text, toggler, Column};
use crate::ui::{select as pick_list, input as text_input};
use iced::{Background, Border, Color, Element, Length};
use jterm_core::models::Snippet;
use jterm_core::settings::{installed_shells, Settings};
use jterm_core::themes::{ACCENTS, THEMES};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    General,
    Terminal,
    Ssh,
    ICloud,
}

impl Page {
    pub const ALL: [Page; 4] = [Page::General, Page::Terminal, Page::Ssh, Page::ICloud];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Terminal => "Terminal",
            Page::Ssh => "SSH",
            Page::ICloud => "iCloud",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Page::General => "⚙",
            Page::Terminal => "▮",
            Page::Ssh => "⇄",
            Page::ICloud => "☁",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    LoginShell,
    OptionAsMeta,
    WarnDangerousProd,
    SidebarAtLaunch,
    ShowSnippetsButton,
    FontMedium,
    MouseReporting,
    UnderlineLinks,
    BackspaceCtrlH,
    SshCompression,
    SshForwardAgent,
}

#[derive(Debug, Clone)]
pub enum Setting {
    Theme(String),
    Accent(String),
    FontFamily(String),
    FontSize(i32),
    Scrollback(u32),
    CursorShape(String),
    Language(String),
    Shell(String),
    StartDir(String),
    Term(String),
    KeepAlive(i32),
    HostKey(String),
    DefaultUser(String),
    DefaultPort(i32),
    Flag(Flag, bool),
    Reset,
}

pub const LANGUAGES: [(&str, &str); 3] = [("system", "Automático (idioma del sistema)"), ("es", "Español"), ("en", "English")];
pub const SYSTEM_SHELL: &str = "Predeterminado del sistema";
pub const SYSTEM_FONT: &str = "Sistema (SF Mono)";
pub const FONTS: [&str; 4] = [SYSTEM_FONT, "Menlo", "Monaco", "Courier New"];
pub const CURSORS: [(&str, &str); 3] = [("block", "Bloque"), ("underline", "Subrayado"), ("beam", "Barra")];
pub const TERMS: [&str; 4] = ["xterm-256color", "xterm", "screen-256color", "vt100"];
pub const HOST_KEYS: [(&str, &str); 2] = [("accept-new", "Aceptar y recordar (recomendado)"), ("yes", "Estricto: solo hosts ya conocidos")];
pub const SCROLLBACKS: [u32; 5] = [1_000, 5_000, 10_000, 50_000, 100_000];

/// Aplica un cambio a los ajustes (lógica pura, probada aparte de la interfaz).
pub fn apply(s: &mut Settings, c: Setting) {
    match c {
        Setting::Theme(t) => s.theme = t,
        Setting::Accent(a) => s.accent = a,
        Setting::FontFamily(f) => s.font_family = if f == SYSTEM_FONT { "system".into() } else { f },
        Setting::FontSize(d) => s.font_size = (s.font_size as i32 + d).clamp(9, 32) as u32,
        Setting::Scrollback(n) => s.scrollback = n,
        Setting::CursorShape(c) => s.cursor_shape = c,
        Setting::Language(l) => s.language = LANGUAGES.iter().find(|(_, label)| *label == l).map(|(c, _)| c.to_string()).unwrap_or_else(|| "system".into()),
        Setting::Shell(p) => s.shell = if p == SYSTEM_SHELL { String::new() } else { p },
        Setting::StartDir(p) => s.start_dir = p,
        Setting::Term(t) => s.term = t,
        Setting::KeepAlive(d) => s.ssh_keep_alive = (s.ssh_keep_alive as i32 + d).clamp(0, 600) as u32,
        Setting::HostKey(h) => s.ssh_host_key = h,
        Setting::DefaultUser(u) => s.default_user = u,
        Setting::DefaultPort(d) => s.default_port = (s.default_port as i32 + d).clamp(1, 65535) as u16,
        Setting::Flag(f, v) => match f {
            Flag::LoginShell => s.login_shell = v,
            Flag::OptionAsMeta => s.option_as_meta = v,
            Flag::WarnDangerousProd => s.warn_dangerous_prod = v,
            Flag::SidebarAtLaunch => s.sidebar_at_launch = v,
            Flag::ShowSnippetsButton => s.show_snippets_button = v,
            Flag::FontMedium => s.font_medium = v,
            Flag::MouseReporting => s.mouse_reporting = v,
            Flag::UnderlineLinks => s.underline_links = v,
            Flag::BackspaceCtrlH => s.backspace_ctrl_h = v,
            Flag::SshCompression => s.ssh_compression = v,
            Flag::SshForwardAgent => s.ssh_forward_agent = v,
        },
        Setting::Reset => *s = Settings::default(),
    }
}

// MARK: Piezas de la ventana de ajustes

fn dim(p: &Palette) -> Color {
    Color { a: 0.55, ..p.fg }
}

/// Fila: título y descripción a la izquierda, control a la derecha.
fn setting_row<'a>(p: &Palette, title: &'a str, detail: Option<&'a str>, control: Element<'a, Message>) -> Element<'a, Message> {
    let mut left: Column<Message> = column![text(tr(title)).size(13).color(p.fg)].spacing(3);
    if let Some(d) = detail {
        left = left.push(text(tr(d)).size(11).color(dim(p)));
    }
    row![left.width(Length::Fill), container(control).align_x(iced::alignment::Horizontal::Right)]
        .spacing(16)
        .align_y(iced::Alignment::Center)
        .into()
}

/// Igual que `setting_row` pero con una descripción que cambia (texto propio).
fn setting_row_owned<'a>(p: &Palette, title: &'a str, detail: String, control: Element<'a, Message>) -> Element<'a, Message> {
    let left: Column<Message> = column![text(tr(title)).size(13).color(p.fg), text(tr(&detail)).size(11).color(dim(p))].spacing(3);
    row![left.width(Length::Fill), container(control).align_x(iced::alignment::Horizontal::Right)].spacing(16).align_y(iced::Alignment::Center).into()
}

/// Tarjeta con título de sección, filas separadas por una línea fina y nota al pie opcional.
fn section<'a>(p: &Palette, title: &'a str, rows: Vec<Element<'a, Message>>, footer: Option<&'a str>) -> Element<'a, Message> {
    let line = Color { a: 0.08, ..p.fg };
    let mut card: Column<Message> = Column::new().spacing(0);
    let n = rows.len();
    for (i, r) in rows.into_iter().enumerate() {
        card = card.push(container(r).padding([11, 14]).width(Length::Fill));
        if i + 1 < n {
            card = card.push(container(space::vertical()).height(1).width(Length::Fill).style(move |_| container::Style { background: Some(Background::Color(line)), ..Default::default() }));
        }
    }
    let card = container(card).width(Length::Fill).style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.045, ..Color::WHITE })),
        border: Border { radius: 10.0.into(), width: 1.0, color: line },
        ..Default::default()
    });
    let mut out: Column<Message> = column![text(tr(title)).size(12).color(dim(p)), card].spacing(6);
    if let Some(f) = footer {
        out = out.push(text(tr(f)).size(11).color(dim(p)));
    }
    out.into()
}

fn switch<'a>(v: bool, f: Flag) -> Element<'a, Message> {
    toggler(v).on_toggle(move |b| Message::Setting(Setting::Flag(f, b))).size(18).into()
}

fn stepper<'a>(p: &Palette, label: String, minus: Setting, plus: Setting) -> Element<'a, Message> {
    row![
        action_button("−", p.fg, p.fg, false).on_press(Message::Setting(minus)),
        container(text(tr(&label)).size(13).color(p.fg)).width(Length::Fixed(78.0)).align_x(iced::alignment::Horizontal::Center),
        action_button("+", p.fg, p.fg, false).on_press(Message::Setting(plus)),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

fn choice<'a, T>(options: Vec<T>, selected: Option<T>, width: f32, on: impl Fn(T) -> Message + 'a) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    // Las opciones se muestran traducidas; el valor que se devuelve sigue siendo el original.
    let opts: Vec<crate::i18n::Tr<T>> = options.into_iter().map(crate::i18n::Tr).collect();
    pick_list(opts, selected.map(crate::i18n::Tr), move |t: crate::i18n::Tr<T>| on(t.0)).width(Length::Fixed(width)).text_size(13).into()
}

fn page_general<'a>(s: &'a Settings, p: &Palette) -> Vec<Element<'a, Message>> {
    let mut shells = vec![SYSTEM_SHELL.to_string()];
    shells.extend(installed_shells());
    let cur = if s.shell.is_empty() { SYSTEM_SHELL.to_string() } else { s.shell.clone() };
    vec![
        section(
            p,
            "Terminal local",
            vec![
                setting_row(p, "Shell", Some("Programa que se ejecuta en las pestañas locales nuevas."), choice(shells, Some(cur), 230.0, |v| Message::Setting(Setting::Shell(v)))),
                setting_row(p, "Shell de inicio de sesión", Some("Ejecuta el shell con -l para cargar tu perfil (.zprofile, PATH, etc.)."), switch(s.login_shell, Flag::LoginShell)),
                setting_row(
                    p,
                    "Carpeta inicial",
                    Some("Directorio donde abren las pestañas locales nuevas. Vacío = carpeta personal."),
                    text_input("Carpeta personal", &s.start_dir).on_input(|v| Message::Setting(Setting::StartDir(v))).padding(6).size(13).width(Length::Fixed(230.0)).into(),
                ),
            ],
            Some("Los cambios se aplican a las pestañas nuevas."),
        ),
        section(
            p,
            "Idioma",
            vec![setting_row(
                p,
                "Idioma de la interfaz",
                Some("Cambia el idioma de los menús, ajustes y diálogos. Se aplica al reiniciar jTerm."),
                choice(
                    LANGUAGES.iter().map(|(_, l)| l.to_string()).collect(),
                    LANGUAGES.iter().find(|(c, _)| *c == s.language).map(|(_, l)| l.to_string()),
                    230.0,
                    |l| Message::Setting(Setting::Language(l)),
                ),
            )],
            None,
        ),
        section(p, "Ventana", vec![
                setting_row(p, "Mostrar la barra lateral al iniciar", None, switch(s.sidebar_at_launch, Flag::SidebarAtLaunch)),
                setting_row(p, "Mostrar el botón de comandos guardados", Some("Botón flotante en la esquina inferior derecha del terminal. Si lo ocultas, sigues pudiendo abrir la lista con ⇧⌘K."), switch(s.show_snippets_button, Flag::ShowSnippetsButton)),
            ], None),
        section(
            p,
            "Confirmaciones",
            vec![setting_row(
                p,
                "Avisar de comandos delicados en producción",
                Some("En pestañas con entorno Producción pregunta antes de ejecutar rm, DROP, TRUNCATE, DELETE sin WHERE, docker rm, kubectl delete, reinicios y similares."),
                switch(s.warn_dangerous_prod, Flag::WarnDangerousProd),
            )],
            Some("El entorno de cada servidor se elige al editarlo."),
        ),
        section(
            p,
            "Datos",
            vec![setting_row(p, "Restablecer ajustes", Some("Vuelve todos los ajustes a sus valores por defecto. No toca servidores ni comandos."), action_button("Restablecer", Color::from_rgb8(0xff, 0x6b, 0x6b), p.fg, false).on_press(Message::Setting(Setting::Reset)).into())],
            None,
        ),
    ]
}

fn page_terminal<'a>(s: &'a Settings, p: &Palette) -> Vec<Element<'a, Message>> {
    let themes: Vec<String> = THEMES.iter().map(|t| t.name.to_string()).collect();
    let accents: Vec<String> = ACCENTS.iter().map(|(n, _)| n.to_string()).collect();
    let fonts: Vec<String> = FONTS.iter().map(|f| f.to_string()).collect();
    let font_cur = if s.font_family == "system" { SYSTEM_FONT.to_string() } else { s.font_family.clone() };
    let cursors: Vec<String> = CURSORS.iter().map(|(_, l)| l.to_string()).collect();
    let cursor_cur = CURSORS.iter().find(|(k, _)| *k == s.cursor_shape).map(|(_, l)| l.to_string());
    let terms: Vec<String> = TERMS.iter().map(|t| t.to_string()).collect();
    vec![
        section(
            p,
            "Apariencia",
            vec![
                setting_row(p, "Tema de colores", None, choice(themes, Some(s.theme.clone()), 190.0, |t| Message::Setting(Setting::Theme(t)))),
                setting_row(p, "Color de acento", Some("Botones, selección y resaltados de la app."), choice(accents, Some(s.accent.clone()), 150.0, |a| Message::Setting(Setting::Accent(a)))),
                setting_row(p, "Fuente", Some("Solo fuentes monoespaciadas."), choice(fonts, Some(font_cur), 230.0, |f| Message::Setting(Setting::FontFamily(f)))),
                setting_row(p, "Tamaño de fuente", None, stepper(p, format!("{} pt", s.font_size), Setting::FontSize(-1), Setting::FontSize(1))),
                setting_row(p, "Texto más grueso", Some("Usa el peso Medium. Ayuda si el texto se ve fino sobre fondos oscuros."), switch(s.font_medium, Flag::FontMedium)),
            ],
            None,
        ),
        section(
            p,
            "Cursor",
            vec![setting_row(p, "Forma", None, choice(cursors, cursor_cur, 150.0, |l| {
                let key = CURSORS.iter().find(|(_, label)| *label == l).map(|(k, _)| k.to_string()).unwrap_or_else(|| "block".into());
                Message::Setting(Setting::CursorShape(key))
            }))],
            None,
        ),
        section(
            p,
            "Comportamiento",
            vec![
                setting_row(p, "Líneas de historial", Some("Más líneas permiten desplazarte más atrás, pero usan más memoria por pestaña."), choice(SCROLLBACKS.to_vec(), Some(s.scrollback), 120.0, |n| Message::Setting(Setting::Scrollback(n)))),
                setting_row(p, "Subrayar las URLs de la salida", Some("Las URLs se ven subrayadas; ⌘+clic las abre. Con ⌘ pulsado también se subrayan las rutas de archivo que existen (solo en pestañas locales)."), switch(s.underline_links, Flag::UnderlineLinks)),
                setting_row(p, "Permitir que las aplicaciones capturen el mouse", Some("Necesario para tmux, vim o htop con ratón. Si lo desactivas, el mouse solo selecciona texto. Con Mayús pulsado siempre selecciona."), switch(s.mouse_reporting, Flag::MouseReporting)),
                setting_row(p, "Usar Option como Meta", Some("Permite atajos como Option+← para saltar palabras en el shell. AltGr sigue escribiendo @ | #."), switch(s.option_as_meta, Flag::OptionAsMeta)),
            ],
            Some("Los cambios de historial se aplican a las pestañas nuevas."),
        ),
        section(
            p,
            "Compatibilidad",
            vec![
                setting_row(p, "Tipo de terminal (TERM)", Some("Algunos servidores antiguos no conocen xterm-256color. Se aplica a las pestañas nuevas."), choice(terms, Some(s.term.clone()), 190.0, |t| Message::Setting(Setting::Term(t)))),
                setting_row(p, "La tecla Borrar envía Ctrl-H", Some("Actívalo si en algún servidor la tecla Borrar no funciona bien."), switch(s.backspace_ctrl_h, Flag::BackspaceCtrlH)),
            ],
            None,
        ),
    ]
}

fn page_ssh<'a>(s: &'a Settings, p: &Palette) -> Vec<Element<'a, Message>> {
    let hosts: Vec<String> = HOST_KEYS.iter().map(|(_, l)| l.to_string()).collect();
    let host_cur = HOST_KEYS.iter().find(|(k, _)| *k == s.ssh_host_key).map(|(_, l)| l.to_string());
    let keep = if s.ssh_keep_alive == 0 { "Desactivado".to_string() } else { format!("{} s", s.ssh_keep_alive) };
    vec![
        section(
            p,
            "Conexión",
            vec![
                setting_row(p, "Mantener la conexión activa", Some("Envía un latido cada cierto tiempo para que el servidor o el router no cierren sesiones inactivas."), stepper(p, keep, Setting::KeepAlive(-5), Setting::KeepAlive(5))),
                setting_row(p, "Hosts desconocidos", Some("Qué hacer la primera vez que te conectas a un servidor. Si su clave cambia después, la conexión siempre se rechaza."), choice(hosts, host_cur, 250.0, |l| {
                    let key = HOST_KEYS.iter().find(|(_, label)| *label == l).map(|(k, _)| k.to_string()).unwrap_or_else(|| "accept-new".into());
                    Message::Setting(Setting::HostKey(key))
                })),
                setting_row(p, "Comprimir el tráfico", Some("Útil en conexiones lentas; en redes rápidas puede ser más lento."), switch(s.ssh_compression, Flag::SshCompression)),
                setting_row(p, "Reenviar el agente SSH", Some("Permite que el servidor use tus llaves locales para saltar a otros equipos. Actívalo solo en servidores de confianza."), switch(s.ssh_forward_agent, Flag::SshForwardAgent)),
            ],
            Some("Se aplica a las conexiones nuevas."),
        ),
        section(
            p,
            "Servidores nuevos",
            vec![
                setting_row(p, "Usuario por defecto", Some("Se rellena al agregar un servidor."), text_input("usuario", &s.default_user).on_input(|v| Message::Setting(Setting::DefaultUser(v))).padding(6).size(13).width(Length::Fixed(190.0)).into()),
                setting_row(p, "Puerto por defecto", None, stepper(p, s.default_port.to_string(), Setting::DefaultPort(-1), Setting::DefaultPort(1))),
            ],
            None,
        ),
    ]
}

fn page_icloud<'a>(v: &crate::sync_ui::SyncView, p: &Palette) -> Vec<Element<'a, Message>> {
    use crate::sync_ui::KindFlag;
    let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
    let mut blocks: Vec<Element<'a, Message>> = vec![];
    if !v.folder_ok {
        blocks.push(text(tr("iCloud Drive no está activo en este Mac. Actívalo en Ajustes del Sistema ▸ tu nombre ▸ iCloud ▸ Unidad de iCloud para poder sincronizar.")).size(12).color(Color::from_rgb8(0xf2, 0xc9, 0x7d)).into());
    }
    let toggle = |on: bool, f: Box<dyn Fn(bool) -> Message + 'a>| -> Element<'a, Message> { toggler(on).on_toggle(f).size(18).into() };
    let mut main: Vec<Element<'a, Message>> = vec![setting_row(
        p,
        "Sincronizar con iCloud",
        Some("Mantiene tus conexiones, comandos y ajustes iguales en todos tus Mac. Los datos se guardan cifrados en la carpeta “jTerm” de tu iCloud Drive."),
        toggle(v.prefs.enabled, Box::new(Message::SyncToggle)),
    )];
    if v.prefs.enabled {
        let btn = action_button(if v.syncing { "Sincronizando…" } else { "Sincronizar ahora" }, fg, fg, false);
        let btn = if v.syncing { btn } else { btn.on_press(Message::SyncNow) };
        main.push(setting_row_owned(p, "Estado", v.status.clone(), btn.into()));
        if v.needs_pass {
            main.push(setting_row(
                p,
                if v.missing_pass { "Falta la contraseña de sincronización" } else { "La contraseña de sincronización no coincide" },
                Some(if v.missing_pass { "Ingrésala para seguir sincronizando." } else { "Cambió en otro Mac o se ingresó mal. Ingrésala de nuevo para seguir sincronizando." }),
                action_button("Ingresar contraseña…", Color::from_rgb8(0x5c, 0x9e, 0xff), fg, true).on_press(Message::SyncReenter).into(),
            ));
        }
    }
    blocks.push(section(p, "Sincronización", main, None));
    if !v.prefs.enabled {
        blocks.push(text(tr("El cambio más reciente de cada elemento gana, y los borrados también se propagan. Tus datos viajan cifrados con una contraseña que solo tú conoces: ni Apple ni nadie con acceso a tu iCloud puede leerlos.")).size(12).color(dim).into());
        return blocks;
    }
    let kind_row = |label: &'static str, detail: Option<&'static str>, on: bool, k: KindFlag| setting_row(p, label, detail, toggle(on, Box::new(move |b| Message::SyncKind(k, b))));
    blocks.push(section(
        p,
        "Qué sincronizar",
        vec![
            kind_row("Conexiones (servidores)", None, v.prefs.servers, KindFlag::Servers),
            kind_row("Cuentas de AWS", None, v.prefs.aws, KindFlag::Aws),
            kind_row("Comandos guardados", None, v.prefs.commands, KindFlag::Commands),
            kind_row("Ajustes", Some("Tema, fuente, SSH y demás. No incluye lo propio de cada Mac, como el shell o las carpetas iniciales."), v.prefs.settings, KindFlag::Settings),
            setting_row(
                p,
                "Incluir contraseñas y claves secretas",
                Some("Contraseñas de servidores, passphrases y claves secretas de AWS. Viajan cifradas con tu contraseña de sincronización. Actívalo en cada Mac donde quieras recibirlas."),
                toggle(v.prefs.secrets, Box::new(Message::SyncSecrets)),
            ),
        ],
        None,
    ));
    blocks.push(section(
        p,
        "Seguridad",
        vec![
            setting_row(p, "Contraseña de sincronización", Some("Cifra tus datos en iCloud. Si la cambias, tendrás que ingresar la nueva en tus otros Mac."), action_button("Cambiar…", fg, fg, false).on_press(Message::SyncChangePass).into()),
            setting_row(p, "Borrar los datos de iCloud", Some("Elimina el archivo de sincronización y desactiva la sincronización aquí. Tus datos en este Mac no se tocan."), action_button("Borrar…", Color::from_rgb8(0xff, 0x6b, 0x6b), fg, false).on_press(Message::SyncDelete).into()),
        ],
        None,
    ));
    blocks
}

pub fn settings_view<'a>(s: &'a Settings, page: Page, p: &Palette, accent: Color, sync: &crate::sync_ui::SyncView) -> Element<'a, Message> {
    let fg = p.fg;
    let mut nav: Column<Message> = Column::new().spacing(2).width(Length::Fixed(150.0));
    for pg in Page::ALL {
        let sel = pg == page;
        nav = nav.push(
            button(row![text(pg.icon()).size(14).color(if sel { accent } else { dim(p) }), text(tr(pg.title())).size(13).color(fg)].spacing(10).align_y(iced::Alignment::Center))
                .width(Length::Fill)
                .padding([7, 10])
                .on_press(Message::SettingsPage(pg))
                .style(move |_, st| button::Style {
                    background: Some(Background::Color(if sel { Color { a: 0.16, ..accent } } else if matches!(st, button::Status::Hovered) { Color { a: 0.07, ..fg } } else { Color::TRANSPARENT })),
                    text_color: fg,
                    border: Border { radius: 7.0.into(), ..Default::default() },
                    ..Default::default()
                }),
        );
    }
    let sections = match page {
        Page::General => page_general(s, p),
        Page::Terminal => page_terminal(s, p),
        Page::Ssh => page_ssh(s, p),
        Page::ICloud => page_icloud(sync, p),
    };
    let content = Column::with_children(sections).spacing(18).padding(iced::Padding { right: 14.0, ..Default::default() });
    let body = column![
        row![
            text(tr("Ajustes")).size(16).color(fg),
            space::horizontal(),
            action_button("Cerrar", fg, fg, false).on_press(Message::ClosePanel)
        ]
        .align_y(iced::Alignment::Center),
        row![nav, scrollable(content).width(Length::Fill).height(Length::Fill)].spacing(18).height(Length::Fill),
    ]
    .spacing(14)
    .height(Length::Fill);
    container(dialog(body.into(), p, 820.0)).height(Length::Fill).max_height(600.0).into()
}

#[derive(Default)]
pub struct SnippetForm {
    pub id: Option<Uuid>,
    pub name: String,
    /// Editor multilínea: un comando puede tener varias líneas (scripts, heredocs…).
    pub command: iced::widget::text_editor::Content,
    pub auto_run: bool,
}

impl SnippetForm {
    pub fn from(s: &Snippet) -> Self {
        SnippetForm { id: Some(s.id), name: s.name.clone(), command: iced::widget::text_editor::Content::with_text(&s.command), auto_run: s.auto_run }
    }

    /// Texto del comando sin el salto de línea final que añade el editor.
    pub fn command_text(&self) -> String {
        let t = self.command.text();
        t.strip_suffix('\n').unwrap_or(&t).to_string()
    }

    /// El comando resultante, o `None` si no hay texto que guardar.
    pub fn build(&self) -> Option<Snippet> {
        let command = self.command_text();
        if command.trim().is_empty() {
            return None;
        }
        let mut s = Snippet::new(self.name.trim(), &command);
        if let Some(id) = self.id {
            s.id = id;
        }
        s.auto_run = self.auto_run;
        Some(s)
    }
}

#[derive(Default)]
pub struct SnippetState {
    pub query: String,
    pub form: Option<SnippetForm>,
    /// Comando que se está arrastrando para reordenar.
    pub drag: Option<Uuid>,
    /// Fila sobre la que está el cursor durante el arrastre (destino del soltado).
    pub over: Option<Uuid>,
}

/// Vista previa de una línea para el listado: recorta las líneas largas para que no desborden la fila.
fn preview(line: &str) -> String {
    const MAX: usize = 56;
    if line.chars().count() > MAX { line.chars().take(MAX - 1).collect::<String>() + "…" } else { line.to_string() }
}

pub const SEARCH_ID: &str = "snippet-search";

pub fn snippets_view<'a>(items: Vec<&'a Snippet>, st: &'a SnippetState, p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
    if let Some(f) = &st.form {
        let body = column![
            text(tr(if f.id.is_some() { "Editar comando" } else { "Nuevo comando" })).size(16).color(fg),
            column![
                text(tr("NOMBRE (OPCIONAL)")).size(10).color(dim),
                text_input("Reiniciar servicio", &f.name).on_input(|s| Message::SnipField(false, s)).padding(7).size(13)
            ]
            .spacing(4),
            column![
                text(tr("COMANDO")).size(10).color(dim),
                iced::widget::text_editor(&f.command).placeholder("sudo systemctl restart nginx").on_action(Message::SnipEditor).padding(7).size(13).font(crate::term_view::FONT).height(Length::Fixed(150.0))
            ]
            .spacing(4),
            checkbox(f.auto_run).label("Ejecutar al insertar (pulsa Retorno)").on_toggle(Message::SnipAutoRun).size(16).text_size(13),
            row![
                space::horizontal(),
                action_button("Cancelar", fg, fg, false).on_press(Message::SnipBack),
                action_button("Guardar", accent, fg, true).on_press(Message::SnipSave),
            ]
            .spacing(8),
        ]
        .spacing(12);
        return dialog(body.into(), p, 460.0);
    }

    let mut list: Column<Message> = Column::new().spacing(2);
    if items.is_empty() {
        list = list.push(text(tr(if st.query.is_empty() { "Aún no hay comandos guardados." } else { "Sin resultados." })).size(12).color(dim));
    }
    // Reordenar solo tiene sentido con la lista completa (sin filtro de búsqueda).
    let reorder = st.query.trim().is_empty();
    // Posiciones de origen y destino del arrastre, para dibujar la línea de inserción.
    let pos = |id: Option<Uuid>| id.and_then(|id| items.iter().position(|s| s.id == id));
    let (from, to) = (pos(st.drag), pos(st.over));
    for (i, s) in items.iter().copied().enumerate() {
        let id = s.id;
        let title = s.title();
        let mut info: Column<Message> = column![text(title).size(13).color(fg)].spacing(1);
        let first = s.first_line();
        if !s.name.trim().is_empty() && !first.is_empty() {
            info = info.push(text(preview(&first)).size(11).font(crate::term_view::FONT).color(dim).wrapping(iced::widget::text::Wrapping::None));
        }
        let mut tags = row![].spacing(6);
        if s.auto_run {
            tags = tags.push(text(tr("↩")).size(11).color(accent));
        }
        if s.line_count() > 1 {
            tags = tags.push(text(tr(&format!("{} líneas", s.line_count()))).size(10).color(dim));
        }
        let pick = iced::widget::button(row![container(info).width(Length::Fill).clip(true), tags].align_y(iced::Alignment::Center).spacing(8))
            .width(Length::Fill)
            .padding([6, 8])
            .on_press(Message::SnipInsert(id))
            .style(move |_, status| iced::widget::button::Style {
                background: Some(Background::Color(if matches!(status, iced::widget::button::Status::Hovered) { Color { a: 0.12, ..fg } } else { Color::TRANSPARENT })),
                text_color: fg,
                border: Border { radius: 6.0.into(), ..Default::default() },
                ..Default::default()
            });
        let dragging = st.drag == Some(id);
        let handle: Element<Message> = if reorder {
            iced::widget::mouse_area(container(text("⠿").size(16).color(if dragging { accent } else { dim })).padding([4, 6]))
                .on_press(Message::SnipDragStart(id))
                .interaction(if st.drag.is_some() { iced::mouse::Interaction::Grabbing } else { iced::mouse::Interaction::Grab })
                .into()
        } else {
            space::horizontal().width(Length::Shrink).into()
        };
        let line = row![handle, pick, action_button("✎", dim, fg, false).on_press(Message::SnipEdit(id)), action_button("🗑", dim, fg, false).on_press(Message::SnipDelete(id))].spacing(2).align_y(iced::Alignment::Center);
        let line = container(line).style(move |_| container::Style {
            background: dragging.then(|| Background::Color(Color { a: 0.10, ..accent })),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        });
        // Línea de inserción: encima del destino al subir, debajo al bajar. Siempre ocupa su sitio para no mover la lista.
        let bar = |on: bool| container(space::vertical().height(Length::Fixed(2.0))).width(Length::Fill).style(move |_| container::Style {
            background: on.then(|| Background::Color(accent)),
            ..Default::default()
        });
        let target = matches!((from, to), (Some(f), Some(t)) if f != t && t == i);
        let below = matches!((from, to), (Some(f), Some(t)) if f < t);
        let cell = column![bar(target && !below), line, bar(target && below)].spacing(0);
        list = list.push(if reorder { iced::widget::mouse_area(cell).on_enter(Message::SnipDragOver(id)).into() } else { Element::from(cell) });
    }
    let body = column![
        row![text(tr("Comandos guardados")).size(16).color(fg), space::horizontal(), action_button("+ Nuevo", accent, fg, true).on_press(Message::SnipNew)].align_y(iced::Alignment::Center),
        text_input("Buscar por nombre o comando…", &st.query).id(SEARCH_ID).on_input(Message::SnipQuery).padding(7).size(13),
        container(scrollable(list)).max_height(320),
        row![space::horizontal(), action_button("Cerrar", fg, fg, false).on_press(Message::ClosePanel)],
    ]
    .spacing(12);
    // Soltar en cualquier punto del diálogo termina el arrastre.
    iced::widget::mouse_area(dialog(body.into(), p, 520.0)).on_release(Message::SnipDrop).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_changes() {
        let mut s = Settings::default();
        apply(&mut s, Setting::Theme("Nord".into()));
        apply(&mut s, Setting::FontSize(2));
        apply(&mut s, Setting::Flag(Flag::OptionAsMeta, false));
        assert_eq!((s.theme.as_str(), s.font_size, s.option_as_meta), ("Nord", 15, false));
        apply(&mut s, Setting::FontSize(100));
        assert_eq!(s.font_size, 32);
        apply(&mut s, Setting::FontSize(-100));
        assert_eq!(s.font_size, 9);
        apply(&mut s, Setting::KeepAlive(-100));
        assert_eq!(s.ssh_keep_alive, 0);
        apply(&mut s, Setting::DefaultPort(-100));
        assert_eq!(s.default_port, 1);
        apply(&mut s, Setting::Shell(SYSTEM_SHELL.into()));
        assert!(s.shell.is_empty());
        apply(&mut s, Setting::FontFamily(SYSTEM_FONT.into()));
        assert_eq!(s.font_family, "system");
        apply(&mut s, Setting::FontFamily("Menlo".into()));
        assert_eq!(s.font_family, "Menlo");
        apply(&mut s, Setting::Reset);
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn snippet_form_builds_only_with_a_command() {
        let mut f = SnippetForm::default();
        assert!(f.build().is_none());
        f.command = iced::widget::text_editor::Content::with_text("ls -la");
        f.name = " listar ".into();
        f.auto_run = true;
        let s = f.build().unwrap();
        assert_eq!((s.name.as_str(), s.command.as_str(), s.auto_run), ("listar", "ls -la", true));
        let id = Uuid::new_v4();
        f.id = Some(id);
        assert_eq!(f.build().unwrap().id, id);
    }
}
