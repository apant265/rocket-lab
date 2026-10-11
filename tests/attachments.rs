use rocket_lab::{assembly::*, config::Mission};
fn stage() -> rocket_lab::config::Stage {
    let mut s = Mission::demo(false).stages.remove(0);
    s.geometry = Some(rocket_lab::config::StageGeometry::generic(&s, true));
    s
}
#[test]
fn end_attachment_extends_the_stage_without_overlapping_original_tube() {
    let mut s = stage();
    let end = s.length_m;
    let i = attach_tube(&mut s, end);
    let p = &s.components[i];
    assert_eq!(p.position_m, end);
    assert!((s.length_m - end - p.length_m).abs() < 1e-12);
    let original = s
        .components
        .iter()
        .find(|p| p.name == "Original body tube")
        .unwrap();
    assert!((original.position_m + original.length_m - p.position_m).abs() < 1e-12);
    validate(&s).unwrap();
}
#[test]
fn forward_attachment_and_resize_shift_contents_without_scaling_them() {
    let mut s = stage();
    let front = airframe_front_m(&s);
    let old_cg = s.motor.cg_m;
    let i = attach_tube(&mut s, front);
    let size = s.components[i].length_m;
    assert!((s.motor.cg_m - old_cg - size).abs() < 1e-12);
    let original = s
        .components
        .iter()
        .find(|p| p.name == "Original body tube")
        .unwrap();
    assert!((original.position_m - front - size).abs() < 1e-12);
    let downstream = original.position_m;
    s.components[i].length_m += 0.2;
    shift_aft(&mut s, front + size, 0.2, Some(i));
    fit_shell(&mut s);
    assert!(
        (s.components
            .iter()
            .find(|p| p.name == "Original body tube")
            .unwrap()
            .position_m
            - downstream
            - 0.2)
            .abs()
            < 1e-12
    );
    validate(&s).unwrap();
}
#[test]
fn explicit_sections_preserve_both_measured_and_material_mass_and_cg() {
    for material in [false, true] {
        let mut s = stage();
        s.materials = Some(Structure::for_stage(&s));
        s.materials.as_mut().unwrap().use_material_mass = material;
        let totals = |s: &rocket_lab::config::Stage| {
            let p = dry_pieces(s);
            (
                p.iter().map(|p| p.0).sum::<f64>(),
                p.iter().map(|p| p.0 * p.1).sum::<f64>(),
            )
        };
        let before = totals(&s);
        explicit_shell(&mut s);
        let after = totals(&s);
        assert!((before.0 - after.0).abs() < 1e-9);
        assert!((before.1 - after.1).abs() < 1e-9);
    }
}
#[test]
fn missing_nose_attaches_above_body_and_repeated_drop_reuses_it() {
    let mut s = stage();
    s.geometry.as_mut().unwrap().nose_length_m = 0.0;
    let i = attach_nose(&mut s);
    assert_eq!(s.components[i].position_m, 0.0);
    let body = s
        .components
        .iter()
        .find(|p| p.kind == PartKind::BodyTube)
        .unwrap();
    assert!((body.position_m - s.components[i].length_m).abs() < 1e-12);
    assert_eq!(attach_nose(&mut s), i);
    assert_eq!(
        s.components
            .iter()
            .filter(|p| p.kind == PartKind::NoseCone)
            .count(),
        1
    );
    validate(&s).unwrap();
}
