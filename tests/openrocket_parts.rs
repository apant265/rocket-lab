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

#[test]
fn transition_is_a_tapered_section_and_next_tube_matches_its_aft_end() {
    let mut m = Mission::demo(false);
    let s = &mut m.stages[0];
    s.geometry = Some(StageGeometry::generic(s, true));
    let original_length = s.length_m;
    let original_diameter = s.diameter_m;
    let transition = attach_transition(s, original_length);
    let p = &s.components[transition];
    assert_eq!(p.position_m, original_length);
    assert_eq!(p.fore_diameter_m, original_diameter);
    assert!(p.diameter_m < p.fore_diameter_m);
    assert!((s.length_m - original_length - p.length_m).abs() < 1e-10);
    let aft = p.diameter_m;
    let end = p.position_m + p.length_m;
    assert_eq!(tube_attachment(s, end), end);
    let body = attach_tube(s, end);
    assert_eq!(s.components[body].diameter_m, aft);
    assert_eq!(s.components[body].position_m, end);
    validate(s).unwrap();
    assert_eq!(m.stages.len(), 1);
    m.validate().unwrap();
}

#[test]
fn front_transition_does_not_hide_the_implicit_body_tail_attachment() {
    let mut s = stage();
    s.geometry.as_mut().unwrap().nose_length_m = 0.;
    let mut front = Part::new(PartKind::Transition, &s);
    front.position_m = 0.;
    front.diameter_m = s.diameter_m;
    s.components.push(front);
    assert_eq!(tube_attachment(&s, s.length_m - 0.001), s.length_m);
    assert_eq!(
        tube_attachment(&s, airframe_front_m(&s) + 0.001),
        airframe_front_m(&s)
    );
}

#[test]
fn inner_tube_rings_and_motor_follow_their_parent_and_survive_reindexing() {
    let mut s = stage();
    let inner = attach_internal(&mut s, PartKind::InnerTube, 0.7);
    let body = s.components[inner].parent_component.unwrap();
    let start = s.components[inner].position_m;
    let ring = attach_internal(&mut s, PartKind::CenteringRing, start);
    assert_eq!(s.components[ring].parent_component, Some(inner));
    let body_bore = s.components[body].diameter_m - 2. * s.components[body].thickness_m;
    assert_eq!(s.components[ring].diameter_m, body_bore);
    assert!(
        (s.components[ring].diameter_m
            - 2. * s.components[ring].thickness_m
            - s.components[inner].diameter_m)
            .abs()
            < 1e-12
    );
    s.components[inner].motor_mount = true;
    s.components[inner].motor_length_m = s.components[inner].length_m * 0.9;
    refit_internals(&mut s);
    let old_motor_cg = s.motor.cg_m;
    s.components[inner].position_m += 0.01;
    move_children(&mut s, inner, 0.01);
    s.components[inner].diameter_m *= 0.9;
    refit_internals(&mut s);
    assert!((s.components[ring].position_m - start - 0.01).abs() < 1e-12);
    assert!((s.motor.cg_m - old_motor_cg - 0.01).abs() < 1e-12);
    assert!(
        (s.components[ring].diameter_m
            - 2. * s.components[ring].thickness_m
            - s.components[inner].diameter_m)
            .abs()
            < 1e-12
    );
    validate(&s).unwrap();
    let tree = take_component_tree(&mut s, inner);
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[1].parent_component, Some(0));
    assert!(s.components.iter().all(|p| p.parent_component.is_none()));
    let saved = serde_json::to_string(&tree).unwrap();
    let restored: Vec<Part> = serde_json::from_str(&saved).unwrap();
    assert!(restored[0].motor_mount);
    assert!(restored[1].auto_ring_fit);
}

#[test]
fn invalid_mounts_and_parent_relationships_cannot_launch() {
    let mut s = stage();
    let inner = attach_internal(&mut s, PartKind::InnerTube, 0.7);
    let valid = s.clone();
    s.components[inner].parent_component = Some(inner);
    assert!(validate(&s).is_err());
    s = valid.clone();
    s.components[inner].motor_mount = true;
    s.components[inner].motor_length_m = s.length_m * 2.;
    assert!(validate(&s).is_err());
    s = valid;
    let parent = s.components[inner].parent_component.unwrap();
    remove_component(&mut s, parent);
    assert!(
        s.components
            .iter()
            .find(|p| p.kind == PartKind::InnerTube)
            .unwrap()
            .parent_component
            .is_none()
    );
}
