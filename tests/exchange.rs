use rocket_lab::{
    assembly::PartKind,
    config::Mission,
    exchange::{self, Axis, Geometry, Units},
    surface::{MeshDocument, MeshPart, MeshStage},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn file(extension: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("rocket-lab-exchange-tests-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    root.join(format!(
        "{}.{extension}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn cube() -> Geometry {
    let positions = vec![
        [-0.05, -0.15, -0.05],
        [0.05, -0.15, -0.05],
        [0.05, 0.15, -0.05],
        [-0.05, 0.15, -0.05],
        [-0.05, -0.15, 0.05],
        [0.05, -0.15, 0.05],
        [0.05, 0.15, 0.05],
        [-0.05, 0.15, 0.05],
    ];
    let indices = vec![
        0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3,
        1, 2, 6, 1, 6, 5,
    ];
    Geometry::new("box.stl".into(), positions, indices).unwrap()
}

#[test]
fn hollow_geometry_uses_the_outer_triangle_envelope_between_vertex_rings() {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for sign in [-1., 1.] {
        for (top, bottom, r0, r1) in [
            (0.15, -0.15, 0.05, 0.02),
            (0.15, 0., 0.04, 0.025),
            (0., -0.15, 0.025, 0.01),
        ] {
            let start = positions.len() as u32;
            positions.extend([
                [sign * r0, top, -0.001],
                [sign * r0, top, 0.001],
                [sign * r1, bottom, 0.001],
                [sign * r1, bottom, -0.001],
            ]);
            indices.extend([start, start + 1, start + 2, start, start + 2, start + 3]);
        }
    }
    let g = Geometry::new("hollow taper".into(), positions, indices).unwrap();
    for fraction in [0.1, 0.25, 0.5, 0.75, 0.9] {
        let expected = (0.05_f64 - 0.03 * fraction).hypot(0.001);
        assert!(
            (g.radius(g.length_m * fraction, g.length_m, g.diameter_m) - expected).abs() < 1e-7
        );
    }
    g.validate().unwrap();
}
fn model(g: &Geometry) -> MeshDocument {
    MeshDocument {
        physics_version: 1,
        schema_version: 1,
        generator: "exchange fixture".into(),
        stages: vec![MeshStage {
            components: None,
            component_shell: false,
            length_m: 0.3,
            diameter_m: g.diameter_m,
            geometry: None,
        }],
        parts: vec![MeshPart {
            component_index: None,
            name: "test box".into(),
            stage: 0,
            kind: "body".into(),
            base_color: [0.4; 3],
            positions: g.positions.clone(),
            normals: vec![[1., 0., 0.]; g.positions.len()],
            indices: g.indices.clone(),
        }],
    }
}
#[test]
fn binary_stl_and_obj_roundtrip_preserve_units_axes_and_volume() {
    let g = cube();
    assert!(g.closed);
    assert!((g.volume_m3 - 0.003).abs() < 1e-8);
    for ext in ["stl", "obj"] {
        for units in Units::ALL {
            for axis in [Axis::X, Axis::Y, Axis::Z] {
                let path = file(ext);
                exchange::export(&model(&g), &path, units, axis, None, None, true).unwrap();
                let imported = exchange::import(&path, units, axis, 0.1).unwrap();
                assert!(imported.closed, "{ext} {units:?} {axis:?}");
                assert!((imported.volume_m3 - g.volume_m3).abs() < 1e-8);
                assert!((imported.length_m - g.length_m).abs() < 1e-6);
                assert!((imported.diameter_m - g.diameter_m).abs() < 1e-6);
            }
        }
    }
}
#[test]
fn stl_binary_header_can_begin_with_solid() {
    let path = file("stl");
    exchange::export(
        &model(&cube()),
        &path,
        Units::Meters,
        Axis::Y,
        None,
        None,
        true,
    )
    .unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[..5].copy_from_slice(b"solid");
    std::fs::write(&path, bytes).unwrap();
    assert!(
        exchange::import(&path, Units::Meters, Axis::Y, 0.1)
            .unwrap()
            .closed
    );
}
#[test]
fn ascii_stl_negative_obj_indices_and_concave_faces_are_supported() {
    let stl = file("stl");
    std::fs::write(&stl,"solid fin\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 20 0 0\nvertex 0 30 0\nendloop\nendfacet\nendsolid fin\n").unwrap();
    let g = exchange::import(&stl, Units::Millimeters, Axis::Y, 0.1).unwrap();
    assert!(!g.closed);
    assert!((g.length_m - 0.03).abs() < 1e-7);
    let obj = file("obj");
    std::fs::write(
        &obj,
        "v 0 0 0\nv 2 0 0\nv 2 1 0\nv 1 .4 0\nv 0 1 0\nf -5/1 -4/2 -3/3 -2/4 -1/5\n",
    )
    .unwrap();
    let g = exchange::import(&obj, Units::Centimeters, Axis::X, 0.1).unwrap();
    assert_eq!(g.indices.len(), 9);
    assert!(!g.closed);
    let area: f64 = g
        .indices
        .chunks_exact(3)
        .map(|f| {
            let [a, b, c] =
                [f[0], f[1], f[2]].map(|i| glam::Vec3::from_array(g.positions[i as usize]));
            ((b - a).cross(c - a).length() * 0.5) as f64
        })
        .sum();
    assert!((area - 0.00014).abs() < 1e-10);
}
#[test]
fn malformed_geometry_is_rejected_without_changing_the_design() {
    for content in [
        "v NaN 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3",
        "v 0 0 0\nf 0 1 2",
        "v 0 0 0\nv 0 0 0\nv 0 0 0\nf 1 2 3",
        "v 0 0 0\nf 1 2 3",
    ] {
        let path = file("obj");
        std::fs::write(&path, content).unwrap();
        assert!(exchange::import(&path, Units::Meters, Axis::Y, 0.1).is_err());
    }
    let path = file("stl");
    std::fs::write(&path, [0u8; 90]).unwrap();
    assert!(exchange::import(&path, Units::Meters, Axis::Z, 0.1).is_err());
    let mut m = Mission::demo(false);
    let before = serde_json::to_string(&m).unwrap();
    let mut huge = cube();
    for p in &mut huge.positions {
        p[1] *= 100.;
    }
    let huge = Geometry::new(huge.source, huge.positions, huge.indices).unwrap();
    assert!(exchange::insert(&mut m, 0, huge, false).is_err());
    assert_eq!(serde_json::to_string(&m).unwrap(), before);
    let mut stale = cube();
    stale.volume_m3 = 123.;
    assert!(stale.validate().is_err());
}
#[test]
fn embedded_import_survives_save_load_and_density_scaling() {
    let mut m = Mission::demo(false);
    let i = exchange::insert(&mut m, 0, cube(), false).unwrap();
    let p = &mut m.stages[0].components[i];
    assert_eq!(p.kind, PartKind::CustomMesh);
    let mass = p.mass();
    p.length_m *= 2.;
    p.diameter_m *= 2.;
    assert!((p.mass() / mass - 8.).abs() < 1e-6);
    let saved = file("json");
    m.save(&saved).unwrap();
    let loaded = Mission::load(saved).unwrap();
    loaded.validate().unwrap();
    assert_eq!(
        loaded.stages[0].components[i]
            .mesh
            .as_ref()
            .unwrap()
            .indices
            .len(),
        36
    );
}
#[test]
fn replacement_removes_implicit_shell_preserves_mass_and_maps_actual_cfd_envelope() {
    let mut m = Mission::demo(false);
    m.stages = vec![m.stages.last().unwrap().clone()];
    let old_motor = m.stages[0].motor.points.clone();
    let dry = m.stages[0].dry_mass_kg;
    exchange::insert(&mut m, 0, cube(), true).unwrap();
    m.validate().unwrap();
    assert!(m.stages[0].materials.as_ref().unwrap().component_shell);
    assert_eq!(m.stages[0].motor.points, old_motor);
    assert!(
        (rocket_lab::assembly::dry_pieces(&m.stages[0])
            .iter()
            .map(|p| p.0)
            .sum::<f64>()
            - dry)
            .abs()
            < 1e-8
    );
    let g = m.stages[0].components[0].mesh.as_ref().unwrap();
    assert!((rocket_lab::cfd::body_radius(&m, 0, 0.15) - g.diameter_m * 0.5).abs() < 1e-7);
    let mut mesh = model(g);
    mesh.stages[0].geometry = m.stages[0].geometry.clone();
    mesh.stages[0].component_shell = true;
    mesh.stages[0].components = Some(serde_json::to_value(&m.stages[0].components).unwrap());
    mesh.parts[0].component_index = Some(0);
    mesh.parts[0].kind = "custom_mesh".into();
    for p in &mut mesh.parts[0].positions {
        p[1] -= 0.15;
    }
    rocket_lab::model_physics::apply(&mesh, &mut m).unwrap();
    m.validate().unwrap();
    assert!(
        m.stages[0]
            .model_mass
            .as_ref()
            .unwrap()
            .inertia_kg_m2
            .iter()
            .flatten()
            .all(|v| v.is_finite())
    );
}
#[test]
fn open_import_requires_mass_override_and_exports_are_scoped() {
    let mut g = cube();
    g.indices.truncate(3);
    let g = Geometry::new(g.source, g.positions, g.indices).unwrap();
    let mut m = Mission::demo(false);
    let i = exchange::insert(&mut m, 0, g, false).unwrap();
    assert!(m.stages[0].components[i].mass_override);
    m.stages[0].components[i].mass_override = false;
    assert!(m.validate().is_err());
    let mut document = model(&cube());
    let mut fitting = document.parts[0].clone();
    fitting.kind = "nozzle".into();
    fitting.component_index = Some(2);
    document.parts.push(fitting);
    let path = file("stl");
    exchange::export(
        &document,
        &path,
        Units::Meters,
        Axis::Y,
        Some(0),
        None,
        false,
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap().len(), 84 + 12 * 50);
    exchange::export(
        &document,
        &path,
        Units::Meters,
        Axis::Y,
        Some(0),
        Some(2),
        true,
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap().len(), 84 + 12 * 50);
    assert!(
        exchange::export(
            &document,
            &path,
            Units::Meters,
            Axis::Y,
            None,
            Some(2),
            true
        )
        .is_err()
    );
}
