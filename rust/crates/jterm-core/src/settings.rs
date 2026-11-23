use serde::{Deserialize, Serialize};

/// Preferencias del usuario. Los valores por defecto son los de la versión Swift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    // General
    /// "system", "es" o "en" (se aplica al reiniciar).
    pub language: String,
    pub shell: String,
    pub login_shell: bool,
    pub start_dir: String,
    pub confirm_close: bool,
    pub confirm_quit: bool,
    pub sidebar_at_launch: bool,
    pub show_snippets_button: bool,
    // Terminal
    pub theme: String,
    pub accent: String,
    pub font_family: String,
    pub font_size: u32,
    /// Texto un poco más grueso (peso Medium): compensa el trazado fino de algunos fondos oscuros.
    pub font_medium: bool,
    pub cursor_shape: String,
    pub cursor_blink: bool,
    pub scrollback: u32,
    pub option_as_meta: bool,
    pub mouse_reporting: bool,
    pub backspace_ctrl_h: bool,
    pub term: String,
    pub confirm_multiline_paste: bool,
    pub warn_dangerous_prod: bool,
    pub auto_record_prod: bool,
    pub underline_links: bool,
    pub recording_dir: String,
    // SSH
    pub ssh_keep_alive: u32,
    pub ssh_host_key: String,
    pub ssh_compression: bool,
    pub ssh_forward_agent: bool,
    pub ssh_extra: String,
    pub default_user: String,
    pub default_port: u16,
    // Archivos
    pub files_local_hidden: bool,
    pub files_remote_hidden: bool,
    pub files_local_start: String,
    pub files_confirm_delete: bool,
    pub transfer_dismiss: u32,
    // AWS
    pub aws_region: String,
    pub aws_user: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: "system".into(), shell: String::new(), login_shell: true, start_dir: String::new(), confirm_close: true,
            confirm_quit: true, sidebar_at_launch: true, show_snippets_button: true,
            theme: "jTerm".into(), accent: "Azul".into(), font_family: "system".into(), font_size: 13, font_medium: false,
            cursor_shape: "block".into(), cursor_blink: true, scrollback: 5000, option_as_meta: true,
            mouse_reporting: true, backspace_ctrl_h: false, term: "xterm-256color".into(),
            confirm_multiline_paste: true, warn_dangerous_prod: true, auto_record_prod: false,
            underline_links: true, recording_dir: String::new(),
            ssh_keep_alive: 30, ssh_host_key: "accept-new".into(), ssh_compression: false,
            ssh_forward_agent: false, ssh_extra: String::new(), default_user: String::new(), default_port: 22,
            files_local_hidden: false, files_remote_hidden: false, files_local_start: String::new(),
            files_confirm_delete: true, transfer_dismiss: 4,
            aws_region: "us-east-1".into(), aws_user: "ec2-user".into(),
        }
    }
}

impl Settings {
    /// Opciones de `ssh` derivadas de los ajustes (compartidas por terminal, archivos y AWS).
    pub fn ssh_args(&self) -> Vec<String> {
        let mut a = vec!["-o".to_string(), format!("StrictHostKeyChecking={}", self.ssh_host_key)];
        if self.ssh_keep_alive > 0 {
            a.extend(["-o".into(), format!("ServerAliveInterval={}", self.ssh_keep_alive)]);
        }
        if self.ssh_compression {
            a.push("-C".into());
        }
        if self.ssh_forward_agent {
            a.extend(["-o".into(), "ForwardAgent=yes".into()]);
        }
        for line in self.ssh_extra.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            a.extend(["-o".into(), line.to_string()]);
        }
        a
    }

    /// El shell elegido si es ejecutable; si no, `$SHELL` o zsh.
    pub fn resolved_shell(&self) -> String {
        if !self.shell.is_empty() && is_executable(&self.shell) {
            return self.shell.clone();
        }
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
    }
}

fn is_executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// Shells instalados (de /etc/shells).
pub fn installed_shells() -> Vec<String> {
    match std::fs::read_to_string("/etc/shells") {
        Ok(t) => t.lines().map(str::trim).filter(|l| l.starts_with('/') && is_executable(l)).map(String::from).collect(),
        Err(_) => vec!["/bin/zsh".into(), "/bin/bash".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_gets_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"Nord","fontSize":15}"#).unwrap();
        assert_eq!(s.theme, "Nord");
        assert_eq!(s.font_size, 15);
        assert_eq!(s.scrollback, 5000);
        assert!(s.confirm_close);
    }

    #[test]
    fn ssh_args_follow_settings() {
        let mut s = Settings::default();
        assert_eq!(s.ssh_args(), ["-o", "StrictHostKeyChecking=accept-new", "-o", "ServerAliveInterval=30"]);
        s.ssh_keep_alive = 0;
        s.ssh_compression = true;
        s.ssh_forward_agent = true;
        s.ssh_extra = "# nota\n  Foo=bar \n\n".into();
        assert_eq!(
            s.ssh_args(),
            ["-o", "StrictHostKeyChecking=accept-new", "-C", "-o", "ForwardAgent=yes", "-o", "Foo=bar"]
        );
    }
}
