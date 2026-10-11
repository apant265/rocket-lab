//! A real computed planar CFD slice beside the 3D model, with the 2D color scale.
use super::*;
use bevy::image::ImageSampler;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

#[derive(Component)]
pub(super) struct Slice;
pub(super) struct Cache {
    signature: String,
    mesh: Handle<Mesh>,
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
}
type AssetsForSlice<'a> = (
    ResMut<'a, Assets<Mesh>>,
    ResMut<'a, Assets<Image>>,
    ResMut<'a, Assets<StandardMaterial>>,
);
type SliceQuery<'w, 's> =
    Query<'w, 's, (&'static mut Transform, &'static mut Visibility), With<Slice>>;
type SourceQuery<'w, 's> = Query<'w, 's, &'static Transform, (With<SurfaceMesh>, Without<Slice>)>;
pub(super) fn update(
    mut commands: Commands,
    context: (Res<Lab>, Res<snapshot::Snapshots>),
    mut cache: Local<Option<Cache>>,
    mut assets: AssetsForSlice,
    mut slice: SliceQuery,
    source: SourceQuery,
) {
    let (lab, w) = context;
    let visible = w.view_time(&lab).is_some()
        && matches!(lab.surface_mode, 1 | 2)
        && w.captured(&lab, snapshot::Mode::Flow).is_some();
    for (mut transform, mut visibility) in &mut slice {
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if let Some(t) = source.iter().next() {
            *transform = *t;
        }
    }
    if !visible {
        return;
    }
    let signature = w.field_signature();
    if cache.as_ref().is_some_and(|c| c.signature == signature) {
        return;
    }
    let Some(f) = w.flow_solution() else {
        return;
    };
    let p = analysis_fields::Planar::new(f, w.field());
    let rgba = p
        .display
        .iter()
        .flat_map(|value| {
            let color = value.map_or([0.314, 0.314, 0.314, 1.], |v| {
                surface::palette(((v - p.range[0]) / (p.range[1] - p.range[0])) as f32)
            });
            [
                (color[0] * 255.) as u8,
                (color[1] * 255.) as u8,
                (color[2] * 255.) as u8,
                255,
            ]
        })
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: p.width as u32,
            height: p.height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    // Behind the mesh in model z: the plane is a slice, not a volume result.
    let z = -(lab
        .flight
        .mission
        .stages
        .iter()
        .map(|s| s.diameter_m)
        .fold(0., f64::max)
        * 0.6) as f32;
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [p.y[1] as f32, -p.x[0] as f32, z],
            [p.y[1] as f32, -p.x[1] as f32, z],
            [p.y[0] as f32, -p.x[1] as f32, z],
            [p.y[0] as f32, -p.x[0] as f32, z],
        ],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 4]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    if let Some(c) = cache.as_mut() {
        *assets.0.get_mut(&c.mesh).expect("slice mesh") = mesh;
        let old_image = c.image.clone();
        c.image = assets.1.add(image);
        assets
            .2
            .get_mut(&c.material)
            .expect("slice material")
            .base_color_texture = Some(c.image.clone());
        assets.1.remove(old_image.id());
        c.signature = signature;
    } else {
        let image = assets.1.add(image);
        let mesh = assets.0.add(mesh);
        let material = assets.2.add(StandardMaterial {
            base_color_texture: Some(image.clone()),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            Slice,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            source.iter().next().copied().unwrap_or_default(),
            Visibility::Visible,
        ));
        *cache = Some(Cache {
            signature,
            mesh,
            image,
            material,
        });
    }
}
