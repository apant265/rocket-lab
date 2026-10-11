use rocket_lab::{cfd, config::Mission, environment::*, experiments::*, flight::Simulator};
#[test]
fn imported_wind_profile_is_shared_by_flight_and_surface() {
    let mut m = Mission::load("examples/advanced-laboratory.json").unwrap();
    m.duration = 6.0;
    let flight = Simulator::new(m.clone()).unwrap().run();
    for sample in flight.samples.iter().step_by(7) {
        let surface = rocket_lab::surface::SurfaceConditions::from_sample(sample, &m);
        assert!((surface.q_pa - sample.q_pa).abs() < 1e-6);
    }
}
#[test]
fn signed_reference_descent_velocity_is_compared_as_speed() {
    let path =
        std::env::temp_dir().join(format!("rocket-reference-speed-{}.csv", std::process::id()));
    std::fs::write(&path, "Time (sec),Stage,Altitude (ft),Velocity (ft/sec),Mach Number\n0,1,0,100,0.1\n1,1,50,-100,0.1\n").unwrap();
    let points = rocket_lab::imports::reference_flight(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!((points[0].speed_mps - 30.48).abs() < 1e-12);
    assert_eq!(points[0].speed_mps, points[1].speed_mps);
}
#[test]
fn shock_tube_matches_exact_solution_and_refines() {
    let a = cfd::sod_check(200);
    let b = cfd::sod_check(400);
    assert!(b.passed, "rho={} p={}", b.density_l1, b.pressure_l1);
    assert!(b.density_l1 < a.density_l1);
    assert!(b.pressure_l1 < a.pressure_l1);
}
#[test]
fn slip_wall_flux_has_no_mass_or_energy_transport() {
    let a = cfd::conserved(1.2, 300.0, 20.0, 80000.0);
    let f = cfd::rusanov(a, cfd::reflected(a, 0), 0);
    assert_eq!(f[0], 0.0);
    assert_eq!(f[3], 0.0);
    assert!(f[1] > 80000.0);
}
#[test]
fn uniform_flow_is_preserved() {
    let far = cfd::conserved(1.0, 1.0, 0.2, 1.0);
    let p = cfd::Problem {
        nx: 8,
        ny: 8,
        dx: 0.1,
        dy: 0.1,
        x0: 0.0,
        y0: 0.0,
        solid: vec![false; 64],
        far,
        steps: 10,
        dt: 0.01,
        mach: 0.8,
        alpha_deg: 0.0,
    };
    let f = cfd::solve(p).unwrap();
    assert!(f.cells.iter().all(|c| *c == far));
}
#[test]
fn terrain_bilinear_orientation_and_extent() {
    let t = Terrain {
        source: "test".into(),
        west_deg: 0.0,
        south_deg: 0.0,
        east_deg: 1.0,
        north_deg: 1.0,
        width: 2,
        height: 2,
        elevation_m: vec![100.0, 200.0, 300.0, 400.0],
    };
    t.validate().unwrap();
    assert_eq!(t.elevation(1.0, 0.0), Some(100.0));
    assert_eq!(t.elevation(0.5, 0.5), Some(250.0));
    assert_eq!(t.elevation(0.0, 1.0), Some(400.0));
    assert_eq!(t.elevation(2.0, 0.5), None);
}
#[test]
fn wind_profile_uses_msl_and_clamps() {
    let mut m = Mission::demo(false);
    m.wind.gust_mps = 0.0;
    m.environment.wind_profile = vec![
        WindLevel {
            altitude_m: 600.0,
            east_mps: 2.0,
            north_mps: 0.0,
        },
        WindLevel {
            altitude_m: 1600.0,
            east_mps: 10.0,
            north_mps: 4.0,
        },
    ];
    assert_eq!(wind(&m, 1100.0, 0.0), glam::DVec3::new(6.0, 2.0, 0.0));
    assert_eq!(wind(&m, 2000.0, 0.0).x, 10.0);
}
#[test]
fn air_profile_log_pressure_interpolation() {
    let mut m = Mission::demo(false);
    m.environment.air_profile = vec![
        AirLevel {
            altitude_m: 0.0,
            pressure_pa: 100000.0,
            temperature_k: 300.0,
        },
        AirLevel {
            altitude_m: 10000.0,
            pressure_pa: 25000.0,
            temperature_k: 200.0,
        },
    ];
    m.validate().unwrap();
    let a = rocket_lab::atmosphere::for_mission(5000.0, &m);
    assert!((a.pressure - 50000.0).abs() < 1e-8);
    assert_eq!(a.temperature, 250.0);
}
#[test]
fn reefing_has_hold_and_second_inflation() {
    let r = RecoveryModel {
        reef_fraction: 0.2,
        reef_hold_s: 3.0,
        ..Default::default()
    };
    assert!((r.inflation(1.0, 2.0) - 0.1).abs() < 1e-10);
    assert_eq!(r.inflation(4.0, 2.0), 0.2);
    assert!((r.inflation(6.0, 2.0) - 0.6).abs() < 1e-10);
    assert_eq!(r.inflation(8.0, 2.0), 1.0);
}
#[test]
fn coefficient_grid_trilinear_log_reynolds() {
    let mut p = Vec::new();
    for re in [100.0_f64, 10000.0] {
        for a in [0.0, 90.0] {
            for m in [0.0, 2.0] {
                p.push(Coefficient {
                    ca_off: m + a / 90.0 + re.log10(),
                    ca_on: 0.0,
                    cn: a / 90.0,
                    cm: -a / 90.0,
                });
            }
        }
    }
    let g = AeroGrid {
        stage: 0,
        source: "test".into(),
        mach: vec![0.0, 2.0],
        alpha_deg: vec![0.0, 90.0],
        reynolds: vec![100.0, 10000.0],
        points: p,
    };
    let c = g.sample(1.0, 45.0, 1000.0);
    assert!((c.ca_off - 4.5).abs() < 1e-10);
    assert_eq!(c.cn, 0.5);
    assert_eq!(c.cm, -0.5);
}
#[test]
fn complete_sensor_outage_disables_recovery() {
    let mut m = Mission::demo(false);
    m.experiments.sensor_faults = vec![
        SensorFault {
            kind: FaultKind::Barometer,
            start_s: 0.0,
            end_s: 2000.0,
        },
        SensorFault {
            kind: FaultKind::Gnss,
            start_s: 0.0,
            end_s: 2000.0,
        },
    ];
    let f = Simulator::new(m).unwrap().run();
    assert!(!f.events.iter().any(|e| e.kind == "deployment"));
    assert!(f.summary.completed);
    assert!(
        f.samples
            .iter()
            .skip(2)
            .all(|s| !s.baro_valid && !s.gnss_valid)
    );
}
#[test]
fn programmable_recovery_uses_confirmed_descent() {
    let mut m = Mission::demo(false);
    m.experiments.replace_recovery_logic = true;
    m.experiments.commands = vec![
        CommandRule {
            name: "custom apogee drogue".into(),
            stage: 0,
            action: Action::Drogue,
            after_s: 0.0,
            descending: true,
            altitude_below_m: 1000000.0,
            max_mach: 0.7,
            max_q_pa: 10000.0,
        },
        CommandRule {
            name: "custom main".into(),
            stage: 0,
            action: Action::Main,
            after_s: 0.0,
            descending: true,
            altitude_below_m: 450.0,
            max_mach: 0.5,
            max_q_pa: 3000.0,
        },
    ];
    let f = Simulator::new(m).unwrap().run();
    assert_eq!(f.events.iter().filter(|e| e.kind == "command").count(), 2);
    let apex = f.events.iter().find(|e| e.kind == "apogee").unwrap().time_s;
    assert!(
        f.events
            .iter()
            .filter(|e| e.kind == "deployment")
            .all(|e| e.time_s > apex)
    );
    assert!(f.summary.landing_speed_mps.unwrap() < 15.0);
}
#[test]
fn landing_ellipse_preserves_center_and_axis() {
    let e = rocket_lab::export::landing_ellipse(&[[0.0, 2.0], [2.0, 2.0], [4.0, 2.0], [6.0, 2.0]])
        .unwrap();
    assert_eq!(e["center_east_m"], 3.0);
    assert_eq!(e["center_north_m"], 2.0);
    assert_eq!(e["minor_radius_m"], 0.0);
    assert_eq!(e["polygon_enu_m"].as_array().unwrap().len(), 65);
}
#[test]
fn per_component_shell_does_not_double_count_primary() {
    use rocket_lab::assembly::*;
    let mut m = Mission::demo(false);
    let s = &mut m.stages[0];
    let mut material = Structure::for_stage(s);
    material.component_shell = true;
    material.hardware_mass_kg = 2.0;
    s.materials = Some(material);
    let mut part = Part::new(PartKind::BodyTube, s);
    part.mass_override = true;
    part.mass_kg = 3.0;
    s.components.push(part);
    let mass: f64 = dry_pieces(s).iter().map(|p| p.0).sum();
    assert_eq!(mass, 5.0);
}

#[test]
fn moving_canopy_remains_finite_and_transmits_line_tension() {
    let mut m = Mission::demo(false);
    m.duration = 70.0;
    m.sample_period = 0.5;
    m.experiments.recovery = vec![RecoveryModel {
        stage: 0,
        main: false,
        canopy_mass_kg: 0.1,
        ..Default::default()
    }];
    let f = Simulator::new(m).unwrap().run();
    assert!(f.recovery_samples.len() > 20);
    assert!(
        f.recovery_samples
            .iter()
            .all(|p| p.tension_n.is_finite() && p.tension_n >= 0.0 && p.altitude_agl_m.is_finite())
    );
    assert!(f.recovery_samples.iter().any(|p| p.tension_n > 1.0));
    assert!(
        f.samples
            .iter()
            .all(|s| s.temperature_k.is_finite() && s.speed_mps.is_finite())
    );
}
