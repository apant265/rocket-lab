//! Spherical georeferencing using the same Earth and tangent frame as the flight integrator.
use crate::{atmosphere::EARTH_RADIUS, config::Launch};
use glam::DVec3;
use serde::Serialize;
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Coordinate {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
}
fn basis(site: &Launch) -> (DVec3, DVec3, DVec3) {
    let (s, c) = site.latitude_deg.to_radians().sin_cos();
    let (sl, cl) = site.longitude_deg.to_radians().sin_cos();
    (
        DVec3::new(-sl, cl, 0.0),
        DVec3::new(-s * cl, -s * sl, c),
        DVec3::new(c * cl, c * sl, s),
    )
}
pub fn from_enu(site: &Launch, enu: DVec3) -> Coordinate {
    let (e, n, u) = basis(site);
    let p = u * (EARTH_RADIUS + site.altitude_m) + e * enu.x + n * enu.y + u * enu.z;
    Coordinate {
        latitude_deg: p.z.atan2(p.x.hypot(p.y)).to_degrees(),
        longitude_deg: p.y.atan2(p.x).to_degrees(),
        altitude_m: p.length() - EARTH_RADIUS,
    }
}
pub fn to_enu(site: &Launch, c: Coordinate) -> DVec3 {
    let (e, n, u) = basis(site);
    let lat = c.latitude_deg.to_radians();
    let lon = c.longitude_deg.to_radians();
    let r = EARTH_RADIUS + c.altitude_m;
    let p = DVec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()) * r
        - u * (EARTH_RADIUS + site.altitude_m);
    DVec3::new(p.dot(e), p.dot(n), p.dot(u))
}
pub fn from_sample(site: &Launch, east: f64, north: f64, agl: f64) -> Coordinate {
    let r = EARTH_RADIUS + site.altitude_m + agl;
    let up =
        (r * r - east * east - north * north).max(0.0).sqrt() - (EARTH_RADIUS + site.altitude_m);
    from_enu(site, DVec3::new(east, north, up))
}
