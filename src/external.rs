//! OpenCFD v2406 / CalculiX adapter. Errors never substitute a preview result.
use crate::{analysis, cfd, config::Result, flight::FlightResult, thermal};
use std::{
    path::Path,
    process::{Command, Stdio},
};
fn invoke(mode: &str, problem: serde_json::Value, directory: &Path) -> Result<serde_json::Value> {
    std::fs::create_dir_all(directory)?;
    let request = directory.join("openfoam-request.json");
    std::fs::write(
        &request,
        serde_json::to_vec_pretty(&serde_json::json!({"mode":mode,"problem":problem}))?,
    )?;
    let log = std::fs::File::create(directory.join("external-run.log"))?;
    let mut command =
        Command::new(std::env::var("ROCKET_PYTHON").unwrap_or_else(|_| "python".into()));
    command
        .args(["scripts/openfoam_analysis.py", "--request"])
        .arg(&request)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let status = command.status()?;
    if !status.success() {
        return Err(format!(
            "OpenFOAM failed; no preview substitution. {}",
            std::fs::read_to_string(directory.join("external-run.log"))?
                .chars()
                .rev()
                .take(2500)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        )
        .into());
    }
    Ok(serde_json::from_slice(&std::fs::read(
        directory.join("openfoam-result.json"),
    )?)?)
}
pub fn flow(p: cfd::Problem, directory: &Path) -> Result<cfd::Solution> {
    let output = invoke("flow", serde_json::to_value(&p)?, directory)?;
    let cells: Vec<cfd::Cell> = serde_json::from_value(output["cells"].clone())?;
    if cells.len() != p.nx * p.ny
        || cells
            .iter()
            .any(|c| c.iter().any(|v| !v.is_finite()) || c[0] <= 0.0 || cfd::pressure(*c) <= 0.0)
    {
        return Err("Invalid external flow field".into());
    }
    Ok(cfd::Solution {
        problem: p,
        cells,
        residuals: Vec::new(),
        backend: output["backend"].as_str().unwrap_or("OpenFOAM").into(),
    })
}
pub fn heat(
    flight: &FlightResult,
    resolution: thermal::Resolution,
    layers: usize,
    boundary_step: f64,
    directory: &Path,
) -> Result<thermal::History> {
    let p = thermal::prepare_with_resolution(flight, resolution);
    let selected: Vec<_> = p
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.exposed)
        .map(|(i, _)| i)
        .collect();
    let columns: Vec<_> = selected
        .iter()
        .map(|&i| {
            let mut c = p.columns[i].clone();
            // laplacianFoam uses isotropic diffusion with the entered through-wall
            // effective conductivity. The preview's contact graph is not imported.
            c.material.thermal.conductivity_w_m_k =
                c.radial_w_k * c.thickness_m / (2.0 * c.area_m2);
            c.links.clear();
            c
        })
        .collect();
    let boundaries: Vec<Vec<_>> = p
        .boundaries
        .iter()
        .map(|b| selected.iter().map(|&i| b[i]).collect())
        .collect();
    let dx: Vec<_> = columns
        .iter()
        .map(|c| {
            let stage = &flight.mission.stages[c.stage];
            if c.kind == "fin" {
                stage
                    .geometry
                    .as_ref()
                    .map_or(stage.diameter_m, |g| g.fin_span_m)
                    / resolution.fin_cells as f64
            } else {
                c.axial_bounds_m
                    .map_or(stage.length_m / resolution.axial_cells as f64, |b| {
                        b[1] - b[0]
                    })
            }
        })
        .collect();
    let output = invoke(
        "thermal",
        serde_json::json!({"columns":columns,"boundaries":boundaries,"times":p.times,"initial_k":p.initial_k,"step":resolution.max_step_s,"write_step":boundary_step,"layers":layers,"dx":dx}),
        directory,
    )?;
    let times: Vec<f64> = serde_json::from_value(output["times"].clone())?;
    let temperatures: Vec<Vec<[f64; 2]>> = serde_json::from_value(output["temperatures"].clone())?;
    if times.len() != temperatures.len()
        || times.is_empty()
        || times.windows(2).any(|p| p[1] <= p[0])
        || temperatures.iter().any(|r| {
            r.len() != columns.len() || r.iter().flatten().any(|t| !t.is_finite() || *t <= 0.0)
        })
    {
        return Err("Invalid OpenFOAM temperature history".into());
    }
    Ok(thermal::History {
        columns,
        times,
        temperatures,
        backend: output["backend"].as_str().unwrap_or("OpenFOAM").into(),
    })
}
pub fn stress(
    f: &FlightResult,
    h: &thermal::History,
    divisions: usize,
    poisson: f64,
    directory: &Path,
) -> Result<(Vec<analysis::StressPoint>, String)> {
    if !(0.0..0.49).contains(&poisson) {
        return Err("Poisson ratio must be in [0, 0.49)".into());
    }
    let s = f.samples.last().ok_or("Empty flight")?;
    let mut fins = Vec::new();
    for (stage, p) in f.mission.stages.iter().enumerate().skip(s.stage) {
        let Some(g) = &p.geometry else {
            continue;
        };
        let column = h
            .columns
            .iter()
            .find(|c| c.stage == stage && c.kind == "fin")
            .ok_or("Missing fin material region")?;
        let temperature = h.fin_temperature(s.time_s, stage, 0.0, false);
        let (youngs, strength, nu) = crate::vehicle_stress::properties(f, column, temperature);
        let youngs = youngs.ok_or_else(|| format!("S{}: enter fin Young's modulus", stage + 1))?;
        fins.push(serde_json::json!({"stage":stage,"geometry":g,"divisions":divisions,"pressure":s.q_pa*1.2*s.alpha_deg.to_radians().sin().abs(),"temperature_k":temperature,"youngs":youngs,"allowable":strength,"poisson":if column.material.mechanical.is_some(){nu}else{poisson}}));
    }
    let output = invoke(
        "stress",
        serde_json::json!({"fins":fins,"poisson":poisson}),
        directory,
    )?;
    let rows: Vec<analysis::StressPoint> = serde_json::from_value(output["rows"].clone())?;
    if rows.is_empty()
        || rows.iter().any(|r| {
            r.stage >= f.mission.stages.len()
                || !r.stress_pa.is_finite()
                || r.stress_pa < 0.0
                || r.deflection_m.is_some_and(|d| !d.is_finite())
        })
    {
        return Err("Invalid OpenFOAM stress field".into());
    }
    Ok((
        rows,
        output["backend"].as_str().unwrap_or("OpenFOAM").into(),
    ))
}
/// Revolved quadratic shell FEA over every attached body/nose material section.
/// The separately solved fin solids use `stress`; joints and mounts are idealized.
pub fn vehicle_stress(
    f: &FlightResult,
    h: &thermal::History,
    divisions: usize,
    directory: &Path,
) -> Result<crate::vehicle_stress::Field> {
    let mut field = crate::vehicle_stress::solve(f, h, divisions);
    let s = f.samples.last().ok_or("Empty flight")?;
    let mut shells = Vec::new();
    for (index, c) in h
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.stage >= s.stage && (c.kind == "body" || c.kind == "nose"))
    {
        let stage = &f.mission.stages[c.stage];
        let offset = f.mission.stages[c.stage + 1..]
            .iter()
            .map(|s| s.length_m)
            .sum::<f64>();
        let count = h
            .columns
            .iter()
            .filter(|p| p.stage == c.stage && (p.kind == "body" || p.kind == "nose"))
            .count();
        let dx = stage.length_m / count as f64;
        let temp = h.temperatures[h.frame(s.time_s)][index][0];
        let (e, strength, poisson) = crate::vehicle_stress::properties(f, c, temp);
        let e = e.ok_or_else(|| {
            format!(
                "S{} {}: enter Young's modulus for {} before full-vehicle CalculiX analysis",
                c.stage + 1,
                c.kind,
                c.material.name
            )
        })?;
        let [start, end] = c.axial_bounds_m.unwrap_or([
            (c.station_m - dx * 0.5).max(0.),
            (c.station_m + dx * 0.5).min(stage.length_m),
        ]);
        shells.push(serde_json::json!({"column":index,"stage":c.stage,"start":offset+start,"end":offset+end,"r0":crate::vehicle_stress::radius_at(f,c,start),"r1":crate::vehicle_stress::radius_at(f,c,end),"thickness":c.thickness_m,"youngs":e,"poisson":poisson,"allowable":strength,"temperature_k":temp,"material":c.material.name,"pressure":if c.kind=="body"{s.q_pa*s.alpha_deg.to_radians().sin().powi(2)}else{0.}}));
    }
    shells.sort_by(|a, b| {
        a["start"]
            .as_f64()
            .unwrap()
            .total_cmp(&b["start"].as_f64().unwrap())
    });
    let circumference = if divisions <= 32 {
        16
    } else if divisions <= 64 {
        24
    } else if divisions <= 128 {
        32
    } else if divisions <= 256 {
        48
    } else {
        96
    };
    let output = invoke(
        "vehicle-stress",
        serde_json::json!({"shells":shells,"loads":field.loads,"circumference":circumference}),
        directory,
    )?;
    for row in output["rows"]
        .as_array()
        .ok_or("Missing vehicle FEA rows")?
    {
        let column = row["column"].as_u64().ok_or("Missing structural column")? as usize;
        let point = field
            .points
            .iter_mut()
            .find(|p| p.column == column)
            .ok_or("Unknown structural column")?;
        let stress = row["stress_pa"]
            .as_f64()
            .filter(|s| s.is_finite() && *s >= 0.)
            .ok_or("Invalid vehicle stress")?;
        point.stress_pa = stress;
        point.deflection_m = row["deflection_m"]
            .as_f64()
            .filter(|d| d.is_finite() && *d >= 0.);
        let (e, strength, _) =
            crate::vehicle_stress::properties(f, &h.columns[column], point.temperature_k);
        point.strain = e.map(|e| stress / e);
        point.margin = strength.and_then(|limit| (stress > 1e-9).then_some(limit / stress));
        // Shell tensors are saved in the external case; these are not beam components.
        point.axial_pa = row["axial_pa"].as_f64().unwrap_or(0.);
        point.hoop_pa = row["hoop_pa"].as_f64().unwrap_or(0.);
        point.shear_pa = row["shear_pa"].as_f64().unwrap_or(0.);
        point.bending_pa = 0.;
    }
    let expected = shells.len();
    if output["rows"].as_array().unwrap().len() != expected {
        return Err("Incomplete vehicle FEA field".into());
    }
    field.backend = output["backend"]
        .as_str()
        .ok_or("Missing structural backend")?
        .into();
    Ok(field)
}
