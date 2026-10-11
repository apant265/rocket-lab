//! Explanations stay in tooltips rather than occupying the property panels.
use super::*;
pub(super) fn hint(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new("?").small().corner_radius(12))
        .on_hover_text(text)
}
pub(super) fn status(ui: &mut egui::Ui, text: &str) {
    if text.is_empty() {
        return;
    }
    let label = if text.to_lowercase().contains("fail") || text.to_lowercase().contains("error") {
        "Error"
    } else if text.starts_with("Saved") || text.contains("exported to") {
        "Saved"
    } else if text.starts_with("GPU") || text.starts_with("CPU") {
        "Result details"
    } else {
        "Status"
    };
    ui.small(label).on_hover_text(text);
}
