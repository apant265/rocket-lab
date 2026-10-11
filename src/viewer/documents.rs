use super::*;
use crate::project;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum Action {
    New,
    Open,
    Save,
    SaveAs,
}
struct Pending {
    mission: Mission,
    path: Option<PathBuf>,
}

#[derive(Resource, Default)]
pub(super) struct Documents {
    path: Option<PathBuf>,
    saved: Option<String>,
    initialized: bool,
    action: Option<Action>,
    pending: Option<Pending>,
    confirm: bool,
    new_open: bool,
    name: String,
}

impl Documents {
    fn dirty(&self, mission: &Mission) -> bool {
        self.saved
            .as_ref()
            .is_none_or(|saved| *saved != project::signature(mission))
    }
    pub fn controls(&mut self, ui: &mut egui::Ui, enabled: bool) {
        ui.add_enabled_ui(enabled && !self.new_open && self.pending.is_none(), |ui| {
            ui.menu_button("File", |ui| {
                for (action, label, shortcut, hint) in [
                    (Action::New, "New rocket…", "Ctrl+N", "Create a named single-stage design with editable body, nose and fins."),
                    (Action::Open, "Open rocket…", "Ctrl+O", "Load a Rocket Lab mission JSON file, including saved parts, motors and launch conditions."),
                    (Action::Save, "Save", "Ctrl+S", "Save the editable design. Model geometry embedded in parts is included."),
                    (Action::SaveAs, "Save as…", "Ctrl+Shift+S", "Save a separate copy as a Rocket Lab JSON file."),
                ] {
                    if ui.add(egui::Button::new(label).shortcut_text(shortcut)).on_hover_text(hint).clicked() {
                        self.action = Some(action);
                        ui.close();
                    }
                }
                if let Some(path) = &self.path {
                    ui.separator();
                    ui.add(egui::Label::new(path.file_name().unwrap_or_default().to_string_lossy()).truncate()).on_hover_text(path.display().to_string());
                }
            });
        });
    }
    fn save(&mut self, lab: &mut Lab, save_as: bool) -> bool {
        let chosen = if save_as || self.path.is_none() {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Save rocket")
                .add_filter("Rocket Lab design", &["json"]);
            if let Some(path) = &self.path {
                if let Some(parent) = path.parent() {
                    dialog = dialog.set_directory(parent);
                }
                dialog =
                    dialog.set_file_name(path.file_name().unwrap_or_default().to_string_lossy());
            } else {
                dialog = dialog.set_file_name("rocket.json");
            }
            dialog.save_file()
        } else {
            self.path.clone()
        };
        let Some(mut path) = chosen else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("json");
        }
        if !is_rocket(&path) {
            lab.status = "Save rocket as .json; use Export for model formats".into();
            return false;
        }
        match project::save(&path, &lab.draft) {
            Ok(()) => {
                self.saved = Some(project::signature(&lab.draft));
                lab.reset_mission = lab.draft.clone();
                lab.status = format!("Saved {}", path.display());
                self.path = Some(path);
                true
            }
            Err(error) => {
                lab.status = format!("Could not save rocket: {error}");
                false
            }
        }
    }
    fn queue(&mut self, mission: Mission, path: Option<PathBuf>, current: &Mission) {
        self.confirm = self.dirty(current);
        self.pending = Some(Pending { mission, path });
    }
    fn open(&mut self, path: PathBuf, lab: &mut Lab) {
        match project::load(&path) {
            Ok(mission) => self.queue(mission, Some(path), &lab.draft),
            Err(error) => lab.status = format!("Could not open rocket: {error}"),
        }
    }
    /// Returns true when a different document has been installed.
    pub fn show(&mut self, ctx: &egui::Context, lab: &mut Lab, enabled: bool) -> bool {
        if !self.initialized {
            self.saved = Some(project::signature(&lab.draft));
            self.initialized = true;
        }
        if !enabled {
            return false;
        }
        if !self.new_open && self.pending.is_none() {
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::S) {
                    self.action = Some(Action::SaveAs);
                } else if i.consume_key(egui::Modifiers::CTRL, egui::Key::S) {
                    self.action = Some(Action::Save);
                } else if i.consume_key(egui::Modifiers::CTRL, egui::Key::O) {
                    self.action = Some(Action::Open);
                } else if i.consume_key(egui::Modifiers::CTRL, egui::Key::N) {
                    self.action = Some(Action::New);
                }
            });
            if let Some(path) = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .find(|p| is_rocket(p))
            }) {
                self.open(path, lab);
            }
        }
        if let Some(action) = self.action.take() {
            match action {
                Action::New => {
                    self.name = "Untitled rocket".into();
                    self.new_open = true;
                }
                Action::Open => {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Open rocket")
                        .add_filter("Rocket Lab design", &["json"])
                        .pick_file()
                    {
                        self.open(path, lab);
                    }
                }
                Action::Save => {
                    self.save(lab, false);
                }
                Action::SaveAs => {
                    self.save(lab, true);
                }
            }
        }
        if self.new_open {
            let mut open = true;
            let mut create = false;
            let mut cancel = false;
            egui::Window::new("New rocket").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.horizontal(|ui| { ui.label("Name"); ui.text_edit_singleline(&mut self.name); });
                ui.label("Single stage · Body · Nose · Fins").on_hover_text("A fresh editable design. Choose the actual motor and configure materials, mass and recovery before launch.");
                ui.horizontal(|ui| {
                    create = ui.add_enabled(!self.name.trim().is_empty(), egui::Button::new("Create")).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
            self.new_open = open && !cancel;
            if create {
                match project::new_rocket(&self.name) {
                    Ok(mission) => {
                        self.new_open = false;
                        self.queue(mission, None, &lab.draft);
                    }
                    Err(error) => lab.status = error.to_string(),
                }
            }
        }
        if self.pending.is_some() && self.confirm {
            let mut open = true;
            let mut save = false;
            let mut discard = false;
            let mut cancel = false;
            egui::Window::new("Unsaved changes")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(&lab.draft.name);
                    ui.horizontal(|ui| {
                        save = ui.button("Save & continue").clicked();
                        discard = ui.button("Discard changes").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            if cancel || !open {
                self.pending = None;
            } else if discard || (save && self.save(lab, false)) {
                self.confirm = false;
            }
        }
        if !self.confirm
            && let Some(pending) = self.pending.take()
        {
            // Prepare the new preview before changing any current document state.
            match Simulator::new(pending.mission).map(Simulator::into_result) {
                Ok(flight) => {
                    lab.model = None;
                    install_flight(lab, flight);
                    lab.reset_mission = lab.draft.clone();
                    lab.workspace = 0;
                    lab.selected_stage = lab.draft.stages.len() - 1;
                    lab.selected_part = 0;
                    lab.selected_component = None;
                    lab.geometry_dirty = lab.model.is_none();
                    lab.preview_recovery = false;
                    lab.show_internals = false;
                    lab.editor_open = false;
                    lab.editor_text = serde_json::to_string_pretty(&lab.draft).unwrap();
                    lab.reference.clear();
                    lab.motor_search.clear();
                    lab.dragging = false;
                    lab.drag_cursor = None;
                    lab.left_press = None;
                    lab.left_was_drag = false;
                    lab.speed = 10.;
                    lab.tab = 0;
                    reset_camera(lab);
                    self.saved = pending
                        .path
                        .as_ref()
                        .map(|_| project::signature(&lab.draft));
                    lab.status = pending.path.as_ref().map_or_else(
                        || "New rocket · edit parts and choose a motor".into(),
                        |p| format!("Opened {}", p.display()),
                    );
                    self.path = pending.path;
                    return true;
                }
                Err(error) => lab.status = format!("Could not open rocket: {error}"),
            }
        }
        false
    }
}
fn is_rocket(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_an_edited_document_requires_a_save_decision() {
        let mut m = project::new_rocket("Existing").unwrap();
        let mut docs = Documents {
            saved: Some(project::signature(&m)),
            ..Default::default()
        };
        assert!(!docs.dirty(&m));
        m.stages[0].components[0].length_m += 0.1;
        docs.queue(project::new_rocket("Next").unwrap(), None, &m);
        assert!(docs.confirm);
        assert_eq!(docs.pending.as_ref().unwrap().mission.name, "Next");
        assert_eq!(m.name, "Existing");
    }
}
