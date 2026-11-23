//! Informes de ratón para programas de terminal (tmux, vim, htop…): protocolos xterm X10/normal y SGR.

use alacritty_terminal::term::TermMode;
use iced::keyboard::Modifiers;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    fn code(self) -> u8 {
        match self {
            Button::Left => 0,
            Button::Middle => 1,
            Button::Right => 2,
        }
    }
}

fn mods(m: Modifiers) -> u8 {
    4 * m.shift() as u8 + 8 * m.alt() as u8 + 16 * m.control() as u8
}

/// Secuencia para un evento. `col` y `row` empiezan en 0. `code` ya incluye botón, modificadores y bit de movimiento.
fn encode(mode: TermMode, code: u8, col: usize, row: usize, release: bool) -> Option<Vec<u8>> {
    let (x, y) = (col + 1, row + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }
    // Protocolos antiguos: un byte por valor (máx. 223) y la liberación se informa como botón 3.
    if x > 223 || y > 223 {
        return None;
    }
    let code = if release { (code & !3) | 3 } else { code };
    Some(vec![0x1b, b'[', b'M', 32 + code, 32 + x as u8, 32 + y as u8])
}

pub fn press(mode: TermMode, b: Button, m: Modifiers, col: usize, row: usize) -> Option<Vec<u8>> {
    encode(mode, b.code() + mods(m), col, row, false)
}

pub fn release(mode: TermMode, b: Button, m: Modifiers, col: usize, row: usize) -> Option<Vec<u8>> {
    // X10 (solo clics) no informa de las liberaciones.
    if !mode.intersects(TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG) && !mode.contains(TermMode::SGR_MOUSE) && !mode.contains(TermMode::MOUSE_REPORT_CLICK) {
        return None;
    }
    encode(mode, b.code() + mods(m), col, row, true)
}

/// Movimiento: con un botón pulsado (arrastre) o sin botón (modo de movimiento completo).
pub fn motion(mode: TermMode, held: Option<Button>, m: Modifiers, col: usize, row: usize) -> Option<Vec<u8>> {
    let any = mode.contains(TermMode::MOUSE_MOTION);
    let drag = mode.contains(TermMode::MOUSE_DRAG);
    match held {
        Some(_) if !(any || drag) => return None,
        None if !any => return None,
        _ => {}
    }
    let base = held.map_or(3, Button::code);
    encode(mode, base + 32 + mods(m), col, row, false)
}

/// Rueda: arriba (`up`) o abajo.
pub fn wheel(mode: TermMode, up: bool, m: Modifiers, col: usize, row: usize) -> Option<Vec<u8>> {
    encode(mode, 64 + (!up) as u8 + mods(m), col, row, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SGR: TermMode = TermMode::from_bits_truncate(TermMode::SGR_MOUSE.bits() | TermMode::MOUSE_REPORT_CLICK.bits());
    const LEGACY: TermMode = TermMode::MOUSE_REPORT_CLICK;

    #[test]
    fn sgr_press_release_and_wheel() {
        let none = Modifiers::empty();
        assert_eq!(press(SGR, Button::Left, none, 0, 0), Some(b"\x1b[<0;1;1M".to_vec()));
        assert_eq!(release(SGR, Button::Left, none, 9, 4), Some(b"\x1b[<0;10;5m".to_vec()));
        assert_eq!(press(SGR, Button::Right, Modifiers::CTRL, 1, 1), Some(b"\x1b[<18;2;2M".to_vec()));
        assert_eq!(wheel(SGR, true, none, 2, 3), Some(b"\x1b[<64;3;4M".to_vec()));
        assert_eq!(wheel(SGR, false, none, 2, 3), Some(b"\x1b[<65;3;4M".to_vec()));
    }

    #[test]
    fn legacy_encoding_and_limits() {
        let none = Modifiers::empty();
        assert_eq!(press(LEGACY, Button::Left, none, 0, 0), Some(vec![0x1b, b'[', b'M', 32, 33, 33]));
        assert_eq!(release(LEGACY, Button::Left, none, 0, 0), Some(vec![0x1b, b'[', b'M', 35, 33, 33]));
        assert_eq!(press(LEGACY, Button::Left, none, 300, 0), None);
    }

    #[test]
    fn motion_depends_on_mode() {
        let none = Modifiers::empty();
        assert_eq!(motion(LEGACY, Some(Button::Left), none, 1, 1), None);
        let drag = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
        assert_eq!(motion(drag, Some(Button::Left), none, 1, 1), Some(b"\x1b[<32;2;2M".to_vec()));
        assert_eq!(motion(drag, None, none, 1, 1), None);
        let any = TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE;
        assert_eq!(motion(any, None, none, 1, 1), Some(b"\x1b[<35;2;2M".to_vec()));
    }
}
