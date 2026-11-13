import AppKit

extension Notification.Name {
    /// Servidores o cuentas AWS cambiaron fuera de las listas (p. ej. al importar).
    static let jtermDataChanged = Notification.Name("jterm.dataChanged")
}

final class FlippedView: NSView { override var isFlipped: Bool { true } }

final class Callback: NSObject {
    let fn: () -> Void
    init(_ f: @escaping () -> Void) { fn = f }
    @objc func call() { fn() }
}

// MARK: - Controles

/// Campo de texto compacto con el estilo de los formularios.
final class CompactField: NSView, NSTextFieldDelegate {
    let field = FocusField()
    var onCommit: ((String) -> Void)?
    /// Tab / Mayús+Tab explícitos (si no se asignan, se usa el orden normal de AppKit).
    var onTab: (() -> Void)?, onBacktab: (() -> Void)?
    private let box = NSView()

    init(value: String, placeholder: String, width: CGFloat, mono: Bool = false) {
        super.init(frame: .zero)
        box.wantsLayer = true; box.layer?.cornerRadius = 7; box.layer?.borderWidth = 1
        setFocused(false)
        field.stringValue = value
        field.isBordered = false; field.drawsBackground = false; field.focusRingType = .none
        field.font = mono ? .monospacedSystemFont(ofSize: 12, weight: .regular) : .systemFont(ofSize: 13)
        field.textColor = .white
        field.placeholderAttributedString = NSAttributedString(string: placeholder, attributes: [
            .foregroundColor: NSColor.white.withAlphaComponent(0.28), .font: field.font!])
        field.lineBreakMode = .byTruncatingHead
        field.target = self; field.action = #selector(commit); field.delegate = self
        field.onFocus = { [weak self] on in self?.setFocused(on); if !on { self?.commit() } }
        for v in [box, field] { v.translatesAutoresizingMaskIntoConstraints = false }
        addSubview(box); box.addSubview(field)
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: width), heightAnchor.constraint(equalToConstant: 28),
            box.leadingAnchor.constraint(equalTo: leadingAnchor), box.trailingAnchor.constraint(equalTo: trailingAnchor),
            box.topAnchor.constraint(equalTo: topAnchor), box.bottomAnchor.constraint(equalTo: bottomAnchor),
            field.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 8),
            field.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
            field.centerYAnchor.constraint(equalTo: box.centerYAnchor),
        ])
    }
    required init?(coder: NSCoder) { fatalError() }
    func setValue(_ s: String) { field.stringValue = s }
    func control(_ c: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
        if sel == #selector(NSResponder.insertTab(_:)), let f = onTab { f(); return true }
        if sel == #selector(NSResponder.insertBacktab(_:)), let f = onBacktab { f(); return true }
        return false
    }
    private func setFocused(_ on: Bool) {
        box.layer?.backgroundColor = NSColor.white.withAlphaComponent(on ? 0.07 : 0.05).cgColor
        box.layer?.borderColor = (on ? Theme.accent : NSColor.white.withAlphaComponent(0.10)).cgColor
    }
    @objc private func commit() { onCommit?(field.stringValue.trimmingCharacters(in: .whitespaces)) }
}

/// Campo de varias líneas (opciones avanzadas).
/// Vista de texto que avisa cuando gana o pierde el foco.
final class FocusTextView: NSTextView {
    var onFocus: ((Bool) -> Void)?
    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok {
            onFocus?(true)
            if selectedRange().location == NSNotFound { setSelectedRange(NSRange(location: 0, length: 0)) }
            // El cursor no se dibuja solo con el texto vacío dentro de un popover: se fuerza.
            DispatchQueue.main.async { [weak self] in
                self?.updateInsertionPointStateAndRestartTimer(true); self?.needsDisplay = true
            }
        }
        return ok
    }
    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder(); if ok { onFocus?(false) }; return ok
    }
    override func drawInsertionPoint(in rect: NSRect, color: NSColor, turnedOn flag: Bool) {
        var r = rect; r.size.width = max(r.size.width, 2)
        super.drawInsertionPoint(in: r, color: Theme.accent, turnedOn: flag)
    }
}

final class NotesField: NSView, NSTextViewDelegate {
    private let text = FocusTextView()
    private let box = NSView()
    var onCommit: ((String) -> Void)?

    init(value: String, placeholder: String, width: CGFloat, height: CGFloat) {
        super.init(frame: .zero)
        box.wantsLayer = true; box.layer?.cornerRadius = 7; box.layer?.borderWidth = 1
        box.layer?.backgroundColor = NSColor.white.withAlphaComponent(0.05).cgColor
        box.layer?.borderColor = NSColor.white.withAlphaComponent(0.10).cgColor
        text.string = value
        text.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        text.textColor = .white; text.insertionPointColor = .white
        text.drawsBackground = false; text.isRichText = false
        text.isAutomaticQuoteSubstitutionEnabled = false; text.isAutomaticDashSubstitutionEnabled = false
        text.isAutomaticTextReplacementEnabled = false; text.isAutomaticSpellingCorrectionEnabled = false
        text.textContainerInset = NSSize(width: 4, height: 6)
        text.delegate = self
        text.insertionPointColor = Theme.accent
        text.onFocus = { [weak self] on in
            guard let b = self?.box else { return }
            b.layer?.backgroundColor = NSColor.white.withAlphaComponent(on ? 0.07 : 0.05).cgColor
            b.layer?.borderColor = (on ? Theme.accent : NSColor.white.withAlphaComponent(0.10)).cgColor
        }
        text.frame = NSRect(x: 0, y: 0, width: width - 4, height: height - 4)   // sin esto el cursor no se dibuja con el texto vacío
        let scroll = NSScrollView(); scroll.documentView = text
        scroll.drawsBackground = false; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        text.minSize = NSSize(width: 0, height: height); text.isVerticallyResizable = true
        text.autoresizingMask = [.width]
        text.textContainer?.widthTracksTextView = true
        for v in [box, scroll] { v.translatesAutoresizingMaskIntoConstraints = false }
        addSubview(box); addSubview(scroll)
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: width), heightAnchor.constraint(equalToConstant: height),
            box.leadingAnchor.constraint(equalTo: leadingAnchor), box.trailingAnchor.constraint(equalTo: trailingAnchor),
            box.topAnchor.constraint(equalTo: topAnchor), box.bottomAnchor.constraint(equalTo: bottomAnchor),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2), scroll.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -2),
            scroll.topAnchor.constraint(equalTo: topAnchor, constant: 2), scroll.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -2),
        ])
    }
    required init?(coder: NSCoder) { fatalError() }
    var value: String { text.string }
    /// La vista de texto, para encadenarla en el orden de Tab.
    var textView: NSView { text }
    var onTab: (() -> Void)?, onBacktab: (() -> Void)?
    /// Tab y Mayús+Tab cambian de campo; ⌥Tab inserta un tabulador.
    func textView(_ tv: NSTextView, doCommandBy sel: Selector) -> Bool {
        switch sel {
        case #selector(NSResponder.insertTab(_:)):
            if let f = onTab { f() } else { tv.window?.selectNextKeyView(nil) }; return true
        case #selector(NSResponder.insertBacktab(_:)):
            if let f = onBacktab { f() } else { tv.window?.selectPreviousKeyView(nil) }; return true
        default: return false
        }
    }
    func textDidEndEditing(_ n: Notification) { onCommit?(text.string) }
}

/// Valor numérico con botones − / +.
final class StepperControl: NSView {
    private let label = NSTextField(labelWithString: "")
    private let stepper = NSStepper()
    private let format: (Int) -> String
    var onChange: ((Int) -> Void)?

    init(value: Int, min: Int, max: Int, step: Int, format: @escaping (Int) -> String) {
        self.format = format
        super.init(frame: .zero)
        stepper.minValue = Double(min); stepper.maxValue = Double(max); stepper.increment = Double(step)
        stepper.integerValue = value; stepper.valueWraps = false
        stepper.target = self; stepper.action = #selector(changed)
        label.stringValue = format(value); label.font = .monospacedDigitSystemFont(ofSize: 13, weight: .regular)
        label.alignment = .right; label.textColor = .white
        let s = NSStackView(views: [label, stepper]); s.spacing = 8
        s.translatesAutoresizingMaskIntoConstraints = false
        addSubview(s)
        NSLayoutConstraint.activate([
            s.leadingAnchor.constraint(equalTo: leadingAnchor), s.trailingAnchor.constraint(equalTo: trailingAnchor),
            s.topAnchor.constraint(equalTo: topAnchor), s.bottomAnchor.constraint(equalTo: bottomAnchor),
            label.widthAnchor.constraint(greaterThanOrEqualToConstant: 70),
        ])
    }
    required init?(coder: NSCoder) { fatalError() }
    @objc private func changed() { label.stringValue = format(stepper.integerValue); onChange?(stepper.integerValue) }
}

/// Vista previa del tema, la fuente y la paleta actuales.
final class ThemePreview: NSView {
    private let text = NSTextField(wrappingLabelWithString: "")

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true; layer?.cornerRadius = 8; layer?.borderWidth = 1
        layer?.borderColor = NSColor.white.withAlphaComponent(0.12).cgColor
        text.translatesAutoresizingMaskIntoConstraints = false
        text.maximumNumberOfLines = 0; text.isSelectable = false
        addSubview(text)
        NSLayoutConstraint.activate([
            text.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 18), text.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -18),
            text.topAnchor.constraint(equalTo: topAnchor, constant: 16), text.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -16),
        ])
        NotificationCenter.default.addObserver(self, selector: #selector(refresh), name: Settings.didChange, object: nil)
        refresh()
    }
    required init?(coder: NSCoder) { fatalError() }
    deinit { NotificationCenter.default.removeObserver(self) }

    @objc private func refresh() {
        let s = Settings.shared, th = s.theme
        layer?.backgroundColor = th.bgColor.cgColor
        let base = s.font
        let font = NSFont(descriptor: base.fontDescriptor, size: min(base.pointSize, 18)) ?? base
        let bold = NSFontManager.shared.convert(font, toHaveTrait: .boldFontMask)
        func c(_ i: Int) -> NSColor { NSColor(hex: th.ansi[i]) }
        let out = NSMutableAttributedString()
        func add(_ s: String, _ color: NSColor, bold b: Bool = false) {
            out.append(NSAttributedString(string: s, attributes: [.font: b ? bold : font, .foregroundColor: color]))
        }
        add("jose@servidor", c(10), bold: true); add(":", th.fgColor); add("~/proyecto", c(12), bold: true); add("$ ", th.fgColor)
        add("ls -la\n", th.fgColor)
        add("drwxr-xr-x  5 jose  staff   160  ", th.fgColor); add("src\n", c(12), bold: true)
        add("-rwxr-xr-x  1 jose  staff   814  ", th.fgColor); add("deploy.sh\n", c(10))
        add("-rw-r--r--  1 jose  staff  2.1K  ", th.fgColor); add("notas.txt\n", th.fgColor)
        add("aviso: ", c(11), bold: true); add("certificado por vencer   ", th.fgColor)
        add("error: ", c(9), bold: true); add("conexión rechazada\n", th.fgColor)
        for i in 0..<16 { add("██", c(i)); if i == 7 { add(" ", th.fgColor) } }
        text.attributedStringValue = out
    }
}

// MARK: - Ventana de ajustes

final class SettingsTabButton: NSView {
    private let label: NSTextField
    private let icon: NSImageView
    var onSelect: (() -> Void)?
    var selected = false { didSet { refresh() } }
    private var hovering = false { didSet { refresh() } }

    init(title: String, symbol: String) {
        label = NSTextField(labelWithString: title)
        icon = NSImageView(image: NSImage(systemSymbolName: symbol, accessibilityDescription: nil)!)
        super.init(frame: .zero)
        NotificationCenter.default.addObserver(self, selector: #selector(accentChanged), name: Settings.accentDidChange, object: nil)
        wantsLayer = true; layer?.cornerRadius = 8
        label.font = .systemFont(ofSize: 13, weight: .medium)
        icon.symbolConfiguration = .init(pointSize: 13, weight: .medium)
        let s = NSStackView(views: [icon, label]); s.spacing = 9
        s.edgeInsets = NSEdgeInsets(top: 7, left: 10, bottom: 7, right: 10)
        s.translatesAutoresizingMaskIntoConstraints = false
        addSubview(s)
        NSLayoutConstraint.activate([
            s.leadingAnchor.constraint(equalTo: leadingAnchor), s.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
            s.topAnchor.constraint(equalTo: topAnchor), s.bottomAnchor.constraint(equalTo: bottomAnchor),
            icon.widthAnchor.constraint(equalToConstant: 18),
        ])
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
        refresh()
    }
    required init?(coder: NSCoder) { fatalError() }
    @objc private func accentChanged() { refresh() }
    private func refresh() {
        layer?.backgroundColor = (selected ? Theme.accent.withAlphaComponent(0.20) : (hovering ? Theme.hover : .clear)).cgColor
        label.textColor = selected ? .white : .secondaryLabelColor
        icon.contentTintColor = selected ? Theme.accent : .secondaryLabelColor
    }
    override func mouseEntered(with e: NSEvent) { hovering = true }
    override func mouseExited(with e: NSEvent) { hovering = false }
    override func mouseDown(with e: NSEvent) { onSelect?() }
}

final class SettingsWindow: NSObject {
    static let shared = SettingsWindow()
    private var window: NSWindow?
    private let scroll = NSScrollView()
    private let doc = FlippedView()
    private let pageStack = NSStackView()
    private var tabButtons: [SettingsTabButton] = []
    private var keep: [AnyObject] = []
    private var current = 0
    private let s = Settings.shared

    private let pages: [(title: String, symbol: String)] = [
        ("General", "gearshape"), ("Terminal", "terminal"), ("SSH", "network"),
        ("Archivos", "arrow.left.arrow.right.square"), ("AWS", "cloud"), ("iCloud", "icloud"),
        ("Atajos", "keyboard"), ("Datos", "externaldrive"),
    ]

    func show(page: Int? = nil) {
        if window == nil { build() }
        select(page ?? current)
        window?.makeKeyAndOrderFront(nil)
        window?.makeFirstResponder(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    // MARK: Estructura

    private func build() {
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 840, height: 640),
                         styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                         backing: .buffered, defer: false)
        w.title = "Ajustes"
        w.titleVisibility = .hidden   // si no, "Ajustes" se solapa con el título de cada página
        w.appearance = NSAppearance(named: .darkAqua)
        w.titlebarAppearsTransparent = true
        w.backgroundColor = Theme.bg
        w.minSize = NSSize(width: 780, height: 460)
        w.isReleasedWhenClosed = false
        w.center()
        w.setFrameAutosaveName("jTermSettings")

        let root = NSView()
        w.contentView = root

        let side = NSView(); side.wantsLayer = true; side.layer?.backgroundColor = Theme.bar.cgColor
        let list = NSStackView(); list.orientation = .vertical; list.alignment = .leading; list.spacing = 2
        for (i, p) in pages.enumerated() {
            let b = SettingsTabButton(title: p.title, symbol: p.symbol)
            b.onSelect = { [weak self] in self?.select(i) }
            b.translatesAutoresizingMaskIntoConstraints = false
            tabButtons.append(b); list.addArrangedSubview(b)
            b.widthAnchor.constraint(equalTo: list.widthAnchor).isActive = true
        }
        for v in [side, scroll, list] { v.translatesAutoresizingMaskIntoConstraints = false }
        root.addSubview(side); root.addSubview(scroll); side.addSubview(list)
        NSLayoutConstraint.activate([
            side.leadingAnchor.constraint(equalTo: root.leadingAnchor), side.topAnchor.constraint(equalTo: root.topAnchor),
            side.bottomAnchor.constraint(equalTo: root.bottomAnchor), side.widthAnchor.constraint(equalToConstant: 190),
            list.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor, constant: 26),
            list.leadingAnchor.constraint(equalTo: side.leadingAnchor, constant: 10),
            list.trailingAnchor.constraint(equalTo: side.trailingAnchor, constant: -10),
            scroll.leadingAnchor.constraint(equalTo: side.trailingAnchor), scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor), scroll.bottomAnchor.constraint(equalTo: root.bottomAnchor),
        ])

        scroll.drawsBackground = false; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        doc.translatesAutoresizingMaskIntoConstraints = false
        scroll.documentView = doc
        pageStack.orientation = .vertical; pageStack.alignment = .leading; pageStack.spacing = 22
        pageStack.translatesAutoresizingMaskIntoConstraints = false
        doc.addSubview(pageStack)
        let clip = scroll.contentView
        NSLayoutConstraint.activate([
            doc.leadingAnchor.constraint(equalTo: clip.leadingAnchor), doc.trailingAnchor.constraint(equalTo: clip.trailingAnchor),
            doc.topAnchor.constraint(equalTo: clip.topAnchor),
            pageStack.leadingAnchor.constraint(equalTo: doc.leadingAnchor, constant: 30),
            pageStack.trailingAnchor.constraint(equalTo: doc.trailingAnchor, constant: -30),
            pageStack.topAnchor.constraint(equalTo: doc.topAnchor, constant: 26),
            pageStack.bottomAnchor.constraint(equalTo: doc.bottomAnchor, constant: -30),
        ])
        window = w
        NotificationCenter.default.addObserver(forName: .syncStatusChanged, object: nil, queue: .main) { [weak self] _ in
            if let self, self.window?.isVisible == true, self.current == 5 { self.select(5) }
        }
    }

    private func select(_ i: Int) {
        current = i
        for (n, b) in tabButtons.enumerated() { b.selected = n == i }
        keep.removeAll()
        pageStack.arrangedSubviews.forEach { pageStack.removeArrangedSubview($0); $0.removeFromSuperview() }
        let title = NSTextField(labelWithString: pages[i].title)
        title.font = .systemFont(ofSize: 24, weight: .bold); title.textColor = .white
        pageStack.addArrangedSubview(title)
        pageStack.setCustomSpacing(16, after: title)
        let blocks: [NSView]
        switch i {
        case 0: blocks = generalPage()
        case 1: blocks = terminalPage()
        case 2: blocks = sshPage()
        case 3: blocks = filesPage()
        case 4: blocks = awsPage()
        case 5: blocks = syncPage()
        case 6: blocks = shortcutsPage()
        default: blocks = dataPage()
        }
        for b in blocks { pageStack.addArrangedSubview(b); b.widthAnchor.constraint(equalTo: pageStack.widthAnchor).isActive = true }
        doc.scroll(.zero)
    }

    // MARK: Constructores

    private func section(_ title: String?, _ rows: [NSView], footer: String? = nil) -> NSView {
        let card = NSStackView(); card.orientation = .vertical; card.spacing = 0; card.alignment = .leading
        card.wantsLayer = true; card.layer?.cornerRadius = 10; card.layer?.borderWidth = 1
        card.layer?.backgroundColor = NSColor.white.withAlphaComponent(0.04).cgColor
        card.layer?.borderColor = NSColor.white.withAlphaComponent(0.08).cgColor
        for (i, r) in rows.enumerated() {
            card.addArrangedSubview(r); r.widthAnchor.constraint(equalTo: card.widthAnchor).isActive = true
            if i < rows.count - 1 {
                let line = NSView(); line.wantsLayer = true; line.layer?.backgroundColor = NSColor.white.withAlphaComponent(0.06).cgColor
                line.translatesAutoresizingMaskIntoConstraints = false
                card.addArrangedSubview(line)
                line.heightAnchor.constraint(equalToConstant: 1).isActive = true
                line.widthAnchor.constraint(equalTo: card.widthAnchor).isActive = true
            }
        }
        var views: [NSView] = []
        if let title { views.append(smallCaption(title)) }
        views.append(card)
        if let footer {
            let f = NSTextField(wrappingLabelWithString: footer)
            f.font = .systemFont(ofSize: 11); f.textColor = .tertiaryLabelColor; f.maximumNumberOfLines = 0
            views.append(f)
        }
        let v = NSStackView(views: views); v.orientation = .vertical; v.alignment = .leading; v.spacing = 7
        card.widthAnchor.constraint(equalTo: v.widthAnchor).isActive = true
        return v
    }

    /// Un NSTextField con varias líneas calcula mal su altura dentro de un NSStackView (la fila queda más baja que el texto
    /// y los márgenes "desaparecen"). Se mide el texto con el ancho real y se fija esa altura.
    private func fixHeight(_ label: NSTextField, width: CGFloat) {
        let r = (label.stringValue as NSString).boundingRect(
            with: NSSize(width: width, height: .greatestFiniteMagnitude),
            options: [.usesLineFragmentOrigin, .usesFontLeading], attributes: [.font: label.font ?? .systemFont(ofSize: 11)])
        label.heightAnchor.constraint(equalToConstant: ceil(r.height) + 2).isActive = true
    }

    private func row(_ title: String, _ detail: String? = nil, _ control: NSView) -> NSView {
        let t = NSTextField(labelWithString: title)
        t.font = .systemFont(ofSize: 13); t.textColor = .white
        var lefts: [NSView] = [t]
        if let detail {
            let d = NSTextField(wrappingLabelWithString: detail)
            d.font = .systemFont(ofSize: 11); d.textColor = .secondaryLabelColor
            d.maximumNumberOfLines = 0; d.preferredMaxLayoutWidth = 270
            d.translatesAutoresizingMaskIntoConstraints = false
            d.widthAnchor.constraint(equalToConstant: 270).isActive = true
            fixHeight(d, width: 270)
            lefts.append(d)
        }
        let left = NSStackView(views: lefts); left.orientation = .vertical; left.alignment = .leading; left.spacing = 4
        left.setContentHuggingPriority(.defaultLow, for: .horizontal)
        control.setContentHuggingPriority(.required, for: .horizontal)
        control.setContentCompressionResistancePriority(.required, for: .horizontal)
        let r = NSStackView(views: [left, control]); r.spacing = 16; r.alignment = .centerY
        r.distribution = .fill
        // con descripción de varias líneas hace falta más aire abajo, o el texto se ve pegado a la línea divisoria
        r.edgeInsets = NSEdgeInsets(top: 16, left: 20, bottom: detail == nil ? 16 : 22, right: 20)
        return r
    }

    /// Fila con el control debajo del texto (para campos anchos).
    private func rowStacked(_ title: String, _ detail: String, _ control: NSView) -> NSView {
        let t = NSTextField(labelWithString: title); t.font = .systemFont(ofSize: 13); t.textColor = .white
        let d = NSTextField(wrappingLabelWithString: detail)
        d.font = .systemFont(ofSize: 11); d.textColor = .secondaryLabelColor; d.maximumNumberOfLines = 0
        d.preferredMaxLayoutWidth = 500
        d.translatesAutoresizingMaskIntoConstraints = false
        d.widthAnchor.constraint(equalToConstant: 500).isActive = true
        fixHeight(d, width: 500)
        let v = NSStackView(views: [t, d, control]); v.orientation = .vertical; v.alignment = .leading; v.spacing = 4
        v.setCustomSpacing(10, after: d)
        v.edgeInsets = NSEdgeInsets(top: 16, left: 20, bottom: 22, right: 20)
        return v
    }

    private func toggle(_ on: Bool, _ set: @escaping (Bool) -> Void) -> NSView {
        let sw = NSSwitch(); sw.state = on ? .on : .off
        let cb = Callback { [unowned sw] in set(sw.state == .on) }
        sw.target = cb; sw.action = #selector(Callback.call); keep.append(cb)
        return sw
    }

    private func choice(_ items: [String], _ selected: Int, width: CGFloat = 200, _ pick: @escaping (Int) -> Void) -> NSView {
        let d = Dropdown(symbol: nil)
        d.items = items; d.selectedIndex = max(0, min(selected, items.count - 1))
        d.onChange = { [unowned d] in pick(d.selectedIndex) }
        d.widthAnchor.constraint(equalToConstant: width).isActive = true
        keep.append(d)
        return d
    }

    private func stepper(_ value: Int, _ lo: Int, _ hi: Int, step: Int = 1, format: @escaping (Int) -> String = { "\($0)" },
                         _ set: @escaping (Int) -> Void) -> NSView {
        let c = StepperControl(value: value, min: lo, max: hi, step: step, format: format)
        c.onChange = set
        return c
    }

    private func action(_ title: String, primary: Bool = false, _ fn: @escaping () -> Void) -> PillButton {
        let cb = Callback(fn); keep.append(cb)
        let b = PillButton(title, primary: primary, target: cb, action: #selector(Callback.call))
        return b
    }

    private func note(_ text: String) -> NSView {
        let l = NSTextField(wrappingLabelWithString: text)
        l.font = .systemFont(ofSize: 12); l.textColor = .secondaryLabelColor; l.maximumNumberOfLines = 0
        return l
    }

    private func pathPicker(_ value: String, placeholder: String, _ set: @escaping (String) -> Void) -> NSView {
        let f = CompactField(value: value, placeholder: placeholder, width: 190, mono: true)
        f.onCommit = set
        let pick = action("Elegir…") { [weak f, weak self] in
            let p = NSOpenPanel(); p.canChooseDirectories = true; p.canChooseFiles = false; p.canCreateDirectories = true
            if p.runModal() == .OK, let u = p.url { f?.setValue(u.path); set(u.path); _ = self }
        }
        let s = NSStackView(views: [f, pick]); s.spacing = 8
        return s
    }

    // MARK: Páginas

    private func generalPage() -> [NSView] {
        let shells = Settings.installedShells()
        let shellIdx = shells.firstIndex(of: s.shell).map { $0 + 1 } ?? 0
        return [
            section("Terminal local", [
                row("Shell", "Programa que se ejecuta en las pestañas locales nuevas.",
                    choice(["Predeterminado del sistema"] + shells, shellIdx, width: 230) { [s] i in s.shell = i == 0 ? "" : shells[i - 1] }),
                row("Shell de inicio de sesión", "Ejecuta el shell con -l para cargar tu perfil (.zprofile, PATH, etc.).",
                    toggle(s.loginShell) { [s] in s.loginShell = $0 }),
                row("Carpeta inicial", "Directorio donde abren las pestañas locales nuevas. Vacío = carpeta personal.",
                    pathPicker(s.startDirectory, placeholder: "Carpeta personal") { [s] in s.startDirectory = $0 }),
            ], footer: "Los cambios se aplican a las pestañas nuevas."),
            section("Ventana", [
                row("Mostrar la barra lateral al iniciar", nil, toggle(s.sidebarAtLaunch) { [s] in s.sidebarAtLaunch = $0 }),
                row("Mostrar el botón de comandos guardados", "Botón flotante en la esquina inferior izquierda del terminal. Si lo ocultas, sigues pudiendo abrir la lista con ⇧⌘K.",
                    toggle(s.showSnippetsButton) { [s] in s.showSnippetsButton = $0 }),
            ]),
            section("Confirmaciones", [
                row("Al cerrar una pestaña con conexión activa", "Pide confirmación antes de cerrar una sesión SSH o de archivos abierta.",
                    toggle(s.confirmClose) { [s] in s.confirmClose = $0 }),
                row("Al salir con conexiones activas", "Pide confirmación antes de cerrar jTerm si hay conexiones abiertas.",
                    toggle(s.confirmQuit) { [s] in s.confirmQuit = $0 }),
            ]),
        ]
    }

    private func terminalPage() -> [NSView] {
        let themes = TerminalTheme.all.map(\.name)
        let fams = Settings.monospacedFamilies()
        let famIdx = fams.firstIndex(of: s.fontFamily).map { $0 + 1 } ?? 0
        let scrolls = [1000, 5000, 10000, 50000, 100000]
        let scrollIdx = scrolls.enumerated().min { abs($0.element - s.scrollback) < abs($1.element - s.scrollback) }?.offset ?? 1
        let shapes = ["block", "underline", "bar"]
        let terms = ["xterm-256color", "xterm", "screen-256color", "tmux-256color", "vt100"]
        let preview = ThemePreview()
        preview.translatesAutoresizingMaskIntoConstraints = false
        return [
            section("Apariencia", [
                row("Tema de colores", nil, choice(themes, themes.firstIndex(of: s.themeName) ?? 0, width: 190) { [s] in s.themeName = themes[$0] }),
                row("Color de acento", "Botones, selección y resaltados de la app. Algunos detalles cambian al reabrir jTerm.",
                    choice(Settings.accents.map(\.name), Settings.accents.firstIndex { $0.name == s.accentName } ?? 0, width: 150) { [s] in s.accentName = Settings.accents[$0].name }),
                row("Fuente", "Solo se muestran fuentes monoespaciadas.",
                    choice(["Sistema (SF Mono)"] + fams, famIdx, width: 230) { [s] i in s.fontFamily = i == 0 ? "system" : fams[i - 1] }),
                row("Tamaño de fuente", nil, stepper(s.fontSize, 9, 32, format: { "\($0) pt" }) { [s] in s.fontSize = $0 }),
                preview,
            ]),
            section("Cursor", [
                row("Forma", nil, choice(["Bloque", "Subrayado", "Barra"], shapes.firstIndex(of: s.cursorShape) ?? 0, width: 150) { [s] in s.cursorShape = shapes[$0] }),
                row("Parpadeo", nil, toggle(s.cursorBlink) { [s] in s.cursorBlink = $0 }),
            ]),
            section("Comportamiento", [
                row("Líneas de historial", "Más líneas permiten desplazarte más atrás, pero usan más memoria por pestaña.",
                    choice(["1.000", "5.000", "10.000", "50.000", "100.000"], scrollIdx, width: 130) { [s] in s.scrollback = scrolls[$0] }),
                row("Usar Option como Meta", "Permite atajos como Option+← para saltar palabras en el shell.",
                    toggle(s.optionAsMeta) { [s] in s.optionAsMeta = $0 }),
                row("Subrayar las URLs de la salida", "Las URLs se ven subrayadas; ⌘+clic las abre. Con ⌘ pulsado también se subrayan las rutas de archivo.",
                    toggle(s.underlineLinks) { [s] in s.underlineLinks = $0 }),
                row("Permitir que las aplicaciones capturen el mouse", "Necesario para tmux, vim o htop con ratón. Si lo desactivas, el mouse solo selecciona texto.",
                    toggle(s.mouseReporting) { [s] in s.mouseReporting = $0 }),
            ]),
            section("Seguridad", [
                row("Confirmar antes de pegar varias líneas", "Muestra las líneas y pregunta antes de enviarlas al terminal. No se pregunta dentro de vim, nano o less.",
                    toggle(s.confirmMultilinePaste) { [s] in s.confirmMultilinePaste = $0 }),
                row("Avisar de comandos delicados en producción", "En pestañas con entorno Producción pregunta antes de ejecutar rm, DROP, TRUNCATE, DELETE sin WHERE, docker rm, kubectl delete, reinicios y similares.",
                    toggle(s.warnDangerousProd) { [s] in s.warnDangerousProd = $0 }),
            ], footer: "El entorno de cada servidor o cuenta AWS se elige al editarlo."),
            section("Grabaciones", [
                row("Grabar automáticamente las pestañas de producción", "Cada pestaña con entorno Producción empieza a grabarse sola. También puedes grabar cualquier pestaña con ⌥⌘R.",
                    toggle(s.autoRecordProd) { [s] in s.autoRecordProd = $0 }),
                row("Carpeta de grabaciones", "Se guarda el texto de lo que muestra el terminal, un archivo por sesión. Vacío = la carpeta de datos de jTerm.",
                    pathPicker(s.recordingDirectory, placeholder: "Predeterminada") { [s] in s.recordingDirectory = $0 }),
            ], footer: "Las grabaciones pueden contener información sensible; solo tu usuario puede leerlas."),
            section("Compatibilidad", [
                row("Tipo de terminal (TERM)", "Algunos servidores antiguos no conocen xterm-256color. Se aplica a las pestañas nuevas.",
                    choice(terms, terms.firstIndex(of: s.termName) ?? 0, width: 190) { [s] in s.termName = terms[$0] }),
                row("La tecla Borrar envía Ctrl-H", "Actívalo si en algún servidor la tecla Borrar no funciona bien.",
                    toggle(s.backspaceCtrlH) { [s] in s.backspaceCtrlH = $0 }),
            ]),
        ]
    }

    private func sshPage() -> [NSView] {
        let policies = ["accept-new", "yes"]
        let extra = NotesField(value: s.sshExtraOptions, placeholder: "", width: 500, height: 84)
        extra.onCommit = { [s] in s.sshExtraOptions = $0 }
        let user = CompactField(value: s.defaultUser, placeholder: "root", width: 150)
        user.onCommit = { [s] in s.defaultUser = $0 }
        return [
            section("Conexión", [
                row("Mantener la conexión activa", "Envía un latido cada cierto tiempo para que el servidor o el router no cierren sesiones inactivas.",
                    stepper(s.sshKeepAlive, 0, 600, step: 5, format: { $0 == 0 ? "Desactivado" : "\($0) s" }) { [s] in s.sshKeepAlive = $0 }),
                row("Hosts desconocidos", "Qué hacer la primera vez que te conectas a un servidor. Si su clave cambia después, la conexión siempre se rechaza.",
                    choice(["Aceptar y recordar (recomendado)", "Estricto: solo hosts ya conocidos"], policies.firstIndex(of: s.sshHostKey) ?? 0, width: 250) { [s] in s.sshHostKey = policies[$0] }),
                row("Comprimir el tráfico", "Útil en conexiones lentas; en redes rápidas puede ser más lento.",
                    toggle(s.sshCompression) { [s] in s.sshCompression = $0 }),
                row("Reenviar el agente SSH", "Permite que el servidor use tus llaves locales para saltar a otros equipos. Actívalo solo en servidores de confianza.",
                    toggle(s.sshForwardAgent) { [s] in s.sshForwardAgent = $0 }),
            ], footer: "Se aplica a las conexiones nuevas (terminal y transferencia de archivos)."),
            section("Servidores nuevos", [
                row("Usuario por defecto", "Se rellena al agregar un servidor.", user),
                row("Puerto por defecto", nil, stepper(s.defaultPort, 1, 65535) { [s] in s.defaultPort = $0 }),
            ]),
            section("Avanzado", [
                rowStacked("Opciones adicionales de ssh", "Una por línea, con formato Clave=Valor (por ejemplo ServerAliveCountMax=3). Se pasan a ssh con -o. Las líneas que empiezan con # se ignoran.", extra),
            ]),
        ]
    }

    private func filesPage() -> [NSView] {
        let delays = [0, 2, 4, 10, 30]
        return [
            section("Paneles", [
                row("Mostrar archivos ocultos en este Mac", nil, toggle(s.filesLocalHidden) { [s] in s.filesLocalHidden = $0 }),
                row("Mostrar archivos ocultos en el servidor", nil, toggle(s.filesRemoteHidden) { [s] in s.filesRemoteHidden = $0 }),
                row("Carpeta inicial de este Mac", "Vacío = carpeta personal.",
                    pathPicker(s.filesLocalStart, placeholder: "Carpeta personal") { [s] in s.filesLocalStart = $0 }),
            ], footer: "Se aplica a las pestañas de archivos nuevas."),
            section("Operaciones", [
                row("Confirmar antes de eliminar", "En el servidor, eliminar no se puede deshacer; en tu Mac los archivos van a la papelera.",
                    toggle(s.filesConfirmDelete) { [s] in s.filesConfirmDelete = $0 }),
                row("Ocultar transferencias completadas", nil,
                    choice(["No ocultar", "A los 2 s", "A los 4 s", "A los 10 s", "A los 30 s"], delays.firstIndex(of: s.transferDismiss) ?? 2, width: 150) { [s] in s.transferDismiss = delays[$0] }),
            ]),
        ]
    }

    private func awsPage() -> [NSView] {
        let regions = AWSClient.regions
        let user = CompactField(value: s.awsUser, placeholder: "ec2-user", width: 150)
        user.onCommit = { [s] in s.awsUser = $0.isEmpty ? "ec2-user" : $0 }
        // cuadrícula de regiones
        let selected = Set(s.awsRegions)
        let grid = NSStackView(); grid.orientation = .vertical; grid.alignment = .leading; grid.spacing = 6
        var rowViews: [NSView] = []
        for (n, r) in regions.enumerated() {
            let b = NSButton(checkboxWithTitle: r, target: nil, action: nil)
            b.state = selected.contains(r) ? .on : .off
            b.font = .systemFont(ofSize: 12)
            let cb = Callback { [weak self, unowned b] in
                guard let self else { return }
                var cur = Set(self.s.awsRegions)
                if b.state == .on { cur.insert(r) } else { cur.remove(r) }
                self.s.awsRegions = regions.filter { cur.contains($0) }
            }
            keep.append(cb); b.target = cb; b.action = #selector(Callback.call)
            b.widthAnchor.constraint(equalToConstant: 130).isActive = true
            rowViews.append(b)
            if rowViews.count == 3 || n == regions.count - 1 {
                grid.addArrangedSubview(NSStackView(views: rowViews)); rowViews = []
            }
        }
        let known = (UserDefaults.standard.dictionaryRepresentation().keys.filter { $0.hasPrefix("aws.user.") }).count
        let all = action("Todas") { [weak self] in self?.s.awsRegions = regions; self?.select(4) }
        let none = action("Ninguna") { [weak self] in self?.s.awsRegions = []; self?.select(4) }
        let gridBox = NSStackView(views: [grid, NSStackView(views: [all, none])])
        gridBox.orientation = .vertical; gridBox.alignment = .leading; gridBox.spacing = 12
        gridBox.edgeInsets = NSEdgeInsets(top: 16, left: 20, bottom: 16, right: 20)
        (gridBox.arrangedSubviews[1] as? NSStackView)?.spacing = 8

        return [
            section("Cuentas nuevas", [
                row("Región por defecto", "Se usa al agregar una cuenta.",
                    choice(regions, regions.firstIndex(of: s.awsRegion) ?? 0, width: 170) { [s] in s.awsRegion = regions[$0] }),
                row("Usuario SSH por defecto", "Sugerido al conectarte a una instancia: ec2-user (Amazon Linux), ubuntu, admin…", user),
            ]),
            section("Regiones consultadas", [gridBox],
                    footer: "Solo afecta a la opción “Todas las regiones”. Menos regiones hacen el listado más rápido."),
            section("Mantenimiento", [
                row("Usuarios recordados", known == 0 ? "No hay usuarios recordados." : "jTerm recuerda el usuario de \(known) instancia\(known == 1 ? "" : "s"). Olvídalos para que vuelva a preguntar.",
                    action("Olvidar") { [weak self] in
                        for k in UserDefaults.standard.dictionaryRepresentation().keys where k.hasPrefix("aws.user.") { UserDefaults.standard.removeObject(forKey: k) }
                        UserDefaults.standard.removeObject(forKey: "aws.lastUser")
                        self?.select(4)
                    }),
                row("Llave de Instance Connect", "Llave SSH propia de jTerm que se envía a AWS al conectar. Regenerarla crea una nueva en la próxima conexión.",
                    action("Regenerar") { [weak self] in
                        let path = ServerStore.dir.appendingPathComponent("eic_ed25519").path
                        try? FileManager.default.removeItem(atPath: path); try? FileManager.default.removeItem(atPath: path + ".pub")
                        showAlert("Llave eliminada", "Se generará una nueva la próxima vez que te conectes a una instancia.", window: self?.window)
                    }),
            ]),
        ]
    }

    // MARK: iCloud

    private func syncPage() -> [NSView] {
        let m = SyncManager.shared
        var blocks: [NSView] = []
        if SyncManager.iCloudFolder() == nil {
            blocks.append(note("iCloud Drive no está activo en este Mac. Actívalo en Ajustes del Sistema ▸ tu nombre ▸ iCloud ▸ Unidad de iCloud para poder sincronizar."))
        }
        var status = "Todavía no se ha sincronizado."
        if m.isSyncing { status = "Sincronizando…" }
        else if let e = m.lastError { status = "Error: " + e }
        else if let l = m.lastSync {
            let f = RelativeDateTimeFormatter(); f.locale = Locale(identifier: "es"); f.unitsStyle = .full
            status = "Última sincronización: " + (Date().timeIntervalSince(l) < 10 ? "justo ahora" : f.localizedString(for: l, relativeTo: Date()))
        }
        let syncNowBtn = action(m.isSyncing ? "Sincronizando…" : "Sincronizar ahora") { SyncManager.shared.syncNow() }
        syncNowBtn.isEnabled = !m.isSyncing

        var main: [NSView] = [
            row("Sincronizar con iCloud", "Mantiene tus conexiones, comandos y ajustes iguales en todos tus Mac. Los datos se guardan cifrados en la carpeta “jTerm” de tu iCloud Drive.",
                toggle(m.enabled) { [weak self] on in on ? self?.enableSync() : self?.confirmDisableSync() }),
        ]
        if m.enabled {
            main.append(row("Estado", status, syncNowBtn))
            if m.needsPassphrase {
                let missing = m.passphrase == nil
                main.append(row(missing ? "Falta la contraseña de sincronización" : "La contraseña de sincronización no coincide",
                                missing ? "Ingrésala para seguir sincronizando." : "Cambió en otro Mac o se ingresó mal. Ingrésala de nuevo para seguir sincronizando.",
                                action("Ingresar contraseña…", primary: true) { [weak self] in self?.reenterSyncPassphrase() }))
            }
        }
        blocks.append(section("Sincronización", main))
        guard m.enabled else {
            blocks.append(section(nil, [row("Cómo funciona", "El cambio más reciente de cada elemento gana, y los borrados también se propagan. Tus datos viajan cifrados con una contraseña que solo tú conoces: ni Apple ni nadie con acceso a tu iCloud puede leerlos.", NSView())]))
            return blocks
        }
        blocks.append(section("Qué sincronizar", [
            row("Conexiones (servidores)", nil, toggle(m.syncServers) { m.syncServers = $0; m.scheduleSync(after: 1) }),
            row("Cuentas de AWS", nil, toggle(m.syncAWS) { m.syncAWS = $0; m.scheduleSync(after: 1) }),
            row("Comandos guardados", nil, toggle(m.syncCommands) { m.syncCommands = $0; m.scheduleSync(after: 1) }),
            row("Ajustes", "Tema, fuente, SSH y demás. No incluye lo propio de cada Mac, como el shell o las carpetas iniciales.", toggle(m.syncSettings) { m.syncSettings = $0; m.scheduleSync(after: 1) }),
            row("Incluir contraseñas y claves secretas", "Contraseñas de servidores, passphrases y claves secretas de AWS. Viajan cifradas con tu contraseña de sincronización. Actívalo en cada Mac donde quieras recibirlas.",
                toggle(m.syncSecrets) { [weak self] on in on ? self?.confirmSyncSecrets() : { m.syncSecrets = false; m.scheduleSync(after: 1) }() }),
        ]))
        blocks.append(section("Seguridad", [
            row("Contraseña de sincronización", "Cifra tus datos en iCloud. Si la cambias, tendrás que ingresar la nueva en tus otros Mac.",
                action("Cambiar…") { [weak self] in self?.changeSyncPassphrase() }),
            row("Borrar los datos de iCloud", "Elimina el archivo de sincronización y desactiva la sincronización aquí. Tus datos en este Mac no se tocan.",
                action("Borrar…") { [weak self] in self?.confirmDeleteSyncData() }),
        ]))
        return blocks
    }

    private func enableSync() {
        let m = SyncManager.shared
        guard let storage = m.storage else { showAlert("iCloud Drive no está activo", "Actívalo en Ajustes del Sistema ▸ tu nombre ▸ iCloud ▸ Unidad de iCloud.", window: window); select(5); return }
        let pass: String
        if storage.hasData {
            guard let p = askPassphrase("Contraseña de sincronización",
                                        "Ya hay datos de jTerm en tu iCloud. Ingresa la contraseña de sincronización con la que se crearon.", confirm: false) else { select(5); return }
            do { try storage.verify(passphrase: p) }
            catch { showAlert("No se pudo activar", error.localizedDescription, window: window); select(5); return }
            pass = p
        } else {
            guard let p = askPassphrase("Crea una contraseña de sincronización",
                                        "Cifra tus datos en iCloud. La necesitarás para activar la sincronización en tus otros Mac y no se puede recuperar.", confirm: true) else { select(5); return }
            pass = p
        }
        m.passphrase = pass; m.resetState(); m.enabled = true
        select(5)
        m.syncNow { [weak self] result in
            switch result {
            case .success(let r):
                showAlert("Sincronización activada", "Se subieron \(r.uploaded) elementos y se descargaron \(r.downloaded). Se mantendrá actualizada automáticamente.", window: self?.window)
            case .failure(let e):
                showAlert("No se pudo sincronizar", e.localizedDescription, window: self?.window)
            }
        }
    }

    private func confirmDisableSync() {
        let a = NSAlert(); a.messageText = "¿Dejar de sincronizar con iCloud?"
        a.informativeText = "Tus datos en este Mac se conservan y los del iCloud también. Para volver a sincronizar tendrás que ingresar la contraseña de sincronización."
        a.addButton(withTitle: "Dejar de sincronizar"); a.addButton(withTitle: "Cancelar")
        if a.runModal() == .alertFirstButtonReturn { SyncManager.shared.disable() }
        select(5)
    }

    private func confirmSyncSecrets() {
        let a = NSAlert(); a.messageText = "¿Sincronizar también contraseñas y claves?"
        a.informativeText = "Se guardarán en tu iCloud cifradas con tu contraseña de sincronización. Si la olvidas, no hay forma de recuperarlas desde iCloud."
        a.addButton(withTitle: "Sincronizar"); a.addButton(withTitle: "Cancelar")
        if a.runModal() == .alertFirstButtonReturn { SyncManager.shared.syncSecrets = true; SyncManager.shared.scheduleSync(after: 1) }
        select(5)
    }

    private func changeSyncPassphrase() {
        guard let p = askPassphrase("Nueva contraseña de sincronización", "Se volverá a cifrar el archivo de iCloud. Tendrás que ingresarla en tus otros Mac.", confirm: true) else { return }
        do { try SyncManager.shared.changePassphrase(to: p); showAlert("Contraseña cambiada", "Ingresa la nueva en tus otros Mac cuando te la pidan.", window: window) }
        catch { showAlert("No se pudo cambiar", error.localizedDescription, window: window) }
    }

    private func reenterSyncPassphrase() {
        let m = SyncManager.shared
        guard let storage = m.storage,
              let p = askPassphrase("Contraseña de sincronización", "Ingresa la contraseña con la que se cifraron los datos en iCloud.", confirm: false) else { return }
        do { try storage.verify(passphrase: p); m.passphrase = p; m.syncNow() }
        catch { showAlert("No coincide", error.localizedDescription, window: window) }
    }

    private func confirmDeleteSyncData() {
        let a = NSAlert(); a.messageText = "¿Borrar los datos de iCloud?"
        a.informativeText = "Se elimina el archivo de sincronización de tu iCloud y se desactiva la sincronización en este Mac. Los otros Mac conservan sus datos locales. Tus datos en este Mac no se tocan."
        a.alertStyle = .warning; a.addButton(withTitle: "Borrar"); a.addButton(withTitle: "Cancelar")
        if a.runModal() == .alertFirstButtonReturn { SyncManager.shared.storage?.delete(); SyncManager.shared.disable() }
        select(5)
    }

    private func shortcutsPage() -> [NSView] {
        func key(_ k: String, _ d: String) -> NSView {
            let l = NSTextField(labelWithString: k)
            l.font = .monospacedSystemFont(ofSize: 12, weight: .medium); l.textColor = .white
            let box = NSView(); box.wantsLayer = true; box.layer?.cornerRadius = 5
            box.layer?.backgroundColor = NSColor.white.withAlphaComponent(0.08).cgColor
            l.translatesAutoresizingMaskIntoConstraints = false; box.addSubview(l)
            NSLayoutConstraint.activate([
                l.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 8), l.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
                l.topAnchor.constraint(equalTo: box.topAnchor, constant: 3), l.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -3),
            ])
            return row(d, nil, box)
        }
        return [
            section("Pestañas y ventana", [
                key("⌘T", "Nueva pestaña local"), key("⌘W", "Cerrar pestaña"),
                key("⇧⌘]", "Pestaña siguiente"), key("⇧⌘[", "Pestaña anterior"),
                key("⌘1 … ⌘9", "Ir a la pestaña número N"), key("⌘B", "Mostrar u ocultar la barra lateral"),
                key("⇧⌘K", "Abrir la lista de comandos guardados"), key("⌘,", "Abrir los ajustes"),
            ]),
            section("Terminal", [
                key("⌘C / ⌘V", "Copiar / pegar"), key("⌘A", "Seleccionar todo"),
            ]),
            section("Gestor de archivos", [
                key("Retorno", "Renombrar el elemento seleccionado"), key("Suprimir", "Eliminar la selección"),
                key("Doble clic", "Abrir carpeta (o abrir el archivo en tu Mac)"),
            ], footer: "También puedes arrastrar archivos entre los dos paneles, o desde Finder hacia el servidor."),
        ]
    }

    private func dataPage() -> [NSView] {
        let version = (Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String) ?? "0.1.0"
        let verLabel = NSTextField(labelWithString: "jTerm \(version)")
        verLabel.font = .systemFont(ofSize: 13); verLabel.textColor = .secondaryLabelColor
        return [
            section("Servidores y cuentas", [
                row("Exportar", "Guarda tus servidores y cuentas AWS en un archivo. Opcionalmente incluye las contraseñas y claves secretas, y entonces el archivo se cifra con una contraseña que eliges.",
                    action("Exportar…") { [weak self] in self?.exportData() }),
                row("Importar", "Agrega los servidores y cuentas de un archivo exportado. Si está cifrado, te pedirá su contraseña y restaurará también las claves.",
                    action("Importar…") { [weak self] in self?.importData() }),
            ]),
            section("Almacenamiento", [
                row("Carpeta de datos", "Aquí se guardan tus servidores, cuentas y la llave de Instance Connect. Las contraseñas están en el Keychain de macOS.",
                    action("Mostrar en Finder") { NSWorkspace.shared.activateFileViewerSelecting([ServerStore.dir]) }),
            ]),
            section("Restablecer", [
                row("Restablecer ajustes", "Vuelve todos los ajustes a sus valores originales. No borra servidores ni cuentas.",
                    action("Restablecer…") { [weak self] in
                        let a = NSAlert(); a.messageText = "¿Restablecer todos los ajustes?"
                        a.informativeText = "Tus servidores, cuentas y contraseñas no se tocan."
                        a.addButton(withTitle: "Restablecer"); a.addButton(withTitle: "Cancelar")
                        if a.runModal() == .alertFirstButtonReturn { Settings.shared.resetAll(); self?.select(self?.current ?? 0) }
                    }),
            ]),
            section("Acerca de", [row("Versión", nil, verLabel)]),
        ]
    }

    // MARK: Exportar / importar

    private struct ExpServer: Codable { var name: String; var host: String; var port: Int; var user: String; var auth: String; var keyPath: String; var secret: String? }
    private struct ExpAWS: Codable { var name: String; var accessKeyId: String; var region: String; var secret: String? }
    private struct ExpSnippet: Codable { var name: String; var command: String; var autoRun: Bool }
    private struct Export: Codable { var version: Int; var servers: [ExpServer]; var awsAccounts: [ExpAWS]; var commands: [ExpSnippet]? }

    /// Pide una contraseña (con confirmación al exportar). Devuelve nil si se cancela.
    private func askPassphrase(_ title: String, _ info: String, confirm: Bool) -> String? {
        // Contenedor con posiciones explícitas: dentro de un NSStackView los campos (sin ancho propio) quedaban en 0 px.
        let h: CGFloat = confirm ? 58 : 24
        let stack = NSView(frame: NSRect(x: 0, y: 0, width: 280, height: h))
        let f1 = NSSecureTextField(frame: NSRect(x: 0, y: confirm ? 34 : 0, width: 280, height: 24)); f1.placeholderString = "Contraseña"
        let f2 = NSSecureTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24)); f2.placeholderString = "Repetir contraseña"
        stack.addSubview(f1)
        if confirm { stack.addSubview(f2) }
        let a = NSAlert(); a.messageText = title; a.informativeText = info; a.accessoryView = stack
        a.addButton(withTitle: "Continuar"); a.addButton(withTitle: "Cancelar")
        a.window.initialFirstResponder = f1
        while true {
            guard a.runModal() == .alertFirstButtonReturn else { return nil }
            if !confirm { return f1.stringValue.isEmpty ? nil : f1.stringValue }
            if f1.stringValue.count < 8 { a.informativeText = "Usa al menos 8 caracteres.\n\n" + info; continue }
            if f1.stringValue != f2.stringValue { a.informativeText = "Las contraseñas no coinciden.\n\n" + info; continue }
            return f1.stringValue
        }
    }

    private func exportData() {
        let check = NSButton(checkboxWithTitle: "Incluir contraseñas y claves secretas", target: nil, action: nil)
        check.frame = NSRect(x: 0, y: 0, width: 320, height: 22)
        let a = NSAlert()
        a.messageText = "Exportar servidores y cuentas"
        a.informativeText = "Sin esta opción el archivo solo tiene los datos de conexión.\n\nSi incluyes las contraseñas, passphrases y claves secretas de AWS, el archivo se cifra con una contraseña que eliges ahora. Sin ella no se puede abrir ni recuperar."
        a.accessoryView = check
        a.addButton(withTitle: "Continuar"); a.addButton(withTitle: "Cancelar")
        guard a.runModal() == .alertFirstButtonReturn else { return }
        let withSecrets = check.state == .on

        var pass: String?
        if withSecrets {
            guard let p = askPassphrase("Contraseña del archivo",
                                        "Guárdala en un lugar seguro: es la única forma de abrir el archivo.", confirm: true) else { return }
            pass = p
        }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = withSecrets ? "jterm-servidores-cifrado.json" : "jterm-servidores.json"
        panel.allowedContentTypes = [.json]
        guard panel.runModal() == .OK, let url = panel.url else { return }

        let st = AppState.shared
        let servers = st.store.servers, accounts = st.awsStore.accounts
        Task {
            // El Keychain puede mostrar un diálogo de autorización: se lee fuera del hilo principal.
            let secrets: (srv: [UUID: String], aws: [UUID: String]) = withSecrets
                ? await Task.detached {
                    var s: [UUID: String] = [:], w: [UUID: String] = [:]
                    for x in servers { if let v = Keychain.read(x.id).secret, !v.isEmpty { s[x.id] = v } }
                    for x in accounts { if let v = Keychain.read(x.id).secret, !v.isEmpty { w[x.id] = v } }
                    return (s, w)
                }.value
                : ([:], [:])
            let out = Export(version: 2,
                servers: servers.map { ExpServer(name: $0.name, host: $0.host, port: $0.port, user: $0.user,
                                                 auth: $0.auth == .key ? "key" : "password", keyPath: $0.keyPath, secret: secrets.srv[$0.id]) },
                awsAccounts: accounts.map { ExpAWS(name: $0.name, accessKeyId: $0.accessKeyId, region: $0.region, secret: secrets.aws[$0.id]) },
                commands: SnippetStore.shared.items.map { ExpSnippet(name: $0.name, command: $0.command, autoRun: $0.autoRun) })
            do {
                let enc = JSONEncoder(); enc.outputFormatting = [.prettyPrinted, .sortedKeys]
                var data = try enc.encode(out)
                if let pass { data = try await Task.detached { try ExportCrypto.encrypt(data, passphrase: pass) }.value }
                try data.write(to: url, options: .atomic)
                try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
                await MainActor.run {
                    var msg = "\(out.servers.count) servidores, \(out.awsAccounts.count) cuentas y \(out.commands?.count ?? 0) comandos guardados en \(url.lastPathComponent)."
                    if withSecrets {
                        msg += "\n\nIncluye \(secrets.srv.count) contraseñas/passphrases y \(secrets.aws.count) claves secretas de AWS, cifradas con tu contraseña."
                        if servers.contains(where: { $0.auth == .key && !$0.keyPath.isEmpty }) {
                            msg += "\n\nLas llaves privadas (.pem, id_ed25519…) no se incluyen: solo su ruta. Cópialas aparte al nuevo equipo."
                        }
                    }
                    showAlert("Exportación completa", msg, window: self.window)
                }
            } catch { await MainActor.run { showAlert("No se pudo exportar", error.localizedDescription, window: self.window) } }
        }
    }

    private func importData() {
        let panel = NSOpenPanel(); panel.allowedContentTypes = [.json]; panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            var data = try Data(contentsOf: url)
            if let env = ExportCrypto.envelope(from: data) {
                guard let pass = askPassphrase("Archivo cifrado", "Ingresa la contraseña con la que se exportó.", confirm: false) else { return }
                do { data = try ExportCrypto.decrypt(env, passphrase: pass) }
                catch { return showAlert("No se pudo abrir el archivo", error.localizedDescription, window: window) }
            }
            let imp = try JSONDecoder().decode(Export.self, from: data)
            let st = AppState.shared
            var addedS = 0, addedA = 0, skipped = 0, restored = 0
            for e in imp.servers {
                if st.store.servers.contains(where: { $0.host == e.host && $0.user == e.user && $0.port == e.port }) { skipped += 1; continue }
                var s = Server(); s.name = e.name; s.host = e.host; s.port = e.port; s.user = e.user
                s.auth = e.auth == "key" ? .key : .password; s.keyPath = e.keyPath
                st.store.servers.append(s); addedS += 1
                if let sec = e.secret, !sec.isEmpty { Keychain.set(sec, for: s.id); restored += 1 }
            }
            for e in imp.awsAccounts {
                if st.awsStore.accounts.contains(where: { $0.accessKeyId == e.accessKeyId }) { skipped += 1; continue }
                var a = AWSAccount(); a.name = e.name; a.accessKeyId = e.accessKeyId; a.region = e.region
                st.awsStore.upsert(a, secret: e.secret); addedA += 1
                if let sec = e.secret, !sec.isEmpty { restored += 1 }
            }
            let addedC = SnippetStore.shared.add(contentsOf: (imp.commands ?? []).map { Snippet(name: $0.name, command: $0.command, autoRun: $0.autoRun) })
            NotificationCenter.default.post(name: .jtermDataChanged, object: nil)
            var msg = "Se agregaron \(addedS) servidores, \(addedA) cuentas y \(addedC) comandos."
            if skipped > 0 { msg += " Se omitieron \(skipped) que ya existían." }
            msg += restored > 0 ? "\n\nSe restauraron \(restored) contraseñas y claves en el Keychain."
                                : "\n\nEl archivo no incluía contraseñas: edita cada servidor o cuenta para ingresarlas."
            showAlert("Importación completa", msg, window: window)
        } catch { showAlert("No se pudo importar", "El archivo no tiene el formato esperado.\n\(error.localizedDescription)", window: window) }
    }
}
