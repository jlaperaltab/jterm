//! Traducción de teclas a los bytes que espera un terminal (xterm).

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

fn modifier_param(m: Modifiers) -> u8 {
    1 + m.shift() as u8 + 2 * m.alt() as u8 + 4 * m.control() as u8
}

fn csi(final_byte: char, m: Modifiers, app_cursor: bool) -> Vec<u8> {
    let p = modifier_param(m);
    if p > 1 {
        format!("\x1b[1;{p}{final_byte}").into_bytes()
    } else if app_cursor {
        format!("\x1bO{final_byte}").into_bytes()
    } else {
        format!("\x1b[{final_byte}").into_bytes()
    }
}

fn tilde(n: u8, m: Modifiers) -> Vec<u8> {
    let p = modifier_param(m);
    if p > 1 { format!("\x1b[{n};{p}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() }
}

/// Bytes para una pulsación. `text` es el texto que produjo la tecla (ya con Mayús/AltGr aplicados).
/// `option_as_meta`: Option/Alt envía ESC + tecla, salvo que el teclado produzca un carácter ASCII
/// con AltGr (`@`, `|`, `#`… en teclados español y latino), que se envía tal cual.
pub fn encode(key: &Key, modified: &Key, text: Option<&str>, m: Modifiers, app_cursor: bool, option_as_meta: bool) -> Option<Vec<u8>> {
    if let Key::Named(n) = key {
        let bytes = match n {
            Named::Enter => b"\r".to_vec(),
            Named::Backspace => if m.alt() { b"\x1b\x7f".to_vec() } else { b"\x7f".to_vec() },
            Named::Tab => if m.shift() { b"\x1b[Z".to_vec() } else { b"\t".to_vec() },
            Named::Escape => b"\x1b".to_vec(),
            Named::Space if m.control() => vec![0],
            Named::Space => if m.alt() && option_as_meta { b"\x1b ".to_vec() } else { b" ".to_vec() },
            // Option+← / Option+→ saltan palabras, como en Terminal.app.
            Named::ArrowLeft if m.alt() && !m.control() && !m.shift() => b"\x1bb".to_vec(),
            Named::ArrowRight if m.alt() && !m.control() && !m.shift() => b"\x1bf".to_vec(),
            Named::ArrowUp => csi('A', m, app_cursor),
            Named::ArrowDown => csi('B', m, app_cursor),
            Named::ArrowRight => csi('C', m, app_cursor),
            Named::ArrowLeft => csi('D', m, app_cursor),
            Named::Home => csi('H', m, app_cursor),
            Named::End => csi('F', m, app_cursor),
            Named::Insert => tilde(2, m),
            Named::Delete => tilde(3, m),
            Named::PageUp => tilde(5, m),
            Named::PageDown => tilde(6, m),
            Named::F1 => ss3_or_csi('P', m),
            Named::F2 => ss3_or_csi('Q', m),
            Named::F3 => ss3_or_csi('R', m),
            Named::F4 => ss3_or_csi('S', m),
            Named::F5 => tilde(15, m),
            Named::F6 => tilde(17, m),
            Named::F7 => tilde(18, m),
            Named::F8 => tilde(19, m),
            Named::F9 => tilde(20, m),
            Named::F10 => tilde(21, m),
            Named::F11 => tilde(23, m),
            Named::F12 => tilde(24, m),
            _ => return None,
        };
        return Some(bytes);
    }

    // Las combinaciones con ⌘ son atajos de la aplicación, no del terminal.
    if m.logo() {
        return None;
    }

    if m.control() {
        if let Key::Character(c) = key {
            let ch = c.chars().next()?.to_ascii_lowercase();
            return match ch {
                'a'..='z' => Some(vec![ch as u8 - b'a' + 1]),
                '[' | '3' => Some(vec![0x1b]),
                '\\' | '4' => Some(vec![0x1c]),
                ']' | '5' => Some(vec![0x1d]),
                '^' | '6' => Some(vec![0x1e]),
                '_' | '7' | '/' => Some(vec![0x1f]),
                '@' | '2' | ' ' => Some(vec![0]),
                _ => None,
            };
        }
        return None;
    }

    let txt = text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control));
    if m.alt() && option_as_meta {
        // AltGr: si la tecla produjo un ASCII distinto de la tecla base, se envía tal cual.
        if let (Some(t), Key::Character(base)) = (txt, key) {
            if t.is_ascii() && !t.eq_ignore_ascii_case(base.as_str()) {
                return Some(t.as_bytes().to_vec());
            }
        }
        let base = match modified {
            Key::Character(c) => c.to_string(),
            _ => return txt.map(|t| t.as_bytes().to_vec()),
        };
        let mut v = vec![0x1b];
        v.extend(base.as_bytes());
        return Some(v);
    }
    txt.map(|t| t.as_bytes().to_vec())
}

fn ss3_or_csi(c: char, m: Modifiers) -> Vec<u8> {
    let p = modifier_param(m);
    if p > 1 { format!("\x1b[1;{p}{c}").into_bytes() } else { format!("\x1bO{c}").into_bytes() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::Named;

    fn ch(s: &str) -> Key {
        Key::Character(s.into())
    }
    fn enc(key: Key, text: Option<&str>, m: Modifiers) -> Option<Vec<u8>> {
        encode(&key.clone(), &key, text, m, false, true)
    }

    #[test]
    fn plain_and_named() {
        assert_eq!(enc(ch("a"), Some("a"), Modifiers::empty()), Some(b"a".to_vec()));
        assert_eq!(enc(ch("ñ"), Some("ñ"), Modifiers::empty()), Some("ñ".as_bytes().to_vec()));
        assert_eq!(enc(Key::Named(Named::Enter), None, Modifiers::empty()), Some(b"\r".to_vec()));
        assert_eq!(enc(Key::Named(Named::Backspace), None, Modifiers::empty()), Some(vec![0x7f]));
        assert_eq!(enc(Key::Named(Named::Tab), None, Modifiers::SHIFT), Some(b"\x1b[Z".to_vec()));
        assert_eq!(enc(Key::Named(Named::Delete), None, Modifiers::empty()), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn arrows_follow_application_cursor_mode() {
        let up = Key::Named(Named::ArrowUp);
        assert_eq!(encode(&up, &up, None, Modifiers::empty(), false, true), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode(&up, &up, None, Modifiers::empty(), true, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(encode(&up, &up, None, Modifiers::SHIFT, true, true), Some(b"\x1b[1;2A".to_vec()));
        assert_eq!(enc(Key::Named(Named::ArrowLeft), None, Modifiers::ALT), Some(b"\x1bb".to_vec()));
    }

    #[test]
    fn control_letters() {
        assert_eq!(enc(ch("c"), Some("\u{3}"), Modifiers::CTRL), Some(vec![3]));
        assert_eq!(enc(ch("D"), None, Modifiers::CTRL), Some(vec![4]));
        assert_eq!(enc(ch("["), None, Modifiers::CTRL), Some(vec![0x1b]));
        assert_eq!(enc(Key::Named(Named::Space), None, Modifiers::CTRL), Some(vec![0]));
    }

    #[test]
    fn option_as_meta_and_altgr() {
        // Option+b en teclado US → ESC b.
        assert_eq!(enc(ch("b"), Some("∫"), Modifiers::ALT), Some(b"\x1bb".to_vec()));
        // AltGr español: Option+2 produce "@" → se envía "@".
        assert_eq!(enc(ch("2"), Some("@"), Modifiers::ALT), Some(b"@".to_vec()));
        // Sin option_as_meta se envía el texto producido.
        let k = ch("b");
        assert_eq!(encode(&k, &k, Some("∫"), Modifiers::ALT, false, false), Some("∫".as_bytes().to_vec()));
    }

    #[test]
    fn command_shortcuts_are_not_sent() {
        assert_eq!(enc(ch("t"), Some("t"), Modifiers::LOGO), None);
        assert_eq!(enc(Key::Named(Named::F5), None, Modifiers::empty()), Some(b"\x1b[15~".to_vec()));
        assert_eq!(enc(Key::Named(Named::F1), None, Modifiers::empty()), Some(b"\x1bOP".to_vec()));
    }
}
