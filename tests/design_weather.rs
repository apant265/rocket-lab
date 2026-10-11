use rocket_lab::{
    atmosphere,
    config::{Mission, NoseShape, StageGeometry},
    design,
    flight::Simulator,
    surface::SurfaceConditions,
};
fn design_mission() -> Mission {
    let mut m = Mission::load("examples/hpr.json").unwrap();
    let top = m.stages.len() - 1;
    for (i, s) in m.stages.iter_mut().enumerate() {
        s.geometry = Some(StageGeometry::generic(s, i == top));
    }
    m
}
#[test]
fn measured_atmosphere_is_anchored_to_station_at_elevated_site() {
    let mut m = design_mission();
    m.launch.altitude_m = 2400.0;
    m.atmosphere.custom = true;
    m.atmosphere.temperature_c = 28.0;
    m.atmosphere.pressure_hpa = 740.0;
    let a = atmosphere::for_mission(2400.0, &m);
    assert!((a.temperature - 301.15).abs() < 1e-9);
    assert!((a.pressure - 74000.0).abs() < 1e-8);
    assert!((a.density - 74000.0 / (287.05287 * 301.15)).abs() < 1e-10);
    for geopotential in [
        11000.0, 20000.0, 32000.0, 47000.0, 51000.0, 71000.0, 84852.0,
    ] {
        let h = atmosphere::EARTH_RADIUS * geopotential / (atmosphere::EARTH_RADIUS - geopotential);
        let before = atmosphere::for_mission(h - 0.001, &m);
        let after = atmosphere::for_mission(h + 0.001, &m);
        assert!((before.pressure - after.pressure).abs() / before.pressure < 1e-6);
        assert!((before.temperature - after.temperature).abs() < 1e-4);
    }
}
#[test]
fn flight_and_surface_use_the_same_custom_weather_and_relative_wind() {
    let mut m = design_mission();
    m.duration = 12.0;
    m.atmosphere.custom = true;
    m.atmosphere.temperature_c = 30.0;
    m.atmosphere.pressure_hpa = 900.0;
    m.wind.east_mps = 5.0;
    m.wind.north_mps = -3.0;
    m.wind.gust_mps = 1.5;
    let result = Simulator::new(m.clone()).unwrap().run();
    for t in result.samples.iter().step_by(17) {
        let c = SurfaceConditions::from_sample(t, &m);
        let air = atmosphere::for_mission(t.altitude_m, &m);
        assert!((c.q_pa - t.q_pa).abs() < 1e-6);
        assert!((c.ambient_pa - air.pressure).abs() < 1e-8);
        assert!((c.q_pa - 0.5 * air.density * c.relative_speed_mps.powi(2)).abs() < 1e-6);
    }
}
#[test]
fn legacy_weather_defaults_preserve_standard_profile_and_bad_weather_rejected() {
    let mut m = Mission::load("examples/hpr.json").unwrap();
    assert!(!m.atmosphere.custom);
    for h in [0.0, 7000.0, 30000.0, 90000.0] {
        assert_eq!(
            atmosphere::sample(h).pressure,
            atmosphere::for_mission(h, &m).pressure
        );
    }
    m.atmosphere.pressure_hpa = 0.0;
    assert!(m.validate().is_err());
    m.atmosphere.pressure_hpa = 1013.25;
    m.atmosphere.temperature_c = f64::NAN;
    assert!(m.validate().is_err());
}
#[test]
fn fin_span_and_position_change_normal_force_and_center_of_pressure() {
    let mut m = design_mission();
    design::estimate_aero(&mut m);
    m.validate().unwrap();
    let base = m.stages[0].aero[0].clone();
    m.stages[0].geometry.as_mut().unwrap().fin_span_m *= 1.5;
    design::estimate_aero(&mut m);
    assert!(m.stages[0].aero[0].cn_alpha_per_rad > base.cn_alpha_per_rad);
    let cp = m.stages[0].aero[0].cp_m;
    m.stages[0].geometry.as_mut().unwrap().fin_position_m -= 0.15;
    design::estimate_aero(&mut m);
    assert!(m.stages[0].aero[0].cp_m < cp);
    assert!(m.stages[0].aero_source.contains("PRELIMINARY"));
}
#[test]
fn shape_profiles_and_nose_cp_match_analytic_low_mach_limits() {
    let mut m = design_mission();
    let nose = m.stages[0].geometry.as_ref().unwrap().nose_length_m;
    // Vanishing fins isolates the nose CP: conical 2L/3, ellipsoid L/3.
    m.stages[0].geometry.as_mut().unwrap().fin_span_m = 1e-8;
    design::estimate_aero(&mut m);
    assert!((m.stages[0].aero[0].cp_m / nose - 2.0 / 3.0).abs() < 1e-5);
    m.stages[0].geometry.as_mut().unwrap().nose_shape = NoseShape::Elliptical;
    design::estimate_aero(&mut m);
    assert!((m.stages[0].aero[0].cp_m / nose - 1.0 / 3.0).abs() < 1e-5);
    for shape in [
        NoseShape::Conical,
        NoseShape::TangentOgive,
        NoseShape::Elliptical,
    ] {
        assert!(design::nose_radius_fraction(shape, 0.0, 5.0).abs() < 1e-10);
        assert!((design::nose_radius_fraction(shape, 1.0, 5.0) - 1.0).abs() < 1e-10);
    }
    let encoded = serde_json::to_string(&m).unwrap();
    let decoded: Mission = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.stages[0].geometry, m.stages[0].geometry);
}
#[test]
fn each_staging_configuration_gets_a_nose_referenced_cp() {
    let mut m = design_mission();
    let mut booster = m.stages[0].clone();
    booster.geometry.as_mut().unwrap().nose_length_m = 0.0;
    m.stages.insert(0, booster);
    design::estimate_aero(&mut m);
    m.validate().unwrap();
    assert!(m.stages[0].aero[0].cp_m > m.stages[1].aero[0].cp_m);
    for (active, s) in m.stages.iter().enumerate() {
        assert!(
            s.aero.iter().all(|a| a.cp_m > 0.0
                && a.cp_m < m.stages[active..].iter().map(|s| s.length_m).sum::<f64>())
        );
    }
}
