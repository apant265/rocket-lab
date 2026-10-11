use super::*;
use crate::{
    geo::{self, Coordinate},
    satellite::{self, Bounds, Request, SatelliteImage},
};
#[derive(Component)]
pub(super) struct SatelliteGround;
#[derive(Resource)]
pub(super) struct MapState {
    pub source: usize,
    pub span_km: f64,
    pub date: String,
    pub path: String,
    pub bounds: Bounds,
    pub open: bool,
    pub show_ground: bool,
    pub status: String,
    pub preview: Option<egui::TextureHandle>,
    pub loaded_bounds: Option<Bounds>,
    pub attribution: String,
    pub site: Option<crate::config::Launch>,
    task: Option<std::thread::JoinHandle<crate::config::Result<SatelliteImage>>>,
    requested_site: Option<crate::config::Launch>,
    cached_attribution: Option<String>,
}
impl Default for MapState {
    fn default() -> Self {
        Self {
            source: 0,
            span_km: 160.0,
            date: "2025-06-15".into(),
            path: "output/site/satellite.png".into(),
            bounds: Bounds {
                west: -107.9,
                south: 32.0,
                east: -106.0,
                north: 34.0,
                mercator: false,
            },
            open: false,
            show_ground: true,
            status: "Choose a launch coordinate, apply it, then load satellite imagery.".into(),
            preview: None,
            loaded_bounds: None,
            attribution: String::new(),
            site: None,
            task: None,
            requested_site: None,
            cached_attribution: None,
        }
    }
}
impl MapState {
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
    pub fn start(&mut self, site: &crate::config::Launch) {
        if self.busy() {
            return;
        }
        let r = Request {
            source: self.source,
            latitude: site.latitude_deg,
            longitude: site.longitude_deg,
            span_km: self.span_km,
            date: self.date.clone(),
            image_path: self.path.clone(),
            bounds: self.bounds.clone(),
        };
        self.requested_site = Some(site.clone());
        self.cached_attribution = None;
        self.status = "Loading actual satellite imagery…".into();
        self.task = Some(std::thread::spawn(move || satellite::fetch(r)));
    }
    fn matching(&self, site: &crate::config::Launch) -> bool {
        self.site.as_ref().is_some_and(|s| {
            s.latitude_deg == site.latitude_deg
                && s.longitude_deg == site.longitude_deg
                && s.altitude_m == site.altitude_m
        })
    }
}
pub(super) fn cached(lab: Res<Lab>, mut state: ResMut<MapState>) {
    let result = (|| -> crate::config::Result<()> {
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read("output/site/source.json")?)?;
        let site = &lab.flight.mission.launch;
        if metadata["provider"] != "NASA EOSDIS GIBS"
            || metadata["latitude"].as_f64() != Some(site.latitude_deg)
            || metadata["longitude"].as_f64() != Some(site.longitude_deg)
        {
            return Ok(());
        }
        let bounds: Bounds = serde_json::from_slice(&std::fs::read("output/site/bounds.json")?)?;
        bounds.validate()?;
        state.bounds = bounds;
        state.source = 2;
        state.start(site);
        state.source = 0;
        state.cached_attribution = metadata["attribution"]
            .as_str()
            .map(|s| format!("{s} · local cache"));
        if let Some(date) = metadata["date"].as_str() {
            state.date = date.into();
        }
        Ok(())
    })();
    if result.is_err() {
        state.status =
            "Choose coordinates and Load satellite; no matching cached NASA image.".into();
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn poll(
    lab: Res<Lab>,
    mut state: ResMut<MapState>,
    mut contexts: EguiContexts,
    mut commands: Commands,
    existing: Query<Entity, With<SatelliteGround>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) -> bevy::prelude::Result {
    if !state.task.as_ref().is_some_and(|t| t.is_finished()) {
        return Ok(());
    }
    let result = state.task.take().unwrap().join();
    let data = match result {
        Ok(Ok(data)) => data,
        Ok(Err(e)) => {
            state.status = e.to_string();
            return Ok(());
        }
        Err(_) => {
            state.status = "Satellite worker failed".into();
            return Ok(());
        }
    };
    let site = state.requested_site.take().unwrap();
    let ctx = contexts.ctx_mut()?;
    state.preview = Some(ctx.load_texture(
        "satellite-original",
        egui::ColorImage::from_rgba_unmultiplied(
            [data.width as usize, data.height as usize],
            &data.rgba,
        ),
        egui::TextureOptions::LINEAR,
    ));
    let image = Image::new(
        Extent3d {
            width: data.width,
            height: data.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    let image = images.add(image);
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uv = Vec::new();
    let mut indices = Vec::new();
    let count = 32;
    for y in 0..=count {
        for x in 0..=count {
            let u = x as f64 / count as f64;
            let v = y as f64 / count as f64;
            let (lat, lon) = data.bounds.coordinate(u, v);
            let p = geo::to_enu(
                &site,
                Coordinate {
                    latitude_deg: lat,
                    longitude_deg: lon,
                    altitude_m: lab
                        .flight
                        .mission
                        .environment
                        .terrain
                        .as_ref()
                        .and_then(|t| t.elevation(lat, lon))
                        .unwrap_or(site.altitude_m),
                },
            );
            positions.push([
                p.x as f32 * 0.001,
                p.z as f32 * 0.001 + 0.003,
                -p.y as f32 * 0.001,
            ]);
            normals.push([0.0, 1.0, 0.0]);
            uv.push([u as f32, v as f32]);
        }
    }
    for y in 0..count {
        for x in 0..count {
            let a = y * (count + 1) + x;
            let b = a + count + 1;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_normals();
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(image),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
        SatelliteGround,
    ));
    state.loaded_bounds = Some(data.bounds);
    state.site = Some(site);
    state.attribution = state.cached_attribution.take().unwrap_or(data.attribution);
    state.status = if data.source == 1 {
        "Google satellite loaded in memory. Original attribution is preserved.".into()
    } else {
        "Satellite image loaded. Landings and flight tracks are georeferenced.".into()
    };
    Ok(())
}
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
pub(super) fn visibility(
    lab: Res<Lab>,
    state: Res<MapState>,
    mut ground: Query<&mut Visibility, With<SatelliteGround>>,
) {
    for mut v in &mut ground {
        *v = if state.show_ground
            && state.matching(&lab.flight.mission.launch)
            && lab.workspace == 1
            && !lab.inspection
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}
pub(super) fn controls(ui: &mut egui::Ui, state: &mut MapState, lab: &mut Lab) {
    ui.separator();
    ui.label(
        egui::RichText::new("SATELLITE / LANDING MAP")
            .color(theme::palette(ui).accent)
            .strong(),
    );
    egui::ComboBox::from_id_salt("satellite_provider")
        .selected_text(
            [
                "NASA MODIS · no key",
                "Google satellite · API key",
                "Offline PNG / JPEG",
            ][state.source],
        )
        .show_ui(ui, |ui| {
            for (i, name) in [
                "NASA MODIS · no key",
                "Google satellite · API key",
                "Offline PNG / JPEG",
            ]
            .iter()
            .enumerate()
            {
                ui.selectable_value(&mut state.source, i, *name);
            }
        });
    units::edit(ui, "Map span / km", &mut state.span_km, 1.);
    state.span_km = state.span_km.clamp(1., 500.);
    if state.source == 0 {
        ui.text_edit_singleline(&mut state.date);
        help::hint(
            ui,
            "NASA true-color imagery, ~250 m; cloud cover is part of the image.",
        );
    }
    if state.source == 1 {
        help::hint(
            ui,
            "Set GOOGLE_MAPS_API_KEY before starting the app. Maps Static API + billing required. Requests occur only when you click Load.",
        );
    }
    if state.source == 2 {
        ui.text_edit_singleline(&mut state.path);
        for (label, v) in [
            ("West", &mut state.bounds.west),
            ("South", &mut state.bounds.south),
            ("East", &mut state.bounds.east),
            ("North", &mut state.bounds.north),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(egui::DragValue::new(v).speed(0.001));
            });
        }
        ui.checkbox(&mut state.bounds.mercator, "Image uses Web Mercator");
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!state.busy(), egui::Button::new("Load satellite"))
            .clicked()
        {
            state.start(&lab.flight.mission.launch);
        }
        if ui.button("Map & landings").clicked() {
            state.open = true;
        }
    });
    if state.busy() {
        ui.spinner();
    }
    help::status(ui, &state.status);
    ui.checkbox(&mut state.show_ground, "Satellite ground in 3D");
    if ui.button("Top-down flight view").clicked() {
        super::fit_flight(lab);
        let target = lab.overview_target;
        lab.free_camera = Some(
            Transform::from_translation(target + Vec3::Y * lab.zoom).looking_at(target, -Vec3::Z),
        );
    }
    help::hint(
        ui,
        "Imagery adds geographic context. Ground contact uses a spherical Earth at site height; elevation terrain is not imported.",
    );
}
pub(super) fn window(ctx: &egui::Context, state: &mut MapState, lab: &Lab) {
    if !state.open {
        return;
    }
    let mut open = state.open;
    egui::Window::new("Satellite / geographic landing map").open(&mut open).default_pos(ctx.available_rect().left_top()+egui::vec2(20.0,15.0)).default_width(760.0).vscroll(true).show(ctx,|ui|{
        if let (Some(texture),Some(bounds),Some(site))=(&state.preview,&state.loaded_bounds,&state.site) {
            if !state.matching(&lab.flight.mission.launch){ui.colored_label(theme::palette(ui).warning,"Map origin differs from the applied mission; load imagery again.");}
            let width=ui.available_width().min(650.0).min((ctx.available_rect().height()-210.0).max(160.0));let response=ui.add(egui::Image::new((texture.id(),egui::vec2(width,width))));let rect=response.rect;
            let overlay=if state.attribution.starts_with("Google") {egui::Rect::from_min_max(rect.min,egui::pos2(rect.right(),rect.bottom()-40.0))}else{rect};let painter=ui.painter_at(overlay);
            let point=|east,north,agl|{let c=geo::from_sample(site,east,north,agl);let(x,y)=bounds.uv(c.latitude_deg,c.longitude_deg);egui::pos2(rect.left()+x as f32*rect.width(),rect.top()+y as f32*rect.height())};
            let samples=&lab.flight.samples;let stride=(samples.len()/1300).max(1);let mut last=None;
            for s in samples.iter().step_by(stride).chain(samples.last().into_iter()){let p=point(s.east_m,s.north_m,s.altitude_agl_m);if let Some(a)=last {painter.line_segment([a,p],egui::Stroke::new(2.0,if s.time_s<=lab.time{egui::Color32::from_rgb(70,230,240)}else{egui::Color32::from_rgb(255,190,80)}));}last=Some(p);}
            let start=point(0.0,0.0,0.0);painter.circle_filled(start,5.0,egui::Color32::WHITE);painter.text(start+egui::vec2(8.0,0.0),egui::Align2::LEFT_CENTER,"Launch",egui::FontId::proportional(13.0),egui::Color32::WHITE);
            if let (Some(e),Some(n))=(lab.flight.summary.landing_east_m,lab.flight.summary.landing_north_m){let p=point(e,n,0.0);painter.circle_filled(p,6.0,egui::Color32::LIGHT_GREEN);painter.text(p+egui::vec2(8.0,0.0),egui::Align2::LEFT_CENTER,"Retained landing",egui::FontId::proportional(13.0),egui::Color32::WHITE);}
            for stage in 0..lab.flight.mission.stages.len(){let rows:Vec<_>=lab.flight.debris_samples.iter().filter(|s|s.stage==stage).collect();let mut last=None;for s in rows.iter().step_by((rows.len()/700).max(1)){let p=point(s.east_m,s.north_m,s.altitude_agl_m);if let Some(a)=last{painter.line_segment([a,p],egui::Stroke::new(1.0,egui::Color32::LIGHT_RED));}last=Some(p);}if let Some(s)=rows.last(){let p=point(s.east_m,s.north_m,s.altitude_agl_m);painter.circle_filled(p,4.0,egui::Color32::LIGHT_RED);painter.text(p+egui::vec2(8.0,14.0*(stage as f32+1.0)),egui::Align2::LEFT_CENTER,format!("S{} {}",stage+1,if s.landed{"landing"}else{"end state"}),egui::FontId::proportional(12.0),egui::Color32::WHITE);}}
            ui.small(&state.attribution);
        } else {help::hint(ui, "Load satellite imagery using the launch conditions panel.");}
        let site=&lab.flight.mission.launch;ui.label(format!("Launch: {:.6}°, {:.6}°",site.latitude_deg,site.longitude_deg));
        if let (Some(e),Some(n))=(lab.flight.summary.landing_east_m,lab.flight.summary.landing_north_m){let c=geo::from_sample(site,e,n,0.0);ui.label(format!("Retained landing: {:.6}°, {:.6}° · {} · bearing {:.1}°",c.latitude_deg,c.longitude_deg,units::value(ui,"Downrange",e.hypot(n)/1000.,"km",2),e.atan2(n).to_degrees().rem_euclid(360.0)));ui.hyperlink_to("Open landing in Google Maps",format!("https://www.google.com/maps?q={},{}",c.latitude_deg,c.longitude_deg));}
        for stage in 0..lab.flight.mission.stages.len(){if let Some(s)=lab.flight.debris_samples.iter().rev().find(|s|s.stage==stage){let c=geo::from_sample(site,s.east_m,s.north_m,s.altitude_agl_m);ui.label(format!("S{} {}: {:.6}°, {:.6}° · {} · touchdown {}",stage+1,if s.landed{"landing"}else{"end state"},c.latitude_deg,c.longitude_deg,units::value(ui,"Downrange",s.east_m.hypot(s.north_m)/1000.,"km",2),units::value(ui,"Touchdown",s.landing_speed_mps,"m/s",2)));}}
        help::hint(ui, "Cyan: flown; amber: future retained track; red: detached stages. Map track shows the geographic ground projection. CSV export includes trajectory.geojson.");
    });
    state.open = open;
}
