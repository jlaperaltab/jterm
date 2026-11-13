import AppKit

// MARK: - Pestaña individual

/// Alto común de la pestaña y de los botones de la barra (barra lateral, nueva pestaña, archivos), para que se vean alineados.
let tabBarItemHeight: CGFloat = 28

/// Botón de la barra sin los márgenes de alineación que AppKit añade a los NSButton: así mide exactamente lo que se le pide
/// y queda del mismo tamaño que las pestañas.
final class BarButton: NSButton {
    override var alignmentRectInsets: NSEdgeInsets { NSEdgeInsets() }
}

final class TabButton: NSView {
    let tab: Tab
    private let label = NSTextField(labelWithString: "")
    private let icon = NSImageView()
    private let close = NSButton()
    private let envBar = NSView()
    private var hovering = false { didSet { refresh() } }
    var active = false { didSet { refresh() } }

    init(tab: Tab) {
        self.tab = tab
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 4
        label.stringValue = tab.title
        label.font = .systemFont(ofSize: 12)
        label.lineBreakMode = .byTruncatingTail
        icon.image = NSImage(systemSymbolName: tab.isRecording ? "record.circle.fill" : tab.iconName, accessibilityDescription: nil)
        icon.symbolConfiguration = .init(pointSize: 11, weight: .regular)
        close.image = NSImage(systemSymbolName: "xmark", accessibilityDescription: "Cerrar")
        close.symbolConfiguration = .init(pointSize: 9, weight: .bold)
        close.isBordered = false
        close.target = self; close.action = #selector(closeTab)
        let stack = NSStackView(views: [icon, label, close])
        stack.spacing = 7
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 11, bottom: 0, right: 8)
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
            label.widthAnchor.constraint(lessThanOrEqualToConstant: 190),
            heightAnchor.constraint(equalToConstant: tabBarItemHeight),
        ])
        if let env = tab.env {      // franja inferior con el color del entorno
            envBar.wantsLayer = true; envBar.layer?.cornerRadius = 1; envBar.layer?.backgroundColor = env.color.cgColor
            envBar.translatesAutoresizingMaskIntoConstraints = false
            addSubview(envBar)
            NSLayoutConstraint.activate([
                envBar.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8), envBar.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
                envBar.bottomAnchor.constraint(equalTo: bottomAnchor), envBar.heightAnchor.constraint(equalToConstant: 2),
            ])
        }
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
        refresh()
    }
    required init?(coder: NSCoder) { fatalError() }

    private func refresh() {
        // Cada pestaña es una "pastilla" con fondo y borde propios, para ver dónde empieza y termina; la activa destaca más.
        let bg: NSColor, border: NSColor
        if let env = tab.env {
            bg = env.color.withAlphaComponent(active ? 0.26 : (hovering ? 0.16 : 0.10))
            border = env.color.withAlphaComponent(active ? 0.70 : 0.32)
        } else {
            bg = NSColor.white.withAlphaComponent(active ? 0.15 : (hovering ? 0.09 : 0.045))
            border = NSColor.white.withAlphaComponent(active ? 0.26 : 0.09)
        }
        layer?.backgroundColor = bg.cgColor
        layer?.borderColor = border.cgColor
        layer?.borderWidth = 1
        let c: NSColor = active ? .white : .secondaryLabelColor
        label.textColor = c; icon.contentTintColor = tab.isRecording ? .systemRed : (tab.isConnected ? .systemGreen : c); close.contentTintColor = c
        close.alphaValue = (hovering || active) ? 1 : 0
    }
    override func mouseEntered(with e: NSEvent) { hovering = true }
    override func mouseExited(with e: NSEvent) { hovering = false }
    override func mouseDown(with e: NSEvent) { AppState.shared.select(tab) }
    @objc private func closeTab() { AppState.shared.requestClose(tab) }
}

// MARK: - Barra de pestañas

final class TabBar: NSView {
    private let stack = NSStackView()
    var onToggleSidebar: (() -> Void)?
    private let filesButton = BarButton()
    private let toggleButton = BarButton()
    /// Refleja si la barra lateral está visible: el botón se resalta mientras lo está.
    var sidebarVisible = true { didSet { updateToggle() } }
    /// Mismo aspecto de "pastilla" que las pestañas y el botón de la barra lateral.
    private func stylePill(_ b: BarButton) {
        b.isBordered = false; b.wantsLayer = true
        b.layer?.cornerRadius = 4; b.layer?.borderWidth = 1
        b.layer?.backgroundColor = NSColor.white.withAlphaComponent(0.045).cgColor
        b.layer?.borderColor = NSColor.white.withAlphaComponent(0.09).cgColor
        b.contentTintColor = .secondaryLabelColor
        b.symbolConfiguration = .init(pointSize: 12, weight: .regular)
    }
    private func updateToggle() {
        toggleButton.contentTintColor = sidebarVisible ? Theme.accent : .secondaryLabelColor
        toggleButton.layer?.backgroundColor = (sidebarVisible ? Theme.accent.withAlphaComponent(0.20) : NSColor.white.withAlphaComponent(0.045)).cgColor
        toggleButton.layer?.borderColor = (sidebarVisible ? Theme.accent.withAlphaComponent(0.55) : NSColor.white.withAlphaComponent(0.09)).cgColor
        toggleButton.toolTip = sidebarVisible ? "Ocultar la barra lateral (⌘B)" : "Mostrar la barra lateral (⌘B)"
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        NotificationCenter.default.addObserver(self, selector: #selector(accentChanged), name: Settings.accentDidChange, object: nil)
        wantsLayer = true
        layer?.backgroundColor = Theme.bar.cgColor
        let toggle = toggleButton
        toggle.image = NSImage(systemSymbolName: "sidebar.left", accessibilityDescription: "Servidores")
        toggle.target = self; toggle.action = #selector(self.toggle)
        toggle.isBordered = false; toggle.wantsLayer = true; toggle.layer?.cornerRadius = 4; toggle.layer?.borderWidth = 1
        let plus = BarButton()
        plus.image = NSImage(systemSymbolName: "plus", accessibilityDescription: "Nueva pestaña")
        plus.target = self; plus.action = #selector(newTab)
        stylePill(plus)
        filesButton.image = NSImage(systemSymbolName: "arrow.left.arrow.right.square", accessibilityDescription: "Transferir archivos")
        stylePill(filesButton)
        filesButton.toolTip = "Transferir archivos de esta conexión"
        filesButton.target = self; filesButton.action = #selector(openFiles)
        stack.spacing = 6
        let row = NSStackView(views: [toggle, stack, plus, filesButton])
        row.spacing = 10
        row.translatesAutoresizingMaskIntoConstraints = false
        addSubview(row)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 78), // botones de la ventana
            row.centerYAnchor.constraint(equalTo: centerYAnchor),
            row.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -8),
            toggle.widthAnchor.constraint(equalToConstant: tabBarItemHeight), toggle.heightAnchor.constraint(equalToConstant: tabBarItemHeight),
            plus.widthAnchor.constraint(equalToConstant: tabBarItemHeight), plus.heightAnchor.constraint(equalToConstant: tabBarItemHeight),
            filesButton.widthAnchor.constraint(equalToConstant: tabBarItemHeight), filesButton.heightAnchor.constraint(equalToConstant: tabBarItemHeight),
        ])
        updateToggle()
    }
    required init?(coder: NSCoder) { fatalError() }
    @objc private func accentChanged() { updateToggle(); reload() }

    func reload() {
        stack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for t in AppState.shared.tabs {
            let b = TabButton(tab: t)
            b.active = t.id == AppState.shared.selected
            stack.addArrangedSubview(b)
        }
        filesButton.isHidden = AppState.shared.current?.fileTarget == nil
    }
    override func mouseDown(with e: NSEvent) { window?.performDrag(with: e) }
    @objc private func toggle() { onToggleSidebar?() }
    @objc private func newTab() { AppState.shared.newLocal() }
    @objc private func openFiles() { AppState.shared.openFilesForCurrent() }
}

// MARK: - Barra lateral (Servidores / AWS)

final class Sidebar: NSView, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    private let table = NSTableView()
    private let serversView = NSView()
    let aws = AWSPanel()
    let tunnels = TunnelsPanel()
    private var store: ServerStore { AppState.shared.store }
    var onEdit: ((Server?) -> Void)?
    var onEditAccount: ((AWSAccount?) -> Void)? { didSet { aws.onEditAccount = onEditAccount } }
    var onEditTunnel: ((Tunnel?) -> Void)? { didSet { tunnels.onEdit = onEditTunnel } }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.backgroundColor = Theme.bar.withAlphaComponent(0.6).cgColor

        let seg = NSSegmentedControl(labels: ["Servidores", "AWS", "Túneles"], trackingMode: .selectOne,
                                     target: self, action: #selector(switchTab(_:)))
        seg.selectedSegment = 0; seg.segmentStyle = .texturedRounded

        // Lista de servidores
        let title = NSTextField(labelWithString: "GUARDADOS")
        title.font = .systemFont(ofSize: 11, weight: .semibold); title.textColor = .secondaryLabelColor
        let add = NSButton(image: NSImage(systemSymbolName: "plus", accessibilityDescription: "Agregar")!,
                           target: self, action: #selector(addServer))
        add.isBordered = false; add.contentTintColor = .secondaryLabelColor
        let header = NSStackView(views: [title, NSView(), add])
        header.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 6, right: 12)

        let col = NSTableColumn(identifier: .init("s")); col.resizingMask = .autoresizingMask
        table.addTableColumn(col)
        table.headerView = nil
        table.backgroundColor = .clear
        table.rowHeight = 42
        table.dataSource = self; table.delegate = self
        table.doubleAction = #selector(connect); table.target = self
        let menu = NSMenu(); menu.delegate = self; table.menu = menu
        let scroll = NSScrollView()
        scroll.documentView = table; scroll.drawsBackground = false; scroll.hasVerticalScroller = true

        for v in [header, scroll] { v.translatesAutoresizingMaskIntoConstraints = false; serversView.addSubview(v) }
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: serversView.topAnchor),
            header.leadingAnchor.constraint(equalTo: serversView.leadingAnchor),
            header.trailingAnchor.constraint(equalTo: serversView.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: header.bottomAnchor),
            scroll.leadingAnchor.constraint(equalTo: serversView.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: serversView.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: serversView.bottomAnchor),
        ])

        aws.isHidden = true; tunnels.isHidden = true
        for v in [seg, serversView, aws, tunnels] as [NSView] { v.translatesAutoresizingMaskIntoConstraints = false; addSubview(v) }
        NSLayoutConstraint.activate([
            seg.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            seg.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            seg.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
        ])
        for v in [serversView, aws, tunnels] as [NSView] {
            NSLayoutConstraint.activate([
                v.topAnchor.constraint(equalTo: seg.bottomAnchor, constant: 8),
                v.leadingAnchor.constraint(equalTo: leadingAnchor),
                v.trailingAnchor.constraint(equalTo: trailingAnchor),
                v.bottomAnchor.constraint(equalTo: bottomAnchor),
            ])
        }
    }
    required init?(coder: NSCoder) { fatalError() }

    @objc private func switchTab(_ s: NSSegmentedControl) {
        let i = s.selectedSegment
        serversView.isHidden = i != 0; aws.isHidden = i != 1; tunnels.isHidden = i != 2
        if i == 1 { aws.activate() }
    }

    func reload() { table.reloadData(); aws.reloadRows() }
    func numberOfRows(in t: NSTableView) -> Int { store.servers.count }
    func tableView(_ t: NSTableView, heightOfRow row: Int) -> CGFloat {
        AppState.shared.isConnected(key: "server:\(store.servers[row].id.uuidString)") ? 60 : 42
    }

    func tableView(_ t: NSTableView, viewFor c: NSTableColumn?, row: Int) -> NSView? {
        let s = store.servers[row]
        let name = NSTextField(labelWithString: s.name.isEmpty ? s.host : s.name)
        name.font = .systemFont(ofSize: 12, weight: .medium)
        let sub = NSTextField(labelWithString: (s.user.isEmpty ? "" : s.user + "@") + s.host)
        sub.font = .systemFont(ofSize: 10); sub.textColor = .secondaryLabelColor
        let icon = NSImageView(image: NSImage(systemSymbolName: s.auth == .key ? "key.fill" : "lock.fill", accessibilityDescription: nil)!)
        icon.contentTintColor = Theme.accent
        name.lineBreakMode = .byTruncatingTail; sub.lineBreakMode = .byTruncatingTail
        name.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        name.setContentCompressionResistancePriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        sub.setContentCompressionResistancePriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        let nameRow = NSStackView(views: [name] + (s.env.map { [EnvTag($0)] } ?? [])); nameRow.spacing = 6
        let texts = NSStackView(views: [nameRow, sub]); texts.orientation = .vertical; texts.alignment = .leading; texts.spacing = 1
        if AppState.shared.isConnected(key: "server:\(s.id.uuidString)") {
            texts.addArrangedSubview(ConnectedBadge()); texts.setCustomSpacing(5, after: sub)
        }
        texts.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)   // ocupa todo el ancho libre
        icon.setContentCompressionResistancePriority(.required, for: .horizontal)
        icon.setContentHuggingPriority(.required, for: .horizontal)
        let views: [NSView] = [icon, texts]
        let line = NSStackView(views: views); line.spacing = 8
        line.edgeInsets = NSEdgeInsets(top: 4, left: 8, bottom: 4, right: 8)
        return line
    }

    @objc private func addServer() { onEdit?(nil) }
    @objc private func connect() {
        let r = table.clickedRow; if r >= 0 { AppState.shared.connect(store.servers[r]) }
    }
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard table.clickedRow >= 0 else { return }
        menu.addItem(withTitle: "Conectar", action: #selector(connect), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Transferir archivos…", action: #selector(files), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Editar…", action: #selector(edit), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Eliminar", action: #selector(remove), keyEquivalent: "").target = self
    }
    @objc private func files() {
        let r = table.clickedRow; guard r >= 0 else { return }
        let s = store.servers[r]
        AppState.shared.openFiles(key: "server:\(s.id.uuidString)") { await SSHTargets.server(s) }
    }
    @objc private func edit() { let r = table.clickedRow; if r >= 0 { onEdit?(store.servers[r]) } }
    @objc private func remove() {
        let r = table.clickedRow; guard r >= 0 else { return }
        store.remove(store.servers[r]); reload()
    }
}
