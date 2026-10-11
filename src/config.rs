use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mission {
    pub name: String,
    pub description: String,
    pub dt: f64,
    pub duration: f64,
    pub sample_period: f64,
    pub launch: Launch,
    pub wind: Wind,
    #[serde(default)]
    pub atmosphere: AtmosphereSettings,
    pub stages: Vec<Stage>,
    pub recovery: Recovery,
    pub avionics: Avionics,
    #[serde(default)]
    pub environment: crate::environment::Environment,
    #[serde(default)]
    pub experiments: crate::experiments::Experiments,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AtmosphereSettings {
    pub custom: bool,
    pub temperature_c: f64,
    /// Station pressure, not sea-level-reduced pressure.
    pub pressure_hpa: f64,
    /// Visual cloud layer; no condensation/turbulence model.
    pub cloud_cover: f64,
    pub cloud_base_agl_m: f64,
    pub cloud_thickness_m: f64,
}
impl Default for AtmosphereSettings {
    fn default() -> Self {
        Self {
            custom: false,
            temperature_c: 15.0,
            pressure_hpa: 1013.25,
            cloud_cover: 0.35,
            cloud_base_agl_m: 1800.0,
            cloud_thickness_m: 350.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoseShape {
    Conical,
    TangentOgive,
    Elliptical,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageGeometry {
    pub nose_shape: NoseShape,
    pub nose_length_m: f64,
    pub fin_count: u32,
    pub fin_root_chord_m: f64,
    pub fin_tip_chord_m: f64,
    pub fin_span_m: f64,
    pub fin_sweep_m: f64,
    pub fin_thickness_m: f64,
    /// Root leading edge, aft of this stage's nose.
    pub fin_position_m: f64,
}
impl StageGeometry {
    pub fn generic(stage: &Stage, top: bool) -> Self {
        let chord = stage.length_m * 0.14;
        Self {
            nose_shape: NoseShape::Conical,
            nose_length_m: if top { stage.length_m * 0.22 } else { 0.0 },
            fin_count: 4,
            fin_root_chord_m: chord,
            fin_tip_chord_m: chord * 0.4,
            fin_span_m: stage.diameter_m,
            fin_sweep_m: chord * 0.65,
            fin_thickness_m: 0.005,
            fin_position_m: stage.length_m - chord.max(chord * 1.05),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub altitude_m: f64,
    pub latitude_deg: f64,
    #[serde(default)]
    pub longitude_deg: f64,
    pub elevation_deg: f64,
    pub azimuth_deg: f64,
    pub rail_length_m: f64,
    pub earth_rotation: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wind {
    pub east_mps: f64,
    pub north_mps: f64,
    pub shear_per_km: f64,
    pub gust_mps: f64,
    pub gust_period_s: f64,
}
impl Wind {
    pub fn velocity(&self, agl: f64, t: f64) -> DVec3 {
        let f = 1.0 + self.shear_per_km * agl.max(0.0) / 1000.0;
        let g = self.gust_mps * (t * std::f64::consts::TAU / self.gust_period_s).sin();
        DVec3::new(self.east_mps * f + g, self.north_mps * f + g * 0.37, 0.0)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_mass: Option<crate::model_physics::ModelMass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Recovery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materials: Option<crate::assembly::Structure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<crate::assembly::Part>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<StageGeometry>,
    pub name: String,
    pub length_m: f64,
    pub diameter_m: f64,
    pub dry_mass_kg: f64,
    pub dry_cg_m: f64,
    pub payload_mass_kg: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub payload_name: String,
    pub payload_cg_m: f64,
    pub motor: Motor,
    /// Applied to the complete remaining stack, relative to its nose.
    pub aero: Vec<AeroPoint>,
    pub aero_source: String,
    pub ignition_delay_s: f64,
    pub separation: Trigger,
    pub separation_speed_mps: f64,
    pub pitch_damping: f64,
    pub roll_damping: f64,
    pub fin_cant_deg: f64,
    pub max_q_pa: f64,
    pub max_accel_g: f64,
    pub max_temperature_k: f64,
    pub nose_radius_m: f64,
    pub thermal_capacity_j_per_k: f64,
    pub heated_area_m2: f64,
    pub emissivity: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motor {
    pub name: String,
    pub propellant_mass_kg: f64,
    pub cg_m: f64,
    pub points: Vec<[f64; 2]>,
    pub thrust_offset_m: [f64; 2],
    pub misalignment_deg: [f64; 2],
}
impl Motor {
    pub fn burn_time(&self) -> f64 {
        self.points.last().map_or(0.0, |p| p[0])
    }
    pub fn thrust(&self, t: f64) -> f64 {
        if t < 0.0 || t >= self.burn_time() {
            return 0.0;
        }
        interpolate(&self.points, t)
    }
    pub fn impulse_to(&self, t: f64) -> f64 {
        self.points
            .windows(2)
            .map(|w| {
                let a = w[0];
                let b = w[1];
                let end = t.min(b[0]);
                if end <= a[0] {
                    0.0
                } else {
                    let y = a[1] + (b[1] - a[1]) * (end - a[0]) / (b[0] - a[0]);
                    (end - a[0]) * (a[1] + y) * 0.5
                }
            })
            .sum()
    }
    pub fn remaining(&self, t: f64) -> f64 {
        self.propellant_mass_kg
            * (1.0 - self.impulse_to(t.max(0.0)) / self.impulse_to(self.burn_time()))
                .clamp(0.0, 1.0)
    }
}
pub fn interpolate(points: &[[f64; 2]], x: f64) -> f64 {
    if x <= points[0][0] {
        return points[0][1];
    }
    for w in points.windows(2) {
        if x <= w[1][0] {
            return w[0][1] + (w[1][1] - w[0][1]) * (x - w[0][0]) / (w[1][0] - w[0][0]);
        }
    }
    points.last().unwrap()[1]
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AeroPoint {
    pub mach: f64,
    pub cd_power_off: f64,
    pub cd_power_on: f64,
    pub cn_alpha_per_rad: f64,
    pub cp_m: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    Burnout { delay_s: f64 },
    Time { time_s: f64 },
    Mach { mach: f64 },
    Altitude { altitude_agl_m: f64 },
    Never,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub drogue: Chute,
    pub main: Chute,
    pub main_altitude_agl_m: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chute {
    pub enabled: bool,
    pub area_m2: f64,
    pub cd: f64,
    pub inflation_s: f64,
    pub max_q_pa: f64,
    pub max_mach: f64,
    pub max_load_n: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Avionics {
    pub sample_hz: f64,
    pub latency_s: f64,
    pub baro_ceiling_m: f64,
    pub baro_bias_m: f64,
    pub baro_noise_m: f64,
    pub gnss_ceiling_m: f64,
    pub gnss_noise_m: f64,
    pub accel_range_g: f64,
    pub gyro_range_deg_s: f64,
    pub battery_wh: f64,
    pub power_w: f64,
    pub apogee_confirm_samples: usize,
    pub ignition_max_tilt_deg: f64,
    pub ignition_min_altitude_m: f64,
}
impl Mission {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let m: Self = serde_json::from_str(&fs::read_to_string(path)?)?;
        m.validate()?;
        Ok(m)
    }
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        self.environment.validate()?;
        self.experiments.validate(self.stages.len())?;
        for r in &self.experiments.recovery {
            let mass = crate::assembly::dry_pieces(&self.stages[r.stage])
                .iter()
                .map(|p| p.0)
                .sum::<f64>();
            if r.canopy_mass_kg >= mass * 0.2 {
                return Err("Canopy point mass must be <20% of allocated stage dry mass".into());
            }
        }
        // Reject non-finite values everywhere, including values changed through the UI.
        fn finite(v: &serde_json::Value) -> bool {
            match v {
                serde_json::Value::Null => false,
                serde_json::Value::Array(a) => a.iter().all(finite),
                serde_json::Value::Object(o) => o.values().all(finite),
                _ => true,
            }
        }
        if !finite(&serde_json::to_value(self)?) {
            return Err("configuration contains a non-finite number".into());
        }
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !(0.0001..=0.05).contains(&self.dt)
            || !positive(self.duration)
            || self.duration > 20000.0
            || self.sample_period < self.dt
        {
            return Err("dt must be 0.0001–0.05 s; duration 0–20000 s; sample_period >= dt".into());
        }
        if self.stages.is_empty() || self.stages.len() > 12 {
            return Err("provide 1–12 stages, ordered booster to sustainer".into());
        }
        let weather = &self.atmosphere;
        if !(-50.0..=50.0).contains(&weather.temperature_c)
            || !(200.0..=1100.0).contains(&weather.pressure_hpa)
            || !(0.0..=1.0).contains(&weather.cloud_cover)
            || !(100.0..=15000.0).contains(&weather.cloud_base_agl_m)
            || !(20.0..=3000.0).contains(&weather.cloud_thickness_m)
        {
            return Err("Invalid site temperature, station pressure or visual cloud layer".into());
        }
        for (i, stage) in self.stages.iter().enumerate() {
            if let Some(model) = &stage.model_mass {
                model.validate(stage)?;
            }
            if let Some(g) = &stage.geometry
                && (g.nose_length_m < 0.0
                    || (i == self.stages.len() - 1 && g.nose_length_m <= 0.0)
                    || (i < self.stages.len() - 1 && g.nose_length_m != 0.0)
                    || g.nose_length_m > stage.length_m - 0.01
                    || !(3..=8).contains(&g.fin_count)
                    || (g.nose_shape == NoseShape::TangentOgive
                        && g.nose_length_m > 0.0
                        && g.nose_length_m < stage.diameter_m * 0.5)
                    || !positive(g.fin_root_chord_m)
                    || !positive(g.fin_tip_chord_m)
                    || !positive(g.fin_span_m)
                    || !positive(g.fin_thickness_m)
                    || g.fin_thickness_m > stage.diameter_m * 0.5
                    || g.fin_position_m < g.nose_length_m
                    || g.fin_sweep_m < 0.0
                    || g.fin_position_m + g.fin_root_chord_m
                        > stage.length_m + stage.diameter_m * 2.0
                    || g.fin_position_m + g.fin_sweep_m + g.fin_tip_chord_m
                        > stage.length_m + stage.diameter_m * 2.0)
            {
                return Err(format!("Invalid nose or fin geometry on {}", stage.name).into());
            }
        }
        if !(0.0..=90.0).contains(&self.launch.elevation_deg)
            || self.launch.rail_length_m <= 0.0
            || self.launch.altitude_m < -500.0
            || self.launch.altitude_m > 8000.0
            || self.launch.latitude_deg.abs() > 90.0
            || self.launch.longitude_deg.abs() > 180.0
            || !positive(self.wind.gust_period_s)
        {
            return Err("invalid launch site, rail, elevation, or gust period".into());
        }
        let av = &self.avionics;
        if !positive(av.sample_hz)
            || av.sample_hz > 1.0 / self.dt
            || av.latency_s < 0.0
            || !positive(av.battery_wh)
            || av.power_w < 0.0
            || !positive(av.accel_range_g)
            || !positive(av.gyro_range_deg_s)
            || !positive(av.baro_ceiling_m)
            || av.baro_noise_m < 0.0
            || av.apogee_confirm_samples == 0
            || !(0.0..=90.0).contains(&av.ignition_max_tilt_deg)
            || av.ignition_min_altitude_m < 0.0
        {
            return Err("invalid avionics configuration".into());
        }
        if !positive(av.gnss_ceiling_m) || av.gnss_noise_m < 0.0 {
            return Err("invalid GNSS sensor ceiling or noise".into());
        }
        for recovery in std::iter::once(&self.recovery)
            .chain(self.stages.iter().filter_map(|s| s.recovery.as_ref()))
        {
            if recovery.main_altitude_agl_m < 0.0 {
                return Err("main altitude must be non-negative".into());
            }
            for c in [&recovery.drogue, &recovery.main] {
                if !positive(c.area_m2)
                    || !positive(c.cd)
                    || !positive(c.inflation_s)
                    || !positive(c.max_q_pa)
                    || !positive(c.max_mach)
                    || !positive(c.max_load_n)
                {
                    return Err("invalid parachute limits or geometry".into());
                }
            }
        }
        for (i, s) in self.stages.iter().enumerate() {
            crate::assembly::validate(s)?;
            if !positive(s.length_m)
                || !positive(s.diameter_m)
                || !positive(s.dry_mass_kg)
                || s.payload_mass_kg < 0.0
                || s.motor.propellant_mass_kg < 0.0
                || s.ignition_delay_s < 0.0
                || s.separation_speed_mps < 0.0
                || s.pitch_damping < 0.0
                || s.roll_damping < 0.0
                || !positive(s.nose_radius_m)
                || !positive(s.thermal_capacity_j_per_k)
                || !positive(s.heated_area_m2)
                || !(0.0..=1.0).contains(&s.emissivity)
                || !positive(s.max_q_pa)
                || !positive(s.max_accel_g)
                || !positive(s.max_temperature_k)
            {
                return Err(format!(
                    "invalid stage {i} mass, geometry, thermal properties or limits"
                )
                .into());
            }
            for cg in [s.dry_cg_m, s.payload_cg_m, s.motor.cg_m] {
                if !(0.0..=s.length_m).contains(&cg) {
                    return Err(format!("stage {i} CG outside stage").into());
                }
            }
            let p = &s.motor.points;
            if p.len() < 2
                || p[0][0] != 0.0
                || p.iter().any(|p| p[0] < 0.0 || p[1] < 0.0)
                || p.windows(2).any(|w| w[1][0] <= w[0][0])
                || p[0][1] != 0.0
                || p.last().unwrap()[1] != 0.0
                || !positive(s.motor.impulse_to(s.motor.burn_time()))
            {
                return Err(format!("stage {i}: thrust curve must start at [0,0], end at zero thrust, and have increasing times and positive impulse").into());
            }
            if s.aero.len() < 2
                || s.aero[0].mach != 0.0
                || s.aero.iter().any(|a| {
                    a.cd_power_off < 0.0
                        || a.cd_power_on < 0.0
                        || a.cn_alpha_per_rad < 0.0
                        || a.cp_m < 0.0
                })
                || s.aero.windows(2).any(|w| w[1].mach <= w[0].mach)
            {
                return Err(format!("stage {i}: invalid aerodynamic table").into());
            }
            match s.separation {
                Trigger::Burnout { delay_s } if delay_s < 0.0 => {
                    return Err("negative separation delay".into());
                }
                Trigger::Time { time_s } if time_s < 0.0 => {
                    return Err("negative stage time".into());
                }
                Trigger::Mach { mach } if mach <= 0.0 => {
                    return Err("Mach trigger must be positive".into());
                }
                Trigger::Altitude { altitude_agl_m } if altitude_agl_m <= 0.0 => {
                    return Err("altitude trigger must be positive".into());
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub fn demo(hypersonic: bool) -> Self {
        let dimensions = if hypersonic {
            vec![
                (2.0, 0.20, 22.0, 30.0, 8000.0, 7.0),
                (1.5, 0.15, 12.0, 17.0, 4200.0, 8.0),
                (2.3, 0.102, 10.0, 10.0, 1800.0, 10.0),
            ]
        } else {
            vec![(2.4, 0.15, 13.0, 5.0, 2200.0, 4.0)]
        };
        let count = dimensions.len();
        let mut remaining_length: f64 = dimensions.iter().map(|x| x.0).sum();
        let stages = dimensions
            .into_iter()
            .enumerate()
            .map(|(i, (length, d, mass, prop, thrust, burn))| {
                let cp = remaining_length * 0.76;
                remaining_length -= length;
                let aero = [
                    (0.0, 0.44),
                    (0.7, 0.46),
                    (0.95, 0.68),
                    (1.1, 0.83),
                    (1.4, 0.64),
                    (2.0, 0.45),
                    (3.0, 0.32),
                    (5.0, 0.24),
                    (8.0, 0.23),
                    (15.0, 0.24),
                    (25.0, 0.26),
                ]
                .into_iter()
                .map(|(mach, cd)| AeroPoint {
                    mach,
                    cd_power_off: cd,
                    cd_power_on: cd * 0.87,
                    cn_alpha_per_rad: 8.0,
                    cp_m: cp,
                })
                .collect();
                Stage {
                    model_mass: None,
                    geometry: None,
                    recovery: None,
                    materials: None,
                    components: Vec::new(),
                    name: format!("Stage {}", i + 1),
                    length_m: length,
                    diameter_m: d,
                    dry_mass_kg: mass,
                    dry_cg_m: length * 0.55,
                    payload_mass_kg: if i == count - 1 { 2.0 } else { 0.0 },
                    payload_name: String::new(),
                    payload_cg_m: length * 0.18,
                    motor: Motor {
                        name: "SYNTHETIC demonstration motor".into(),
                        propellant_mass_kg: prop,
                        cg_m: length * 0.75,
                        points: vec![
                            [0.0, 0.0],
                            [0.12, thrust],
                            [burn - 0.15, thrust],
                            [burn, 0.0],
                        ],
                        thrust_offset_m: [0.0, 0.0],
                        misalignment_deg: [0.0, 0.0],
                    },
                    aero,
                    aero_source: "Synthetic engineering curve; not RASAero, CFD or measured data"
                        .into(),
                    ignition_delay_s: if i == 0 { 0.0 } else { 1.0 },
                    separation: if i == count - 1 {
                        Trigger::Never
                    } else {
                        Trigger::Burnout { delay_s: 0.3 }
                    },
                    separation_speed_mps: 0.5,
                    pitch_damping: 25.0,
                    roll_damping: 3.0,
                    fin_cant_deg: 0.0,
                    max_q_pa: 150000.0,
                    max_accel_g: 40.0,
                    max_temperature_k: 650.0,
                    nose_radius_m: 0.003,
                    thermal_capacity_j_per_k: 1800.0,
                    heated_area_m2: 0.012,
                    emissivity: 0.8,
                }
            })
            .collect();
        let chute = |area, cd, q, mach, load| Chute {
            enabled: true,
            area_m2: area,
            cd,
            inflation_s: 2.0,
            max_q_pa: q,
            max_mach: mach,
            max_load_n: load,
        };
        Self{environment:Default::default(),experiments:Default::default(),name:if hypersonic{"Synthetic sounding rocket"}else{"Collegiate HPR demonstrator"}.into(),description:"Demonstration only. Replace synthetic motor and aero data before comparing with a real design.".into(),dt:0.005,duration:1200.0,sample_period:0.1,
            launch:Launch{altitude_m:600.0,latitude_deg:32.0,longitude_deg:0.0,elevation_deg:88.0,azimuth_deg:0.0,rail_length_m:6.0,earth_rotation:true},wind:Wind{east_mps:2.0,north_mps:0.0,shear_per_km:0.015,gust_mps:0.3,gust_period_s:8.0},atmosphere:AtmosphereSettings::default(),stages,
            recovery:Recovery{drogue:chute(0.6,0.9,10000.0,0.7,12000.0),main:chute(12.0,1.5,3000.0,0.5,20000.0),main_altitude_agl_m:450.0},
            avionics:Avionics{sample_hz:50.0,latency_s:0.08,baro_ceiling_m:30000.0,baro_bias_m:0.0,baro_noise_m:0.3,gnss_ceiling_m:300000.0,gnss_noise_m:1.0,accel_range_g:32.0,gyro_range_deg_s:1000.0,battery_wh:5.0,power_w:2.0,apogee_confirm_samples:8,ignition_max_tilt_deg:35.0,ignition_min_altitude_m:100.0}}
    }
}
