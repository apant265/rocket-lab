//! Evaluated Blender geometry supplies dry mass distribution and crossflow geometry.
//! Motors/payload and unmeshed internals retain their configured lumped models.
use crate::{
    config::{Mission, Result, Stage},
    surface::{MeshDocument, MeshPart},
};
use glam::{DMat3, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMass {
    pub mass_kg: f64,
    /// Flight body coordinates: +Z nose, origin at this stage's nose.
    pub centroid_m: [f64; 3],
    /// Column-major, about centroid, in flight body coordinates.
    pub inertia_kg_m2: [[f64; 3]; 3],
    pub side_area_m2: [f64; 2],
    pub side_station_m: [f64; 2],
    pub generator: String,
    pub mesh_checksum_fnv1a64: String,
    pub input_fingerprint: String,
}
pub fn fingerprint(stage: &Stage) -> String {
    let mut value = serde_json::to_value(stage).expect("serializable stage");
    value.as_object_mut().unwrap().remove("model_mass");
    value.to_string()
}
impl ModelMass {
    pub fn validate(&self, stage: &Stage) -> Result<()> {
        let i = DMat3::from_cols_array_2d(&self.inertia_kg_m2);
        if self.input_fingerprint != fingerprint(stage) {
            return Err("3D model physics is stale; rebuild the model before launching".into());
        }
        let symmetric = (i - i.transpose())
            .to_cols_array()
            .iter()
            .all(|v| v.abs() < 1e-8);
        if !self.mass_kg.is_finite()
            || self.mass_kg <= 0.0
            || self.centroid_m.iter().any(|v| !v.is_finite())
            || !i.is_finite()
            || !symmetric
            || i.x_axis.x <= 0.0
            || i.x_axis.x * i.y_axis.y - i.x_axis.y.powi(2) <= 0.0
            || i.determinant() <= 0.0
            || self.side_area_m2.iter().any(|v| !v.is_finite() || *v < 0.0)
            || self.side_station_m.iter().any(|v| !v.is_finite())
        {
            return Err("Invalid mesh-derived mass, centroid, inertia or projected area".into());
        }
        Ok(())
    }
}
fn outer(a: DVec3, b: DVec3) -> DMat3 {
    DMat3::from_cols(a * b.x, a * b.y, a * b.z)
}
pub fn parallel_axis(mass: f64, displacement: DVec3) -> DMat3 {
    DMat3::IDENTITY * (mass * displacement.length_squared())
        - outer(displacement, displacement) * mass
}
/// Carry quenched propellant into the detached stage's dry model, once.
pub fn retain_propellant(stage: &mut Stage, fuel: f64) {
    let Some(mut model) = stage.model_mass.take() else {
        return;
    };
    let old = DVec3::from_array(model.centroid_m);
    let p = lump(
        fuel,
        stage.motor.cg_m,
        stage.length_m * 0.5,
        stage.diameter_m * 0.5,
    );
    let mass = model.mass_kg + fuel;
    let center = (old * model.mass_kg + p.center * fuel) / mass;
    let tensor = DMat3::from_cols_array_2d(&model.inertia_kg_m2)
        + parallel_axis(model.mass_kg, old - center)
        + p.inertia
        + parallel_axis(fuel, p.center - center);
    model.mass_kg = mass;
    model.centroid_m = center.to_array();
    model.inertia_kg_m2 = tensor.to_cols_array_2d();
    model.input_fingerprint = fingerprint(stage);
    stage.model_mass = Some(model);
}
#[derive(Clone, Copy)]
struct Integral {
    weight: f64,
    first: DVec3,
    second: DMat3,
}
impl Default for Integral {
    fn default() -> Self {
        Self {
            weight: 0.0,
            first: DVec3::ZERO,
            second: DMat3::ZERO,
        }
    }
}
impl Integral {
    fn add(&mut self, other: Self) {
        self.weight += other.weight;
        self.first += other.first;
        self.second += other.second;
    }
    fn piece(self, mass: f64, station_override: Option<f64>) -> Result<Piece> {
        if !self.weight.is_finite() || self.weight.abs() < 1e-15 {
            return Err("Mesh has zero or invalid physical measure".into());
        }
        let mut center = self.first / self.weight;
        let covariance = self.second / self.weight - outer(center, center);
        let inertia = DMat3::IDENTITY
            * (covariance.x_axis.x + covariance.y_axis.y + covariance.z_axis.z)
            * mass
            - covariance * mass;
        if let Some(station) = station_override {
            center.z = -station;
        }
        Ok(Piece {
            mass,
            center,
            inertia,
        })
    }
}
struct Piece {
    mass: f64,
    center: DVec3,
    inertia: DMat3,
}
fn point(p: [f32; 3], offset: f64) -> DVec3 {
    DVec3::new(p[0] as f64, -p[2] as f64, p[1] as f64 + offset)
}
fn triangle(a: DVec3, b: DVec3, c: DVec3, volume: bool) -> Integral {
    let sum = a + b + c;
    let diagonal = outer(a, a) + outer(b, b) + outer(c, c);
    let weight = if volume {
        a.dot(b.cross(c)) / 6.0
    } else {
        (b - a).cross(c - a).length() / 2.0
    };
    Integral {
        weight,
        first: sum * (weight / if volume { 4.0 } else { 3.0 }),
        second: (diagonal + outer(sum, sum)) * (weight / if volume { 20.0 } else { 12.0 }),
    }
}
fn integrate(parts: &[&MeshPart], offset: f64, volume: bool) -> Result<Integral> {
    let mut result = Integral::default();
    for part in parts {
        // Corner normals duplicate vertices; weld by exported position for closure checks.
        let mut edges: HashMap<([u32; 3], [u32; 3]), (usize, i32)> = HashMap::new();
        let mut local = Integral::default();
        for face in part.indices.chunks_exact(3) {
            let raw = face.map_positions(part);
            let [a, b, c] = raw.map(|v| point(v, offset));
            if (b - a).cross(c - a).length_squared() < 1e-28 {
                continue;
            }
            local.add(triangle(a, b, c, volume));
            for (p, q) in [(raw[0], raw[1]), (raw[1], raw[2]), (raw[2], raw[0])] {
                let p = p.map(|v| if v == 0.0 { 0 } else { v.to_bits() });
                let q = q.map(|v| if v == 0.0 { 0 } else { v.to_bits() });
                let (key, sign) = if p < q { ((p, q), 1) } else { ((q, p), -1) };
                let edge = edges.entry(key).or_default();
                edge.0 += 1;
                edge.1 += sign;
            }
        }
        if volume
            && edges
                .values()
                .any(|&(count, winding)| count != 2 || winding != 0)
        {
            return Err(format!(
                "{} is not a closed, consistently wound mass mesh",
                part.name
            )
            .into());
        }
        if volume && local.weight < 0.0 {
            local.weight *= -1.0;
            local.first *= -1.0;
            local.second *= -1.0;
        }
        result.add(local);
    }
    Ok(result)
}
trait FacePositions {
    fn map_positions(&self, part: &MeshPart) -> [[f32; 3]; 3];
}
impl FacePositions for [u32] {
    fn map_positions(&self, part: &MeshPart) -> [[f32; 3]; 3] {
        std::array::from_fn(|i| part.positions[self[i] as usize])
    }
}
fn lump(mass: f64, station: f64, length: f64, radius: f64) -> Piece {
    Piece {
        mass,
        center: DVec3::new(0.0, 0.0, -station),
        inertia: DMat3::from_diagonal(DVec3::new(
            mass * (3.0 * radius * radius + length * length) / 12.0,
            mass * (3.0 * radius * radius + length * length) / 12.0,
            mass * radius * radius / 2.0,
        )),
    }
}
fn projection(parts: &[&MeshPart], offset: f64) -> ([f64; 2], [f64; 2]) {
    let mut area = [0.0; 2];
    let mut moment = [0.0; 2];
    for p in parts {
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for &v in &p.positions {
            let v = point(v, offset);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        let axis_center = (lo + hi) * 0.5;
        for f in p.indices.chunks_exact(3) {
            let [a, b, c] = f.map_positions(p).map(|v| point(v, offset));
            let cross = (b - a).cross(c - a);
            let center = (a + b + c) / 3.0;
            // Inner bore surfaces don't contribute to the exterior silhouette.
            if p.kind != "fin"
                && cross.x * (center.x - axis_center.x) + cross.y * (center.y - axis_center.y)
                    < -1e-14
            {
                continue;
            }
            for axis in 0..2 {
                let projected = cross[axis].abs() / 4.0;
                area[axis] += projected;
                moment[axis] += projected * (-center.z);
            }
        }
    }
    (
        area,
        std::array::from_fn(|i| {
            if area[i] > 0.0 {
                moment[i] / area[i]
            } else {
                0.0
            }
        }),
    )
}
pub fn apply(model: &MeshDocument, mission: &mut Mission) -> Result<()> {
    model.validate(mission)?;
    if model.physics_version != 1 {
        return Err("Model lacks physics metadata; export it again with Blender".into());
    }
    let mut updated = mission.clone();
    let mut checksum = 0xcbf29ce484222325_u64;
    for part in &model.parts {
        for byte in part
            .name
            .bytes()
            .chain(part.kind.bytes())
            .chain((part.stage as u64).to_le_bytes())
            .chain((part.component_index.unwrap_or(usize::MAX) as u64).to_le_bytes())
            .chain(
                part.positions
                    .iter()
                    .flatten()
                    .flat_map(|v| v.to_le_bytes()),
            )
            .chain(part.indices.iter().flat_map(|v| v.to_le_bytes()))
        {
            checksum = (checksum ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    for index in 0..mission.stages.len() {
        let stage = &mission.stages[index];
        let offset: f64 = mission.stages[index + 1..].iter().map(|s| s.length_m).sum();
        let physical: Vec<_> = model
            .parts
            .iter()
            .filter(|p| {
                p.stage == index && !matches!(p.kind.as_str(), "seam" | "retainer" | "nozzle")
            })
            .collect();
        if physical.is_empty() {
            return Err(format!("No physical exterior mesh for {}", stage.name).into());
        }
        let native: Vec<_> = physical
            .iter()
            .copied()
            .filter(|p| p.component_index.is_none())
            .collect();
        let mut pieces = Vec::new();
        let shell = stage.materials.as_ref().is_some_and(|m| m.component_shell);
        if let Some(m) = stage
            .materials
            .as_ref()
            .filter(|m| shell || m.use_material_mass)
        {
            pieces.push(lump(
                m.hardware_mass_kg,
                m.hardware_cg_m,
                stage.length_m * 0.1,
                stage.diameter_m * 0.5,
            ));
            if !shell {
                for (kind, density) in [
                    ("body", m.airframe.density_kg_m3),
                    ("nose", m.nose.density_kg_m3),
                    ("fin", m.fins.density_kg_m3),
                ] {
                    let group: Vec<_> = native.iter().copied().filter(|p| p.kind == kind).collect();
                    if !group.is_empty() {
                        let integral = integrate(&group, offset, true)?;
                        pieces.push(integral.piece(integral.weight * density, None)?);
                    }
                }
            }
        } else {
            // A measured dry mass doesn't reveal individual densities. Distribute it
            // over the actual exterior surface, then honor the measured axial CG.
            if native.is_empty() {
                return Err("Measured dry mass has no exterior geometry".into());
            }
            pieces.push(
                integrate(&native, offset, false)?
                    .piece(stage.dry_mass_kg, Some(stage.dry_cg_m))?,
            );
        }
        for (component_index, p) in stage.components.iter().enumerate() {
            let group: Vec<_> = physical
                .iter()
                .copied()
                .filter(|m| m.component_index == Some(component_index))
                .collect();
            if group.is_empty() {
                if p.external() {
                    return Err(format!("Missing exported component {}", p.name).into());
                }
                pieces.push(lump(p.mass(), p.cg(), p.length_m, p.diameter_m * 0.5));
            } else {
                let integral = integrate(&group, offset, true)?;
                let mass = if p.mass_override {
                    p.mass()
                } else {
                    integral.weight * p.material.density_kg_m3
                };
                pieces.push(integral.piece(mass, if p.cg_override { Some(p.cg()) } else { None })?);
            }
        }
        let mass: f64 = pieces.iter().map(|p| p.mass).sum();
        if mass <= 0.0 {
            return Err("Model dry mass is zero".into());
        }
        let center = pieces.iter().map(|p| p.center * p.mass).sum::<DVec3>() / mass;
        let inertia = pieces.iter().fold(DMat3::ZERO, |i, p| {
            i + p.inertia + parallel_axis(p.mass, p.center - center)
        });
        let (side_area_m2, side_station_m) = projection(&physical, offset);
        let properties = ModelMass {
            mass_kg: mass,
            centroid_m: center.to_array(),
            inertia_kg_m2: inertia.to_cols_array_2d(),
            side_area_m2,
            side_station_m,
            generator: model.generator.clone(),
            mesh_checksum_fnv1a64: format!("{checksum:016x}"),
            input_fingerprint: fingerprint(stage),
        };
        properties.validate(stage)?;
        updated.stages[index].model_mass = Some(properties);
    }
    updated.validate()?;
    *mission = updated;
    Ok(())
}

pub fn export_model(mission: &Mission, out: &std::path::Path) -> Result<()> {
    use std::{
        path::PathBuf,
        process::{Command, Stdio},
    };
    mission.validate()?;
    std::fs::create_dir_all(out)?;
    let out = std::fs::canonicalize(out)?;
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
        .ok_or("Blender not found; set BLENDER_PATH to the Blender executable")?;
    let input = out.join("mission.json");
    mission.save(&input)?;
    let log = std::fs::File::create(out.join("blender.log"))?;
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
        .arg(input)
        .arg("--out")
        .arg(&out)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    if !command.status()?.success() {
        return Err(format!(
            "Blender export failed; see {}",
            out.join("blender.log").display()
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DQuat;
    fn cuboid(size: DVec3, center: DVec3, rotation: DQuat) -> MeshPart {
        let vertices = [
            (-1., -1., -1.),
            (1., -1., -1.),
            (1., 1., -1.),
            (-1., 1., -1.),
            (-1., -1., 1.),
            (1., -1., 1.),
            (1., 1., 1.),
            (-1., 1., 1.),
        ];
        MeshPart {
            component_index: None,
            name: "test box".into(),
            stage: 0,
            kind: "body".into(),
            base_color: [0.5; 3],
            positions: vertices
                .iter()
                .map(|&(x, y, z)| {
                    let p = center + rotation * (DVec3::new(x, y, z) * size / 2.0);
                    [p.x as f32, p.z as f32, -p.y as f32]
                })
                .collect(),
            normals: vec![[1., 0., 0.]; 8],
            indices: vec![
                0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0,
                7, 3, 1, 2, 6, 1, 6, 5,
            ],
        }
    }
    #[test]
    fn tetrahedron_integrals_match_rotated_box_full_tensor_and_centroid() {
        let size = DVec3::new(0.2, 0.4, 1.0);
        let center = DVec3::new(0.3, -0.2, -1.0);
        let rotation = DQuat::from_rotation_y(0.37) * DQuat::from_rotation_z(0.29);
        let mesh = cuboid(size, center, rotation);
        let integral = integrate(&[&mesh], 0.0, true).unwrap();
        assert!((integral.weight - size.element_product()).abs() < 1e-8);
        let piece = integral.piece(8.0, None).unwrap();
        assert!((piece.center - center).length() < 1e-7);
        let diagonal = DVec3::new(
            size.y.powi(2) + size.z.powi(2),
            size.x.powi(2) + size.z.powi(2),
            size.x.powi(2) + size.y.powi(2),
        ) * 8.0
            / 12.0;
        let r = DMat3::from_quat(rotation);
        let expected = r * DMat3::from_diagonal(diagonal) * r.transpose();
        assert!(
            (piece.inertia - expected)
                .to_cols_array()
                .iter()
                .all(|v| v.abs() < 1e-6)
        );
        assert!(piece.inertia.x_axis.z.abs() > 0.01);
    }
    #[test]
    fn measured_surface_mass_honors_cg_and_open_volume_is_rejected() {
        let mut mesh = cuboid(DVec3::ONE, DVec3::new(0., 0., -1.), DQuat::IDENTITY);
        let integral = integrate(&[&mesh], 0.0, false).unwrap();
        assert!((integral.weight - 6.0).abs() < 1e-10);
        let p = integral.piece(18.0, Some(1.2)).unwrap();
        assert_eq!(p.center.z, -1.2);
        assert!((p.inertia.x_axis.x - 5.0).abs() < 1e-10);
        mesh.indices.truncate(mesh.indices.len() - 3);
        assert!(integrate(&[&mesh], 0.0, true).is_err());
    }
    fn model(m: &Mission, mesh: MeshPart) -> MeshDocument {
        MeshDocument {
            schema_version: 1,
            physics_version: 1,
            generator: "analytic test".into(),
            stages: m
                .stages
                .iter()
                .map(|s| crate::surface::MeshStage {
                    components: Some(serde_json::to_value(&s.components).unwrap()),
                    component_shell: false,
                    length_m: s.length_m,
                    diameter_m: s.diameter_m,
                    geometry: s.geometry.clone(),
                })
                .collect(),
            parts: vec![mesh],
        }
    }
    #[test]
    fn mesh_application_changes_inertia_preserves_payload_and_invalidates_after_edit() {
        let mut m = Mission::demo(false);
        let payload = m.stages[0].payload_mass_kg;
        let old = crate::flight::mass_properties(&m, 0, 0.0, 0.0);
        let mesh = cuboid(
            DVec3::new(0.15, 0.15, 2.4),
            DVec3::new(0., 0., -1.2),
            DQuat::IDENTITY,
        );
        apply(&model(&m, mesh), &mut m).unwrap();
        let mass = crate::flight::mass_properties(&m, 0, 0.0, 0.0);
        assert_eq!(m.stages[0].payload_mass_kg, payload);
        assert!((mass.kg - old.kg).abs() < 1e-10);
        assert!((mass.cg_m - old.cg_m).abs() < 1e-8);
        assert!((mass.inertia.z - old.inertia.z).abs() > 0.01);
        let saved = serde_json::to_string(&m).unwrap();
        let mut reloaded: Mission = serde_json::from_str(&saved).unwrap();
        reloaded.validate().unwrap();
        reloaded.stages[0].dry_mass_kg += 1.0;
        assert!(
            reloaded
                .validate()
                .unwrap_err()
                .to_string()
                .contains("stale")
        );
    }
    #[test]
    fn staged_model_tensors_use_parallel_axis_and_drop_separated_mass() {
        let mut m = Mission::demo(true);
        for s in &mut m.stages {
            s.model_mass = Some(ModelMass {
                mass_kg: 4.0,
                centroid_m: [0.02, 0., -s.length_m / 2.0],
                inertia_kg_m2: DMat3::from_diagonal(DVec3::new(1., 2., 0.1)).to_cols_array_2d(),
                side_area_m2: [1., 1.],
                side_station_m: [s.length_m / 2.0; 2],
                generator: "test".into(),
                mesh_checksum_fnv1a64: "analytic fixture".into(),
                input_fingerprint: fingerprint(s),
            });
        }
        m.validate().unwrap();
        let full = crate::flight::mass_properties(&m, 0, 0.0, 0.0);
        let separated = crate::flight::mass_properties(&m, 1, 0.0, 0.0);
        assert!(full.kg > separated.kg);
        assert!(full.tensor.x_axis.x > separated.tensor.x_axis.x);
        assert!(full.cg_body_m.x > 0.0 && full.tensor.x_axis.z.abs() > 0.001);
    }
    #[test]
    fn component_volume_density_and_measured_override_both_feed_full_tensor() {
        let mut m = Mission::demo(false);
        let mut p = crate::assembly::Part::new(crate::assembly::PartKind::BodyTube, &m.stages[0]);
        p.material.density_kg_m3 = 1000.0;
        p.mass_override = false;
        m.stages[0].components.push(p);
        let native = cuboid(
            DVec3::new(0.15, 0.15, 2.4),
            DVec3::new(0., 0., -1.2),
            DQuat::IDENTITY,
        );
        let mut component = cuboid(
            DVec3::new(0.1, 0.1, 1.0),
            DVec3::new(0.2, 0., -1.5),
            DQuat::IDENTITY,
        );
        component.component_index = Some(0);
        let mut mesh = model(&m, native);
        mesh.parts.push(component);
        apply(&mesh, &mut m).unwrap();
        let mass = m.stages[0].model_mass.as_ref().unwrap();
        assert!((mass.mass_kg - 23.0).abs() < 1e-5);
        assert!(mass.centroid_m[0] > 0.08);
        assert!(mass.inertia_kg_m2[0][2].abs() > 0.01);
        let p = &mut m.stages[0].components[0];
        p.mass_override = true;
        p.mass_kg = 0.7;
        p.cg_override = true;
        p.cg_m = 0.2;
        mesh.stages[0].components = Some(serde_json::to_value(&m.stages[0].components).unwrap());
        apply(&mesh, &mut m).unwrap();
        assert!((m.stages[0].model_mass.as_ref().unwrap().mass_kg - 13.7).abs() < 1e-8);
    }
    #[test]
    fn quenched_propellant_is_retained_once_in_mesh_mass() {
        let mut m = Mission::demo(false);
        let mesh = cuboid(
            DVec3::new(0.15, 0.15, 2.4),
            DVec3::new(0., 0., -1.2),
            DQuat::IDENTITY,
        );
        apply(&model(&m, mesh), &mut m).unwrap();
        let before = crate::flight::mass_properties(&m, 0, 0.0, 0.0);
        let fuel = m.stages[0].motor.propellant_mass_kg;
        m.stages[0].motor.propellant_mass_kg = 0.0;
        retain_propellant(&mut m.stages[0], fuel);
        m.validate().unwrap();
        let after = crate::flight::mass_properties(&m, 0, 0.0, 0.0);
        assert!((after.kg - before.kg).abs() < 1e-10);
        assert!((after.cg_body_m - before.cg_body_m).length() < 1e-10);
        assert!(
            (after.tensor - before.tensor)
                .to_cols_array()
                .iter()
                .all(|v| v.abs() < 1e-10)
        );
    }
    #[test]
    fn blender_component_validation_normalizes_float_colors_but_rejects_dimension_changes() {
        let mut m = Mission::demo(false);
        let part = crate::assembly::Part::new(crate::assembly::PartKind::BodyTube, &m.stages[0]);
        m.stages[0].components.push(part);
        let cube = cuboid(DVec3::ONE, DVec3::new(0., 0., -1.), DQuat::IDENTITY);
        let mut mesh = model(&m, cube);
        let components = mesh.stages[0]
            .components
            .as_mut()
            .unwrap()
            .as_array_mut()
            .unwrap();
        components[0]["appearance_rgba"] = serde_json::json!([0.96, 0.61, 0.18, 1.0]);
        mesh.validate(&m).unwrap();
        mesh.stages[0].components.as_mut().unwrap()[0]["length_m"] = serde_json::json!(0.123456);
        assert!(mesh.validate(&m).is_err());
    }
}
