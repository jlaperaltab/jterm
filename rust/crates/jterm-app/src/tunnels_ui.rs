//! Pestaña «Túneles» de la barra lateral y su editor.

use crate::colors::Palette;
use crate::editor::{action_button, dialog};
use crate::ui::{self, caption, input as text_input, select as pick_list};
use crate::i18n::tr;
use crate::Message;
use iced::widget::{button, column, container, row, scrollable, space, text, Column};
use iced::{Background, Border, Color, Element, Length};
use jterm_core::models::{Server, Tunnel, TunnelKind};
use jterm_core::tunnels::{self, TunnelState};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunField {
    Name,
    Listen,
    Host,
    Port,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerOpt {
    pub id: Uuid,
    pub label: String,
}

impl fmt::Display for ServerOpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn server_label(s: &Server) -> String {
    if s.name.is_empty() { s.host.clone() } else { s.name.clone() }
}

#[derive(Debug, Clone)]
pub struct TunnelEditor {
    pub original: Option<Tunnel>,
    pub name: String,
    pub kind: TunnelKind,
    pub listen: String,
    pub host: String,
    pub port: String,
    pub server: Option<Uuid>,
    pub error: Option<String>,
}

impl TunnelEditor {
    pub fn new(first_server: Option<Uuid>) -> Self {
        TunnelEditor { original: None, name: String::new(), kind: TunnelKind::Local, listen: String::new(), host: "localhost".into(), port: String::new(), server: first_server, error: None }
    }

    pub fn from_tunnel(t: &Tunnel) -> Self {
        TunnelEditor {
            original: Some(t.clone()), name: t.name.clone(), kind: t.kind, listen: t.listen_port.to_string(), host: t.dest_host.clone(),
            port: t.dest_port.to_string(), server: t.server_id, error: None,
        }
    }

    pub fn set(&mut self, f: TunField, v: String) {
        self.error = None;
        let digits = |s: String| s.chars().filter(char::is_ascii_digit).take(5).collect::<String>();
        match f {
            TunField::Name => self.name = v,
            TunField::Listen => self.listen = digits(v),
            TunField::Host => self.host = v,
            TunField::Port => self.port = digits(v),
        }
    }

    pub fn build(&self, is_root: bool) -> Result<Tunnel, String> {
        let mut t = self.original.clone().unwrap_or(Tunnel {
            id: Uuid::new_v4(), name: String::new(), kind: TunnelKind::Local, listen_port: 0, dest_host: String::new(), dest_port: 0, server_id: None,
        });
        t.name = self.name.trim().into();
        t.kind = self.kind;
        t.listen_port = self.listen.parse().unwrap_or(0);
        t.dest_host = self.host.trim().into();
        t.dest_port = self.port.parse().unwrap_or(0);
        t.server_id = self.server;
        match t.validation_error(is_root) {
            Some(e) => Err(e.to_string()),
            None => Ok(t),
        }
    }

    pub fn view<'a>(&'a self, servers: &'a [Server], p: &Palette, accent: Color) -> Element<'a, Message> {
        let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
        let local = self.kind == TunnelKind::Local;
        let opts: Vec<ServerOpt> = servers.iter().map(|s| ServerOpt { id: s.id, label: server_label(s) }).collect();
        let selected = opts.iter().find(|o| Some(o.id) == self.server).cloned();
        let help = if local {
            "Abre un puerto en este Mac (solo accesible desde él) y lo lleva, a través del servidor, al destino. Ejemplo: puerto 5433 → db.interna:5432 y luego conectas tu cliente a localhost:5433."
        } else {
            "Abre un puerto en el servidor y lo lleva a un destino de este Mac. Sirve para que el servidor llegue a algo tuyo, como una app en localhost:3000."
        };
        let mut col: Column<Message> = column![
            text(tr(if self.original.is_some() { "Editar túnel" } else { "Nuevo túnel" })).size(16).color(fg),
            text(tr("Reenvío de puertos por SSH")).size(11).color(dim),
            column![caption("Nombre", fg), text_input("Base de datos de producción", &self.name).on_input(|s| Message::TunEdField(TunField::Name, s)).on_submit(Message::TunEdSave)].spacing(5),
            column![caption("Tipo", fg), ui::segmented(&["Local (-L)", "Remoto (-R)"], if local { 0 } else { 1 }, accent, fg, |i| Message::TunEdKind(if i == 0 { TunnelKind::Local } else { TunnelKind::Remote }))].spacing(5),
            text(tr(help)).size(11).color(dim),
            row![
                column![caption(if local { "Puerto en este Mac" } else { "Puerto en el servidor" }, fg), text_input("5433", &self.listen).on_input(|s| Message::TunEdField(TunField::Listen, s)).on_submit(Message::TunEdSave)].spacing(5).width(Length::FillPortion(2)),
                column![caption(if local { "Destino (visto desde el servidor)" } else { "Destino (en este Mac)" }, fg), text_input("db.interna", &self.host).on_input(|s| Message::TunEdField(TunField::Host, s)).on_submit(Message::TunEdSave)].spacing(5).width(Length::FillPortion(3)),
                column![caption("Puerto destino", fg), text_input("5432", &self.port).on_input(|s| Message::TunEdField(TunField::Port, s)).on_submit(Message::TunEdSave)].spacing(5).width(Length::FillPortion(2)),
            ]
            .spacing(8),
            column![caption("Servidor por el que pasa", fg), pick_list(opts, selected, |o: ServerOpt| Message::TunEdServer(Some(o.id))).placeholder("Primero guarda un servidor").width(Length::Fill)].spacing(5),
        ]
        .spacing(12);
        if let Some(e) = &self.error {
            col = col.push(text(tr(e)).size(12).color(Color::from_rgb8(0xff, 0x6b, 0x6b)));
        }
        let mut buttons = row![].spacing(8);
        if self.original.is_some() {
            buttons = buttons.push(action_button("Eliminar", Color::from_rgb8(0xff, 0x6b, 0x6b), fg, false).on_press(Message::TunEdDelete));
        }
        buttons = buttons.push(space::horizontal());
        buttons = buttons.push(action_button("Cancelar", fg, fg, false).on_press(Message::TunEdCancel));
        buttons = buttons.push(action_button("Guardar", accent, fg, true).on_press(Message::TunEdSave));
        dialog(col.push(buttons).into(), p, 520.0)
    }
}

/// Texto y color del estado de un túnel.
pub fn state_text(s: &TunnelState) -> Option<(String, Color)> {
    match s {
        TunnelState::Stopped => None,
        TunnelState::Starting => Some(("Conectando…".into(), Color::from_rgb8(0xff, 0x9f, 0x0a))),
        TunnelState::Active => Some(("Activo".into(), Color::from_rgb8(0x30, 0xd1, 0x58))),
        TunnelState::Failed(m) => Some((m.clone(), Color::from_rgb8(0xff, 0x6b, 0x6b))),
    }
}

pub fn panel_view<'a>(tunnels: &'a [Tunnel], servers: &'a [Server], mgr: &tunnels::TunnelManager, p: &Palette) -> Element<'a, Message> {
    let (fg, dim) = (p.fg, Color { a: 0.5, ..p.fg });
    let mut col: Column<Message> = Column::new().spacing(4).padding([4, 10]).width(Length::Fill);
    col = col.push(row![caption("Túneles SSH", fg), space::horizontal(), ui::icon_button("+", dim, fg).on_press(Message::TunEdNew)].align_y(iced::Alignment::Center));
    if tunnels.is_empty() {
        col = col.push(text(tr("Aún no hay túneles.\n\nUn túnel conecta un puerto de tu Mac con un servicio al que solo llega el servidor (una base de datos, un panel interno…). Guarda uno con + y actívalo con un clic.")).size(12).color(dim));
    }
    for t in tunnels {
        let state = mgr.state(t.id);
        let running = state.is_running();
        let via = servers.iter().find(|s| Some(s.id) == t.server_id).map(|s| format!(" · vía {}", server_label(s))).unwrap_or_else(|| " · sin servidor".into());
        let dot = match &state {
            TunnelState::Active => Color::from_rgb8(0x30, 0xd1, 0x58),
            TunnelState::Starting => Color::from_rgb8(0xff, 0x9f, 0x0a),
            TunnelState::Failed(_) => Color::from_rgb8(0xff, 0x6b, 0x6b),
            TunnelState::Stopped => Color { a: 0.25, ..fg },
        };
        let mut info: Column<Message> = column![
            row![text(tr("●")).size(9).color(dot), text(t.title()).size(13).color(fg)].spacing(7).align_y(iced::Alignment::Center),
            text(format!("{}{via}", t.summary())).size(11).color(dim),
        ]
        .spacing(2);
        if let Some((msg, c)) = state_text(&state) {
            info = info.push(text(tr(&msg)).size(11).color(c));
        }
        let toggle = ui::compact(if running { "■" } else { "▶" }, if running { Color::from_rgb8(0xff, 0x6b, 0x6b) } else { fg }, fg).on_press(Message::TunToggle(t.id));
        let toggle = if running { ui::compact("Detener", Color::from_rgb8(0xff, 0x6b, 0x6b), fg).on_press(Message::TunToggle(t.id)) } else { toggle };
        let row_btn = button(info).width(Length::Fill).padding([7, 9]).on_press(Message::TunEdEdit(t.id)).style(move |_, st| button::Style {
            background: Some(Background::Color(if matches!(st, button::Status::Hovered) { Color { a: 0.07, ..fg } } else { Color::TRANSPARENT })),
            text_color: fg,
            border: Border { radius: 9.0.into(), ..Default::default() },
            ..Default::default()
        });
        col = col.push(column![row_btn, container(toggle).padding([0, 9])].spacing(2));
    }
    scrollable(col).height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> TunnelEditor {
        let mut e = TunnelEditor::new(Some(Uuid::new_v4()));
        e.listen = "5433".into();
        e.port = "5432".into();
        e.host = " db.interna ".into();
        e
    }

    #[test]
    fn builds_and_validates() {
        let t = editor().build(false).unwrap();
        assert_eq!((t.listen_port, t.dest_port, t.dest_host.as_str()), (5433, 5432, "db.interna"));
        let mut e = editor();
        e.port.clear();
        assert!(e.build(false).is_err());
        let mut e = editor();
        e.server = None;
        assert!(e.build(false).unwrap_err().contains("servidor"));
        let mut e = editor();
        e.listen = "80".into();
        assert!(e.build(false).is_err(), "puertos < 1024 solo para el administrador");
        assert!(e.build(true).is_ok());
        let mut e = editor();
        e.kind = TunnelKind::Remote;
        e.listen = "80".into();
        assert!(e.build(false).is_ok(), "en remoto el puerto bajo se abre en el servidor");
    }

    #[test]
    fn editing_keeps_identity_and_digits_only() {
        let t = editor().build(false).unwrap();
        let mut e = TunnelEditor::from_tunnel(&t);
        e.set(TunField::Listen, "1a2b3c4".into());
        assert_eq!(e.listen, "1234");
        e.set(TunField::Name, "nuevo".into());
        assert_eq!(e.build(false).unwrap().id, t.id);
    }

    #[test]
    fn state_texts() {
        assert!(state_text(&TunnelState::Stopped).is_none());
        assert_eq!(state_text(&TunnelState::Active).unwrap().0, "Activo");
        assert_eq!(state_text(&TunnelState::Failed("x".into())).unwrap().0, "x");
    }
}
