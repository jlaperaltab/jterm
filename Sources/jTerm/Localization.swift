import AppKit

/// Idioma de la interfaz. El español es el idioma base: `L("texto en español")` devuelve la traducción
/// si el idioma activo es otro y existe en `Strings.en`; si no, devuelve el mismo texto.
enum Lang: String, CaseIterable {
    case system, es, en

    static let key = "pref.language"
    static let didChange = Notification.Name("jterm.language.didChange")

    /// Idioma elegido por el usuario (o `.system`).
    static var preference: Lang {
        get { Lang(rawValue: UserDefaults.standard.string(forKey: key) ?? "") ?? .system }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: key); NotificationCenter.default.post(name: didChange, object: nil) }
    }

    /// Idioma efectivo: nunca `.system`. Se calcula una vez por arranque para que toda la interfaz sea coherente.
    static let active: Lang = {
        switch preference {
        case .es, .en: return preference
        case .system:
            let first = Locale.preferredLanguages.first ?? "es"
            return first.hasPrefix("es") ? .es : .en
        }
    }()

    var locale: Locale { Locale(identifier: self == .en ? "en" : "es") }
    var label: String {
        switch self {
        case .system: return L("Automático (idioma del sistema)")
        case .es: return "Español"
        case .en: return "English"
        }
    }
}

/// Traduce un texto de la interfaz (la clave es el texto en español).
func L(_ es: String) -> String {
    guard Lang.active == .en else { return es }
    return Strings.en[es] ?? es
}

/// Igual que `L`, pero sustituye `%@` / `%d` por los argumentos (para textos con valores).
func L(_ es: String, _ args: CVarArg...) -> String {
    String(format: L(es), arguments: args)
}

/// Cierra y vuelve a abrir la app (para aplicar el cambio de idioma).
func relaunchApp() {
    let path = Bundle.main.bundlePath
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/bin/sh")
    p.arguments = ["-c", "sleep 0.6; /usr/bin/open -n \"$0\"", path]
    try? p.run()
    NSApp.terminate(nil)
}
