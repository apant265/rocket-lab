//! Editable rocket files. Evaluated model caches are rebuilt after loading.
use crate::{
    assembly,
    config::{Mission, Result},
    design,
};
use std::{
    fs,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: u64 = 256 * 1024 * 1024;
static SAVE_ID: AtomicU64 = AtomicU64::new(0);

pub fn new_rocket(name: &str) -> Result<Mission> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 128 {
        return Err("Enter a rocket name between 1 and 128 characters".into());
    }
    let mut mission = Mission::demo(false);
    mission.name = name.into();
    mission.description =
        "New single-stage design. Configure materials, motor and launch conditions.".into();
    design::initialize_geometry(&mut mission);
    let stage = &mut mission.stages[0];
    stage.name = "Sustainer".into();
    stage.payload_mass_kg = 0.;
    stage.payload_name.clear();
    assembly::explicit_shell(stage);
    for part in &mut stage.components {
        part.name = part
            .name
            .replace("Original body tube", "Body tube")
            .replace("Original fin set", "Fin set");
    }
    mission.validate()?;
    Ok(mission)
}

fn editable(mission: &Mission) -> Mission {
    let mut mission = mission.clone();
    for stage in &mut mission.stages {
        stage.model_mass = None;
    }
    mission
}

pub fn signature(mission: &Mission) -> String {
    serde_json::to_string(&editable(mission)).expect("mission serializes")
}

pub fn load(path: &Path) -> Result<Mission> {
    if fs::metadata(path)?.len() > MAX_BYTES {
        return Err("Rocket file exceeds 256 MiB".into());
    }
    let mission: Mission = serde_json::from_slice(&fs::read(path)?)?;
    let mut mission = editable(&mission);
    // Validate before any stage indexing or geometry initialization.
    mission.validate()?;
    design::initialize_geometry(&mut mission);
    mission.validate()?;
    Ok(mission)
}

/// Write alongside the destination, then replace it only after a complete write.
pub fn save(path: &Path, mission: &Mission) -> Result<()> {
    let mission = editable(mission);
    mission.validate()?;
    let bytes = serde_json::to_vec_pretty(&mission)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Rocket file exceeds 256 MiB".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = parent.join(format!(
        ".rocket-lab-{}-{}.tmp",
        std::process::id(),
        SAVE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rocket-project-{}-{}.json",
            std::process::id(),
            SAVE_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }
    #[test]
    fn new_rocket_is_independent_single_stage_with_editable_parts() {
        let m = new_rocket("  New design  ").unwrap();
        assert_eq!(m.name, "New design");
        assert_eq!(m.stages.len(), 1);
        assert_eq!(m.stages[0].payload_mass_kg, 0.);
        assert_eq!(m.stages[0].components.len(), 3);
        assert!(m.stages[0].materials.as_ref().unwrap().component_shell);
        assert!(new_rocket(" ").is_err());
        let flight = crate::flight::Simulator::new(m).unwrap().into_result();
        assert_eq!(flight.samples.len(), 1);
        assert_eq!(flight.summary.end_time_s, 0.);
    }
    #[test]
    fn saved_design_keeps_attachments_and_replaces_previous_file() {
        let path = path();
        let mut mission = new_rocket("Round trip").unwrap();
        let stage = &mut mission.stages[0];
        assembly::attach_internal(stage, assembly::PartKind::InnerTube, 1.5);
        assembly::attach_internal(stage, assembly::PartKind::CenteringRing, 1.6);
        save(&path, &mission).unwrap();
        assert_eq!(signature(&load(&path).unwrap()), signature(&mission));
        mission.name = "Saved again".into();
        save(&path, &mission).unwrap();
        assert_eq!(load(&path).unwrap().name, "Saved again");
        let previous = fs::read(&path).unwrap();
        mission.stages.clear();
        assert!(save(&path, &mission).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn malformed_and_empty_stage_files_are_rejected() {
        let path = path();
        fs::write(&path, "{bad}").unwrap();
        assert!(load(&path).is_err());
        let mut m = Mission::demo(false);
        m.stages.clear();
        fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(load(&path).is_err());
        fs::remove_file(path).unwrap();
    }
}
