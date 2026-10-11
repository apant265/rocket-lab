use super::*;
use crate::assembly;
use crate::config::{Result, StageGeometry};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
};

#[derive(Clone)]
pub(super) struct MotorEntry {
    pub name: String,
    pub diameter_m: f64,
    pub length_m: f64,
    pub propellant_mass_kg: f64,
    pub total_mass_kg: f64,
}
pub(super) fn motor_library() -> Vec<MotorEntry> {
    let mut names = HashSet::new();
    fs::read_to_string("sources/rasp.eng")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let f: Vec<_> = line.split_whitespace().collect();
            if f.len() < 7
                || line.trim_start().starts_with(';')
                || f[0].parse::<f64>().is_ok()
                || !names.insert(f[0].to_string())
            {
                return None;
            }
            Some(MotorEntry {
                name: f[0].into(),
                diameter_m: f[1].parse::<f64>().ok()? * 0.001,
                length_m: f[2].parse::<f64>().ok()? * 0.001,
                propellant_mass_kg: f[4].parse().ok()?,
                total_mass_kg: f[5].parse().ok()?,
            })
        })
        .collect()
}
pub(super) fn initialize_geometry(m: &mut Mission) {
    crate::design::initialize_geometry(m);
}
pub(super) fn prepare(lab: &mut Lab) -> Result<Mission> {
    let mut m = lab.draft.clone();
    for stage in &mut m.stages {
        stage.model_mass = None;
        for part in &mut stage.components {
            part.size_tube_fins(stage.diameter_m);
        }
    }
    m.validate()?;
    if lab.geometry_dirty {
        crate::design::estimate_aero(&mut m);
    }
    m.validate()?;
    Ok(m)
}
pub(super) fn run(lab: &mut Lab, job: &mut BuildJob) {
    let mission = prepare(lab).and_then(|mut m| {
        crate::model_physics::apply(
            lab.model
                .as_ref()
                .ok_or("Export a 3D model before launching")?,
            &mut m,
        )?;
        Ok(m)
    });
    match mission {
        Ok(m) => start_flight(lab, job, m, lab.model.clone(), true),
        Err(e) => lab.status = e.to_string(),
    }
}
pub(super) fn launch(lab: &mut Lab, job: &mut BuildJob) {
    if lab.geometry_dirty
        || lab
            .model
            .as_ref()
            .is_none_or(|m| m.physics_version != 1 || m.validate(&lab.draft).is_err())
    {
        start_model(lab, job);
        job.launch_after_build = job.busy();
    } else {
        run(lab, job);
    }
}
#[cfg(test)]
mod stage_tests {
    use super::*;
    #[test]
    fn transition_inserts_a_booster_below_selected_stage_and_remaps_controls() {
        let mut m = Mission::load("examples/advanced-laboratory.json").unwrap();
        initialize_geometry(&mut m);
        let payload = m.stages[0].payload_mass_kg;
        let diameter = m.stages[0].diameter_m;
        assert_eq!(transition_booster(&mut m, 0).unwrap(), 0);
        assert_eq!(m.stages.len(), 2);
        assert_eq!(m.stages[1].payload_mass_kg, payload);
        assert_eq!(m.stages[0].payload_mass_kg, 0.0);
        assert_eq!(m.stages[0].components[0].position_m, 0.0);
        assert_eq!(m.stages[0].components[0].fore_diameter_m, diameter);
        assert_eq!(m.stages[0].components[0].diameter_m, m.stages[0].diameter_m);
        assert!(m.experiments.recovery.iter().all(|r| r.stage == 1));
        assert!(m.experiments.thermal.iter().all(|r| r.stage == 1));
        m.validate().unwrap();
        // Insert between existing stages, preserving the original bottom stage.
        let bottom = m.stages[0].name.clone();
        transition_booster(&mut m, 1).unwrap();
        assert_eq!(m.stages[0].name, bottom);
        assert_eq!(m.experiments.recovery[0].stage, 2);
        m.validate().unwrap();
    }
}
/// Insert immediately aft of the chosen stage; mission indexes remain bottom-first.
fn insert_booster(m: &mut Mission, index: usize) -> Result<usize> {
    if m.stages.len() >= 12 {
        return Err("Maximum 12 stages reached".into());
    }
    let index = index.min(m.stages.len() - 1);
    let mut s = m.stages[index].clone();
    s.name = format!("Booster {}", m.stages.len());
    s.payload_mass_kg = 0.0;
    s.payload_name.clear();
    s.components.clear();
    s.ignition_delay_s = 0.0;
    s.separation = crate::config::Trigger::Burnout { delay_s: 0.0 };
    if s.geometry.is_none() {
        s.geometry = Some(StageGeometry::generic(&s, false));
    }
    s.geometry.as_mut().unwrap().nose_length_m = 0.0;
    s.recovery = Some(s.recovery.clone().unwrap_or_else(|| m.recovery.clone()));
    for r in &mut m.experiments.recovery {
        if r.stage >= index {
            r.stage += 1;
        }
    }
    for r in &mut m.experiments.aero_grids {
        if r.stage >= index {
            r.stage += 1;
        }
    }
    for r in &mut m.experiments.commands {
        if r.stage >= index {
            r.stage += 1;
        }
    }
    for r in &mut m.experiments.thermal {
        if r.stage >= index {
            r.stage += 1;
        }
    }
    if let Some(material) = s.materials.as_mut() {
        material.component_shell = false;
    }
    m.stages.insert(index, s);
    Ok(index)
}
fn transition_booster(m: &mut Mission, above: usize) -> Result<usize> {
    let fore_diameter = m.stages[above].diameter_m;
    let index = insert_booster(m, above)?;
    let s = &mut m.stages[index];
    s.diameter_m = fore_diameter * 1.2;
    let mut transition = crate::assembly::Part::new(crate::assembly::PartKind::Transition, s);
    transition.name = "Booster transition".into();
    transition.position_m = 0.0;
    transition.fore_diameter_m = fore_diameter;
    transition.diameter_m = s.diameter_m;
    s.components.push(transition);
    Ok(index)
}
#[derive(Clone, Copy, Debug)]
pub(super) enum DropPart {
    Builtin(usize),
    Extra(crate::assembly::PartKind),
    Existing { stage: usize, component: usize },
}
pub(super) fn drop_part(lab: &mut Lab, payload: DropPart, stage: usize, station: Option<f64>) {
    let i = stage.min(lab.draft.stages.len() - 1);
    lab.selected_stage = i;
    lab.selected_component = None;
    let shell_kind = match payload {
        DropPart::Builtin(0) | DropPart::Extra(crate::assembly::PartKind::BodyTube) => Some(false),
        DropPart::Builtin(1) | DropPart::Extra(crate::assembly::PartKind::NoseCone) => Some(true),
        _ => None,
    };
    if let Some(nose) = shell_kind {
        let i = if nose { lab.draft.stages.len() - 1 } else { i };
        let s = &mut lab.draft.stages[i];
        let component = if nose {
            crate::assembly::attach_nose(s)
        } else {
            let attach = station.unwrap_or(s.length_m);
            crate::assembly::attach_tube(s, attach)
        };
        lab.selected_stage = i;
        lab.selected_component = Some(component);
        lab.geometry_dirty = true;
        lab.status = if nose {
            "Nose attached at the top. Edit its offset in Properties."
        } else {
            "Tube attached end to end. Edit its dimensions and offset in Properties."
        }
        .into();
        return;
    }
    match payload {
        DropPart::Builtin(part) => {
            lab.selected_part = part;
            if part == 0 {
                let s = &mut lab.draft.stages[i];
                s.length_m += 0.2;
                s.geometry.as_mut().unwrap().fin_position_m += 0.2;
            }
            if part == 1 {
                lab.selected_stage = lab.draft.stages.len() - 1;
            }
            if part == 4 {
                let s = &mut lab.draft.stages[i];
                s.payload_mass_kg = 5.0 * crate::assembly::KG_PER_LB;
                s.payload_name = "Kate and Anvil".into();
            }
            if part == 5 {
                lab.preview_recovery = true;
                lab.zoom = 12.0;
                lab.free_camera = None;
            }
        }
        DropPart::Extra(kind) => {
            if kind == crate::assembly::PartKind::Transition {
                match transition_booster(&mut lab.draft, i) {
                    Ok(index) => {
                        lab.selected_stage = index;
                        lab.selected_component = Some(0);
                    }
                    Err(e) => {
                        lab.status = e.to_string();
                        return;
                    }
                }
                lab.geometry_dirty = true;
                lab.status = "Transition attached; new booster created below the selected stage. Edit its transition, body and motor in 3D.".into();
                return;
            }
            let s = &mut lab.draft.stages[i];
            let mut p = crate::assembly::Part::new(kind, s);
            if kind.planar_fin()
                && let Some(g) = p.shape.as_mut()
            {
                g.fin_root_chord_m = p.length_m * 0.7;
                g.fin_tip_chord_m = p.length_m * 0.3;
                g.fin_sweep_m = p.length_m * 0.5;
                g.fin_position_m = 0.0;
                g.nose_length_m = 0.0;
            }
            if kind.planar_fin() {
                p.diameter_m = s.diameter_m;
                let g = p.shape.as_ref().unwrap();
                p.length_m = g.fin_root_chord_m.max(g.fin_sweep_m + g.fin_tip_chord_m);
                if kind == crate::assembly::PartKind::FreeformFin {
                    p.fin_points = vec![
                        [0.0, 0.0],
                        [g.fin_sweep_m, g.fin_span_m],
                        [g.fin_sweep_m + g.fin_tip_chord_m, g.fin_span_m],
                        [g.fin_root_chord_m, 0.0],
                    ];
                }
            }
            if kind == crate::assembly::PartKind::BodyTube {
                p.diameter_m = s.diameter_m;
            }

            if let Some(station) = station {
                p.position_m = station.clamp(0.0, s.length_m - p.length_m);
            }
            if let Some(chute) = &p.recovery {
                let recovery = s.recovery.get_or_insert_with(|| lab.draft.recovery.clone());
                if p.deploy_at_apogee {
                    recovery.drogue = chute.clone();
                } else {
                    recovery.main = chute.clone();
                }
            }
            let recovery = p.recovery.clone();
            let apogee = p.deploy_at_apogee;
            s.components.push(p);
            lab.selected_component = Some(s.components.len() - 1);
            if i == lab.draft.stages.len() - 1
                && let Some(chute) = recovery
            {
                if apogee {
                    lab.draft.recovery.drogue = chute;
                } else {
                    lab.draft.recovery.main = chute;
                }
            }
        }
        DropPart::Existing {
            stage: from,
            component,
        } => {
            if from >= lab.draft.stages.len()
                || component >= lab.draft.stages[from].components.len()
            {
                return;
            }
            let mut p = lab.draft.stages[from].components.remove(component);
            let shell = matches!(
                p.kind,
                crate::assembly::PartKind::BodyTube | crate::assembly::PartKind::NoseCone
            );
            if shell {
                let old_end = p.position_m + p.length_m;
                assembly::shift_aft(&mut lab.draft.stages[from], old_end, -p.length_m, None);
                assembly::fit_shell(&mut lab.draft.stages[from]);
                assembly::explicit_shell(&mut lab.draft.stages[i]);
                let target = if p.kind == crate::assembly::PartKind::NoseCone {
                    0.0
                } else {
                    assembly::tube_attachment(
                        &lab.draft.stages[i],
                        station.unwrap_or(lab.draft.stages[i].length_m),
                    )
                };
                let s = &mut lab.draft.stages[i];
                assembly::shift_aft(s, target, p.length_m, None);
                p.position_m = target;
                p.axial_offset_m = 0.0;
                s.components.push(p);
                assembly::fit_shell(s);
                lab.selected_component = Some(s.components.len() - 1);
                lab.geometry_dirty = true;
                lab.status =
                    "Section moved to an attachment boundary. Edit its offset in properties."
                        .into();
                return;
            }
            let s = &mut lab.draft.stages[i];
            p.length_m = p.length_m.min(s.length_m * 0.8);
            if !p.external() {
                p.diameter_m = p.diameter_m.min(s.diameter_m);
            }
            p.size_tube_fins(s.diameter_m);
            p.thickness_m = p.thickness_m.min(p.diameter_m * 0.5);
            p.cg_m = p.cg_m.min(p.length_m);
            p.position_m = station
                .unwrap_or(p.position_m)
                .clamp(0.0, s.length_m - p.length_m);
            s.components.push(p);
            lab.selected_component = Some(s.components.len() - 1);
        }
    }
    lab.geometry_dirty = true;
    lab.status =
        "Component placed. Edit its dimensions / material, then launch the simulation.".into();
}
pub(super) fn remove_stage(lab: &mut Lab) {
    if lab.draft.stages.len() <= 1 {
        return;
    }
    let index = lab.selected_stage;
    lab.draft.experiments.recovery.retain(|r| r.stage != index);
    for r in &mut lab.draft.experiments.recovery {
        if r.stage > index {
            r.stage -= 1;
        }
    }
    lab.draft
        .experiments
        .aero_grids
        .retain(|r| r.stage != index);
    for r in &mut lab.draft.experiments.aero_grids {
        if r.stage > index {
            r.stage -= 1;
        }
    }
    lab.draft.experiments.commands.retain(|r| r.stage != index);
    for r in &mut lab.draft.experiments.commands {
        if r.stage > index {
            r.stage -= 1;
        }
    }
    lab.draft.experiments.thermal.retain(|r| r.stage != index);
    for r in &mut lab.draft.experiments.thermal {
        if r.stage > index {
            r.stage -= 1;
        }
    }
    let removed = lab.draft.stages.remove(index);
    lab.selected_component = None;
    lab.selected_stage = lab.selected_stage.min(lab.draft.stages.len() - 1);
    let top = lab.draft.stages.last_mut().unwrap();
    if top.geometry.as_ref().unwrap().nose_length_m == 0.0 {
        top.geometry.as_mut().unwrap().nose_length_m = removed
            .geometry
            .unwrap()
            .nose_length_m
            .min(top.length_m * 0.4)
            .max(top.length_m * 0.22);
    }
    lab.geometry_dirty = true;
}
pub(super) fn select_motor(lab: &mut Lab, entry: &MotorEntry) {
    let stage = &mut lab.draft.stages[lab.selected_stage];
    match imports::import_motor_named("sources/rasp.eng", Some(&entry.name)) {
        Ok(mut motor) => {
            let old = lab
                .motor_library
                .iter()
                .find(|m| Some(m.name.as_str()) == stage.motor.name.split_whitespace().next());
            let old_case = old.map_or(0.0, |m| m.total_mass_kg - m.propellant_mass_kg);
            let new_case = entry.total_mass_kg - entry.propellant_mass_kg;
            motor.cg_m = stage.length_m - entry.length_m * 0.5;
            if let Some(material) = stage.materials.as_mut().filter(|m| m.use_material_mass) {
                if old.is_some()
                    && material.hardware_mass_kg >= old_case
                    && material.hardware_mass_kg + new_case - old_case > 0.0
                {
                    let moment = material.hardware_mass_kg * material.hardware_cg_m
                        - old_case * stage.motor.cg_m
                        + new_case * motor.cg_m;
                    material.hardware_mass_kg += new_case - old_case;
                    material.hardware_cg_m = moment / material.hardware_mass_kg;
                    lab.status="Motor and material-model hardware casing mass updated. Verify hardware CG.".into();
                } else {
                    lab.status = format!(
                        "Motor loaded; include its {:.3} lb empty casing in other hardware mass or a component.",
                        new_case / crate::assembly::KG_PER_LB
                    );
                }
            } else if old.is_some() && stage.dry_mass_kg > old_case {
                let moment = stage.dry_mass_kg * stage.dry_cg_m - old_case * stage.motor.cg_m
                    + new_case * motor.cg_m;
                stage.dry_mass_kg += new_case - old_case;
                stage.dry_cg_m = moment / stage.dry_mass_kg;
                lab.status =
                    "Motor curve and casing mass updated. Verify measured mass and CG.".into();
            } else {
                lab.status = format!(
                    "Motor loaded; include its {:.3} lb casing in dry mass and verify CG.",
                    new_case / crate::assembly::KG_PER_LB
                );
            }
            stage.motor = motor;
        }
        Err(e) => lab.status = e.to_string(),
    }
}
#[derive(Resource, Default)]
pub(super) struct BuildJob {
    task: Option<ModelBuild>,
    launch_after_build: bool,
    flight: Option<FlightBuild>,
    pub progress: Option<crate::progress::Progress>,
}
struct FlightBuild {
    task: std::thread::JoinHandle<std::result::Result<FlightResult, String>>,
    revision: u32,
    model: Option<MeshDocument>,
    launch: bool,
}
struct ModelBuild {
    child: Child,
    revision: u32,
    out: PathBuf,
    mission: Mission,
}
impl BuildJob {
    pub fn busy(&self) -> bool {
        self.task.is_some() || self.flight.is_some()
    }
}
pub(super) fn start_model(lab: &mut Lab, job: &mut BuildJob) {
    if job.busy() {
        return;
    }
    let result = (|| -> Result<ModelBuild> {
        let m = prepare(lab)?;
        lab.draft = m.clone();
        lab.model_revision += 1;
        lab.playing = false;
        lab.geometry_dirty = false;
        let out =
            std::env::current_dir()?.join(format!("output/design/model-{}", lab.model_revision));
        fs::create_dir_all(&out)?;
        let mission = out.join("mission.json");
        lab.draft.save(&mission)?;
        let blender = std::env::var_os("BLENDER_PATH")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| {
                [
                    "C:/Program Files/Blender Foundation/Blender 5.2/blender.exe",
                    "C:/Program Files/Blender Foundation/Blender 4.5/blender.exe",
                ]
                .into_iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
            })
            .ok_or("Blender not found. Set BLENDER_PATH to blender.exe.")?;
        let log = fs::File::create(out.join("blender.log"))?;
        let mut command = Command::new(blender);
        command
            .args([
                "--background",
                "--factory-startup",
                "--python",
                "scripts/build_model.py",
                "--",
                "--mission",
            ])
            .arg(mission)
            .arg("--out")
            .arg(&out)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        Ok(ModelBuild {
            child: command.spawn()?,
            revision: lab.model_revision,
            out,
            mission: m,
        })
    })();
    match result {
        Ok(task) => {
            job.progress = Some(crate::progress::Progress::new(
                "Blender model export",
                "blender-model",
            ));
            job.task = Some(task);
            lab.status = "Blender is exporting the 3D model and preparing flight physics…".into();
        }
        Err(e) => lab.status = e.to_string(),
    }
}
fn start_flight(
    lab: &mut Lab,
    job: &mut BuildJob,
    mission: Mission,
    model: Option<MeshDocument>,
    launch: bool,
) {
    let bytes = serde_json::to_vec(&mission).expect("validated mission");
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100000001b3)
    });
    let progress =
        crate::progress::Progress::new("6DoF flight · CPU f64", format!("flight-{hash:016x}"));
    let worker = progress.clone();
    let expected = lab.flight.summary.end_time_s.max(1.0).min(mission.duration);
    worker.stage("6DoF flight · CPU f64 · estimated flight horizon", expected);
    job.flight = Some(FlightBuild {
        revision: lab.model_revision,
        model,
        launch,
        task: std::thread::spawn(move || {
            let sim = Simulator::new(mission).map_err(|e| e.to_string())?;
            let mut target = expected;
            Ok(sim.run_with_progress(|time, limit| {
                if time > target {
                    target = limit;
                    worker.extend_total("6DoF flight · CPU f64 · time limit", limit);
                }
                worker.update(time);
            }))
        }),
    });
    job.progress = Some(progress);
    lab.playing = false;
    lab.status = "Calculating flight in the background…".into();
}
pub(super) fn poll_model(mut lab: ResMut<Lab>, mut job: ResMut<BuildJob>) {
    if job.flight.as_ref().is_some_and(|f| f.task.is_finished()) {
        let task = job.flight.take().unwrap();
        let result = task
            .task
            .join()
            .unwrap_or_else(|_| Err("Flight worker failed".into()));
        if let Some(p) = job.progress.take() {
            p.finish(result.is_ok());
        }
        match result {
            Ok(flight) if task.revision == lab.model_revision => {
                if let Some(model) = task.model {
                    lab.model = Some(model);
                }
                install_flight(&mut lab, flight);
                lab.geometry_dirty = false;
                if task.launch {
                    lab.workspace = 1;
                    lab.playing = true;
                    reset_camera(&mut lab);
                }
                lab.status =
                    "Simulation complete · exported 3D model physics · CPU f64 flight".into();
            }
            Ok(_) => lab.status = "Design changed; discarded the older flight calculation.".into(),
            Err(e) => lab.status = e,
        }
    }
    let Some(task) = job.task.as_mut() else {
        return;
    };
    let finished = match task.child.try_wait() {
        Ok(None) => return,
        Ok(Some(s)) => s.success(),
        Err(_) => false,
    };
    let task = job.task.take().unwrap();
    if let Some(p) = job.progress.take() {
        p.finish(finished);
    }
    let launch_after_build = std::mem::take(&mut job.launch_after_build);
    if task.revision != lab.model_revision {
        lab.status = "Model build finished for an older configuration. Export model again.".into();
        return;
    }
    if !finished {
        lab.status = format!(
            "Blender build failed; see {}",
            task.out.join("blender.log").display()
        );
        return;
    }
    let result = (|| -> Result<(MeshDocument, Mission)> {
        let model = MeshDocument::load(task.out.join("rocket.mesh.json"), &task.mission)?;
        let mut mission = task.mission;
        crate::model_physics::apply(&model, &mut mission)?;
        mission.save(task.out.join("simulation-mission.json"))?;
        Ok((model, mission))
    })();
    match result {
        Ok((model, mission)) => {
            start_flight(&mut lab, &mut job, mission, Some(model), launch_after_build)
        }
        Err(e) => lab.status = format!("Model physics failed: {e}"),
    }
}
