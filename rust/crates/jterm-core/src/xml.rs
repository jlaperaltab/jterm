//! Recorrido mínimo de XML por rutas ("A/B/C"), suficiente para las respuestas de EC2 y S3.

use quick_xml::events::Event;
use quick_xml::Reader;

/// Evento de `walk`: apertura o cierre de un elemento.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ev {
    Start,
    End,
}

/// Recorre el XML llamando a `on(Start, ruta, "")` al abrir un elemento y a `on(End, ruta, texto)` al cerrarlo.
/// Los prefijos de espacio de nombres se ignoran. Si el XML está roto, se detiene en silencio.
pub fn walk(xml: &[u8], mut on: impl FnMut(Ev, &str, &str)) {
    let mut reader = Reader::from_reader(xml);
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                path.push(local(e.name().as_ref()));
                text.clear();
                on(Ev::Start, &path.join("/"), "");
            }
            Ok(Event::Empty(e)) => {
                path.push(local(e.name().as_ref()));
                let p = path.join("/");
                on(Ev::Start, &p, "");
                on(Ev::End, &p, "");
                path.pop();
                text.clear();
            }
            Ok(Event::Text(t)) => {
                if let Ok(s) = t.unescape() {
                    text.push_str(&s);
                }
            }
            Ok(Event::CData(c)) => text.push_str(&String::from_utf8_lossy(&c)),
            Ok(Event::End(_)) => {
                let p = path.join("/");
                on(Ev::End, &p, &text);
                path.pop();
                text.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
}

fn local(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name);
    s.rsplit(':').next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_text() {
        let mut got = Vec::new();
        walk(b"<a xmlns='x'><b>hola &amp; chao</b><c/></a>", |ev, p, t| if ev == Ev::End { got.push((p.to_string(), t.to_string())) });
        assert_eq!(got, [("a/b".into(), "hola & chao".into()), ("a/c".into(), "".into()), ("a".into(), "".into())]);
    }
}
