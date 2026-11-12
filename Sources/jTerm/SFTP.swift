import Foundation

// MARK: - Utilidades

struct ProcResult { var status: Int32; var out: String; var err: String }

private final class DataBox { var data = Data() }

func runProcess(_ exe: String, _ args: [String], env: [String: String] = [:]) async -> ProcResult {
    await withCheckedContinuation { (cont: CheckedContinuation<ProcResult, Never>) in
        let p = Process()
        p.executableURL = URL(fileURLWithPath: exe); p.arguments = args
        var e = ProcessInfo.processInfo.environment
        for (k, v) in env { e[k] = v }
        p.environment = e
        let o = Pipe(), er = Pipe()
        p.standardOutput = o; p.standardError = er; p.standardInput = FileHandle.nullDevice
        do { try p.run() } catch {
            cont.resume(returning: ProcResult(status: -1, out: "", err: error.localizedDescription)); return
        }
        DispatchQueue.global().async {
            let od = DataBox(), ed = DataBox()
            let g = DispatchGroup()
            g.enter(); DispatchQueue.global().async { od.data = o.fileHandleForReading.readDataToEndOfFile(); g.leave() }
            g.enter(); DispatchQueue.global().async { ed.data = er.fileHandleForReading.readDataToEndOfFile(); g.leave() }
            g.wait(); p.waitUntilExit()
            cont.resume(returning: ProcResult(status: p.terminationStatus,
                                              out: String(decoding: od.data, as: UTF8.self),
                                              err: String(decoding: ed.data, as: UTF8.self)))
        }
    }
}

extension String {
    var trimmed: String { trimmingCharacters(in: .whitespacesAndNewlines) }
}

/// Comillas y escapes para el modo batch de sftp.
enum SFTPQuote {
    /// sftp usa el nombre literal si existe, así que solo se escapan barras invertidas y comillas dobles.
    static func arg(_ s: String) -> String {
        "\"" + s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
    }
    /// Comillas simples para un comando de shell remoto.
    static func shell(_ s: String) -> String { "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'" }
}

// MARK: - Parser de `ls -la` de sftp

struct RemoteEntry: Equatable {
    var name: String
    var isDir: Bool
    var isLink: Bool
    var size: Int64
    var modified: String
}

enum SFTPParser {
    private static let re = try! NSRegularExpression(
        pattern: #"^([\-dlcbps])[rwxsStT\-]{9}[+@.]?\s+\S+\s+(\S+)\s+(\S+)\s+(\d+)\s+([A-Za-z]{3})\s+(\d{1,2})\s+(\d{1,2}:\d{2}|\d{4})\s+(.+)$"#)

    static func parseLs(_ text: String) -> [RemoteEntry] {
        var out: [RemoteEntry] = []
        for raw in text.split(whereSeparator: { $0 == "\n" || $0 == "\r" }) {
            let line = String(raw)
            let ns = line as NSString
            guard let m = re.firstMatch(in: line, range: NSRange(location: 0, length: ns.length)) else { continue }
            let kind = ns.substring(with: m.range(at: 1))
            var name = ns.substring(with: m.range(at: 8))
            let isLink = kind == "l"
            if isLink, let r = name.range(of: " -> ") { name = String(name[..<r.lowerBound]) }
            // Algunas versiones imprimen la ruta completa: nos quedamos con el último componente.
            if let slash = name.lastIndex(of: "/") { name = String(name[name.index(after: slash)...]) }
            if name == "." || name == ".." || name.isEmpty { continue }
            out.append(RemoteEntry(name: name, isDir: kind == "d", isLink: isLink,
                                   size: Int64(ns.substring(with: m.range(at: 4))) ?? 0,
                                   modified: "\(ns.substring(with: m.range(at: 5))) \(ns.substring(with: m.range(at: 6))) \(ns.substring(with: m.range(at: 7)))"))
        }
        return out
    }
}

// MARK: - Sesión

final class TransferJob {
    let id = UUID()
    var process: Process?
    private(set) var cancelled = false
    func cancel() { cancelled = true; process?.terminate() }
}

private final class TextBox { var text = "" }

/// Una conexión SSH maestra (ControlMaster) reutilizada por todas las operaciones sftp/ssh:
/// la contraseña o la llave temporal se usan una sola vez.
final class SFTPSession {
    let target: SSHTarget
    private let sock: String
    var socketPath: String { sock }
    private var master: Process?
    private let masterErr = Pipe()
    private(set) var home = "/"
    private var closed = false
    var onDisconnect: (() -> Void)?
    var isAlive: Bool { !closed && master?.isRunning == true }

    static var ctlDir: String {
        let d = "/tmp/jterm-\(getuid())"
        try? FileManager.default.createDirectory(atPath: d, withIntermediateDirectories: true,
                                                 attributes: [.posixPermissions: 0o700])
        return d
    }

    init(target: SSHTarget) {
        self.target = target
        sock = Self.ctlDir + "/" + String(UUID().uuidString.prefix(8)).lowercased() + ".sock"
    }

    private var env: [String: String] { target.env }

    func connect() async throws {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
        p.arguments = ["-M", "-N", "-S", sock, "-o", "ControlPersist=no", "-o", "ConnectTimeout=15", "-p", String(target.port)]
            + SSHSettings.args() + target.options + [target.destination]
        var e = ProcessInfo.processInfo.environment
        for (k, v) in env { e[k] = v }
        p.environment = e
        p.standardInput = FileHandle.nullDevice; p.standardOutput = FileHandle.nullDevice; p.standardError = masterErr
        p.terminationHandler = { [weak self] _ in
            guard let self, !self.closed else { return }
            DispatchQueue.main.async { self.onDisconnect?() }
        }
        try p.run()
        master = p

        var ready = false
        for _ in 0..<100 {
            if !p.isRunning {
                let msg = String(decoding: masterErr.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self).trimmed
                throw AWSError(message: msg.isEmpty ? "La conexión SSH terminó inesperadamente." : msg)
            }
            if FileManager.default.fileExists(atPath: sock) {
                let r = await runProcess("/usr/bin/ssh", ["-S", sock, "-O", "check", target.destination])
                if r.status == 0 { ready = true; break }
            }
            try await Task.sleep(nanoseconds: 200_000_000)
        }
        guard ready else { close(); throw AWSError(message: "Tiempo de espera agotado al conectar con \(target.host).") }
        home = (try? await pwd()) ?? "/"
    }

    func close() {
        closed = true
        if master?.isRunning == true {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
            p.arguments = ["-S", sock, "-O", "exit", target.destination]
            p.standardOutput = FileHandle.nullDevice; p.standardError = FileHandle.nullDevice
            try? p.run(); p.waitUntilExit()
            master?.terminate()
        }
        master = nil
        try? FileManager.default.removeItem(atPath: sock)
    }

    // MARK: sftp (modo batch)

    private func writeBatch(_ commands: [String]) throws -> String {
        let path = Self.ctlDir + "/b-" + String(UUID().uuidString.prefix(8)).lowercased() + ".txt"
        try (commands.joined(separator: "\n") + "\n").write(toFile: path, atomically: true, encoding: .utf8)
        return path
    }

    private func clean(_ s: String) -> String {
        let s = s.replacingOccurrences(of: "^D", with: "").replacingOccurrences(of: "\u{04}", with: "").replacingOccurrences(of: "\u{08}", with: "")
        let noAnsi = s.replacingOccurrences(of: "\u{1b}\\[[0-9;?]*[A-Za-z]", with: "", options: .regularExpression)
        let lines = noAnsi.split(whereSeparator: { $0 == "\n" || $0 == "\r" })
            .map { String($0).trimmed }
            .filter { !$0.isEmpty && !$0.hasPrefix("sftp>") && !$0.contains("Permanently added") }
        return lines.suffix(3).joined(separator: "\n")
    }

    @discardableResult
    func sftp(_ commands: [String]) async throws -> String {
        let batch = try writeBatch(commands)
        defer { try? FileManager.default.removeItem(atPath: batch) }
        let r = await runProcess("/usr/bin/sftp", ["-q", "-o", "ControlPath=\(sock)", "-P", String(target.port),
                                                   "-b", batch, target.destination], env: env)
        guard r.status == 0 else {
            let msg = clean(r.err + "\n" + r.out)
            throw AWSError(message: msg.isEmpty ? "sftp terminó con código \(r.status)." : msg)
        }
        return r.out
    }

    func pwd() async throws -> String {
        let out = try await sftp(["pwd"])
        for line in out.split(separator: "\n") where line.contains("Remote working directory:") {
            return String(line.components(separatedBy: "Remote working directory:").last ?? "/").trimmed
        }
        return "/"
    }

    func list(_ path: String) async throws -> [RemoteEntry] {
        SFTPParser.parseLs(try await sftp(["ls -la \(SFTPQuote.arg(path))"]))
    }

    func mkdir(_ path: String) async throws { try await sftp(["mkdir \(SFTPQuote.arg(path))"]) }

    func rename(_ from: String, to: String) async throws {
        try await sftp(["rename \(SFTPQuote.arg(from)) \(SFTPQuote.arg(to))"])
    }

    func removeRemote(_ paths: [String]) async throws {
        let cmd = "rm -rf -- " + paths.map(SFTPQuote.shell).joined(separator: " ")
        let r = await runProcess("/usr/bin/ssh", ["-S", sock, target.destination, cmd], env: env)
        guard r.status == 0 else { throw AWSError(message: r.err.trimmed.isEmpty ? "No se pudo eliminar." : r.err.trimmed) }
    }

    // MARK: Transferencias con progreso

    enum Direction { case upload, download }

    /// scp sobre la conexión maestra, dentro de un pty (vía `script`) para obtener su barra de progreso.
    /// upload: `sources` son rutas locales y `destDir` remota; download: al revés.
    func runTransfer(_ job: TransferJob, direction: Direction, sources: [String], destDir: String,
                     progress: @escaping (Int, String) -> Void) async throws {
        let dest = target.destination
        var args = ["-q", "/dev/null", "/usr/bin/scp", "-r", "-o", "ControlPath=\(sock)", "-P", String(target.port)]
        switch direction {
        case .upload: args += sources + ["\(dest):\(destDir)"]
        case .download: args += sources.map { "\(dest):\($0)" } + [destDir]
        }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/script")
        p.arguments = args
        var e = ProcessInfo.processInfo.environment
        for (k, v) in env { e[k] = v }
        p.environment = e
        let out = Pipe()
        p.standardOutput = out; p.standardError = out; p.standardInput = FileHandle.nullDevice
        job.process = p

        let box = TextBox()
        let re = try! NSRegularExpression(pattern: #"^(.*?)\s+(\d{1,3})%\s+\S+\s+(\S+/s)"#)
        out.fileHandleForReading.readabilityHandler = { h in
            let d = h.availableData
            guard !d.isEmpty else { return }
            box.text += String(decoding: d, as: UTF8.self)
            if box.text.count > 4000 { box.text = String(box.text.suffix(2000)) }
            let plain = box.text.replacingOccurrences(of: "\u{1b}\\[[0-9;?]*[A-Za-z]", with: "", options: .regularExpression)
            let lines = plain.split(whereSeparator: { $0 == "\n" || $0 == "\r" }).map(String.init)
            if let last = lines.last(where: { $0.contains("%") }) {
                let ns = last as NSString
                if let m = re.firstMatch(in: last, range: NSRange(location: 0, length: ns.length)) {
                    let name = ns.substring(with: m.range(at: 1)).trimmed
                    progress(Int(ns.substring(with: m.range(at: 2))) ?? 0, "\(name) · \(ns.substring(with: m.range(at: 3)))")
                }
            }
        }
        try p.run()
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            DispatchQueue.global().async { p.waitUntilExit(); c.resume() }
        }
        out.fileHandleForReading.readabilityHandler = nil
        if job.cancelled { throw AWSError(message: "Cancelado") }
        guard p.terminationStatus == 0 else {
            let msg = clean(box.text).replacingOccurrences(of: "\u{04}\u{08}\u{08}", with: "")
            throw AWSError(message: msg.isEmpty ? "La transferencia falló (código \(p.terminationStatus))." : msg)
        }
    }
}
