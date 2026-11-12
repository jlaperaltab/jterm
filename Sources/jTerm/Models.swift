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
    var env: ServerEnv?       // entorno (producción, staging…); nil = sin etiqueta
}

/// Guarda secretos (contraseña / passphrase / clave secreta AWS) en el Keychain de macOS.
///
/// Todos los secretos viven en UN solo elemento del Keychain (un JSON {id: secreto}). Antes había uno por servidor y cada
/// elemento tiene su propia lista de permisos, así que macOS pedía la clave una vez por elemento tras cada actualización
/// (3 servidores = 3 avisos). Con un solo elemento es un único aviso. Los elementos antiguos se migran al usarlos.
enum Keychain {
    private static let service = "cl.enovus.terminal.servers"
    private static let vaultAccount = "vault"
    private static let lock = NSLock()
    private static var vault: [String: String]?

    private static func base(_ account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account]
    }

    /// Lee el elemento único (una vez por ejecución). Debe llamarse con `lock` tomado.
    private static func loadVault() -> (dict: [String: String]?, status: OSStatus) {
        if let v = vault { return (v, errSecSuccess) }
        var q = base(vaultAccount)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: AnyObject?
        let st = SecItemCopyMatching(q as CFDictionary, &out)
        switch st {
        case errSecSuccess:
            guard let data = out as? Data, let d = try? JSONDecoder().decode([String: String].self, from: data) else {
                return (nil, errSecDecode)       // no se pisa un elemento que no se puede leer
            }
            vault = d; return (d, st)
        case errSecItemNotFound:
            vault = [:]; return ([:], errSecSuccess)
        default:
            return (nil, st)                     // permiso denegado, llavero bloqueado… se reintenta la próxima vez
        }
    }

    /// Guarda el elemento único. Debe llamarse con `lock` tomado.
    @discardableResult
    private static func persist() -> Bool {
        guard let v = vault, let data = try? JSONEncoder().encode(v) else { return false }
        SecItemDelete(base(vaultAccount) as CFDictionary)
        var item = base(vaultAccount)
        item[kSecValueData as String] = data
        let st = SecItemAdd(item as CFDictionary, nil)
        if st != errSecSuccess { NSLog("jTerm: no se pudo guardar en el Keychain (\(st))") }
        return st == errSecSuccess
    }

    /// Lee un elemento del formato antiguo (uno por servidor).
    private static func legacyRead(_ id: String) -> (secret: String?, status: OSStatus) {
        var q = base(id)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: AnyObject?
        let st = SecItemCopyMatching(q as CFDictionary, &out)
        guard st == errSecSuccess, let data = out as? Data else { return (nil, st) }
        return (String(data: data, encoding: .utf8), st)
    }

    static func set(_ secret: String, for id: UUID) {
        lock.lock(); defer { lock.unlock() }
        let key = id.uuidString
        guard loadVault().dict != nil else { NSLog("jTerm: Keychain no disponible; no se guardó el secreto"); return }
        if secret.isEmpty { vault?[key] = nil } else { vault?[key] = secret }
        if persist() { SecItemDelete(base(key) as CFDictionary) }      // el elemento antiguo (si lo hay) ya no hace falta
    }

    /// Devuelve el secreto y el OSStatus (errSecItemNotFound = no hay secreto guardado).
    static func read(_ id: UUID) -> (secret: String?, status: OSStatus) {
        lock.lock(); defer { lock.unlock() }
        let key = id.uuidString
        let (d, st) = loadVault()
        guard let d else { return (nil, st) }
        if let s = d[key] { return (s, errSecSuccess) }
        // ¿Quedó guardado en el formato antiguo? Se migra al elemento único.
        let old = legacyRead(key)
        guard let s = old.secret else { return (nil, old.status) }
        vault?[key] = s
        if persist() { SecItemDelete(base(key) as CFDictionary) }
        return (s, errSecSuccess)
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
