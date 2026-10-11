//! Unilateral Kelvin-Voigt tether: tension only, with moving rigid-body anchor.
//! Canopy is a point mass with inflation-dependent drag; no fabric aerodynamics.
use glam::DVec3;
#[derive(Clone, Debug)]
pub struct CanopyState {
    pub main: bool,
    pub position: DVec3,
    pub velocity: DVec3,
}
pub fn tension(
    anchor: DVec3,
    anchor_velocity: DVec3,
    canopy: &CanopyState,
    rest_length: f64,
    k: f64,
    c: f64,
) -> DVec3 {
    let delta = canopy.position - anchor;
    let length = delta.length();
    let extension = length - rest_length;
    if extension <= 0.0 {
        return DVec3::ZERO;
    }
    let direction = delta / length;
    let rate = (canopy.velocity - anchor_velocity).dot(direction);
    direction * (k * extension + c * rate).max(0.0)
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Sample {
    pub time_s: f64,
    pub stage: usize,
    pub main: bool,
    pub east_m: f64,
    pub north_m: f64,
    pub altitude_agl_m: f64,
    pub tension_n: f64,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tether_never_pushes() {
        let c = CanopyState {
            main: true,
            position: DVec3::Z * 5.0,
            velocity: DVec3::ZERO,
        };
        assert_eq!(
            tension(DVec3::ZERO, DVec3::ZERO, &c, 10.0, 500.0, 20.0),
            DVec3::ZERO
        );
        let c = CanopyState {
            position: DVec3::Z * 11.0,
            ..c
        };
        assert_eq!(
            tension(DVec3::ZERO, DVec3::ZERO, &c, 10.0, 500.0, 20.0),
            DVec3::Z * 500.0
        );
    }
}
