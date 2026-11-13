import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuItemValidation {
    var window: NSWindow!
    let tabBar = TabBar()
    let sidebar = Sidebar()
    let container = NSView()
    var sidebarWidth: NSLayoutConstraint!
    let resizeHandle = ResizeHandle()
    let envStrip = NSView()      // franja de color arriba del terminal cuando la pestaña tiene entorno
    let snippetButton = SnippetButton()
    let snippets = SnippetsPopover()
    static let defaultSidebarWidth: CGFloat = 220
    var savedSidebarWidth: CGFloat = {
        let w = CGFloat(UserDefaults.standard.double(forKey: "sidebarWidth"))
        return w >= 170 ? min(w, 520) : defaultSidebarWidth
    }()
    var editor: ServerEditor?
    var shown: Tab?
    var sidebarVisible = true
    private var sidebarBeforeFiles: Bool?

    func applicationDidFinishLaunching(_ n: Notification) {
        NSApp.appearance = NSAppearance(named: .darkAqua)
        buildMenu()

        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 640),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                          backing: .buffered, defer: false)
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.backgroundColor = Theme.bar
        window.minSize = NSSize(width: 640, height: 380)
        window.center()
        window.setFrameAutosaveName("TerminalMain")

        let root = NSView()
        window.contentView = root
        container.wantsLayer = true
        container.layer?.backgroundColor = Theme.bg.cgColor
        for v in [tabBar, sidebar, container] { v.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(v) }
        sidebarWidth = sidebar.widthAnchor.constraint(equalToConstant: savedSidebarWidth)
        NSLayoutConstraint.activate([
            tabBar.topAnchor.constraint(equalTo: root.topAnchor), tabBar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            tabBar.trailingAnchor.constraint(equalTo: root.trailingAnchor), tabBar.heightAnchor.constraint(equalToConstant: 42),
            sidebar.topAnchor.constraint(equalTo: tabBar.bottomAnchor), sidebar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            sidebar.leadingAnchor.constraint(equalTo: root.leadingAnchor), sidebarWidth,
            container.topAnchor.constraint(equalTo: tabBar.bottomAnchor), container.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            container.leadingAnchor.constraint(equalTo: sidebar.trailingAnchor), container.trailingAnchor.constraint(equalTo: root.trailingAnchor),
        ])

        envStrip.wantsLayer = true; envStrip.isHidden = true
        envStrip.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(envStrip)
        NSLayoutConstraint.activate([
            envStrip.topAnchor.constraint(equalTo: container.topAnchor), envStrip.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            envStrip.trailingAnchor.constraint(equalTo: container.trailingAnchor), envStrip.heightAnchor.constraint(equalToConstant: 3),
        ])

        resizeHandle.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(resizeHandle)
        NSLayoutConstraint.activate([
            resizeHandle.centerXAnchor.constraint(equalTo: sidebar.trailingAnchor),
            resizeHandle.widthAnchor.constraint(equalToConstant: 8),
            resizeHandle.topAnchor.constraint(equalTo: tabBar.bottomAnchor),
            resizeHandle.bottomAnchor.constraint(equalTo: root.bottomAnchor),
        ])
        resizeHandle.onDrag = { [weak self] x in self?.setSidebarWidth(x) }
        resizeHandle.onReset = { [weak self] in self?.setSidebarWidth(Self.defaultSidebarWidth) }

        // Botón flotante de comandos guardados (esquina inferior izquierda del terminal)
        root.addSubview(snippetButton)
        NSLayoutConstraint.activate([
            snippetButton.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -22),   // deja libre la barra de desplazamiento
            snippetButton.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -14),
        ])
        snippetButton.onClick = { [weak self] in self?.showSnippets() }
        snippets.onPick = { [weak self] s, run in self?.insertSnippet(s, runNow: run) }
        snippets.onVisibilityChanged = { [weak self] shown in self?.snippetButton.active = shown }

        tabBar.onToggleSidebar = { [weak self] in self?.toggleSidebar() }
        sidebar.onEdit = { [weak self] s in self?.edit(s) }
        sidebar.onEditAccount = { [weak self] a in self?.editAccount(a) }
        sidebar.onEditTunnel = { [weak self] t in self?.editTunnel(t) }
        AppState.shared.onChange = { [weak self] in self?.refresh() }
        AppState.shared.newLocal()
        applyTheme()
        NotificationCenter.default.addObserver(self, selector: #selector(applyTheme), name: Settings.didChange, object: nil)
        NotificationCenter.default.addObserver(forName: .jtermDataChanged, object: nil, queue: .main) { [weak self] _ in
            self?.sidebar.reload(); self?.sidebar.aws.reloadAccounts()
        }
        if !Settings.shared.sidebarAtLaunch { toggleSidebar() }
        window.makeKeyAndOrderFront(nil)
        SyncManager.shared.start()
                    // --- el "otro Mac" cambia cosas en el archivo ---
                        // --- borrado local se propaga al archivo ---
                                // limpieza de lo que la prueba dejó en el Keychain
        NSApp.activate(ignoringOtherApps: true)
        MainActor.assumeIsolated { Updater.shared.checkOnLaunch() }
    }

    func refresh() {
        tabBar.reload()
        sidebar.reload()
        updateSnippetButton()
        guard let tab = AppState.shared.current else { return }
        envStrip.isHidden = tab.env == nil
        envStrip.layer?.backgroundColor = tab.env?.color.cgColor
        if shown !== tab {
            shown?.content.removeFromSuperview()
            let v = tab.content
            v.translatesAutoresizingMaskIntoConstraints = false
            container.addSubview(v)
            NSLayoutConstraint.activate([
                v.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: 14),
                v.trailingAnchor.constraint(equalTo: container.trailingAnchor),
                v.topAnchor.constraint(equalTo: container.topAnchor, constant: 8),
                v.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -6),
            ])
            shown = tab
        }
        // En el gestor de archivos se oculta la barra lateral para dar espacio a los paneles.
        if tab is FileTab {
            if sidebarBeforeFiles == nil { sidebarBeforeFiles = sidebarVisible }
            if sidebarVisible { toggleSidebar() }
        } else if let was = sidebarBeforeFiles {
            sidebarBeforeFiles = nil
            if was != sidebarVisible { toggleSidebar() }
        }
        window.makeFirstResponder(tab.focusView)
    }

    func toggleSidebar() {
        sidebarVisible.toggle()
        tabBar.sidebarVisible = sidebarVisible
        sidebarWidth.constant = sidebarVisible ? savedSidebarWidth : 0
        sidebar.isHidden = !sidebarVisible
        resizeHandle.isHidden = !sidebarVisible
    }

    /// Ancho de la barra lateral entre 180 y 520 px; se recuerda para la próxima vez.
    func setSidebarWidth(_ w: CGFloat) {
        guard sidebarVisible else { return }
        let maxW = min(520, max(220, window.frame.width - 360))
        savedSidebarWidth = min(max(w, 180), maxW)
        sidebarWidth.constant = savedSidebarWidth
        UserDefaults.standard.set(Double(savedSidebarWidth), forKey: "sidebarWidth")
    }

    func edit(_ s: Server?) {
        let e = ServerEditor(server: s)
        editor = e
        e.present(on: window) { [weak self] in self?.editor = nil; self?.sidebar.reload(); self?.refresh() }
    }

    var tunnelEditor: TunnelEditor?
    func editTunnel(_ t: Tunnel?) {
        let e = TunnelEditor(tunnel: t, servers: AppState.shared.store.servers)
        tunnelEditor = e
        e.present(on: window) { [weak self] in self?.tunnelEditor = nil }
    }

    var accountEditor: AWSAccountEditor?
    func editAccount(_ a: AWSAccount?) {
        let e = AWSAccountEditor(account: a)
        accountEditor = e
        e.present(on: window) { [weak self] in self?.accountEditor = nil; self?.sidebar.aws.reloadAccounts() }
    }

    @objc func applyTheme() {
        container.layer?.backgroundColor = Settings.shared.theme.bgColor.cgColor
        updateSnippetButton()
    }

    /// El botón solo se muestra en pestañas de terminal y si el ajuste lo permite.
    func updateSnippetButton() {
        snippetButton.isHidden = !(AppState.shared.current is TerminalTab) || !Settings.shared.showSnippetsButton
    }

    @objc func showSnippets() {
        guard AppState.shared.current is TerminalTab else { NSSound.beep(); return }
        // Mismo lugar que el botón, aunque esté oculto por el ajuste.
        snippets.toggle(relativeTo: NSRect(x: container.bounds.maxX - 22 - 36, y: 14, width: 36, height: 36), of: container)
    }

    /// Pega el comando en la consola activa; lo ejecuta si el comando lo pide o con ⌥.
    func insertSnippet(_ s: Snippet, runNow: Bool) {
        guard let tab = AppState.shared.current as? TerminalTab, let v = tab.view as? JTermTerminalView else { NSSound.beep(); return }
        guard v.approvePaste(s.command, fromSnippet: true) else { return }
        v.pasteText(s.command.trimmingCharacters(in: .newlines))
        if s.autoRun || runNow { v.send([0x0d]) }
        window.makeFirstResponder(v)
    }

    func applicationShouldTerminate(_ app: NSApplication) -> NSApplication.TerminateReply {
        let active = AppState.shared.tabs.filter { $0.isConnected }
        let tunnels = TunnelManager.shared.activeCount
        guard Settings.shared.confirmQuit, !active.isEmpty || tunnels > 0 else { return .terminateNow }
        let a = NSAlert()
        let parts = [active.isEmpty ? nil : (active.count == 1 ? "1 conexión activa" : "\(active.count) conexiones activas"),
                     tunnels == 0 ? nil : (tunnels == 1 ? "1 túnel activo" : "\(tunnels) túneles activos")].compactMap { $0 }
        a.messageText = "Hay " + parts.joined(separator: " y ")
        a.informativeText = "Si sales de jTerm se cerrarán."
        a.addButton(withTitle: "Salir"); a.addButton(withTitle: "Cancelar")
        return a.runModal() == .alertFirstButtonReturn ? .terminateNow : .terminateCancel
    }

    func applicationWillTerminate(_ n: Notification) { AppState.shared.shutdownAll(); TunnelManager.shared.stopAll() }

    func applicationShouldTerminateAfterLastWindowClosed(_ s: NSApplication) -> Bool { true }

    // MARK: Menú
    func buildMenu() {
        let main = NSMenu()
        func add(_ menu: NSMenu, _ title: String, _ sel: Selector?, _ key: String = "",
                 _ mods: NSEvent.ModifierFlags = .command, target: AnyObject? = nil) {
            let i = menu.addItem(withTitle: title, action: sel, keyEquivalent: key)
            i.keyEquivalentModifierMask = mods; i.target = target
        }
        func submenu(_ title: String) -> NSMenu {
            let m = NSMenu(title: title)
            main.addItem(withTitle: title, action: nil, keyEquivalent: "").submenu = m
            return m
        }
        let app = submenu("jTerm")
        add(app, "Acerca de jTerm", #selector(showAbout), "", target: self)
        app.items.last?.image = NSImage(systemSymbolName: "info.circle", accessibilityDescription: nil)
        add(app, "Buscar actualizaciones…", #selector(checkUpdates), "", target: self)
        app.items.last?.image = NSImage(systemSymbolName: "arrow.triangle.2.circlepath", accessibilityDescription: nil)
        app.addItem(.separator())
        add(app, "Ajustes…", #selector(openSettings), ",", target: self)
        app.addItem(.separator())
        add(app, "Salir de jTerm", #selector(NSApplication.terminate(_:)), "q")

        let file = submenu("Archivo")
        add(file, "Nueva pestaña", #selector(newTab), "t", target: self)
        add(file, "Cerrar pestaña", #selector(closeTab), "w", target: self)

        let edit = submenu("Edición")
        add(edit, "Copiar", #selector(NSText.copy(_:)), "c")
        add(edit, "Pegar", #selector(NSText.paste(_:)), "v")
        add(edit, "Seleccionar todo", #selector(NSText.selectAll(_:)), "a")
        edit.addItem(.separator())
        // Buscar en el historial del terminal: la barra de búsqueda es la del propio terminal (la reciben por la cadena de respuesta).
        for (title, key, mods, tag) in [("Buscar…", "f", NSEvent.ModifierFlags.command, NSFindPanelAction.showFindPanel),
                                        ("Buscar siguiente", "g", .command, .next),
                                        ("Buscar anterior", "g", [.command, .shift], .previous),
                                        ("Usar la selección para buscar", "e", .command, .setFindString)] {
            let it = edit.addItem(withTitle: title, action: NSSelectorFromString("performFindPanelAction:"), keyEquivalent: key)
            it.keyEquivalentModifierMask = mods; it.tag = Int(tag.rawValue)
        }

        let term = submenu("Terminal")
        add(term, "Prompt anterior", #selector(prevPrompt), String(UnicodeScalar(NSUpArrowFunctionKey)!), .command, target: self)
        add(term, "Prompt siguiente", #selector(nextPrompt), String(UnicodeScalar(NSDownArrowFunctionKey)!), .command, target: self)
        term.addItem(.separator())
        add(term, "Grabar sesión", #selector(toggleRecording), "r", [.command, .option], target: self)
        add(term, "Mostrar grabaciones en Finder", #selector(revealRecordings), "", target: self)

        let cmds = submenu("Comandos")
        add(cmds, "Mostrar comandos guardados", #selector(showSnippets), "k", [.command, .shift], target: self)

        add(cmds, "Buscar servidor, instancia o comando…", #selector(showFinder), "k", target: self)

        let tabs = submenu("Pestañas")
        add(tabs, "Siguiente", #selector(nextTab), "]", [.command, .shift], target: self)
        add(tabs, "Anterior", #selector(prevTab), "[", [.command, .shift], target: self)
        add(tabs, "Mostrar/ocultar servidores", #selector(sidebarAction), "b", target: self)
        for i in 1...9 {
            let it = tabs.addItem(withTitle: "Pestaña \(i)", action: #selector(goTab(_:)), keyEquivalent: "\(i)")
            it.tag = i - 1; it.target = self
        }
        NSApp.mainMenu = main
    }
    @objc func showAbout() {
        let repo = (Bundle.main.object(forInfoDictionaryKey: "JTermUpdateRepo") as? String) ?? ""
        let credits = NSMutableAttributedString(
            string: "Terminal para macOS con SSH, AWS, archivos y comandos guardados.\n",
            attributes: [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.labelColor])
        if !repo.isEmpty, let url = URL(string: "https://github.com/\(repo)") {
            credits.append(NSAttributedString(string: "github.com/\(repo)",
                attributes: [.font: NSFont.systemFont(ofSize: 11), .link: url]))
        }
        credits.addAttribute(.paragraphStyle, value: { let p = NSMutableParagraphStyle(); p.alignment = .center; return p }(),
                             range: NSRange(location: 0, length: credits.length))
        NSApp.orderFrontStandardAboutPanel(options: [.applicationName: "jTerm", .credits: credits])
        NSApp.activate(ignoringOtherApps: true)
    }
    // MARK: Terminal: prompts y grabación

    private var terminalView: JTermTerminalView? { (AppState.shared.current as? TerminalTab)?.view as? JTermTerminalView }
    @objc func prevPrompt() { terminalView?.jumpPrompt(-1) }
    @objc func nextPrompt() { terminalView?.jumpPrompt(1) }

    @objc func toggleRecording() {
        guard let tab = AppState.shared.current as? TerminalTab else { return NSSound.beep() }
        if tab.isRecording { tab.stopRecording(); return }
        do { try tab.startRecording() }
        catch { showAlert("No se pudo grabar la sesión", error.localizedDescription, window: window) }
    }
    @objc func revealRecordings() {
        let dir = SessionRecorder.defaultDirectory
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        if let url = (AppState.shared.current as? TerminalTab)?.recordingURL { NSWorkspace.shared.activateFileViewerSelecting([url]) }
        else { NSWorkspace.shared.open(dir) }
    }

    /// El título de "Grabar sesión" cambia según la pestaña; los atajos de prompt solo valen en terminales.
    @objc func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let tab = AppState.shared.current as? TerminalTab
        switch item.action {
        case #selector(toggleRecording):
            item.title = tab?.isRecording == true ? "Detener grabación" : "Grabar sesión"
            return tab != nil
        case #selector(prevPrompt), #selector(nextPrompt): return tab != nil
        default: return true
        }
    }

    @MainActor @objc func checkUpdates() { Updater.shared.checkManually() }
    /// Elementos del buscador rápido (⌘K).
    func finderItems() -> [FinderItem] {
        let st = AppState.shared
        var items: [FinderItem] = []
        for t in st.tabs {
            items.append(FinderItem(kind: .tab, title: t.title, subtitle: t.isConnected ? "Pestaña abierta · conectada" : "Pestaña abierta",
                                    symbol: t.iconName, env: t.env, run: { _ in st.select(t) }))
        }
        for s in st.store.servers {
            items.append(FinderItem(kind: .server, title: s.name.isEmpty ? s.host : s.name, subtitle: (s.user.isEmpty ? "" : s.user + "@") + s.host,
                                    symbol: s.auth == .key ? "key.fill" : "lock.fill", env: s.env, keywords: s.host, run: { _ in st.connect(s) }))
        }
        let aws = sidebar.aws
        let acc = aws.currentAccount
        for i in aws.loadedInstances where i.state == "running" {
            let ip = i.publicIP.isEmpty ? i.privateIP : i.publicIP
            items.append(FinderItem(kind: .instance, title: i.name.isEmpty ? i.id : i.name,
                                    subtitle: "AWS\(acc.map { " · " + ($0.name.isEmpty ? $0.accessKeyId : $0.name) } ?? "") · \(i.region) · \(ip)",
                                    symbol: "cloud.fill", env: acc?.env, keywords: "\(i.id) \(i.publicIP) \(i.privateIP) \(i.type)",
                                    run: { _ in aws.connect(i) }))
        }
        for c in SnippetStore.shared.items {
            items.append(FinderItem(kind: .snippet, title: c.title, subtitle: c.firstLine + (c.lineCount > 1 ? "  (+\(c.lineCount - 1) líneas)" : ""),
                                    symbol: "chevron.left.forwardslash.chevron.right", keywords: c.command,
                                    run: { [weak self] alt in self?.insertSnippet(c, runNow: alt) }))
        }
        for t in st.tunnelStore.tunnels {
            let running = [.active, .starting].contains(TunnelManager.shared.state(of: t.id))
            items.append(FinderItem(kind: .action, title: (running ? "Detener túnel: " : "Activar túnel: ") + t.title, subtitle: t.summary,
                                    symbol: "point.3.connected.trianglepath.dotted", keywords: "tunel túnel ssh puerto \(t.listenPort) \(t.destHost)",
                                    run: { [weak self] _ in AppState.shared.toggleTunnel(t, window: self?.window) }))
        }
        if let acc = acc {
            items.append(FinderItem(kind: .action, title: "Explorar S3", subtitle: "Buckets de \(acc.name.isEmpty ? acc.accessKeyId : acc.name)",
                                    symbol: "shippingbox", env: acc.env, keywords: "s3 bucket archivos", run: { _ in aws.openS3() }))
        }
        items.append(FinderItem(kind: .action, title: "Nueva pestaña local", subtitle: "Abre un terminal en este Mac", symbol: "terminal",
                                run: { _ in st.newLocal() }))
        items.append(FinderItem(kind: .action, title: "Ajustes", subtitle: "Abre los ajustes de jTerm", symbol: "gearshape",
                                run: { [weak self] _ in self?.openSettings() }))
        return items
    }
    @objc func showFinder() { QuickFinder.shared.toggle(over: window, items: finderItems()) }
    @objc func openSettings() { SettingsWindow.shared.show() }
    @objc func newTab() { AppState.shared.newLocal() }
    @objc func closeTab() { if let t = AppState.shared.current { AppState.shared.requestClose(t) } }
    @objc func nextTab() { AppState.shared.step(1) }
    @objc func prevTab() { AppState.shared.step(-1) }
    @objc func sidebarAction() { toggleSidebar() }
    @objc func goTab(_ s: NSMenuItem) { AppState.shared.select(index: s.tag) }
}

@main
enum Main {
    static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}
