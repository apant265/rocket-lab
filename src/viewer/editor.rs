//! Interactive assembly view. Draft edits update geometry immediately; the
//! applied flight and evaluated Blender surface remain separate until launch.
use super::*;
use crate::assembly::{Part, PartKind};
use builder::DropPart;

#[derive(Resource, Default)]
pub(super) struct Editor {
    pub textures: Vec<egui::TextureHandle>,
    pub category: usize,
    pub placement: Option<Placement>,
    pub armed: Option<DropPart>,
    pub palette_drag: bool,
    pick_meshes: Vec<PickMesh>,
    canopy_meshes: Vec<PickMesh>,
    canopy_signature: String,
    signature: String,
    ghost_signature: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Selection {
    pub stage: usize,
    pub builtin: usize,
    pub component: Option<usize>,
}
struct PickMesh {
    selection: Selection,
    internal: bool,
    min: Vec3,
    max: Vec3,
    triangles: Vec<[Vec3; 3]>,
}
impl PickMesh {
    fn new(
        mesh: &Mesh,
        translation: Vec3,
        scale: f32,
        selection: Selection,
        internal: bool,
    ) -> Self {
        Self::transformed(
            mesh,
            &Transform::from_translation(translation).with_scale(Vec3::splat(scale)),
            selection,
            internal,
        )
    }
    fn transformed(
        mesh: &Mesh,
        transform: &Transform,
        selection: Selection,
        internal: bool,
    ) -> Self {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("draft mesh requires positions")
        };
        let vertices: Vec<_> = positions
            .iter()
            .map(|p| transform.transform_point(Vec3::from_array(*p)))
            .collect();
        let indices: Vec<usize> = mesh
            .indices()
            .map(|i| i.iter().collect())
            .unwrap_or_else(|| (0..vertices.len()).collect());
        Self {
            selection,
            internal,
            min: vertices
                .iter()
                .copied()
                .fold(Vec3::splat(f32::INFINITY), Vec3::min),
            max: vertices
                .iter()
                .copied()
                .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max),
            triangles: indices
                .chunks_exact(3)
                .map(|t| [vertices[t[0]], vertices[t[1]], vertices[t[2]]])
                .collect(),
        }
    }
    fn hit(&self, origin: Vec3, direction: Vec3) -> Option<f32> {
        let mut near = 0.0_f32;
        let mut far = f32::INFINITY;
        for i in 0..3 {
            if direction[i].abs() < 1e-8 {
                if origin[i] < self.min[i] || origin[i] > self.max[i] {
                    return None;
                }
            } else {
                let a = (self.min[i] - origin[i]) / direction[i];
                let b = (self.max[i] - origin[i]) / direction[i];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
                if near > far {
                    return None;
                }
            }
        }
        self.triangles
            .iter()
            .filter_map(|&[a, b, c]| {
                let e1 = b - a;
                let e2 = c - a;
                let p = direction.cross(e2);
                let det = e1.dot(p);
                if det.abs() < 1e-9 {
                    return None;
                }
                let t = origin - a;
                let u = t.dot(p) / det;
                if !(0.0..=1.0).contains(&u) {
                    return None;
                }
                let q = t.cross(e1);
                let v = direction.dot(q) / det;
                if v < 0.0 || u + v > 1.0 {
                    return None;
                }
                let distance = e2.dot(q) / det;
                (distance >= 0.0).then_some(distance)
            })
            .min_by(f32::total_cmp)
    }
}
impl Editor {
    pub(super) fn pick(&self, origin: Vec3, direction: Vec3, reveal: bool) -> Option<Selection> {
        // Transparent shells let users select the visible contents through them.
        let nearest = |internal_only: bool| {
            self.pick_meshes
                .iter()
                .chain(self.canopy_meshes.iter())
                .filter(|p| (!p.internal || reveal) && (!internal_only || p.internal))
                .filter_map(|p| p.hit(origin, direction).map(|d| (d, p.selection)))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|p| p.1)
        };
        if reveal {
            nearest(true).or_else(|| nearest(false))
        } else {
            nearest(false)
        }
    }
}
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct SelectionBox;
pub(super) fn selection_style(mut configs: ResMut<GizmoConfigStore>) {
    let (config, _) = configs.config_mut::<SelectionBox>();
    config.line.width = 2.0;
    config.depth_bias = -1.0;
}
impl Editor {
    pub(super) fn selection_bounds(
        &self,
        selection: Selection,
        revealing: bool,
    ) -> Option<(Vec3, Vec3)> {
        // Recovery properties describe the deployed canopy when its preview is visible.
        let canopy = self.canopy_meshes.iter().find(|p| p.selection == selection);
        let mut bounds = canopy.map(|p| (p.min, p.max));
        if bounds.is_none() {
            for p in self.pick_meshes.iter().filter(|p| {
                p.selection == selection
                    && (!p.internal || revealing || selection.component.is_some())
            }) {
                bounds = Some(bounds.map_or((p.min, p.max), |(min, max)| {
                    (min.min(p.min), max.max(p.max))
                }));
            }
        }
        bounds.filter(|(min, max)| min.is_finite() && max.is_finite() && min.cmple(*max).all())
    }
}
pub(super) fn draw_selection(lab: Res<Lab>, editor: Res<Editor>, mut gizmos: Gizmos<SelectionBox>) {
    if lab.workspace != 0 {
        return;
    }
    let selection = Selection {
        stage: lab.selected_stage,
        builtin: if lab.selected_component.is_some() {
            0
        } else {
            lab.selected_part
        },
        component: lab.selected_component,
    };
    let Some((min, max)) =
        editor.selection_bounds(selection, lab.show_internals || editor.placement.is_some())
    else {
        return;
    };
    let pad = Vec3::splat(((max - min).max_element() * 0.025).clamp(0.006, 0.025));
    let min = min - pad;
    let max = max + pad;
    let corners: [Vec3; 8] = std::array::from_fn(|i| {
        Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        )
    });
    let accent = lab.theme.palette().accent;
    let color = Color::srgb_u8(accent.r(), accent.g(), accent.b());
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                gizmos.line(corners[i], corners[i | bit], color);
            }
        }
    }
}
pub(super) fn smoke_canopy_pick(editor: &Editor) -> Option<Selection> {
    let mesh = editor.canopy_meshes.last()?;
    let [a, b, c] = *mesh.triangles.first()?;
    editor.pick((a + b + c) / 3.0 + Vec3::Z * 5.0, -Vec3::Z, false)
}
pub(super) fn select(lab: &mut Lab, selection: Selection) {
    lab.selected_stage = selection.stage;
    lab.selected_part = selection.builtin;
    lab.selected_component = selection.component;
    lab.status = "Component selected · edit its properties on the right.".into();
}
#[derive(Clone, Copy)]
pub(super) struct Placement {
    pub payload: DropPart,
    pub stage: usize,
    pub station: f64,
}
#[derive(Component)]
pub(super) struct DraftPart {
    internal: bool,
    stage: usize,
    component: Option<usize>,
    builtin: usize,
}
#[derive(Component)]
pub(super) struct Ghost {
    position: Vec3,
    follow_station: bool,
}
pub(super) fn textures(editor: &mut Editor, ctx: &egui::Context) {
    if !editor.textures.is_empty() {
        return;
    }
    const PNGS: [&[u8]; 23] = [
        include_bytes!("../../assets/parts/00.png"),
        include_bytes!("../../assets/parts/01.png"),
        include_bytes!("../../assets/parts/02.png"),
        include_bytes!("../../assets/parts/03.png"),
        include_bytes!("../../assets/parts/04.png"),
        include_bytes!("../../assets/parts/05.png"),
        include_bytes!("../../assets/parts/06.png"),
        include_bytes!("../../assets/parts/07.png"),
        include_bytes!("../../assets/parts/08.png"),
        include_bytes!("../../assets/parts/09.png"),
        include_bytes!("../../assets/parts/10.png"),
        include_bytes!("../../assets/parts/11.png"),
        include_bytes!("../../assets/parts/12.png"),
        include_bytes!("../../assets/parts/13.png"),
        include_bytes!("../../assets/parts/14.png"),
        include_bytes!("../../assets/parts/15.png"),
        include_bytes!("../../assets/parts/16.png"),
        include_bytes!("../../assets/parts/17.png"),
        include_bytes!("../../assets/parts/18.png"),
        include_bytes!("../../assets/parts/19.png"),
        include_bytes!("../../assets/parts/20.png"),
        include_bytes!("../../assets/parts/21.png"),
        include_bytes!("../../assets/parts/22.png"),
    ];
    for (i, bytes) in PNGS.iter().enumerate() {
        let rgba = image::load_from_memory(bytes)
            .expect("bundled Blender preview")
            .into_rgba8();
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        editor.textures.push(ctx.load_texture(
            format!("part_model_{i}"),
            image,
            egui::TextureOptions::LINEAR,
        ));
    }
}
pub(super) fn dimensions(lab: &Lab) -> (f32, f32) {
    let length = lab
        .draft
        .stages
        .iter()
        .map(|s| s.length_m as f32)
        .sum::<f32>();
    (length, 2.48 / length)
}
/// Closest approach of the cursor ray to the assembly axis. Reject edge-on
/// views and rays behind the camera; placement is independent of screen Y.
pub(super) fn ray_station(origin: Vec3, direction: Vec3, nose: Vec3, scale: f32) -> Option<f64> {
    let axis = -Vec3::Y;
    let w = origin - nose;
    let b = direction.dot(axis);
    let denominator = 1.0 - b * b;
    if denominator < 1e-4 {
        return None;
    }
    let along = (axis.dot(w) - b * direction.dot(w)) / denominator;
    let distance = (b * axis.dot(w) - direction.dot(w)) / denominator;
    if distance < 0.0 {
        return None;
    }
    Some((along / scale) as f64)
}
pub(super) fn locate(draft: &Mission, payload: DropPart, station: f64) -> Placement {
    let total = draft.stages.iter().map(|s| s.length_m).sum::<f64>();
    let station = station.clamp(0.0, total);
    let mut offset = 0.0;
    let mut selected = 0;
    let exterior_kind = match payload {
        DropPart::Existing { stage, component } => {
            Some(draft.stages[stage].components[component].kind)
        }
        DropPart::Extra(kind) => Some(kind),
        DropPart::Builtin(0) => Some(PartKind::BodyTube),
        DropPart::Builtin(1) => Some(PartKind::NoseCone),
        _ => None,
    };
    for (i, s) in draft.stages.iter().enumerate().rev() {
        selected = i;
        if station <= offset + s.length_m {
            break;
        }
        offset += s.length_m;
    }
    if exterior_kind == Some(PartKind::NoseCone) {
        selected = draft.stages.len() - 1;
        offset = 0.0;
    }
    let s = &draft.stages[selected];
    if matches!(
        exterior_kind,
        Some(PartKind::BodyTube | PartKind::Transition)
    ) {
        return Placement {
            payload,
            stage: selected,
            station: crate::assembly::tube_attachment(s, station - offset),
        };
    }
    if exterior_kind == Some(PartKind::NoseCone) {
        return Placement {
            payload,
            stage: selected,
            station: 0.0,
        };
    }
    let length = match payload {
        DropPart::Extra(kind) => Part::new(kind, s).length_m,
        DropPart::Existing { stage, component } => draft.stages[stage].components[component]
            .length_m
            .min(s.length_m * 0.8),
        _ => 0.0,
    };
    let station = ((station - offset - length * 0.5) / 0.01).round() * 0.01;
    Placement {
        payload,
        stage: selected,
        station: station.clamp(0.0, s.length_m - length),
    }
}
pub(super) struct Piece {
    pub(super) mesh: Mesh,
    pub(super) position: Vec3,
    pub(super) color: Color,
    pub(super) internal: bool,
    component: Option<usize>,
    builtin: usize,
}
fn piece(mesh: Mesh, station: f32, color: Color, internal: bool) -> Piece {
    Piece {
        mesh,
        position: Vec3::new(0.0, -station, 0.0),
        color,
        internal,
        component: None,
        builtin: 0,
    }
}
fn lathe(rings: &[(f32, f32)], inner: bool) -> Mesh {
    let n = 64;
    let mut positions = vec![];
    let mut normals = vec![];
    let mut indices = vec![];
    for (j, &(y, r)) in rings.iter().enumerate() {
        let (dy, dr) = if j + 1 < rings.len() {
            (rings[j + 1].0 - y, rings[j + 1].1 - r)
        } else {
            (y - rings[j - 1].0, r - rings[j - 1].1)
        };
        for i in 0..=n {
            let a = i as f32 * std::f32::consts::TAU / n as f32;
            positions.push([r * a.cos(), y, r * a.sin()]);
            normals.push(
                Vec3::new(dy.abs() * a.cos(), -dr * dy.signum(), dy.abs() * a.sin())
                    .normalize_or_zero()
                    .to_array(),
            );
        }
    }
    for j in 0..rings.len() - 1 {
        for i in 0..n {
            let a = (j * (n + 1) + i) as u32;
            let b = a + 1;
            let c = a + (n + 1) as u32;
            indices.extend(if inner {
                [a, c, b, b, c, c + 1]
            } else {
                [a, b, c, b, c + 1, c]
            });
        }
    }
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    m.insert_indices(Indices::U32(indices));
    m
}
fn tube(radius: f32, length: f32, wall: f32) -> Mesh {
    let ri = (radius - wall).max(0.0001);
    let h = length * 0.5;
    // A closed annular profile, including open bore and both annular end faces.
    let mut m = lathe(
        &[(h, radius), (-h, radius), (-h, ri), (h, ri), (h, radius)],
        false,
    );
    // Double-sided materials also show the bore when inspecting a tube from above.
    m.compute_area_weighted_normals();
    m
}
fn extra(p: &Part) -> Piece {
    let r = p.diameter_m as f32 * 0.5;
    let h = p.length_m as f32;
    let color = Color::srgba(
        p.appearance_rgba[0],
        p.appearance_rgba[1],
        p.appearance_rgba[2],
        p.appearance_rgba[3],
    );
    let mesh = match p.kind {
        PartKind::CustomMesh => {
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            );
            if let Some(source) = &p.mesh {
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_POSITION,
                    source.scaled_positions(p.length_m, p.diameter_m),
                );
                mesh.insert_indices(Indices::U32(source.indices.clone()));
                mesh.compute_area_weighted_normals();
            }
            mesh
        }
        PartKind::BodyTube
        | PartKind::InnerTube
        | PartKind::Coupler
        | PartKind::CenteringRing
        | PartKind::EngineBlock
        | PartKind::LaunchLug
        | PartKind::TubeFin
        | PartKind::PodSet => tube(r, h, p.thickness_m as f32),
        PartKind::NoseCone | PartKind::Transition => {
            let fore = if p.kind == PartKind::NoseCone {
                0.0
            } else {
                p.fore_diameter_m as f32 * 0.5
            };
            let profile: Vec<_> = (0..=32)
                .map(|i| {
                    let u = i as f32 / 32.0;
                    let radius = if p.kind == PartKind::NoseCone {
                        r * crate::design::nose_radius_fraction(
                            p.shape
                                .as_ref()
                                .map_or(crate::config::NoseShape::Conical, |g| g.nose_shape),
                            u as f64,
                            h as f64 / r as f64,
                        ) as f32
                    } else {
                        fore + (r - fore) * u
                    };
                    (h * 0.5 - h * u, radius)
                })
                .collect();
            lathe(&profile, true)
        }
        PartKind::FinSet | PartKind::EllipticalFin | PartKind::FreeformFin => outline_fin_mesh(p),
        PartKind::Avionics | PartKind::Streamer => Cuboid::new(r * 1.4, h, r * 0.3).mesh().build(),
        PartKind::ShockCord => Torus::new(r * 0.65, r).mesh().build(),
        _ => Cylinder::new(r, h).mesh().build(),
    };
    let mut result = piece(
        mesh,
        (p.position_m + p.length_m * 0.5) as f32,
        color,
        !p.external(),
    );
    if p.kind.planar_fin() {
        result.position.y = -p.position_m as f32;
    }
    let angle = p.radial_angle_deg.to_radians() as f32;
    if !p.kind.planar_fin() {
        result.position.x = p.radial_offset_m as f32 * angle.cos();
        result.position.z = p.radial_offset_m as f32 * angle.sin();
    }
    result
}
fn extra_instances(p: &Part) -> Vec<Piece> {
    (0..p.instance_count)
        .map(|i| {
            let mut part = p.clone();
            part.radial_angle_deg += 360.0 * i as f64 / p.instance_count as f64;
            extra(&part)
        })
        .collect()
}
fn outline_fin_mesh(p: &Part) -> Mesh {
    let points = p.fin_outline();
    let triangles = crate::assembly::triangulate_fin(&points).unwrap_or_default();
    let Some(g) = &p.shape else {
        return Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
    };
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let n = points.len() as u32;
    let clockwise = (0..points.len())
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        < 0.0;
    for fin in 0..g.fin_count {
        let a = (p.radial_angle_deg.to_radians()
            + fin as f64 * std::f64::consts::TAU / g.fin_count as f64) as f32;
        let base = positions.len() as u32;
        for side in [-0.5, 0.5] {
            for point in &points {
                let r = (p.diameter_m * 0.5 + p.radial_offset_m + point[1]) as f32;
                let z = (g.fin_thickness_m * side) as f32;
                positions.push([
                    r * a.cos() - z * a.sin(),
                    -point[0] as f32,
                    r * a.sin() + z * a.cos(),
                ]);
            }
        }
        for tri in &triangles {
            let face = if clockwise {
                *tri
            } else {
                [tri[2], tri[1], tri[0]]
            };
            indices.extend(face.map(|i| base + i as u32));
            indices.extend(face.iter().rev().map(|&i| base + n + i as u32));
        }
        for i in 0..n {
            let j = (i + 1) % n;
            let triangles = if clockwise {
                [[i, n + j, j], [i, n + i, n + j]]
            } else {
                [[i, j, n + j], [i, n + j, n + i]]
            };
            for tri in triangles {
                indices.extend(tri.map(|v| base + v));
            }
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_area_weighted_normals();
    mesh
}
fn nose_mesh(s: &crate::config::Stage) -> Mesh {
    let g = s.geometry.as_ref().unwrap();
    let r = s.diameter_m as f32 * 0.5;
    let l = g.nose_length_m as f32;
    let rings = (0..=48)
        .map(|i| {
            let u = i as f32 / 48.;
            let radius = match g.nose_shape {
                crate::config::NoseShape::Conical => r * u,
                crate::config::NoseShape::Elliptical => r * (1.0 - (1.0 - u).powi(2)).sqrt(),
                crate::config::NoseShape::TangentOgive => {
                    let rho = (r * r + l * l) / (2. * r);
                    (rho * rho - (l - l * u).powi(2)).max(0.).sqrt() + r - rho
                }
            };
            (-l * u, radius.max(0.00001))
        })
        .collect::<Vec<_>>();
    lathe(&rings, false)
}
fn fin_mesh(s: &crate::config::Stage) -> Mesh {
    let g = s.geometry.as_ref().unwrap();
    let r = s.diameter_m as f32 * 0.5;
    let mut pos = vec![];
    let mut indices = vec![];
    for i in 0..g.fin_count {
        let a = i as f32 * std::f32::consts::TAU / g.fin_count as f32;
        let base = pos.len() as u32;
        for z in [
            -g.fin_thickness_m as f32 * 0.5,
            g.fin_thickness_m as f32 * 0.5,
        ] {
            for (x, y) in [
                (r, 0.),
                (r + g.fin_span_m as f32, -g.fin_sweep_m as f32),
                (
                    r + g.fin_span_m as f32,
                    -(g.fin_sweep_m + g.fin_tip_chord_m) as f32,
                ),
                (r, -g.fin_root_chord_m as f32),
            ] {
                pos.push([
                    x * a.cos() - z * a.sin(),
                    y - g.fin_position_m as f32,
                    x * a.sin() + z * a.cos(),
                ]);
            }
        }
        for triangle in [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ] {
            indices.extend(triangle.map(|j| base + j));
        }
    }
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(indices));
    m.compute_area_weighted_normals();
    m
}
pub(super) fn body_front(s: &crate::config::Stage) -> f64 {
    crate::assembly::airframe_front_m(s)
}
type CanopyPickQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Canopy,
        &'static Transform,
        &'static Visibility,
        &'static Mesh3d,
    ),
    (Without<Ghost>, Without<DraftPart>),
>;
fn builtin(s: &crate::config::Stage, index: usize) -> Vec<Piece> {
    let g = s.geometry.as_ref().unwrap();
    let r = s.diameter_m as f32 * 0.5;
    let l = s.length_m as f32;
    let front = body_front(s) as f32;
    let pieces = match index {
        0 => vec![piece(
            tube(
                r,
                l - front,
                s.materials.as_ref().unwrap().airframe_wall_m as f32,
            ),
            (l + front) * 0.5,
            Color::srgb(0.74, 0.81, 0.85),
            false,
        )],
        1 => {
            if g.nose_length_m > 0. {
                vec![piece(nose_mesh(s), 0., Color::srgb(0.4, 0.55, 0.63), false)]
            } else {
                vec![]
            }
        }
        2 => vec![piece(fin_mesh(s), 0., Color::srgb(0.25, 0.38, 0.45), false)],
        3 => vec![piece(
            Cylinder::new(
                s.components
                    .iter()
                    .find(|p| p.motor_mount)
                    .map_or(r * 0.65, |p| {
                        if p.motor_diameter_m > 0. {
                            p.motor_diameter_m as f32 * 0.5
                        } else {
                            (p.diameter_m - 2. * p.thickness_m) as f32 * 0.45
                        }
                    }),
                s.components
                    .iter()
                    .find(|p| p.motor_mount)
                    .map_or(l * 0.24, |p| p.motor_length_m as f32),
            )
            .mesh()
            .build(),
            s.motor.cg_m as f32,
            Color::srgb(0.51, 0.58, 0.65),
            true,
        )],
        4 => vec![piece(
            Cuboid::new(r * 1.3, l * 0.08, r * 0.4).mesh().build(),
            s.payload_cg_m as f32,
            Color::srgb(0.04, 0.72, 0.42),
            true,
        )],
        5 => vec![piece(
            Cylinder::new(r * 0.6, l * 0.10).mesh().build(),
            l * 0.25,
            Color::srgb(0.85, 0.40, 0.12),
            true,
        )],
        _ => vec![],
    };
    pieces
        .into_iter()
        .map(|mut p| {
            p.builtin = index;
            p
        })
        .collect()
}
/// Use the builder's actual parts for flight previews without an exported mesh.
pub(super) fn flight_pieces(stage: &crate::config::Stage) -> Vec<Piece> {
    let explicit = stage.materials.as_ref().is_some_and(|m| m.component_shell);
    let mut pieces: Vec<_> = if explicit {
        Vec::new()
    } else {
        (0..=2).flat_map(|i| builtin(stage, i)).collect()
    };
    pieces.extend(stage.components.iter().flat_map(extra_instances));
    pieces.retain(|p| !p.internal);
    pieces
}
#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    mut editor: ResMut<Editor>,
    lab: Res<Lab>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut parts: Query<(
        Entity,
        &DraftPart,
        &mut Visibility,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    canopies: CanopyPickQuery<'_, '_>,
    mut ghosts: Query<(
        Entity,
        &Ghost,
        &mut Transform,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let signature = serde_json::to_string(&lab.draft.stages).unwrap_or_default();
    let (length, scale) = dimensions(&lab);
    if signature != editor.signature {
        for (e, _, _, m, mat) in &parts {
            commands.entity(e).despawn();
            meshes.remove(m.0.id());
            materials.remove(mat.0.id());
        }
        editor.pick_meshes.clear();
        let mut offset = 0.;
        for (stage, s) in lab.draft.stages.iter().enumerate().rev() {
            let component_shell = s.materials.as_ref().is_some_and(|m| m.component_shell);
            let mut pieces = (0..=5)
                .filter(|i| !component_shell || *i >= 3)
                .flat_map(|i| builtin(s, i))
                .collect::<Vec<_>>();
            if s.payload_mass_kg == 0. {
                pieces.retain(|p| p.color != Color::srgb(0.04, 0.72, 0.42));
            }
            for (i, p) in s.components.iter().enumerate() {
                for mut v in extra_instances(p) {
                    v.component = Some(i);
                    pieces.push(v);
                }
            }
            for p in pieces {
                let translation = (p.position + Vec3::Y * (length * 0.5 - offset)) * scale;
                editor.pick_meshes.push(PickMesh::new(
                    &p.mesh,
                    translation,
                    scale,
                    Selection {
                        stage,
                        builtin: p.builtin,
                        component: p.component,
                    },
                    p.internal,
                ));
                let alpha = if p.internal { 1.0 } else { 0.92 };
                commands.spawn((
                    Mesh3d(meshes.add(p.mesh)),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: p.color.with_alpha(alpha),
                        double_sided: true,
                        cull_mode: None,
                        ..default()
                    })),
                    Transform::from_translation(
                        (p.position + Vec3::Y * (length * 0.5 - offset)) * scale,
                    )
                    .with_scale(Vec3::splat(scale)),
                    DraftPart {
                        internal: p.internal,
                        stage,
                        component: p.component,
                        builtin: p.builtin,
                    },
                    Visibility::Hidden,
                ));
            }
            offset += s.length_m as f32;
        }
        editor.signature = signature;
    }
    let canopy_signature = if lab.workspace == 0 && lab.preview_recovery {
        format!(
            "{:?}",
            canopies
                .iter()
                .map(|(c, t, v, _)| (c.stage, t.translation, t.scale, *v))
                .collect::<Vec<_>>()
        )
    } else {
        String::new()
    };
    if canopy_signature != editor.canopy_signature {
        editor.canopy_meshes.clear();
        if lab.workspace == 0 && lab.preview_recovery {
            for (c, t, v, m) in &canopies {
                if *v == Visibility::Visible
                    && let Some(mesh) = meshes.get(&m.0)
                {
                    editor.canopy_meshes.push(PickMesh::transformed(
                        mesh,
                        t,
                        Selection {
                            stage: c.stage,
                            builtin: 5,
                            component: None,
                        },
                        false,
                    ));
                }
            }
        }
        editor.canopy_signature = canopy_signature;
    }
    let revealing = lab.show_internals || editor.placement.is_some();
    for (_, p, mut visible, _, mat) in &mut parts {
        *visible = if lab.workspace == 0
            && (!p.internal
                || revealing
                || (lab.selected_stage == p.stage
                    && lab.selected_component == p.component
                    && p.component.is_some()))
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if let Some(m) = materials.get_mut(&mat.0) {
            m.alpha_mode = if revealing && !p.internal {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            };
            m.base_color =
                m.base_color
                    .with_alpha(if revealing && !p.internal { 0.14 } else { 1. });
            m.emissive = if p.stage == lab.selected_stage
                && p.component == lab.selected_component
                && (p.component.is_some() || p.builtin == lab.selected_part)
            {
                LinearRgba::rgb(0.12, 0.07, 0.02)
            } else {
                LinearRgba::BLACK
            };
        }
    }
    let placement = if lab.workspace == 0 {
        editor.placement
    } else {
        None
    };
    let ghost_signature = placement.map_or(String::new(), |p| {
        format!(
            "{:?}/{}/{:.4}/{}",
            p.payload, p.stage, p.station, editor.signature
        )
    });
    if ghost_signature == editor.ghost_signature {
        if let Some(p) = placement {
            let offset = lab.draft.stages[p.stage + 1..]
                .iter()
                .map(|s| s.length_m as f32)
                .sum::<f32>();
            for (_, ghost, mut transform, _, _) in &mut ghosts {
                transform.translation = (ghost.position
                    + Vec3::Y
                        * (length * 0.5
                            - offset
                            - if ghost.follow_station {
                                p.station as f32
                            } else {
                                0.0
                            }))
                    * scale;
                transform.scale = Vec3::splat(scale);
            }
        }
        return;
    }
    for (e, _, _, m, mat) in &ghosts {
        commands.entity(e).despawn();
        meshes.remove(m.0.id());
        materials.remove(mat.0.id());
    }
    editor.ghost_signature = ghost_signature;
    if let Some(p) = placement {
        let s = &lab.draft.stages[p.stage];
        let offset = lab.draft.stages[p.stage + 1..]
            .iter()
            .map(|s| s.length_m as f32)
            .sum::<f32>();
        let mut pieces = match p.payload {
            DropPart::Builtin(0) => {
                let mut preview = s.clone();
                let index = crate::assembly::attach_tube(&mut preview, p.station);
                extra_instances(&preview.components[index])
            }
            DropPart::Builtin(1) => {
                let mut part = Part::new(PartKind::NoseCone, s);
                part.diameter_m = s.diameter_m;
                part.length_m = s
                    .geometry
                    .as_ref()
                    .map_or(part.length_m, |g| g.nose_length_m.max(part.length_m));
                part.position_m = 0.0;
                extra_instances(&part)
            }
            DropPart::Extra(k) => {
                let mut part = Part::new(k, s);
                part.position_m = p.station;
                if matches!(k, PartKind::BodyTube | PartKind::NoseCone) {
                    part.diameter_m = s.diameter_m;
                }
                if k == PartKind::Transition {
                    let mut preview = s.clone();
                    let index = crate::assembly::attach_transition(&mut preview, p.station);
                    part = preview.components[index].clone();
                }
                if matches!(
                    k,
                    PartKind::InnerTube | PartKind::CenteringRing | PartKind::EngineBlock
                ) {
                    let mut preview = s.clone();
                    let index = crate::assembly::attach_internal(&mut preview, k, p.station);
                    part = preview.components[index].clone();
                }
                extra_instances(&part)
            }
            DropPart::Existing { stage, component } => {
                let mut part = lab.draft.stages[stage].components[component].clone();
                part.position_m = p.station;
                part.length_m = part.length_m.min(s.length_m * 0.8);
                if !part.external() {
                    part.diameter_m = part.diameter_m.min(s.diameter_m);
                }
                part.size_tube_fins(s.diameter_m);
                extra_instances(&part)
            }
            DropPart::Builtin(5) => {
                let mut canopy = piece(
                    parachute::mesh(),
                    p.station as f32,
                    Color::srgb(1., 0.45, 0.12),
                    false,
                );
                let factor = s.diameter_m as f32;
                canopy.mesh.scale_by(Vec3::splat(factor));
                vec![canopy]
            }
            DropPart::Builtin(i) => builtin(s, i),
        };
        for piece in &mut pieces {
            commands.spawn((
                Mesh3d(meshes.add(std::mem::replace(
                    &mut piece.mesh,
                    Mesh::new(
                        PrimitiveTopology::TriangleList,
                        RenderAssetUsages::default(),
                    ),
                ))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgba(0.15, 1., 0.85, 0.65),
                    emissive: LinearRgba::rgb(0.1, 0.4, 0.25),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                })),
                Transform::from_translation(
                    (piece.position + Vec3::Y * (length * 0.5 - offset)) * scale,
                )
                .with_scale(Vec3::splat(scale)),
                Ghost {
                    position: piece.position
                        + Vec3::Y
                            * if matches!(
                                p.payload,
                                DropPart::Extra(_)
                                    | DropPart::Existing { .. }
                                    | DropPart::Builtin(0)
                                    | DropPart::Builtin(1)
                                    | DropPart::Builtin(5)
                            ) {
                                p.station as f32
                            } else {
                                0.0
                            },
                    follow_station: matches!(
                        p.payload,
                        DropPart::Extra(_)
                            | DropPart::Existing { .. }
                            | DropPart::Builtin(0)
                            | DropPart::Builtin(1)
                            | DropPart::Builtin(5)
                    ),
                },
            ));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn picker(stage: &crate::config::Stage) -> Editor {
        let mut editor = Editor::default();
        for i in 0..=5 {
            for p in builtin(stage, i) {
                editor.pick_meshes.push(PickMesh::new(
                    &p.mesh,
                    p.position,
                    1.0,
                    Selection {
                        stage: 0,
                        builtin: i,
                        component: None,
                    },
                    p.internal,
                ));
            }
        }
        editor
    }
    #[test]
    fn picking_distinguishes_body_nose_fins_and_empty_space() {
        let mut m = Mission::demo(false);
        builder::initialize_geometry(&mut m);
        let s = &m.stages[0];
        let g = s.geometry.as_ref().unwrap();
        let e = picker(s);
        let hit = |x: f32, y: f32| {
            e.pick(Vec3::new(x, y, 5.0), -Vec3::Z, false)
                .map(|p| p.builtin)
        };
        assert_eq!(hit(0.0, -g.nose_length_m as f32 * 0.4), Some(1));
        assert_eq!(hit(0.0, -s.length_m as f32 * 0.5), Some(0));
        let x = (s.diameter_m * 0.5 + g.fin_span_m * 0.5) as f32;
        let y = -(g.fin_position_m + g.fin_sweep_m * 0.5 + g.fin_tip_chord_m * 0.25) as f32;
        assert_eq!(hit(x, y), Some(2));
        assert_eq!(hit(4.0, 0.0), None);
        assert!(
            e.pick(
                Vec3::new(0.0, -s.length_m as f32 * 0.5, 5.0),
                Vec3::Z,
                false
            )
            .is_none()
        );
    }
    #[test]
    fn picking_revealed_motor_and_component_respects_world_transform() {
        let mut m = Mission::demo(false);
        builder::initialize_geometry(&mut m);
        let s = &m.stages[0];
        let mut e = picker(s);
        let origin = Vec3::new(5.0, -s.motor.cg_m as f32, 0.0);
        assert_ne!(e.pick(origin, -Vec3::X, false).unwrap().builtin, 3);
        assert_eq!(e.pick(origin, -Vec3::X, true).unwrap().builtin, 3);
        e.pick_meshes.clear();
        let mut p = Part::new(PartKind::Transition, s);
        p.position_m = 0.0;
        p.fore_diameter_m = s.diameter_m;
        p.diameter_m = s.diameter_m * 1.2;
        let piece = extra(&p);
        let offset = Vec3::new(2.0, 3.0, 0.0);
        let scale = 0.7;
        let target = Selection {
            stage: 2,
            builtin: 0,
            component: Some(7),
        };
        e.pick_meshes.push(PickMesh::new(
            &piece.mesh,
            offset + piece.position * scale,
            scale,
            target,
            false,
        ));
        assert_eq!(
            e.pick(
                offset - Vec3::Y * p.length_m as f32 * scale * 0.5 + Vec3::Z * 5.0,
                -Vec3::Z,
                false
            ),
            Some(target)
        );
        assert!(
            e.pick(
                Vec3::new(0.0, -p.length_m as f32 * 0.5, 5.0),
                -Vec3::Z,
                false
            )
            .is_none()
        );
    }
    #[test]
    fn transition_and_body_share_an_edge_without_overlapping_surfaces() {
        let mut m = Mission::demo(false);
        builder::initialize_geometry(&mut m);
        let s = &mut m.stages[0];
        s.geometry.as_mut().unwrap().nose_length_m = 0.0;
        let mut p = Part::new(PartKind::Transition, s);
        p.position_m = 0.0;
        s.components.push(p.clone());
        let body = builtin(s, 0).remove(0);
        let bounds = PickMesh::new(
            &body.mesh,
            body.position,
            1.0,
            Selection {
                stage: 0,
                builtin: 0,
                component: None,
            },
            false,
        );
        assert!((bounds.max.y + p.length_m as f32).abs() < 1e-5);
        let station = p.length_m as f32 * 0.25;
        let distance = flight_pieces(s)
            .iter()
            .filter_map(|piece| {
                PickMesh::new(
                    &piece.mesh,
                    piece.position,
                    1.0,
                    Selection {
                        stage: 0,
                        builtin: 0,
                        component: None,
                    },
                    false,
                )
                .hit(Vec3::new(0.0, -station, 5.0), -Vec3::Z)
            })
            .reduce(f32::min)
            .unwrap();
        let expected_radius =
            (p.fore_diameter_m + (p.diameter_m - p.fore_diameter_m) * 0.25) as f32 * 0.5;
        assert!((distance - (5.0 - expected_radius)).abs() < 1e-5);
        assert!(
            bounds
                .hit(Vec3::new(0.0, -p.length_m as f32 * 0.5, 5.0), -Vec3::Z)
                .is_none()
        );
    }
    #[test]
    fn placement_selects_the_pointed_stage_and_clamps_the_whole_part_inside_it() {
        let mut draft = Mission::demo(true);
        builder::initialize_geometry(&mut draft);
        let top = draft.stages.len() - 1;
        let offset = draft.stages[top].length_m;
        let p = locate(&draft, DropPart::Extra(PartKind::InnerTube), offset + 0.6);
        assert_eq!(p.stage, top - 1);
        let length = Part::new(PartKind::InnerTube, &draft.stages[p.stage]).length_m;
        assert!((p.station + length * 0.5 - 0.6).abs() <= 0.0051);
        let p = locate(&draft, DropPart::Extra(PartKind::InnerTube), 1e6);
        assert_eq!(p.stage, 0);
        assert!(p.station + length <= draft.stages[0].length_m);
        assert_eq!(locate(&draft, DropPart::Builtin(1), 1e6).stage, top);
    }
    #[test]
    fn ray_placement_uses_world_axis_and_handles_camera_pan() {
        let nose = Vec3::new(0., 2., 0.);
        assert!(
            (ray_station(
                Vec3::new(3., 0.8, 4.),
                Vec3::new(-3., 0., -4.).normalize(),
                nose,
                0.2
            )
            .unwrap()
                - 6.)
                .abs()
                < 1e-5
        );
        assert!(ray_station(Vec3::ZERO, Vec3::Y, nose, 1.).is_none());
        assert!(
            ray_station(
                Vec3::new(3., 0.8, 4.),
                Vec3::new(3., 0., 4.).normalize(),
                nose,
                1.
            )
            .is_none()
        );
    }
    #[test]
    fn annular_tube_has_an_open_bore_and_finite_normals() {
        let mesh = tube(0.1, 0.3, 0.01);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!()
        };
        assert!(p.iter().any(|p| (p[0].hypot(p[2]) - 0.09).abs() < 1e-5));
    }
    #[test]
    fn custom_fins_are_closed_outward_solids_with_the_correct_volume() {
        let s = Mission::demo(false).stages.remove(0);
        for kind in [
            PartKind::FinSet,
            PartKind::EllipticalFin,
            PartKind::FreeformFin,
        ] {
            let mut p = Part::new(kind, &s);
            if kind == PartKind::FreeformFin {
                p.fin_points = vec![[0.0, 0.0], [0.0, 0.1], [0.05, 0.04], [0.1, 0.1], [0.1, 0.0]];
            }
            p.radial_offset_m = 0.025;
            let mesh = outline_fin_mesh(&p);
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!()
            };
            let Some(Indices::U32(t)) = mesh.indices() else {
                panic!()
            };
            let volume: f32 = t
                .chunks_exact(3)
                .map(|tri| {
                    let a = Vec3::from_array(v[tri[0] as usize]);
                    let b = Vec3::from_array(v[tri[1] as usize]);
                    let c = Vec3::from_array(v[tri[2] as usize]);
                    a.dot(b.cross(c)) / 6.0
                })
                .sum();
            let g = p.shape.as_ref().unwrap();
            let expected = p.fin_area_cg().0 * g.fin_thickness_m * g.fin_count as f64;
            assert!(
                (volume as f64 / expected - 1.0).abs() < 1e-4,
                "{kind:?} signed volume {volume} / {expected}"
            );
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) =
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
            else {
                panic!()
            };
            assert!(normals.iter().all(|n| n.iter().all(|v| v.is_finite())));
        }
    }
}
