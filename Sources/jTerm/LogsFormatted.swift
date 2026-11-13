import AppKit

struct LogViewOptions: Equatable {
    var badges = true
    var json = true
    var group = true
}

// MARK: - Dibujo de las filas

enum LogRowRenderer {
    enum Kind {
        case group(LogGroupInfo, collapsed: Bool)
        case event(ParsedLog, expanded: Bool, grouped: Bool)
    }
    struct Content {
        var kind: Kind
        var opts: LogViewOptions
        var time: String
        var fullTime: String
        var timeWidth: CGFloat
        var hover = false
    }

    static let rowHeight: CGFloat = 26
    static let groupHeight: CGFloat = 34

    static func color(_ l: LogLevel, _ th: TerminalTheme) -> NSColor {
        switch l {
        case .error: return NSColor(hex: th.ansi[9])
        case .warn: return NSColor(hex: th.ansi[11])
        case .info: return NSColor(hex: th.ansi[12])
        case .debug, .platform: return NSColor(hex: th.ansi[8])
        }
    }

    static func monoFont() -> NSFont {
        let base = Settings.shared.font
        return NSFont(descriptor: base.fontDescriptor, size: min(max(base.pointSize, 11), 13)) ?? base
    }

    private static func para(_ mode: NSLineBreakMode) -> NSMutableParagraphStyle { let p = NSMutableParagraphStyle(); p.lineBreakMode = mode; return p }
    private static func width(_ s: String, _ f: NSFont) -> CGFloat { ceil((s as NSString).size(withAttributes: [.font: f]).width) }

    /// Texto del detalle desplegado (JSON con formato, líneas siguientes, o el mensaje completo si no cabe).
    static func detail(_ p: ParsedLog, opts: LogViewOptions, th: TerminalTheme, font: NSFont, fits: Bool) -> NSAttributedString? {
        let dim = NSColor(hex: th.ansi[8])
        let out = NSMutableAttributedString()
        func add(_ s: String, _ c: NSColor) { out.append(NSAttributedString(string: s, attributes: [.font: font, .foregroundColor: c])) }
        if let node = p.json, opts.json {
            var budget = 6000
            for (s, kind) in OrderedJSON.pretty(node, budget: &budget) {
                let c: NSColor
                switch kind {
                case .key: c = NSColor(hex: th.ansi[12])
                case .string: c = NSColor(hex: th.ansi[10])
                case .number: c = NSColor(hex: th.ansi[11])
                case .literal: c = NSColor(hex: th.ansi[13])
                case .punct: c = dim
                }
                add(s, c)
            }
            if budget <= 0 { add("\n… (usa “Copiar JSON” para ver todo)", dim) }
            return out
        }
        if p.lines.count > 1 {
            let extra = p.extraLines
            let shown = extra.prefix(80)
            for (i, l) in shown.enumerated() {
                let isLast = i == extra.count - 1
                add(l + (i < shown.count - 1 ? "\n" : ""), (p.level == .error && isLast) ? NSColor(hex: th.ansi[9]) : (p.level == .error ? dim : th.fgColor))
            }
            if extra.count > 80 { add("\n… \(extra.count - 80) líneas más", dim) }
            return out
        }
        if !fits { add(p.json != nil ? p.lines[0] : p.summary, th.fgColor); return out }
        return nil
    }

    private struct Chip { var text: String; var fg: NSColor; var bg: NSColor }

    private static func drawChips(_ chips: [Chip], x: CGFloat, y: CGFloat, font: NSFont, draw: Bool) -> CGFloat {
        var cx = x
        for c in chips {
            let w = width(c.text, font) + 14
            if draw {
                c.bg.setFill(); NSBezierPath(roundedRect: NSRect(x: cx, y: y, width: w, height: 18), xRadius: 4, yRadius: 4).fill()
                (c.text as NSString).draw(at: NSPoint(x: cx + 7, y: y + 2), withAttributes: [.font: font, .foregroundColor: c.fg])
            }
            cx += w + 6
        }
        return cx - x
    }

    static func render(_ c: Content, width w: CGFloat, draw: Bool) -> (height: CGFloat, copyRect: NSRect) {
        let th = Settings.shared.theme
        let mono = monoFont()
        let sans = NSFont.systemFont(ofSize: 12), chipFont = NSFont.systemFont(ofSize: 11, weight: .medium)
        func col(_ i: Int) -> NSColor { NSColor(hex: th.ansi[i]) }
        let fg = th.fgColor, dim = col(8)
        let chipBg = fg.withAlphaComponent(0.09)

        switch c.kind {
        case .group(let info, let collapsed):
            if draw {
                fg.withAlphaComponent(c.hover ? 0.08 : 0.05).setFill(); NSRect(x: 0, y: 0, width: w, height: groupHeight).fill()
                fg.withAlphaComponent(0.06).setFill(); NSRect(x: 0, y: 0, width: w, height: 0.5).fill()
            }
            let ridShown = info.rid.count > 13 ? String(info.rid.prefix(8)) + "…" : info.rid
            let title = "Invocación \(ridShown)"
            let tf = NSFont.systemFont(ofSize: 13, weight: .medium)
            if draw { (title as NSString).draw(at: NSPoint(x: 14, y: 8), withAttributes: [.font: tf, .foregroundColor: fg]) }
            var chips: [Chip] = []
            if let r = info.report {
                if let d = r.durationMs { chips.append(Chip(text: d >= 1000 ? String(format: "%.2f s", d / 1000) : String(format: "%.0f ms", d), fg: col(7), bg: chipBg)) }
                if let used = r.maxMemoryMB, let size = r.memorySizeMB { chips.append(Chip(text: "\(used) / \(size) MB", fg: col(7), bg: chipBg)) }
            } else if !info.ended { chips.append(Chip(text: "en curso", fg: col(7), bg: chipBg)) }
            chips.append(Chip(text: "\(info.count) evento\(info.count == 1 ? "" : "s")", fg: col(7), bg: chipBg))
            if let st = info.report?.status, st.lowercased() != "success" { chips.append(Chip(text: st, fg: col(9), bg: col(9).withAlphaComponent(0.2))) }
            else if info.hasError { chips.append(Chip(text: "con error", fg: col(9), bg: col(9).withAlphaComponent(0.2))) }
            _ = drawChips(chips, x: 14 + width(title, tf) + 12, y: 8, font: chipFont, draw: draw)
            if draw { ((collapsed ? "›" : "⌄") as NSString).draw(at: NSPoint(x: w - 22, y: 8), withAttributes: [.font: sans, .foregroundColor: dim]) }
            return (groupHeight, .zero)

        case .event(let p, let expanded, let grouped):
            let indent: CGFloat = grouped ? 28 : 14
            let badgeX = indent + c.timeWidth + 8, badgeW: CGFloat = 62
            let msgX = badgeX + badgeW + 10, chevX = w - 22
            let msgW = max(60, chevX - msgX - 8)
            let levelColor = color(p.level, th)

            // Primera línea: resumen + datos clave
            var chips: [Chip] = []
            let summary: String
            if p.json != nil && c.opts.json {
                summary = p.summary
                chips = p.chips.map { Chip(text: "\($0.key) \($0.value)", fg: col(7), bg: chipBg) }
            } else if p.json != nil { summary = p.lines[0] }
            else {
                summary = p.summary
                if p.lines.count > 1 { chips = [Chip(text: p.isTraceback ? "traceback · \(p.lines.count - 1) líneas" : "\(p.lines.count - 1) líneas más", fg: p.isTraceback ? col(9) : col(7), bg: p.isTraceback ? col(9).withAlphaComponent(0.2) : chipBg)] }
            }
            let textColor: NSColor = p.level == .error ? col(9).blended(withFraction: 0.35, of: fg) ?? col(9)
                : p.level == .warn ? col(11) : (p.level == .debug || p.level == .platform ? dim : fg)
            let natural = width(summary, mono)
            let chipsW = chips.isEmpty ? 0 : drawChips(chips, x: 0, y: 0, font: chipFont, draw: false) - 6
            var textW = min(natural, msgW)
            var showChips = !chips.isEmpty
            if showChips {
                if natural + chipsW + 8 <= msgW { textW = natural }
                else if msgW - chipsW - 8 >= 90 { textW = msgW - chipsW - 8 }
                else { showChips = false; textW = msgW }
            }

            // Detalle desplegado
            var blockH: CGFloat = 0
            var detailText: NSAttributedString?
            if expanded {
                detailText = detail(p, opts: c.opts, th: th, font: mono, fits: natural <= textW + 1)
                if let d = detailText {
                    blockH = ceil(d.boundingRect(with: NSSize(width: msgW - 16, height: .greatestFiniteMagnitude), options: [.usesLineFragmentOrigin, .usesFontLeading]).height) + 16
                }
            }
            let metaY = rowHeight + (blockH > 0 ? blockH + 6 : 0) + 2
            let height = expanded ? metaY + 22 + 6 : rowHeight

            if draw {
                if p.level == .error { col(9).withAlphaComponent(0.09).setFill(); NSRect(x: 0, y: 0, width: w, height: height).fill() }
                else if c.hover { fg.withAlphaComponent(0.04).setFill(); NSRect(x: 0, y: 0, width: w, height: height).fill() }
                fg.withAlphaComponent(0.06).setFill(); NSRect(x: 0, y: 0, width: w, height: 0.5).fill()
                (c.time as NSString).draw(at: NSPoint(x: indent, y: 6), withAttributes: [.font: mono, .foregroundColor: dim])
                // etiqueta de nivel
                if c.opts.badges {
                    levelColor.withAlphaComponent(0.2).setFill()
                    NSBezierPath(roundedRect: NSRect(x: badgeX, y: 4, width: 58, height: 18), xRadius: 4, yRadius: 4).fill()
                    let lf = NSFont.systemFont(ofSize: 10.5, weight: .semibold), lw = width(p.level.label, lf)
                    (p.level.label as NSString).draw(at: NSPoint(x: badgeX + (58 - lw) / 2, y: 6), withAttributes: [.font: lf, .foregroundColor: levelColor])
                } else {
                    (p.level.label as NSString).draw(at: NSPoint(x: badgeX, y: 6), withAttributes: [.font: NSFont.systemFont(ofSize: 11, weight: .semibold), .foregroundColor: levelColor])
                }
                (summary as NSString).draw(with: NSRect(x: msgX, y: 5, width: textW, height: 17), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine],
                                           attributes: [.font: mono, .foregroundColor: textColor, .paragraphStyle: para(.byTruncatingTail)])
                if showChips { _ = drawChips(chips, x: msgX + textW + 8, y: 4, font: chipFont, draw: true) }
                ((expanded ? "⌄" : "›") as NSString).draw(at: NSPoint(x: chevX, y: 5), withAttributes: [.font: sans, .foregroundColor: dim])
                if let d = detailText, blockH > 0 {
                    NSColor.black.withAlphaComponent(0.28).setFill()
                    NSBezierPath(roundedRect: NSRect(x: msgX, y: rowHeight - 2, width: msgW, height: blockH), xRadius: 6, yRadius: 6).fill()
                    d.draw(with: NSRect(x: msgX + 8, y: rowHeight + 6, width: msgW - 16, height: blockH - 12), options: [.usesLineFragmentOrigin])
                }
            }
            var copyRect = NSRect.zero
            if expanded {
                let meta = "\(c.fullTime)  ·  stream \(p.event.stream)  ·  "
                let mf = NSFont.systemFont(ofSize: 12)
                let metaW = width(meta, mf)
                let linkText = p.json != nil && c.opts.json ? "Copiar JSON" : "Copiar mensaje"
                copyRect = NSRect(x: msgX + metaW, y: metaY, width: width(linkText, mf), height: 18)
                if draw {
                    (meta as NSString).draw(at: NSPoint(x: msgX, y: metaY), withAttributes: [.font: mf, .foregroundColor: dim])
                    (linkText as NSString).draw(at: NSPoint(x: copyRect.minX, y: metaY), withAttributes: [.font: mf, .foregroundColor: col(12)])
                }
            }
            return (height, copyRect)
        }
    }
}

final class LogRowView: NSView {
    var content: LogRowRenderer.Content? { didSet { needsDisplay = true } }
    var onToggle: (() -> Void)?
    var onCopy: (() -> Void)?
    var menuProvider: (() -> NSMenu?)?
    private var copyRect = NSRect.zero
    private var hovering = false { didSet { needsDisplay = true } }
    override var isFlipped: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self))
    }
    required init?(coder: NSCoder) { fatalError() }

    override func draw(_ dirtyRect: NSRect) {
        guard var c = content else { return }
        c.hover = hovering
        copyRect = LogRowRenderer.render(c, width: bounds.width, draw: true).copyRect
    }
    override func mouseEntered(with e: NSEvent) { hovering = true }
    override func mouseExited(with e: NSEvent) { hovering = false }
    override func mouseDown(with e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        if !copyRect.isEmpty, copyRect.insetBy(dx: -4, dy: -3).contains(p) { onCopy?() } else { onToggle?() }
    }
    override func menu(for event: NSEvent) -> NSMenu? { menuProvider?() }
}

// MARK: - Vista formateada (filtros por nivel + lista de filas)

final class FormattedLogsView: NSView, NSTableViewDataSource, NSTableViewDelegate {
    var opts = LogViewOptions() { didSet { if oldValue != opts { rebuild() } } }
    var timeWithDate = false
    var onCopyText: ((String, String) -> Void)?        // (texto, descripción)

    private let chipsBar = NSStackView()
    private let scroll = NSScrollView()
    private let table = NSTableView()
    private(set) var parsed: [ParsedLog] = []
    private var rows: [LogRowModel] = []
    private var infos: [String: LogGroupInfo] = [:]
    private var filter: LogLevel?
    private var expanded = Set<String>()
    private var collapsedGroups = Set<String>()
    private var heightCache: [String: CGFloat] = [:]
    private var lastWidth: CGFloat = 0
    private static let timeShort: DateFormatter = { let f = DateFormatter(); f.dateFormat = "HH:mm:ss.SSS"; return f }()
    private static let timeLong: DateFormatter = { let f = DateFormatter(); f.dateFormat = "dd/MM HH:mm:ss.SSS"; return f }()
    private static let timeFull: DateFormatter = { let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd HH:mm:ss.SSS"; return f }()

    var rowCount: Int { rows.count }
    var groupCount: Int { rows.filter { if case .group = $0 { return true }; return false }.count }
    var currentFilter: LogLevel? { filter }
    var isAtBottom: Bool { let v = scroll.contentView.bounds, d = table.frame; return v.maxY >= d.maxY - 40 }

    override init(frame: NSRect) {
        super.init(frame: frame)
        chipsBar.orientation = .horizontal; chipsBar.spacing = 6; chipsBar.alignment = .centerY
        chipsBar.edgeInsets = NSEdgeInsets(top: 8, left: 14, bottom: 8, right: 14)
        let col = NSTableColumn(identifier: .init("l")); col.resizingMask = .autoresizingMask
        table.addTableColumn(col)
        table.headerView = nil; table.intercellSpacing = .zero; table.selectionHighlightStyle = .none
        table.backgroundColor = Settings.shared.theme.bgColor
        table.dataSource = self; table.delegate = self
        scroll.documentView = table; scroll.hasVerticalScroller = true; scroll.drawsBackground = true
        scroll.backgroundColor = Settings.shared.theme.bgColor
        wantsLayer = true; layer?.backgroundColor = Settings.shared.theme.bgColor.cgColor
        for v in [chipsBar, scroll] { v.translatesAutoresizingMaskIntoConstraints = false; addSubview(v) }
        NSLayoutConstraint.activate([
            chipsBar.topAnchor.constraint(equalTo: topAnchor), chipsBar.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.topAnchor.constraint(equalTo: chipsBar.bottomAnchor), scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor), scroll.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }
    required init?(coder: NSCoder) { fatalError() }

    override func layout() {
        super.layout()
        let w = scroll.contentSize.width
        if abs(w - lastWidth) > 1 {
            lastWidth = w; heightCache = [:]
            table.tableColumns.first?.width = w
            if !expanded.isEmpty { table.noteHeightOfRows(withIndexesChanged: IndexSet(0..<rows.count)) }
        }
    }

    // MARK: Datos

    func reset() {
        parsed = []; rows = []; infos = [:]; expanded = []; collapsedGroups = []; heightCache = [:]; filter = nil
        rebuildChips(); table.reloadData()
    }

    func update(_ p: [ParsedLog], scrollToEnd: Bool) {
        parsed = p
        rebuild(scrollToEnd: scrollToEnd)
    }

    private func rebuild(scrollToEnd: Bool = false) {
        infos = LogGrouping.infos(parsed)
        rows = LogGrouping.rows(parsed: parsed, group: opts.group, filter: filter, collapsed: collapsedGroups)
        heightCache = [:]
        rebuildChips()
        table.reloadData()
        if scrollToEnd, !rows.isEmpty { table.scrollRowToVisible(rows.count - 1) }
    }

    func setFilter(_ l: LogLevel?) { filter = l; rebuild() }
    func collapseAllGroups(_ collapse: Bool) {
        collapsedGroups = collapse ? Set(infos.keys) : []
        rebuild()
    }
    func toggleExpanded(eventId: String) {
        if !expanded.insert(eventId).inserted { expanded.remove(eventId) }
        heightCache = [:]
        table.reloadData()
    }
    func isExpanded(_ id: String) -> Bool { expanded.contains(id) }

    private func rebuildChips() {
        chipsBar.arrangedSubviews.forEach { chipsBar.removeArrangedSubview($0); $0.removeFromSuperview() }
        func chip(_ title: String, _ on: Bool, _ target: LogLevel?) {
            let b = NSButton(title: "", target: self, action: #selector(chipTapped(_:)))
            b.isBordered = false; b.wantsLayer = true; b.layer?.cornerRadius = 4
            b.layer?.backgroundColor = (on ? Theme.accent.withAlphaComponent(0.25) : NSColor.white.withAlphaComponent(0.06)).cgColor
            b.layer?.borderWidth = 0.5; b.layer?.borderColor = (on ? Theme.accent : NSColor.white.withAlphaComponent(0.12)).cgColor
            b.attributedTitle = NSAttributedString(string: " \(title) ", attributes: [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: on ? NSColor.white : NSColor.secondaryLabelColor])
            b.tag = target.map { $0.rawValue } ?? -1
            b.translatesAutoresizingMaskIntoConstraints = false
            b.heightAnchor.constraint(equalToConstant: 26).isActive = true
            chipsBar.addArrangedSubview(b)
        }
        chip("Todos \(parsed.count)", filter == nil, nil)
        for l in LogLevel.allCases {
            let n = parsed.filter { $0.level == l }.count
            if n > 0 || filter == l { chip("\(l.filterTitle) \(n)", filter == l, l) }
        }
    }
    @objc private func chipTapped(_ b: NSButton) { setFilter(b.tag < 0 ? nil : LogLevel(rawValue: b.tag)) }

    // MARK: Tabla

    private func timeText(_ p: ParsedLog) -> String {
        let d = Date(timeIntervalSince1970: Double(p.event.timestamp) / 1000)
        return (timeWithDate ? Self.timeLong : Self.timeShort).string(from: d)
    }
    private var timeWidth: CGFloat { timeWithDate ? 130 : 96 }

    private func content(for row: Int) -> LogRowRenderer.Content? {
        switch rows[row] {
        case .group(let rid):
            guard let info = infos[rid] else { return nil }
            return LogRowRenderer.Content(kind: .group(info, collapsed: collapsedGroups.contains(rid)), opts: opts, time: "", fullTime: "", timeWidth: timeWidth)
        case .event(let i):
            let p = parsed[i]
            let inGroup = opts.group && p.requestId != nil && p.platform != .start && p.platform != .end && p.platform != .report
            return LogRowRenderer.Content(kind: .event(p, expanded: expanded.contains(p.event.id), grouped: inGroup), opts: opts, time: timeText(p),
                                          fullTime: Self.timeFull.string(from: Date(timeIntervalSince1970: Double(p.event.timestamp) / 1000)), timeWidth: timeWidth)
        }
    }

    func numberOfRows(in t: NSTableView) -> Int { rows.count }

    func tableView(_ t: NSTableView, heightOfRow row: Int) -> CGFloat {
        switch rows[row] {
        case .group: return LogRowRenderer.groupHeight
        case .event(let i):
            guard expanded.contains(parsed[i].event.id), let c = content(for: row) else { return LogRowRenderer.rowHeight }
            let key = parsed[i].event.id
            if let h = heightCache[key] { return h }
            let h = LogRowRenderer.render(c, width: max(lastWidth, 300), draw: false).height
            heightCache[key] = h; return h
        }
    }

    func tableView(_ t: NSTableView, viewFor c: NSTableColumn?, row: Int) -> NSView? {
        let id = NSUserInterfaceItemIdentifier("logrow")
        let v = (t.makeView(withIdentifier: id, owner: self) as? LogRowView) ?? { let n = LogRowView(frame: .zero); n.identifier = id; return n }()
        v.content = content(for: row)
        v.onToggle = { [weak self] in self?.toggleRow(row) }
        v.onCopy = { [weak self] in self?.copyRow(row) }
        v.menuProvider = { [weak self] in self?.menu(forRow: row) }
        return v
    }

    private func toggleRow(_ row: Int) {
        guard rows.indices.contains(row) else { return }
        switch rows[row] {
        case .group(let rid):
            if !collapsedGroups.insert(rid).inserted { collapsedGroups.remove(rid) }
            rebuild()
        case .event(let i):
            toggleExpanded(eventId: parsed[i].event.id)
        }
    }

    // MARK: Copiar

    private func jsonText(_ p: ParsedLog) -> String? {
        guard let n = p.json else { return nil }
        var b = 1_000_000; return OrderedJSON.pretty(n, budget: &b).map(\.0).joined()
    }
    private func copyRow(_ row: Int) {
        guard rows.indices.contains(row), case .event(let i) = rows[row] else { return }
        let p = parsed[i]
        if opts.json, let j = jsonText(p) { onCopyText?(j, "JSON copiado") } else { onCopyText?(p.event.message.trimmingCharacters(in: .newlines), "Mensaje copiado") }
    }
    private func menu(forRow row: Int) -> NSMenu? {
        guard rows.indices.contains(row), case .event(let i) = rows[row] else { return nil }
        let p = parsed[i], m = NSMenu()
        let items: [(String, String, String)] = [("Copiar mensaje", p.event.message.trimmingCharacters(in: .newlines), "Mensaje copiado"),
            ("Copiar evento completo", "\(timeText(p))  \(p.event.stream)  \(p.event.message.trimmingCharacters(in: .newlines))", "Evento copiado")]
        for (title, text, desc) in items {
            let it = NSMenuItem(title: title, action: #selector(menuCopy(_:)), keyEquivalent: ""); it.target = self; it.representedObject = [text, desc]; m.addItem(it)
        }
        if let j = jsonText(p) {
            let it = NSMenuItem(title: "Copiar JSON con formato", action: #selector(menuCopy(_:)), keyEquivalent: ""); it.target = self; it.representedObject = [j, "JSON copiado"]; m.addItem(it)
        }
        return m
    }
    @objc private func menuCopy(_ s: NSMenuItem) { if let a = s.representedObject as? [String] { onCopyText?(a[0], a[1]) } }
}
