import AppKit
import CryptoKit

/// Busca versiones nuevas en los Releases de GitHub y se actualiza sola.
///
/// Flujo: consulta `releases/latest` → compara con la versión instalada → descarga el .dmg del release
/// → verifica su SHA-256 (archivo `.dmg.sha256` del mismo release) → copia la .app nueva junto a la actual
/// → un script auxiliar espera a que jTerm se cierre, intercambia las apps y la vuelve a abrir.
/// El repositorio se lee de la clave `JTermUpdateRepo` del Info.plist ("usuario/repo"; la pone build_app.sh).
@MainActor
final class Updater {
    static let shared = Updater()

    struct Release {
        let version: String, notes: String, dmg: URL, sha: URL?
    }
    enum UpdateError: LocalizedError {
        case msg(String)
        var errorDescription: String? { if case .msg(let m) = self { return m } else { return nil } }
    }

    private let lastCheckKey = "update.lastCheck"
    private var busy = false
    private var progress: NSPanel?

    var repo: String? {
        let r = (Bundle.main.object(forInfoDictionaryKey: "JTermUpdateRepo") as? String)?.trimmingCharacters(in: .whitespaces)
        return (r?.isEmpty ?? true) ? nil : r
    }
    var currentVersion: String? { Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String }

    // MARK: Entrada

    /// Comprobación silenciosa al abrir la app: como mucho una vez cada 24 h y solo avisa si hay algo nuevo.
    func checkOnLaunch() {
        guard repo != nil, currentVersion != nil else { return }
        let last = UserDefaults.standard.double(forKey: lastCheckKey)
        guard Date().timeIntervalSince1970 - last > 24 * 3600 else { return }
        Task { await check(manual: false) }
    }

    /// Desde el menú "Buscar actualizaciones…".
    func checkManually() { Task { await check(manual: true) } }

    private func check(manual: Bool) async {
        guard !busy else { return }
        guard let repo, let current = currentVersion else {
            if manual { info("Las actualizaciones no están disponibles", "Esta compilación no tiene configurado el repositorio de GitHub (ejecútala desde la app empaquetada con build_app.sh).") }
            return
        }
        busy = true; defer { busy = false }
        var installing = false
        do {
            let rel = try await fetchLatest(repo: repo)
            UserDefaults.standard.set(Date().timeIntervalSince1970, forKey: lastCheckKey)
            guard Self.isNewer(rel.version, than: current) else {
                if manual { info("jTerm está al día", "Tienes la versión \(current), que es la más reciente.") }
                return
            }
            let a = NSAlert()
            a.messageText = "Hay una versión nueva: jTerm \(rel.version)"
            a.informativeText = "Tienes la \(current).\(rel.notes.isEmpty ? "" : "\n\n" + String(rel.notes.prefix(600)))"
            a.addButton(withTitle: "Actualizar ahora"); a.addButton(withTitle: "Más tarde")
            guard a.runModal() == .alertFirstButtonReturn else { return }
            installing = true
            try await install(rel)
        } catch {
            hideProgress()
            if manual || installing { info("No se pudo actualizar", error.localizedDescription) }
            else { NSLog("jTerm updater: \(error.localizedDescription)") }
        }
    }

    // MARK: GitHub

    private func fetchLatest(repo: String) async throws -> Release {
        var req = URLRequest(url: URL(string: "https://api.github.com/repos/\(repo)/releases/latest")!)
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        req.setValue("jTerm-updater", forHTTPHeaderField: "User-Agent")
        req.timeoutInterval = 15
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse else { throw UpdateError.msg("Respuesta no válida.") }
        if http.statusCode == 404 { throw UpdateError.msg("No hay ningún release publicado en \(repo) (o el repositorio no es público).") }
        guard http.statusCode == 200, let j = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let tag = j["tag_name"] as? String else { throw UpdateError.msg("GitHub respondió con el código \(http.statusCode).") }
        let assets = (j["assets"] as? [[String: Any]] ?? []).compactMap { a -> (String, URL)? in
            guard let n = a["name"] as? String, let u = (a["browser_download_url"] as? String).flatMap(URL.init) else { return nil }
            return (n, u)
        }
        guard let dmg = assets.first(where: { $0.0.hasSuffix(".dmg") }) else { throw UpdateError.msg("El release \(tag) no incluye un archivo .dmg.") }
        let sha = assets.first(where: { $0.0 == dmg.0 + ".sha256" })
        let version = tag.hasPrefix("v") || tag.hasPrefix("V") ? String(tag.dropFirst()) : tag
        return Release(version: version, notes: (j["body"] as? String) ?? "", dmg: dmg.1, sha: sha?.1)
    }

    static func isNewer(_ a: String, than b: String) -> Bool {
        func parts(_ s: String) -> [Int] { s.split(separator: "-").first.map { $0.split(separator: ".").map { Int($0) ?? 0 } } ?? [] }
        let x = parts(a), y = parts(b)
        for i in 0..<max(x.count, y.count) {
            let p = i < x.count ? x[i] : 0, q = i < y.count ? y[i] : 0
            if p != q { return p > q }
        }
        return false
    }

    // MARK: Instalación

    private func install(_ rel: Release) async throws {
        guard let shaURL = rel.sha else { throw UpdateError.msg("El release no incluye el archivo .sha256; por seguridad no se instala.") }
        let dest = Bundle.main.bundleURL
        guard dest.pathExtension == "app" else { throw UpdateError.msg("jTerm no se está ejecutando desde una .app.") }
        let fm = FileManager.default
        guard fm.isWritableFile(atPath: dest.deletingLastPathComponent().path) else {
            // Sin permiso de escritura (p. ej. instalada por otro usuario): descarga manual.
            NSWorkspace.shared.open(rel.dmg)
            throw UpdateError.msg("No tengo permiso para modificar \(dest.deletingLastPathComponent().path). Se descargará el instalador para que lo arrastres tú.")
        }
        showProgress("Descargando jTerm \(rel.version)…")
        defer { hideProgress() }

        let work = fm.temporaryDirectory.appendingPathComponent("jterm-update-\(UUID().uuidString)")
        try fm.createDirectory(at: work, withIntermediateDirectories: true)
        var keepWork = false
        defer { if !keepWork { try? fm.removeItem(at: work) } }

        // 1) Descarga + verificación del hash
        let (tmp, _) = try await URLSession.shared.download(from: rel.dmg)
        let dmgFile = work.appendingPathComponent("update.dmg")
        try fm.moveItem(at: tmp, to: dmgFile)
        let (shaTmp, _) = try await URLSession.shared.download(from: shaURL)
        let expected = (try String(contentsOf: shaTmp, encoding: .utf8)).split(whereSeparator: \.isWhitespace).first.map { String($0).lowercased() } ?? ""
        try? fm.removeItem(at: shaTmp)
        let actual = try await Task.detached { () -> String in
            SHA256.hash(data: try Data(contentsOf: dmgFile, options: .mappedIfSafe)).map { String(format: "%02x", $0) }.joined()
        }.value
        guard expected.count == 64, expected == actual else { throw UpdateError.msg("La descarga está dañada (el SHA-256 no coincide). No se instaló.") }

        // 2) Montar el DMG y copiar la app junto a la actual (mismo volumen → el intercambio final es atómico)
        showProgress("Preparando la actualización…")
        let mount = work.appendingPathComponent("mnt")
        try fm.createDirectory(at: mount, withIntermediateDirectories: true)
        let staged = dest.deletingLastPathComponent().appendingPathComponent(".jTerm-update.app")
        try? fm.removeItem(at: staged)
        let attach = await Self.run("/usr/bin/hdiutil", ["attach", "-nobrowse", "-readonly", "-noverify", "-noautoopen", "-mountpoint", mount.path, dmgFile.path])
        guard attach.status == 0 else { throw UpdateError.msg("No se pudo abrir el instalador descargado.") }
        let copy: (status: Int32, out: String)
        copy = await Self.run("/usr/bin/ditto", [mount.appendingPathComponent("jTerm.app").path, staged.path])
        _ = await Self.run("/usr/bin/hdiutil", ["detach", "-force", mount.path])
        guard copy.status == 0 else { try? fm.removeItem(at: staged); throw UpdateError.msg("No se pudo copiar la nueva versión.") }

        // 3) Comprobar que lo copiado es jTerm y la versión esperada
        let plist = NSDictionary(contentsOf: staged.appendingPathComponent("Contents/Info.plist"))
        guard plist?["CFBundleIdentifier"] as? String == Bundle.main.bundleIdentifier,
              plist?["CFBundleShortVersionString"] as? String == rel.version else {
            try? fm.removeItem(at: staged)
            throw UpdateError.msg("El paquete descargado no corresponde a jTerm \(rel.version).")
        }

        // 4) Script auxiliar: espera a que salgamos, intercambia y reabre
        let script = work.appendingPathComponent("swap.sh")
        try """
        #!/bin/sh
        PID="$1"; DEST="$2"; NEW="$3"; WORK="$4"
        i=0
        while kill -0 "$PID" 2>/dev/null; do
          i=$((i+1)); [ $i -gt 240 ] && { rm -rf "$NEW" "$WORK"; exit 1; }   # el usuario canceló la salida
          sleep 0.5
        done
        OLD="$DEST.old"
        rm -rf "$OLD"
        mv "$DEST" "$OLD" && mv "$NEW" "$DEST" || { [ -e "$DEST" ] || mv "$OLD" "$DEST"; rm -rf "$NEW" "$WORK"; exit 1; }
        xattr -dr com.apple.quarantine "$DEST" 2>/dev/null
        rm -rf "$OLD"
        open "$DEST"
        rm -rf "$WORK"
        """.write(to: script, atomically: true, encoding: .utf8)
        keepWork = true
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", "nohup /bin/sh \"$0\" \"$@\" >/dev/null 2>&1 &", script.path, String(ProcessInfo.processInfo.processIdentifier), dest.path, staged.path, work.path]
        try p.run()
        hideProgress()
        NSApp.terminate(nil)
    }

    nonisolated private static func run(_ exe: String, _ args: [String]) async -> (status: Int32, out: String) {
        await Task.detached {
            let p = Process(), pipe = Pipe()
            p.executableURL = URL(fileURLWithPath: exe); p.arguments = args
            p.standardOutput = pipe; p.standardError = pipe
            do { try p.run() } catch { return (-1, "\(error)") }
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            p.waitUntilExit()
            return (p.terminationStatus, String(data: data, encoding: .utf8) ?? "")
        }.value
    }

    // MARK: UI

    private func info(_ title: String, _ text: String) {
        let a = NSAlert(); a.messageText = title; a.informativeText = text; a.addButton(withTitle: "OK"); a.runModal()
    }

    private func showProgress(_ text: String) {
        if progress == nil {
            let w = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 320, height: 90), styleMask: [.titled], backing: .buffered, defer: false)
            w.title = "Actualizando jTerm"
            let label = NSTextField(labelWithString: text); label.tag = 1
            let bar = NSProgressIndicator(); bar.style = .bar; bar.isIndeterminate = true; bar.startAnimation(nil)
            let stack = NSStackView(views: [label, bar]); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 10
            stack.translatesAutoresizingMaskIntoConstraints = false
            w.contentView!.addSubview(stack)
            NSLayoutConstraint.activate([
                stack.leadingAnchor.constraint(equalTo: w.contentView!.leadingAnchor, constant: 20),
                stack.trailingAnchor.constraint(equalTo: w.contentView!.trailingAnchor, constant: -20),
                stack.centerYAnchor.constraint(equalTo: w.contentView!.centerYAnchor),
                bar.widthAnchor.constraint(equalTo: stack.widthAnchor),
            ])
            w.center(); w.makeKeyAndOrderFront(nil); progress = w
        } else {
            (progress?.contentView?.viewWithTag(1) as? NSTextField)?.stringValue = text
        }
    }
    private func hideProgress() { progress?.orderOut(nil); progress = nil }
}
