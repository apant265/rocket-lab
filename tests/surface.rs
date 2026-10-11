use glam::DVec3;
use rocket_lab::{
    config::Mission,
    flight::Simulator,
    surface::{MeshDocument, SurfaceConditions, stagnation_cp, streamline_point},
};
#[test]
fn stagnation_pressure_matches_perfect_gas_limits_and_sonic_continuity() {
    assert_eq!(stagnation_cp(0.0), 1.0);
    assert!((stagnation_cp(0.0001) - 1.0).abs() < 1e-7);
    let sonic = ((1.2_f64).powf(3.5) - 1.0) / 0.7;
    assert!((stagnation_cp(1.0) - sonic).abs() < 1e-12);
    assert!((stagnation_cp(1.0 - 1e-7) - stagnation_cp(1.0 + 1e-7)).abs() < 1e-6);
    // Rayleigh pitot/normal-shock limit for gamma=1.4.
    assert!((stagnation_cp(1e4) - 1.83937105113).abs() < 1e-7);
}
#[test]
fn incidence_and_pressure_bounds_are_preserved() {
    let c = SurfaceConditions {
        ambient_pa: 20000.0,
        q_pa: 30000.0,
        cp_max: stagnation_cp(5.0),
        upstream_model: DVec3::Y,
        relative_speed_mps: 1500.0,
    };
    assert_eq!(c.pressure([0.0, -1.0, 0.0]), 20000.0);
    assert_eq!(c.pressure([1.0, 0.0, 0.0]), 20000.0);
    assert_eq!(c.tangential_fraction([0.0, 1.0, 0.0]), 0.0);
    assert_eq!(c.tangential_fraction([1.0, 0.0, 0.0]), 1.0);
    assert!((c.tangential_fraction([0.6, 0.8, 0.0]) - 0.6).abs() < 1e-7);
    assert!((c.pressure([0.0, 1.0, 0.0]) - (20000.0 + 30000.0 * c.cp_max)).abs() < 1e-9);
    assert!((c.cp([0.6, 0.8, 0.0]) - c.cp_max * 0.64).abs() < 1e-7);
    for f in [0.0, 0.2, 0.5, 0.9, 1.0] {
        assert!((c.color_fraction(c.cp_at_color(f)) as f64 - f).abs() < 1e-7);
    }
    let still = SurfaceConditions { q_pa: 0.0, ..c };
    assert_eq!(still.pressure([0.0, 1.0, 0.0]), 20000.0);
    assert_eq!(still.color_fraction(c.cp_max), 0.0);
}
#[test]
fn bundled_blender_meshes_have_valid_normals_winding_and_dimensions() {
    for (path, mission) in [
        (
            "assets/models/rocket.mesh.json",
            "examples/plumbbob-provisional.json",
        ),
        ("assets/models/hpr/rocket.mesh.json", "examples/hpr.json"),
        (
            "assets/models/sounding/rocket.mesh.json",
            "examples/sounding.json",
        ),
    ] {
        let m = Mission::load(mission).unwrap();
        let model = MeshDocument::load(path, &m).unwrap();
        assert!(model.generator.contains("Blender"));
        let mut tested = 0;
        for part in &model.parts {
            for triangle in part.indices.chunks_exact(3) {
                let v = |i: u32| {
                    let p = part.positions[i as usize];
                    DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64)
                };
                let face = (v(triangle[1]) - v(triangle[0])).cross(v(triangle[2]) - v(triangle[0]));
                if face.length_squared() < 1e-16 {
                    continue;
                }
                let n = part.normals[triangle[0] as usize];
                assert!(
                    face.dot(DVec3::new(n[0] as f64, n[1] as f64, n[2] as f64)) >= -1e-10,
                    "Reversed face on {}",
                    part.name
                );
                tested += 1;
            }
        }
        assert!(tested > 10000);
        let mut changed = m.clone();
        changed.stages[0].diameter_m *= 1.1;
        assert!(model.validate(&changed).is_err());
        let mut invalid = model.clone();
        invalid.parts[0].indices[0] = u32::MAX;
        assert!(invalid.validate(&m).is_err());
        let mut invalid = model.clone();
        invalid.parts[0].normals[0] = [0.0; 3];
        assert!(invalid.validate(&m).is_err());
    }
}
#[test]
fn incidence_uses_flight_attitude_and_air_relative_velocity() {
    let m = Mission::load("examples/plumbbob-provisional.json").unwrap();
    let flight = Simulator::new(m.clone()).unwrap().run();
    let t = flight.samples.iter().find(|t| t.time_s >= 4.0).unwrap();
    let c = SurfaceConditions::from_sample(t, &m);
    assert!((c.q_pa - t.q_pa).abs() < 1e-7);
    let angle = c.upstream_model.y.clamp(-1.0, 1.0).acos().to_degrees();
    assert!((angle - t.alpha_deg).abs() < 1e-7);
}
#[test]
fn geometric_airflow_paths_clear_the_body_and_point_aft() {
    let m = Mission::load("examples/plumbbob-provisional.json").unwrap();
    let model = MeshDocument::load("assets/models/rocket.mesh.json", &m).unwrap();
    for stage in 0..m.stages.len() {
        let profile = model.body_profile(stage);
        assert!(profile.len() > 60);
        for p in profile.iter().skip(1).take(profile.len() - 2) {
            let v = streamline_point(&profile, p[0], 0.10, 0.3);
            assert!(v[0].hypot(v[2]) > p[1]);
            assert!((v[1] + p[0]).abs() < 1e-5);
        }
    }
}
