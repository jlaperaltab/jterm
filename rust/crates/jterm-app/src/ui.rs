//! Piezas de interfaz compartidas: iconos (Lucide), botones, campos, desplegables y control segmentado.

use crate::Message;
use iced::widget::{button, container, pick_list, row, text, text_input, PickList, Text, TextInput};
use iced::widget::{overlay::menu, text::Wrapping};
use iced::{Background, Border, Color, Element, Font, Length, Theme};
pub use lucide_icons::{Icon, LUCIDE_FONT_BYTES};
use std::borrow::Borrow;

pub const ICON_FONT: Font = Font::with_name("lucide");

pub fn icon<'a>(i: Icon, size: f32, color: Color) -> Text<'a> {
    text(char::from(i).to_string()).font(ICON_FONT).size(size).color(color)
}

/// Símbolo (carácter) → icono. Permite escribir etiquetas como "✎" o "⬆ Subir" y dibujarlas con la fuente de iconos.
pub fn glyph_icon(c: char) -> Option<Icon> {
    Some(match c {
        '✎' => Icon::Pencil,
        '+' => Icon::Plus,
        '↻' => Icon::RefreshCw,
        '…' => Icon::Ellipsis,
        '×' | '✕' => Icon::X,
        '⧉' => Icon::Copy,
        '←' => Icon::ArrowLeft,
        '↑' => Icon::ArrowUp,
        '↓' => Icon::ArrowDown,
        '🔍' | '🔎' => Icon::Search,
        '🗑' => Icon::Trash2,
        '⬆' => Icon::Upload,
        '⬇' => Icon::Download,
        '🔗' => Icon::Link,
        '🪣' => Icon::Database,
        '⌘' => Icon::Command,
        '☰' => Icon::PanelLeft,
        '⚙' => Icon::Settings,
        '−' => Icon::Minus,
        '📁' => Icon::Folder,
        '📄' => Icon::File,
        '▸' => Icon::ChevronRight,
        '▾' => Icon::ChevronDown,
        '○' => Icon::Circle,
        '✓' => Icon::Check,
        '#' => Icon::Hash,
        _ => return None,
    })
}

/// Contenido de un botón: icono + texto si la etiqueta empieza por un símbolo conocido; si no, solo texto.
pub fn label<'a>(s: &str, size: f32, color: Color) -> Element<'a, Message> {
    let s = &crate::i18n::tr(s);
    let mut chars = s.chars();
    let first = chars.next();
    let rest = chars.as_str().trim_start().to_string();
    match first.and_then(glyph_icon) {
        Some(i) if rest.is_empty() => icon(i, size + 1.0, color).into(),
        Some(i) => row![icon(i, size + 1.0, color), text(rest).size(size).color(color).wrapping(Wrapping::None)].spacing(6).align_y(iced::Alignment::Center).into(),
        None => text(s.to_string()).size(size).color(color).wrapping(Wrapping::None).into(),
    }
}

/// Color de texto legible sobre `c`.
pub fn on_color(c: Color) -> Color {
    let lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
    if lum > 0.6 { Color::from_rgb(0.08, 0.09, 0.11) } else { Color::WHITE }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Relleno con el color de acento.
    Primary,
    /// Fondo suave.
    Secondary,
    /// Sin fondo hasta pasar el ratón.
    Ghost,
}

/// Botón con forma de pastilla.
pub fn pill<'a>(text_label: &str, kind: Kind, color: Color, fg: Color) -> button::Button<'a, Message> {
    let text_color = if kind == Kind::Primary { on_color(color) } else { color };
    button(label(text_label, 13.0, text_color)).padding([7, 14]).style(move |_, st| {
        let hover = matches!(st, button::Status::Hovered | button::Status::Pressed);
        let (bg, border) = match kind {
            Kind::Primary => (Color { a: if hover { 1.0 } else { 0.92 }, ..color }, Color::TRANSPARENT),
            Kind::Secondary => (Color { a: if hover { 0.16 } else { 0.09 }, ..fg }, Color { a: 0.1, ..fg }),
            Kind::Ghost => (if hover { Color { a: 0.1, ..fg } } else { Color::TRANSPARENT }, Color { a: 0.14, ..color }),
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color,
            border: Border { radius: 9.0.into(), width: if kind == Kind::Primary { 0.0 } else { 1.0 }, color: border },
            ..Default::default()
        }
    })
}

/// Botón pequeño para paneles estrechos (barra lateral): menos relleno y texto de 12 px.
pub fn compact<'a>(text_label: &str, color: Color, fg: Color) -> button::Button<'a, Message> {
    button(label(text_label, 12.0, color)).padding([5, 9]).style(move |_, st| {
        let hover = matches!(st, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(Color { a: if hover { 0.16 } else { 0.07 }, ..fg })),
            text_color: color,
            border: Border { radius: 8.0.into(), width: 1.0, color: Color { a: 0.12, ..fg } },
            ..Default::default()
        }
    })
}

/// Botón de solo icono (o texto corto), sin borde.
pub fn icon_button<'a>(label_text: &str, color: Color, fg: Color) -> button::Button<'a, Message> {
    button(label(label_text, 13.0, color)).padding([6, 8]).style(move |_, st| button::Style {
        background: Some(Background::Color(if matches!(st, button::Status::Hovered | button::Status::Pressed) { Color { a: 0.12, ..fg } } else { Color::TRANSPARENT })),
        text_color: color,
        border: Border { radius: 8.0.into(), ..Default::default() },
        ..Default::default()
    })
}

// MARK: Campos y desplegables (usan la paleta del tema de iced)

pub fn input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = theme.palette();
    let focused = matches!(status, text_input::Status::Focused { .. });
    let hovered = matches!(status, text_input::Status::Hovered);
    text_input::Style {
        background: Background::Color(Color { a: if focused { 0.09 } else { 0.06 }, ..p.text }),
        border: Border { radius: 9.0.into(), width: 1.0, color: if focused { p.primary } else { Color { a: if hovered { 0.22 } else { 0.12 }, ..p.text } } },
        icon: Color { a: 0.6, ..p.text },
        placeholder: Color { a: 0.38, ..p.text },
        value: p.text,
        selection: Color { a: 0.35, ..p.primary },
    }
}

pub fn input<'a>(placeholder: &str, value: &str) -> TextInput<'a, Message> {
    text_input(&crate::i18n::tr(placeholder), value).style(input_style).padding([8, 11]).size(13)
}

pub fn pick_style(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let p = theme.palette();
    let active = !matches!(status, pick_list::Status::Active);
    pick_list::Style {
        text_color: p.text,
        placeholder_color: Color { a: 0.45, ..p.text },
        handle_color: Color { a: 0.55, ..p.text },
        background: Background::Color(Color { a: if active { 0.12 } else { 0.07 }, ..p.text }),
        border: Border { radius: 9.0.into(), width: 1.0, color: Color { a: if active { 0.22 } else { 0.12 }, ..p.text } },
    }
}

pub fn menu_style(theme: &Theme) -> menu::Style {
    let p = theme.palette();
    let bg = Color { r: (p.background.r + 0.07).min(1.0), g: (p.background.g + 0.07).min(1.0), b: (p.background.b + 0.07).min(1.0), a: 1.0 };
    menu::Style {
        background: Background::Color(bg),
        border: Border { radius: 10.0.into(), width: 1.0, color: Color { a: 0.16, ..p.text } },
        text_color: p.text,
        selected_text_color: on_color(p.primary),
        selected_background: Background::Color(p.primary),
        shadow: iced::Shadow { color: Color { a: 0.45, ..Color::BLACK }, offset: iced::Vector::new(0.0, 6.0), blur_radius: 18.0 },
    }
}

pub fn select<'a, T, L, V>(options: L, selected: Option<V>, on: impl Fn(T) -> Message + 'a) -> PickList<'a, T, L, V, Message>
where
    T: ToString + PartialEq + Clone + 'a,
    L: Borrow<[T]> + 'a,
    V: Borrow<T> + 'a,
{
    pick_list(options, selected, on).style(pick_style).menu_style(menu_style).padding([8, 11]).text_size(13)
}

/// Botón con aspecto de campo desplegable (para abrir un selector propio).
pub fn field_button<'a>(content: impl Into<Element<'a, Message>>) -> button::Button<'a, Message> {
    button(content).padding([8, 11]).style(|theme: &Theme, st| {
        let p = theme.palette();
        let hover = matches!(st, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(Color { a: if hover { 0.12 } else { 0.07 }, ..p.text })),
            text_color: p.text,
            border: Border { radius: 9.0.into(), width: 1.0, color: Color { a: if hover { 0.22 } else { 0.12 }, ..p.text } },
            ..Default::default()
        }
    })
}

/// Control segmentado ("Servidores | AWS | Túneles"). `on(i)` produce el mensaje de la opción `i`.
pub fn segmented<'a>(options: &[&str], selected: usize, accent: Color, fg: Color, on: impl Fn(usize) -> Message) -> Element<'a, Message> {
    let mut r = iced::widget::Row::new().spacing(2);
    for (i, o) in options.iter().enumerate() {
        let sel = i == selected;
        r = r.push(
            button(container(label(o, 12.0, if sel { fg } else { Color { a: 0.6, ..fg } })).center_x(Length::Fill))
                .width(Length::Fill)
                .padding([6, 10])
                .on_press(on(i))
                .style(move |_, st| button::Style {
                    background: Some(Background::Color(if sel { Color { a: 0.16, ..fg } } else if matches!(st, button::Status::Hovered) { Color { a: 0.06, ..fg } } else { Color::TRANSPARENT })),
                    text_color: fg,
                    border: Border { radius: 7.0.into(), width: if sel { 1.0 } else { 0.0 }, color: Color { a: 0.18, ..accent } },
                    ..Default::default()
                }),
        );
    }
    container(r)
        .padding(3)
        .style(move |_| container::Style { background: Some(Background::Color(Color { a: 0.06, ..fg })), border: Border { radius: 10.0.into(), ..Default::default() }, ..Default::default() })
        .into()
}

/// Fila de una lista que se selecciona con un clic y (si `on_open` existe) se abre con doble clic.
pub fn selectable_row<'a>(content: impl Into<Element<'a, Message>>, selected: bool, fg: Color, on_select: Message, on_open: Option<Message>) -> Element<'a, Message> {
    let body = container(content).padding([6, 9]).width(Length::Fill).style(move |_| container::Style {
        background: Some(Background::Color(if selected { Color { a: 0.2, ..fg } } else { Color::TRANSPARENT })),
        border: Border { radius: 6.0.into(), ..Default::default() },
        ..Default::default()
    });
    let mut area = iced::widget::mouse_area(body).on_press(on_select);
    if let Some(open) = on_open {
        area = area.on_double_click(open);
    }
    area.into()
}

/// Texto pequeño en mayúsculas sobre un campo.
pub fn caption<'a>(s: &str, color: Color) -> Text<'a> {
    text(crate::i18n::tr(s).to_uppercase()).size(10).color(Color { a: 0.5, ..color })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_map_to_icons() {
        let ch = |c: char| glyph_icon(c).map(char::from);
        assert_eq!(ch('✎'), Some(char::from(Icon::Pencil)));
        assert_eq!(ch('🪣'), Some(char::from(Icon::Database)));
        assert_eq!(ch('a'), None);
    }

    #[test]
    fn readable_text_color() {
        assert_eq!(on_color(Color::from_rgb8(250, 250, 100)), Color::from_rgb(0.08, 0.09, 0.11));
        assert_eq!(on_color(Color::from_rgb8(30, 40, 120)), Color::WHITE);
    }
}

