//! SI atmosphere. US Standard Atmosphere lapse layers to 84.852 km geopotential.
//! Above that, a continuous exponential engineering extension, not a thermosphere model.
use glam::DVec3;
pub const EARTH_RADIUS: f64 = 6_371_000.0;
pub const MU: f64 = 3.986004418e14;
const R: f64 = 287.05287;
const G: f64 = 9.80665;
#[derive(Clone, Copy, Debug)]
pub struct Air {
    pub density: f64,
    pub pressure: f64,
    pub temperature: f64,
    pub sound_speed: f64,
    pub viscosity: f64,
    pub mean_free_path: f64,
}
pub fn sample(altitude: f64) -> Air {
    profile(altitude, 288.15, 101325.0)
}
/// A dry-air standard lapse profile anchored to the launch site's temperature
/// and station pressure. Legacy missions keep the exact standard profile.
pub fn for_mission(altitude: f64, mission: &crate::config::Mission) -> Air {
    let levels = &mission.environment.air_profile;
    if !levels.is_empty()
        && altitude >= levels[0].altitude_m
        && altitude <= levels.last().unwrap().altitude_m
    {
        let i = levels
            .partition_point(|p| p.altitude_m <= altitude)
            .saturating_sub(1);
        let j = (i + 1).min(levels.len() - 1);
        let u = ((altitude - levels[i].altitude_m)
            / (levels[j].altitude_m - levels[i].altitude_m).max(1e-9))
        .clamp(0.0, 1.0);
        return properties(
            levels[i].temperature_k + (levels[j].temperature_k - levels[i].temperature_k) * u,
            (levels[i].pressure_pa.ln()
                + (levels[j].pressure_pa.ln() - levels[i].pressure_pa.ln()) * u)
                .exp(),
        );
    }
    let w = &mission.atmosphere;
    if !w.custom {
        return sample(altitude);
    }
    let site = mission.launch.altitude_m;
    let sea_temperature = 288.15 + (w.temperature_c + 273.15 - sample(site).temperature);
    let pressure_scale = w.pressure_hpa * 100.0 / profile(site, sea_temperature, 101325.0).pressure;
    profile(altitude, sea_temperature, 101325.0 * pressure_scale)
}
fn profile(altitude: f64, sea_temperature: f64, sea_pressure: f64) -> Air {
    let geometric = altitude.max(-500.0);
    let h = EARTH_RADIUS * geometric / (EARTH_RADIUS + geometric);
    let heights = [
        0.0, 11000.0, 20000.0, 32000.0, 47000.0, 51000.0, 71000.0, 84852.0,
    ];
    let lapse = [-0.0065, 0.0, 0.001, 0.0028, 0.0, -0.0028, -0.002];
    let (mut t, mut p) = (sea_temperature, sea_pressure);
    for i in 0..7 {
        let dh = h.min(heights[i + 1]) - heights[i];
        let next_t = t + lapse[i] * dh;
        p *= if lapse[i] == 0.0 {
            (-G * dh / (R * t)).exp()
        } else {
            (t / next_t).powf(G / (R * lapse[i]))
        };
        t = next_t;
        if h <= heights[i + 1] {
            break;
        }
    }
    if h > 84852.0 {
        p *= (-(h - 84852.0) / 7000.0).exp();
    }
    properties(t, p)
}
fn properties(t: f64, p: f64) -> Air {
    let rho = p / (R * t);
    let viscosity = 1.716e-5 * (t / 273.15).powf(1.5) * (273.15 + 110.4) / (t + 110.4);
    Air {
        density: rho,
        pressure: p,
        temperature: t,
        sound_speed: (1.4 * R * t).sqrt(),
        viscosity,
        mean_free_path: viscosity / p.max(1e-30) * (std::f64::consts::PI * R * t / 2.0).sqrt(),
    }
}
pub fn radial(position: DVec3, site_altitude: f64) -> DVec3 {
    position + DVec3::Z * (EARTH_RADIUS + site_altitude)
}
pub fn altitude(position: DVec3, site_altitude: f64) -> f64 {
    radial(position, site_altitude).length() - EARTH_RADIUS
}
pub fn gravity(position: DVec3, site_altitude: f64) -> DVec3 {
    let r = radial(position, site_altitude);
    -MU * r / r.length().powi(3)
}
