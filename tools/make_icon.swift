// Genera Resources/AppIcon.icns : swift tools/make_icon.swift
import AppKit

func render(_ px: Int) -> Data {
    let s = CGFloat(px)
    let img = NSImage(size: NSSize(width: s, height: s))
    img.lockFocus()
    let rect = NSRect(x: s * 0.06, y: s * 0.06, width: s * 0.88, height: s * 0.88)
    let path = NSBezierPath(roundedRect: rect, xRadius: s * 0.2, yRadius: s * 0.2)

    // sombra suave
    NSGraphicsContext.saveGraphicsState()
    let sh = NSShadow(); sh.shadowColor = NSColor.black.withAlphaComponent(0.45)
    sh.shadowOffset = NSSize(width: 0, height: -s * 0.015); sh.shadowBlurRadius = s * 0.035
    sh.set(); NSColor.black.setFill(); path.fill()
    NSGraphicsContext.restoreGraphicsState()

    // fondo degradado oscuro
    NSGradient(colors: [NSColor(srgbRed: 0.17, green: 0.19, blue: 0.25, alpha: 1),
                        NSColor(srgbRed: 0.05, green: 0.06, blue: 0.09, alpha: 1)])!.draw(in: path, angle: -90)
    // borde sutil
    NSColor.white.withAlphaComponent(0.12).setStroke(); path.lineWidth = s * 0.006; path.stroke()

    // barra superior con 3 puntos
    let dot = s * 0.028, y = rect.maxY - s * 0.115
    for (i, c) in [NSColor(srgbRed: 1, green: 0.37, blue: 0.34, alpha: 1),
                   NSColor(srgbRed: 1, green: 0.74, blue: 0.18, alpha: 1),
                   NSColor(srgbRed: 0.16, green: 0.79, blue: 0.25, alpha: 1)].enumerated() {
        c.setFill()
        NSBezierPath(ovalIn: NSRect(x: rect.minX + s * 0.095 + CGFloat(i) * s * 0.07, y: y, width: dot * 2, height: dot * 2)).fill()
    }

    // "j" + cursor de bloque
    let font = NSFont.monospacedSystemFont(ofSize: s * 0.56, weight: .heavy)
    let grad = NSGradient(colors: [NSColor(srgbRed: 0.55, green: 0.80, blue: 1.0, alpha: 1),
                                   NSColor(srgbRed: 0.30, green: 0.50, blue: 1.0, alpha: 1)])!
    let str = NSAttributedString(string: "j", attributes: [.font: font, .foregroundColor: NSColor.black])
    let size = str.size()
    let origin = NSPoint(x: s * 0.5 - size.width * 0.78, y: s * 0.5 - size.height * 0.56)
    // máscara del texto → degradado
    let mask = NSImage(size: NSSize(width: s, height: s))
    mask.lockFocus(); str.draw(at: origin); mask.unlockFocus()
    NSGraphicsContext.saveGraphicsState()
    let ctx = NSGraphicsContext.current!.cgContext
    var r = CGRect(x: 0, y: 0, width: s, height: s)
    if let cg = mask.cgImage(forProposedRect: &r, context: nil, hints: nil) {
        ctx.clip(to: r, mask: cg)
        grad.draw(in: NSRect(x: 0, y: 0, width: s, height: s), angle: -90)
    }
    NSGraphicsContext.restoreGraphicsState()
    NSColor(srgbRed: 0.45, green: 0.72, blue: 1.0, alpha: 1).setFill()
    NSBezierPath(roundedRect: NSRect(x: s * 0.60, y: s * 0.25, width: s * 0.13, height: s * 0.07),
                 xRadius: s * 0.012, yRadius: s * 0.012).fill()
    img.unlockFocus()
    let rep = NSBitmapImageRep(data: img.tiffRepresentation!)!
    return rep.representation(using: .png, properties: [:])!
}

let dir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "build/AppIcon.iconset"
try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128),
                   ("128x128@2x", 256), ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
    try! render(px).write(to: URL(fileURLWithPath: "\(dir)/icon_\(name).png"))
}
