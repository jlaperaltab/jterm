import AppKit

/// Capa transparente encima del terminal que subraya las URLs (siempre) y el enlace o ruta bajo el cursor cuando se pulsa ⌘.
/// No recibe clics: todo pasa al terminal.
final class LinkOverlay: NSView {
    struct Line { var rect: NSRect; var strong: Bool }
    var lines: [Line] = [] { didSet { needsDisplay = true } }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override var isOpaque: Bool { false }

    override func draw(_ dirtyRect: NSRect) {
        for l in lines {
            let y = l.rect.minY + 1.5
            let path = NSBezierPath()
            path.move(to: NSPoint(x: l.rect.minX, y: y)); path.line(to: NSPoint(x: l.rect.maxX, y: y))
            path.lineWidth = l.strong ? 1.5 : 1
            (l.strong ? Theme.accent : Theme.accent.withAlphaComponent(0.6)).setStroke()
            path.stroke()
        }
    }
}
