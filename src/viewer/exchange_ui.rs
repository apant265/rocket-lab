use super::*;
use crate::exchange::{self, Axis, Geometry, Units};
use std::{path::PathBuf, thread::JoinHandle, time::Duration};

const IMPORTS: &[&str] = &[
    "stl", "obj", "step", "stp", "iges", "igs", "brep", "brp", "fcstd", "glb", "gltf", "ply",
    "blend", "fbx", "dae",
];
const EXPORTS: &[(&str, &str)] = &[
    ("STL", "stl"),
    ("OBJ", "obj"),
    ("GLB", "glb"),
    ("Blender", "blend"),
    ("PLY", "ply"),
    ("STEP", "step"),
    ("IGES", "iges"),
    ("BREP", "brep"),
    ("FreeCAD", "fcstd"),
    ("FBX", "fbx"),
    ("glTF", "gltf"),
    ("Mesh JSON", "json"),
];

#[derive(Resource)]
pub(super) struct Exchange {
    import_open: bool,
    export_open: bool,
    path: Option<PathBuf>,
    units: Units,
    axis: Axis,
    scale: f64,
    tolerance_mm: f64,
    replace: bool,
    format: usize,
    scope: usize,
    fittings: bool,
    task: Option<Job>,
}
impl Default for Exchange {
    fn default() -> Self {
        Self {
            import_open: false,
            export_open: false,
            path: None,
            units: Units::Millimeters,
            axis: Axis::Z,
            scale: 1.,
            tolerance_mm: 0.1,
            replace: false,
            format: 0,
            scope: 0,
            fittings: true,
            task: None,
        }
    }
}
struct Job {
    task: JoinHandle<std::result::Result<Outcome, String>>,
    signature: String,
}
enum Outcome {
    Imported {
        mesh: Geometry,
        stage: usize,
        replace: bool,
    },
    Exported(PathBuf),
}
impl Exchange {
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
    fn choose(&mut self, path: PathBuf) {
        self.units = if matches!(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "obj" | "glb" | "gltf" | "blend" | "fbx" | "dae"
        ) {
            Units::Meters
        } else {
            Units::Millimeters
        };
        self.axis = if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("obj"))
        {
            Axis::Y
        } else {
            Axis::Z
        };
        self.scale = 1.;
        self.path = Some(path);
        self.import_open = true;
        self.export_open = false;
    }
    pub fn import_button(&mut self, ui: &mut egui::Ui, enabled: bool) {
        if ui.add_enabled(enabled&&!self.busy(),egui::Button::new("Import"))
            .on_hover_text("Import STL, OBJ, STEP, IGES, BREP, FreeCAD or Blender-supported geometry. Drop a file into the window to import.").clicked()
            && let Some(path)=rfd::FileDialog::new().set_title("Import rocket geometry").add_filter("Models and CAD",IMPORTS).pick_file() {self.choose(path);}
    }
    pub fn export_button(&mut self, ui: &mut egui::Ui, enabled: bool) {
        if ui.add_enabled(enabled&&!self.busy(),egui::Button::new("Export"))
            .on_hover_text("Export the evaluated vehicle, stage or selected component as STL, OBJ, GLB, Blender or faceted CAD.").clicked() {self.export_open=true;self.import_open=false;}
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        lab: &mut Lab,
        history: &mut history::History,
        build_busy: bool,
    ) {
        self.poll(lab, history);
        if !self.busy()
            && !build_busy
            && let Some(path) = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .find(|p| {
                        p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                            IMPORTS.iter().any(|format| e.eq_ignore_ascii_case(format))
                        })
                    })
            })
        {
            self.choose(path);
        }
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.import_open {
            let mut open = true;
            egui::Window::new("Import model").open(&mut open).collapsible(false).resizable(false).show(ctx,|ui| {
                ui.label(self.path.as_ref().and_then(|p|p.file_name()).unwrap_or_default().to_string_lossy());
                let cad=self.path.as_ref().is_some_and(|p|exchange::cad_format(p));
                ui.add_enabled_ui(!cad,|ui|units(ui,&mut self.units));
                if cad {ui.label("CAD units: mm").on_hover_text("FreeCAD resolves the file's native units. Geometry is converted from millimeters to meters.");}
                axes(ui,&mut self.axis);
                ui.horizontal(|ui| {ui.label("Scale");ui.add(egui::DragValue::new(&mut self.scale).speed(0.01).range(0.000001..=1000000.));});
                if cad {ui.horizontal(|ui| {ui.label("Tessellation / mm");ui.add(egui::DragValue::new(&mut self.tolerance_mm).speed(0.01).range(0.001..=10.));}).response.on_hover_text("Smaller deflection creates more triangles from CAD surfaces.");}
                egui::ComboBox::from_id_salt("import_stage").selected_text(&lab.draft.stages[lab.selected_stage].name).show_ui(ui,|ui| {for (i,s) in lab.draft.stages.iter().enumerate() {ui.selectable_value(&mut lab.selected_stage,i,&s.name);}});
                ui.horizontal(|ui| {ui.selectable_value(&mut self.replace,false,"Add part");ui.selectable_value(&mut self.replace,true,"Replace exterior").on_hover_text("Replace this stage's exterior with the model, keep motor/payload/recovery settings, and use the current measured dry mass as an editable override. Undo restores the previous design.");});
                if ui.add_enabled(!self.busy()&&!build_busy,egui::Button::new("Import model")).clicked() {self.start_import(lab);}
            });
            self.import_open &= open;
        }
        if self.export_open {
            let mut open = true;
            egui::Window::new("Export model").open(&mut open).collapsible(false).resizable(false).show(ctx,|ui| {
                let before=self.format;
                egui::ComboBox::from_id_salt("export_format").selected_text(EXPORTS[self.format].0).show_ui(ui,|ui| {for (i,(name,_)) in EXPORTS.iter().enumerate() {ui.selectable_value(&mut self.format,i,*name);}});
                if self.format!=before {self.units=if matches!(EXPORTS[self.format].1,"glb"|"gltf"|"blend"|"fbx"|"json") {Units::Meters} else {Units::Millimeters};self.axis=if matches!(EXPORTS[self.format].1,"glb"|"gltf"|"json") {Axis::Y} else {Axis::Z};}
                let ext=EXPORTS[self.format].1;
                let fixed=matches!(ext,"glb"|"gltf"|"blend"|"fbx"|"json");
                let cad=matches!(ext,"step"|"iges"|"brep"|"fcstd");
                if cad {self.units=Units::Millimeters;}
                ui.add_enabled_ui(!fixed&&!cad,|ui| {units(ui,&mut self.units);});
                ui.add_enabled_ui(!fixed,|ui| {axes(ui,&mut self.axis);});
                if matches!(ext,"step"|"iges"|"brep"|"fcstd") {ui.label("Faceted CAD").on_hover_text("FreeCAD converts evaluated triangles into faceted shells/solids. Original parametric feature history and analytic CAD surfaces are not recovered. CAD files store millimeters; the units control applies to mesh formats.");}
                egui::ComboBox::from_id_salt("export_scope").selected_text(["Whole vehicle","Selected stage","Selected part"][self.scope]).show_ui(ui,|ui| {
                    ui.selectable_value(&mut self.scope,0,"Whole vehicle");ui.selectable_value(&mut self.scope,1,"Selected stage");
                    ui.add_enabled_ui(lab.selected_component.is_some(),|ui| {ui.selectable_value(&mut self.scope,2,"Selected part");});
                });
                ui.checkbox(&mut self.fittings,"Fittings").on_hover_text("Include illustrative seams, retainers and nozzle contours. Unmeshed internal hardware stays in mission data.");
                if ui.add_enabled(!self.busy()&&!build_busy&&(self.scope!=2||lab.selected_component.is_some()),egui::Button::new("Save as…")).clicked()
                    && let Some(mut path)=rfd::FileDialog::new().set_title("Export rocket geometry").add_filter(EXPORTS[self.format].0,&[ext]).set_file_name(format!("rocket.{ext}")).save_file() {
                    if path.extension().is_none() {path.set_extension(ext);}
                    self.start_export(lab,path);
                }
                if ui.add_enabled(!self.busy()&&!build_busy,egui::Button::new("Blender + flight bundle")).on_hover_text("Keep the existing full export: Blender, GLB, evaluated mesh JSON and model-derived simulation under output/design.").clicked() {
                    self.export_open=false;lab.status="Preparing Blender bundle…".into();
                    // Handled by the header through take_bundle_request.
                    self.format=EXPORTS.len();
                }
            });
            self.export_open &= open;
        }
    }
    pub fn take_bundle_request(&mut self) -> bool {
        if self.format == EXPORTS.len() {
            self.format = 0;
            true
        } else {
            false
        }
    }
    fn start_import(&mut self, lab: &mut Lab) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let (units, axis, tolerance, scale, stage, replace) = (
            self.units,
            self.axis,
            self.tolerance_mm,
            self.scale,
            lab.selected_stage,
            self.replace,
        );
        let signature = serde_json::to_string(&lab.draft).unwrap();
        self.task = Some(Job {
            signature,
            task: std::thread::spawn(move || {
                let result = (|| -> crate::config::Result<Outcome> {
                    if !scale.is_finite() || scale <= 0. {
                        return Err("Import scale must be positive".into());
                    }
                    let mut mesh = exchange::import(&path, units, axis, tolerance)?;
                    if scale != 1. {
                        for p in &mut mesh.positions {
                            *p = p.map(|x| (x as f64 * scale) as f32);
                        }
                        mesh = Geometry::new(mesh.source, mesh.positions, mesh.indices)?;
                    }
                    Ok(Outcome::Imported {
                        mesh,
                        stage,
                        replace,
                    })
                })();
                result.map_err(|e| e.to_string())
            }),
        });
        self.import_open = false;
        lab.playing = false;
        lab.status = "Importing geometry…".into();
    }
    fn start_export(&mut self, lab: &mut Lab, path: PathBuf) {
        let mission = match builder::prepare(lab) {
            Ok(m) => m,
            Err(e) => {
                lab.status = e.to_string();
                return;
            }
        };
        let model = lab.model.clone().filter(|m| {
            !lab.geometry_dirty && m.validate(&mission).is_ok() && m.physics_version == 1
        });
        let (units, axis, fittings) = (self.units, self.axis, self.fittings);
        let stage = (self.scope != 0).then_some(lab.selected_stage);
        let component = if self.scope == 2 {
            lab.selected_component
        } else {
            None
        };
        let signature = serde_json::to_string(&lab.draft).unwrap();
        self.task = Some(Job {
            signature,
            task: std::thread::spawn(move || {
                let result = (|| -> crate::config::Result<Outcome> {
                    let model = if let Some(model) = model {
                        model
                    } else {
                        let directory = std::env::current_dir()?.join(format!(
                            "output/exchange/export-{}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)?
                                .as_nanos()
                        ));
                        crate::model_physics::export_model(&mission, &directory)?;
                        MeshDocument::load(directory.join("rocket.mesh.json"), &mission)?
                    };
                    exchange::export(&model, &path, units, axis, stage, component, fittings)?;
                    Ok(Outcome::Exported(path))
                })();
                result.map_err(|e| e.to_string())
            }),
        });
        self.export_open = false;
        lab.status = "Exporting geometry…".into();
    }
    fn poll(&mut self, lab: &mut Lab, history: &mut history::History) {
        if !self.task.as_ref().is_some_and(|t| t.task.is_finished()) {
            return;
        }
        let job = self.task.take().unwrap();
        match job
            .task
            .join()
            .unwrap_or_else(|_| Err("Model exchange worker failed".into()))
        {
            Ok(Outcome::Imported {
                mesh,
                stage,
                replace,
            }) => {
                if job.signature != serde_json::to_string(&lab.draft).unwrap() {
                    lab.status = "Design changed during import; import again.".into();
                    return;
                }
                let closed = mesh.closed;
                history.observe(&lab.draft, true);
                match exchange::insert(&mut lab.draft, stage, mesh, replace) {
                    Ok(i) => {
                        history.observe(&lab.draft, true);
                        lab.selected_stage = stage;
                        lab.selected_component = Some(i);
                        lab.workspace = 0;
                        lab.playing = false;
                        lab.geometry_dirty = true;
                        lab.model_revision += 1;
                        reset_camera(lab);
                        lab.status = if closed {
                            "Imported model · edit properties"
                        } else {
                            "Imported open mesh · mass override"
                        }
                        .into();
                    }
                    Err(e) => lab.status = e.to_string(),
                }
            }
            Ok(Outcome::Exported(path)) => lab.status = format!("Exported {}", path.display()),
            Err(e) => lab.status = e,
        }
    }
}
fn units(ui: &mut egui::Ui, value: &mut Units) {
    ui.horizontal(|ui| {
        ui.label("Units");
        egui::ComboBox::from_id_salt("exchange_units")
            .selected_text(value.label())
            .show_ui(ui, |ui| {
                for units in Units::ALL {
                    ui.selectable_value(value, units, units.label());
                }
            });
    });
}
fn axes(ui: &mut egui::Ui, value: &mut Axis) {
    ui.horizontal(|ui| {ui.label("Nose axis");for axis in [Axis::X,Axis::Y,Axis::Z] {ui.selectable_value(value,axis,axis.label());}}).response.on_hover_text("The positive selected source axis points toward the nose. Imports center the bounding box and preserve relative geometry.");
}
