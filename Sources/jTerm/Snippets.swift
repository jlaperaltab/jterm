import Foundation

/// Un comando guardado por el usuario (global, no depende del servidor).
struct Snippet: Codable, Identifiable, Equatable {
    var id = UUID()
    var name = ""
    var command = ""
    /// Si es true, al insertarlo se pulsa Enter automáticamente.
    var autoRun = false

    var firstLine: String {
        command.split(whereSeparator: \.isNewline).first.map { String($0).trimmingCharacters(in: .whitespaces) } ?? ""
    }
    /// Nombre a mostrar: el nombre si existe, si no la primera línea del comando.
    var title: String {
        let n = name.trimmingCharacters(in: .whitespaces)
        return n.isEmpty ? firstLine : n
    }
    var lineCount: Int { command.split(whereSeparator: \.isNewline).count }
}

extension Notification.Name {
    static let snippetsDidChange = Notification.Name("jterm.snippets.didChange")
}

/// Repositorio de comandos, guardado en Application Support/jTerm/commands.json (solo legible por el usuario).
final class SnippetStore {
    static var fileURL: URL = ServerStore.dir.appendingPathComponent("commands.json")
    static let shared = SnippetStore()

    private(set) var items: [Snippet] = []
    private let url: URL

    init(url: URL? = nil) {
        self.url = url ?? Self.fileURL
        load()
    }

    private func load() {
        guard let data = try? Data(contentsOf: url),
              let list = try? JSONDecoder().decode([Snippet].self, from: data) else { return }
        items = list
    }

    private func save() {
        let enc = JSONEncoder(); enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? enc.encode(items) else { return }
        try? data.write(to: url, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        NotificationCenter.default.post(name: .snippetsDidChange, object: nil)
        NotificationCenter.default.post(name: .syncLocalChanged, object: nil)
    }

    func rekey(_ old: UUID, to new: UUID) {
        guard let i = items.firstIndex(where: { $0.id == old }) else { return }
        items[i].id = new; save()
    }

    func upsert(_ s: Snippet) {
        if let i = items.firstIndex(where: { $0.id == s.id }) { items[i] = s } else { items.append(s) }
        save()
    }

    func remove(_ id: UUID) {
        items.removeAll { $0.id == id }
        save()
    }

    func duplicate(_ id: UUID) {
        guard let i = items.firstIndex(where: { $0.id == id }) else { return }
        var copy = items[i]; copy.id = UUID()
        copy.name = copy.name.isEmpty ? "" : copy.name + " (copia)"
        items.insert(copy, at: i + 1)
        save()
    }

    /// Agrega los que aún no existen (mismo nombre y comando). Devuelve cuántos se agregaron.
    @discardableResult
    func add(contentsOf list: [Snippet]) -> Int {
        var added = 0
        for s in list where !items.contains(where: { $0.name == s.name && $0.command == s.command }) {
            var n = s; n.id = UUID(); items.append(n); added += 1
        }
        if added > 0 { save() }
        return added
    }

    /// Filtra por nombre o comando (sin distinguir mayúsculas ni acentos).
    func search(_ q: String) -> [Snippet] {
        let t = q.trimmingCharacters(in: .whitespaces)
        guard !t.isEmpty else { return items }
        let opts: String.CompareOptions = [.caseInsensitive, .diacriticInsensitive]
        return items.filter { $0.name.range(of: t, options: opts) != nil || $0.command.range(of: t, options: opts) != nil }
    }
}
