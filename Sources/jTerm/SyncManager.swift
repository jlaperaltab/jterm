import AppKit

/// Sincroniza conexiones, cuentas AWS, comandos y ajustes mediante un archivo cifrado en iCloud Drive.
final class SyncManager: SyncLocalAdapter {
    static let shared = SyncManager()
    /// Para pruebas: usa esta carpeta en vez de iCloud Drive.
    static var folderOverride: URL?

    private let d = UserDefaults.standard
    private let queue = DispatchQueue(label: "jterm.sync", qos: .utility)
    private static let passphraseID = UUID(uuidString: "5E7C0000-0000-4000-8000-00000000A5C1")!

    // Estado visible (hilo principal)
    private(set) var isSyncing = false
    private(set) var lastSync: Date?
    private(set) var lastError: String?
    private(set) var needsPassphrase = false
    private var applying = false
    private var pendingRerun = false
    private var debounce: DispatchWorkItem?
    private var timer: Timer?
    private var lastActivationSync = Date.distantPast

    // MARK: Preferencias de la sincronización (no se sincronizan)

    private func flag(_ k: String, _ def: Bool) -> Bool { d.object(forKey: "sync." + k) == nil ? def : d.bool(forKey: "sync." + k) }
    var enabled: Bool { get { flag("enabled", false) } set { d.set(newValue, forKey: "sync.enabled") } }
    var syncServers: Bool { get { flag("servers", true) } set { d.set(newValue, forKey: "sync.servers") } }
    var syncAWS: Bool { get { flag("aws", true) } set { d.set(newValue, forKey: "sync.aws") } }
    var syncCommands: Bool { get { flag("commands", true) } set { d.set(newValue, forKey: "sync.commands") } }
    var syncSettings: Bool { get { flag("settings", true) } set { d.set(newValue, forKey: "sync.settings") } }
    var syncSecrets: Bool { get { flag("secrets", false) } set { d.set(newValue, forKey: "sync.secrets") } }

    var enabledKinds: Set<String> {
        var s = Set<String>()
        if syncServers { s.insert(SyncKind.server) }
        if syncAWS { s.insert(SyncKind.aws) }
        if syncCommands { s.insert(SyncKind.command) }
        if syncSettings { s.insert(SyncKind.settings) }
        return s
    }

    var passphrase: String? {
        get { Keychain.read(Self.passphraseID).secret }
        set { if let v = newValue { Keychain.set(v, for: Self.passphraseID) } else { Keychain.delete(Self.passphraseID) } }
    }

    static func iCloudFolder() -> URL? {
        if let o = folderOverride { return o }
        let base = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Mobile Documents/com~apple~CloudDocs")
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: base.path, isDirectory: &isDir), isDir.boolValue else { return nil }
        return base.appendingPathComponent("jTerm", isDirectory: true)
    }
    var storage: SyncStorage? { Self.iCloudFolder().map(SyncStorage.init) }

    // MARK: Estado persistido

    private var stateURL: URL { ServerStore.dir.appendingPathComponent("sync-state.json") }
    private func loadState() -> SyncState {
        (try? Data(contentsOf: stateURL)).flatMap { try? JSONDecoder().decode(SyncState.self, from: $0) } ?? SyncState()
    }
    private func saveState(_ s: SyncState) {
        guard let data = try? JSONEncoder().encode(s) else { return }
        try? data.write(to: stateURL, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: stateURL.path)
    }
    func resetState() { try? FileManager.default.removeItem(at: stateURL) }

    // MARK: Ciclo de vida

    func start() {
        NotificationCenter.default.addObserver(forName: .syncLocalChanged, object: nil, queue: .main) { [weak self] _ in
            guard let self, self.enabled, !self.applying else { return }
            self.scheduleSync(after: 3)
        }
        NotificationCenter.default.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            guard let self, self.enabled, Date().timeIntervalSince(self.lastActivationSync) > 20 else { return }
            self.lastActivationSync = Date(); self.scheduleSync(after: 1)
        }
        timer = Timer.scheduledTimer(withTimeInterval: 120, repeats: true) { [weak self] _ in
            if self?.enabled == true { self?.syncNow() }
        }
        if enabled { scheduleSync(after: 2) }
    }

    func scheduleSync(after delay: TimeInterval) {
        debounce?.cancel()
        let w = DispatchWorkItem { [weak self] in self?.syncNow() }
        debounce = w
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: w)
    }

    private func notify() { NotificationCenter.default.post(name: .syncStatusChanged, object: nil) }

    // MARK: Sincronizar

    func syncNow(completion: ((Result<SyncResult, Error>) -> Void)? = nil) {
        guard enabled else { completion?(.failure(SyncError.noFolder)); return }
        if isSyncing { pendingRerun = true; return }
        guard let storage, let pass = passphrase else {
            needsPassphrase = passphrase == nil
            lastError = storage == nil ? SyncError.noFolder.localizedDescription : "Falta la contraseña de sincronización."
            notify(); completion?(.failure(SyncError.noFolder)); return
        }
        isSyncing = true; notify()
        let state = loadState(), kinds = enabledKinds, secrets = syncSecrets
        queue.async { [weak self] in
            guard let self else { return }
            let outcome = Result { try SyncRunner.run(adapter: self, storage: storage, passphrase: pass, state: state,
                                                      enabled: kinds, manageSecrets: secrets) }
            DispatchQueue.main.async {
                self.isSyncing = false
                switch outcome {
                case .success(let (res, newState)):
                    self.saveState(newState); self.lastSync = Date(); self.lastError = nil; self.needsPassphrase = false
                    completion?(.success(res))
                case .failure(let e):
                    self.lastError = e.localizedDescription
                    self.needsPassphrase = (e as? SyncError).map { if case .badPassphrase = $0 { return true } else { return false } } ?? false
                    completion?(.failure(e))
                }
                self.notify()
                if self.pendingRerun { self.pendingRerun = false; self.scheduleSync(after: 1) }
            }
        }
    }

    // MARK: Adaptador: lectura y escritura de los datos locales (se ejecuta en un hilo de fondo)

    private static let syncedPrefs = [
        "loginShell", "confirmClose", "confirmQuit", "sidebarAtLaunch", "showSnippetsButton", "theme", "fontFamily", "fontSize",
        "cursorShape", "cursorBlink", "scrollback", "optionAsMeta", "mouseReporting", "backspaceCtrlH", "term", "confirmMultilinePaste", "warnDangerousProd", "autoRecordProd", "underlineLinks",
        "sshKeepAlive", "sshHostKey", "sshCompression", "sshForwardAgent", "sshExtra", "defaultUser", "defaultPort",
        "filesLocalHidden", "filesRemoteHidden", "filesConfirmDelete", "transferDismiss", "awsRegion", "awsUser", "awsRegions",
    ]   // se excluyen a propósito los específicos de cada Mac: shell, carpetas iniciales

    private func jsonValue(_ any: Any) -> JSONValue? {
        if let n = any as? NSNumber {
            if CFGetTypeID(n) == CFBooleanGetTypeID() { return .bool(n.boolValue) }
            return n.doubleValue == Double(n.intValue) ? .int(n.intValue) : .double(n.doubleValue)
        }
        if let s = any as? String { return .string(s) }
        if let a = any as? [Any] { return .array(a.compactMap(jsonValue)) }
        return nil
    }
    private func anyValue(_ v: JSONValue) -> Any {
        switch v {
        case .string(let s): return s
        case .int(let i): return i
        case .double(let x): return x
        case .bool(let b): return b
        case .array(let a): return a.map(anyValue)
        }
    }

    func snapshot(enabled: Set<String>, includeSecrets: Bool) -> [String: SyncItem] {
        var items: [String: SyncItem] = [:]
        var secretJobs: [(String, UUID)] = []
        DispatchQueue.main.sync {
            let st = AppState.shared
            if enabled.contains(SyncKind.server) {
                for s in st.store.servers {
                    let key = "server:\(s.id.uuidString)"
                    var fields: [String: JSONValue] = ["name": .string(s.name), "host": .string(s.host), "port": .int(s.port), "user": .string(s.user),
                                                       "auth": .string(s.auth == .key ? "key" : "password"), "keyPath": .string(s.keyPath)]
                    if let e = s.env { fields["env"] = .string(e.rawValue) }
                    if let j = s.jump { fields["jump"] = .string(j.uuidString) }
                    items[key] = SyncItem(fields: fields,
                                          secret: nil, naturalKey: "\(s.host.lowercased())|\(s.user)|\(s.port)")
                    secretJobs.append((key, s.id))
                }
            }
            if enabled.contains(SyncKind.aws) {
                for a in st.awsStore.accounts {
                    let key = "aws:\(a.id.uuidString)"
                    var fields: [String: JSONValue] = ["name": .string(a.name), "accessKeyId": .string(a.accessKeyId), "region": .string(a.region)]
                    if let e = a.env { fields["env"] = .string(e.rawValue) }
                    items[key] = SyncItem(fields: fields,
                                          secret: nil, naturalKey: a.accessKeyId)
                    secretJobs.append((key, a.id))
                }
            }
            if enabled.contains(SyncKind.command) {
                for c in SnippetStore.shared.items {
                    items["cmd:\(c.id.uuidString)"] = SyncItem(fields: ["name": .string(c.name), "command": .string(c.command), "autoRun": .bool(c.autoRun)],
                                                               secret: nil, naturalKey: "\(c.name)|\(c.command)")
                }
            }
            if enabled.contains(SyncKind.settings) {
                var f: [String: JSONValue] = [:]
                for k in Self.syncedPrefs { if let v = d.object(forKey: "pref." + k).flatMap(jsonValue) { f[k] = v } }
                if !f.isEmpty { items["settings:main"] = SyncItem(fields: f) }
            }
        }
        if includeSecrets {                                    // el Keychain se lee fuera del hilo principal
            for (key, id) in secretJobs { if let sec = Keychain.read(id).secret, !sec.isEmpty { items[key]?.secret = sec } }
        }
        return items
    }

    func rekey(_ map: [String: String]) {
        DispatchQueue.main.sync {
            applying = true; defer { applying = false }
            for (old, new) in map {
                guard let o = UUID(uuidString: String(old.drop(while: { $0 != ":" }).dropFirst())),
                      let n = UUID(uuidString: String(new.drop(while: { $0 != ":" }).dropFirst())) else { continue }
                switch SyncKind.of(old) {
                case SyncKind.server: AppState.shared.store.rekey(o, to: n)
                case SyncKind.aws: AppState.shared.awsStore.rekey(o, to: n)
                case SyncKind.command: SnippetStore.shared.rekey(o, to: n)
                default: break
                }
            }
        }
    }

    func apply(upserts: [String: SyncItem], deletes: [String], manageSecrets: Bool) {
        guard !upserts.isEmpty || !deletes.isEmpty else { return }
        DispatchQueue.main.sync {
            applying = true; defer { applying = false }
            let st = AppState.shared
            func id(_ key: String) -> UUID? { UUID(uuidString: String(key.drop(while: { $0 != ":" }).dropFirst())) }
            for (key, item) in upserts {
                let f = item.fields
                switch SyncKind.of(key) {
                case SyncKind.server:
                    guard let uid = id(key) else { continue }
                    var s = st.store.servers.first { $0.id == uid } ?? Server()
                    s.id = uid; s.name = f["name"]?.string ?? ""; s.host = f["host"]?.string ?? ""; s.port = f["port"]?.int ?? 22
                    s.user = f["user"]?.string ?? ""; s.auth = f["auth"]?.string == "key" ? .key : .password; s.keyPath = f["keyPath"]?.string ?? ""
                    s.env = f["env"]?.string.flatMap(ServerEnv.init(rawValue:))
                    s.jump = f["jump"]?.string.flatMap(UUID.init(uuidString:))
                    st.store.upsert(s, secret: manageSecrets ? (item.secret ?? "") : nil)
                case SyncKind.aws:
                    guard let uid = id(key) else { continue }
                    var a = st.awsStore.accounts.first { $0.id == uid } ?? AWSAccount()
                    a.id = uid; a.name = f["name"]?.string ?? ""; a.accessKeyId = f["accessKeyId"]?.string ?? ""; a.region = f["region"]?.string ?? "us-east-1"
                    a.env = f["env"]?.string.flatMap(ServerEnv.init(rawValue:))
                    st.awsStore.upsert(a, secret: manageSecrets ? item.secret : nil)
                case SyncKind.command:
                    guard let uid = id(key) else { continue }
                    SnippetStore.shared.upsert(Snippet(id: uid, name: f["name"]?.string ?? "", command: f["command"]?.string ?? "", autoRun: f["autoRun"]?.bool ?? false))
                case SyncKind.settings:
                    for k in Self.syncedPrefs {
                        if let v = f[k] { d.set(anyValue(v), forKey: "pref." + k) } else { d.removeObject(forKey: "pref." + k) }
                    }
                    NotificationCenter.default.post(name: Settings.didChange, object: nil)
                default: break
                }
            }
            for key in deletes {
                guard let uid = id(key) else { continue }
                switch SyncKind.of(key) {
                case SyncKind.server: if let s = st.store.servers.first(where: { $0.id == uid }) { st.store.remove(s) }
                case SyncKind.aws: if let a = st.awsStore.accounts.first(where: { $0.id == uid }) { st.awsStore.remove(a) }
                case SyncKind.command: SnippetStore.shared.remove(uid)
                default: break
                }
            }
            NotificationCenter.default.post(name: .jtermDataChanged, object: nil)
        }
    }

    // MARK: Acciones desde los ajustes

    /// Cambia la contraseña de sincronización: vuelve a cifrar el archivo con la nueva.
    func changePassphrase(to new: String) throws {
        guard let storage, let old = passphrase else { throw SyncError.noFolder }
        let (doc, _) = try storage.read(passphrase: old)
        if let doc { try storage.write(doc, passphrase: new) }
        passphrase = new
        needsPassphrase = false; lastError = nil; notify()
    }

    /// Desactiva la sincronización en este Mac (los datos locales no se tocan).
    func disable() {
        enabled = false; passphrase = nil; resetState()
        lastError = nil; needsPassphrase = false; lastSync = nil; notify()
    }
}
