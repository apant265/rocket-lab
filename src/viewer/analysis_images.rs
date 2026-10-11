//! Large 2D views of computed fields. The fin map is explicitly a span envelope.
use super::*;
#[path = "flow_geometry.rs"]
mod flow_geometry;
#[path = "flow_trails.rs"]
mod flow_trails;

use crate::viewer::analysis_fields::{self, FlowField};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ThermalImage {
    #[default]
    Vehicle,
    Wall,
    History,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum StressImage {
    #[default]
    Vehicle,
    Fin,
}
#[derive(Default)]
pub(super) struct Images {
    pub(super) field: FlowField,
    thermal_mode: ThermalImage,
    stress_mode: StressImage,
    pub(super) shape_stage: Option<usize>,
    shape_cache: Option<(String, thermal_surface::ShapeRaster)>,
    stage: usize,
    vectors: bool,
    motion_paused: bool,
    trails: Option<flow_trails::Trails>,
    zoom: f32,
    pan: egui::Vec2,
    cache: Option<(String, FieldMap, egui::TextureHandle)>,
    status: String,
}
impl Images {
    pub(in crate::viewer) fn field(&self) -> FlowField {
        self.field
    }
    pub(in crate::viewer) fn scope(&self) -> Option<usize> {
        self.shape_stage
    }
    pub(super) fn invalidate(&mut self) {
        self.cache = None;
        self.shape_cache = None;
        self.trails = None;
        self.status.clear();
    }
}
#[derive(Clone)]
struct FieldMap {
    width: usize,
    height: usize,
    vector_mesh: Option<egui::Mesh>,
    values: Vec<Option<f64>>,
    coverage: Option<Vec<bool>>,
    source_columns: Option<Vec<Option<usize>>>,
    flow_display: Option<Vec<Option<f64>>>,
    boundary: Option<flow_geometry::Boundary>,
    x: [f64; 2],
    y: [f64; 2],
    aspect: f32,
    title: String,
    unit: &'static str,
    x_label: String,
    y_label: String,
    range: [f64; 2],
    note: &'static str,
}
impl FieldMap {
    fn display_units(&mut self, system: units::System) {
        let c = system.conversion(&self.title, self.unit);
        if !c.unit.is_empty() {
            self.values
                .iter_mut()
                .flatten()
                .for_each(|v| *v = c.display(*v));
            if let Some(display) = &mut self.flow_display {
                display
                    .iter_mut()
                    .flatten()
                    .for_each(|v| *v = c.display(*v));
            }
            self.range = self.range.map(|v| c.display(v));
            self.unit = c.unit;
        }
        self.title = system.label(&self.title);
        let x_name = self
            .x_label
            .split_once(" / ")
            .map_or(self.x_label.as_str(), |p| p.0);
        let y_name = self
            .y_label
            .split_once(" / ")
            .map_or(self.y_label.as_str(), |p| p.0);
        let x = system.conversion(
            "analysis axis",
            self.x_label.split_once(" / ").map_or("", |p| p.1),
        );
        let y = system.conversion(
            "analysis axis",
            self.y_label.split_once(" / ").map_or("", |p| p.1),
        );
        self.x = self.x.map(|v| x.display(v));
        self.y = self.y.map(|v| y.display(v));
        if !x.unit.is_empty() {
            self.x_label = format!("{x_name} / {}", x.unit);
        }
        if !y.unit.is_empty() {
            self.y_label = format!("{y_name} / {}", y.unit);
        }
    }
    fn automatic_range(&mut self, zero: bool) {
        let low = self
            .values
            .iter()
            .flatten()
            .copied()
            .reduce(f64::min)
            .unwrap_or(0.0);
        let high = self
            .values
            .iter()
            .flatten()
            .copied()
            .reduce(f64::max)
            .unwrap_or(1.0);
        self.range = [if zero { 0.0 } else { low }, high];
        if self.range[1] - self.range[0] < 1e-9 {
            let delta = self.range[0].abs().max(1.0) * 0.01;
            self.range = if zero {
                [0.0, delta]
            } else {
                [self.range[0] - delta, self.range[1] + delta]
            };
        }
    }
    fn pixels(&self) -> Vec<egui::Color32> {
        self.display_pixels(true)
    }
    fn display_pixels(&self, include_boundary: bool) -> Vec<egui::Color32> {
        let radii = include_boundary
            .then(|| self.boundary.as_ref().map(|b| b.pixel_radii(self.width)))
            .flatten();
        self.flow_display
            .as_ref()
            .unwrap_or(&self.values)
            .iter()
            .enumerate()
            .map(|(i, value)| {
                if self.coverage.as_ref().is_some_and(|mask| !mask[i]) {
                    return egui::Color32::TRANSPARENT;
                }
                let color = value.map_or(egui::Color32::from_gray(80), |value| {
                    let c = surface::palette(
                        ((value - self.range[0]) / (self.range[1] - self.range[0])) as f32,
                    );
                    egui::Color32::from_rgb(
                        (255.0 * c[0]) as u8,
                        (255.0 * c[1]) as u8,
                        (255.0 * c[2]) as u8,
                    )
                });
                let fraction = radii.as_ref().map_or(0., |r| {
                    flow_geometry::Boundary::coverage(
                        r[i % self.width],
                        i / self.width,
                        self.height,
                    )
                });
                if fraction == 0. {
                    color
                } else {
                    let channels = color.to_array();
                    egui::Color32::from_rgb(
                        (channels[0] as f32 * (1. - fraction) + 80. * fraction).round() as u8,
                        (channels[1] as f32 * (1. - fraction) + 80. * fraction).round() as u8,
                        (channels[2] as f32 * (1. - fraction) + 80. * fraction).round() as u8,
                    )
                }
            })
            .collect()
    }
    fn value_at(&self, u: f64, v: f64) -> Option<f64> {
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
            return None;
        }
        self.values
            [(v * self.height as f64) as usize * self.width + (u * self.width as f64) as usize]
    }
}
/// Fill texture-only wall cells from the closest solved fluid row in the same
/// column. The continuous body hides its interior; numerical hover/CSV stay raw.
fn reconstruct_wall_display(map: &FieldMap) -> Vec<Option<f64>> {
    analysis_fields::fill_wall(&map.values, map.width, map.height)
}

fn flow_map(f: &crate::cfd::Solution, field: FlowField) -> FieldMap {
    let p = &f.problem;
    let planar = analysis_fields::Planar::new(f, field);
    let values = planar.values;
    let mut map = FieldMap {
        vector_mesh: None,
        flow_display: None,
        boundary: None,
        coverage: None,
        source_columns: None,
        width: p.nx,
        height: p.ny,
        values,
        x: [p.x0, p.x0 + p.nx as f64 * p.dx],
        y: [p.y0, p.y0 + p.ny as f64 * p.dy],
        aspect: (p.nx as f64 * p.dx / (p.ny as f64 * p.dy)) as f32,
        title: field.label().into(),
        unit: field.unit(),
        x_label: "Axial station / m".into(),
        y_label: "Crossflow position / m".into(),
        range: planar.range,
        note: "Computed planar field. 3D projects this same field onto the mesh; it does not add circumferential flow data.",
    };
    if field != FlowField::Cp {
        map.automatic_range(false);
    }
    map
}
fn thermal_map(
    h: &thermal::History,
    stage: usize,
    length: f64,
    time: f64,
    history: bool,
    inner: bool,
) -> Option<FieldMap> {
    let mut columns: Vec<_> = h
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.stage == stage && (c.kind == "body" || c.kind == "nose"))
        .collect();
    columns.sort_by(|a, b| a.1.station_m.total_cmp(&b.1.station_m));
    if columns.is_empty() || h.times.is_empty() {
        return None;
    }
    // Rasterize the solved axial columns with nearest-column sampling, no invented wall layers.
    let width = 256;
    let height = if history {
        h.times.len().clamp(2, 384)
    } else {
        2
    };
    let end = time.min(*h.times.last().unwrap()).max(0.0);
    let frame = h.frame(end);
    let indices: Vec<_> = (0..width)
        .map(|x| {
            let station = (x as f64 + 0.5) / width as f64 * length;
            columns
                .iter()
                .min_by(|a, b| {
                    (a.1.station_m - station)
                        .abs()
                        .total_cmp(&(b.1.station_m - station).abs())
                })
                .unwrap()
                .0
        })
        .collect();
    let mut values = Vec::with_capacity(width * height);
    for y in 0..height {
        let (f, layer) = if history {
            (
                h.frame(end * (1.0 - (y as f64 + 0.5) / height as f64)),
                usize::from(inner),
            )
        } else {
            (frame, y)
        };
        for &i in &indices {
            values.push(Some(h.temperatures[f][i][layer] - 273.15));
        }
    }
    let mut map = FieldMap {
        vector_mesh: None,
        flow_display: None,
        boundary: None,
        coverage: None,
        source_columns: None,
        width,
        height,
        values,
        x: [0.0, length],
        y: if history { [0.0, end] } else { [0.0, 2.0] },
        aspect: 2.8,
        title: if history {
            format!(
                "S{} · {} wall temperature history",
                stage + 1,
                if inner { "inner" } else { "outer" }
            )
        } else {
            format!("S{} · heat propagation through the wall", stage + 1)
        },
        unit: "°C",
        x_label: "Station aft stage front / m".into(),
        y_label: if history {
            "Time since launch / s"
        } else {
            "Outer / inner half-wall averages"
        }
        .into(),
        range: [0.0, 1.0],
        note: if history {
            "Solved heating history through the viewed time. Axial columns are sampled without interpolation."
        } else {
            "Two displayed bands: outer half-wall average above, inner half-wall average below. Gray means no data."
        },
    };
    map.automatic_range(false);
    Some(map)
}
fn stress_map(
    rows: &[analysis::StressPoint],
    stage: usize,
    g: &crate::config::StageGeometry,
) -> Option<FieldMap> {
    let rows: Vec<_> = rows.iter().filter(|r| r.stage == stage).collect();
    if rows.is_empty() {
        return None;
    }
    let width = 256;
    let height = 192;
    let chord_extent = g.fin_root_chord_m.max(g.fin_sweep_m + g.fin_tip_chord_m);
    let mut values = Vec::with_capacity(width * height);
    for j in 0..height {
        let fraction = 1.0 - (j as f64 + 0.5) / height as f64;
        let lead = g.fin_sweep_m * fraction;
        let chord = g.fin_root_chord_m + (g.fin_tip_chord_m - g.fin_root_chord_m) * fraction;
        let row = rows
            .iter()
            .min_by(|a, b| {
                (a.span_fraction - fraction)
                    .abs()
                    .total_cmp(&(b.span_fraction - fraction).abs())
            })
            .unwrap();
        for i in 0..width {
            let x = (i as f64 + 0.5) / width as f64 * chord_extent;
            values.push((x >= lead && x <= lead + chord).then_some(row.stress_pa / 1e6));
        }
    }
    let mut map = FieldMap {
        vector_mesh: None,
        flow_display: None,
        boundary: None,
        coverage: None,
        source_columns: None,
        width,
        height,
        values,
        x: [0.0, chord_extent],
        y: [0.0, g.fin_span_m],
        aspect: (chord_extent / g.fin_span_m) as f32,
        title: format!("S{} · fin stress span envelope", stage + 1),
        unit: "MPa",
        x_label: "Aft of root leading edge / m".into(),
        y_label: "Fin span from root / m".into(),
        range: [0.0, 1.0],
        note: "Span-wise envelope on the primary trapezoidal fin. Color is constant across each chord; this is not a resolved chordwise FEA contour.",
    };
    map.automatic_range(true);
    Some(map)
}
fn vehicle_map(
    raster: &thermal_surface::ShapeRaster,
    h: &thermal::History,
    time: f64,
    inner: bool,
) -> FieldMap {
    let mut map = FieldMap {
        vector_mesh: None,
        width: raster.width,
        height: raster.height,
        values: {
            let values: Vec<_> = (0..h.columns.len())
                .map(|i| Some(thermal_surface::temperature(h, i, time, inner) - 273.15))
                .collect();
            raster
                .blends
                .iter()
                .map(|b| b.and_then(|b| b.value(&values)))
                .collect()
        },
        flow_display: None,
        boundary: None,
        coverage: Some(raster.covered.clone()),
        source_columns: Some(raster.columns.clone()),
        x: raster.x,
        y: raster.y,
        aspect: ((raster.x[1] - raster.x[0]) / (raster.y[1] - raster.y[0])) as f32,
        title: format!(
            "Vehicle temperature · {} · T+ {time:.3} s",
            if inner { "inner" } else { "outer" }
        ),
        unit: "°C",
        x_label: "Station aft vehicle nose / m".into(),
        y_label: "Lateral position / m".into(),
        range: [0., 1.],
        note: "Exported mesh shape colored by computed material regions. Gray = uncomputed parts. Circumferential temperature is uniform within each region.",
    };
    map.range = thermal_surface::scoped_range(h, time, inner, raster.first_stage, raster.only)
        .map(|t| t - 273.15);
    let values = (0..h.columns.len())
        .map(|i| Some(thermal_surface::temperature(h, i, time, inner) - 273.15))
        .collect::<Vec<_>>();
    map.vector_mesh = Some(vehicle_vectors(raster, &values, map.range));
    map
}
fn save(map: &FieldMap, key: Key, stage: usize, viewed_time: f64) -> Result<String, String> {
    let dir = std::path::Path::new("output/analysis-images");
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let kind = map
        .title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let path = dir.join(format!(
        "r{}-t{:016x}-s{}-{}-{}.png",
        key.revision,
        viewed_time.to_bits(),
        stage + 1,
        key.detail.label().to_lowercase(),
        kind
    ));
    let bytes: Vec<u8> = map.pixels().iter().flat_map(|c| c.to_array()).collect();
    image::RgbaImage::from_raw(map.width as u32, map.height as u32, bytes)
        .ok_or("Invalid image size")?
        .save(&path)
        .map_err(|e| e.to_string())?;
    let metadata = serde_json::json!({"title":map.title,"unit":map.unit,"x_extent":map.x,"y_extent":map.y,"x_label":map.x_label,"y_label":map.y_label,
        "range":map.range,"width":map.width,"height":map.height,"captured_time_s":key.time,"viewed_time_s":viewed_time,"backend":key.backend.label(),"note":map.note,
        "row_order":"top to bottom; high to low y except wall-band view (outer then inner)","masked_rgba":[80,80,80,255],"outside_vehicle_rgba":map.coverage.as_ref().map(|_| [0,0,0,0])});
    std::fs::write(
        path.with_extension("json"),
        serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(format!("Saved {} and matching metadata", path.display()))
}
fn vehicle_stress_map(
    raster: &thermal_surface::ShapeRaster,
    field: &crate::vehicle_stress::Field,
) -> FieldMap {
    let mut map = FieldMap {
        vector_mesh: None,
        width: raster.width,
        height: raster.height,
        values: {
            let values = stress_values(raster, field);
            raster
                .blends
                .iter()
                .map(|b| b.and_then(|b| b.value(&values)))
                .collect()
        },
        flow_display: None,
        boundary: None,
        coverage: Some(raster.covered.clone()),
        source_columns: Some(raster.columns.clone()),
        x: raster.x,
        y: raster.y,
        aspect: ((raster.x[1] - raster.x[0]) / (raster.y[1] - raster.y[0])) as f32,
        title: "Vehicle stress · section envelope".into(),
        unit: "MPa",
        x_label: "Station aft vehicle nose / m".into(),
        y_label: "Lateral position / m".into(),
        range: [0., 1.],
        note: "Peak section stress on the exported shape. Gray = unresolved mounts or hardware. CPU beam/shell screening, or CalculiX bonded S8 shell and separate primary fin solids. Circumferential peaks are shown uniformly; contact, buckling, laminate failure and flutter are unresolved.",
    };
    map.range = [0., (field.peak() / 1e6).max(1e-9)];
    map.vector_mesh = Some(vehicle_vectors(
        raster,
        &stress_values(raster, field),
        map.range,
    ));
    map
}
fn stress_values(
    raster: &thermal_surface::ShapeRaster,
    field: &crate::vehicle_stress::Field,
) -> Vec<Option<f64>> {
    let count = raster
        .faces
        .iter()
        .flat_map(|(_, bs)| bs.iter().flatten())
        .map(|b| b.a.max(b.b))
        .max()
        .map_or(0, |i| i + 1);
    (0..count)
        .map(|i| field.at_column(i).map(|p| p.stress_pa / 1e6))
        .collect()
}
fn vehicle_vectors(
    raster: &thermal_surface::ShapeRaster,
    values: &[Option<f64>],
    range: [f64; 2],
) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    for (vertices, blends) in &raster.faces {
        for (v, b) in vertices.iter().zip(blends) {
            let color =
                b.and_then(|b| b.value(values))
                    .map_or(egui::Color32::from_gray(80), |value| {
                        let c =
                            surface::palette(((value - range[0]) / (range[1] - range[0])) as f32);
                        egui::Color32::from_rgb(
                            (c[0] * 255.) as u8,
                            (c[1] * 255.) as u8,
                            (c[2] * 255.) as u8,
                        )
                    });
            mesh.indices.push(mesh.vertices.len() as u32);
            mesh.vertices.push(egui::epaint::Vertex {
                pos: egui::pos2(
                    ((-v[1] - raster.x[0]) / (raster.x[1] - raster.x[0])) as f32,
                    ((raster.y[1] - v[0]) / (raster.y[1] - raster.y[0])) as f32,
                ),
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
    }
    mesh
}
pub(super) fn show(
    ui: &mut egui::Ui,
    state: &mut Images,
    w: &Snapshots,
    lab: &Lab,
    thermal_view: &mut thermal_view::ThermalView,
) {
    ui.horizontal(|ui| {
        ui.heading("2D analysis");
        ui.label(format!("{} · T+ {:.3} s", w.mode.label(), lab.time));
    });
    let Some(key) = w
        .completed
        .filter(|k| k.revision == lab.model_revision && k.mode == w.mode)
    else {
        ui.add_space(24.0);
        ui.label(if w.busy() {
            "Computing…"
        } else {
            "No analysis result"
        }).on_hover_text("Choose CFD, Thermal or Stress in the inspector, then Compute to display its 2D result.");
        help::status(ui, &w.report);
        return;
    };
    ui.small(format!(
        "Captured T+ {:.3} s · {} · {} detail",
        key.time,
        key.backend.label(),
        key.detail.label()
    ));
    if key != Key::new(w, lab) {
        ui.colored_label(theme::palette(ui).warning, "Captured result")
            .on_hover_text("Compute to update for changed time or settings.");
    }
    let mut save_requested = false;
    ui.horizontal_wrapped(|ui| {
        if key.mode == Mode::Flow {
            for field in [
                FlowField::Cp,
                FlowField::Pressure,
                FlowField::Speed,
                FlowField::Mach,
            ] {
                ui.selectable_value(
                    &mut state.field,
                    field,
                    units::system(ui).label(field.label()),
                );
            }
            ui.checkbox(&mut state.vectors, "Velocity arrows");
            let mut motion = !state.motion_paused;
            ui.checkbox(&mut motion, "Flow motion").on_hover_text("Animated tracers advect through the computed velocity field. Display playback is slowed; this captured field is not a transient CFD animation.");
            state.motion_paused = !motion;
        } else {
            if key.mode == Mode::Stress {
                ui.selectable_value(
                    &mut state.stress_mode,
                    StressImage::Vehicle,
                    "Vehicle shape",
                );
                ui.selectable_value(&mut state.stress_mode, StressImage::Fin, "Fin envelope");
            }
            if key.mode == Mode::Thermal {
                ui.selectable_value(
                    &mut state.thermal_mode,
                    ThermalImage::Vehicle,
                    "Vehicle shape",
                );
                ui.selectable_value(&mut state.thermal_mode, ThermalImage::Wall, "Wall section");
                ui.selectable_value(
                    &mut state.thermal_mode,
                    ThermalImage::History,
                    "Heating history",
                );
                if state.thermal_mode != ThermalImage::Wall {
                    ui.checkbox(&mut thermal_view.inner, "Inner wall");
                }
            }
            if (key.mode == Mode::Thermal && state.thermal_mode == ThermalImage::Vehicle)
                || (key.mode == Mode::Stress && state.stress_mode == StressImage::Vehicle)
            {
                egui::ComboBox::from_id_salt("thermal_shape_scope")
                    .selected_text(
                        state
                            .shape_stage
                            .map_or("Retained vehicle".into(), |i| format!("Stage {}", i + 1)),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut state.shape_stage, None, "Retained vehicle");
                        for (i, s) in lab.flight.mission.stages.iter().enumerate() {
                            ui.selectable_value(
                                &mut state.shape_stage,
                                Some(i),
                                format!("S{} · {}", i + 1, s.name),
                            );
                        }
                    });
            } else {
                egui::ComboBox::from_id_salt("image_stage")
                    .selected_text(format!("Stage {}", state.stage + 1))
                    .show_ui(ui, |ui| {
                        for (i, s) in lab.flight.mission.stages.iter().enumerate() {
                            if key.mode != Mode::Stress || w.stress.iter().any(|r| r.stage == i) {
                                ui.selectable_value(
                                    &mut state.stage,
                                    i,
                                    format!("S{} · {}", i + 1, s.name),
                                );
                            }
                        }
                    });
            }
        }
        if ui.button("Fit image").clicked() {
            state.zoom = 1.0;
            state.pan = egui::Vec2::ZERO;
        }
        save_requested = ui.button("Save PNG + metadata").clicked();
    });
    if key.mode == Mode::Flow {
        state.stage = analysis::sample_at(&lab.flight, key.time).stage;
    }
    state.stage = state.stage.min(lab.flight.mission.stages.len() - 1);
    if key.mode == Mode::Stress && !w.stress.iter().any(|r| r.stage == state.stage) {
        state.stage = w.stress.first().map_or(0, |r| r.stage);
    }
    let viewed_time = lab.time.min(key.time);
    if key.mode == Mode::Thermal {
        ui.small(format!("Thermal field through T+ {viewed_time:.3} s"));
    }
    let signature = format!(
        "{key:?}-{:?}-{}-{:?}-{:?}-{}-{:016x}-{:?}-{:?}",
        state.field,
        state.stage,
        state.thermal_mode,
        state.shape_stage,
        thermal_view.inner,
        if key.mode == Mode::Thermal {
            viewed_time.to_bits()
        } else {
            0
        },
        units::system(ui),
        state.stress_mode
    );
    if state.cache.as_ref().is_none_or(|c| c.0 != signature) {
        let map = match key.mode {
            Mode::Flow => w.flow.as_ref().map(|f| {
                let mut map = flow_map(f,state.field);
                map.boundary = Some(flow_geometry::Boundary::new(&lab.flight.mission,state.stage,&f.problem));
                map.flow_display = Some(reconstruct_wall_display(&map));
                map.note = "Computed planar solver field with continuous rocket silhouette. The solver wall remains Cartesian. Display-only near-wall values sample neighboring fluid cells; hover and raw CSV retain the solved-cell mask. 3D projects this same planar field onto the mesh; fin and circumferential flow remain unresolved.";
                map
            }),
            Mode::Thermal => thermal_view
                .history
                .as_ref()
                .filter(|_| thermal_view.revision == Some(key.revision))
                .and_then(|h| {
                    if state.thermal_mode == ThermalImage::Vehicle {
                        let active = analysis::sample_at(&lab.flight, viewed_time).stage;
                        let scope = format!("{key:?}-{active}-{:?}", state.shape_stage);
                        if state.shape_cache.as_ref().is_none_or(|c| c.0 != scope) {
                            state.shape_cache = lab
                                .model
                                .as_ref()
                                .and_then(|model| {
                                    thermal_surface::ShapeRaster::new(
                                        model,
                                        &lab.flight.mission,
                                        h,
                                        active,
                                        state.shape_stage,
                                    )
                                })
                                .map(|r| (scope, r));
                        }
                        return state.shape_cache.as_ref().map(|(_, r)| {
                            let mut map = vehicle_map(r, h, viewed_time, thermal_view.inner);
                            map.title.push_str(
                                &state.shape_stage.map_or(" · retained vehicle".into(), |i| {
                                    format!(" · stage {}", i + 1)
                                }),
                            );
                            map
                        });
                    }
                    thermal_map(
                        h,
                        state.stage,
                        lab.flight.mission.stages[state.stage].length_m,
                        viewed_time,
                        state.thermal_mode == ThermalImage::History,
                        thermal_view.inner,
                    )
                }),
            Mode::Stress => {
                if state.stress_mode == StressImage::Vehicle {
                    thermal_view
                        .history
                        .as_ref()
                        .filter(|_| thermal_view.revision == Some(key.revision))
                        .and_then(|h| {
                            let active = analysis::sample_at(&lab.flight, key.time).stage;
                            let scope = format!("{key:?}-{active}-{:?}", state.shape_stage);
                            if state.shape_cache.as_ref().is_none_or(|c| c.0 != scope) {
                                state.shape_cache = lab
                                    .model
                                    .as_ref()
                                    .and_then(|model| {
                                        thermal_surface::ShapeRaster::new(
                                            model,
                                            &lab.flight.mission,
                                            h,
                                            active,
                                            state.shape_stage,
                                        )
                                    })
                                    .map(|r| (scope, r));
                            }
                            state
                                .shape_cache
                                .as_ref()
                                .zip(w.vehicle_stress.as_ref())
                                .map(|((_, r), f)| vehicle_stress_map(r, f))
                        })
                } else {
                    lab.flight.mission.stages[state.stage]
                        .geometry
                        .as_ref()
                        .and_then(|g| stress_map(&w.stress, state.stage, g))
                }
            }
        };
        if let Some(mut map) = map {
            map.display_units(units::system(ui));
            let tex = ui.ctx().load_texture(
                "analysis_2d",
                if map.vector_mesh.is_some() {
                    // Vehicle geometry is painted as triangles. Keep a tiny
                    // unused texture handle; PNG exports retain the 4K raster.
                    egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE])
                } else {
                    egui::ColorImage::new(
                        [map.width, map.height],
                        map.display_pixels(key.mode != Mode::Flow),
                    )
                },
                egui::TextureOptions::LINEAR,
            );
            state.cache = Some((signature, map, tex));
            state.zoom = 1.0;
            state.pan = egui::Vec2::ZERO;
        } else {
            state.cache = None;
            ui.label("No vehicle field").on_hover_text("Export a matching 3D model for the vehicle shape. Wall, history and fin envelope views are available without a mesh.");
            return;
        }
    }
    let Some((_, map, tex)) = &state.cache else {
        return;
    };
    if save_requested {
        state.status = save(
            map,
            key,
            state.shape_stage.unwrap_or(state.stage),
            if key.mode == Mode::Thermal {
                viewed_time
            } else {
                key.time
            },
        )
        .unwrap_or_else(|e| e);
    }
    ui.label(&map.title);
    help::hint(ui, map.note);
    ui.small("Wheel · zoom   Drag · pan   Hover · inspect values");
    if !state.status.is_empty() {
        help::status(ui, &state.status);
    }
    let colors = theme::palette(ui);
    let size = egui::vec2(
        ui.available_width().max(80.0),
        (ui.available_height() - 50.0).max(90.0),
    );
    let (canvas, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let plot = egui::Rect::from_min_max(
        canvas.min + egui::vec2(58.0, 8.0),
        canvas.max - egui::vec2(12.0, 30.0),
    );
    let fit = egui::vec2(
        plot.width().min(plot.height() * map.aspect),
        plot.height().min(plot.width() / map.aspect),
    );
    if state.zoom <= 0.0 {
        state.zoom = 1.0;
    }
    if response.dragged() {
        state.pan += ui.input(|i| i.pointer.delta());
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let old = state.zoom;
            state.zoom = (state.zoom * (scroll * 0.003).exp()).clamp(0.5, 16.0);
            if let Some(pointer) = response.hover_pos() {
                let anchor = pointer - plot.center();
                state.pan = anchor - (anchor - state.pan) * (state.zoom / old);
            }
        }
    }
    let r = egui::Rect::from_center_size(plot.center() + state.pan, fit * state.zoom);
    let painter = ui.painter().with_clip_rect(plot);
    painter.rect_filled(plot, 0.0, colors.surface);
    if let Some(vector_mesh) = &map.vector_mesh {
        let mut mesh = vector_mesh.clone();
        for v in &mut mesh.vertices {
            v.pos = r.min + v.pos.to_vec2() * r.size();
        }
        painter.add(egui::Shape::mesh(mesh));
    } else {
        painter.image(
            tex.id(),
            r,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    if key.mode == Mode::Flow
        && !state.motion_paused
        && let Some(flow) = &w.flow
    {
        let trails = state
            .trails
            .get_or_insert_with(|| flow_trails::Trails::new(flow));
        trails.paint(&painter, r, ui.input(|i| i.time));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
    if key.mode == Mode::Flow
        && state.vectors
        && let Some(f) = &w.flow
    {
        let p = &f.problem;
        let stride = (p.nx / 28).max(4);
        for j in (0..p.ny).step_by(stride) {
            for i in (0..p.nx).step_by(stride) {
                let n = j * p.nx + i;
                if p.solid[n] {
                    continue;
                }
                let c = f.cells[n];
                let length = c[1].hypot(c[2]).max(1e-12);
                painter.arrow(
                    egui::pos2(
                        r.left() + (i as f32 + 0.5) / p.nx as f32 * r.width(),
                        r.bottom() - (j as f32 + 0.5) / p.ny as f32 * r.height(),
                    ),
                    egui::vec2((c[1] / length) as f32 * 8.0, -(c[2] / length) as f32 * 8.0),
                    egui::Stroke::new(0.7, egui::Color32::WHITE),
                );
            }
        }
    }
    if key.mode == Mode::Flow
        && let Some(boundary) = &map.boundary
    {
        boundary.paint(&painter, r);
    }
    let font = egui::FontId::proportional(11.0);
    let axis_y = r.bottom().clamp(plot.top() + 12.0, plot.bottom());
    ui.painter().text(
        egui::pos2(plot.center().x, axis_y + 29.0),
        egui::Align2::CENTER_BOTTOM,
        &map.x_label,
        font.clone(),
        colors.text,
    );
    for i in 0..=4 {
        let t = i as f32 / 4.0;
        let x = r.left() + r.width() * t;
        if x >= plot.left() && x <= plot.right() {
            ui.painter().text(
                egui::pos2(x, axis_y + 3.0),
                egui::Align2::CENTER_TOP,
                format!("{:.2}", map.x[0] + (map.x[1] - map.x[0]) * t as f64),
                font.clone(),
                colors.muted,
            );
        }
        let y = r.bottom() - r.height() * t;
        if y >= plot.top()
            && y <= plot.bottom()
            && (key.mode != Mode::Thermal || state.thermal_mode != ThermalImage::Wall)
        {
            ui.painter().text(
                egui::pos2(plot.left() - 5.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("{:.2}", map.y[0] + (map.y[1] - map.y[0]) * t as f64),
                font.clone(),
                colors.muted,
            );
        }
    }
    if key.mode == Mode::Thermal && state.thermal_mode == ThermalImage::Wall {
        for (v, label) in [(0.25, "Outer"), (0.75, "Inner")] {
            let y = r.top() + r.height() * v;
            if plot.contains(egui::pos2(plot.center().x, y)) {
                ui.painter().text(
                    egui::pos2(plot.left() - 5.0, y),
                    egui::Align2::RIGHT_CENTER,
                    label,
                    font.clone(),
                    colors.muted,
                );
            }
        }
    }
    if let Some(pointer) = response
        .hover_pos()
        .filter(|p| plot.contains(*p) && r.contains(*p))
    {
        let u = ((pointer.x - r.left()) / r.width()) as f64;
        let v = ((pointer.y - r.top()) / r.height()) as f64;
        let text = if let Some(value) = map.value_at(u, v) {
            let station = map.x[0] + u * (map.x[1] - map.x[0]);
            let ordinate = if key.mode == Mode::Thermal && state.thermal_mode == ThermalImage::Wall
            {
                if v < 0.5 {
                    "Outer half-wall average".into()
                } else {
                    "Inner half-wall average".into()
                }
            } else {
                format!(
                    "{} = {:.4}",
                    map.y_label,
                    map.y[1] - v * (map.y[1] - map.y[0])
                )
            };
            let mut text = format!(
                "{}: {:.4} {}\n{} = {:.4}\n{}",
                map.title, value, map.unit, map.x_label, station, ordinate
            );
            if (key.mode == Mode::Thermal || key.mode == Mode::Stress)
                && let Some(h) = &thermal_view.history
                && let Some(c) = if let Some(columns) = &map.source_columns {
                    columns[(v * map.height as f64) as usize * map.width
                        + (u * map.width as f64) as usize]
                        .and_then(|i| h.columns.get(i))
                } else {
                    h.columns
                        .iter()
                        .filter(|c| {
                            c.stage == state.stage && (c.kind == "body" || c.kind == "nose")
                        })
                        .min_by(|a, b| {
                            (a.station_m
                                - units::system(ui)
                                    .conversion("analysis axis", "m")
                                    .stored(station))
                            .abs()
                            .total_cmp(
                                &(b.station_m
                                    - units::system(ui)
                                        .conversion("analysis axis", "m")
                                        .stored(station))
                                .abs(),
                            )
                        })
                }
            {
                text.push_str(&format!(
                    "\nS{} · {} · {}\nWall {} · k {}",
                    c.stage + 1,
                    c.kind,
                    c.material.name,
                    units::value(ui, "Wall", c.thickness_m * 1000., "mm", 3),
                    units::value(
                        ui,
                        "Conductivity",
                        c.material.thermal.conductivity_w_m_k,
                        "W/(m·K)",
                        3
                    )
                ));
                if key.mode == Mode::Stress
                    && let Some(columns) = &map.source_columns
                    && let Some(index) = columns[(v * map.height as f64) as usize * map.width
                        + (u * map.width as f64) as usize]
                    && let Some(p) = w.vehicle_stress.as_ref().and_then(|f| f.at_column(index))
                {
                    text.push_str(&format!(
                        "\nStress / E: {}\nStrength / stress: {}\nTemperature: {}",
                        p.strain
                            .map_or("Needs modulus".into(), |v| format!("{v:.6}")),
                        p.margin
                            .map_or("Needs strength".into(), |v| format!("{v:.3}")),
                        units::value(ui, "Temperature", p.temperature_k, "K", 2)
                    ));
                }
            }
            text
        } else {
            "Masked / no field value".into()
        };
        response.on_hover_text_at_pointer(text);
    }
    ui.horizontal(|ui| {
        ui.small(format!("{:.3} {}", map.range[0], map.unit));
        let (r, _) = ui.allocate_exact_size(
            egui::vec2((ui.available_width() - 110.0).max(80.0), 12.0),
            egui::Sense::hover(),
        );
        for i in 0..128 {
            let c = surface::palette(i as f32 / 127.0);
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(r.left() + r.width() * i as f32 / 128.0, r.top()),
                    egui::pos2(r.left() + r.width() * (i + 1) as f32 / 128.0, r.bottom()),
                ),
                0.0,
                egui::Color32::from_rgb(
                    (c[0] * 255.0) as u8,
                    (c[1] * 255.0) as u8,
                    (c[2] * 255.0) as u8,
                ),
            );
        }
        ui.small(format!("{:.3} {}", map.range[1], map.unit));
    });
    ui.small(&map.y_label);
}

pub(super) fn export_check(state: &Images, w: &Snapshots, lab: &Lab, path: &str) {
    let (_, map, _) = state.cache.as_ref().expect("2D field texture created");
    let key = w.completed.expect("computed image");
    let time = if key.mode == Mode::Thermal {
        lab.time.min(key.time)
    } else {
        key.time
    };
    let file = save(map, key, state.stage, time).expect("export PNG");
    std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"full_size_2d_view":w.image_view,
        "title":map.title,"width":map.width,"height":map.height,"range":map.range,"unit":map.unit,
        "computed_cells":map.values.iter().flatten().count(),"export":file,"captured_time_s":key.time,"viewed_time_s":time})).unwrap()).unwrap();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flow_image_uses_actual_values_flips_rows_and_masks_the_vehicle() {
        let cells = vec![
            crate::cfd::conserved(2.0, 3.0, 4.0, 100_000.0),
            crate::cfd::conserved(2.0, 0.0, 0.0, 200_000.0),
            crate::cfd::conserved(2.0, 6.0, 8.0, 300_000.0),
            crate::cfd::conserved(2.0, 0.0, 0.0, 400_000.0),
        ];
        let f = crate::cfd::Solution {
            problem: crate::cfd::Problem {
                nx: 2,
                ny: 2,
                dx: 2.0,
                dy: 1.0,
                x0: -1.0,
                y0: -0.5,
                solid: vec![false, true, false, false],
                far: cells[0],
                steps: 1,
                dt: 0.1,
                mach: 1.0,
                alpha_deg: 0.0,
            },
            cells,
            residuals: vec![],
            backend: "reference".into(),
        };
        let map = flow_map(&f, FlowField::Pressure);
        assert_eq!(map.x, [-1.0, 3.0]);
        assert_eq!(map.y, [-0.5, 1.5]);
        assert_eq!(map.aspect, 2.0);
        assert_eq!(
            map.values,
            vec![Some(300.0), Some(400.0), Some(100.0), None]
        );
        assert_eq!(map.value_at(0.25, 0.75), Some(100.0));
        assert_eq!(flow_map(&f, FlowField::Speed).values[0], Some(10.0));
        let cp = flow_map(&f, FlowField::Cp);
        assert_eq!(cp.values[2], Some(0.0));
        assert_eq!(map.pixels()[3], egui::Color32::from_gray(80));
        assert_eq!(reconstruct_wall_display(&map)[3], Some(400.));
        assert_eq!(map.values[3], None);
        assert!(map.value_at(1.0, 0.0).is_none());
        for field in [
            FlowField::Cp,
            FlowField::Pressure,
            FlowField::Speed,
            FlowField::Mach,
        ] {
            let mut imperial_map = flow_map(&f, field);
            imperial_map.display_units(units::System::Imperial);
            assert_eq!(
                imperial_map.title,
                units::System::Imperial.label(field.label())
            );
            let map = flow_map(&f, field);
            let planar = analysis_fields::Planar::new(&f, field);
            assert_eq!(map.range, planar.range);
            assert_eq!(planar.range, analysis_fields::ranges(&f)[field.index()]);
            let reconstructed = reconstruct_wall_display(&map);
            for row in 0..2 {
                for col in 0..2 {
                    let vertex = [
                        map.y[1] - (row as f64 + 0.5),
                        -(map.x[0] + (col as f64 + 0.5) * 2.),
                        0.,
                    ];
                    assert_eq!(planar.at(vertex), reconstructed[row * 2 + col]);
                    assert_eq!(planar.at([vertex[0], vertex[1], 9.]), planar.at(vertex));
                }
            }
        }
    }
    #[test]
    fn spatial_sampling_interpolates_inside_material_and_keeps_interfaces() {
        let mut h = history();
        let mission = Mission::demo(false);
        h.columns[0].axial_bounds_m = Some([0., 0.5]);
        h.columns[1].axial_bounds_m = Some([0.5, 1.]);
        let sample = |h: &thermal::History, x: f64| {
            thermal_surface::Mapping::new(h, &mission, 0, "body", None)
                .at([0., -x, 0.])
                .unwrap()
                .value(&[Some(300.), Some(500.)])
                .unwrap()
        };
        assert_eq!(sample(&h, 0.5), 400.);
        assert_eq!(sample(&h, 0.4), 360.);
        // Different properties with the same label still define a material interface.
        h.columns[1].material.thermal.conductivity_w_m_k *= 2.;
        assert_eq!(sample(&h, 0.49), 300.);
        assert_eq!(sample(&h, 0.51), 500.);
        assert!(
            thermal_surface::Mapping::new(&h, &mission, 0, "unsupported", None)
                .at([0., -0.5, 0.])
                .is_none()
        );
    }
    fn history() -> thermal::History {
        let material = crate::assembly::Material::presets().remove(0);
        let columns = [0.25, 0.75].map(|station| thermal::Column {
            stage: 0,
            kind: "body".into(),
            station_m: station,
            axial_bounds_m: None,
            radial_fraction: 0.0,
            material: material.clone(),
            thickness_m: 0.002,
            area_m2: 1.0,
            capacity_j_k: 1.0,
            radial_w_k: 1.0,
            links: vec![],
            incidence: 1.0,
            exposed: true,
        });
        thermal::History {
            columns: columns.to_vec(),
            times: vec![0.0, 10.0],
            temperatures: vec![
                vec![[288.15, 288.15], [288.15, 288.15]],
                vec![[373.15, 293.15], [473.15, 303.15]],
            ],
            backend: "reference".into(),
        }
    }
    #[test]
    fn vehicle_image_uses_mesh_silhouette_material_values_and_transparent_background() {
        use crate::surface::MeshPart;
        let mut mission = Mission::demo(false);
        mission.stages[0].length_m = 1.;
        mission.stages[0].diameter_m = 0.1;
        let mut h = history();
        for (kind, station, temperature) in [("nose", 0.1, 350.), ("fin", 0.8, 400.)] {
            let mut c = h.columns[0].clone();
            c.kind = kind.into();
            c.station_m = station;
            h.columns.push(c);
            h.temperatures[0].push([288.15, 288.15]);
            h.temperatures[1].push([temperature, 300.]);
        }
        let part = |kind: &str, positions: Vec<[f32; 3]>, indices: Vec<u32>| MeshPart {
            component_index: None,
            name: kind.into(),
            stage: 0,
            kind: kind.into(),
            base_color: [1.; 3],
            normals: vec![],
            positions,
            indices,
        };
        let model = MeshDocument {
            schema_version: 1,
            physics_version: 0,
            generator: "test".into(),
            stages: vec![],
            parts: vec![
                part(
                    "body",
                    vec![
                        [0.05, -0.2, 0.],
                        [-0.05, -0.2, 0.],
                        [-0.05, -1., 0.],
                        [0.05, -1., 0.],
                    ],
                    vec![0, 1, 2, 0, 2, 3],
                ),
                part(
                    "nose",
                    vec![[0., 0., 0.], [-0.05, -0.2, 0.], [0.05, -0.2, 0.]],
                    vec![0, 1, 2],
                ),
                part(
                    "fin",
                    vec![[0.05, -0.6, 0.], [0.3, -0.9, 0.], [0.05, -0.9, 0.]],
                    vec![0, 1, 2],
                ),
                part(
                    "unsupported",
                    vec![
                        [-0.2, -0.5, 0.],
                        [-0.1, -0.5, 0.],
                        [-0.1, -0.7, 0.],
                        [-0.2, -0.7, 0.],
                    ],
                    vec![0, 1, 2, 0, 2, 3],
                ),
            ],
        };
        let raster = thermal_surface::ShapeRaster::new(&model, &mission, &h, 0, None).unwrap();
        let map = vehicle_map(&raster, &h, 5., false);
        let inner = vehicle_map(&raster, &h, 5., true);
        assert!(raster.covered.iter().any(|covered| !covered));
        assert!(
            raster.columns.contains(&Some(2)),
            "nose temperature must be mapped"
        );
        assert!(
            raster.columns.contains(&Some(3)),
            "fin temperature must be mapped"
        );
        let pixels = map.pixels();
        let structural = crate::vehicle_stress::Field {
            points: (0..h.columns.len())
                .map(|i| crate::vehicle_stress::Point {
                    column: i,
                    stage: 0,
                    kind: h.columns[i].kind.clone(),
                    station_m: h.columns[i].station_m,
                    radial_fraction: 0.,
                    material: h.columns[i].material.name.clone(),
                    temperature_k: 300.,
                    stress_pa: (i + 1) as f64 * 1e6,
                    axial_pa: 0.,
                    bending_pa: 0.,
                    hoop_pa: 0.,
                    shear_pa: 0.,
                    strain: None,
                    margin: None,
                    deflection_m: None,
                })
                .collect(),
            ..Default::default()
        };
        let mut stress = vehicle_stress_map(&raster, &structural);
        let stress_samples: Vec<_> = (0..h.columns.len()).map(|i| Some((i + 1) as f64)).collect();
        for (i, blend) in raster.blends.iter().enumerate() {
            assert_eq!(
                stress.values[i],
                blend.and_then(|b| b.value(&stress_samples))
            );
        }
        assert!(map.vector_mesh.as_ref().unwrap().vertices.len() > 1000);
        assert!(map.values.iter().flatten().any(|v| *v > 65. && *v < 75.));
        assert_eq!(
            map.range,
            thermal_surface::range(&h, 5., false, 0).map(|t| t - 273.15)
        );
        assert_eq!(stress.range, [0., 4.]);
        stress.display_units(units::System::Imperial);
        assert_eq!(stress.unit, "ksi");
        assert!(stress.x_label.ends_with("ft") && stress.y_label.ends_with("ft"));
        assert!((stress.x[1] / stress.y[1] - raster.x[1] / raster.y[1]).abs() < 1e-9);
        assert!((stress.range[1] - 4e3 / 6894.757293168).abs() < 1e-9);
        for (i, column) in raster.columns.iter().enumerate() {
            if let Some(column) = column {
                let _ = column;
                let samples: Vec<_> = (0..h.columns.len())
                    .map(|i| Some(thermal_surface::temperature(&h, i, 5., false) - 273.15))
                    .collect();
                let expected = raster.blends[i].unwrap().value(&samples).unwrap();
                assert!((map.values[i].unwrap() - expected).abs() < 1e-10);
                let samples: Vec<_> = (0..h.columns.len())
                    .map(|i| Some(thermal_surface::temperature(&h, i, 5., true) - 273.15))
                    .collect();
                let expected_inner = raster.blends[i].unwrap().value(&samples).unwrap();
                assert!((inner.values[i].unwrap() - expected_inner).abs() < 1e-10);
            } else if raster.covered[i] {
                assert_eq!(pixels[i], egui::Color32::from_gray(80));
            } else {
                assert_eq!(pixels[i], egui::Color32::TRANSPARENT);
            }
        }
        assert!(
            raster
                .covered
                .iter()
                .zip(&raster.columns)
                .any(|(covered, col)| *covered && col.is_none())
        );
        // A detached stage must disappear from the retained vehicle silhouette.
        let mut staged = model.clone();
        staged.parts[1].stage = 1;
        mission.stages.push(mission.stages[0].clone());
        let retained = thermal_surface::ShapeRaster::new(&staged, &mission, &h, 1, None).unwrap();
        assert!(retained.x[1] < 0.3);
        assert!(thermal_surface::ShapeRaster::new(&model, &mission, &h, 1, None).is_none());
    }
    #[test]
    fn wall_bands_keep_outer_and_inner_averages_and_history_excludes_future_heat() {
        let h = history();
        let wall = thermal_map(&h, 0, 1.0, 10.0, false, false).unwrap();
        assert_eq!(wall.height, 2);
        assert_eq!(wall.values[0], Some(100.0));
        assert_eq!(wall.values[255], Some(200.0));
        assert_eq!(wall.values[256], Some(20.0));
        assert_eq!(wall.values[511], Some(30.0));
        let past = thermal_map(&h, 0, 1.0, 5.0, true, false).unwrap();
        assert_eq!(past.y, [0.0, 5.0]);
        assert!(past.values.iter().all(|v| *v == Some(15.0)));
        assert!(thermal_map(&h, 1, 1.0, 10.0, false, false).is_none());
    }
    #[test]
    fn stress_map_masks_outside_the_fin_and_preserves_span_envelope() {
        let s = Mission::demo(false).stages.remove(0);
        let g = crate::config::StageGeometry::generic(&s, true);
        let rows = vec![
            analysis::StressPoint {
                stage: 0,
                span_fraction: 0.0,
                stress_pa: 10e6,
                temperature_k: 288.15,
                deflection_m: None,
                margin: None,
            },
            analysis::StressPoint {
                stage: 0,
                span_fraction: 1.0,
                stress_pa: 1e6,
                temperature_k: 288.15,
                deflection_m: None,
                margin: None,
            },
        ];
        let map = stress_map(&rows, 0, &g).unwrap();
        assert!(map.values.iter().any(|v| v.is_none()));
        assert!(map.values.contains(&Some(10.0)));
        assert!(map.values.contains(&Some(1.0)));
        assert_eq!(map.range, [0.0, 10.0]);
        assert!(stress_map(&rows, 1, &g).is_none());
        assert!(map.note.contains("not a resolved chordwise FEA contour"));
        let mut unloaded = rows.clone();
        for row in &mut unloaded {
            row.stress_pa = 0.0;
        }
        let zero = stress_map(&unloaded, 0, &g).unwrap();
        assert_eq!(zero.range[0], 0.0);
        assert!(zero.range[1] > 0.0);
    }
    #[test]
    fn png_export_retains_pixels_units_and_the_viewed_thermal_timestamp() {
        let map = thermal_map(&history(), 0, 1.0, 5.0, false, false).unwrap();
        let key = Key {
            revision: u32::MAX,
            time: 10.0,
            mode: Mode::Thermal,
            detail: Detail::Preview,
            gpu: false,
            backend: Backend::Preview,
            poisson: 0.3,
        };
        let message = save(&map, key, 0, 5.0).unwrap();
        let path = message
            .strip_prefix("Saved ")
            .unwrap()
            .strip_suffix(" and matching metadata")
            .unwrap();
        let png = image::open(path).unwrap().to_rgba8();
        assert_eq!(png.dimensions(), (256, 2));
        assert_eq!(png.get_pixel(0, 0).0, map.pixels()[0].to_array());
        let metadata: serde_json::Value = serde_json::from_slice(
            &std::fs::read(std::path::Path::new(path).with_extension("json")).unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["viewed_time_s"], 5.0);
        assert_eq!(metadata["captured_time_s"], 10.0);
        assert_eq!(metadata["unit"], "°C");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(std::path::Path::new(path).with_extension("json")).unwrap();
    }
}
