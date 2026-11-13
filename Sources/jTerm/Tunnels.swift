import Foundation

extension Notification.Name {
    static let tunnelsDidChange = Notification.Name("jterm.tunnels.didChange")           // cambió la lista guardada
    static let tunnelStateChanged = Notification.Name("jterm.tunnels.stateChanged")      // un túnel cambió de estado
}

/// Un túnel SSH guardado: reenvío de un puerto (-L) o reenvío inverso (-R) a través de un servidor guardado.
struct Tunnel: Codable, Identifiable, Hashable {
    enum Kind: String, Codable, CaseIterable {
        case local, remote
        var title: String { self == .local ? "Local (-L)" : "Remoto (-R)" }
    }
    var id = UUID()
    var name = ""
    var kind: Kind = .local
    /// Local: puerto que se abre en este Mac. Remoto: puerto que se abre en el servidor.
    var listenPort = 0
    /// Local: host visto desde el servidor (p. ej. una base de datos interna). Remoto: host visto desde este Mac.
    var destHost = "localhost"
    var destPort = 0
    var serverId: UUID?

    private var host: String { destHost.contains(":") && !destHost.hasPrefix("[") ? "[\(destHost)]" : destHost }

    /// Argumento de -L o -R. Local escucha solo en 127.0.0.1: nadie más de tu red puede usar el túnel.
    var forwardSpec: String {
        kind == .local ? "127.0.0.1:\(listenPort):\(host):\(destPort)" : "\(listenPort):\(host):\(destPort)"
    }
    var flag: String { kind == .local ? "-L" : "-R" }
    var summary: String {
        kind == .local ? "localhost:\(listenPort) → \(destHost):\(destPort)" : "servidor:\(listenPort) → \(destHost):\(destPort)"
    }
    var title: String { name.isEmpty ? summary : name }

    /// Texto del problema si los datos no sirven, o nil si están bien.
    func validationError() -> String? {
        if !(1...65535).contains(listenPort) { return "El puerto debe estar entre 1 y 65535." }
        if !(1...65535).contains(destPort) { return "El puerto de destino debe estar entre 1 y 65535." }
        if destHost.trimmingCharacters(in: .whitespaces).isEmpty || destHost.contains(" ") { return "Indica el host de destino (sin espacios)." }
        if kind == .local, listenPort < 1024, getuid() != 0 { return "Los puertos menores a 1024 solo los puede abrir el administrador. Usa uno mayor." }
        if serverId == nil { return "Elige el servidor por el que pasa el túnel." }
        return nil
    }
}

final class TunnelStore {
    private(set) var tunnels: [Tunnel] = []
    private let url: URL

    init(url: URL = ServerStore.dir.appendingPathComponent("tunnels.json")) {
        self.url = url
        if let data = try? Data(contentsOf: url), let list = try? JSONDecoder().decode([Tunnel].self, from: data) { tunnels = list }
    }
    private func save() {
        let enc = JSONEncoder(); enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        if let data = try? enc.encode(tunnels) {
            try? data.write(to: url, options: .atomic)
            try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        }
        NotificationCenter.default.post(name: .tunnelsDidChange, object: nil)
    }
    func upsert(_ t: Tunnel) {
        if let i = tunnels.firstIndex(where: { $0.id == t.id }) { tunnels[i] = t } else { tunnels.append(t) }
        save()
    }
    func remove(_ t: Tunnel) { tunnels.removeAll { $0.id == t.id }; save() }
}

/// Ejecuta los túneles: un proceso `ssh -N -L/-R …` por cada uno. Todo ocurre en el hilo principal.
final class TunnelManager {
    static let shared = TunnelManager()
    /// Ruta de ssh; las pruebas la cambian por un ssh de mentira.
    static var sshPath = "/usr/bin/ssh"
    /// Segundos que ssh debe seguir vivo tras conectar para dar el túnel por activo (si el reenvío falla, ssh termina enseguida).
    static var settleDelay = 1.0

    enum State: Equatable { case stopped, starting, active, failed(String) }

    private var states: [UUID: State] = [:]
    private var procs: [UUID: Process] = [:]
    private var stderrTail: [UUID: String] = [:]
    private var stoppedByUser: Set<UUID> = []

    func state(of id: UUID) -> State { states[id] ?? .stopped }
    var activeCount: Int { states.values.filter { $0 == .active || $0 == .starting }.count }

    private func set(_ id: UUID, _ s: State) {
        states[id] = s
        NotificationCenter.default.post(name: .tunnelStateChanged, object: nil, userInfo: ["id": id])
    }

    static func arguments(for t: Tunnel, target: SSHTarget) -> [String] {
        ["-N", "-T", "-o", "ExitOnForwardFailure=yes", "-o", "ConnectTimeout=15",
         "-o", "PermitLocalCommand=yes", "-o", "LocalCommand=echo jterm-tunnel-up", "-p", String(target.port)]
            + SSHSettings.args() + target.options + [t.flag, t.forwardSpec, target.destination]
    }

    /// Inicia el túnel pasando por `server`. Lee el Keychain fuera del hilo principal.
    func start(_ t: Tunnel, server: Server) {
        guard procs[t.id] == nil, states[t.id] != .starting else { return }
        stoppedByUser.remove(t.id)
        set(t.id, .starting)
        Task {
            let target = await SSHTargets.server(server)
            await MainActor.run { self.launch(t, target: target) }
        }
    }

    func launch(_ t: Tunnel, target: SSHTarget) {
        set(t.id, .starting)
        let p = Process()
        p.executableURL = URL(fileURLWithPath: Self.sshPath)
        p.arguments = Self.arguments(for: t, target: target)
        var env = ProcessInfo.processInfo.environment
        for (k, v) in target.env { env[k] = v }
        p.environment = env
        let out = Pipe(), err = Pipe()
        p.standardInput = FileHandle.nullDevice; p.standardOutput = out; p.standardError = err
        stderrTail[t.id] = ""
        out.fileHandleForReading.readabilityHandler = { [weak self] h in
            let d = h.availableData
            if d.isEmpty { h.readabilityHandler = nil; return }
            if String(decoding: d, as: UTF8.self).contains("jterm-tunnel-up") { DispatchQueue.main.async { self?.connected(t.id) } }
        }
        err.fileHandleForReading.readabilityHandler = { [weak self] h in
            let d = h.availableData
            if d.isEmpty { h.readabilityHandler = nil; return }
            let s = String(decoding: d, as: UTF8.self)
            DispatchQueue.main.async { self?.stderrTail[t.id] = String(((self?.stderrTail[t.id] ?? "") + s).suffix(2000)) }
        }
        p.terminationHandler = { [weak self] proc in
            DispatchQueue.main.async {
                // Da un momento para que llegue lo último que ssh escribió antes de salir
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { self?.ended(t, proc) }
            }
        }
        do { try p.run() } catch {
            set(t.id, .failed("No se pudo ejecutar ssh: \(error.localizedDescription)")); return
        }
        procs[t.id] = p
    }

    private func connected(_ id: UUID) {
        guard states[id] == .starting, let p = procs[id] else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.settleDelay) { [weak self] in
            guard let self, self.states[id] == .starting, self.procs[id] === p, p.isRunning else { return }
            self.set(id, .active)
        }
    }

    private func ended(_ t: Tunnel, _ proc: Process) {
        guard procs[t.id] === proc else { return }
        procs[t.id] = nil
        if stoppedByUser.remove(t.id) != nil { set(t.id, .stopped); return }
        let was = states[t.id]
        set(t.id, .failed(Self.explain(stderr: stderrTail[t.id] ?? "", tunnel: t, status: proc.terminationStatus, wasActive: was == .active)))
    }

    func stop(_ id: UUID) {
        guard let p = procs[id] else { set(id, .stopped); return }
        stoppedByUser.insert(id)
        p.terminate()
    }

    func stopAll() {
        for id in Array(procs.keys) { stop(id) }
    }

    /// Mensaje comprensible a partir de lo que ssh escribió al fallar.
    static func explain(stderr: String, tunnel t: Tunnel, status: Int32, wasActive: Bool) -> String {
        let low = stderr.lowercased()
        if low.contains("address already in use") || low.contains("cannot listen") && low.contains("port") {
            return "El puerto \(t.listenPort) ya está en uso\(t.kind == .local ? " en este Mac" : " en el servidor"). Cierra lo que lo usa o elige otro."
        }
        if low.contains("remote port forwarding failed") { return "El servidor rechazó el reenvío remoto del puerto \(t.listenPort) (¿ocupado o no permitido?)." }
        if low.contains("administratively prohibited") || low.contains("open failed") { return "El servidor no permite este reenvío (AllowTcpForwarding)." }
        if low.contains("permission denied") { return "El servidor rechazó la autenticación. Revisa la contraseña guardada o la llave." }
        if low.contains("connection refused") { return "Conexión rechazada por el servidor." }
        if low.contains("timed out") || low.contains("no route to host") { return "No se pudo llegar al servidor (tiempo agotado)." }
        if low.contains("could not resolve") || low.contains("name or service not known") { return "No se encontró el servidor (revisa el host)." }
        if low.contains("host key verification failed") { return "No se pudo verificar la llave del servidor (revisa known_hosts)." }
        let last = stderr.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }.last { !$0.isEmpty }
        if let last { return last }
        return wasActive ? "La conexión del túnel se cerró." : "ssh terminó con el código \(status)."
    }
}
