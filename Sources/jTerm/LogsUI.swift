import AppKit

// MARK: - Pestaña

/// Pestaña para ver logs de CloudWatch de una cuenta de AWS.
final class LogsTab: Tab {
    let id = UUID()
    var title: String
    let iconName = "doc.text.magnifyingglass"
    let view: LogsView
    var content: NSView { view }
    var focusView: NSView { view }
    var onExit: ((Tab) -> Void)?
    var fileTarget: (() async throws -> SSHTarget)? { nil }
    var connectionKey: String? { nil }
    var isConnected: Bool { false }

    init(account: AWSAccount, region: String, credentials: @escaping () throws -> AWSCredentials) {
        title = "Logs · " + (account.name.isEmpty ? account.accessKeyId : account.name)
        view = LogsView(accountId: account.id.uuidString, region: region, credentials: credentials)
    }
    func shutdown() { view.stopFollow() }
}

// MARK: - Selector de grupos de logs (ventana emergente con búsqueda)

final class LogGroupPicker: NSObject, NSPopoverDelegate, NSTableViewDataSource, NSTableViewDelegate, NSTextFieldDelegate {
    var onPick: ((String) -> Void)?
    var onReload: (() -> Void)?
    private let popover = NSPopover()
    private let root = NSView(frame: NSRect(x: 0, y: 0, width: 420, height: 440))
    private var search: CompactField!
    private let table = NSTableView()
    private let note = NSTextField(wrappingLabelWithString: "")
    private var all: [LogGroup] = []
    private var filtered: [LogGroup] = []
    private var loading = false
    var isShown: Bool { popover.isShown }

    override init() {
        super.init()
        popover.behavior = .transient
        popover.appearance = NSAppearance(named: .darkAqua)
        root.wantsLayer = true
        root.layer?.backgroundColor = NSColor(srgbRed: 0.105, green: 0.115, blue: 0.14, alpha: 1).cgColor
        let vc = NSViewController(); vc.view = root; popover.contentViewController = vc

        search = CompactField(value: "", placeholder: "Buscar grupo de logs…", width: 330)
        search.field.delegate = self
        let reload = IconButton(symbol: "arrow.clockwise", tip: "Recargar la lista", size: 30, target: self, action: #selector(reloadTapped))
        let top = NSStackView(views: [search, reload]); top.spacing = 8

        let col = NSTableColumn(identifier: .init("g")); col.resizingMask = .autoresizingMask
        table.addTableColumn(col)
        table.headerView = nil; table.backgroundColor = .clear; table.rowHeight = 44
        table.dataSource = self; table.delegate = self
        table.target = self; table.action = #selector(rowClicked)
        let scroll = NSScrollView()
        scroll.documentView = table; scroll.drawsBackground = false; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true

        note.font = .systemFont(ofSize: 12); note.textColor = .secondaryLabelColor; note.alignment = .center; note.maximumNumberOfLines = 0

        for v in [top, scroll, note] { v.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(v) }
        NSLayoutConstraint.activate([
            root.widthAnchor.constraint(equalToConstant: 420), root.heightAnchor.constraint(equalToConstant: 440),
            top.topAnchor.constraint(equalTo: root.topAnchor, constant: 14),
            top.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 14),
            top.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -14),
            scroll.topAnchor.constraint(equalTo: top.bottomAnchor, constant: 10),
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 6),
            scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -6),
            scroll.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -8),
            note.centerXAnchor.constraint(equalTo: root.centerXAnchor), note.centerYAnchor.constraint(equalTo: scroll.centerYAnchor),
            note.widthAnchor.constraint(equalToConstant: 300),
        ])
    }

    func show(groups: [LogGroup], loading: Bool, relativeTo rect: NSRect, of view: NSView) {
        if popover.isShown { popover.performClose(nil); return }
        search.setValue(""); update(groups: groups, loading: loading)
        popover.show(relativeTo: rect, of: view, preferredEdge: .maxY)
        DispatchQueue.main.async { [weak self] in self?.search.window?.makeFirstResponder(self?.search.field) }
    }

    /// Refresca la lista (p. ej. cuando termina de cargar mientras el selector está abierto).
    func update(groups: [LogGroup], loading: Bool) {
        all = groups; self.loading = loading; applyFilter()
    }

    private func applyFilter() {
        let q = search.field.stringValue.trimmingCharacters(in: .whitespaces)
        filtered = q.isEmpty ? all : all.filter { $0.name.range(of: q, options: [.caseInsensitive, .diacriticInsensitive]) != nil }
        table.reloadData()
        note.isHidden = !filtered.isEmpty
        note.stringValue = loading ? "Cargando grupos de logs…" : (all.isEmpty ? "No hay grupos de logs en esta región." : "Ningún grupo coincide con la búsqueda.")
    }

    func controlTextDidChange(_ n: Notification) { applyFilter() }
    func control(_ c: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
        if sel == #selector(NSResponder.insertNewline(_:)) { if let g = filtered.first { pick(g.name) }; return true }
        if sel == #selector(NSResponder.cancelOperation(_:)) { popover.performClose(nil); return true }
        return false
    }

    func numberOfRows(in t: NSTableView) -> Int { filtered.count }
    func tableView(_ t: NSTableView, viewFor c: NSTableColumn?, row: Int) -> NSView? {
        let g = filtered[row]
        let name = NSTextField(labelWithString: g.name)
        name.font = .monospacedSystemFont(ofSize: 12, weight: .regular); name.textColor = .white; name.lineBreakMode = .byTruncatingHead
        name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        var parts: [String] = []
        parts.append(g.retentionDays.map { "Retención: \($0) días" } ?? "Retención: sin límite")
        parts.append(ByteCountFormatter.string(fromByteCount: g.storedBytes, countStyle: .file))
        let detail = NSTextField(labelWithString: parts.joined(separator: " · "))
        detail.font = .systemFont(ofSize: 10); detail.textColor = .secondaryLabelColor
        let s = NSStackView(views: [name, detail]); s.orientation = .vertical; s.alignment = .leading; s.spacing = 2
        s.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 4, right: 10)
        return s
    }
    @objc private func rowClicked() { let r = table.clickedRow; if filtered.indices.contains(r) { pick(filtered[r].name) } }
    @objc private func reloadTapped() { onReload?() }
    private func pick(_ name: String) { popover.performClose(nil); onPick?(name) }
}

// MARK: - Vista de logs

enum LogsMode: String { case formatted = "fmt", text }


final class LogsView: NSView {
    static let maxEvents = 2000        // eventos que carga una búsqueda
    private static let hardCap = 5000  // al seguir, se descartan los más antiguos

    private let accountId: String
    private let credentials: () throws -> AWSCredentials
    private var region: String

    private let regionField = Dropdown(symbol: "globe")
    private let groupField = Dropdown(symbol: "text.alignleft")
    private let rangeField = Dropdown(symbol: "clock")
    private let filter = CompactField(value: "", placeholder: "ERROR  ·  ?ERROR ?WARN  ·  { $.level = \"error\" }", width: 340, mono: true)
    private let streamPrefix = CompactField(value: "", placeholder: "Prefijo del stream (opcional)", width: 200)
    private var searchButton: PillButton!
    private var followButton: PillButton!
    private let text = NSTextView()
    private let scroll = NSScrollView()
    private let status = NSTextField(labelWithString: "")
    private let picker = LogGroupPicker()
    private var mode: LogsMode = .formatted
    private var opts = LogViewOptions()
    private let formatted = FormattedLogsView()
    private var modeControl: NSSegmentedControl!
    private var parsed: [ParsedLog] = []
    private var tracker = RidTracker()
    private var textDirty = false
    var formattedView: FormattedLogsView { formatted }
    var currentMode: LogsMode { mode }

    private var groups: [LogGroup] = []
    private var groupsLoading = false
    private(set) var selectedGroup: String?
    private var events: [LogEvent] = []
    private var seen = Set<String>()
    private var generation = 0
    private var searching = false
    private(set) var following = false
    private var followTimer: Timer?
    private var followBusy = false
    private var followFrom: Int64 = 0
    private let ranges: [(String, TimeInterval)] = [("Últimos 5 minutos", 300), ("Últimos 15 minutos", 900), ("Última hora", 3600),
                                                    ("Últimas 6 horas", 21_600), ("Últimas 24 horas", 86_400), ("Últimos 3 días", 259_200),
                                                    ("Últimos 7 días", 604_800)]

    /// Texto sin formato de todos los eventos (lo que se copia y se guarda, sea cual sea la vista).
    var plainText: String {
        events.map { e in
            let msg = e.message.hasSuffix("\n") ? String(e.message.dropLast()) : e.message
            return Self.timeFormat.string(from: Date(timeIntervalSince1970: Double(e.timestamp) / 1000)) + "  " + e.stream + "  " + msg + "\n"
        }.joined()
    }
    var statusText: String { status.stringValue }
    var eventCount: Int { events.count }

    init(accountId: String, region: String, credentials: @escaping () throws -> AWSCredentials) {
        self.accountId = accountId; self.region = region; self.credentials = credentials
        let d = UserDefaults.standard
        mode = LogsMode(rawValue: d.string(forKey: "logs.mode") ?? "") ?? .formatted         // la primera vez: formateada
        opts = LogViewOptions(badges: d.object(forKey: "logs.opt.badges") as? Bool ?? true,
                              json: d.object(forKey: "logs.opt.json") as? Bool ?? true,
                              group: d.object(forKey: "logs.opt.group") as? Bool ?? true)
        super.init(frame: .zero)
        wantsLayer = true; layer?.backgroundColor = Theme.bg.cgColor
        build()
        loadGroups()
    }
    required init?(coder: NSCoder) { fatalError() }
    deinit { followTimer?.invalidate() }

    // MARK: Construcción

    private func labeled(_ title: String, _ v: NSView) -> NSStackView {
        let s = NSStackView(views: [smallCaption(title), v]); s.orientation = .vertical; s.alignment = .leading; s.spacing = 4
        return s
    }

    private func build() {
        regionField.items = AWSClient.regions; regionField.select(title: region)
        regionField.onChange = { [weak self] in
            guard let self, let r = self.regionField.titleOfSelected, r != self.region else { return }
            self.region = r; self.stopFollow(); self.groups = []; self.selectedGroup = nil; self.clearResults(); self.loadGroups()
        }
        groupField.placeholder = "Elegir grupo de logs…"
        groupField.customAction = { [weak self] in self?.openPicker() }
        rangeField.items = ranges.map(\.0); rangeField.selectedIndex = 2

        searchButton = PillButton("Buscar", primary: true, target: self, action: #selector(runSearch))
        followButton = PillButton("Seguir", primary: false, target: self, action: #selector(toggleFollow))
        let more = IconButton(symbol: "ellipsis.circle", tip: "Copiar, guardar o limpiar", size: 32, target: self, action: #selector(showMore(_:)))
        filter.field.target = self; filter.field.action = #selector(runSearch)
        streamPrefix.field.target = self; streamPrefix.field.action = #selector(runSearch)

        regionField.widthAnchor.constraint(equalToConstant: 150).isActive = true
        rangeField.widthAnchor.constraint(equalToConstant: 190).isActive = true
        let regionCol = labeled("REGIÓN", regionField), rangeCol = labeled("RANGO", rangeField)
        let groupCol = labeled("GRUPO DE LOGS", groupField)
        groupCol.setContentHuggingPriority(.defaultLow, for: .horizontal)
        groupField.widthAnchor.constraint(greaterThanOrEqualToConstant: 240).isActive = true
        let row1 = NSStackView(views: [regionCol, groupCol, rangeCol]); row1.spacing = 12; row1.alignment = .bottom
        groupField.setContentHuggingPriority(.defaultLow, for: .horizontal)

        let filterCol = labeled("FILTRO (sintaxis de CloudWatch)", filter), streamCol = labeled("STREAM", streamPrefix)
        let spacer = NSView(); spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        modeControl = NSSegmentedControl(labels: ["Formateado", "Texto"], trackingMode: .selectOne, target: self, action: #selector(modeChanged))
        modeControl.segmentStyle = .texturedRounded
        modeControl.selectedSegment = mode == .formatted ? 0 : 1
        modeControl.toolTip = "Cambiar entre la vista formateada y el texto sin formato"
        let row2 = NSStackView(views: [filterCol, streamCol, spacer, searchButton, followButton, modeControl, more]); row2.spacing = 10; row2.alignment = .bottom
        for b in [searchButton!, followButton!] { b.widthAnchor.constraint(greaterThanOrEqualToConstant: 92).isActive = true }

        let header = NSStackView(views: [row1, row2]); header.orientation = .vertical; header.alignment = .leading; header.spacing = 12
        header.edgeInsets = NSEdgeInsets(top: 12, left: 16, bottom: 12, right: 16)
        row1.widthAnchor.constraint(equalTo: header.widthAnchor, constant: -32).isActive = true
        row2.widthAnchor.constraint(equalTo: header.widthAnchor, constant: -32).isActive = true

        let th = Settings.shared.theme
        text.isEditable = false; text.isSelectable = true; text.isRichText = true
        text.backgroundColor = th.bgColor; text.drawsBackground = true; text.insertionPointColor = th.fgColor
        text.textContainerInset = NSSize(width: 12, height: 10)
        text.isVerticallyResizable = true; text.autoresizingMask = [.width]
        text.textContainer?.widthTracksTextView = true
        text.font = Settings.shared.font
        scroll.documentView = text; scroll.hasVerticalScroller = true; scroll.drawsBackground = true; scroll.backgroundColor = th.bgColor
        scroll.wantsLayer = true; scroll.layer?.borderWidth = 1; scroll.layer?.borderColor = NSColor.white.withAlphaComponent(0.07).cgColor

        status.font = .systemFont(ofSize: 11); status.textColor = .secondaryLabelColor; status.lineBreakMode = .byTruncatingTail
        status.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        formatted.opts = opts
        formatted.onCopyText = { [weak self] text, desc in
            NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string); self?.setStatus(desc + ".")
        }
        for v in [header, scroll, formatted, status] { v.translatesAutoresizingMaskIntoConstraints = false; addSubview(v) }
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: topAnchor), header.leadingAnchor.constraint(equalTo: leadingAnchor),
            header.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.topAnchor.constraint(equalTo: header.bottomAnchor),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor), scroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: status.topAnchor, constant: -6),
            formatted.topAnchor.constraint(equalTo: scroll.topAnchor), formatted.leadingAnchor.constraint(equalTo: scroll.leadingAnchor),
            formatted.trailingAnchor.constraint(equalTo: scroll.trailingAnchor), formatted.bottomAnchor.constraint(equalTo: scroll.bottomAnchor),
            status.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 16), status.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -16),
            status.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -8),
        ])
        setStatus("Elige un grupo de logs y pulsa Buscar.")
        applyMode()

        picker.onPick = { [weak self] name in self?.selectGroup(name) }
        picker.onReload = { [weak self] in self?.loadGroups() }
    }

    func setStatus(_ s: String, error: Bool = false) {
        status.stringValue = s
        status.textColor = error ? .systemRed : .secondaryLabelColor
        status.toolTip = s
    }

    // MARK: Grupos

    func openPicker() {
        picker.show(groups: groups, loading: groupsLoading, relativeTo: groupField.bounds, of: groupField)
    }

    func selectGroup(_ name: String) {
        selectedGroup = name
        groupField.items = [name]; groupField.selectedIndex = 0
        UserDefaults.standard.set(name, forKey: "logs.group.\(accountId).\(region)")
        stopFollow()
    }

    func loadGroups() {
        guard !groupsLoading else { return }
        groupsLoading = true
        setStatus("Cargando grupos de logs…")
        let region = self.region, creds = credentials
        picker.update(groups: groups, loading: true)
        Task.detached { [weak self] in
            do {
                let c = try creds()
                var all: [LogGroup] = []; var token: String?; var pages = 0
                repeat {
                    let r = try await AWSClient.describeLogGroups(c, region: region, nextToken: token)
                    all += r.groups; token = r.nextToken; pages += 1
                } while token != nil && pages < 40
                let sorted = all.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
                await MainActor.run {
                    guard let self, region == self.region else { return }
                    self.groups = sorted; self.groupsLoading = false
                    self.picker.update(groups: sorted, loading: false)
                    self.setStatus("\(sorted.count) grupos de logs en \(region). Elige uno y pulsa Buscar.")
                    if self.selectedGroup == nil, let last = UserDefaults.standard.string(forKey: "logs.group.\(self.accountId).\(region)"),
                       sorted.contains(where: { $0.name == last }) { self.selectGroup(last) }
                }
            } catch {
                await MainActor.run {
                    guard let self else { return }
                    self.groupsLoading = false
                    self.picker.update(groups: self.groups, loading: false)
                    self.setStatus(AWSClient.logsHint(for: error.localizedDescription), error: true)
                }
            }
        }
    }

    // MARK: Buscar

    private var nowMs: Int64 { Int64(Date().timeIntervalSince1970 * 1000) }

    @objc func runSearch() {
        guard let group = selectedGroup else { setStatus("Primero elige un grupo de logs.", error: true); openPicker(); return }
        stopFollow()
        generation += 1; let gen = generation
        clearResults()
        searching = true; searchButton.isEnabled = false
        setStatus("Buscando…")
        let seconds = ranges[max(0, rangeField.selectedIndex)].1
        formatted.timeWithDate = seconds >= 86_400
        let start = nowMs - Int64(seconds * 1000)
        let pattern = filter.field.stringValue.trimmingCharacters(in: .whitespaces)
        let prefix = streamPrefix.field.stringValue.trimmingCharacters(in: .whitespaces)
        let region = self.region, creds = credentials
        Task.detached { [weak self] in
            do {
                let c = try creds()
                var token: String?; var total = 0; var pages = 0
                repeat {
                    let r = try await AWSClient.filterLogEvents(c, region: region, group: group, start: start, pattern: pattern,
                                                                streamPrefix: prefix, nextToken: token, limit: 1000)
                    token = r.nextToken; pages += 1; total += r.events.count
                    let keepGoing: Bool = await MainActor.run {
                        guard let self, gen == self.generation else { return false }
                        self.append(r.events, scrollToEnd: true)
                        self.setStatus("Buscando… \(self.events.count) eventos")
                        return true
                    }
                    if !keepGoing { return }
                } while token != nil && total < LogsView.maxEvents && pages < 40
                await MainActor.run {
                    guard let self, gen == self.generation else { return }
                    self.searching = false; self.searchButton.isEnabled = true
                    let n = self.events.count
                    let more = token != nil ? " Hay más eventos: acota el rango o el filtro para verlos." : ""
                    self.setStatus(n == 0 ? "Sin eventos en el rango y filtro elegidos." : "\(n) eventos en \(group) (\(region)).\(more)")
                }
            } catch {
                await MainActor.run {
                    guard let self, gen == self.generation else { return }
                    self.searching = false; self.searchButton.isEnabled = true
                    self.setStatus(AWSClient.logsHint(for: error.localizedDescription), error: true)
                }
            }
        }
    }

    // MARK: Seguir (consulta cada pocos segundos los eventos nuevos)

    @objc func toggleFollow() { following ? stopFollow() : startFollow() }

    func startFollow() {
        guard selectedGroup != nil else { setStatus("Primero elige un grupo de logs.", error: true); openPicker(); return }
        following = true
        followButton.title = "Detener"
        followButton.layer?.backgroundColor = NSColor.systemRed.withAlphaComponent(0.55).cgColor
        followButton.attributedTitle = NSAttributedString(string: "Detener", attributes: [.foregroundColor: NSColor.white, .font: NSFont.systemFont(ofSize: 13, weight: .semibold)])
        followFrom = events.last?.timestamp ?? (nowMs - 60_000)
        followTimer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in self?.pollFollow() }
        pollFollow()
    }

    func stopFollow() {
        guard following || followTimer != nil else { return }
        following = false
        followTimer?.invalidate(); followTimer = nil
        followBusy = false
        followButton?.layer?.backgroundColor = NSColor.white.withAlphaComponent(0.08).cgColor
        followButton?.attributedTitle = NSAttributedString(string: "Seguir", attributes: [.foregroundColor: NSColor.white, .font: NSFont.systemFont(ofSize: 13, weight: .semibold)])
    }

    func pollFollow() {
        guard following, !followBusy, let group = selectedGroup else { return }
        followBusy = true
        let gen = generation, start = followFrom, region = self.region, creds = credentials
        let pattern = filter.field.stringValue.trimmingCharacters(in: .whitespaces)
        let prefix = streamPrefix.field.stringValue.trimmingCharacters(in: .whitespaces)
        Task.detached { [weak self] in
            do {
                let c = try creds()
                var all: [LogEvent] = []; var token: String?; var pages = 0
                repeat {
                    let r = try await AWSClient.filterLogEvents(c, region: region, group: group, start: start, pattern: pattern,
                                                                streamPrefix: prefix, nextToken: token, limit: 1000)
                    all += r.events; token = r.nextToken; pages += 1
                } while token != nil && pages < 5
                await MainActor.run {
                    guard let self, self.following, gen == self.generation else { return }
                    self.followBusy = false
                    let before = self.events.count
                    self.append(all, scrollToEnd: self.isAtBottom)
                    if let last = all.map(\.timestamp).max() { self.followFrom = max(self.followFrom, last) }
                    let f = DateFormatter(); f.dateFormat = "HH:mm:ss"
                    self.setStatus("Siguiendo · \(self.events.count) eventos (+\(self.events.count - before)) · \(f.string(from: Date()))")
                }
            } catch {
                await MainActor.run {
                    guard let self else { return }
                    self.stopFollow()
                    self.setStatus(AWSClient.logsHint(for: error.localizedDescription), error: true)
                }
            }
        }
    }

    // MARK: Resultados

    private var isAtBottom: Bool {
        if mode == .formatted { return formatted.isAtBottom }
        let vis = scroll.contentView.bounds, doc = text.frame
        return vis.maxY >= doc.maxY - 40
    }

    private func clearResults() {
        events = []; seen = []; parsed = []; tracker = RidTracker(); textDirty = false
        text.textStorage?.setAttributedString(NSAttributedString())
        formatted.reset()
    }

    private static let timeFormat: DateFormatter = { let f = DateFormatter(); f.dateFormat = "dd/MM HH:mm:ss.SSS"; return f }()

    private func line(_ e: LogEvent) -> NSAttributedString {
        let th = Settings.shared.theme, font = Settings.shared.font
        func c(_ i: Int) -> NSColor { NSColor(hex: th.ansi[i]) }
        let msg = e.message.hasSuffix("\n") ? String(e.message.dropLast()) : e.message
        let lower = msg.lowercased()
        let level: NSColor = lower.contains("error") || lower.contains("exception") || lower.contains("fatal") || lower.contains("traceback") ? c(9)
            : (lower.contains("warn") ? c(11) : th.fgColor)
        let stream = e.stream.count > 14 ? "…" + e.stream.suffix(13) : e.stream
        let out = NSMutableAttributedString()
        out.append(NSAttributedString(string: Self.timeFormat.string(from: Date(timeIntervalSince1970: Double(e.timestamp) / 1000)) + "  ",
                                      attributes: [.font: font, .foregroundColor: c(8)]))
        out.append(NSAttributedString(string: stream + "  ", attributes: [.font: font, .foregroundColor: c(6)]))
        out.append(NSAttributedString(string: msg + "\n", attributes: [.font: font, .foregroundColor: level]))
        return out
    }

    /// Agrega eventos nuevos (sin repetir los ya mostrados) y actualiza la vista activa.
    private func append(_ new: [LogEvent], scrollToEnd: Bool) {
        let fresh = new.filter { seen.insert($0.id).inserted }
        guard !fresh.isEmpty else { return }
        events += fresh
        for e in fresh { var p = LogParser.parse(e); tracker.assign(&p); parsed.append(p) }
        var trimmed = false
        if events.count > Self.hardCap {
            let drop = events.count - Self.hardCap + 1000
            events.removeFirst(drop); parsed.removeFirst(drop); trimmed = true
        }
        if mode == .text {
            if trimmed { rebuildText() }
            else { let add = NSMutableAttributedString(); fresh.forEach { add.append(line($0)) }; text.textStorage?.append(add) }
            if scrollToEnd { text.scrollToEndOfDocument(nil) }
            textDirty = false
        } else {
            textDirty = true
            formatted.update(parsed, scrollToEnd: scrollToEnd)
        }
    }

    private func rebuildText() {
        let all = NSMutableAttributedString(); events.forEach { all.append(line($0)) }
        text.textStorage?.setAttributedString(all)
        textDirty = false
    }

    // MARK: Cambio de vista

    @objc private func modeChanged() { setMode(modeControl.selectedSegment == 0 ? .formatted : .text) }

    func setMode(_ m: LogsMode) {
        mode = m
        UserDefaults.standard.set(m.rawValue, forKey: "logs.mode")
        modeControl.selectedSegment = m == .formatted ? 0 : 1
        applyMode()
    }

    private func applyMode() {
        formatted.isHidden = mode != .formatted
        scroll.isHidden = mode != .text
        if mode == .formatted { formatted.opts = opts; formatted.update(parsed, scrollToEnd: false) }
        else if textDirty { rebuildText() }
    }

    private func toggleOption(_ key: String, _ keyPath: WritableKeyPath<LogViewOptions, Bool>) {
        opts[keyPath: keyPath].toggle()
        UserDefaults.standard.set(opts[keyPath: keyPath], forKey: key)
        formatted.opts = opts
    }
    @objc private func optBadges() { toggleOption("logs.opt.badges", \.badges) }
    @objc private func optJSON() { toggleOption("logs.opt.json", \.json) }
    @objc private func optGroup() { toggleOption("logs.opt.group", \.group) }
    @objc private func collapseAll() { formatted.collapseAllGroups(true) }
    @objc private func expandAll() { formatted.collapseAllGroups(false) }

    // MARK: Menú de exportación

    @objc private func showMore(_ b: NSButton) {
        let m = NSMenu()
        func opt(_ title: String, _ sel: Selector, _ on: Bool) { let i = m.addItem(withTitle: title, action: sel, keyEquivalent: ""); i.target = self; i.state = on ? .on : .off }
        opt("Etiquetas de nivel", #selector(optBadges), opts.badges)
        opt("JSON legible", #selector(optJSON), opts.json)
        opt("Agrupar por invocación", #selector(optGroup), opts.group)
        m.addItem(withTitle: "Plegar todas las invocaciones", action: #selector(collapseAll), keyEquivalent: "").target = self
        m.addItem(withTitle: "Expandir todas las invocaciones", action: #selector(expandAll), keyEquivalent: "").target = self
        m.addItem(.separator())
        m.addItem(withTitle: "Copiar todo", action: #selector(copyAll), keyEquivalent: "").target = self
        m.addItem(withTitle: "Guardar como…", action: #selector(saveAs), keyEquivalent: "").target = self
        m.addItem(.separator())
        m.addItem(withTitle: "Limpiar pantalla", action: #selector(clearTapped), keyEquivalent: "").target = self
        m.popUp(positioning: nil, at: NSPoint(x: 0, y: b.bounds.height + 2), in: b)
    }
    @objc private func copyAll() {
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(plainText, forType: .string)
        setStatus("Copiado: \(events.count) eventos.")
    }
    @objc private func saveAs() {
        let p = NSSavePanel(); p.nameFieldStringValue = "logs-\((selectedGroup ?? "cloudwatch").replacingOccurrences(of: "/", with: "_")).log"
        guard p.runModal() == .OK, let url = p.url else { return }
        do { try plainText.write(to: url, atomically: true, encoding: .utf8); setStatus("Guardado en \(url.lastPathComponent).") }
        catch { setStatus(error.localizedDescription, error: true) }
    }
    @objc private func clearTapped() { stopFollow(); generation += 1; clearResults(); setStatus("Pantalla limpia.") }
}
