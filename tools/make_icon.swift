// Genera Resources/AppIcon.icns (icono v2).
//   swift tools/make_icon.swift build/AppIcon.iconset && iconutil -c icns build/AppIcon.iconset -o Resources/AppIcon.icns
// El icono anterior sigue disponible en tools/make_icon_v1.swift.
import AppKit

func srgb(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> CGColor {
    CGColor(colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!, components: [r, g, b, a])!
}

func render(_ px: Int) -> Data {
    let s = CGFloat(px)
    let img = NSImage(size: NSSize(width: s, height: s))
    img.lockFocus()
    let ctx = NSGraphicsContext.current!.cgContext
    let space = CGColorSpace(name: CGColorSpace.sRGB)!

    // Cuerpo del icono: rectángulo de esquinas continuas ("squircle") como los de macOS.
    let body = CGRect(x: s * 0.0977, y: s * 0.0977, width: s * 0.8047, height: s * 0.8047)
    let radius = body.width * 0.2237
    let bodyPath = CGPath(roundedRect: body, cornerWidth: radius, cornerHeight: radius, transform: nil)

    // Sombra
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -s * 0.012), blur: s * 0.03, color: srgb(0, 0, 0, 0.5))
    ctx.addPath(bodyPath); ctx.setFillColor(srgb(0.05, 0.06, 0.12)); ctx.fillPath()
    ctx.restoreGState()

    // Fondo: azul noche en diagonal
    ctx.saveGState()
    ctx.addPath(bodyPath); ctx.clip()
    let bg = CGGradient(colorsSpace: space, colors: [srgb(0.13, 0.17, 0.36), srgb(0.05, 0.06, 0.15)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(bg, start: CGPoint(x: body.minX, y: body.maxY), end: CGPoint(x: body.maxX, y: body.minY), options: [])
    // Resplandor suave detrás del símbolo
    let glow = CGGradient(colorsSpace: space, colors: [srgb(0.36, 0.55, 1.0, 0.34), srgb(0.36, 0.55, 1.0, 0)] as CFArray, locations: [0, 1])!
    ctx.drawRadialGradient(glow, startCenter: CGPoint(x: s * 0.42, y: s * 0.56), startRadius: 0,
                           endCenter: CGPoint(x: s * 0.42, y: s * 0.56), endRadius: s * 0.42, options: [])
    ctx.restoreGState()

    // La "j" de jTerm: glifo monoespaciado grueso, convertido en trazado para poder rellenarlo con el degradado.
    let jFont = CTFontCreateWithName("Menlo-Bold" as CFString, s * 0.46, nil)
    var jChar: UniChar = 0x6A
    var jGlyph: CGGlyph = 0
    CTFontGetGlyphsForCharacters(jFont, &jChar, &jGlyph, 1)
    var jMove = CGAffineTransform(translationX: s * 0.468, y: s * 0.372)
    let jPath = CTFontCreatePathForGlyph(jFont, jGlyph, &jMove)!

    // Símbolo: ">", "j" y cursor "_", rellenos con un degradado cian → azul → violeta.
    let sym = CGGradient(colorsSpace: space,
                         colors: [srgb(0.38, 0.92, 1.0), srgb(0.36, 0.62, 1.0), srgb(0.66, 0.54, 1.0)] as CFArray,
                         locations: [0, 0.55, 1])!
    func fillWithSymbolGradient(_ build: () -> Void) {
        ctx.saveGState()
        build(); ctx.clip()
        ctx.drawLinearGradient(sym, start: CGPoint(x: s * 0.28, y: s * 0.72), end: CGPoint(x: s * 0.74, y: s * 0.28), options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
        ctx.restoreGState()
    }
    // Sombra del símbolo
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -s * 0.01), blur: s * 0.025, color: srgb(0.2, 0.35, 1.0, 0.55))
    let chevron = CGMutablePath()
    chevron.move(to: CGPoint(x: s * 0.262, y: s * 0.640))
    chevron.addLine(to: CGPoint(x: s * 0.410, y: s * 0.520))
    chevron.addLine(to: CGPoint(x: s * 0.262, y: s * 0.400))
    ctx.addPath(chevron); ctx.setLineWidth(s * 0.072); ctx.setLineCap(.round); ctx.setLineJoin(.round)
    ctx.setStrokeColor(srgb(0.4, 0.7, 1.0)); ctx.strokePath()
    ctx.restoreGState()
    fillWithSymbolGradient {
        ctx.addPath(chevron); ctx.setLineWidth(s * 0.072); ctx.setLineCap(.round); ctx.setLineJoin(.round)
        ctx.replacePathWithStrokedPath()
    }
    // Sombra de la "j" (mismo brillo que la del chevron)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -s * 0.01), blur: s * 0.025, color: srgb(0.2, 0.35, 1.0, 0.55))
    ctx.addPath(jPath); ctx.setFillColor(srgb(0.45, 0.65, 1.0)); ctx.fillPath()
    ctx.restoreGState()
    fillWithSymbolGradient { ctx.addPath(jPath) }
    let cursor = CGPath(roundedRect: CGRect(x: s * 0.672, y: s * 0.352, width: s * 0.125, height: s * 0.056),
                        cornerWidth: s * 0.028, cornerHeight: s * 0.028, transform: nil)
    fillWithSymbolGradient { ctx.addPath(cursor) }

    // Brillo superior (efecto cristal) y borde fino
    ctx.saveGState()
    ctx.addPath(bodyPath); ctx.clip()
    let gloss = CGGradient(colorsSpace: space, colors: [srgb(1, 1, 1, 0.16), srgb(1, 1, 1, 0)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(gloss, start: CGPoint(x: s * 0.5, y: body.maxY), end: CGPoint(x: s * 0.5, y: body.midY + body.height * 0.1), options: [])
    ctx.restoreGState()
    ctx.addPath(bodyPath); ctx.setLineWidth(s * 0.004); ctx.setStrokeColor(srgb(1, 1, 1, 0.16)); ctx.strokePath()

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
