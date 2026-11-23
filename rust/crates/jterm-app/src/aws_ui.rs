//! Panel AWS de la barra lateral (cuentas, regiones, instancias EC2) y editor de cuentas.

use crate::colors::Palette;
use crate::editor::{action_button, dialog};
use crate::i18n::tr;
use crate::Message;
use iced::widget::{button, column, container, row, scrollable, space, text, Column, Row};
use crate::ui::{select as pick_list, input as text_input};
use iced::{Background, Border, Color, Element, Length};
use jterm_core::aws::{AwsAccount, Ec2Instance, REGIONS};
use jterm_core::models::ServerEnv;
use std::fmt;
use uuid::Uuid;

pub const ALL_REGIONS: &str = "Todas las regiones";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwsField {
    Name,
    KeyId,
    Secret,
}

#[derive(Debug, Clone)]
pub struct AwsEditor {
    pub original: Option<AwsAccount>,
    pub name: String,
    pub key_id: String,
    /// Vacío = conservar la clave secreta guardada.
    pub secret: String,
    pub region: String,
    pub env: Option<ServerEnv>,
    pub error: Option<String>,
}

impl AwsEditor {
    pub fn new(default_region: &str) -> Self {
        AwsEditor { original: None, name: String::new(), key_id: String::new(), secret: String::new(), region: default_region.into(), env: None, error: None }
    }

    pub fn from_account(a: &AwsAccount) -> Self {
        AwsEditor { original: Some(a.clone()), name: a.name.clone(), key_id: a.access_key_id.clone(), secret: String::new(), region: a.region.clone(), env: a.env, error: None }
    }

    pub fn set(&mut self, f: AwsField, v: String) {
        self.error = None;
        match f {
            AwsField::Name => self.name = v,
            AwsField::KeyId => self.key_id = v,
            AwsField::Secret => self.secret = v,
        }
    }

    pub fn build(&self) -> Result<(AwsAccount, Option<String>), String> {
        let key = self.key_id.trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Err("Indica el ID de clave de acceso (sin espacios).".into());
        }
        if self.original.is_none() && self.secret.is_empty() {
            return Err("Indica la clave de acceso secreta.".into());
        }
        let mut a = self.original.clone().unwrap_or(AwsAccount { id: Uuid::new_v4(), name: String::new(), access_key_id: String::new(), region: String::new(), env: None });
        a.name = self.name.trim().into();
        a.access_key_id = key.into();
        a.region = self.region.clone();
        a.env = self.env;
        Ok((a, (!self.secret.is_empty()).then(|| self.secret.trim().to_string())))
    }

    pub fn view<'a>(&'a self, p: &Palette, accent: Color) -> Element<'a, Message> {
        let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
        let label = |s: &'static str| text(tr(s)).size(10).color(dim);
        let regions: Vec<String> = REGIONS.iter().map(|r| r.to_string()).collect();
        let mut col: Column<Message> = column![
            text(tr(if self.original.is_some() { "Editar cuenta de AWS" } else { "Nueva cuenta de AWS" })).size(16).color(fg),
            column![label("NOMBRE"), text_input("Producción", &self.name).on_input(|s| Message::AwsEdField(AwsField::Name, s)).on_submit(Message::AwsEdSave).padding(7).size(13)].spacing(4),
            column![label("ID DE CLAVE DE ACCESO"), text_input("AKIA…", &self.key_id).on_input(|s| Message::AwsEdField(AwsField::KeyId, s)).on_submit(Message::AwsEdSave).padding(7).size(13)].spacing(4),
            column![
                label("CLAVE DE ACCESO SECRETA"),
                text_input(if self.original.is_some() { "(sin cambios)" } else { "" }, &self.secret)
                    .secure(true)
                    .on_input(|s| Message::AwsEdField(AwsField::Secret, s))
                    .on_submit(Message::AwsEdSave)
                    .padding(7)
                    .size(13),
                text(tr("Se guarda en el Keychain de macOS, nunca en un archivo.")).size(10).color(dim),
            ]
            .spacing(4),
            column![label("REGIÓN POR DEFECTO"), pick_list(regions, Some(self.region.clone()), Message::AwsEdRegion).width(Length::Fill)].spacing(4),
            column![label("ENTORNO"), pick_list(&crate::editor::EnvOpt::ALL[..], Some(crate::editor::EnvOpt(self.env)), |e| Message::AwsEdEnv(e.0)).width(Length::Fill)].spacing(4),
        ]
        .spacing(10);
        if let Some(e) = &self.error {
            col = col.push(text(tr(e)).size(12).color(Color::from_rgb8(0xff, 0x6b, 0x6b)));
        }
        let mut buttons = row![].spacing(8);
        if self.original.is_some() {
            buttons = buttons.push(action_button("Eliminar", Color::from_rgb8(0xff, 0x6b, 0x6b), fg, false).on_press(Message::AwsEdDelete));
        }
        buttons = buttons.push(space::horizontal());
        buttons = buttons.push(action_button("Cancelar", fg, fg, false).on_press(Message::AwsEdCancel));
        buttons = buttons.push(action_button("Guardar", accent, fg, true).on_press(Message::AwsEdSave));
        dialog(col.push(buttons).into(), p, 420.0)
    }
}

/// Estado del panel AWS.
#[derive(Default)]
pub struct AwsPanel {
    pub account: Option<Uuid>,
    pub region: String,
    pub instances: Vec<Ec2Instance>,
    pub status: String,
    pub is_error: bool,
    pub loading: bool,
    /// Descarta respuestas de consultas antiguas.
    pub generation: u64,
}

/// Primero las que están en ejecución, luego por nombre.
pub fn sort_instances(v: &mut [Ec2Instance]) {
    v.sort_by_key(|i| (i.state != "running", i.name.to_lowercase()));
}

pub fn state_color(state: &str) -> Color {
    match state {
        "running" => Color::from_rgb8(0x30, 0xd1, 0x58),
        "pending" | "stopping" | "shutting-down" => Color::from_rgb8(0xff, 0x9f, 0x0a),
        _ => Color::from_rgb8(0x8e, 0x8e, 0x93),
    }
}

pub fn ip_label(i: &Ec2Instance) -> String {
    if !i.public_ip.is_empty() {
        i.public_ip.clone()
    } else if !i.private_ip.is_empty() {
        format!("{} (privada)", i.private_ip)
    } else {
        i.state.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountOpt {
    pub id: Uuid,
    pub label: String,
}

impl fmt::Display for AccountOpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn panel_view<'a>(panel: &'a AwsPanel, accounts: &'a [AwsAccount], p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
    let opts: Vec<AccountOpt> = accounts.iter().map(|a| AccountOpt { id: a.id, label: if a.name.is_empty() { a.access_key_id.clone() } else { a.name.clone() } }).collect();
    let selected = opts.iter().find(|o| Some(o.id) == panel.account).cloned();
    let env = accounts.iter().find(|a| Some(a.id) == panel.account).and_then(|a| a.env);
    let mut regions = vec![ALL_REGIONS.to_string()];
    regions.extend(REGIONS.iter().map(|r| r.to_string()));

    let small = |label: &'static str, on: Message| crate::ui::compact(label, fg, fg).on_press(on);
    let mut col: Column<Message> = Column::new().spacing(6).padding(10).width(Length::Fill);
    col = col.push(text(tr("CUENTA")).size(10).color(dim));
    col = col.push(
        row![
            pick_list(opts, selected, |o: AccountOpt| Message::AwsAccountPick(o.id)).placeholder("Sin cuentas").width(Length::Fill).text_size(12).padding([5, 8]),
            small("✎", Message::AwsEdEdit),
            small("+", Message::AwsEdNew),
        ]
        .spacing(4),
    );
    col = col.push(text(tr("REGIÓN")).size(10).color(dim));
    col = col.push(
        row![
            pick_list(regions.into_iter().map(crate::i18n::Tr).collect::<Vec<_>>(), Some(crate::i18n::Tr(if panel.region.is_empty() { ALL_REGIONS.to_string() } else { panel.region.clone() })), |t: crate::i18n::Tr<String>| Message::AwsRegionPick(t.0)).width(Length::Fill).text_size(12).padding([5, 8]),
            small(if panel.loading { "…" } else { "↻" }, Message::AwsRefresh),
        ]
        .spacing(4),
    );
    col = col.push(
        row![
            crate::ui::compact("🪣 S3", fg, fg).on_press(Message::S3Open).width(Length::Fill),
            crate::ui::compact("🔎 Logs", fg, fg).on_press(Message::LogsOpen).width(Length::Fill),
        ]
        .spacing(4),
    );
    if !panel.status.is_empty() {
        col = col.push(text(tr(&panel.status)).size(11).color(if panel.is_error { Color::from_rgb8(0xff, 0x6b, 0x6b) } else { dim }));
    }

    let mut list: Column<Message> = Column::new().spacing(1);
    for (idx, i) in panel.instances.iter().enumerate() {
        let mut name_row: Row<Message> = Row::new().spacing(6).align_y(iced::Alignment::Center);
        name_row = name_row.push(text(tr("●")).size(9).color(state_color(&i.state)));
        name_row = name_row.push(text(i.name.clone()).size(12).color(fg));
        if let Some(e) = env {
            let (r, g, b) = e.rgb();
            name_row = name_row.push(text(e.tag()).size(8).color(Color::from_rgb8(r, g, b)));
        }
        let info = column![
            name_row,
            text(format!("{} · {}", i.kind, ip_label(i))).size(10).color(dim),
            // ID de la instancia (para buscarla en la consola de AWS, en tickets, etc.).
            text(i.id.clone()).size(10).font(crate::term_view::FONT).color(Color { a: 0.4, ..fg }).wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(2);
        let pick = button(info).width(Length::Fill).padding([6, 8]).on_press(Message::AwsConnect(idx)).style(move |_, st| button::Style {
            background: Some(Background::Color(if matches!(st, button::Status::Hovered) { Color { a: 0.1, ..fg } } else { Color::TRANSPARENT })),
            text_color: fg,
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        });
        let mut line = row![pick].align_y(iced::Alignment::Center);
        if !i.public_ip.is_empty() {
            line = line.push(crate::ui::icon_button("⧉", dim, fg).on_press(Message::AwsCopy(i.public_ip.clone())));
        }
        line = line.push(crate::ui::icon_button("#", dim, fg).on_press(Message::AwsCopy(i.id.clone())));
        list = list.push(line);
    }
    // Margen a la derecha: la barra de desplazamiento se dibuja encima del contenido y tapaba los botones.
    col = col.push(scrollable(container(list).padding(iced::Padding { right: 14.0, ..Default::default() })).height(Length::Fill));
    let _ = accent;
    col.height(Length::Fill).into()
}

/// Diálogo que pregunta el usuario del sistema de la instancia.
pub fn user_prompt_view<'a>(instance: &'a Ec2Instance, user: &'a str, p: &Palette, accent: Color) -> Element<'a, Message> {
    let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
    let body = column![
        text(tr(&format!("Usuario para {}", instance.name))).size(16).color(fg),
        text(tr("Usuario del sistema de la instancia: ec2-user (Amazon Linux), ubuntu (Ubuntu), admin (Debian)…")).size(12).color(dim),
        text_input("ec2-user", user).on_input(Message::AwsUserInput).on_submit(Message::AwsUserOk).padding(7).size(13),
        row![
            space::horizontal(),
            action_button("Cancelar", fg, fg, false).on_press(Message::AwsUserCancel),
            action_button("Conectar", accent, fg, true).on_press(Message::AwsUserOk),
        ]
        .spacing(8),
    ]
    .spacing(12);
    dialog(container(body).into(), p, 420.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(name: &str, state: &str) -> Ec2Instance {
        Ec2Instance { id: format!("i-{name}"), name: name.into(), state: state.into(), ..Default::default() }
    }

    #[test]
    fn running_first_then_by_name() {
        let mut v = vec![inst("zeta", "running"), inst("beta", "stopped"), inst("Alfa", "running"), inst("alfa2", "stopped")];
        sort_instances(&mut v);
        assert_eq!(v.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Alfa", "zeta", "alfa2", "beta"]);
    }

    #[test]
    fn ip_labels() {
        let mut i = inst("a", "running");
        assert_eq!(ip_label(&i), "running");
        i.private_ip = "10.0.0.5".into();
        assert_eq!(ip_label(&i), "10.0.0.5 (privada)");
        i.public_ip = "1.2.3.4".into();
        assert_eq!(ip_label(&i), "1.2.3.4");
    }

    #[test]
    fn editor_validation_and_secret_handling() {
        let mut e = AwsEditor::new("us-east-1");
        assert!(e.build().is_err());
        e.key_id = "AKIA123".into();
        assert!(e.build().unwrap_err().contains("secreta"), "una cuenta nueva exige la clave secreta");
        e.secret = " sek ".into();
        let (a, s) = e.build().unwrap();
        assert_eq!((a.access_key_id.as_str(), a.region.as_str(), s.as_deref()), ("AKIA123", "us-east-1", Some("sek")));
        let mut ed = AwsEditor::from_account(&a);
        ed.set(AwsField::Name, " renombrada ".into());
        let (b, s) = ed.build().unwrap();
        assert_eq!((b.id, b.name.as_str(), s), (a.id, "renombrada", None));
        ed.key_id = "con espacio".into();
        assert!(ed.build().is_err());
    }
}
