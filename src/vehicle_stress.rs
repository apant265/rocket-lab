//! Quasi-static section loads over the retained vehicle. Annular beam/shell
//! screening, with material-specific stiffness and strength. No joint/contact FEA.
use crate::{
    assembly::PartKind,
    flight::{FlightResult, Telemetry},
    thermal::{Column, History},
};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Point {
    pub column: usize,
    pub stage: usize,
    pub kind: String,
    pub station_m: f64,
    pub radial_fraction: f64,
    pub material: String,
    pub temperature_k: f64,
    pub stress_pa: f64,
    pub axial_pa: f64,
    pub bending_pa: f64,
    pub hoop_pa: f64,
    pub shear_pa: f64,
    pub strain: Option<f64>,
    pub margin: Option<f64>,
    pub deflection_m: Option<f64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Field {
    pub points: Vec<Point>,
    pub backend: String,
    pub axial_acceleration_mps2: f64,
    pub lateral_acceleration_mps2: f64,
    pub lateral_force_n: f64,
    pub drag_n: f64,
    pub unsupported: Vec<String>,
    pub loads: Vec<Load>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Load {
    pub station_m: f64,
    pub force_n: [f64; 3],
}
impl Field {
    pub fn at_column(&self, column: usize) -> Option<&Point> {
        self.points.iter().find(|p| p.column == column)
    }
    pub fn peak(&self) -> f64 {
        self.points.iter().map(|p| p.stress_pa).fold(0., f64::max)
    }
}
/// Import primary fin results with the actual material temperature used by that
/// solver. External primary solids currently use a uniform root temperature.
pub fn merge_primary_fins(
    f: &FlightResult,
    h: &History,
    field: &mut Field,
    rows: &[crate::analysis::StressPoint],
) {
    for point in field.points.iter_mut().filter(|p| p.kind == "fin") {
        if let Some(row) = rows
            .iter()
            .filter(|r| r.stage == point.stage)
            .min_by(|a, b| {
                (a.span_fraction - point.radial_fraction)
                    .abs()
                    .total_cmp(&(b.span_fraction - point.radial_fraction).abs())
            })
        {
            point.stress_pa = row.stress_pa;
            point.bending_pa = row.stress_pa;
            point.temperature_k = row.temperature_k;
            point.deflection_m = row.deflection_m;
            let (e, strength, _) = properties(f, &h.columns[point.column], point.temperature_k);
            point.strain = e.map(|e| point.stress_pa / e);
            point.margin = strength
                .and_then(|limit| (point.stress_pa > 1e-9).then_some(limit / point.stress_pa));
        }
    }
}
pub fn section_stress(
    radius: f64,
    thickness: f64,
    axial_n: f64,
    bending_nm: f64,
    shear_n: f64,
    pressure_pa: f64,
) -> [f64; 5] {
    let inner = (radius - thickness).max(0.);
    let area = PI * (radius.powi(2) - inner.powi(2)).max(1e-20);
    let inertia = PI * 0.25 * (radius.powi(4) - inner.powi(4)).max(1e-24);
    let axial = axial_n / area;
    let bending = bending_nm.abs() * radius / inertia;
    let hoop = -pressure_pa * radius / thickness.max(1e-9);
    let shear = 2. * shear_n.abs() / area;
    let vm = |long: f64| {
        (long * long + hoop * hoop - long * hoop + 3. * shear * shear)
            .max(0.)
            .sqrt()
    };
    [
        vm(axial + bending).max(vm(axial - bending)),
        axial,
        bending,
        hoop,
        shear,
    ]
}
pub fn radius(f: &FlightResult, c: &Column) -> f64 {
    radius_at(f, c, c.station_m)
}
pub fn radius_at(f: &FlightResult, c: &Column, station: f64) -> f64 {
    let s = &f.mission.stages[c.stage];
    if s.materials.as_ref().is_some_and(|m| m.component_shell)
        && let Some(p) = s.components.iter().find(|p| {
            matches!(
                p.kind,
                PartKind::BodyTube | PartKind::NoseCone | PartKind::Transition
            ) && c.station_m >= p.position_m
                && c.station_m <= p.position_m + p.length_m
        })
    {
        let u = ((station - p.position_m) / p.length_m.max(1e-9)).clamp(0., 1.);
        return match p.kind {
            PartKind::Transition => {
                (p.fore_diameter_m + (p.diameter_m - p.fore_diameter_m) * u) * 0.5
            }
            PartKind::NoseCone => nose_radius(
                p.diameter_m * 0.5,
                u,
                p.shape
                    .as_ref()
                    .map_or(crate::config::NoseShape::Conical, |g| g.nose_shape),
                p.length_m,
            ),
            _ => p.diameter_m * 0.5,
        }
        .max(c.thickness_m);
    }
    let r = s.diameter_m * 0.5;
    if c.kind == "nose"
        && let Some(g) = &s.geometry
    {
        nose_radius(
            r,
            (station / g.nose_length_m.max(1e-9)).clamp(0., 1.),
            g.nose_shape,
            g.nose_length_m,
        )
        .max(c.thickness_m)
    } else {
        r
    }
}
fn nose_radius(r: f64, u: f64, shape: crate::config::NoseShape, length: f64) -> f64 {
    match shape {
        crate::config::NoseShape::Conical => r * u,
        crate::config::NoseShape::Elliptical => r * (1. - (1. - u).powi(2)).max(0.).sqrt(),
        crate::config::NoseShape::TangentOgive => {
            let rho = (r * r + length * length) / (2. * r.max(1e-9));
            (rho * rho - (length * (1. - u)).powi(2)).max(0.).sqrt() + r - rho
        }
    }
}
pub fn properties(
    f: &FlightResult,
    c: &Column,
    temperature: f64,
) -> (Option<f64>, Option<f64>, f64) {
    if let Some(m) = &c.material.mechanical {
        let (e, strength) = m.at(temperature);
        return (Some(e), strength, m.poisson_ratio);
    }
    if c.kind == "fin"
        && let Some(law) = f
            .mission
            .experiments
            .thermal
            .iter()
            .find(|l| l.stage == c.stage && l.region == "fin")
    {
        let soften = ((law.ablation_temperature_k - temperature)
            / (law.ablation_temperature_k - 288.15).max(1.))
        .clamp(0.01, 1.);
        return (
            Some(law.youngs_modulus_pa * soften),
            Some(law.allowable_stress_pa * soften),
            0.3,
        );
    }
    (None, None, 0.3)
}
fn acceleration(f: &FlightResult, s: &Telemetry) -> (f64, f64) {
    let prev = f
        .samples
        .iter()
        .rev()
        .find(|p| p.time_s < s.time_s - 1e-7 && p.stage == s.stage);
    let q = glam::DQuat::from_xyzw(s.qx, s.qy, s.qz, s.qw);
    if let Some(p) = prev {
        let dt = s.time_s - p.time_s;
        let specific =
            glam::DVec3::new(s.vx - p.vx, s.vy - p.vy, s.vz - p.vz) / dt + glam::DVec3::Z * 9.80665;
        let body = q.conjugate() * specific;
        (body.z, body.x.hypot(body.y))
    } else {
        (s.specific_force_g * 9.80665, 0.)
    }
}
pub fn solve(f: &FlightResult, h: &History, divisions: usize) -> Field {
    let s = f.samples.last().expect("recorded flight");
    let (axial_acceleration_mps2, lateral_acceleration_mps2) = acceleration(f, s);
    let active = &f.mission.stages[s.stage];
    let area = PI * (active.diameter_m * 0.5).powi(2);
    let coeff = crate::flight::coefficients(active, s.mach);
    let lateral_force_n =
        s.q_pa * area * coeff.cn_alpha_per_rad * s.alpha_deg.to_radians().sin().abs();
    let drag_n = s.q_pa * area * s.cd;
    let mut field = Field {
        backend: "CPU / full retained vehicle / annular beam and shell sections".into(),
        axial_acceleration_mps2,
        lateral_acceleration_mps2,
        lateral_force_n,
        drag_n,
        ..Default::default()
    };
    let mut pieces = Vec::new();
    for (stage, p) in f.mission.stages.iter().enumerate().skip(s.stage) {
        let offset = f.mission.stages[stage + 1..]
            .iter()
            .map(|s| s.length_m)
            .sum::<f64>();
        if let Some(model) = &p.model_mass {
            let dry = crate::assembly::dry_pieces(p);
            let sum = dry.iter().map(|p| p.0).sum::<f64>();
            if sum > 1e-9 {
                // Section density/geometry supplies the distribution; calibrate
                // its total and centroid to the evaluated Blender dry model.
                let shift = -model.centroid_m[2] - dry.iter().map(|p| p.0 * p.1).sum::<f64>() / sum;
                pieces.extend(
                    dry.iter()
                        .map(|piece| (piece.0 * model.mass_kg / sum, offset + piece.1 + shift)),
                );
            } else {
                pieces.push((model.mass_kg, offset - model.centroid_m[2]));
            }
        } else {
            pieces.extend(
                crate::assembly::dry_pieces(p)
                    .iter()
                    .map(|piece| (piece.0, offset + piece.1)),
            );
        }
        pieces.push((p.payload_mass_kg, offset + p.payload_cg_m));
        let ignition = f
            .events
            .iter()
            .rev()
            .find(|e| e.stage == stage && e.kind == "ignition")
            .map_or(0., |e| e.time_s);
        pieces.push((
            p.motor.remaining(if stage == s.stage {
                s.time_s - ignition
            } else {
                0.
            }),
            offset + p.motor.cg_m,
        ));
    }
    let total = pieces.iter().map(|p| p.0).sum::<f64>().max(1e-9);
    pieces.iter_mut().for_each(|p| p.0 *= s.mass_kg / total);
    field.loads = pieces
        .iter()
        .map(|p| Load {
            station_m: p.1,
            force_n: [
                -p.0 * lateral_acceleration_mps2,
                0.,
                p.0 * axial_acceleration_mps2,
            ],
        })
        .collect();
    field.loads.push(Load {
        station_m: s.cp_m,
        force_n: [lateral_force_n, 0., drag_n],
    });
    let frame = h.frame(s.time_s);
    for (index, c) in h
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.stage >= s.stage)
    {
        let stage = &f.mission.stages[c.stage];
        let offset = f.mission.stages[c.stage + 1..]
            .iter()
            .map(|s| s.length_m)
            .sum::<f64>();
        let global = c.station_m + offset;
        let temperature_k = h.temperatures[frame][index][0];
        let (youngs, strength, _) = properties(f, c, temperature_k);
        let mut point = Point {
            column: index,
            stage: c.stage,
            kind: c.kind.clone(),
            station_m: c.station_m,
            radial_fraction: c.radial_fraction,
            material: c.material.name.clone(),
            temperature_k,
            stress_pa: 0.,
            axial_pa: 0.,
            bending_pa: 0.,
            hoop_pa: 0.,
            shear_pa: 0.,
            strain: None,
            margin: None,
            deflection_m: None,
        };
        if c.kind == "body" || c.kind == "nose" {
            let ahead: Vec<_> = pieces.iter().filter(|p| p.1 <= global).collect();
            let mass = ahead.iter().map(|p| p.0).sum::<f64>();
            let first_moment = ahead.iter().map(|p| p.0 * (global - p.1)).sum::<f64>();
            let aero_ahead = global >= s.cp_m;
            let axial = -(mass * axial_acceleration_mps2 + if aero_ahead { drag_n } else { 0. });
            let shear =
                if aero_ahead { lateral_force_n } else { 0. } - mass * lateral_acceleration_mps2;
            let bending = lateral_force_n * (global - s.cp_m).max(0.)
                - lateral_acceleration_mps2 * first_moment;
            let pressure =
                s.q_pa * (c.incidence.powi(2) + s.alpha_deg.to_radians().sin().powi(2)).min(1.);
            let [vm, axial, bending, hoop, shear] =
                section_stress(radius(f, c), c.thickness_m, axial, bending, shear, pressure);
            point.stress_pa = vm;
            point.axial_pa = axial;
            point.bending_pa = bending;
            point.hoop_pa = hoop;
            point.shear_pa = shear;
        } else if c.kind == "fin"
            || stage
                .components
                .iter()
                .any(|p| p.name == c.kind && p.kind.planar_fin())
        {
            let g = stage
                .components
                .iter()
                .find(|p| p.name == c.kind && p.kind.planar_fin())
                .and_then(|p| p.shape.as_ref())
                .or(stage.geometry.as_ref());
            let Some(g) = g else {
                continue;
            };
            let span = g.fin_span_m;
            let x = c.radial_fraction * span;
            let pressure = s.q_pa * 1.2 * s.alpha_deg.to_radians().sin().abs();
            let n = divisions.clamp(8, 1024);
            let dx = (span - x) / n as f64;
            let chord = |y: f64| {
                g.fin_root_chord_m + (g.fin_tip_chord_m - g.fin_root_chord_m) * y / span.max(1e-9)
            };
            let moment = (0..n)
                .map(|j| {
                    let y = x + (j as f64 + 0.5) * dx;
                    pressure * chord(y) * (y - x) * dx
                })
                .sum::<f64>();
            let inertia = chord(x) * c.thickness_m.powi(3) / 12.;
            point.stress_pa = moment * c.thickness_m * 0.5 / inertia.max(1e-24);
            point.bending_pa = point.stress_pa;
        } else {
            field
                .unsupported
                .push(format!("S{} / {}", c.stage + 1, c.kind));
            continue;
        }
        point.strain = youngs.map(|e| point.stress_pa / e);
        point.margin =
            strength.and_then(|limit| (point.stress_pa > 1e-9).then_some(limit / point.stress_pa));
        field.points.push(point);
    }
    field
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn annular_section_matches_axial_bending_and_pressure_references() {
        let r: f64 = 0.1;
        let t = 0.005;
        let area = PI * (r * r - (r - t).powi(2));
        let i = PI / 4. * (r.powi(4) - (r - t).powi(4));
        let pure = section_stress(r, t, 1000., 0., 0., 0.);
        assert!((pure[0] - 1000. / area).abs() < 1e-8);
        let bend = section_stress(r, t, 0., 200., 0., 0.);
        assert!((bend[0] - 200. * r / i).abs() < 1e-8);
        let hoop = section_stress(r, t, 0., 0., 0., 500.);
        assert!((hoop[0] - 500. * r / t).abs() < 1e-8);
        assert!(
            section_stress(r, t, -1000., 200., 40., 500.)
                .iter()
                .all(|v| v.is_finite())
        );
    }
    #[test]
    fn local_material_changes_stiffness_and_margin_and_missing_data_stays_absent() {
        let mut m = crate::config::Mission::demo(false);
        m.duration = 1.;
        for s in &mut m.stages {
            s.geometry = Some(crate::config::StageGeometry::generic(s, true));
        }
        let f = crate::flight::Simulator::new(m).unwrap().run();
        let mut h = crate::thermal::solve(&crate::thermal::prepare(&f));
        let mut c = h.columns[0].clone();
        c.material.mechanical = Some(crate::assembly::MechanicalMaterial {
            youngs_modulus_pa: 10e9,
            poisson_ratio: 0.3,
            allowable_stress_pa: Some(100e6),
            temperature_factors: vec![[288.15, 1., 1.], [388.15, 0.5, 0.25]],
        });
        let (e, strength, _) = properties(&f, &c, 338.15);
        assert_eq!(e, Some(7.5e9));
        assert_eq!(strength, Some(62.5e6));
        assert_eq!(properties(&f, &h.columns[0], 288.15).0, None);
        h.columns[0].material = c.material;
        let out = solve(&f, &h, 32);
        assert!(out.points.iter().any(|p| p.kind == "nose"));
        assert!(out.points.iter().any(|p| p.kind == "body"));
        assert!(out.points.iter().any(|p| p.kind == "fin"));
        assert!(
            out.points
                .iter()
                .all(|p| p.stress_pa.is_finite() && p.stress_pa >= 0.)
        );
        assert!(out.at_column(0).unwrap().strain.is_some());
        assert!(
            out.points
                .iter()
                .filter(|p| p.kind == "body")
                .all(|p| p.strain.is_none())
        );
    }
    #[test]
    fn thin_material_section_is_resolved_at_its_boundaries_and_detached_stages_are_excluded() {
        let mut m = crate::config::Mission::demo(false);
        m.stages.push(m.stages[0].clone());
        m.duration = 0.2;
        for (i, s) in m.stages.iter_mut().enumerate() {
            s.geometry = Some(crate::config::StageGeometry::generic(s, i == 1));
        }
        let s = &mut m.stages[0];
        crate::assembly::explicit_shell(s);
        let mut ring = crate::assembly::Part::new(PartKind::Transition, s);
        ring.position_m = s.length_m * 0.513;
        ring.length_m = s.length_m * 0.001;
        ring.cg_m = ring.length_m * 0.5;
        ring.material.name = "Thin verification section".into();
        ring.material.mechanical = Some(crate::assembly::MechanicalMaterial {
            youngs_modulus_pa: 70e9,
            poisson_ratio: 0.3,
            allowable_stress_pa: Some(120e6),
            temperature_factors: vec![],
        });
        // Put it first so this explicit segment owns the small overlapping cut.
        s.components.insert(0, ring.clone());
        let f = crate::flight::Simulator::new(m).unwrap().run();
        let h = crate::thermal::solve(&crate::thermal::prepare_with_resolution(
            &f,
            crate::thermal::Resolution {
                axial_cells: 8,
                ..Default::default()
            },
        ));
        let column = h
            .columns
            .iter()
            .position(|c| c.material.name == ring.material.name)
            .expect("thin section must not disappear between grid centers");
        let bounds = h.columns[column].axial_bounds_m.unwrap();
        assert!(
            (bounds[0] - ring.position_m).abs() < 1e-9
                && (bounds[1] - ring.position_m - ring.length_m).abs() < 1e-9
        );
        let field = solve(&f, &h, 16);
        assert_eq!(
            field.at_column(column).unwrap().material,
            ring.material.name
        );
        assert!(field.at_column(column).unwrap().strain.is_some());
        let mut detached = f.clone();
        detached.samples.last_mut().unwrap().stage = 1;
        assert!(solve(&detached, &h, 16).points.iter().all(|p| p.stage >= 1));
    }
}
