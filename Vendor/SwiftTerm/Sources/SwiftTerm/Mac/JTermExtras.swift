//
//  JTermExtras.swift — añadido por jTerm (ver JTERM_PATCH.md)
//
#if os(macOS)
import AppKit

extension TerminalView {
    /// Fila absoluta del cursor, contada desde el inicio del historial (no cambia al desplazarse).
    public func jtermCursorAbsoluteRow() -> Int {
        let b = terminal.displayBuffer
        return b.linesTop + b.yBase + b.y
    }
    /// Fila absoluta de la primera fila visible.
    public func jtermTopAbsoluteRow() -> Int {
        let b = terminal.displayBuffer
        return b.linesTop + b.yDisp
    }
    /// Primera fila absoluta que aún existe en el historial (las anteriores ya se descartaron).
    public func jtermOldestAbsoluteRow() -> Int { terminal.displayBuffer.linesTop }
    /// Desplaza la vista para que `row` (absoluta) quede en la primera fila visible.
    public func jtermScroll(toAbsoluteRow row: Int) {
        let b = terminal.displayBuffer
        scrollTo(row: max(0, min(b.yBase, row - b.linesTop)))
    }
    /// Tamaño de una celda del terminal en puntos (para dibujar encima, p. ej. subrayar enlaces).
    public var jtermCellSize: CGSize { cellDimension.map { CGSize(width: $0.width, height: $0.height) } ?? .zero }
    /// true si la vista está desplazada hacia arriba (no sigue la salida nueva).
    public var jtermIsScrolledUp: Bool { let b = terminal.displayBuffer; return b.yDisp < b.yBase }
    /// Baja hasta la última línea.
    public func jtermScrollToBottom() { scrollTo(row: terminal.displayBuffer.yBase) }
}
#endif
