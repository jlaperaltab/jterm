import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    let tabBar = TabBar()
    let sidebar = Sidebar()
    let container = NSView()
    var sidebarWidth: NSLayoutConstraint!
    let resizeHandle = ResizeHandle()
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
            tabBar.trailingAnchor.constraint(equalTo: root.trailingAnchor), tabBar.heightAnchor.constraint(equalToConstant: 38),
            sidebar.topAnchor.constraint(equalTo: tabBar.bottomAnchor), sidebar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            sidebar.leadingAnchor.constraint(equalTo: root.leadingAnchor), sidebarWidth,
            container.topAnchor.constraint(equalTo: tabBar.bottomAnchor), container.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            container.leadingAnchor.constraint(equalTo: sidebar.trailingAnchor), container.trailingAnchor.constraint(equalTo: root.trailingAnchor),
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

        tabBar.onToggleSidebar = { [weak self] in self?.toggleSidebar() }
        sidebar.onEdit = { [weak self] s in self?.edit(s) }
        sidebar.onEditAccount = { [weak self] a in self?.editAccount(a) }
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
        v.pasteText(s.command.trimmingCharacters(in: .newlines))
        if s.autoRun || runNow { v.send([0x0d]) }
        window.makeFirstResponder(v)
    }

    func applicationShouldTerminate(_ app: NSApplication) -> NSApplication.TerminateReply {
        let active = AppState.shared.tabs.filter { $0.isConnected }
        guard Settings.shared.confirmQuit, !active.isEmpty else { return .terminateNow }
        let a = NSAlert()
        a.messageText = active.count == 1 ? "Hay 1 conexión activa" : "Hay \(active.count) conexiones activas"
        a.informativeText = "Si sales de jTerm se cerrarán."
        a.addButton(withTitle: "Salir"); a.addButton(withTitle: "Cancelar")
        return a.runModal() == .alertFirstButtonReturn ? .terminateNow : .terminateCancel
    }

    func applicationWillTerminate(_ n: Notification) { AppState.shared.shutdownAll() }

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
        add(app, "Buscar actualizaciones…", #selector(checkUpdates), "", target: self)
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

        let cmds = submenu("Comandos")
        add(cmds, "Mostrar comandos guardados", #selector(showSnippets), "k", [.command, .shift], target: self)

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
    @MainActor @objc func checkUpdates() { Updater.shared.checkManually() }
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
