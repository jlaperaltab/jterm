import AppKit

extension NSPasteboard.PasteboardType {
    static let jLocal = NSPasteboard.PasteboardType("com.jterm.local-path")
    static let jRemote = NSPasteboard.PasteboardType("com.jterm.remote-path")
}

struct FileItem {
    var name: String
    var path: String
    var isDir: Bool
    var size: Int64
    var modified: String
}

private let dateFmt: DateFormatter = {
    let f = DateFormatter(); f.dateFormat = "d MMM yyyy HH:mm"; return f
}()

enum FileSource {
    static func local(_ dir: String) throws -> [FileItem] {
        let keys: [URLResourceKey] = [.isDirectoryKey, .fileSizeKey, .contentModificationDateKey]
        let urls = try FileManager.default.contentsOfDirectory(at: URL(fileURLWithPath: dir), includingPropertiesForKeys: keys)
        return urls.map { u in
            let v = try? u.resourceValues(forKeys: Set(keys))
            return FileItem(name: u.lastPathComponent, path: u.path, isDir: v?.isDirectory ?? false,
                            size: Int64(v?.fileSize ?? 0), modified: v?.contentModificationDate.map(dateFmt.string) ?? "")
        }
    }
    static func remote(_ session: SFTPSession, _ dir: String) async throws -> [FileItem] {
        try await session.list(dir).map {
            FileItem(name: $0.name, path: (dir as NSString).appendingPathComponent($0.name),
                     isDir: $0.isDir, size: $0.size, modified: $0.modified)
        }
    }
}

// MARK: - Tabla con tecla Suprimir

final class FileTable: NSTableView {
    var onDeleteKey: (() -> Void)?
    var onReturnKey: (() -> Void)?
    override func keyDown(with e: NSEvent) {
        switch e.keyCode {
        case 51, 117: onDeleteKey?()          // Suprimir / Borrar
        case 36, 76: onReturnKey?()           // Retorno / Intro: renombrar
        default: super.keyDown(with: e)
        }
    }
}

// MARK: - Panel de archivos (local o remoto)

final class FilePane: NSView, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    enum Kind { case local, remote }
    let kind: Kind

    var loader: ((String) async throws -> [FileItem])?
    var homePath = ""
    var onDrop: (([String], String) -> Void)?
    var onTransferSelected: (() -> Void)?
    var onMkdir: ((String) async -> Void)?
    var onDelete: (([FileItem]) async throws -> Void)?
    var onRename: ((FileItem, String) async throws -> Void)?
    var onRetry: (() -> Void)?
    /// Acciones extra del menú contextual para un elemento (las usa el explorador de S3).
    var extraActions: ((FileItem) -> [(title: String, run: () -> Void)])?

    private(set) var path = ""
    private var items: [FileItem] = []
    private var visible: [FileItem] = []
    private var showHidden: Bool

    private let titleLabel = NSTextField(labelWithString: "")
    private let pathField = FocusField()
    private let pathBox = NSView()
    private let table = FileTable()
    private let errorLabel = NSTextField(wrappingLabelWithString: "")
    private let overlay = NSView()
    private let overlayLabel = NSTextField(wrappingLabelWithString: "")
    private let spinner = NSProgressIndicator()
    private var retryButton: PillButton!
    private var hiddenButton: IconButton!
    private var renameButton: IconButton!
    private var deleteButton: IconButton!

    init(kind: Kind) {
        self.kind = kind
        showHidden = kind == .local ? Settings.shared.filesLocalHidden : Settings.shared.filesRemoteHidden
        super.init(frame: .zero)
        NotificationCenter.default.addObserver(self, selector: #selector(accentChanged), name: Settings.accentDidChange, object: nil)
        wantsLayer = true
        layer?.cornerRadius = 10; layer?.borderWidth = 1
        layer?.backgroundColor = NSColor.white.withAlphaComponent(0.03).cgColor
        layer?.borderColor = NSColor.white.withAlphaComponent(0.08).cgColor
        build()
        table.registerForDraggedTypes(kind == .local ? [.jRemote] : [.jLocal, .fileURL])
    }
    required init?(coder: NSCoder) { fatalError() }
    @objc private func accentChanged() { table.reloadData() }

    func setTitle(_ s: String) { titleLabel.stringValue = s }

    private func build() {
        let icon = NSImageView(image: NSImage(systemSymbolName: kind == .local ? "laptopcomputer" : "server.rack", accessibilityDescription: nil)!)
        icon.symbolConfiguration = .init(pointSize: 12, weight: .medium); icon.contentTintColor = Theme.accent
        titleLabel.font = .systemFont(ofSize: 13, weight: .semibold); titleLabel.textColor = .white
        titleLabel.lineBreakMode = .byTruncatingTail
        titleLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        hiddenButton = IconButton(symbol: showHidden ? "eye" : "eye.slash", tip: "Mostrar archivos ocultos", size: 26, target: self, action: #selector(toggleHidden))
        let mkdir = IconButton(symbol: "folder.badge.plus", tip: "Nueva carpeta", size: 26, target: self, action: #selector(newFolder))
        let refresh = IconButton(symbol: "arrow.clockwise", tip: "Actualizar", size: 26, target: self, action: #selector(reload))
        renameButton = IconButton(symbol: "pencil", tip: "Renombrar (Retorno)", size: 26, target: self, action: #selector(renameMenu))
        deleteButton = IconButton(symbol: "trash", tip: "Eliminar (Suprimir)", size: 26, target: self, action: #selector(deleteMenu))
        renameButton.isEnabled = false; deleteButton.isEnabled = false
        let top = NSStackView(views: [icon, titleLabel, NSView(), renameButton, deleteButton, mkdir, hiddenButton, refresh]); top.spacing = 2

        let up = IconButton(symbol: "chevron.up", tip: "Carpeta superior", size: 26, target: self, action: #selector(goUp))
        let home = IconButton(symbol: "house", tip: "Carpeta personal", size: 26, target: self, action: #selector(goHome))

        pathBox.wantsLayer = true; pathBox.layer?.cornerRadius = 7; pathBox.layer?.borderWidth = 1
        setPathFocus(false)
        pathField.isBordered = false; pathField.drawsBackground = false; pathField.focusRingType = .none
        pathField.font = .monospacedSystemFont(ofSize: 11, weight: .regular); pathField.textColor = .white
        pathField.lineBreakMode = .byTruncatingHead
        pathField.target = self; pathField.action = #selector(pathEntered)
        pathField.onFocus = { [weak self] in self?.setPathFocus($0) }
        pathField.translatesAutoresizingMaskIntoConstraints = false
        pathBox.translatesAutoresizingMaskIntoConstraints = false
        pathBox.addSubview(pathField)
        NSLayoutConstraint.activate([
            pathField.leadingAnchor.constraint(equalTo: pathBox.leadingAnchor, constant: 8),
            pathField.trailingAnchor.constraint(equalTo: pathBox.trailingAnchor, constant: -8),
            pathField.centerYAnchor.constraint(equalTo: pathBox.centerYAnchor),
            pathBox.heightAnchor.constraint(equalToConstant: 26),
        ])
        let nav = NSStackView(views: [up, home, pathBox]); nav.spacing = 4

        errorLabel.font = .systemFont(ofSize: 11); errorLabel.textColor = .systemRed
        errorLabel.maximumNumberOfLines = 3; errorLabel.isHidden = true

        // Tabla
        for (id, title, w) in [("name", "Nombre", 200.0), ("size", "Tamaño", 70.0), ("date", "Fecha", 110.0)] {
            let c = NSTableColumn(identifier: .init(id)); c.title = title; c.width = w
            c.minWidth = id == "name" ? 120 : 60
            c.resizingMask = id == "name" ? .autoresizingMask : .userResizingMask
            table.addTableColumn(c)
        }
        table.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        table.backgroundColor = .clear; table.rowHeight = 26
        table.allowsMultipleSelection = true
        table.usesAlternatingRowBackgroundColors = false
        table.dataSource = self; table.delegate = self
        table.doubleAction = #selector(openClicked); table.target = self
        table.setDraggingSourceOperationMask(.copy, forLocal: false)
        table.setDraggingSourceOperationMask(.copy, forLocal: true)
        table.draggingDestinationFeedbackStyle = .regular
        table.onDeleteKey = { [weak self] in self?.deleteSelected() }
        table.onReturnKey = { [weak self] in self?.renameSelected() }
        let menu = NSMenu(); menu.delegate = self; table.menu = menu
        let scroll = NSScrollView()
        scroll.documentView = table; scroll.drawsBackground = false; scroll.hasVerticalScroller = true

        // Superposición (conectando / error)
        overlay.wantsLayer = true; overlay.layer?.backgroundColor = Theme.bg.withAlphaComponent(0.92).cgColor
        overlay.layer?.cornerRadius = 10
        spinner.style = .spinning; spinner.controlSize = .regular; spinner.isIndeterminate = true
        overlayLabel.font = .systemFont(ofSize: 12); overlayLabel.textColor = .secondaryLabelColor; overlayLabel.alignment = .center
        retryButton = PillButton("Reintentar", primary: true, target: self, action: #selector(retry))
        let oStack = NSStackView(views: [spinner, overlayLabel, retryButton]); oStack.orientation = .vertical; oStack.spacing = 12
        oStack.alignment = .centerX
        oStack.translatesAutoresizingMaskIntoConstraints = false
        overlay.addSubview(oStack)
        NSLayoutConstraint.activate([
            oStack.centerXAnchor.constraint(equalTo: overlay.centerXAnchor),
            oStack.centerYAnchor.constraint(equalTo: overlay.centerYAnchor),
            oStack.leadingAnchor.constraint(greaterThanOrEqualTo: overlay.leadingAnchor, constant: 20),
            oStack.trailingAnchor.constraint(lessThanOrEqualTo: overlay.trailingAnchor, constant: -20),
        ])
        overlay.isHidden = true

        for v in [top, nav, errorLabel, scroll, overlay] { v.translatesAutoresizingMaskIntoConstraints = false; addSubview(v) }
        NSLayoutConstraint.activate([
            top.topAnchor.constraint(equalTo: topAnchor, constant: 10),
            top.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            top.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            nav.topAnchor.constraint(equalTo: top.bottomAnchor, constant: 8),
            nav.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            nav.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            errorLabel.topAnchor.constraint(equalTo: nav.bottomAnchor, constant: 6),
            errorLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            errorLabel.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            scroll.topAnchor.constraint(equalTo: errorLabel.bottomAnchor, constant: 6),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -2),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -4),
            overlay.topAnchor.constraint(equalTo: topAnchor), overlay.bottomAnchor.constraint(equalTo: bottomAnchor),
            overlay.leadingAnchor.constraint(equalTo: leadingAnchor), overlay.trailingAnchor.constraint(equalTo: trailingAnchor),
        ])
    }

    private func setPathFocus(_ on: Bool) {
        pathBox.layer?.backgroundColor = NSColor.white.withAlphaComponent(on ? 0.07 : 0.05).cgColor
        pathBox.layer?.borderColor = (on ? Theme.accent : NSColor.white.withAlphaComponent(0.10)).cgColor
    }

    // MARK: Estado de superposición

    func showOverlay(_ text: String, spinning: Bool, retry: Bool) {
        overlayLabel.stringValue = text
        overlayLabel.textColor = retry ? .systemRed : .secondaryLabelColor
        spinner.isHidden = !spinning; retryButton.isHidden = !retry
        if spinning { spinner.startAnimation(nil) } else { spinner.stopAnimation(nil) }
        overlay.isHidden = false
    }
    func hideOverlay() { overlay.isHidden = true; spinner.stopAnimation(nil) }
    @objc private func retry() { onRetry?() }

    // MARK: Navegación

    func navigate(_ p: String) {
        guard let loader else { return }
        errorLabel.isHidden = true
        Task {
            do {
                let list = try await loader(p)
                await MainActor.run {
                    self.path = p; self.items = list
                    self.pathField.stringValue = p
                    self.applyFilter()
                }
            } catch {
                await MainActor.run {
                    self.pathField.stringValue = self.path
                    self.errorLabel.stringValue = error.localizedDescription
                    self.errorLabel.isHidden = false
                }
            }
        }
    }

    private func applyFilter() {
        visible = items.filter { showHidden || !$0.name.hasPrefix(".") }.sorted {
            if $0.isDir != $1.isDir { return $0.isDir }
            return $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending
        }
        table.reloadData()
    }

    @objc func reload() { navigate(path.isEmpty ? homePath : path) }
    @objc private func goUp() { navigate((path as NSString).deletingLastPathComponent.isEmpty ? "/" : (path as NSString).deletingLastPathComponent) }
    @objc private func goHome() { navigate(homePath) }
    @objc private func toggleHidden() {
        showHidden.toggle()
        hiddenButton.image = NSImage(systemSymbolName: showHidden ? "eye" : "eye.slash", accessibilityDescription: nil)
        applyFilter()
    }
    @objc private func pathEntered() {
        var p = pathField.stringValue.trimmed
        if p.hasPrefix("~") { p = homePath + p.dropFirst() }
        if !p.isEmpty { navigate(p) }
    }
    @objc private func openClicked() {
        let r = table.clickedRow; guard visible.indices.contains(r) else { return }
        let it = visible[r]
        if it.isDir { navigate(it.path) }
        else if kind == .local { NSWorkspace.shared.open(URL(fileURLWithPath: it.path)) }
        else { onTransferSelected?() }
    }

    // MARK: Selección y acciones

    /// El elemento listado con esa ruta, si está en la carpeta actual.
    func item(atPath p: String) -> FileItem? { items.first { $0.path == p } }

    var selectedItems: [FileItem] { table.selectedRowIndexes.compactMap { visible.indices.contains($0) ? visible[$0] : nil } }

    @objc private func newFolder() {
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 260, height: 24))
        field.placeholderString = "Nombre de la carpeta"
        let a = NSAlert(); a.messageText = "Nueva carpeta"; a.informativeText = "En \(path)"
        a.accessoryView = field; a.addButton(withTitle: "Crear"); a.addButton(withTitle: "Cancelar")
        a.window.initialFirstResponder = field
        guard a.runModal() == .alertFirstButtonReturn else { return }
        let name = field.stringValue.trimmed
        guard !name.isEmpty, !name.contains("/") else { return }
        Task { await onMkdir?((path as NSString).appendingPathComponent(name)); await MainActor.run { self.reload() } }
    }

    private func deleteSelected() {
        let sel = selectedItems
        guard !sel.isEmpty else { return }
        let a = NSAlert()
        a.messageText = sel.count == 1 ? "¿Eliminar “\(sel[0].name)”?" : "¿Eliminar \(sel.count) elementos?"
        a.informativeText = kind == .local ? "Se moverán a la papelera." : "Se eliminarán del servidor y no se pueden recuperar."
        a.alertStyle = .warning
        a.addButton(withTitle: "Eliminar"); a.addButton(withTitle: "Cancelar")
        if Settings.shared.filesConfirmDelete { guard a.runModal() == .alertFirstButtonReturn else { return } }
        Task {
            do { try await onDelete?(sel) }
            catch { await MainActor.run { showAlert("No se pudo eliminar", error.localizedDescription, window: self.window) } }
            await MainActor.run { self.reload() }
        }
    }

    private func renameSelected() {
        let sel = selectedItems
        guard sel.count == 1 else { return NSSound.beep() }
        let item = sel[0]
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24))
        field.stringValue = item.name
        let a = NSAlert(); a.messageText = "Renombrar"; a.informativeText = "“\(item.name)”"
        a.accessoryView = field; a.addButton(withTitle: "Renombrar"); a.addButton(withTitle: "Cancelar")
        a.window.initialFirstResponder = field
        guard a.runModal() == .alertFirstButtonReturn else { return }
        let name = field.stringValue.trimmed
        guard !name.isEmpty, name != item.name else { return }
        guard !name.contains("/") else { return showAlert("Nombre no válido", "El nombre no puede contener “/”.", window: window) }
        guard !items.contains(where: { $0.name == name }) else {
            return showAlert("Ya existe “\(name)”", "Elige otro nombre.", window: window)
        }
        Task {
            do { try await onRename?(item, ((item.path as NSString).deletingLastPathComponent as NSString).appendingPathComponent(name)) }
            catch { await MainActor.run { showAlert("No se pudo renombrar", error.localizedDescription, window: self.window) } }
            await MainActor.run { self.reload() }
        }
    }

    func tableViewSelectionDidChange(_ n: Notification) {
        let c = table.selectedRowIndexes.count
        renameButton.isEnabled = c == 1
        deleteButton.isEnabled = c >= 1
    }

    // MARK: Menú contextual

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let r = table.clickedRow
        if visible.indices.contains(r), !table.selectedRowIndexes.contains(r) { table.selectRowIndexes([r], byExtendingSelection: false) }
        if visible.indices.contains(r) {
            if visible[r].isDir { menu.addItem(withTitle: "Abrir", action: #selector(openClicked), keyEquivalent: "").target = self }
            menu.addItem(withTitle: kind == .local ? "Subir al servidor" : "Descargar a este Mac",
                         action: #selector(transferSelected), keyEquivalent: "").target = self
            if let extras = extraActions?(visible[r]), !extras.isEmpty {
                for a in extras {
                    let mi = NSMenuItem(title: a.title, action: #selector(runExtra(_:)), keyEquivalent: "")
                    mi.target = self; mi.representedObject = ActionBox(a.run); menu.addItem(mi)
                }
            }
            menu.addItem(.separator())
        }
        menu.addItem(withTitle: "Nueva carpeta…", action: #selector(newFolder), keyEquivalent: "").target = self
        if visible.indices.contains(r) {
            menu.addItem(withTitle: "Renombrar…", action: #selector(renameMenu), keyEquivalent: "").target = self
            menu.addItem(withTitle: "Eliminar…", action: #selector(deleteMenu), keyEquivalent: "").target = self
        }
        menu.addItem(withTitle: "Actualizar", action: #selector(reload), keyEquivalent: "").target = self
    }
    @objc private func transferSelected() { onTransferSelected?() }
    @objc private func runExtra(_ s: NSMenuItem) { (s.representedObject as? ActionBox)?.fn() }
    @objc private func deleteMenu() { deleteSelected() }
    @objc private func renameMenu() { renameSelected() }

    // MARK: Tabla

    func numberOfRows(in t: NSTableView) -> Int { visible.count }

    func tableView(_ t: NSTableView, viewFor c: NSTableColumn?, row: Int) -> NSView? {
        let it = visible[row]
        switch c?.identifier.rawValue {
        case "name":
            let icon = NSImageView(image: NSImage(systemSymbolName: it.isDir ? "folder.fill" : "doc", accessibilityDescription: nil)!)
            icon.symbolConfiguration = .init(pointSize: 12, weight: .regular)
            icon.contentTintColor = it.isDir ? Theme.accent : .secondaryLabelColor
            let l = NSTextField(labelWithString: it.name)
            l.font = .systemFont(ofSize: 12); l.lineBreakMode = .byTruncatingMiddle
            l.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            let s = NSStackView(views: [icon, l]); s.spacing = 7
            s.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 0, right: 4)
            return s
        case "size":
            let l = NSTextField(labelWithString: it.isDir ? "—" : ByteCountFormatter.string(fromByteCount: it.size, countStyle: .file))
            l.font = .systemFont(ofSize: 11); l.textColor = .secondaryLabelColor; l.alignment = .right
            return l
        default:
            let l = NSTextField(labelWithString: it.modified)
            l.font = .systemFont(ofSize: 11); l.textColor = .secondaryLabelColor; l.lineBreakMode = .byTruncatingTail
            return l
        }
    }

    // MARK: Arrastrar y soltar

    func tableView(_ t: NSTableView, pasteboardWriterForRow row: Int) -> NSPasteboardWriting? {
        let it = visible[row]
        let pb = NSPasteboardItem()
        pb.setString(it.path, forType: kind == .local ? .jLocal : .jRemote)
        if kind == .local { pb.setString(URL(fileURLWithPath: it.path).absoluteString, forType: .fileURL) }
        return pb
    }

    private func droppedPaths(_ info: NSDraggingInfo) -> [String] {
        var out: [String] = []
        for item in info.draggingPasteboard.pasteboardItems ?? [] {
            if kind == .local {
                if let s = item.string(forType: .jRemote) { out.append(s) }
            } else if let s = item.string(forType: .jLocal) {
                out.append(s)
            } else if let s = item.string(forType: .fileURL), let u = URL(string: s), u.isFileURL {
                out.append(u.path)
            }
        }
        return out
    }

    func tableView(_ t: NSTableView, validateDrop info: NSDraggingInfo, proposedRow row: Int,
                   proposedDropOperation op: NSTableView.DropOperation) -> NSDragOperation {
        guard !droppedPaths(info).isEmpty else { return [] }
        if visible.indices.contains(row), visible[row].isDir { t.setDropRow(row, dropOperation: .on) }
        else { t.setDropRow(-1, dropOperation: .on) }
        return .copy
    }

    func tableView(_ t: NSTableView, acceptDrop info: NSDraggingInfo, row: Int, dropOperation: NSTableView.DropOperation) -> Bool {
        let paths = droppedPaths(info)
        guard !paths.isEmpty else { return false }
        let dest = visible.indices.contains(row) && visible[row].isDir ? visible[row].path : path
        onDrop?(paths, dest)
        return true
    }
}

final class ActionBox: NSObject {
    let fn: () -> Void
    init(_ fn: @escaping () -> Void) { self.fn = fn }
}

// MARK: - Cola de transferencias

final class TransferRow: NSView {
    private let title = NSTextField(labelWithString: "")
    private let detail = NSTextField(labelWithString: "")
    private let bar = NSProgressIndicator()
    private let icon = NSImageView()
    private let action: IconButton
    var onCancel: (() -> Void)?
    var onDismiss: (() -> Void)?
    private var finished = false

    init(title t: String, upload: Bool) {
        action = IconButton(symbol: "xmark", tip: "Cancelar", size: 22)
        super.init(frame: .zero)
        wantsLayer = true; layer?.cornerRadius = 8
        layer?.backgroundColor = NSColor.white.withAlphaComponent(0.05).cgColor
        icon.image = NSImage(systemSymbolName: upload ? "arrow.up.circle.fill" : "arrow.down.circle.fill", accessibilityDescription: nil)
        icon.symbolConfiguration = .init(pointSize: 16, weight: .regular); icon.contentTintColor = Theme.accent
        title.stringValue = t; title.font = .systemFont(ofSize: 12, weight: .medium); title.lineBreakMode = .byTruncatingMiddle
        detail.font = .systemFont(ofSize: 10); detail.textColor = .secondaryLabelColor; detail.lineBreakMode = .byTruncatingMiddle
        detail.stringValue = "Iniciando…"
        bar.style = .bar; bar.isIndeterminate = true; bar.minValue = 0; bar.maxValue = 100; bar.controlSize = .small
        bar.startAnimation(nil)
        action.target = self; action.action = #selector(tapped)
        let texts = NSStackView(views: [title, bar, detail]); texts.orientation = .vertical; texts.alignment = .leading; texts.spacing = 3
        let row = NSStackView(views: [icon, texts, action]); row.spacing = 10; row.alignment = .centerY
        row.edgeInsets = NSEdgeInsets(top: 8, left: 10, bottom: 8, right: 8)
        row.translatesAutoresizingMaskIntoConstraints = false
        addSubview(row)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: leadingAnchor), row.trailingAnchor.constraint(equalTo: trailingAnchor),
            row.topAnchor.constraint(equalTo: topAnchor), row.bottomAnchor.constraint(equalTo: bottomAnchor),
            bar.widthAnchor.constraint(equalTo: texts.widthAnchor),
        ])
    }
    required init?(coder: NSCoder) { fatalError() }

    func update(_ pct: Int, _ text: String) {
        if bar.isIndeterminate { bar.stopAnimation(nil); bar.isIndeterminate = false }
        bar.doubleValue = Double(pct); detail.stringValue = "\(pct)% · \(text)"
    }
    func finish(error: String?) {
        finished = true
        bar.stopAnimation(nil); bar.isIndeterminate = false
        action.image = NSImage(systemSymbolName: "xmark", accessibilityDescription: "Cerrar")
        action.toolTip = "Cerrar"
        if let error {
            bar.doubleValue = 0; detail.stringValue = error; detail.textColor = .systemRed; detail.lineBreakMode = .byTruncatingTail
            icon.image = NSImage(systemSymbolName: "exclamationmark.triangle.fill", accessibilityDescription: nil); icon.contentTintColor = .systemRed
        } else {
            bar.doubleValue = 100; detail.stringValue = "Completado"
            icon.image = NSImage(systemSymbolName: "checkmark.circle.fill", accessibilityDescription: nil); icon.contentTintColor = .systemGreen
            let secs = Settings.shared.transferDismiss
            if secs > 0 { DispatchQueue.main.asyncAfter(deadline: .now() + .seconds(secs)) { [weak self] in self?.onDismiss?() } }
        }
    }
    @objc private func tapped() { finished ? onDismiss?() : onCancel?() }
}

// MARK: - Vista principal del gestor

final class FileTransferView: NSView {
    let local = FilePane(kind: .local)
    let remote = FilePane(kind: .remote)
    private let queue = NSStackView()
    private(set) var session: SFTPSession?
    var onRetry: (() -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true; layer?.backgroundColor = Theme.bg.cgColor

        local.setTitle("Este Mac")
        local.homePath = NSHomeDirectory()
        local.loader = { dir in try FileSource.local(dir) }
        local.onMkdir = { path in try? FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: false) }
        local.onDelete = { items in for i in items { try FileManager.default.trashItem(at: URL(fileURLWithPath: i.path), resultingItemURL: nil) } }
        local.onRename = { item, newPath in try FileManager.default.moveItem(atPath: item.path, toPath: newPath) }
        local.onDrop = { [weak self] paths, dest in self?.download(paths, to: dest) }
        local.onTransferSelected = { [weak self] in self?.uploadSelected() }

        remote.onDrop = { [weak self] paths, dest in self?.upload(paths, to: dest) }
        remote.onTransferSelected = { [weak self] in self?.downloadSelected() }
        remote.onRetry = { [weak self] in self?.onRetry?() }

        let toRemote = IconButton(symbol: "arrow.right", tip: "Subir selección al servidor", size: 32, target: self, action: #selector(uploadTapped))
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
        showConnecting("Conectando…")
    }
    required init?(coder: NSCoder) { fatalError() }

    // MARK: Estado de la conexión

    func showConnecting(_ text: String) { remote.showOverlay(text, spinning: true, retry: false) }
    func showError(_ text: String) { remote.showOverlay(text, spinning: false, retry: true) }

    func attach(_ s: SFTPSession, startPath: String? = nil) {
        session = s
        remote.setTitle(s.target.title)
        remote.homePath = s.home
        remote.loader = { [weak s] dir in
            guard let s else { throw AWSError(message: "Sesión cerrada") }
            return try await FileSource.remote(s, dir)
        }
        remote.onMkdir = { [weak s] path in try? await s?.mkdir(path) }
        remote.onDelete = { [weak self] items in try await self?.session?.removeRemote(items.map(\.path)) }
        remote.onRename = { [weak s] item, newPath in try await s?.rename(item.path, to: newPath) }
        remote.hideOverlay()
        var start = s.home
        if let p = startPath, !p.isEmpty {
            // ruta de un archivo → su carpeta; "~" → carpeta personal del servidor
            start = p.hasPrefix("~") ? s.home + p.dropFirst() : p
            if !start.hasSuffix("/"), (start as NSString).pathExtension.count > 0 { start = (start as NSString).deletingLastPathComponent }
        }
        remote.navigate(start)
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
        let sel = remote.selectedItems.map(\.path)
        guard !sel.isEmpty else { return NSSound.beep() }
        download(sel, to: local.path)
    }

    func upload(_ paths: [String], to dir: String) {
        run(paths, dir, .upload, "Subiendo \(describe(paths))") { self.remote.reload() }
    }
    func download(_ paths: [String], to dir: String) {
        run(paths, dir, .download, "Descargando \(describe(paths))") { self.local.reload() }
    }
    private func describe(_ p: [String]) -> String { p.count == 1 ? (p[0] as NSString).lastPathComponent : "\(p.count) elementos" }

    private func run(_ sources: [String], _ dest: String, _ dir: SFTPSession.Direction, _ title: String, done: @escaping () -> Void) {
        guard let session, !dest.isEmpty else { return }
        let job = TransferJob()
        let row = TransferRow(title: title, upload: dir == .upload)
        row.onCancel = { job.cancel() }
        row.onDismiss = { [weak self, weak row] in if let row { self?.queue.removeArrangedSubview(row); row.removeFromSuperview() } }
        queue.addArrangedSubview(row)
        row.widthAnchor.constraint(equalTo: queue.widthAnchor).isActive = true
        Task {
            do {
                try await session.runTransfer(job, direction: dir, sources: sources, destDir: dest) { pct, text in
                    DispatchQueue.main.async { row.update(pct, text) }
                }
                await MainActor.run { row.finish(error: nil); done() }
            } catch {
                await MainActor.run { row.finish(error: error.localizedDescription); done() }
            }
        }
    }
}

// MARK: - Pestaña

final class FileTab: Tab {
    let id = UUID()
    var title = "Archivos"
    let iconName = "arrow.left.arrow.right.square"
    let view = FileTransferView()
    var content: NSView { view }
    var focusView: NSView { view }
    var onExit: ((Tab) -> Void)?
    var fileTarget: (() async throws -> SSHTarget)? { nil }
    let connectionKey: String?
    var isConnected: Bool { session?.isAlive == true }
    private let provider: () async throws -> SSHTarget
    private var session: SFTPSession?
    private let startPath: String?

    init(key: String?, provider: @escaping () async throws -> SSHTarget, startPath: String? = nil) {
        self.connectionKey = key
        self.provider = provider
        self.startPath = startPath
        view.onRetry = { [weak self] in self?.start() }
        start()
    }

    func start() {
        session?.close(); session = nil
        AppState.shared.onChange?()
        view.showConnecting("Conectando…")
        Task { [self] in
            do {
                let target = try await provider()
                await MainActor.run { self.title = "Archivos · \(target.title)"; AppState.shared.onChange?() }
                let s = SFTPSession(target: target)
                try await s.connect()
                await MainActor.run {
                    self.session = s
                    s.onDisconnect = { [weak self, weak s] in
                        guard let self, self.session === s else { return }
                        self.session = nil
                        self.view.showError("La conexión se cerró.")
                        AppState.shared.onChange?()
                    }
                    self.view.attach(s, startPath: self.startPath)
                    AppState.shared.onChange?()
                }
            } catch {
                await MainActor.run { self.view.showError(error.localizedDescription) }
            }
        }
    }

    func shutdown() { session?.close(); session = nil }
}
