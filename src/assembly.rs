//! Component bookkeeping. Density values are editable engineering defaults, not certifications.
use crate::config::{Result, Stage};
use serde::{Deserialize, Serialize};
pub const KG_PER_LB: f64 = 0.45359237;
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub name: String,
    pub density_kg_m3: f64,
    pub thermal: ThermalMaterial,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mechanical: Option<MechanicalMaterial>,
}
impl<'de> Deserialize<'de> for Material {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Saved {
            name: String,
            density_kg_m3: f64,
            #[serde(default)]
            thermal: Option<ThermalMaterial>,
            #[serde(default)]
            mechanical: Option<MechanicalMaterial>,
        }
        let saved = Saved::deserialize(deserializer)?;
        let thermal = saved.thermal.unwrap_or_else(|| {
            Material::presets()
                .into_iter()
                .find(|m| m.name == saved.name)
                .map(|m| m.thermal)
                .unwrap_or_default()
        });
        Ok(Self {
            name: saved.name,
            density_kg_m3: saved.density_kg_m3,
            thermal,
            mechanical: saved.mechanical,
        })
    }
}
/// Entered isotropic structural data; an absent value never invents strength.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanicalMaterial {
    pub youngs_modulus_pa: f64,
    pub poisson_ratio: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowable_stress_pa: Option<f64>,
    /// Temperature K, modulus factor, strength factor; empty means no degradation law.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub temperature_factors: Vec<[f64; 3]>,
}
impl MechanicalMaterial {
    pub fn valid(&self) -> bool {
        self.youngs_modulus_pa.is_finite()
            && self.youngs_modulus_pa > 0.
            && self.poisson_ratio.is_finite()
            && (-1.0..0.5).contains(&self.poisson_ratio)
            && self
                .allowable_stress_pa
                .is_none_or(|s| s.is_finite() && s > 0.)
            && self
                .temperature_factors
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite()) && p[0] > 0. && p[1] > 0. && p[2] > 0.)
            && self
                .temperature_factors
                .windows(2)
                .all(|p| p[0][0] < p[1][0])
    }
    pub fn at(&self, temperature: f64) -> (f64, Option<f64>) {
        let factors = if self.temperature_factors.is_empty() {
            [1., 1.]
        } else {
            let i = self
                .temperature_factors
                .partition_point(|p| p[0] <= temperature)
                .saturating_sub(1);
            let a = self.temperature_factors[i];
            let b = self.temperature_factors[(i + 1).min(self.temperature_factors.len() - 1)];
            let u = ((temperature - a[0]) / (b[0] - a[0]).max(1e-9)).clamp(0., 1.);
            [a[1] + u * (b[1] - a[1]), a[2] + u * (b[2] - a[2])]
        };
        (
            self.youngs_modulus_pa * factors[0],
            self.allowable_stress_pa.map(|s| s * factors[1]),
        )
    }
}
/// Effective, isotropic engineering properties. Composites require measured values.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThermalMaterial {
    pub conductivity_w_m_k: f64,
    pub specific_heat_j_kg_k: f64,
    pub emissivity: f64,
    pub limit_temperature_k: f64,
}
impl Default for ThermalMaterial {
    fn default() -> Self {
        Self {
            conductivity_w_m_k: 0.3,
            specific_heat_j_kg_k: 900.0,
            emissivity: 0.85,
            limit_temperature_k: 423.15,
        }
    }
}
impl ThermalMaterial {
    pub fn valid(&self) -> bool {
        self.conductivity_w_m_k.is_finite()
            && self.conductivity_w_m_k > 0.0
            && self.specific_heat_j_kg_k.is_finite()
            && self.specific_heat_j_kg_k > 0.0
            && self.emissivity.is_finite()
            && (0.0..=1.0).contains(&self.emissivity)
            && self.limit_temperature_k.is_finite()
            && self.limit_temperature_k > 0.0
    }
}
impl Material {
    pub fn presets() -> Vec<Self> {
        [
            ("Fiberglass / G10", 1850.0),
            ("Carbon composite", 1600.0),
            ("Aluminum", 2700.0),
            ("Birch plywood", 650.0),
            ("Balsa", 160.0),
            ("Cardboard", 680.0),
            ("PLA", 1240.0),
            ("Steel", 7850.0),
            ("Nylon", 1150.0),
        ]
        .into_iter()
        .map(|(name, density)| Self {
            name: name.into(),
            density_kg_m3: density,
            mechanical: None,
            thermal: {
                let (k, cp, limit) = match name {
                    "Carbon composite" => (1.0, 900.0, 423.15),
                    "Aluminum" => (167.0, 900.0, 573.15),
                    "Birch plywood" => (0.13, 1700.0, 423.15),
                    "Balsa" => (0.05, 1700.0, 423.15),
                    "Cardboard" => (0.08, 1400.0, 373.15),
                    "PLA" => (0.13, 1800.0, 333.15),
                    "Steel" => (16.0, 500.0, 873.15),
                    "Nylon" => (0.25, 1700.0, 393.15),
                    _ => (0.3, 900.0, 423.15),
                };
                ThermalMaterial {
                    conductivity_w_m_k: k,
                    specific_heat_j_kg_k: cp,
                    emissivity: 0.85,
                    limit_temperature_k: limit,
                }
            },
        })
        .collect()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Structure {
    #[serde(default)]
    pub component_shell: bool,
    pub use_material_mass: bool,
    pub airframe: Material,
    pub nose: Material,
    pub fins: Material,
    pub airframe_wall_m: f64,
    pub nose_wall_m: f64,
    /// Hardware / empty motor casing mass, in addition to the computed shells and fins.
    pub hardware_mass_kg: f64,
    pub hardware_cg_m: f64,
    pub finish: String,
}
impl Structure {
    pub fn for_stage(s: &Stage) -> Self {
        let material = Material::presets().remove(0);
        Self {
            component_shell: false,
            use_material_mass: false,
            airframe: material.clone(),
            nose: material.clone(),
            fins: material,
            airframe_wall_m: 0.002,
            nose_wall_m: 0.002,
            hardware_mass_kg: 0.0,
            hardware_cg_m: s.length_m * 0.65,
            finish: "Smooth paint".into(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    CustomMesh,
    BodyTube,
    NoseCone,
    Transition,
    FinSet,
    EllipticalFin,
    FreeformFin,
    TubeFin,
    LaunchLug,
    EngineBlock,
    Streamer,
    Parachute,
    PodSet,
    InnerTube,
    Coupler,
    Bulkhead,
    CenteringRing,
    RailButton,
    ShockCord,
    Mass,
    Avionics,
}
impl PartKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::CustomMesh => "Imported model",
            Self::BodyTube => "Body tube",
            Self::NoseCone => "Nose cone",
            Self::Transition => "Transition",
            Self::FinSet => "Trapezoidal fins",
            Self::EllipticalFin => "Elliptical fins",
            Self::FreeformFin => "Freeform fins",
            Self::TubeFin => "Tube fins",
            Self::LaunchLug => "Launch lug",
            Self::EngineBlock => "Engine block",
            Self::Streamer => "Streamer",
            Self::Parachute => "Parachute",
            Self::PodSet => "Pods",
            Self::InnerTube => "Inner / motor tube",
            Self::Coupler => "Tube coupler",
            Self::Bulkhead => "Bulkhead",
            Self::CenteringRing => "Centering ring",
            Self::RailButton => "Rail button",
            Self::ShockCord => "Shock cord",
            Self::Mass => "Mass / ballast",
            Self::Avionics => "Avionics bay",
        }
    }
    pub fn planar_fin(self) -> bool {
        matches!(self, Self::FinSet | Self::EllipticalFin | Self::FreeformFin)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    /// Index of the containing tube in this stage. Positions remain stage-local.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_component: Option<usize>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_ring_fit: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub motor_mount: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub motor_length_m: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub motor_diameter_m: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub motor_overhang_m: f64,
    /// Embedded, centered +Y-nose triangle geometry; portable with saved missions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<crate::exchange::Geometry>,
    #[serde(default)]
    pub auto_tube_diameter: bool,
    #[serde(default = "one")]
    pub instance_count: u32,
    #[serde(default)]
    pub radial_offset_m: f64,
    #[serde(default)]
    pub radial_angle_deg: f64,
    #[serde(default)]
    pub fin_points: Vec<[f64; 2]>,
    #[serde(default)]
    pub comments: String,
    #[serde(default = "normal_finish")]
    pub finish: String,
    #[serde(default = "part_color")]
    pub appearance_rgba: [f32; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cd_override: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<crate::config::Chute>,
    #[serde(default)]
    pub deploy_at_apogee: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<crate::config::StageGeometry>,
    #[serde(default)]
    pub fore_diameter_m: f64,
    pub kind: PartKind,
    pub name: String,
    pub position_m: f64,
    /// Manual adjustment from the snapped attachment, independent of stage origin.
    #[serde(default)]
    pub axial_offset_m: f64,
    pub length_m: f64,
    pub diameter_m: f64,
    pub thickness_m: f64,
    pub material: Material,
    pub mass_override: bool,
    pub mass_kg: f64,
    pub cg_override: bool,
    pub cg_m: f64,
}
fn one() -> u32 {
    1
}
fn is_false(v: &bool) -> bool {
    !v
}
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}
fn normal_finish() -> String {
    "Normal".into()
}
fn part_color() -> [f32; 4] {
    [0.96, 0.61, 0.18, 1.0]
}
/// Ear clipping of a simple polygon; supports concave freeform fins.
pub fn triangulate_fin(points: &[[f64; 2]]) -> Result<Vec<[usize; 3]>> {
    if points.len() < 3 || points.len() > 64 {
        return Err("Fin outline needs 3–64 points".into());
    }
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let area = (0..points.len())
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>();
    if area.abs() < 1e-12 {
        return Err("Fin outline has zero area".into());
    }
    for i in 0..points.len() {
        for j in i + 1..points.len() {
            let next_i = (i + 1) % points.len();
            let next_j = (j + 1) % points.len();
            if next_i == j || next_j == i {
                continue;
            }
            let a = points[i];
            let b = points[next_i];
            let c = points[j];
            let d = points[next_j];
            if cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0 {
                return Err("Fin outline crosses itself".into());
            }
        }
    }
    let sign = area.signum();
    let mut remaining = (0..points.len()).collect::<Vec<_>>();
    let mut triangles = Vec::new();
    while remaining.len() > 3 {
        let ear = (0..remaining.len())
            .find(|&i| {
                let a = remaining[(i + remaining.len() - 1) % remaining.len()];
                let b = remaining[i];
                let c = remaining[(i + 1) % remaining.len()];
                if sign * cross(points[a], points[b], points[c]) <= 1e-14 {
                    return false;
                }
                !remaining.iter().any(|&p| {
                    p != a
                        && p != b
                        && p != c
                        && sign * cross(points[a], points[b], points[p]) >= -1e-14
                        && sign * cross(points[b], points[c], points[p]) >= -1e-14
                        && sign * cross(points[c], points[a], points[p]) >= -1e-14
                })
            })
            .ok_or("Invalid or degenerate fin outline")?;
        triangles.push([
            remaining[(ear + remaining.len() - 1) % remaining.len()],
            remaining[ear],
            remaining[(ear + 1) % remaining.len()],
        ]);
        remaining.remove(ear);
    }
    triangles.push([remaining[0], remaining[1], remaining[2]]);
    Ok(triangles)
}
impl Part {
    pub fn size_tube_fins(&mut self, body_diameter: f64) {
        if self.kind == PartKind::TubeFin && self.auto_tube_diameter && self.instance_count >= 3 {
            let sine = (std::f64::consts::PI / self.instance_count as f64).sin();
            self.diameter_m = body_diameter * sine / (1.0 - sine);
            self.radial_offset_m = (body_diameter + self.diameter_m) * 0.5;
        }
    }
    pub fn new(kind: PartKind, s: &Stage) -> Self {
        let d = if matches!(kind, PartKind::RailButton | PartKind::LaunchLug) {
            0.015_f64.min(s.diameter_m * 0.3)
        } else if kind == PartKind::TubeFin {
            s.diameter_m
        } else if kind == PartKind::PodSet {
            s.diameter_m * 0.45
        } else {
            s.diameter_m * 0.8
        };
        let length = match kind {
            PartKind::Bulkhead | PartKind::CenteringRing | PartKind::EngineBlock => 0.006,
            PartKind::RailButton => 0.012,
            _ => (s.length_m * 0.1).min(0.25),
        }
        .min(s.length_m * 0.1);
        let mut part = Self {
            parent_component: None,
            auto_ring_fit: false,
            motor_mount: false,
            motor_length_m: 0.0,
            motor_diameter_m: 0.0,
            motor_overhang_m: 0.0,
            mesh: None,
            auto_tube_diameter: kind == PartKind::TubeFin,
            instance_count: if kind == PartKind::TubeFin {
                6
            } else if kind == PartKind::PodSet {
                2
            } else {
                1
            },
            radial_offset_m: if matches!(
                kind,
                PartKind::LaunchLug | PartKind::RailButton | PartKind::TubeFin | PartKind::PodSet
            ) {
                s.diameter_m * 0.5 + d * 0.5
            } else {
                0.0
            },
            radial_angle_deg: 0.0,
            fin_points: if kind == PartKind::FreeformFin {
                vec![
                    [0.0, 0.0],
                    [length * 0.5, d],
                    [length * 0.8, d],
                    [length, 0.0],
                ]
            } else {
                Vec::new()
            },
            comments: String::new(),
            finish: normal_finish(),
            appearance_rgba: part_color(),
            cd_override: None,
            recovery: matches!(kind, PartKind::Parachute | PartKind::Streamer).then_some(
                crate::config::Chute {
                    enabled: true,
                    area_m2: if kind == PartKind::Streamer { 0.2 } else { 1.0 },
                    cd: if kind == PartKind::Streamer { 0.4 } else { 1.5 },
                    inflation_s: 1.0,
                    max_q_pa: 5000.0,
                    max_mach: 0.5,
                    max_load_n: 10000.0,
                },
            ),
            deploy_at_apogee: true,
            shape: if kind.planar_fin() || kind == PartKind::NoseCone {
                Some(crate::config::StageGeometry::generic(s, true))
            } else {
                None
            },
            fore_diameter_m: d * 0.7,
            kind,
            name: kind.label().into(),
            position_m: s.length_m * 0.65,
            axial_offset_m: 0.0,
            length_m: length,
            diameter_m: d,
            thickness_m: if matches!(kind, PartKind::CenteringRing | PartKind::EngineBlock) {
                d * 0.225
            } else {
                0.002_f64.min(d * 0.1)
            },
            material: Material::presets().remove(0),
            mass_override: matches!(
                kind,
                PartKind::Mass
                    | PartKind::Avionics
                    | PartKind::ShockCord
                    | PartKind::RailButton
                    | PartKind::Streamer
                    | PartKind::Parachute
            ),
            mass_kg: 0.1,
            cg_override: false,
            cg_m: length * 0.5,
        };
        if kind.planar_fin() {
            let g = part.shape.as_mut().unwrap();
            g.fin_root_chord_m = length * 0.7;
            g.fin_tip_chord_m = length * 0.3;
            g.fin_sweep_m = length * 0.5;
            g.fin_position_m = 0.0;
            g.nose_length_m = 0.0;
            part.length_m = g.fin_root_chord_m.max(g.fin_sweep_m + g.fin_tip_chord_m);
            part.diameter_m = s.diameter_m;
            if kind == PartKind::FreeformFin {
                part.fin_points = vec![
                    [0.0, 0.0],
                    [g.fin_sweep_m, g.fin_span_m],
                    [g.fin_sweep_m + g.fin_tip_chord_m, g.fin_span_m],
                    [g.fin_root_chord_m, 0.0],
                ];
            }
            part.cg_m = part.length_m * 0.5;
        }
        part
    }

    pub fn mass(&self) -> f64 {
        self.single_mass() * self.instance_count as f64
    }
    fn single_mass(&self) -> f64 {
        if self.mass_override {
            return self.mass_kg;
        }
        if let Some(mesh) = &self.mesh {
            return mesh.volume_m3 * (self.diameter_m / mesh.diameter_m).powi(2) * self.length_m
                / mesh.length_m
                * self.material.density_kg_m3;
        }
        let r = self.diameter_m * 0.5;
        if self.kind.planar_fin()
            && let Some(g) = &self.shape
        {
            return self.fin_area_cg().0
                * g.fin_thickness_m
                * g.fin_count as f64
                * self.material.density_kg_m3;
        }
        if matches!(self.kind, PartKind::NoseCone | PartKind::Transition) {
            let fore = if self.kind == PartKind::NoseCone {
                0.0
            } else {
                self.fore_diameter_m * 0.5
            };
            return std::f64::consts::PI
                * (r + fore)
                * (r - fore).hypot(self.length_m)
                * self.thickness_m
                * self.material.density_kg_m3;
        }
        let inner = match self.kind {
            PartKind::Bulkhead | PartKind::Avionics | PartKind::Mass => 0.0,
            _ => (r - self.thickness_m).max(0.0),
        };
        std::f64::consts::PI * (r * r - inner * inner) * self.length_m * self.material.density_kg_m3
    }
    pub fn cg(&self) -> f64 {
        self.position_m
            + if self.cg_override {
                self.cg_m
            } else if self.kind.planar_fin() && self.shape.is_some() {
                self.fin_area_cg().1
            } else if let Some(mesh) = &self.mesh {
                self.length_m * (0.5 - mesh.centroid_m[1] / mesh.length_m)
            } else if self.kind == PartKind::NoseCone {
                self.length_m * 2.0 / 3.0
            } else {
                self.length_m * 0.5
            }
    }
    pub fn external(&self) -> bool {
        matches!(
            self.kind,
            PartKind::CustomMesh
                | PartKind::BodyTube
                | PartKind::NoseCone
                | PartKind::Transition
                | PartKind::FinSet
                | PartKind::EllipticalFin
                | PartKind::FreeformFin
                | PartKind::TubeFin
                | PartKind::LaunchLug
                | PartKind::PodSet
                | PartKind::RailButton
        )
    }
    /// Outline coordinates: x aft of fin leading edge, y outboard from body.
    pub fn fin_outline(&self) -> Vec<[f64; 2]> {
        let Some(g) = &self.shape else {
            return Vec::new();
        };
        match self.kind {
            PartKind::FreeformFin => self.fin_points.clone(),
            PartKind::EllipticalFin => (0..=48)
                .map(|i| {
                    let a = std::f64::consts::PI * i as f64 / 48.0;
                    [
                        g.fin_root_chord_m * (1.0 - a.cos()) * 0.5,
                        g.fin_span_m * a.sin(),
                    ]
                })
                .collect(),
            _ => vec![
                [0.0, 0.0],
                [g.fin_sweep_m, g.fin_span_m],
                [g.fin_sweep_m + g.fin_tip_chord_m, g.fin_span_m],
                [g.fin_root_chord_m, 0.0],
            ],
        }
    }
    pub fn fin_area_cg(&self) -> (f64, f64) {
        let pts = self.fin_outline();
        let mut twice = 0.0;
        let mut moment = 0.0;
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            let cross = a[0] * b[1] - b[0] * a[1];
            twice += cross;
            moment += (a[0] + b[0]) * cross;
        }
        (
            twice.abs() * 0.5,
            if twice.abs() > 1e-12 {
                moment / (3.0 * twice)
            } else {
                0.0
            },
        )
    }
}
/// (mass, axial CG, length, radius), before stage offset. Payload and propellant remain separate.
/// Axial station where the cylindrical airframe starts after a nose or front transition.
pub fn airframe_front_m(s: &Stage) -> f64 {
    s.components
        .iter()
        .filter(|p| p.kind == PartKind::Transition && p.position_m.abs() < 1e-6)
        .map(|p| p.length_m)
        .fold(
            s.geometry.as_ref().map_or(0.0, |g| g.nose_length_m),
            f64::max,
        )
        .min(s.length_m - 0.001)
}
/// Make the existing shell explicit before attaching separately editable segments.
/// Preserve an entered whole-stage dry measurement as hardware allocation;
/// material-mass designs retain the calculated shells, fins and hardware.
pub fn explicit_shell(s: &mut Stage) {
    if s.materials.as_ref().is_some_and(|m| m.component_shell) {
        return;
    }
    let mut materials = s
        .materials
        .clone()
        .unwrap_or_else(|| Structure::for_stage(s));
    let measured = !materials.use_material_mass;
    let g = s
        .geometry
        .clone()
        .unwrap_or_else(|| crate::config::StageGeometry::generic(s, true));
    if s.geometry.is_none() {
        s.geometry = Some(g.clone());
    }
    let mut parts = Vec::new();
    let front = airframe_front_m(s);
    let mut body = Part::new(PartKind::BodyTube, s);
    body.name = "Original body tube".into();
    body.position_m = front;
    body.length_m = s.length_m - front;
    body.diameter_m = s.diameter_m;
    body.thickness_m = materials.airframe_wall_m;
    body.material = materials.airframe.clone();
    parts.push(body);
    if g.nose_length_m > 0.0 {
        let mut nose = Part::new(PartKind::NoseCone, s);
        nose.name = "Nose cone".into();
        nose.position_m = 0.0;
        nose.length_m = g.nose_length_m;
        nose.diameter_m = s.diameter_m;
        nose.thickness_m = materials.nose_wall_m;
        nose.material = materials.nose.clone();
        nose.shape = Some(g.clone());
        parts.push(nose);
    }
    let mut fins = Part::new(PartKind::FinSet, s);
    fins.name = "Original fin set".into();
    fins.position_m = g.fin_position_m;
    fins.length_m = g.fin_root_chord_m.max(g.fin_sweep_m + g.fin_tip_chord_m);
    fins.diameter_m = s.diameter_m;
    fins.shape = Some(g);
    fins.material = materials.fins.clone();
    parts.push(fins);
    if measured {
        materials.hardware_mass_kg = s.dry_mass_kg;
        materials.hardware_cg_m = s.dry_cg_m;
        for p in &mut parts {
            p.mass_override = true;
            p.mass_kg = 0.0;
        }
    }
    for p in &mut parts {
        p.cg_m = p.length_m * 0.5;
    }
    s.components.extend(parts);
    materials.component_shell = true;
    s.materials = Some(materials);
}

/// Move stations below an insertion/resize plane without scaling dimensions.
pub fn shift_aft(s: &mut Stage, plane: f64, delta: f64, except: Option<usize>) {
    for (i, p) in s.components.iter_mut().enumerate() {
        if Some(i) != except && p.position_m >= plane - 1e-8 {
            p.position_m += delta;
        }
    }
    for position in [&mut s.dry_cg_m, &mut s.payload_cg_m, &mut s.motor.cg_m] {
        if *position >= plane - 1e-8 {
            *position += delta;
        }
    }
    if let Some(m) = &mut s.materials
        && m.hardware_cg_m >= plane - 1e-8
    {
        m.hardware_cg_m += delta;
    }
    if let Some(g) = &mut s.geometry
        && g.fin_position_m >= plane - 1e-8
    {
        g.fin_position_m += delta;
    }
    s.length_m += delta;
}

pub fn fit_shell(s: &mut Stage) {
    if s.materials.as_ref().is_some_and(|m| m.component_shell)
        && let Some(diameter) = s
            .components
            .iter()
            .filter(|p| {
                matches!(
                    p.kind,
                    PartKind::BodyTube
                        | PartKind::NoseCone
                        | PartKind::Transition
                        | PartKind::CustomMesh
                )
            })
            .map(|p| {
                if p.kind == PartKind::Transition {
                    p.diameter_m.max(p.fore_diameter_m)
                } else {
                    p.diameter_m
                }
            })
            .reduce(f64::max)
    {
        s.diameter_m = diameter;
    }
    let min = s
        .components
        .iter()
        .map(|p| p.position_m)
        .fold(0.0, f64::min);
    if min < 0.0 {
        shift_aft(s, min, -min, None);
    }
    s.length_m = s
        .components
        .iter()
        .map(|p| p.position_m + p.length_m)
        .chain([
            s.motor.cg_m,
            s.payload_cg_m,
            s.dry_cg_m,
            s.materials.as_ref().map_or(0.0, |m| m.hardware_cg_m),
        ])
        .fold(0.02, f64::max);
    if let Some(g) = &mut s.geometry {
        if s.materials.as_ref().is_some_and(|m| m.component_shell) {
            g.nose_length_m = 0.0;
        }
        if let Some(p) = s.components.iter().find(|p| p.kind == PartKind::NoseCone) {
            g.nose_length_m = p.position_m + p.length_m;
            if let Some(shape) = &p.shape {
                g.nose_shape = shape.nose_shape;
            }
        }
        if let Some(p) = s.components.iter().find(|p| p.kind == PartKind::FinSet) {
            g.fin_position_m = p.position_m;
            if let Some(shape) = &p.shape {
                g.fin_count = shape.fin_count;
                g.fin_root_chord_m = shape.fin_root_chord_m;
                g.fin_tip_chord_m = shape.fin_tip_chord_m;
                g.fin_span_m = shape.fin_span_m;
                g.fin_sweep_m = shape.fin_sweep_m;
                g.fin_thickness_m = shape.fin_thickness_m;
            }
            if let Some(m) = &mut s.materials {
                m.fins = p.material.clone();
            }
        }
    }
}

/// End attachment on the pointed tube, or on the existing implicit airframe.
pub fn tube_attachment(s: &Stage, cursor: f64) -> f64 {
    let implicit = (!s.materials.as_ref().is_some_and(|m| m.component_shell))
        .then_some([airframe_front_m(s), s.length_m]);
    s.components
        .iter()
        .filter(|p| matches!(p.kind, PartKind::BodyTube | PartKind::Transition))
        .map(|p| [p.position_m, p.position_m + p.length_m])
        .chain(implicit)
        .flatten()
        .min_by(|a, b| (cursor - a).abs().total_cmp(&(cursor - b).abs()))
        .unwrap_or(s.length_m)
}

pub fn attach_tube(s: &mut Stage, station: f64) -> usize {
    explicit_shell(s);
    let mut p = Part::new(PartKind::BodyTube, s);
    p.name = "Body tube".into();
    p.position_m = station;
    p.diameter_m = attachment_diameter(s, station);
    if let Some(m) = &s.materials {
        p.material = m.airframe.clone();
        p.thickness_m = m.airframe_wall_m;
    }
    shift_aft(s, station, p.length_m, None);
    s.components.push(p);
    fit_shell(s);
    s.components.len() - 1
}

fn attachment_diameter(s: &Stage, station: f64) -> f64 {
    let aft = s.components.iter().find(|p| {
        matches!(p.kind, PartKind::BodyTube | PartKind::Transition)
            && (p.position_m + p.length_m - station).abs() < 1e-7
    });
    if let Some(p) = aft {
        return p.diameter_m;
    }
    s.components
        .iter()
        .find(|p| {
            matches!(p.kind, PartKind::BodyTube | PartKind::Transition)
                && (p.position_m - station).abs() < 1e-7
        })
        .map_or(s.diameter_m, |p| {
            if p.kind == PartKind::Transition {
                p.fore_diameter_m
            } else {
                p.diameter_m
            }
        })
}

/// A transition is a shell section in the current stage, never a stage command.
pub fn attach_transition(s: &mut Stage, station: f64) -> usize {
    explicit_shell(s);
    let mut p = Part::new(PartKind::Transition, s);
    p.position_m = station;
    p.fore_diameter_m = attachment_diameter(s, station);
    p.diameter_m = p.fore_diameter_m * 0.8;
    if let Some(m) = &s.materials {
        p.material = m.airframe.clone();
        p.thickness_m = m.airframe_wall_m.min(p.diameter_m * 0.1);
    }
    // Match the tube below an insertion; at the tail the next dropped tube
    // inherits the transition's aft diameter instead.
    if let Some(next) = s
        .components
        .iter()
        .find(|p| p.kind == PartKind::BodyTube && (p.position_m - station).abs() < 1e-7)
    {
        p.diameter_m = next.diameter_m;
    }
    shift_aft(s, station, p.length_m, None);
    s.components.push(p);
    fit_shell(s);
    s.components.len() - 1
}

/// Internal tubes attach inside a body; rings attach around an inner tube.
pub fn attach_internal(s: &mut Stage, kind: PartKind, station: f64) -> usize {
    explicit_shell(s);
    let wanted = if matches!(kind, PartKind::CenteringRing | PartKind::EngineBlock) {
        PartKind::InnerTube
    } else {
        PartKind::BodyTube
    };
    let parent = s
        .components
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == wanted)
        .min_by(|(_, a), (_, b)| {
            let distance = |p: &Part| {
                (p.position_m - station).max(0.) + (station - p.position_m - p.length_m).max(0.)
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(i, _)| i);
    let mut p = Part::new(kind, s);
    p.parent_component = parent;
    if let Some(i) = parent {
        let tube = &s.components[i];
        p.length_m = p.length_m.min(tube.length_m);
        p.position_m = station.clamp(
            tube.position_m,
            tube.position_m + tube.length_m - p.length_m,
        );
        if kind == PartKind::InnerTube {
            p.diameter_m = (tube.diameter_m - 2. * tube.thickness_m) * 0.6;
        }
        if kind == PartKind::EngineBlock {
            p.diameter_m = tube.diameter_m - 2. * tube.thickness_m;
            p.thickness_m = p.diameter_m * 0.225;
        }
        p.auto_ring_fit = kind == PartKind::CenteringRing;
    } else {
        p.position_m = station.clamp(0., (s.length_m - p.length_m).max(0.));
    }
    s.components.push(p);
    let index = s.components.len() - 1;
    refit_internals(s);
    index
}

/// Keep rings coaxial and fit their bore to the tube they surround.
pub fn refit_internals(s: &mut Stage) {
    for i in 0..s.components.len() {
        let Some(parent) = s.components[i].parent_component else {
            continue;
        };
        if parent >= s.components.len() {
            continue;
        }
        if s.components[i].auto_ring_fit && s.components[i].kind == PartKind::CenteringRing {
            let mount = &s.components[parent];
            let outer = mount
                .parent_component
                .and_then(|j| s.components.get(j))
                .map_or(
                    s.diameter_m - 2. * s.materials.as_ref().map_or(0.002, |m| m.airframe_wall_m),
                    |body| body.diameter_m - 2. * body.thickness_m,
                );
            let bore = mount.diameter_m;
            let radial = mount.radial_offset_m;
            s.components[i].diameter_m = outer;
            s.components[i].thickness_m = (outer - bore) * 0.5;
            s.components[i].radial_offset_m = radial;
        }
    }
    sync_motor_mount(s);
}

pub fn sync_motor_mount(s: &mut Stage) {
    if let Some(p) = s.components.iter().find(|p| p.motor_mount) {
        s.motor.cg_m = p.position_m + p.length_m + p.motor_overhang_m - p.motor_length_m * 0.5;
    }
}

/// Children follow a moved tube without changing their local axial offsets.
pub fn move_children(s: &mut Stage, parent: usize, delta: f64) {
    let children: Vec<_> = s
        .components
        .iter()
        .enumerate()
        .filter(|(_, p)| p.parent_component == Some(parent))
        .map(|(i, _)| i)
        .collect();
    for child in children {
        s.components[child].position_m += delta;
        move_children(s, child, delta);
    }
}

/// Reindex references when deleting a part; detached children keep their position.
pub fn remove_component(s: &mut Stage, index: usize) -> Part {
    let removed = s.components.remove(index);
    for p in &mut s.components {
        if let Some(parent) = p.parent_component {
            if parent == index {
                p.parent_component = None;
                p.auto_ring_fit = false;
            } else if parent > index {
                p.parent_component = Some(parent - 1);
            }
        }
    }
    removed
}

/// Extract a component with its children; returned parent indexes are local.
pub fn take_component_tree(s: &mut Stage, root: usize) -> Vec<Part> {
    let mut indexes = vec![root];
    let mut cursor = 0;
    while cursor < indexes.len() {
        let parent = indexes[cursor];
        indexes.extend(
            s.components
                .iter()
                .enumerate()
                .filter(|(_, p)| p.parent_component == Some(parent))
                .map(|(i, _)| i),
        );
        cursor += 1;
    }
    let mut parts: Vec<_> = indexes.iter().map(|&i| s.components[i].clone()).collect();
    for p in &mut parts {
        p.parent_component = p
            .parent_component
            .and_then(|parent| indexes.iter().position(|&i| i == parent));
    }
    indexes.sort_unstable();
    for i in indexes.into_iter().rev() {
        remove_component(s, i);
    }
    parts
}

pub fn attach_nose(s: &mut Stage) -> usize {
    explicit_shell(s);
    if let Some(i) = s
        .components
        .iter()
        .position(|p| p.kind == PartKind::NoseCone)
    {
        return i;
    }
    let mut p = Part::new(PartKind::NoseCone, s);
    p.name = "Nose cone".into();
    p.position_m = 0.0;
    p.diameter_m = s.diameter_m;
    if let Some(m) = &s.materials {
        p.material = m.nose.clone();
        p.thickness_m = m.nose_wall_m;
    }
    shift_aft(s, 0.0, p.length_m, None);
    s.components.push(p);
    fit_shell(s);
    s.components.len() - 1
}
pub fn dry_pieces(s: &Stage) -> Vec<(f64, f64, f64, f64)> {
    let r = s.diameter_m * 0.5;
    let mut p = Vec::new();
    if let Some(m) = s.materials.as_ref().filter(|m| m.component_shell) {
        p.push((m.hardware_mass_kg, m.hardware_cg_m, s.length_m * 0.1, r));
    } else if let Some(m) = s.materials.as_ref().filter(|m| m.use_material_mass) {
        let nose = s.geometry.as_ref().map_or(0.0, |g| g.nose_length_m);
        let front = airframe_front_m(s);
        let body = s.length_m - front;
        let inner = (r - m.airframe_wall_m).max(0.0);
        p.push((
            std::f64::consts::PI * (r * r - inner * inner) * body * m.airframe.density_kg_m3,
            front + body * 0.5,
            body,
            r,
        ));
        // Thin conical-shell estimate; all curved nose shapes use this stated approximation.
        if nose > 0.0 {
            p.push((
                std::f64::consts::PI * r * r.hypot(nose) * m.nose_wall_m * m.nose.density_kg_m3,
                2.0 * nose / 3.0,
                nose,
                r,
            ));
        }
        if let Some(g) = &s.geometry {
            let mass = (g.fin_root_chord_m + g.fin_tip_chord_m)
                * 0.5
                * g.fin_span_m
                * g.fin_thickness_m
                * g.fin_count as f64
                * m.fins.density_kg_m3;
            let cg = g.fin_position_m
                + (g.fin_root_chord_m.powi(2)
                    + g.fin_root_chord_m * g.fin_tip_chord_m
                    + g.fin_tip_chord_m.powi(2)
                    + g.fin_sweep_m * (g.fin_root_chord_m + 2.0 * g.fin_tip_chord_m))
                    / (3.0 * (g.fin_root_chord_m + g.fin_tip_chord_m));
            p.push((mass, cg, g.fin_root_chord_m, r + g.fin_span_m));
        }
        p.push((m.hardware_mass_kg, m.hardware_cg_m, s.length_m * 0.1, r));
    } else {
        p.push((s.dry_mass_kg, s.dry_cg_m, s.length_m, r));
    }
    for c in &s.components {
        let radius = if c.kind.planar_fin() {
            let g = c.shape.as_ref().unwrap();
            let r = c.diameter_m * 0.5;
            let span = g.fin_span_m;
            let cr = g.fin_root_chord_m;
            let ct = g.fin_tip_chord_m;
            (2.0 * (r * r
                + 2.0 * r * span * (cr + 2.0 * ct) / (3.0 * (cr + ct))
                + span * span * (cr + 3.0 * ct) / (6.0 * (cr + ct))))
                .sqrt()
        } else {
            (2.0 * c.radial_offset_m.powi(2) + (c.diameter_m * 0.5).powi(2)).sqrt()
        };
        p.push((c.mass(), c.cg(), c.length_m, radius));
    }
    p
}
pub fn validate(s: &Stage) -> Result<()> {
    if s.components.iter().filter(|p| p.motor_mount).count() > 1 {
        return Err("Only one active motor mount is supported per stage".into());
    }
    for (i, p) in s.components.iter().enumerate() {
        if p.motor_mount
            && (p.kind != PartKind::InnerTube
                || p.instance_count != 1
                || !p.motor_length_m.is_finite()
                || p.motor_length_m <= 0.0
                || !p.motor_overhang_m.is_finite()
                || p.motor_length_m > p.length_m + p.motor_overhang_m
                || !p.motor_diameter_m.is_finite()
                || p.motor_diameter_m < 0.0
                || p.motor_diameter_m > p.diameter_m - 2. * p.thickness_m + 1e-8
                || p.motor_overhang_m < 0.0)
        {
            return Err(
                "Motor mount needs one inner tube and a motor that fits its length / overhang"
                    .into(),
            );
        }
        if let Some(parent) = p.parent_component {
            let tube = s.components.get(parent).ok_or("Missing parent tube")?;
            if !matches!(
                p.kind,
                PartKind::InnerTube | PartKind::CenteringRing | PartKind::EngineBlock
            ) {
                return Err("Only internal tube / ring components can have a parent tube".into());
            }
            let expected = if matches!(p.kind, PartKind::CenteringRing | PartKind::EngineBlock) {
                PartKind::InnerTube
            } else {
                PartKind::BodyTube
            };
            if parent == i
                || tube.kind != expected
                || p.position_m < tube.position_m - 1e-8
                || p.position_m + p.length_m > tube.position_m + tube.length_m + 1e-8
            {
                return Err("Internal component must fit within its parent tube".into());
            }
            if p.kind == PartKind::InnerTube
                && p.diameter_m > tube.diameter_m - 2. * tube.thickness_m + 1e-8
            {
                return Err("Inner tube exceeds its parent's bore".into());
            }
            if p.kind == PartKind::EngineBlock
                && p.diameter_m > tube.diameter_m - 2. * tube.thickness_m + 1e-8
            {
                return Err("Engine block exceeds the motor tube bore".into());
            }
            if p.auto_ring_fit
                && (p.kind != PartKind::CenteringRing || p.diameter_m <= tube.diameter_m)
            {
                return Err("Centering ring must surround a smaller inner tube".into());
            }
        }
    }
    if let Some(m) = &s.materials {
        if m.use_material_mass && s.geometry.is_none() {
            return Err("Material mass calculation requires explicit stage geometry".into());
        }
        if [&m.airframe, &m.nose, &m.fins].iter().any(|m| {
            m.density_kg_m3 <= 0.0
                || !m.thermal.valid()
                || m.mechanical.as_ref().is_some_and(|m| !m.valid())
        }) || m.airframe_wall_m <= 0.0
            || m.airframe_wall_m > s.diameter_m * 0.5
            || m.nose_wall_m <= 0.0
            || m.nose_wall_m > s.diameter_m * 0.5
            || m.hardware_mass_kg < 0.0
            || !(0.0..=s.length_m).contains(&m.hardware_cg_m)
        {
            return Err(format!("Invalid material / shell dimensions on {}", s.name).into());
        }
    }
    for c in &s.components {
        if c.appearance_rgba.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err("Invalid component color".into());
        }
        if c.kind == PartKind::CustomMesh {
            let mesh = c
                .mesh
                .as_ref()
                .ok_or("Imported part has no embedded geometry")?;
            mesh.validate()?;
            if !mesh.closed && !c.mass_override {
                return Err(
                    "Open imported mesh needs a mass override or a repaired closed solid".into(),
                );
            }
        } else if c.mesh.is_some() {
            return Err("Embedded mesh requires an imported-model component".into());
        }
        if let Some(chute) = &c.recovery
            && [
                chute.area_m2,
                chute.cd,
                chute.inflation_s,
                chute.max_q_pa,
                chute.max_mach,
                chute.max_load_n,
            ]
            .iter()
            .any(|v| *v <= 0.0)
        {
            return Err("Recovery part needs positive area, drag and deployment limits".into());
        }

        if c.fore_diameter_m < 0.0
            || (c.kind == PartKind::Transition && c.fore_diameter_m > s.diameter_m)
        {
            return Err("Component fore diameter is outside stage envelope".into());
        }
        if !(1..=16).contains(&c.instance_count)
            || c.radial_offset_m < 0.0
            || c.cd_override.is_some_and(|v| v < 0.0)
        {
            return Err("Invalid instances, radial offset or drag override".into());
        }
        if c.kind.planar_fin() {
            let Some(g) = &c.shape else {
                return Err("Fin set requires geometry".into());
            };
            if !(3..=8).contains(&g.fin_count)
                || g.fin_root_chord_m <= 0.0
                || g.fin_tip_chord_m <= 0.0
                || g.fin_span_m <= 0.0
                || g.fin_thickness_m <= 0.0
                || g.fin_sweep_m < 0.0
            {
                return Err("Invalid component fin geometry".into());
            }
            if c.instance_count != 1 {
                return Err("Use fin count to repeat planar fins".into());
            }
            let points = c.fin_outline();
            triangulate_fin(&points)?;
            if points
                .iter()
                .any(|p| p[0] < 0.0 || p[1] < -1e-12 || p[0] > c.length_m + 1e-8)
            {
                return Err("Fin outline exceeds its axial length".into());
            }
        }
        if !c.external() && c.radial_offset_m + c.diameter_m * 0.5 > s.diameter_m * 0.5 + 1e-8 {
            return Err("Internal part exceeds the radial body envelope".into());
        }
        if c.length_m <= 0.0
            || c.diameter_m <= 0.0
            || (matches!(
                c.kind,
                PartKind::BodyTube | PartKind::NoseCone | PartKind::Transition
            ) && c.diameter_m > s.diameter_m)
            || c.thickness_m <= 0.0
            || c.thickness_m > c.diameter_m * 0.5
            || c.material.density_kg_m3 <= 0.0
            || !c.material.thermal.valid()
            || c.material.mechanical.as_ref().is_some_and(|m| !m.valid())
            || c.mass_kg < 0.0
            || c.position_m < 0.0
            || c.position_m + c.length_m > s.length_m
            || !(0.0..=c.length_m).contains(&c.cg_m)
        {
            return Err(format!("Invalid component dimensions or mass: {}", c.name).into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structural_material_schema_preserves_old_materials_and_round_trips_new_data() {
        let mut material = Material::presets().remove(0);
        let old = serde_json::to_value(&material).unwrap();
        assert!(old.get("mechanical").is_none());
        let restored: Material = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), old);
        material.mechanical = Some(MechanicalMaterial {
            youngs_modulus_pa: 50e9,
            poisson_ratio: 0.3,
            allowable_stress_pa: Some(100e6),
            temperature_factors: vec![[300., 1., 1.], [400., 0.5, 0.25]],
        });
        let new = serde_json::to_value(&material).unwrap();
        let restored: Material = serde_json::from_value(new.clone()).unwrap();
        assert_eq!(serde_json::to_value(&restored).unwrap(), new);
        let m = restored.mechanical.unwrap();
        assert!(m.valid());
        assert_eq!(m.at(350.), (37.5e9, Some(62.5e6)));
        assert_eq!(m.at(200.), (50e9, Some(100e6)));
        assert_eq!(m.at(500.), (25e9, Some(25e6)));
        for value in [0., -1., f64::NAN, f64::INFINITY] {
            let mut invalid = m.clone();
            invalid.youngs_modulus_pa = value;
            assert!(!invalid.valid());
        }
        let mut invalid = m.clone();
        invalid.temperature_factors.swap(0, 1);
        assert!(!invalid.valid());
        invalid = m.clone();
        invalid.temperature_factors[1][2] = 0.;
        assert!(!invalid.valid());
        invalid = m.clone();
        invalid.poisson_ratio = 0.5;
        assert!(!invalid.valid());
    }
    #[test]
    fn front_transition_replaces_cylindrical_shell_mass_and_updates_its_cg() {
        let mut s = crate::config::Mission::demo(false).stages.remove(0);
        s.geometry = Some(crate::config::StageGeometry::generic(&s, false));
        s.geometry.as_mut().unwrap().nose_length_m = 0.0;
        let mut material = Structure::for_stage(&s);
        material.use_material_mass = true;
        s.materials = Some(material);
        let before = dry_pieces(&s);
        let mut transition = Part::new(PartKind::Transition, &s);
        transition.position_m = 0.0;
        s.components.push(transition.clone());
        let after = dry_pieces(&s);
        let remaining = s.length_m - transition.length_m;
        assert!((after[0].0 - before[0].0 * remaining / s.length_m).abs() < 1e-10);
        assert!((after[0].1 - (transition.length_m + remaining * 0.5)).abs() < 1e-10);
        assert!((after[0].2 - remaining).abs() < 1e-10);
        assert!((after.last().unwrap().0 - transition.mass()).abs() < 1e-10);
    }
}
