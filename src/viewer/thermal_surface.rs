//! Mapping computed material temperatures onto exported geometry. This does not
//! add thermal degrees of freedom or invent circumferential temperature fields.
use super::*;
use crate::thermal;

/// Spatial interpolation stays inside the selected component and material.
#[derive(Clone, Copy, Debug)]
pub(super) struct Blend {
    pub a: usize,
    pub b: usize,
    pub u: f64,
}
impl Blend {
    pub fn value(self, values: &[Option<f64>]) -> Option<f64> {
        let a = *values.get(self.a)?.as_ref()?;
        if self.a == self.b {
            return Some(a);
        }
        Some(a * (1. - self.u) + *values.get(self.b)?.as_ref()? * self.u)
    }
}
pub(super) struct Mapping<'a> {
    h: &'a thermal::History,
    columns: Vec<usize>,
    offset: f64,
    radius: f64,
    span: f64,
    fin: bool,
    continuous: Vec<bool>,
}
impl<'a> Mapping<'a> {
    pub fn new(
        h: &'a thermal::History,
        mission: &Mission,
        stage: usize,
        kind: &str,
        component: Option<usize>,
    ) -> Self {
        let s = &mission.stages[stage];
        let part = component.and_then(|i| s.components.get(i));
        let region = if kind == "body" || kind == "nose" {
            kind
        } else {
            part.map(|p| p.name.as_str())
                .filter(|name| {
                    h.columns
                        .iter()
                        .any(|c| c.stage == stage && c.kind == *name)
                })
                .unwrap_or(kind)
        };
        let fin = region == "fin";
        let mut columns: Vec<_> = h
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.stage == stage
                    && c.kind == region
                    && part.is_none_or(|p| {
                        fin || c.station_m >= p.position_m - 1e-6
                            && c.station_m <= p.position_m + p.length_m + 1e-6
                    })
            })
            .map(|(i, _)| i)
            .collect();
        columns.sort_by(|a, b| {
            let coordinate = |i: usize| {
                if fin {
                    h.columns[i].radial_fraction
                } else {
                    h.columns[i].station_m
                }
            };
            coordinate(*a).total_cmp(&coordinate(*b))
        });
        let continuous = columns
            .windows(2)
            .map(|pair| {
                let a = &h.columns[pair[0]];
                let b = &h.columns[pair[1]];
                a.thickness_m == b.thickness_m
                    && serde_json::to_value(&a.material).ok()
                        == serde_json::to_value(&b.material).ok()
                    && (fin
                        || a.axial_bounds_m
                            .zip(b.axial_bounds_m)
                            .is_none_or(|(a, b)| (a[1] - b[0]).abs() < 1e-6))
            })
            .collect();
        Self {
            h,
            columns,
            continuous,
            offset: mission.stages[stage + 1..].iter().map(|s| s.length_m).sum(),
            radius: s.diameter_m * 0.5,
            span: s.geometry.as_ref().map_or(1., |g| g.fin_span_m.max(1e-9)),
            fin,
        }
    }
    pub fn at(&self, v: [f64; 3]) -> Option<Blend> {
        if self.columns.is_empty() {
            return None;
        }
        let x = if self.fin {
            ((v[0].hypot(v[2]) - self.radius) / self.span).clamp(0., 1.)
        } else {
            -v[1] - self.offset
        };
        let coordinate = |i: usize| {
            if self.fin {
                self.h.columns[i].radial_fraction
            } else {
                self.h.columns[i].station_m
            }
        };
        let hi = self
            .columns
            .partition_point(|&i| coordinate(i) < x)
            .min(self.columns.len() - 1);
        let lo = hi.saturating_sub(1);
        let a = self.columns[lo];
        let b = self.columns[hi];
        let nearest = if (coordinate(a) - x).abs() <= (coordinate(b) - x).abs() {
            a
        } else {
            b
        };
        let contained = |i: usize| {
            self.fin
                || self.h.columns[i]
                    .axial_bounds_m
                    .is_none_or(|r| x >= r[0] - 1e-6 && x <= r[1] + 1e-6)
        };
        let selected = if contained(nearest) {
            nearest
        } else if contained(a) {
            a
        } else if contained(b) {
            b
        } else {
            return None;
        };
        if a == b || !self.continuous[lo] {
            return Some(Blend {
                a: selected,
                b: selected,
                u: 0.,
            });
        }
        Some(Blend {
            a,
            b,
            u: ((x - coordinate(a)) / (coordinate(b) - coordinate(a)).max(1e-12)).clamp(0., 1.),
        })
    }
}
pub(super) fn temperature(h: &thermal::History, index: usize, time: f64, inner: bool) -> f64 {
    let frame = h.frame(time);
    let next = (frame + 1).min(h.times.len() - 1);
    let u = ((time - h.times[frame]) / (h.times[next] - h.times[frame]).max(1e-9)).clamp(0., 1.);
    let layer = usize::from(inner);
    h.temperatures[frame][index][layer] * (1. - u) + h.temperatures[next][index][layer] * u
}
pub(super) fn range(h: &thermal::History, time: f64, inner: bool, active_stage: usize) -> [f64; 2] {
    scoped_range(h, time, inner, active_stage, None)
}
pub(super) fn scoped_range(
    h: &thermal::History,
    time: f64,
    inner: bool,
    active_stage: usize,
    only: Option<usize>,
) -> [f64; 2] {
    let mut range = [f64::INFINITY, f64::NEG_INFINITY];
    for (i, _) in h
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| only.map_or(c.stage >= active_stage, |s| c.stage == s))
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

pub(super) type Face = ([[f64; 3]; 3], [Option<Blend>; 3]);
pub(super) struct ShapeRaster {
    pub width: usize,
    pub height: usize,
    pub x: [f64; 2],
    pub y: [f64; 2],
    pub covered: Vec<bool>,
    pub columns: Vec<Option<usize>>,
    pub blends: Vec<Option<Blend>>,
    pub faces: Vec<Face>,
    pub first_stage: usize,
    pub only: Option<usize>,
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
        let width = 4096;
        let height = ((y[1] - y[0]) / (x[1] - x[0]) * width as f64)
            .ceil()
            .clamp(128., 2048.) as usize;
        let mut out = Self {
            width,
            height,
            x,
            y,
            covered: vec![false; width * height],
            columns: vec![None; width * height],
            blends: vec![None; width * height],
            faces: Vec::new(),
            first_stage,
            only,
        };
        let mut depth = vec![f64::NEG_INFINITY; width * height];
        for p in parts {
            let mapping = Mapping::new(h, mission, p.stage, &p.kind, p.component_index);
            let refined = field_mesh::refined(p, mission);
            for triangle in refined.indices.chunks_exact(3) {
                let vertices =
                    std::array::from_fn(|i| refined.positions[triangle[i] as usize].map(f64::from));
                out.faces.push((vertices, vertices.map(|v| mapping.at(v))));
            }
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
                        out.blends[n] = mapping.at(vertex);
                        out.columns[n] = out.blends[n].map(|b| if b.u < 0.5 { b.a } else { b.b });
                    }
                }
            }
        }
        out.faces.sort_by(|a, b| {
            let depth = |face: &Face| face.0.iter().map(|v| v[2]).sum::<f64>();
            depth(a).total_cmp(&depth(b))
        });
        Some(out)
    }
}
