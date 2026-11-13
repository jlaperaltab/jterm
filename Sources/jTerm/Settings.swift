import AppKit

// MARK: - Temas de color

extension NSColor {
    /// "#rrggbb"
    convenience init(hex: String) {
        let s = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        var v: UInt64 = 0
        Scanner(string: s).scanHexInt64(&v)
        self.init(srgbRed: CGFloat((v >> 16) & 0xff) / 255, green: CGFloat((v >> 8) & 0xff) / 255,
                  blue: CGFloat(v & 0xff) / 255, alpha: 1)
    }
}

struct TerminalTheme {
    let name: String
    let bg: String, fg: String, cursor: String, selection: String
    let ansi: [String]   // 8 normales + 8 brillantes
    var bgColor: NSColor { NSColor(hex: bg) }
    var fgColor: NSColor { NSColor(hex: fg) }
    var cursorColor: NSColor { NSColor(hex: cursor) }
    var selectionColor: NSColor { NSColor(hex: selection) }

    static let all: [TerminalTheme] = [
        TerminalTheme(name: "jTerm", bg: "#171a1f", fg: "#dbe0eb", cursor: "#5c9eff", selection: "#2f4a73",
            ansi: ["#2b303b", "#ff6b6b", "#7fd98a", "#f2c97d", "#5c9eff", "#c792ea", "#5fd3d3", "#c8cdd8",
                   "#4b5263", "#ff8585", "#9be7a4", "#ffdc96", "#82b6ff", "#dab0f5", "#8be9e9", "#eef1f7"]),
        TerminalTheme(name: "Dracula", bg: "#282a36", fg: "#f8f8f2", cursor: "#f8f8f2", selection: "#44475a",
            ansi: ["#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
                   "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff"]),
        TerminalTheme(name: "Nord", bg: "#2e3440", fg: "#d8dee9", cursor: "#d8dee9", selection: "#434c5e",
            ansi: ["#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
                   "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4"]),
        TerminalTheme(name: "Tokyo Night", bg: "#1a1b26", fg: "#c0caf5", cursor: "#c0caf5", selection: "#33467c",
            ansi: ["#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
                   "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5"]),
        TerminalTheme(name: "One Dark", bg: "#282c34", fg: "#abb2bf", cursor: "#528bff", selection: "#3e4451",
            ansi: ["#282c34", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#abb2bf",
                   "#5c6370", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#ffffff"]),
        TerminalTheme(name: "Gruvbox", bg: "#282828", fg: "#ebdbb2", cursor: "#ebdbb2", selection: "#504945",
            ansi: ["#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
                   "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2"]),
        TerminalTheme(name: "Monokai", bg: "#272822", fg: "#f8f8f2", cursor: "#f8f8f0", selection: "#49483e",
            ansi: ["#272822", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4", "#f8f8f2",
                   "#75715e", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4", "#f9f8f5"]),
        TerminalTheme(name: "Solarized Dark", bg: "#002b36", fg: "#839496", cursor: "#93a1a1", selection: "#073642",
            ansi: ["#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
                   "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3"]),
        TerminalTheme(name: "Solarized Light", bg: "#fdf6e3", fg: "#657b83", cursor: "#586e75", selection: "#eee8d5",
            ansi: ["#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
                   "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3"]),
    ]
}

// MARK: - Ajustes

/// Preferencias del usuario (UserDefaults). Cada cambio notifica a las pestañas abiertas.
final class Settings {
    static let shared = Settings()
    static let didChange = Notification.Name("jterm.settings.didChange")
    static let accentDidChange = Notification.Name("jterm.settings.accentDidChange")
    private let d = UserDefaults.standard
    private static let prefix = "pref."

    private init() {
        d.register(defaults: [
            k("loginShell"): true, k("confirmClose"): true, k("confirmQuit"): true, k("sidebarAtLaunch"): true, k("showSnippetsButton"): true,
            k("theme"): "jTerm", k("accent"): "Azul", k("fontFamily"): "system", k("fontSize"): 13.0,
            k("cursorShape"): "block", k("cursorBlink"): true, k("scrollback"): 5000,
            k("optionAsMeta"): true, k("mouseReporting"): true, k("backspaceCtrlH"): false, k("term"): "xterm-256color",
            k("confirmMultilinePaste"): true, k("warnDangerousProd"): true, k("autoRecordProd"): false, k("underlineLinks"): true,
            k("sshKeepAlive"): 30, k("sshHostKey"): "accept-new", k("sshCompression"): false, k("sshForwardAgent"): false,
            k("defaultPort"): 22,
            k("filesLocalHidden"): false, k("filesRemoteHidden"): false, k("filesConfirmDelete"): true, k("transferDismiss"): 4,
            k("awsRegion"): "us-east-1", k("awsUser"): "ec2-user",
        ])
    }

    private func k(_ n: String) -> String { Self.prefix + n }
    private func changed() {
        NotificationCenter.default.post(name: Self.didChange, object: nil)
        NotificationCenter.default.post(name: .syncLocalChanged, object: nil)
    }
    private func set(_ v: Any?, _ n: String) { d.set(v, forKey: k(n)); changed() }
    private func s(_ n: String) -> String { d.string(forKey: k(n)) ?? "" }
    private func b(_ n: String) -> Bool { d.bool(forKey: k(n)) }
    private func i(_ n: String) -> Int { d.integer(forKey: k(n)) }

    // General
    var shell: String { get { s("shell") } set { set(newValue, "shell") } }
    var loginShell: Bool { get { b("loginShell") } set { set(newValue, "loginShell") } }
    var startDirectory: String { get { s("startDir") } set { set(newValue, "startDir") } }
    var confirmClose: Bool { get { b("confirmClose") } set { set(newValue, "confirmClose") } }
    var confirmQuit: Bool { get { b("confirmQuit") } set { set(newValue, "confirmQuit") } }
    var sidebarAtLaunch: Bool { get { b("sidebarAtLaunch") } set { set(newValue, "sidebarAtLaunch") } }
    var showSnippetsButton: Bool { get { b("showSnippetsButton") } set { set(newValue, "showSnippetsButton") } }
    // Terminal
    var themeName: String { get { s("theme") } set { set(newValue, "theme") } }
    /// Colores de acento disponibles (nombre → hex).
    static let accents: [(name: String, hex: String)] = [
        ("Azul", "#5c9eff"), ("Violeta", "#a78bfa"), ("Rosa", "#f472b6"), ("Rojo", "#f87171"),
        ("Naranja", "#fb923c"), ("Amarillo", "#facc15"), ("Verde", "#4ade80"), ("Turquesa", "#2dd4bf"),
    ]
    var accentName: String { get { s("accent") } set { set(newValue, "accent"); NotificationCenter.default.post(name: Self.accentDidChange, object: nil) } }
    private(set) lazy var accentCache: (String, NSColor) = ("", .systemBlue)
    var accentColor: NSColor {
        let name = accentName
        if accentCache.0 == name { return accentCache.1 }
        let hex = Self.accents.first { $0.name == name }?.hex ?? Self.accents[0].hex
        let v = Int(hex.dropFirst(), radix: 16) ?? 0x5c9eff
        let c = NSColor(srgbRed: CGFloat((v >> 16) & 255) / 255, green: CGFloat((v >> 8) & 255) / 255, blue: CGFloat(v & 255) / 255, alpha: 1)
        accentCache = (name, c)
        return c
    }
    var fontFamily: String { get { s("fontFamily") } set { set(newValue, "fontFamily") } }
    var fontSize: Int { get { d.double(forKey: k("fontSize")) > 0 ? Int(d.double(forKey: k("fontSize"))) : 13 } set { set(Double(newValue), "fontSize") } }
    var cursorShape: String { get { s("cursorShape") } set { set(newValue, "cursorShape") } }
    var cursorBlink: Bool { get { b("cursorBlink") } set { set(newValue, "cursorBlink") } }
    var scrollback: Int { get { i("scrollback") } set { set(newValue, "scrollback") } }
    var optionAsMeta: Bool { get { b("optionAsMeta") } set { set(newValue, "optionAsMeta") } }
    var mouseReporting: Bool { get { b("mouseReporting") } set { set(newValue, "mouseReporting") } }
    var backspaceCtrlH: Bool { get { b("backspaceCtrlH") } set { set(newValue, "backspaceCtrlH") } }
    var termName: String { get { s("term") } set { set(newValue, "term") } }
    var confirmMultilinePaste: Bool { get { b("confirmMultilinePaste") } set { set(newValue, "confirmMultilinePaste") } }
    var underlineLinks: Bool { get { b("underlineLinks") } set { set(newValue, "underlineLinks") } }
    var autoRecordProd: Bool { get { b("autoRecordProd") } set { set(newValue, "autoRecordProd") } }
    var recordingDirectory: String { get { s("recordingDir") } set { set(newValue, "recordingDir") } }
    var warnDangerousProd: Bool { get { b("warnDangerousProd") } set { set(newValue, "warnDangerousProd") } }
    // SSH
    var sshKeepAlive: Int { get { i("sshKeepAlive") } set { set(newValue, "sshKeepAlive") } }
    var sshHostKey: String { get { s("sshHostKey") } set { set(newValue, "sshHostKey") } }
    var sshCompression: Bool { get { b("sshCompression") } set { set(newValue, "sshCompression") } }
    var sshForwardAgent: Bool { get { b("sshForwardAgent") } set { set(newValue, "sshForwardAgent") } }
    var sshExtraOptions: String { get { s("sshExtra") } set { set(newValue, "sshExtra") } }
    var defaultUser: String { get { s("defaultUser") } set { set(newValue, "defaultUser") } }
    var defaultPort: Int { get { i("defaultPort") } set { set(newValue, "defaultPort") } }
    // Archivos
    var filesLocalHidden: Bool { get { b("filesLocalHidden") } set { set(newValue, "filesLocalHidden") } }
    var filesRemoteHidden: Bool { get { b("filesRemoteHidden") } set { set(newValue, "filesRemoteHidden") } }
    var filesLocalStart: String { get { s("filesLocalStart") } set { set(newValue, "filesLocalStart") } }
    var filesConfirmDelete: Bool { get { b("filesConfirmDelete") } set { set(newValue, "filesConfirmDelete") } }
    var transferDismiss: Int { get { i("transferDismiss") } set { set(newValue, "transferDismiss") } }
    // AWS
    var awsRegion: String { get { s("awsRegion") } set { set(newValue, "awsRegion") } }
    var awsUser: String { get { s("awsUser") } set { set(newValue, "awsUser") } }
    var awsRegions: [String] {
        get { (d.array(forKey: k("awsRegions")) as? [String]) ?? AWSClient.regions }
        set { set(newValue, "awsRegions") }
    }

    // Derivados
    var theme: TerminalTheme { TerminalTheme.all.first { $0.name == themeName } ?? TerminalTheme.all[0] }

    var font: NSFont {
        let size = CGFloat(fontSize)
        if fontFamily != "system", let f = NSFontManager.shared.font(withFamily: fontFamily, traits: [], weight: 5, size: size) { return f }
        return NSFont.monospacedSystemFont(ofSize: size, weight: .regular)
    }

    var resolvedShell: String {
        if !shell.isEmpty, FileManager.default.isExecutableFile(atPath: shell) { return shell }
        return ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
    }
    var resolvedStartDirectory: String? {
        var isDir: ObjCBool = false
        let p = (startDirectory as NSString).expandingTildeInPath
        return !startDirectory.isEmpty && FileManager.default.fileExists(atPath: p, isDirectory: &isDir) && isDir.boolValue ? p : nil
    }
    var resolvedLocalStart: String {
        var isDir: ObjCBool = false
        let p = (filesLocalStart as NSString).expandingTildeInPath
        return !filesLocalStart.isEmpty && FileManager.default.fileExists(atPath: p, isDirectory: &isDir) && isDir.boolValue ? p : NSHomeDirectory()
    }

    func resetAll() {
        for key in d.dictionaryRepresentation().keys where key.hasPrefix(Self.prefix) { d.removeObject(forKey: key) }
        changed()
        NotificationCenter.default.post(name: Self.accentDidChange, object: nil)
    }

    /// Shells instalados (de /etc/shells).
    static func installedShells() -> [String] {
        guard let text = try? String(contentsOfFile: "/etc/shells") else { return ["/bin/zsh", "/bin/bash"] }
        return text.split(separator: "\n").map { String($0).trimmingCharacters(in: .whitespaces) }
            .filter { $0.hasPrefix("/") && FileManager.default.isExecutableFile(atPath: $0) }
    }

    /// Familias de fuente monoespaciadas instaladas.
    static func monospacedFamilies() -> [String] {
        NSFontManager.shared.availableFontFamilies.filter {
            NSFontManager.shared.font(withFamily: $0, traits: [], weight: 5, size: 12)?.isFixedPitch == true
        }
    }
}

/// Opciones de `ssh` derivadas de los ajustes (compartidas por terminal, archivos y AWS).
enum SSHSettings {
    static func args() -> [String] {
        let s = Settings.shared
        var a = ["-o", "StrictHostKeyChecking=\(s.sshHostKey)"]
        if s.sshKeepAlive > 0 { a += ["-o", "ServerAliveInterval=\(s.sshKeepAlive)"] }
        if s.sshCompression { a.append("-C") }
        if s.sshForwardAgent { a += ["-o", "ForwardAgent=yes"] }
        for raw in s.sshExtraOptions.split(whereSeparator: \.isNewline) {
            let line = String(raw).trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") { continue }
            a += ["-o", line]
        }
        return a
    }
}
