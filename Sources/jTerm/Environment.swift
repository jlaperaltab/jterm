import AppKit

/// Entorno de un servidor o cuenta AWS. Sirve para colorear las pestañas y para los avisos de seguridad.
enum ServerEnv: String, Codable, CaseIterable {
    case prod, staging, dev

    var title: String {
        switch self { case .prod: return "Producción"; case .staging: return "Staging"; case .dev: return "Desarrollo" }
    }
    /// Texto corto para etiquetas.
    var tag: String {
        switch self { case .prod: return "PROD"; case .staging: return "STAGING"; case .dev: return "DEV" }
    }
    var color: NSColor {
        switch self {
        case .prod: return NSColor(srgbRed: 1.0, green: 0.27, blue: 0.23, alpha: 1)
        case .staging: return NSColor(srgbRed: 1.0, green: 0.80, blue: 0.0, alpha: 1)
        case .dev: return NSColor(srgbRed: 0.19, green: 0.82, blue: 0.35, alpha: 1)
        }
    }
}

/// Etiqueta pequeña con el color del entorno (PROD, STAGING, DEV).
final class EnvTag: NSView {
    private let label = NSTextField(labelWithString: "")
    init(_ env: ServerEnv) {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 4
        layer?.backgroundColor = env.color.withAlphaComponent(0.20).cgColor
        label.stringValue = env.tag
        label.font = .systemFont(ofSize: 8.5, weight: .bold)
        label.textColor = env.color
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 5),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -5),
            label.topAnchor.constraint(equalTo: topAnchor, constant: 1.5),
            label.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -1.5),
        ])
        setContentHuggingPriority(.required, for: .horizontal)
        setContentCompressionResistancePriority(.required, for: .horizontal)
    }
    required init?(coder: NSCoder) { fatalError() }
}

/// Selector de entorno para los editores: Ninguno / Producción / Staging / Desarrollo.
final class EnvPicker: NSStackView {
    let segmented = NSSegmentedControl(labels: ["Ninguno"] + ServerEnv.allCases.map(\.title), trackingMode: .selectOne,
                                       target: nil, action: nil)
    init(selected: ServerEnv?) {
        super.init(frame: .zero)
        let caption = NSTextField(labelWithString: "ENTORNO")
        caption.font = .systemFont(ofSize: 10, weight: .semibold); caption.textColor = .tertiaryLabelColor
        segmented.segmentStyle = .rounded
        segmented.selectedSegment = selected.flatMap { ServerEnv.allCases.firstIndex(of: $0) }.map { $0 + 1 } ?? 0
        orientation = .vertical; alignment = .leading; spacing = 5
        addArrangedSubview(caption); addArrangedSubview(segmented)
        segmented.widthAnchor.constraint(equalTo: widthAnchor).isActive = true
    }
    required init(coder: NSCoder) { fatalError() }
    var value: ServerEnv? {
        let i = segmented.selectedSegment
        return i >= 1 && i <= ServerEnv.allCases.count ? ServerEnv.allCases[i - 1] : nil
    }
}

/// Detecta comandos que conviene confirmar antes de ejecutarlos en producción.
enum CommandGuard {
    private static let patterns: [(String, String)] = [
        (#"(?:^|[\s;&|(`])(?:sudo\s+)?rm\s+\S"#, "rm (borra archivos)"),
        (#"\bdrop\s+(?:table|database|schema|index|view|user|role|column|materialized)\b"#, "DROP (borra objetos de la base de datos)"),
        (#"\btruncate\s+table\b"#, "TRUNCATE TABLE (vacía una tabla)"),
        (#"\bdelete\s+from\s+\w+(?![^;]*\bwhere\b)"#, "DELETE sin WHERE (borra todas las filas)"),
        (#"\bmkfs(?:\.\w+)?\b"#, "mkfs (formatea un disco)"),
        (#"\bdd\s+[^;|]*\bof="#, "dd (escribe sobre un disco o archivo)"),
        (#"(?:^|[\s;&|(`])(?:sudo\s+)?(?:shutdown|reboot|halt|poweroff)\b"#, "apagado o reinicio del servidor"),
        (#"\bkubectl\s+delete\b"#, "kubectl delete"),
        (#"\bdocker\s+(?:rm|rmi|system\s+prune|volume\s+(?:rm|prune)|container\s+prune)\b"#, "docker (borra contenedores o imágenes)"),
    ]
    private static let compiled: [(NSRegularExpression, String)] = patterns.compactMap { p, label in
        (try? NSRegularExpression(pattern: p, options: [.caseInsensitive])).map { ($0, label) }
    }

    /// Descripción del primer patrón delicado que aparece en el texto, o nil si no hay ninguno.
    static func risk(in raw: String) -> String? {
        // "git rm" solo quita archivos del repositorio: no es el rm del sistema.
        let text = raw.replacingOccurrences(of: #"\bgit\s+rm\b"#, with: "git-rm", options: [.regularExpression, .caseInsensitive])
        let range = NSRange(text.startIndex..., in: text)
        for (re, label) in compiled where re.firstMatch(in: text, options: [], range: range) != nil { return label }
        return nil
    }
}
