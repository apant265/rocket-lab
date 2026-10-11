use super::*;
use crate::{analysis, thermal};
#[derive(Resource, Default)]
pub(super) struct ThermalView {
    pub history: Option<thermal::History>,
    pub inner: bool,
    pub status: String,
    pub revision: Option<u32>,
    pub selected_stage: usize,
}
impl ThermalView {
    pub fn covers(&self, lab: &Lab) -> bool {
        self.revision == Some(lab.model_revision)
            && self
                .history
                .as_ref()
                .is_some_and(|h| h.times.last().is_some_and(|t| lab.time <= *t + 1e-9))
    }
}
pub(super) fn legend(
    ui: &mut egui::Ui,
    view: &mut ThermalView,
    lab: &Lab,
    w: &snapshot::Snapshots,
) {
    if view.revision != Some(lab.model_revision) || view.history.is_none() {
        return;
    }
    ui.checkbox(&mut view.inner, "Inner material temperatures");
    let time = w.view_time(lab).unwrap_or(lab.time);
    let stage = analysis::sample_at(&lab.flight, time).stage;
    let [low, high] = thermal_surface::scoped_range(
        view.history.as_ref().unwrap(),
        time,
        view.inner,
        stage,
        w.scope(),
    );
    ui.small(format!(
        "{} – {}",
        units::value(ui, "Temperature", low, "K", 2),
        units::value(ui, "Temperature", high, "K", 2)
    ));
    let (r, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
    for i in 0..100 {
        let c = surface::palette(i as f32 / 99.);
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(r.left() + r.width() * i as f32 / 100., r.top()),
                egui::pos2(r.left() + r.width() * (i + 1) as f32 / 100., r.bottom()),
            ),
            0.,
            egui::Color32::from_rgb(
                (255. * c[0]) as u8,
                (255. * c[1]) as u8,
                (255. * c[2]) as u8,
            ),
        );
    }
}
pub(super) fn show(ui: &mut egui::Ui, view: &mut ThermalView, lab: &Lab, w: &snapshot::Snapshots) {
    if !view.covers(lab) {
        help::hint(
            ui,
            "Compute thermal history through this timestamp to view material temperatures.",
        );
        return;
    }
    help::status(ui, &view.status);
    legend(ui, view, lab, w);
    let Some(h) = &view.history else {
        ui.spinner();
        return;
    };
    let stage = analysis::sample_at(&lab.flight, lab.time).stage;
    let [low, high] = thermal_surface::range(h, lab.time, view.inner, stage);
    ui.small("Outer and inner wall averages");
    let (r, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::hover());
    for i in 0..100 {
        let c = surface::palette(i as f32 / 99.0);
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(r.left() + r.width() * i as f32 / 100.0, r.top()),
                egui::pos2(r.left() + r.width() * (i + 1) as f32 / 100.0, r.bottom()),
            ),
            0.0,
            egui::Color32::from_rgb(
                (c[0] * 255.) as u8,
                (c[1] * 255.) as u8,
                (c[2] * 255.) as u8,
            ),
        );
    }
    egui::ComboBox::from_id_salt("thermal_stage")
        .selected_text(format!("Stage {}", view.selected_stage + 1))
        .show_ui(ui, |ui| {
            for (i, s) in lab.flight.mission.stages.iter().enumerate() {
                ui.selectable_value(
                    &mut view.selected_stage,
                    i,
                    format!("S{} · {}", i + 1, s.name),
                );
            }
        });
    view.selected_stage = view.selected_stage.min(lab.flight.mission.stages.len() - 1);
    let frame = h.frame(lab.time);
    ui.label("Heat propagation / outer → inner wall");
    let cols = h
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.stage == view.selected_stage && (c.kind == "body" || c.kind == "nose"))
        .collect::<Vec<_>>();
    let width = (ui.available_width() / cols.len().max(1) as f32).min(23.0);
    let (r, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 42.0), egui::Sense::hover());
    for (j, (i, c)) in cols.iter().enumerate() {
        for layer in 0..2 {
            let t = h.temperatures[frame][*i][layer];
            let color = surface::palette(((t - low) / (high - low)) as f32);
            let cell = egui::Rect::from_min_size(
                r.min + egui::vec2(j as f32 * width, layer as f32 * 20.0),
                egui::vec2(width - 1.0, 18.0),
            );
            ui.painter().rect_filled(
                cell,
                2.0,
                egui::Color32::from_rgb(
                    (color[0] * 255.) as u8,
                    (color[1] * 255.) as u8,
                    (color[2] * 255.) as u8,
                ),
            );
            ui.interact(cell, ui.id().with((j, layer)), egui::Sense::hover())
                .on_hover_text(format!(
                    "{} · {} · {}\n{} · k {}\nWall {}",
                    c.kind,
                    units::value(ui, "Station", c.station_m, "m", 3),
                    c.material.name,
                    units::value(ui, "Temperature", t, "K", 1),
                    units::value(
                        ui,
                        "Conductivity",
                        c.material.thermal.conductivity_w_m_k,
                        "W/(m·K)",
                        3
                    ),
                    units::value(ui, "Wall", c.thickness_m * 1000., "mm", 3)
                ));
        }
    }
    help::hint(ui, "Left = stage nose · top = outer · bottom = inner");
    egui::Grid::new("thermal_regions")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Region");
            let c = units::system(ui).conversion("Temperature", "°C");
            ui.strong(format!("Outer {}", c.unit));
            ui.strong(format!("Inner {}", c.unit));
            ui.strong(format!("Limit {}", c.unit));
            ui.end_row();
            let mut kinds = std::collections::BTreeSet::new();
            for c in h.columns.iter().filter(|c| c.stage == view.selected_stage) {
                kinds.insert(c.kind.as_str());
            }
            for kind in kinds {
                let selected = h
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.stage == view.selected_stage && c.kind == kind)
                    .collect::<Vec<_>>();
                let outer = selected
                    .iter()
                    .map(|(i, _)| h.temperatures[frame][*i][0])
                    .fold(0., f64::max);
                let inner = selected
                    .iter()
                    .map(|(i, _)| h.temperatures[frame][*i][1])
                    .fold(0., f64::max);
                let limit = selected
                    .iter()
                    .map(|(_, c)| c.material.thermal.limit_temperature_k)
                    .fold(f64::INFINITY, f64::min);
                ui.label(kind);
                ui.colored_label(
                    if outer > limit {
                        theme::palette(ui).danger
                    } else {
                        theme::palette(ui).text
                    },
                    format!("{:.1}", c.display(outer - 273.15)),
                );
                ui.label(format!("{:.1}", c.display(inner - 273.15)));
                ui.label(format!("{:.0}", c.display(limit - 273.15)));
                ui.end_row();
            }
        });
    if ui.button("Export thermal history CSV").clicked() {
        view.status = match h.export("output/thermal") {
            Ok(_) => "Thermal history exported to output/thermal/thermal.csv".into(),
            Err(e) => e.to_string(),
        };
    }
    help::hint(
        ui,
        "Estimated convection and material conduction; optional axial/through properties and motor heat. No resolved CFD wall heating, fracture or mass-loss feedback. Composite and rarefied-flow models need validation.",
    );
}
