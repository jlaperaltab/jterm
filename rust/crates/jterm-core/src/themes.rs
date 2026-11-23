/// Color "#rrggbb" → (r, g, b).
pub fn parse_hex(s: &str) -> (u8, u8, u8) {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0);
    (((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8)
}

pub struct TerminalTheme {
    pub name: &'static str,
    pub bg: &'static str,
    pub fg: &'static str,
    pub cursor: &'static str,
    pub selection: &'static str,
    /// 8 normales + 8 brillantes.
    pub ansi: [&'static str; 16],
}

pub const ACCENTS: [(&str, &str); 8] = [
    ("Azul", "#5c9eff"), ("Violeta", "#a78bfa"), ("Rosa", "#f472b6"), ("Rojo", "#f87171"),
    ("Naranja", "#fb923c"), ("Amarillo", "#facc15"), ("Verde", "#4ade80"), ("Turquesa", "#2dd4bf"),
];

pub fn accent_hex(name: &str) -> &'static str {
    ACCENTS.iter().find(|(n, _)| *n == name).map(|(_, h)| *h).unwrap_or(ACCENTS[0].1)
}

pub const THEMES: [TerminalTheme; 9] = [
    TerminalTheme { name: "jTerm", bg: "#171a1f", fg: "#dbe0eb", cursor: "#5c9eff", selection: "#2f4a73",
        ansi: ["#2b303b", "#ff6b6b", "#7fd98a", "#f2c97d", "#5c9eff", "#c792ea", "#5fd3d3", "#c8cdd8",
               "#4b5263", "#ff8585", "#9be7a4", "#ffdc96", "#82b6ff", "#dab0f5", "#8be9e9", "#eef1f7"] },
    TerminalTheme { name: "Dracula", bg: "#282a36", fg: "#f8f8f2", cursor: "#f8f8f2", selection: "#44475a",
        ansi: ["#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
               "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff"] },
    TerminalTheme { name: "Nord", bg: "#2e3440", fg: "#d8dee9", cursor: "#d8dee9", selection: "#434c5e",
        ansi: ["#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
               "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4"] },
    TerminalTheme { name: "Tokyo Night", bg: "#1a1b26", fg: "#c0caf5", cursor: "#c0caf5", selection: "#33467c",
        ansi: ["#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
               "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5"] },
    TerminalTheme { name: "One Dark", bg: "#282c34", fg: "#abb2bf", cursor: "#528bff", selection: "#3e4451",
        ansi: ["#282c34", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#abb2bf",
               "#5c6370", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#ffffff"] },
    TerminalTheme { name: "Gruvbox", bg: "#282828", fg: "#ebdbb2", cursor: "#ebdbb2", selection: "#504945",
        ansi: ["#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
               "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2"] },
    TerminalTheme { name: "Monokai", bg: "#272822", fg: "#f8f8f2", cursor: "#f8f8f0", selection: "#49483e",
        ansi: ["#272822", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4", "#f8f8f2",
               "#75715e", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4", "#f9f8f5"] },
    TerminalTheme { name: "Solarized Dark", bg: "#002b36", fg: "#839496", cursor: "#93a1a1", selection: "#073642",
        ansi: ["#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
               "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3"] },
    TerminalTheme { name: "Solarized Light", bg: "#fdf6e3", fg: "#657b83", cursor: "#586e75", selection: "#eee8d5",
        ansi: ["#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
               "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3"] },
];

/// El tema con ese nombre, o el primero si no existe.
pub fn theme(name: &str) -> &'static TerminalTheme {
    THEMES.iter().find(|t| t.name == name).unwrap_or(&THEMES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_lookup() {
        assert_eq!(parse_hex("#5c9eff"), (0x5c, 0x9e, 0xff));
        assert_eq!(theme("Nord").name, "Nord");
        assert_eq!(theme("nope").name, "jTerm");
        assert_eq!(accent_hex("Rosa"), "#f472b6");
        assert_eq!(accent_hex("?"), "#5c9eff");
    }
}
