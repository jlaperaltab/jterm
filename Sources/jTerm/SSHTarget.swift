import Foundation

/// Destino SSH ya resuelto (opciones, entorno con askpass, etc.) que usa el módulo de transferencia.
struct SSHTarget {
    var title: String
    var user: String
    var host: String
    var port: Int = 22
    var options: [String] = []
    var env: [String: String] = [:]
    var destination: String { user.isEmpty ? host : "\(user)@\(host)" }
}

enum SSHTargets {
    /// Servidor guardado (contraseña o llave).
    static func server(_ s: Server) async -> SSHTarget {
        var t = SSHTarget(title: s.name.isEmpty ? s.host : s.name, user: s.user, host: s.host, port: s.port)
        if s.auth == .key, !s.keyPath.isEmpty {
            let path = (s.keyPath as NSString).expandingTildeInPath
            t.options += ["-i", path, "-o", "IdentitiesOnly=yes"]
            t.env["TERMINAL_KEY"] = path
        }
        // El Keychain puede mostrar un diálogo: leerlo fuera del hilo principal.
        let servers = await MainActor.run { AppState.shared.store.servers }
        let (kc, jump) = await Task.detached { (Keychain.read(s.id), SSHJump.setup(for: s, servers: servers)) }.value
        t.options += jump.options
        for (k, v) in jump.env { t.env[k] = v }
        if let secret = kc.secret {
            t.env["SSH_ASKPASS"] = ServerStore.askpassScript()
            t.env["SSH_ASKPASS_REQUIRE"] = "force"
            t.env["TERMINAL_SECRET"] = secret
        }
        return t
    }

    /// Instancia EC2: empuja la llave temporal (Instance Connect) y devuelve el destino.
    static func aws(_ i: EC2Instance, user: String, store: AWSStore, account: AWSAccount) async throws -> SSHTarget {
        let (creds, key) = try await Task.detached { (try store.credentials(account), try EICKey.ensure()) }.value
        try await AWSClient.sendSSHPublicKey(creds, region: i.region, instanceId: i.id, zone: i.zone,
                                             user: user, publicKey: key.pub)
        var t = SSHTarget(title: i.name, user: user, host: i.publicDNS.isEmpty ? i.publicIP : i.publicDNS)
        t.options = ["-i", key.path, "-o", "IdentitiesOnly=yes", "-o", "HostKeyAlias=\(i.id)"]
        return t
    }
}
