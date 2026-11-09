//! Personalizar interface: mark toolbar tools and menu commands, then turn them off.
//!
//! Off means the row is hidden and the command does not run from the menu, a shortcut or
//! automation. Pixels already stored in the document are not deleted and are still drawn.
//! A filter that only runs when chosen has no per-frame work to stop. The 3D gizmo stops
//! asking for a new mesh while extrusion commands are off; the last render stays.
//!
//! The draft is not a document edit and is not an undo step. OK writes preferences.

use crate::PhotocraftApp;
use crate::state::Tool;

#[derive(Clone, Debug, PartialEq)]
pub struct ChromeDraft {
    pub(crate) commands: Vec<String>,
    pub(crate) tools: Vec<String>,
    pub(crate) menus: Vec<String>,
}

impl ChromeDraft {
    fn from_prefs(app: &PhotocraftApp) -> Self {
        let f = &app.session.prefs().features;
        Self { commands: f.commands.clone(), tools: f.tools.clone(), menus: f.menus.clone() }
    }

    pub fn toggle_command(&mut self, id: &str) {
        if photocraft_engine::prefs::FeatureGates::essential(id) {
            return;
        }
        toggle(&mut self.commands, id);
    }

    pub fn toggle_menu(&mut self, path: &str) {
        toggle(&mut self.menus, path);
    }

    pub fn toggle_tool(&mut self, name: &str) {
        toggle(&mut self.tools, name);
    }

    pub fn command_marked(&self, id: &str, menu: &[&str]) -> bool {
        self.commands.iter().any(|c| c == id) || self.menus.iter().any(|gate| menu_prefix(menu, gate))
    }

    pub fn tool_marked(&self, tool: Tool) -> bool {
        self.tools.iter().any(|t| t == &format!("{tool:?}"))
    }

    fn clear(&mut self) {
        self.commands.clear();
        self.tools.clear();
        self.menus.clear();
    }
}

fn toggle(list: &mut Vec<String>, id: &str) {
    if let Some(i) = list.iter().position(|c| c == id) {
        list.remove(i);
    } else {
        list.push(id.to_string());
    }
}

fn menu_prefix(menu: &[&str], gate: &str) -> bool {
    let parts: Vec<&str> = gate.split('/').filter(|s| !s.is_empty()).collect();
    !parts.is_empty() && menu.len() >= parts.len() && menu.iter().zip(parts.iter()).all(|(a, b)| a == b)
}

pub fn editing(app: &PhotocraftApp) -> bool {
    app.chrome_edit.is_some()
}

/// The 3D axis widget stays out of the frame when extrusion commands are off.
pub fn blocks_extrude(app: &PhotocraftApp) -> bool {
    let f = &app.session.prefs().features;
    f.blocks_command("extrude.set", &["3D"]) || f.blocks_command("extrude.fromLayer", &["3D"]) || f.menus.iter().any(|m| m == "3D")
}

pub fn tool_blocked(app: &PhotocraftApp, tool: Tool) -> bool {
    app.chrome_edit.is_none() && app.session.prefs().features.blocks_tool(&format!("{tool:?}"))
}

/// Shell commands are not in the engine catalogue. Same rule as [`FeatureGates::blocks_command`].
pub fn blocked(app: &PhotocraftApp, id: &str) -> Option<String> {
    if photocraft_engine::prefs::FeatureGates::essential(id) {
        return None;
    }
    let menu = command_menu(id);
    if app.session.prefs().features.blocks_command(id, &menu) {
        Some(photocraft_engine::prefs::FeatureGates::NOTE.into())
    } else {
        None
    }
}

fn command_menu(id: &str) -> Vec<&str> {
    if let Some(spec) = photocraft_engine::commands::find(id) {
        return spec.menu.to_vec();
    }
    crate::menus::UI_COMMANDS.iter().find(|(cmd, ..)| *cmd == id).map(|(_, _, path, _)| path.to_vec()).unwrap_or_default()
}

pub fn hidden_tools(app: &PhotocraftApp) -> Vec<String> {
    let mut names = app.session.prefs().toolbar.hidden.clone();
    if app.chrome_edit.is_none() {
        names.extend(app.session.prefs().features.tools.iter().cloned());
    }
    names
}

/// If the current tool was turned off, leave it for one that still runs and drop the pointer.
pub fn enforce(app: &mut PhotocraftApp) {
    if app.chrome_edit.is_some() {
        return;
    }
    let name = format!("{:?}", app.ui.tool);
    if !app.session.prefs().features.blocks_tool(&name) {
        return;
    }
    let fallback = [Tool::Move, Tool::Hand, Tool::Zoom].into_iter().chain(Tool::ALL).find(|tool| !app.session.prefs().features.blocks_tool(&format!("{tool:?}")));
    if let Some(tool) = fallback {
        app.ui.tool = tool;
    }
    app.drag = None;
    app.tool_override = None;
    app.live_stroke = None;
    app.move_mods = crate::move_mods::MoveDrag::default();
    app.ui.direct_selection.drag = None;
}

pub fn buttons(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.x = 4.0;
    if app.chrome_edit.is_some() {
        let ok = ui.add(egui::Button::new(crate::i18n::tr(crate::i18n::current(), "OK")).fill(t.accent));
        let cancel = ui.button(crate::i18n::tr(crate::i18n::current(), "Cancel"));
        let all = ui.button(crate::i18n::tr(crate::i18n::current(), "Show all"));
        if ok.clicked() {
            apply(app);
        }
        if cancel.clicked() {
            app.chrome_edit = None;
        }
        if all.clicked() {
            if let Some(edit) = app.chrome_edit.as_mut() {
                edit.clear();
            }
            apply(app);
        }
        ok.on_hover_text(crate::i18n::tr(crate::i18n::current(), "Turn off the marked tools and commands"));
    } else if ui.button(crate::i18n::tr(crate::i18n::current(), "Customize")).clicked() {
        app.chrome_edit = Some(ChromeDraft::from_prefs(app));
    }
}

fn apply(app: &mut PhotocraftApp) {
    let Some(draft) = app.chrome_edit.clone() else { return };
    app.session.edit_prefs(|p| {
        p.features.commands = draft.commands.clone();
        p.features.tools = draft.tools.clone();
        p.features.menus = draft.menus.clone();
    });
    app.chrome_edit = None;
    enforce(app);
}

/// Menu rows call this instead of running the command while Customize is open.
pub fn take_menu_toggle(app: &mut PhotocraftApp, id: &str) {
    if let Some(edit) = app.chrome_edit.as_mut() {
        if let Some(path) = id.strip_prefix("menu:") {
            edit.toggle_menu(path);
        } else {
            edit.toggle_command(id);
        }
    }
}

pub fn menu_marked(app: &PhotocraftApp, id: &str, path: &[String]) -> bool {
    let menu: Vec<&str> = path.iter().map(String::as_str).collect();
    app.chrome_edit.as_ref().is_some_and(|edit| edit.command_marked(id, &menu))
}

pub fn top_marked(app: &PhotocraftApp, top: &str) -> bool {
    app.chrome_edit.as_ref().is_some_and(|edit| edit.menus.iter().any(|m| m == top))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_deactivated_command_does_not_run_and_save_cannot_be_deactivated() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width": 32, "height": 32})).unwrap();
        app.session.edit_prefs(|p| {
            p.features.commands = vec!["file.save".into(), "image.imageRotation.180".into()];
        });
        assert!(!app.session.prefs().features.blocks_command("file.save", &["File"]), "save stays available");
        assert!(blocked(&app, "file.save").is_none());
        let err = app.session.execute("image.imageRotation.180", json!({})).unwrap_err();
        assert!(err.to_string().contains("desativada"), "{err}");
    }

    #[test]
    fn turning_off_the_active_tool_releases_the_pointer() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        app.ui.tool = Tool::Brush;
        app.session.edit_prefs(|p| p.features.tools = vec!["Brush".into()]);
        enforce(&mut app);
        assert_eq!(app.ui.tool, Tool::Move);
        assert!(app.drag.is_none());
    }
}
