//! Material heat diffusion driven by the solved flight. Two finite-volume cells
//! through each wall, axial conduction, fin-root and component contact links.
//! This is an engineering screening model, not a conjugate CFD calculation.
use crate::{
    assembly::{Material, Structure},
    atmosphere,
    config::NoseShape,
    flight::FlightResult,
};
use serde::Serialize;

pub const SIGMA: f64 = 5.670374419e-8;
#[derive(Clone, Debug, Serialize)]
pub struct Column {
    pub stage: usize,
    pub kind: String,
    pub station_m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axial_bounds_m: Option<[f64; 2]>,
    pub radial_fraction: f64,
    pub material: Material,
    pub thickness_m: f64,
    pub area_m2: f64,
    pub capacity_j_k: f64,
    pub radial_w_k: f64,
    pub links: Vec<(usize, f64)>,
    pub incidence: f64,
    pub exposed: bool,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Boundary {
    pub dt: f64,
    pub air_k: f64,
    pub recovery_k: f64,
    pub h_w_m2_k: f64,
    pub active_stage: usize,
    pub source_w: f64,
}
#[derive(Clone, Debug)]
pub struct Problem {
    pub columns: Vec<Column>,
    pub times: Vec<f64>,
    pub boundaries: Vec<Vec<Boundary>>,
    pub initial_k: f64,
    pub max_step: f64,
}
#[derive(Clone, Debug)]
pub struct History {
    pub columns: Vec<Column>,
    pub times: Vec<f64>,
    pub temperatures: Vec<Vec<[f64; 2]>>,
    pub backend: String,
}
impl History {
    pub fn fin_temperature(&self, time: f64, stage: usize, fraction: f64, inner: bool) -> f64 {
        let index = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.stage == stage && c.kind == "fin")
            .min_by(|(_, a), (_, b)| {
                (a.radial_fraction - fraction)
                    .abs()
                    .total_cmp(&(b.radial_fraction - fraction).abs())
            });
        let Some((index, _)) = index else {
            return self.temperature(time, stage, "body", 0.0, inner);
        };
        let frame = self.frame(time);
        let next = (frame + 1).min(self.times.len() - 1);
        let u = ((time - self.times[frame]) / (self.times[next] - self.times[frame]).max(1e-9))
            .clamp(0.0, 1.0);
        self.temperatures[frame][index][usize::from(inner)] * (1.0 - u)
            + self.temperatures[next][index][usize::from(inner)] * u
    }
    pub fn frame(&self, time: f64) -> usize {
        self.times.partition_point(|t| *t <= time).saturating_sub(1)
    }
    pub fn temperature(
        &self,
        time: f64,
        stage: usize,
        kind: &str,
        station: f64,
        inner: bool,
    ) -> f64 {
        let idx = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.stage == stage && c.kind == kind)
            .min_by(|(_, a), (_, b)| {
                (a.station_m - station)
                    .abs()
                    .total_cmp(&(b.station_m - station).abs())
            })
            .or_else(|| {
                self.columns
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.stage == stage)
                    .min_by(|(_, a), (_, b)| {
                        (a.station_m - station)
                            .abs()
                            .total_cmp(&(b.station_m - station).abs())
                    })
            });
        let Some((i, _)) = idx else {
            return 288.15;
        };
        let f = self.frame(time);
        let next = (f + 1).min(self.times.len() - 1);
        let u =
            ((time - self.times[f]) / (self.times[next] - self.times[f]).max(1e-9)).clamp(0.0, 1.0);
        self.temperatures[f][i][usize::from(inner)] * (1.0 - u)
            + self.temperatures[next][i][usize::from(inner)] * u
    }
    pub fn export(&self, directory: impl AsRef<std::path::Path>) -> crate::config::Result<()> {
        std::fs::create_dir_all(&directory)?;
        let mut csv = csv::Writer::from_path(directory.as_ref().join("thermal.csv"))?;
        csv.write_record([
            "time_s",
            "stage",
            "region",
            "station_m",
            "outer_c",
            "inner_c",
            "limit_c",
        ])?;
        for (time, values) in self.times.iter().zip(&self.temperatures) {
            for (c, t) in self.columns.iter().zip(values) {
                csv.serialize((
                    *time,
                    c.stage + 1,
                    &c.kind,
                    c.station_m,
                    t[0] - 273.15,
                    t[1] - 273.15,
                    c.material.thermal.limit_temperature_k - 273.15,
                ))?;
            }
        }
        csv.flush()?;
        std::fs::write(
            directory.as_ref().join("thermal-model.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"backend":self.backend,"columns":self.columns,"model":if self.backend.starts_with("OpenFOAM") {"OpenFOAM isotropic material-region wall strips; prescribed recovery-temperature convection, outer/inner half-wall averages. No radiation, internal sources, joints, interregion contacts, CFD-resolved wall flux, ablation or flight feedback."} else {"Two-cell wall diffusion; axial and estimated fin-root / component contacts; recovery-temperature convection and radiation. No ablation, failure, anisotropy, CFD, or feedback into flight dynamics."}}),
            )?,
        )?;
        Ok(())
    }
}
#[allow(clippy::too_many_arguments)] // Physical column descriptor; units are explicit at call sites.
fn add_column(
    columns: &mut Vec<Column>,
    stage: usize,
    kind: &str,
    station: f64,
    material: &Material,
    thickness: f64,
    area: f64,
    incidence: f64,
    exposed: bool,
) -> usize {
    let area = area.max(1e-8);
    let thickness = thickness.max(1e-6);
    let i = columns.len();
    columns.push(Column {
        stage,
        kind: kind.into(),
        station_m: station,
        axial_bounds_m: None,
        radial_fraction: 0.0,
        material: material.clone(),
        thickness_m: thickness,
        area_m2: area,
        capacity_j_k: material.density_kg_m3
            * material.thermal.specific_heat_j_kg_k
            * area
            * thickness
            / 2.0,
        radial_w_k: 2.0 * material.thermal.conductivity_w_m_k * area / thickness,
        links: vec![],
        incidence,
        exposed,
    });
    i
}
fn connect(columns: &mut [Column], a: usize, b: usize, g: f64) {
    if a != b && g > 0.0 {
        columns[a].links.push((b, g));
        columns[b].links.push((a, g));
    }
}
/// Bounded spatial and integration resolution. The wall remains two cells thick.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Resolution {
    pub axial_cells: usize,
    pub fin_cells: usize,
    pub max_step_s: f64,
}
impl Default for Resolution {
    fn default() -> Self {
        Self {
            axial_cells: 16,
            fin_cells: 4,
            max_step_s: 0.05,
        }
    }
}
pub fn prepare(flight: &FlightResult) -> Problem {
    prepare_with_resolution(flight, Resolution::default())
}
pub fn prepare_with_resolution(flight: &FlightResult, resolution: Resolution) -> Problem {
    let axial = resolution.axial_cells.clamp(8, 256);
    let fins = resolution.fin_cells.clamp(2, 64);
    let step = if resolution.max_step_s.is_finite() {
        resolution.max_step_s.clamp(0.001, 0.05)
    } else {
        0.05
    };
    let m = &flight.mission;
    let mut columns = Vec::new();
    let mut ends = Vec::new();
    for (stage, s) in m.stages.iter().enumerate() {
        let defaults = Structure::for_stage(s);
        let materials = s.materials.as_ref().unwrap_or(&defaults);
        let g = s.geometry.as_ref();
        let nose = g.map_or(0.0, |g| g.nose_length_m);
        let mut bounds = (0..=axial)
            .map(|i| s.length_m * i as f64 / axial as f64)
            .collect::<Vec<_>>();
        if nose > 0. && nose < s.length_m {
            bounds.push(nose);
        }
        {
            for p in s.components.iter().filter(|p| {
                matches!(
                    p.kind,
                    crate::assembly::PartKind::BodyTube
                        | crate::assembly::PartKind::NoseCone
                        | crate::assembly::PartKind::Transition
                ) && (materials.component_shell
                    || p.kind == crate::assembly::PartKind::Transition && p.position_m.abs() < 1e-6)
            }) {
                bounds.extend(
                    [p.position_m, p.position_m + p.length_m].map(|x| x.clamp(0., s.length_m)),
                );
            }
        }
        bounds.sort_by(f64::total_cmp);
        bounds.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        let axial = bounds.len() - 1;
        let r = s.diameter_m * 0.5;
        let first = columns.len();
        for j in 0..axial {
            let dx = bounds[j + 1] - bounds[j];
            let station = (bounds[j + 1] + bounds[j]) * 0.5;
            let is_nose = station < nose;
            let mut t = if is_nose {
                materials.nose_wall_m
            } else {
                materials.airframe_wall_m
            };
            let mut material = if is_nose {
                &materials.nose
            } else {
                &materials.airframe
            };
            let shell_part = s.components.iter().find(|p| {
                matches!(
                    p.kind,
                    crate::assembly::PartKind::BodyTube
                        | crate::assembly::PartKind::NoseCone
                        | crate::assembly::PartKind::Transition
                ) && (materials.component_shell
                    || p.kind == crate::assembly::PartKind::Transition && p.position_m.abs() < 1e-6)
                    && station >= p.position_m
                    && station <= p.position_m + p.length_m
            });
            if let Some(part) = shell_part {
                material = &part.material;
                t = part.thickness_m;
            }
            let radius = |x: f64| {
                if let Some(part) =
                    shell_part.filter(|p| p.kind == crate::assembly::PartKind::Transition)
                {
                    let u = ((x - part.position_m) / part.length_m).clamp(0.0, 1.0);
                    return (part.fore_diameter_m + (part.diameter_m - part.fore_diameter_m) * u)
                        * 0.5;
                }
                if x >= nose || nose <= 0.0 {
                    r
                } else {
                    let u = (x / nose).clamp(0.0001, 1.0);
                    match g.unwrap().nose_shape {
                        NoseShape::Conical => r * u,
                        NoseShape::Elliptical => r * (1.0 - (1.0 - u).powi(2)).sqrt(),
                        NoseShape::TangentOgive => {
                            let rho = (r * r + nose * nose) / (2.0 * r);
                            (rho * rho - (nose - x).powi(2)).max(0.0).sqrt() + r - rho
                        }
                    }
                }
            };
            let slope = (radius((station + dx * 0.1).min(s.length_m))
                - radius((station - dx * 0.1).max(0.0)))
                / (dx * 0.2);
            let incidence = if is_nose
                || shell_part.is_some_and(|p| p.kind == crate::assembly::PartKind::Transition)
            {
                slope.abs() / slope.hypot(1.0)
            } else {
                0.0
            };
            let area = std::f64::consts::TAU
                * radius(station).max(s.nose_radius_m)
                * dx
                * slope.hypot(1.0);
            let idx = add_column(
                &mut columns,
                stage,
                if is_nose { "nose" } else { "body" },
                station,
                material,
                t,
                area,
                incidence,
                true,
            );
            columns[idx].axial_bounds_m = Some([bounds[j], bounds[j + 1]]);
            if j > 0 {
                let p = &columns[idx - 1];
                let k = 2.0
                    / (1.0 / material.thermal.conductivity_w_m_k
                        + 1.0 / p.material.thermal.conductivity_w_m_k);
                let spacing = station - p.station_m;
                connect(
                    &mut columns,
                    idx - 1,
                    idx,
                    k * std::f64::consts::TAU * radius(station) * t / spacing,
                );
            }
        }
        ends.push((first, first + axial - 1));
        if let Some(g) = g {
            let area =
                (g.fin_root_chord_m + g.fin_tip_chord_m) * 0.5 * g.fin_span_m * g.fin_count as f64;
            let mut last = None;
            for j in 0..fins {
                let idx = add_column(
                    &mut columns,
                    stage,
                    "fin",
                    g.fin_position_m + (g.fin_root_chord_m * 0.5),
                    &materials.fins,
                    g.fin_thickness_m,
                    area / fins as f64,
                    0.08,
                    true,
                );
                let conductance = materials.fins.thermal.conductivity_w_m_k
                    * g.fin_thickness_m
                    * g.fin_root_chord_m
                    * g.fin_count as f64
                    / (g.fin_span_m / fins as f64).max(1e-5);
                let root = first
                    + bounds
                        .partition_point(|x| *x <= g.fin_position_m)
                        .saturating_sub(1)
                        .min(axial - 1);
                connect(&mut columns, last.unwrap_or(root), idx, conductance);
                last = Some(idx);
                // Fin stations are stored axially; the cells resolve root-to-tip diffusion.
                columns[idx].station_m += j as f64 * 1e-7;
                columns[idx].radial_fraction = (j as f64 + 0.5) / fins as f64;
            }
        }
        for part in &s.components {
            // A front reducer is already resolved by the axial shell grid.
            if part.kind == crate::assembly::PartKind::Transition && part.position_m.abs() < 1e-6 {
                continue;
            }
            if materials.component_shell
                && matches!(
                    part.kind,
                    crate::assembly::PartKind::BodyTube
                        | crate::assembly::PartKind::NoseCone
                        | crate::assembly::PartKind::Transition
                        | crate::assembly::PartKind::FinSet
                )
            {
                continue;
            }
            if part.mass_override && part.mass_kg == 0.0 {
                continue;
            }
            let idx = add_column(
                &mut columns,
                stage,
                &part.name,
                part.cg(),
                &part.material,
                part.thickness_m,
                std::f64::consts::PI * part.diameter_m * part.length_m,
                0.0,
                false,
            );
            let root = first
                + bounds
                    .partition_point(|x| *x <= part.cg())
                    .saturating_sub(1)
                    .min(axial - 1);
            if part.mass_override {
                columns[idx].capacity_j_k =
                    (part.mass_kg * part.material.thermal.specific_heat_j_kg_k / 2.0).max(1e-6);
            }
            // Effective contact: 1% area, includes unspecified bond/contact resistance.
            let k = part
                .material
                .thermal
                .conductivity_w_m_k
                .min(materials.airframe.thermal.conductivity_w_m_k);
            let contact = k * columns[idx].area_m2 * 0.01 / 0.002;
            connect(&mut columns, root, idx, contact);
        }
    }
    // A short effective coupler contact links adjoining stage shells while
    // attached. These links are cut by the flight's actual separation events.
    for stage in 0..m.stages.len() - 1 {
        let a = ends[stage].0;
        let b = ends[stage + 1].1;
        let k = columns[a]
            .material
            .thermal
            .conductivity_w_m_k
            .min(columns[b].material.thermal.conductivity_w_m_k);
        let conductance =
            k * std::f64::consts::PI * m.stages[stage].diameter_m * columns[a].thickness_m / 0.02;
        connect(&mut columns, a, b, conductance);
    }
    // Effective composite directions supplied by the user. Scale symmetric
    // contacts with harmonic directional ratios, preserving energy symmetry.
    let factors: Vec<_> = columns
        .iter()
        .map(|c| {
            m.experiments
                .thermal
                .iter()
                .find(|l| l.stage == c.stage && l.region == c.kind)
                .map_or(1.0, |l| {
                    l.axial_k_w_m_k / c.material.thermal.conductivity_w_m_k
                })
        })
        .collect();
    for (i, c) in columns.iter_mut().enumerate() {
        if let Some(l) = m
            .experiments
            .thermal
            .iter()
            .find(|l| l.stage == c.stage && l.region == c.kind)
        {
            c.radial_w_k = 2.0 * l.through_k_w_m_k * c.area_m2 / c.thickness_m;
        }
        for (j, g) in &mut c.links {
            *g *= 2.0 / (1.0 / factors[i] + 1.0 / factors[*j]);
        }
    }
    let initial_k = atmosphere::for_mission(m.launch.altitude_m, m).temperature;
    let max_step = columns
        .iter()
        .map(|c| {
            let sum = c.links.iter().map(|(_, g)| g).sum::<f64>();
            if sum > 0.0 {
                0.2 * c.capacity_j_k / sum
            } else {
                0.05
            }
        })
        .fold(step, f64::min)
        .max(1e-5);
    let mut times = Vec::new();
    let mut boundaries = Vec::new();
    let mut previous = 0.0;
    for sample in &flight.samples {
        let dt = sample.time_s - previous;
        previous = sample.time_s;
        times.push(sample.time_s);
        boundaries.push(
            columns
                .iter()
                .map(|c| {
                    let (altitude, mach, alpha) = if c.stage >= sample.stage {
                        (
                            sample.altitude_m,
                            sample.mach,
                            sample.alpha_deg.to_radians(),
                        )
                    } else if let Some(d) = flight
                        .debris_samples
                        .iter()
                        .rev()
                        .find(|d| d.stage == c.stage && d.time_s <= sample.time_s)
                    {
                        let wind = crate::environment::wind(
                            m,
                            d.altitude_agl_m + m.launch.altitude_m,
                            d.time_s,
                        );
                        let relative = glam::DVec3::new(d.vx, d.vy, d.vz) - wind;
                        let body =
                            glam::DQuat::from_xyzw(d.qx, d.qy, d.qz, d.qw).conjugate() * relative;
                        (
                            d.altitude_agl_m + m.launch.altitude_m,
                            d.mach,
                            body.x.hypot(body.y).atan2(body.z),
                        )
                    } else {
                        (m.launch.altitude_m, 0.0, 0.0)
                    };
                    let air = atmosphere::for_mission(altitude, m);
                    let speed = mach * air.sound_speed;
                    let recovery = air.temperature * (1.0 + 0.9 * 0.2 * mach * mach);
                    let length = c.station_m.max(0.01);
                    let re = air.density * speed * length / air.viscosity;
                    let flat = 0.332 * re.sqrt() * 0.71_f64.cbrt() * 0.026 / length;
                    let sg = 1.83e-4
                        * (air.density / m.stages[c.stage].nose_radius_m).sqrt()
                        * speed.powi(3);
                    let incidence =
                        (c.incidence * alpha.cos().abs() + alpha.sin().abs() * 0.5).min(1.0);
                    let h = if c.exposed {
                        flat.max(sg / (recovery - air.temperature).max(1.0) * incidence.powi(2))
                    } else {
                        0.0
                    };
                    Boundary {
                        dt,
                        air_k: air.temperature,
                        recovery_k: recovery,
                        h_w_m2_k: h,
                        active_stage: sample.stage,
                        source_w: m
                            .experiments
                            .thermal
                            .iter()
                            .find(|l| {
                                l.stage == c.stage
                                    && l.region == c.kind
                                    && sample.time_s <= l.motor_heat_until_s
                            })
                            .map_or(0.0, |l| {
                                let area: f64 = columns
                                    .iter()
                                    .filter(|a| a.stage == c.stage && a.kind == c.kind)
                                    .map(|a| a.area_m2)
                                    .sum();
                                l.motor_heat_w * c.area_m2 / area.max(1e-9)
                            }),
                    }
                })
                .collect(),
        );
    }
    Problem {
        columns,
        times,
        boundaries,
        initial_k,
        max_step,
    }
}
/// Backward Euler through the wall (exact 2x2 solve), conservative explicit
/// axial/contact exchange. Radiation is linearized about the previous skin.
pub fn advance(
    columns: &[Column],
    old: &[[f64; 2]],
    boundaries: &[Boundary],
    dt: f64,
) -> Vec<[f64; 2]> {
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let b = boundaries[i];
            let t = old[i];
            let axial = c
                .links
                .iter()
                .filter(|(j, _)| {
                    columns[*j].stage == c.stage
                        || (columns[*j].stage >= b.active_stage && c.stage >= b.active_stage)
                })
                .map(|(j, g)| g * (old[*j][0] - t[0]))
                .sum::<f64>();
            let inner = c
                .links
                .iter()
                .filter(|(j, _)| {
                    columns[*j].stage == c.stage
                        || (columns[*j].stage >= b.active_stage && c.stage >= b.active_stage)
                })
                .map(|(j, g)| g * (old[*j][1] - t[1]))
                .sum::<f64>();
            let capacity = c.capacity_j_k / dt;
            let conv = b.h_w_m2_k * c.area_m2;
            let rad = if c.exposed {
                c.material.thermal.emissivity
                    * SIGMA
                    * c.area_m2
                    * (t[0] + b.air_k)
                    * (t[0] * t[0] + b.air_k * b.air_k)
            } else {
                0.0
            };
            let a = capacity + c.radial_w_k + conv + rad;
            let d = capacity + c.radial_w_k;
            let x = capacity * t[0] + axial + conv * b.recovery_k + rad * b.air_k;
            let y = capacity * t[1] + inner + b.source_w;
            let determinant = capacity * (capacity + 2.0 * c.radial_w_k + conv + rad)
                + c.radial_w_k * (conv + rad);
            [
                (x * d + c.radial_w_k * y) / determinant,
                (a * y + c.radial_w_k * x) / determinant,
            ]
        })
        .collect()
}
pub fn solve(problem: &Problem) -> History {
    solve_with_progress(problem, |_, _| {})
}
pub fn solve_with_progress(problem: &Problem, mut progress: impl FnMut(usize, usize)) -> History {
    let mut state = vec![[problem.initial_k; 2]; problem.columns.len()];
    let mut frames = Vec::new();
    for boundaries in &problem.boundaries {
        let interval = boundaries.first().map_or(0.0, |b| b.dt);
        let n = (interval / problem.max_step).ceil().max(1.0) as usize;
        if interval > 0.0 {
            for _ in 0..n {
                state = advance(&problem.columns, &state, boundaries, interval / n as f64);
            }
        }
        frames.push(state.clone());
        progress(frames.len(), problem.boundaries.len());
    }
    History {
        columns: problem.columns.clone(),
        times: problem.times.clone(),
        temperatures: frames,
        backend: "CPU / f64 finite volume".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_material_files_recover_the_matching_thermal_properties() {
        let aluminum: Material =
            serde_json::from_str(r#"{"name":"Aluminum","density_kg_m3":2700.0}"#).unwrap();
        assert_eq!(aluminum.thermal.conductivity_w_m_k, 167.0);
        let custom: Material =
            serde_json::from_str(r#"{"name":"Custom material","density_kg_m3":2000.0}"#).unwrap();
        assert!(custom.thermal.valid());
        let mut m = crate::config::Mission::demo(false);
        m.stages[0].materials = Some(Structure::for_stage(&m.stages[0]));
        m.stages[0]
            .materials
            .as_mut()
            .unwrap()
            .airframe
            .thermal
            .conductivity_w_m_k = -1.0;
        assert!(m.validate().is_err());
    }
    fn columns(material: Material) -> Vec<Column> {
        let mut c = vec![];
        add_column(&mut c, 0, "body", 0.0, &material, 0.002, 1.0, 0.0, false);
        c
    }
    #[test]
    fn insulated_wall_conserves_energy_and_equilibrates() {
        let c = columns(Material::presets().remove(2));
        let mut t = vec![[500.0, 300.0]];
        let b = vec![Boundary {
            dt: 1.0,
            air_k: 300.0,
            recovery_k: 300.0,
            h_w_m2_k: 0.0,
            active_stage: 0,
            source_w: 0.0,
        }];
        for _ in 0..30 {
            t = advance(&c, &t, &b, 0.1);
        }
        assert!((t[0][0] + t[0][1] - 800.0).abs() < 1e-7);
        assert!((t[0][0] - t[0][1]).abs() < 0.01);
    }
    #[test]
    fn aluminum_transmits_heat_faster_than_fiberglass() {
        let mut c = columns(Material::presets().remove(2));
        let mut f = columns(Material::presets().remove(0));
        c[0].exposed = true;
        f[0].exposed = true;
        let b = vec![Boundary {
            dt: 1.0,
            air_k: 300.0,
            recovery_k: 700.0,
            h_w_m2_k: 400.0,
            active_stage: 0,
            source_w: 0.0,
        }];
        let a = advance(&c, &[[300.0; 2]], &b, 0.1);
        let g = advance(&f, &[[300.0; 2]], &b, 0.1);
        assert!(a[0][1] > g[0][1]);
        assert!(g[0][0] > g[0][1]);
    }
    #[test]
    fn axial_diffusion_conserves_energy_and_reaches_an_internal_part() {
        let mut c = columns(Material::presets().remove(0));
        c.push(c[0].clone());
        connect(&mut c, 0, 1, 5.0);
        let b = vec![
            Boundary {
                dt: 1.0,
                air_k: 300.0,
                recovery_k: 300.0,
                h_w_m2_k: 0.0,
                active_stage: 0,
                source_w: 0.0,
            };
            2
        ];
        let t = advance(&c, &[[500.0; 2], [300.0; 2]], &b, 0.01);
        assert!(t[1][1] > 300.0);
        assert!((t.iter().flatten().sum::<f64>() - 1600.0).abs() < 1e-8);
    }
    #[test]
    fn stage_separation_cuts_thermal_contact() {
        let mut c = columns(Material::presets().remove(0));
        c.push(c[0].clone());
        c[1].stage = 1;
        connect(&mut c, 0, 1, 5.0);
        let b = vec![
            Boundary {
                dt: 1.0,
                air_k: 300.0,
                recovery_k: 300.0,
                h_w_m2_k: 0.0,
                active_stage: 1,
                source_w: 0.0,
            };
            2
        ];
        let t = advance(&c, &[[500.0; 2], [300.0; 2]], &b, 0.01);
        assert!((t[1][1] - 300.0).abs() < 1e-10);
        assert!((t[0][0] - 500.0).abs() < 1e-10);
    }
}
