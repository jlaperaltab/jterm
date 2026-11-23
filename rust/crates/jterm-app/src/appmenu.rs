//! Menú superior de macOS (barra de menús nativa) con muda. Los eventos se leen desde el temporizador de la interfaz.

use crate::i18n::tr;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuAction {
    Settings,
    Copy,
    Paste,
    CheckUpdates,
    Quit,
    NewTab,
    ClosePane,
    CloseTab,
    Search,
    FindNext,
    FindPrev,
    PromptPrev,
    PromptNext,
    SplitRight,
    SplitDown,
    SplitRightLocal,
    SplitDownLocal,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    NextPane,
    PrevPane,
    Zoom,
    Broadcast,
    Snippets,
    Finder,
    NextTab,
    PrevTab,
    Sidebar,
    GoTab(u8),
}

impl MenuAction {
    /// ¿Solo tiene sentido con varios paneles en la pestaña?
    pub fn needs_many_panes(self) -> bool {
        use MenuAction::*;
        matches!(self, FocusLeft | FocusRight | FocusUp | FocusDown | NextPane | PrevPane | Zoom | Broadcast)
    }
}

/// Un ítem: texto, acción y atajo (modificadores, tecla).
pub struct Entry {
    pub title: String,
    pub action: MenuAction,
    pub key: Option<(Modifiers, Code)>,
}

fn e(title: &str, action: MenuAction, key: Option<(Modifiers, Code)>) -> Entry {
    Entry { title: title.into(), action, key }
}

pub enum Row {
    Item(Entry),
    Separator,
}

/// Contenido de los menús propios (los estándar del sistema se añaden aparte).
pub fn sections() -> Vec<(&'static str, Vec<Row>)> {
    use Code::*;
    use MenuAction::*;
    let cmd = Modifiers::SUPER;
    let cmd_shift = Modifiers::SUPER | Modifiers::SHIFT;
    let cmd_alt = Modifiers::SUPER | Modifiers::ALT;
    let k = |m: Modifiers, c: Code| Some((m, c));
    let item = |t: &str, a: MenuAction, key: Option<(Modifiers, Code)>| Row::Item(e(t, a, key));
    let digits = [Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9];

    let mut tabs = vec![
        item("Siguiente", NextTab, k(cmd_shift, BracketRight)),
        item("Anterior", PrevTab, k(cmd_shift, BracketLeft)),
        item("Mostrar u ocultar la barra lateral", Sidebar, k(cmd, KeyB)),
        Row::Separator,
    ];
    for (i, d) in digits.iter().enumerate() {
        tabs.push(item(&format!("Pestaña {}", i + 1), GoTab(i as u8), k(cmd, *d)));
    }

    vec![
        (
            "Archivo",
            vec![
                item("Nueva pestaña", NewTab, k(cmd, KeyT)),
                item("Cerrar panel o pestaña", ClosePane, k(cmd, KeyW)),
                item("Cerrar pestaña entera", CloseTab, k(cmd_alt, KeyW)),
            ],
        ),
        (
            "Edición",
            vec![
                // Sin atajo en el menú a propósito: si el menú se queda con ⌘C/⌘V, AppKit se traga la tecla y ni el
                // terminal ni los campos de texto la reciben. El atajo lo recibe la propia interfaz.
                item("Copiar", MenuAction::Copy, None),
                item("Pegar", MenuAction::Paste, None),
                Row::Separator,
                item("Buscar…", Search, k(cmd, KeyF)),
                item("Buscar siguiente", FindNext, k(cmd, KeyG)),
                item("Buscar anterior", FindPrev, k(cmd_shift, KeyG)),
            ],
        ),
        (
            "Terminal",
            vec![item("Prompt anterior", PromptPrev, k(cmd, ArrowUp)), item("Prompt siguiente", PromptNext, k(cmd, ArrowDown))],
        ),
        (
            "Paneles",
            vec![
                item("Dividir a la derecha", SplitRight, k(cmd, KeyD)),
                item("Dividir hacia abajo", SplitDown, k(cmd_shift, KeyD)),
                item("Dividir a la derecha con terminal local", SplitRightLocal, None),
                item("Dividir hacia abajo con terminal local", SplitDownLocal, None),
                Row::Separator,
                item("Ir al panel de la izquierda", FocusLeft, k(cmd_alt, ArrowLeft)),
                item("Ir al panel de la derecha", FocusRight, k(cmd_alt, ArrowRight)),
                item("Ir al panel de arriba", FocusUp, k(cmd_alt, ArrowUp)),
                item("Ir al panel de abajo", FocusDown, k(cmd_alt, ArrowDown)),
                item("Panel siguiente", NextPane, k(cmd, BracketRight)),
                item("Panel anterior", PrevPane, k(cmd, BracketLeft)),
                Row::Separator,
                item("Maximizar panel", Zoom, k(cmd_shift, Enter)),
                item("Escribir en todos los paneles", Broadcast, k(cmd_alt, KeyI)),
            ],
        ),
        ("Comandos", vec![item("Mostrar comandos guardados", Snippets, k(cmd_shift, KeyK)), item("Buscar servidor, instancia o comando…", Finder, k(cmd, KeyK))]),
        ("Pestañas", tabs),
    ]
}

pub struct AppMenu {
    _menu: Menu,
    ids: HashMap<MenuId, MenuAction>,
    items: HashMap<MenuAction, MenuItem>,
    many: Option<bool>,
}

impl AppMenu {
    /// Crea la barra de menús y la instala. Debe llamarse en el hilo principal, con la aplicación ya iniciada.
    pub fn install() -> Result<Self, String> {
        let err = |e: muda::Error| e.to_string();
        let menu = Menu::new();
        let mut ids = HashMap::new();
        let mut items: HashMap<MenuAction, MenuItem> = HashMap::new();
        let mut make = |entry: Entry| -> MenuItem {
            let accel = entry.key.map(|(m, c)| Accelerator::new(m, c));
            let it = MenuItem::new(tr(&entry.title), true, accel);
            ids.insert(it.id().clone(), entry.action);
            items.insert(entry.action, it.clone());
            it
        };

        // Menú de la aplicación.
        let about = AboutMetadata {
            name: Some("jTerm".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            comments: Some(tr("Terminal para macOS con SSH, AWS, archivos y comandos guardados.")),
            ..Default::default()
        };
        let settings = make(e("Ajustes…", MenuAction::Settings, Some((Modifiers::SUPER, Code::Comma))));
        let updates = make(e("Buscar actualizaciones…", MenuAction::CheckUpdates, None));
        let quit = make(e("Salir de jTerm", MenuAction::Quit, Some((Modifiers::SUPER, Code::KeyQ))));
        let app = Submenu::with_items(
            "jTerm",
            true,
            &[
                &PredefinedMenuItem::about(Some(&tr("Acerca de jTerm")), Some(about)),
                &updates,
                &PredefinedMenuItem::separator(),
                &settings,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(Some(&tr("Ocultar jTerm"))),
                &PredefinedMenuItem::hide_others(Some(&tr("Ocultar otros"))),
                &PredefinedMenuItem::show_all(Some(&tr("Mostrar todo"))),
                &PredefinedMenuItem::separator(),
                &quit,
            ],
        )
        .map_err(err)?;
        menu.append(&app).map_err(err)?;

        for (title, rows) in sections() {
            let sub = Submenu::new(tr(title), true);
            for row in rows {
                match row {
                    Row::Separator => sub.append(&PredefinedMenuItem::separator()).map_err(err)?,
                    Row::Item(entry) => sub.append(&make(entry)).map_err(err)?,
                }
            }
            menu.append(&sub).map_err(err)?;
        }

        let window = Submenu::with_items(
            &tr("Ventana"),
            true,
            &[&PredefinedMenuItem::minimize(Some(&tr("Minimizar"))), &PredefinedMenuItem::maximize(Some("Zoom")), &PredefinedMenuItem::fullscreen(Some(&tr("Pantalla completa")))],
        )
        .map_err(err)?;
        menu.append(&window).map_err(err)?;

        #[cfg(target_os = "macos")]
        {
            menu.init_for_nsapp();
            window.set_as_windows_menu_for_nsapp();
        }
        Ok(AppMenu { _menu: menu, ids, items, many: None })
    }

    /// Acciones pulsadas desde la última consulta.
    pub fn poll(&self) -> Vec<MenuAction> {
        MenuEvent::receiver().try_iter().filter_map(|ev| self.ids.get(&ev.id).copied()).collect()
    }

    /// Activa o desactiva los ítems que solo valen con varios paneles (solo hace algo si cambió).
    pub fn set_many_panes(&mut self, many: bool) {
        if self.many == Some(many) {
            return;
        }
        self.many = Some(many);
        for (a, it) in &self.items {
            if a.needs_many_panes() {
                it.set_enabled(many);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn all_entries() -> Vec<Entry> {
        sections().into_iter().flat_map(|(_, rows)| rows).filter_map(|r| if let Row::Item(e) = r { Some(e) } else { None }).collect()
    }

    #[test]
    fn titles_actions_and_shortcuts_are_unique() {
        let entries = all_entries();
        let titles: HashSet<&str> = entries.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(titles.len(), entries.len(), "títulos repetidos");
        let mut seen = HashSet::new();
        for x in &entries {
            assert!(seen.insert(x.action), "acción repetida: {:?}", x.action);
        }
        let keys: Vec<_> = entries.iter().filter_map(|x| x.key).map(|(m, c)| (m.bits(), format!("{c:?}"))).collect();
        assert_eq!(keys.iter().collect::<HashSet<_>>().len(), keys.len(), "atajos repetidos");
    }

    #[test]
    fn expected_menus_exist_with_the_swift_shortcuts() {
        let names: Vec<&str> = sections().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["Archivo", "Edición", "Terminal", "Paneles", "Comandos", "Pestañas"]);
        let entries = all_entries();
        let key_of = |a: MenuAction| entries.iter().find(|x| x.action == a).and_then(|x| x.key);
        assert_eq!(key_of(MenuAction::NewTab), Some((Modifiers::SUPER, Code::KeyT)));
        assert_eq!(key_of(MenuAction::SplitDown), Some((Modifiers::SUPER | Modifiers::SHIFT, Code::KeyD)));
        assert_eq!(key_of(MenuAction::Broadcast), Some((Modifiers::SUPER | Modifiers::ALT, Code::KeyI)));
        assert_eq!(key_of(MenuAction::GoTab(8)), Some((Modifiers::SUPER, Code::Digit9)));
        assert!(entries.iter().filter(|x| matches!(x.action, MenuAction::GoTab(_))).count() == 9);
    }

    #[test]
    fn pane_actions_need_several_panes() {
        assert!(MenuAction::Zoom.needs_many_panes() && MenuAction::NextPane.needs_many_panes());
        assert!(!MenuAction::SplitRight.needs_many_panes() && !MenuAction::NewTab.needs_many_panes());
    }
}
