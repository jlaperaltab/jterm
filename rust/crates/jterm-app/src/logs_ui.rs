//! Visor de logs de CloudWatch: estado, consultas (paginadas, genéricas sobre el transporte) y vista.
//! No llama a AWS por sí mismo: las consultas se lanzan desde los botones de la vista.

use crate::colors::Palette;
use crate::editor::{action_button, dialog};
use crate::term_view::FONT;
use crate::i18n::tr;
use crate::Message;
use chrono::{Local, TimeZone};
use iced::widget::{button, column, container, row, scrollable, space, text, Column, Row};
use crate::ui::{select as pick_list, input as text_input};
use iced::{Background, Border, Color, Element, Length};
use jterm_core::aws::{AwsAccount, FilterParams, LogEvent, LogGroup, REGIONS};
use jterm_core::logs::{self, LogLevel, ParsedLog, Platform, RidTracker, Row as LogRow, Token};
use jterm_core::sigv4::Credentials;
use jterm_net::aws_client::AwsClient;
use jterm_net::transport::{ReqwestTransport, Transport};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

pub type Client = AwsClient<ReqwestTransport>;

/// Tope de eventos de una búsqueda y de lo que se conserva siguiendo en vivo.
pub const MAX_SEARCH_EVENTS: usize = 10_000;
pub const MAX_KEPT_EVENTS: usize = 20_000;
/// Filas que se dibujan a la vez (las últimas); «Mostrar anteriores» amplía la ventana.
pub const WINDOW_STEP: usize = 300;
pub const FOLLOW_SECS: u64 = 3;
/// Caracteres del nombre del grupo que caben en su campo (el resto se recorta por la izquierda).
pub const GROUP_LABEL_CHARS: usize = 30;
pub const SCROLL_ID: &str = "logs-scroll";

pub const RANGES: [(&str, i64); 7] = [
    ("Últimos 5 minutos", 300),
    ("Últimos 15 minutos", 900),
    ("Última hora", 3600),
    ("Últimas 6 horas", 21_600),
    ("Últimas 24 horas", 86_400),
    ("Últimos 3 días", 259_200),
    ("Últimos 7 días", 604_800),
];

pub fn range_seconds(label: &str) -> i64 {
    RANGES.iter().find(|(l, _)| *l == label).map(|(_, s)| *s).unwrap_or(3600)
}

// MARK: Consultas

/// Todos los grupos de la región (hasta 40 páginas), ordenados sin distinguir mayúsculas.
pub async fn fetch_groups<T: Transport>(client: &AwsClient<T>, c: &Credentials, region: &str) -> Result<Vec<LogGroup>, String> {
    let mut all = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..40 {
        let (mut g, next) = client.describe_log_groups(c, region, token.as_deref()).await.map_err(|e| aws_hint(&e.0))?;
        all.append(&mut g);
        match next {
            Some(t) => token = Some(t),
            None => break,
        }
    }
    all.sort_by_key(|g| g.name.to_lowercase());
    Ok(all)
}

/// Una página de eventos de la búsqueda.
pub async fn fetch_page<T: Transport>(
    client: &AwsClient<T>, c: &Credentials, region: &str, group: &str, start: i64, pattern: &str, prefix: &str, token: Option<&str>,
) -> Result<(Vec<LogEvent>, Option<String>), String> {
    let p = FilterParams { group, start, end: None, pattern, stream_prefix: prefix, next_token: token, limit: 1000 };
    client.filter_log_events(c, region, &p).await.map_err(|e| aws_hint(&e.0))
}

/// Eventos nuevos desde `start` (hasta 5 páginas) para el modo «Seguir».
pub async fn fetch_follow<T: Transport>(
    client: &AwsClient<T>, c: &Credentials, region: &str, group: &str, start: i64, pattern: &str, prefix: &str,
) -> Result<Vec<LogEvent>, String> {
    let mut all = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..5 {
        let (mut ev, next) = fetch_page(client, c, region, group, start, pattern, prefix, token.as_deref()).await?;
        all.append(&mut ev);
        match next {
            Some(t) => token = Some(t),
            None => break,
        }
    }
    Ok(all)
}

fn aws_hint(m: &str) -> String {
    jterm_core::aws::logs_hint(m)
}

// MARK: Estado

pub struct LogsState {
    pub account: AwsAccount,
    pub region: String,
    pub client: Arc<Client>,
    /// Se lee del Keychain la primera vez que hace falta (no al abrir la pestaña).
    pub creds: Option<Credentials>,
    pub groups: Vec<LogGroup>,
    pub groups_loading: bool,
    pub picker: Option<String>,
    pub group: Option<String>,
    pub range: String,
    pub filter: String,
    pub stream_prefix: String,
    pub parsed: Vec<ParsedLog>,
    seen: HashSet<String>,
    tracker: RidTracker,
    pub level: Option<LogLevel>,
    pub by_invocation: bool,
    pub pretty_json: bool,
    pub text_mode: bool,
    pub collapsed: HashSet<String>,
    pub expanded: HashSet<usize>,
    pub window: usize,
    pub searching: bool,
    pub following: bool,
    pub follow_busy: bool,
    pub follow_from: i64,
    pub last_poll: Option<Instant>,
    pub next_token: Option<String>,
    pub pages: usize,
    pub status: String,
    pub is_error: bool,
    pub generation: u64,
    /// ¿La lista está desplazada al final? (solo entonces el seguimiento en vivo la mantiene abajo)
    pub at_bottom: bool,
}

impl LogsState {
    pub fn new(account: AwsAccount, region: String, client: Arc<Client>) -> Self {
        LogsState {
            account, region, client, creds: None, groups: vec![], groups_loading: false, picker: None, group: None,
            range: RANGES[2].0.into(), filter: String::new(), stream_prefix: String::new(), parsed: vec![], seen: HashSet::new(),
            tracker: RidTracker::default(), level: None, by_invocation: true, pretty_json: true, text_mode: false,
            collapsed: HashSet::new(), expanded: HashSet::new(), window: WINDOW_STEP, searching: false, following: false,
            follow_busy: false, follow_from: 0, last_poll: None, next_token: None, pages: 0,
            status: "Elige un grupo de logs y pulsa Buscar.".into(), is_error: false, generation: 0, at_bottom: true,
        }
    }

    pub fn clear_results(&mut self) {
        self.parsed.clear();
        self.seen.clear();
        self.tracker = RidTracker::default();
        self.collapsed.clear();
        self.expanded.clear();
        self.window = WINDOW_STEP;
        self.next_token = None;
        self.pages = 0;
    }

    /// Añade eventos nuevos (sin repetir los ya vistos), ordenados por hora dentro de lo recibido. Devuelve cuántos entraron.
    pub fn append(&mut self, mut events: Vec<LogEvent>) -> usize {
        events.sort_by_key(|e| e.timestamp);
        let mut added = 0;
        for e in events {
            if !self.seen.insert(e.id.clone()) {
                continue;
            }
            let mut p = logs::parse(&e);
            self.tracker.assign(&mut p);
            self.parsed.push(p);
            added += 1;
        }
        // Conservar solo los últimos eventos; los índices expandidos dejan de valer, así que se reinician.
        if self.parsed.len() > MAX_KEPT_EVENTS {
            let drop_n = self.parsed.len() - MAX_KEPT_EVENTS;
            self.parsed.drain(..drop_n);
            self.expanded.clear();
        }
        added
    }

    pub fn event_count(&self) -> usize {
        self.parsed.len()
    }

    /// Filas visibles según nivel y agrupación (todas, sin ventana).
    pub fn rows(&self) -> Vec<LogRow> {
        logs::rows(&self.parsed, self.by_invocation, self.level, &self.collapsed)
    }

    pub fn count_level(&self, l: LogLevel) -> usize {
        self.parsed.iter().filter(|p| p.level == l).count()
    }

    /// Texto plano de todos los eventos (para copiar o guardar).
    pub fn plain_text(&self) -> String {
        self.parsed.iter().map(|p| format!("{} {}", format_time(p.event.timestamp, true), p.event.message.trim_end())).collect::<Vec<_>>().join("\n")
    }

    pub fn start_ms_for_range(&self, now_ms: i64) -> i64 {
        now_ms - range_seconds(&self.range) * 1000
    }
}

/// Recorta por el principio con «…» para que quepa en `max` caracteres (lo importante de una ruta está al final).
pub fn ellipsize_start(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - (max - 1)).collect();
    format!("…{tail}")
}

/// Hora local de un evento: `HH:MM:SS.mmm`, o con fecha si `with_date`.
pub fn format_time(ms: i64, with_date: bool) -> String {
    match Local.timestamp_millis_opt(ms).single() {
        Some(t) if with_date => t.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        Some(t) => t.format("%H:%M:%S%.3f").to_string(),
        None => String::new(),
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Resumen de una invocación para su encabezado.
pub fn group_summary(info: &logs::GroupInfo) -> String {
    let mut parts: Vec<String> = vec![if info.count == 1 { "1 evento".into() } else { format!("{} eventos", info.count) }];
    if let Some(r) = &info.report {
        if let Some(d) = r.duration_ms {
            parts.push(format!("{d:.0} ms"));
        }
        if let (Some(max), Some(size)) = (r.max_memory_mb, r.memory_size_mb) {
            parts.push(format!("{max}/{size} MB"));
        }
        if let Some(i) = r.init_ms {
            parts.push(format!("arranque en frío {i:.0} ms"));
        }
    } else if info.started && !info.ended {
        parts.push("en curso".into());
    }
    parts.join(" · ")
}

pub fn level_color(l: LogLevel) -> Color {
    match l {
        LogLevel::Error => Color::from_rgb8(0xff, 0x6b, 0x6b),
        LogLevel::Warn => Color::from_rgb8(0xf2, 0xc9, 0x7d),
        LogLevel::Info => Color::from_rgb8(0x5c, 0x9e, 0xff),
        LogLevel::Debug => Color::from_rgb8(0x8e, 0x93, 0xa0),
        LogLevel::Platform => Color::from_rgb8(0xc7, 0x92, 0xea),
    }
}

// MARK: Vista

pub fn view<'a>(st: &'a LogsState, p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim, bg) = (p.fg, Color { a: 0.55, ..p.fg }, p.bg);
    let regions: Vec<String> = REGIONS.iter().map(|r| r.to_string()).collect();
    let ranges: Vec<String> = RANGES.iter().map(|(l, _)| l.to_string()).collect();
    let busy = st.searching;

    let group_label = st.group.as_deref().map(|g| ellipsize_start(g, GROUP_LABEL_CHARS)).unwrap_or_else(|| "Elegir grupo de logs…".into());
    let field = |cap: &str, w: Element<'a, Message>| -> Element<'a, Message> { column![crate::ui::caption(cap, fg), w].spacing(5).into() };
    let group_btn = crate::ui::field_button(
        row![
            crate::ui::icon(crate::ui::Icon::ScrollText, 13.0, dim),
            text(group_label).size(13).font(FONT).width(Length::Fill).wrapping(iced::widget::text::Wrapping::None),
            crate::ui::icon(crate::ui::Icon::ChevronsUpDown, 12.0, dim)
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fixed(300.0))
    .on_press(Message::LogsPicker(true));
    let row_a = row![
        field("Región", pick_list(regions, Some(st.region.clone()), Message::LogsRegion).width(Length::Fixed(140.0)).into()),
        container(field("Grupo de logs", group_btn.width(Length::Fill).into())).width(Length::Fill),
        field("Rango", pick_list(ranges.into_iter().map(crate::i18n::Tr).collect::<Vec<_>>(), Some(crate::i18n::Tr(st.range.clone())), |t: crate::i18n::Tr<String>| Message::LogsRange(t.0)).width(Length::Fixed(170.0)).into()),
    ]
    .spacing(10)
    .align_y(iced::Alignment::End);
    let row_b = row![
        container(field("Filtro (sintaxis de CloudWatch)", text_input("ERROR · ?ERROR ?WARN · { $.level = \"error\" }", &st.filter).on_input(Message::LogsFilter).on_submit(Message::LogsSearch).width(Length::Fill).into())).width(Length::Fill),
        field("Stream", text_input("Prefijo (opcional)", &st.stream_prefix).on_input(Message::LogsStream).on_submit(Message::LogsSearch).width(Length::Fixed(170.0)).into()),
        action_button(if busy { "Buscando…" } else { "Buscar" }, accent, fg, true).on_press_maybe((!busy).then_some(Message::LogsSearch)),
        if st.following { action_button("Detener", Color::from_rgb8(0xff, 0x6b, 0x6b), fg, true) } else { crate::ui::pill("Seguir", crate::ui::Kind::Secondary, fg, fg) }.on_press(Message::LogsFollow),
    ]
    .spacing(10)
    .align_y(iced::Alignment::End);
    let top = column![row_a, row_b].spacing(10);

    // Chips de nivel + opciones de vista.
    let chip = |label: String, sel: bool, color: Color, on: Message| {
        button(text(tr(&label)).size(11).color(if sel { Color::BLACK } else { color })).padding([3, 9]).on_press(on).style(move |_, s| button::Style {
            background: Some(Background::Color(if sel { color } else if matches!(s, button::Status::Hovered) { Color { a: 0.22, ..color } } else { Color { a: 0.1, ..color } })),
            text_color: color,
            border: Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        })
    };
    let mut chips: Row<Message> = Row::new().spacing(6).align_y(iced::Alignment::Center);
    chips = chips.push(chip(format!("Todos {}", st.event_count()), st.level.is_none(), fg, Message::LogsLevel(None)));
    for l in [LogLevel::Error, LogLevel::Warn, LogLevel::Info, LogLevel::Debug, LogLevel::Platform] {
        chips = chips.push(chip(format!("{} {}", l.label(), st.count_level(l)), st.level == Some(l), level_color(l), Message::LogsLevel(Some(l))));
    }
    let toggle = |label: &'static str, on: bool, m: Message| crate::ui::compact(label, if on { accent } else { dim }, fg).on_press(m);
    let tools = row![
        toggle("Agrupar", st.by_invocation, Message::LogsToggleGroup),
        toggle("JSON", st.pretty_json, Message::LogsTogglePretty),
        toggle("Texto", st.text_mode, Message::LogsMode(!st.text_mode)),
        crate::ui::compact("⧉ Copiar", fg, fg).on_press(Message::LogsCopyAll),
        crate::ui::compact("⬇", fg, fg).on_press(Message::LogsSave),
        crate::ui::compact("🗑", fg, fg).on_press(Message::LogsClear),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    let options = Row::new().push(chips).push(tools).spacing(18).wrap().vertical_spacing(6);

    // Filas (solo la ventana final).
    let rows = st.rows();
    let infos = logs::infos(&st.parsed);
    let skip = rows.len().saturating_sub(st.window);
    let mut list: Column<Message> = Column::new().spacing(1).padding(iced::Padding { top: 0.0, bottom: 0.0, left: 0.0, right: 10.0 });
    if skip > 0 {
        list = list.push(
            container(action_button("Mostrar anteriores ↑", accent, fg, false).on_press(Message::LogsMore)).padding([4, 0]).center_x(Length::Fill),
        );
    }
    for r in rows.iter().skip(skip) {
        match r {
            LogRow::Group(rid) => {
                let Some(info) = infos.get(rid) else { continue };
                let collapsed = st.collapsed.contains(rid);
                let badge = if info.has_error { text(tr("ERROR")).size(10).color(level_color(LogLevel::Error)) } else { text(tr("OK")).size(10).color(Color::from_rgb8(0x7f, 0xd9, 0x8a)) };
                let head = row![
                    text(if collapsed { "▸" } else { "▾" }).size(12).color(dim),
                    text(format!("{}…", rid.chars().take(8).collect::<String>())).size(12).font(FONT).color(fg),
                    badge,
                    text(group_summary(info)).size(11).color(dim),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center);
                list = list.push(
                    button(head).width(Length::Fill).padding([5, 8]).on_press(Message::LogsCollapse(rid.clone())).style(move |_, s| button::Style {
                        background: Some(Background::Color(Color { a: if matches!(s, button::Status::Hovered) { 0.12 } else { 0.07 }, ..fg })),
                        text_color: fg,
                        border: Border { radius: 6.0.into(), ..Default::default() },
                        ..Default::default()
                    }),
                );
            }
            LogRow::Event(i) => list = list.push(event_row(st, *i, p, dim)),
        }
    }
    if rows.is_empty() && !st.searching {
        list = list.push(container(text(tr(if st.parsed.is_empty() { "Sin eventos todavía." } else { "Ningún evento coincide con el filtro de nivel." })).size(12).color(dim)).padding(14));
    }

    let status = text(tr(&st.status)).size(11).color(if st.is_error { Color::from_rgb8(0xff, 0x6b, 0x6b) } else { dim });
    let list_box = container(scrollable(list).id(SCROLL_ID).on_scroll(|v| Message::LogsScrolled(v.relative_offset().y)).height(Length::Fill))
        .padding(iced::Padding { top: 6.0, bottom: 6.0, left: 4.0, right: 4.0 })
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.04, ..fg })),
            border: Border { radius: 10.0.into(), width: 1.0, color: Color { a: 0.08, ..fg } },
            ..Default::default()
        });
    container(column![top, options, list_box, status].spacing(10).padding(14))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style { background: Some(Background::Color(bg)), ..Default::default() })
        .into()
}

fn event_row<'a>(st: &'a LogsState, i: usize, p: &Palette, dim: Color) -> Element<'a, Message> {
    let fg = p.fg;
    let Some(pl) = st.parsed.get(i) else { return space::vertical().into() };
    let with_date = range_seconds(&st.range) >= 86_400;
    let time = text(format_time(pl.event.timestamp, with_date)).size(11).font(FONT).color(dim);

    if st.text_mode {
        let t = text(pl.event.message.trim_end().to_string()).size(12).font(FONT).color(fg);
        return row![time, t].spacing(10).padding([1, 8]).into();
    }

    let color = level_color(pl.level);
    let badge = container(text(pl.level.label()).size(9).color(color)).padding([2, 5]).width(Length::Fixed(52.0)).center_x(Length::Fixed(52.0)).style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.16, ..color })),
        border: Border { radius: 5.0.into(), ..Default::default() },
        ..Default::default()
    });
    let mut head: Row<Message> = Row::new().spacing(8).align_y(iced::Alignment::Center);
    head = head.push(time).push(badge);
    let summary = if pl.summary.is_empty() { pl.event.message.trim().to_string() } else { pl.summary.clone() };
    let one_line: String = summary.chars().take(400).collect();
    head = head.push(text(one_line).size(12).font(FONT).color(if pl.level == LogLevel::Error { color } else { fg }).width(Length::Fill));
    for (k, v) in &pl.chips {
        head = head.push(text(format!("{k}={v}")).size(10).font(FONT).color(dim));
    }
    if let Some(r) = &pl.report {
        if let Some(d) = r.duration_ms {
            head = head.push(text(format!("{d:.0} ms")).size(10).color(dim));
        }
    }
    let tint = match pl.level { LogLevel::Error => 0.08, LogLevel::Warn => 0.05, _ => 0.0 };
    let has_detail = pl.json.is_some() || !pl.extra_lines().is_empty() || pl.platform == Some(Platform::Report);
    let open = st.expanded.contains(&i);
    let mut col: Column<Message> = column![button(head).width(Length::Fill).padding([4, 8]).on_press(Message::LogsExpand(i)).style(move |_, s| button::Style {
        background: Some(Background::Color(if matches!(s, button::Status::Hovered) { Color { a: 0.08, ..fg } } else if tint > 0.0 { Color { a: tint, ..color } } else { Color::TRANSPARENT })),
        text_color: fg,
        border: Border { radius: 6.0.into(), ..Default::default() },
        ..Default::default()
    })]
    .spacing(2);

    if open || (has_detail && pl.is_traceback) {
        let mut detail: Column<Message> = Column::new().spacing(0).padding([4, 70]);
        match (&pl.json, st.pretty_json) {
            (Some(v), true) => {
                let mut budget = 6000;
                let tokens = logs::pretty(v, 0, &mut budget);
                // Una línea por salto: se agrupan los fragmentos de cada línea en una fila.
                let mut line: Row<Message> = Row::new();
                for (frag, tok) in tokens {
                    let c = match tok {
                        Token::Key => Color::from_rgb8(0x5c, 0x9e, 0xff),
                        Token::Str => Color::from_rgb8(0x7f, 0xd9, 0x8a),
                        Token::Number => Color::from_rgb8(0xf2, 0xc9, 0x7d),
                        Token::Literal => Color::from_rgb8(0xc7, 0x92, 0xea),
                        Token::Punct => dim,
                    };
                    let mut rest = frag.as_str();
                    while let Some(nl) = rest.find('\n') {
                        let (head_part, tail) = rest.split_at(nl);
                        if !head_part.is_empty() {
                            line = line.push(text(head_part.to_string()).size(12).font(FONT).color(c));
                        }
                        detail = detail.push(std::mem::replace(&mut line, Row::new()));
                        rest = &tail[1..];
                    }
                    if !rest.is_empty() {
                        line = line.push(text(rest.to_string()).size(12).font(FONT).color(c));
                    }
                }
                detail = detail.push(line);
            }
            _ => {
                for l in pl.lines.iter().take(200) {
                    detail = detail.push(text(l.clone()).size(12).font(FONT).color(if pl.is_traceback { level_color(LogLevel::Error) } else { fg }));
                }
            }
        }
        detail = detail.push(row![action_button("Copiar evento", dim, fg, false).on_press(Message::LogsCopyEvent(i))].padding([4, 0]));
        col = col.push(detail);
    }
    col.into()
}

/// Selector de grupo de logs (diálogo con búsqueda).
pub fn picker_view<'a>(st: &'a LogsState, query: &'a str, p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
    let q = query.trim().to_lowercase();
    let mut list: Column<Message> = Column::new().spacing(1);
    let mut shown = 0;
    for g in st.groups.iter().filter(|g| q.is_empty() || g.name.to_lowercase().contains(&q)).take(300) {
        shown += 1;
        let retention = g.retention_days.map(|d| format!("Retención: {d} días")).unwrap_or_else(|| "Retención: sin límite".into());
        list = list.push(
            button(column![text(g.name.clone()).size(13).font(FONT).color(fg), text(tr(&retention)).size(10).color(dim)].spacing(2))
                .width(Length::Fill)
                .padding([6, 10])
                .on_press(Message::LogsPick(g.name.clone()))
                .style(move |_, s| button::Style {
                    background: Some(Background::Color(if matches!(s, button::Status::Hovered) { Color { a: 0.12, ..fg } } else { Color::TRANSPARENT })),
                    text_color: fg,
                    border: Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                }),
        );
    }
    let note = if st.groups_loading {
        "Cargando grupos de logs…"
    } else if st.groups.is_empty() {
        "No hay grupos cargados. Pulsa Recargar para consultarlos."
    } else if shown == 0 {
        "Ningún grupo coincide con la búsqueda."
    } else {
        ""
    };
    let body = column![
        row![text(tr("Grupo de logs")).size(16).color(fg), space::horizontal(), action_button(if st.groups_loading { "…" } else { "Recargar" }, accent, fg, false).on_press(Message::LogsLoadGroups)].align_y(iced::Alignment::Center),
        text_input("Buscar grupo de logs…", query).id("logs-group-search").on_input(Message::LogsPickerQuery).padding(7).size(13),
        container(scrollable(list)).max_height(340),
        text(tr(note)).size(11).color(dim),
        row![space::horizontal(), action_button("Cancelar", fg, fg, false).on_press(Message::LogsPicker(false))],
    ]
    .spacing(10);
    dialog(body.into(), p, 560.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jterm_net::transport::{response, FakeTransport};

    fn creds() -> Credentials {
        Credentials { access_key: "AK".into(), secret: "SK".into() }
    }

    fn ev(id: &str, ts: i64, msg: &str) -> LogEvent {
        LogEvent { id: id.into(), stream: "s".into(), timestamp: ts, message: msg.into() }
    }

    fn state() -> LogsState {
        let acc = AwsAccount { id: uuid::Uuid::new_v4(), name: "t".into(), access_key_id: "AK".into(), region: "us-east-1".into(), env: None };
        LogsState::new(acc, "us-east-1".into(), Arc::new(AwsClient::new(ReqwestTransport::new().unwrap())))
    }

    #[test]
    fn append_dedupes_sorts_and_parses() {
        let mut st = state();
        assert_eq!(st.append(vec![ev("b", 20, "ERROR boom"), ev("a", 10, "hola")]), 2);
        assert_eq!(st.parsed[0].event.id, "a", "ordenados por hora");
        // Un segundo sondeo con solapamiento no repite eventos.
        assert_eq!(st.append(vec![ev("b", 20, "ERROR boom"), ev("c", 30, "{\"level\":\"warn\",\"msg\":\"x\"}")]), 1);
        assert_eq!(st.event_count(), 3);
        assert_eq!((st.count_level(LogLevel::Error), st.count_level(LogLevel::Warn)), (1, 1));
        st.level = Some(LogLevel::Error);
        assert_eq!(st.rows(), [LogRow::Event(1)]);
        assert!(st.plain_text().contains("ERROR boom"));
        st.clear_results();
        assert_eq!(st.event_count(), 0);
        assert_eq!(st.append(vec![ev("a", 10, "otra vez")]), 1, "tras limpiar, los ids vuelven a ser nuevos");
    }

    #[test]
    fn append_keeps_only_the_latest_events() {
        let mut st = state();
        let many: Vec<LogEvent> = (0..(MAX_KEPT_EVENTS as i64 + 5)).map(|i| ev(&format!("e{i}"), i, "x")).collect();
        st.append(many);
        assert_eq!(st.event_count(), MAX_KEPT_EVENTS);
        assert_eq!(st.parsed[0].event.id, "e5");
    }

    #[test]
    fn invocation_grouping_uses_the_stream_tracker() {
        let mut st = state();
        let rid = "6bc28136-a1b2-4c3d-8e4f-0123456789ab";
        st.append(vec![
            ev("1", 1, &format!("START RequestId: {rid} Version: $LATEST")),
            ev("2", 2, "dentro"),
            ev("3", 3, &format!("REPORT RequestId: {rid}\tDuration: 5.00 ms\tBilled Duration: 5 ms\tMemory Size: 128 MB\tMax Memory Used: 50 MB")),
        ]);
        let rows = st.rows();
        assert_eq!(rows, [LogRow::Group(rid.into()), LogRow::Event(1)]);
        let infos = logs::infos(&st.parsed);
        assert_eq!(group_summary(&infos[rid]), "1 evento · 5 ms · 50/128 MB");
    }

    #[test]
    fn long_group_names_are_trimmed_from_the_start() {
        assert_eq!(ellipsize_start("/aws/lambda/corto", 30), "/aws/lambda/corto");
        let t = ellipsize_start("/aws/ec2/instance/suite-enovus/log/application", 30);
        assert_eq!(t.chars().count(), 30);
        assert!(t.starts_with('…') && t.ends_with("log/application"));
    }

    #[test]
    fn ranges_and_times() {
        assert_eq!(range_seconds("Última hora"), 3600);
        assert_eq!(range_seconds("Últimos 7 días"), 604_800);
        assert_eq!(range_seconds("desconocido"), 3600);
        let st = state();
        assert_eq!(st.start_ms_for_range(10_000_000), 10_000_000 - 3_600_000);
        assert_eq!(format_time(0, true).len(), "1970-01-01 00:00:00.000".len());
        assert_eq!(format_time(0, false).len(), "00:00:00.000".len());
    }

    #[tokio::test]
    async fn follow_and_groups_paginate_and_sort() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let t = FakeTransport::new(move |req| {
            let body = String::from_utf8_lossy(&req.body).to_string();
            let is_events = req.headers.iter().any(|(_, v)| v.ends_with("FilterLogEvents"));
            if is_events {
                if body.contains("\"nextToken\"") {
                    response(200, r#"{"events":[{"timestamp":2,"logStreamName":"s","message":"dos","eventId":"2"}]}"#)
                } else {
                    n2.fetch_add(1, Ordering::SeqCst);
                    response(200, r#"{"events":[{"timestamp":1,"logStreamName":"s","message":"uno","eventId":"1"}],"nextToken":"T"}"#)
                }
            } else if body.contains("\"nextToken\"") {
                response(200, r#"{"logGroups":[{"logGroupName":"/a-segundo"}]}"#)
            } else {
                response(200, r#"{"logGroups":[{"logGroupName":"/Zeta"}],"nextToken":"G"}"#)
            }
        });
        let c = AwsClient::new(t);
        let ev = fetch_follow(&c, &creds(), "us-east-1", "/g", 0, "ERROR", "").await.unwrap();
        assert_eq!(ev.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["1", "2"]);
        let g = fetch_groups(&c, &creds(), "us-east-1").await.unwrap();
        assert_eq!(g.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["/a-segundo", "/Zeta"]);
        let first = c.transport().recorded().into_iter().find(|r| String::from_utf8_lossy(&r.body).contains("filterPattern")).unwrap();
        assert!(String::from_utf8_lossy(&first.body).contains("\"ERROR\""));
    }

    #[tokio::test]
    async fn permission_errors_get_an_iam_hint() {
        let c = AwsClient::new(FakeTransport::new(|_| response(400, r#"{"__type":"AccessDeniedException","message":"User is not authorized to perform: logs:FilterLogEvents"}"#)));
        let e = fetch_page(&c, &creds(), "us-east-1", "/g", 0, "", "", None).await.unwrap_err();
        assert!(e.contains("Permisos IAM necesarios"), "{e}");
    }
}
