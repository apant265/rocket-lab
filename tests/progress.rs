use rocket_lab::{cfd, config::Mission, flight::Simulator, thermal};

#[test]
fn flight_progress_observes_integration_without_changing_trajectory() {
    let mut mission = Mission::load("examples/advanced-laboratory.json").unwrap();
    mission.duration = 2.0;
    let reference = Simulator::new(mission.clone()).unwrap().run();
    let mut work = Vec::new();
    let observed = Simulator::new(mission)
        .unwrap()
        .run_with_progress(|done, total| work.push((done, total)));
    assert!(work.len() > 2);
    assert!(work.windows(2).all(|v| v[0].0 <= v[1].0));
    assert_eq!(work.last().unwrap().0, observed.summary.end_time_s);
    assert_eq!(
        serde_json::to_value(reference.samples).unwrap(),
        serde_json::to_value(observed.samples).unwrap()
    );
    assert_eq!(
        serde_json::to_value(reference.events).unwrap(),
        serde_json::to_value(observed.events).unwrap()
    );
}

#[test]
fn flow_progress_counts_completed_iterations_and_preserves_solution() {
    let mission = Mission::load("examples/advanced-laboratory.json").unwrap();
    let p = cfd::prepare(
        &mission,
        cfd::Request {
            stage: 0,
            altitude_m: 0.0,
            mach: 1.2,
            alpha_deg: 2.0,
            grid: [32, 32],
            steps: 100,
        },
    )
    .unwrap();
    let reference = cfd::solve(p.clone()).unwrap();
    let mut work = Vec::new();
    let observed = cfd::solve_with_progress(p, |done, total| work.push((done, total))).unwrap();
    assert!(work.windows(2).all(|v| v[0].0 < v[1].0));
    assert_eq!(work.last(), Some(&(100, 100)));
    assert_eq!(reference.cells, observed.cells);
    assert_eq!(reference.residuals, observed.residuals);
}

#[test]
fn thermal_progress_counts_completed_history_frames_and_preserves_temperatures() {
    let mut mission = Mission::load("examples/advanced-laboratory.json").unwrap();
    mission.duration = 2.0;
    let p = thermal::prepare(&Simulator::new(mission).unwrap().run());
    let reference = thermal::solve(&p);
    let mut work = Vec::new();
    let observed = thermal::solve_with_progress(&p, |done, total| work.push((done, total)));
    assert_eq!(work.len(), observed.times.len());
    assert_eq!(
        work.last(),
        Some(&(observed.times.len(), observed.times.len()))
    );
    assert!(work.windows(2).all(|v| v[0].0 < v[1].0));
    assert_eq!(reference.temperatures, observed.temperatures);
}
