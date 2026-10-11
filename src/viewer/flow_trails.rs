//! Display advection through a captured velocity field, not a transient CFD solve.
use crate::cfd::Solution;
use bevy_egui::egui;

pub(super) struct Path {
    points: Vec<[f64; 2]>,
    times: Vec<f64>,
}
impl Path {
    fn at(&self, time: f64) -> Option<[f64; 2]> {
        if time < 0.0 || time > *self.times.last()? {
            return None;
        }
        let end = self
            .times
            .partition_point(|t| *t < time)
            .min(self.times.len() - 1);
        let start = end.saturating_sub(1);
        let fraction =
            (time - self.times[start]) / (self.times[end] - self.times[start]).max(1e-15);
        Some(std::array::from_fn(|i| {
            self.points[start][i] + fraction * (self.points[end][i] - self.points[start][i])
        }))
    }
}
pub(super) struct Trails {
    paths: Vec<Path>,
    transit: f64,
}
fn velocity(f: &Solution, point: [f64; 2]) -> Option<[f64; 2]> {
    let p = &f.problem;
    let grid = [(point[0] - p.x0) / p.dx, (point[1] - p.y0) / p.dy];
    if grid[0] < 0. || grid[1] < 0. || grid[0] >= p.nx as f64 || grid[1] >= p.ny as f64 {
        return None;
    }
    // A point in a solid cell cannot borrow a fluid velocity from its neighbors.
    if p.solid[grid[1] as usize * p.nx + grid[0] as usize] {
        return None;
    }
    let centered = [
        (grid[0] - 0.5).clamp(0., (p.nx - 1) as f64),
        (grid[1] - 0.5).clamp(0., (p.ny - 1) as f64),
    ];
    let ix = centered[0] as usize;
    let iy = centered[1] as usize;
    let tx = centered[0] - ix as f64;
    let ty = centered[1] - iy as f64;
    let mut result = [0.; 2];
    let mut weight = 0.;
    for (x, wx) in [(ix, 1. - tx), ((ix + 1).min(p.nx - 1), tx)] {
        for (y, wy) in [(iy, 1. - ty), ((iy + 1).min(p.ny - 1), ty)] {
            let i = y * p.nx + x;
            if p.solid[i] {
                continue;
            }
            let c = f.cells[i];
            let w = wx * wy;
            if !c[0].is_finite() || c[0] <= 0. {
                return None;
            }
            for axis in 0..2 {
                result[axis] += w * c[axis + 1] / c[0];
            }
            weight += w;
        }
    }
    if weight < 1e-9 {
        return None;
    }
    result = result.map(|v| v / weight);
    result.iter().all(|v| v.is_finite()).then_some(result)
}
fn integrate(f: &Solution, start: [f64; 2], distance: f64) -> Path {
    let mut path = Path {
        points: vec![start],
        times: vec![0.],
    };
    let mut point = start;
    let mut time = 0.;
    for _ in 0..8192 {
        let Some(v) = velocity(f, point) else { break };
        let speed = v[0].hypot(v[1]);
        if speed < 1e-9 {
            break;
        }
        let dt = distance / speed;
        let midpoint = [point[0] + v[0] * dt * 0.5, point[1] + v[1] * dt * 0.5];
        let Some(mid) = velocity(f, midpoint) else {
            break;
        };
        let next = [point[0] + mid[0] * dt, point[1] + mid[1] * dt];
        if velocity(f, next).is_none() {
            break;
        }
        point = next;
        time += dt;
        path.points.push(point);
        path.times.push(time);
    }
    path
}
impl Trails {
    pub(super) fn new(f: &Solution) -> Self {
        let p = &f.problem;
        let width = p.nx as f64 * p.dx;
        let height = p.ny as f64 * p.dy;
        let speed = p.far[1].hypot(p.far[2]) / p.far[0];
        let count = match p.nx {
            0..=128 => 28,
            129..=512 => 64,
            513..=1024 => 104,
            _ => 160,
        };
        let distance = (width / 1800.).min(p.dx.min(p.dy) * 1.5);
        let mut paths = vec![];
        // Seed incoming boundaries. All paths advance using local computed velocity.
        for vertical in [true, false] {
            let normal = if vertical { p.far[1] } else { p.far[2] };
            let fraction = normal.abs() / p.far[1].hypot(p.far[2]).max(1e-9);
            if fraction < 0.01 {
                continue;
            }
            let seeds = (count as f64 * fraction).ceil() as usize;
            for seed in 0..seeds {
                let u = (seed as f64 + 0.5) / seeds as f64;
                let start = if vertical {
                    [
                        p.x0 + if normal > 0. {
                            p.dx * 0.6
                        } else {
                            width - p.dx * 0.6
                        },
                        p.y0 + height * u,
                    ]
                } else {
                    [
                        p.x0 + width * u,
                        p.y0 + if normal > 0. {
                            p.dy * 0.6
                        } else {
                            height - p.dy * 0.6
                        },
                    ]
                };
                let mut path = integrate(f, start, distance);
                if path.points.len() > 8 {
                    for point in &mut path.points {
                        point[0] = (point[0] - p.x0) / width;
                        point[1] = 1. - (point[1] - p.y0) / height;
                    }
                    paths.push(path);
                }
            }
        }
        Self {
            paths,
            transit: width / speed.max(1e-9),
        }
    }
    pub(super) fn paint(&self, painter: &egui::Painter, rect: egui::Rect, clock: f64) {
        let screen = |point: [f64; 2]| {
            egui::pos2(
                rect.left() + point[0] as f32 * rect.width(),
                rect.top() + point[1] as f32 * rect.height(),
            )
        };
        // Six display seconds per freestream transit. Relative local speeds are retained.
        let physical_time = clock * self.transit / 6.;
        // Keep the flow readable when the long rocket section is fitted into a
        // short image; zooming reveals the complete set of integrated paths.
        let visible = (rect.height() / 3.5).max(12.0) as usize;
        let stride = self.paths.len().div_ceil(visible).max(1);
        for (i, path) in self.paths.iter().enumerate() {
            if !i.is_multiple_of(stride) {
                continue;
            }
            let duration = *path.times.last().unwrap();
            for particle in 0..3 {
                let head = (physical_time
                    + duration * (particle as f64 / 3. + (i as f64 * 0.618).fract()))
                .rem_euclid(duration);
                let mut previous = None;
                for segment in 0..=24 {
                    let time = head - self.transit * 0.045 * (1. - segment as f64 / 24.);
                    let Some(point) = path.at(time).map(screen) else {
                        continue;
                    };
                    if let Some(last) = previous {
                        let opacity = (segment * 6).min(145) as u8;
                        painter.line_segment(
                            [last, point],
                            egui::Stroke::new(3.5, egui::Color32::from_white_alpha(opacity / 5)),
                        );
                        painter.line_segment(
                            [last, point],
                            egui::Stroke::new(1.25, egui::Color32::from_white_alpha(opacity)),
                        );
                    }
                    previous = Some(point);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn uniform(u: f64, v: f64) -> Solution {
        let p = crate::cfd::Problem {
            nx: 64,
            ny: 64,
            dx: 0.1,
            dy: 0.1,
            x0: 0.,
            y0: 0.,
            solid: vec![false; 4096],
            far: crate::cfd::conserved(1., u, v, 100000.),
            steps: 1,
            dt: 0.01,
            mach: 0.1,
            alpha_deg: 0.,
        };
        Solution {
            cells: vec![p.far; 4096],
            problem: p,
            residuals: vec![],
            backend: "test".into(),
        }
    }
    #[test]
    fn trajectories_follow_velocity_and_speed_with_no_wall_crossing() {
        let f = uniform(2., 1.);
        let path = integrate(&f, [0.1, 0.2], 0.01);
        for (point, time) in path.points.iter().zip(&path.times) {
            assert!((point[0] - (0.1 + 2. * time)).abs() < 1e-9);
            assert!((point[1] - (0.2 + time)).abs() < 1e-9);
        }
        let faster = integrate(&uniform(4., 2.), [0.1, 0.2], 0.01);
        assert!((path.times.last().unwrap() / faster.times.last().unwrap() - 2.).abs() < 1e-9);
        let mut wall = uniform(2., 0.);
        for y in 0..64 {
            wall.problem.solid[y * 64 + 30] = true;
        }
        let blocked = integrate(&wall, [0.1, 0.2], 0.01);
        assert!(blocked.points.iter().all(|p| p[0] < 3.));
        assert!(velocity(&wall, [3.05, 0.2]).is_none());
        assert!(Trails::new(&uniform(0., 0.)).paths.is_empty());
    }
    #[test]
    fn timed_samples_interpolate_without_wrapping_a_tail_across_the_domain() {
        let path = Path {
            points: vec![[0., 0.], [1., 0.]],
            times: vec![0., 2.],
        };
        assert_eq!(path.at(1.), Some([0.5, 0.]));
        assert_eq!(path.at(-0.1), None);
        assert_eq!(path.at(2.1), None);
    }
}
