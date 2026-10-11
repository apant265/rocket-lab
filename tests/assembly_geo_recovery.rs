use glam::DVec3;
use rocket_lab::{
    assembly::{self, Part, PartKind, Structure},
    config::*,
    flight::*,
    geo,
};
#[test]
fn component_density_overrides_and_positions_feed_mass_cg_and_inertia() {
    let mut m = Mission::demo(false);
    let before = mass_properties(&m, 0, 0.0, 0.0);
    let mut part = Part::new(PartKind::Bulkhead, &m.stages[0]);
    part.position_m = 0.2;
    part.length_m = 0.01;
    part.diameter_m = 0.08;
    part.material.density_kg_m3 = 650.0;
    let expected = std::f64::consts::PI * 0.04_f64.powi(2) * 0.01 * 650.0;
    assert!((part.mass() - expected).abs() < 1e-12);
    let cg = part.cg();
    m.stages[0].components.push(part);
    m.validate().unwrap();
    let after = mass_properties(&m, 0, 0.0, 0.0);
    assert!((after.kg - before.kg - expected).abs() < 1e-10);
    assert!((after.cg_m - (before.kg * before.cg_m + expected * cg) / after.kg).abs() < 1e-10);
    assert!(after.inertia.z > before.inertia.z);
    let part = &mut m.stages[0].components[0];
    part.mass_override = true;
    part.mass_kg = 0.45359237;
    part.cg_override = true;
    part.cg_m = 0.002;
    assert_eq!(part.cg(), 0.202);
    let overridden = mass_properties(&m, 0, 0.0, 0.0);
    assert!((overridden.kg - before.kg - assembly::KG_PER_LB).abs() < 1e-10);
}
#[test]
fn material_model_replaces_measured_dry_mass_and_validates_dimensions() {
    let mut m = Mission::demo(false);
    let s = &mut m.stages[0];
    s.geometry = Some(StageGeometry::generic(s, true));
    let mut materials = Structure::for_stage(s);
    materials.use_material_mass = true;
    s.materials = Some(materials);
    let a = assembly::dry_pieces(s).iter().map(|p| p.0).sum::<f64>();
    s.materials.as_mut().unwrap().airframe.density_kg_m3 *= 2.0;
    let b = assembly::dry_pieces(s).iter().map(|p| p.0).sum::<f64>();
    assert!(b > a);
    m.validate().unwrap();
    m.stages[0].materials.as_mut().unwrap().airframe_wall_m = 1.0;
    assert!(m.validate().is_err());
}
#[test]
fn spherical_georeferencing_roundtrips_and_preserves_site_altitude() {
    let mut site = Mission::demo(false).launch;
    site.latitude_deg = 32.99;
    site.longitude_deg = -106.975;
    site.altitude_m = 1376.0;
    for enu in [
        DVec3::ZERO,
        DVec3::new(25000.0, 55000.0, 170000.0),
        DVec3::new(-10000.0, -2000.0, -35.0),
    ] {
        let c = geo::from_enu(&site, enu);
        assert!((geo::to_enu(&site, c) - enu).length() < 1e-7);
    }
    let c = geo::from_sample(&site, 20000.0, 55000.0, 0.0);
    assert!(c.latitude_deg > site.latitude_deg && c.longitude_deg > site.longitude_deg);
    assert!((c.altitude_m - site.altitude_m).abs() < 1e-7);
    site.longitude_deg = 181.0;
    let mut m = Mission::demo(false);
    m.launch = site;
    assert!(m.validate().is_err());
}
fn recovering_stage_mission() -> Mission {
    let mut m = Mission::demo(true);
    m.launch.elevation_deg = 90.0;
    m.launch.earth_rotation = false;
    m.wind.east_mps = 0.0;
    m.wind.north_mps = 0.0;
    m.wind.gust_mps = 0.0;
    m.duration = 1800.0;
    let recovery = m.recovery.clone();
    for s in &mut m.stages {
        let mut r = recovery.clone();
        r.main.max_load_n = 1000000.0;
        r.drogue.max_load_n = 1000000.0;
        s.recovery = Some(r);
    }
    m
}
#[test]
fn every_detached_stage_deploys_inflates_and_lands_under_its_own_main() {
    let r = Simulator::new(recovering_stage_mission()).unwrap().run();
    assert!(r.summary.completed);
    for stage in 0..r.mission.stages.len() - 1 {
        assert!(r.events.iter().any(|e| e.stage == stage
            && e.kind == "deployment"
            && e.detail.contains("Detached")
            && e.detail.contains("main")));
        assert!(
            r.debris_samples
                .iter()
                .any(|s| s.stage == stage && s.main_fraction > 0.1 && s.main_fraction < 0.99)
        );
        let last = r
            .debris_samples
            .iter()
            .rev()
            .find(|s| s.stage == stage)
            .unwrap();
        assert!(last.landed, "stage {stage}");
        assert!(last.landing_speed_mps < 15.0, "{}", last.landing_speed_mps);
        assert!(last.main_fraction > 0.99);
        assert_eq!(last.drogue_fraction, 0.0);
        assert!(
            r.debris_samples
                .iter()
                .filter(|s| s.stage == stage)
                .all(|s| s.mass_kg.is_finite() && s.chute_load_n.is_finite())
        );
    }
}
#[test]
fn detached_deployment_interlocks_and_line_failure_are_stage_specific() {
    let mut m = recovering_stage_mission();
    m.stages[0].recovery.as_mut().unwrap().main.max_q_pa = 0.000001;
    m.stages[0].recovery.as_mut().unwrap().drogue.max_q_pa = 0.000001;
    m.stages[1].recovery.as_mut().unwrap().drogue.max_load_n = 0.001;
    let r = Simulator::new(m).unwrap().run();
    assert!(
        r.events
            .iter()
            .any(|e| e.stage == 0 && e.kind == "deployment_deferred")
    );
    assert!(
        r.events
            .iter()
            .any(|e| e.stage == 1 && e.kind == "chute_failure" && e.detail.contains("Detached"))
    );
    assert!(
        r.debris_samples
            .iter()
            .filter(|s| s.stage == 0)
            .all(|s| s.main_fraction == 0.0 && s.drogue_fraction == 0.0)
    );
}
#[test]
fn user_payload_is_five_pounds_and_original_source_mass_is_preserved() {
    let source = Mission::load("examples/plumbbob-provisional.json").unwrap();
    let user = Mission::load("examples/kate-anvil-desert.json").unwrap();
    assert_eq!(user.stages.last().unwrap().payload_name, "Kate and Anvil");
    assert!(
        (user.stages.last().unwrap().payload_mass_kg / assembly::KG_PER_LB - 5.0).abs() < 1e-12
    );
    assert!(
        (mass_properties(&source, 0, 0.0, 0.0).kg - mass_properties(&user, 0, 0.0, 0.0).kg).abs()
            < 1e-10
    );
    assert!(
        (mass_properties(&source, 0, 0.0, 0.0).cg_m - mass_properties(&user, 0, 0.0, 0.0).cg_m)
            .abs()
            < 1e-10
    );
}
