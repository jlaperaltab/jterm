import AppKit

private let s3DateFmt: DateFormatter = {
    let f = DateFormatter(); f.dateFormat = "d MMM yyyy HH:mm"; return f
}()

/// Cuenta bytes de varias transferencias a la vez (se actualiza desde hilos distintos).
private final class Meter {
    private let lock = NSLock()
    private var finished: Int64 = 0, finishedCount = 0
    private var current: [Int: Int64] = [:]
    func set(_ i: Int, _ v: Int64) { lock.lock(); current[i] = v; lock.unlock() }
    func finish(_ i: Int, size: Int64) { lock.lock(); current[i] = nil; finished += size; finishedCount += 1; lock.unlock() }
    var snapshot: (bytes: Int64, files: Int) { lock.lock(); defer { lock.unlock() }; return (finished + current.values.reduce(0, +), finishedCount) }
}

// MARK: - Pestaña

/// Explorador de S3: tu Mac a la izquierda y los buckets de la cuenta a la derecha.
final class S3Tab: Tab {
    let id = UUID()
    var title: String
    let iconName = "shippingbox"
    let view: S3TransferView
    var content: NSView { view }
    var focusView: NSView { view }
    var onExit: ((Tab) -> Void)?
    var fileTarget: (() async throws -> SSHTarget)? { nil }
    var connectionKey: String? { nil }
    var isConnected: Bool { false }
    let env: ServerEnv?

    init(account: AWSAccount, credentials: @escaping () throws -> AWSCredentials) {
        let name = account.name.isEmpty ? account.accessKeyId : account.name
        title = "S3 · " + name
        env = account.env
        view = S3TransferView(accountTitle: name, client: S3Client(defaultRegion: account.region, credentials: credentials))
    }
    func shutdown() { view.cancelAll() }
}

// MARK: - Vista

final class S3TransferView: NSView {
    let local = FilePane(kind: .local)
    let remote = FilePane(kind: .remote)
    private let queue = NSStackView()
    private let client: S3Client
    private var tasks: [UUID: Task<Void, Never>] = [:]

    init(accountTitle: String, client: S3Client) {
        self.client = client
        super.init(frame: .zero)
        wantsLayer = true; layer?.backgroundColor = Theme.bg.cgColor

        local.setTitle("Este Mac")
        local.homePath = NSHomeDirectory()
        local.loader = { dir in try FileSource.local(dir) }
        local.onMkdir = { path in try? FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: false) }
        local.onDelete = { items in for i in items { try FileManager.default.trashItem(at: URL(fileURLWithPath: i.path), resultingItemURL: nil) } }
        local.onRename = { item, newPath in try FileManager.default.moveItem(atPath: item.path, toPath: newPath) }
        local.onDrop = { [weak self] paths, dest in self?.download(paths, to: dest) }
        local.onTransferSelected = { [weak self] in self?.uploadSelected() }

        remote.setTitle("S3 · " + accountTitle)
        remote.homePath = "/"
        remote.loader = { [weak self] p in
            guard let self else { return [] }
            return try await self.load(p)
        }
        remote.onMkdir = { [weak self] path in await self?.makeFolder(path) }
        remote.onDelete = { [weak self] items in try await self?.delete(items) }
        remote.onRename = { [weak self] item, newPath in try await self?.rename(item, to: newPath) }
        remote.onDrop = { [weak self] paths, dest in self?.upload(paths, to: dest) }
        remote.onTransferSelected = { [weak self] in self?.downloadSelected() }
        remote.extraActions = { [weak self] item in self?.menuActions(for: item) ?? [] }

        let toRemote = IconButton(symbol: "arrow.right", tip: "Subir selección a S3", size: 32, target: self, action: #selector(uploadTapped))
        let toLocal = IconButton(symbol: "arrow.left", tip: "Descargar selección a este Mac", size: 32, target: self, action: #selector(downloadTapped))
        let strip = NSStackView(views: [toRemote, toLocal]); strip.orientation = .vertical; strip.spacing = 8

        queue.orientation = .vertical; queue.spacing = 6; queue.alignment = .leading

        for v in [local, strip, remote, queue] { v.translatesAutoresizingMaskIntoConstraints = false; addSubview(v) }
        NSLayoutConstraint.activate([
            local.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            local.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            strip.leadingAnchor.constraint(equalTo: local.trailingAnchor, constant: 4),
            strip.centerYAnchor.constraint(equalTo: local.centerYAnchor),
            strip.widthAnchor.constraint(equalToConstant: 36),
            remote.leadingAnchor.constraint(equalTo: strip.trailingAnchor, constant: 4),
            remote.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            remote.topAnchor.constraint(equalTo: local.topAnchor),
            remote.widthAnchor.constraint(equalTo: local.widthAnchor),
            local.bottomAnchor.constraint(equalTo: queue.topAnchor, constant: -8),
            remote.bottomAnchor.constraint(equalTo: local.bottomAnchor),
            queue.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            queue.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            queue.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -8),
        ])
        local.navigate(Settings.shared.resolvedLocalStart)
        remote.navigate("/")          // lista los buckets: es la primera llamada a AWS y ocurre al abrir la pestaña
    }
    required init?(coder: NSCoder) { fatalError() }

    func cancelAll() { tasks.values.forEach { $0.cancel() }; tasks.removeAll() }

    // MARK: Listado

    private func load(_ path: String) async throws -> [FileItem] {
        let (bucket, key) = S3Path.parse(path)
        guard let bucket else {
            return try await client.buckets().map {
                FileItem(name: $0.name, path: S3Path.path(bucket: $0.name), isDir: true, size: 0,
                         modified: $0.created.map(s3DateFmt.string) ?? "")
            }
        }
        let prefix = S3Path.prefix(key)
        let listing = try await client.list(bucket: bucket, prefix: prefix)
        var out: [FileItem] = []
        for p in listing.prefixes {
            let k = String(p.dropLast())
            out.append(FileItem(name: String(k.dropFirst(prefix.count)), path: S3Path.path(bucket: bucket, key: k), isDir: true, size: 0, modified: ""))
        }
        for o in listing.objects where o.key != prefix {         // el propio marcador de la carpeta no se muestra
            out.append(FileItem(name: String(o.key.dropFirst(prefix.count)), path: S3Path.path(bucket: bucket, key: o.key),
                                isDir: false, size: o.size, modified: o.modified.map(s3DateFmt.string) ?? ""))
        }
        return out
    }

    // MARK: Cambios

    private func fail(_ title: String, _ text: String) {
        DispatchQueue.main.async { showAlert(title, text, window: self.window) }
    }

    private func makeFolder(_ path: String) async {
        let (bucket, key) = S3Path.parse(path)
        guard let bucket, !key.isEmpty else { return fail("Abre un bucket primero", "Las carpetas se crean dentro de un bucket; los buckets no se crean desde jTerm.") }
        do { try await client.createFolder(bucket: bucket, prefix: key) }
        catch { fail("No se pudo crear la carpeta", error.localizedDescription) }
    }

    private func delete(_ items: [FileItem]) async throws {
        for item in items {
            let (bucket, key) = S3Path.parse(item.path)
            guard let bucket else { continue }
            if key.isEmpty { throw AWSError(message: "Los buckets no se pueden eliminar desde jTerm.") }
            if item.isDir {
                let all = try await client.list(bucket: bucket, prefix: S3Path.prefix(key), recursive: true).objects.map(\.key)
                try await client.delete(bucket: bucket, keys: all.isEmpty ? [S3Path.prefix(key)] : all)
            } else {
                try await client.delete(bucket: bucket, keys: [key])
            }
        }
    }

    private func rename(_ item: FileItem, to newPath: String) async throws {
        guard !item.isDir else { throw AWSError(message: "S3 no puede renombrar carpetas: habría que copiar y borrar cada objeto.") }
        let (b1, k1) = S3Path.parse(item.path), (b2, k2) = S3Path.parse(newPath)
        guard let b1, let b2, b1 == b2, !k1.isEmpty, !k2.isEmpty else { throw AWSError(message: "No se puede renombrar este elemento.") }
        try await client.copy(bucket: b1, from: k1, to: k2)
        try await client.delete(bucket: b1, keys: [k1])
    }

    private func menuActions(for item: FileItem) -> [(title: String, run: () -> Void)] {
        let (bucket, key) = S3Path.parse(item.path)
        guard let bucket else { return [] }
        var out: [(title: String, run: () -> Void)] = []
        let uri = "s3://" + bucket + (key.isEmpty ? "" : "/" + key) + (item.isDir ? "/" : "")
        if !item.isDir {
            out.append(("Copiar URL firmada (1 hora)", { [weak self] in
                guard let self else { return }
                Task {
                    do {
                        let url = try await self.client.presignedURL(bucket: bucket, key: key, expires: 3600)
                        await MainActor.run {
                            NSPasteboard.general.clearContents(); NSPasteboard.general.setString(url.absoluteString, forType: .string)
                            NSSound(named: "Pop")?.play()
                        }
                    } catch { self.fail("No se pudo crear la URL", error.localizedDescription) }
                }
            }))
        }
        out.append(("Copiar ruta de S3", { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(uri, forType: .string) }))
        return out
    }

    // MARK: Transferencias

    @objc private func uploadTapped() { uploadSelected() }
    @objc private func downloadTapped() { downloadSelected() }
    private func uploadSelected() {
        let sel = local.selectedItems.map(\.path)
        guard !sel.isEmpty else { return NSSound.beep() }
        upload(sel, to: remote.path)
    }
    private func downloadSelected() {
        let sel = remote.selectedItems
        guard !sel.isEmpty else { return NSSound.beep() }
        download(sel.map(\.path), to: local.path)
    }
    private func describe(_ p: [String]) -> String { p.count == 1 ? (p[0] as NSString).lastPathComponent : "\(p.count) elementos" }
    private func fmt(_ b: Int64) -> String { ByteCountFormatter.string(fromByteCount: b, countStyle: .file) }

    /// Una fila en la cola de transferencias y la tarea que la ejecuta (se puede cancelar).
    private func runJob(_ title: String, upload: Bool, done: @escaping () -> Void,
                        work: @escaping (@escaping (Int, String) -> Void) async throws -> Void) {
        let jobId = UUID()
        let row = TransferRow(title: title, upload: upload)
        row.onDismiss = { [weak self, weak row] in if let row { self?.queue.removeArrangedSubview(row); row.removeFromSuperview() } }
        queue.addArrangedSubview(row)
        row.widthAnchor.constraint(equalTo: queue.widthAnchor).isActive = true
        let task = Task { [weak self] in
            do {
                try await work { pct, text in DispatchQueue.main.async { row.update(pct, text) } }
                await MainActor.run { row.finish(error: nil); done() }
            } catch {
                let cancelled = error is CancellationError || (error as? URLError)?.code == .cancelled
                await MainActor.run { row.finish(error: cancelled ? "Cancelado" : error.localizedDescription); done() }
            }
            await MainActor.run { self?.tasks[jobId] = nil }
        }
        tasks[jobId] = task
        row.onCancel = { task.cancel() }
    }

    /// Ejecuta `perform` para cada elemento, de 4 en 4, informando el avance total.
    private func transfer<T>(_ items: [T], size: @escaping (T) -> Int64, report: @escaping (Int, String) -> Void,
                             perform: @escaping (T, @escaping (Int64) -> Void) async throws -> Void) async throws {
        let total = max(items.reduce(Int64(0)) { $0 + size($1) }, 1)
        let meter = Meter()
        let fmt = self.fmt
        let publish: () -> Void = {
            let s = meter.snapshot
            report(Int(min(100, s.bytes * 100 / total)), "\(s.files)/\(items.count) archivos · \(fmt(s.bytes)) de \(fmt(total))")
        }
        try await withThrowingTaskGroup(of: Void.self) { group in
            var next = 0
            func add() {
                guard next < items.count else { return }
                let i = next; next += 1
                group.addTask {
                    try Task.checkCancellation()
                    try await perform(items[i]) { meter.set(i, $0); publish() }
                    meter.finish(i, size: size(items[i])); publish()
                }
            }
            for _ in 0..<min(4, items.count) { add() }
            while try await group.next() != nil { add() }
        }
    }

    func upload(_ paths: [String], to dest: String) {
        let (bucket, key) = S3Path.parse(dest)
        guard let bucket else {
            return showAlert("Abre un bucket primero", "Entra a un bucket (o a una carpeta dentro de él) para subir archivos.", window: window)
        }
        let base = S3Path.prefix(key)
        let client = self.client
        runJob("Subiendo \(describe(paths))", upload: true, done: { [weak self] in self?.remote.reload() }) { report in
            let (files, emptyDirs) = S3TransferView.scanLocal(paths, base: base)
            for d in emptyDirs { try await client.createFolder(bucket: bucket, prefix: d) }
            try await self.transfer(files, size: { $0.size }, report: report) { f, progress in
                try await client.upload(file: f.url, bucket: bucket, key: f.key, progress: progress)
            }
            if files.isEmpty { report(100, "Nada que subir") }
        }
    }

    /// Recorre las rutas locales: archivos con su clave de S3, y carpetas vacías (se crean como marcadores).
    /// Va aparte, en una función síncrona, porque el enumerador de carpetas no se puede usar dentro de código asíncrono.
    private static func scanLocal(_ paths: [String], base: String) -> (files: [(url: URL, key: String, size: Int64)], emptyDirs: [String]) {
        var files: [(url: URL, key: String, size: Int64)] = []
        var emptyDirs: [String] = []
        let fm = FileManager.default
        for p in paths {
            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: p, isDirectory: &isDir) else { continue }
            let url = URL(fileURLWithPath: p)
            if !isDir.boolValue {
                let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize).map(Int64.init) ?? 0
                files.append((url, base + url.lastPathComponent, size)); continue
            }
            let root = base + url.lastPathComponent + "/"
            var any = false, foundEmpty = false
            let keys: [URLResourceKey] = [.isDirectoryKey, .fileSizeKey]
            if let en = fm.enumerator(at: url, includingPropertiesForKeys: keys, options: []) {
                for case let f as URL in en {
                    let v = try? f.resourceValues(forKeys: Set(keys))
                    let rel = String(f.path.dropFirst(url.path.count + 1))
                    if v?.isDirectory == true {
                        if ((try? fm.contentsOfDirectory(atPath: f.path)) ?? []).isEmpty { emptyDirs.append(root + rel + "/"); foundEmpty = true }
                    } else { any = true; files.append((f, root + rel, Int64(v?.fileSize ?? 0))) }
                }
            }
            if !any && !foundEmpty { emptyDirs.append(root) }       // carpeta vacía: se sube solo su marcador
        }
        return (files, emptyDirs)
    }

    func download(_ paths: [String], to dest: String) {
        guard !dest.isEmpty else { return }
        let base = URL(fileURLWithPath: dest, isDirectory: true)
        let client = self.client
        let selected = paths.map { p -> (path: String, isDir: Bool, size: Int64) in
            let it = remote.item(atPath: p); return (p, it?.isDir ?? false, it?.size ?? 0)
        }
        runJob("Descargando \(describe(paths))", upload: false, done: { [weak self] in self?.local.reload() }) { report in
            var files: [(bucket: String, key: String, dest: URL, size: Int64)] = []
            var skipped = 0
            for s in selected {
                let (bucket, key) = S3Path.parse(s.path)
                guard let bucket else { continue }
                if !s.isDir {
                    files.append((bucket, key, base.appendingPathComponent((key as NSString).lastPathComponent), s.size)); continue
                }
                let prefix = S3Path.prefix(key)
                let name = key.isEmpty ? bucket : (key as NSString).lastPathComponent
                let root = base.appendingPathComponent(name, isDirectory: true)
                try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
                for o in try await client.list(bucket: bucket, prefix: prefix, recursive: true).objects {
                    let rel = String(o.key.dropFirst(prefix.count))
                    guard let dest = S3Path.safeDestination(base: root, relative: rel) else { if !rel.isEmpty { skipped += 1 }; continue }
                    if o.key.hasSuffix("/") { try FileManager.default.createDirectory(at: dest, withIntermediateDirectories: true) }
                    else { files.append((bucket, o.key, dest, o.size)) }
                }
            }
            try await self.transfer(files, size: { $0.size }, report: report) { f, progress in
                try await client.download(bucket: f.bucket, key: f.key, to: f.dest, progress: progress)
            }
            if skipped > 0 { throw AWSError(message: "Se omitieron \(skipped) objetos con nombres que salían de la carpeta de destino.") }
        }
    }
}
