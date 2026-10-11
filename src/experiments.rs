//! Opt-in engineering models, data grids and deterministic fault injection.
//! Every setting defaults to the original model; imported grids take precedence.
use crate::config::Result;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Experiments {
    pub aero_grids: Vec<AeroGrid>,
    pub rarefied_bridge: bool,
    pub recovery: Vec<RecoveryModel>,
    pub sensor_faults: Vec<SensorFault>,
    pub commands: Vec<CommandRule>,
    pub replace_recovery_logic: bool,
    pub thermal: Vec<ThermalLaw>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AeroGrid {
    pub stage: usize,
    pub source: String,
    pub mach: Vec<f64>,
    pub alpha_deg: Vec<f64>,
    pub reynolds: Vec<f64>,
    pub points: Vec<Coefficient>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coefficient {
    pub ca_off: f64,
    pub ca_on: f64,
    pub cn: f64,
    pub cm: f64,
}
impl AeroGrid {
    /// Row-major Re, alpha (0–180°), Mach. CN points opposite transverse velocity;
    /// CM is about nose, positive in the lever.cross(normal) direction / q A D.
    pub fn sample(&self, mach: f64, alpha_deg: f64, re: f64) -> Coefficient {
        let bracket = |axis: &[f64], x: f64| {
            let i = axis
                .partition_point(|v| *v <= x)
                .saturating_sub(1)
                .min(axis.len() - 2);
            (i, ((x - axis[i]) / (axis[i + 1] - axis[i])).clamp(0.0, 1.0))
        };
        let (i, u) = bracket(&self.mach, mach);
        let (j, v) = bracket(&self.alpha_deg, alpha_deg);
        let log: Vec<_> = self.reynolds.iter().map(|r| r.ln()).collect();
        let (k, w) = bracket(&log, re.max(1.0).ln());
        let mut c = Coefficient {
            ca_off: 0.0,
            ca_on: 0.0,
            cn: 0.0,
            cm: 0.0,
        };
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    let weight = if x == 0 { 1.0 - u } else { u }
                        * if y == 0 { 1.0 - v } else { v }
                        * if z == 0 { 1.0 - w } else { w };
                    let a = self.points
                        [((k + z) * self.alpha_deg.len() + j + y) * self.mach.len() + i + x];
                    c.ca_off += weight * a.ca_off;
                    c.ca_on += weight * a.ca_on;
                    c.cn += weight * a.cn;
                    c.cm += weight * a.cm;
                }
            }
        }
        c
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecoveryModel {
    /// Set >0 to enable a separate canopy point mass. Mass is part of stage dry mass.
    pub canopy_mass_kg: f64,
    pub tether_length_m: f64,
    pub tether_stiffness_n_m: f64,
    pub tether_damping_ns_m: f64,
    pub stage: usize,
    pub main: bool,
    pub reef_fraction: f64,
    pub reef_hold_s: f64,
    pub opening_factor: f64,
    pub attachment_fraction: f64,
}
impl Default for RecoveryModel {
    fn default() -> Self {
        Self {
            canopy_mass_kg: 0.0,
            tether_length_m: 8.0,
            tether_stiffness_n_m: 500.0,
            tether_damping_ns_m: 20.0,
            stage: 0,
            main: false,
            reef_fraction: 1.0,
            reef_hold_s: 0.0,
            opening_factor: 1.0,
            attachment_fraction: 0.1,
        }
    }
}
impl RecoveryModel {
    pub fn inflation(&self, elapsed: f64, inflation_s: f64) -> f64 {
        if elapsed <= inflation_s {
            self.reef_fraction * (elapsed / inflation_s).clamp(0.0, 1.0)
        } else if elapsed < inflation_s + self.reef_hold_s {
            self.reef_fraction
        } else {
            self.reef_fraction
                + (1.0 - self.reef_fraction)
                    * ((elapsed - inflation_s - self.reef_hold_s) / inflation_s).clamp(0.0, 1.0)
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    Barometer,
    Gnss,
    Power,
    DroguePyro,
    MainPyro,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensorFault {
    pub kind: FaultKind,
    pub start_s: f64,
    pub end_s: f64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Drogue,
    Main,
    Separate,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRule {
    pub name: String,
    pub stage: usize,
    pub action: Action,
    pub after_s: f64,
    pub descending: bool,
    pub altitude_below_m: f64,
    pub max_mach: f64,
    pub max_q_pa: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThermalLaw {
    pub stage: usize,
    pub region: String,
    /// Layered wall effective through/axial conductivity. Explicit material data.
    pub through_k_w_m_k: f64,
    pub axial_k_w_m_k: f64,
    pub motor_heat_w: f64,
    pub motor_heat_until_s: f64,
    pub youngs_modulus_pa: f64,
    pub allowable_stress_pa: f64,
    pub ablation_temperature_k: f64,
    pub heat_of_ablation_j_kg: f64,
}
impl Experiments {
    pub fn fault(&self, kind: FaultKind, t: f64) -> bool {
        self.sensor_faults.iter().any(|f| {
            std::mem::discriminant(&f.kind) == std::mem::discriminant(&kind)
                && t >= f.start_s
                && t < f.end_s
        })
    }
    pub fn recovery(&self, stage: usize, main: bool) -> RecoveryModel {
        self.recovery
            .iter()
            .find(|r| r.stage == stage && r.main == main)
            .cloned()
            .unwrap_or(RecoveryModel {
                stage,
                main,
                ..Default::default()
            })
    }
    pub fn validate(&self, count: usize) -> Result<()> {
        let ordered = |a: &[f64]| {
            a.len() >= 2 && a.iter().all(|x| x.is_finite()) && a.windows(2).all(|w| w[1] > w[0])
        };
        let mut stages = std::collections::HashSet::new();
        for g in &self.aero_grids {
            if g.stage >= count
                || !stages.insert(g.stage)
                || !ordered(&g.mach)
                || !ordered(&g.alpha_deg)
                || !ordered(&g.reynolds)
                || g.mach[0] < 0.0
                || g.alpha_deg[0] < 0.0
                || *g.alpha_deg.last().unwrap() > 180.0
                || g.reynolds[0] <= 0.0
                || g.mach
                    .len()
                    .checked_mul(g.alpha_deg.len())
                    .and_then(|n| n.checked_mul(g.reynolds.len()))
                    != Some(g.points.len())
                || g.points.iter().any(|p| {
                    ![p.ca_off, p.ca_on, p.cn, p.cm]
                        .iter()
                        .all(|x| x.is_finite())
                        || p.ca_off < 0.0
                        || p.ca_on < 0.0
                })
            {
                return Err("Invalid stage Mach/alpha/Re coefficient grid".into());
            }
        }
        let mut recovery = std::collections::HashSet::new();
        for r in &self.recovery {
            if r.canopy_mass_kg < 0.0
                || r.tether_length_m <= 0.0
                || r.tether_stiffness_n_m <= 0.0
                || r.tether_damping_ns_m < 0.0
                || r.stage >= count
                || !recovery.insert((r.stage, r.main))
                || !(0.0..=1.0).contains(&r.reef_fraction)
                || r.reef_hold_s < 0.0
                || r.opening_factor < 1.0
                || !(0.0..=1.0).contains(&r.attachment_fraction)
            {
                return Err("Invalid recovery reefing, attachment or opening factor".into());
            }
        }
        for f in &self.sensor_faults {
            if f.start_s < 0.0 || f.end_s <= f.start_s || !f.end_s.is_finite() {
                return Err("Fault time interval must increase".into());
            }
        }
        for c in &self.commands {
            if c.stage >= count
                || c.after_s < 0.0
                || c.altitude_below_m < 0.0
                || c.max_mach <= 0.0
                || c.max_q_pa <= 0.0
            {
                return Err("Invalid programmable command envelope".into());
            }
        }
        for t in &self.thermal {
            if t.stage >= count
                || t.through_k_w_m_k <= 0.0
                || t.axial_k_w_m_k <= 0.0
                || t.motor_heat_w < 0.0
                || t.motor_heat_until_s < 0.0
                || t.youngs_modulus_pa <= 0.0
                || t.allowable_stress_pa <= 0.0
                || t.ablation_temperature_k <= 0.0
                || t.heat_of_ablation_j_kg <= 0.0
            {
                return Err("Invalid anisotropic thermal/structural law".into());
            }
        }
        Ok(())
    }
}
