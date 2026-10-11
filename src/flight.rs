//! Body +Z points toward the nose; world is a rotating launch-tangent ENU frame.
//! CG/CP stations are distances aft of the active stack nose. All computation is f64.
use crate::{
    atmosphere::{self, Air},
    config::*,
};
use glam::{DMat3, DQuat, DVec3, Vec3Swizzles};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Debug)]
pub struct State {
    pub position: DVec3,
    pub velocity: DVec3,
    pub attitude: DQuat,
    pub omega: DVec3,
    pub temperature_k: f64,
}
impl State {
    fn packed(&self) -> [f64; 14] {
        let q = self.attitude.to_array();
        [
            self.position.x,
            self.position.y,
            self.position.z,
            self.velocity.x,
            self.velocity.y,
            self.velocity.z,
            q[0],
            q[1],
            q[2],
            q[3],
            self.omega.x,
            self.omega.y,
            self.omega.z,
            self.temperature_k,
        ]
    }
    fn unpack(y: [f64; 14]) -> Self {
        Self {
            position: DVec3::new(y[0], y[1], y[2]),
            velocity: DVec3::new(y[3], y[4], y[5]),
            attitude: DQuat::from_xyzw(y[6], y[7], y[8], y[9]).normalize(),
            omega: DVec3::new(y[10], y[11], y[12]),
            temperature_k: y[13],
        }
    }
    pub fn is_finite(&self) -> bool {
        self.packed().iter().all(|v| v.is_finite())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Mass {
    pub kg: f64,
    pub cg_m: f64,
    pub inertia: DVec3,
    pub tensor: DMat3,
    pub cg_body_m: DVec3,
}
pub fn mass_properties(m: &Mission, active: usize, t: f64, ignition: f64) -> Mass {
    let mut pieces = Vec::new();
    let mut models = Vec::new();
    let mut offset = 0.0;
    for i in (active..m.stages.len()).rev() {
        let s = &m.stages[i];
        let fuel = s
            .motor
            .remaining(if i == active { t - ignition } else { 0.0 });
        if let Some(model) = &s.model_mass {
            let center = DVec3::from_array(model.centroid_m) - DVec3::Z * offset;
            models.push((
                model.mass_kg,
                center,
                DMat3::from_cols_array_2d(&model.inertia_kg_m2),
            ));
        } else {
            for (kg, cg, length, r) in crate::assembly::dry_pieces(s) {
                pieces.push((kg, cg + offset, length, r));
            }
        }
        for (kg, cg, length) in [
            (s.payload_mass_kg, s.payload_cg_m, s.length_m * 0.1),
            (fuel, s.motor.cg_m, s.length_m * 0.5),
        ] {
            pieces.push((kg, cg + offset, length, s.diameter_m * 0.5));
        }
        offset += s.length_m;
    }
    let kg: f64 = pieces.iter().map(|p| p.0).sum::<f64>() + models.iter().map(|p| p.0).sum::<f64>();
    let cg_body = (models.iter().map(|p| p.1 * p.0).sum::<DVec3>()
        - DVec3::Z * pieces.iter().map(|p| p.0 * p.1).sum::<f64>())
        / kg;
    let mut tensor = DMat3::ZERO;
    for (mass, x, length, r) in pieces {
        let transverse = mass * (3.0 * r * r + length * length) / 12.0;
        tensor += DMat3::from_diagonal(DVec3::new(transverse, transverse, 0.5 * mass * r * r))
            + crate::model_physics::parallel_axis(mass, -DVec3::Z * x - cg_body);
    }
    for (mass, center, inertia) in models {
        tensor += inertia + crate::model_physics::parallel_axis(mass, center - cg_body);
    }
    Mass {
        kg,
        cg_m: -cg_body.z,
        inertia: DVec3::new(tensor.x_axis.x, tensor.y_axis.y, tensor.z_axis.z),
        tensor,
        cg_body_m: cg_body,
    }
}
pub fn coefficients(stage: &Stage, mach: f64) -> AeroPoint {
    let a = &stage.aero;
    let mix = |lo: &AeroPoint, hi: &AeroPoint, f: f64| AeroPoint {
        mach,
        cd_power_off: lo.cd_power_off + (hi.cd_power_off - lo.cd_power_off) * f,
        cd_power_on: lo.cd_power_on + (hi.cd_power_on - lo.cd_power_on) * f,
        cn_alpha_per_rad: lo.cn_alpha_per_rad + (hi.cn_alpha_per_rad - lo.cn_alpha_per_rad) * f,
        cp_m: lo.cp_m + (hi.cp_m - lo.cp_m) * f,
    };
    for w in a.windows(2) {
        if mach <= w[1].mach {
            return mix(
                &w[0],
                &w[1],
                ((mach - w[0].mach) / (w[1].mach - w[0].mach)).clamp(0.0, 1.0),
            );
        }
    }
    mix(a.last().unwrap(), a.last().unwrap(), 0.0)
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum Regime {
    Subsonic,
    Transonic,
    Supersonic,
    Hypersonic,
}
impl Regime {
    pub fn from_mach(m: f64) -> Self {
        if m < 0.8 {
            Self::Subsonic
        } else if m < 1.2 {
            Self::Transonic
        } else if m < 5.0 {
            Self::Supersonic
        } else {
            Self::Hypersonic
        }
    }
}
#[derive(Clone, Debug)]
pub struct Diagnostics {
    pub altitude_m: f64,
    pub agl_m: f64,
    pub air: Air,
    pub mach: f64,
    pub q_pa: f64,
    pub alpha_rad: f64,
    pub cd: f64,
    pub cp_m: f64,
    pub mass: Mass,
    pub thrust_n: f64,
    pub force_world: DVec3,
    pub torque_body: DVec3,
    pub heat_flux_w_m2: f64,
    pub knudsen: f64,
    pub reynolds: f64,
    pub chute_load_n: f64,
}

fn forces(
    m: &Mission,
    active: usize,
    ignition: f64,
    t: f64,
    state: &State,
    chute_cda: f64,
) -> Diagnostics {
    let s = &m.stages[active];
    let mass = mass_properties(m, active, t, ignition);
    let altitude = atmosphere::altitude(state.position, m.launch.altitude_m);
    let agl = altitude - m.launch.altitude_m;
    let air = atmosphere::for_mission(altitude, m);
    let wind = crate::environment::wind(m, altitude, t);
    let relative = state.velocity - wind;
    let speed = relative.length();
    let mach = speed / air.sound_speed;
    let q = 0.5 * air.density * speed * speed;
    let rot = state.attitude.normalize();
    let vb = rot.conjugate() * relative;
    let alpha = vb.xy().length().atan2(vb.z);
    let aero = coefficients(s, mach);
    let thrust = s.motor.thrust(t - ignition);
    let cd = if thrust > 0.0 {
        aero.cd_power_on
    } else {
        aero.cd_power_off
    };
    let area = std::f64::consts::PI * s.diameter_m.powi(2) / 4.0;
    let length = m.stages[active..].iter().map(|s| s.length_m).sum::<f64>();
    // Keep the supplied small-angle data on ascent. At large incidence use
    // crossflow drag on the side projection, acting at the projected centroid.
    // Never reverse transverse drag tail-first: this permits descent instability
    // and tumbling instead of artificially stabilizing a backwards rocket.
    let blend =
        ((alpha.min(std::f64::consts::PI - alpha).to_degrees() - 15.0) / 30.0).clamp(0.0, 1.0);
    let transverse = -DVec3::new(vb.x, vb.y, 0.0).normalize_or_zero();
    let normal_small = transverse
        * q
        * area
        * aero.cn_alpha_per_rad
        * alpha.sin()
        * alpha.cos().abs()
        * (1.0 - blend);
    let mut side_area = 0.0;
    let mut side_moment = 0.0;
    let mut offset = 0.0;
    for stage in m.stages[active..].iter().rev() {
        if let Some(model) = &stage.model_mass {
            let direction = vb.xy().normalize_or_zero().abs();
            let weights = [direction.x.powi(2), direction.y.powi(2)];
            let a = weights[0] * model.side_area_m2[0] + weights[1] * model.side_area_m2[1];
            let moment = weights[0] * model.side_area_m2[0] * model.side_station_m[0]
                + weights[1] * model.side_area_m2[1] * model.side_station_m[1];
            side_area += a;
            side_moment += moment + a * offset;
            offset += stage.length_m;
            continue;
        }
        let body_area = stage.length_m * stage.diameter_m;
        side_area += body_area;
        side_moment += body_area * (offset + stage.length_m * 0.5);
        let component_shell = stage.materials.as_ref().is_some_and(|m| m.component_shell);
        let mut sets = Vec::new();
        if !component_shell && let Some(g) = &stage.geometry {
            sets.push(g.clone());
        }
        for p in stage
            .components
            .iter()
            .filter(|p| p.kind == crate::assembly::PartKind::FinSet)
        {
            if let Some(g) = &p.shape {
                let mut g = g.clone();
                g.fin_position_m = p.position_m;
                sets.push(g);
            }
        }
        for g in &sets {
            let fin_area = (g.fin_root_chord_m + g.fin_tip_chord_m)
                * 0.5
                * g.fin_span_m
                * g.fin_count as f64
                * 0.5;
            side_area += fin_area;
            side_moment += fin_area * (offset + g.fin_position_m + g.fin_root_chord_m * 0.5);
        }
        offset += stage.length_m;
    }
    let normal_cross = transverse
        * q
        * 1.2
        * side_area
        * alpha.sin().powi(2)
        * blend
        * if aero.cn_alpha_per_rad > 0.0 {
            1.0
        } else {
            0.0
        };
    let mut normal = normal_small + normal_cross;
    let mut drag = -relative.normalize_or_zero() * q * area * (cd + 0.8 * alpha.sin().powi(2));
    let mis = s.motor.misalignment_deg;
    let thrust_body =
        DVec3::new(mis[0].to_radians().tan(), mis[1].to_radians().tan(), 1.0).normalize() * thrust;
    let radial_cg = DVec3::new(mass.cg_body_m.x, mass.cg_body_m.y, 0.0);
    let lever = DVec3::Z * (mass.cg_m - aero.cp_m);
    let thrust_lever = DVec3::new(
        s.motor.thrust_offset_m[0],
        s.motor.thrust_offset_m[1],
        mass.cg_m - m.stages[active..].iter().map(|s| s.length_m).sum::<f64>(),
    ) - radial_cg;
    let rate_scale = q * area * length * length / (2.0 * speed.max(1.0));
    let damping = -DVec3::new(
        state.omega.x * s.pitch_damping,
        state.omega.y * s.pitch_damping,
        state.omega.z * s.roll_damping,
    ) * rate_scale;
    let roll = DVec3::Z * q * area * s.diameter_m * s.fin_cant_deg.to_radians() * 0.5;
    let mut aero_torque = lever.cross(normal_small)
        + (DVec3::Z * (mass.cg_m - side_moment / side_area.max(1e-12))).cross(normal_cross);
    let mut reported_cd = cd;
    let mut reported_cp = aero.cp_m;
    let re = air.density * speed * length / air.viscosity;
    if let Some(grid) = m.experiments.aero_grids.iter().find(|g| g.stage == active) {
        let c = grid.sample(mach, alpha.to_degrees(), re);
        let ca = if thrust > 0.0 { c.ca_on } else { c.ca_off };
        normal = transverse * q * area * c.cn;
        drag = rot * (-DVec3::Z * vb.z.signum() * q * area * ca);
        aero_torque =
            DVec3::Z.cross(transverse) * q * area * (mass.cg_m * c.cn + s.diameter_m * c.cm);
        reported_cd = ca;
        reported_cp = if c.cn.abs() > 1e-9 {
            -c.cm * s.diameter_m / c.cn
        } else {
            aero.cp_m
        };
    } else if m.experiments.rarefied_bridge {
        // Screening only: empirical logarithmic blend toward diffuse projected
        // free-molecule drag. Not a DSMC or accommodation/chemistry model.
        let w = ((air.mean_free_path / s.diameter_m / 0.01).log10() / 3.0).clamp(0.0, 1.0);
        let molecular_cd = 2.0 * (alpha.cos().abs() + side_area / area * alpha.sin().abs());
        drag = -relative.normalize_or_zero()
            * q
            * area
            * ((1.0 - w) * (cd + 0.8 * alpha.sin().powi(2)) + w * molecular_cd);
        normal *= 1.0 - w;
        aero_torque *= 1.0 - w;
        reported_cd = (1.0 - w) * cd + w * molecular_cd;
    }
    aero_torque += (-radial_cg).cross(normal + rot.conjugate() * drag);
    let chute_load = q * chute_cda;
    let chute_force = -relative.normalize_or_zero() * chute_load;
    let force = rot * (normal + thrust_body) + drag + chute_force;
    // Effective suspension attachment near the nose. The canopy force acts
    // through a lever arm and can align/damp the body once it inflates.
    let attachment = m.experiments.recovery(active, false).attachment_fraction;
    let chute_torque = (DVec3::Z * (mass.cg_m - length * attachment) - radial_cg)
        .cross(rot.conjugate() * chute_force);
    // Suttonâ€“Graves screening estimate, Earth-air SI coefficient; continuum only.
    let heat = 1.83e-4 * (air.density / s.nose_radius_m).sqrt() * speed.powi(3);
    Diagnostics {
        altitude_m: altitude,
        agl_m: agl,
        air,
        mach,
        q_pa: q,
        alpha_rad: alpha,
        cd: reported_cd,
        cp_m: reported_cp,
        mass,
        thrust_n: thrust,
        force_world: force,
        torque_body: aero_torque + thrust_lever.cross(thrust_body) + damping + roll + chute_torque,
        heat_flux_w_m2: heat,
        knudsen: air.mean_free_path / s.diameter_m,
        reynolds: air.density * speed * length / air.viscosity,
        chute_load_n: chute_load,
    }
}
fn derivative(
    m: &Mission,
    active: usize,
    ignition: f64,
    t: f64,
    y: [f64; 14],
    chute_cda: f64,
    rail: Option<DVec3>,
) -> [f64; 14] {
    let state = State::unpack(y);
    let d = forces(m, active, ignition, t, &state, chute_cda);
    let s = &m.stages[active];
    let earth_rate = 7.292115e-5;
    let latitude = m.launch.latitude_deg.to_radians();
    let earth_omega = DVec3::new(
        0.0,
        earth_rate * latitude.cos(),
        earth_rate * latitude.sin(),
    );
    let mut accel =
        d.force_world / d.mass.kg + atmosphere::gravity(state.position, m.launch.altitude_m);
    if m.launch.earth_rotation {
        let r = atmosphere::radial(state.position, m.launch.altitude_m);
        accel -= 2.0 * earth_omega.cross(state.velocity) + earth_omega.cross(earth_omega.cross(r));
    }
    let inertia = d.mass.tensor;
    // Angular momentum carried out by burned propellant is assumed to cancel I-dot*omega.
    // No arbitrary I-dot spin-up is applied to a motor burning along the body axis.
    let mut angular =
        inertia.inverse() * (d.torque_body - state.omega.cross(inertia * state.omega));
    let omega = if rail.is_some() {
        DVec3::ZERO
    } else {
        state.omega
    };
    let dq = state.attitude * DQuat::from_xyzw(omega.x, omega.y, omega.z, 0.0) * 0.5;
    let mut velocity = state.velocity;
    if let Some(rail_axis) = rail {
        accel = rail_axis * accel.dot(rail_axis);
        if state.position.dot(rail_axis) <= 0.0
            && velocity.dot(rail_axis) <= 0.0
            && accel.dot(rail_axis) < 0.0
        {
            accel = DVec3::ZERO;
            velocity = DVec3::ZERO;
        }
        angular = DVec3::ZERO;
    }
    let radiation = s.emissivity
        * 5.670374419e-8
        * (state.temperature_k.max(1.0).powi(4) - d.air.temperature.powi(4));
    let heating = (d.heat_flux_w_m2 - radiation) * s.heated_area_m2 / s.thermal_capacity_j_per_k;
    [
        velocity.x, velocity.y, velocity.z, accel.x, accel.y, accel.z, dq.x, dq.y, dq.z, dq.w,
        angular.x, angular.y, angular.z, heating,
    ]
}
fn rk4(state: &State, dt: f64, mut f: impl FnMut(f64, [f64; 14]) -> [f64; 14]) -> State {
    let y = state.packed();
    let add = |k: [f64; 14], scale: f64| std::array::from_fn(|i| y[i] + scale * k[i]);
    let k1 = f(0.0, y);
    let k2 = f(dt * 0.5, add(k1, dt * 0.5));
    let k3 = f(dt * 0.5, add(k2, dt * 0.5));
    let k4 = f(dt, add(k3, dt));
    State::unpack(std::array::from_fn(|i| {
        y[i] + dt * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]) / 6.0
    }))
}

fn damping_step_limit(m: &Mission, active: usize, d: &Diagnostics) -> f64 {
    let length = m.stages[active..].iter().map(|s| s.length_m).sum::<f64>();
    let s = &m.stages[active];
    let rate = d.q_pa * std::f64::consts::PI * s.diameter_m.powi(2) / 4.0 * length * length
        / (2.0 * (d.mach * d.air.sound_speed).max(1.0));
    let matrix = d.mass.tensor.inverse()
        * DMat3::from_diagonal(DVec3::new(s.pitch_damping, s.pitch_damping, s.roll_damping) * rate);
    let decay = matrix
        .transpose()
        .to_cols_array_2d()
        .iter()
        .map(|row| row.iter().map(|v| v.abs()).sum::<f64>())
        .fold(0.0, f64::max);
    if decay > 0.0 {
        0.4 / decay
    } else {
        f64::INFINITY
    }
}

fn suspension_chute<'a>(drogue: &'a ChuteState, main: &'a ChuteState) -> Option<&'a ChuteState> {
    let c = if main.deployed.is_some() && !main.failed {
        main
    } else {
        drogue
    };
    (c.deployed.is_some() && !c.failed && c.model.canopy_mass_kg > 0.0).then_some(c)
}
fn suspension_init(
    m: &Mission,
    state: &State,
    chute: Option<&ChuteState>,
    canopy: &mut Option<crate::recovery::CanopyState>,
) {
    let Some(c) = chute else {
        *canopy = None;
        return;
    };
    if canopy.as_ref().is_none_or(|p| p.main != c.model.main) {
        let altitude = atmosphere::altitude(state.position, m.launch.altitude_m);
        let direction =
            -(state.velocity - crate::environment::wind(m, altitude, 0.0)).normalize_or_zero();
        let direction = if direction.length_squared() > 0.0 {
            direction
        } else {
            state.attitude * DVec3::Z
        };
        *canopy = Some(crate::recovery::CanopyState {
            main: c.model.main,
            position: state.position + direction * c.model.tether_length_m * 0.95,
            velocity: state.velocity,
        });
    }
}
fn suspension_tension(
    m: &Mission,
    active: usize,
    state: &State,
    canopy: &crate::recovery::CanopyState,
    model: &crate::experiments::RecoveryModel,
    mass: &Mass,
) -> (DVec3, DVec3) {
    let length = m.stages[active..].iter().map(|s| s.length_m).sum::<f64>();
    let lever = -DVec3::Z * (length * model.attachment_fraction) - mass.cg_body_m;
    let offset = state.attitude * lever;
    let anchor = state.position + offset;
    let velocity = state.velocity + state.attitude * state.omega.cross(lever);
    (
        crate::recovery::tension(
            anchor,
            velocity,
            canopy,
            model.tether_length_m,
            model.tether_stiffness_n_m,
            model.tether_damping_ns_m,
        ),
        lever,
    )
}
#[allow(clippy::too_many_arguments)]
fn suspension_step(
    m: &Mission,
    active: usize,
    ignition: f64,
    t: f64,
    dt: f64,
    state: &State,
    canopy: &crate::recovery::CanopyState,
    chute: &ChuteState,
) -> (State, crate::recovery::CanopyState) {
    let cfg = if chute.model.main {
        &m.recovery.main
    } else {
        &m.recovery.drogue
    };
    let mut y = [0.0; 20];
    y[..14].copy_from_slice(&state.packed());
    y[14..17].copy_from_slice(&canopy.position.to_array());
    y[17..20].copy_from_slice(&canopy.velocity.to_array());
    let f = |offset: f64, y: [f64; 20]| {
        let body = State::unpack(y[..14].try_into().unwrap());
        let point = crate::recovery::CanopyState {
            main: canopy.main,
            position: DVec3::new(y[14], y[15], y[16]),
            velocity: DVec3::new(y[17], y[18], y[19]),
        };
        let d = forces(m, active, ignition, t + offset, &body, 0.0);
        let mut dy = [0.0; 20];
        dy[..14].copy_from_slice(&derivative(
            m,
            active,
            ignition,
            t + offset,
            body.packed(),
            0.0,
            None,
        ));
        let (tension, lever) = suspension_tension(m, active, &body, &point, &chute.model, &d.mass);
        let canopy_mass = chute.model.canopy_mass_kg;
        let body_mass = (d.mass.kg - canopy_mass).max(1e-6);
        let correction = d.force_world * (1.0 / body_mass - 1.0 / d.mass.kg) + tension / body_mass;
        dy[3] += correction.x;
        dy[4] += correction.y;
        dy[5] += correction.z;
        let angular = d.mass.tensor.inverse() * lever.cross(body.attitude.conjugate() * tension);
        dy[10] += angular.x;
        dy[11] += angular.y;
        dy[12] += angular.z;
        let altitude = atmosphere::altitude(point.position, m.launch.altitude_m);
        let air = atmosphere::for_mission(altitude, m);
        let relative = point.velocity - crate::environment::wind(m, altitude, t + offset);
        let drag = -relative
            * relative.length()
            * 0.5
            * air.density
            * chute.effective_cda(t + offset, cfg);
        let mut acceleration = (drag - tension) / canopy_mass
            + atmosphere::gravity(point.position, m.launch.altitude_m);
        if m.launch.earth_rotation {
            let lat = m.launch.latitude_deg.to_radians();
            let omega = DVec3::new(0.0, 7.292115e-5 * lat.cos(), 7.292115e-5 * lat.sin());
            let r = atmosphere::radial(point.position, m.launch.altitude_m);
            acceleration -= 2.0 * omega.cross(point.velocity) + omega.cross(omega.cross(r));
        }
        dy[14..17].copy_from_slice(&point.velocity.to_array());
        dy[17..20].copy_from_slice(&acceleration.to_array());
        dy
    };
    let add = |k: [f64; 20], scale: f64| std::array::from_fn(|i| y[i] + k[i] * scale);
    let k1 = f(0.0, y);
    let k2 = f(dt * 0.5, add(k1, dt * 0.5));
    let k3 = f(dt * 0.5, add(k2, dt * 0.5));
    let k4 = f(dt, add(k3, dt));
    let y: [f64; 20] =
        std::array::from_fn(|i| y[i] + dt * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]) / 6.0);
    (
        State::unpack(y[..14].try_into().unwrap()),
        crate::recovery::CanopyState {
            main: canopy.main,
            position: DVec3::new(y[14], y[15], y[16]),
            velocity: DVec3::new(y[17], y[18], y[19]),
        },
    )
}
fn suspension_limit(
    m: &Mission,
    state: &State,
    chute: &ChuteState,
    t: f64,
    total_mass: f64,
) -> f64 {
    let mass = chute.model.canopy_mass_kg;
    let reduced = mass * (total_mass - mass) / total_mass;
    let altitude = atmosphere::altitude(state.position, m.launch.altitude_m);
    let air = atmosphere::for_mission(altitude, m);
    let speed = (state.velocity - crate::environment::wind(m, altitude, t)).length();
    let cfg = if chute.model.main {
        &m.recovery.main
    } else {
        &m.recovery.drogue
    };
    (0.2 * (reduced / chute.model.tether_stiffness_n_m).sqrt())
        .min(0.4 * reduced / chute.model.tether_damping_ns_m.max(1e-9))
        .min(0.4 * mass / (air.density * speed * chute.effective_cda(t, cfg)).max(1e-9))
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub time_s: f64,
    pub stage: usize,
    pub kind: String,
    pub detail: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Telemetry {
    pub time_s: f64,
    pub stage: usize,
    pub altitude_m: f64,
    pub altitude_agl_m: f64,
    pub east_m: f64,
    pub north_m: f64,
    pub speed_mps: f64,
    pub vertical_mps: f64,
    pub mach: f64,
    pub regime: Regime,
    pub q_pa: f64,
    pub alpha_deg: f64,
    pub mass_kg: f64,
    pub cg_m: f64,
    pub cp_m: f64,
    pub stability_calibers: f64,
    pub cd: f64,
    pub thrust_n: f64,
    pub specific_force_g: f64,
    pub roll_rate_deg_s: f64,
    pub pitch_rate_deg_s: f64,
    pub yaw_rate_deg_s: f64,
    pub temperature_k: f64,
    pub heat_flux_w_m2: f64,
    pub knudsen: f64,
    pub reynolds: f64,
    pub chute_load_n: f64,
    pub drogue_fraction: f64,
    pub main_fraction: f64,
    pub sensed_altitude_agl_m: f64,
    pub battery_wh: f64,
    pub baro_valid: bool,
    pub gnss_valid: bool,
    pub accel_saturated: bool,
    pub gyro_saturated: bool,
    pub qw: f64,
    pub qx: f64,
    pub qy: f64,
    pub qz: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub name: String,
    pub apogee_agl_m: f64,
    pub max_mach: f64,
    pub max_q_pa: f64,
    pub max_specific_force_g: f64,
    pub max_temperature_k: f64,
    pub landing_speed_mps: Option<f64>,
    pub landing_east_m: Option<f64>,
    pub landing_north_m: Option<f64>,
    pub end_time_s: f64,
    pub completed: bool,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct FlightResult {
    pub mission: Mission,
    pub samples: Vec<Telemetry>,
    pub events: Vec<Event>,
    pub summary: Summary,
    pub debris_samples: Vec<DebrisSample>,
    pub recovery_samples: Vec<crate::recovery::Sample>,
}
#[derive(Clone, Debug, Serialize)]
pub struct DebrisSample {
    pub time_s: f64,
    pub stage: usize,
    pub east_m: f64,
    pub north_m: f64,
    pub altitude_agl_m: f64,
    pub mass_kg: f64,
    pub cg_m: f64,
    pub qw: f64,
    pub qx: f64,
    pub qy: f64,
    pub qz: f64,
    pub landed: bool,
    pub drogue_fraction: f64,
    pub main_fraction: f64,
    pub chute_load_n: f64,
    pub mach: f64,
    pub q_pa: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub landing_speed_mps: f64,
}
#[derive(Clone, Debug)]
pub struct Debris {
    pub stage: usize,
    pub state: State,
    pub landed: bool,
    mission: Mission,
    drogue: ChuteState,
    main: ChuteState,
    apogee: bool,
    inhibited: [bool; 2],
    landing_speed_mps: f64,
    suspension: Option<crate::recovery::CanopyState>,
}
#[derive(Clone, Copy, Debug)]
struct Sensor {
    altitude: f64,
    baro_valid: bool,
    gnss_valid: bool,
    accel_saturated: bool,
    gyro_saturated: bool,
}
#[derive(Clone, Debug, Default)]
struct ChuteState {
    deployed: Option<f64>,
    failed: bool,
    model: crate::experiments::RecoveryModel,
}
impl Debris {
    fn cda(&self, t: f64) -> f64 {
        self.drogue.effective_cda(t, &self.mission.recovery.drogue)
            + self.main.effective_cda(t, &self.mission.recovery.main)
    }
    fn diagnostics(&self, t: f64) -> Diagnostics {
        let mut d = forces(
            &self.mission,
            0,
            f64::NEG_INFINITY,
            t,
            &self.state,
            if self.suspension.is_some() {
                0.0
            } else {
                self.cda(t)
            },
        );
        if let (Some(point), Some(c)) =
            (&self.suspension, suspension_chute(&self.drogue, &self.main))
        {
            let (force, lever) =
                suspension_tension(&self.mission, 0, &self.state, point, &c.model, &d.mass);
            d.force_world += force;
            d.torque_body += lever.cross(self.state.attitude.conjugate() * force);
            d.chute_load_n = force.length();
        }
        d
    }
    fn recovery_events(&mut self, t: f64) -> Vec<Event> {
        let mut events = Vec::new();
        if self.landed {
            return events;
        }
        let d = self.diagnostics(t);
        let descending = self.state.velocity.dot(
            atmosphere::radial(self.state.position, self.mission.launch.altitude_m).normalize(),
        ) < 0.0;
        if descending && !self.apogee {
            self.apogee = true;
        }
        for main in [false, true] {
            let main_deployed = self.main.deployed.is_some();
            let cfg = if main {
                &self.mission.recovery.main
            } else {
                &self.mission.recovery.drogue
            };
            let chute = if main {
                &mut self.main
            } else {
                &mut self.drogue
            };
            let name = if main { "main" } else { "drogue" };
            let label = |kind: &str, detail: String| Event {
                time_s: t,
                stage: self.stage,
                kind: kind.into(),
                detail,
            };
            let load = if self.suspension.as_ref().is_some_and(|p| p.main == main) {
                d.chute_load_n
            } else {
                d.q_pa * cfg.area_m2 * cfg.cd * chute.fraction(t, cfg) * chute.opening(t, cfg)
            };
            if load > cfg.max_load_n && !chute.failed {
                chute.failed = true;
                events.push(label(
                    "chute_failure",
                    format!("Detached S{} {name}: line load {load:.0} N", self.stage + 1),
                ));
            }
            let powered = self.mission.avionics.battery_wh * 3600.0
                > t * self.mission.avionics.power_w
                && !self
                    .mission
                    .experiments
                    .fault(crate::experiments::FaultKind::Power, t);
            let fault = if main {
                crate::experiments::FaultKind::MainPyro
            } else {
                crate::experiments::FaultKind::DroguePyro
            };
            let requested = cfg.enabled
                && !self.mission.experiments.fault(fault, t)
                && powered
                && descending
                && self.apogee
                && (!main
                    || crate::environment::clearance(&self.mission, self.state.position)
                        <= self.mission.recovery.main_altitude_agl_m);
            if requested && !chute.failed && chute.deployed.is_none() && (main || !main_deployed) {
                if d.q_pa <= cfg.max_q_pa && d.mach <= cfg.max_mach {
                    chute.deployed = Some(t);
                    events.push(label(
                        "deployment",
                        format!(
                            "Detached S{} {name} parachute (ideal recovery controller)",
                            self.stage + 1
                        ),
                    ));
                } else if !self.inhibited[main as usize] {
                    self.inhibited[main as usize] = true;
                    events.push(label(
                        "deployment_deferred",
                        format!("Detached S{} {name}: Mach/q interlock", self.stage + 1),
                    ));
                }
            }
        }
        if self.main.deployed.is_some() {
            self.drogue.failed = true;
        }
        events
    }
}
impl ChuteState {
    fn effective_cda(&self, t: f64, c: &Chute) -> f64 {
        self.fraction(t, c) * c.cd * c.area_m2 * self.opening(t, c)
    }
    fn opening(&self, t: f64, c: &Chute) -> f64 {
        self.deployed.map_or(1.0, |start| {
            if t - start < 2.0 * c.inflation_s + self.model.reef_hold_s {
                self.model.opening_factor
            } else {
                1.0
            }
        })
    }

    fn fraction(&self, t: f64, c: &Chute) -> f64 {
        if self.failed {
            0.0
        } else {
            self.deployed
                .map_or(0.0, |start| self.model.inflation(t - start, c.inflation_s))
        }
    }
}
pub struct Simulator {
    pub mission: Mission,
    pub state: State,
    pub time: f64,
    pub active: usize,
    pub debris: Vec<Debris>,
    pub events: Vec<Event>,
    pub samples: Vec<Telemetry>,
    pub debris_samples: Vec<DebrisSample>,
    pub recovery_samples: Vec<crate::recovery::Sample>,
    pub finished: bool,
    ignition: f64,
    ignited: bool,
    rail_axis: DVec3,
    on_rail: bool,
    airborne: bool,
    apogee: bool,
    burnout_reported: bool,
    drogue: ChuteState,
    main: ChuteState,
    sensor: Sensor,
    queue: VecDeque<(f64, Sensor)>,
    next_sensor: f64,
    next_sample: f64,
    last_altitude: Option<f64>,
    descent_count: usize,
    suspension: Option<crate::recovery::CanopyState>,
    warning_keys: HashSet<String>,
    fired_commands: HashSet<usize>,
    regime: Regime,
    summary: Summary,
}
impl Simulator {
    pub fn new(m: Mission) -> Result<Self> {
        m.validate()?;
        let el = m.launch.elevation_deg.to_radians();
        let az = m.launch.azimuth_deg.to_radians();
        let axis = DVec3::new(el.cos() * az.sin(), el.cos() * az.cos(), el.sin());
        let state = State {
            position: DVec3::ZERO,
            velocity: DVec3::ZERO,
            attitude: DQuat::from_rotation_arc(DVec3::Z, axis),
            omega: DVec3::ZERO,
            temperature_k: 288.15,
        };
        let ignition = m.stages[0].ignition_delay_s;
        let summary = Summary {
            name: m.name.clone(),
            apogee_agl_m: 0.0,
            max_mach: 0.0,
            max_q_pa: 0.0,
            max_specific_force_g: 0.0,
            max_temperature_k: 288.15,
            landing_speed_mps: None,
            landing_east_m: None,
            landing_north_m: None,
            end_time_s: 0.0,
            completed: false,
            warnings: Vec::new(),
        };
        let drogue_model = m.experiments.recovery(0, false);
        let main_model = m.experiments.recovery(0, true);
        let mut sim = Self {
            mission: m,
            state,
            time: 0.0,
            active: 0,
            debris: Vec::new(),
            events: Vec::new(),
            samples: Vec::new(),
            debris_samples: Vec::new(),
            recovery_samples: Vec::new(),
            finished: false,
            ignition,
            ignited: false,
            rail_axis: axis,
            on_rail: true,
            airborne: false,
            apogee: false,
            burnout_reported: false,
            drogue: ChuteState {
                deployed: None,
                failed: false,
                model: drogue_model,
            },
            main: ChuteState {
                deployed: None,
                failed: false,
                model: main_model,
            },
            sensor: Sensor {
                altitude: 0.0,
                baro_valid: true,
                gnss_valid: true,
                accel_saturated: false,
                gyro_saturated: false,
            },
            queue: VecDeque::new(),
            next_sensor: 0.0,
            next_sample: 0.0,
            last_altitude: None,
            descent_count: 0,
            suspension: None,
            warning_keys: HashSet::new(),
            fired_commands: HashSet::new(),
            regime: Regime::Subsonic,
            summary,
        };
        sim.event("armed", "Launch sequence initialized");
        sim.record();
        Ok(sim)
    }
    fn event(&mut self, kind: &str, detail: impl Into<String>) {
        self.events.push(Event {
            time_s: self.time,
            stage: self.active,
            kind: kind.into(),
            detail: detail.into(),
        });
    }
    fn warn(&mut self, key: &str, detail: &str) {
        let key = format!("{}:{key}", self.active);
        if self.warning_keys.insert(key) {
            if !self.summary.warnings.iter().any(|w| w == detail) {
                self.summary.warnings.push(detail.into());
            }
            self.event("warning", detail);
        }
    }
    fn battery(&self) -> f64 {
        if self
            .mission
            .experiments
            .fault(crate::experiments::FaultKind::Power, self.time)
        {
            return 0.0;
        }
        (self.mission.avionics.battery_wh - self.mission.avionics.power_w * self.time / 3600.0)
            .max(0.0)
    }
    fn cda(&self) -> f64 {
        self.drogue
            .effective_cda(self.time, &self.mission.recovery.drogue)
            + self
                .main
                .effective_cda(self.time, &self.mission.recovery.main)
    }
    pub fn diagnostics(&self) -> Diagnostics {
        let mut d = forces(
            &self.mission,
            self.active,
            if self.ignited {
                self.ignition
            } else {
                f64::INFINITY
            },
            self.time,
            &self.state,
            if self.suspension.is_some() {
                0.0
            } else {
                self.cda()
            },
        );
        if let (Some(point), Some(c)) =
            (&self.suspension, suspension_chute(&self.drogue, &self.main))
        {
            let (tension, lever) = suspension_tension(
                &self.mission,
                self.active,
                &self.state,
                point,
                &c.model,
                &d.mass,
            );
            d.force_world += tension;
            d.torque_body += lever.cross(self.state.attitude.conjugate() * tension);
            d.chute_load_n = tension.length();
        }
        d
    }
    pub fn advance(&mut self, seconds: f64) {
        let target = (self.time + seconds).min(self.mission.duration);
        while !self.finished && self.time + 1e-10 < target {
            self.step_with_dt(self.mission.dt.min(target - self.time));
        }
    }
    pub fn step(&mut self) {
        self.step_with_dt(self.mission.dt);
    }
    fn step_with_dt(&mut self, requested: f64) {
        if self.finished {
            return;
        }
        self.handle_events();
        for debris in &mut self.debris {
            self.events.extend(debris.recovery_events(self.time));
        }
        suspension_init(
            &self.mission,
            &self.state,
            suspension_chute(&self.drogue, &self.main),
            &mut self.suspension,
        );
        for debris in &mut self.debris {
            suspension_init(
                &debris.mission,
                &debris.state,
                suspension_chute(&debris.drogue, &debris.main),
                &mut debris.suspension,
            );
        }
        let mut dt = requested.min(self.mission.duration - self.time);
        // Resolve stiff damping and canopy drag; a user-selected step is an upper bound.
        let d = self.diagnostics();
        if let Some(c) = suspension_chute(&self.drogue, &self.main) {
            dt = dt.min(suspension_limit(
                &self.mission,
                &self.state,
                c,
                self.time,
                d.mass.kg,
            ));
        }
        if !self.on_rail {
            dt = dt.min(damping_step_limit(&self.mission, self.active, &d));
            let drag_rate = d.air.density * (d.mach * d.air.sound_speed) * self.cda() / d.mass.kg;
            if drag_rate > 0.0 {
                dt = dt.min(0.2 / drag_rate);
            }
        }
        for debris in &self.debris {
            if !debris.landed {
                let d = forces(
                    &debris.mission,
                    0,
                    f64::NEG_INFINITY,
                    self.time,
                    &debris.state,
                    debris.cda(self.time),
                );
                dt = dt.min(damping_step_limit(&debris.mission, 0, &d));
                if let Some(c) = suspension_chute(&debris.drogue, &debris.main) {
                    dt = dt.min(suspension_limit(
                        &debris.mission,
                        &debris.state,
                        c,
                        self.time,
                        d.mass.kg,
                    ));
                }
                let rate =
                    d.air.density * d.mach * d.air.sound_speed * debris.cda(self.time) / d.mass.kg;
                if rate > 0.0 {
                    dt = dt.min(0.2 / rate);
                }
            }
        }
        if dt < 1e-8 && self.mission.duration - self.time > 1e-8 {
            self.warn("stiffness","Required integration step fell below 1e-8 s; inspect mass, inertia and damping inputs");
            self.finished = true;
            return;
        }
        // Split deterministic motor/time discontinuities before RK4 evaluates forces.
        let s = &self.mission.stages[self.active];
        let mut boundaries = vec![self.ignition];
        if self.ignited {
            boundaries.extend(s.motor.points.iter().map(|p| self.ignition + p[0]));
        }
        if let Trigger::Burnout { delay_s } = s.separation {
            boundaries.push(self.ignition + s.motor.burn_time() + delay_s);
        }
        if let Trigger::Time { time_s } = s.separation {
            boundaries.push(time_s);
        }
        for b in boundaries {
            if b > self.time + 1e-9 {
                dt = dt.min(b - self.time);
            }
        }
        let t = self.time;
        let cda = self.cda();
        let ignition = if self.ignited {
            self.ignition
        } else {
            f64::INFINITY
        };
        if !self.summary.completed {
            if let (Some(point), Some(chute)) =
                (&self.suspension, suspension_chute(&self.drogue, &self.main))
            {
                let (state, point) = suspension_step(
                    &self.mission,
                    self.active,
                    ignition,
                    t,
                    dt,
                    &self.state,
                    point,
                    chute,
                );
                self.state = state;
                self.suspension = Some(point);
            } else {
                self.state = rk4(&self.state, dt, |offset, y| {
                    derivative(
                        &self.mission,
                        self.active,
                        ignition,
                        t + offset,
                        y,
                        cda,
                        self.on_rail.then_some(self.rail_axis),
                    )
                });
            }
        }
        self.time += dt;
        if self.on_rail {
            let distance = self.state.position.dot(self.rail_axis).max(0.0);
            self.state.position = self.rail_axis * distance;
            self.state.velocity = self.rail_axis * self.state.velocity.dot(self.rail_axis).max(0.0);
        }
        for debris in &mut self.debris {
            if debris.landed {
                continue;
            }
            let mission = &debris.mission;
            let cda = debris.cda(t);
            if let (Some(point), Some(chute)) = (
                &debris.suspension,
                suspension_chute(&debris.drogue, &debris.main),
            ) {
                let (state, point) = suspension_step(
                    mission,
                    0,
                    f64::NEG_INFINITY,
                    t,
                    dt,
                    &debris.state,
                    point,
                    chute,
                );
                debris.state = state;
                debris.suspension = Some(point);
            } else {
                debris.state = rk4(&debris.state, dt, |offset, y| {
                    derivative(mission, 0, f64::NEG_INFINITY, t + offset, y, cda, None)
                });
            }
            if atmosphere::altitude(debris.state.position, mission.launch.altitude_m)
                <= crate::environment::ground_msl(mission, debris.state.position)
            {
                debris.landed = true;
                debris.landing_speed_mps = debris.state.velocity.length();
                self.events.push(Event {
                    time_s: self.time,
                    stage: debris.stage,
                    kind: "detached_landing".into(),
                    detail: format!(
                        "Detached S{} touchdown {:.2} m/s",
                        debris.stage + 1,
                        debris.landing_speed_mps
                    ),
                });
                debris.state.position =
                    crate::environment::ground_position(mission, debris.state.position);
                debris.state.velocity = DVec3::ZERO;
                debris.state.omega = DVec3::ZERO;
            }
            self.events.extend(debris.recovery_events(self.time));
        }
        if !self.state.is_finite()
            || self
                .suspension
                .as_ref()
                .is_some_and(|p| !p.position.is_finite() || !p.velocity.is_finite())
            || self.debris.iter().any(|d| {
                !d.state.is_finite()
                    || d.suspension
                        .as_ref()
                        .is_some_and(|p| !p.position.is_finite() || !p.velocity.is_finite())
            })
        {
            self.warn(
                "numerics",
                "Numerical integration failed; reduce timestep or examine coefficients",
            );
            self.finished = true;
            return;
        }
        self.handle_events();
        if self.time + 1e-9 >= self.next_sample {
            self.record();
            self.next_sample = self.time + self.mission.sample_period;
        }
        if self.time >= self.mission.duration - 1e-9 && !self.finished {
            self.event("timeout", "Duration reached before landing");
            self.finished = true;
            self.record();
        }
    }
    fn handle_events(&mut self) {
        if self.summary.completed {
            self.finished = self.debris.iter().all(|d| d.landed);
            return;
        }
        let d = self.diagnostics();
        let s = self.mission.stages[self.active].clone();
        if !self.ignited && self.time + 1e-9 >= self.ignition {
            let tilt = (self.state.attitude * DVec3::Z)
                .dot(
                    atmosphere::radial(self.state.position, self.mission.launch.altitude_m)
                        .normalize(),
                )
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees();
            let allowed = self.active == 0
                || (self.battery() > 0.0
                    && (self.sensor.baro_valid || self.sensor.gnss_valid)
                    && self.sensor.altitude >= self.mission.avionics.ignition_min_altitude_m
                    && tilt <= self.mission.avionics.ignition_max_tilt_deg);
            if allowed {
                self.ignited = true;
                self.ignition = self.time;
                self.event("ignition", s.motor.name.clone());
            } else {
                self.warn(
                    "ignition_inhibited",
                    "Upper-stage ignition held by avionics altitude, tilt or power interlock",
                );
            }
        }
        if self.ignited
            && !self.burnout_reported
            && self.time + 1e-9 >= self.ignition + s.motor.burn_time()
        {
            self.burnout_reported = true;
            self.event("burnout", s.name.clone());
        }
        if self.on_rail
            && self.state.position.dot(self.rail_axis) >= self.mission.launch.rail_length_m
        {
            self.on_rail = false;
            self.event(
                "rail_exit",
                format!("{:.2} m/s", self.state.velocity.length()),
            );
        }
        if d.agl_m > 3.0 {
            self.airborne = true;
        }
        let vertical = self.state.velocity.dot(
            atmosphere::radial(self.state.position, self.mission.launch.altitude_m).normalize(),
        );
        if self.airborne && !self.apogee && vertical < 0.0 {
            self.apogee = true;
            self.event("apogee", format!("Truth altitude {:.1} m AGL", d.agl_m));
        }
        let next_regime = Regime::from_mach(d.mach);
        if next_regime != self.regime {
            self.regime = next_regime;
            self.event("regime", format!("{:?} at Mach {:.3}", next_regime, d.mach));
        }
        let specific = d.force_world.length() / d.mass.kg / 9.80665;
        self.summary.apogee_agl_m = self.summary.apogee_agl_m.max(d.agl_m);
        self.summary.max_mach = self.summary.max_mach.max(d.mach);
        self.summary.max_q_pa = self.summary.max_q_pa.max(d.q_pa);
        self.summary.max_specific_force_g = self.summary.max_specific_force_g.max(specific);
        self.summary.max_temperature_k =
            self.summary.max_temperature_k.max(self.state.temperature_k);
        if d.q_pa > s.max_q_pa {
            self.warn("q_limit", "Configured dynamic pressure limit exceeded");
        }
        if specific > s.max_accel_g {
            self.warn("g_limit", "Configured acceleration limit exceeded");
        }
        if self.state.temperature_k > s.max_temperature_k {
            self.warn(
                "thermal_limit",
                "Configured skin temperature limit exceeded; thermal estimate is a screening model",
            );
        }
        if d.knudsen > 0.01 {
            self.warn("rarefied","Knudsen number exceeds 0.01: continuum aerodynamic/heating tables are outside their physical domain");
        }
        if d.altitude_m > 86000.0 {
            self.warn(
                "atmosphere_extension",
                "Above 86 km: approximate exponential atmosphere extension in use",
            );
        }
        if d.mach > s.aero.last().unwrap().mach {
            self.warn(
                "aero_range",
                "Mach outside aerodynamic table; endpoint coefficients held",
            );
        }
        if d.alpha_rad.to_degrees() > 15.0 && !self.on_rail {
            self.warn(
                "high_aoa",
                "Angle of attack exceeds 15 degrees: small-angle normal force data is unreliable",
            );
        }
        if !self.on_rail && d.mach > 0.3 && d.cp_m < d.mass.cg_m {
            self.warn(
                "static_instability",
                "Aerodynamic CP is ahead of CG: negative static stability margin",
            );
        }
        if d.mach >= 5.0 {
            self.warn("hypersonic","Hypersonic regime: perfect-gas aero and Suttonâ€“Graves screening do not resolve real-gas chemistry or ablation");
        }
        if self.battery() <= 0.0 {
            self.warn(
                "power",
                "Flight computer battery exhausted; pyrotechnic commands disabled",
            );
        }
        if self.time + 1e-9 >= self.next_sensor {
            self.sample_sensors(&d);
            self.next_sensor = self.time + 1.0 / self.mission.avionics.sample_hz;
        }
        while self
            .queue
            .front()
            .is_some_and(|(time, _)| *time <= self.time + 1e-9)
        {
            let (_, sensor) = self.queue.pop_front().unwrap();
            self.sensor = sensor;
            let valid = sensor.baro_valid || sensor.gnss_valid;
            if valid && self.airborne && self.battery() > 0.0 {
                // Noise is filtered in the measurement path; confirm consecutive descent samples.
                if self
                    .last_altitude
                    .is_some_and(|last| sensor.altitude < last - 0.001)
                {
                    self.descent_count += 1;
                } else {
                    self.descent_count = 0;
                }
                self.last_altitude = Some(sensor.altitude);
                if !self.mission.experiments.replace_recovery_logic
                    && self.descent_count >= self.mission.avionics.apogee_confirm_samples
                {
                    self.deploy(false, &d);
                }
                if !self.mission.experiments.replace_recovery_logic
                    && self.descent_count >= self.mission.avionics.apogee_confirm_samples
                    && sensor.altitude
                        <= self.mission.recovery.main_altitude_agl_m
                            + crate::environment::ground_msl(&self.mission, self.state.position)
                            - self.mission.launch.altitude_m
                {
                    self.deploy(true, &d);
                }
            } else {
                self.descent_count = 0;
                self.last_altitude = None;
            }
        }
        let rules = self.mission.experiments.commands.clone();
        for (index, rule) in rules.iter().enumerate() {
            if rule.stage == self.active
                && self.time >= rule.after_s
                && (!rule.descending
                    || self.descent_count >= self.mission.avionics.apogee_confirm_samples)
                && (self.sensor.baro_valid || self.sensor.gnss_valid)
                && self.sensor.altitude <= rule.altitude_below_m
                && d.mach <= rule.max_mach
                && d.q_pa <= rule.max_q_pa
                && self.battery() > 0.0
                && !self.fired_commands.contains(&index)
            {
                let fault = match rule.action {
                    crate::experiments::Action::Drogue => {
                        Some(crate::experiments::FaultKind::DroguePyro)
                    }
                    crate::experiments::Action::Main => {
                        Some(crate::experiments::FaultKind::MainPyro)
                    }
                    _ => None,
                };
                if fault.is_some_and(|f| self.mission.experiments.fault(f, self.time)) {
                    continue;
                }
                self.event("command", rule.name.clone());
                match rule.action {
                    crate::experiments::Action::Drogue => self.deploy(false, &d),
                    crate::experiments::Action::Main => self.deploy(true, &d),
                    crate::experiments::Action::Separate => {
                        if self.active + 1 < self.mission.stages.len() && !self.on_rail {
                            self.separate(&d);
                        }
                    }
                }
                self.fired_commands.insert(index);
                if matches!(rule.action, crate::experiments::Action::Separate) {
                    return;
                }
            }
        }
        for main in [false, true] {
            let (cfg, chute) = if main {
                (&self.mission.recovery.main, &self.main)
            } else {
                (&self.mission.recovery.drogue, &self.drogue)
            };
            let load = if self.suspension.as_ref().is_some_and(|p| p.main == main) {
                d.chute_load_n
            } else {
                d.q_pa
                    * cfg.area_m2
                    * cfg.cd
                    * chute.fraction(self.time, cfg)
                    * chute.opening(self.time, cfg)
            };
            if load > cfg.max_load_n && !chute.failed {
                if main {
                    self.main.failed = true;
                } else {
                    self.drogue.failed = true;
                }
                self.event(
                    "chute_failure",
                    format!(
                        "{} load {:.0} N exceeds line strength",
                        if main { "Main" } else { "Drogue" },
                        load
                    ),
                );
            }
        }
        if self.active + 1 < self.mission.stages.len() && !self.on_rail {
            let trigger = match s.separation {
                Trigger::Burnout { delay_s } => {
                    self.ignited
                        && self.time + 1e-9 >= self.ignition + s.motor.burn_time() + delay_s
                }
                Trigger::Time { time_s } => self.time >= time_s,
                Trigger::Mach { mach } => d.mach >= mach && vertical > 0.0,
                Trigger::Altitude { altitude_agl_m } => d.agl_m >= altitude_agl_m && vertical > 0.0,
                Trigger::Never => false,
            };
            if trigger && self.battery() > 0.0 {
                self.separate(&d);
            }
        }
        if self.airborne
            && crate::environment::clearance(&self.mission, self.state.position) <= 0.0
            && vertical < 0.0
        {
            self.summary.landing_speed_mps = Some(self.state.velocity.length());
            self.summary.landing_east_m = Some(self.state.position.x);
            self.summary.landing_north_m = Some(self.state.position.y);
            self.summary.completed = true;
            self.finished = self.debris.iter().all(|d| d.landed);
            self.event(
                "landing",
                format!("{:.2} m/s", self.state.velocity.length()),
            );
            self.state.position =
                crate::environment::ground_position(&self.mission, self.state.position);
            self.state.velocity = DVec3::ZERO;
            self.state.omega = DVec3::ZERO;
            self.record();
        }
    }
    fn sample_sensors(&mut self, d: &Diagnostics) {
        let av = &self.mission.avionics;
        let baro_valid = d.altitude_m <= av.baro_ceiling_m
            && !self
                .mission
                .experiments
                .fault(crate::experiments::FaultKind::Barometer, self.time);
        let gnss_valid = d.altitude_m <= av.gnss_ceiling_m
            && !self
                .mission
                .experiments
                .fault(crate::experiments::FaultKind::Gnss, self.time);
        let noise = if baro_valid {
            av.baro_noise_m * (self.time * 3.17).sin()
        } else {
            av.gnss_noise_m * (self.time * 0.73).sin()
        };
        let measured = d.agl_m + noise + if baro_valid { av.baro_bias_m } else { 0.0 };
        let previous = self
            .queue
            .back()
            .map_or(self.sensor.altitude, |(_, s)| s.altitude);
        let filtered =
            previous + (measured - previous) * (1.0 - (-1.0 / av.sample_hz / 0.12).exp());
        let sensor = Sensor {
            altitude: filtered,
            baro_valid,
            gnss_valid,
            accel_saturated: d.force_world.length() / d.mass.kg / 9.80665 > av.accel_range_g,
            gyro_saturated: self.state.omega.abs().max_element().to_degrees() > av.gyro_range_deg_s,
        };
        self.queue.push_back((self.time + av.latency_s, sensor));
    }
    fn deploy(&mut self, main: bool, d: &Diagnostics) {
        let (cfg, chute) = if main {
            (&self.mission.recovery.main, &self.main)
        } else {
            (&self.mission.recovery.drogue, &self.drogue)
        };
        let fault = if main {
            crate::experiments::FaultKind::MainPyro
        } else {
            crate::experiments::FaultKind::DroguePyro
        };
        if self.battery() <= 0.0 || self.mission.experiments.fault(fault, self.time) {
            return;
        }
        if !cfg.enabled || chute.deployed.is_some() || chute.failed {
            return;
        }
        if d.q_pa > cfg.max_q_pa || d.mach > cfg.max_mach {
            self.warn(
                if main {
                    "main_inhibited"
                } else {
                    "drogue_inhibited"
                },
                "Parachute deployment deferred by Mach/dynamic pressure interlock",
            );
            return;
        }
        if main {
            self.main.deployed = Some(self.time);
            self.drogue.failed = true;
        } else {
            if self.main.deployed.is_some() {
                return;
            }
            self.drogue.deployed = Some(self.time);
        }
        self.event(
            "deployment",
            if main {
                "Main parachute"
            } else {
                "Drogue parachute"
            },
        );
    }
    fn separate(&mut self, d: &Diagnostics) {
        let old = self.active;
        let mut mission = self.mission.clone();
        let mut stage = mission.stages[old].clone();
        let fuel = stage.motor.remaining(if self.ignited {
            self.time - self.ignition
        } else {
            0.0
        });
        // Unburned propellant remains with a deliberately separated powered stage, but its motor is quenched.
        stage.dry_cg_m = (stage.dry_mass_kg * stage.dry_cg_m + fuel * stage.motor.cg_m)
            / (stage.dry_mass_kg + fuel);
        stage.dry_mass_kg += fuel;
        stage.motor.propellant_mass_kg = 0.0;
        if let Some(material) = stage.materials.as_mut().filter(|m| m.use_material_mass) {
            let total = material.hardware_mass_kg + fuel;
            if total > 0.0 {
                material.hardware_cg_m = (material.hardware_mass_kg * material.hardware_cg_m
                    + fuel * stage.motor.cg_m)
                    / total;
                material.hardware_mass_kg = total;
            }
        }
        if let Some(recovery) = &stage.recovery {
            mission.recovery = recovery.clone();
        } else {
            mission.recovery.drogue.enabled = false;
            mission.recovery.main.enabled = false;
        }
        for a in &mut stage.aero {
            a.cp_m = stage.length_m * 0.7;
        }
        crate::model_physics::retain_propellant(&mut stage, fuel);
        mission.stages = vec![stage];
        mission.experiments.aero_grids.clear();
        mission.experiments.commands.clear();
        mission.experiments.thermal.clear();
        mission.experiments.recovery = self
            .mission
            .experiments
            .recovery
            .iter()
            .filter(|r| r.stage == old)
            .cloned()
            .map(|mut r| {
                r.stage = 0;
                r
            })
            .collect();
        let debris_mass = mass_properties(&mission, 0, self.time, f64::NEG_INFINITY);
        let dm = debris_mass.kg;
        self.active += 1;
        let upper = mass_properties(&self.mission, self.active, self.time, f64::INFINITY).kg;
        let axis = self.state.attitude * DVec3::Z;
        let relative_speed = self.mission.stages[old].separation_speed_mps;
        let old_length = self.mission.stages[old].length_m;
        let upper_length = self.mission.stages[self.active..]
            .iter()
            .map(|s| s.length_m)
            .sum::<f64>();
        let upper_mass = mass_properties(&self.mission, self.active, self.time, f64::INFINITY);
        let debris_center = debris_mass.cg_body_m - DVec3::Z * upper_length;
        let debris_offset = debris_center - d.mass.cg_body_m;
        let upper_offset = upper_mass.cg_body_m - d.mass.cg_body_m;
        let mut state = self.state.clone();
        let offset = self.state.attitude * debris_offset;
        state.position += offset;
        state.velocity += self.state.attitude * (self.state.omega.cross(debris_offset))
            - axis * relative_speed * upper / (upper + dm);
        self.debris.push(Debris {
            stage: old,
            state,
            landed: false,
            mission,
            drogue: ChuteState {
                model: self.mission.experiments.recovery(old, false),
                ..Default::default()
            },
            main: ChuteState {
                model: self.mission.experiments.recovery(old, true),
                ..Default::default()
            },
            apogee: false,
            inhibited: [false; 2],
            landing_speed_mps: 0.0,
            suspension: None,
        });
        let offset = self.state.attitude * upper_offset;
        self.state.position += offset;
        self.state.velocity += self.state.attitude * (self.state.omega.cross(upper_offset))
            + axis * relative_speed * dm / (upper + dm);
        self.drogue.model = self.mission.experiments.recovery(self.active, false);
        self.main.model = self.mission.experiments.recovery(self.active, true);
        self.ignition = self.time + self.mission.stages[self.active].ignition_delay_s;
        self.ignited = false;
        self.burnout_reported = false;
        self.event(
            "separation",
            format!(
                "Released stage {}, length {:.2} m; remaining mass {:.2} kg",
                old + 1,
                old_length,
                upper
            ),
        );
        // Playback needs both new CG frames at the exact staging instant, even
        // when the regular telemetry period does not coincide with the trigger.
        self.record();
    }
    fn record(&mut self) {
        if self
            .samples
            .last()
            .is_some_and(|s| (s.time_s - self.time).abs() < 1e-10)
        {
            self.samples.pop();
        }
        while self
            .debris_samples
            .last()
            .is_some_and(|s| (s.time_s - self.time).abs() < 1e-10)
        {
            self.debris_samples.pop();
        }
        for debris in &self.debris {
            let q = debris.state.attitude;
            let d = debris.diagnostics(self.time);
            let mass = mass_properties(&debris.mission, 0, self.time, f64::NEG_INFINITY);
            self.debris_samples.push(DebrisSample {
                time_s: self.time,
                stage: debris.stage,
                east_m: debris.state.position.x,
                north_m: debris.state.position.y,
                altitude_agl_m: atmosphere::altitude(
                    debris.state.position,
                    self.mission.launch.altitude_m,
                ) - self.mission.launch.altitude_m,
                mass_kg: mass.kg,
                cg_m: mass.cg_m,
                qw: q.w,
                qx: q.x,
                qy: q.y,
                qz: q.z,
                landed: debris.landed,
                drogue_fraction: debris
                    .drogue
                    .fraction(self.time, &debris.mission.recovery.drogue),
                main_fraction: debris
                    .main
                    .fraction(self.time, &debris.mission.recovery.main),
                chute_load_n: d.chute_load_n,
                mach: d.mach,
                q_pa: d.q_pa,
                vx: debris.state.velocity.x,
                vy: debris.state.velocity.y,
                vz: debris.state.velocity.z,
                landing_speed_mps: debris.landing_speed_mps,
            });
        }
        if let (Some(point), Some(c)) =
            (&self.suspension, suspension_chute(&self.drogue, &self.main))
        {
            let mass = mass_properties(&self.mission, self.active, self.time, self.ignition);
            let (tension, _) = suspension_tension(
                &self.mission,
                self.active,
                &self.state,
                point,
                &c.model,
                &mass,
            );
            self.recovery_samples.push(crate::recovery::Sample {
                time_s: self.time,
                stage: self.active,
                main: point.main,
                east_m: point.position.x,
                north_m: point.position.y,
                altitude_agl_m: atmosphere::altitude(
                    point.position,
                    self.mission.launch.altitude_m,
                ) - self.mission.launch.altitude_m,
                tension_n: tension.length(),
            });
        }
        for debris in &self.debris {
            if let (Some(point), Some(c)) = (
                &debris.suspension,
                suspension_chute(&debris.drogue, &debris.main),
            ) {
                let mass = mass_properties(&debris.mission, 0, self.time, f64::NEG_INFINITY);
                let (tension, _) =
                    suspension_tension(&debris.mission, 0, &debris.state, point, &c.model, &mass);
                self.recovery_samples.push(crate::recovery::Sample {
                    time_s: self.time,
                    stage: debris.stage,
                    main: point.main,
                    east_m: point.position.x,
                    north_m: point.position.y,
                    altitude_agl_m: atmosphere::altitude(
                        point.position,
                        self.mission.launch.altitude_m,
                    ) - self.mission.launch.altitude_m,
                    tension_n: tension.length(),
                });
            }
        }
        let d = self.diagnostics();
        let q = self.state.attitude;
        let up =
            atmosphere::radial(self.state.position, self.mission.launch.altitude_m).normalize();
        self.samples.push(Telemetry {
            time_s: self.time,
            stage: self.active,
            altitude_m: d.altitude_m,
            altitude_agl_m: d.agl_m,
            east_m: self.state.position.x,
            north_m: self.state.position.y,
            speed_mps: self.state.velocity.length(),
            vertical_mps: self.state.velocity.dot(up),
            mach: d.mach,
            regime: Regime::from_mach(d.mach),
            q_pa: d.q_pa,
            alpha_deg: d.alpha_rad.to_degrees(),
            mass_kg: d.mass.kg,
            cg_m: d.mass.cg_m,
            cp_m: d.cp_m,
            stability_calibers: (d.cp_m - d.mass.cg_m)
                / self.mission.stages[self.active].diameter_m,
            cd: d.cd,
            thrust_n: d.thrust_n,
            specific_force_g: d.force_world.length() / d.mass.kg / 9.80665,
            roll_rate_deg_s: self.state.omega.z.to_degrees(),
            pitch_rate_deg_s: self.state.omega.x.to_degrees(),
            yaw_rate_deg_s: self.state.omega.y.to_degrees(),
            temperature_k: self.state.temperature_k,
            heat_flux_w_m2: d.heat_flux_w_m2,
            knudsen: d.knudsen,
            reynolds: d.reynolds,
            chute_load_n: d.chute_load_n,
            drogue_fraction: self
                .drogue
                .fraction(self.time, &self.mission.recovery.drogue),
            main_fraction: self.main.fraction(self.time, &self.mission.recovery.main),
            sensed_altitude_agl_m: self.sensor.altitude,
            battery_wh: self.battery(),
            baro_valid: self.sensor.baro_valid,
            gnss_valid: self.sensor.gnss_valid,
            accel_saturated: self.sensor.accel_saturated,
            gyro_saturated: self.sensor.gyro_saturated,
            qw: q.w,
            qx: q.x,
            qy: q.y,
            qz: q.z,
            vx: self.state.velocity.x,
            vy: self.state.velocity.y,
            vz: self.state.velocity.z,
        });
    }
    pub fn run(self) -> FlightResult {
        self.run_with_progress(|_, _| {})
    }
    pub fn run_with_progress(mut self, mut progress: impl FnMut(f64, f64)) -> FlightResult {
        let mut steps = 0;
        progress(self.time, self.mission.duration);
        while !self.finished {
            self.step();
            steps += 1;
            if steps % 64 == 0 {
                progress(self.time, self.mission.duration);
            }
        }
        progress(self.time, self.mission.duration);
        self.into_result()
    }
    pub fn into_result(mut self) -> FlightResult {
        self.summary.end_time_s = self.time;
        FlightResult {
            mission: self.mission,
            samples: self.samples,
            events: self.events,
            summary: self.summary,
            debris_samples: self.debris_samples,
            recovery_samples: self.recovery_samples,
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    #[test]
    fn vacuum_coast_conserves_energy_and_angular_momentum() {
        let mut m = Mission::demo(false);
        m.launch.earth_rotation = false;
        m.wind.east_mps = 0.0;
        m.wind.gust_mps = 0.0;
        for a in &mut m.stages[0].aero {
            a.cd_power_off = 0.0;
            a.cn_alpha_per_rad = 0.0;
        }
        m.stages[0].roll_damping = 0.0;
        m.stages[0].pitch_damping = 0.0;
        let mut state = State {
            position: DVec3::Z * 200000.0,
            velocity: DVec3::new(1500.0, 20.0, 500.0),
            attitude: DQuat::IDENTITY,
            omega: DVec3::Z * 0.5,
            temperature_k: 288.0,
        };
        let energy = |s: &State| {
            0.5 * s.velocity.length_squared()
                - atmosphere::MU / atmosphere::radial(s.position, m.launch.altitude_m).length()
        };
        let e0 = energy(&state);
        let l0 = atmosphere::radial(state.position, m.launch.altitude_m).cross(state.velocity);
        for i in 0..1000 {
            let t = i as f64 * 0.02;
            state = rk4(&state, 0.02, |offset, y| {
                derivative(&m, 0, f64::INFINITY, t + offset, y, 0.0, None)
            });
        }
        assert!((energy(&state) / e0 - 1.0).abs() < 1e-11);
        assert!(
            (atmosphere::radial(state.position, m.launch.altitude_m).cross(state.velocity) - l0)
                .length()
                / l0.length()
                < 1e-11
        );
        let expected = DQuat::from_rotation_z(10.0);
        assert!(state.attitude.dot(expected).abs() > 1.0 - 1e-10);
        assert!((state.omega.z - 0.5).abs() < 1e-10);
    }
    #[test]
    fn separation_conserves_linear_momentum_and_center_of_mass() {
        let m = Mission::demo(true);
        let mut sim = Simulator::new(m).unwrap();
        sim.time = 3.0;
        sim.ignition = 0.0;
        sim.ignited = true;
        sim.state.velocity = DVec3::new(12.0, 2.0, 500.0);
        sim.state.omega = DVec3::new(0.1, 0.2, 0.3);
        sim.state.position = DVec3::Z * 1000.0;
        let d = sim.diagnostics();
        let momentum = sim.state.velocity * d.mass.kg;
        let weighted_position = sim.state.position * d.mass.kg;
        sim.separate(&d);
        let debris = &sim.debris[0];
        let dm = mass_properties(&debris.mission, 0, sim.time, f64::NEG_INFINITY).kg;
        let upper = mass_properties(&sim.mission, sim.active, sim.time, f64::INFINITY).kg;
        assert!((dm + upper - d.mass.kg).abs() < 1e-10);
        assert!(
            (sim.state.velocity * upper + debris.state.velocity * dm - momentum).length() < 1e-8
        );
        assert!(
            (sim.state.position * upper + debris.state.position * dm - weighted_position).length()
                < 1e-8
        );
    }
    #[test]
    fn restoring_moment_and_roll_cant_act_on_attitude() {
        let m = Mission::demo(false);
        let mut state = State {
            position: DVec3::Z * 1000.0,
            velocity: DVec3::Z * 300.0,
            attitude: DQuat::from_rotation_y(0.1),
            omega: DVec3::ZERO,
            temperature_k: 288.0,
        };
        let d = forces(&m, 0, f64::INFINITY, 10.0, &state, 0.0);
        assert!(d.torque_body.y < 0.0);
        let mut m = m;
        m.stages[0].fin_cant_deg = 1.0;
        let d = forces(&m, 0, f64::INFINITY, 10.0, &state, 0.0);
        assert!(d.torque_body.z > 0.0);
        state.omega.z = 100.0;
        let damped = forces(&m, 0, f64::INFINITY, 10.0, &state, 0.0);
        assert!(damped.torque_body.z < d.torque_body.z);
    }
    #[test]
    fn tail_first_descent_is_unstable_and_canopy_attachment_restores_attitude() {
        let mut m = Mission::demo(false);
        m.wind.east_mps = 0.0;
        m.wind.north_mps = 0.0;
        m.wind.gust_mps = 0.0;
        let mut state = State {
            position: DVec3::Z * 1000.0,
            velocity: -DVec3::Z * 100.0,
            attitude: DQuat::from_rotation_y(0.1),
            omega: DVec3::ZERO,
            temperature_k: 288.0,
        };
        let initial = forces(&m, 0, f64::INFINITY, 50.0, &state, 0.0);
        assert!(
            initial.torque_body.y > 0.0,
            "Tail-first perturbation must grow, rather than falsely restore"
        );
        let canopy = forces(&m, 0, f64::INFINITY, 50.0, &state, 2.0);
        assert!(
            canopy.torque_body.y < 0.0,
            "Nose suspension must stabilize the descending rocket"
        );
        for step in 0..1000 {
            state = rk4(&state, 0.001, |dt, y| {
                derivative(
                    &m,
                    0,
                    f64::INFINITY,
                    50.0 + step as f64 * 0.001 + dt,
                    y,
                    0.0,
                    None,
                )
            });
        }
        assert!(state.omega.y > 0.0);
        assert!(
            (state.attitude * DVec3::Z).angle_between(DVec3::Z) > 0.1,
            "Descent rotation must follow integrated torque"
        );
    }
}
