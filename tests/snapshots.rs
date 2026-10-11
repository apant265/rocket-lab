use rocket_lab::{analysis, config::Mission, flight::Simulator, thermal};

fn flight() -> rocket_lab::flight::FlightResult {
    let mut m = Mission::load("examples/advanced-laboratory.json").unwrap();
    m.duration = 2.0;
    Simulator::new(m).unwrap().run()
}

#[test]
fn timestamp_interpolation_and_prefix_exclude_future_heating() {
    let mut f = flight();
    let mut a = f.samples[0].clone();
    a.time_s = 0.0;
    a.mach = 0.0;
    a.q_pa = 0.0;
    let mut b = a.clone();
    b.time_s = 1.0;
    b.mach = 2.0;
    b.q_pa = 2000.0;
    let mut c = b.clone();
    c.time_s = 2.0;
    c.mach = 8.0;
    f.samples = vec![a, b, c];
    f.summary.end_time_s = 2.0;
    let sample = analysis::sample_at(&f, 0.375);
    assert!((sample.mach - 0.75).abs() < 1e-12);
    assert!((sample.q_pa - 750.0).abs() < 1e-12);
    let prefix = analysis::prefix(&f, 0.375, 0.1);
    assert_eq!(prefix.samples.last().unwrap().time_s, 0.375);
    assert!(
        prefix
            .samples
            .windows(2)
            .all(|p| p[1].time_s - p[0].time_s <= 0.10000001)
    );
    let resolution = thermal::Resolution::default();
    let before = thermal::solve(&thermal::prepare_with_resolution(&prefix, resolution));
    f.samples[2].mach = 14.0;
    let after = thermal::solve(&thermal::prepare_with_resolution(
        &analysis::prefix(&f, 0.375, 0.1),
        resolution,
    ));
    assert_eq!(before.temperatures, after.temperatures);
}

#[test]
fn stage_discontinuity_does_not_smear_into_an_earlier_snapshot() {
    let mut f = flight();
    let mut a = f.samples[0].clone();
    a.time_s = 0.0;
    a.stage = 0;
    a.mass_kg = 100.0;
    let mut b = a.clone();
    b.time_s = 1.0;
    b.stage = 1;
    b.mass_kg = 20.0;
    f.samples = vec![a, b];
    f.summary.end_time_s = 1.0;
    assert_eq!(analysis::sample_at(&f, 0.99).stage, 0);
    assert_eq!(analysis::sample_at(&f, 0.99).mass_kg, 100.0);
    assert_eq!(analysis::sample_at(&f, 1.0).stage, 1);
}

#[test]
fn thermal_refinement_preserves_cylindrical_capacity_and_stability() {
    let mut f = flight();
    for s in &mut f.mission.stages {
        s.geometry.as_mut().unwrap().nose_length_m = 0.0;
        s.components.clear();
    }
    let coarse = thermal::prepare_with_resolution(
        &f,
        thermal::Resolution {
            axial_cells: 8,
            fin_cells: 2,
            max_step_s: 0.05,
        },
    );
    let fine = thermal::prepare_with_resolution(
        &f,
        thermal::Resolution {
            axial_cells: 32,
            fin_cells: 8,
            max_step_s: 0.0125,
        },
    );
    let capacity =
        |p: &thermal::Problem| p.columns.iter().map(|c| 2.0 * c.capacity_j_k).sum::<f64>();
    assert!((capacity(&coarse) / capacity(&fine) - 1.0).abs() < 1e-12);
    assert!(fine.max_step < coarse.max_step);
    for p in [&coarse, &fine] {
        for (i, c) in p.columns.iter().enumerate() {
            for &(j, g) in &c.links {
                assert!(
                    p.columns[j]
                        .links
                        .iter()
                        .any(|&(k, h)| k == i && (g - h).abs() < 1e-12)
                );
            }
        }
        assert!(
            thermal::solve(p)
                .temperatures
                .iter()
                .flatten()
                .flatten()
                .all(|t| t.is_finite() && *t > 0.0)
        );
    }
}

#[test]
fn refined_fin_beam_converges_to_uniform_cantilever_solution() {
    let mut f = flight();
    f.samples.last_mut().unwrap().stage = 0;
    f.samples.last_mut().unwrap().q_pa = 10000.0;
    f.samples.last_mut().unwrap().alpha_deg = 30.0;
    let g = f.mission.stages[0].geometry.as_mut().unwrap();
    g.fin_root_chord_m = 0.2;
    g.fin_tip_chord_m = 0.2;
    g.fin_span_m = 0.1;
    g.fin_thickness_m = 0.004;
    let law = rocket_lab::experiments::ThermalLaw {
        stage: 0,
        region: "fin".into(),
        through_k_w_m_k: 1.0,
        axial_k_w_m_k: 1.0,
        motor_heat_w: 0.0,
        motor_heat_until_s: 0.0,
        youngs_modulus_pa: 50e9,
        allowable_stress_pa: 300e6,
        ablation_temperature_k: 1000.0,
        heat_of_ablation_j_kg: 1e6,
    };
    f.mission
        .experiments
        .thermal
        .retain(|l| !(l.stage == 0 && l.region == "fin"));
    f.mission.experiments.thermal.push(law);
    let p = thermal::prepare(&f);
    let h = thermal::History {
        columns: p.columns,
        times: vec![0.0, f.summary.end_time_s],
        temperatures: vec![vec![[288.15; 2]; p.boundaries[0].len()]; 2],
        backend: "analytic test".into(),
    };
    let coarse = analysis::fin_stress(&f, &h, 16);
    let fine = analysis::fin_stress(&f, &h, 128);
    let load = 10000.0 * 1.2 * 0.5 * 0.2;
    let inertia = 0.2 * 0.004_f64.powi(3) / 12.0;
    let stress = load * 0.1_f64.powi(2) / 2.0 * 0.004 / 2.0 / inertia;
    let tip = load * 0.1_f64.powi(4) / (8.0 * 50e9 * inertia);
    let a = coarse
        .iter()
        .filter(|r| r.stage == 0)
        .next_back()
        .unwrap()
        .deflection_m
        .unwrap();
    let b = fine
        .iter()
        .filter(|r| r.stage == 0)
        .next_back()
        .unwrap()
        .deflection_m
        .unwrap();
    assert!((fine[0].stress_pa / stress - 1.0).abs() < 1e-12);
    assert!((b - tip).abs() < (a - tip).abs());
    assert!((b / tip - 1.0).abs() < 0.0001);
    f.mission.experiments.thermal.clear();
    assert!(
        analysis::fin_stress(&f, &h, 32)
            .iter()
            .all(|r| r.deflection_m.is_none() && r.margin.is_none())
    );
}
