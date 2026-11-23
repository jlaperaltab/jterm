//! Un terminal: PTY + emulación (alacritty_terminal) + hilo de E/S.

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::tty::{self, Options, Shell};
use alacritty_terminal::vte::ansi::{Color, CursorShape};
use alacritty_terminal::Term;
use std::collections::HashMap;
use std::path::PathBuf;
use crate::links::{self, LinkKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Estado compartido con el hilo de E/S.
#[derive(Default)]
pub struct Shared {
    dirty: AtomicBool,
    exited: AtomicBool,
    bell: AtomicBool,
    title: Mutex<Option<String>>,
    sender: OnceLock<EventLoopSender>,
}

#[derive(Clone)]
struct Proxy(Arc<Shared>);

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        let s = &self.0;
        match event {
            Event::Wakeup => s.dirty.store(true, Ordering::Release),
            Event::Title(t) => {
                *s.title.lock().unwrap() = Some(t);
                s.dirty.store(true, Ordering::Release);
            }
            Event::ResetTitle => *s.title.lock().unwrap() = None,
            Event::Bell => s.bell.store(true, Ordering::Release),
            Event::PtyWrite(text) => {
                if let Some(tx) = s.sender.get() {
                    let _ = tx.send(Msg::Input(text.into_bytes().into()));
                }
            }
            Event::Exit | Event::ChildExit(_) => {
                s.exited.store(true, Ordering::Release);
                s.dirty.store(true, Ordering::Release);
            }
            _ => {}
        }
    }
}

struct Dims {
    cols: usize,
    lines: usize,
}

impl Dimensions for Dims {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Clone, Debug)]
pub struct SpawnOptions {
    pub program: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub scrollback: usize,
    pub cols: u16,
    pub lines: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct CellView {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
}

/// Una coincidencia de la búsqueda: línea de la rejilla (negativa = historial), columna inicial y longitud en celdas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub line: i32,
    pub col: usize,
    pub len: usize,
}

#[derive(Default)]
struct SearchState {
    query: String,
    matches: Vec<Match>,
    current: usize,
}

/// Tramo de celdas de una fila (para subrayar enlaces).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seg {
    pub row: usize,
    pub col: usize,
    pub len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkHit {
    pub kind: LinkKind,
    pub text: String,
    pub segs: Vec<Seg>,
}

/// Línea lógica: filas unidas por el ajuste automático de línea, con la posición (fila, columna) de cada carácter.
struct LogicalLine {
    chars: Vec<char>,
    pos: Vec<(usize, usize)>,
    first_row: usize,
    last_row: usize,
}

pub struct Snapshot {
    pub cols: usize,
    pub lines: usize,
    /// Fila a fila (`lines × cols`).
    pub cells: Vec<CellView>,
    pub cursor: Option<(usize, usize, CursorShape)>,
    pub selection: Option<SelectionRange>,
    pub display_offset: usize,
    /// Coincidencias visibles: (fila, columna, longitud, es la actual).
    pub matches: Vec<(usize, usize, usize, bool)>,
    /// URLs visibles (se subrayan si el ajuste lo pide).
    pub links: Vec<Seg>,
}

impl Snapshot {
    pub fn cell(&self, row: usize, col: usize) -> &CellView {
        &self.cells[row * self.cols + col]
    }

    pub fn is_selected(&self, row: usize, col: usize) -> bool {
        self.selection
            .is_some_and(|s| s.contains(Point::new(Line(row as i32 - self.display_offset as i32), Column(col))))
    }
}

pub struct Backend {
    term: Arc<FairMutex<Term<Proxy>>>,
    shared: Arc<Shared>,
    sender: EventLoopSender,
    search: Mutex<SearchState>,
    /// Filas absolutas (historial + fila de pantalla) donde se pulsó Retorno.
    prompts: Mutex<Vec<i64>>,
    underline_links: AtomicBool,
}

impl Backend {
    pub fn spawn(id: u64, o: SpawnOptions) -> std::io::Result<Backend> {
        let shared = Arc::new(Shared::default());
        let proxy = Proxy(shared.clone());
        let cfg = Config { scrolling_history: o.scrollback, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(cfg, &Dims { cols: o.cols as usize, lines: o.lines as usize }, proxy.clone())));
        let opts = Options {
            shell: o.program.map(|p| Shell::new(p, o.args)),
            working_directory: o.cwd,
            drain_on_exit: false,
            env: o.env,
        };
        let size = WindowSize { num_lines: o.lines, num_cols: o.cols, cell_width: o.cell_width, cell_height: o.cell_height };
        let pty = tty::new(&opts, size, id)?;
        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let sender = event_loop.channel();
        let _ = shared.sender.set(sender.clone());
        event_loop.spawn();
        Ok(Backend { term, shared, sender, search: Mutex::new(SearchState::default()), prompts: Mutex::new(vec![]), underline_links: AtomicBool::new(true) })
    }

    pub fn write(&self, bytes: impl Into<std::borrow::Cow<'static, [u8]>>) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    /// Texto pegado: con "bracketed paste" activo se envuelve para que el programa distinga pegar de teclear.
    pub fn paste(&self, text: &str) {
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        if self.mode().contains(TermMode::BRACKETED_PASTE) {
            let clean = text.replace('\x1b', "");
            self.write(format!("\x1b[200~{clean}\x1b[201~").into_bytes());
        } else {
            self.write(text.into_bytes());
        }
    }

    pub fn resize(&self, cols: u16, lines: u16, cell_width: u16, cell_height: u16) {
        let (cols, lines) = (cols.max(2), lines.max(1));
        {
            let mut t = self.term.lock();
            if t.columns() == cols as usize && t.screen_lines() == lines as usize {
                return;
            }
            t.resize(Dims { cols: cols as usize, lines: lines as usize });
        }
        let _ = self.sender.send(Msg::Resize(WindowSize { num_lines: lines, num_cols: cols, cell_width, cell_height }));
    }

    pub fn size(&self) -> (usize, usize) {
        let t = self.term.lock();
        (t.columns(), t.screen_lines())
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Rueda del ratón: desplaza el historial, o en pantalla alternativa envía flechas (como xterm).
    pub fn scroll_lines(&self, delta: i32) {
        let mode = self.mode();
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let key: &[u8] = match (delta > 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.write(key.repeat(delta.unsigned_abs() as usize));
        } else {
            self.term.lock().scroll_display(Scroll::Delta(delta));
            self.shared.dirty.store(true, Ordering::Release);
        }
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    fn point(&self, term: &Term<Proxy>, row: usize, col: usize) -> Point {
        let off = term.grid().display_offset() as i32;
        Point::new(Line(row as i32 - off), Column(col.min(term.columns().saturating_sub(1))))
    }

    pub fn start_selection(&self, row: usize, col: usize, side: Side, ty: SelectionType) {
        let mut t = self.term.lock();
        let p = self.point(&t, row, col);
        t.selection = Some(Selection::new(ty, p, side));
        self.shared.dirty.store(true, Ordering::Release);
    }

    pub fn update_selection(&self, row: usize, col: usize, side: Side) {
        let mut t = self.term.lock();
        let p = self.point(&t, row, col);
        if let Some(s) = t.selection.as_mut() {
            s.update(p, side);
        }
        self.shared.dirty.store(true, Ordering::Release);
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
        self.shared.dirty.store(true, Ordering::Release);
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term.lock().selection_to_string().filter(|s| !s.is_empty())
    }

    /// ¿Hubo cambios desde la última vez? (lo consulta el temporizador de la interfaz)
    pub fn take_dirty(&self) -> bool {
        self.shared.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn take_bell(&self) -> bool {
        self.shared.bell.swap(false, Ordering::AcqRel)
    }

    pub fn exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }

    pub fn title(&self) -> Option<String> {
        self.shared.title.lock().unwrap().clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        let t = self.term.lock();
        let (cols, lines) = (t.columns(), t.screen_lines());
        let content = t.renderable_content();
        let off = content.display_offset;
        let blank = CellView { ch: ' ', fg: Color::Named(alacritty_terminal::vte::ansi::NamedColor::Foreground), bg: Color::Named(alacritty_terminal::vte::ansi::NamedColor::Background), flags: Flags::empty() };
        let mut cells = vec![blank; cols * lines];
        for c in content.display_iter {
            let row = c.point.line.0 + off as i32;
            if row < 0 || row as usize >= lines || c.point.column.0 >= cols {
                continue;
            }
            cells[row as usize * cols + c.point.column.0] = CellView { ch: c.cell.c, fg: c.cell.fg, bg: c.cell.bg, flags: c.cell.flags };
        }
        let cur = content.cursor;
        let crow = cur.point.line.0 + off as i32;
        let cursor = (content.mode.contains(TermMode::SHOW_CURSOR) && cur.shape != CursorShape::Hidden && crow >= 0 && (crow as usize) < lines)
            .then(|| (crow as usize, cur.point.column.0, cur.shape));
        let matches = {
            let st = self.search.lock().unwrap();
            st.matches
                .iter()
                .enumerate()
                .filter_map(|(i, m)| {
                    let row = m.line + off as i32;
                    (row >= 0 && (row as usize) < lines).then(|| (row as usize, m.col, m.len, i == st.current))
                })
                .collect()
        };
        let links = if self.underline_links.load(Ordering::Relaxed) {
            let mut v = Vec::new();
            for l in Self::logical_lines(&t) {
                for f in links::find(&l.chars.iter().collect::<String>(), false) {
                    v.extend(Self::segs(&l, f.start, f.end));
                }
            }
            v
        } else {
            vec![]
        };
        Snapshot { cols, lines, cells, cursor, selection: content.selection, display_offset: off, matches, links }
    }

    pub fn set_underline_links(&self, on: bool) {
        self.underline_links.store(on, Ordering::Relaxed);
    }

    // MARK: Enlaces

    fn logical_lines(t: &Term<Proxy>) -> Vec<LogicalLine> {
        let (cols, lines) = (t.columns(), t.screen_lines());
        let off = t.grid().display_offset() as i32;
        let mut out: Vec<LogicalLine> = Vec::new();
        let mut continuing = false;
        for r in 0..lines {
            let row = &t.grid()[Line(r as i32 - off)];
            if !continuing {
                out.push(LogicalLine { chars: vec![], pos: vec![], first_row: r, last_row: r });
            }
            let cur = out.last_mut().unwrap();
            cur.last_row = r;
            for c in 0..cols {
                let cell = &row[Column(c)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                cur.chars.push(if cell.c == '\0' { ' ' } else { cell.c });
                cur.pos.push((r, c));
            }
            continuing = cols > 0 && row[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
        }
        out
    }

    /// Tramos por fila de los caracteres `start..end` de una línea lógica.
    fn segs(l: &LogicalLine, start: usize, end: usize) -> Vec<Seg> {
        let mut out: Vec<Seg> = Vec::new();
        for &(row, col) in &l.pos[start..end.min(l.pos.len())] {
            match out.last_mut() {
                Some(s) if s.row == row => s.len = col + 1 - s.col,
                _ => out.push(Seg { row, col, len: 1 }),
            }
        }
        out
    }

    /// Enlace bajo la celda (fila, columna) de la vista. Las rutas solo cuentan si existen.
    pub fn link_at(&self, row: usize, col: usize, paths: bool) -> Option<LinkHit> {
        let t = self.term.lock();
        let l = Self::logical_lines(&t).into_iter().find(|l| (l.first_row..=l.last_row).contains(&row))?;
        let idx = l.pos.iter().rposition(|&(r, c)| r == row && c <= col)?;
        let text: String = l.chars.iter().collect();
        links::find(&text, paths)
            .into_iter()
            .filter(|f| f.kind == LinkKind::Url || links::resolve_path(&f.text).is_some_and(|p| p.exists()))
            .find(|f| f.start <= idx && idx < f.end)
            .map(|f| LinkHit { kind: f.kind, segs: Self::segs(&l, f.start, f.end), text: f.text })
    }

    // MARK: Saltos entre prompts

    /// Anota la fila del cursor como un prompt (se llama al pulsar Retorno).
    pub fn mark_prompt(&self) {
        let t = self.term.lock();
        let abs = t.history_size() as i64 + t.grid().cursor.point.line.0 as i64;
        let mut p = self.prompts.lock().unwrap();
        if p.last() != Some(&abs) {
            p.push(abs);
        }
    }

    /// Fila absoluta de la primera fila visible.
    pub fn top_abs(&self) -> i64 {
        let t = self.term.lock();
        t.history_size() as i64 - t.grid().display_offset() as i64
    }

    /// -1 = prompt anterior, +1 = siguiente (o el final si no hay más). `false` si no hay a dónde ir.
    pub fn jump_prompt(&self, dir: i32) -> bool {
        let mut t = self.term.lock();
        let hist = t.history_size() as i64;
        let off = t.grid().display_offset() as i64;
        let top = hist - off;
        let p = self.prompts.lock().unwrap();
        let target = if dir < 0 { p.iter().rev().find(|&&r| r < top && r >= 0).copied() } else { p.iter().find(|&&r| r > top).copied() };
        let want_off = match target {
            Some(r) => hist - r,
            None if dir > 0 && off > 0 => 0,
            None => return false,
        };
        let want_off = want_off.max(0);
        if want_off == off {
            return false; // ya está ahí
        }
        t.scroll_display(Scroll::Delta((want_off - off) as i32));
        drop(p);
        self.shared.dirty.store(true, Ordering::Release);
        true
    }

    /// Busca `query` (sin distinguir mayúsculas) en el historial y la pantalla. La actual queda en la coincidencia
    /// más cercana por debajo de lo visible. Devuelve (actual, total), con actual desde 1.
    pub fn set_search(&self, query: &str) -> (usize, usize) {
        let matches = if query.is_empty() { vec![] } else { self.find_all(query) };
        let mut st = self.search.lock().unwrap();
        st.query = query.to_string();
        st.current = matches.len().saturating_sub(1);
        st.matches = matches;
        let info = (if st.matches.is_empty() { 0 } else { st.current + 1 }, st.matches.len());
        let target = st.matches.get(st.current).copied();
        drop(st);
        if let Some(m) = target {
            self.reveal(m);
        }
        self.shared.dirty.store(true, Ordering::Release);
        info
    }

    /// Siguiente (`forward`) o anterior coincidencia, dando la vuelta al llegar al final.
    pub fn search_step(&self, forward: bool) -> (usize, usize) {
        let mut st = self.search.lock().unwrap();
        let n = st.matches.len();
        if n == 0 {
            return (0, 0);
        }
        st.current = if forward { (st.current + 1) % n } else { (st.current + n - 1) % n };
        let (cur, m) = (st.current, st.matches[st.current]);
        drop(st);
        self.reveal(m);
        self.shared.dirty.store(true, Ordering::Release);
        (cur + 1, n)
    }

    pub fn clear_search(&self) {
        *self.search.lock().unwrap() = SearchState::default();
        self.shared.dirty.store(true, Ordering::Release);
    }

    fn find_all(&self, query: &str) -> Vec<Match> {
        let needle: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
        let t = self.term.lock();
        let (top, bottom) = (t.topmost_line().0, t.bottommost_line().0);
        let cols = t.columns();
        let mut out = Vec::new();
        for line in top..=bottom {
            let row = &t.grid()[Line(line)];
            // Caracteres de la línea con la columna de cada uno (los espaciadores de caracteres anchos no cuentan).
            let mut chars: Vec<(char, usize)> = Vec::with_capacity(cols);
            for c in 0..cols {
                let cell = &row[Column(c)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                chars.push((cell.c.to_lowercase().next().unwrap_or(cell.c), c));
            }
            if chars.len() < needle.len() {
                continue;
            }
            let mut i = 0;
            while i + needle.len() <= chars.len() {
                if chars[i..i + needle.len()].iter().map(|(c, _)| *c).eq(needle.iter().copied()) {
                    let start = chars[i].1;
                    let end = chars[i + needle.len() - 1].1 + 1;
                    out.push(Match { line, col: start, len: end - start });
                    i += needle.len();
                } else {
                    i += 1;
                }
            }
        }
        out
    }

    /// Desplaza la vista para que la coincidencia quede visible (hacia la mitad de la pantalla).
    fn reveal(&self, m: Match) {
        let mut t = self.term.lock();
        let off = t.grid().display_offset() as i32;
        let lines = t.screen_lines() as i32;
        let row = m.line + off;
        if (0..lines).contains(&row) {
            return;
        }
        let want = if m.line < 0 { -m.line + lines / 2 } else { 0 };
        t.scroll_display(Scroll::Delta(want - off));
    }

    /// Texto de la línea donde está el cursor (para revisar lo que se va a ejecutar al pulsar Retorno).
    pub fn cursor_line_text(&self) -> String {
        let s = self.snapshot();
        match s.cursor {
            Some((row, _, _)) => (0..s.cols).map(|c| s.cell(row, c).ch).collect::<String>().trim_end().to_string(),
            None => String::new(),
        }
    }

    /// ¿Pantalla alternativa (vim, less…)?
    pub fn alt_screen(&self) -> bool {
        self.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Texto visible (para pruebas).
    pub fn visible_text(&self) -> String {
        let s = self.snapshot();
        (0..s.lines)
            .map(|r| (0..s.cols).map(|c| s.cell(r, c).ch).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn opts(program: &str, args: &[&str]) -> SpawnOptions {
        SpawnOptions {
            program: Some(program.into()),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: None,
            env: HashMap::from([("TERM".into(), "xterm-256color".into())]),
            scrollback: 1000,
            cols: 40,
            lines: 10,
            cell_width: 8,
            cell_height: 16,
        }
    }

    fn wait_for(b: &Backend, needle: &str) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if b.visible_text().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Ejecuta `printf` local (sin red, sin SSH) y comprueba que la emulación pinta la salida.
    #[test]
    fn local_command_output_reaches_the_grid() {
        let b = Backend::spawn(1, opts("/bin/sh", &["-c", "printf 'hola\\033[31m rojo\\033[0m\\nsegunda'; sleep 1"])).unwrap();
        assert!(wait_for(&b, "segunda"), "{}", b.visible_text());
        let text = b.visible_text();
        assert!(text.starts_with("hola rojo\nsegunda"), "{text}");
        let s = b.snapshot();
        // Los 4 primeros caracteres tienen el color por defecto; " rojo" usa el rojo ANSI.
        assert!(matches!(s.cell(0, 6).fg, Color::Named(alacritty_terminal::vte::ansi::NamedColor::Red)));
    }

    #[test]
    fn input_is_echoed_by_the_shell_and_resize_works() {
        let b = Backend::spawn(2, opts("/bin/cat", &[])).unwrap();
        b.write(&b"eco\n"[..]);
        assert!(wait_for(&b, "eco"));
        b.resize(60, 20, 8, 16);
        assert_eq!(b.size(), (60, 20));
        b.paste("pegado\n");
        assert!(wait_for(&b, "pegado"));
        b.write(&b"rm -rf x"[..]);
        let end = Instant::now() + Duration::from_secs(5);
        while !b.cursor_line_text().contains("rm -rf x") && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(b.cursor_line_text().contains("rm -rf x"));
        assert!(!b.alt_screen());
    }

    #[test]
    fn search_finds_matches_in_history_and_screen() {
        // 30 líneas en una pantalla de 10: las primeras pasan al historial.
        let b = Backend::spawn(4, opts("/bin/sh", &["-c", "i=1; while [ $i -le 30 ]; do echo \"Linea $i Hola\"; i=$((i+1)); done; sleep 1"])).unwrap();
        assert!(wait_for(&b, "Linea 30"));
        assert_eq!(b.set_search("hola").1, 30);
        assert_eq!(b.set_search("linea 7 ").1, 1);
        assert_eq!(b.set_search("no existe").1, 0);
        let (cur, total) = b.set_search("linea");
        assert_eq!((cur, total), (30, 30));
        // La coincidencia actual (la última) está en pantalla.
        assert!(b.snapshot().matches.iter().any(|m| m.3));
        let (cur, _) = b.search_step(false);
        assert_eq!(cur, 29);
        let (cur, _) = b.search_step(true);
        assert_eq!(cur, 30);
        // Dar la vuelta: de la última a la primera, que está en el historial y debe volverse visible.
        let (cur, _) = b.search_step(true);
        assert_eq!(cur, 1);
        assert!(b.snapshot().display_offset > 0);
        assert!(b.snapshot().matches.iter().any(|m| m.3));
        b.clear_search();
        assert!(b.snapshot().matches.is_empty());
    }

    #[test]
    fn links_are_found_across_wrapped_lines_and_prompts_jump() {
        let url = "https://ejemplo.com/una/ruta/bastante/larga/para/que/se/parta";
        let b = Backend::spawn(5, opts("/bin/sh", &["-c", &format!("echo 'ver {url} fin'; sleep 2")])).unwrap();
        assert!(wait_for(&b, "fin"));
        // La terminal mide 40 columnas: la URL (61 caracteres) ocupa dos filas.
        let hit = b.link_at(0, 10, false).expect("URL bajo el cursor");
        assert_eq!((hit.kind, hit.text.as_str()), (LinkKind::Url, url));
        assert_eq!(hit.segs.len(), 2);
        assert!(b.link_at(1, 20, false).is_some());
        assert!(b.link_at(1, 28, false).is_none(), "tras la URL ya no hay enlace");
        assert!(b.link_at(0, 0, false).is_none());
        assert!(!b.snapshot().links.is_empty());
        b.set_underline_links(false);
        assert!(b.snapshot().links.is_empty());

        let c = Backend::spawn(6, opts("/bin/cat", &[])).unwrap();
        c.write(&b"uno\n"[..]);
        assert!(wait_for(&c, "uno\nuno"));
        c.mark_prompt();
        let first = c.top_abs() + c.snapshot().cursor.unwrap().0 as i64;
        for i in 0..40 {
            c.write(format!("l{i}\n").into_bytes());
        }
        assert!(wait_for(&c, "l39"));
        std::thread::sleep(Duration::from_millis(400)); // deja que cat termine de devolver todo el eco
        c.mark_prompt();
        assert!(c.snapshot().display_offset == 0);
        assert!(c.jump_prompt(-1));
        assert_eq!(c.top_abs(), first, "salta al prompt anterior a lo visible");
        assert!(c.snapshot().display_offset > 0);
        assert!(!c.jump_prompt(-1), "no hay prompts antes del primero");
        assert!(c.jump_prompt(1), "el siguiente prompt está abajo: se vuelve al final");
        assert_eq!(c.snapshot().display_offset, 0);
        assert!(!c.jump_prompt(1));
    }

    #[test]
    fn exit_is_reported_and_selection_copies_text() {
        let b = Backend::spawn(3, opts("/bin/sh", &["-c", "printf 'abcdef'"])).unwrap();
        assert!(wait_for(&b, "abcdef"));
        b.start_selection(0, 1, Side::Left, SelectionType::Simple);
        b.update_selection(0, 3, Side::Right);
        assert_eq!(b.selection_text().as_deref(), Some("bcd"));
        assert!(b.snapshot().is_selected(0, 2));
        b.clear_selection();
        assert!(b.selection_text().is_none());
        let end = Instant::now() + Duration::from_secs(5);
        while !b.exited() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(b.exited());
    }
}
