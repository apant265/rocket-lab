//! Mapping computed material temperatures onto exported geometry. This does not
//! add thermal degrees of freedom or invent circumferential temperature fields.
use super::*;
use crate::thermal;

pub(super) fn column(
    h: &thermal::History,
    mission: &Mission,
    stage: usize,
    kind: &str,
    component: Option<usize>,
    vertex: [f64; 3],
) -> Option<usize> {
    let s = mission.stages.get(stage)?;
    let component_name = component
        .and_then(|i| s.components.get(i))
        .map(|p| p.name.as_str());
    let region = if kind == "body" || kind == "nose" {
        kind
    } else {
        component_name
            .filter(|name| {
                h.columns
                    .iter()
                    .any(|c| c.stage == stage && c.kind == *name)
            })
            .unwrap_or(kind)
    };
    let offset = mission.stages[stage + 1..]
        .iter()
        .map(|s| s.length_m)
        .sum::<f64>();
    let station = -vertex[1] - offset;
    let fraction = s.geometry.as_ref().map(|g| {
        ((vertex[0].hypot(vertex[2]) - s.diameter_m * 0.5) / g.fin_span_m.max(1e-9)).clamp(0., 1.)
    });
    h.columns
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.stage == stage
                && c.kind == region
                && c.axial_bounds_m
                    .is_none_or(|b| station >= b[0] - 1e-6 && station <= b[1] + 1e-6)
        })
        .min_by(|(_, a), (_, b)| {
            let distance = |c: &thermal::Column| {
                if region == "fin" {
                    (c.radial_fraction - fraction.unwrap_or(0.)).abs()
                } else {
                    (c.station_m - station).abs()
                }
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(i, _)| i)
}
pub(super) fn temperature(h: &thermal::History, index: usize, time: f64, inner: bool) -> f64 {
    let frame = h.frame(time);
    let next = (frame + 1).min(h.times.len() - 1);
    let u = ((time - h.times[frame]) / (h.times[next] - h.times[frame]).max(1e-9)).clamp(0., 1.);
    let layer = usize::from(inner);
    h.temperatures[frame][index][layer] * (1. - u) + h.temperatures[next][index][layer] * u
}
pub(super) fn range(h: &thermal::History, time: f64, inner: bool, active_stage: usize) -> [f64; 2] {
    let mut range = [f64::INFINITY, f64::NEG_INFINITY];
    for (i, _) in h
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.stage >= active_stage)
    {
        let t = temperature(h, i, time, inner);
        range[0] = range[0].min(t);
        range[1] = range[1].max(t);
    }
    if !range[0].is_finite() {
        return [273.15, 274.15];
    }
    if range[1] - range[0] < 1e-6 {
        range = [range[0] - 0.5, range[1] + 0.5];
    }
    range
}

pub(super) struct ShapeRaster {
    pub width: usize,
    pub height: usize,
    pub x: [f64; 2],
    pub y: [f64; 2],
    pub covered: Vec<bool>,
    pub columns: Vec<Option<usize>>,
}
impl ShapeRaster {
    pub fn new(
        model: &MeshDocument,
        mission: &Mission,
        h: &thermal::History,
        first_stage: usize,
        only: Option<usize>,
    ) -> Option<Self> {
        let parts: Vec<_> = model
            .parts
            .iter()
            .filter(|p| only.map_or(p.stage >= first_stage, |s| p.stage == s))
            .collect();
        let mut x = [f64::INFINITY, f64::NEG_INFINITY];
        let mut y = x;
        for p in &parts {
            for v in &p.positions {
                x[0] = x[0].min(-v[1] as f64);
                x[1] = x[1].max(-v[1] as f64);
                y[0] = y[0].min(v[0] as f64);
                y[1] = y[1].max(v[0] as f64);
            }
        }
        if !x[0].is_finite() || x[1] <= x[0] || y[1] <= y[0] {
            return None;
        }
        let margin = (x[1] - x[0]) * 0.015;
        x = [x[0] - margin, x[1] + margin];
        y = [y[0] - margin, y[1] + margin];
        let width = 1536;
        let height = ((y[1] - y[0]) / (x[1] - x[0]) * width as f64)
            .ceil()
            .clamp(64., 768.) as usize;
        let mut out = Self {
            width,
            height,
            x,
            y,
            covered: vec![false; width * height],
            columns: vec![None; width * height],
        };
        let mut depth = vec![f64::NEG_INFINITY; width * height];
        for p in parts {
            for triangle in p.indices.chunks_exact(3) {
                let vertices: [[f64; 3]; 3] =
                    std::array::from_fn(|i| p.positions[triangle[i] as usize].map(f64::from));
                let projected = vertices.map(|v| {
                    [
                        (-v[1] - x[0]) / (x[1] - x[0]) * width as f64,
                        (y[1] - v[0]) / (y[1] - y[0]) * height as f64,
                    ]
                });
                let [a, b, c] = projected;
                let denom = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                if denom.abs() < 1e-10 {
                    continue;
                }
                let bounds = |axis: usize, limit: usize| {
                    let low = projected
                        .iter()
                        .map(|v| v[axis])
                        .fold(f64::INFINITY, f64::min)
                        .floor()
                        .max(0.) as usize;
                    let high = projected
                        .iter()
                        .map(|v| v[axis])
                        .fold(f64::NEG_INFINITY, f64::max)
                        .ceil()
                        .max(0.) as usize;
                    (low.min(limit), high.min(limit))
                };
                let (left, right) = bounds(0, width);
                let (top, bottom) = bounds(1, height);
                for row in top..bottom {
                    for col in left..right {
                        let point = [col as f64 + 0.5, row as f64 + 0.5];
                        let u = ((b[1] - c[1]) * (point[0] - c[0])
                            + (c[0] - b[0]) * (point[1] - c[1]))
                            / denom;
                        let v = ((c[1] - a[1]) * (point[0] - c[0])
                            + (a[0] - c[0]) * (point[1] - c[1]))
                            / denom;
                        let w = 1. - u - v;
                        if u < -1e-8 || v < -1e-8 || w < -1e-8 {
                            continue;
                        }
                        let vertex = std::array::from_fn(|axis| {
                            u * vertices[0][axis] + v * vertices[1][axis] + w * vertices[2][axis]
                        });
                        let n = row * width + col;
                        if vertex[2] <= depth[n] {
                            continue;
                        }
                        depth[n] = vertex[2];
                        out.covered[n] = true;
                        out.columns[n] =
                            column(h, mission, p.stage, &p.kind, p.component_index, vertex);
                    }
                }
            }
        }
        Some(out)
    }
}
