//! Análisis de eventos de CloudWatch: nivel, JSON estructurado, líneas de Lambda y agrupación por invocación.

use crate::aws::LogEvent;
use regex::{Regex, RegexBuilder};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

// MARK: JSON ordenado (serde_json con `preserve_order`)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Key,
    Str,
    Number,
    Literal,
    Punct,
}

pub fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t")
}

/// JSON con sangría como lista de fragmentos (para pintarlo con colores). `budget` limita el texto total.
pub fn pretty(n: &Value, indent: usize, budget: &mut i64) -> Vec<(String, Token)> {
    if *budget <= 0 {
        return vec![("…".into(), Token::Punct)];
    }
    let sp = "  ".repeat(indent);
    let sp2 = "  ".repeat(indent + 1);
    match n {
        Value::String(s) => {
            *budget -= s.chars().count() as i64;
            vec![(format!("\"{}\"", escape(s)), Token::Str)]
        }
        Value::Number(x) => vec![(x.to_string(), Token::Number)],
        Value::Bool(b) => vec![(b.to_string(), Token::Literal)],
        Value::Null => vec![("null".into(), Token::Literal)],
        Value::Array(a) if a.is_empty() => vec![("[]".into(), Token::Punct)],
        Value::Array(a) => {
            let mut out = vec![("[\n".into(), Token::Punct)];
            for (i, x) in a.iter().enumerate() {
                out.push((sp2.clone(), Token::Punct));
                out.extend(pretty(x, indent + 1, budget));
                out.push((if i < a.len() - 1 { ",\n" } else { "\n" }.into(), Token::Punct));
            }
            out.push((format!("{sp}]"), Token::Punct));
            out
        }
        Value::Object(o) if o.is_empty() => vec![("{}".into(), Token::Punct)],
        Value::Object(o) => {
            let mut out = vec![("{\n".into(), Token::Punct)];
            for (i, (k, v)) in o.iter().enumerate() {
                out.push((sp2.clone(), Token::Punct));
                out.push((format!("\"{}\"", escape(k)), Token::Key));
                out.push((": ".into(), Token::Punct));
                out.extend(pretty(v, indent + 1, budget));
                out.push((if i < o.len() - 1 { ",\n" } else { "\n" }.into(), Token::Punct));
            }
            out.push((format!("{sp}}}"), Token::Punct));
            out
        }
    }
}

// MARK: Modelo

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Platform,
}

impl LogLevel {
    pub fn label(self) -> &'static str {
        ["ERROR", "WARN", "INFO", "DEBUG", "LAMBDA"][self as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Start,
    End,
    Report,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LambdaReport {
    pub duration_ms: Option<f64>,
    pub billed_ms: Option<i64>,
    pub memory_size_mb: Option<i64>,
    pub max_memory_mb: Option<i64>,
    pub init_ms: Option<f64>,
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedLog {
    pub event: LogEvent,
    pub level: LogLevel,
    pub lines: Vec<String>,
    pub json: Option<Value>,
    pub summary: String,
    pub chips: Vec<(String, String)>,
    pub request_id: Option<String>,
    pub platform: Option<Platform>,
    pub report: Option<LambdaReport>,
    pub is_traceback: bool,
}

impl ParsedLog {
    /// Líneas posteriores a la primera (tracebacks, mensajes largos).
    pub fn extra_lines(&self) -> &[String] {
        self.lines.get(1..).unwrap_or(&[])
    }
}

// MARK: Análisis

const LEVEL_KEYS: [&str; 6] = ["level", "severity", "loglevel", "levelname", "lvl", "log_level"];
const MSG_KEYS: [&str; 5] = ["msg", "message", "event", "log", "text"];
const TIME_KEYS: [&str; 7] = ["time", "timestamp", "ts", "@timestamp", "t", "datetime", "date"];
const RID_KEYS: [&str; 4] = ["requestid", "aws_request_id", "awsrequestid", "request_id"];
const UUID: &str = "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";

fn re(cell: &'static OnceLock<Regex>, pat: &str, ci: bool) -> &'static Regex {
    cell.get_or_init(|| RegexBuilder::new(pat).case_insensitive(ci).build().unwrap())
}

pub fn level_from_word(w: &str) -> Option<LogLevel> {
    match w.to_lowercase().as_str() {
        "error" | "err" | "fatal" | "critical" | "severe" | "panic" | "emerg" | "alert" | "crit" => Some(LogLevel::Error),
        "warn" | "warning" => Some(LogLevel::Warn),
        "info" | "notice" | "information" => Some(LogLevel::Info),
        "debug" | "trace" | "verbose" => Some(LogLevel::Debug),
        _ => None,
    }
}

fn level_from_json(o: &serde_json::Map<String, Value>) -> Option<LogLevel> {
    for (k, v) in o {
        if !LEVEL_KEYS.contains(&k.to_lowercase().as_str()) {
            continue;
        }
        match v {
            Value::String(s) => {
                if let Some(l) = level_from_word(s) {
                    return Some(l);
                }
            }
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    if i >= 50 { return Some(LogLevel::Error); }
                    if i == 40 { return Some(LogLevel::Warn); }
                    if i == 30 { return Some(LogLevel::Info); }
                    if i <= 20 { return Some(LogLevel::Debug); }
                }
            }
            _ => {}
        }
    }
    None
}

/// Nivel a partir del texto, con reglas conservadoras (p. ej. "errors=0" no es un error).
pub fn detect_level(lines: &[String]) -> LogLevel {
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    static C: OnceLock<Regex> = OnceLock::new();
    static D: OnceLock<Regex> = OnceLock::new();
    static E: OnceLock<Regex> = OnceLock::new();
    static F: OnceLock<Regex> = OnceLock::new();
    static G: OnceLock<Regex> = OnceLock::new();
    static H: OnceLock<Regex> = OnceLock::new();
    static I: OnceLock<Regex> = OnceLock::new();
    let all = lines.join("\n");
    if all.contains("Task timed out after") || all.contains("Traceback (most recent call last)") {
        return LogLevel::Error;
    }
    let first = lines.first().map(String::as_str).unwrap_or("");
    if re(&A, r"\b(FATAL|CRITICAL|SEVERE|ERROR|EXCEPTION|PANIC)\b", false).is_match(first)
        || re(&B, r#"^\s*\[?(error|fatal|critical|severe)\]?(\s|:|$)"#, true).is_match(first)
        || re(&C, r#"\blevel[=:]\s*"?(error|fatal)"#, true).is_match(first)
    {
        return LogLevel::Error;
    }
    let exc = re(&D, r"^\s*[A-Za-z_][\w.]*(Error|Exception)(:|$)", false);
    if lines.iter().any(|l| exc.is_match(l)) {
        return LogLevel::Error;
    }
    if re(&E, r"\b(WARN|WARNING)\b", false).is_match(first)
        || re(&F, r"^\s*\[?(warn|warning)\]?(\s|:|$)", true).is_match(first)
        || re(&G, r#"\blevel[=:]\s*"?warn"#, true).is_match(first)
    {
        return LogLevel::Warn;
    }
    if re(&H, r"\b(DEBUG|TRACE)\b", false).is_match(first) || re(&I, r"^\s*\[?(debug|trace)\]?(\s|:|$)", true).is_match(first) {
        return LogLevel::Debug;
    }
    LogLevel::Info
}

/// Quita la palabra de nivel inicial ("ERROR ", "[WARNING] ") cuando coincide con el nivel detectado.
pub fn strip_level_prefix(s: &str, level: LogLevel) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = re(&R, r"^\s*[\[\(]?(FATAL|CRITICAL|SEVERE|ERROR|WARNING|WARN|INFO|DEBUG|TRACE)[\]\)]?\s*[:\-–]?\s+(.+)$", true);
    match r.captures(s) {
        Some(c) if level_from_word(&c[1]) == Some(level) && !c[2].trim().is_empty() => c[2].to_string(),
        _ => s.to_string(),
    }
}

/// Primera captura de `pat` en `s` cuyo texto previo no termina en ninguno de `not_after`
/// (sustituye al lookbehind negativo, que `regex` no soporta).
fn capture_not_after(pat: &str, s: &str, not_after: &[&str]) -> Option<String> {
    let r = Regex::new(pat).unwrap();
    let found = r.captures_iter(s).find_map(|c| {
        let m = c.get(0)?;
        let before = &s[..m.start()];
        (!not_after.iter().any(|p| before.ends_with(p))).then(|| c[1].to_string())
    });
    found
}

fn cap(pat: &str, s: &str) -> Option<String> {
    Regex::new(pat).unwrap().captures(s).map(|c| c[1].to_string())
}

pub fn parse_report(s: &str) -> LambdaReport {
    let f = |p: &str| cap(p, s).and_then(|x| x.parse::<f64>().ok());
    let i = |p: &str| cap(p, s).and_then(|x| x.parse::<i64>().ok());
    LambdaReport {
        duration_ms: capture_not_after(r"Duration:\s*([\d.]+)\s*ms", s, &["Billed ", "Init "]).and_then(|x| x.parse().ok()),
        billed_ms: i(r"Billed Duration:\s*(\d+)\s*ms"),
        memory_size_mb: i(r"Memory Size:\s*(\d+)\s*MB"),
        max_memory_mb: i(r"Max Memory Used:\s*(\d+)\s*MB"),
        init_ms: f(r"Init Duration:\s*([\d.]+)\s*ms"),
        status: cap(r"Status:\s*(\w+)", s),
    }
}

fn is_uuid(s: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, &format!("^{UUID}$"), false).is_match(s)
}

pub fn parse(e: &LogEvent) -> ParsedLog {
    let raw = e.message.trim_end_matches(['\n', '\r', ' ', '\t']).to_string();
    let lines: Vec<String> = raw.replace("\r\n", "\n").split('\n').map(String::from).collect();
    let first = lines[0].clone();
    let mut p = ParsedLog {
        event: e.clone(), level: LogLevel::Info, lines, json: None, summary: String::new(), chips: vec![],
        request_id: None, platform: None, report: None, is_traceback: false,
    };

    // Líneas de la plataforma Lambda
    for (prefix, kind) in [("START RequestId:", Platform::Start), ("END RequestId:", Platform::End), ("REPORT RequestId:", Platform::Report)] {
        if first.starts_with(prefix) {
            p.platform = Some(kind);
            p.level = LogLevel::Platform;
            p.summary = first.clone();
            p.request_id = cap(r"RequestId:\s*(\S+)", &first);
            if kind == Platform::Report {
                let r = parse_report(&raw);
                if r.status.as_deref().is_some_and(|s| s.to_lowercase() != "success") {
                    p.level = LogLevel::Error;
                }
                p.report = Some(r);
            }
            return p;
        }
    }
    for prefix in ["INIT_START", "INIT_REPORT", "RESTORE_START", "RESTORE_REPORT", "EXTENSION", "TELEMETRY"] {
        if first.starts_with(prefix) {
            p.platform = Some(Platform::Other);
            p.level = LogLevel::Platform;
            p.summary = first;
            return p;
        }
    }

    // JSON estructurado
    if raw.starts_with('{') {
        if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(&raw) {
            p.level = level_from_json(&o).unwrap_or_else(|| detect_level(&[first.clone()]));
            let msg = o
                .iter()
                .find(|(k, v)| MSG_KEYS.contains(&k.to_lowercase().as_str()) && v.is_string())
                .and_then(|(_, v)| v.as_str());
            for (k, v) in &o {
                if RID_KEYS.contains(&k.to_lowercase().as_str()) {
                    if let Some(s) = v.as_str() {
                        p.request_id = Some(s.into());
                    }
                }
            }
            for (k, v) in &o {
                let lk = k.to_lowercase();
                let skip = LEVEL_KEYS.contains(&lk.as_str())
                    || TIME_KEYS.contains(&lk.as_str())
                    || MSG_KEYS.contains(&lk.as_str())
                    || RID_KEYS.contains(&lk.as_str());
                if skip || p.chips.len() >= 3 {
                    continue;
                }
                match v {
                    Value::String(s) if s.chars().count() <= 24 && !s.contains('\n') => p.chips.push((k.clone(), s.clone())),
                    Value::Number(n) => p.chips.push((k.clone(), n.to_string())),
                    Value::Bool(b) => p.chips.push((k.clone(), b.to_string())),
                    _ => {}
                }
            }
            p.summary = msg.map(String::from).unwrap_or_else(|| raw.chars().take(240).collect());
            p.json = Some(Value::Object(o));
            return p;
        }
    }

    // Runtimes de Lambda: "<fecha>\t<uuid>\tNIVEL\tmensaje" (Node) y "[NIVEL]\t<fecha>\t<uuid>\tmensaje" (Python)
    let mut body = first.clone();
    let mut word_level = None;
    let parts: Vec<&str> = first.split('\t').collect();
    static BR: OnceLock<Regex> = OnceLock::new();
    if parts.len() >= 4 && is_uuid(parts[1]) && level_from_word(parts[2]).is_some() {
        p.request_id = Some(parts[1].into());
        word_level = level_from_word(parts[2]);
        body = parts[3..].join("\t");
    } else if parts.len() >= 4 && re(&BR, r"^\[(\w+)\]$", false).is_match(parts[0]) && is_uuid(parts[2]) {
        p.request_id = Some(parts[2].into());
        word_level = level_from_word(&parts[0][1..parts[0].len() - 1]);
        body = parts[3..].join("\t");
    }
    p.level = word_level.unwrap_or_else(|| detect_level(&p.lines));
    p.summary = strip_level_prefix(&body, p.level);
    static EX: OnceLock<Regex> = OnceLock::new();
    let ex = re(&EX, r"^\s*[A-Za-z_][\w.]*(Error|Exception)(:|$)", false);
    p.is_traceback = p.lines.iter().any(|l| l.starts_with("Traceback (most recent call last)"))
        || (p.lines.len() > 2 && p.lines.last().is_some_and(|l| ex.is_match(l)));
    p
}

// MARK: Invocaciones y filas

#[derive(Default)]
pub struct RidTracker {
    current: HashMap<String, String>,
}

impl RidTracker {
    /// Asigna a cada evento la invocación a la que pertenece: la propia, o la que está en curso en su stream.
    pub fn assign(&mut self, p: &mut ParsedLog) {
        let stream = p.event.stream.clone();
        if p.platform == Some(Platform::Start) {
            if let Some(rid) = &p.request_id {
                self.current.insert(stream.clone(), rid.clone());
            }
        }
        if p.request_id.is_none() {
            p.request_id = self.current.get(&stream).cloned();
        }
        if p.platform == Some(Platform::End) {
            self.current.remove(&stream);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GroupInfo {
    pub rid: String,
    pub count: usize,
    pub has_error: bool,
    pub report: Option<LambdaReport>,
    pub started: bool,
    pub ended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Group(String),
    Event(usize),
}

pub fn infos(parsed: &[ParsedLog]) -> HashMap<String, GroupInfo> {
    let mut out: HashMap<String, GroupInfo> = HashMap::new();
    for p in parsed {
        let Some(rid) = &p.request_id else { continue };
        let g = out.entry(rid.clone()).or_insert_with(|| GroupInfo { rid: rid.clone(), ..Default::default() });
        if matches!(p.platform, None | Some(Platform::Other)) { g.count += 1; }
        if p.level == LogLevel::Error { g.has_error = true; }
        if p.platform == Some(Platform::Start) { g.started = true; }
        if p.platform == Some(Platform::End) { g.ended = true; }
        if p.platform == Some(Platform::Report) { g.report = p.report.clone(); g.ended = true; }
    }
    out
}

fn is_marker(p: &ParsedLog) -> bool {
    matches!(p.platform, Some(Platform::Start | Platform::End | Platform::Report))
}

/// Orden y contenido de las filas visibles. Con `group`, los eventos de una invocación se reúnen bajo un
/// encabezado y las líneas START/END/REPORT se resumen en él.
pub fn rows(parsed: &[ParsedLog], group: bool, filter: Option<LogLevel>, collapsed: &HashSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    let mut emitted: HashSet<String> = HashSet::new();
    let mut by_rid: HashMap<&str, Vec<usize>> = HashMap::new();
    let show_markers = filter == Some(LogLevel::Platform);
    if group {
        for (i, p) in parsed.iter().enumerate() {
            if let Some(rid) = &p.request_id {
                if !is_marker(p) || show_markers {
                    by_rid.entry(rid).or_default().push(i);
                }
            }
        }
    }
    for (i, p) in parsed.iter().enumerate() {
        if filter.is_some_and(|f| p.level != f) {
            continue;
        }
        if group {
            if let Some(rid) = &p.request_id {
                if is_marker(p) && !show_markers {
                    continue; // START/END/REPORT se resumen en el encabezado
                }
                if !emitted.insert(rid.clone()) {
                    continue;
                }
                out.push(Row::Group(rid.clone()));
                if !collapsed.contains(rid) {
                    for &j in by_rid.get(rid.as_str()).into_iter().flatten() {
                        if filter.is_none() || Some(parsed[j].level) == filter {
                            out.push(Row::Event(j));
                        }
                    }
                }
                continue;
            }
        }
        out.push(Row::Event(i));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(stream: &str, msg: &str) -> LogEvent {
        LogEvent { id: "i".into(), stream: stream.into(), timestamp: 0, message: msg.into() }
    }

    const RID: &str = "6bc28136-a1b2-4c3d-8e4f-0123456789ab";

    #[test]
    fn levels() {
        assert_eq!(parse(&ev("s", "ERROR boom")).level, LogLevel::Error);
        assert_eq!(parse(&ev("s", "all fine errors=0")).level, LogLevel::Info);
        assert_eq!(parse(&ev("s", "[WARNING] disk")).level, LogLevel::Warn);
        assert_eq!(parse(&ev("s", "x\nTraceback (most recent call last):\n  y\nValueError: z")).level, LogLevel::Error);
        assert_eq!(parse(&ev("s", "Task timed out after 3.00 seconds")).level, LogLevel::Error);
        assert_eq!(parse(&ev("s", "DEBUG x")).level, LogLevel::Debug);
    }

    #[test]
    fn strips_prefix() {
        assert_eq!(parse(&ev("s", "ERROR: no se pudo")).summary, "no se pudo");
        assert_eq!(parse(&ev("s", "INFO: x")).summary, "x");
    }

    #[test]
    fn json_event() {
        let p = parse(&ev("s", r#"{"level":"warn","msg":"hola","user":"ana","n":3,"ok":true,"extra":"q","requestId":"R1"}"#));
        assert_eq!(p.level, LogLevel::Warn);
        assert_eq!(p.summary, "hola");
        assert_eq!(p.request_id.as_deref(), Some("R1"));
        assert_eq!(p.chips, [("user".into(), "ana".into()), ("n".into(), "3".into()), ("ok".into(), "true".into())]);
        assert_eq!(parse(&ev("s", r#"{"level":50}"#)).level, LogLevel::Error);
        assert!(parse(&ev("s", "{no")).json.is_none());
    }

    #[test]
    fn lambda_lines() {
        let rep = format!("REPORT RequestId: {RID}\tDuration: 12.50 ms\tBilled Duration: 13 ms\tMemory Size: 128 MB\tMax Memory Used: 64 MB\tInit Duration: 200.1 ms");
        let p = parse(&ev("s", &rep));
        assert_eq!(p.platform, Some(Platform::Report));
        let r = p.report.unwrap();
        assert_eq!((r.duration_ms, r.billed_ms, r.init_ms), (Some(12.5), Some(13), Some(200.1)));
        assert_eq!((r.memory_size_mb, r.max_memory_mb), (Some(128), Some(64)));
        assert_eq!(p.request_id.as_deref(), Some(RID));

        let node = format!("2024-01-01T00:00:00Z\t{RID}\tERROR\tfalló");
        let p = parse(&ev("s", &node));
        assert_eq!((p.level, p.request_id.as_deref(), p.summary.as_str()), (LogLevel::Error, Some(RID), "falló"));
        let py = format!("[INFO]\t2024-01-01T00:00:00Z\t{RID}\thola");
        assert_eq!(parse(&ev("s", &py)).level, LogLevel::Info);

        let failed = format!("REPORT RequestId: {RID}\tDuration: 1 ms\tStatus: timeout");
        assert_eq!(parse(&ev("s", &failed)).level, LogLevel::Error);
        assert_eq!(parse(&ev("s", "INIT_START Runtime Version: x")).platform, Some(Platform::Other));
    }

    #[test]
    fn grouping() {
        let mut tr = RidTracker::default();
        let msgs = [
            format!("START RequestId: {RID} Version: $LATEST"),
            "hola".to_string(),
            "ERROR mal".to_string(),
            format!("END RequestId: {RID}"),
            format!("REPORT RequestId: {RID}\tDuration: 1 ms"),
            "suelto".to_string(),
        ];
        let mut parsed: Vec<ParsedLog> = msgs.iter().map(|m| parse(&ev("s", m))).collect();
        for p in &mut parsed {
            tr.assign(p);
        }
        assert_eq!(parsed[1].request_id.as_deref(), Some(RID));
        assert_eq!(parsed[5].request_id, None); // END cerró la invocación
        let info = &infos(&parsed)[RID];
        assert!(info.has_error && info.started && info.ended && info.report.is_some());
        assert_eq!(info.count, 2);

        let none = HashSet::new();
        assert_eq!(rows(&parsed, true, None, &none), [Row::Group(RID.into()), Row::Event(1), Row::Event(2), Row::Event(5)]);
        assert_eq!(rows(&parsed, false, None, &none).len(), 6);
        let collapsed = HashSet::from([RID.to_string()]);
        assert_eq!(rows(&parsed, true, None, &collapsed), [Row::Group(RID.into()), Row::Event(5)]);
        assert_eq!(rows(&parsed, true, Some(LogLevel::Error), &none), [Row::Group(RID.into()), Row::Event(2)]);
    }

    #[test]
    fn pretty_keeps_order() {
        let v: Value = serde_json::from_str(r#"{"z":1,"a":[true,null],"s":"x\"y"}"#).unwrap();
        let mut b = 1000;
        let txt: String = pretty(&v, 0, &mut b).into_iter().map(|(s, _)| s).collect();
        assert_eq!(txt, "{\n  \"z\": 1,\n  \"a\": [\n    true,\n    null\n  ],\n  \"s\": \"x\\\"y\"\n}");
    }
}
