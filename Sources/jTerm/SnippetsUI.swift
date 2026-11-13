import AppKit

// MARK: - Botón flotante

/// Botón circular flotante que abre la lista de comandos guardados.
final class SnippetButton: NSView {
    var onClick: (() -> Void)?
    private let icon = NSImageView()
    private var hovering = false { didSet { refresh() } }
    /// true mientras la lista de comandos está abierta: el botón queda resaltado.
    var active = false { didSet { refresh() } }

    init() {
        super.init(frame: .zero)
        NotificationCenter.default.addObserver(self, selector: #selector(accentChanged), name: Settings.accentDidChange, object: nil)
        wantsLayer = true
        layer?.cornerRadius = 18; layer?.borderWidth = 1
        layer?.shadowColor = NSColor.black.cgColor; layer?.shadowOpacity = 0.35
        layer?.shadowRadius = 7; layer?.shadowOffset = CGSize(width: 0, height: -1)
        toolTip = "Comandos guardados (⇧⌘K)"
        icon.image = NSImage(systemSymbolName: "list.bullet.rectangle", accessibilityDescription: "Comandos guardados")
        icon.symbolConfiguration = .init(pointSize: 14, weight: .medium)
        icon.translatesAutoresizingMaskIntoConstraints = false
        addSubview(icon)
        translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: 36), heightAnchor.constraint(equalToConstant: 36),
            icon.centerXAnchor.constraint(equalTo: centerXAnchor), icon.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
        refresh()
    }
    required init?(coder: NSCoder) { fatalError() }
    @objc private func accentChanged() { refresh() }

    private func refresh() {
        let lit = hovering || active
        layer?.backgroundColor = (active ? Theme.accent.withAlphaComponent(0.45) : (hovering ? Theme.accent.withAlphaComponent(0.28) : Theme.bar.withAlphaComponent(0.88))).cgColor
        layer?.borderColor = (lit ? Theme.accent : NSColor.white.withAlphaComponent(0.14)).cgColor
        icon.contentTintColor = lit ? .white : .secondaryLabelColor
        icon.image = NSImage(systemSymbolName: active ? "list.bullet.rectangle.fill" : "list.bullet.rectangle", accessibilityDescription: "Comandos guardados")
        toolTip = active ? "Cerrar la lista de comandos (⇧⌘K)" : "Comandos guardados (⇧⌘K)"
        alphaValue = lit ? 1 : 0.8
    }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .pointingHand) }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseEntered(with e: NSEvent) { hovering = true }
    override func mouseExited(with e: NSEvent) { hovering = false }
    override func mouseDown(with e: NSEvent) { onClick?() }
}

// MARK: - Lista emergente

final class SnippetTable: NSTableView {
    var onReturn: (() -> Void)?
    override func keyDown(with e: NSEvent) {
        if e.keyCode == 36 || e.keyCode == 76 { onReturn?() } else { super.keyDown(with: e) }
    }
}

/// Ventana emergente con la lista de comandos (búsqueda, insertar, agregar, editar y eliminar).
final class SnippetsPopover: NSObject, NSPopoverDelegate, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate, NSTextFieldDelegate {
    var onPick: ((Snippet, Bool) -> Void)?       // comando, ejecutar ahora (⌥)
    var onVisibilityChanged: ((Bool) -> Void)?   // la lista se abrió o se cerró

    private let popover = NSPopover()
    private let store = SnippetStore.shared
    private let root = NSView(frame: NSRect(x: 0, y: 0, width: 390, height: 470))
    private let listView = NSView()
    private let editView = NSView()
    private var search: CompactField!
    private let table = SnippetTable()
    private let emptyLabel = NSTextField(wrappingLabelWithString: "")
    private var filtered: [Snippet] = []
    private var nameField: CompactField?
    private var notes: NotesField?
    private var runSwitch: NSSwitch?
    private var editing: Snippet?
    private var tabMonitor: Any?
    var isShown: Bool { popover.isShown }

    override init() {
        super.init()
        popover.behavior = .transient
        popover.appearance = NSAppearance(named: .darkAqua)
        popover.delegate = self
        root.wantsLayer = true
        root.layer?.backgroundColor = NSColor(srgbRed: 0.105, green: 0.115, blue: 0.14, alpha: 1).cgColor
        let vc = NSViewController(); vc.view = root
        popover.contentViewController = vc
        buildList()
        for v in [listView, editView] {
            v.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(v)
            NSLayoutConstraint.activate([
                v.leadingAnchor.constraint(equalTo: root.leadingAnchor), v.trailingAnchor.constraint(equalTo: root.trailingAnchor),
                v.topAnchor.constraint(equalTo: root.topAnchor), v.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            ])
        }
        NSLayoutConstraint.activate([root.widthAnchor.constraint(equalToConstant: 390), root.heightAnchor.constraint(equalToConstant: 470)])
        // Tab entre Nombre y Comando, sin depender del orden de vistas ni de quién tenga el foco primero.
        tabMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] e in
            guard let self, e.keyCode == 48, self.popover.isShown, !self.editView.isHidden,
                  let win = self.root.window, win.isKeyWindow,
                  !e.modifierFlags.contains(.option), !e.modifierFlags.contains(.command), !e.modifierFlags.contains(.control),
                  let name = self.nameField?.field, let cmd = self.notes?.textView else { return e }
            win.makeFirstResponder(win.firstResponder === cmd ? name : cmd)
            return nil
        }
        NotificationCenter.default.addObserver(self, selector: #selector(storeChanged), name: .snippetsDidChange, object: nil)
    }

    // MARK: Mostrar

    func popoverWillShow(_ notification: Notification) { onVisibilityChanged?(true) }
    func popoverDidClose(_ notification: Notification) { onVisibilityChanged?(false) }

    func toggle(relativeTo rect: NSRect, of view: NSView) {
        if popover.isShown { popover.performClose(nil); return }
        search.setValue(""); reload(); showList()
        popover.show(relativeTo: rect, of: view, preferredEdge: .maxY)
        DispatchQueue.main.async { [weak self] in self?.search.window?.makeFirstResponder(self?.search.field) }
    }
    func close() { popover.performClose(nil) }

    private func showList() { listView.isHidden = false; editView.isHidden = true }

    // MARK: Lista

    private func buildList() {
        search = CompactField(value: "", placeholder: "Buscar comando…", width: 300)
        search.field.delegate = self
        let add = IconButton(symbol: "plus", tip: "Nuevo comando", size: 30, target: self, action: #selector(addTapped))
        let top = NSStackView(views: [search, add]); top.spacing = 8

        let col = NSTableColumn(identifier: .init("c")); col.resizingMask = .autoresizingMask
        table.addTableColumn(col)
        table.headerView = nil; table.backgroundColor = .clear; table.rowHeight = 46
        table.selectionHighlightStyle = .regular
        table.dataSource = self; table.delegate = self
        table.target = self; table.action = #selector(rowClicked)
        table.onReturn = { [weak self] in self?.pickSelected(runNow: false) }
        let menu = NSMenu(); menu.delegate = self; table.menu = menu
        let scroll = NSScrollView()
        scroll.documentView = table; scroll.drawsBackground = false; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true

        emptyLabel.font = .systemFont(ofSize: 12); emptyLabel.textColor = .secondaryLabelColor
        emptyLabel.alignment = .center; emptyLabel.maximumNumberOfLines = 0

        let hint = NSTextField(labelWithString: "Clic: pegar en la consola   ·   ⌥ clic: pegar y ejecutar")
        hint.font = .systemFont(ofSize: 10); hint.textColor = .tertiaryLabelColor

        for v in [top, scroll, emptyLabel, hint] { v.translatesAutoresizingMaskIntoConstraints = false; listView.addSubview(v) }
        NSLayoutConstraint.activate([
            top.topAnchor.constraint(equalTo: listView.topAnchor, constant: 14),
            top.leadingAnchor.constraint(equalTo: listView.leadingAnchor, constant: 14),
            top.trailingAnchor.constraint(equalTo: listView.trailingAnchor, constant: -14),
            scroll.topAnchor.constraint(equalTo: top.bottomAnchor, constant: 10),
            scroll.leadingAnchor.constraint(equalTo: listView.leadingAnchor, constant: 6),
            scroll.trailingAnchor.constraint(equalTo: listView.trailingAnchor, constant: -6),
            scroll.bottomAnchor.constraint(equalTo: hint.topAnchor, constant: -8),
            hint.leadingAnchor.constraint(equalTo: listView.leadingAnchor, constant: 16),
            hint.bottomAnchor.constraint(equalTo: listView.bottomAnchor, constant: -12),
            emptyLabel.centerXAnchor.constraint(equalTo: listView.centerXAnchor),
            emptyLabel.centerYAnchor.constraint(equalTo: scroll.centerYAnchor),
            emptyLabel.widthAnchor.constraint(equalToConstant: 280),
        ])
    }

    @objc private func storeChanged() { reload() }

    private func reload() {
        filtered = store.search(search.field.stringValue)
        table.reloadData()
        let hasAny = !store.items.isEmpty
        emptyLabel.isHidden = !filtered.isEmpty
        emptyLabel.stringValue = hasAny ? "Ningún comando coincide con la búsqueda."
                                        : "Aún no hay comandos guardados.\nPulsa + para agregar el primero."
    }

    func numberOfRows(in t: NSTableView) -> Int { filtered.count }

    func tableView(_ t: NSTableView, viewFor c: NSTableColumn?, row: Int) -> NSView? {
        let s = filtered[row]
        let title = NSTextField(labelWithString: s.title.isEmpty ? "(vacío)" : s.title)
        title.font = s.name.isEmpty ? .monospacedSystemFont(ofSize: 12, weight: .regular) : .systemFont(ofSize: 12, weight: .semibold)
        title.textColor = .white; title.lineBreakMode = .byTruncatingTail
        title.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        var lines: [NSView] = [title]
        let detail = !s.name.isEmpty ? s.firstLine : (s.lineCount > 1 ? "+\(s.lineCount - 1) líneas más" : "")
        if !detail.isEmpty {
            let p = NSTextField(labelWithString: detail)
            p.font = .monospacedSystemFont(ofSize: 11, weight: .regular); p.textColor = .secondaryLabelColor
            p.lineBreakMode = .byTruncatingTail
            p.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            lines.append(p)
        }
        let texts = NSStackView(views: lines); texts.orientation = .vertical; texts.alignment = .leading; texts.spacing = 2
        texts.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        var views: [NSView] = [texts]
        if s.autoRun {
            let l = NSTextField(labelWithString: "↵ ejecuta")
            l.font = .systemFont(ofSize: 9, weight: .semibold); l.textColor = Theme.accent
            views.append(l)
        }
        let edit = IconButton(symbol: "square.and.pencil", tip: "Editar", size: 26, target: self, action: #selector(editTapped(_:)))
        edit.tag = row
        views.append(edit)
        let line = NSStackView(views: views); line.spacing = 8; line.alignment = .centerY
        line.distribution = .fill
        line.edgeInsets = NSEdgeInsets(top: 4, left: 10, bottom: 4, right: 6)
        return line
    }

    @objc private func rowClicked() {
        let r = table.clickedRow
        guard filtered.indices.contains(r) else { return }
        pick(filtered[r], runNow: NSEvent.modifierFlags.contains(.option))
    }
    private func pickSelected(runNow: Bool) {
        let r = table.selectedRow
        if filtered.indices.contains(r) { pick(filtered[r], runNow: runNow || NSEvent.modifierFlags.contains(.option)) }
    }
    private func pick(_ s: Snippet, runNow: Bool) {
        popover.performClose(nil)
        onPick?(s, runNow)
    }

    // Teclado en el campo de búsqueda: filtra al escribir, ↓ baja a la lista, Retorno inserta.
    func controlTextDidChange(_ n: Notification) { reload() }
    func control(_ c: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
        switch sel {
        case #selector(NSResponder.moveDown(_:)):
            if !filtered.isEmpty { table.selectRowIndexes([0], byExtendingSelection: false); popover.contentViewController?.view.window?.makeFirstResponder(table) }
            return true
        case #selector(NSResponder.insertNewline(_:)):
            if let first = filtered.first { pick(first, runNow: NSEvent.modifierFlags.contains(.option)) }
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            popover.performClose(nil); return true
        default: return false
        }
    }

    // MARK: Menú contextual

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard filtered.indices.contains(table.clickedRow) else { return }
        for (title, sel) in [("Pegar", #selector(menuInsert)), ("Pegar y ejecutar", #selector(menuRun)), ("Editar…", #selector(menuEdit)),
                             ("Duplicar", #selector(menuDuplicate)), ("Eliminar", #selector(menuDelete))] {
            menu.addItem(withTitle: title, action: sel, keyEquivalent: "").target = self
        }
    }
    private var clicked: Snippet? { filtered.indices.contains(table.clickedRow) ? filtered[table.clickedRow] : nil }
    @objc private func menuInsert() { if let s = clicked { pick(s, runNow: false) } }
    @objc private func menuRun() { if let s = clicked { pick(s, runNow: true) } }
    @objc private func menuEdit() { if let s = clicked { showEdit(s) } }
    @objc private func menuDuplicate() { if let s = clicked { store.duplicate(s.id) } }
    @objc private func menuDelete() { if let s = clicked { confirmDelete(s) } }

    private func confirmDelete(_ s: Snippet) {
        let a = NSAlert(); a.messageText = "¿Eliminar “\(s.title)”?"; a.informativeText = "Se borra de la lista de comandos guardados."
        a.alertStyle = .warning; a.addButton(withTitle: "Eliminar"); a.addButton(withTitle: "Cancelar")
        if a.runModal() == .alertFirstButtonReturn { store.remove(s.id); showList() }
    }

    // MARK: Editor

    @objc private func addTapped() { showEdit(nil) }
    @objc private func editTapped(_ b: NSButton) { if filtered.indices.contains(b.tag) { showEdit(filtered[b.tag]) } }

    func showEdit(_ s: Snippet?) {
        editing = s
        editView.subviews.forEach { $0.removeFromSuperview() }
        let title = NSTextField(labelWithString: s == nil ? "Nuevo comando" : "Editar comando")
        title.font = .systemFont(ofSize: 16, weight: .semibold); title.textColor = .white

        let name = CompactField(value: s?.name ?? "", placeholder: "Ej. Reiniciar nginx (opcional)", width: 350)
        let cmd = NotesField(value: s?.command ?? "", placeholder: "", width: 350, height: 150)
        let sw = NSSwitch(); sw.state = (s?.autoRun ?? false) ? .on : .off
        let runLabel = NSTextField(wrappingLabelWithString: "Ejecutar al insertar (pulsa Enter automáticamente)")
        runLabel.font = .systemFont(ofSize: 12); runLabel.textColor = .white; runLabel.maximumNumberOfLines = 2
        let runRow = NSStackView(views: [sw, runLabel]); runRow.spacing = 10; runRow.alignment = .centerY
        nameField = name; notes = cmd; runSwitch = sw

        let cancel = PillButton("Cancelar", primary: false, target: self, action: #selector(cancelEdit))
        let save = PillButton("Guardar", primary: true, target: self, action: #selector(saveEdit))
        var btnViews: [NSView] = []
        if s != nil { btnViews.append(PillButton("Eliminar", primary: false, target: self, action: #selector(deleteEdit))) }
        btnViews += [NSView(), cancel, save]
        let buttons = NSStackView(views: btnViews); buttons.spacing = 8

        let form = NSStackView(views: [title, smallCaption("Nombre"), name, smallCaption("Comando"), cmd, runRow, buttons])
        form.orientation = .vertical; form.alignment = .leading; form.spacing = 6
        form.setCustomSpacing(14, after: title); form.setCustomSpacing(12, after: name)
        form.setCustomSpacing(14, after: cmd); form.setCustomSpacing(18, after: runRow)
        form.translatesAutoresizingMaskIntoConstraints = false
        editView.addSubview(form)
        NSLayoutConstraint.activate([
            form.topAnchor.constraint(equalTo: editView.topAnchor, constant: 18),
            form.leadingAnchor.constraint(equalTo: editView.leadingAnchor, constant: 20),
            form.trailingAnchor.constraint(equalTo: editView.trailingAnchor, constant: -20),
            buttons.widthAnchor.constraint(equalTo: form.widthAnchor),
        ])
        // Tab explícito entre los dos campos (no depende del orden de vistas del popover).
        name.onTab = { [weak self, weak cmd] in if let t = cmd?.textView { self?.root.window?.makeFirstResponder(t) } }
        name.onBacktab = name.onTab
        cmd.onTab = { [weak self, weak name] in if let f = name?.field { self?.root.window?.makeFirstResponder(f) } }
        cmd.onBacktab = cmd.onTab
        listView.isHidden = true; editView.isHidden = false
        DispatchQueue.main.async { [weak self] in
            self?.root.window?.makeKey()
            self?.root.window?.makeFirstResponder(name.field)
        }
    }

    @objc private func cancelEdit() { showList(); reload() }
    @objc private func deleteEdit() { if let s = editing { confirmDelete(s) } }
    @objc private func saveEdit() {
        let command = (notes?.value ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !command.isEmpty else { NSSound.beep(); return }
        var s = editing ?? Snippet()
        s.name = (nameField?.field.stringValue ?? "").trimmingCharacters(in: .whitespaces)
        s.command = command
        s.autoRun = runSwitch?.state == .on
        store.upsert(s)
        showList(); reload()
    }
}
