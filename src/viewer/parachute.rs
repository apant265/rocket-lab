use super::*;
pub(super) fn mesh() -> Mesh {
    // Open hemispherical gore canopy, central vent, scalloped skirt and alternating panels.
    let rings = 20;
    let sectors = 128;
    let mut p = Vec::new();
    let mut n = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for j in 0..=rings {
        let theta = 0.085 + (std::f32::consts::FRAC_PI_2 - 0.085) * j as f32 / rings as f32;
        for i in 0..=sectors {
            let a = i as f32 * std::f32::consts::TAU / sectors as f32;
            let panel = (a * 16.0).cos();
            let radial = theta.sin() * (1.0 + 0.028 * panel * theta.sin());
            let y = theta.cos() * 0.72 - 0.04 * (1.0 - panel) * theta.sin().powi(8);
            p.push([radial * a.cos(), y, radial * a.sin()]);
            n.push(
                Vec3::new(
                    a.cos() * theta.sin(),
                    theta.cos() / 0.72,
                    a.sin() * theta.sin(),
                )
                .normalize()
                .to_array(),
            );
            let c = if i / 8 % 2 == 0 {
                Color::srgb(1.0, 0.36, 0.07)
            } else {
                Color::srgb(0.96, 0.94, 0.85)
            };
            colors.push(c.to_linear().to_f32_array());
        }
    }
    for j in 0..rings {
        for i in 0..sectors {
            let a = j * (sectors + 1) + i;
            let b = a + sectors + 1;
            indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, p);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

pub(super) fn streamer_mesh() -> Mesh {
    // A wavy ribbon with projected fabric area pi in unit coordinates. The scene
    // scales by sqrt(area/pi), as for the canopy; motion is illustrative, not FSI.
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for j in 0..=64 {
        let t = j as f32 / 64.0;
        for side in [-1.0, 1.0] {
            positions.push([
                side * 0.3 + (t * 18.0).sin() * 0.12,
                t * std::f32::consts::PI / 0.6,
                (t * 24.0).sin() * 0.15,
            ]);
            colors.push(Color::srgb(1.0, 0.38, 0.07).to_linear().to_f32_array());
        }
        if j < 64 {
            let a = j * 2;
            indices.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}
fn uses_streamer(lab: &Lab, stage: usize) -> bool {
    let mission = if lab.workspace == 0 {
        &lab.draft
    } else {
        &lab.flight.mission
    };
    let main = if lab.workspace == 0 {
        true
    } else if stage == mission.stages.len() - 1 {
        current(lab).0.main_fraction > 0.0
    } else {
        lab.flight
            .debris_samples
            .iter()
            .rev()
            .find(|s| s.stage == stage && s.time_s <= lab.time)
            .is_some_and(|s| s.main_fraction > 0.0)
    };
    mission.stages[stage]
        .components
        .iter()
        .rev()
        .find(|p| p.recovery.is_some() && p.deploy_at_apogee != main)
        .is_some_and(|p| p.kind == crate::assembly::PartKind::Streamer)
}
pub(super) fn update(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    lab: Res<Lab>,
    mut query: Query<(
        Entity,
        &mut Canopy,
        &mut Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let count = if lab.workspace == 0 {
        lab.draft.stages.len()
    } else {
        lab.flight.mission.stages.len()
    };
    if query.iter().count() == count {
        for (_, mut canopy, mut handle, _) in &mut query {
            let streamer = uses_streamer(&lab, canopy.stage);
            if streamer != canopy.streamer {
                meshes.remove(handle.0.id());
                handle.0 = meshes.add(if streamer { streamer_mesh() } else { mesh() });
                canopy.streamer = streamer;
            }
        }
        return;
    }
    for (entity, _, mesh, material) in &query {
        commands.entity(entity).despawn();
        meshes.remove(mesh.0.id());
        materials.remove(material.0.id());
    }
    let material = materials.add(StandardMaterial {
        double_sided: true,
        cull_mode: None,
        perceptual_roughness: 0.94,
        ..default()
    });
    for stage in 0..count {
        commands.spawn((
            Mesh3d(meshes.add(if uses_streamer(&lab, stage) {
                streamer_mesh()
            } else {
                mesh()
            })),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            Visibility::Hidden,
            Canopy {
                stage,
                streamer: uses_streamer(&lab, stage),
            },
        ));
    }
}
pub(super) fn scene(
    lab: &Lab,
    query: &mut CanopyQuery,
    gizmos: &mut Gizmos,
    scale: f32,
    position: Vec3,
    attitude: Quat,
    sample: &Telemetry,
) {
    for (canopy, mut transform, mut visibility) in query.iter_mut() {
        let mission = if lab.workspace == 0 {
            &lab.draft
        } else {
            &lab.flight.mission
        };
        let retained = canopy.stage == mission.stages.len() - 1;
        let stage = &mission.stages[canopy.stage];
        let mut attachment = position + attitude * Vec3::Y * sample.cg_m as f32 * scale;
        let mut fractions = (sample.drogue_fraction, sample.main_fraction);
        let recovery = if retained {
            Some(&mission.recovery)
        } else {
            stage.recovery.as_ref()
        };
        let mut velocity = Vec3::new(sample.vx as f32, sample.vz as f32, -sample.vy as f32);
        if !retained && lab.workspace == 1 {
            if let Some(d) = lab
                .flight
                .debris_samples
                .iter()
                .rev()
                .find(|s| s.stage == canopy.stage && s.time_s <= lab.time)
            {
                let p =
                    Vec3::new(d.east_m as f32, d.altitude_agl_m as f32, -d.north_m as f32) * 0.001;
                let mapping = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
                let q = mapping
                    * Quat::from_xyzw(d.qx as f32, d.qy as f32, d.qz as f32, d.qw as f32)
                    * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
                attachment = p + q * Vec3::Y * d.cg_m as f32 * scale;
                fractions = (d.drogue_fraction, d.main_fraction);
                velocity = Vec3::new(d.vx as f32, d.vz as f32, -d.vy as f32);
                if d.landed {
                    fractions = (0.0, 0.0);
                }
            } else {
                fractions = (0.0, 0.0);
            }
        }
        if lab.workspace == 0 && lab.preview_recovery {
            let offset: f32 = lab.draft.stages[canopy.stage + 1..]
                .iter()
                .map(|s| s.length_m as f32)
                .sum();
            attachment = position
                + attitude
                    * Vec3::Y
                    * (lab
                        .draft
                        .stages
                        .iter()
                        .map(|s| s.length_m as f32)
                        .sum::<f32>()
                        * 0.5
                        - offset
                        - stage.length_m as f32 * 0.4)
                    * scale
                + Vec3::X * (canopy.stage as f32 - lab.draft.stages.len() as f32 * 0.5 + 0.5) * 1.2;
            fractions = (0.0, 1.0);
            velocity = -Vec3::Y;
        }
        let fraction = fractions.0.max(fractions.1);
        if lab.workspace == 1
            && let Some(model) = lab.flight.mission.experiments.recovery.iter().find(|r| {
                r.stage == canopy.stage && r.main == (fractions.1 > 0.0) && r.canopy_mass_kg > 0.0
            })
        {
            let anchor_station = (stage.length_m * model.attachment_fraction) as f32;
            if retained {
                attachment -= attitude * Vec3::Y * anchor_station * scale;
            } else if let Some(d) = lab
                .flight
                .debris_samples
                .iter()
                .rev()
                .find(|d| d.stage == canopy.stage && d.time_s <= lab.time)
            {
                let mapping = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
                let q = mapping
                    * Quat::from_xyzw(d.qx as f32, d.qy as f32, d.qz as f32, d.qw as f32)
                    * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
                attachment -= q * Vec3::Y * anchor_station * scale;
            }
        }
        let Some(recovery) = recovery else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let chute = if fractions.1 > 0.0 {
            &recovery.main
        } else {
            &recovery.drogue
        };
        *visibility = if fraction > 0.0 && chute.enabled {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility == Visibility::Hidden {
            continue;
        }
        let radius = (chute.area_m2 as f32 * fraction as f32 / std::f32::consts::PI).sqrt() * scale;
        let up = if lab.workspace == 0 {
            Vec3::Y
        } else {
            (-velocity).normalize_or(Vec3::Y)
        };
        let mut rotation = Quat::from_rotation_arc(Vec3::Y, up);
        transform.translation = attachment + up * radius * 1.8;
        if lab.workspace == 1
            && let Some(p) = lab
                .flight
                .recovery_samples
                .iter()
                .rev()
                .find(|p| p.stage == canopy.stage && p.time_s <= lab.time)
        {
            let body = if canopy.stage == sample.stage {
                Vec3::new(
                    sample.east_m as f32,
                    sample.altitude_agl_m as f32,
                    -sample.north_m as f32,
                )
            } else {
                lab.flight
                    .debris_samples
                    .iter()
                    .rev()
                    .find(|d| d.stage == canopy.stage && d.time_s <= lab.time)
                    .map_or(Vec3::ZERO, |d| {
                        Vec3::new(d.east_m as f32, d.altitude_agl_m as f32, -d.north_m as f32)
                    })
            };
            let point = Vec3::new(p.east_m as f32, p.altitude_agl_m as f32, -p.north_m as f32);
            transform.translation = body * 0.001 + (point - body) * scale;
            rotation = Quat::from_rotation_arc(
                Vec3::Y,
                (transform.translation - attachment).normalize_or(up),
            );
        }
        transform.rotation = rotation;
        transform.scale = Vec3::splat(radius);
        if canopy.streamer {
            gizmos.line(
                attachment,
                transform.translation,
                Color::srgb(0.88, 0.88, 0.79),
            );
            continue;
        }
        for line in 0..16 {
            let a = line as f32 * std::f32::consts::TAU / 16.0;
            let skirt = transform.translation
                + rotation * Vec3::new(a.cos() * 1.028, 0.0, a.sin() * 1.028) * radius;
            gizmos.line(attachment, skirt, Color::srgb(0.88, 0.88, 0.79));
            let mut last = skirt;
            for j in 1..=12 {
                let theta = std::f32::consts::FRAC_PI_2
                    - (std::f32::consts::FRAC_PI_2 - 0.085) * j as f32 / 12.0;
                let next = transform.translation
                    + rotation
                        * Vec3::new(
                            theta.sin() * a.cos() * 1.028,
                            theta.cos() * 0.722,
                            theta.sin() * a.sin() * 1.028,
                        )
                        * radius;
                gizmos.line(last, next, Color::srgb(0.18, 0.18, 0.17));
                last = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    #[test]
    fn fabric_is_an_open_vented_canopy_with_outward_faces() {
        let m = mesh();
        let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        assert_eq!(p.len(), 21 * 129);
        assert!(p.iter().all(|v| v.iter().all(|v| v.is_finite())));
        assert!(p.iter().all(|v| v[0].hypot(v[2]) > 0.08));
        assert!(p.iter().any(|v| v[1] > 0.7));
        assert!(p.iter().any(|v| v[1] < -0.07));
        let Some(Indices::U32(indices)) = m.indices() else {
            panic!("indices")
        };
        for triangle in indices.chunks_exact(3) {
            let a = Vec3::from_array(p[triangle[0] as usize]);
            let b = Vec3::from_array(p[triangle[1] as usize]);
            let c = Vec3::from_array(p[triangle[2] as usize]);
            let n = (b - a).cross(c - a);
            let center = (a + b + c) / 3.0;
            assert!(n.dot(Vec3::new(center.x, center.y.max(0.0), center.z)) > 0.0);
        }
    }
    #[test]
    fn streamer_is_a_finite_open_ribbon_with_projected_recovery_area() {
        let m = streamer_mesh();
        let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!()
        };
        assert_eq!(p.len(), 130);
        assert!(p.iter().all(|v| v.iter().all(|x| x.is_finite())));
        let Some(Indices::U32(indices)) = m.indices() else {
            panic!()
        };
        let area: f32 = indices
            .chunks_exact(3)
            .map(|t| {
                let a = Vec3::from_array(p[t[0] as usize]);
                let b = Vec3::from_array(p[t[1] as usize]);
                let c = Vec3::from_array(p[t[2] as usize]);
                let cross = (b - a).cross(c - a);
                cross.z.abs() * 0.5
            })
            .sum();
        assert!((area - std::f32::consts::PI).abs() < 1e-5);
    }
}
