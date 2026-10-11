use crate::{config::*, flight::*, imports::ReferencePoint};
use std::{fs, path::Path};
pub fn write_flight(result: &FlightResult, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    fs::create_dir_all(path)?;
    let mut writer = csv::Writer::from_path(path.join("telemetry.csv"))?;
    for row in &result.samples {
        writer.serialize(row)?;
    }
    writer.flush()?;
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_path(path.join("detached-stages.csv"))?;
    writer.write_record([
        "time_s",
        "stage",
        "east_m",
        "north_m",
        "altitude_agl_m",
        "mass_kg",
        "cg_m",
        "qw",
        "qx",
        "qy",
        "qz",
        "landed",
        "drogue_fraction",
        "main_fraction",
        "chute_load_n",
        "mach",
        "q_pa",
        "vx",
        "vy",
        "vz",
        "landing_speed_mps",
    ])?;
    for row in &result.debris_samples {
        writer.serialize(row)?;
    }
    writer.flush()?;
    fs::write(
        path.join("summary.json"),
        serde_json::to_string_pretty(&result.summary)?,
    )?;
    fs::write(
        path.join("events.json"),
        serde_json::to_string_pretty(&result.events)?,
    )?;
    result.mission.save(path.join("mission.json"))?;
    let mut recovery_csv = csv::Writer::from_path(path.join("recovery-dynamics.csv"))?;
    for row in &result.recovery_samples {
        recovery_csv.serialize(row)?;
    }
    recovery_csv.flush()?;
    let site = &result.mission.launch;
    let mut features = Vec::new();
    let coordinates: Vec<_> = result
        .samples
        .iter()
        .map(|s| {
            let c = crate::geo::from_sample(site, s.east_m, s.north_m, s.altitude_agl_m);
            serde_json::json!([c.longitude_deg, c.latitude_deg, c.altitude_m])
        })
        .collect();
    features.push(serde_json::json!({"type":"Feature","properties":{"vehicle":"retained","model":if result.mission.environment.terrain.is_some(){"spherical Earth plus imported MSL terrain"}else{"spherical constant-site-height Earth"}},"geometry":{"type":"LineString","coordinates":coordinates}}));
    for stage in 0..result.mission.stages.len() {
        let rows: Vec<_> = result
            .debris_samples
            .iter()
            .filter(|s| s.stage == stage)
            .collect();
        if rows.is_empty() {
            continue;
        }
        let coordinates: Vec<_> = rows
            .iter()
            .map(|s| {
                let c = crate::geo::from_sample(site, s.east_m, s.north_m, s.altitude_agl_m);
                serde_json::json!([c.longitude_deg, c.latitude_deg, c.altitude_m])
            })
            .collect();
        features.push(serde_json::json!({"type":"Feature","properties":{"stage":stage+1,"landed":rows.last().unwrap().landed},"geometry":{"type":"LineString","coordinates":coordinates}}));
        let last = rows.last().unwrap();
        if last.landed {
            let c = crate::geo::from_sample(site, last.east_m, last.north_m, last.altitude_agl_m);
            features.push(serde_json::json!({"type":"Feature","properties":{"stage":stage+1,"event":"landing","speed_mps":last.landing_speed_mps},"geometry":{"type":"Point","coordinates":[c.longitude_deg,c.latitude_deg,c.altitude_m]}}));
        }
    }
    if let (Some(e), Some(n)) = (
        result.summary.landing_east_m,
        result.summary.landing_north_m,
    ) {
        let c = crate::geo::from_sample(
            site,
            e,
            n,
            result.samples.last().map_or(0.0, |s| s.altitude_agl_m),
        );
        features.push(serde_json::json!({"type":"Feature","properties":{"vehicle":"retained","event":"landing"},"geometry":{"type":"Point","coordinates":[c.longitude_deg,c.latitude_deg,c.altitude_m]}}));
    }
    fs::write(
        path.join("trajectory.geojson"),
        serde_json::to_string_pretty(
            &serde_json::json!({"type":"FeatureCollection","features":features}),
        )?,
    )?;
    Ok(())
}
pub fn compare(result: &FlightResult, reference: &[ReferencePoint]) -> serde_json::Value {
    let ref_apogee = reference
        .iter()
        .map(|p| p.altitude_agl_m)
        .fold(0.0, f64::max);
    let ref_mach = reference.iter().map(|p| p.mach).fold(0.0, f64::max);
    let mut sum = 0.0;
    let mut count = 0;
    let mut i = 0;
    for r in reference {
        while i + 1 < result.samples.len() && result.samples[i + 1].time_s < r.time_s {
            i += 1;
        }
        if i + 1 >= result.samples.len() || r.time_s < result.samples[i].time_s {
            continue;
        }
        let a = &result.samples[i];
        let b = &result.samples[i + 1];
        let f = (r.time_s - a.time_s) / (b.time_s - a.time_s);
        let alt = a.altitude_agl_m + (b.altitude_agl_m - a.altitude_agl_m) * f;
        sum += (alt - r.altitude_agl_m).powi(2);
        count += 1;
    }
    serde_json::json!({"warning":"Comparison does not establish validation. Confirm identical motor curves, masses, launch conditions, event timing, and aerodynamic configurations before interpreting differences.","reference_samples":reference.len(),"overlap_samples":count,"reference_apogee_agl_m":ref_apogee,"simulation_apogee_agl_m":result.summary.apogee_agl_m,"apogee_difference_percent":if ref_apogee>0.0{Some(100.0*(result.summary.apogee_agl_m/ref_apogee-1.0))}else{None},"reference_max_mach":ref_mach,"simulation_max_mach":result.summary.max_mach,"altitude_rmse_m":if count>0{Some((sum/count as f64).sqrt())}else{None}})
}
/// Deterministic SplitMix64: seed determines every run, with no OS randomness.
pub struct Random(pub u64);
impl Random {
    pub fn uniform(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut x = self.0;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((x ^ (x >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn normal(&mut self) -> f64 {
        (-2.0 * self.uniform().max(1e-16).ln()).sqrt()
            * (std::f64::consts::TAU * self.uniform()).cos()
    }
}
pub fn dispersion(
    m: &Mission,
    runs: usize,
    seed: u64,
    path: impl AsRef<Path>,
) -> Result<serde_json::Value> {
    if runs == 0 || runs > 10000 {
        return Err("runs must be 1–10000".into());
    }
    let path = path.as_ref();
    fs::create_dir_all(path)?;
    let mut writer = csv::Writer::from_path(path.join("dispersion.csv"))?;
    writer.write_record([
        "run",
        "apogee_agl_m",
        "max_mach",
        "max_q_pa",
        "landing_east_m",
        "landing_north_m",
        "completed",
        "drag_scale",
        "thrust_scale",
        "mass_scale",
        "wind_east_mps",
        "wind_north_mps",
    ])?;
    let mut rng = Random(seed);
    let mut apogees = Vec::new();
    let mut landings = 0;
    let mut landing_points = Vec::new();
    let mut footprint_features = Vec::new();
    for i in 0..runs {
        let mut case = m.clone();
        let cd = (1.0 + 0.1 * rng.normal()).clamp(0.5, 1.5);
        let thrust = (1.0 + 0.03 * rng.normal()).clamp(0.8, 1.2);
        let mass = (1.0 + 0.03 * rng.normal()).clamp(0.8, 1.2);
        case.wind.east_mps += rng.normal() * 2.0;
        case.wind.north_mps += rng.normal() * 2.0;
        case.launch.elevation_deg =
            (case.launch.elevation_deg + rng.normal() * 0.5).clamp(60.0, 90.0);
        for s in &mut case.stages {
            s.dry_mass_kg *= mass;
            if let Some(material) = s.materials.as_mut() {
                material.hardware_mass_kg *= mass;
                material.airframe.density_kg_m3 *= mass;
                material.nose.density_kg_m3 *= mass;
                material.fins.density_kg_m3 *= mass;
            }
            for part in &mut s.components {
                part.mass_kg *= mass;
                part.material.density_kg_m3 *= mass;
            }

            for p in &mut s.motor.points {
                p[1] *= thrust;
            }
            for a in &mut s.aero {
                a.cd_power_on *= cd;
                a.cd_power_off *= cd;
            }
        }
        let east = case.wind.east_mps;
        let north = case.wind.north_mps;
        let result = Simulator::new(case)?.run();
        let s = &result.summary;
        apogees.push(s.apogee_agl_m);
        if s.completed {
            landings += 1;
            if let (Some(e), Some(n)) = (s.landing_east_m, s.landing_north_m) {
                landing_points.push([e, n]);
                let c = crate::geo::from_sample(&m.launch, e, n, 0.0);
                footprint_features.push(serde_json::json!({"type":"Feature","properties":{"run":i,"vehicle":"retained"},"geometry":{"type":"Point","coordinates":[c.longitude_deg,c.latitude_deg]}}));
            }
        }
        for stage in 0..m.stages.len().saturating_sub(1) {
            if let Some(d) = result
                .debris_samples
                .iter()
                .rev()
                .find(|d| d.stage == stage && d.landed)
            {
                let c = crate::geo::from_sample(&m.launch, d.east_m, d.north_m, d.altitude_agl_m);
                footprint_features.push(serde_json::json!({"type":"Feature","properties":{"run":i,"stage":stage+1},"geometry":{"type":"Point","coordinates":[c.longitude_deg,c.latitude_deg]}}));
            }
        }

        writer.write_record([
            i.to_string(),
            s.apogee_agl_m.to_string(),
            s.max_mach.to_string(),
            s.max_q_pa.to_string(),
            s.landing_east_m.map_or(String::new(), |v| v.to_string()),
            s.landing_north_m.map_or(String::new(), |v| v.to_string()),
            s.completed.to_string(),
            cd.to_string(),
            thrust.to_string(),
            mass.to_string(),
            east.to_string(),
            north.to_string(),
        ])?;
    }
    writer.flush()?;
    apogees.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        let x = p * (runs - 1) as f64;
        let lo = x.floor() as usize;
        let hi = x.ceil() as usize;
        apogees[lo] + (apogees[hi] - apogees[lo]) * x.fract()
    };
    let footprint = landing_ellipse(&landing_points);
    if let Some(ref ellipse) = footprint {
        let coords = ellipse["polygon_enu_m"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let c = crate::geo::from_sample(
                    &m.launch,
                    p[0].as_f64().unwrap(),
                    p[1].as_f64().unwrap(),
                    0.0,
                );
                vec![c.longitude_deg, c.latitude_deg]
            })
            .collect::<Vec<_>>();
        footprint_features.push(serde_json::json!({"type":"Feature","properties":{"vehicle":"retained","confidence":0.95,"model":"Gaussian sample covariance ellipse"},"geometry":{"type":"Polygon","coordinates":[coords]}}));
    }
    fs::write(
        path.join("landing-footprint.geojson"),
        serde_json::to_string_pretty(
            &serde_json::json!({"type":"FeatureCollection","features":footprint_features}),
        )?,
    )?;
    let report = serde_json::json!({"landing_ellipse":footprint,"runs":runs,"seed":seed,"completed_landings":landings,"apogee_p05_m":percentile(0.05),"apogee_p50_m":percentile(0.5),"apogee_p95_m":percentile(0.95),"assumed_uncertainties":"independent Gaussian: drag 10%, thrust 3%, dry mass 3%, horizontal wind 2 m/s per axis, elevation 0.5 degrees; scales bounded. These are demonstration assumptions, not measured distributions."});
    fs::write(
        path.join("dispersion-summary.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    Ok(report)
}

/// Statistical ellipse of the landing population (not a confidence interval
/// for the mean, and not a guaranteed containment boundary).
pub fn landing_ellipse(points: &[[f64; 2]]) -> Option<serde_json::Value> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f64;
    let e = points.iter().map(|p| p[0]).sum::<f64>() / n;
    let north = points.iter().map(|p| p[1]).sum::<f64>() / n;
    let a = points.iter().map(|p| (p[0] - e).powi(2)).sum::<f64>() / (n - 1.0);
    let d = points.iter().map(|p| (p[1] - north).powi(2)).sum::<f64>() / (n - 1.0);
    let b = points
        .iter()
        .map(|p| (p[0] - e) * (p[1] - north))
        .sum::<f64>()
        / (n - 1.0);
    let gap = ((a - d).powi(2) + 4.0 * b * b).sqrt();
    let major = (5.991464547 * (a + d + gap) * 0.5).max(0.0).sqrt();
    let minor = (5.991464547 * (a + d - gap) * 0.5).max(0.0).sqrt();
    let angle = 0.5 * (2.0 * b).atan2(a - d);
    let polygon: Vec<_> = (0..=64)
        .map(|i| {
            let t = i as f64 / 64.0 * std::f64::consts::TAU;
            let x = major * t.cos();
            let y = minor * t.sin();
            [
                e + x * angle.cos() - y * angle.sin(),
                north + x * angle.sin() + y * angle.cos(),
            ]
        })
        .collect();
    Some(
        serde_json::json!({"samples":points.len(),"center_east_m":e,"center_north_m":north,"major_radius_m":major,"minor_radius_m":minor,"angle_from_east_deg":angle.to_degrees(),"polygon_enu_m":polygon,"assumption":"95% bivariate Gaussian population ellipse from finite completed runs. Multimodal failures, bias and uncertainty outside specified inputs are not represented."}),
    )
}
