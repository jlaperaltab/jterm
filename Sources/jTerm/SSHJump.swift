import AppKit

/// Lo que hay que añadir a una conexión SSH para pasar por uno o más servidores intermedios (ProxyJump / bastión).
struct JumpSetup {
    var options: [String] = []
    var env: [String: String] = [:]
    var isEmpty: Bool { options.isEmpty }
}

/// Saltos SSH: un servidor guardado puede llegar a su destino a través de otro servidor guardado.
///
/// Se usa `-o ProxyCommand="ssh -W %h:%p …"` en vez de `-J` porque ssh no pasa al salto las opciones de la línea de comandos
/// (`-i`, contraseña por askpass…): así cada salto usa su propia llave y su propia contraseña del Keychain.
enum SSHJump {
    static let maxHops = 4
    /// Ruta de ssh; las pruebas la cambian por un ssh de mentira para ver los argumentos.
    static var sshPath = "/usr/bin/ssh"

    /// Entrecomilla para el shell: el ProxyCommand lo ejecuta `sh -c`.
    static func shq(_ s: String) -> String { "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'" }

    static func destination(_ s: Server) -> String { s.user.isEmpty ? s.host : "\(s.user)@\(s.host)" }
    private static func keyPath(_ s: Server) -> String? {
        s.auth == .key && !s.keyPath.isEmpty ? (s.keyPath as NSString).expandingTildeInPath : nil
    }

    /// Los saltos de un servidor, del más cercano al destino al más lejano. Corta si hay un ciclo (A → B → A).
    static func hops(for s: Server, in servers: [Server]) -> [Server] {
        var out: [Server] = [], seen: Set<UUID> = [s.id]
        var cur = s
        while out.count < maxHops, let id = cur.jump, !seen.contains(id), let j = servers.first(where: { $0.id == id }) {
            out.append(j); seen.insert(id); cur = j
        }
        return out
    }

    /// El comando que usa ssh para llegar al destino pasando por `hops[0]` (que a su vez pasa por `hops[1]`…).
    static func proxyCommand(hops: [Server]) -> String? {
        guard let first = hops.first else { return nil }
        var args = [sshPath, "-W", "%h:%p", "-p", String(first.port)] + SSHSettings.args()
        if let key = keyPath(first) { args += ["-i", key, "-o", "IdentitiesOnly=yes"] }
        if let nested = proxyCommand(hops: Array(hops.dropFirst())) { args += ["-o", "ProxyCommand=" + nested] }
        args.append(destination(first))
        return args.map(shq).joined(separator: " ")
    }

    /// Opciones y entorno (con las contraseñas de los saltos para askpass) para conectar a `s`.
    /// Lee el Keychain: llamarlo fuera del hilo principal.
    static func setup(for s: Server, servers: [Server]) -> JumpSetup {
        let hs = hops(for: s, in: servers)
        guard !hs.isEmpty, let cmd = proxyCommand(hops: hs) else { return JumpSetup() }
        var env: [String: String] = [:]
        var anySecret = false
        for (i, h) in hs.enumerated() {
            let n = i + 1
            env["TERMINAL_JUMP_\(n)_HOST"] = destination(h)
            if let key = keyPath(h) { env["TERMINAL_JUMP_\(n)_KEY"] = key }
            if let secret = Keychain.read(h.id).secret, !secret.isEmpty { env["TERMINAL_JUMP_\(n)_SECRET"] = secret; anySecret = true }
        }
        if anySecret { env["SSH_ASKPASS"] = ServerStore.askpassScript(); env["SSH_ASKPASS_REQUIRE"] = "force" }
        return JumpSetup(options: ["-o", "ProxyCommand=" + cmd], env: env)
    }
}

/// Selector de servidor de salto para el editor: Ninguno + los demás servidores guardados.
final class JumpPicker: NSStackView {
    private let dropdown = Dropdown(symbol: "arrow.triangle.turn.up.right.diamond")
    private var candidates: [Server] = []

    init(current: UUID?, excluding id: UUID, servers: [Server]) {
        super.init(frame: .zero)
        candidates = servers.filter { $0.id != id }
        let caption = NSTextField(labelWithString: "SALTO (PROXYJUMP)")
        caption.font = .systemFont(ofSize: 10, weight: .semibold); caption.textColor = .tertiaryLabelColor
        dropdown.items = ["Ninguno (conexión directa)"] + candidates.map { $0.name.isEmpty ? $0.host : $0.name }
        dropdown.selectedIndex = current.flatMap { c in candidates.firstIndex { $0.id == c } }.map { $0 + 1 } ?? 0
        orientation = .vertical; alignment = .leading; spacing = 5
        addArrangedSubview(caption); addArrangedSubview(dropdown)
        dropdown.widthAnchor.constraint(equalTo: widthAnchor).isActive = true
    }
    required init(coder: NSCoder) { fatalError() }

    var value: UUID? {
        let i = dropdown.selectedIndex
        return i >= 1 && i <= candidates.count ? candidates[i - 1].id : nil
    }
}
