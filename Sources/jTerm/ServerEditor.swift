import AppKit

// MARK: - Campos estilizados

/// Campo de texto sin borde que avisa cuando gana o pierde el foco.
class FocusField: NSTextField {
    var onFocus: ((Bool) -> Void)?
    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder(); if ok { onFocus?(true) }; return ok
    }
    override func textDidEndEditing(_ n: Notification) { super.textDidEndEditing(n); onFocus?(false) }
}
final class SecureFocusField: NSSecureTextField {
    var onFocus: ((Bool) -> Void)?
    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder(); if ok { onFocus?(true) }; return ok
    }
    override func textDidEndEditing(_ n: Notification) { super.textDidEndEditing(n); onFocus?(false) }
}

/// Etiqueta pequeña + caja redondeada con borde que se ilumina con el foco.
final class StyledField: NSStackView {
    let box = NSView()
    private let caption = NSTextField(labelWithString: "")
    private var plain: FocusField?
    private weak var secure: SecureFocusField?
    private var eye: IconButton?
    /// Para campos revelables: valor guardado a mostrar cuando el campo está vacío (p. ej. al editar una cuenta).
    var loadSecret: (() -> String?)?

    /// Valor actual del campo, esté visible u oculto.
    var secretValue: String {
        get { plain?.isHidden == false ? (plain?.stringValue ?? "") : (secure?.stringValue ?? "") }
        set { secure?.stringValue = newValue; plain?.stringValue = newValue }
    }

    init(_ title: String, field: NSTextField, symbol: String? = nil, placeholder: String = "", revealable: Bool = false) {
        super.init(frame: .zero)
        orientation = .vertical; alignment = .leading; spacing = 5

        caption.stringValue = title.uppercased()
        caption.font = .systemFont(ofSize: 10, weight: .semibold)
        caption.textColor = .tertiaryLabelColor

        field.isBordered = false; field.drawsBackground = false
        field.focusRingType = .none
        field.font = .systemFont(ofSize: 13)
        field.textColor = .white
        field.placeholderAttributedString = NSAttributedString(
            string: placeholder, attributes: [.foregroundColor: NSColor.white.withAlphaComponent(0.28), .font: NSFont.systemFont(ofSize: 13)])
        field.translatesAutoresizingMaskIntoConstraints = false

        box.wantsLayer = true
        box.layer?.cornerRadius = 8
        box.layer?.borderWidth = 1
        setFocused(false)
        box.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(field)

        var leading = box.leadingAnchor
        var pad: CGFloat = 10
        if let symbol {
            let icon = NSImageView(image: NSImage(systemSymbolName: symbol, accessibilityDescription: nil)!)
            icon.symbolConfiguration = .init(pointSize: 11, weight: .regular)
            icon.contentTintColor = .tertiaryLabelColor
            icon.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(icon)
            NSLayoutConstraint.activate([
                icon.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 10),
                icon.centerYAnchor.constraint(equalTo: box.centerYAnchor),
                icon.widthAnchor.constraint(equalToConstant: 14),
            ])
            leading = icon.trailingAnchor; pad = 8
        }
        var trailing = box.trailingAnchor
        var trailPad: CGFloat = -10
        if revealable, let secureField = field as? SecureFocusField {
            let p = FocusField()
            p.isBordered = false; p.drawsBackground = false; p.focusRingType = .none
            p.font = .systemFont(ofSize: 13); p.textColor = .white
            p.placeholderAttributedString = field.placeholderAttributedString
            p.translatesAutoresizingMaskIntoConstraints = false
            p.isHidden = true
            box.addSubview(p)
            let e = IconButton(symbol: "eye", tip: "Mostrar contraseña", size: 24, target: self, action: #selector(toggleReveal))
            box.addSubview(e)
            NSLayoutConstraint.activate([
                e.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -5),
                e.centerYAnchor.constraint(equalTo: box.centerYAnchor),
                p.leadingAnchor.constraint(equalTo: leading, constant: pad),
                p.trailingAnchor.constraint(equalTo: e.leadingAnchor, constant: -4),
                p.centerYAnchor.constraint(equalTo: box.centerYAnchor),
            ])
            p.onFocus = { [weak self] in self?.setFocused($0) }
            plain = p; secure = secureField; eye = e
            trailing = e.leadingAnchor; trailPad = -4
        }
        NSLayoutConstraint.activate([
            field.leadingAnchor.constraint(equalTo: leading, constant: pad),
            field.trailingAnchor.constraint(equalTo: trailing, constant: trailPad),
            field.centerYAnchor.constraint(equalTo: box.centerYAnchor),
            box.heightAnchor.constraint(equalToConstant: 34),
        ])
        addArrangedSubview(caption)
        addArrangedSubview(box)
        box.widthAnchor.constraint(equalTo: widthAnchor).isActive = true

        if let f = field as? FocusField { f.onFocus = { [weak self] in self?.setFocused($0) } }
        if let f = field as? SecureFocusField { f.onFocus = { [weak self] in self?.setFocused($0) } }
    }
    required init?(coder: NSCoder) { fatalError() }

    /// Alterna entre contraseña oculta y visible, conservando lo escrito.
    @objc private func toggleReveal() {
        guard let plain, let secure, let eye else { return }
        let showing = plain.isHidden
        if showing {
            if secure.stringValue.isEmpty, let v = loadSecret?(), !v.isEmpty { secure.stringValue = v }
            plain.stringValue = secure.stringValue
            secure.isHidden = true; plain.isHidden = false
            window?.makeFirstResponder(plain)
        } else {
            secure.stringValue = plain.stringValue
            plain.isHidden = true; secure.isHidden = false
            window?.makeFirstResponder(secure)
        }
        eye.image = NSImage(systemSymbolName: showing ? "eye.slash" : "eye", accessibilityDescription: nil)
        eye.toolTip = showing ? "Ocultar contraseña" : "Mostrar contraseña"
    }

    func setFocused(_ on: Bool) {
        box.layer?.backgroundColor = NSColor.white.withAlphaComponent(on ? 0.07 : 0.05).cgColor
        box.layer?.borderColor = (on ? Theme.accent : NSColor.white.withAlphaComponent(0.10)).cgColor
    }
}

/// Botón con forma de píldora.
final class PillButton: NSButton {
    init(_ title: String, primary: Bool, target: AnyObject, action: Selector) {
        super.init(frame: .zero)
        self.title = title; self.target = target; self.action = action
        isBordered = false; wantsLayer = true
        layer?.cornerRadius = 8
        layer?.backgroundColor = (primary ? Theme.accent : NSColor.white.withAlphaComponent(0.08)).cgColor
        attributedTitle = NSAttributedString(string: title, attributes: [
            .foregroundColor: primary ? NSColor(srgbRed: 0.04, green: 0.08, blue: 0.16, alpha: 1) : NSColor.white,
            .font: NSFont.systemFont(ofSize: 13, weight: .semibold)])
        translatesAutoresizingMaskIntoConstraints = false
        heightAnchor.constraint(equalToConstant: 32).isActive = true
        widthAnchor.constraint(greaterThanOrEqualToConstant: 92).isActive = true
    }
    required init?(coder: NSCoder) { fatalError() }
    /// Aire a los lados del texto para que no quede pegado al borde del botón.
    override var intrinsicContentSize: NSSize {
        let base = super.intrinsicContentSize
        return NSSize(width: base.width + 36, height: base.height)
    }
}

// MARK: - Editor de servidor (hoja modal)

final class ServerEditor: NSObject {
    private let server: Server
    private let isNew: Bool
    private let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 460, height: 500),
                                styleMask: [.titled, .fullSizeContentView], backing: .buffered, defer: false)
    private let name = FocusField(), host = FocusField(), port = FocusField(), user = FocusField()
    private let key = FocusField(), secret = SecureFocusField()
    private let auth = NSSegmentedControl(labels: AuthMethod.allCases.map(\.rawValue), trackingMode: .selectOne,
                                          target: nil, action: nil)
    private var keyRow: NSView!
    private var secretField: StyledField!
    private var envPicker: EnvPicker!
    private let form = NSStackView()
    private var done: () -> Void = {}

    init(server: Server?) {
        var fresh = Server(); fresh.user = Settings.shared.defaultUser; fresh.port = Settings.shared.defaultPort
        self.server = server ?? fresh
        isNew = server == nil
        super.init()
        panel.appearance = NSAppearance(named: .darkAqua)
        panel.backgroundColor = Theme.bg
        panel.titlebarAppearsTransparent = true

        name.stringValue = self.server.name; host.stringValue = self.server.host
        port.stringValue = String(self.server.port); user.stringValue = self.server.user
        key.stringValue = self.server.keyPath
        secret.stringValue = Keychain.get(self.server.id) ?? ""

        // Encabezado
        let badge = NSView(); badge.wantsLayer = true; badge.layer?.cornerRadius = 10
        badge.layer?.backgroundColor = Theme.accent.withAlphaComponent(0.18).cgColor
        let sym = NSImageView(image: NSImage(systemSymbolName: "server.rack", accessibilityDescription: nil)!)
        sym.symbolConfiguration = .init(pointSize: 16, weight: .medium); sym.contentTintColor = Theme.accent
        sym.translatesAutoresizingMaskIntoConstraints = false; badge.addSubview(sym)
        NSLayoutConstraint.activate([
            badge.widthAnchor.constraint(equalToConstant: 40), badge.heightAnchor.constraint(equalToConstant: 40),
            sym.centerXAnchor.constraint(equalTo: badge.centerXAnchor), sym.centerYAnchor.constraint(equalTo: badge.centerYAnchor)])
        let title = NSTextField(labelWithString: isNew ? "Nuevo servidor" : "Editar servidor")
        title.font = .systemFont(ofSize: 17, weight: .semibold); title.textColor = .white
        let sub = NSTextField(labelWithString: "Conexión por SSH")
        sub.font = .systemFont(ofSize: 12); sub.textColor = .secondaryLabelColor
        let titles = NSStackView(views: [title, sub]); titles.orientation = .vertical; titles.alignment = .leading; titles.spacing = 1
        let header = NSStackView(views: [badge, titles]); header.spacing = 12

        // Campos
        let nameF = StyledField("Nombre", field: name, symbol: "tag", placeholder: "Producción, Staging…")
        let hostF = StyledField("Host o IP", field: host, symbol: "network", placeholder: "192.168.1.10")
        let portF = StyledField("Puerto", field: port, placeholder: "22")
        portF.widthAnchor.constraint(equalToConstant: 90).isActive = true
        let hostRow = NSStackView(views: [hostF, portF]); hostRow.spacing = 10; hostRow.alignment = .top
        hostRow.distribution = .fill
        let userF = StyledField("Usuario", field: user, symbol: "person", placeholder: "root")
        envPicker = EnvPicker(selected: self.server.env)

        auth.segmentStyle = .rounded
        auth.selectedSegment = AuthMethod.allCases.firstIndex(of: self.server.auth) ?? 0
        auth.target = self; auth.action = #selector(authChanged)
        let authCaption = NSTextField(labelWithString: "ACCESO")
        authCaption.font = .systemFont(ofSize: 10, weight: .semibold); authCaption.textColor = .tertiaryLabelColor
        let authBox = NSStackView(views: [authCaption, auth]); authBox.orientation = .vertical; authBox.alignment = .leading; authBox.spacing = 5

        let keyF = StyledField("Llave privada", field: key, symbol: "key", placeholder: "~/.ssh/id_ed25519")
        let pick = PillButton("Elegir…", primary: false, target: self, action: #selector(pickKey))
        pick.widthAnchor.constraint(greaterThanOrEqualToConstant: 80).isActive = true
        let kr = NSStackView(views: [keyF, pick]); kr.spacing = 10; kr.alignment = .bottom
        keyRow = kr
        secretField = StyledField("Contraseña", field: secret, symbol: "lock", placeholder: "••••••••", revealable: true)

        let lock = NSImageView(image: NSImage(systemSymbolName: "lock.shield", accessibilityDescription: nil)!)
        lock.symbolConfiguration = .init(pointSize: 11, weight: .regular); lock.contentTintColor = .tertiaryLabelColor
        let note = NSTextField(labelWithString: "Los secretos se guardan en el Keychain de macOS.")
        note.font = .systemFont(ofSize: 11); note.textColor = .tertiaryLabelColor
        let noteRow = NSStackView(views: [lock, note]); noteRow.spacing = 5

        let cancel = PillButton("Cancelar", primary: false, target: self, action: #selector(cancel)); cancel.keyEquivalent = "\u{1b}"
        let save = PillButton("Guardar", primary: true, target: self, action: #selector(save)); save.keyEquivalent = "\r"
        let buttons = NSStackView(views: [NSView(), cancel, save]); buttons.spacing = 8

        form.orientation = .vertical; form.alignment = .leading; form.spacing = 14
        form.edgeInsets = NSEdgeInsets(top: 28, left: 24, bottom: 22, right: 24)
        for v in [header, nameF, hostRow, userF, envPicker!, authBox, keyRow!, secretField!, noteRow, buttons] { form.addArrangedSubview(v) }
        form.setCustomSpacing(20, after: header); form.setCustomSpacing(20, after: noteRow)
        form.translatesAutoresizingMaskIntoConstraints = false
        panel.contentView = NSView(); panel.contentView!.addSubview(form)
        NSLayoutConstraint.activate([
            form.leadingAnchor.constraint(equalTo: panel.contentView!.leadingAnchor),
            form.trailingAnchor.constraint(equalTo: panel.contentView!.trailingAnchor),
            form.topAnchor.constraint(equalTo: panel.contentView!.topAnchor),
            form.widthAnchor.constraint(equalToConstant: 460),
        ])
        for v in [nameF, hostRow, userF, envPicker!, authBox, keyRow!, secretField!, buttons] as [NSView] {
            v.widthAnchor.constraint(equalToConstant: 412).isActive = true
        }
        auth.widthAnchor.constraint(equalTo: authBox.widthAnchor).isActive = true
        authChanged()
    }

    func present(on window: NSWindow, done: @escaping () -> Void) {
        self.done = done
        window.beginSheet(panel) { _ in }
        panel.makeFirstResponder(name)
    }

    @objc private func authChanged() {
        let isKey = auth.selectedSegment == 1
        keyRow.isHidden = !isKey
        (secretField.arrangedSubviews.first as? NSTextField)?.stringValue = (isKey ? "Passphrase (opcional)" : "Contraseña").uppercased()
        form.layoutSubtreeIfNeeded()
        panel.setContentSize(NSSize(width: 460, height: form.fittingSize.height))
    }
    @objc private func pickKey() {
        let p = NSOpenPanel()
        p.directoryURL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".ssh")
        p.showsHiddenFiles = true
        if p.runModal() == .OK, let u = p.url { key.stringValue = u.path }
    }
    @objc private func cancel() { finish() }
    @objc private func save() {
        guard !host.stringValue.isEmpty else { NSSound.beep(); panel.makeFirstResponder(host); return }
        var s = server
        s.name = name.stringValue; s.host = host.stringValue
        s.port = Int(port.stringValue) ?? 22; s.user = user.stringValue
        s.auth = AuthMethod.allCases[max(0, auth.selectedSegment)]
        s.keyPath = key.stringValue
        s.env = envPicker.value
        AppState.shared.store.upsert(s, secret: secretField.secretValue)
        finish()
    }
    private func finish() {
        panel.sheetParent?.endSheet(panel); panel.orderOut(nil); done()
    }
}
