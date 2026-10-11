#[path = "analysis_images.rs"]
mod images;
use super::*;
use crate::{analysis, thermal};
use bevy::render::renderer::{RenderAdapterInfo, RenderDevice, RenderQueue};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Backend {
    OpenFoam,
    Preview,
}
impl Backend {
    fn label(self) -> &'static str {
        match self {
            Self::OpenFoam => "External solvers (CPU)",
            Self::Preview => "Built-in analysis",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mode {
    Flow,
    Thermal,
    Stress,
}
impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Flow => "CFD",
            Self::Thermal => "Thermal",
            Self::Stress => "Stress",
        }
    }
}
pub(super) use crate::analysis_detail::Detail;
#[derive(Clone, Copy, PartialEq, Debug)]
struct Key {
    revision: u32,
    time: f64,
    mode: Mode,
    detail: Detail,
    gpu: bool,
    backend: Backend,
    poisson: f64,
}
impl Key {
    fn new(w: &Snapshots, lab: &Lab) -> Self {
        Self {
            revision: lab.model_revision,
            time: lab.time,
            mode: w.mode,
            detail: w.detail,
            gpu: w.gpu,
            backend: w.backend,
            poisson: w.poisson,
        }
    }
}
struct ResultData {
    key: Key,
    flow: Option<crate::cfd::Solution>,
    heat: Option<thermal::History>,
    stress: Vec<analysis::StressPoint>,
    vehicle_stress: Option<crate::vehicle_stress::Field>,
    report: String,
}
#[derive(Resource)]
pub(super) struct Snapshots {
    pub backend: Backend,
    pub poisson: f64,
    pub mode: Mode,
    pub detail: Detail,
    gpu: bool,
    progress: Option<crate::progress::Progress>,
    adapter_name: String,
    hardware_gpu: bool,
    requested: Option<(Key, FlightResult)>,
    task: Option<std::thread::JoinHandle<Result<ResultData, String>>>,
    completed: Option<Key>,
    flow: Option<crate::cfd::Solution>,
    texture: Option<egui::TextureHandle>,
    pub image_view: bool,
    images: images::Images,
    pub stress: Vec<analysis::StressPoint>,
    pub vehicle_stress: Option<crate::vehicle_stress::Field>,
    report: String,
}
impl Default for Snapshots {
    fn default() -> Self {
        Self {
            backend: Backend::Preview,
            poisson: 0.3,
            mode: Mode::Flow,
            detail: Detail::Standard,
            gpu: true,
            progress: None,
            adapter_name: String::new(),
            hardware_gpu: false,
            requested: None,
            task: None,
            completed: None,
            flow: None,
            texture: None,
            image_view: true,
            images: images::Images::default(),
            stress: Vec::new(),
            vehicle_stress: None,
            report: "Select a time, choose detail, then compute.".into(),
        }
    }
}
impl Snapshots {
    pub fn opening() -> Self {
        Self {
            detail: Detail::Preview,
            ..Self::default()
        }
    }
    pub fn preview() -> Self {
        Self {
            backend: Backend::Preview,
            ..Self::default()
        }
    }
    pub fn busy(&self) -> bool {
        self.task.is_some() || self.requested.is_some()
    }
    pub fn request(&mut self, lab: &Lab) {
        if self.busy() {
            return;
        }
        self.requested = Some((Key::new(self, lab), lab.flight.clone()));
        self.progress = Some(crate::progress::Progress::new(
            "Preparing analysis",
            format!(
                "analysis-{:?}-{:?}-{:?}-{}-{}",
                self.backend, self.mode, self.detail, self.gpu, self.adapter_name
            ),
        ));
        self.completed = None;
        self.flow = None;
        self.texture = None;
        self.images.invalidate();
        self.stress.clear();
        self.vehicle_stress = None;
        self.report = format!("Computing {} at T+ {:.3} s…", self.mode.label(), lab.time);
    }
    pub fn stress_matches(&self, lab: &Lab) -> bool {
        self.completed.is_some_and(|k| {
            k.mode == Mode::Stress
                && k.revision == lab.model_revision
                && (k.time - lab.time).abs() < 1e-7
        })
    }
    pub fn external_matches(&self, lab: &Lab, mode: Mode) -> bool {
        self.completed.is_some_and(|k| {
            k.backend == Backend::OpenFoam
                && k.mode == mode
                && k.revision == lab.model_revision
                && (k.time - lab.time).abs() < 1e-7
        })
    }
}
pub(super) fn poll(
    mut w: ResMut<Snapshots>,
    mut thermal_view: ResMut<thermal_view::ThermalView>,
    lab: Res<Lab>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    info: Res<RenderAdapterInfo>,
) {
    w.adapter_name = info.name.clone();
    w.hardware_gpu = info.device_type != wgpu::DeviceType::Cpu;
    if w.task.as_ref().is_some_and(|t| t.is_finished()) {
        let result = w.task.take().unwrap().join();
        if let Some(progress) = w.progress.take() {
            progress.finish(matches!(&result, Ok(Ok(r)) if r.key.revision == lab.model_revision));
        }
        match result {
            Ok(Ok(mut r)) if r.key.revision == lab.model_revision => {
                if let Some(h) = r.heat.take() {
                    thermal_view.history = Some(h);
                    thermal_view.revision = Some(r.key.revision);
                    thermal_view.status = r.report.clone();
                }
                w.completed = Some(r.key);
                w.flow = r.flow;
                w.texture = None;
                w.images.invalidate();
                w.stress = r.stress;
                w.vehicle_stress = r.vehicle_stress;
                w.report = r.report;
            }
            Ok(Ok(_)) => {
                w.report =
                    "Design changed while computing; discarded the previous design's result.".into()
            }
            Ok(Err(e)) => w.report = e,
            Err(_) => w.report = "Analysis worker failed.".into(),
        }
    }
    if w.task.is_none()
        && let Some((key, flight)) = w.requested.take()
    {
        if key.mode != Mode::Flow {
            thermal_view.history = None;
            thermal_view.revision = None;
            thermal_view.status = "Computing captured heating history…".into();
        }
        let device = device.wgpu_device().clone();
        let queue = (**queue.0).clone();
        let name = info.name.clone();
        let hardware_gpu = w.hardware_gpu;
        let progress = w
            .progress
            .as_ref()
            .expect("requested analysis progress")
            .clone();
        w.task = Some(std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let directory = format!(
                "output/snapshots/r{}-t{:016x}-{}-{}-{:?}",
                key.revision,
                key.time.to_bits(),
                key.mode.label().to_lowercase(),
                key.detail.label().to_lowercase(),
                key.backend
            );
            std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
            let sample = analysis::sample_at(&flight, key.time);
            let mut out = ResultData {
                key,
                flow: None,
                heat: None,
                stress: Vec::new(),
                vehicle_stress: None,
                report: String::new(),
            };
            let mut metadata = serde_json::json!({"timestamp_s":sample.time_s,"stage":sample.stage+1,"revision":key.revision,"analysis":key.mode.label(),"detail":key.detail.label(),"gpu_requested":key.gpu,"gpu_adapter":name});
            metadata["requested_backend"] = key.backend.label().into();
            if key.backend == Backend::OpenFoam {
                metadata["gpu_requested"] = false.into();
            }
            if key.mode == Mode::Flow {
                let (grid, steps) = key.detail.flow();
                let problem = crate::cfd::prepare(
                    &flight.mission,
                    crate::cfd::Request {
                        stage: sample.stage,
                        altitude_m: sample.altitude_m,
                        mach: sample.mach,
                        alpha_deg: sample.alpha_deg,
                        grid,
                        steps,
                    },
                )
                .map_err(|e| e.to_string())?;
                let flow = if key.backend == Backend::OpenFoam {
                    progress.stage("OpenFOAM CFD · CPU · meshing and solving", 0.0);
                    crate::external::flow(problem, std::path::Path::new(&directory))
                        .map_err(|e| e.to_string())?
                } else if key.gpu {
                    crate::cfd_gpu::solve_with_progress(problem, |done, total, adapter| {
                        if done == 0 {
                            progress.stage(format!("CFD · GPU {adapter}"), total as f64);
                        }
                        progress.update(done as f64);
                    })
                    .map_err(|e| format!("GPU CFD failed: {e}"))?
                } else {
                    progress.stage("CFD · CPU", steps as f64);
                    crate::cfd::solve_with_progress(problem, |done, _| progress.update(done as f64))
                        .map_err(|e| e.to_string())?
                };
                progress.stage("Saving CFD results", 0.0);
                flow.export(&directory).map_err(|e| e.to_string())?;
                let residual = flow.residuals.last().map(|r| r.1);
                out.report = format!(
                    "{} · {} × {} cells · {} iterations · {}",
                    flow.backend,
                    grid[0],
                    grid[1],
                    steps,
                    residual.map_or_else(
                        || "solver logs saved with case".into(),
                        |r| format!("residual {r:.2e}")
                    )
                );
                metadata["grid"] = serde_json::json!(grid);
                metadata["iterations"] = steps.into();
                metadata["density_residual"] = serde_json::json!(residual);
                metadata["backend"] = flow.backend.clone().into();
                metadata["gpu_used"] = flow.backend.starts_with("GPU").into();
                out.flow = Some(flow);
            } else {
                let prefix = analysis::prefix(&flight, key.time, key.detail.boundary_step());
                let resolution = key.detail.thermal();
                let problem = thermal::prepare_with_resolution(&prefix, resolution);
                let mut note = String::new();
                let wall_layers = key.detail.wall_layers();
                let heat = if key.backend == Backend::OpenFoam {
                    progress.stage("OpenFOAM wall diffusion · CPU", 0.0);
                    crate::external::heat(
                        &prefix,
                        resolution,
                        wall_layers,
                        key.detail.boundary_step(),
                        &std::path::Path::new(&directory).join("thermal"),
                    )
                    .map_err(|e| e.to_string())?
                } else if key.gpu {
                    if !hardware_gpu {
                        return Err("GPU thermal requires a hardware GPU; the renderer is using a CPU adapter.".into());
                    }
                    progress.stage(format!("Thermal · GPU {name}"), problem.times.len() as f64);
                    match thermal_gpu::solve_with_progress(
                        &device,
                        &queue,
                        &problem,
                        &name,
                        |done, _| progress.update(done as f64),
                    ) {
                        Ok(h) => {
                            progress.stage("Verifying GPU temperatures against CPU reference", 0.0);
                            let mut p = problem.clone();
                            p.times.truncate(64);
                            p.boundaries.truncate(64);
                            let reference = thermal::solve(&p);
                            let error = reference
                                .temperatures
                                .iter()
                                .flatten()
                                .flatten()
                                .zip(h.temperatures.iter().take(64).flatten().flatten())
                                .map(|(a, b)| (a - b).abs())
                                .fold(0.0, f64::max);
                            metadata["gpu_cpu_initial_max_error_k"] = error.into();
                            if error > 0.25 {
                                return Err(format!(
                                    "GPU thermal failed accuracy check: Δ{error:.3} K exceeds 0.25 K."
                                ));
                            } else {
                                note = format!("GPU/CPU Δ≤{error:.4} K. ");
                                h
                            }
                        }
                        Err(e) => {
                            return Err(format!("GPU thermal failed: {e}"));
                        }
                    }
                } else {
                    progress.stage("Thermal · CPU", problem.times.len() as f64);
                    thermal::solve_with_progress(&problem, |done, _| progress.update(done as f64))
                };
                progress.stage("Saving thermal results", 0.0);
                heat.export(&directory).map_err(|e| e.to_string())?;
                metadata["thermal_resolution"] = serde_json::json!(resolution);
                metadata["diffusion_max_step_s"] = problem.max_step.into();
                metadata["boundary_max_step_s"] = key.detail.boundary_step().into();
                metadata["columns"] = heat.columns.len().into();
                metadata["frames"] = heat.times.len().into();
                metadata["backend"] = heat.backend.clone().into();
                metadata["gpu_used"] = heat.backend.starts_with("GPU").into();
                metadata["wall_cells"] = if key.backend == Backend::OpenFoam {
                    wall_layers
                } else {
                    2
                }
                .into();
                out.report = format!(
                    "{note}{} · {} columns · diffusion Δt ≤ {:.5} s · history 0–{:.3} s",
                    heat.backend,
                    heat.columns.len(),
                    problem.max_step,
                    key.time
                );
                if key.mode == Mode::Stress {
                    progress.stage("Vehicle stress · CPU", 0.0);
                    out.stress = if key.backend == Backend::OpenFoam {
                        let (rows, backend) = crate::external::stress(
                            &prefix,
                            &heat,
                            key.detail.stress(),
                            key.poisson,
                            &std::path::Path::new(&directory).join("stress"),
                        )
                        .map_err(|e| e.to_string())?;
                        metadata["stress_backend"] = backend.clone().into();
                        metadata["poisson_ratio"] = key.poisson.into();
                        out.report.push_str(&format!(" · {backend}"));
                        rows
                    } else {
                        metadata["stress_backend"] = "CPU beam integration".into();
                        out.report.push_str(" · CPU beam stress");
                        analysis::fin_stress(&prefix, &heat, key.detail.stress())
                    };
                    std::fs::write(
                        format!("{directory}/stress.json"),
                        serde_json::to_vec_pretty(&out.stress).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    metadata["fin_integration_divisions"] = key.detail.stress().into();
                    let mut field = if key.backend == Backend::OpenFoam {
                        crate::external::vehicle_stress(
                            &prefix,
                            &heat,
                            key.detail.stress(),
                            &std::path::Path::new(&directory).join("vehicle-stress"),
                        )
                        .map_err(|e| e.to_string())?
                    } else {
                        crate::vehicle_stress::solve(&prefix, &heat, key.detail.stress())
                    };
                    crate::vehicle_stress::merge_primary_fins(
                        &prefix,
                        &heat,
                        &mut field,
                        &out.stress,
                    );
                    if key.backend == Backend::OpenFoam {
                        field.backend.push_str(
                            "; separate C3D20R primary fins, extra fins use beam screening",
                        );
                    }
                    metadata["vehicle_stress_backend"] = field.backend.clone().into();
                    metadata["vehicle_stress_regions"] = field.points.len().into();
                    std::fs::write(
                        format!("{directory}/vehicle-stress.json"),
                        serde_json::to_vec_pretty(&field).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    out.report
                        .push_str(&format!(" · {} structural regions", field.points.len()));
                    out.vehicle_stress = Some(field);
                    out.report
                        .push_str(&format!(" · {} fin span divisions", key.detail.stress()));
                }
                out.heat = Some(heat);
            }
            metadata["elapsed_s"] = start.elapsed().as_secs_f64().into();
            progress.stage("Saving analysis metadata", 0.0);
            metadata["status"] = out.report.clone().into();
            std::fs::write(
                format!("{directory}/snapshot.json"),
                serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            out.report
                .push_str(&format!(" · {:.2} s", start.elapsed().as_secs_f64()));
            Ok(out)
        }));
    }
}
pub(super) fn controls(ui: &mut egui::Ui, w: &mut Snapshots, lab: &mut Lab) {
    ui.strong("ANALYSIS AT TIMESTAMP");
    ui.label(format!("T+ {:.3} s · retained vehicle", lab.time));
    ui.horizontal(|ui| {
        ui.selectable_value(&mut w.backend, Backend::OpenFoam, "External (CPU)");
        ui.selectable_value(&mut w.backend, Backend::Preview, "Built-in");
    });
    if w.backend == Backend::OpenFoam {
        ui.small("OpenFOAM v2406 + CalculiX · Docker · CPU");
    }
    ui.horizontal(|ui| {
        for mode in [Mode::Flow, Mode::Thermal, Mode::Stress] {
            if ui
                .selectable_value(&mut w.mode, mode, mode.label())
                .clicked()
            {
                lab.tab = 6;
                lab.surface_mode = match mode {
                    Mode::Flow => 1,
                    Mode::Thermal => 3,
                    Mode::Stress => 4,
                };
            }
        }
    });
    egui::ComboBox::from_id_salt("analysis_detail")
        .selected_text(w.detail.label())
        .show_ui(ui, |ui| {
            for d in [
                Detail::Preview,
                Detail::Standard,
                Detail::Fine,
                Detail::Ultra,
            ] {
                ui.selectable_value(&mut w.detail, d, d.label());
            }
        });
    match w.mode {
        Mode::Flow => {
            let (g, n) = w.detail.flow();
            ui.small(format!("{} × {} flow cells · {n} iterations", g[0], g[1]));
        }
        _ => {
            let r = w.detail.thermal();
            ui.small(format!(
                "{} axial + {} fin cells / stage\n{} wall cells · Δt ≤ {:.5} s",
                r.axial_cells,
                r.fin_cells,
                if w.backend == Backend::OpenFoam {
                    w.detail.wall_layers()
                } else {
                    2
                },
                r.max_step_s
            ));
            ui.small(format!(
                "Heating boundary Δt ≤ {:.3} s",
                w.detail.boundary_step()
            ));
            if w.mode == Mode::Stress {
                ui.small(format!(
                    "{} fin divisions · vehicle sections",
                    w.detail.stress()
                ));
                if w.backend == Backend::OpenFoam {
                    ui.add(
                        egui::DragValue::new(&mut w.poisson)
                            .range(0.0..=0.489)
                            .speed(0.005)
                            .prefix("Fallback fin ν "),
                    ).on_hover_text("Used only for a legacy fin law. Each material's entered Poisson ratio takes precedence.");
                    help::hint(
                        ui,
                        "Enter stiffness for every shell and fin material in Build → Material → Structural properties. CalculiX solves bonded S8 shells and separate C3D20R primary fins with prescribed loads; thermal expansion is disabled.",
                    );
                }
            }
        }
    }
    if w.backend == Backend::Preview {
        ui.checkbox(&mut w.gpu, "Use GPU (required)");
        ui.small(format!("Renderer: {}", w.adapter_name));
        help::hint(
            ui,
            "GPU Euler flow / material heat diffusion; stress uses CPU beam integration.",
        );
    }
    let sample = analysis::sample_at(&lab.flight, lab.time);
    if ui
        .add_enabled(
            !w.busy() && (w.mode != Mode::Flow || sample.mach >= 0.05),
            egui::Button::new(format!("Compute at {:.3} s", lab.time)),
        )
        .clicked()
    {
        lab.playing = false;
        lab.tab = 6;
        lab.surface_mode = match w.mode {
            Mode::Flow => 1,
            Mode::Thermal => 3,
            Mode::Stress => 4,
        };
        w.request(lab);
    }
    if w.mode == Mode::Flow && sample.mach < 0.05 {
        help::hint(ui, "Flow solver requires Mach ≥ 0.05.");
    }
    if let Some(progress) = &w.progress {
        let status = progress.status();
        ui.horizontal(|ui| {
            ui.spinner();
            ui.add(egui::Label::new(egui::RichText::new(status.label()).size(11.0)).wrap());
        });
        if let Some(fraction) = status.fraction {
            ui.add(egui::ProgressBar::new(fraction).show_percentage());
        }
    }
    if let Some(k) = w.completed {
        ui.small(format!(
            "Result: {:.3} s · {} · {} · {}",
            k.time,
            k.mode.label(),
            k.detail.label(),
            k.backend.label()
        ));
        if k != Key::new(w, lab) {
            ui.colored_label(theme::palette(ui).warning, "Result needs update")
                .on_hover_text("Time, design or settings changed. Compute to update.");
        }
    }
    ui.separator();
}
pub(super) fn show(
    ui: &mut egui::Ui,
    w: &mut Snapshots,
    lab: &Lab,
    thermal: &mut thermal_view::ThermalView,
) {
    help::status(ui, &w.report);
    let Some(k) = w.completed else {
        return;
    };
    if k.revision != lab.model_revision {
        help::hint(ui, "Result belongs to the previous design.");
        return;
    }
    ui.small(format!(
        "Captured T+ {:.3} s · {} detail",
        k.time,
        k.detail.label()
    ));
    match k.mode {
        Mode::Flow => {
            let Some(f) = &w.flow else {
                return;
            };
            let p = &f.problem;
            if w.texture.is_none() {
                let q = 0.5
                    * p.far[0]
                    * ((p.far[1] / p.far[0]).powi(2) + (p.far[2] / p.far[0]).powi(2));
                let far = crate::cfd::pressure(p.far);
                let colors = (0..p.ny)
                    .rev()
                    .flat_map(|j| (0..p.nx).map(move |i| j * p.nx + i))
                    .map(|i| {
                        if p.solid[i] {
                            egui::Color32::from_gray(190)
                        } else {
                            let t = (((crate::cfd::pressure(f.cells[i]) - far) / q + 0.3) / 1.8)
                                .clamp(0.0, 1.0);
                            egui::Color32::from_rgb(
                                (255.0 * t) as u8,
                                (170.0 * (1.0 - (2.0 * t - 1.0).abs())) as u8,
                                (255.0 * (1.0 - t)) as u8,
                            )
                        }
                    })
                    .collect();
                w.texture = Some(ui.ctx().load_texture(
                    "snapshot_flow",
                    egui::ColorImage::new([p.nx, p.ny], colors),
                    egui::TextureOptions::LINEAR,
                ));
            }
            let r = ui
                .add(
                    egui::Image::new(w.texture.as_ref().unwrap())
                        .maintain_aspect_ratio(false)
                        .fit_to_exact_size(egui::vec2(
                            ui.available_width(),
                            (ui.available_width() as f64 * p.ny as f64 * p.dy
                                / (p.nx as f64 * p.dx))
                                .min(135.0) as f32,
                        )),
                )
                .rect;
            let stride = (p.nx / 40).max(4);
            for j in (0..p.ny).step_by(stride) {
                for i in (0..p.nx).step_by(stride) {
                    let n = j * p.nx + i;
                    if p.solid[n] {
                        continue;
                    }
                    let c = f.cells[n];
                    let l = c[1].hypot(c[2]).max(1e-12);
                    ui.painter().arrow(
                        egui::pos2(
                            r.left() + (i as f32 + 0.5) / p.nx as f32 * r.width(),
                            r.bottom() - (j as f32 + 0.5) / p.ny as f32 * r.height(),
                        ),
                        egui::vec2((c[1] / l) as f32 * 5.0, -(c[2] / l) as f32 * 5.0),
                        egui::Stroke::new(0.5, egui::Color32::WHITE),
                    );
                }
            }
            help::hint(
                ui,
                "Computed planar Cp / velocity · blue −0.3 to red 1.5. The 3D pressure view uses a separate incidence estimate. Inspect saved solver logs and refinement before interpreting results.",
            );
        }
        Mode::Thermal => thermal_view::show(ui, thermal, lab),
        Mode::Stress => {
            if !w.stress_matches(lab) {
                help::hint(
                    ui,
                    "Seek back to the captured timestamp or compute a new stress result.",
                );
                return;
            }
            let Some(field) = &w.vehicle_stress else {
                ui.label("No vehicle field");
                return;
            };
            ui.strong("Retained vehicle").on_hover_text(&field.backend);
            metric(
                ui,
                "Peak stress",
                units::value(ui, "Stress", field.peak() / 1e6, "MPa", 3),
            );
            metric(
                ui,
                "Axial acceleration",
                units::value(ui, "Acceleration", field.axial_acceleration_mps2, "m/s²", 2),
            );
            metric(
                ui,
                "Lateral load",
                units::value(ui, "Force", field.lateral_force_n, "N", 2),
            );
            let groups = field
                .points
                .iter()
                .map(|p| (p.stage, p.kind.as_str(), p.material.as_str()))
                .collect::<std::collections::BTreeSet<_>>();
            for (stage, kind, material) in groups {
                ui.collapsing(format!("S{} · {}",stage+1,kind),|ui| {
                    ui.label(material).on_hover_text("Assigned material for this section; stiffness and strength use its entered temperature factors.");
                    let rows=field.points.iter().filter(|p|p.stage==stage && p.kind==kind && p.material==material).collect::<Vec<_>>();
                    let peak=rows.iter().map(|p|p.stress_pa).fold(0.,f64::max);
                    metric(ui,"Peak stress",units::value(ui,"Stress",peak/1e6,"MPa",3));
                    metric(ui,"Stress / E",rows.iter().filter_map(|p|p.strain).reduce(f64::max).map_or("Needs modulus".into(),|v|format!("{v:.6}")));
                    metric(ui,"Strength / stress",rows.iter().filter_map(|p|p.margin).reduce(f64::min).map_or("Needs strength / zero load".into(),|v|format!("{v:.3}")));
                    metric(ui,"Displacement",rows.iter().filter_map(|p|p.deflection_m).reduce(f64::max).map_or("Not solved".into(),|v|units::value(ui,"Displacement",v*1000.,"mm",4)));
                    metric(ui,"Peak temperature",units::value(ui,"Temperature",rows.iter().map(|p|p.temperature_k).fold(0.,f64::max),"K",2));
                });
            }
            if !field.unsupported.is_empty() {
                ui.small("Unresolved hardware")
                    .on_hover_text(field.unsupported.join("\n"));
            }
            help::hint(
                ui,
                "Colors show peak section von Mises stress, from zero to the vehicle maximum. Stiffness, strength and temperature factors are section-specific. Stress / E is a scalar screening indicator, not a tensor strain field. Strength / stress is a strength ratio; missing data stays unavailable. Joints, contact, buckling, laminate failure, thermal expansion and flutter are unresolved.",
            );
        }
    }
}

pub(super) fn image_panel(
    ui: &mut egui::Ui,
    w: &mut Snapshots,
    lab: &Lab,
    thermal: &thermal_view::ThermalView,
) {
    let mut state = std::mem::take(&mut w.images);
    images::show(ui, &mut state, w, lab, thermal);
    w.images = state;
}
pub(super) fn summary(ui: &mut egui::Ui, w: &Snapshots) {
    help::status(ui, &w.report);
    if w.image_view {
        help::hint(
            ui,
            "2D field images are shown above. Zoom and hover to inspect; Save PNG + metadata exports the raster.",
        );
    }
}

pub(super) fn export_image_check(w: &Snapshots, lab: &Lab, path: &str) {
    images::export_check(&w.images, w, lab, path);
}
