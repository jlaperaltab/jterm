import Foundation
import CryptoKit

extension Notification.Name {
    /// Algo sincronizable cambió en este equipo (servidores, cuentas, comandos o ajustes).
    static let syncLocalChanged = Notification.Name("jterm.sync.localChanged")
    static let syncStatusChanged = Notification.Name("jterm.sync.statusChanged")
}

// MARK: - Modelo

/// Valor JSON simple para guardar campos de cualquier tipo en el archivo de sincronización.
enum JSONValue: Codable, Equatable {
    case string(String), int(Int), double(Double), bool(Bool), array([JSONValue])

    init(from d: Decoder) throws {
        let c = try d.singleValueContainer()
        if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Int.self) { self = .int(v) }
        else if let v = try? c.decode(Double.self) { self = .double(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else { self = .array(try c.decode([JSONValue].self)) }
    }
    func encode(to e: Encoder) throws {
        var c = e.singleValueContainer()
        switch self {
        case .string(let v): try c.encode(v)
        case .int(let v): try c.encode(v)
        case .double(let v): try c.encode(v)
        case .bool(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        }
    }
    var string: String? { if case .string(let s) = self { return s }; return nil }
    var int: Int? { switch self { case .int(let i): return i; case .double(let d): return Int(d); default: return nil } }
    var bool: Bool? { if case .bool(let b) = self { return b }; return nil }
}

/// Un elemento sincronizable (servidor, cuenta AWS, comando o ajustes).
struct SyncItem: Codable, Equatable {
    var fields: [String: JSONValue]
    /// Contraseña / passphrase / clave secreta (solo si el usuario eligió sincronizarlas).
    var secret: String?
    /// Identifica "lo mismo" entre equipos que lo crearon por separado (p. ej. host+usuario+puerto).
    var naturalKey: String?
}

struct SyncRecord: Codable, Equatable {
    var item: SyncItem?          // nil si está borrado
    var updatedAt: Date
    var deleted: Bool
}

struct SyncDocument: Codable, Equatable {
    var format = "jterm-sync"
    var version = 1
    var records: [String: SyncRecord] = [:]
}

/// Lo que este equipo recuerda de la última sincronización (para distinguir "nuevo", "editado" y "borrado").
struct SyncState: Codable, Equatable {
    /// `hash`: contenido sin secreto. `full`: contenido con secreto, solo si ese equipo sincronizaba secretos.
    struct Entry: Codable, Equatable { var hash: String; var full: String? = nil; var updatedAt: Date; var deleted: Bool }
    var entries: [String: Entry] = [:]
}

enum SyncKind {
    static let server = "server", aws = "aws", command = "cmd", settings = "settings"
    static func of(_ key: String) -> String { String(key.prefix(while: { $0 != ":" })) }
}

// MARK: - Motor de combinación (sin entrada/salida: solo lógica)

struct MergeOutcome {
    var document: SyncDocument
    var upserts: [String: SyncItem] = [:]     // a aplicar en este equipo
    var deletes: [String] = []                // a borrar en este equipo
    var state: SyncState
}

enum SyncEngine {
    static func hash(_ item: SyncItem, includeSecret: Bool) -> String {
        var i = item
        if !includeSecret { i.secret = nil }
        let enc = JSONEncoder(); enc.outputFormatting = [.sortedKeys]
        let data = (try? enc.encode(i)) ?? Data()
        return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    /// Elementos creados por separado en dos equipos que son "lo mismo" (misma clave natural): el local adopta la
    /// identidad del remoto para no duplicarlo. Devuelve [claveLocal: claveRemota].
    static func findRekeys(local: [String: SyncItem], remote: SyncDocument?, state: SyncState, enabled: Set<String>) -> [String: String] {
        guard let remote else { return [:] }
        var map: [String: String] = [:]
        var taken = Set<String>()
        for (rkey, rec) in remote.records.sorted(by: { $0.key < $1.key }) {
            guard !rec.deleted, let item = rec.item, let nk = item.naturalKey, enabled.contains(SyncKind.of(rkey)),
                  local[rkey] == nil, state.entries[rkey] == nil else { continue }
            if let (lkey, _) = local.sorted(by: { $0.key < $1.key }).first(where: { (lk, li) in
                SyncKind.of(lk) == SyncKind.of(rkey) && li.naturalKey == nk && remote.records[lk] == nil
                    && state.entries[lk] == nil && !taken.contains(lk)
            }) {
                map[lkey] = rkey; taken.insert(lkey)
            }
        }
        return map
    }

    static func reconcile(local: [String: SyncItem], remote: SyncDocument?, state: SyncState, enabled: Set<String>,
                          manageSecrets: Bool, now: Date, tombstoneTTL: TimeInterval = 90 * 86_400) -> MergeOutcome {
        let remoteRecs = remote?.records ?? [:]
        func h(_ i: SyncItem) -> String { hash(i, includeSecret: manageSecrets) }
        func entry(for r: SyncRecord) -> SyncState.Entry {
            guard let it = r.item else { return SyncState.Entry(hash: "", full: nil, updatedAt: r.updatedAt, deleted: true) }
            return SyncState.Entry(hash: hash(it, includeSecret: false), full: manageSecrets ? hash(it, includeSecret: true) : nil,
                                   updatedAt: r.updatedAt, deleted: false)
        }
        /// ¿Este elemento local sigue igual que en la última sincronización?
        func unchanged(_ key: String, _ item: SyncItem) -> Bool {
            guard let st = state.entries[key], !st.deleted else { return false }
            if manageSecrets {
                if let full = st.full { return hash(item, includeSecret: true) == full }
                return item.secret == nil && hash(item, includeSecret: false) == st.hash      // antes no se seguían los secretos
            }
            return hash(item, includeSecret: false) == st.hash
        }

        // 1) Estado local como registros con fecha de modificación
        var localRecs: [String: SyncRecord] = [:]
        var localUnchanged = Set<String>()
        for (key, item) in local where enabled.contains(SyncKind.of(key)) {
            if unchanged(key, item), let st = state.entries[key] {
                localRecs[key] = SyncRecord(item: item, updatedAt: st.updatedAt, deleted: false); localUnchanged.insert(key)
            } else {
                localRecs[key] = SyncRecord(item: item, updatedAt: now, deleted: false)          // nuevo o editado
            }
        }
        for (key, st) in state.entries where enabled.contains(SyncKind.of(key)) && local[key] == nil {
            localRecs[key] = SyncRecord(item: nil, updatedAt: st.deleted ? st.updatedAt : now, deleted: true)  // borrado
        }

        // 2) Combinar
        var out = MergeOutcome(document: SyncDocument(), state: SyncState())
        for key in Set(localRecs.keys).union(remoteRecs.keys) {
            let kind = SyncKind.of(key)
            guard enabled.contains(kind) else {                                  // tipos que este equipo no sincroniza: se respetan
                if let r = remoteRecs[key] { out.document.records[key] = r }
                if let st = state.entries[key] { out.state.entries[key] = st }
                continue
            }
            let l = localRecs[key], r = remoteRecs[key]
            var winner: SyncRecord
            var fromRemote = false
            switch (l, r) {
            case (let l?, nil): winner = l
            case (nil, let r?): winner = r; fromRemote = true
            case (let l?, let r?):
                let same = (l.deleted && r.deleted) || (!l.deleted && !r.deleted && l.item.map(h) == r.item.map(h))
                let firstSync = state.entries[key] == nil
                if same { winner = l.updatedAt >= r.updatedAt ? l : r }
                else if kind == SyncKind.settings && firstSync { winner = r; fromRemote = true }   // un equipo nuevo no pisa los ajustes
                else if localUnchanged.contains(key) {                           // lo que no cambió aquí nunca pisa lo de otro equipo
                    if r.updatedAt >= l.updatedAt { winner = r; fromRemote = true } else { winner = l }
                }
                else if r.updatedAt > l.updatedAt { winner = r; fromRemote = true }
                else { winner = l }
            default: continue
            }
            if winner.deleted && now.timeIntervalSince(winner.updatedAt) > tombstoneTTL { continue }    // tumba antigua: se descarta
            if !fromRemote, !manageSecrets, var it = winner.item, let remoteSecret = r?.item?.secret {
                it.secret = remoteSecret; winner.item = it                       // no pisar secretos que este equipo no gestiona
            }
            out.document.records[key] = winner
            out.state.entries[key] = entry(for: winner)
            if fromRemote {
                if winner.deleted { if local[key] != nil { out.deletes.append(key) } }
                else if let it = winner.item, local[key].map(h) != h(it) { out.upserts[key] = it }
            }
        }
        out.deletes.sort()
        return out
    }

    /// Une dos documentos (p. ej. una copia en conflicto de iCloud): gana la fecha más reciente de cada elemento.
    static func mergeDocuments(_ a: SyncDocument, _ b: SyncDocument) -> SyncDocument {
        var out = a
        for (k, r) in b.records where (out.records[k]?.updatedAt ?? .distantPast) < r.updatedAt { out.records[k] = r }
        return out
    }
}

// MARK: - Almacenamiento (archivo cifrado en una carpeta, p. ej. iCloud Drive)

enum SyncError: LocalizedError {
    case badPassphrase, downloading, unreadable, noFolder
    var errorDescription: String? {
        switch self {
        case .badPassphrase: return "La contraseña de sincronización no coincide con la del archivo."
        case .downloading: return "iCloud todavía está descargando el archivo de sincronización. Se reintentará en un momento."
        case .unreadable: return "El archivo de sincronización no se puede leer."
        case .noFolder: return "No se encontró la carpeta de iCloud Drive."
        }
    }
}

final class SyncStorage {
    let folder: URL
    static let fileName = "sync.jterm"
    var mainURL: URL { folder.appendingPathComponent(Self.fileName) }

    init(folder: URL) { self.folder = folder }

    /// Archivo principal presente (o su marcador de "aún no descargado" de iCloud).
    var hasData: Bool {
        FileManager.default.fileExists(atPath: mainURL.path) || FileManager.default.fileExists(atPath: placeholderURL.path)
    }
    private var placeholderURL: URL { folder.appendingPathComponent("." + Self.fileName + ".icloud") }

    /// Copias en conflicto que crea iCloud: "sync 2.jterm", "sync 3.jterm"…
    private func conflictURLs() -> [URL] {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
        return names.filter { $0.range(of: #"^sync \d+\.jterm$"#, options: .regularExpression) != nil }.sorted()
            .map { folder.appendingPathComponent($0) }
    }

    private func coordinatedRead(_ url: URL) throws -> Data {
        var coordErr: NSError?, result: Result<Data, Error> = .failure(SyncError.unreadable)
        NSFileCoordinator(filePresenter: nil).coordinate(readingItemAt: url, options: [], error: &coordErr) { u in
            result = Result { try Data(contentsOf: u) }
        }
        if let coordErr { throw coordErr }
        return try result.get()
    }

    private func decode(_ data: Data, passphrase: String) throws -> SyncDocument {
        guard let env = ExportCrypto.envelope(from: data) else { throw SyncError.unreadable }
        let plain: Data
        do { plain = try ExportCrypto.decrypt(env, passphrase: passphrase) } catch { throw SyncError.badPassphrase }
        guard let doc = try? JSONDecoder().decode(SyncDocument.self, from: plain), doc.format == "jterm-sync" else { throw SyncError.unreadable }
        return doc
    }

    /// Lee el documento (y combina las copias en conflicto). `nil` si todavía no existe.
    func read(passphrase: String) throws -> (doc: SyncDocument?, conflicts: [URL]) {
        if !FileManager.default.fileExists(atPath: mainURL.path) {
            if FileManager.default.fileExists(atPath: placeholderURL.path) {
                try? FileManager.default.startDownloadingUbiquitousItem(at: mainURL)
                throw SyncError.downloading
            }
            return (nil, [])
        }
        var doc = try decode(try coordinatedRead(mainURL), passphrase: passphrase)
        let conflicts = conflictURLs()
        for u in conflicts { if let d = try? decode(try coordinatedRead(u), passphrase: passphrase) { doc = SyncEngine.mergeDocuments(doc, d) } }
        return (doc, conflicts)
    }

    func write(_ doc: SyncDocument, passphrase: String, removing conflicts: [URL] = []) throws {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let enc = JSONEncoder(); enc.outputFormatting = [.sortedKeys]
        let data = try ExportCrypto.encrypt(try enc.encode(doc), passphrase: passphrase)
        var coordErr: NSError?, writeErr: Error?
        NSFileCoordinator(filePresenter: nil).coordinate(writingItemAt: mainURL, options: .forReplacing, error: &coordErr) { u in
            do { try data.write(to: u, options: .atomic) } catch { writeErr = error }
        }
        if let coordErr { throw coordErr }
        if let writeErr { throw writeErr }
        for u in conflicts { try? FileManager.default.removeItem(at: u) }
    }

    /// Comprueba que la contraseña sirve para el archivo existente.
    func verify(passphrase: String) throws { _ = try read(passphrase: passphrase) }

    func delete() { try? FileManager.default.removeItem(at: mainURL); conflictURLs().forEach { try? FileManager.default.removeItem(at: $0) } }
}

// MARK: - Ejecución de una sincronización

protocol SyncLocalAdapter: AnyObject {
    func snapshot(enabled: Set<String>, includeSecrets: Bool) -> [String: SyncItem]
    func rekey(_ map: [String: String])
    func apply(upserts: [String: SyncItem], deletes: [String], manageSecrets: Bool)
}

struct SyncResult: Equatable { var uploaded = 0, downloaded = 0, deleted = 0 }

enum SyncRunner {
    /// Una sincronización completa y síncrona. Debe llamarse fuera del hilo principal en la app.
    static func run(adapter: SyncLocalAdapter, storage: SyncStorage, passphrase: String, state: SyncState,
                    enabled: Set<String>, manageSecrets: Bool, now: Date = Date()) throws -> (SyncResult, SyncState) {
        let (remote, conflicts) = try storage.read(passphrase: passphrase)
        var local = adapter.snapshot(enabled: enabled, includeSecrets: manageSecrets)
        let rekeys = SyncEngine.findRekeys(local: local, remote: remote, state: state, enabled: enabled)
        if !rekeys.isEmpty {
            adapter.rekey(rekeys)
            local = adapter.snapshot(enabled: enabled, includeSecrets: manageSecrets)
        }
        // Un cambio hecho después de ver los de otro equipo debe ganar aunque el reloj de este equipo vaya atrasado.
        let latestSeen = remote?.records.values.map(\.updatedAt).max()
        let stamp = latestSeen.map { max(now, $0.addingTimeInterval(0.001)) } ?? now
        let outcome = SyncEngine.reconcile(local: local, remote: remote, state: state, enabled: enabled, manageSecrets: manageSecrets, now: stamp)
        adapter.apply(upserts: outcome.upserts, deletes: outcome.deletes, manageSecrets: manageSecrets)

        var result = SyncResult(downloaded: outcome.upserts.count, deleted: outcome.deletes.count)
        if outcome.document != remote || !conflicts.isEmpty {
            result.uploaded = outcome.document.records.filter { (k, r) in
                enabled.contains(SyncKind.of(k)) && (remote?.records[k] != r)
                    && !(outcome.upserts[k] != nil || (r.deleted && outcome.deletes.contains(k)))
            }.count
            try storage.write(outcome.document, passphrase: passphrase, removing: conflicts)
        }
        return (result, outcome.state)
    }
}
