import AppKit

// MARK: - Utilidades

func showAlert(_ title: String, _ text: String, window: NSWindow? = nil) {
    let a = NSAlert()
    a.messageText = title; a.informativeText = text; a.alertStyle = .warning
    a.addButton(withTitle: "OK")
    if let w = window ?? NSApp.keyWindow { a.beginSheetModal(for: w) } else { a.runModal() }
}

/// Persistencia de cuentas AWS: metadatos en JSON, clave secreta en el Keychain.
final class AWSStore {
    private(set) var accounts: [AWSAccount] = []
    private static var file: URL { ServerStore.dir.appendingPathComponent("aws.json") }

    init() {
        if let data = try? Data(contentsOf: Self.file),
           let list = try? JSONDecoder().decode([AWSAccount].self, from: data) { accounts = list }
    }
    private func save() {
        if let data = try? JSONEncoder().encode(accounts) { try? data.write(to: Self.file, options: .atomic) }
        NotificationCenter.default.post(name: .syncLocalChanged, object: nil)
    }
    func rekey(_ old: UUID, to new: UUID) {
        guard let i = accounts.firstIndex(where: { $0.id == old }) else { return }
        if let secret = Keychain.read(old).secret { Keychain.set(secret, for: new); Keychain.delete(old) }
        accounts[i].id = new; save()
    }
    func upsert(_ a: AWSAccount, secret: String?) {
        if let i = accounts.firstIndex(where: { $0.id == a.id }) { accounts[i] = a } else { accounts.append(a) }
        if let secret, !secret.isEmpty { Keychain.set(secret, for: a.id) }
        save()
    }
    func remove(_ a: AWSAccount) {
        accounts.removeAll { $0.id == a.id }
        Keychain.delete(a.id); save()
    }
    func credentials(_ a: AWSAccount) throws -> AWSCredentials {
        let (secret, status) = Keychain.read(a.id)
        guard let secret, !secret.isEmpty else {
            let msg = SecCopyErrorMessageString(status, nil) as String? ?? "OSStatus \(status)"
            throw AWSError(message: "No se pudo leer la clave secreta del Keychain: \(msg)")
        }
        return AWSCredentials(accessKey: a.accessKeyId, secret: secret)
    }
}

/// Llave SSH propia de jTerm, usada solo con EC2 Instance Connect (AWS la acepta 60 s por conexión).
enum EICKey {
    static func ensure() throws -> (path: String, pub: String) {
        let path = ServerStore.dir.appendingPathComponent("eic_ed25519").path
        if !FileManager.default.fileExists(atPath: path) {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/ssh-keygen")
            p.arguments = ["-q", "-t", "ed25519", "-N", "", "-C", "jterm-instance-connect", "-f", path]
            try p.run(); p.waitUntilExit()
            guard p.terminationStatus == 0 else { throw AWSError(message: "No se pudo generar la llave SSH (ssh-keygen).") }
        }
        guard let pub = try? String(contentsOfFile: path + ".pub").trimmingCharacters(in: .whitespacesAndNewlines) else {
            throw AWSError(message: "No se pudo leer la llave pública.")
        }
        return (path, pub)
    }
}

// MARK: - Panel AWS

final class AWSPanel: NSView, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    var store: AWSStore { AppState.shared.awsStore }
    private var instances: [EC2Instance] = []
    private let accountPopup = Dropdown(symbol: "cloud")
    private let regionPopup = Dropdown(symbol: "globe")
    private let status = NSTextField(wrappingLabelWithString: "")
    private let table = NSTableView()
    private var loadedOnce = false
    private var generation = 0
    var onEditAccount: ((AWSAccount?) -> Void)?

    /// Instancias ya listadas (las usa el buscador rápido) y la cuenta a la que pertenecen.
    var loadedInstances: [EC2Instance] { instances }
    var currentAccount: AWSAccount? { account }

    private var account: AWSAccount? {
        let i = accountPopup.selectedIndex
        return store.accounts.indices.contains(i) ? store.accounts[i] : nil
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        accountPopup.placeholder = "Sin cuentas"
        accountPopup.onChange = { [weak self] in self?.accountChanged() }
        regionPopup.items = ["Todas las regiones"] + AWSClient.regions
        regionPopup.separatorAfter = [0]
        regionPopup.onChange = { [weak self] in self?.refresh() }

        let more = IconButton(symbol: "ellipsis", tip: "Opciones de la cuenta", target: self, action: #selector(showMore(_:)))
        let add = IconButton(symbol: "plus", tip: "Agregar cuenta", target: self, action: #selector(addAccount))
        let refreshBtn = IconButton(symbol: "arrow.clockwise", tip: "Actualizar instancias", target: self, action: #selector(refresh))

        let cap1 = smallCaption("Cuenta"), cap2 = smallCaption("Región")
        let row1 = NSStackView(views: [accountPopup, more, add]); row1.spacing = 4
        let logsBtn = IconButton(symbol: "doc.text.magnifyingglass", tip: "Ver logs de CloudWatch", target: self, action: #selector(openLogsTapped))
        let row2 = NSStackView(views: [regionPopup, refreshBtn, logsBtn]); row2.spacing = 4
        accountPopup.setContentHuggingPriority(.defaultLow, for: .horizontal)
        regionPopup.setContentHuggingPriority(.defaultLow, for: .horizontal)

        status.font = .systemFont(ofSize: 11); status.textColor = .secondaryLabelColor
        status.maximumNumberOfLines = 4

        let col = NSTableColumn(identifier: .init("i")); col.resizingMask = .autoresizingMask
        table.addTableColumn(col)
        table.headerView = nil; table.backgroundColor = .clear; table.rowHeight = 44
        table.dataSource = self; table.delegate = self
        table.doubleAction = #selector(connectClicked); table.target = self
        let menu = NSMenu(); menu.delegate = self; table.menu = menu
        let scroll = NSScrollView()
        scroll.documentView = table; scroll.drawsBackground = false; scroll.hasVerticalScroller = true

        for v in [cap1, row1, cap2, row2, status, scroll] { v.translatesAutoresizingMaskIntoConstraints = false; addSubview(v) }
        NSLayoutConstraint.activate([
            cap1.topAnchor.constraint(equalTo: topAnchor, constant: 2),
            cap1.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
            row1.topAnchor.constraint(equalTo: cap1.bottomAnchor, constant: 5),
            row1.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            row1.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            cap2.topAnchor.constraint(equalTo: row1.bottomAnchor, constant: 12),
            cap2.leadingAnchor.constraint(equalTo: cap1.leadingAnchor),
            row2.topAnchor.constraint(equalTo: cap2.bottomAnchor, constant: 5),
            row2.leadingAnchor.constraint(equalTo: row1.leadingAnchor),
            row2.trailingAnchor.constraint(equalTo: row1.trailingAnchor),
            status.topAnchor.constraint(equalTo: row2.bottomAnchor, constant: 10),
            status.leadingAnchor.constraint(equalTo: cap1.leadingAnchor),
            status.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            scroll.topAnchor.constraint(equalTo: status.bottomAnchor, constant: 6),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        reloadAccounts()
    }
    required init?(coder: NSCoder) { fatalError() }

    // MARK: Cuentas

    /// Se llama al mostrar la pestaña. No consulta AWS por sí sola: el listado se pide con el botón de actualizar.
    func activate() {
        if account == nil { setStatus("Agrega una cuenta de AWS con +.") }
        else if !loadedOnce { setStatus("Pulsa ↻ para listar las instancias.") }
    }

    func reloadAccounts() {
        let selected = UserDefaults.standard.string(forKey: "aws.selected")
        accountPopup.items = store.accounts.map { $0.name.isEmpty ? $0.accessKeyId : $0.name }
        accountPopup.selectedIndex = store.accounts.isEmpty ? -1 : 0
        if let i = store.accounts.firstIndex(where: { $0.id.uuidString == selected }) { accountPopup.selectedIndex = i }
        accountChanged()
    }

    @objc private func accountChanged() {
        instances = []; table.reloadData(); loadedOnce = false; generation += 1
        guard let a = account else { setStatus("Agrega una cuenta de AWS con +."); return }
        UserDefaults.standard.set(a.id.uuidString, forKey: "aws.selected")
        regionPopup.select(title: a.region)
        setStatus("Pulsa ↻ para listar las instancias.")
    }

    @objc private func addAccount() { onEditAccount?(nil) }

    /// Abre la pestaña de logs de CloudWatch para la cuenta y la región elegidas. No llama a AWS hasta que se usa la pestaña.
    @objc private func openLogsTapped() {
        guard let acc = account else { setStatus("Agrega una cuenta de AWS con +.", error: true); return }
        let picked = regionPopup.titleOfSelected
        let region = (picked == nil || picked == "Todas las regiones") ? acc.region : picked!
        let store = self.store
        AppState.shared.openLogs(account: acc, region: region) { try store.credentials(acc) }
    }
    @objc private func showMore(_ b: NSButton) {
        let m = NSMenu()
        if account != nil {
            m.addItem(withTitle: "Editar cuenta…", action: #selector(editAccount), keyEquivalent: "").target = self
            m.addItem(withTitle: "Eliminar cuenta", action: #selector(removeAccount), keyEquivalent: "").target = self
        } else {
            m.addItem(withTitle: "Agregar cuenta…", action: #selector(addAccount), keyEquivalent: "").target = self
        }
        m.popUp(positioning: nil, at: NSPoint(x: 0, y: b.bounds.height), in: b)
    }
    @objc private func editAccount() { if let a = account { onEditAccount?(a) } }
    @objc private func removeAccount() {
        guard let a = account else { return }
        let alert = NSAlert()
        alert.messageText = "¿Eliminar la cuenta “\(a.name.isEmpty ? a.accessKeyId : a.name)”?"
        alert.informativeText = "Se borra también la clave secreta guardada en el Keychain."
        alert.addButton(withTitle: "Eliminar"); alert.addButton(withTitle: "Cancelar")
        if alert.runModal() == .alertFirstButtonReturn { store.remove(a); reloadAccounts() }
    }

    // MARK: Listado

    private func setStatus(_ s: String, error: Bool = false) {
        status.stringValue = s
        status.textColor = error ? NSColor.systemRed : .secondaryLabelColor
        status.isHidden = s.isEmpty
    }

    @objc func refresh() {
        guard let acc = account else { setStatus("Agrega una cuenta de AWS con +."); return }
        let regionTitle = regionPopup.titleOfSelected ?? acc.region
        let chosen = Settings.shared.awsRegions.filter { AWSClient.regions.contains($0) }
        let regions = regionTitle == "Todas las regiones" ? (chosen.isEmpty ? AWSClient.regions : chosen) : [regionTitle]
        generation += 1; let gen = generation
        setStatus("Cargando instancias…")
        let store = self.store
        Task.detached {
            do {
                let creds = try store.credentials(acc)
                let (found, errors) = await AWSPanel.fetch(creds, regions)
                await MainActor.run {
                    guard gen == self.generation else { return }
                    self.loadedOnce = true
                    self.instances = found.sorted {
                        ($0.state == "running" ? 0 : 1, $0.name.lowercased()) < ($1.state == "running" ? 0 : 1, $1.name.lowercased())
                    }
                    self.table.reloadData()
                    if found.isEmpty, let e = errors.first, errors.count == regions.count {
                        self.setStatus(e, error: true)
                    } else {
                        self.setStatus("\(found.count) instancia\(found.count == 1 ? "" : "s")")
                    }
                }
            } catch {
                await MainActor.run { if gen == self.generation { self.setStatus(error.localizedDescription, error: true) } }
            }
        }
    }

    private static func fetch(_ creds: AWSCredentials, _ regions: [String]) async -> ([EC2Instance], [String]) {
        await withTaskGroup(of: ([EC2Instance], String?).self) { group in
            for r in regions {
                group.addTask {
                    do { return (try await AWSClient.describeInstances(creds, region: r), nil) }
                    catch { return ([], error.localizedDescription) }
                }
            }
            var all: [EC2Instance] = []; var errs: [String] = []
            for await (list, err) in group { all += list; if let err { errs.append(err) } }
            return (all, errs)
        }
    }

    func reloadRows() { table.reloadData() }

    // MARK: Tabla

    func numberOfRows(in t: NSTableView) -> Int { instances.count }
    func tableView(_ t: NSTableView, heightOfRow row: Int) -> CGFloat {
        AppState.shared.isConnected(key: "aws:\(instances[row].id)") ? 62 : 44
    }

    func tableView(_ t: NSTableView, viewFor c: NSTableColumn?, row: Int) -> NSView? {
        let i = instances[row]
        let dot = NSView(); dot.wantsLayer = true; dot.layer?.cornerRadius = 4
        let color: NSColor = {
            switch i.state {
            case "running": return .systemGreen
            case "pending", "stopping", "shutting-down": return .systemOrange
            default: return .systemGray
            }
        }()
        dot.layer?.backgroundColor = color.cgColor
        dot.translatesAutoresizingMaskIntoConstraints = false
        dot.widthAnchor.constraint(equalToConstant: 8).isActive = true
        dot.heightAnchor.constraint(equalToConstant: 8).isActive = true
        let name = NSTextField(labelWithString: i.name)
        name.font = .systemFont(ofSize: 12, weight: .medium); name.lineBreakMode = .byTruncatingTail
        let nameRow = NSStackView(views: [name] + (account?.env.map { [EnvTag($0)] } ?? [])); nameRow.spacing = 6
        let ip = i.publicIP.isEmpty ? (i.privateIP.isEmpty ? i.state : i.privateIP + " (privada)") : i.publicIP
        let sub = NSTextField(labelWithString: "\(i.type) · \(ip)")
        sub.font = .systemFont(ofSize: 10); sub.textColor = .secondaryLabelColor; sub.lineBreakMode = .byTruncatingTail
        name.lineBreakMode = .byTruncatingTail; sub.lineBreakMode = .byTruncatingTail
        name.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        name.setContentCompressionResistancePriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        sub.setContentCompressionResistancePriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        let texts = NSStackView(views: [nameRow, sub]); texts.orientation = .vertical; texts.alignment = .leading; texts.spacing = 1
        if AppState.shared.isConnected(key: "aws:\(i.id)") {
            texts.addArrangedSubview(ConnectedBadge()); texts.setCustomSpacing(5, after: sub)
        }
        texts.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)   // ocupa todo el ancho libre
        let views: [NSView] = [dot, texts]
        let line = NSStackView(views: views); line.spacing = 9
        line.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 4, right: 8)
        return line
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard table.clickedRow >= 0 else { return }
        menu.addItem(withTitle: "Conectar", action: #selector(connectClicked), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Transferir archivos…", action: #selector(filesClicked), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Copiar IP pública", action: #selector(copyIP), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Copiar ID de instancia", action: #selector(copyID), keyEquivalent: "").target = self
    }
    @objc private func copyIP() {
        let r = table.clickedRow; guard r >= 0 else { return }
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(instances[r].publicIP, forType: .string)
    }
    @objc private func copyID() {
        let r = table.clickedRow; guard r >= 0 else { return }
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(instances[r].id, forType: .string)
    }

    // MARK: Conexión

    @objc private func filesClicked() {
        let r = table.clickedRow; if r >= 0 { openFiles(instances[r]) }
    }

    func openFiles(_ i: EC2Instance) {
        guard let acc = account else { return }
        guard i.state == "running" else {
            showAlert("La instancia no está en ejecución", "Estado actual: \(i.state).", window: window); return
        }
        guard !(i.publicDNS.isEmpty && i.publicIP.isEmpty) else {
            showAlert("Instancia sin IP pública", "La transferencia de archivos usa SSH y necesita una IP o DNS público.", window: window); return
        }
        guard let user = askUser(for: i) else { return }
        let store = self.store
        AppState.shared.openFiles(key: "aws:\(i.id)") { try await SSHTargets.aws(i, user: user, store: store, account: acc) }
    }

    @objc private func connectClicked() {
        let r = table.clickedRow; if r >= 0 { connect(instances[r]) }
    }

    private func askUser(for i: EC2Instance) -> String? {
        let key = "aws.user.\(i.id)"
        if let saved = UserDefaults.standard.string(forKey: key) { return saved }
        let combo = NSComboBox(frame: NSRect(x: 0, y: 0, width: 240, height: 26))
        combo.addItems(withObjectValues: ["ec2-user", "ubuntu", "admin", "root", "centos", "fedora", "bitnami"])
        combo.stringValue = UserDefaults.standard.string(forKey: "aws.lastUser") ?? Settings.shared.awsUser
        let a = NSAlert()
        a.messageText = "Usuario para \(i.name)"
        a.informativeText = "Usuario del sistema de la instancia: ec2-user (Amazon Linux), ubuntu (Ubuntu), admin (Debian)…"
        a.accessoryView = combo
        a.addButton(withTitle: "Conectar"); a.addButton(withTitle: "Cancelar")
        guard a.runModal() == .alertFirstButtonReturn else { return nil }
        let user = combo.stringValue.trimmingCharacters(in: .whitespaces)
        guard !user.isEmpty else { return nil }
        UserDefaults.standard.set(user, forKey: key); UserDefaults.standard.set(user, forKey: "aws.lastUser")
        return user
    }

    private func awsCLI() -> String? {
        ["/opt/homebrew/bin/aws", "/usr/local/bin/aws", "/usr/bin/aws"].first { FileManager.default.isExecutableFile(atPath: $0) }
    }
    private func ssmPlugin() -> Bool {
        ["/opt/homebrew/bin/session-manager-plugin", "/usr/local/bin/session-manager-plugin",
         "/usr/local/sessionmanagerplugin/bin/session-manager-plugin"].contains { FileManager.default.isExecutableFile(atPath: $0) }
    }

    func connect(_ i: EC2Instance) {
        guard let acc = account else { return }
        guard i.state == "running" else {
            showAlert("La instancia no está en ejecución", "Estado actual: \(i.state).", window: window); return
        }
        let store = self.store
        if !(i.publicDNS.isEmpty && i.publicIP.isEmpty) {
            // Instance Connect: llave temporal + SSH normal
            guard let user = askUser(for: i) else { return }
            setStatus("Conectando a \(i.name)…")
            Task.detached {
                do {
                    let creds = try store.credentials(acc)
                    let key = try EICKey.ensure()
                    try await AWSClient.sendSSHPublicKey(creds, region: i.region, instanceId: i.id, zone: i.zone,
                                                         user: user, publicKey: key.pub)
                    await MainActor.run {
                        let tab = TerminalTab.awsSSH(i, user: user, keyPath: key.path)
                        tab.env = acc.env
                        tab.fileTarget = { try await SSHTargets.aws(i, user: user, store: store, account: acc) }
                        AppState.shared.add(tab)
                        self.setStatus("\(self.instances.count) instancias")
                    }
                } catch {
                    await MainActor.run {
                        self.setStatus("")
                        UserDefaults.standard.removeObject(forKey: "aws.user.\(i.id)")
                        showAlert("No se pudo conectar a \(i.name)", error.localizedDescription, window: self.window)
                    }
                }
            }
        } else {
            // Sin IP pública: SSM Session Manager
            guard let cli = awsCLI(), ssmPlugin() else {
                showAlert("Instancia sin IP pública",
                          "Para conectarte necesitas el AWS CLI y el plugin session-manager-plugin instalados, y el agente SSM en la instancia.",
                          window: window)
                return
            }
            Task.detached {
                do {
                    let creds = try store.credentials(acc)
                    await MainActor.run { let tab = TerminalTab.awsSSM(i, creds: creds, awsPath: cli); tab.env = acc.env; AppState.shared.add(tab) }
                } catch {
                    await MainActor.run { showAlert("No se pudo conectar a \(i.name)", error.localizedDescription, window: self.window) }
                }
            }
        }
    }
}

// MARK: - Editor de cuenta AWS

final class AWSAccountEditor: NSObject {
    private let account: AWSAccount
    private let isNew: Bool
    private let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 460, height: 480),
                                styleMask: [.titled, .fullSizeContentView], backing: .buffered, defer: false)
    private let name = FocusField(), akid = FocusField(), secret = SecureFocusField(), region = FocusField()
    private var secretF: StyledField!
    private var envPicker: EnvPicker!
    private var done: () -> Void = {}

    init(account: AWSAccount?) {
        var fresh = AWSAccount(); fresh.region = Settings.shared.awsRegion
        self.account = account ?? fresh
        isNew = account == nil
        super.init()
        panel.appearance = NSAppearance(named: .darkAqua)
        panel.backgroundColor = Theme.bg
        panel.titlebarAppearsTransparent = true
        name.stringValue = self.account.name; akid.stringValue = self.account.accessKeyId
        region.stringValue = self.account.region

        let badge = NSView(); badge.wantsLayer = true; badge.layer?.cornerRadius = 10
        badge.layer?.backgroundColor = Theme.accent.withAlphaComponent(0.18).cgColor
        let sym = NSImageView(image: NSImage(systemSymbolName: "cloud", accessibilityDescription: nil)!)
        sym.symbolConfiguration = .init(pointSize: 16, weight: .medium); sym.contentTintColor = Theme.accent
        sym.translatesAutoresizingMaskIntoConstraints = false; badge.addSubview(sym)
        NSLayoutConstraint.activate([
            badge.widthAnchor.constraint(equalToConstant: 40), badge.heightAnchor.constraint(equalToConstant: 40),
            sym.centerXAnchor.constraint(equalTo: badge.centerXAnchor), sym.centerYAnchor.constraint(equalTo: badge.centerYAnchor)])
        let title = NSTextField(labelWithString: isNew ? "Nueva cuenta de AWS" : "Editar cuenta de AWS")
        title.font = .systemFont(ofSize: 17, weight: .semibold); title.textColor = .white
        let sub = NSTextField(labelWithString: "Credenciales IAM (access key)")
        sub.font = .systemFont(ofSize: 12); sub.textColor = .secondaryLabelColor
        let titles = NSStackView(views: [title, sub]); titles.orientation = .vertical; titles.alignment = .leading; titles.spacing = 1
        let header = NSStackView(views: [badge, titles]); header.spacing = 12

        let nameF = StyledField("Nombre", field: name, symbol: "tag", placeholder: "Producción")
        let akidF = StyledField("AWS_ACCESS_KEY_ID", field: akid, symbol: "person.badge.key", placeholder: "AKIA…")
        secretF = StyledField("AWS_SECRET_ACCESS_KEY", field: secret, symbol: "lock",
                                  placeholder: isNew ? "••••••••••••••••" : "Sin cambios", revealable: true)
        if !isNew { let id = self.account.id; secretF.loadSecret = { Keychain.read(id).secret } }
        let regionF = StyledField("Región por defecto", field: region, symbol: "globe", placeholder: "sa-east-1")
        envPicker = EnvPicker(selected: self.account.env)

        let lock = NSImageView(image: NSImage(systemSymbolName: "lock.shield", accessibilityDescription: nil)!)
        lock.symbolConfiguration = .init(pointSize: 11, weight: .regular); lock.contentTintColor = .tertiaryLabelColor
        let note = NSTextField(wrappingLabelWithString: "La clave secreta se guarda en el Keychain de macOS. Permisos IAM: ec2:DescribeInstances y ec2-instance-connect:SendSSHPublicKey.")
        note.font = .systemFont(ofSize: 11); note.textColor = .tertiaryLabelColor
        let noteRow = NSStackView(views: [lock, note]); noteRow.spacing = 5; noteRow.alignment = .top

        let cancel = PillButton("Cancelar", primary: false, target: self, action: #selector(cancel)); cancel.keyEquivalent = "\u{1b}"
        let save = PillButton("Guardar", primary: true, target: self, action: #selector(save)); save.keyEquivalent = "\r"
        let buttons = NSStackView(views: [NSView(), cancel, save]); buttons.spacing = 8

        let form = NSStackView()
        form.orientation = .vertical; form.alignment = .leading; form.spacing = 14
        form.edgeInsets = NSEdgeInsets(top: 28, left: 24, bottom: 22, right: 24)
        for v in [header, nameF, akidF, secretF!, regionF, envPicker!, noteRow, buttons] { form.addArrangedSubview(v) }
        form.setCustomSpacing(20, after: header); form.setCustomSpacing(20, after: noteRow)
        form.translatesAutoresizingMaskIntoConstraints = false
        panel.contentView = NSView(); panel.contentView!.addSubview(form)
        NSLayoutConstraint.activate([
            form.leadingAnchor.constraint(equalTo: panel.contentView!.leadingAnchor),
            form.trailingAnchor.constraint(equalTo: panel.contentView!.trailingAnchor),
            form.topAnchor.constraint(equalTo: panel.contentView!.topAnchor),
            form.widthAnchor.constraint(equalToConstant: 460),
        ])
        for v in [nameF, akidF, secretF!, regionF, envPicker!, noteRow, buttons] as [NSView] {
            v.widthAnchor.constraint(equalToConstant: 412).isActive = true
        }
        form.layoutSubtreeIfNeeded()
        panel.setContentSize(NSSize(width: 460, height: form.fittingSize.height))
    }

    func present(on window: NSWindow, done: @escaping () -> Void) {
        self.done = done
        window.beginSheet(panel) { _ in }
        panel.makeFirstResponder(name)
    }

    @objc private func cancel() { finish() }
    @objc private func save() {
        let key = akid.stringValue.trimmingCharacters(in: .whitespaces)
        let sec = secretF.secretValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !key.isEmpty else { NSSound.beep(); panel.makeFirstResponder(akid); return }
        guard !(isNew && sec.isEmpty) else { NSSound.beep(); panel.makeFirstResponder(secret); return }
        var a = account
        a.name = name.stringValue; a.accessKeyId = key
        let r = region.stringValue.trimmingCharacters(in: .whitespaces)
        a.region = r.isEmpty ? "us-east-1" : r
        a.env = envPicker.value
        AppState.shared.awsStore.upsert(a, secret: sec)
        finish()
    }
    private func finish() { panel.sheetParent?.endSheet(panel); panel.orderOut(nil); done() }
}
