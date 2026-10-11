use rocket_lab::{
    assembly::{self, Part, PartKind},
    cfd,
    config::Mission,
    surface::MeshDocument,
};

fn reducers() -> Mission {
    let mut m = Mission::demo(true);
    rocket_lab::design::initialize_geometry(&mut m);
    for stage in 0..2 {
        let mut p = Part::new(PartKind::Transition, &m.stages[stage]);
        p.position_m = 0.0;
        p.length_m = 0.4;
        p.diameter_m = m.stages[stage].diameter_m;
        p.fore_diameter_m = m.stages[stage + 1].diameter_m;
        m.stages[stage].components.push(p);
    }
    m
}

#[test]
fn front_reducers_taper_with_both_implicit_and_explicit_airframes() {
    let mut m = reducers();
    for explicit in [false, true] {
        if explicit {
            for s in &mut m.stages {
                assembly::explicit_shell(s);
            }
        }
        for active in 0..2 {
            for index in active..2 {
                let s = &m.stages[index];
                let p = s
                    .components
                    .iter()
                    .find(|p| p.kind == PartKind::Transition)
                    .unwrap();
                let offset: f64 = m.stages[index + 1..].iter().map(|s| s.length_m).sum();
                for u in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
                    let expected =
                        (p.fore_diameter_m + (p.diameter_m - p.fore_diameter_m) * u) * 0.5;
                    assert!(
                        (cfd::body_radius(&m, active, offset + p.length_m * u) - expected).abs()
                            < 1e-12
                    );
                }
                assert!(
                    (cfd::body_radius(&m, active, offset + p.length_m + 0.01) - s.diameter_m * 0.5)
                        .abs()
                        < 1e-12
                );
            }
        }
    }
}

#[test]
fn solver_mask_follows_the_cone_instead_of_a_full_width_rectangle() {
    let m = reducers();
    let p = cfd::prepare(
        &m,
        cfd::Request {
            stage: 0,
            altitude_m: 0.0,
            mach: 1.0,
            alpha_deg: 0.0,
            grid: [512, 256],
            steps: 1,
        },
    )
    .unwrap();
    let offset: f64 = m.stages[1..].iter().map(|s| s.length_m).sum();
    let fore = m.stages[1].diameter_m * 0.5;
    let aft = m.stages[0].diameter_m * 0.5;
    let mut cone_columns = 0;
    let mut removed_square_cells = 0;
    for i in 0..p.nx {
        let x = p.x0 + (i as f64 + 0.5) * p.dx;
        let u = (x - offset) / 0.4;
        if !(0.0..1.0).contains(&u) {
            continue;
        }
        cone_columns += 1;
        let radius = fore + (aft - fore) * u;
        for j in 0..p.ny {
            let y = (p.y0 + (j as f64 + 0.5) * p.dy).abs();
            assert_eq!(p.solid[j * p.nx + i], y <= radius);
            if y <= aft && y > radius {
                removed_square_cells += 1;
            }
        }
    }
    assert!(cone_columns >= 15);
    assert!(removed_square_cells >= 40);
}

#[test]
fn default_tapers_match_adjacent_diameters_and_preserve_assembled_dry_measurements() {
    let mut m = Mission::load("examples/kate-anvil-desert.json").unwrap();
    rocket_lab::design::initialize_geometry(&mut m);
    for (index, original_mass, original_cg) in [
        (0, 16.82850687200001, 1.1585073167972715),
        (1, 10.636511471, 0.9717069957766794),
    ] {
        let s = &m.stages[index];
        let p = s
            .components
            .iter()
            .find(|p| p.kind == PartKind::Transition)
            .unwrap();
        assert_eq!(p.position_m, 0.0);
        assert_eq!(p.fore_diameter_m, m.stages[index + 1].diameter_m);
        assert_eq!(p.diameter_m, s.diameter_m);
        let pieces = assembly::dry_pieces(s);
        let mass: f64 = pieces.iter().map(|p| p.0).sum();
        let cg = pieces.iter().map(|p| p.0 * p.1).sum::<f64>() / mass;
        assert!((mass - original_mass).abs() < 1e-10);
        assert!((cg - original_cg).abs() < 1e-10);
    }
}

#[test]
fn blender_transition_rings_match_the_cfd_cone_without_an_overlapping_cylinder() {
    let mut m = Mission::load("examples/kate-anvil-desert.json").unwrap();
    rocket_lab::design::initialize_geometry(&mut m);
    let model = MeshDocument::load("assets/models/kate-anvil/rocket.mesh.json", &m).unwrap();
    for index in 0..2 {
        let definition = &m.stages[index];
        let (component, transition) = definition
            .components
            .iter()
            .enumerate()
            .find(|(_, p)| p.kind == PartKind::Transition)
            .unwrap();
        let part = model
            .parts
            .iter()
            .find(|p| p.stage == index && p.component_index == Some(component) && p.kind == "body")
            .unwrap();
        let offset: f64 = m.stages[index + 1..].iter().map(|s| s.length_m).sum();
        let mut rings: Vec<(f64, f64)> = part
            .positions
            .iter()
            .map(|v| (-f64::from(v[1]), f64::from(v[0]).hypot(f64::from(v[2]))))
            .collect();
        rings.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.total_cmp(&a.1)));
        rings.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-7);
        assert!(rings.len() >= 60);
        for (x, radius) in rings {
            let u = ((x - offset) / transition.length_m).clamp(0.0, 1.0);
            let expected = (transition.fore_diameter_m
                + (transition.diameter_m - transition.fore_diameter_m) * u)
                * 0.5;
            assert!((radius - expected).abs() < 2e-6);
            assert!(
                (radius
                    - cfd::body_radius(
                        &m,
                        0,
                        x.clamp(offset + 1e-6, offset + transition.length_m - 1e-6)
                    ))
                .abs()
                    < 2e-6
            );
        }
        let body = model
            .parts
            .iter()
            .find(|p| p.stage == index && p.kind == "body" && p.component_index.is_none())
            .unwrap();
        assert!(
            body.positions
                .iter()
                .all(|v| -f64::from(v[1]) >= offset + transition.length_m - 1e-6)
        );
    }
}

#[test]
fn thermal_shell_resolves_reducer_material_and_sloped_surface_area() {
    let mut m = reducers();
    m.duration = 0.1;
    m.stages[0].components[0]
        .material
        .thermal
        .conductivity_w_m_k = 13.0;
    let transition = m.stages[0].components[0].clone();
    let f = rocket_lab::flight::Simulator::new(m).unwrap().run();
    let problem = rocket_lab::thermal::prepare_with_resolution(
        &f,
        rocket_lab::thermal::Resolution {
            axial_cells: 64,
            fin_cells: 2,
            max_step_s: 0.01,
        },
    );
    let columns: Vec<_> = problem
        .columns
        .iter()
        .filter(|c| c.stage == 0 && c.kind == "body" && c.station_m < transition.length_m)
        .collect();
    assert!(columns.len() >= 10);
    for c in columns {
        assert_eq!(c.material.thermal.conductivity_w_m_k, 13.0);
        let [a, b] = c.axial_bounds_m.unwrap();
        assert!(b <= transition.length_m + 1e-9);
        let slope =
            (transition.diameter_m - transition.fore_diameter_m) / (2.0 * transition.length_m);
        let radius = transition.fore_diameter_m * 0.5 + slope * c.station_m;
        let expected = std::f64::consts::TAU * radius * (b - a) * slope.hypot(1.0);
        assert!((c.area_m2 - expected).abs() < 1e-10);
    }
    assert!(
        !problem
            .columns
            .iter()
            .any(|c| c.stage == 0 && c.kind == transition.name)
    );
}
