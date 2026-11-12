import Foundation

// MARK: - JSON que conserva el orden de las claves

indirect enum JSONNode {
    case object([(String, JSONNode)]), array([JSONNode]), string(String), number(String), bool(Bool), null
}

enum OrderedJSON {
    static func parse(_ s: String) -> JSONNode? {
        var p = P(b: Array(s.utf8)); p.ws()
        guard let n = p.value(), (p.ws(), p.i == p.b.count).1 else { return nil }
        return n
    }

    private struct P {
        let b: [UInt8]; var i = 0; var depth = 0
        init(b: [UInt8]) { self.b = b }
        mutating func ws() { while i < b.count, [0x20, 0x09, 0x0A, 0x0D].contains(b[i]) { i += 1 } }
        mutating func value() -> JSONNode? {
            guard i < b.count, depth < 200 else { return nil }
            switch b[i] {
            case UInt8(ascii: "{"):
                i += 1; depth += 1; defer { depth -= 1 }
                var pairs: [(String, JSONNode)] = []; ws()
                if i < b.count, b[i] == UInt8(ascii: "}") { i += 1; return .object(pairs) }
                while true {
                    ws(); guard i < b.count, b[i] == UInt8(ascii: "\""), let k = str() else { return nil }
                    ws(); guard i < b.count, b[i] == UInt8(ascii: ":") else { return nil }; i += 1; ws()
                    guard let v = value() else { return nil }
                    pairs.append((k, v)); ws()
                    guard i < b.count else { return nil }
                    if b[i] == UInt8(ascii: ",") { i += 1; continue }
                    if b[i] == UInt8(ascii: "}") { i += 1; return .object(pairs) }
                    return nil
                }
            case UInt8(ascii: "["):
                i += 1; depth += 1; defer { depth -= 1 }
                var items: [JSONNode] = []; ws()
                if i < b.count, b[i] == UInt8(ascii: "]") { i += 1; return .array(items) }
                while true {
                    ws(); guard let v = value() else { return nil }
                    items.append(v); ws()
                    guard i < b.count else { return nil }
                    if b[i] == UInt8(ascii: ",") { i += 1; continue }
                    if b[i] == UInt8(ascii: "]") { i += 1; return .array(items) }
                    return nil
                }
            case UInt8(ascii: "\""): return str().map(JSONNode.string)
            case UInt8(ascii: "t"): return lit("true") ? .bool(true) : nil
            case UInt8(ascii: "f"): return lit("false") ? .bool(false) : nil
            case UInt8(ascii: "n"): return lit("null") ? .null : nil
            default:
                let s = i
                while i < b.count, "-+.eE0123456789".utf8.contains(b[i]) { i += 1 }
                guard i > s else { return nil }
                return .number(String(decoding: b[s..<i], as: UTF8.self))
            }
        }
        mutating func lit(_ w: String) -> Bool {
            let u = Array(w.utf8); guard i + u.count <= b.count, Array(b[i..<i + u.count]) == u else { return false }
            i += u.count; return true
        }
        mutating func hex4() -> UInt32? {
            guard i + 4 <= b.count, let v = UInt32(String(decoding: b[i..<i + 4], as: UTF8.self), radix: 16) else { return nil }
            i += 4; return v
        }
        mutating func str() -> String? {
            i += 1; var out = [UInt8]()
            while i < b.count {
                let c = b[i]; i += 1
                if c == UInt8(ascii: "\"") { return String(decoding: out, as: UTF8.self) }
                if c != UInt8(ascii: "\\") { out.append(c); continue }
                guard i < b.count else { return nil }
                let e = b[i]; i += 1
                switch e {
                case UInt8(ascii: "n"): out.append(0x0A)
                case UInt8(ascii: "t"): out.append(0x09)
                case UInt8(ascii: "r"): out.append(0x0D)
                case UInt8(ascii: "b"): out.append(0x08)
                case UInt8(ascii: "f"): out.append(0x0C)
                case UInt8(ascii: "u"):
                    guard var u = hex4() else { return nil }
                    if (0xD800...0xDBFF).contains(u), i + 1 < b.count, b[i] == UInt8(ascii: "\\"), b[i + 1] == UInt8(ascii: "u") {
                        i += 2; guard let lo = hex4(), (0xDC00...0xDFFF).contains(lo) else { return nil }
                        u = 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00)
                    }
                    guard let sc = Unicode.Scalar(u) else { return nil }
                    out.append(contentsOf: Array(String(Character(sc)).utf8))
                default: out.append(e)       // \" \\ \/
                }
            }
            return nil
        }
    }

    enum Token { case key, string, number, literal, punct }

    /// JSON con sangría como lista de fragmentos (para pintarlo con colores).
    static func pretty(_ n: JSONNode, indent: Int = 0, budget: inout Int) -> [(String, Token)] {
        guard budget > 0 else { return [("…", .punct)] }
        let sp = String(repeating: "  ", count: indent), sp2 = String(repeating: "  ", count: indent + 1)
        switch n {
        case .string(let s): budget -= s.count; return [("\"" + escape(s) + "\"", .string)]
        case .number(let s): return [(s, .number)]
        case .bool(let v): return [(v ? "true" : "false", .literal)]
        case .null: return [("null", .literal)]
        case .array(let a):
            if a.isEmpty { return [("[]", .punct)] }
            var out: [(String, Token)] = [("[\n", .punct)]
            for (i, x) in a.enumerated() {
                out.append((sp2, .punct)); out += pretty(x, indent: indent + 1, budget: &budget)
                out.append((i < a.count - 1 ? ",\n" : "\n", .punct))
            }
            return out + [(sp + "]", .punct)]
        case .object(let o):
            if o.isEmpty { return [("{}", .punct)] }
            var out: [(String, Token)] = [("{\n", .punct)]
            for (i, kv) in o.enumerated() {
                out.append((sp2, .punct)); out.append(("\"" + escape(kv.0) + "\"", .key)); out.append((": ", .punct))
                out += pretty(kv.1, indent: indent + 1, budget: &budget)
                out.append((i < o.count - 1 ? ",\n" : "\n", .punct))
            }
            return out + [(sp + "}", .punct)]
        }
    }

    static func escape(_ s: String) -> String {
        s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
            .replacingOccurrences(of: "\n", with: "\\n").replacingOccurrences(of: "\t", with: "\\t")
    }
}

// MARK: - Eventos analizados

enum LogLevel: Int, CaseIterable {
    case error, warn, info, debug, platform
    var label: String { ["ERROR", "WARN", "INFO", "DEBUG", "LAMBDA"][rawValue] }
    var filterTitle: String { ["ERROR", "WARN", "INFO", "DEBUG", "Plataforma"][rawValue] }
}
enum LogPlatform { case start, end, report, other }

struct LambdaReport: Equatable {
    var durationMs: Double?, billedMs: Int?, memorySizeMB: Int?, maxMemoryMB: Int?, initMs: Double?
    var status: String?
}

struct ParsedLog {
    var event: LogEvent
    var level: LogLevel = .info
    var lines: [String] = [""]
    var json: JSONNode?
    var summary = ""
    var chips: [(key: String, value: String)] = []
    var requestId: String?
    var platform: LogPlatform?
    var report: LambdaReport?
    var isTraceback = false
    /// Líneas posteriores a la primera (tracebacks, mensajes largos).
    var extraLines: [String] { Array(lines.dropFirst()) }
}

enum LogParser {
    private static let uuid = "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
    private static let levelKeys = ["level", "severity", "loglevel", "levelname", "lvl", "log_level"]
    private static let msgKeys = ["msg", "message", "event", "log", "text"]
    private static let timeKeys = ["time", "timestamp", "ts", "@timestamp", "t", "datetime", "date"]
    private static let ridKeys = ["requestid", "aws_request_id", "awsrequestid", "request_id"]

    private static func rx(_ pattern: String, _ s: String, options: NSRegularExpression.Options = []) -> [String]? {
        guard let re = try? NSRegularExpression(pattern: pattern, options: options) else { return nil }
        let ns = s as NSString
        guard let m = re.firstMatch(in: s, range: NSRange(location: 0, length: ns.length)) else { return nil }
        return (0..<m.numberOfRanges).map { m.range(at: $0).location == NSNotFound ? "" : ns.substring(with: m.range(at: $0)) }
    }

    static func levelFromWord(_ w: String) -> LogLevel? {
        switch w.lowercased() {
        case "error", "err", "fatal", "critical", "severe", "panic", "emerg", "alert", "crit": return .error
        case "warn", "warning": return .warn
        case "info", "notice", "information": return .info
        case "debug", "trace", "verbose": return .debug
        default: return nil
        }
    }

    private static func levelFromJSON(_ pairs: [(String, JSONNode)]) -> LogLevel? {
        for (k, v) in pairs where levelKeys.contains(k.lowercased()) {
            switch v {
            case .string(let s): if let l = levelFromWord(s) { return l }
            case .number(let n):
                if let i = Int(n) { if i >= 50 { return .error }; if i == 40 { return .warn }; if i == 30 { return .info }; if i <= 20 { return .debug } }
            default: break
            }
        }
        return nil
    }

    /// Nivel a partir del texto, con reglas conservadoras para no etiquetar mal (p. ej. "errors=0" no es un error).
    static func detectLevel(_ lines: [String]) -> LogLevel {
        let all = lines.joined(separator: "\n")
        if all.contains("Task timed out after") || all.contains("Traceback (most recent call last)") { return .error }
        let first = lines.first ?? ""
        if rx(#"\b(FATAL|CRITICAL|SEVERE|ERROR|EXCEPTION|PANIC)\b"#, first) != nil
            || rx(#"^\s*\[?(error|fatal|critical|severe)\]?(\s|:|$)"#, first, options: .caseInsensitive) != nil
            || rx(#"\blevel[=:]\s*"?(error|fatal)"#, first, options: .caseInsensitive) != nil { return .error }
        if lines.contains(where: { rx(#"^\s*[A-Za-z_][\w.]*(Error|Exception)(:|$)"#, $0) != nil }) { return .error }
        if rx(#"\b(WARN|WARNING)\b"#, first) != nil || rx(#"^\s*\[?(warn|warning)\]?(\s|:|$)"#, first, options: .caseInsensitive) != nil
            || rx(#"\blevel[=:]\s*"?warn"#, first, options: .caseInsensitive) != nil { return .warn }
        if rx(#"\b(DEBUG|TRACE)\b"#, first) != nil || rx(#"^\s*\[?(debug|trace)\]?(\s|:|$)"#, first, options: .caseInsensitive) != nil { return .debug }
        return .info
    }

    /// Quita del texto la palabra de nivel inicial ("ERROR ", "[WARNING] ", "INFO: ") cuando coincide con el nivel detectado,
    /// porque la insignia ya lo indica. El mensaje original no se modifica.
    static func stripLevelPrefix(_ s: String, _ level: LogLevel) -> String {
        guard let m = rx(#"^\s*[\[\(]?(FATAL|CRITICAL|SEVERE|ERROR|WARNING|WARN|INFO|DEBUG|TRACE)[\]\)]?\s*[:\-–]?\s+(.+)$"#, s, options: .caseInsensitive),
              levelFromWord(m[1]) == level, !m[2].trimmingCharacters(in: .whitespaces).isEmpty else { return s }
        return m[2]
    }

    static func parseReport(_ s: String) -> LambdaReport {
        func d(_ p: String) -> Double? { rx(p, s).flatMap { Double($0[1]) } }
        func i(_ p: String) -> Int? { rx(p, s).flatMap { Int($0[1]) } }
        var r = LambdaReport(durationMs: d(#"(?<!Billed )(?<!Init )Duration:\s*([\d.]+)\s*ms"#), billedMs: i(#"Billed Duration:\s*(\d+)\s*ms"#),
                             memorySizeMB: i(#"Memory Size:\s*(\d+)\s*MB"#), maxMemoryMB: i(#"Max Memory Used:\s*(\d+)\s*MB"#),
                             initMs: d(#"Init Duration:\s*([\d.]+)\s*ms"#))
        r.status = rx(#"Status:\s*(\w+)"#, s)?[1]
        return r
    }

    static func parse(_ e: LogEvent) -> ParsedLog {
        var p = ParsedLog(event: e)
        var raw = e.message
        while let l = raw.last, l == "\n" || l == "\r" || l == " " || l == "\t" { raw.removeLast() }
        p.lines = raw.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n")
        let first = p.lines[0]

        // Líneas de la plataforma Lambda
        for (prefix, kind) in [("START RequestId:", LogPlatform.start), ("END RequestId:", .end), ("REPORT RequestId:", .report)] where first.hasPrefix(prefix) {
            p.platform = kind; p.level = .platform; p.summary = first
            p.requestId = rx(#"RequestId:\s*(\S+)"#, first)?[1]
            if kind == .report { p.report = parseReport(raw); if let st = p.report?.status, st.lowercased() != "success" { p.level = .error } }
            return p
        }
        for prefix in ["INIT_START", "INIT_REPORT", "RESTORE_START", "RESTORE_REPORT", "EXTENSION", "TELEMETRY"] where first.hasPrefix(prefix) {
            p.platform = .other; p.level = .platform; p.summary = first; return p
        }

        // JSON estructurado
        if raw.hasPrefix("{"), let node = OrderedJSON.parse(raw), case .object(let pairs) = node {
            p.json = node
            p.level = levelFromJSON(pairs) ?? detectLevel([first])
            if p.level == .info, levelFromJSON(pairs) == nil { p.level = .info }
            var msg: String?
            for (k, v) in pairs where msgKeys.contains(k.lowercased()) { if case .string(let s) = v { msg = s; break } }
            for (k, v) in pairs where ridKeys.contains(k.lowercased()) { if case .string(let s) = v { p.requestId = s } }
            let skip = Set(levelKeys + timeKeys + msgKeys + ridKeys)
            for (k, v) in pairs where !skip.contains(k.lowercased()) && p.chips.count < 3 {
                switch v {
                case .string(let s) where s.count <= 24 && !s.contains("\n"): p.chips.append((k, s))
                case .number(let n): p.chips.append((k, n))
                case .bool(let b): p.chips.append((k, b ? "true" : "false"))
                default: break
                }
            }
            p.summary = msg ?? String(raw.prefix(240))
            return p
        }

        // Formatos de los runtimes de Lambda: "<fecha>\t<uuid>\tNIVEL\tmensaje" (Node) y "[NIVEL]\t<fecha>\t<uuid>\tmensaje" (Python)
        var body = first, wordLevel: LogLevel?
        let parts = first.components(separatedBy: "\t")
        if parts.count >= 4, rx("^" + uuid + "$", parts[1]) != nil, let l = levelFromWord(parts[2]) {
            p.requestId = parts[1]; wordLevel = l; body = parts[3...].joined(separator: "\t")
        } else if parts.count >= 4, rx(#"^\[(\w+)\]$"#, parts[0]) != nil, rx("^" + uuid + "$", parts[2]) != nil {
            p.requestId = parts[2]; wordLevel = levelFromWord(String(parts[0].dropFirst().dropLast())); body = parts[3...].joined(separator: "\t")
        }
        p.level = wordLevel ?? detectLevel(p.lines)
        p.summary = stripLevelPrefix(body, p.level)
        let last = p.lines.last ?? ""
        p.isTraceback = p.lines.contains { $0.hasPrefix("Traceback (most recent call last)") }
            || (p.lines.count > 2 && rx(#"^\s*[A-Za-z_][\w.]*(Error|Exception)(:|$)"#, last) != nil)
        return p
    }
}

// MARK: - Invocaciones y filas

struct RidTracker {
    private var current: [String: String] = [:]
    /// Asigna a cada evento la invocación a la que pertenece: la indicada en el propio evento, o la que está en curso en su stream.
    mutating func assign(_ p: inout ParsedLog) {
        let stream = p.event.stream
        if p.platform == .start, let rid = p.requestId { current[stream] = rid }
        if p.requestId == nil { p.requestId = current[stream] }
        if p.platform == .end { current[stream] = nil }
    }
}

struct LogGroupInfo {
    var rid: String
    var count = 0
    var hasError = false
    var report: LambdaReport?
    var started = false, ended = false
}

enum LogRowModel: Equatable { case group(String), event(Int) }

enum LogGrouping {
    static func infos(_ parsed: [ParsedLog]) -> [String: LogGroupInfo] {
        var out: [String: LogGroupInfo] = [:]
        for p in parsed {
            guard let rid = p.requestId else { continue }
            var g = out[rid] ?? LogGroupInfo(rid: rid)
            if p.platform == nil || p.platform == .other { g.count += 1 }
            if p.level == .error { g.hasError = true }
            if p.platform == .start { g.started = true }
            if p.platform == .end { g.ended = true }
            if p.platform == .report { g.report = p.report; g.ended = true }
            out[rid] = g
        }
        return out
    }

    /// Orden y contenido de las filas visibles. Con `group`, los eventos de una invocación se reúnen bajo un encabezado
    /// y las líneas START/END/REPORT se resumen en él.
    static func rows(parsed: [ParsedLog], group: Bool, filter: LogLevel?, collapsed: Set<String>) -> [LogRowModel] {
        var out: [LogRowModel] = []
        var emitted = Set<String>()
        var byRid: [String: [Int]] = [:]
        if group {
            for (i, p) in parsed.enumerated() {
                guard let rid = p.requestId, !(p.platform == .start || p.platform == .end || p.platform == .report) || filter == .platform else { continue }
                byRid[rid, default: []].append(i)
            }
        }
        for (i, p) in parsed.enumerated() {
            if let f = filter, p.level != f { continue }
            if group, let rid = p.requestId, !(p.platform == .start || p.platform == .end || p.platform == .report) || filter == .platform {
                guard emitted.insert(rid).inserted else { continue }
                out.append(.group(rid))
                if !collapsed.contains(rid) { for j in byRid[rid] ?? [] where filter == nil || parsed[j].level == filter { out.append(.event(j)) } }
                continue
            }
            if group, p.requestId != nil, p.platform == .start || p.platform == .end || p.platform == .report, filter != .platform { continue }
            out.append(.event(i))
        }
        return out
    }
}
