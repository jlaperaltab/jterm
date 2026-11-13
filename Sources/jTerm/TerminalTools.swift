import Foundation
import Darwin

// MARK: - Texto limpio a partir de la salida del terminal

/// Convierte la salida cruda del terminal (con secuencias de escape, colores, retrocesos, barras de progreso con \r…)
/// en texto legible, línea a línea. Funciona en streaming: se puede alimentar con trozos cortados en cualquier punto.
struct AnsiTextCleaner {
    private enum State { case normal, esc, escIntermediate, csi, osc, oscEsc }
    private var state = State.normal
    private var line: [Character] = []
    private var cursor = 0
    private var carry: [UInt8] = []          // bytes de un carácter UTF-8 que quedó cortado entre dos trozos
    private var csi: [UInt8] = []

    /// Procesa un trozo y devuelve las líneas que quedaron completas (cada una con su salto de línea).
    mutating func feed(_ data: [UInt8]) -> String {
        let bytes = carry + data
        carry = []
        var out = ""
        var i = 0
        while i < bytes.count {
            let b = bytes[i]
            switch state {
            case .normal:
                if b >= 0x80 {
                    let need = b >= 0xF0 ? 4 : (b >= 0xE0 ? 3 : (b >= 0xC0 ? 2 : 1))
                    if i + need > bytes.count { carry = Array(bytes[i...]); return out }
                    for ch in String(decoding: bytes[i..<(i + need)], as: UTF8.self) { put(ch) }
                    i += need; continue
                }
                switch b {
                case 0x1B: state = .esc
                case 0x0A: out += flush() + "\n"
                case 0x0D: cursor = 0
                case 0x08: cursor = max(0, cursor - 1)
                case 0x09: put("\t")
                case 0x20...0x7E: put(Character(UnicodeScalar(b)))
                default: break                    // BEL y demás controles
                }
            case .esc:
                switch b {
                case 0x5B: state = .csi; csi = []
                case 0x5D, 0x50, 0x58, 0x5E, 0x5F: state = .osc        // OSC, DCS, SOS, PM, APC: hasta BEL o ESC \
                case 0x20...0x2F: state = .escIntermediate
                default: state = .normal
                }
            case .escIntermediate:
                if !(0x20...0x2F).contains(b) { state = .normal }
            case .csi:
                if (0x40...0x7E).contains(b) { applyCSI(final: b); state = .normal } else { csi.append(b) }
            case .osc:
                if b == 0x07 { state = .normal } else if b == 0x1B { state = .oscEsc }
            case .oscEsc:
                state = .normal
            }
            i += 1
            if line.count > 20_000 { out += flush() + "\n" }       // una "línea" absurdamente larga: se parte
        }
        return out
    }

    /// Lo que quedó sin terminar (la última línea sin salto de línea).
    mutating func finish() -> String { line.isEmpty ? "" : flush() }

    private mutating func put(_ c: Character) {
        if cursor < line.count { line[cursor] = c } else { line.append(c) }
        cursor += 1
    }
    private mutating func flush() -> String {
        var s = String(line)
        while s.last == " " { s.removeLast() }
        line = []; cursor = 0
        return s
    }
    private mutating func applyCSI(final: UInt8) {
        let params = String(decoding: csi, as: UTF8.self).split(separator: ";", omittingEmptySubsequences: false).map { Int($0) }
        let n = max(1, params.first.flatMap { $0 } ?? 1)
        switch final {
        case UInt8(ascii: "K"):                                  // borrar hasta el final de la línea (o toda)
            let mode = params.first.flatMap { $0 } ?? 0
            if mode == 2 { line = []; cursor = 0 } else if mode == 0, cursor < line.count { line.removeSubrange(cursor...) }
        case UInt8(ascii: "C"): cursor += n
        case UInt8(ascii: "D"): cursor = max(0, cursor - n)
        case UInt8(ascii: "G"): cursor = max(0, n - 1)
        case UInt8(ascii: "P"): if cursor < line.count { line.removeSubrange(cursor..<min(line.count, cursor + n)) }
        default: break
        }
        while cursor > line.count { line.append(" ") }
    }
}

// MARK: - Grabación de la sesión

/// Guarda lo que muestra un terminal en un archivo de texto (como el comando `script`, pero legible).
/// Solo se registra la salida: lo que escribes aparece si el shell lo repite en pantalla, y las contraseñas no.
final class SessionRecorder {
    let url: URL
    private let handle: FileHandle
    private let queue = DispatchQueue(label: "cl.jotaele.jterm.recorder")
    private var cleaner = AnsiTextCleaner()
    private var closed = false

    /// Carpeta donde se guardan las grabaciones: la elegida en Ajustes o, si no, la de datos de jTerm.
    static var defaultDirectory: URL {
        let custom = Settings.shared.recordingDirectory.trimmingCharacters(in: .whitespaces)
        if !custom.isEmpty { return URL(fileURLWithPath: (custom as NSString).expandingTildeInPath, isDirectory: true) }
        return ServerStore.dir.appendingPathComponent("Grabaciones", isDirectory: true)
    }

    init(directory: URL, title: String, env: ServerEnv?) throws {
        let fm = FileManager.default
        try fm.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd HH.mm.ss"
        let safe = title.map { "/:\\".contains($0) ? "-" : $0 }.map(String.init).joined().trimmingCharacters(in: .whitespaces)
        var target = directory.appendingPathComponent("\(f.string(from: Date())) – \(safe.isEmpty ? "sesión" : safe).log")
        var n = 2
        while fm.fileExists(atPath: target.path) { target = directory.appendingPathComponent("\(f.string(from: Date())) – \(safe) (\(n)).log"); n += 1 }
        let header = "# jTerm · sesión grabada\n# Pestaña: \(title)\n# Inicio: \(DateFormatter.localizedString(from: Date(), dateStyle: .medium, timeStyle: .medium))\n"
            + (env.map { "# Entorno: \($0.title)\n" } ?? "") + "# ----------------------------------------\n"
        guard fm.createFile(atPath: target.path, contents: Data(header.utf8), attributes: [.posixPermissions: 0o600]) else {
            throw CocoaError(.fileWriteUnknown)
        }
        handle = try FileHandle(forWritingTo: target)
        try handle.seekToEnd()
        url = target
    }

    func write(_ slice: ArraySlice<UInt8>) {
        let data = Array(slice)
        queue.async { [self] in
            guard !closed else { return }
            let text = cleaner.feed(data)
            if !text.isEmpty { try? handle.write(contentsOf: Data(text.utf8)) }
        }
    }

    /// Una nota en medio de la grabación (p. ej. al entrar a una aplicación de pantalla completa).
    func note(_ text: String) {
        queue.async { [self] in
            guard !closed else { return }
            let pending = cleaner.finish()
            try? handle.write(contentsOf: Data((pending + (pending.isEmpty ? "" : "\n") + text + "\n").utf8))
        }
    }

    func stop() {
        queue.sync {
            guard !closed else { return }
            closed = true
            let rest = cleaner.finish()
            let f = DateFormatter(); f.dateStyle = .medium; f.timeStyle = .medium
            try? handle.write(contentsOf: Data((rest + (rest.isEmpty ? "" : "\n") + "# ---------------------------------------- \n# Fin: \(f.string(from: Date()))\n").utf8))
            try? handle.close()
        }
    }
}

// MARK: - Enlaces y rutas clicables

enum LinkDetector {
    enum Target: Equatable {
        case url(URL)
        case path(String, line: Int?)
    }
    /// Lo encontrado y las columnas que ocupa en la línea (para subrayarlo).
    struct Match: Equatable { let target: Target; let range: Range<Int> }

    private static let urlRegex = try! NSRegularExpression(pattern: #"\b(?:https?|ftp)://[^\s<>"'`]+"#)
    private static let edgePunctuation = CharacterSet(charactersIn: ".,;:!?)]}>'\"")
    private static let wrapChars = CharacterSet(charactersIn: "([{<\"'`")

    private static func isIn(_ c: Character, _ set: CharacterSet) -> Bool { c.unicodeScalars.first.map(set.contains) ?? false }

    /// Todas las URLs de la línea con las columnas que ocupan.
    static func urlMatches(in line: String) -> [(range: Range<Int>, url: URL)] {
        guard line.contains("://") else { return [] }
        let ns = line as NSString
        var out: [(Range<Int>, URL)] = []
        for m in urlRegex.matches(in: line, range: NSRange(location: 0, length: ns.length)) {
            var s = ns.substring(with: m.range)
            // Se quita la puntuación del final, salvo un ")" que cierra un "(" de la propia URL (p. ej. Wikipedia)
            while let last = s.unicodeScalars.last, edgePunctuation.contains(last) {
                if last == ")" && s.filter({ $0 == ")" }).count <= s.filter({ $0 == "(" }).count { break }
                s.removeLast()
            }
            guard let url = URL(string: s), let r = Range(NSRange(location: m.range.location, length: (s as NSString).length), in: line) else { continue }
            let start = line.distance(from: line.startIndex, to: r.lowerBound), end = line.distance(from: line.startIndex, to: r.upperBound)
            out.append((start..<end, url))
        }
        return out
    }

    /// Qué hay bajo la columna `column` de la línea: una URL, una ruta de archivo, o nada.
    static func target(in line: String, column: Int, allowBareFilenames: Bool = true) -> Target? {
        match(in: line, column: column, allowBareFilenames: allowBareFilenames)?.target
    }

    static func match(in line: String, column: Int, allowBareFilenames: Bool = true) -> Match? {
        guard !line.isEmpty, column >= 0 else { return nil }
        let chars = Array(line)
        let col = min(column, chars.count - 1)

        for u in urlMatches(in: line) where u.range.contains(col) { return Match(target: .url(u.url), range: u.range) }

        // Ruta: la "palabra" bajo el cursor, delimitada por espacios y comillas/paréntesis
        guard !chars[col].isWhitespace else { return nil }
        func isBreak(_ c: Character) -> Bool { c.isWhitespace || "\"'`<>|,;()[]{}".contains(c) }
        var lo = col, hi = col
        while lo > 0, !isBreak(chars[lo - 1]) { lo -= 1 }
        while hi < chars.count - 1, !isBreak(chars[hi + 1]) { hi += 1 }
        while lo <= hi, isIn(chars[lo], wrapChars) { lo += 1 }
        while hi >= lo, isIn(chars[hi], edgePunctuation), chars[hi] != "/" { hi -= 1 }
        guard lo <= hi, col >= lo, col <= hi else { return nil }
        let token = String(chars[lo...hi])

        // sufijo ":línea" o ":línea:columna"
        var path = token, lineNo: Int?
        let parts = token.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
        if parts.count >= 2, let n = Int(parts[parts.count - 1]) {
            if parts.count >= 3, let l = Int(parts[parts.count - 2]) { path = parts.dropLast(2).joined(separator: ":"); lineNo = l }
            else { path = parts.dropLast().joined(separator: ":"); lineNo = n }
        }
        guard !path.isEmpty, !path.contains("://") else { return nil }
        let range = lo..<(hi + 1)
        let looksLikePath = path.hasPrefix("/") || path.hasPrefix("~") || path.hasPrefix("./") || path.hasPrefix("../") || path.contains("/")
        if looksLikePath {
            guard path.rangeOfCharacter(from: .alphanumerics) != nil else { return nil }
            return Match(target: .path(path, line: lineNo), range: range)
        }
        // nombre suelto con extensión (README.md, app.log): solo si se pidió, y que no sea un número como "1.5"
        guard allowBareFilenames, let dot = path.lastIndex(of: "."), dot != path.startIndex else { return nil }
        let ext = path[path.index(after: dot)...]
        guard (1...8).contains(ext.count), ext.allSatisfy({ $0.isLetter || $0.isNumber }), ext.contains(where: { $0.isLetter }) else { return nil }
        return Match(target: .path(path, line: lineNo), range: range)
    }
}

/// Carpeta actual de un proceso (el shell de una pestaña local), para resolver rutas relativas.
func currentDirectory(ofProcess pid: pid_t) -> String? {
    var info = proc_vnodepathinfo()
    let size = Int32(MemoryLayout<proc_vnodepathinfo>.size)
    guard proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, 0, &info, size) == size else { return nil }
    return withUnsafePointer(to: &info.pvi_cdir.vip_path) {
        $0.withMemoryRebound(to: CChar.self, capacity: Int(MAXPATHLEN)) { String(cString: $0) }
    }
}
