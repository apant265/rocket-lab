use glam::DVec3;
use rocket_lab::{atmosphere, config::*, flight::*, imports};
fn calm(hypersonic: bool) -> Mission {
    let mut m = Mission::demo(hypersonic);
    m.wind.east_mps = 0.0;
    m.wind.north_mps = 0.0;
    m.wind.gust_mps = 0.0;
    m.launch.elevation_deg = 90.0;
    m.launch.earth_rotation = false;
    m
}
#[test]
fn standard_atmosphere_checkpoints() {
    let sea = atmosphere::sample(0.0);
    assert!((sea.density - 1.225).abs() < 0.001);
    assert!((sea.sound_speed - 340.294).abs() < 0.01);
    let at11 = atmosphere::sample(11000.0);
    assert!((at11.temperature - 216.77).abs() < 0.2);
    assert!((at11.pressure - 22700.0).abs() < 100.0);
    let at20 = atmosphere::sample(20000.0);
    assert!((at20.pressure - 5529.0).abs() < 20.0);
}
#[test]
fn atmosphere_is_continuous_and_positive() {
    for h in [
        0.0, 11000.0, 20000.0, 32000.0, 47000.0, 51000.0, 71000.0, 86000.0, 100000.0, 170000.0,
    ] {
        let a = atmosphere::sample(h);
        let b = atmosphere::sample(h + 0.1);
        assert!(a.density > 0.0 && a.pressure > 0.0 && a.temperature > 0.0);
        assert!((a.density / b.density - 1.0).abs() < 0.001);
    }
}
#[test]
fn spherical_gravity_and_altitude() {
    assert!((atmosphere::altitude(DVec3::new(0.0, 0.0, 100000.0), 600.0) - 100600.0).abs() < 1e-6);
    let g0 = atmosphere::gravity(DVec3::ZERO, 0.0).length();
    let g1 = atmosphere::gravity(DVec3::Z * 100000.0, 0.0).length();
    assert!((g0 / g1 - (1.0 + 100000.0 / atmosphere::EARTH_RADIUS).powi(2)).abs() < 1e-10);
}
#[test]
fn motor_impulse_and_propellant_bookkeeping() {
    let motor = Motor {
        name: "test".into(),
        propellant_mass_kg: 2.0,
        cg_m: 1.0,
        points: vec![[0.0, 0.0], [1.0, 100.0], [3.0, 100.0], [4.0, 0.0]],
        thrust_offset_m: [0.0; 2],
        misalignment_deg: [0.0; 2],
    };
    assert!((motor.impulse_to(4.0) - 300.0).abs() < 1e-10);
    assert!((motor.remaining(2.0) - 1.0).abs() < 1e-10);
    assert_eq!(motor.remaining(-1.0), 2.0);
    assert_eq!(motor.remaining(10.0), 0.0);
    assert_eq!(motor.thrust(4.0), 0.0);
}
#[test]
fn mass_cg_and_inertia_change_during_burn() {
    let m = calm(true);
    let a = mass_properties(&m, 0, 0.0, 0.0);
    let b = mass_properties(&m, 0, m.stages[0].motor.burn_time(), 0.0);
    assert!((a.kg - b.kg - m.stages[0].motor.propellant_mass_kg).abs() < 1e-10);
    assert!(a.cg_m > b.cg_m);
    assert!(a.inertia.cmpgt(b.inertia).all());
}
#[test]
fn aerodynamic_interpolation_and_regime_boundaries() {
    let m = calm(false);
    let s = &m.stages[0];
    let at = coefficients(s, 1.025);
    assert!((at.cd_power_off - (0.68 + 0.83) * 0.5).abs() < 1e-10);
    assert_eq!(Regime::from_mach(0.79), Regime::Subsonic);
    assert_eq!(Regime::from_mach(0.8), Regime::Transonic);
    assert_eq!(Regime::from_mach(1.2), Regime::Supersonic);
    assert_eq!(Regime::from_mach(5.0), Regime::Hypersonic);
}
#[test]
fn hpr_flight_lands_under_main() {
    let result = Simulator::new(calm(false)).unwrap().run();
    assert!(result.summary.completed, "{:?}", result.summary);
    assert!(result.summary.apogee_agl_m > 1000.0);
    assert!(result.events.iter().any(|e| e.kind == "rail_exit"));
    assert!(result.events.iter().any(|e| e.kind == "apogee"));
    assert!(
        result
            .events
            .iter()
            .any(|e| e.kind == "deployment" && e.detail.contains("Main"))
    );
    assert!(result.summary.landing_speed_mps.unwrap() < 15.0);
    assert!(result.samples.windows(2).all(|w| w[1].time_s > w[0].time_s));
    assert!(
        result
            .samples
            .iter()
            .all(|s| (s.qw * s.qw + s.qx * s.qx + s.qy * s.qy + s.qz * s.qz - 1.0).abs() < 1e-10)
    );
}
#[test]
fn staged_sounding_flight_crosses_all_regimes() {
    let mut m = calm(true);
    m.duration = 220.0;
    let result = Simulator::new(m).unwrap().run();
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| e.kind == "separation")
            .count(),
        2
    );
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| e.kind == "ignition")
            .count(),
        3
    );
    assert!(result.summary.max_mach > 5.0, "{:?}", result.summary);
    for regime in [
        Regime::Subsonic,
        Regime::Transonic,
        Regime::Supersonic,
        Regime::Hypersonic,
    ] {
        assert!(result.samples.iter().any(|s| s.regime == regime));
    }
    assert!(
        result
            .summary
            .warnings
            .iter()
            .any(|w| w.contains("Hypersonic"))
    );
    assert!(result.debris_samples.iter().all(|s| {
        [
            s.east_m,
            s.north_m,
            s.altitude_agl_m,
            s.qw,
            s.qx,
            s.qy,
            s.qz,
        ]
        .iter()
        .all(|v| v.is_finite())
    }));
}

#[test]
fn mach_trigger_allows_separation_in_every_regime() {
    for threshold in [0.5, 1.0, 2.0, 5.0] {
        let mut m = calm(true);
        m.duration = 12.0;
        m.stages[0].separation = Trigger::Mach { mach: threshold };
        for p in &mut m.stages[0].motor.points {
            p[1] *= 5.0;
        }
        let result = Simulator::new(m).unwrap().run();
        let event = result
            .events
            .iter()
            .find(|e| e.kind == "separation")
            .expect("first-stage separation");
        assert!(event.time_s > 0.0);
        let before = result
            .samples
            .iter()
            .rev()
            .find(|s| s.time_s <= event.time_s && s.stage == 0)
            .unwrap();
        assert!(
            (before.mach - threshold).abs() < 0.3,
            "threshold {threshold}, actual {}",
            before.mach
        );
    }
}

#[test]
fn valid_motor_and_aero_imports() {
    let motor = imports::import_motor("examples/demo.eng").unwrap();
    assert_eq!(motor.propellant_mass_kg, 5.0);
    assert_eq!(motor.burn_time(), 4.0);
    assert!(motor.impulse_to(4.0) > 8000.0);
    let aero = imports::import_aero("examples/aero-si.csv").unwrap();
    assert_eq!(aero.len(), 11);
    assert_eq!(aero.last().unwrap().mach, 25.0);
}

#[test]
fn named_database_motor_variants_match_supplied_impulse() {
    for (name, impulse) in [
        ("O6000W", 39687.2142305),
        ("O3400-IM-P", 21041.010582),
        ("N5800-CS", 20367.689496),
    ] {
        let motor = imports::import_motor_named("sources/rasp.eng", Some(name)).unwrap();
        assert!((motor.impulse_to(motor.burn_time()) - impulse).abs() < 1e-6);
    }
    assert!(imports::import_motor_named("sources/rasp.eng", Some("UNKNOWN-MOTOR")).is_err());
    assert!(imports::import_motor("sources/rasp.eng").is_err());
}
#[test]
fn provisional_source_mapping_matches_older_loaded_mass_and_cg() {
    let m = Mission::load("examples/plumbbob-provisional.json").unwrap();
    let mass = mass_properties(&m, 0, 0.0, 0.0);
    assert!((mass.kg - 182.0 * 0.45359237).abs() < 1e-8);
    assert!((mass.cg_m - 137.0 * 0.0254).abs() < 1e-8);
    assert!(m.stages.iter().all(|s| s.aero_source.contains("SYNTHETIC")));
}
#[test]
fn timestep_refinement_preserves_apogee() {
    let mut m = calm(false);
    let a = Simulator::new(m.clone()).unwrap().run();
    m.dt = 0.0025;
    let b = Simulator::new(m).unwrap().run();
    let relative = (a.summary.apogee_agl_m / b.summary.apogee_agl_m - 1.0).abs();
    assert!(relative < 0.002, "{relative}");
    assert!(
        (a.summary.landing_speed_mps.unwrap() - b.summary.landing_speed_mps.unwrap()).abs() < 0.2
    );
}
#[test]
fn repeated_runs_are_identical() {
    let mut m = calm(false);
    m.duration = 10.0;
    let a = Simulator::new(m.clone()).unwrap().run();
    let b = Simulator::new(m).unwrap().run();
    assert_eq!(
        serde_json::to_string(&a.samples).unwrap(),
        serde_json::to_string(&b.samples).unwrap()
    );
}
#[test]
fn no_launch_when_thrust_is_insufficient() {
    let mut m = calm(false);
    m.duration = 10.0;
    for p in &mut m.stages[0].motor.points {
        p[1] *= 0.001;
    }
    let r = Simulator::new(m).unwrap().run();
    assert!(!r.summary.completed);
    assert!(r.summary.apogee_agl_m < 1e-6);
    assert!(!r.events.iter().any(|e| e.kind == "rail_exit"));
}
#[test]
fn battery_failure_prevents_recovery() {
    let mut m = calm(false);
    let nominal = Simulator::new(m.clone()).unwrap().run();
    m.avionics.battery_wh = 0.00001;
    let r = Simulator::new(m).unwrap().run();
    assert!(r.summary.completed);
    assert!(!r.events.iter().any(|e| e.kind == "deployment"));
    // Large-angle crossflow can slow a tumbling ballistic body. Compare with
    // successful recovery rather than assuming a nose-first ballistic speed.
    assert!(
        r.summary.landing_speed_mps.unwrap() > 2.0 * nominal.summary.landing_speed_mps.unwrap()
    );
}
#[test]
fn chute_interlock_blocks_unsafe_deployment() {
    let mut m = calm(false);
    m.recovery.main.max_q_pa = 0.000001;
    m.recovery.drogue.max_q_pa = 0.000001;
    let r = Simulator::new(m).unwrap().run();
    assert!(!r.events.iter().any(|e| e.kind == "deployment"));
    assert!(
        r.events
            .iter()
            .any(|e| e.kind == "warning" && e.detail.contains("deployment deferred"))
    );
}
#[test]
fn altitude_sensor_outage_prevents_false_apogee() {
    let mut m = calm(false);
    m.avionics.baro_ceiling_m = 1.0;
    m.avionics.gnss_ceiling_m = 1.0;
    let r = Simulator::new(m).unwrap().run();
    assert!(!r.events.iter().any(|e| e.kind == "deployment"));
}
#[test]
fn validation_rejects_broken_inputs() {
    let mut m = calm(false);
    m.stages[0].motor.points[1][0] = 0.0;
    assert!(m.validate().is_err());
    let mut m = calm(false);
    m.dt = f64::NAN;
    assert!(m.validate().is_err());
    let mut m = calm(false);
    m.stages[0].aero[1].mach = 0.0;
    assert!(m.validate().is_err());
    let mut m = calm(false);
    m.avionics.sample_hz = 0.0;
    assert!(m.validate().is_err());
}
#[test]
fn supplied_reference_units() {
    let rows = imports::reference_flight("sources/Plumbbob_MKIII_R75 PDR Flight Data.CSV").unwrap();
    assert_eq!(rows.len(), 2456);
    let apogee = rows.iter().map(|r| r.altitude_agl_m).fold(0.0, f64::max);
    assert!((apogee - 168140.96928).abs() < 1.0);
    let mach = rows.iter().map(|r| r.mach).fold(0.0, f64::max);
    assert!((mach - 6.2076).abs() < 0.001);
}
#[test]
fn partial_cfd_and_flight_csv_are_not_aero_tables() {
    assert!(imports::import_aero("sources/gpu_drag_table.csv").is_err());
    assert!(imports::import_aero("sources/Plumbbob_MKIII_R75 PDR Flight Data.CSV").is_err());
}
#[test]
fn supplied_designs_are_readable() {
    assert_eq!(
        imports::inspect_design("sources/Plumbbob_MKIII_R75.CDX1")
            .unwrap()
            .geometry
            .len(),
        4
    );
    assert!(
        !imports::inspect_design("sources/Plumbbob_MKIII_R75 (1).ork")
            .unwrap()
            .geometry
            .is_empty()
    );
}
