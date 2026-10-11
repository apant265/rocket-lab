//! Shared scalar fields and scales for 2D images and 3D surface inspection.
use crate::cfd::{self, Solution};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum FlowField {
    #[default]
    Cp,
    Pressure,
    Speed,
    Mach,
}
impl FlowField {
    pub fn index(self) -> usize {
        match self {
            Self::Cp => 0,
            Self::Pressure => 1,
            Self::Speed => 2,
            Self::Mach => 3,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Cp => "Pressure coefficient",
            Self::Pressure => "Pressure / kPa",
            Self::Speed => "Air speed / m/s",
            Self::Mach => "Mach",
        }
    }
    pub fn unit(self) -> &'static str {
        match self {
            Self::Cp => "Cp",
            Self::Pressure => "kPa",
            Self::Speed => "m/s",
            Self::Mach => "Mach",
        }
    }
}
pub(super) struct Planar {
    pub values: Vec<Option<f64>>,
    pub display: Vec<Option<f64>>,
    pub range: [f64; 2],
    pub width: usize,
    pub height: usize,
    pub x: [f64; 2],
    pub y: [f64; 2],
}
impl Planar {
    pub fn new(f: &Solution, field: FlowField) -> Self {
        let p = &f.problem;
        let far = cfd::pressure(p.far);
        let q = 0.5 * (p.far[1].powi(2) + p.far[2].powi(2)) / p.far[0];
        let mut values = Vec::with_capacity(p.nx * p.ny);
        for j in (0..p.ny).rev() {
            for i in 0..p.nx {
                let n = j * p.nx + i;
                let c = f.cells[n];
                let pressure = cfd::pressure(c);
                let speed = c[1].hypot(c[2]) / c[0];
                let value = match field {
                    FlowField::Cp => (pressure - far) / q.max(1e-12),
                    FlowField::Pressure => pressure / 1000.,
                    FlowField::Speed => speed,
                    FlowField::Mach => speed / (cfd::GAMMA * pressure / c[0]).sqrt(),
                };
                values.push((!p.solid[n] && value.is_finite()).then_some(value));
            }
        }
        let mut range = if field == FlowField::Cp {
            [-0.3, 1.5]
        } else {
            [
                values
                    .iter()
                    .flatten()
                    .copied()
                    .fold(f64::INFINITY, f64::min),
                values
                    .iter()
                    .flatten()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max),
            ]
        };
        if range[1] - range[0] < 1e-9 {
            let delta = range[0].abs().max(1.) * 0.01;
            range = [range[0] - delta, range[1] + delta];
        }
        let display = fill_wall(&values, p.nx, p.ny);
        Self {
            values,
            display,
            range,
            width: p.nx,
            height: p.ny,
            x: [p.x0, p.x0 + p.dx * p.nx as f64],
            y: [p.y0, p.y0 + p.dy * p.ny as f64],
        }
    }
    /// Planar field projected through the vehicle's z direction. It does not
    /// create circumferential CFD data or resolve fin aerodynamics.
    pub fn at(&self, v: [f64; 3]) -> Option<f64> {
        let u = (-v[1] - self.x[0]) / (self.x[1] - self.x[0]);
        let t = (self.y[1] - v[0]) / (self.y[1] - self.y[0]);
        if !(0. ..=1.).contains(&u) || !(0. ..=1.).contains(&t) {
            return None;
        }
        let x = (u * self.width as f64 - 0.5).clamp(0., (self.width - 1) as f64);
        let y = (t * self.height as f64 - 0.5).clamp(0., (self.height - 1) as f64);
        let ix = x as usize;
        let iy = y as usize;
        let mut value = 0.;
        let mut weight = 0.;
        for (col, wx) in [
            (ix, 1. - (x - ix as f64)),
            ((ix + 1).min(self.width - 1), x - ix as f64),
        ] {
            for (row, wy) in [
                (iy, 1. - (y - iy as f64)),
                ((iy + 1).min(self.height - 1), y - iy as f64),
            ] {
                if let Some(v) = self.display[row * self.width + col] {
                    value += v * wx * wy;
                    weight += wx * wy;
                }
            }
        }
        (weight > 1e-9).then_some(value / weight)
    }
}
pub(super) fn fill_wall(values: &[Option<f64>], width: usize, height: usize) -> Vec<Option<f64>> {
    let mut display = values.to_vec();
    let mut distances = vec![usize::MAX; height];
    for col in 0..width {
        let mut previous = None;
        for (row, distance) in distances.iter_mut().enumerate() {
            let index = row * width + col;
            if let Some(v) = values[index] {
                previous = Some((row, v));
                *distance = 0;
            } else if let Some((source, v)) = previous {
                *distance = row - source;
                display[index] = Some(v);
            } else {
                *distance = usize::MAX;
            }
        }
        let mut next = None;
        for row in (0..height).rev() {
            let index = row * width + col;
            if let Some(v) = values[index] {
                next = Some((row, v));
            } else if let Some((source, v)) = next
                && source - row < distances[row]
            {
                display[index] = Some(v);
            }
        }
    }
    display
}

/// Single pass over raw solved fluid cells; no display reconstruction contributes extrema.
pub(super) fn ranges(f: &Solution) -> [[f64; 2]; 4] {
    let mut out = [[f64::INFINITY, f64::NEG_INFINITY]; 4];
    for (i, &c) in f.cells.iter().enumerate() {
        if f.problem.solid[i] {
            continue;
        }
        let p = cfd::pressure(c);
        let v = c[1].hypot(c[2]) / c[0];
        for (range, value) in
            out.iter_mut()
                .zip([0., p / 1000., v, v / (cfd::GAMMA * p / c[0]).sqrt()])
        {
            if value.is_finite() {
                range[0] = range[0].min(value);
                range[1] = range[1].max(value);
            }
        }
    }
    for range in &mut out {
        if !range[0].is_finite() {
            *range = [0., 1.];
        } else if range[1] - range[0] < 1e-9 {
            let delta = range[0].abs().max(1.) * 0.01;
            *range = [range[0] - delta, range[1] + delta];
        }
    }
    out[0] = [-0.3, 1.5];
    out
}
