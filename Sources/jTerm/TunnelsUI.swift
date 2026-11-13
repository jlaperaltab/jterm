import AppKit

// MARK: - Panel de túneles (pestaña "Túneles" de la barra lateral)

final class TunnelsPanel: NSView, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    private let table = NSTableView()
    private let emptyLabel = NSTextField(wrappingLabelWithString: "")
    var onEdit: ((Tunnel?) -> Void)?
    private var tunnels: [Tunnel] { AppState.shared.tunnelStore.tunnels }

    override init(frame: NSRect) {
        super.init(frame: frame)
        let title = NSTextField(labelWithString: "TÚNELES SSH")
        title.font = .systemFont(ofSize: 11, weight: .semibold); title.textColor = .secondaryLabelColor
        let add = NSButton(image: NSImage(systemSymbolName: "plus", accessibilityDescription: "Agregar túnel")!, target: self, action: #selector(addTunnel))
        add.isBordered = false; add.contentTintColor = .secondaryLabelColor
        let header = NSStackView(views: [title, NSView(), add])
        header.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 6, right: 12)

        let col = NSTableColumn(identifier: .init("t")); col.resizingMask = .autoresizingMask
        table.addTableColumn(col); table.headerView = nil; table.backgroundColor = .clear
        table.dataSource = self; table.delegate = self
        table.doubleAction = #selector(toggleClicked); table.target = self
        let menu = NSMenu(); menu.delegate = self; table.menu = menu
        let scroll = NSScrollView(); scroll.documentView = table; scroll.drawsBackground = false; scroll.hasVerticalScroller = true

        emptyLabel.stringValue = "Aún no hay túneles.\n\nUn túnel conecta un puerto de tu Mac con un servicio al que solo llega el servidor (una base de datos, un panel interno…). Guarda uno con + y actívalo con un clic."
        emptyLabel.font = .systemFont(ofSize: 11); emptyLabel.textColor = .tertiaryLabelColor; emptyLabel.maximumNumberOfLines = 0

        for v in [header, scroll, emptyLabel] as [NSView] { v.translatesAutoresizingMaskIntoConstraints = false; addSubview(v) }
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: topAnchor), header.leadingAnchor.constraint(equalTo: leadingAnchor),
            header.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.topAnchor.constraint(equalTo: header.bottomAnchor), scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor), scroll.bottomAnchor.constraint(equalTo: bottomAnchor),
            emptyLabel.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 8),
            emptyLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
            emptyLabel.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -14),
        ])
        NotificationCenter.default.addObserver(self, selector: #selector(reload), name: .tunnelsDidChange, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(reload), name: .tunnelStateChanged, object: nil)
        reload()
    }
    required init?(coder: NSCoder) { fatalError() }
    deinit { NotificationCenter.default.removeObserver(self) }

    @objc func reload() {
        table.reloadData()
        emptyLabel.isHidden = !tunnels.isEmpty
    }
    @objc private func addTunnel() { onEdit?(nil) }
    @objc private func toggleClicked() { let r = table.clickedRow; if tunnels.indices.contains(r) { AppState.shared.toggleTunnel(tunnels[r], window: window) } }
    @objc private func toggleButton(_ b: NSButton) { if tunnels.indices.contains(b.tag) { AppState.shared.toggleTunnel(tunnels[b.tag], window: window) } }

    // MARK: Tabla

    func numberOfRows(in tableView: NSTableView) -> Int { tunnels.count }
    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        if case .failed = TunnelManager.shared.state(of: tunnels[row].id) { return 66 }
        return 48
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let t = tunnels[row]
        let state = TunnelManager.shared.state(of: t.id)
        let color: NSColor
        switch state {
        case .stopped: color = .systemGray
        case .starting: color = .systemOrange
        case .active: color = .systemGreen
        case .failed: color = .systemRed
        }
        let dot = NSView(); dot.wantsLayer = true; dot.layer?.cornerRadius = 4; dot.layer?.backgroundColor = color.cgColor
        dot.translatesAutoresizingMaskIntoConstraints = false
        dot.widthAnchor.constraint(equalToConstant: 8).isActive = true; dot.heightAnchor.constraint(equalToConstant: 8).isActive = true

        let name = NSTextField(labelWithString: t.title)
        name.font = .systemFont(ofSize: 12, weight: .medium); name.lineBreakMode = .byTruncatingTail
        let via = AppState.shared.store.servers.first { $0.id == t.serverId }.map { " · vía " + ($0.name.isEmpty ? $0.host : $0.name) } ?? " · sin servidor"
        let sub = NSTextField(labelWithString: (t.kind == .local ? "-L  " : "-R  ") + t.summary + via)
        sub.font = .systemFont(ofSize: 10); sub.textColor = .secondaryLabelColor; sub.lineBreakMode = .byTruncatingTail
        for l in [name, sub] {
            l.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)
            l.setContentCompressionResistancePriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        }
        var lines: [NSView] = [name, sub]
        if case .failed(let msg) = state {
            let e = NSTextField(wrappingLabelWithString: msg)
            e.font = .systemFont(ofSize: 10); e.textColor = .systemRed; e.maximumNumberOfLines = 2; e.lineBreakMode = .byTruncatingTail
            e.preferredMaxLayoutWidth = 170
            lines.append(e)
        } else if state == .starting {
            let s = NSTextField(labelWithString: "Conectando…"); s.font = .systemFont(ofSize: 10); s.textColor = .systemOrange
            lines.append(s)
        }
        let texts = NSStackView(views: lines); texts.orientation = .vertical; texts.alignment = .leading; texts.spacing = 1
        texts.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)

        let running = state == .active || state == .starting
        let btn = IconButton(symbol: running ? "stop.fill" : "play.fill", tip: running ? "Detener túnel" : "Iniciar túnel", size: 26, target: self, action: #selector(toggleButton(_:)))
        btn.tag = row
        btn.contentTintColor = running ? .systemRed : .systemGreen
        btn.setContentHuggingPriority(.required, for: .horizontal)

        let line = NSStackView(views: [dot, texts, btn]); line.spacing = 9; line.alignment = .centerY
        line.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 4, right: 8)
        return line
    }

    // MARK: Menú contextual

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let r = table.clickedRow
        guard tunnels.indices.contains(r) else { return }
        let t = tunnels[r]
        let running = [.active, .starting].contains(TunnelManager.shared.state(of: t.id))
        menu.addItem(withTitle: running ? "Detener" : "Iniciar", action: #selector(toggleClicked), keyEquivalent: "").target = self
        if t.kind == .local { menu.addItem(withTitle: "Copiar dirección (localhost:\(t.listenPort))", action: #selector(copyAddress), keyEquivalent: "").target = self }
        menu.addItem(.separator())
        menu.addItem(withTitle: "Editar…", action: #selector(editClicked), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Eliminar", action: #selector(removeClicked), keyEquivalent: "").target = self
    }
    @objc private func copyAddress() {
        let r = table.clickedRow; guard tunnels.indices.contains(r) else { return }
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("localhost:\(tunnels[r].listenPort)", forType: .string)
    }
    @objc private func editClicked() { let r = table.clickedRow; if tunnels.indices.contains(r) { onEdit?(tunnels[r]) } }
    @objc private func removeClicked() {
        let r = table.clickedRow; guard tunnels.indices.contains(r) else { return }
        let t = tunnels[r]
        let a = NSAlert(); a.messageText = "¿Eliminar el túnel “\(t.title)”?"; a.informativeText = "Si está activo, se detiene."
        a.addButton(withTitle: "Eliminar"); a.addButton(withTitle: "Cancelar")
        guard a.runModal() == .alertFirstButtonReturn else { return }
        TunnelManager.shared.stop(t.id)
        AppState.shared.tunnelStore.remove(t)
    }
}

// MARK: - Editor de túnel (hoja modal)

final class TunnelEditor: NSObject {
    private let tunnel: Tunnel
    private let isNew: Bool
    private let servers: [Server]
    private let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 460, height: 560),
                                styleMask: [.titled, .fullSizeContentView], backing: .buffered, defer: false)
    private let name = FocusField(), listen = FocusField(), host = FocusField(), port = FocusField()
    private let kind = NSSegmentedControl(labels: Tunnel.Kind.allCases.map(\.title), trackingMode: .selectOne, target: nil, action: nil)
    private let serverDropdown = Dropdown(symbol: "server.rack")
    private var listenF: StyledField!, hostF: StyledField!
    private let hint = NSTextField(wrappingLabelWithString: "")
    private let form = NSStackView()
    private var done: () -> Void = {}

    init(tunnel: Tunnel?, servers: [Server]) {
        var fresh = Tunnel(); fresh.serverId = servers.first?.id
        self.tunnel = tunnel ?? fresh
        self.servers = servers
        isNew = tunnel == nil
        super.init()
        panel.appearance = NSAppearance(named: .darkAqua)
        panel.backgroundColor = Theme.bg
        panel.titlebarAppearsTransparent = true

        name.stringValue = self.tunnel.name
        listen.stringValue = self.tunnel.listenPort > 0 ? String(self.tunnel.listenPort) : ""
        host.stringValue = self.tunnel.destHost
        port.stringValue = self.tunnel.destPort > 0 ? String(self.tunnel.destPort) : ""

        let badge = NSView(); badge.wantsLayer = true; badge.layer?.cornerRadius = 10
        badge.layer?.backgroundColor = Theme.accent.withAlphaComponent(0.18).cgColor
        let sym = NSImageView(image: NSImage(systemSymbolName: "point.3.connected.trianglepath.dotted", accessibilityDescription: nil)!)
        sym.symbolConfiguration = .init(pointSize: 16, weight: .medium); sym.contentTintColor = Theme.accent
        sym.translatesAutoresizingMaskIntoConstraints = false; badge.addSubview(sym)
        NSLayoutConstraint.activate([
            badge.widthAnchor.constraint(equalToConstant: 40), badge.heightAnchor.constraint(equalToConstant: 40),
            sym.centerXAnchor.constraint(equalTo: badge.centerXAnchor), sym.centerYAnchor.constraint(equalTo: badge.centerYAnchor)])
        let title = NSTextField(labelWithString: isNew ? "Nuevo túnel" : "Editar túnel")
        title.font = .systemFont(ofSize: 17, weight: .semibold); title.textColor = .white
        let sub = NSTextField(labelWithString: "Reenvío de puertos por SSH")
        sub.font = .systemFont(ofSize: 12); sub.textColor = .secondaryLabelColor
        let titles = NSStackView(views: [title, sub]); titles.orientation = .vertical; titles.alignment = .leading; titles.spacing = 1
        let header = NSStackView(views: [badge, titles]); header.spacing = 12

        let nameF = StyledField("Nombre", field: name, symbol: "tag", placeholder: "Base de datos de producción")
        kind.segmentStyle = .rounded
        kind.selectedSegment = Tunnel.Kind.allCases.firstIndex(of: self.tunnel.kind) ?? 0
        kind.target = self; kind.action = #selector(kindChanged)
        let kindCaption = NSTextField(labelWithString: "TIPO")
        kindCaption.font = .systemFont(ofSize: 10, weight: .semibold); kindCaption.textColor = .tertiaryLabelColor
        let kindBox = NSStackView(views: [kindCaption, kind]); kindBox.orientation = .vertical; kindBox.alignment = .leading; kindBox.spacing = 5
        kind.widthAnchor.constraint(equalTo: kindBox.widthAnchor).isActive = true

        listenF = StyledField("Puerto en este Mac", field: listen, symbol: "number", placeholder: "5433")
        hostF = StyledField("Destino", field: host, symbol: "network", placeholder: "db.interna")
        let portF = StyledField("Puerto destino", field: port, placeholder: "5432")
        portF.widthAnchor.constraint(equalToConstant: 110).isActive = true
        let destRow = NSStackView(views: [hostF, portF]); destRow.spacing = 10; destRow.alignment = .top; destRow.distribution = .fill

        let srvCaption = NSTextField(labelWithString: "SERVIDOR POR EL QUE PASA")
        srvCaption.font = .systemFont(ofSize: 10, weight: .semibold); srvCaption.textColor = .tertiaryLabelColor
        serverDropdown.items = servers.map { $0.name.isEmpty ? $0.host : $0.name }
        serverDropdown.placeholder = "Primero guarda un servidor"
        serverDropdown.selectedIndex = servers.firstIndex { $0.id == self.tunnel.serverId } ?? (servers.isEmpty ? -1 : 0)
        let srvBox = NSStackView(views: [srvCaption, serverDropdown]); srvBox.orientation = .vertical; srvBox.alignment = .leading; srvBox.spacing = 5
        serverDropdown.widthAnchor.constraint(equalTo: srvBox.widthAnchor).isActive = true

        hint.font = .systemFont(ofSize: 11); hint.textColor = .tertiaryLabelColor; hint.maximumNumberOfLines = 0
        let lock = NSImageView(image: NSImage(systemSymbolName: "info.circle", accessibilityDescription: nil)!)
        lock.symbolConfiguration = .init(pointSize: 11, weight: .regular); lock.contentTintColor = .tertiaryLabelColor
        let hintRow = NSStackView(views: [lock, hint]); hintRow.spacing = 5; hintRow.alignment = .top

        let cancel = PillButton("Cancelar", primary: false, target: self, action: #selector(cancelTapped)); cancel.keyEquivalent = "\u{1b}"
        let save = PillButton("Guardar", primary: true, target: self, action: #selector(saveTapped)); save.keyEquivalent = "\r"
        let buttons = NSStackView(views: [NSView(), cancel, save]); buttons.spacing = 8

        form.orientation = .vertical; form.alignment = .leading; form.spacing = 14
        form.edgeInsets = NSEdgeInsets(top: 28, left: 24, bottom: 22, right: 24)
        for v in [header, nameF, kindBox, listenF!, destRow, srvBox, hintRow, buttons] as [NSView] { form.addArrangedSubview(v) }
        form.setCustomSpacing(20, after: header); form.setCustomSpacing(20, after: hintRow)
        form.translatesAutoresizingMaskIntoConstraints = false
        panel.contentView = NSView(); panel.contentView!.addSubview(form)
        NSLayoutConstraint.activate([
            form.leadingAnchor.constraint(equalTo: panel.contentView!.leadingAnchor),
            form.trailingAnchor.constraint(equalTo: panel.contentView!.trailingAnchor),
            form.topAnchor.constraint(equalTo: panel.contentView!.topAnchor),
            form.widthAnchor.constraint(equalToConstant: 460),
        ])
        for v in [nameF, kindBox, listenF!, destRow, srvBox, hintRow, buttons] as [NSView] { v.widthAnchor.constraint(equalToConstant: 412).isActive = true }
        kindChanged()
    }

    func present(on window: NSWindow, done: @escaping () -> Void) {
        self.done = done
        window.beginSheet(panel) { _ in }
        panel.makeFirstResponder(name)
    }

    private var selectedKind: Tunnel.Kind { Tunnel.Kind.allCases[max(0, kind.selectedSegment)] }

    @objc private func kindChanged() {
        let local = selectedKind == .local
        (listenF.arrangedSubviews.first as? NSTextField)?.stringValue = (local ? "Puerto en este Mac" : "Puerto en el servidor").uppercased()
        (hostF.arrangedSubviews.first as? NSTextField)?.stringValue = (local ? "Destino (visto desde el servidor)" : "Destino (en este Mac)").uppercased()
        hint.stringValue = local
            ? "Abre un puerto en este Mac (solo accesible desde él) y lo lleva, a través del servidor, al destino. Ejemplo: puerto 5433 → db.interna:5432 y luego conectas tu cliente a localhost:5433."
            : "Abre un puerto en el servidor y lo lleva a un destino de este Mac. Sirve para que el servidor llegue a algo tuyo, como una app en localhost:3000."
        form.layoutSubtreeIfNeeded()
        panel.setContentSize(NSSize(width: 460, height: form.fittingSize.height))
    }

    @objc private func cancelTapped() { finish() }
    @objc private func saveTapped() {
        var t = tunnel
        t.name = name.stringValue.trimmingCharacters(in: .whitespaces)
        t.kind = selectedKind
        t.listenPort = Int(listen.stringValue.trimmingCharacters(in: .whitespaces)) ?? 0
        t.destHost = host.stringValue.trimmingCharacters(in: .whitespaces)
        t.destPort = Int(port.stringValue.trimmingCharacters(in: .whitespaces)) ?? 0
        t.serverId = servers.indices.contains(serverDropdown.selectedIndex) ? servers[serverDropdown.selectedIndex].id : nil
        if let error = t.validationError() { NSSound.beep(); return showAlert("Revisa el túnel", error, window: panel) }
        if t != tunnel, [.active, .starting].contains(TunnelManager.shared.state(of: t.id)) { TunnelManager.shared.stop(t.id) }   // los cambios se aplican al reiniciarlo
        AppState.shared.tunnelStore.upsert(t)
        finish()
    }
    private func finish() { panel.sheetParent?.endSheet(panel); panel.orderOut(nil); done() }
}
