import AppKit

// MARK: - Árbol de paneles (lógica pura, sin vistas)

enum PaneDirection { case left, right, up, down }

/// Elige el panel vecino en una dirección a partir de los rectángulos de los paneles.
/// Las coordenadas tienen el origen arriba a la izquierda (y crece hacia abajo).
enum PaneNavigator {
    static func neighbor(of current: Int, rects: [CGRect], direction: PaneDirection) -> Int? {
        guard rects.indices.contains(current) else { return nil }
        let c = rects[current]
        let eps: CGFloat = 2
        var best: (index: Int, gap: CGFloat, overlap: CGFloat, dist: CGFloat)?
        for (i, o) in rects.enumerated() where i != current {
            let gap: CGFloat, overlap: CGFloat, dist: CGFloat
            switch direction {
            case .right: gap = o.minX - c.maxX; overlap = min(c.maxY, o.maxY) - max(c.minY, o.minY); dist = abs(o.midY - c.midY)
            case .left:  gap = c.minX - o.maxX; overlap = min(c.maxY, o.maxY) - max(c.minY, o.minY); dist = abs(o.midY - c.midY)
            case .down:  gap = o.minY - c.maxY; overlap = min(c.maxX, o.maxX) - max(c.minX, o.minX); dist = abs(o.midX - c.midX)
            case .up:    gap = c.minY - o.maxY; overlap = min(c.maxX, o.maxX) - max(c.minX, o.minX); dist = abs(o.midX - c.midX)
            }
            guard gap >= -eps, overlap > 0 else { continue }      // tiene que estar al otro lado y compartir borde
            if let b = best {
                if gap < b.gap - eps || (abs(gap - b.gap) <= eps && (overlap > b.overlap + eps || (abs(overlap - b.overlap) <= eps && dist < b.dist))) {
                    best = (i, gap, overlap, dist)
                }
            } else { best = (i, gap, overlap, dist) }
        }
        return best?.index
    }
}

/// Árbol binario: cada nodo es un panel (hoja) o una división en dos mitades.
final class PaneTree<Leaf: AnyObject> {
    final class Node {
        weak var parent: Node?
        var leaf: Leaf?
        /// true = las dos mitades van una al lado de la otra (división vertical, como ⌘D).
        var sideBySide = true
        var first: Node?
        var second: Node?
        /// Parte del espacio que ocupa la primera mitad.
        var ratio: CGFloat = 0.5
        var isLeaf: Bool { leaf != nil }
        init(leaf: Leaf) { self.leaf = leaf }
        init(sideBySide: Bool, first: Node, second: Node) {
            self.sideBySide = sideBySide; self.first = first; self.second = second
            first.parent = self; second.parent = self
        }
    }

    private(set) var root: Node
    init(_ leaf: Leaf) { root = Node(leaf: leaf) }

    /// Paneles en orden de lectura (izquierda→derecha, arriba→abajo según el árbol).
    var leaves: [Leaf] {
        var out: [Leaf] = []
        func walk(_ n: Node) { if let l = n.leaf { out.append(l) } else { walk(n.first!); walk(n.second!) } }
        walk(root)
        return out
    }
    var count: Int { leaves.count }

    func node(for leaf: Leaf) -> Node? {
        func find(_ n: Node) -> Node? {
            if let l = n.leaf { return l === leaf ? n : nil }
            return find(n.first!) ?? find(n.second!)
        }
        return find(root)
    }

    /// Divide `leaf` en dos y coloca `new` a su derecha (o debajo). Con `newFirst` el nuevo va antes.
    @discardableResult
    func split(_ leaf: Leaf, with new: Leaf, sideBySide: Bool, newFirst: Bool = false) -> Node? {
        guard let target = node(for: leaf) else { return nil }
        let parent = target.parent
        let fresh = Node(leaf: new)
        let joined = newFirst ? Node(sideBySide: sideBySide, first: fresh, second: target)
                              : Node(sideBySide: sideBySide, first: target, second: fresh)
        joined.parent = parent
        if let p = parent {
            if p.first === target { p.first = joined } else { p.second = joined }
        } else { root = joined }
        return joined
    }

    /// Quita un panel; su hermano ocupa el lugar de la división. Devuelve el panel que debe recibir el foco,
    /// o nil si no se pudo (era el único).
    @discardableResult
    func remove(_ leaf: Leaf) -> Leaf? {
        guard let n = node(for: leaf), let p = n.parent else { return nil }
        let sibling = (p.first === n) ? p.second! : p.first!
        sibling.parent = p.parent
        if let gp = p.parent {
            if gp.first === p { gp.first = sibling } else { gp.second = sibling }
        } else { root = sibling }
        // el foco pasa al panel contiguo: el último del hermano si el cerrado iba después, el primero si iba antes
        func edge(_ x: Node, last: Bool) -> Leaf { x.leaf ?? edge(last ? x.second! : x.first!, last: last) }
        return edge(sibling, last: p.second === n)
    }

    /// Todos los nodos de división.
    var splits: [Node] {
        var out: [Node] = []
        func walk(_ n: Node) { if !n.isLeaf { out.append(n); walk(n.first!); walk(n.second!) } }
        walk(root)
        return out
    }

    /// Reparte el espacio para que los paneles de un mismo eje queden del mismo tamaño.
    func equalize() {
        for s in splits {
            func axisWeight(_ n: Node) -> Int {
                if n.isLeaf { return 1 }
                return n.sideBySide == s.sideBySide ? axisWeight(n.first!) + axisWeight(n.second!) : 1
            }
            let f = axisWeight(s.first!), g = axisWeight(s.second!)
            s.ratio = CGFloat(f) / CGFloat(f + g)
        }
    }
}

// MARK: - Vistas

/// Contenedor de un panel: el terminal, un borde para el panel activo y un velo en los inactivos.
final class PaneView: NSView {
    let terminal: NSView
    private let dim = NSView()
    private var inset: [NSLayoutConstraint] = []
    var onClick: (() -> Void)?

    init(terminal: NSView) {
        self.terminal = terminal
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 4
        terminal.translatesAutoresizingMaskIntoConstraints = false
        addSubview(terminal)
        dim.wantsLayer = true
        dim.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.22).cgColor
        dim.translatesAutoresizingMaskIntoConstraints = false
        dim.isHidden = true
        addSubview(dim)
        inset = [terminal.leadingAnchor.constraint(equalTo: leadingAnchor), terminal.trailingAnchor.constraint(equalTo: trailingAnchor),
                 terminal.topAnchor.constraint(equalTo: topAnchor), terminal.bottomAnchor.constraint(equalTo: bottomAnchor)]
        NSLayoutConstraint.activate(inset + [
            dim.leadingAnchor.constraint(equalTo: leadingAnchor), dim.trailingAnchor.constraint(equalTo: trailingAnchor),
            dim.topAnchor.constraint(equalTo: topAnchor), dim.bottomAnchor.constraint(equalTo: bottomAnchor)])
    }
    required init?(coder: NSCoder) { fatalError() }

    /// Con varios paneles se deja un margen para el borde de foco.
    func apply(multiple: Bool, active: Bool, envColor: NSColor?) {
        let m: CGFloat = multiple ? 3 : 0
        inset[0].constant = m; inset[1].constant = -m; inset[2].constant = m; inset[3].constant = -m
        layer?.borderWidth = multiple ? 1.5 : 0
        let accent = envColor ?? Theme.accent
        layer?.borderColor = (active ? accent.withAlphaComponent(0.85) : NSColor.white.withAlphaComponent(0.07)).cgColor
        dim.isHidden = !(multiple && !active)
    }

    // Un clic sobre el velo de un panel inactivo lo activa (el velo no intercepta el ratón: lo recibe el terminal).
    override func hitTest(_ point: NSPoint) -> NSView? {
        let v = super.hitTest(point)
        return v === dim ? terminal : v
    }
}

/// División con una línea fina de separación.
final class DividerSplitView: NSSplitView {
    override var dividerColor: NSColor { NSColor.white.withAlphaComponent(0.14) }
    override var dividerThickness: CGFloat { 5 }
    override func drawDivider(in rect: NSRect) {
        dividerColor.setFill()
        if isVertical { NSRect(x: rect.midX - 0.5, y: rect.minY, width: 1, height: rect.height).fill() }
        else { NSRect(x: rect.minX, y: rect.midY - 0.5, width: rect.width, height: 1).fill() }
    }
}

/// Vista de una pestaña con paneles: reconstruye las divisiones a partir del árbol.
final class SplitContainerView: NSView, NSSplitViewDelegate {
    var tree: PaneTree<TerminalTab>!
    var paneViews: [ObjectIdentifier: PaneView] = [:]
    var zoomed: TerminalTab?
    var activePane: () -> TerminalTab? = { nil }
    private var splitNodes: [ObjectIdentifier: PaneTree<TerminalTab>.Node] = [:]
    private var applying = false
    private var rootView: NSView?
    private var rebuildGeneration = 0

    func paneView(for t: TerminalTab) -> PaneView {
        if let v = paneViews[ObjectIdentifier(t)] { return v }
        let v = PaneView(terminal: t.view)
        paneViews[ObjectIdentifier(t)] = v
        return v
    }

    func forget(_ t: TerminalTab) {
        paneViews[ObjectIdentifier(t)]?.removeFromSuperview()
        paneViews[ObjectIdentifier(t)] = nil
    }

    /// Vuelve a montar todas las vistas según el árbol (los terminales se reutilizan, no se recrean).
    func rebuild() {
        // Mientras se reorganizan las vistas pasan por tamaños intermedios: no se avisa de ellos al programa que corre dentro.
        let terminals = tree.leaves.map(\.view)
        terminals.forEach { $0.jtermResizeLocked = true }
        rebuildGeneration += 1
        let generation = rebuildGeneration
        // Se desbloquea cuando AppKit terminó de colocar las vistas (en la siguiente vuelta), así el programa ve un solo tamaño.
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.layoutSubtreeIfNeeded()
            self.applying = true; self.applyRatios(); self.applying = false
            self.layoutSubtreeIfNeeded()
            if generation == self.rebuildGeneration { terminals.forEach { $0.jtermResizeLocked = false } }
        }
        rootView?.removeFromSuperview()
        splitNodes.removeAll()
        applying = true
        let multiple = tree.count > 1
        let rootNode: NSView
        if let z = zoomed, multiple {
            tree.leaves.forEach { paneView(for: $0).removeFromSuperview() }
            rootNode = paneView(for: z)
        } else {
            rootNode = build(tree.root)
        }
        rootNode.translatesAutoresizingMaskIntoConstraints = false
        addSubview(rootNode)
        NSLayoutConstraint.activate([
            rootNode.leadingAnchor.constraint(equalTo: leadingAnchor), rootNode.trailingAnchor.constraint(equalTo: trailingAnchor),
            rootNode.topAnchor.constraint(equalTo: topAnchor), rootNode.bottomAnchor.constraint(equalTo: bottomAnchor)])
        rootView = rootNode
        layoutSubtreeIfNeeded()
        applyRatios()
        applying = false
        refreshDecorations()
    }

    private func build(_ n: PaneTree<TerminalTab>.Node) -> NSView {
        if let leaf = n.leaf {
            let v = paneView(for: leaf)
            v.removeFromSuperview()
            return v
        }
        let sv = DividerSplitView()
        sv.isVertical = n.sideBySide
        sv.dividerStyle = .thin
        sv.delegate = self
        let a = build(n.first!), b = build(n.second!)
        sv.addArrangedSubview(a); sv.addArrangedSubview(b)
        sv.setHoldingPriority(.defaultLow, forSubviewAt: 0)
        splitNodes[ObjectIdentifier(sv)] = n
        return sv
    }

    private func applyRatios() {
        func walk(_ v: NSView) {
            guard let sv = v as? NSSplitView, let n = splitNodes[ObjectIdentifier(sv)] else { return }
            sv.layoutSubtreeIfNeeded()
            let total = (sv.isVertical ? sv.bounds.width : sv.bounds.height) - sv.dividerThickness
            if total > 10 { sv.setPosition(total * n.ratio, ofDividerAt: 0) }
            sv.arrangedSubviews.forEach(walk)
        }
        if let r = rootView { walk(r) }
    }

    func equalize() {
        tree.equalize()
        applying = true; applyRatios(); applying = false
    }

    /// Borde del panel activo, velo de los demás y color de entorno.
    func refreshDecorations() {
        let multiple = tree.count > 1
        let act = activePane()
        for l in tree.leaves {
            paneView(for: l).apply(multiple: multiple, active: l === act, envColor: l.env?.color)
        }
    }

    /// Rectángulos de los paneles visibles con el origen arriba a la izquierda.
    func paneRects() -> [(TerminalTab, CGRect)] {
        layoutSubtreeIfNeeded()
        return tree.leaves.compactMap { l in
            guard let pv = paneViews[ObjectIdentifier(l)], pv.window != nil || pv.superview != nil else { return nil }
            var r = convert(pv.bounds, from: pv)
            if !isFlipped { r.origin.y = bounds.height - r.maxY }
            return (l, r)
        }
    }

    // MARK: NSSplitViewDelegate

    func splitView(_ sv: NSSplitView, constrainMinCoordinate p: CGFloat, ofSubviewAt i: Int) -> CGFloat { p + 80 }
    func splitView(_ sv: NSSplitView, constrainMaxCoordinate p: CGFloat, ofSubviewAt i: Int) -> CGFloat { p - 80 }
    func splitView(_ sv: NSSplitView, canCollapseSubview v: NSView) -> Bool { false }

    func splitViewDidResizeSubviews(_ n: Notification) {
        guard !applying, let sv = n.object as? NSSplitView, let node = splitNodes[ObjectIdentifier(sv)],
              sv.arrangedSubviews.count == 2 else { return }
        let a = sv.arrangedSubviews[0].frame
        let total = (sv.isVertical ? sv.bounds.width : sv.bounds.height) - sv.dividerThickness
        guard total > 10 else { return }
        node.ratio = max(0.1, min(0.9, (sv.isVertical ? a.width : a.height) / total))
    }
}

// MARK: - Pestaña con paneles

/// Una pestaña de la barra que contiene uno o varios terminales divididos.
final class SplitTab: NSObject, Tab {
    let id = UUID()
    let container = SplitContainerView()
    let tree: PaneTree<TerminalTab>
    private(set) var active: TerminalTab
    var onExit: ((Tab) -> Void)?
    /// Todo lo que se escribe en el panel activo se envía también a los demás.
    private(set) var broadcast = false

    var panes: [TerminalTab] { tree.leaves }
    var zoomed: TerminalTab? { container.zoomed }

    init(_ first: TerminalTab) {
        tree = PaneTree(first)
        active = first
        super.init()
        container.tree = tree
        container.activePane = { [weak self] in self?.active }
        adopt(first)
        container.rebuild()
    }

    // MARK: Tab

    var title: String { active.title }
    var iconName: String { panes.count > 1 ? "rectangle.split.2x1" : active.iconName }
    var content: NSView { container }
    var focusView: NSView { active.view }
    var fileTarget: (() async throws -> SSHTarget)? { active.fileTarget }
    var connectionKey: String? { active.connectionKey }
    var isConnected: Bool { panes.contains { $0.isConnected } }
    var env: ServerEnv? { active.env }
    var isRecording: Bool { panes.contains { $0.isRecording } }
    func hasConnection(to key: String) -> Bool { panes.contains { $0.connectionKey == key && $0.isConnected } }
    func shutdown() { panes.forEach { $0.shutdown() } }

    // MARK: Paneles

    private func adopt(_ pane: TerminalTab) {
        pane.onExit = { [weak self] p in if let t = p as? TerminalTab { self?.close(pane: t) } }
        (pane.view as? JTermTerminalView)?.onFocus = { [weak self, weak pane] in
            guard let self, let pane else { return }
            self.setActive(pane, focus: false)
        }
        (pane.view as? JTermTerminalView)?.broadcastSink = { [weak self, weak pane] data in
            guard let self, let pane else { return }
            self.forward(data, from: pane)
        }
    }

    func setActive(_ pane: TerminalTab, focus: Bool = true) {
        guard panes.contains(where: { $0 === pane }) else { return }
        let changed = active !== pane
        active = pane
        container.refreshDecorations()
        if focus { container.window?.makeFirstResponder(pane.view) }
        if changed { AppState.shared.onChange?() }
    }

    /// Divide el panel activo. `clone`: el nuevo panel repite la sesión (mismo servidor); si no, abre una shell local.
    func split(sideBySide: Bool, clone: Bool) {
        let new = clone ? active.makeSibling() : TerminalTab.local(startDirectory: active.currentWorkingDirectory)
        adopt(new)
        if container.zoomed != nil { container.zoomed = nil }
        tree.split(active, with: new, sideBySide: sideBySide)
        active = new
        container.rebuild()
        container.window?.makeFirstResponder(new.view)
        AppState.shared.onChange?()
    }

    /// Cierra un panel (sin confirmar). Si era el último, se cierra la pestaña.
    func close(pane: TerminalTab) {
        guard panes.contains(where: { $0 === pane }) else { return }
        if panes.count == 1 { pane.shutdown(); onExit?(self); return }
        let wasActive = active === pane
        let next = tree.remove(pane)
        pane.shutdown()
        (pane.view as? JTermTerminalView)?.onFocus = nil
        (pane.view as? JTermTerminalView)?.broadcastSink = nil
        if container.zoomed === pane { container.zoomed = nil }
        container.forget(pane)
        if wasActive, let next { active = next }
        if panes.count == 1 { broadcast = false; container.zoomed = nil }
        container.rebuild()
        container.window?.makeFirstResponder(active.view)
        AppState.shared.onChange?()
    }

    /// ⌘W: cierra el panel activo si hay varios; con uno solo, cierra la pestaña.
    func requestClosePane() {
        guard panes.count > 1 else { AppState.shared.requestClose(self); return }
        if Settings.shared.confirmClose, active.isConnected {
            let a = NSAlert()
            a.messageText = "¿Cerrar el panel “\(active.title)”?"
            a.informativeText = "La conexión está activa y se cerrará."
            a.addButton(withTitle: "Cerrar"); a.addButton(withTitle: "Cancelar")
            guard a.runModal() == .alertFirstButtonReturn else { return }
        }
        close(pane: active)
    }

    func focus(_ direction: PaneDirection) {
        guard panes.count > 1 else { return }
        if container.zoomed != nil { container.zoomed = nil; container.rebuild() }
        let rects = container.paneRects()
        guard let cur = rects.firstIndex(where: { $0.0 === active }),
              let i = PaneNavigator.neighbor(of: cur, rects: rects.map(\.1), direction: direction) else { NSSound.beep(); return }
        setActive(rects[i].0)
    }

    /// Panel siguiente (+1) o anterior (-1) en orden cíclico.
    func cycle(_ d: Int) {
        let p = panes
        guard p.count > 1, let i = p.firstIndex(where: { $0 === active }) else { return }
        if container.zoomed != nil { container.zoomed = nil; container.rebuild() }
        setActive(p[(i + d + p.count) % p.count])
    }

    func toggleZoom() {
        guard panes.count > 1 else { NSSound.beep(); return }
        container.zoomed = container.zoomed == nil ? active : nil
        container.rebuild()
        container.window?.makeFirstResponder(active.view)
        AppState.shared.onChange?()
    }

    func equalize() {
        guard panes.count > 1 else { return }
        if container.zoomed != nil { container.zoomed = nil; container.rebuild() }
        container.equalize()
    }

    // MARK: Escribir en todos los paneles

    func toggleBroadcast() {
        guard panes.count > 1 else { NSSound.beep(); return }
        if !broadcast, panes.contains(where: { $0.env == .prod }) {
            let a = NSAlert()
            a.alertStyle = .critical
            a.messageText = "Hay un panel de PRODUCCIÓN"
            a.informativeText = "Todo lo que escribas se enviará también a ese panel. Los comandos delicados seguirán pidiendo confirmación en él."
            a.addButton(withTitle: "Cancelar"); a.addButton(withTitle: "Activar de todos modos")
            guard a.runModal() == .alertSecondButtonReturn else { return }
        }
        broadcast.toggle()
        AppState.shared.onChange?()
    }

    private func forward(_ data: ArraySlice<UInt8>, from source: TerminalTab) {
        guard broadcast else { return }
        for p in panes where p !== source {
            // Un Retorno en un panel de producción pasa por su propia revisión de comandos delicados.
            if data.count == 1, data.first == 0x0d, let v = p.view as? JTermTerminalView, !v.approveReturn() { continue }
            p.view.send(data: data)
        }
    }
}
