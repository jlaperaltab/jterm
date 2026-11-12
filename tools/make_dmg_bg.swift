// Genera el fondo del DMG: swift tools/make_dmg_bg.swift <salida.png>
import AppKit
let W = 660, H = 400, S = 2
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: W*S, pixelsHigh: H*S, bitsPerSample: 8, samplesPerPixel: 4,
    hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
rep.size = NSSize(width: W, height: H)   // 144 dpi -> Finder lo muestra a 660x400 pt
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
let r = NSRect(x: 0, y: 0, width: W, height: H)
NSGradient(colors: [NSColor(red: 0.97, green: 0.97, blue: 1.0, alpha: 1), NSColor(red: 0.84, green: 0.86, blue: 0.96, alpha: 1)])!.draw(in: r, angle: -35)
func text(_ s: String, size: CGFloat, weight: NSFont.Weight, alpha: CGFloat, y: CGFloat) {
    let p = NSMutableParagraphStyle(); p.alignment = .center
    (s as NSString).draw(in: NSRect(x: 0, y: y, width: CGFloat(W), height: size + 8), withAttributes: [
        .font: NSFont.systemFont(ofSize: size, weight: weight),
        .foregroundColor: NSColor(red: 0.12, green: 0.13, blue: 0.22, alpha: alpha), .paragraphStyle: p])
}
text("jTerm", size: 34, weight: .bold, alpha: 1, y: 330)
text("Arrastra jTerm a Aplicaciones para instalarlo", size: 15, weight: .regular, alpha: 0.75, y: 298)
// flecha entre los dos iconos (centro y = 200 desde arriba -> 200 desde abajo)
let a = NSBezierPath(); a.lineWidth = 7; a.lineCapStyle = .round; a.lineJoinStyle = .round
a.move(to: NSPoint(x: 262, y: 195)); a.line(to: NSPoint(x: 392, y: 195))
a.move(to: NSPoint(x: 366, y: 221)); a.line(to: NSPoint(x: 394, y: 195)); a.line(to: NSPoint(x: 366, y: 169))
NSColor(red: 0.35, green: 0.30, blue: 0.80, alpha: 0.9).setStroke(); a.stroke()
text("Primera vez: lee LEEME.txt si macOS bloquea la app", size: 12, weight: .regular, alpha: 0.5, y: 22)
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
