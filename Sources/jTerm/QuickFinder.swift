import AppKit

/// Un resultado del buscador rápido (⌘K): pestaña abierta, servidor, instancia EC2, comando guardado o acción.
struct FinderItem {
    enum Kind: Int { case tab, server, instance, snippet, action }
    let kind: Kind
    let title: String
    let subtitle: String
    let symbol: String
    var env: ServerEnv?
    /// Palabras extra que también se buscan (p. ej. "prod", el ID de la instancia).
    var keywords = ""
    /// `alternate` es true si se pulsó con ⌥ (en comandos guardados: ejecutar en vez de solo pegar).
    let run: (_ alternate: Bool) -> Void

    var kindLabel: String {
        switch kind {
        case .tab: return "Pestaña"; case .server: return "Servidor"; case .instance: return "EC2"
        case .snippet: return "Comando"; case .action: return "Acción"
        }
    }
}

/// Búsqueda difusa: cada palabra de la consulta debe aparecer (prefijo > subcadena > letras en orden).
enum Fuzzy {
    static func score(_ query: String, item: FinderItem) -> Int? {
        let tokens = query.lowercased().split(whereSeparator: { $0 == " " }).map(String.init)
        if tokens.isEmpty { return 0 }
        let title = item.title.lowercased()
        let hay = (item.title + " " + item.subtitle + " " + item.keywords + " " + (item.env?.title ?? "") + " " + (item.env?.tag ?? "")).lowercased()
        var total = 0
        for t in tokens {
            if title.hasPrefix(t) { total += 300 }
            else if let r = title.range(of: t) { total += 200 - min(50, title.distance(from: title.startIndex, to: r.lowerBound)) }
            else if hay.contains(t) { total += 100 }
            else if t.count >= 3, isSubsequence(t, of: title) { total += 40 }
            else { return nil }
        }
        return total
    }
    private static func isSubsequence(_ needle: String, of hay: String) -> Bool {
        var it = hay.makeIterator()
        for ch in needle { var found = false; while let c = it.next() { if c == ch { found = true; break } }; if !found { return false } }
        return true
    }
}

private final class FinderPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}

private final class FinderRowView: NSTableRowView {
    override func drawSelection(in dirtyRect: NSRect) {
        let r = bounds.insetBy(dx: 6, dy: 2)
        Theme.accent.withAlphaComponent(0.28).setFill()
        NSBezierPath(roundedRect: r, xRadius: 8, yRadius: 8).fill()
    }
}

private final class FinderCell: NSView {
    init(item: FinderItem) {
        super.init(frame: .zero)
        let icon = NSImageView(image: NSImage(systemSymbolName: item.symbol, accessibilityDescription: nil) ?? NSImage())
        icon.symbolConfiguration = .init(pointSize: 14, weight: .regular)
        icon.contentTintColor = item.env?.color ?? Theme.accent
        icon.translatesAutoresizingMaskIntoConstraints = false
        icon.widthAnchor.constraint(equalToConstant: 22).isActive = true
        let title = NSTextField(labelWithString: item.title)
        title.font = .systemFont(ofSize: 13, weight: .medium); title.textColor = .white; title.lineBreakMode = .byTruncatingTail
        let sub = NSTextField(labelWithString: item.subtitle)
        sub.font = .systemFont(ofSize: 11); sub.textColor = .secondaryLabelColor; sub.lineBreakMode = .byTruncatingTail
        for l in [title, sub] {
            l.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)
            l.setContentCompressionResistancePriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        }
        let titleRow = NSStackView(views: [title] + (item.env.map { [EnvTag($0)] } ?? [])); titleRow.spacing = 6
        let texts = NSStackView(views: [titleRow, sub]); texts.orientation = .vertical; texts.alignment = .leading; texts.spacing = 1
        texts.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        let kind = NSTextField(labelWithString: item.kindLabel)
        kind.font = .systemFont(ofSize: 10, weight: .medium); kind.textColor = .tertiaryLabelColor
        kind.setContentHuggingPriority(.required, for: .horizontal)
        kind.setContentCompressionResistancePriority(.required, for: .horizontal)
        let row = NSStackView(views: [icon, texts, kind]); row.spacing = 10; row.alignment = .centerY
        row.translatesAutoresizingMaskIntoConstraints = false
        addSubview(row)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 18), row.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -18),
            row.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }
    required init?(coder: NSCoder) { fatalError() }
}

/// Panel flotante de búsqueda rápida (⌘K). Escribes parte de un nombre y entras directo.
final class QuickFinder: NSObject, NSTableViewDataSource, NSTableViewDelegate, NSTextFieldDelegate, NSWindowDelegate {
    static let shared = QuickFinder()

    private static let width: CGFloat = 600, searchH: CGFloat = 52, rowH: CGFloat = 46, footerH: CGFloat = 26, maxRows = 8
    private let panel = FinderPanel(contentRect: NSRect(x: 0, y: 0, width: 600, height: 200),
                                    styleMask: [.borderless], backing: .buffered, defer: false)
    private let field = NSTextField()
    private let table = NSTableView()
    private let scroll = NSScrollView()
    private let emptyLabel = NSTextField(labelWithString: "Sin resultados")
    private let divider = NSView()
    private var all: [FinderItem] = []
    private var shown: [FinderItem] = []
    private var topY: CGFloat = 0
    private weak var parent: NSWindow?

    private override init() {
        super.init()
        panel.isOpaque = false; panel.backgroundColor = .clear; panel.hasShadow = true
        panel.level = .floating; panel.hidesOnDeactivate = true; panel.isReleasedWhenClosed = false
        panel.delegate = self

        let box = NSView(); box.wantsLayer = true
        box.layer?.cornerRadius = 14; box.layer?.masksToBounds = true; box.layer?.borderWidth = 1
        box.layer?.backgroundColor = NSColor(srgbRed: 0.11, green: 0.12, blue: 0.15, alpha: 0.98).cgColor
        box.layer?.borderColor = NSColor.white.withAlphaComponent(0.12).cgColor
        panel.contentView = box

        let glass = NSImageView(image: NSImage(systemSymbolName: "magnifyingglass", accessibilityDescription: nil)!)
        glass.symbolConfiguration = .init(pointSize: 16, weight: .medium); glass.contentTintColor = .secondaryLabelColor
        field.isBordered = false; field.drawsBackground = false; field.focusRingType = .none
        field.font = .systemFont(ofSize: 18); field.textColor = .white
        field.placeholderAttributedString = NSAttributedString(string: "Buscar servidor, instancia, pestaña o comando…", attributes: [
            .foregroundColor: NSColor.white.withAlphaComponent(0.30), .font: NSFont.systemFont(ofSize: 18)])
        field.delegate = self
        divider.wantsLayer = true; divider.layer?.backgroundColor = NSColor.white.withAlphaComponent(0.08).cgColor

        let col = NSTableColumn(identifier: .init("f")); col.resizingMask = .autoresizingMask
        table.addTableColumn(col); table.headerView = nil; table.backgroundColor = .clear
        table.rowHeight = Self.rowH; table.intercellSpacing = .zero; table.selectionHighlightStyle = .regular
        table.dataSource = self; table.delegate = self
        table.target = self; table.action = #selector(rowClicked)
        scroll.documentView = table; scroll.drawsBackground = false; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true

        emptyLabel.font = .systemFont(ofSize: 13); emptyLabel.textColor = .secondaryLabelColor; emptyLabel.alignment = .center
        let hint = NSTextField(labelWithString: "↩ abrir     ⌥↩ pegar y ejecutar comando     ↑↓ moverse     esc cerrar")
        hint.font = .systemFont(ofSize: 10); hint.textColor = .tertiaryLabelColor; hint.alignment = .center

        for v in [glass, field, divider, scroll, emptyLabel, hint] as [NSView] { v.translatesAutoresizingMaskIntoConstraints = false; box.addSubview(v) }
        NSLayoutConstraint.activate([
            glass.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 20),
            glass.centerYAnchor.constraint(equalTo: box.topAnchor, constant: Self.searchH / 2),
            field.leadingAnchor.constraint(equalTo: glass.trailingAnchor, constant: 12),
            field.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -20),
            field.centerYAnchor.constraint(equalTo: glass.centerYAnchor),
            divider.topAnchor.constraint(equalTo: box.topAnchor, constant: Self.searchH),
            divider.leadingAnchor.constraint(equalTo: box.leadingAnchor), divider.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            divider.heightAnchor.constraint(equalToConstant: 1),
            scroll.topAnchor.constraint(equalTo: divider.bottomAnchor, constant: 4),
            scroll.leadingAnchor.constraint(equalTo: box.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: hint.topAnchor, constant: -2),
            emptyLabel.centerXAnchor.constraint(equalTo: box.centerXAnchor),
            emptyLabel.topAnchor.constraint(equalTo: divider.bottomAnchor, constant: 18),
            hint.leadingAnchor.constraint(equalTo: box.leadingAnchor), hint.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            hint.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -6),
        ])
    }

    // MARK: Mostrar / ocultar

    func toggle(over window: NSWindow, items: [FinderItem]) {
        if panel.isVisible { close(); return }
        parent = window
        all = items
        field.stringValue = ""
        filter()
        let x = window.frame.midX - Self.width / 2
        topY = window.frame.maxY - 150
        panel.setFrameOrigin(NSPoint(x: x, y: topY - panel.frame.height))
        window.addChildWindow(panel, ordered: .above)
        panel.makeKeyAndOrderFront(nil)
        panel.makeFirstResponder(field)
    }

    private func close() {
        parent?.removeChildWindow(panel)
        panel.orderOut(nil)
        parent?.makeKeyAndOrderFront(nil)
    }
    func windowDidResignKey(_ notification: Notification) { if panel.isVisible { close() } }

    // MARK: Filtro

    func controlTextDidChange(_ obj: Notification) { filter() }

    private func filter() {
        let q = field.stringValue.trimmingCharacters(in: .whitespaces)
        if q.isEmpty {
            shown = all
        } else {
            shown = all.compactMap { it in Fuzzy.score(q, item: it).map { (it, $0) } }
                .sorted { ($0.1, -$0.0.kind.rawValue) > ($1.1, -$1.0.kind.rawValue) }.map(\.0)
        }
        table.reloadData()
        if !shown.isEmpty { table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false); table.scrollRowToVisible(0) }
        emptyLabel.isHidden = !shown.isEmpty
        scroll.isHidden = shown.isEmpty
        let rows = min(Self.maxRows, shown.count)
        let listH = shown.isEmpty ? 64 : CGFloat(rows) * Self.rowH + 8
        let h = Self.searchH + 1 + listH + Self.footerH
        panel.setFrame(NSRect(x: panel.frame.origin.x, y: topY - h, width: Self.width, height: h), display: true)
    }

    // MARK: Tabla

    func numberOfRows(in tableView: NSTableView) -> Int { shown.count }
    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? { FinderRowView() }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? { FinderCell(item: shown[row]) }

    @objc private func rowClicked() {
        let r = table.clickedRow
        if r >= 0 { open(row: r, alternate: NSEvent.modifierFlags.contains(.option)) }
    }

    private func move(_ d: Int) {
        guard !shown.isEmpty else { return }
        let next = min(max(table.selectedRow + d, 0), shown.count - 1)
        table.selectRowIndexes(IndexSet(integer: next), byExtendingSelection: false)
        table.scrollRowToVisible(next)
    }

    private func open(row: Int, alternate: Bool) {
        guard shown.indices.contains(row) else { return }
        let item = shown[row]
        close()
        DispatchQueue.main.async { item.run(alternate) }     // tras devolver el foco a la ventana principal
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
        switch sel {
        case #selector(NSResponder.moveUp(_:)): move(-1); return true
        case #selector(NSResponder.moveDown(_:)): move(1); return true
        case #selector(NSResponder.insertNewline(_:)):
            open(row: table.selectedRow, alternate: NSApp.currentEvent?.modifierFlags.contains(.option) ?? false); return true
        case #selector(NSResponder.cancelOperation(_:)): close(); return true
        case #selector(NSResponder.insertTab(_:)): move(1); return true
        case #selector(NSResponder.insertBacktab(_:)): move(-1); return true
        default: return false
        }
    }
}
