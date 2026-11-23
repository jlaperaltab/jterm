//! Colores del terminal: paleta de 256 colores y tema → iced::Color.

use alacritty_terminal::vte::ansi::{Color as TermColor, NamedColor};
use iced::Color;
use jterm_core::themes::{parse_hex, TerminalTheme};

pub fn rgb(hex: &str) -> Color {
    let (r, g, b) = parse_hex(hex);
    Color::from_rgb8(r, g, b)
}

pub struct Palette {
    pub fg: Color,
    pub bg: Color,
    pub cursor: Color,
    pub selection: Color,
    pub ansi: [Color; 16],
}

impl Palette {
    pub fn from_theme(t: &TerminalTheme) -> Self {
        let mut ansi = [Color::BLACK; 16];
        for (i, h) in t.ansi.iter().enumerate() {
            ansi[i] = rgb(h);
        }
        Palette { fg: rgb(t.fg), bg: rgb(t.bg), cursor: rgb(t.cursor), selection: rgb(t.selection), ansi }
    }

    /// Índice 0-255 del modo indexado de xterm.
    pub fn indexed(&self, i: u8) -> Color {
        match i {
            0..=15 => self.ansi[i as usize],
            16..=231 => {
                let n = i - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
                Color::from_rgb8(level(n / 36), level((n / 6) % 6), level(n % 6))
            }
            232..=255 => {
                let v = 8 + 10 * (i - 232);
                Color::from_rgb8(v, v, v)
            }
        }
    }

    pub fn resolve(&self, c: TermColor, is_fg: bool) -> Color {
        match c {
            TermColor::Spec(rgb) => Color::from_rgb8(rgb.r, rgb.g, rgb.b),
            TermColor::Indexed(i) => self.indexed(i),
            TermColor::Named(n) => match n {
                NamedColor::Foreground | NamedColor::BrightForeground => self.fg,
                NamedColor::Background => self.bg,
                NamedColor::Cursor => self.cursor,
                NamedColor::DimForeground => dim(self.fg),
                other => {
                    let i = other as usize;
                    if i < 16 {
                        self.ansi[i]
                    } else if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&i) {
                        dim(self.ansi[i - NamedColor::DimBlack as usize])
                    } else if is_fg { self.fg } else { self.bg }
                }
            },
        }
    }
}

fn dim(c: Color) -> Color {
    Color { r: c.r * 0.66, g: c.g * 0.66, b: c.b * 0.66, a: c.a }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::Rgb;
    use jterm_core::themes::theme;

    #[test]
    fn xterm_256() {
        let p = Palette::from_theme(theme("jTerm"));
        assert_eq!(p.indexed(16), Color::from_rgb8(0, 0, 0));
        assert_eq!(p.indexed(231), Color::from_rgb8(255, 255, 255));
        assert_eq!(p.indexed(196), Color::from_rgb8(255, 0, 0));
        assert_eq!(p.indexed(232), Color::from_rgb8(8, 8, 8));
        assert_eq!(p.indexed(255), Color::from_rgb8(238, 238, 238));
        assert_eq!(p.indexed(1), rgb("#ff6b6b"));
    }

    #[test]
    fn resolves_named_and_spec() {
        let p = Palette::from_theme(theme("Nord"));
        assert_eq!(p.resolve(TermColor::Named(NamedColor::Foreground), true), rgb("#d8dee9"));
        assert_eq!(p.resolve(TermColor::Named(NamedColor::Background), false), rgb("#2e3440"));
        assert_eq!(p.resolve(TermColor::Named(NamedColor::Red), true), rgb("#bf616a"));
        assert_eq!(p.resolve(TermColor::Named(NamedColor::BrightBlue), true), p.ansi[12]);
        assert_eq!(p.resolve(TermColor::Spec(Rgb { r: 1, g: 2, b: 3 }), true), Color::from_rgb8(1, 2, 3));
        assert_eq!(p.resolve(TermColor::Indexed(2), true), p.ansi[2]);
    }
}
