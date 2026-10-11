//! Reproducible numerical convergence and independent reference acceptance.
//! A failed comparison is retained as a failed check; no automatic tuning.
use crate::{
    config::{Mission, Result},
    flight::{FlightResult, Simulator},
    imports::ReferencePoint,
};
use serde::Serialize;
#[derive(Serialize)]
struct Residual {
    time_s: f64,
    reference_altitude_m: f64,
    simulation_altitude_m: f64,
    altitude_error_m: f64,
    reference_speed_mps: f64,
    simulation_speed_mps: f64,
    speed_error_mps: f64,
    reference_mach: f64,
    simulation_mach: f64,
}
pub fn reference(
    result: &FlightResult,
    reference: &[ReferencePoint],
    out: &std::path::Path,
    tolerance_percent: f64,
) -> Result<serde_json::Value> {
    if !tolerance_percent.is_finite() || tolerance_percent <= 0.0 {
        return Err("Reference tolerance must be positive".into());
    }
    std::fs::create_dir_all(out)?;
    let mut csv = csv::Writer::from_path(out.join("reference-residuals.csv"))?;
    let mut sum = [0.0; 3];
    let mut count = 0;
    let mut max_alt_error: f64 = 0.0;
    for r in reference {
        let i = result
            .samples
            .partition_point(|s| s.time_s < r.time_s)
            .saturating_sub(1);
        if i + 1 >= result.samples.len() || r.time_s < result.samples[i].time_s {
            continue;
        }
        let (a, b) = (&result.samples[i], &result.samples[i + 1]);
        let f = (r.time_s - a.time_s) / (b.time_s - a.time_s);
        let mix = |x, y| x + (y - x) * f;
        let alt = mix(a.altitude_agl_m, b.altitude_agl_m);
        let speed = mix(a.speed_mps, b.speed_mps);
        let mach = mix(a.mach, b.mach);
        sum[0] += (alt - r.altitude_agl_m).powi(2);
        sum[1] += (speed - r.speed_mps).powi(2);
        sum[2] += (mach - r.mach).powi(2);
        count += 1;
        max_alt_error = max_alt_error.max((alt - r.altitude_agl_m).abs());
        csv.serialize(Residual {
            time_s: r.time_s,
            reference_altitude_m: r.altitude_agl_m,
            simulation_altitude_m: alt,
            altitude_error_m: alt - r.altitude_agl_m,
            reference_speed_mps: r.speed_mps,
            simulation_speed_mps: speed,
            speed_error_mps: speed - r.speed_mps,
            reference_mach: r.mach,
            simulation_mach: mach,
        })?;
    }
    csv.flush()?;
    let peak_alt = reference
        .iter()
        .map(|r| r.altitude_agl_m)
        .fold(0.0, f64::max);
    let peak_mach = reference.iter().map(|r| r.mach).fold(0.0, f64::max);
    let alt_percent = if peak_alt > 0.0 {
        100.0 * (result.summary.apogee_agl_m / peak_alt - 1.0)
    } else {
        0.0
    };
    let mach_percent = if peak_mach > 0.0 {
        100.0 * (result.summary.max_mach / peak_mach - 1.0)
    } else {
        0.0
    };
    let report = serde_json::json!({"status":if count>0&&peak_alt>0.0&&peak_mach>0.0&&alt_percent.abs()<=tolerance_percent&&mach_percent.abs()<=tolerance_percent{"within_selected_peak_tolerances"}else{"failed_reference_check"},"configuration_identity_confirmed":false,"tolerance_percent":tolerance_percent,"apogee_error_percent":alt_percent,"peak_mach_error_percent":mach_percent,"altitude_rmse_m":if count>0{Some((sum[0]/count as f64).sqrt())}else{None},"speed_rmse_mps":if count>0{Some((sum[1]/count as f64).sqrt())}else{None},"mach_rmse":if count>0{Some((sum[2]/count as f64).sqrt())}else{None},"max_altitude_error_m":max_alt_error,"overlap_samples":count,"reference_samples":reference.len(),"reference_coverage_fraction":count as f64/reference.len().max(1) as f64,"interpretation":"Peak tolerances and numerical convergence are separate from physical validation. Confirm motor revisions, mass/CG, aero tables, atmosphere and run identity. This module never tunes coefficients to match the reference."});
    std::fs::write(
        out.join("reference-check.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    Ok(report)
}
pub fn convergence(m: &Mission, out: &std::path::Path) -> Result<serde_json::Value> {
    if m.dt / 4.0 < 0.0001 {
        return Err("Convergence requires original dt >= 0.0004 s".into());
    }
    std::fs::create_dir_all(out)?;
    let mut rows = Vec::new();
    for scale in [1.0, 0.5, 0.25] {
        let mut case = m.clone();
        case.dt *= scale;
        let f = Simulator::new(case)?.run();
        rows.push(serde_json::json!({"dt_s":m.dt*scale,"apogee_m":f.summary.apogee_agl_m,"max_mach":f.summary.max_mach,"max_q_pa":f.summary.max_q_pa,"landing_east_m":f.summary.landing_east_m,"landing_north_m":f.summary.landing_north_m,"landing_speed_mps":f.summary.landing_speed_mps,"completed":f.summary.completed,"events":f.events.iter().filter(|e|matches!(e.kind.as_str(),"ignition"|"separation"|"apogee"|"deployment"|"landing")).collect::<Vec<_>>() }));
    }
    let get = |i: usize, key: &str| rows[i][key].as_f64().unwrap();
    let error = |key: &str| 100.0 * (get(1, key) - get(2, key)).abs() / get(2, key).abs().max(1e-9);
    let apogee_error = error("apogee_m");
    let mach_error = error("max_mach");
    let q_error = error("max_q_pa");
    let landing_error = match (
        rows[1]["landing_east_m"].as_f64(),
        rows[2]["landing_east_m"].as_f64(),
        rows[1]["landing_north_m"].as_f64(),
        rows[2]["landing_north_m"].as_f64(),
    ) {
        (Some(a), Some(b), Some(c), Some(d)) => Some((a - b).hypot(c - d)),
        _ => None,
    };
    let report = serde_json::json!({"mission":m.name,"runs":rows,"fine_pair_apogee_difference_percent":apogee_error,"fine_pair_mach_difference_percent":mach_error,"fine_pair_q_difference_percent":q_error,"fine_pair_landing_difference_m":landing_error,"passed":apogee_error<0.5&&mach_error<0.5&&q_error<1.0&&landing_error.is_some_and(|e|e<50.0),"criteria":"Fine-pair: apogee/Mach <0.5%, q <1%, both landings present and <50 m apart. Deterministic event thresholds and output sampling can limit observed order; convergence is not validation."});
    std::fs::write(
        out.join("convergence.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    Ok(report)
}
