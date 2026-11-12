import Foundation
import Security

enum AuthMethod: String, Codable, CaseIterable, Identifiable {
    case password = "Contraseña"
    case key = "Llave (key)"
    var id: String { rawValue }
}

struct Server: Identifiable, Codable, Hashable {
    var id = UUID()
    var name = ""
    var host = ""
    var port = 22
    var user = ""
    var auth: AuthMethod = .password
    var keyPath = ""          // ruta a la llave privada (auth == .key)
}

/// Guarda secretos (contraseña / passphrase) en el Keychain de macOS.
enum Keychain {
    private static let service = "cl.enovus.terminal.servers"

    static func set(_ secret: String, for id: UUID) {
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: id.uuidString,
        ]
        SecItemDelete(base as CFDictionary)
        guard !secret.isEmpty else { return }
        var item = base
        item[kSecValueData as String] = Data(secret.utf8)
        SecItemAdd(item as CFDictionary, nil)
    }

    /// Devuelve el secreto y el OSStatus (errSecItemNotFound = no hay secreto guardado).
    static func read(_ id: UUID) -> (secret: String?, status: OSStatus) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: id.uuidString,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var out: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &out)
        guard status == errSecSuccess, let data = out as? Data else { return (nil, status) }
        return (String(data: data, encoding: .utf8), status)
    }

    static func get(_ id: UUID) -> String? { read(id).secret }

    static func delete(_ id: UUID) { set("", for: id) }
}

/// Lista de servidores en JSON (sin secretos) dentro de Application Support.
final class ServerStore {
    var servers: [Server] = [] { didSet { save() } }
    private var loading = true

    static var dir: URL {
        let d = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("jTerm", isDirectory: true)
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true,
                                                 attributes: [.posixPermissions: 0o700])
        return d
    }
    private static var file: URL { dir.appendingPathComponent("servers.json") }

    init() {
        if let data = try? Data(contentsOf: Self.file),
           let list = try? JSONDecoder().decode([Server].self, from: data) {
            servers = list
        }
        loading = false
    }

    private func save() {
        guard !loading, let data = try? JSONEncoder().encode(servers) else { return }
        try? data.write(to: Self.file, options: .atomic)
        NotificationCenter.default.post(name: .syncLocalChanged, object: nil)
    }

    /// Cambia la identidad de un servidor (al adoptar el mismo servidor creado en otro equipo), moviendo su contraseña.
    func rekey(_ old: UUID, to new: UUID) {
        guard let i = servers.firstIndex(where: { $0.id == old }) else { return }
        if let secret = Keychain.read(old).secret { Keychain.set(secret, for: new); Keychain.delete(old) }
        servers[i].id = new
    }

    func upsert(_ s: Server, secret: String?) {
        if let i = servers.firstIndex(where: { $0.id == s.id }) { servers[i] = s } else { servers.append(s) }
        if let secret { Keychain.set(secret, for: s.id) }
    }

    func remove(_ s: Server) {
        servers.removeAll { $0.id == s.id }
        Keychain.delete(s.id)
    }

    /// Script askpass: ssh lo invoca para obtener la contraseña desde una variable de entorno,
    /// así el secreto nunca aparece en la línea de comandos ni en disco.
    static func askpassScript() -> String {
        let url = dir.appendingPathComponent("askpass.sh")
        // Solo responde a prompts de contraseña (o a la passphrase de la llave configurada).
        // Cualquier otra pregunta (p. ej. confirmar el host) falla en vez de recibir la contraseña.
        let body = """
        #!/bin/sh
        case "$1" in
          *passphrase*)
            if [ -n "$TERMINAL_KEY" ]; then
              case "$1" in *"$TERMINAL_KEY"*) printf '%s\\n' "$TERMINAL_SECRET"; exit 0;; esac
            fi
            exit 1 ;;
          *assword*) printf '%s\\n' "$TERMINAL_SECRET" ;;
          *) exit 1 ;;
        esac

        """
        if (try? String(contentsOf: url)) != body {
            try? body.write(to: url, atomically: true, encoding: .utf8)
            try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
        }
        return url.path
    }
}
