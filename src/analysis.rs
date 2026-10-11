//! Time-local post-processing of a recorded flight, with causal heating history.
use crate::flight::{FlightResult, Telemetry};

/// Interpolates continuous recorded quantities; stage changes are discontinuities.
pub fn sample_at(f: &FlightResult, time: f64) -> Telemetry {
    let time = if time.is_finite() {
        time.clamp(0.0, f.summary.end_time_s)
    } else {
        0.0
    };
    let i = f
        .samples
        .partition_point(|s| s.time_s <= time)
        .saturating_sub(1);
    let a = &f.samples[i];
    let b = &f.samples[(i + 1).min(f.samples.len() - 1)];
    let u = if a.stage == b.stage && b.time_s > a.time_s {
        ((time - a.time_s) / (b.time_s - a.time_s)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut s = a.clone();
    macro_rules! interpolate { ($($field:ident),*) => { $(s.$field = a.$field + (b.$field-a.$field)*u;)* }; }
    interpolate!(
        altitude_m,
        altitude_agl_m,
        east_m,
        north_m,
        speed_mps,
        vertical_mps,
        mach,
        q_pa,
        alpha_deg,
        mass_kg,
        cg_m,
        cp_m,
        stability_calibers,
        cd,
        thrust_n,
        specific_force_g,
        roll_rate_deg_s,
        pitch_rate_deg_s,
        yaw_rate_deg_s,
        temperature_k,
        heat_flux_w_m2,
        knudsen,
        reynolds,
        chute_load_n,
        drogue_fraction,
        main_fraction,
        sensed_altitude_agl_m,
        battery_wh,
        vx,
        vy,
        vz
    );
    let q = glam::DQuat::from_xyzw(a.qx, a.qy, a.qz, a.qw)
        .slerp(glam::DQuat::from_xyzw(b.qx, b.qy, b.qz, b.qw), u);
    (s.qx, s.qy, s.qz, s.qw) = (q.x, q.y, q.z, q.w);
    s.time_s = time;
    s.regime = crate::flight::Regime::from_mach(s.mach);
    s
}

/// Keep every recorded knot and event, subdivide boundary intervals, end exactly
/// at the requested time. Refinement does not invent new flight dynamics.
pub fn prefix(f: &FlightResult, time: f64, boundary_step: f64) -> FlightResult {
    let end = sample_at(f, time).time_s;
    let mut knots: Vec<_> = f
        .samples
        .iter()
        .map(|s| s.time_s)
        .chain(f.events.iter().map(|e| e.time_s))
        .chain([0.0, end])
        .filter(|t| *t <= end)
        .collect();
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    let step = if boundary_step.is_finite() {
        boundary_step.clamp(0.01, 1.0)
    } else {
        0.1
    };
    let mut samples = vec![sample_at(f, 0.0)];
    for pair in knots.windows(2) {
        let count = ((pair[1] - pair[0]) / step).ceil().max(1.0) as usize;
        for j in 1..=count {
            samples.push(sample_at(
                f,
                pair[0] + (pair[1] - pair[0]) * j as f64 / count as f64,
            ));
        }
    }
    let mut out = f.clone();
    out.samples = samples;
    out.events.retain(|e| e.time_s <= end);
    out.debris_samples.retain(|e| e.time_s <= end);
    out.recovery_samples.retain(|e| e.time_s <= end);
    out.summary.end_time_s = end;
    out
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StressPoint {
    pub stage: usize,
    pub span_fraction: f64,
    pub stress_pa: f64,
    pub temperature_k: f64,
    pub deflection_m: Option<f64>,
    pub margin: Option<f64>,
}

/// Midpoint integration of the tapered fin's transverse load and beam curvature.
/// Retained fins only; deflection/strength need supplied material laws.
pub fn fin_stress(
    f: &FlightResult,
    h: &crate::thermal::History,
    divisions: usize,
) -> Vec<StressPoint> {
    let sample = f.samples.last().expect("recorded flight");
    let n = divisions.clamp(8, 512);
    let pressure = sample.q_pa * 1.2 * sample.alpha_deg.to_radians().sin().abs();
    let mut rows = Vec::new();
    for (stage, s) in f.mission.stages.iter().enumerate().skip(sample.stage) {
        let Some(g) = &s.geometry else {
            continue;
        };
        let column = h
            .columns
            .iter()
            .find(|c| c.stage == stage && c.kind == "fin");
        let chord = |x: f64| {
            g.fin_root_chord_m + (g.fin_tip_chord_m - g.fin_root_chord_m) * x / g.fin_span_m
        };
        let dx = g.fin_span_m / n as f64;
        let moment = |x: f64| {
            let width = (g.fin_span_m - x) / n as f64;
            (0..n)
                .map(|j| {
                    let t = x + (j as f64 + 0.5) * width;
                    pressure * chord(t) * (t - x) * width
                })
                .sum::<f64>()
        };
        let properties = |x: f64| {
            let temp = h.fin_temperature(sample.time_s, stage, x / g.fin_span_m, false);
            let (e, strength, _) = column.map_or((None, None, 0.3), |c| {
                crate::vehicle_stress::properties(f, c, temp)
            });
            (temp, e, strength)
        };
        let inertia = |x: f64| chord(x) * g.fin_thickness_m.powi(3) / 12.0;
        let curvature: Vec<_> = (0..n)
            .map(|j| {
                let x = (j as f64 + 0.5) * dx;
                properties(x).1.map_or(0., |e| moment(x) / (e * inertia(x)))
            })
            .collect();
        for j in 0..=n {
            let x = j as f64 * dx;
            let (temperature_k, youngs, strength) = properties(x);
            let stress_pa = moment(x) * g.fin_thickness_m * 0.5 / inertia(x).max(1e-20);
            rows.push(StressPoint {
                stage,
                span_fraction: j as f64 / n as f64,
                stress_pa,
                temperature_k,
                deflection_m: youngs.map(|_| {
                    curvature
                        .iter()
                        .take(j)
                        .enumerate()
                        .map(|(k, c)| (x - (k as f64 + 0.5) * dx) * c * dx)
                        .sum()
                }),
                margin: strength.and_then(|limit| (stress_pa > 1e-9).then_some(limit / stress_pa)),
            });
        }
    }
    rows
}
