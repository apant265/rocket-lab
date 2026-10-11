//! Continuous body silhouette for display; the finite-volume wall stays Cartesian.
use bevy_egui::egui;
#[derive(Clone)]
pub(super) struct Boundary {
    /// Normalized x and radius / displayed domain height (centerline is v=0.5).
    points: Vec<[f64; 2]>,
}
impl Boundary {
    pub(super) fn new(m: &crate::config::Mission, active: usize, p: &crate::cfd::Problem) -> Self {
        let length = m.stages[active..].iter().map(|s| s.length_m).sum::<f64>();
        let mut stations = (0..=4096)
            .map(|i| length * i as f64 / 4096.)
            .collect::<Vec<_>>();
        let mut offset = 0.;
        for stage in m.stages[active..].iter().rev() {
            for station in [
                0.,
                stage.length_m,
                stage.geometry.as_ref().map_or(0., |g| g.nose_length_m),
            ] {
                for d in [-1e-9, 0., 1e-9] {
                    stations.push((offset + station + d).clamp(0., length));
                }
            }
            for part in &stage.components {
                for station in [part.position_m, part.position_m + part.length_m] {
                    for d in [-1e-9, 0., 1e-9] {
                        stations.push((offset + station + d).clamp(0., length));
                    }
                }
            }
            offset += stage.length_m;
        }
        stations.sort_by(f64::total_cmp);
        stations.dedup();
        let points = stations
            .into_iter()
            .map(|x| {
                [
                    (x - p.x0) / (p.nx as f64 * p.dx),
                    crate::cfd::body_radius(m, active, x) / (p.ny as f64 * p.dy),
                ]
            })
            .collect();
        Self { points }
    }
    pub(super) fn radius(&self, x: f64) -> f64 {
        if x < self.points[0][0] || x > self.points.last().unwrap()[0] {
            return 0.;
        }
        let end = self
            .points
            .partition_point(|p| p[0] < x)
            .min(self.points.len() - 1);
        let start = end.saturating_sub(1);
        let a = self.points[start];
        let b = self.points[end];
        let t = (x - a[0]) / (b[0] - a[0]).max(1e-15);
        a[1] + t * (b[1] - a[1])
    }
    pub(super) fn pixel_radii(&self, width: usize) -> Vec<[f64; 4]> {
        (0..width)
            .map(|i| {
                std::array::from_fn(|s| {
                    self.radius((i as f64 + (s as f64 + 0.5) / 4.) / width as f64)
                })
            })
            .collect()
    }
    pub(super) fn coverage(radii: [f64; 4], row: usize, height: usize) -> f32 {
        let low = row as f64 / height as f64;
        let high = (row + 1) as f64 / height as f64;
        (radii
            .iter()
            .map(|r| ((high.min(0.5 + r) - low.max(0.5 - r)) * height as f64).clamp(0., 1.))
            .sum::<f64>()
            / 4.) as f32
    }
    pub(super) fn paint(&self, painter: &egui::Painter, r: egui::Rect) {
        let screen = |point: [f64; 2], sign: f64| {
            egui::pos2(
                r.left() + point[0] as f32 * r.width(),
                r.top() + (0.5 + sign * point[1]) as f32 * r.height(),
            )
        };
        let color = egui::Color32::from_gray(80);
        let mut mesh = egui::Mesh::default();
        for pair in self.points.windows(2) {
            if pair[0][1] == 0. && pair[1][1] == 0. {
                continue;
            }
            let start = mesh.vertices.len() as u32;
            for point in [
                screen(pair[0], -1.),
                screen(pair[1], -1.),
                screen(pair[1], 1.),
                screen(pair[0], 1.),
            ] {
                mesh.colored_vertex(point, color);
            }
            mesh.add_triangle(start, start + 1, start + 2);
            mesh.add_triangle(start, start + 2, start + 3);
        }
        painter.add(egui::Shape::mesh(mesh));
        for sign in [-1., 1.] {
            let points = self.points.iter().map(|p| screen(*p, sign)).collect();
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(1., egui::Color32::from_gray(115)),
            ));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_transition_outline_is_tapered_at_every_zoom() {
        let mut mission = crate::config::Mission::load("examples/kate-anvil-desert.json").unwrap();
        crate::design::initialize_geometry(&mut mission);
        let p = crate::cfd::prepare(
            &mission,
            crate::cfd::Request {
                stage: 0,
                altitude_m: 0.,
                mach: 1.,
                alpha_deg: 0.,
                grid: [96, 64],
                steps: 1,
            },
        )
        .unwrap();
        let boundary = Boundary::new(&mission, 0, &p);
        for index in 0..2 {
            let taper = &mission.stages[index].components[0];
            let offset: f64 = mission.stages[index + 1..].iter().map(|s| s.length_m).sum();
            for fraction in [0.1, 0.25, 0.5, 0.75, 0.9] {
                let x = offset + taper.length_m * fraction;
                let u = (x - p.x0) / (p.nx as f64 * p.dx);
                let radius = boundary.radius(u) * (p.ny as f64 * p.dy);
                let expected = (taper.fore_diameter_m
                    + (taper.diameter_m - taper.fore_diameter_m) * fraction)
                    * 0.5;
                assert!((radius - expected).abs() < 1e-9);
                assert!(radius < taper.diameter_m * 0.5);
            }
        }
    }
    #[test]
    fn smooth_boundary_and_partial_pixel_coverage_do_not_depend_on_grid_stairs() {
        let mission = crate::config::Mission::demo(false);
        let p = crate::cfd::prepare(
            &mission,
            crate::cfd::Request {
                stage: 0,
                altitude_m: 0.,
                mach: 1.,
                alpha_deg: 0.,
                grid: [96, 64],
                steps: 1,
            },
        )
        .unwrap();
        let boundary = Boundary::new(&mission, 0, &p);
        let length = mission.stages[0].length_m;
        for x in [length * 0.017, length * 0.123, length * 0.75] {
            let u = (x - p.x0) / (p.nx as f64 * p.dx);
            assert!(
                (boundary.radius(u) * p.ny as f64 * p.dy - crate::cfd::body_radius(&mission, 0, x))
                    .abs()
                    < 1e-6
            );
        }
        let coverage = Boundary::coverage([0.123; 4], 39, 64);
        assert!(coverage > 0. && coverage < 1.);
        assert_eq!(Boundary::coverage([0.; 4], 32, 64), 0.);
        assert_eq!(Boundary::coverage([0.123; 4], 32, 64), 1.);
    }
}
