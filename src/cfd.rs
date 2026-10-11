//! Experimental two-dimensional perfect-gas Euler finite-volume solver.
//! Conservative Rusanov flux, Cartesian stair-step slip walls, explicit CFL.
//! This is a planar inviscid section, not whole-vehicle drag or a heating solver.
use crate::config::{Mission, Result, StageGeometry};
use serde::Serialize;
pub const GAMMA: f64 = 1.4;
pub type Cell = [f64; 4];
#[derive(Clone, Debug, Serialize)]
pub struct Problem {
    pub nx: usize,
    pub ny: usize,
    pub dx: f64,
    pub dy: f64,
    pub x0: f64,
    pub y0: f64,
    pub solid: Vec<bool>,
    pub far: Cell,
    pub steps: usize,
    pub dt: f64,
    pub mach: f64,
    pub alpha_deg: f64,
}
#[derive(Clone, Debug)]
pub struct Solution {
    pub problem: Problem,
    pub cells: Vec<Cell>,
    pub residuals: Vec<(usize, f64)>,
    pub backend: String,
}
pub fn conserved(rho: f64, u: f64, v: f64, p: f64) -> Cell {
    [
        rho,
        rho * u,
        rho * v,
        p / (GAMMA - 1.0) + 0.5 * rho * (u * u + v * v),
    ]
}
pub fn pressure(c: Cell) -> f64 {
    (GAMMA - 1.0) * (c[3] - 0.5 * (c[1] * c[1] + c[2] * c[2]) / c[0])
}
pub fn wave(c: Cell, axis: usize) -> f64 {
    (c[axis + 1] / c[0]).abs() + (GAMMA * pressure(c) / c[0]).sqrt()
}
pub fn flux(c: Cell, axis: usize) -> Cell {
    let p = pressure(c);
    let v = c[axis + 1] / c[0];
    let mut f = [c[0] * v, c[1] * v, c[2] * v, (c[3] + p) * v];
    f[axis + 1] += p;
    f
}
pub fn rusanov(a: Cell, b: Cell, axis: usize) -> Cell {
    let (fa, fb) = (flux(a, axis), flux(b, axis));
    let speed = wave(a, axis).max(wave(b, axis));
    std::array::from_fn(|i| 0.5 * (fa[i] + fb[i]) - 0.5 * speed * (b[i] - a[i]))
}
pub fn reflected(mut c: Cell, axis: usize) -> Cell {
    c[axis + 1] *= -1.0;
    c
}
#[derive(Clone, Copy)]
pub struct Request {
    pub stage: usize,
    pub altitude_m: f64,
    pub mach: f64,
    pub alpha_deg: f64,
    pub grid: [usize; 2],
    pub steps: usize,
}
/// Continuous axisymmetric body profile, shared by the grid mask and display.
/// Axial x starts at the retained nose; fins are outside this planar section.
pub fn body_radius(m: &Mission, active: usize, x: f64) -> f64 {
    let mut offset = 0.0;
    for (index, s) in m.stages.iter().enumerate().skip(active).rev() {
        if x >= offset && x <= offset + s.length_m {
            let g = s
                .geometry
                .clone()
                .unwrap_or_else(|| StageGeometry::generic(s, index == m.stages.len() - 1));
            let local = x - offset;
            let r = if local < g.nose_length_m {
                crate::design::nose_radius_fraction(
                    g.nose_shape,
                    local / g.nose_length_m,
                    g.nose_length_m / (s.diameter_m * 0.5),
                ) * s.diameter_m
                    * 0.5
            } else {
                s.diameter_m * 0.5
            };
            let r = if s.materials.as_ref().is_some_and(|m| m.component_shell) {
                s.components
                    .iter()
                    .filter(|p| {
                        matches!(
                            p.kind,
                            crate::assembly::PartKind::BodyTube
                                | crate::assembly::PartKind::NoseCone
                                | crate::assembly::PartKind::Transition
                        ) && local >= p.position_m
                            && local <= p.position_m + p.length_m
                    })
                    .map(|p| {
                        let u = (local - p.position_m) / p.length_m;
                        let aft = p.diameter_m * 0.5;
                        match p.kind {
                            crate::assembly::PartKind::NoseCone => {
                                aft * crate::design::nose_radius_fraction(
                                    p.shape
                                        .as_ref()
                                        .map_or(crate::config::NoseShape::Conical, |g| {
                                            g.nose_shape
                                        }),
                                    u,
                                    p.length_m / aft,
                                )
                            }
                            crate::assembly::PartKind::Transition => {
                                p.fore_diameter_m * 0.5 + (aft - p.fore_diameter_m * 0.5) * u
                            }
                            _ => aft,
                        }
                    })
                    .fold(0.0, f64::max)
            } else {
                r
            };
            return r;
        }
        offset += s.length_m;
    }
    0.0
}
pub fn prepare(m: &Mission, request: Request) -> Result<Problem> {
    let Request {
        stage: active,
        altitude_m,
        mach,
        alpha_deg,
        grid: [nx, ny],
        steps,
    } = request;
    if active >= m.stages.len()
        || !(32..=2048).contains(&nx)
        || !(32..=2048).contains(&ny)
        || !(1..=100000).contains(&steps)
        || !mach.is_finite()
        || !(0.05..=15.0).contains(&mach)
        || !alpha_deg.is_finite()
        || !(0.0..=180.0).contains(&alpha_deg)
        || !altitude_m.is_finite()
    {
        return Err("CFD: valid stage, 32–2048 cells per axis, Mach 0.05–15, alpha 0–180, 1–100000 steps required".into());
    }
    let length = m.stages[active..].iter().map(|s| s.length_m).sum::<f64>();
    let radius = m.stages[active..]
        .iter()
        .map(|s| s.diameter_m * 0.5)
        .fold(0.0, f64::max);
    let (x0, y0) = (-0.3 * length, -(radius * 6.0).max(length * 0.12));
    let (dx, dy) = (length * 1.8 / nx as f64, -2.0 * y0 / ny as f64);
    let air = crate::atmosphere::for_mission(altitude_m, m);
    let speed = mach * air.sound_speed;
    let far = conserved(
        air.density,
        speed * alpha_deg.to_radians().cos(),
        speed * alpha_deg.to_radians().sin(),
        air.pressure,
    );
    let radii: Vec<_> = (0..nx)
        .map(|i| body_radius(m, active, x0 + (i as f64 + 0.5) * dx))
        .collect();
    let mut solid = vec![false; nx * ny];
    for j in 0..ny {
        for i in 0..nx {
            let y = y0 + (j as f64 + 0.5) * dy;
            let r = radii[i];
            solid[j * nx + i] = y.abs() <= r && r > 0.0;
        }
    }
    if !solid.iter().any(|s| *s) {
        return Err("Body is unresolved on CFD grid".into());
    }
    let dt = 0.15 * dx.min(dy) / (speed + 3.0 * air.sound_speed);
    Ok(Problem {
        nx,
        ny,
        dx,
        dy,
        x0,
        y0,
        solid,
        far,
        steps,
        dt,
        mach,
        alpha_deg,
    })
}
pub fn advance(p: &Problem, old: &[Cell], dt: f64) -> Vec<Cell> {
    let mut next = old.to_vec();
    for j in 0..p.ny {
        for i in 0..p.nx {
            let k = j * p.nx + i;
            if p.solid[k] {
                continue;
            }
            let c = old[k];
            let neighbor = |x: isize, y: isize, axis| {
                if x < 0 || y < 0 || x >= p.nx as isize || y >= p.ny as isize {
                    return p.far;
                }
                let n = y as usize * p.nx + x as usize;
                if p.solid[n] {
                    reflected(c, axis)
                } else {
                    old[n]
                }
            };
            let right = rusanov(c, neighbor(i as isize + 1, j as isize, 0), 0);
            let left = rusanov(neighbor(i as isize - 1, j as isize, 0), c, 0);
            let top = rusanov(c, neighbor(i as isize, j as isize + 1, 1), 1);
            let bottom = rusanov(neighbor(i as isize, j as isize - 1, 1), c, 1);
            next[k] = std::array::from_fn(|a| {
                c[a] - dt / p.dx * (right[a] - left[a]) - dt / p.dy * (top[a] - bottom[a])
            });
        }
    }
    next
}
pub fn check(cells: &[Cell], solid: &[bool]) -> Result<()> {
    if cells.iter().zip(solid).any(|(c, s)| {
        !*s && (c[0] <= 0.0 || pressure(*c) <= 0.0 || c.iter().any(|v| !v.is_finite()))
    }) {
        return Err("CFD lost positive density/pressure; reduce CFL/refine grid. No clipped state is accepted.".into());
    }
    Ok(())
}
pub fn solve(p: Problem) -> Result<Solution> {
    solve_with_progress(p, |_, _| {})
}
pub fn solve_with_progress(p: Problem, mut progress: impl FnMut(usize, usize)) -> Result<Solution> {
    let mut cells = vec![p.far; p.nx * p.ny];
    let mut residuals = Vec::new();
    for step in 0..p.steps {
        let next = advance(&p, &cells, p.dt);
        check(&next, &p.solid)?;
        if step % 50 == 0 || step + 1 == p.steps {
            let r = cells
                .iter()
                .zip(&next)
                .zip(&p.solid)
                .filter(|(_, s)| !**s)
                .map(|((a, b), _)| (a[0] - b[0]).abs() / p.far[0])
                .fold(0.0, f64::max);
            residuals.push((step + 1, r));
            progress(step + 1, p.steps);
        }
        cells = next;
    }
    Ok(Solution {
        problem: p,
        cells,
        residuals,
        backend: "CPU f64 / planar Euler".into(),
    })
}
impl Solution {
    pub fn export(&self, out: impl AsRef<std::path::Path>) -> Result<()> {
        std::fs::create_dir_all(&out)?;
        let out = out.as_ref();
        let p = &self.problem;
        let mut csv = csv::Writer::from_path(out.join("flow.csv"))?;
        csv.write_record([
            "x_m",
            "y_m",
            "solid",
            "density_kg_m3",
            "pressure_pa",
            "temperature_k",
            "u_mps",
            "v_mps",
            "mach",
            "cp",
        ])?;
        let q = 0.5 * p.far[0] * ((p.far[1] / p.far[0]).powi(2) + (p.far[2] / p.far[0]).powi(2));
        let preview_x = p.nx.div_ceil(1024);
        let preview_y = p.ny.div_ceil(512);
        let mut svg = String::from(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 1024 660'><rect width='1024' height='660' fill='#101925'/><text x='20' y='25' fill='white' font-size='17'>Planar Euler CFD · pressure coefficient (experimental, inviscid)</text>",
        );
        for j in 0..p.ny {
            for i in 0..p.nx {
                let k = j * p.nx + i;
                let c = self.cells[k];
                let press = pressure(c);
                let cp = (press - pressure(p.far)) / q;
                let (u, v) = (c[1] / c[0], c[2] / c[0]);
                let mach = u.hypot(v) / (GAMMA * press / c[0]).sqrt();
                csv.serialize((
                    p.x0 + (i as f64 + 0.5) * p.dx,
                    p.y0 + (j as f64 + 0.5) * p.dy,
                    p.solid[k],
                    c[0],
                    press,
                    press / (287.05287 * c[0]),
                    u,
                    v,
                    mach,
                    cp,
                ))?;
                // CSV retains every cell; bound the vector overview's file size.
                if !i.is_multiple_of(preview_x) || !j.is_multiple_of(preview_y) {
                    continue;
                }
                let color = if p.solid[k] {
                    "#c1ccd6".to_owned()
                } else {
                    let t = ((cp + 0.3) / 1.8).clamp(0.0, 1.0);
                    format!(
                        "rgb({},{},{})",
                        (255.0 * t) as u8,
                        (170.0 * (1.0 - (2.0 * t - 1.0).abs())) as u8,
                        (255.0 * (1.0 - t)) as u8
                    )
                };
                svg.push_str(&format!(
                    "<rect x='{:.2}' y='{:.2}' width='{:.2}' height='{:.2}' fill='{color}'/>",
                    20.0 + i as f64 * 984.0 / p.nx as f64,
                    45.0 + (p.ny - (j + preview_y).min(p.ny)) as f64 * 560.0 / p.ny as f64,
                    984.0 * preview_x as f64 / p.nx as f64 + 0.1,
                    560.0 * preview_y as f64 / p.ny as f64 + 0.1
                ));
            }
        }
        csv.flush()?;
        // Sparse vectors from computed velocity, not analytic streamlines.
        for j in (0..p.ny).step_by((p.ny / 64).max(8)) {
            for i in (0..p.nx).step_by((p.nx / 128).max(8)) {
                let k = j * p.nx + i;
                if p.solid[k] {
                    continue;
                }
                let c = self.cells[k];
                let l = c[1].hypot(c[2]).max(1e-12);
                let x = 20.0 + i as f64 * 984.0 / p.nx as f64;
                let y = 45.0 + (p.ny - 1 - j) as f64 * 560.0 / p.ny as f64;
                svg.push_str(&format!("<path d='M{x:.2},{y:.2} l{:.2},{:.2}' stroke='white' opacity='.45' stroke-width='.7'/>",c[1]/l*7.0,-c[2]/l*7.0));
            }
        }
        let external = self.backend.starts_with("OpenFOAM");
        let limitations = if external {
            "OpenFOAM planar laminar compressible perfect-gas section; Cartesian stair-step no-slip adiabatic wall and freestream farfield. No 3D fins, turbulence, conjugate heating, chemistry, rarefaction or flight feedback. Finite flow-time budget; steady convergence not established."
        } else {
            "2D planar first-order Euler; Cartesian stair-step slip wall. Fixed farfield boundaries. Body section only. No turbulence, boundary layer, thermal flux, fin interference, moving stages/canopies, chemistry or DSMC. Do not use as a full-rocket drag database."
        };
        svg.push_str(if external {"<text x='20' y='638' fill='white' font-size='13'>OpenFOAM planar laminar flow · no-slip adiabatic stair-step wall. Finite-time field; inspect solver logs.</text></svg>"} else {"<text x='20' y='638' fill='white' font-size='13'>Blue Cp ≤ −0.3 · red Cp ≥ 1.5. Slip wall / stair-step grid / γ=1.4. No viscosity, heating, fins, chemistry or rarefaction.</text></svg>"});
        std::fs::write(out.join("pressure-flow.svg"), svg)?;
        let last = self.residuals.last().map(|(_, r)| *r).unwrap_or(1.0);
        std::fs::write(
            out.join("compute.json"),
            serde_json::to_string_pretty(
                &serde_json::json!({"backend":self.backend,"nx":p.nx,"ny":p.ny,"iterations":p.steps,"mach":p.mach,"alpha_deg":p.alpha_deg,"dt_s":p.dt,"flow_time_s":p.dt*p.steps as f64,"residuals":self.residuals,"steady_residual_below_1e_minus_5":last<1e-5,"validated_vehicle_coefficients":false,"limitations":limitations}),
            )?,
        )?;
        Ok(())
    }
}
#[derive(Serialize)]
pub struct ShockCheck {
    pub density_l1: f64,
    pub pressure_l1: f64,
    pub cells: usize,
    pub passed: bool,
}
/// Independent Sod exact Riemann solution at t=.2, using known star pressure
/// and velocity from the shock-tube root, verifies shocks/contact/rarefaction.
pub fn sod_check(n: usize) -> ShockCheck {
    let dx = 1.0 / n as f64;
    let mut cells: Vec<_> = (0..n)
        .map(|i| {
            if i < n / 2 {
                conserved(1.0, 0.0, 0.0, 1.0)
            } else {
                conserved(0.125, 0.0, 0.0, 0.1)
            }
        })
        .collect();
    let mut t = 0.0;
    while t < 0.2 {
        let max = cells.iter().map(|c| wave(*c, 0)).fold(0.0, f64::max);
        let dt = (0.4 * dx / max).min(0.2 - t);
        let mut next = cells.clone();
        for i in 0..n {
            let left = rusanov(cells[i.saturating_sub(1)], cells[i], 0);
            let right = rusanov(cells[i], cells[(i + 1).min(n - 1)], 0);
            next[i] = std::array::from_fn(|k| cells[i][k] - dt / dx * (right[k] - left[k]));
        }
        cells = next;
        t += dt;
    }
    let (ps, us): (f64, f64) = (0.303130178050647, 0.92745262004895);
    let c0 = GAMMA.sqrt();
    let head = -c0;
    let tail = us - c0 * ps.powf((GAMMA - 1.0) / (2.0 * GAMMA));
    let shock = (GAMMA * 0.1 / 0.125
        * ((GAMMA + 1.0) / (2.0 * GAMMA) * (ps / 0.1) + (GAMMA - 1.0) / (2.0 * GAMMA)))
        .sqrt();
    let mut rho_error = 0.0;
    let mut pressure_error = 0.0;
    for (i, c) in cells.iter().enumerate() {
        let xi = ((i as f64 + 0.5) * dx - 0.5) / 0.2;
        let (rho, p) = if xi < head {
            (1.0, 1.0)
        } else if xi < tail {
            let a = 2.0 / (GAMMA + 1.0) + (GAMMA - 1.0) / (GAMMA + 1.0) * (-xi / c0);
            (
                a.powf(2.0 / (GAMMA - 1.0)),
                a.powf(2.0 * GAMMA / (GAMMA - 1.0)),
            )
        } else if xi < us {
            (ps.powf(1.0 / GAMMA), ps)
        } else if xi < shock {
            let ratio = ps / 0.1;
            let g = (GAMMA - 1.0) / (GAMMA + 1.0);
            (0.125 * (ratio + g) / (g * ratio + 1.0), ps)
        } else {
            (0.125, 0.1)
        };
        rho_error += (c[0] - rho).abs() * dx;
        pressure_error += (pressure(*c) - p).abs() * dx;
    }
    ShockCheck {
        density_l1: rho_error,
        pressure_l1: pressure_error,
        cells: n,
        passed: rho_error < 0.025 && pressure_error < 0.02,
    }
}
