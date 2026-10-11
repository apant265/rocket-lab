//! Visualization estimates, kept separate from the flight force model.
//! Modified Newtonian incidence is a hypersonic approximation. At lower Mach
//! it is only an explanatory map; no expansion, shielding, wake or CFD solution.
use crate::{
    atmosphere,
    config::{Mission, Result},
    flight::Telemetry,
};
use glam::{DQuat, DVec3};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
pub struct MeshDocument {
    #[serde(default)]
    pub physics_version: u32,
    pub schema_version: u32,
    pub generator: String,
    pub stages: Vec<MeshStage>,
    pub parts: Vec<MeshPart>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct MeshStage {
    #[serde(default)]
    pub components: Option<serde_json::Value>,
    #[serde(default)]
    pub component_shell: bool,
    pub length_m: f64,
    pub diameter_m: f64,
    #[serde(default)]
    pub geometry: Option<crate::config::StageGeometry>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct MeshPart {
    #[serde(default)]
    pub component_index: Option<usize>,
    pub name: String,
    pub stage: usize,
    pub kind: String,
    pub base_color: [f32; 3],
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}
impl MeshDocument {
    /// Axisymmetric body envelope; deliberately excludes fins and decorative fittings.
    pub fn body_profile(&self, stage: usize) -> Vec<[f32; 2]> {
        let mut rings: Vec<[f32; 2]> = self
            .parts
            .iter()
            .filter(|p| p.stage >= stage && (p.kind == "body" || p.kind == "nose"))
            .flat_map(|p| p.positions.iter().map(|v| [-v[1], v[0].hypot(v[2])]))
            .collect();
        rings.sort_by(|a, b| a[0].total_cmp(&b[0]).then(b[1].total_cmp(&a[1])));
        rings.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-6);
        // Geometrical closure aft of the body, not a predicted separated wake.
        if let Some(&[s, r]) = rings.last() {
            rings.push([s + r * 2.0, r * 0.5]);
            rings.push([s + r * 4.0, 0.0]);
        }
        rings
    }
    pub fn load(path: impl AsRef<Path>, mission: &Mission) -> Result<Self> {
        let model: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        model.validate(mission)?;
        Ok(model)
    }
    pub fn validate(&self, mission: &Mission) -> Result<()> {
        if self.schema_version != 1
            || self.stages.len() != mission.stages.len()
            || self.parts.is_empty()
        {
            return Err("Model schema or stage count does not match mission".into());
        }
        for (a, b) in self.stages.iter().zip(&mission.stages) {
            let components = a
                .components
                .as_ref()
                .map(|value| {
                    serde_json::from_value::<Vec<crate::assembly::Part>>(value.clone())
                        .and_then(serde_json::to_value)
                })
                .transpose()?;
            if (b.materials.as_ref().is_some_and(|m| m.component_shell) != a.component_shell)
                || (b.components.iter().any(|p| p.external())
                    && components.as_ref() != Some(&serde_json::to_value(&b.components)?))
            {
                return Err("Exterior component layout changed; regenerate Blender model".into());
            }
            if !a.length_m.is_finite()
                || !a.diameter_m.is_finite()
                || (a.length_m - b.length_m).abs() > 1e-5
                || (a.diameter_m - b.diameter_m).abs() > 1e-5
                || (a.geometry.is_some() && a.geometry != b.geometry)
            {
                return Err(
                    "Blender model dimensions do not match mission; regenerate the model".into(),
                );
            }
        }
        for p in &self.parts {
            if p.stage >= self.stages.len()
                || p.positions.len() < 3
                || p.positions.len() != p.normals.len()
                || p.indices.is_empty()
                || !p.indices.len().is_multiple_of(3)
                || p.indices.iter().any(|&i| i as usize >= p.positions.len())
                || p.positions.iter().flatten().any(|v| !v.is_finite())
                || p.base_color
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                || p.normals.iter().any(|n| {
                    let v = DVec3::new(n[0] as f64, n[1] as f64, n[2] as f64);
                    !v.is_finite() || (v.length() - 1.0).abs() > 0.01
                })
            {
                return Err(format!("Invalid mesh geometry in {}", p.name).into());
            }
        }
        Ok(())
    }
}
/// Geometrical displacement streamtube around an axisymmetric body envelope.
/// This draws direction/clearance only, not Navier-Stokes or compressible flow.
pub fn streamline_point(profile: &[[f32; 2]], station: f32, gap: f32, angle: f32) -> [f32; 3] {
    let i = profile.partition_point(|p| p[0] < station);
    let radius = if i == 0 || i == profile.len() {
        0.0
    } else {
        let a = profile[i - 1];
        let b = profile[i];
        let t = ((station - a[0]) / (b[0] - a[0]).max(1e-7)).clamp(0.0, 1.0);
        a[1] + t * (b[1] - a[1])
    };
    let r = gap.hypot(radius);
    [r * angle.cos(), -station, r * angle.sin()]
}

/// Perfect gas gamma=1.4 stagnation Cp: isentropic below M=1; normal shock
/// followed by isentropic stagnation above it. exp_m1 avoids low-M cancellation.
pub fn stagnation_cp(mach: f64) -> f64 {
    let m = mach.abs();
    if m < 1e-5 {
        return 1.0;
    }
    let m2 = m * m;
    let pressure_ratio = if m <= 1.0 {
        (3.5 * (0.2 * m2).ln_1p()).exp_m1()
    } else {
        let shock_pressure = (2.8 * m2 - 0.4) / 2.4;
        let downstream_m2 = (0.4 * m2 + 2.0) / (2.8 * m2 - 0.4);
        shock_pressure * (1.0 + 0.2 * downstream_m2).powf(3.5) - 1.0
    };
    pressure_ratio / (0.7 * m2)
}
#[derive(Clone, Copy, Debug)]
pub struct SurfaceConditions {
    pub ambient_pa: f64,
    pub q_pa: f64,
    pub cp_max: f64,
    pub upstream_model: DVec3,
    pub relative_speed_mps: f64,
}
impl SurfaceConditions {
    pub fn from_sample(t: &Telemetry, mission: &Mission) -> Self {
        let air = atmosphere::for_mission(t.altitude_m, mission);
        let wind = crate::environment::wind(mission, t.altitude_m, t.time_s);
        let relative = DVec3::new(t.vx, t.vy, t.vz) - wind;
        let speed = relative.length();
        let q = DQuat::from_xyzw(t.qx, t.qy, t.qz, t.qw).normalize();
        let body = q.conjugate() * relative.normalize_or_zero();
        Self {
            ambient_pa: air.pressure,
            q_pa: 0.5 * air.density * speed * speed,
            cp_max: stagnation_cp(speed / air.sound_speed),
            upstream_model: DVec3::new(body.x, body.z, -body.y),
            relative_speed_mps: speed,
        }
    }
    pub fn cp(&self, normal: [f32; 3]) -> f64 {
        let n = DVec3::new(normal[0] as f64, normal[1] as f64, normal[2] as f64);
        self.cp_max * n.dot(self.upstream_model).clamp(0.0, 1.0).powi(2)
    }
    pub fn pressure(&self, normal: [f32; 3]) -> f64 {
        self.ambient_pa + self.q_pa * self.cp(normal)
    }
    /// Magnitude of the tangential projection of a unit freestream vector.
    /// This is incidence geometry, not wall velocity or a solved flow field.
    pub fn tangential_fraction(&self, normal: [f32; 3]) -> f64 {
        if self.relative_speed_mps < 1e-8 {
            return 0.0;
        }
        let n = DVec3::new(normal[0] as f64, normal[1] as f64, normal[2] as f64);
        (1.0 - n.dot(self.upstream_model).clamp(-1.0, 1.0).powi(2))
            .max(0.0)
            .sqrt()
    }
    pub fn color_fraction(&self, cp: f64) -> f32 {
        if self.q_pa < 1e-8 {
            return 0.0;
        }
        ((80.0 * cp.max(0.0)).ln_1p() / (80.0 * self.cp_max).ln_1p()) as f32
    }
    pub fn cp_at_color(&self, fraction: f64) -> f64 {
        ((80.0 * self.cp_max).ln_1p() * fraction).exp_m1() / 80.0
    }
}
/// Sequential blue/cyan/green/yellow/red palette. Input is a logarithmic Cp scale.
pub fn palette(t: f32) -> [f32; 4] {
    const STOPS: [[f32; 3]; 5] = [
        [0.025, 0.12, 0.48],
        [0.0, 0.72, 0.93],
        [0.22, 0.90, 0.45],
        [1.0, 0.80, 0.08],
        [1.0, 0.12, 0.045],
    ];
    let u = t.clamp(0.0, 1.0) * 4.0;
    let i = (u as usize).min(3);
    let f = u - i as f32;
    let c = std::array::from_fn::<_, 3, _>(|j| STOPS[i][j] * (1.0 - f) + STOPS[i + 1][j] * f);
    [c[0], c[1], c[2], 1.0]
}
pub fn write_map(
    model: &MeshDocument,
    mission: &Mission,
    t: &Telemetry,
    out: impl AsRef<Path>,
) -> Result<()> {
    model.validate(mission)?;
    std::fs::create_dir_all(&out)?;
    let conditions = SurfaceConditions::from_sample(t, mission);
    let mut csv = csv::Writer::from_path(out.as_ref().join("surface-pressure.csv"))?;
    csv.write_record([
        "part",
        "stage",
        "vertex",
        "x_m",
        "y_m",
        "z_m",
        "cp_estimate",
        "pressure_pa",
        "freestream_tangential_fraction",
    ])?;
    for part in model.parts.iter().filter(|p| p.stage >= t.stage) {
        for (i, (p, n)) in part.positions.iter().zip(&part.normals).enumerate() {
            csv.write_record([
                part.name.clone(),
                (part.stage + 1).to_string(),
                i.to_string(),
                p[0].to_string(),
                p[1].to_string(),
                p[2].to_string(),
                conditions.cp(*n).to_string(),
                conditions.pressure(*n).to_string(),
                conditions.tangential_fraction(*n).to_string(),
            ])?;
        }
    }
    csv.flush()?;
    std::fs::write(
        out.as_ref().join("surface-pressure.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "time_s":t.time_s,"mach":t.mach,"ambient_pa":conditions.ambient_pa,"q_pa":conditions.q_pa,
            "cp_max":conditions.cp_max,"generator":model.generator,
            "model":"Unshielded modified Newtonian windward incidence; perfect gas gamma=1.4",
            "airflow_color":"Tangential projection of unit freestream onto each surface; incidence geometry, not wall speed or solved flow",
            "limitations":"Hypersonic approximation; lower regimes illustrative. No expansion, base pressure, boundary layer, fin/body interference or resolved flow. Does not change flight forces.",
            "coordinates":"Whole-stack nose origin; +Y nose; meters. Vertices duplicated at split normals."
        }))?,
    )?;
    Ok(())
}
