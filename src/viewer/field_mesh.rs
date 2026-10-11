//! Display tessellation of the exported triangles. This preserves geometry and
//! adds no simulation degrees of freedom. Both inspection views use this mesh.
use crate::{config::Mission, surface::MeshPart};
use bevy::prelude::Vec3;

pub(super) fn refined(part: &MeshPart, mission: &Mission) -> MeshPart {
    let mut out = part.clone();
    out.positions.clear();
    out.normals.clear();
    out.indices.clear();
    let stage = &mission.stages[part.stage];
    let axial_step = (stage.length_m / 256.).max(0.001);
    let radial_step = stage
        .geometry
        .as_ref()
        .map_or(1., |g| (g.fin_span_m / 64.).max(0.001));
    for indices in part.indices.chunks_exact(3) {
        let vertices = std::array::from_fn(|i| {
            let index = indices[i] as usize;
            (
                Vec3::from_array(part.positions[index]),
                part.normals
                    .get(index)
                    .copied()
                    .map(Vec3::from_array)
                    .unwrap_or(Vec3::Z),
            )
        });
        split(
            vertices,
            axial_step as f32,
            radial_step as f32,
            part.kind == "fin",
            0,
            &mut out,
        );
    }
    out
}
fn split(
    v: [(Vec3, Vec3); 3],
    axial: f32,
    radial: f32,
    fin: bool,
    depth: usize,
    out: &mut MeshPart,
) {
    let mut edge = 0;
    let mut longest = 0.;
    for i in 0..3 {
        let a = v[i].0;
        let b = v[(i + 1) % 3].0;
        let mut length = (a.y - b.y).abs() / axial;
        if fin {
            length = length.max((a.x.hypot(a.z) - b.x.hypot(b.z)).abs() / radial);
        }
        if length > longest {
            longest = length;
            edge = i;
        }
    }
    if longest > 1.01 && depth < 18 {
        let a = v[edge];
        let b = v[(edge + 1) % 3];
        let c = v[(edge + 2) % 3];
        let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1).normalize_or_zero());
        split([a, mid, c], axial, radial, fin, depth + 1, out);
        split([mid, b, c], axial, radial, fin, depth + 1, out);
    } else {
        for (p, n) in v {
            out.indices.push(out.positions.len() as u32);
            out.positions.push(p.to_array());
            out.normals.push(n.to_array());
        }
    }
}
