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

    /// False for Arquivo and anything under it: that menu holds Salvar and Sair, which stay reachable.
    pub fn toggle_menu(&mut self, path: &str) -> bool {
        if path == "File" || path.starts_with("File/") {
            return false;
        }
        toggle(&mut self.menus, path);
        true
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

pub(crate) fn draft_id() -> egui::Id {
    egui::Id::unique("chrome-draft")
}

/// Salvar, Guardar como, Guardar uma cópia e Sair stay in Arquivo. The button in the title bar
/// is the way back when other menus are off.
const KEEP_SAVE: &str = "Salvar, Salvar como, Salvar uma cópia e Sair ficam sempre disponíveis.";

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
    let refused = {
        let Some(edit) = app.chrome_edit.as_mut() else { return };
        if let Some(path) = id.strip_prefix("menu:") {
            !edit.toggle_menu(path)
        } else if photocraft_engine::prefs::FeatureGates::essential(id) {
            true
        } else {
            edit.toggle_command(id);
            false
        }
    };
    if refused {
        app.ui.status = KEEP_SAVE.into();
        app.ui.status_error = true;
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
    use egui::{Key, Modifiers, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

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

    fn size_of(h: &Harness<'static, PhotocraftApp>) -> (u32, u32) {
        let doc = &h.state().session.active().unwrap().doc;
        (doc.size.width, doc.size.height)
    }

    fn shot(h: &mut Harness<'static, PhotocraftApp>, name: &str) {
        let img = h.render().unwrap_or_else(|e| panic!("render {name}: {e}"));
        let _ = std::fs::create_dir_all("/opt/cursor/artifacts");
        img.save(format!("/opt/cursor/artifacts/{name}.png")).unwrap_or_else(|e| panic!("save {name}: {e}"));
    }

    fn chrome(store: Rc<RefCell<Option<String>>>) -> Harness<'static, PhotocraftApp> {
        // The real app installs load/save before the first frame. `PhotocraftApp::new` reads
        // preferences then, which is the restart this test is checking.
        let load = store.clone();
        let save = store.clone();
        let services = crate::Services {
            export: Some(Box::new(|_, _, _| Err("o teste não grava o documento".into()))),
            load_prefs: Some(Box::new(move || load.borrow().clone())),
            save_prefs: Some(Box::new(move |text: &str| {
                *save.borrow_mut() = Some(text.to_string());
                Ok(())
            })),
            ..crate::Services::default()
        };
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
        app.run("file.new", json!({"width": 400, "height": 300})).unwrap();
        app.session.edit_prefs(|p| p.interface.language = "pt-br".into());
        app.ui.tool = Tool::Move;
        let mut h = Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(
            |ui, app: &mut PhotocraftApp| {
                // Preferences load on the first frame, before fonts are ready to draw.
                crate::prefs_ui::tick(app, ui.ctx());
                let ctx = ui.ctx().clone();
                if !ctx.fonts(|f| f.families().contains(&egui::FontFamily::Name("medium".into()))) {
                    return;
                }
                crate::i18n::set_current(crate::i18n::Lang::from_pref(&app.session.prefs().interface.language));
                crate::chrome_edit::enforce(app);
                crate::shortcuts::handle(app, ui.ctx());
                crate::panels::title_bar(app, ui);
                crate::panels::toolbar(app, ui);
                egui::CentralPanel::default().show(ui, |ui| crate::canvas::document_area(app, ui));
            },
            app,
        );
        PhotocraftApp::setup_context(&h.ctx, crate::theme::ThemeKind::ALL[0]);
        h.run_steps(6);
        h
    }

    fn click_label(h: &mut Harness<'static, PhotocraftApp>, label: &str) {
        let at = h.get_by_label(label).rect().center();
        h.hover_at(at);
        h.run_steps(1);
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.run_steps(1);
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(3);
    }

    fn hover_label(h: &mut Harness<'static, PhotocraftApp>, label: &str) {
        let at = h.get_by_label_contains(label).rect().center();
        h.hover_at(at);
        h.run_steps(10);
    }

    /// Personalizar, mark a toolbar tool and a nested menu command without running it,
    /// cancel, then OK. The choice survives a new process's preferences and comes back with Mostrar tudo.
    #[test]
    fn personalizar_marks_a_tool_and_a_nested_command_and_the_choice_survives_restart() {
        let store = Rc::new(RefCell::new(None));
        let mut h = chrome(store.clone());
        let before = size_of(&h);
        assert_eq!(before, (400, 300));
        let layers = h.state().session.active().unwrap().doc.layers.len();
        click_label(&mut h, "Personalizar");
        assert!(h.state().chrome_edit.is_some(), "Personalizar opens the marking mode");
        click_label(&mut h, "Ferramenta Pincel  (B)");
        assert!(h.state().chrome_edit.as_ref().unwrap().tool_marked(Tool::Brush), "the brush click marks the tool");
        assert_eq!(h.state().ui.tool, Tool::Move, "marking the brush does not switch to it");
        click_label(&mut h, "Imagem");
        hover_label(&mut h, "Rotação da imagem");
        click_label(&mut h, "180°");
        assert_eq!(size_of(&h), before, "marking 180° must not rotate the document");
        assert!(
            h.state().chrome_edit.as_ref().unwrap().commands.iter().any(|c| c == "image.imageRotation.180"),
            "the nested command is marked, draft {:?}",
            h.state().chrome_edit
        );
        h.key_press(Key::Escape);
        h.run_steps(2);
        click_label(&mut h, "Arquivo");
        click_label(&mut h, "Salvar Ctrl+S");
        assert!(!h.state().chrome_edit.as_ref().unwrap().commands.iter().any(|c| c == "file.save"), "Salvar cannot be marked");
        assert!(h.state().ui.status.contains("Salvar"), "the bar says save stays available: {}", h.state().ui.status);
        click_label(&mut h, "Cancelar");
        assert!(h.state().chrome_edit.is_none());
        assert!(h.state().session.prefs().features.tools.is_empty());
        assert!(h.state().session.prefs().features.commands.is_empty());
        click_label(&mut h, "Personalizar");
        click_label(&mut h, "Ferramenta Pincel  (B)");
        click_label(&mut h, "Imagem");
        hover_label(&mut h, "Rotação da imagem");
        click_label(&mut h, "180°");
        shot(&mut h, "customize-marking");
        click_label(&mut h, "OK");
        assert!(h.state().chrome_edit.is_none());
        h.run_steps(4);
        let saved = store.borrow().clone().expect("OK writes the preferences");
        assert!(saved.contains("image.imageRotation.180"), "{saved}");
        assert!(saved.contains("Brush"), "{saved}");
        assert!(!h.state().session.prefs().features.blocks_command("file.save", &["File"]));
        assert_eq!(h.state().session.active().unwrap().doc.layers.len(), layers);
        assert_eq!(size_of(&h), before);
        assert!(h.query_by_label("Ferramenta Pincel  (B)").is_none(), "the brush leaves the toolbar");
        h.key_press_modifiers(Modifiers::NONE, Key::B);
        h.run_steps(2);
        assert_ne!(h.state().ui.tool, Tool::Brush, "B must not select a deactivated brush");
        let err = h.state_mut().run("image.imageRotation.180", json!({})).unwrap_err();
        assert!(err.contains("desativada"), "{err}");
        assert_eq!(size_of(&h), before);
        shot(&mut h, "customize-off");
        drop(h);

        let mut check = photocraft_engine::Session::new();
        let invalid = check.load_prefs_json(&saved).unwrap_or_else(|e| panic!("reload: {e}"));
        assert!(
            check.prefs().features.commands.iter().any(|c| c == "image.imageRotation.180"),
            "stored preferences do not reload the command ({invalid:?} {:?})",
            check.prefs().features
        );
        let again_store = Rc::new(RefCell::new(Some(saved)));
        let mut again = chrome(again_store.clone());
        again.run_steps(4);
        let stored_now = again_store.borrow().clone().unwrap_or_default();
        assert!(
            again.state().session.prefs().features.blocks_command("image.imageRotation.180", &["Image", "Image Rotation"]),
            "a new launch reloads the choice, features {:?} status {:?} store has id {}",
            again.state().session.prefs().features,
            again.state().ui.status,
            stored_now.contains("image.imageRotation.180")
        );
        assert!(again.state().session.prefs().features.blocks_tool("Brush"));
        again.get_by_label("Personalizar");
        let err = again.state_mut().run("image.imageRotation.180", json!({})).unwrap_err();
        assert!(err.contains("desativada"), "{err}");
        click_label(&mut again, "Personalizar");
        click_label(&mut again, "Mostrar tudo");
        again.run_steps(2);
        assert!(again.state().session.prefs().features.commands.is_empty());
        assert!(again.state().session.prefs().features.tools.is_empty());
        click_label(&mut again, "Ferramenta Pincel  (B)");
        assert_eq!(again.state().ui.tool, Tool::Brush, "the brush runs again after Mostrar tudo");
        again.state_mut().run("image.imageRotation.180", json!({})).expect("180° runs again");
        assert_eq!(size_of(&again), before, "180° keeps the canvas size and does not resample the picture into a new document");
        shot(&mut again, "customize-restored");
    }
}
