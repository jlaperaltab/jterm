import AppKit

/// Botón de icono compacto con resaltado al pasar el cursor.
final class IconButton: NSButton {
    private var hovering = false { didSet { refresh() } }
    override var isEnabled: Bool { didSet { alphaValue = isEnabled ? 1 : 0.3; if !isEnabled { hovering = false } } }

    init(symbol: String, tip: String, size: CGFloat = 28, target: AnyObject? = nil, action: Selector? = nil) {
        super.init(frame: .zero)
        image = NSImage(systemSymbolName: symbol, accessibilityDescription: tip)
        symbolConfiguration = .init(pointSize: 12, weight: .medium)
        imagePosition = .imageOnly
        isBordered = false
        wantsLayer = true
        layer?.cornerRadius = 7
        toolTip = tip
        self.target = target; self.action = action
        translatesAutoresizingMaskIntoConstraints = false
        widthAnchor.constraint(equalToConstant: size).isActive = true
        heightAnchor.constraint(equalToConstant: size).isActive = true
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
        refresh()
    }
    required init?(coder: NSCoder) { fatalError() }

    private func refresh() {
        layer?.backgroundColor = (hovering ? NSColor.white.withAlphaComponent(0.09) : .clear).cgColor
        contentTintColor = hovering ? .white : .secondaryLabelColor
    }
    override func mouseEntered(with e: NSEvent) { hovering = true }
    override func mouseExited(with e: NSEvent) { hovering = false }
}

/// Selector desplegable con el mismo estilo que los campos de los formularios.
final class Dropdown: NSView {
    var items: [String] = [] { didSet { if selectedIndex >= items.count { selectedIndex = items.isEmpty ? -1 : 0 }; updateTitle() } }
    var separatorAfter: Set<Int> = []
    var selectedIndex = -1 { didSet { updateTitle() } }
    var placeholder = "—" { didSet { updateTitle() } }
    var onChange: (() -> Void)?
    /// Si se define, el clic ejecuta esta acción (p. ej. abrir un selector con búsqueda) en vez del menú.
    var customAction: (() -> Void)?

    var titleOfSelected: String? { items.indices.contains(selectedIndex) ? items[selectedIndex] : nil }
    @discardableResult func select(title: String) -> Bool {
        guard let i = items.firstIndex(of: title) else { return false }
        selectedIndex = i; return true
    }

    private let label = NSTextField(labelWithString: "")
    private var hovering = false { didSet { style() } }
    private var open = false { didSet { style() } }

    init(symbol: String?) {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 8; layer?.borderWidth = 1
        translatesAutoresizingMaskIntoConstraints = false

        let icon = NSImageView(image: NSImage(systemSymbolName: symbol ?? "circle", accessibilityDescription: nil)!)
        icon.isHidden = symbol == nil
        icon.symbolConfiguration = .init(pointSize: 11, weight: .regular); icon.contentTintColor = .tertiaryLabelColor
        let chevron = NSImageView(image: NSImage(systemSymbolName: "chevron.up.chevron.down", accessibilityDescription: nil)!)
        chevron.symbolConfiguration = .init(pointSize: 9, weight: .semibold); chevron.contentTintColor = .tertiaryLabelColor
        label.font = .systemFont(ofSize: 13); label.textColor = .white; label.lineBreakMode = .byTruncatingTail
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        for v in [icon, label, chevron] { v.translatesAutoresizingMaskIntoConstraints = false; addSubview(v) }
        NSLayoutConstraint.activate([
            heightAnchor.constraint(equalToConstant: 30),
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: symbol == nil ? 0 : 14),
            label.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: symbol == nil ? 0 : 8),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
            label.trailingAnchor.constraint(lessThanOrEqualTo: chevron.leadingAnchor, constant: -6),
            chevron.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            chevron.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
        style(); updateTitle()
    }
    required init?(coder: NSCoder) { fatalError() }

    private func updateTitle() {
        label.stringValue = titleOfSelected ?? placeholder
        label.textColor = titleOfSelected == nil ? NSColor.white.withAlphaComponent(0.35) : .white
    }
    private func style() {
        layer?.backgroundColor = NSColor.white.withAlphaComponent(hovering || open ? 0.08 : 0.05).cgColor
        layer?.borderColor = (open ? Theme.accent : NSColor.white.withAlphaComponent(hovering ? 0.18 : 0.10)).cgColor
    }
    override func mouseEntered(with e: NSEvent) { hovering = true }
    override func mouseExited(with e: NSEvent) { hovering = false }
    override func mouseDown(with e: NSEvent) {
        if let a = customAction { a(); return }
        guard !items.isEmpty else { return }
        let menu = NSMenu()
        menu.minimumWidth = bounds.width
        for (i, t) in items.enumerated() {
            let it = NSMenuItem(title: t, action: #selector(picked(_:)), keyEquivalent: "")
            it.target = self; it.tag = i; it.state = i == selectedIndex ? .on : .off
            menu.addItem(it)
            if separatorAfter.contains(i) { menu.addItem(.separator()) }
        }
        open = true
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: -4), in: self)
        open = false
    }
    @objc private func picked(_ s: NSMenuItem) {
        let changed = s.tag != selectedIndex
        selectedIndex = s.tag
        if changed { onChange?() }
    }
}

func smallCaption(_ text: String) -> NSTextField {
    let l = NSTextField(labelWithString: text.uppercased())
    l.font = .systemFont(ofSize: 10, weight: .semibold); l.textColor = .tertiaryLabelColor
    return l
}

/// Asa vertical para redimensionar un panel arrastrando su borde.
final class ResizeHandle: NSView {
    var onDrag: ((CGFloat) -> Void)?   // ancho deseado (x en coordenadas de la ventana)
    var onReset: (() -> Void)?
    private var hovering = false { didSet { needsDisplay = true } }
    private var dragging = false { didSet { needsDisplay = true } }

    override init(frame: NSRect) {
        super.init(frame: frame)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
    }
    required init?(coder: NSCoder) { fatalError() }

    override func resetCursorRects() { addCursorRect(bounds, cursor: .resizeLeftRight) }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseEntered(with e: NSEvent) { hovering = true }
    override func mouseExited(with e: NSEvent) { hovering = false }
    override func mouseDown(with e: NSEvent) {
        if e.clickCount == 2 { onReset?() } else { dragging = true }
    }
    override func mouseDragged(with e: NSEvent) { onDrag?(e.locationInWindow.x) }
    override func mouseUp(with e: NSEvent) { dragging = false }

    override func draw(_ dirty: NSRect) {
        let on = hovering || dragging
        (on ? Theme.accent.withAlphaComponent(dragging ? 0.9 : 0.6) : NSColor.white.withAlphaComponent(0.07)).setFill()
        NSRect(x: bounds.midX - 0.5, y: 0, width: on ? 2 : 1, height: bounds.height).fill()
    }
}


/// Etiqueta verde "Conectado" con punto.
final class ConnectedBadge: NSView {
    init() {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.backgroundColor = NSColor.systemGreen.withAlphaComponent(0.16).cgColor
        let dot = NSView(); dot.wantsLayer = true; dot.layer?.cornerRadius = 3
        dot.layer?.backgroundColor = NSColor.systemGreen.cgColor
        dot.translatesAutoresizingMaskIntoConstraints = false
        dot.widthAnchor.constraint(equalToConstant: 6).isActive = true
        dot.heightAnchor.constraint(equalToConstant: 6).isActive = true
        let l = NSTextField(labelWithString: "Conectado")
        l.font = .systemFont(ofSize: 9, weight: .semibold); l.textColor = .systemGreen
        let s = NSStackView(views: [dot, l]); s.spacing = 4
        s.edgeInsets = NSEdgeInsets(top: 2, left: 6, bottom: 2, right: 7)
        s.translatesAutoresizingMaskIntoConstraints = false
        addSubview(s)
        NSLayoutConstraint.activate([
            s.leadingAnchor.constraint(equalTo: leadingAnchor), s.trailingAnchor.constraint(equalTo: trailingAnchor),
            s.topAnchor.constraint(equalTo: topAnchor), s.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        setContentHuggingPriority(.required, for: .horizontal)
        setContentCompressionResistancePriority(.required, for: .horizontal)
    }
    required init?(coder: NSCoder) { fatalError() }
}
