use super::*;
#[derive(Resource, Default)]
pub(super) struct History {
    undo: Vec<Mission>,
    redo: Vec<Mission>,
    last: Option<Mission>,
}
impl History {
    pub fn observe(&mut self, m: &Mission, commit: bool) {
        if self.last.is_none() {
            self.last = Some(m.clone());
            return;
        }
        if commit
            && serde_json::to_string(self.last.as_ref().unwrap()).ok()
                != serde_json::to_string(m).ok()
        {
            self.undo.push(self.last.replace(m.clone()).unwrap());
            self.redo.clear();
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
        }
    }
    fn restore(lab: &mut Lab, m: Mission) {
        lab.draft = m;
        lab.geometry_dirty = true;
        lab.selected_stage = lab.selected_stage.min(lab.draft.stages.len() - 1);
        lab.selected_component = None;
        lab.status = "Draft restored; apply before simulating".into();
    }
    pub fn controls(&mut self, ui: &mut egui::Ui, lab: &mut Lab, busy: bool) {
        let undo = ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z));
        let redo = ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Y));
        ui.horizontal(|ui| {
            if !busy
                && (undo
                    || ui
                        .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                        .clicked())
                && let Some(m) = self.undo.pop()
            {
                self.redo.push(lab.draft.clone());
                self.last = Some(m.clone());
                Self::restore(lab, m);
            }
            if !busy
                && (redo
                    || ui
                        .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                        .clicked())
                && let Some(m) = self.redo.pop()
            {
                self.undo.push(lab.draft.clone());
                self.last = Some(m.clone());
                Self::restore(lab, m);
            }
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resetting_a_loaded_design_keeps_the_edited_draft_undoable() {
        let baseline = Mission::demo(false);
        let mut edited = baseline.clone();
        let mut h = History::default();
        h.observe(&baseline, true);
        edited.stages[0].length_m += 0.4;
        h.observe(&edited, true);
        h.observe(&baseline, true);
        assert_eq!(
            h.undo.last().unwrap().stages[0].length_m,
            edited.stages[0].length_m
        );
        assert_eq!(
            h.last.unwrap().stages[0].length_m,
            baseline.stages[0].length_m
        );
    }
    #[test]
    fn edit_groups_and_redo_branch() {
        let mut h = History::default();
        let mut m = Mission::demo(false);
        h.observe(&m, true);
        m.stages[0].length_m += 0.2;
        h.observe(&m, false);
        assert!(h.undo.is_empty());
        h.observe(&m, true);
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.undo[0].stages[0].length_m, 2.4);
        h.redo.push(m.clone());
        m.stages[0].length_m += 0.1;
        h.observe(&m, true);
        assert!(h.redo.is_empty());
    }
}
