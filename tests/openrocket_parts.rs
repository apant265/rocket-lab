use rocket_lab::{
    assembly::*,
    config::{Mission, StageGeometry},
};
fn stage() -> rocket_lab::config::Stage {
    let mut s = Mission::demo(false).stages.remove(0);
    s.geometry = Some(StageGeometry::generic(&s, true));
    s
}
#[test]
fn ellipse_mass_and_cg_follow_the_actual_semielipse() {
    let s = stage();
    let p = Part::new(PartKind::EllipticalFin, &s);
    let g = p.shape.as_ref().unwrap();
    let area = std::f64::consts::PI * g.fin_root_chord_m * g.fin_span_m / 4.0;
    assert!((p.fin_area_cg().0 / area - 1.0).abs() < 0.001);
    assert!((p.cg() - p.position_m - g.fin_root_chord_m * 0.5).abs() < 1e-10);
    assert!(
        (p.mass() / (area * g.fin_thickness_m * g.fin_count as f64 * p.material.density_kg_m3)
            - 1.0)
            .abs()
            < 0.001
    );
}
#[test]
fn concave_freeform_triangulation_preserves_area_and_rejects_crossing() {
    let points = [[0.0, 0.0], [0.0, 1.0], [0.5, 0.4], [1.0, 1.0], [1.0, 0.0]];
    let triangles = triangulate_fin(&points).unwrap();
    assert_eq!(triangles.len(), 3);
    let area: f64 = triangles
        .iter()
        .map(|t| {
            let a = points[t[0]];
            let b = points[t[1]];
            let c = points[t[2]];
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5
        })
        .sum();
    assert!((area - 0.7).abs() < 1e-12);
    assert!(triangulate_fin(&[[0.0, 0.0], [1.0, 1.0], [0.0, 1.0], [1.0, 0.0]]).is_err());
}
#[test]
fn repeated_tube_fin_mass_and_radial_inertia_are_retained() {
    let mut s = stage();
    let mut p = Part::new(PartKind::TubeFin, &s);
    p.instance_count = 1;
    let single = p.mass();
    p.instance_count = 6;
    assert!((p.mass() - 6.0 * single).abs() < 1e-12);
    s.components.push(p.clone());
    validate(&s).unwrap();
    let pieces = dry_pieces(&s);
    let radial = pieces.last().unwrap().3;
    assert!(radial > p.radial_offset_m);
}
#[test]
fn new_part_types_round_trip_with_their_properties() {
    let mut s = stage();
    for kind in [
        PartKind::EllipticalFin,
        PartKind::FreeformFin,
        PartKind::TubeFin,
        PartKind::LaunchLug,
        PartKind::EngineBlock,
        PartKind::Streamer,
        PartKind::Parachute,
        PartKind::PodSet,
    ] {
        let mut p = Part::new(kind, &s);
        p.comments = "Measured part".into();
        p.finish = "Smooth".into();
        let text = serde_json::to_string(&p).unwrap();
        let copy: Part = serde_json::from_str(&text).unwrap();
        assert_eq!(copy.kind, kind);
        assert_eq!(copy.comments, p.comments);
        assert_eq!(copy.instance_count, p.instance_count);
        s.components.push(p);
        validate(&s).unwrap();
    }
}
#[test]
fn component_drag_and_finish_change_the_preliminary_table() {
    let mut m = Mission::demo(false);
    m.stages[0].geometry = Some(StageGeometry::generic(&m.stages[0], true));
    let mut p = Part::new(PartKind::BodyTube, &m.stages[0]);
    p.cd_override = Some(0.5);
    m.stages[0].components.push(p);
    rocket_lab::design::estimate_aero(&mut m);
    let with = m.stages[0].aero[0].cd_power_off;
    m.stages[0].components[0].cd_override = None;
    rocket_lab::design::estimate_aero(&mut m);
    let without = m.stages[0].aero[0].cd_power_off;
    assert!((with - without - 0.5 * 0.8_f64.powi(2)).abs() < 1e-12);
    m.stages[0].components[0].finish = "Polished".into();
    rocket_lab::design::estimate_aero(&mut m);
    assert!(m.stages[0].aero[0].cd_power_off < without);
}

#[test]
fn automatic_tube_fins_touch_the_body_and_neighbors() {
    let s = stage();
    let mut p = Part::new(PartKind::TubeFin, &s);
    for count in [3, 4, 6, 12] {
        p.instance_count = count;
        p.size_tube_fins(s.diameter_m);
        assert!((p.radial_offset_m - p.diameter_m * 0.5 - s.diameter_m * 0.5).abs() < 1e-12);
        let center_distance = 2.0 * p.radial_offset_m * (std::f64::consts::PI / count as f64).sin();
        assert!((center_distance - p.diameter_m).abs() < 1e-12);
    }
    p.auto_tube_diameter = false;
    p.diameter_m = 0.03;
    p.size_tube_fins(s.diameter_m);
    assert_eq!(p.diameter_m, 0.03);
}
