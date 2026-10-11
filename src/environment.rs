//! Imported profiles and georeferenced terrain. Heights are MSL, never inferred
//! from satellite image colors. Out-of-bounds terrain uses the launch elevation.
use crate::{
    config::{Mission, Result},
    geo,
};
use glam::DVec3;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Environment {
    pub source: String,
    pub wind_profile: Vec<WindLevel>,
    pub air_profile: Vec<AirLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terrain: Option<Terrain>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindLevel {
    pub altitude_m: f64,
    pub east_mps: f64,
    pub north_mps: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AirLevel {
    pub altitude_m: f64,
    pub pressure_pa: f64,
    pub temperature_k: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
    pub source: String,
    pub west_deg: f64,
    pub south_deg: f64,
    pub east_deg: f64,
    pub north_deg: f64,
    pub width: usize,
    pub height: usize,
    /// Row-major, north row first; all values are meters above mean sea level.
    pub elevation_m: Vec<f64>,
}
impl Terrain {
    pub fn elevation(&self, latitude: f64, longitude: f64) -> Option<f64> {
        let x =
            (longitude - self.west_deg) / (self.east_deg - self.west_deg) * (self.width - 1) as f64;
        let y = (self.north_deg - latitude) / (self.north_deg - self.south_deg)
            * (self.height - 1) as f64;
        if !(0.0..=(self.width - 1) as f64).contains(&x)
            || !(0.0..=(self.height - 1) as f64).contains(&y)
        {
            return None;
        }
        let ix = (x.floor() as usize).min(self.width - 2);
        let iy = (y.floor() as usize).min(self.height - 2);
        let (u, v) = (x - ix as f64, y - iy as f64);
        let at = |dx, dy| self.elevation_m[(iy + dy) * self.width + ix + dx];
        Some(
            (at(0, 0) * (1.0 - u) + at(1, 0) * u) * (1.0 - v)
                + (at(0, 1) * (1.0 - u) + at(1, 1) * u) * v,
        )
    }
    pub fn validate(&self) -> Result<()> {
        if self.width < 2
            || self.height < 2
            || self.width > 4096
            || self.height > 4096
            || self.width.checked_mul(self.height) != Some(self.elevation_m.len())
            || ![-180.0..=180.0]
                .iter()
                .all(|r| r.contains(&self.west_deg) && r.contains(&self.east_deg))
            || !(-90.0..=90.0).contains(&self.south_deg)
            || !(-90.0..=90.0).contains(&self.north_deg)
            || self.east_deg <= self.west_deg
            || self.north_deg <= self.south_deg
            || self
                .elevation_m
                .iter()
                .any(|z| !z.is_finite() || !(-500.0..=9000.0).contains(z))
        {
            return Err("Invalid terrain grid, extent or MSL elevations".into());
        }
        Ok(())
    }
}
impl Environment {
    pub fn validate(&self) -> Result<()> {
        if self.wind_profile.iter().any(|p| {
            ![p.altitude_m, p.east_mps, p.north_mps]
                .iter()
                .all(|v| v.is_finite())
                || p.east_mps.abs() > 200.0
                || p.north_mps.abs() > 200.0
        }) || self
            .wind_profile
            .windows(2)
            .any(|w| w[1].altitude_m <= w[0].altitude_m)
            || self.air_profile.iter().any(|p| {
                ![p.altitude_m, p.pressure_pa, p.temperature_k]
                    .iter()
                    .all(|v| v.is_finite())
                    || p.pressure_pa <= 0.0
                    || !(100.0..=2000.0).contains(&p.temperature_k)
            })
            || self
                .air_profile
                .windows(2)
                .any(|w| w[1].altitude_m <= w[0].altitude_m || w[1].pressure_pa >= w[0].pressure_pa)
        {
            return Err(
                "Profiles need finite, increasing MSL heights and positive decreasing air pressure"
                    .into(),
            );
        }
        if let Some(t) = &self.terrain {
            t.validate()?;
        }
        Ok(())
    }
}
pub fn wind(m: &Mission, altitude_m: f64, t: f64) -> DVec3 {
    let p = &m.environment.wind_profile;
    if p.is_empty() {
        return m.wind.velocity(altitude_m - m.launch.altitude_m, t);
    }
    let i = p
        .partition_point(|p| p.altitude_m <= altitude_m)
        .saturating_sub(1);
    let j = (i + 1).min(p.len() - 1);
    let u = ((altitude_m - p[i].altitude_m) / (p[j].altitude_m - p[i].altitude_m).max(1e-9))
        .clamp(0.0, 1.0);
    let g = m.wind.gust_mps * (t * std::f64::consts::TAU / m.wind.gust_period_s).sin();
    DVec3::new(
        p[i].east_mps + (p[j].east_mps - p[i].east_mps) * u + g,
        p[i].north_mps + (p[j].north_mps - p[i].north_mps) * u + g * 0.37,
        0.0,
    )
}
pub fn ground_msl(m: &Mission, position: DVec3) -> f64 {
    if m.environment.terrain.is_none() {
        return m.launch.altitude_m;
    }
    let c = geo::from_enu(&m.launch, position);
    m.environment
        .terrain
        .as_ref()
        .and_then(|t| t.elevation(c.latitude_deg, c.longitude_deg))
        .unwrap_or(m.launch.altitude_m)
}
pub fn clearance(m: &Mission, position: DVec3) -> f64 {
    crate::atmosphere::altitude(position, m.launch.altitude_m) - ground_msl(m, position)
}
pub fn ground_position(m: &Mission, position: DVec3) -> DVec3 {
    let r = crate::atmosphere::radial(position, m.launch.altitude_m);
    r.normalize() * (crate::atmosphere::EARTH_RADIUS + ground_msl(m, position))
        - DVec3::Z * (crate::atmosphere::EARTH_RADIUS + m.launch.altitude_m)
}
/// ESRI ASCII geographic (degrees) DEM. NODATA is rejected; fill missing samples
/// with a documented method before import. Projected rasters need reprojection.
pub fn import_ascii(path: impl AsRef<std::path::Path>) -> Result<Terrain> {
    let text = std::fs::read_to_string(&path)?;
    let mut lines = text.lines();
    let mut header = std::collections::HashMap::new();
    for _ in 0..6 {
        let f = lines
            .next()
            .ok_or("Missing DEM header")?
            .split_whitespace()
            .collect::<Vec<_>>();
        if f.len() != 2 {
            return Err("Invalid DEM header".into());
        }
        header.insert(f[0].to_lowercase(), f[1].parse::<f64>()?);
    }
    let get = |s: &str| {
        header
            .get(s)
            .copied()
            .ok_or_else(|| format!("DEM missing {s}"))
    };
    let (w, h) = (get("ncols")?, get("nrows")?);
    if w.fract() != 0.0
        || h.fract() != 0.0
        || !(2.0..=4096.0).contains(&w)
        || !(2.0..=4096.0).contains(&h)
    {
        return Err("DEM dimensions must be integer 2–4096".into());
    }
    let cell = get("cellsize")?;
    if cell <= 0.0 {
        return Err("DEM cellsize must be positive".into());
    }
    let origin = |corner: &str, center: &str| {
        header
            .get(center)
            .copied()
            .or_else(|| header.get(corner).map(|v| v + cell * 0.5))
            .ok_or_else(|| format!("DEM missing {corner} / {center}"))
    };
    let west = origin("xllcorner", "xllcenter")?;
    let south = origin("yllcorner", "yllcenter")?;
    let elevation_m = lines
        .flat_map(str::split_whitespace)
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if header
        .get("nodata_value")
        .is_some_and(|n| elevation_m.contains(n))
    {
        return Err("DEM contains NODATA; fill/reproject before importing".into());
    }
    let terrain = Terrain {
        source: path.as_ref().display().to_string(),
        west_deg: west,
        south_deg: south,
        east_deg: west + (w - 1.0) * cell,
        north_deg: south + (h - 1.0) * cell,
        width: w as usize,
        height: h as usize,
        elevation_m,
    };
    terrain.validate()?;
    Ok(terrain)
}
