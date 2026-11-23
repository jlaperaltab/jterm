//! Formulario de servidores: estado, validación y vista.

use crate::colors::Palette;
use crate::i18n::tr;
use crate::Message;
use iced::widget::{button, column, container, row, text, Column};
use crate::ui::{select as pick_list, input as text_input};
use iced::{Background, Border, Color, Element, Length};
use jterm_core::models::{AuthMethod, Server, ServerEnv};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthOpt(pub AuthMethod);

impl AuthOpt {
    pub const ALL: [AuthOpt; 2] = [AuthOpt(AuthMethod::Password), AuthOpt(AuthMethod::Key)];
}

impl fmt::Display for AuthOpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            AuthMethod::Password => "Contraseña",
            AuthMethod::Key => "Llave (key)",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvOpt(pub Option<ServerEnv>);

impl EnvOpt {
    pub const ALL: [EnvOpt; 4] = [EnvOpt(None), EnvOpt(Some(ServerEnv::Prod)), EnvOpt(Some(ServerEnv::Staging)), EnvOpt(Some(ServerEnv::Dev))];
}

impl fmt::Display for EnvOpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            None => "Ninguno",
            Some(ServerEnv::Prod) => "Producción",
            Some(ServerEnv::Staging) => "Staging",
            Some(ServerEnv::Dev) => "Desarrollo",
        })
    }
}

/// Servidor de salto (ProxyJump): ninguno u otro servidor guardado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpOpt {
    pub id: Option<Uuid>,
    pub label: String,
}

impl fmt::Display for JumpOpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Host,
    Port,
    User,
    KeyPath,
    StartDir,
    Password,
}

#[derive(Debug, Clone)]
pub struct Editor {
    /// `Some` si se edita un servidor existente.
    pub original: Option<Server>,
    pub name: String,
    pub host: String,
    pub port: String,
    pub user: String,
    pub auth: AuthMethod,
    pub key_path: String,
    pub start_dir: String,
    pub env: Option<ServerEnv>,
    pub jump: Option<Uuid>,
    /// Vacío = no cambiar la contraseña guardada.
    pub password: String,
    pub error: Option<String>,
}

impl Editor {
    pub fn new(default_user: &str, default_port: u16) -> Self {
        Editor {
            original: None, name: String::new(), host: String::new(), port: default_port.to_string(), user: default_user.into(),
            auth: AuthMethod::Password, key_path: String::new(), start_dir: String::new(), env: None, jump: None, password: String::new(), error: None,
        }
    }

    pub fn from_server(s: &Server) -> Self {
        Editor {
            original: Some(s.clone()), name: s.name.clone(), host: s.host.clone(), port: s.port.to_string(), user: s.user.clone(),
            auth: s.auth, key_path: s.key_path.clone(), start_dir: s.start_dir.clone(), env: s.env, jump: s.jump, password: String::new(), error: None,
        }
    }

    pub fn set(&mut self, f: Field, v: String) {
        self.error = None;
        match f {
            Field::Name => self.name = v,
            Field::Host => self.host = v,
            Field::Port => self.port = v.chars().filter(char::is_ascii_digit).take(5).collect(),
            Field::User => self.user = v,
            Field::KeyPath => self.key_path = v,
            Field::StartDir => self.start_dir = v,
            Field::Password => self.password = v,
        }
    }

    /// Servidor resultante y contraseña nueva (si se escribió una). `Err` con el motivo si los datos no sirven.
    pub fn build(&self) -> Result<(Server, Option<String>), String> {
        let host = self.host.trim();
        if host.is_empty() || host.contains(char::is_whitespace) {
            return Err("Indica el host (sin espacios).".into());
        }
        let port: u16 = self.port.parse().ok().filter(|p| *p >= 1).ok_or("El puerto debe estar entre 1 y 65535.")?;
        if self.auth == AuthMethod::Key && self.key_path.trim().is_empty() {
            return Err("Indica la ruta de la llave privada.".into());
        }
        let mut s = self.original.clone().unwrap_or_default();
        s.name = self.name.trim().into();
        s.host = host.into();
        s.port = port;
        s.user = self.user.trim().into();
        s.auth = self.auth;
        s.key_path = if self.auth == AuthMethod::Key { self.key_path.trim().into() } else { String::new() };
        s.start_dir = self.start_dir.trim().into();
        s.env = self.env;
        s.jump = self.jump.filter(|j| *j != s.id);
        Ok((s, (!self.password.is_empty()).then(|| self.password.clone())))
    }

    pub fn view<'a>(&'a self, jumps: Vec<JumpOpt>, p: &Palette, accent: Color) -> Element<'a, Message> {
        let (fg, dim) = (p.fg, Color { a: 0.55, ..p.fg });
        let label = |s: &'static str| text(tr(s)).size(10).color(dim);
        let input = |ph: &'static str, v: &'a str, f: Field| text_input(ph, v).on_input(move |s| Message::EditField(f, s)).on_submit(Message::EditSave).padding(7).size(13);
        let jump_sel = jumps.iter().find(|j| j.id == self.jump).cloned();
        let mut col: Column<Message> = column![
            text(tr(if self.original.is_some() { "Editar servidor" } else { "Nuevo servidor" })).size(16).color(fg),
            column![label("NOMBRE"), input("Producción web", &self.name, Field::Name)].spacing(4),
            row![
                column![label("HOST"), input("ejemplo.com", &self.host, Field::Host)].spacing(4).width(Length::FillPortion(3)),
                column![label("PUERTO"), input("22", &self.port, Field::Port)].spacing(4).width(Length::FillPortion(1)),
            ]
            .spacing(8),
            column![label("USUARIO"), input("root", &self.user, Field::User)].spacing(4),
            column![label("AUTENTICACIÓN"), pick_list(&AuthOpt::ALL[..], Some(AuthOpt(self.auth)), |a| Message::EditAuth(a.0)).width(Length::Fill)].spacing(4),
        ]
        .spacing(10);
        if self.auth == AuthMethod::Key {
            col = col.push(column![label("RUTA DE LA LLAVE"), input("~/.ssh/id_ed25519", &self.key_path, Field::KeyPath)].spacing(4));
        }
        let pw_label = if self.auth == AuthMethod::Key { "PASSPHRASE (OPCIONAL)" } else { "CONTRASEÑA" };
        let ph = if self.original.is_some() { "(sin cambios)" } else { "" };
        col = col.push(
            column![
                text(tr(pw_label)).size(10).color(dim),
                text_input(ph, &self.password).secure(true).on_input(|s| Message::EditField(Field::Password, s)).on_submit(Message::EditSave).padding(7).size(13),
                text(tr("Se guarda en el Keychain de macOS, nunca en un archivo.")).size(10).color(dim),
            ]
            .spacing(4),
        );
        col = col.push(
            column![
                label("DIRECTORIO INICIAL (OPCIONAL)"),
                input("/var/www/app", &self.start_dir, Field::StartDir),
                text(tr("Carpeta donde abre la sesión SSH y el gestor de archivos.")).size(10).color(dim),
            ]
            .spacing(4),
        );
        col = col.push(
            column![label("ENTORNO"), pick_list(&EnvOpt::ALL[..], Some(EnvOpt(self.env)), |e| Message::EditEnv(e.0)).width(Length::Fill)].spacing(4),
        );
        col = col.push(
            column![
                label("SALTO (PROXYJUMP)"),
                pick_list(jumps, jump_sel, |j: JumpOpt| Message::EditJump(j.id)).width(Length::Fill)
            ]
            .spacing(4),
        );
        if let Some(e) = &self.error {
            col = col.push(text(tr(e)).size(12).color(Color::from_rgb8(0xff, 0x6b, 0x6b)));
        }
        let mut buttons = row![].spacing(8);
        if self.original.is_some() {
            buttons = buttons.push(action_button("Eliminar", Color::from_rgb8(0xff, 0x6b, 0x6b), fg, false).on_press(Message::EditDelete));
        }
        buttons = buttons.push(iced::widget::space::horizontal());
        buttons = buttons.push(action_button("Cancelar", fg, fg, false).on_press(Message::EditCancel));
        buttons = buttons.push(action_button("Guardar", accent, fg, true).on_press(Message::EditSave));
        col = col.push(buttons);
        dialog(col.into(), p, 420.0)
    }
}

/// Botón de los diálogos: `filled` = acción principal (acento); si no, borde suave.
pub fn action_button<'a>(label: &'a str, color: Color, fg: Color, filled: bool) -> button::Button<'a, Message> {
    crate::ui::pill(label, if filled { crate::ui::Kind::Primary } else { crate::ui::Kind::Ghost }, color, fg)
}

/// Caja de diálogo centrada con el fondo de la paleta.
pub fn dialog<'a>(content: Element<'a, Message>, p: &Palette, width: f32) -> Element<'a, Message> {
    let bg = Color { r: p.bg.r * 0.85, g: p.bg.g * 0.85, b: p.bg.b * 0.85, a: 1.0 };
    let boxed = container(content)
        .padding(20)
        .width(Length::Fill)
        .max_width(width)
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: 12.0.into(), width: 1.0, color: Color { a: 0.15, ..Color::WHITE } },
            ..Default::default()
        });
    // Con ancho flexible la caja se pegaba a la izquierda: se centra horizontalmente en el espacio disponible.
    container(boxed).width(Length::Fill).center_x(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled() -> Editor {
        let mut e = Editor::new("root", 22);
        e.host = " ejemplo.com ".into();
        e.name = " web ".into();
        e
    }

    #[test]
    fn builds_trimmed_server() {
        let (s, pw) = filled().build().unwrap();
        assert_eq!((s.host.as_str(), s.name.as_str(), s.port, s.user.as_str()), ("ejemplo.com", "web", 22, "root"));
        assert_eq!(pw, None);
        assert!(s.key_path.is_empty());
    }

    #[test]
    fn validation() {
        let mut e = filled();
        e.host = "con espacio".into();
        assert!(e.build().is_err());
        e.host = "".into();
        assert!(e.build().is_err());
        let mut e = filled();
        e.port = "0".into();
        assert!(e.build().is_err());
        e.port = "99999".into();
        assert!(e.build().is_err());
        let mut e = filled();
        e.auth = AuthMethod::Key;
        assert!(e.build().unwrap_err().contains("llave"));
        e.key_path = "~/.ssh/id".into();
        assert_eq!(e.build().unwrap().0.key_path, "~/.ssh/id");
    }

    #[test]
    fn editing_keeps_identity_and_secret_only_when_typed() {
        let mut s = Server::default();
        s.host = "h".into();
        s.jump = Some(Uuid::new_v4());
        let mut e = Editor::from_server(&s);
        e.set(Field::Name, "nuevo".into());
        let (out, pw) = e.build().unwrap();
        assert_eq!((out.id, out.jump, pw), (s.id, s.jump, None));
        e.set(Field::Password, "secreto".into());
        assert_eq!(e.build().unwrap().1.as_deref(), Some("secreto"));
        // Un servidor no puede saltar por sí mismo.
        e.jump = Some(s.id);
        assert_eq!(e.build().unwrap().0.jump, None);
    }

    #[test]
    fn port_field_keeps_digits_only() {
        let mut e = filled();
        e.set(Field::Port, "2a2b2".into());
        assert_eq!(e.port, "222");
    }
}
