//! Buscador rápido (⌘K): pestañas abiertas, servidores, instancias EC2, comandos guardados y acciones.

use crate::appmenu::MenuAction;
use crate::colors::Palette;
use crate::ui::{self, Icon};
use crate::i18n::tr;
use crate::Message;
use iced::widget::{column, container, row, text, Column};
use iced::{Background, Border, Color, Element, Length};
use jterm_core::models::ServerEnv;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Tab,
    Server,
    Instance,
    Snippet,
    Action,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Tab => "Pestaña",
            Kind::Server => "Servidor",
            Kind::Instance => "EC2",
            Kind::Snippet => "Comando",
            Kind::Action => "Acción",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Kind::Tab => Icon::SquareTerminal,
            Kind::Server => Icon::Server,
            Kind::Instance => Icon::Cloud,
            Kind::Snippet => Icon::SquareTerminal,
            Kind::Action => Icon::Zap,
        }
    }
}

/// Qué hacer al elegir el resultado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Tab(usize),
    Server(Uuid),
    /// Posición en la lista de instancias cargada en el panel AWS.
    Instance(usize),
    Snippet(Uuid),
    Action(MenuAction),
    /// Vista de AWS o archivos abierta como pestaña.
    Screen(crate::Screen),
}

#[derive(Debug, Clone)]
pub struct Item {
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    pub env: Option<ServerEnv>,
    /// Palabras extra que también se buscan (p. ej. el ID de la instancia).
    pub keywords: String,
    pub target: Target,
    /// Icono propio (si no, el del tipo).
    pub icon: Option<Icon>,
}

fn env_words(e: Option<ServerEnv>) -> String {
    match e {
        None => String::new(),
        Some(ServerEnv::Prod) => "producción produccion prod".into(),
        Some(ServerEnv::Staging) => "staging".into(),
        Some(ServerEnv::Dev) => "desarrollo dev".into(),
    }
}

/// Puntuación de una búsqueda difusa: cada palabra de la consulta debe aparecer
/// (prefijo del título > subcadena del título > en cualquier campo > letras en orden en el título).
pub fn score(query: &str, item: &Item) -> Option<i32> {
    let q = jterm_core::models::fold(query);
    let tokens: Vec<&str> = q.split_whitespace().collect();
    if tokens.is_empty() {
        return Some(0);
    }
    let title = jterm_core::models::fold(&item.title);
    let hay = jterm_core::models::fold(&format!("{} {} {} {}", item.title, item.subtitle, item.keywords, env_words(item.env)));
    let mut total = 0;
    for t in tokens {
        if title.starts_with(t) {
            total += 300;
        } else if let Some(pos) = title.find(t) {
            total += 200 - (title[..pos].chars().count() as i32).min(50);
        } else if hay.contains(t) {
            total += 100;
        } else if t.chars().count() >= 3 && is_subsequence(t, &title) {
            total += 40;
        } else {
            return None;
        }
    }
    Some(total)
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|n| it.any(|c| c == n))
}

/// Índices de `items` que coinciden, mejor primero (a igual puntuación, por tipo: pestañas, servidores…).
pub fn filter(items: &[Item], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..items.len()).collect();
    }
    let mut scored: Vec<(i32, usize)> = items.iter().enumerate().filter_map(|(i, it)| score(query, it).map(|s| (s, i))).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(items[a.1].kind.cmp(&items[b.1].kind)).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

/// Primera fila visible de una ventana de `max` filas que mantiene `selected` a la vista.
pub fn window_start(selected: usize, max: usize) -> usize {
    if selected < max { 0 } else { selected + 1 - max }
}

pub const MAX_ROWS: usize = 8;
pub const INPUT_ID: &str = "finder-input";

pub struct FinderState {
    pub query: String,
    pub items: Vec<Item>,
    pub shown: Vec<usize>,
    pub selected: usize,
}

impl FinderState {
    pub fn new(items: Vec<Item>) -> Self {
        let shown = (0..items.len()).collect();
        FinderState { query: String::new(), items, shown, selected: 0 }
    }

    pub fn set_query(&mut self, q: String) {
        self.shown = filter(&self.items, &q);
        self.query = q;
        self.selected = 0;
    }

    pub fn move_selection(&mut self, d: i32) {
        if self.shown.is_empty() {
            return;
        }
        self.selected = (self.selected as i32 + d).clamp(0, self.shown.len() as i32 - 1) as usize;
    }

    pub fn current(&self) -> Option<&Item> {
        self.shown.get(self.selected).map(|&i| &self.items[i])
    }
}

pub fn view<'a>(st: &'a FinderState, p: &Palette, accent: Color) -> Element<'a, Message> {
    let fg = p.fg;
    let dim = Color { a: 0.5, ..fg };
    // Fondo fijo y oscuro (como el de la versión Swift), sea cual sea el tema del terminal.
    let bg = Color::from_rgba(0.11, 0.12, 0.15, 0.98);
    let hair = Color { a: 0.08, ..Color::WHITE };

    // Campo sin borde ni fondo, con la lupa a la izquierda.
    let input = iced::widget::text_input("Buscar servidor, instancia, pestaña o comando…", &st.query)
        .id(INPUT_ID)
        .on_input(Message::FinderQuery)
        .size(18)
        .padding([0, 0])
        .style(move |_, _| iced::widget::text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border { radius: 0.0.into(), width: 0.0, color: Color::TRANSPARENT },
            icon: dim,
            placeholder: Color { a: 0.3, ..Color::WHITE },
            value: Color::WHITE,
            selection: Color { a: 0.35, ..accent },
        });
    let search = container(row![ui::icon(Icon::Search, 17.0, dim), input].spacing(12).align_y(iced::Alignment::Center)).padding([0, 20]).height(Length::Fixed(52.0)).center_y(Length::Fixed(52.0));
    let divider = container(iced::widget::space::vertical()).height(1).width(Length::Fill).style(move |_| container::Style { background: Some(Background::Color(hair)), ..Default::default() });

    let start = window_start(st.selected, MAX_ROWS);
    let mut list: Column<Message> = Column::new().spacing(0);
    for (pos, &idx) in st.shown.iter().enumerate().skip(start).take(MAX_ROWS) {
        let it = &st.items[idx];
        let sel = pos == st.selected;
        let color = it.env.map(|e| {
            let (r, g, b) = e.rgb();
            Color::from_rgb8(r, g, b)
        });
        let mut title: iced::widget::Row<Message> = row![text(it.title.clone()).size(13).color(Color::WHITE).wrapping(iced::widget::text::Wrapping::None)].spacing(8).align_y(iced::Alignment::Center);
        if let (Some(e), Some(c)) = (it.env, color) {
            title = title.push(container(text(e.tag()).size(8).color(c)).padding([1, 5]).style(move |_| container::Style {
                background: Some(Background::Color(Color { a: 0.2, ..c })),
                border: Border { radius: 4.0.into(), ..Default::default() },
                ..Default::default()
            }));
        }
        let line = row![
            container(ui::icon(it.icon.unwrap_or(it.kind.icon()), 15.0, color.unwrap_or(accent))).width(Length::Fixed(22.0)),
            column![title, text(tr(&it.subtitle)).size(11).color(dim).wrapping(iced::widget::text::Wrapping::None)].spacing(1).width(Length::Fill),
            text(tr(it.kind.label())).size(10).color(Color { a: 0.38, ..Color::WHITE }),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center);
        list = list.push(
            iced::widget::mouse_area(container(line).padding([0, 12]).height(Length::Fixed(46.0)).center_y(Length::Fixed(46.0)).width(Length::Fill).style(move |_| container::Style {
                background: Some(Background::Color(if sel { Color { a: 0.28, ..accent } } else { Color::TRANSPARENT })),
                border: Border { radius: 8.0.into(), ..Default::default() },
                ..Default::default()
            }))
            .on_press(Message::FinderRun(pos, false)),
        );
    }
    let body: Element<Message> = if st.shown.is_empty() {
        container(text(tr("Sin resultados")).size(13).color(dim)).height(Length::Fixed(64.0)).center(Length::Fill).into()
    } else {
        container(list).padding([6, 6]).into()
    };
    let hint = container(text(tr("↩ abrir     ⌥↩ pegar y ejecutar comando     ↑↓ moverse     esc cerrar")).size(10).color(Color { a: 0.35, ..Color::WHITE })).padding([6, 0]).center_x(Length::Fill);
    container(column![search, divider, body, hint])
        .width(Length::Fixed(600.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: 14.0.into(), width: 1.0, color: Color { a: 0.12, ..Color::WHITE } },
            shadow: iced::Shadow { color: Color { a: 0.55, ..Color::BLACK }, offset: iced::Vector::new(0.0, 14.0), blur_radius: 34.0 },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: Kind, title: &str, sub: &str) -> Item {
        Item { kind, title: title.into(), subtitle: sub.into(), env: None, keywords: String::new(), target: Target::Action(MenuAction::NewTab), icon: None }
    }

    #[test]
    fn scoring_prefers_prefix_then_substring_then_other_fields() {
        let it = item(Kind::Server, "Producción web", "ana@10.0.0.1");
        let s = |q: &str| score(q, &it);
        assert_eq!(s(""), Some(0));
        assert_eq!(s("prod"), Some(300));
        assert_eq!(s("web"), Some(200 - 11));
        assert_eq!(s("10.0"), Some(100), "campos distintos del título valen menos");
        assert_eq!(s("pwb"), Some(40), "letras en orden");
        assert_eq!(s("zzz"), None);
        assert_eq!(s("prod web"), Some(300 + 189));
        assert_eq!(s("prod zzz"), None, "todas las palabras deben aparecer");
        assert!(s("PRODUCCION").is_some(), "sin distinguir mayúsculas ni acentos");
    }

    #[test]
    fn environment_words_are_searchable() {
        let mut it = item(Kind::Server, "db", "root@h");
        it.env = Some(ServerEnv::Prod);
        assert!(score("prod", &it).is_some());
        assert!(score("produccion", &it).is_some());
        assert!(score("staging", &it).is_none());
        let mut ec2 = item(Kind::Instance, "api", "t3.micro · 1.2.3.4");
        ec2.keywords = "i-0abc123".into();
        assert!(score("i-0abc", &ec2).is_some(), "el ID de la instancia se busca");
    }

    #[test]
    fn filter_orders_by_score_then_kind() {
        let items = vec![item(Kind::Action, "Nueva pestaña", ""), item(Kind::Server, "pestaña de pruebas", ""), item(Kind::Tab, "web", "pestaña"), item(Kind::Snippet, "pe", "")];
        // Sin consulta: todos, en el orden original.
        assert_eq!(filter(&items, "  "), [0, 1, 2, 3]);
        let r = filter(&items, "pest");
        assert_eq!(r, [1, 0, 2], "prefijo (servidor) > subcadena > otro campo; el comando 'pe' no coincide");
        assert!(filter(&items, "xyz").is_empty());
        // Igual puntuación: gana el tipo que va antes (pestaña antes que servidor).
        let tie = vec![item(Kind::Server, "web", ""), item(Kind::Tab, "web", "")];
        assert_eq!(filter(&tie, "web"), [1, 0]);
    }

    #[test]
    fn selection_moves_and_clamps_and_the_window_follows() {
        let items: Vec<Item> = (0..20).map(|i| item(Kind::Action, &format!("a{i}"), "")).collect();
        let mut st = FinderState::new(items);
        st.move_selection(-1);
        assert_eq!(st.selected, 0);
        st.move_selection(100);
        assert_eq!(st.selected, 19);
        assert_eq!(window_start(0, MAX_ROWS), 0);
        assert_eq!(window_start(7, MAX_ROWS), 0);
        assert_eq!(window_start(8, MAX_ROWS), 1);
        assert_eq!(window_start(19, MAX_ROWS), 12);
        st.set_query("a1".into());
        assert_eq!(st.selected, 0, "al escribir vuelve al primer resultado");
        assert_eq!(st.current().unwrap().title, "a1");
        let mut empty = FinderState::new(vec![]);
        empty.move_selection(1);
        assert!(empty.current().is_none());
    }
}
