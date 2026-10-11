//! Keep enlarged vehicle pieces joined while their CG tracks use kilometre scale.
use super::*;

pub(super) const WORLD_SCALE: f32 = 0.001;

/// Vector from a body's CG to the original stack nose, in mesh coordinates.
pub(super) fn center_shift(
    mission: &Mission,
    stages: std::ops::Range<usize>,
    cg_m: f64,
    mass_kg: f64,
) -> Vec3 {
    let first = mission.stages[stages]
        .iter()
        .filter_map(|s| s.model_mass.as_ref())
        .fold(glam::DVec3::ZERO, |v, m| {
            v + glam::DVec3::from_array(m.centroid_m) * m.mass_kg
        });
    Vec3::new(
        (-first.x / mass_kg) as f32,
        cg_m as f32,
        (first.y / mass_kg) as f32,
    )
}

/// Accumulated CG displacements at separation, frozen in world orientation.
/// Multiplying these by (vehicle scale - trajectory scale) preserves each joint
/// without changing the flight tracks or rotating debris about the stack's CG.
pub(super) fn origin_offsets(flight: &FlightResult, active: usize) -> Vec<Vec3> {
    let count = flight.mission.stages.len();
    let mut offsets = vec![Vec3::ZERO; count];
    for event in flight
        .events
        .iter()
        .filter(|e| e.kind == "separation" && e.stage > 0 && e.stage <= active && e.stage < count)
    {
        let upper_index = flight.samples.partition_point(|s| s.time_s < event.time_s);
        let Some(upper) = flight
            .samples
            .get(upper_index)
            .filter(|s| s.stage == event.stage && (s.time_s - event.time_s).abs() < 1e-8)
        else {
            continue;
        };
        let debris_index = flight
            .debris_samples
            .partition_point(|s| s.time_s < event.time_s);
        let Some(debris) = flight.debris_samples[debris_index..]
            .iter()
            .take_while(|s| (s.time_s - event.time_s).abs() < 1e-8)
            .find(|s| s.stage + 1 == event.stage)
        else {
            continue;
        };
        let upper_center = center_shift(
            &flight.mission,
            event.stage..count,
            upper.cg_m,
            upper.mass_kg,
        );
        let nose_offset = flight.mission.stages[event.stage..]
            .iter()
            .map(|s| s.length_m)
            .sum::<f64>();
        let debris_center = center_shift(
            &flight.mission,
            debris.stage..debris.stage + 1,
            nose_offset + debris.cg_m,
            debris.mass_kg,
        );
        let combined = (upper_center * upper.mass_kg as f32
            + debris_center * debris.mass_kg as f32)
            / (upper.mass_kg + debris.mass_kg) as f32;
        let mapping = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        let rotation = mapping
            * Quat::from_xyzw(
                upper.qx as f32,
                upper.qy as f32,
                upper.qz as f32,
                upper.qw as f32,
            )
            * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let inherited = offsets[debris.stage];
        offsets[debris.stage] = inherited + rotation * (combined - debris_center);
        offsets[event.stage..].fill(inherited + rotation * (combined - upper_center));
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged_flight(off_axis: bool) -> FlightResult {
        let mut mission = Mission::demo(true);
        mission.duration = 18.0;
        mission.sample_period = 0.137;
        mission.launch.elevation_deg = 78.0;
        if off_axis {
            // Quenched propellant must remain in the booster CG frame.
            mission.stages[0].separation = crate::config::Trigger::Time { time_s: 2.037 };
            for stage in &mut mission.stages {
                stage.model_mass = Some(crate::model_physics::ModelMass {
                    mass_kg: stage.dry_mass_kg,
                    centroid_m: [0.003, -0.002, -stage.dry_cg_m],
                    inertia_kg_m2: glam::DMat3::from_diagonal(glam::DVec3::new(10.0, 10.0, 1.0))
                        .to_cols_array_2d(),
                    side_area_m2: [stage.length_m * stage.diameter_m; 2],
                    side_station_m: [stage.length_m * 0.5; 2],
                    generator: "staging regression fixture".into(),
                    mesh_checksum_fnv1a64: "fixture".into(),
                    input_fingerprint: crate::model_physics::fingerprint(stage),
                });
            }
        }
        Simulator::new(mission).unwrap().run()
    }

    #[test]
    fn both_separations_keep_the_joint_at_the_aft_end_of_the_retained_vehicle() {
        for off_axis in [false, true] {
            let flight = staged_flight(off_axis);
            let events: Vec<_> = flight
                .events
                .iter()
                .filter(|e| e.kind == "separation")
                .collect();
            assert_eq!(events.len(), 2);
            let total_length: f32 = flight
                .mission
                .stages
                .iter()
                .map(|s| s.length_m as f32)
                .sum();
            for event in events {
                let upper = flight
                    .samples
                    .iter()
                    .find(|s| s.time_s == event.time_s)
                    .unwrap();
                let debris = flight
                    .debris_samples
                    .iter()
                    .find(|s| s.time_s == event.time_s && s.stage + 1 == event.stage)
                    .unwrap();
                let origins = origin_offsets(&flight, upper.stage);
                let previous = origin_offsets(&flight, upper.stage - 1);
                let upper_length: f64 = flight.mission.stages[upper.stage..]
                    .iter()
                    .map(|s| s.length_m)
                    .sum();
                let upper_center = center_shift(
                    &flight.mission,
                    upper.stage..flight.mission.stages.len(),
                    upper.cg_m,
                    upper.mass_kg,
                );
                let debris_center = center_shift(
                    &flight.mission,
                    debris.stage..debris.stage + 1,
                    upper_length + debris.cg_m,
                    debris.mass_kg,
                );
                let rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
                    * Quat::from_xyzw(
                        upper.qx as f32,
                        upper.qy as f32,
                        upper.qz as f32,
                        upper.qw as f32,
                    )
                    * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
                let upper_position = world(upper);
                let debris_position = Vec3::new(
                    debris.east_m as f32,
                    debris.altitude_agl_m as f32,
                    -debris.north_m as f32,
                ) * WORLD_SCALE;
                let combined_position = (upper_position * upper.mass_kg as f32
                    + debris_position * debris.mass_kg as f32)
                    / (upper.mass_kg + debris.mass_kg) as f32;
                let combined_center = (upper_center * upper.mass_kg as f32
                    + debris_center * debris.mass_kg as f32)
                    / (upper.mass_kg + debris.mass_kg) as f32;
                let joint = -Vec3::Y * upper_length as f32;
                for scale in [WORLD_SCALE, 0.56 / total_length, 2.48 / total_length] {
                    let upper_joint = upper_position
                        + origins[upper.stage] * (scale - WORLD_SCALE)
                        + rotation * (joint + upper_center) * scale;
                    let debris_joint = debris_position
                        + origins[debris.stage] * (scale - WORLD_SCALE)
                        + rotation * (joint + debris_center) * scale;
                    let previous_joint = combined_position
                        + previous[debris.stage] * (scale - WORLD_SCALE)
                        + rotation * (joint + combined_center) * scale;
                    assert!(
                        upper_joint.distance(debris_joint) < 5e-6,
                        "S{}: joint gap {}, off-axis {off_axis}",
                        event.stage,
                        upper_joint.distance(debris_joint)
                    );
                    assert!(
                        upper_joint.distance(previous_joint) < 5e-6,
                        "Retained joint jumped at S{}",
                        event.stage
                    );
                    assert!(
                        debris_joint.distance(previous_joint) < 5e-6,
                        "Booster joint jumped at S{}",
                        event.stage
                    );
                    let upper_nose = upper_position
                        + origins[upper.stage] * (scale - WORLD_SCALE)
                        + rotation * upper_center * scale;
                    assert!(
                        (upper_nose.distance(upper_joint) - upper_length as f32 * scale).abs()
                            < 5e-6
                    );
                }
            }
        }
    }

    #[test]
    fn rewinding_undoes_stage_offsets_and_later_staging_preserves_the_first_booster() {
        let flight = staged_flight(false);
        assert!(origin_offsets(&flight, 0).iter().all(|v| *v == Vec3::ZERO));
        let first = origin_offsets(&flight, 1);
        let second = origin_offsets(&flight, 2);
        assert!(first[0].length() > 0.1);
        assert_eq!(first[1], first[2]);
        assert_eq!(first[0], second[0]);
        assert_ne!(first[1], second[1]);
        assert_ne!(first[2], second[2]);
    }
}
