import AppKit
import SwiftTerm

/// Terminal con un pegado que se comporta como el de Terminal.app o iTerm, y con avisos de seguridad por entorno.
final class JTermTerminalView: LocalProcessTerminalView {
    private static let pasteStart: [UInt8] = [0x1b, 0x5b, 0x32, 0x30, 0x30, 0x7e]   // ESC [ 200 ~
    private static let pasteEnd: [UInt8] = [0x1b, 0x5b, 0x32, 0x30, 0x31, 0x7e]     // ESC [ 201 ~

    /// Entorno de la conexión (producción activa los avisos de comandos delicados) y nombre para los diálogos.
    var env: ServerEnv?
    var envName = ""
    /// Si no es nil, todo lo que llega del proceso se guarda también en este archivo.
    var recorder: SessionRecorder?
    /// ⌘+clic sobre una URL o ruta de la salida (línea bajo el cursor y columna).
    var linkHandler: ((String, Int) -> Bool)?
    /// Filas absolutas donde el usuario pulsó Retorno (los prompts), para saltar entre ellos.
    private(set) var promptRows: [Int] = []
    /// En una conexión SSH las rutas son del servidor: solo cuentan las absolutas (o con ~), y no se comprueba que existan.
    var isRemote = false

    private let overlay = LinkOverlay()
    private var hover: (row: Int, range: Range<Int>)?
    private var refreshPending = false
    private var monitor: Any?

    override init(frame: NSRect) {
        super.init(frame: frame)
        returnInterceptor = { [weak self] in self?.approveReturn() ?? true }
        linkClickInterceptor = { [weak self] line, col in self?.linkHandler?(line, col) ?? false }
        overlay.frame = bounds; overlay.autoresizingMask = [.width, .height]
        addSubview(overlay)
        // ⌘ pulsado o ratón en movimiento: se actualiza el enlace subrayado bajo el cursor
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.flagsChanged, .mouseMoved, .mouseExited]) { [weak self] e in
            if let self, e.window != nil, e.window === self.window { self.updateHover() }
            return e
        }
    }
    deinit { if let monitor { NSEvent.removeMonitor(monitor) } }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); scheduleLinkRefresh() }
    override func setFrameSize(_ newSize: NSSize) { super.setFrameSize(newSize); scheduleLinkRefresh() }

    // MARK: Enlaces subrayados

    func linkMatch(in line: String, column: Int) -> LinkDetector.Match? {
        guard let m = LinkDetector.match(in: line, column: column, allowBareFilenames: !isRemote) else { return nil }
        if isRemote, case .path(let p, _) = m.target, !(p.hasPrefix("/") || p.hasPrefix("~")) { return nil }
        return m
    }

    func scheduleLinkRefresh() {
        guard !refreshPending else { return }
        refreshPending = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) { [weak self] in self?.refreshLinks() }
    }

    private func refreshLinks() {
        refreshPending = false
        let cell = jtermCellSize
        let t = getTerminal()
        guard cell.width > 0, cell.height > 0 else { return }
        func rect(_ row: Int, _ r: Range<Int>) -> NSRect {
            NSRect(x: CGFloat(r.lowerBound) * cell.width, y: frame.height - CGFloat(row + 1) * cell.height,
                   width: CGFloat(r.count) * cell.width, height: cell.height)
        }
        var out: [LinkOverlay.Line] = []
        if Settings.shared.underlineLinks, !t.isCurrentBufferAlternate {        // en vim, htop… no se toca la pantalla
            for row in 0..<t.rows {
                guard let text = t.getLine(row: row)?.translateToString(trimRight: true), text.contains("://") else { continue }
                for u in LinkDetector.urlMatches(in: text) where hover?.row != row || hover?.range != u.range {
                    out.append(.init(rect: rect(row, u.range), strong: false))
                }
            }
        }
        if let h = hover { out.append(.init(rect: rect(h.row, h.range), strong: true)) }
        overlay.lines = out
    }

    private func setHover(_ h: (row: Int, range: Range<Int>)?) {
        let changed = hover?.row != h?.row || hover?.range != h?.range
        hover = h
        if h != nil { NSCursor.pointingHand.set() }
        if changed { refreshLinks() }
    }

    /// Con ⌘ pulsado, subraya (más marcado) el enlace o ruta que hay bajo el cursor.
    private func updateHover() {
        guard let window, NSEvent.modifierFlags.contains(.command) else { return setHover(nil) }
        let p = convert(window.mouseLocationOutsideOfEventStream, from: nil)
        let cell = jtermCellSize
        guard bounds.contains(p), cell.width > 0, cell.height > 0 else { return setHover(nil) }
        let col = Int(p.x / cell.width), row = Int((frame.height - p.y) / cell.height)
        let t = getTerminal()
        guard row >= 0, row < t.rows, let text = t.getLine(row: row)?.translateToString(trimRight: true),
              let m = linkMatch(in: text, column: col) else { return setHover(nil) }
        setHover((row, m.range))
    }

    // La salida cambia o se desplaza: hay que volver a calcular qué subrayar
    override func rangeChanged(source: TerminalView, startY: Int, endY: Int) { super.rangeChanged(source: source, startY: startY, endY: endY); scheduleLinkRefresh() }
    override func scrolled(source: TerminalView, position: Double) { super.scrolled(source: source, position: position); scheduleLinkRefresh() }

    /// Antes de entregar la salida al terminal se copia (limpia) al archivo de grabación. Las aplicaciones de pantalla
    /// completa (vim, htop…) no se graban: solo dejarían texto desordenado.
    override func dataReceived(slice: ArraySlice<UInt8>) {
        defer { scheduleLinkRefresh() }                 // llegó salida nueva: hay que volver a subrayar las URLs
        guard let rec = recorder else { super.dataReceived(slice: slice); return }
        let t = getTerminal()
        let wasAlt = t.isCurrentBufferAlternate
        super.dataReceived(slice: slice)
        let isAlt = t.isCurrentBufferAlternate
        if !wasAlt && !isAlt { rec.write(slice) }
        else if !wasAlt && isAlt { rec.note("[aplicación de pantalla completa]") }
        else if wasAlt && !isAlt { rec.note("[fin de la aplicación de pantalla completa]") }
    }

    /// Al escribir algo con el historial desplazado, se vuelve al final (como en Terminal.app). Las teclas de control
    /// (flechas, respuestas del terminal…) no lo hacen: empiezan por ESC.
    override func send(source: TerminalView, data: ArraySlice<UInt8>) {
        if jtermIsScrolledUp, let first = data.first, first != 0x1b { jtermScrollToBottom() }
        super.send(source: source, data: data)
    }

    // MARK: Saltar entre prompts

    private func markPrompt() {
        guard !getTerminal().isCurrentBufferAlternate else { return }
        let row = jtermCursorAbsoluteRow(), oldest = jtermOldestAbsoluteRow()
        promptRows.removeAll { $0 < oldest }
        if promptRows.last != row { promptRows.append(row) }
    }

    /// direction -1 = prompt anterior, +1 = siguiente (o el final si no hay más).
    func jumpPrompt(_ direction: Int) {
        let top = jtermTopAbsoluteRow(), oldest = jtermOldestAbsoluteRow()
        if direction < 0 {
            guard let target = promptRows.last(where: { $0 < top && $0 >= oldest }) else { return NSSound.beep() }
            jtermScroll(toAbsoluteRow: target)
        } else if let target = promptRows.first(where: { $0 > top }) {
            jtermScroll(toAbsoluteRow: target)
        } else {
            jtermScrollToBottom()
        }
    }
    required init?(coder: NSCoder) { fatalError() }

    /// Cada salto de línea pegado se envía como Retorno (\r), igual que la tecla Enter. SwiftTerm envía \n,
    /// que programas como nano interpretan como Ctrl-J ("Justificar") y desordenan el texto.
    override func paste(_ sender: Any) {
        guard let text = NSPasteboard.general.string(forType: .string), !text.isEmpty else { return }
        guard approvePaste(text, fromSnippet: false) else { return }
        pasteText(text)
    }

    // MARK: Avisos de seguridad

    private var inProd: Bool { env == .prod && Settings.shared.warnDangerousProd }

    /// Pregunta antes de pegar varias líneas, o un comando delicado en producción. Devuelve true si se puede pegar.
    /// Los comandos guardados (`fromSnippet`) solo se revisan por el riesgo: el usuario ya eligió su texto.
    func approvePaste(_ text: String, fromSnippet: Bool) -> Bool {
        let trimmed = text.trimmingCharacters(in: .newlines)
        let lines = trimmed.components(separatedBy: .newlines)
        let risk = inProd ? CommandGuard.risk(in: text) : nil
        let multi = !fromSnippet && Settings.shared.confirmMultilinePaste && lines.count > 1 && !getTerminal().isCurrentBufferAlternate
        guard risk != nil || multi else { return true }

        let preview = lines.prefix(8).map { $0.count > 100 ? String($0.prefix(100)) + "…" : $0 }.joined(separator: "\n")
            + (lines.count > 8 ? "\n… y \(lines.count - 8) líneas más" : "")
        let a = NSAlert()
        a.alertStyle = risk != nil ? .critical : .warning
        if let risk {
            a.messageText = "Comando delicado en PRODUCCIÓN"
            a.informativeText = "Vas a pegar en “\(envName)”: \(risk).\n\n\(preview)"
        } else {
            a.messageText = "¿Pegar \(lines.count) líneas?"
            a.informativeText = "Se enviarán al terminal. Si alguna línea es un comando, se ejecutará.\n\n\(preview)"
        }
        a.addButton(withTitle: "Cancelar"); a.addButton(withTitle: "Pegar de todos modos")
        return a.runModal() == .alertSecondButtonReturn
    }

    /// Al pulsar Retorno en una pestaña de producción, revisa la línea que se va a ejecutar (lo que se ve en pantalla,
    /// así que cubre lo escrito, el historial y el autocompletado). Devuelve false para no enviar el Retorno.
    private func approveReturn() -> Bool {
        markPrompt()
        guard inProd else { return true }
        let t = getTerminal()
        guard !t.isCurrentBufferAlternate else { return true }      // vim, nano, less…
        let row = t.buffer.y
        guard let line = t.getLine(row: row) else { return true }
        var text = line.translateToString(trimRight: true)
        // Comando largo que ocupa varias filas: las anteriores llenas hasta el borde son el inicio de la misma línea.
        var r = row - 1
        while r >= 0, row - r <= 6, let prev = t.getLine(row: r) {
            let p = prev.translateToString(trimRight: true)
            guard p.count >= t.cols - 1 else { break }
            text = p + text; r -= 1
        }
        guard let risk = CommandGuard.risk(in: text) else { return true }
        let a = NSAlert()
        a.alertStyle = .critical
        a.messageText = "Comando delicado en PRODUCCIÓN"
        a.informativeText = "Estás en “\(envName)”: \(risk).\n\n\(text.trimmingCharacters(in: .whitespaces))"
        a.addButton(withTitle: "Cancelar"); a.addButton(withTitle: "Ejecutar de todos modos")
        return a.runModal() == .alertSecondButtonReturn
    }

    func pasteText(_ raw: String) {
        var text = raw.replacingOccurrences(of: "\r\n", with: "\r").replacingOccurrences(of: "\n", with: "\r")
        let bracketed = getTerminal().bracketedPasteMode
        if bracketed {
            // Un texto pegado no debe poder cerrar el modo de pegado por su cuenta.
            text = text.replacingOccurrences(of: "\u{1b}[201~", with: "")
            send(data: Self.pasteStart[...])
        }
        send(txt: text)
        if bracketed { send(data: Self.pasteEnd[...]) }
    }
}

/// Una pestaña de la ventana: terminal o gestor de archivos.
protocol Tab: AnyObject {
    var id: UUID { get }
    var title: String { get }
    var iconName: String { get }
    var content: NSView { get }
    var focusView: NSView { get }
    var onExit: ((Tab) -> Void)? { get set }
    /// Si existe, permite abrir el gestor de archivos para el mismo destino.
    var fileTarget: (() async throws -> SSHTarget)? { get }
    /// Identifica el servidor al que pertenece la pestaña (para el badge “Conectado”).
    var connectionKey: String? { get }
    var isConnected: Bool { get }
    /// Entorno de la conexión (para colorear la pestaña); nil si no tiene.
    var env: ServerEnv? { get }
    /// true mientras la sesión se está grabando a un archivo.
    var isRecording: Bool { get }
    func shutdown()
}

extension Tab {
    var env: ServerEnv? { nil }
    var isRecording: Bool { false }
}

final class TerminalTab: NSObject, LocalProcessTerminalViewDelegate, Tab {
    let id = UUID()
    var title: String
    let isSSH: Bool
    let view: LocalProcessTerminalView
    var onExit: ((Tab) -> Void)?
    var fileTarget: (() async throws -> SSHTarget)?
    var connectionKey: String?
    var env: ServerEnv? { didSet { applyEnv(); if env == .prod, Settings.shared.autoRecordProd, recorder == nil { try? startRecording() } } }
    private(set) var recorder: SessionRecorder?
    var isRecording: Bool { recorder != nil }
    private(set) var connected = false
    var isConnected: Bool { connected }
    /// ssh ejecuta LocalCommand solo cuando la autenticación fue exitosa: imprime un título que detectamos.
    static let markerTitle = "jterm-connected"
    static let markerArgs = ["-o", "PermitLocalCommand=yes", "-o", "LocalCommand=printf '\\033]0;jterm-connected\\007'"]
    var content: NSView { view }
    var focusView: NSView { view }
    var iconName: String { isSSH ? "network" : "terminal" }
    func shutdown() { stopRecording() }
    var closesOnExit = true

    init(title: String, executable: String, args: [String], env: [String: String] = [:], isSSH: Bool = false) {
        self.title = title
        self.isSSH = isSSH
        view = JTermTerminalView(frame: NSRect(x: 0, y: 0, width: 800, height: 500))
        super.init()
        view.processDelegate = self
        (view as? JTermTerminalView)?.linkHandler = { [weak self] line, col in self?.handleLinkClick(line: line, column: col) ?? false }
        (view as? JTermTerminalView)?.isRemote = isSSH
        applySettings()
        var environment = Terminal.getEnvironmentVariables(termName: Settings.shared.termName)
        for (k, v) in env { environment.append("\(k)=\(v)") }
        view.startProcess(executable: executable, args: args, environment: environment,
                          currentDirectory: isSSH ? nil : Settings.shared.resolvedStartDirectory)
        NotificationCenter.default.addObserver(self, selector: #selector(settingsChanged), name: Settings.didChange, object: nil)
    }

    deinit { NotificationCenter.default.removeObserver(self) }
    @objc private func settingsChanged() { applySettings() }
    func applyEnv() { (view as? JTermTerminalView).map { $0.env = env; $0.envName = title } }

    // MARK: Grabación

    func startRecording() throws {
        guard recorder == nil else { return }
        let r = try SessionRecorder(directory: SessionRecorder.defaultDirectory, title: title, env: env)
        recorder = r
        (view as? JTermTerminalView)?.recorder = r
        AppState.shared.onChange?()
    }
    func stopRecording() {
        guard let r = recorder else { return }
        (view as? JTermTerminalView)?.recorder = nil
        recorder = nil
        r.stop()
        AppState.shared.onChange?()
    }
    var recordingURL: URL? { recorder?.url }

    // MARK: Enlaces y rutas (⌘+clic)

    /// Abre la URL o ruta que hay bajo el cursor. En pestañas locales la ruta se abre con la app predeterminada;
    /// en pestañas SSH la ruta es del servidor, así que se muestra en el gestor de archivos de esa conexión.
    func handleLinkClick(line: String, column: Int) -> Bool {
        guard let target = LinkDetector.target(in: line, column: column, allowBareFilenames: !isSSH) else { return false }
        switch target {
        case .url(let url):
            NSWorkspace.shared.open(url); return true
        case .path(let raw, _):
            if isSSH {
                guard raw.hasPrefix("/") || raw.hasPrefix("~"), fileTarget != nil, let make = fileTarget else { NSSound.beep(); return true }
                AppState.shared.openFiles(key: connectionKey, startPath: raw, make)
                return true
            }
            var path = (raw as NSString).expandingTildeInPath
            if !path.hasPrefix("/") {
                let pid = view.process.shellPid
                let base = pid > 0 ? currentDirectory(ofProcess: pid) : nil
                path = ((base ?? NSHomeDirectory()) as NSString).appendingPathComponent(path)
            }
            path = (path as NSString).standardizingPath
            guard FileManager.default.fileExists(atPath: path) else { NSSound.beep(); return true }
            NSWorkspace.shared.open(URL(fileURLWithPath: path))
            return true
        }
    }

    private var appliedTheme = "", appliedFont = ""

    private static func termColor(_ hex: String) -> Color {
        let c = NSColor(hex: hex)
        return Color(red: UInt16(c.redComponent * 65535), green: UInt16(c.greenComponent * 65535), blue: UInt16(c.blueComponent * 65535))
    }

    /// Aplica los ajustes (tema, fuente, cursor, historial, teclado) a este terminal; se llama también en vivo.
    func applySettings() {
        let s = Settings.shared, th = s.theme
        let font = s.font
        if appliedFont != "\(font.fontName)-\(font.pointSize)" { view.font = font; appliedFont = "\(font.fontName)-\(font.pointSize)" }
        if appliedTheme != th.name {
            view.nativeBackgroundColor = th.bgColor
            view.nativeForegroundColor = th.fgColor
            view.caretColor = th.cursorColor
            view.selectedTextBackgroundColor = th.selectionColor
            view.installColors(th.ansi.map(Self.termColor))
            appliedTheme = th.name
        }
        let term = view.getTerminal()
        let blink = s.cursorBlink
        switch s.cursorShape {
        case "underline": term.setCursorStyle(blink ? .blinkUnderline : .steadyUnderline)
        case "bar": term.setCursorStyle(blink ? .blinkBar : .steadyBar)
        default: term.setCursorStyle(blink ? .blinkBlock : .steadyBlock)
        }
        term.options.scrollback = s.scrollback
        if !term.isCurrentBufferAlternate { term.buffer.changeHistorySize(s.scrollback) }
        view.optionAsMetaKey = s.optionAsMeta
        view.allowMouseReporting = s.mouseReporting
        view.backspaceSendsControlH = s.backspaceCtrlH
        (view as? JTermTerminalView)?.scheduleLinkRefresh()
    }

    static func local() -> TerminalTab {
        let s = Settings.shared
        return TerminalTab(title: "Local", executable: s.resolvedShell, args: s.loginShell ? ["-l"] : [])
    }

    static func ssh(_ s: Server, keychain: (secret: String?, status: OSStatus), jump: JumpSetup = JumpSetup()) -> TerminalTab {
        var args = ["-p", String(s.port)] + SSHSettings.args() + markerArgs
        var env: [String: String] = [:]
        if s.auth == .key, !s.keyPath.isEmpty {
            let path = (s.keyPath as NSString).expandingTildeInPath
            args += ["-i", path, "-o", "IdentitiesOnly=yes"]
            env["TERMINAL_KEY"] = path
        }
        args += jump.options
        for (k, v) in jump.env { env[k] = v }
        let (stored, status) = keychain
        var warning: String?
        if let secret = stored {
            env["SSH_ASKPASS"] = ServerStore.askpassScript()
            env["SSH_ASKPASS_REQUIRE"] = "force"
            env["TERMINAL_SECRET"] = secret
        } else if status != errSecItemNotFound {
            let msg = SecCopyErrorMessageString(status, nil) as String? ?? "OSStatus \(status)"
            warning = "[jTerm] No se pudo leer la contraseña del Keychain: \(msg). Escríbela a mano si ssh la pide."
        }
        args.append(s.user.isEmpty ? s.host : "\(s.user)@\(s.host)")
        let tab = TerminalTab(title: s.name.isEmpty ? s.host : s.name,
                              executable: "/usr/bin/ssh", args: args, env: env, isSSH: true)
        tab.closesOnExit = false
        tab.fileTarget = { await SSHTargets.server(s) }
        tab.connectionKey = "server:\(s.id.uuidString)"
        tab.env = s.env
        if let warning { tab.view.feed(text: "\u{1b}[33m\(warning)\u{1b}[0m\r\n") }
        return tab
    }

    /// SSH a una instancia EC2 con la llave temporal de Instance Connect.
    static func awsSSH(_ i: EC2Instance, user: String, keyPath: String) -> TerminalTab {
        let host = i.publicDNS.isEmpty ? i.publicIP : i.publicDNS
        let args = ["-i", keyPath, "-o", "IdentitiesOnly=yes", "-o", "HostKeyAlias=\(i.id)"]
            + SSHSettings.args() + markerArgs + ["\(user)@\(host)"]
        let tab = TerminalTab(title: i.name, executable: "/usr/bin/ssh", args: args, isSSH: true)
        tab.closesOnExit = false
        tab.connectionKey = "aws:\(i.id)"
        return tab
    }

    /// Sesión SSM (instancias sin IP pública). Requiere AWS CLI + session-manager-plugin.
    static func awsSSM(_ i: EC2Instance, creds: AWSCredentials, awsPath: String) -> TerminalTab {
        let env = ["AWS_ACCESS_KEY_ID": creds.accessKey, "AWS_SECRET_ACCESS_KEY": creds.secret,
                   "AWS_DEFAULT_REGION": i.region,
                   "PATH": "/opt/homebrew/bin:/usr/local/bin:/usr/local/sessionmanagerplugin/bin:/usr/bin:/bin"]
        let tab = TerminalTab(title: i.name, executable: awsPath,
                              args: ["ssm", "start-session", "--target", i.id, "--region", i.region],
                              env: env, isSSH: true)
        tab.closesOnExit = false
        tab.connectionKey = "aws:\(i.id)"
        tab.connected = true   // SSM no ejecuta LocalCommand: se considera conectada mientras el proceso vive
        return tab
    }

    // MARK: LocalProcessTerminalViewDelegate
    func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}
    func setTerminalTitle(source: LocalProcessTerminalView, title: String) {
        guard title == Self.markerTitle else { return }
        DispatchQueue.main.async { self.connected = true; AppState.shared.onChange?() }
    }
    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
    func processTerminated(source: TerminalView, exitCode: Int32?) {
        DispatchQueue.main.async {
            self.connected = false
            self.stopRecording()
            // Una conexión SSH nunca cierra su pestaña sola (un reinicio del servidor la corta con código 0 y se perdería la salida)
            if self.closesOnExit || (exitCode == 0 && !self.isSSH) { self.onExit?(self); return }
            AppState.shared.onChange?()
            let code = exitCode.map(String.init) ?? "?"
            self.view.feed(text: "\r\n\u{1b}[31m[jTerm] La conexión terminó (código \(code)). Cierra la pestaña con ⌘W.\u{1b}[0m\r\n")
        }
    }
}

enum Theme {
    static let bg = NSColor(srgbRed: 0.09, green: 0.10, blue: 0.12, alpha: 1)
    static let bar = NSColor(srgbRed: 0.07, green: 0.08, blue: 0.09, alpha: 1)
    static let accent = NSColor(srgbRed: 0.36, green: 0.62, blue: 1.0, alpha: 1)
    static let hover = NSColor.white.withAlphaComponent(0.07)
}

final class AppState {
    static let shared = AppState()
    private(set) var tabs: [Tab] = []
    private(set) var selected: UUID?
    let store = ServerStore()
    let awsStore = AWSStore()
    let tunnelStore = TunnelStore()
    var onChange: (() -> Void)?

    var current: Tab? { tabs.first { $0.id == selected } }

    func add(_ tab: Tab) {
        tab.onExit = { [weak self] t in self?.close(t) }
        tabs.append(tab)
        selected = tab.id
        onChange?()
    }
    func newLocal() { add(TerminalTab.local()) }

    /// Inicia o detiene un túnel guardado (un clic).
    func toggleTunnel(_ t: Tunnel, window: NSWindow? = nil) {
        switch TunnelManager.shared.state(of: t.id) {
        case .starting, .active:
            TunnelManager.shared.stop(t.id)
        case .stopped, .failed:
            if let e = t.validationError() { return showAlert("Túnel no válido", e, window: window) }
            guard let server = store.servers.first(where: { $0.id == t.serverId }) else {
                return showAlert("Falta el servidor", "El servidor de este túnel ya no existe. Edita el túnel y elige otro.", window: window)
            }
            TunnelManager.shared.start(t, server: server)
        }
    }
    func openS3(account: AWSAccount, credentials: @escaping () throws -> AWSCredentials) {
        add(S3Tab(account: account, credentials: credentials))
    }
    func openLogs(account: AWSAccount, region: String, credentials: @escaping () throws -> AWSCredentials) {
        add(LogsTab(account: account, region: region, credentials: credentials))
    }
    func connect(_ s: Server) {
        // El Keychain puede mostrar un diálogo de autorización: leerlo fuera del hilo principal.
        let servers = store.servers
        DispatchQueue.global(qos: .userInitiated).async {
            let kc = Keychain.read(s.id)
            let jump = SSHJump.setup(for: s, servers: servers)
            DispatchQueue.main.async { self.add(TerminalTab.ssh(s, keychain: kc, jump: jump)) }
        }
    }

    /// Abre el gestor de archivos (local | servidor) para el destino indicado.
    func openFiles(key: String?, startPath: String? = nil, _ make: @escaping () async throws -> SSHTarget) {
        add(FileTab(key: key, provider: make, startPath: startPath))
    }
    func openFilesForCurrent() {
        if let make = current?.fileTarget { openFiles(key: current?.connectionKey, make) }
    }

    /// ¿Hay alguna pestaña con conexión abierta a este servidor?
    func isConnected(key: String) -> Bool {
        tabs.contains { $0.connectionKey == key && $0.isConnected }
    }

    /// Cierra una pestaña pidiendo confirmación si tiene una conexión activa (según los ajustes).
    func requestClose(_ tab: Tab) {
        if Settings.shared.confirmClose, tab.isConnected {
            let a = NSAlert()
            a.messageText = "¿Cerrar “\(tab.title)”?"
            a.informativeText = "La conexión está activa y se cerrará."
            a.addButton(withTitle: "Cerrar"); a.addButton(withTitle: "Cancelar")
            guard a.runModal() == .alertFirstButtonReturn else { return }
        }
        close(tab)
    }

    func close(_ tab: Tab) {
        guard let i = tabs.firstIndex(where: { $0.id == tab.id }) else { return }
        tabs.remove(at: i)
        tab.shutdown()
        if tabs.isEmpty { NSApp.terminate(nil); return }
        if selected == tab.id { selected = tabs[min(i, tabs.count - 1)].id }
        onChange?()
    }
    func select(_ tab: Tab) { selected = tab.id; onChange?() }
    func step(_ d: Int) {
        guard let i = tabs.firstIndex(where: { $0.id == selected }) else { return }
        select(tabs[(i + d + tabs.count) % tabs.count])
    }
    func select(index: Int) { if tabs.indices.contains(index) { select(tabs[index]) } }
    func shutdownAll() { tabs.forEach { $0.shutdown() } }
}
