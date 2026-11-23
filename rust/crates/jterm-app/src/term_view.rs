//! Dibujo del terminal sobre un `canvas` de iced.

use crate::backend::{Backend, LinkHit, Seg, Snapshot};
use crate::colors::Palette;
use crate::mouse::{self as xterm_mouse, Button};
use crate::Message;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::index::Side;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;
use iced::advanced::text::Alignment;
use iced::mouse;
use iced::widget::canvas::{self, Action, Cache, Geometry, Program, Text};
use iced::widget::text::{LineHeight, Shaping};
use iced::{alignment, font, Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme};
use std::sync::Arc;

/// Medidas de la celda para una fuente monoespaciada (Menlo avanza 0,602 em).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub font_size: f32,
    pub cw: f32,
    pub ch: f32,
}

impl Metrics {
    pub fn new(font_size: f32) -> Self {
        Self::with_advance(font_size, 0.602)
    }

    /// `advance_em`: ancho de un carácter en unidades de la fuente (em), medido de la fuente real.
    pub fn with_advance(font_size: f32, advance_em: f32) -> Self {
        Metrics { font_size, cw: font_size * advance_em, ch: (font_size * 1.3).ceil() }
    }

    /// Métricas para la fuente de los ajustes, con el ancho real de sus caracteres.
    pub fn for_settings(font_family: &str, font_size: f32, medium: bool) -> Self {
        Self::with_advance(font_size, advance_em(font_family, medium))
    }

    /// Columnas y filas que caben en `size`.
    pub fn grid(&self, size: Size) -> (u16, u16) {
        (((size.width / self.cw).floor() as u16).max(2), ((size.height / self.ch).floor() as u16).max(1))
    }

    /// (fila, columna, lado) bajo un punto del canvas.
    pub fn locate(&self, p: Point, cols: u16, lines: u16) -> (usize, usize, Side) {
        let x = p.x.max(0.0);
        let col = ((x / self.cw) as usize).min(cols.saturating_sub(1) as usize);
        let row = ((p.y.max(0.0) / self.ch) as usize).min(lines.saturating_sub(1) as usize);
        let side = if x - col as f32 * self.cw > self.cw / 2.0 { Side::Right } else { Side::Left };
        (row, col, side)
    }
}

/// Fuente monoespaciada de respaldo para textos de la interfaz.
pub const FONT: Font = Font::with_name("Menlo");

/// ¿Se cargó SF Mono desde Terminal.app? (la fuente del sistema no se puede pedir por nombre sin cargarla)
pub static SF_MONO_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Fuente del terminal según el ajuste ("system" = SF Mono si está disponible, si no Menlo).
pub fn terminal_font(family: &str) -> Font {
    match family {
        "Menlo" => Font::with_name("Menlo"),
        "Monaco" => Font::with_name("Monaco"),
        "Courier New" => Font::with_name("Courier New"),
        _ if SF_MONO_LOADED.load(std::sync::atomic::Ordering::Relaxed) => Font::with_name("SF Mono"),
        _ => FONT,
    }
}

/// Nombre de familia que se pide a la fuente para un ajuste.
pub fn family_name(family: &str) -> &'static str {
    match family {
        "Menlo" => "Menlo",
        "Monaco" => "Monaco",
        "Courier New" => "Courier New",
        _ if SF_MONO_LOADED.load(std::sync::atomic::Ordering::Relaxed) => "SF Mono",
        _ => "Menlo",
    }
}

/// Ancho real de un carácter (en em) de la fuente elegida, medido con el mismo motor de texto que dibuja el terminal.
/// Si el motor no puede medirla, 0,602 (Menlo y SF Mono). Con un valor equivocado, los tramos de texto se desalinean
/// de la cuadrícula (huecos y letras que se pisan en programas como nano).
pub fn advance_em(font_family: &str, medium: bool) -> f32 {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(String, bool), f32>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (family_name(font_family).to_string(), medium);
    if let Some(v) = cache.lock().unwrap().get(&key) {
        return *v;
    }
    let measured = measure_advance_em(&key.0, medium).filter(|v| (0.4..0.9).contains(v)).unwrap_or(0.602);
    cache.lock().unwrap().insert(key, measured);
    measured
}

pub fn measure_advance_em(family: &str, medium: bool) -> Option<f32> {
    use iced::advanced::graphics::text::cosmic_text::{Attrs, Buffer, Family, Metrics as CtMetrics, Shaping, Weight};
    let fs = iced::advanced::graphics::text::font_system();
    let mut guard = fs.write().ok()?;
    let raw = guard.raw();
    let size = 100.0;
    let mut buf = Buffer::new(raw, CtMetrics::new(size, size * 1.3));
    buf.set_size(raw, Some(100_000.0), Some(1_000.0));
    let attrs = Attrs::new().family(Family::Name(family)).weight(Weight(if medium { 500 } else { 400 }));
    let sample = "0".repeat(80);
    buf.set_text(raw, &sample, &attrs, Shaping::Basic, None);
    buf.shape_until_scroll(raw, false);
    let run = buf.layout_runs().next()?;
    let last = run.glyphs.last()?;
    Some((last.x + last.w) / 80.0 / size)
}

pub struct TermView<'a> {
    pub tab: u64,
    pub backend: &'a Arc<Backend>,
    pub palette: &'a Palette,
    pub metrics: Metrics,
    pub cache: &'a Cache,
    pub cursor_color: Color,
    pub font: Font,
    pub medium: bool,
    /// "block", "underline" o "beam": forma del cursor elegida en ajustes.
    pub cursor_shape: &'a str,
    pub mouse_reporting: bool,
    /// Las rutas de archivo solo son enlaces en pestañas locales (en SSH son del servidor).
    pub allow_paths: bool,
    pub link_color: Color,
}

#[derive(Default)]
pub struct ViewState {
    grid: (u16, u16),
    dragging: bool,
    mods: iced::keyboard::Modifiers,
    /// Botón que la aplicación del terminal ve pulsado (informes de ratón).
    held: Option<Button>,
    last_cell: (usize, usize),
    hover: Option<LinkHit>,
}

impl<'a> Program<Message> for TermView<'a> {
    type State = ViewState;

    fn update(&self, st: &mut ViewState, event: &canvas::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Action<Message>> {
        use iced::Event::{Keyboard, Mouse, Window};
        let m = self.metrics;
        let write = |bytes: Option<Vec<u8>>| bytes.map(|b| Action::publish(Message::Write(self.tab, b)).and_capture());
        // ¿La aplicación pide los eventos del ratón? Con Mayús pulsado, el terminal conserva el control (selección).
        let reporting = || self.mouse_reporting && !st.mods.shift() && self.backend.mode().intersects(TermMode::MOUSE_MODE);
        match event {
            Keyboard(iced::keyboard::Event::ModifiersChanged(mods)) => {
                st.mods = *mods;
                if !mods.logo() && st.hover.take().is_some() {
                    return Some(Action::publish(Message::Redraw(self.tab)));
                }
                None
            }
            Window(_) => {
                // El estado de un canvas puede reutilizarse al cambiar de pestaña o dividir un panel, así que no basta
                // con recordar el último tamaño: se compara con el tamaño real del terminal de ESTE panel.
                let g = m.grid(bounds.size());
                let (cols, lines) = self.backend.size();
                if g != st.grid || (cols, lines) != (g.0 as usize, g.1 as usize) {
                    st.grid = g;
                    return Some(Action::publish(Message::Grid(self.tab, g.0, g.1)));
                }
                None
            }
            Mouse(mouse::Event::ButtonPressed(btn)) => {
                let p = cursor.position_in(bounds)?;
                let (r, c, s) = m.locate(p, st.grid.0, st.grid.1);
                // ⌘+clic sobre un enlace lo abre, aunque la aplicación tenga el ratón.
                if st.mods.logo() && *btn == mouse::Button::Left {
                    if let Some(h) = self.backend.link_at(r, c, self.allow_paths) {
                        return Some(Action::publish(Message::OpenLink(h.kind, h.text)).and_capture());
                    }
                }
                if reporting() {
                    let b = match btn {
                        mouse::Button::Left => Button::Left,
                        mouse::Button::Middle => Button::Middle,
                        mouse::Button::Right => Button::Right,
                        _ => return None,
                    };
                    st.held = Some(b);
                    st.last_cell = (r, c);
                    return write(xterm_mouse::press(self.backend.mode(), b, st.mods, c, r));
                }
                if *btn != mouse::Button::Left {
                    return None;
                }
                st.dragging = true;
                Some(Action::publish(Message::SelStart(self.tab, r, c, s)).and_capture())
            }
            Mouse(mouse::Event::CursorMoved { .. }) => {
                // Con ⌘ pulsado se subraya el enlace bajo el cursor.
                let hit = if st.mods.logo() {
                    cursor.position_in(bounds).and_then(|p| {
                        let (r, c, _) = m.locate(p, st.grid.0, st.grid.1);
                        self.backend.link_at(r, c, self.allow_paths)
                    })
                } else {
                    None
                };
                if hit != st.hover {
                    st.hover = hit;
                    return Some(Action::publish(Message::Redraw(self.tab)));
                }
                if st.held.is_some() || (reporting() && cursor.is_over(bounds)) {
                    let p = cursor.position_from(bounds.position())?;
                    let (r, c, _) = m.locate(p, st.grid.0, st.grid.1);
                    if (r, c) == st.last_cell {
                        return None;
                    }
                    st.last_cell = (r, c);
                    return write(xterm_mouse::motion(self.backend.mode(), st.held, st.mods, c, r));
                }
                if st.dragging {
                    let p = cursor.position_from(bounds.position())?;
                    let (r, c, s) = m.locate(p, st.grid.0, st.grid.1);
                    return Some(Action::publish(Message::SelMove(self.tab, r, c, s)));
                }
                None
            }
            Mouse(mouse::Event::ButtonReleased(btn)) => {
                if let Some(b) = st.held.take() {
                    let p = cursor.position_from(bounds.position())?;
                    let (r, c, _) = m.locate(p, st.grid.0, st.grid.1);
                    return write(xterm_mouse::release(self.backend.mode(), b, st.mods, c, r));
                }
                if *btn == mouse::Button::Left {
                    st.dragging = false;
                }
                None
            }
            Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 3.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / m.ch,
                };
                if reporting() {
                    let p = cursor.position_in(bounds)?;
                    let (r, c, _) = m.locate(p, st.grid.0, st.grid.1);
                    let n = (lines.abs().round() as usize).clamp(1, 5);
                    let one = xterm_mouse::wheel(self.backend.mode(), lines > 0.0, st.mods, c, r)?;
                    return write(Some(one.repeat(n)));
                }
                Some(Action::publish(Message::Scroll(self.tab, lines)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, st: &ViewState, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            let snap = self.backend.snapshot();
            let hover: &[Seg] = st.hover.as_ref().map_or(&[], |h| &h.segs);
            paint(frame, &snap, &Style { hover, link_color: self.link_color, palette: self.palette, metrics: self.metrics, cursor_color: self.cursor_color, font: self.font, medium: self.medium, cursor_shape: self.cursor_shape }, bounds.size());
        });
        vec![geometry]
    }

    fn mouse_interaction(&self, st: &ViewState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if st.hover.is_some() && cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else if cursor.is_over(bounds) { mouse::Interaction::Text } else { mouse::Interaction::default() }
    }
}

struct Run {
    col: usize,
    text: String,
    color: Color,
    bold: bool,
    italic: bool,
}

struct Style<'a> {
    palette: &'a Palette,
    metrics: Metrics,
    cursor_color: Color,
    font: Font,
    medium: bool,
    cursor_shape: &'a str,
    hover: &'a [Seg],
    link_color: Color,
}

fn font_for(base: Font, medium: bool, bold: bool, italic: bool) -> Font {
    Font {
        weight: if bold {
            font::Weight::Bold
        } else if medium {
            font::Weight::Medium
        } else {
            font::Weight::Normal
        },
        style: if italic { font::Style::Italic } else { font::Style::Normal },
        ..base
    }
}

fn paint(frame: &mut canvas::Frame, s: &Snapshot, st: &Style, size: Size) {
    let (p, m, cursor_color) = (st.palette, st.metrics, st.cursor_color);
    frame.fill_rectangle(Point::ORIGIN, size, p.bg);
    let cell_size = Size::new(m.cw + 0.5, m.ch);
    for row in 0..s.lines {
        let y = row as f32 * m.ch;

        // Fondos: tramos contiguos del mismo color.
        let mut col = 0;
        while col < s.cols {
            let (bg, _) = colors_of(s, p, row, col);
            let start = col;
            while col < s.cols && colors_of(s, p, row, col).0 == bg {
                col += 1;
            }
            if bg != p.bg {
                frame.fill_rectangle(Point::new(start as f32 * m.cw, y), Size::new((col - start) as f32 * m.cw + 0.5, m.ch), bg);
            }
        }

        // Texto: los tramos ASCII del mismo estilo se dibujan juntos; el resto, carácter a carácter para no desalinear.
        let mut runs: Vec<Run> = Vec::new();
        for col in 0..s.cols {
            let c = s.cell(row, col);
            if c.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::HIDDEN) || c.ch == ' ' || c.ch == '\0' {
                continue;
            }
            let (_, fg) = colors_of(s, p, row, col);
            let bold = c.flags.intersects(Flags::BOLD);
            let italic = c.flags.intersects(Flags::ITALIC);
            if c.ch.is_ascii() {
                if let Some(r) = runs.last_mut() {
                    if r.color == fg && r.bold == bold && r.italic == italic && r.col + r.text.len() == col {
                        r.text.push(c.ch);
                        continue;
                    }
                }
            }
            runs.push(Run { col, text: c.ch.to_string(), color: fg, bold, italic });
        }
        for r in runs {
            let shaping = if r.text.is_ascii() { Shaping::Basic } else { Shaping::Advanced };
            frame.fill_text(Text {
                content: r.text,
                position: Point::new(r.col as f32 * m.cw, y),
                max_width: f32::INFINITY,
                color: r.color,
                size: Pixels(m.font_size),
                line_height: LineHeight::Absolute(Pixels(m.ch)),
                font: font_for(st.font, st.medium, r.bold, r.italic),
                align_x: Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping,
            });
        }

        // Subrayado y tachado.
        for col in 0..s.cols {
            let c = s.cell(row, col);
            let line = |frame: &mut canvas::Frame, dy: f32| {
                let (_, fg) = colors_of(s, p, row, col);
                frame.fill_rectangle(Point::new(col as f32 * m.cw, y + dy), Size::new(m.cw + 0.5, 1.0), fg);
            };
            if c.flags.intersects(Flags::ALL_UNDERLINES) {
                line(frame, m.ch - 2.0);
            }
            if c.flags.contains(Flags::STRIKEOUT) {
                line(frame, m.ch / 2.0);
            }
        }
    }

    // Enlaces: las URLs siempre (si el ajuste lo pide) y el enlace bajo el cursor con ⌘, más marcado.
    for sg in &s.links {
        frame.fill_rectangle(Point::new(sg.col as f32 * m.cw, sg.row as f32 * m.ch + m.ch - 2.0), Size::new(sg.len as f32 * m.cw, 1.0), Color { a: 0.6, ..st.link_color });
    }
    for sg in st.hover {
        frame.fill_rectangle(Point::new(sg.col as f32 * m.cw, sg.row as f32 * m.ch + m.ch - 2.5), Size::new(sg.len as f32 * m.cw, 1.5), st.link_color);
    }

    // Coincidencias de la búsqueda.
    for &(row, col, len, current) in &s.matches {
        let color = if current { Color::from_rgba(1.0, 0.78, 0.2, 0.75) } else { Color::from_rgba(1.0, 0.78, 0.2, 0.32) };
        frame.fill_rectangle(Point::new(col as f32 * m.cw, row as f32 * m.ch), Size::new(len as f32 * m.cw, m.ch), color);
    }

    if let Some((row, col, shape)) = s.cursor {
        // La forma de los ajustes solo manda mientras el programa no pida otra (vim usa barra en insertar, etc.).
        let shape = match (shape, st.cursor_shape) {
            (CursorShape::Block, "underline") => CursorShape::Underline,
            (CursorShape::Block, "beam") => CursorShape::Beam,
            (other, _) => other,
        };
        let (x, y) = (col as f32 * m.cw, row as f32 * m.ch);
        match shape {
            CursorShape::Underline => frame.fill_rectangle(Point::new(x, y + m.ch - 2.0), Size::new(m.cw, 2.0), cursor_color),
            CursorShape::Beam => frame.fill_rectangle(Point::new(x, y), Size::new(2.0, m.ch), cursor_color),
            CursorShape::HollowBlock => {
                let path = canvas::Path::rectangle(Point::new(x + 0.5, y + 0.5), Size::new(m.cw - 1.0, m.ch - 1.0));
                frame.stroke(&path, canvas::Stroke::default().with_color(cursor_color).with_width(1.0));
            }
            _ => {
                frame.fill_rectangle(Point::new(x, y), cell_size, cursor_color);
                // El carácter bajo el cursor se vuelve a dibujar con el color de fondo para que se lea.
                let c = s.cell(row, col);
                if c.ch != ' ' && c.ch != '\0' {
                    frame.fill_text(Text {
                        content: c.ch.to_string(),
                        position: Point::new(x, y),
                        max_width: f32::INFINITY,
                        color: p.bg,
                        size: Pixels(m.font_size),
                        line_height: LineHeight::Absolute(Pixels(m.ch)),
                        font: font_for(st.font, st.medium, c.flags.intersects(Flags::BOLD), c.flags.intersects(Flags::ITALIC)),
                        align_x: Alignment::Left,
                        align_y: alignment::Vertical::Top,
                        shaping: Shaping::Advanced,
                    });
                }
            }
        }
    }
}

/// (fondo, texto) de una celda, con vídeo inverso y selección aplicados.
fn colors_of(s: &Snapshot, p: &Palette, row: usize, col: usize) -> (Color, Color) {
    let c = s.cell(row, col);
    let (mut fg, mut bg) = (p.resolve(c.fg, true), p.resolve(c.bg, false));
    if c.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if s.is_selected(row, col) {
        bg = p.selection;
        fg = p.fg;
    }
    (bg, fg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menlo_advance_is_measured_from_the_real_font() {
        // Menlo viene con macOS: su avance es 1233/2048 em ≈ 0,602.
        let v = measure_advance_em("Menlo", false).expect("el motor de texto mide Menlo");
        assert!((v - 0.602).abs() < 0.01, "{v}");
        // Una familia inexistente cae al valor por defecto sin romper nada.
        assert!((advance_em("Fuente Que No Existe", false) - 0.602).abs() < 0.2);
        let m = Metrics::with_advance(10.0, 0.5);
        assert_eq!((m.cw, m.ch), (5.0, 13.0));
    }

    #[test]
    fn grid_and_locate() {
        let m = Metrics::new(13.0);
        let (cols, lines) = m.grid(Size::new(800.0, 600.0));
        assert_eq!((cols, lines), (102, 35));
        assert_eq!(m.grid(Size::new(1.0, 1.0)), (2, 1));
        let (r, c, side) = m.locate(Point::new(m.cw * 3.0 + 1.0, m.ch * 2.0 + 1.0), cols, lines);
        assert_eq!((r, c, side), (2, 3, Side::Left));
        let (_, c, side) = m.locate(Point::new(m.cw * 3.0 + m.cw * 0.9, 0.0), cols, lines);
        assert_eq!((c, side), (3, Side::Right));
        // Fuera de los límites se ajusta al borde.
        let (r, c, _) = m.locate(Point::new(99999.0, 99999.0), cols, lines);
        assert_eq!((r, c), (lines as usize - 1, cols as usize - 1));
    }
}

