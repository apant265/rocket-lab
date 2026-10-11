use super::*;
use crate::assembly::{self, KG_PER_LB, Material, PartKind};
use crate::config::{Chute, NoseShape, Trigger};
fn pounds(ui: &mut egui::Ui, label: &str, kg: &mut f64) -> bool {
    number(ui, &label.replace(" / lb", " / kg"), kg, 0.01 * KG_PER_LB)
}
fn palette_button(
    ui: &mut egui::Ui,
    index: usize,
    label: &str,
    payload: builder::DropPart,
    image: Option<egui::TextureId>,
) -> egui::Response {
    let response = ui
        .push_id(("part_palette", index), |ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(113.0, 104.0), egui::Sense::click_and_drag());
            let bg = if response.hovered() || response.dragged() {
                theme::palette(ui).hover
            } else {
                theme::palette(ui).raised
            };
            ui.painter().rect_filled(rect, 8.0, bg);
            ui.painter().rect_stroke(
                rect,
                8.0,
                egui::Stroke::new(
                    1.0,
                    if response.hovered() {
                        theme::palette(ui).accent
                    } else {
                        theme::palette(ui).border
                    },
                ),
                egui::StrokeKind::Inside,
            );
            if let Some(image) = image {
                ui.painter().image(
                    image,
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(4.0, 1.0),
                        egui::vec2(105.0, 79.0),
                    ),
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            ui.painter().text(
                rect.center_bottom() - egui::vec2(0.0, 13.0),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                theme::palette(ui).text,
            );
            response
        })
        .inner;
    response.dnd_set_drag_payload(payload);
    response.on_hover_text("Drag into the 3D view to attach. Click a component to edit dimensions, material and offset.")
}
fn release_part(ctx: &egui::Context, rect: egui::Rect) -> Option<builder::DropPart> {
    if ctx.input(|i| i.pointer.any_released())
        && ctx.pointer_interact_pos().is_some_and(|p| rect.contains(p))
    {
        egui::DragAndDrop::take_payload::<builder::DropPart>(ctx).map(|p| *p)
    } else {
        None
    }
}

const PARTS: [&str; 6] = [
    "Airframe",
    "Nose cone",
    "Fin set",
    "Motor & staging",
    "Payload & avionics",
    "Recovery",
];
fn number(ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64) -> bool {
    units::edit(ui, label, v, speed)
}
fn section(ui: &mut egui::Ui, title: &str) {
    let mut title = title.to_lowercase();
    if let Some(first) = title.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(title)
            .size(14.0)
            .strong()
            .color(theme::palette(ui).accent),
    );
    ui.add_space(4.0);
}
fn primary(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(label)
                .strong()
                .color(theme::palette(ui).on_accent),
        )
        .fill(theme::palette(ui).accent)
        .min_size(egui::vec2(135.0, 31.0)),
    )
    .clicked()
}
fn navigation_panel(ctx: &egui::Context) -> egui::SidePanel {
    egui::SidePanel::left("navigation")
        .default_width(265.0)
        .width_range(190.0..=(ctx.content_rect().width() * 0.4).clamp(190.0, 600.0))
        .resizable(true)
        .show_separator_line(true)
}
fn inspector_panel(ctx: &egui::Context) -> egui::SidePanel {
    egui::SidePanel::right("inspector")
        .default_width(290.0)
        .width_range(210.0..=(ctx.content_rect().width() * 0.4).clamp(210.0, 600.0))
        .resizable(true)
        .show_separator_line(true)
}
fn sidebar_scroll<R>(
    ui: &mut egui::Ui,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            let mut clip = ui.clip_rect();
            clip.max.x = clip.max.x.min(ui.max_rect().right());
            ui.set_clip_rect(clip);
            contents(ui)
        })
}
fn analysis_panel(ctx: &egui::Context, height: f32) -> egui::TopBottomPanel {
    egui::TopBottomPanel::bottom("analysis")
        .default_height(height)
        .height_range(110.0..=(ctx.content_rect().height() * 0.65).max(110.0))
        .resizable(true)
        .show_separator_line(true)
}
/// Keep content's minimum size from feeding back into the panel's saved height.
fn bounded_panel_content(ui: &mut egui::Ui) -> egui::Ui {
    let rect = ui.available_rect_before_wrap();
    ui.expand_to_include_rect(rect);
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(*ui.layout()));
    content.set_clip_rect(rect);
    content
}
#[allow(clippy::too_many_arguments)]
pub(super) fn interface(
    mut contexts: EguiContexts,
    mut lab: ResMut<Lab>,
    mut job: ResMut<builder::BuildJob>,
    mut map: ResMut<map::MapState>,
    mut catalogue: ResMut<editor::Editor>,
    mut thermal: ResMut<thermal_view::ThermalView>,
    mut history: ResMut<history::History>,
    mut snapshots: ResMut<snapshot::Snapshots>,
    camera: Query<(&Camera, &GlobalTransform), With<FlightCamera>>,
) -> bevy::prelude::Result {
    let ctx = contexts.ctx_mut()?;
    editor::textures(&mut catalogue, ctx);
    history.observe(
        &lab.draft,
        !ctx.input(|i| i.pointer.any_down()) && !ctx.wants_keyboard_input(),
    );
    lab.theme.apply(ctx);
    let colors = lab.theme.palette();
    egui::TopBottomPanel::top("header")
        .exact_height(54.0)
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        lab.workspace == 0,
                        egui::RichText::new("Build").size(15.0).strong(),
                    )
                    .clicked()
                {
                    lab.workspace = 0;
                    lab.playing = false;
                    reset_camera(&mut lab);
                }
                if ui
                    .selectable_label(
                        lab.workspace == 1,
                        egui::RichText::new("Simulate").size(15.0).strong(),
                    )
                    .clicked()
                {
                    lab.workspace = 1;
                    reset_camera(&mut lab);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!job.busy(), egui::Button::new("Export 3D model"))
                        .on_hover_text("Export Blender + GLB and calculate mass, centroid and inertia from the evaluated geometry.").clicked() {
                        builder::start_model(&mut lab, &mut job);
                    }
                    if primary(ui, "Launch simulation", !job.busy()) {
                        builder::launch(&mut lab, &mut job);
                    }
                    if ui.add_enabled(!job.busy(), egui::Button::new("Reset rocket"))
                        .on_hover_text("Restore the loaded design and reset flight and camera. Undo restores your draft edits.")
                        .clicked() {
                        history.observe(&lab.draft, true);
                        reset_rocket(&mut lab);
                        history.observe(&lab.draft, true);
                        catalogue.armed = None;
                        catalogue.placement = None;

                        egui::DragAndDrop::clear_payload(ctx);
                    }
                    let previous = lab.theme;
                    units::selector(ui);
                    ui.selectable_value(&mut lab.theme, theme::Theme::Light, "Light");
                    ui.selectable_value(&mut lab.theme, theme::Theme::Dark, "Dark");
                    if lab.theme != previous && !lab.smoke
                        && let Err(e) = lab.theme.save() {
                        lab.status = format!("Theme changed; preference could not be saved: {e}");
                    }
                    if job.busy() {
                        ui.spinner();
                    }
                    ui.add(egui::Label::new(egui::RichText::new(&lab.draft.name).color(theme::palette(ui).muted)).truncate());
                });
            });
        });
    egui::TopBottomPanel::bottom("status")
        .exact_height(27.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(progress) = &job.progress {
                    ui.spinner();
                    let status = progress.status();
                    ui.label(egui::RichText::new(status.label()).size(11.0));
                    if let Some(fraction) = status.fraction {
                        ui.add(egui::ProgressBar::new(fraction).desired_width(100.0));
                    }
                    return;
                }
                help::status(ui, &lab.status);
            });
        });
    analysis_panel(
        ctx,
        if lab.workspace == 0 {
            200.0
        } else if lab.tab == 6 && snapshots.image_view {
            128.0
        } else {
            244.0
        },
    )
    .show(ctx, |ui| {
        let mut content = bounded_panel_content(ui);
        if lab.workspace == 0 {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(&mut content, |ui| build_summary(ui, &lab));
        } else {
            analysis(&mut content, &mut lab, &mut snapshots, &mut thermal);
        }
    });
    navigation_panel(ctx).show(ctx, |ui| {
        sidebar_scroll(ui, |ui| {
            if lab.workspace == 0 {
                history.controls(ui, &mut lab, job.busy());
                build_parts(ui, &mut lab, &mut job, &mut catalogue);
            } else {
                let enabled = !job.busy();
                ui.add_enabled_ui(enabled, |ui| {
                    conditions(ui, &mut lab, &mut job);
                    map::controls(ui, &mut map, &mut lab);
                });
            }
        });
    });
    inspector_panel(ctx).show(ctx, |ui| {
        sidebar_scroll(ui, |ui| {
            if lab.workspace == 0 {
                ui.add_enabled_ui(!job.busy(), |ui| {
                    component_editor(ui, &mut lab, &mut catalogue)
                });
            } else {
                snapshot::controls(ui, &mut snapshots, &mut lab);
                if snapshots.mode == snapshot::Mode::Thermal {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut snapshots.image_view, true, "Vehicle image");
                        ui.selectable_value(&mut snapshots.image_view, false, "3D vehicle");
                    });
                    if !snapshots.image_view {
                        thermal_view::legend(ui, &mut thermal, &lab);
                    }
                }
                let image_view = lab.tab == 6 && snapshots.image_view;
                telemetry(ui, &mut lab, image_view);
            }
        });
    });
    if lab.geometry_dirty {
        for stage in &mut lab.draft.stages {
            for part in &mut stage.components {
                part.size_tube_fins(stage.diameter_m);
            }
        }
    }
    let rect = ctx.available_rect();
    lab.viewport = [rect.left(), rect.top(), rect.width(), rect.height()];
    catalogue.placement = None;
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        catalogue.armed = None;
        egui::DragAndDrop::clear_payload(ctx);
    }
    let payload = egui::DragAndDrop::payload::<builder::DropPart>(ctx)
        .map(|p| *p)
        .or(catalogue.armed);
    if lab.workspace == 0 && !job.busy() {
        if ctx.input(|i| i.pointer.primary_pressed()) {
            lab.left_was_drag = false;
        }
        if payload.is_none()
            && !lab.left_was_drag
            && !lab.dragging
            && ctx.input(|i| i.pointer.primary_clicked())
            && let Some(cursor) = ctx.pointer_interact_pos().filter(|p| {
                rect.contains(*p)
                    && ctx
                        .layer_id_at(*p)
                        .is_none_or(|layer| layer.order == egui::Order::Background)
            })
            && let Ok((camera, transform)) = camera.single()
            && let Ok(ray) = camera.viewport_to_world(transform, Vec2::new(cursor.x, cursor.y))
            && let Some(selection) = catalogue.pick(ray.origin, *ray.direction, lab.show_internals)
        {
            editor::select(&mut lab, selection);
        }
        if let Some(payload) = payload
            && let Some(cursor) = ctx.pointer_interact_pos().filter(|p| {
                rect.contains(*p)
                    && ctx
                        .layer_id_at(*p)
                        .is_none_or(|layer| layer.order == egui::Order::Background)
            })
            && let Ok((camera, transform)) = camera.single()
            && let Ok(ray) = camera.viewport_to_world(transform, Vec2::new(cursor.x, cursor.y))
        {
            let (length, scale) = editor::dimensions(&lab);
            if let Some(station) = editor::ray_station(
                ray.origin,
                *ray.direction,
                Vec3::Y * length * 0.5 * scale,
                scale,
            ) {
                let placement = editor::locate(&lab.draft, payload, station);
                catalogue.placement = Some(placement);
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("drop_preview"),
                ));
                painter.rect_stroke(
                    rect.shrink(8.0),
                    8.0,
                    egui::Stroke::new(2.0, colors.accent),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    cursor + egui::vec2(18.0, 18.0),
                    egui::Align2::LEFT_TOP,
                    if matches!(
                        placement.payload,
                        builder::DropPart::Extra(PartKind::Transition)
                    ) {
                        format!(
                            "Create booster below S{}\nRelease to place · Esc cancels",
                            placement.stage + 1
                        )
                    } else {
                        format!(
                            "Attach to S{} · {:.2} m\nRelease to place · Esc cancels",
                            placement.stage + 1,
                            placement.station
                        )
                    },
                    egui::FontId::proportional(12.0),
                    colors.accent,
                );
                let released = release_part(ctx, rect);
                if released.is_some()
                    || (catalogue.armed.is_some() && ctx.input(|i| i.pointer.primary_released()))
                {
                    builder::drop_part(
                        &mut lab,
                        placement.payload,
                        placement.stage,
                        Some(placement.station),
                    );
                    catalogue.placement = None;
                    catalogue.armed = None;
                    lab.show_internals |= lab
                        .selected_component
                        .map_or((3..=5).contains(&lab.selected_part), |c| {
                            !lab.draft.stages[lab.selected_stage].components[c].external()
                        });
                }
            }
        }
    } else {
        catalogue.armed = None;
    }
    if lab.workspace == 1 && lab.tab == 6 && snapshots.image_view {
        egui::CentralPanel::default().show(ctx, |ui| {
            snapshot::image_panel(ui, &mut snapshots, &lab, &thermal)
        });
    } else {
        egui::Area::new(egui::Id::new("viewport_tools"))
        .fixed_pos(rect.left_top() + egui::vec2(15.0, 13.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(colors.surface.gamma_multiply(0.96))
                .stroke(egui::Stroke::new(1.0, colors.border))
                .corner_radius(8.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Vehicle").clicked() {
                            lab.followed_stage=None;
                            reset_camera(&mut lab);
                        }
                        if lab.workspace == 1 && ui.button("Fit flight").clicked() {
                            fit_flight(&mut lab);
                        }
                        if lab.workspace == 1 {
                            for (i, name) in ["Model", "Pressure", "Airflow", "Thermal", "Stress"].iter().enumerate() {
                                let clicked=ui.selectable_value(&mut lab.surface_mode,i,*name).clicked();
                                if clicked && i==2 {lab.streamlines=true;}
                                if clicked && i==3 {thermal.selected_stage=current(&lab).0.stage;}
                                if clicked && i>0 {
                                    snapshots.mode=match i {3=>snapshot::Mode::Thermal,4=>snapshot::Mode::Stress,_=>snapshot::Mode::Flow};
                                    if i>=3 {lab.tab=6;}
                                }
                            }
                        } else {
                            ui.label(
                                egui::RichText::new("3D DESIGN PREVIEW")
                                    .color(theme::palette(ui).muted)
                                    .size(11.0),
                            );
                            if ui.checkbox(&mut lab.preview_recovery,"Recovery preview").changed(){lab.inspection=true;lab.zoom=12.0;lab.free_camera=None;}
                            ui.checkbox(&mut lab.show_internals,"Internals");
                        }
                    });
                    if lab.workspace == 1 {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut lab.show_past, "Flown");
                            ui.checkbox(&mut lab.show_future, "Projected");
                            ui.checkbox(&mut lab.show_weather, "Sky");
                            ui.checkbox(&mut lab.show_grid,"Grid");
                        });
                        egui::ComboBox::from_id_salt("follow_stage").selected_text(lab.followed_stage.map_or("Follow retained vehicle".into(),|s|format!("Follow detached S{}",s+1))).show_ui(ui,|ui|{let old=lab.followed_stage;ui.selectable_value(&mut lab.followed_stage,None,"Retained vehicle");for stage in 0..lab.flight.mission.stages.len()-1{ui.selectable_value(&mut lab.followed_stage,Some(stage),format!("Detached S{}",stage+1));}if old!=lab.followed_stage{reset_camera(&mut lab);lab.inspection=false;lab.zoom=3.0;}});
                    }
                    ui.label(
                        egui::RichText::new(
                            "Camera controls",
                        )
                        .size(10.0)
                        .color(theme::palette(ui).muted),
                    ).on_hover_text("Click · properties\nHold left or right drag · move camera\nShift + right drag · look\nWheel · zoom");
                });
        });
    }
    editor(ctx, &mut lab, &job);
    map::window(ctx, &mut map, &lab);
    Ok(())
}
/// Explicit groups and thumbnail IDs keep the palette independent of display order.
pub(super) fn palette_entries() -> Vec<(usize, &'static str, builder::DropPart, usize)> {
    use builder::DropPart::{Builtin, Extra};
    vec![
        (0, "Nose cone", Builtin(1), 1),
        (0, "Body tube", Builtin(0), 0),
        (0, "Transition", Extra(PartKind::Transition), 15),
        (0, "Trapezoidal fins", Extra(PartKind::FinSet), 2),
        (0, "Elliptical fins", Extra(PartKind::EllipticalFin), 16),
        (0, "Freeform fins", Extra(PartKind::FreeformFin), 17),
        (0, "Tube fins", Extra(PartKind::TubeFin), 18),
        (0, "Launch lug", Extra(PartKind::LaunchLug), 19),
        (0, "Rail button", Extra(PartKind::RailButton), 11),
        (1, "Inner tube", Extra(PartKind::InnerTube), 7),
        (1, "Tube coupler", Extra(PartKind::Coupler), 8),
        (1, "Centering ring", Extra(PartKind::CenteringRing), 10),
        (1, "Bulkhead", Extra(PartKind::Bulkhead), 9),
        (1, "Engine block", Extra(PartKind::EngineBlock), 20),
        (2, "Parachute", Extra(PartKind::Parachute), 5),
        (2, "Streamer", Extra(PartKind::Streamer), 21),
        (2, "Shock cord", Extra(PartKind::ShockCord), 12),
        (2, "Mass component", Extra(PartKind::Mass), 13),
        (2, "Motor", Builtin(3), 3),
        (2, "Payload", Builtin(4), 4),
        (2, "Avionics bay", Extra(PartKind::Avionics), 14),
        (3, "Pods", Extra(PartKind::PodSet), 22),
    ]
}

fn build_parts(
    ui: &mut egui::Ui,
    lab: &mut Lab,
    job: &mut builder::BuildJob,
    catalogue: &mut editor::Editor,
) {
    section(ui, "Parts");
    egui::ComboBox::from_id_salt("part_group")
        .selected_text(
            [
                "Body & fins",
                "Inner components",
                "Mass & recovery",
                "Assemblies",
            ][catalogue.category.min(3)],
        )
        .show_ui(ui, |ui| {
            for (i, name) in [
                "Body & fins",
                "Inner components",
                "Mass & recovery",
                "Assemblies",
            ]
            .iter()
            .enumerate()
            {
                ui.selectable_value(&mut catalogue.category, i, *name);
            }
        });
    ui.add_enabled_ui(!job.busy(), |ui| {
        let columns = ((ui.available_width() + 5.0) / 118.0).floor().max(1.0) as usize;
        egui::Grid::new("parts_palette")
            .num_columns(columns)
            .spacing([5.0, 5.0])
            .show(ui, |ui| {
                let mut row = 0;
                for (i, (group, label, payload, image)) in palette_entries().into_iter().enumerate()
                {
                    if group != catalogue.category {
                        continue;
                    }
                    let response =
                        palette_button(ui, i, label, payload, Some(catalogue.textures[image].id()));
                    if response.clicked() {
                        catalogue.armed = Some(payload);
                        lab.status =
                            "Part selected: move into 3D and click to attach; Esc cancels.".into();
                    }
                    row += 1;
                    if row % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    });
}

fn component_editor(ui: &mut egui::Ui, lab: &mut Lab, catalogue: &mut editor::Editor) {
    let i = lab.selected_stage.min(lab.draft.stages.len() - 1);
    lab.selected_stage = i;
    ui.label(format!("Stage {} · {}", i + 1, lab.draft.stages[i].name));
    if let Some(model) = &lab.draft.stages[i].model_mass {
        if model.validate(&lab.draft.stages[i]).is_ok() {
            metric(
                ui,
                "Model dry mass",
                units::value(ui, "Mass", model.mass_kg, "kg", 2),
            );
            metric(
                ui,
                "Model CG",
                units::value(ui, "CG", -model.centroid_m[2], "m", 3),
            );
        } else {
            help::hint(ui, "3D model needs export after these edits.");
        }
    }
    help::hint(
        ui,
        "Click a component in 3D to edit. Enable Internals for motor, payload and recovery.",
    );
    if lab.draft.stages.len() > 1 && ui.button("Remove selected stage").clicked() {
        builder::remove_stage(lab);
        return;
    }
    let top = i == lab.draft.stages.len() - 1;
    if let Some(c) = lab.selected_component {
        if c < lab.draft.stages[i].components.len() {
            extra_editor(ui, lab, i, c, catalogue);
            return;
        }
        lab.selected_component = None;
    }
    section(ui, PARTS[lab.selected_part]);
    ui.text_edit_singleline(&mut lab.draft.stages[i].name);
    ui.separator();
    let stage = &mut lab.draft.stages[i];
    match lab.selected_part {
        0 => {
            let old = stage.length_m;
            let front = editor::body_front(stage);
            let mut body = old - front;
            if number(ui, "Body length / m", &mut body, 0.005) {
                let next = body + front;
                if next > 0.02 {
                    let ratio = next / old;
                    stage.length_m = next;
                    stage.dry_cg_m *= ratio;
                    stage.payload_cg_m *= ratio;
                    stage.motor.cg_m *= ratio;
                    if let Some(m) = &mut stage.materials {
                        m.hardware_cg_m *= ratio;
                    }
                    for c in &mut stage.components {
                        c.position_m *= ratio;
                        c.position_m = c.position_m.min((next - c.length_m).max(0.0));
                    }
                    stage.geometry.as_mut().unwrap().fin_position_m *= ratio;
                    lab.geometry_dirty = true;
                }
            }
            if number(ui, "Diameter / m", &mut stage.diameter_m, 0.001) {
                for c in &mut stage.components {
                    if c.kind == PartKind::Transition && c.position_m.abs() < 1e-6 {
                        c.diameter_m = stage.diameter_m;
                    }
                }
                lab.geometry_dirty = true;
            }
            section(ui, "MEASURED MASS PROPERTIES");
            pounds(ui, "Dry mass / lb", &mut stage.dry_mass_kg);
            number(ui, "Dry CG aft nose / m", &mut stage.dry_cg_m, 0.005);
            help::hint(
                ui,
                "Dry mass includes empty motor casing, fins and electronics. Payload and propellant are added separately.",
            );
            materials_editor(ui, stage);
            ui.collapsing("Structure & thermal limits", |ui| {
                number(ui, "Max q / Pa", &mut stage.max_q_pa, 100.0);
                number(ui, "Acceleration / g", &mut stage.max_accel_g, 0.1);
                number(ui, "Temperature / K", &mut stage.max_temperature_k, 1.0);
                number(
                    ui,
                    "Heat capacity / J/K",
                    &mut stage.thermal_capacity_j_per_k,
                    10.0,
                );
                number(ui, "Heated area / m²", &mut stage.heated_area_m2, 0.001);
                number(ui, "Emissivity", &mut stage.emissivity, 0.01);
            });
        }
        1 if top => {
            let g = stage.geometry.as_mut().unwrap();
            let old = g.nose_length_m;
            egui::ComboBox::from_id_salt("nose_shape")
                .selected_text(format!("{:?}", g.nose_shape))
                .show_ui(ui, |ui| {
                    for (shape, label) in [
                        (NoseShape::Conical, "Conical"),
                        (NoseShape::TangentOgive, "Tangent ogive"),
                        (NoseShape::Elliptical, "Elliptical"),
                    ] {
                        lab.geometry_dirty |= ui
                            .selectable_value(&mut g.nose_shape, shape, label)
                            .changed();
                    }
                });
            if number(ui, "Nose length / m", &mut g.nose_length_m, 0.005) {
                let delta = g.nose_length_m - old;
                stage.length_m += delta;
                g.fin_position_m += delta;
                stage.dry_cg_m += delta;
                stage.motor.cg_m += delta;
                stage.payload_cg_m += delta;
                if let Some(m) = &mut stage.materials {
                    m.hardware_cg_m += delta;
                }
                for c in &mut stage.components {
                    c.position_m = (c.position_m + delta).max(0.0);
                }
                lab.geometry_dirty = true;
            }
            lab.geometry_dirty |= number(
                ui,
                "Heating tip radius / m",
                &mut stage.nose_radius_m,
                0.0001,
            );
            help::hint(
                ui,
                "Body length stays fixed. Recheck measured CG after changing the nose. Thermal tip radius is a screening input.",
            );
        }
        2 => {
            let g = stage.geometry.as_mut().unwrap();
            ui.horizontal(|ui| {
                ui.label("Fin count");
                lab.geometry_dirty |= ui
                    .add(egui::DragValue::new(&mut g.fin_count).range(3..=8))
                    .changed();
            });
            for (label, value, speed) in [
                ("Root chord / m", &mut g.fin_root_chord_m, 0.002),
                ("Tip chord / m", &mut g.fin_tip_chord_m, 0.002),
                ("Span / m", &mut g.fin_span_m, 0.002),
                ("Sweep / m", &mut g.fin_sweep_m, 0.002),
                ("Thickness / m", &mut g.fin_thickness_m, 0.0002),
                ("Root leading edge / m", &mut g.fin_position_m, 0.005),
            ] {
                lab.geometry_dirty |= number(ui, label, value, speed);
            }
            number(ui, "Fin cant / deg", &mut stage.fin_cant_deg, 0.05);
            help::hint(
                ui,
                "Trapezoidal hexfoil fins. Position is aft of this stage’s nose. Geometry updates the preliminary small-angle stability estimate.",
            );
        }
        3 => {
            let current = stage.motor.name.clone();
            metric(ui, "Loaded motor", current);
            metric(
                ui,
                "Stage",
                format!("S{} / {}", i + 1, if top { "Sustainer" } else { "Booster" }),
            );
            metric(ui, "Burn time", format!("{:.2} s", stage.motor.burn_time()));
            ui.text_edit_singleline(&mut lab.motor_search);
            let query = lab.motor_search.to_lowercase();
            let diameter = stage.diameter_m;
            let mut chosen = None;
            egui::ComboBox::from_id_salt("motor_library")
                .selected_text("Select a motor curve…")
                .width(240.0)
                .height(220.0)
                .show_ui(ui, |ui| {
                    for entry in lab.motor_library.iter().filter(|e| {
                        e.name.to_lowercase().contains(&query)
                            && e.diameter_m <= diameter
                            && e.length_m <= stage.length_m
                    }) {
                        if ui
                            .selectable_label(
                                false,
                                format!(
                                    "{} / {}",
                                    entry.name,
                                    units::value(ui, "Diameter", entry.diameter_m * 1000., "mm", 2)
                                ),
                            )
                            .clicked()
                        {
                            chosen = Some(entry.clone());
                        }
                    }
                });
            help::hint(
                ui,
                "Search motor names. Library filters by outer diameter and stage length; verify actual motor mount clearance.",
            );
            pounds(ui, "Propellant / lb", &mut stage.motor.propellant_mass_kg);
            number(ui, "Motor CG / m", &mut stage.motor.cg_m, 0.005);
            number(ui, "Ignition delay / s", &mut stage.ignition_delay_s, 0.1);
            section(ui, "SEPARATION EVENT");
            trigger_editor(ui, &mut stage.separation);
            number(
                ui,
                "Separation speed / m/s",
                &mut stage.separation_speed_mps,
                0.1,
            );
            if let Some(entry) = chosen {
                builder::select_motor(lab, &entry);
            }
        }
        4 => {
            ui.text_edit_singleline(&mut stage.payload_name);
            pounds(ui, "Payload mass / lb", &mut stage.payload_mass_kg);
            if ui
                .button(format!(
                    "Kate and Anvil · {}",
                    units::value(ui, "Payload", 5. * KG_PER_LB, "kg", 2)
                ))
                .clicked()
            {
                stage.payload_name = "Kate and Anvil".into();
                stage.payload_mass_kg = 5.0 * KG_PER_LB;
            }
            number(ui, "Payload CG / m", &mut stage.payload_cg_m, 0.005);
            section(ui, "FLIGHT COMPUTER");
            let a = &mut lab.draft.avionics;
            number(ui, "Sample rate / Hz", &mut a.sample_hz, 1.0);
            number(ui, "Latency / s", &mut a.latency_s, 0.005);
            number(ui, "Battery / Wh", &mut a.battery_wh, 0.1);
            number(ui, "Power / W", &mut a.power_w, 0.1);
            number(ui, "Barometer ceiling / m", &mut a.baro_ceiling_m, 100.0);
            number(ui, "GNSS ceiling / m", &mut a.gnss_ceiling_m, 100.0);
            number(ui, "Acceleration range / g", &mut a.accel_range_g, 1.0);
            number(ui, "Gyro range / deg/s", &mut a.gyro_range_deg_s, 10.0);
            ui.collapsing("Airstart interlocks", |ui| {
                number(ui, "Maximum tilt / deg", &mut a.ignition_max_tilt_deg, 0.5);
                number(ui, "Minimum AGL / m", &mut a.ignition_min_altitude_m, 10.0);
            });
            help::hint(
                ui,
                "One idealized flight computer controls the active stack. This does not execute firmware or model radio links.",
            );
        }
        5 => {
            let recovery = if top {
                &mut lab.draft.recovery
            } else {
                stage
                    .recovery
                    .get_or_insert_with(|| lab.draft.recovery.clone())
            };
            ui.small(if top {
                "Retained vehicle recovery · sampled avionics controller"
            } else {
                "This detached stage · independent ideal recovery controller"
            });
            chute_editor(ui, "DROGUE / APOGEE", &mut recovery.drogue);
            chute_editor(ui, "MAIN / DESCENT", &mut recovery.main);
            number(
                ui,
                "Main deployment AGL / m",
                &mut recovery.main_altitude_agl_m,
                10.0,
            );
        }
        _ => {
            help::hint(ui, "Select the top stage to edit its nose cone.");
        }
    }
    ui.separator();
    if lab.selected_part == 2 {
        structural_inputs(ui, lab, i);
    }
    if let Err(e) = lab.draft.validate() {
        ui.colored_label(theme::palette(ui).danger, "Invalid design")
            .on_hover_text(e.to_string());
    }
    ui.collapsing("Aerodynamic source",|ui|{ui.small(&lab.draft.stages[i].aero_source);help::hint(ui, "Edited geometry uses a preliminary low-Mach CN/CP model with synthetic Mach-dependent drag. Full coefficient tables remain editable in mission JSON.");});
}
fn structural_inputs(ui: &mut egui::Ui, lab: &mut Lab, stage: usize) {
    ui.collapsing("Legacy fin law", |ui| {
        let index = lab.draft.experiments.thermal.iter().position(|l| l.stage == stage && l.region == "fin");
        if index.is_none() {
            help::hint(ui, "Enter structural data in this fin's Material → Structural properties. Existing mission fin laws remain a fallback when the material has no mechanical properties.");
            return;
        }
        let law = &mut lab.draft.experiments.thermal[index.unwrap()];
        let mut modulus = law.youngs_modulus_pa / 1e9;
        let mut strength = law.allowable_stress_pa / 1e6;
        if number(ui, "Young’s modulus / GPa", &mut modulus, 0.1) {law.youngs_modulus_pa = modulus * 1e9; lab.geometry_dirty = true;}
        if number(ui, "Allowable strength / MPa", &mut strength, 1.0) {law.allowable_stress_pa = strength * 1e6; lab.geometry_dirty = true;}
        lab.geometry_dirty |= number(ui, "Softening threshold / K", &mut law.ablation_temperature_k, 1.0);
        help::hint(ui, "Positive modulus and strength are required. 1e9 K disables effective softening; an entered threshold enables a simple linear reduction. This is an isotropic approximation.");
    });
}
fn material_editor(ui: &mut egui::Ui, id: &str, material: &mut Material) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(&material.name)
        .show_ui(ui, |ui| {
            for preset in Material::presets() {
                if ui
                    .selectable_label(material.name == preset.name, &preset.name)
                    .clicked()
                {
                    *material = preset;
                }
            }
        });
    ui.push_id(id,|ui| {
        ui.collapsing("Structural properties",|ui| {
            let mut enabled=material.mechanical.is_some();
            if ui.checkbox(&mut enabled,"Entered structural data").on_hover_text("Enter measured or manufacturer values for this material. Modulus controls stiffness; allowable stress controls the strength ratio. Missing data produces no strain or margin.").changed() {
                material.mechanical=enabled.then_some(assembly::MechanicalMaterial {youngs_modulus_pa:0.,poisson_ratio:0.3,allowable_stress_pa:None,temperature_factors:vec![]});
            }
            if let Some(m)=&mut material.mechanical {
                let mut modulus=m.youngs_modulus_pa/1e9;
                if number(ui,"Young’s modulus / GPa",&mut modulus,0.1) {m.youngs_modulus_pa=modulus*1e9;}
                number(ui,"Poisson ratio",&mut m.poisson_ratio,0.005);
                let mut entered=m.allowable_stress_pa.is_some();
                if ui.checkbox(&mut entered,"Allowable strength").changed() {m.allowable_stress_pa=entered.then_some(0.);}
                if let Some(strength)=&mut m.allowable_stress_pa {
                    let mut mpa=*strength/1e6;if number(ui,"Allowable stress / MPa",&mut mpa,1.) {*strength=mpa*1e6;}
                }
                ui.collapsing("Temperature factors",|ui| {
                    help::hint(ui,"Measured temperature-dependent factors for modulus and allowable strength. Empty means constant properties. Temperatures must increase; factors must remain positive.");
                    for (i,p) in m.temperature_factors.iter_mut().enumerate() {ui.push_id(i,|ui| {
                        number(ui,"Temperature / K",&mut p[0],1.);
                        number(ui,"Stiffness factor",&mut p[1],0.01);
                        number(ui,"Strength factor",&mut p[2],0.01);
                    });}
                    if ui.button("Add temperature").clicked() {let t=m.temperature_factors.last().map_or(288.15,|p|p[0]+50.);m.temperature_factors.push([t,1.,1.]);}
                    if !m.temperature_factors.is_empty() && ui.button("Remove temperature").clicked() {m.temperature_factors.pop();}
                });
                if !m.valid() {ui.colored_label(theme::palette(ui).danger,"Material data required").on_hover_text("Modulus and entered strength must be positive. Poisson ratio must be greater than -1 and below 0.5. Check temperature factors.");}
            }
        });
    });
    ui.collapsing("Heat transfer properties", |ui| {
        number(
            ui,
            "Conductivity / W/(m·K)",
            &mut material.thermal.conductivity_w_m_k,
            0.01,
        );
        number(
            ui,
            "Heat capacity / J/(kg·K)",
            &mut material.thermal.specific_heat_j_kg_k,
            1.0,
        );
        number(ui, "Emissivity", &mut material.thermal.emissivity, 0.01);
        let mut limit = material.thermal.limit_temperature_k - 273.15;
        if number(ui, "Screening limit / °C", &mut limit, 1.0) {
            material.thermal.limit_temperature_k = limit + 273.15;
        }
        help::hint(
            ui,
            "Editable isotropic estimates. Use measured values for composites.",
        );
    });
    if number(ui, "Density / kg/m³", &mut material.density_kg_m3, 1.) {
        material.name = "Custom material".into();
    }
}
fn materials_editor(ui: &mut egui::Ui, stage: &mut crate::config::Stage) {
    let m = stage.materials.as_mut().unwrap();
    section(ui, "MATERIALS / MASS MODEL");
    ui.checkbox(
        &mut m.use_material_mass,
        "Calculate dry mass from materials",
    );
    help::hint(
        ui,
        "When off, measured dry mass is used. Added palette components always contribute separately.",
    );
    ui.collapsing("Airframe material", |ui| {
        material_editor(ui, "body_material", &mut m.airframe);
        number(ui, "Wall thickness / m", &mut m.airframe_wall_m, 0.0001);
    });
    ui.collapsing("Nose material", |ui| {
        material_editor(ui, "nose_material", &mut m.nose);
        number(ui, "Shell thickness / m", &mut m.nose_wall_m, 0.0001);
    });
    ui.collapsing("Fin material", |ui| {
        material_editor(ui, "fin_material", &mut m.fins);
    });
    ui.checkbox(&mut m.component_shell, "Use individual exterior segments");
    if m.use_material_mass || m.component_shell {
        pounds(ui, "Other dry hardware / lb", &mut m.hardware_mass_kg);
        number(ui, "Hardware CG / m", &mut m.hardware_cg_m, 0.005);
        help::hint(
            ui,
            "Add empty motor casing, electronics and hardware here, or as components. Do not count the same mass twice. Nose shells use a thin conical estimate.",
        );
    }
    egui::ComboBox::from_id_salt("surface_finish")
        .selected_text(&m.finish)
        .show_ui(ui, |ui| {
            for name in ["Polished", "Smooth paint", "Unfinished", "Rough"] {
                ui.selectable_value(&mut m.finish, name.into(), name);
            }
        });
    help::hint(
        ui,
        "Finish is stored as design metadata; imported drag tables retain their source values.",
    );
    let dry: f64 = assembly::dry_pieces(stage).iter().map(|p| p.0).sum();
    metric(
        ui,
        "Effective dry mass",
        format!("{:.3} lb", dry / KG_PER_LB),
    );
}
fn extra_editor(
    ui: &mut egui::Ui,
    lab: &mut Lab,
    stage: usize,
    component: usize,
    catalogue: &mut editor::Editor,
) {
    if ui.button("Move in 3D").clicked() {
        catalogue.armed = Some(builder::DropPart::Existing { stage, component });
    }
    let body_diameter = lab.draft.stages[stage].diameter_m;
    let c = &mut lab.draft.stages[stage].components[component];
    let before = serde_json::to_string(c).unwrap();
    section(ui, c.kind.label());
    ui.text_edit_singleline(&mut c.name);
    if c.kind.planar_fin() {
        egui::ComboBox::from_id_salt("fin_planform")
            .selected_text(c.kind.label())
            .show_ui(ui, |ui| {
                for kind in [
                    PartKind::FinSet,
                    PartKind::EllipticalFin,
                    PartKind::FreeformFin,
                ] {
                    let points = c.fin_outline();
                    if ui
                        .selectable_value(&mut c.kind, kind, kind.label())
                        .changed()
                        && kind == PartKind::FreeformFin
                        && c.fin_points.is_empty()
                    {
                        c.fin_points = points;
                    }
                }
            });
    }
    let old_length = c.length_m;
    let old_position = c.position_m;
    let old_offset = c.axial_offset_m;
    if number(ui, "Axial offset / m", &mut c.axial_offset_m, 0.002) {
        c.position_m += c.axial_offset_m - old_offset;
        lab.geometry_dirty = true;
    }
    help::hint(
        ui,
        "Positive offset moves toward the motor; negative moves toward the nose.",
    );
    lab.geometry_dirty |= number(ui, "Position aft stage front / m", &mut c.position_m, 0.002);
    if c.kind.planar_fin() {
        metric(ui, "Outline length / m", format!("{:.4}", c.length_m));
        lab.geometry_dirty |= number(ui, "Attachment diameter / m", &mut c.diameter_m, 0.0005);
    } else {
        for (label, value, speed) in [
            ("Length / m", &mut c.length_m, 0.001),
            ("Outer diameter / m", &mut c.diameter_m, 0.0005),
            ("Wall thickness / m", &mut c.thickness_m, 0.0001),
        ] {
            lab.geometry_dirty |= number(ui, label, value, speed);
        }
    }
    if c.kind == PartKind::Transition {
        help::hint(
            ui,
            "This booster copies the target motor and mass as a starting template. Click its body and motor in 3D to enter the actual specification.",
        );
        lab.geometry_dirty |= number(ui, "Fore diameter / m", &mut c.fore_diameter_m, 0.001);
    }
    if let Some(g) = c.shape.as_mut()
        && c.kind == PartKind::NoseCone
    {
        egui::ComboBox::from_id_salt("component_nose_shape")
            .selected_text(format!("{:?}", g.nose_shape))
            .show_ui(ui, |ui| {
                for (shape, label) in [
                    (NoseShape::Conical, "Conical"),
                    (NoseShape::TangentOgive, "Tangent ogive"),
                    (NoseShape::Elliptical, "Elliptical"),
                ] {
                    ui.selectable_value(&mut g.nose_shape, shape, label);
                }
            });
    }
    if let Some(g) = c.shape.as_mut()
        && c.kind.planar_fin()
    {
        ui.add(
            egui::DragValue::new(&mut g.fin_count)
                .range(3..=8)
                .prefix("Fin count "),
        );
        if c.kind != PartKind::FreeformFin {
            number(ui, "Root chord / m", &mut g.fin_root_chord_m, 0.001);
            number(ui, "Span / m", &mut g.fin_span_m, 0.001);
        }
        if c.kind == PartKind::FinSet {
            number(ui, "Tip chord / m", &mut g.fin_tip_chord_m, 0.001);
            number(ui, "Sweep / m", &mut g.fin_sweep_m, 0.001);
        }
        number(ui, "Fin thickness / m", &mut g.fin_thickness_m, 0.0001);
    }
    if c.kind == PartKind::FreeformFin {
        egui::CollapsingHeader::new("Freeform outline points")
            .default_open(true)
            .show(ui, |ui| {
                help::hint(
                    ui,
                    "Coordinates: aft / outboard from the root leading edge, in meters.",
                );
                ui.vertical(|ui| {
                    for (i, point) in c.fin_points.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            ui.strong(format!("Point {}", i + 1));
                            for (axis, value) in point.iter_mut().enumerate() {
                                number(
                                    ui,
                                    if axis == 0 { "Aft / m" } else { "Outboard / m" },
                                    value,
                                    0.001,
                                );
                                *value = value.clamp(0., 100.);
                            }
                        });
                        ui.add_space(6.);
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(c.fin_points.len() < 64, egui::Button::new("Add point"))
                        .clicked()
                    {
                        let n = c.fin_points.len();
                        let a = c.fin_points[n - 2];
                        let b = c.fin_points[n - 1];
                        c.fin_points
                            .insert(n - 1, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]);
                    }
                    if ui
                        .add_enabled(c.fin_points.len() > 3, egui::Button::new("Remove point"))
                        .clicked()
                    {
                        c.fin_points.remove(c.fin_points.len() - 2);
                    }
                });
            });
        if let Some(g) = &mut c.shape {
            g.fin_span_m = c.fin_points.iter().map(|p| p[1]).fold(0.0, f64::max);
            g.fin_root_chord_m = c.fin_points.last().map_or(c.length_m, |p| p[0]).max(0.001);
        }
    }
    if c.kind.planar_fin() {
        c.length_m = c.fin_outline().iter().map(|p| p[0]).fold(0.001, f64::max);
        c.cg_m = c.length_m * 0.5;
        let outline = c.fin_outline();
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 85.0), egui::Sense::hover());
        let xmax = c.length_m.max(0.001);
        let ymax = outline.iter().map(|p| p[1]).fold(0.001, f64::max);
        let points: Vec<_> = outline
            .iter()
            .map(|p| {
                egui::pos2(
                    rect.left() + 5.0 + (rect.width() - 10.0) * (p[0] / xmax) as f32,
                    rect.bottom() - 5.0 - (rect.height() - 10.0) * (p[1] / ymax) as f32,
                )
            })
            .collect();
        if points.len() >= 3 {
            ui.painter().add(egui::Shape::closed_line(
                points,
                egui::Stroke::new(1.5, theme::palette(ui).accent),
            ));
        }
    } else {
        ui.add(
            egui::DragValue::new(&mut c.instance_count)
                .range(1..=16)
                .prefix("Instance count "),
        );
        number(ui, "Radial offset / m", &mut c.radial_offset_m, 0.001);
        if c.instance_count > 1 {
            help::hint(
                ui,
                "Evenly spaced around the body. Mass is per instance; motor thrust remains configured per stage.",
            );
        }
    }
    if c.kind == PartKind::TubeFin {
        ui.checkbox(&mut c.auto_tube_diameter, "Automatic tube-fin diameter");
        c.size_tube_fins(body_diameter);
        help::hint(
            ui,
            "Automatic sizing makes adjacent tubes touch; use at least three tubes.",
        );
    }
    number(ui, "Rotation / deg", &mut c.radial_angle_deg, 1.0);
    if let Some(chute) = &mut c.recovery {
        ui.checkbox(&mut c.deploy_at_apogee, "Deploy at apogee (drogue channel)");
        chute_editor(
            ui,
            if c.kind == PartKind::Streamer {
                "STREAMER DRAG"
            } else {
                "CANOPY"
            },
            chute,
        );
        help::hint(
            ui,
            "Uses this stage’s drogue or main channel; one active device per channel. Area/Cd are prescribed recovery inputs.",
        );
    }
    material_editor(ui, "component_material", &mut c.material);
    ui.collapsing("Appearance & finish", |ui| {
        ui.color_edit_button_rgba_unmultiplied(&mut c.appearance_rgba);
        egui::ComboBox::from_id_salt("component_finish")
            .selected_text(&c.finish)
            .show_ui(ui, |ui| {
                for finish in ["Rough", "Normal", "Smooth", "Polished"] {
                    ui.selectable_value(&mut c.finish, finish.into(), finish);
                }
            });
    });
    if c.external() {
        ui.collapsing("Drag contribution", |ui| {
            let mut enabled = c.cd_override.is_some();
            if ui.checkbox(&mut enabled, "Specify component Cd").changed() {
                c.cd_override = enabled.then_some(0.0);
            }
            if let Some(cd) = &mut c.cd_override {
                number(ui, "Additional Cd", cd, 0.01);
            }
            help::hint(
                ui,
                "Adds a frontal-area-weighted contribution to the preliminary builder drag table.",
            );
        });
    }
    ui.collapsing("Comments", |ui| {
        ui.text_edit_multiline(&mut c.comments);
    });
    if c.mass_override && c.mass_kg == 0.0 {
        help::hint(
            ui,
            "Original measured dry mass is retained in the stage hardware allocation. Disable this override to use component density; recheck the stage allocation.",
        );
    }
    ui.checkbox(&mut c.mass_override, "Override component mass");
    if c.mass_override {
        pounds(ui, "Measured mass / lb", &mut c.mass_kg);
    }
    ui.checkbox(&mut c.cg_override, "Override local CG");
    if c.cg_override {
        number(ui, "CG aft part front / m", &mut c.cg_m, 0.001);
    }
    metric(
        ui,
        "Component mass",
        format!("{:.4} lb", c.mass() / KG_PER_LB),
    );
    metric(ui, "CG aft stage nose", format!("{:.4} m", c.cg()));
    help::hint(
        ui,
        "Component dimensions and density affect mass, CG and inertia. External segments enter the preliminary geometry model.",
    );
    lab.geometry_dirty |= serde_json::to_string(c).unwrap() != before;
    let shell = matches!(c.kind, PartKind::BodyTube | PartKind::NoseCone);
    let fin = c.kind.planar_fin();
    let recovery = c.recovery.clone();
    let apogee = c.deploy_at_apogee;
    if c.position_m != old_position && c.axial_offset_m == old_offset {
        c.axial_offset_m += c.position_m - old_position;
    }
    let length_delta = c.length_m - old_length;
    c.cg_m = c.cg_m.min(c.length_m);
    if shell
        && lab.draft.stages[stage]
            .materials
            .as_ref()
            .is_some_and(|m| m.component_shell)
    {
        if length_delta != 0.0 {
            assembly::shift_aft(
                &mut lab.draft.stages[stage],
                old_position + old_length,
                length_delta,
                Some(component),
            );
        }
        assembly::fit_shell(&mut lab.draft.stages[stage]);
    }
    let c = &lab.draft.stages[stage].components[component];
    if c.kind == PartKind::Transition && c.position_m.abs() < 1e-6 {
        lab.draft.stages[stage].diameter_m = c.diameter_m;
    }
    if let Some(chute) = recovery {
        let global = lab.draft.recovery.clone();
        if stage == lab.draft.stages.len() - 1 {
            if apogee {
                lab.draft.recovery.drogue = chute;
            } else {
                lab.draft.recovery.main = chute;
            }
        } else {
            let r = lab.draft.stages[stage].recovery.get_or_insert(global);
            if apogee {
                r.drogue = chute;
            } else {
                r.main = chute;
            }
        }
    }
    if fin {
        if lab.draft.stages[stage]
            .materials
            .as_ref()
            .is_some_and(|m| m.component_shell)
        {
            assembly::fit_shell(&mut lab.draft.stages[stage]);
        }
        structural_inputs(ui, lab, stage);
    }
    if ui.button("Remove this component").clicked() {
        let removed = lab.draft.stages[stage].components.remove(component);
        if removed.kind == PartKind::NoseCone {
            lab.draft.stages[stage]
                .geometry
                .as_mut()
                .unwrap()
                .nose_length_m = 0.0;
        }
        lab.selected_component = None;
        lab.geometry_dirty = true;
    }
    if let Err(e) = lab.draft.validate() {
        ui.colored_label(theme::palette(ui).danger, "Invalid part")
            .on_hover_text(e.to_string());
    }
}
fn trigger_editor(ui: &mut egui::Ui, t: &mut Trigger) {
    let old = match t {
        Trigger::Burnout { .. } => 0,
        Trigger::Time { .. } => 1,
        Trigger::Mach { .. } => 2,
        Trigger::Altitude { .. } => 3,
        Trigger::Never => 4,
    };
    let mut index = old;
    egui::ComboBox::from_id_salt("separation_trigger")
        .selected_text(
            [
                "Burnout + delay",
                "Elapsed time",
                "Mach reached",
                "Altitude reached",
                "Never",
            ][index],
        )
        .show_ui(ui, |ui| {
            for (i, n) in [
                "Burnout + delay",
                "Elapsed time",
                "Mach reached",
                "Altitude reached",
                "Never",
            ]
            .iter()
            .enumerate()
            {
                ui.selectable_value(&mut index, i, *n);
            }
        });
    if index != old {
        *t = match index {
            0 => Trigger::Burnout { delay_s: 0.0 },
            1 => Trigger::Time { time_s: 10.0 },
            2 => Trigger::Mach { mach: 1.0 },
            3 => Trigger::Altitude {
                altitude_agl_m: 3000.0,
            },
            _ => Trigger::Never,
        };
    }
    match t {
        Trigger::Burnout { delay_s } => {
            number(ui, "Delay / s", delay_s, 0.1);
        }
        Trigger::Time { time_s } => {
            number(ui, "Time / s", time_s, 0.1);
        }
        Trigger::Mach { mach } => {
            number(ui, "Mach threshold", mach, 0.05);
        }
        Trigger::Altitude { altitude_agl_m } => {
            number(ui, "Altitude AGL / m", altitude_agl_m, 10.0);
        }
        Trigger::Never => {}
    }
}
fn chute_editor(ui: &mut egui::Ui, label: &str, c: &mut Chute) {
    section(ui, label);
    ui.checkbox(&mut c.enabled, "Enabled");
    let mut diameter = (4.0 * c.area_m2 / std::f64::consts::PI).sqrt();
    if number(ui, "Diameter / m", &mut diameter, 0.01) {
        c.area_m2 = std::f64::consts::PI * diameter.max(0.01).powi(2) * 0.25;
    }
    number(ui, "Drag coefficient", &mut c.cd, 0.01);
    number(ui, "Inflation / s", &mut c.inflation_s, 0.1);
    ui.push_id(label, |ui| {
        ui.collapsing("Deployment envelope", |ui| {
            number(ui, "Maximum q / Pa", &mut c.max_q_pa, 100.0);
            number(ui, "Maximum Mach", &mut c.max_mach, 0.01);
            number(ui, "Line load / N", &mut c.max_load_n, 100.0);
        });
    });
}
fn build_summary(ui: &mut egui::Ui, lab: &Lab) {
    section(ui, "VEHICLE OVERVIEW");
    let length: f64 = lab.draft.stages.iter().map(|s| s.length_m).sum();
    let mass = crate::flight::mass_properties(&lab.draft, 0, 0.0, 0.0).kg;
    ui.columns(4, |c| {
        metric(
            &mut c[0],
            "Launch mass",
            format!("{:.2} lb", mass / KG_PER_LB),
        );
        metric(&mut c[0], "Stages", lab.draft.stages.len().to_string());
        metric(&mut c[1], "Length", format!("{length:.3} m"));
        metric(
            &mut c[1],
            "Diameter",
            format!("{:.1} mm", lab.draft.stages[0].diameter_m * 1000.0),
        );
        c[2].label(egui::RichText::new("Motors by stage").color(theme::palette(&c[2]).muted));
        for (i, s) in lab.draft.stages.iter().enumerate() {
            c[2].small(format!("S{}: {}", i + 1, s.motor.name));
        }
        metric(
            &mut c[2],
            "Payload",
            format!(
                "{:.2} lb",
                lab.draft
                    .stages
                    .iter()
                    .map(|s| s.payload_mass_kg)
                    .sum::<f64>()
                    / KG_PER_LB
            ),
        );
        let sample = &lab.flight.samples[0];
        metric(
            &mut c[3],
            "Applied CG / CP",
            format!("{:.2} / {:.2} m", sample.cg_m, sample.cp_m),
        );
        metric(
            &mut c[3],
            "Applied static margin",
            format!("{:.2} cal", sample.stability_calibers),
        );
    });
    ui.separator();
    help::hint(
        ui,
        "1  Add parts in 3D     2  Click components to edit properties     3  Launch simulation     4  Set launch conditions and simulate",
    );
    help::hint(
        ui,
        "3D preview follows the draft. CG/CP and flight update after applying it. Builder aero is preliminary; use validated coefficients for flight predictions.",
    );
}
fn conditions(ui: &mut egui::Ui, lab: &mut Lab, job: &mut builder::BuildJob) {
    use units::edit_launch as number;
    section(ui, "LAUNCH CONDITIONS");
    let m = &mut lab.draft;
    number(ui, "Site altitude MSL / m", &mut m.launch.altitude_m, 10.0);
    number(ui, "Latitude / deg", &mut m.launch.latitude_deg, 0.1);
    number(ui, "Longitude / deg", &mut m.launch.longitude_deg, 0.1);
    number(ui, "Rail elevation / deg", &mut m.launch.elevation_deg, 0.1);
    number(ui, "Azimuth / deg", &mut m.launch.azimuth_deg, 0.5);
    number(ui, "Rail length / m", &mut m.launch.rail_length_m, 0.1);
    ui.checkbox(&mut m.launch.earth_rotation, "Earth rotation");
    section(ui, "WIND PROFILE");
    let mut speed = m.wind.east_mps.hypot(m.wind.north_mps);
    let mut heading = (-m.wind.east_mps)
        .atan2(-m.wind.north_mps)
        .to_degrees()
        .rem_euclid(360.0);
    let changed = number(ui, "Mean speed / m/s", &mut speed, 0.1)
        | number(ui, "From bearing / deg", &mut heading, 1.0);
    if changed {
        m.wind.east_mps = -speed * heading.to_radians().sin();
        m.wind.north_mps = -speed * heading.to_radians().cos();
    }
    number(ui, "Gust amplitude / m/s", &mut m.wind.gust_mps, 0.1);
    number(ui, "Gust period / s", &mut m.wind.gust_period_s, 0.1);
    number(ui, "Shear / 1/km", &mut m.wind.shear_per_km, 0.01);
    section(ui, "ATMOSPHERE");
    if ui
        .checkbox(&mut m.atmosphere.custom, "Use measured site weather")
        .changed()
        && m.atmosphere.custom
    {
        let a = crate::atmosphere::sample(m.launch.altitude_m);
        m.atmosphere.temperature_c = a.temperature - 273.15;
        m.atmosphere.pressure_hpa = a.pressure / 100.0;
    }
    ui.add_enabled_ui(m.atmosphere.custom, |ui| {
        number(
            ui,
            "Site temperature / °C",
            &mut m.atmosphere.temperature_c,
            0.1,
        );
        number(
            ui,
            "Station pressure / hPa",
            &mut m.atmosphere.pressure_hpa,
            0.5,
        );
    });
    help::hint(
        ui,
        "Station pressure is measured at the site. Dry-air lapse profile is anchored to this temperature and pressure.",
    );
    ui.collapsing("Cloud layer · visual", |ui| {
        ui.add(egui::Slider::new(&mut m.atmosphere.cloud_cover, 0.0..=1.0).text("cover"));
        number(
            ui,
            "Cloud base AGL / m",
            &mut m.atmosphere.cloud_base_agl_m,
            50.0,
        );
        number(
            ui,
            "Thickness / m",
            &mut m.atmosphere.cloud_thickness_m,
            25.0,
        );
    });
    ui.separator();
    if primary(ui, "Apply & launch", !job.busy()) {
        builder::launch(lab, job);
    }
    help::hint(
        ui,
        "The projected arc uses the complete integrated flight for the applied conditions. Wind is deterministic, not a forecast.",
    );
}
fn telemetry(ui: &mut egui::Ui, lab: &mut Lab, image_view: bool) {
    let sample = crate::analysis::sample_at(&lab.flight, lab.time);
    if let Some(stage) = lab.followed_stage {
        section(ui, &format!("FOLLOWING DETACHED S{}", stage + 1));
        if let Some(d) = lab
            .flight
            .debris_samples
            .iter()
            .rev()
            .find(|d| d.stage == stage && d.time_s <= lab.time)
        {
            metric(ui, "Altitude AGL", format!("{:.0} m", d.altitude_agl_m));
            metric(ui, "Mach", format!("{:.3}", d.mach));
            metric(
                ui,
                "Dynamic pressure",
                units::value(ui, "Dynamic pressure", d.q_pa, "Pa", 2),
            );
            metric(
                ui,
                "Drogue / main",
                format!(
                    "{:.0}% / {:.0}%",
                    d.drogue_fraction * 100.0,
                    d.main_fraction * 100.0
                ),
            );
            metric(ui, "Line load", format!("{:.0} N", d.chute_load_n));
            if d.landed {
                metric(ui, "Touchdown", format!("{:.2} m/s", d.landing_speed_mps));
            }
        } else {
            help::hint(ui, "This stage has not separated at the selected time.");
        }
        ui.separator();
    }
    section(ui, "RETAINED VEHICLE TELEMETRY");
    if lab.flight.mission.stages[sample.stage].model_mass.is_some() {
        help::hint(
            ui,
            "Flight uses exported 3D mass, full inertia and crossflow shape.",
        );
    }
    ui.label(
        egui::RichText::new(format!("{:?} / STAGE {}", sample.regime, sample.stage + 1))
            .color(theme::palette(ui).accent),
    );
    metric(
        ui,
        "Altitude AGL",
        units::value(ui, "Altitude AGL", sample.altitude_agl_m, "m", 0),
    );
    metric(ui, "Mach", format!("{:.3}", sample.mach));
    metric(
        ui,
        "Ground speed",
        units::value(ui, "Ground speed", sample.speed_mps, "m/s", 0),
    );
    metric(
        ui,
        "Dynamic pressure",
        units::value(ui, "Dynamic pressure", sample.q_pa, "Pa", 2),
    );
    metric(
        ui,
        "Specific force",
        format!("{:.1} g", sample.specific_force_g),
    );
    let air = crate::atmosphere::for_mission(sample.altitude_m, &lab.flight.mission);
    let wind = lab
        .flight
        .mission
        .wind
        .velocity(sample.altitude_agl_m, sample.time_s);
    ui.collapsing("Ambient atmosphere", |ui| {
        metric(ui, "Pressure", format!("{:.2} hPa", air.pressure / 100.0));
        metric(
            ui,
            "Temperature",
            format!("{:.1} °C", air.temperature - 273.15),
        );
        metric(ui, "Density", format!("{:.4} kg/m³", air.density));
        metric(ui, "Wind", format!("{:.1} m/s", wind.length()));
    });
    ui.collapsing("Attitude & rotation",|ui|{
        metric(ui,"Pitch rate",format!("{:.2} °/s",sample.pitch_rate_deg_s));
        metric(ui,"Yaw rate",format!("{:.2} °/s",sample.yaw_rate_deg_s));
        metric(ui,"Roll rate",format!("{:.2} °/s",sample.roll_rate_deg_s));
        let q=glam::DQuat::from_xyzw(sample.qx,sample.qy,sample.qz,sample.qw);
        metric(ui,"Tilt / launch vertical",format!("{:.1}°",(q*glam::DVec3::Z).angle_between(glam::DVec3::Z).to_degrees()));
        help::hint(ui, "Rotation follows aerodynamic, thrust, suspension and damping torques and body inertia.");
    });
    if !image_view {
        section(ui, "SURFACE VIEW");
        surface_legend(ui, lab, &sample);
        ui.checkbox(&mut lab.streamlines, "Airflow paths");
        ui.checkbox(&mut lab.mach_cone, "Mach-angle guide");
        egui::ComboBox::from_label("Focus")
            .selected_text(["Whole vehicle", "Forward body", "Stage fins", "Nose tip"][lab.focus])
            .show_ui(ui, |ui| {
                for (i, label) in ["Whole vehicle", "Forward body", "Stage fins", "Nose tip"]
                    .iter()
                    .enumerate()
                {
                    if ui.selectable_value(&mut lab.focus, i, *label).clicked() {
                        lab.inspection = true;
                        lab.overview = false;
                        lab.zoom = 4.0;
                        lab.pan_offset = Vec3::ZERO;
                    }
                }
            });
    }
    ui.collapsing("Mass & stability", |ui| {
        metric(ui, "Mass", format!("{:.2} lb", sample.mass_kg / KG_PER_LB));
        metric(
            ui,
            "CG / CP",
            format!("{:.2} / {:.2} m", sample.cg_m, sample.cp_m),
        );
        metric(
            ui,
            "Static margin",
            format!("{:.2} cal", sample.stability_calibers),
        );
        metric(ui, "Angle of attack", format!("{:.2}°", sample.alpha_deg));
    });
    ui.collapsing("Recovery & avionics", |ui| {
        metric(
            ui,
            "Drogue / main",
            format!(
                "{:.0}% / {:.0}%",
                sample.drogue_fraction * 100.0,
                sample.main_fraction * 100.0
            ),
        );
        metric(ui, "Line load", format!("{:.0} N", sample.chute_load_n));
        metric(ui, "Battery", format!("{:.2} Wh", sample.battery_wh));
        ui.small(format!(
            "Barometer {} · GNSS {}",
            if sample.baro_valid {
                "valid"
            } else {
                "outside envelope"
            },
            if sample.gnss_valid {
                "valid"
            } else {
                "outside envelope"
            }
        ));
    });
    section(ui, "PROJECTED RESULT");
    metric(
        ui,
        "Apogee",
        units::value(ui, "Apogee", lab.flight.summary.apogee_agl_m, "m", 0),
    );
    metric(
        ui,
        "Peak Mach",
        format!("{:.2}", lab.flight.summary.max_mach),
    );
    metric(
        ui,
        "Peak q",
        format!("{:.2} kPa", lab.flight.summary.max_q_pa / 1000.0),
    );
}
fn analysis(
    ui: &mut egui::Ui,
    lab: &mut Lab,
    snapshots: &mut snapshot::Snapshots,
    thermal: &mut thermal_view::ThermalView,
) {
    ui.horizontal(|ui| {
        if ui
            .button(if lab.playing { "Pause" } else { "Play" })
            .clicked()
        {
            lab.playing = !lab.playing;
        }
        if ui.button("Rewind flight").clicked() {
            lab.time = 0.0;
            lab.playing = false;
        }
        if ui
            .button("Apogee")
            .on_hover_text("Inspect the integrated attitude and rotation at apogee")
            .clicked()
            && let Some(event) = lab.flight.events.iter().find(|e| e.kind == "apogee")
        {
            lab.time = event.time_s;
            lab.playing = false;
            lab.inspection = true;
            lab.overview = false;
            lab.free_camera = None;
            lab.focus = 0;
        }
        let end = lab.flight.summary.end_time_s;
        if ui
            .button("Back")
            .on_hover_text("Previous timestamp / Left arrow")
            .clicked()
        {
            seek(lab, lab.time - lab.seek_step);
        }
        if ui
            .button("Forward")
            .on_hover_text("Next timestamp / Right arrow")
            .clicked()
        {
            seek(lab, lab.time + lab.seek_step);
        }
        ui.add(
            egui::DragValue::new(&mut lab.seek_step)
                .range(0.001..=60.0)
                .speed(0.1)
                .suffix(" s step"),
        );
        if ui
            .add(egui::Slider::new(&mut lab.time, 0.0..=end).show_value(false))
            .changed()
        {
            seek(lab, lab.time);
        }
        if ui
            .add(
                egui::DragValue::new(&mut lab.time)
                    .range(0.0..=end)
                    .speed(0.01)
                    .max_decimals(3)
                    .prefix("T+ ")
                    .suffix(" s"),
            )
            .changed()
        {
            seek(lab, lab.time);
        }
        ui.add(
            egui::Slider::new(&mut lab.speed, 0.25..=100.0)
                .logarithmic(true)
                .text("×"),
        );
    });
    ui.horizontal(|ui| {
        for (i, n) in [
            "Trajectory",
            "Aerodynamics",
            "Thermal",
            "Events",
            "Model limits",
            "Export",
            "Analysis",
        ]
        .iter()
        .enumerate()
        {
            ui.selectable_value(&mut lab.tab, i, *n);
        }
    });
    if lab.tab == 6 {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut snapshots.image_view, true, "2D images");
            ui.selectable_value(&mut snapshots.image_view, false, "3D vehicle");
        });
    }
    match lab.tab {
        0 => ui.columns(2, |c| {
            chart(
                &mut c[0],
                "ALTITUDE AGL / m",
                &lab.flight,
                lab.time,
                |s| s.altitude_agl_m,
                &lab.reference,
            );
            chart(
                &mut c[1],
                "AIR-RELATIVE MACH",
                &lab.flight,
                lab.time,
                |s| s.mach,
                &[],
            );
        }),
        1 => ui.columns(2, |c| {
            chart(
                &mut c[0],
                "DYNAMIC PRESSURE / Pa",
                &lab.flight,
                lab.time,
                |s| s.q_pa,
                &[],
            );
            chart(
                &mut c[1],
                "ANGLE OF ATTACK / deg",
                &lab.flight,
                lab.time,
                |s| s.alpha_deg,
                &[],
            );
        }),
        2 => ui.columns(2, |c| {
            chart(
                &mut c[0],
                "THERMAL SCREENING / K",
                &lab.flight,
                lab.time,
                |s| s.temperature_k,
                &[],
            );
            chart(
                &mut c[1],
                "STAGNATION HEATING / W/m²",
                &lab.flight,
                lab.time,
                |s| s.heat_flux_w_m2,
                &[],
            );
        }),
        3 => {
            egui::ScrollArea::vertical()
                .max_height(ui.available_height().max(20.0) - 18.0)
                .show(ui, |ui| {
                    for e in lab
                        .flight
                        .events
                        .iter()
                        .filter(|e| e.time_s <= lab.time)
                        .rev()
                    {
                        ui.label(format!(
                            "T+ {:7.2}  S{}  {}  {}",
                            e.time_s,
                            e.stage + 1,
                            e.kind,
                            e.detail
                        ));
                    }
                });
        }
        4 => {
            egui::ScrollArea::vertical().max_height(ui.available_height().max(20.0) - 18.0).show(ui,|ui|{
            help::hint(ui, "Engineering prototype. Geometry-based aero is preliminary; imported example aero is provisional. 3D pressure/airflow maps use incidence approximations; Analysis computes a separate planar inviscid Euler section. Spatial heating uses prescribed boundaries and material diffusion. Structural/ablation screening has no fracture, flutter or removed-mass feedback. Above 86 km the atmosphere is approximate; real-gas and rarefied effects require validated external data.");
            for s in &lab.flight.mission.stages {ui.small(&s.aero_source);}
            for w in &lab.flight.summary.warnings {ui.colored_label(theme::palette(ui).warning,"Model warning").on_hover_text(w);}
        });
        }
        6 => {
            if snapshots.image_view {
                snapshot::summary(ui, snapshots);
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(ui.available_height().max(20.0) - 18.0)
                    .show(ui, |ui| snapshot::show(ui, snapshots, lab, thermal));
            }
        }
        _ => {
            ui.horizontal(|ui| {
                ui.label("Output folder");
                ui.text_edit_singleline(&mut lab.save_path);
                if ui.button("Export CSV + JSON").clicked() {
                    lab.status = match export::write_flight(&lab.flight, &lab.save_path) {
                        Ok(()) => format!("Exported {}", lab.save_path),
                        Err(e) => e.to_string(),
                    };
                }
            });
            ui.horizontal(|ui| {
                ui.label("Reference CSV");
                ui.text_edit_singleline(&mut lab.reference_path);
                if ui.button("Load reference").clicked() {
                    match imports::reference_flight(&lab.reference_path) {
                        Ok(r) => {
                            lab.reference = r;
                            lab.status = "Reference loaded".into();
                        }
                        Err(e) => lab.status = e.to_string(),
                    }
                }
            });
        }
    }
    help::hint(
        ui,
        "Cyan: flown arc   Amber: projected arc / reference   White: vehicle / apogee   Space: play/pause   Left/Right: step   Home: rewind",
    );
}
fn editor(ctx: &egui::Context, lab: &mut Lab, job: &builder::BuildJob) {
    if !lab.editor_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Mission JSON / advanced coefficients")
        .open(&mut open)
        .default_size([740.0, 620.0])
        .show(ctx, |ui| {
            if ui
                .add_enabled(!job.busy(), egui::Button::new("Validate & apply"))
                .clicked()
            {
                let result = serde_json::from_str::<Mission>(&lab.editor_text)
                    .map_err(|e| e.to_string())
                    .and_then(|mut m| {
                        builder::initialize_geometry(&mut m);
                        m.validate().map_err(|e| e.to_string())?;
                        Ok(m)
                    });
                match result {
                    Ok(m) => {
                        apply_mission(lab, m);
                        lab.geometry_dirty = false;
                        lab.selected_stage = lab.selected_stage.min(lab.draft.stages.len() - 1);
                    }
                    Err(e) => lab.status = e,
                }
            }
            help::status(ui, &lab.status);
            egui::ScrollArea::both().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut lab.editor_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(30),
                );
            });
        });
    lab.editor_open = open;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sidebar_text_and_values_stay_clear_of_scrollbars_in_both_themes() {
        for theme in [theme::Theme::Dark, theme::Theme::Light] {
            for width in [190., 265., 400.] {
                let ctx = egui::Context::default();
                let mut value = 12.5;
                for frame in 0..12 {
                    let mut inner = egui::Rect::NOTHING;
                    let mut outer = egui::Rect::NOTHING;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1440., 900.),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            theme.apply(ctx);
                            let panel =
                                navigation_panel(ctx).default_width(width).show(ctx, |ui| {
                                    outer = ui.available_rect_before_wrap();
                                    inner = sidebar_scroll(ui, |ui| {
                                        for _ in 0..20 {
                                            units::edit_launch(ui, "Launch elevation / deg", &mut value, 0.1);
                                            units::edit_launch(ui, "Mean speed / m/s", &mut value, 0.1);
                                            number(
                                                ui,
                                                "Thermal conductivity / W/(m·K)",
                                                &mut value,
                                                0.1,
                                            );
                                        }
                                    })
                                    .inner_rect;
                                });
                            assert!((panel.response.rect.width() - width).abs() < 0.5, "sidebar {width} grew to {} on frame {frame}", panel.response.rect.width());
                        },
                    );
                    if frame < 2 {
                        continue;
                    }
                    assert!(
                        outer.right() - inner.right() >= 15.9,
                        "missing scrollbar gutter"
                    );
                    let mut visible_labels = 0;
                    for shape in output.shapes {
                        if let egui::Shape::Text(text) = shape.shape {
                            let rect = text.galley.rect.translate(text.pos.to_vec2());
                            if rect.intersects(inner) {
                                if text.galley.job.text == "mph" {
                                    assert!(text.galley.job.sections.iter().all(|s| s.format.font_id.size >= 20.), "launch units must be larger");
                                }
                                assert!(rect.left() >= inner.left() - 0.5, "left-clipped text");
                                if text.galley.job.text == "Launch elevation" {
                                    assert!((rect.left() - inner.left()).abs() < 0.5, "labels must stay left-aligned");
                                }
                                assert!(
                                    rect.right() <= inner.right() + 0.5,
                                    "{} at {width}: {} extends into the gutter: {rect:?}, {inner:?}",
                                    if theme == theme::Theme::Dark {
                                        "dark"
                                    } else {
                                        "light"
                                    },
                                    text.galley.job.text
                                );
                                assert!(
                                    shape.clip_rect.right() < outer.right() - 8.,
                                    "text clip overlaps scrollbar"
                                );
                                visible_labels += 1;
                            }
                        }
                    }
                    assert!(visible_labels >= 12);
                }
            }
        }
    }
    #[test]
    fn charts_cannot_grow_the_bottom_panel_between_frames() {
        let mut mission = Mission::demo(false);
        mission.duration = 1.0;
        let flight = Simulator::new(mission).unwrap().run();
        let ctx = egui::Context::default();
        let mut heights = Vec::new();
        for _ in 0..120 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440., 940.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    theme::Theme::Dark.apply(ctx);
                    let response = analysis_panel(ctx, 244.).show(ctx, |ui| {
                        let mut content = bounded_panel_content(ui);
                        content.horizontal(|ui| {
                            ui.button("Play").clicked();
                            ui.button("Rewind").clicked();
                        });
                        content.horizontal(|ui| {
                            ui.label("Trajectory");
                            ui.label("Aerodynamics");
                        });
                        content.columns(2, |cols| {
                            chart(
                                &mut cols[0],
                                "ALTITUDE",
                                &flight,
                                0.,
                                |s| s.altitude_agl_m,
                                &[],
                            );
                            chart(&mut cols[1], "MACH", &flight, 0., |s| s.mach, &[]);
                        });
                        content.small("Flown arc / projected arc / time controls");
                    });
                    heights.push(response.response.rect.height());
                },
            );
        }
        assert!(
            heights.iter().all(|h| (*h - 244.).abs() < 1.),
            "panel heights: {heights:?}"
        );
    }
    #[test]
    fn all_panel_edges_resize_and_keep_their_size_across_frames() {
        fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> [egui::Rect; 3] {
            let mut rects = [egui::Rect::NOTHING; 3];
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440., 940.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    theme::Theme::Dark.apply(ctx);
                    rects[2] = analysis_panel(ctx, 244.)
                        .show(ctx, |ui| {
                            let mut content = bounded_panel_content(ui);
                            content.label("Timeline and analysis");
                        })
                        .response
                        .rect;
                    rects[0] = navigation_panel(ctx)
                        .show(ctx, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.label("Parts");
                                });
                        })
                        .response
                        .rect;
                    rects[1] = inspector_panel(ctx)
                        .show(ctx, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.label("Properties");
                                });
                        })
                        .response
                        .rect;
                    egui::CentralPanel::default().show(ctx, |_| {});
                },
            );
            rects
        }
        let edge = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        let ctx = egui::Context::default();
        let mut rects = frame(&ctx, vec![]);
        for index in 0..3 {
            let before = rects[index];
            let origin = match index {
                0 => before.right_center(),
                1 => before.left_center(),
                _ => before.center_top(),
            };
            let destination = origin
                + match index {
                    0 => egui::vec2(80., 0.),
                    1 => egui::vec2(-80., 0.),
                    _ => egui::vec2(0., -100.),
                };
            frame(&ctx, vec![egui::Event::PointerMoved(origin)]);
            frame(&ctx, vec![edge(origin, true)]);
            frame(&ctx, vec![egui::Event::PointerMoved(destination)]);
            frame(&ctx, vec![]);
            frame(&ctx, vec![edge(destination, false)]);
            rects = frame(&ctx, vec![]);
            let change = if index == 2 {
                rects[index].height() - before.height()
            } else {
                rects[index].width() - before.width()
            };
            assert!(
                (change - if index == 2 { 100. } else { 80. }).abs() < 2.,
                "panel {index}: changed {change}"
            );
            assert_eq!(rects, frame(&ctx, vec![]));
        }
        assert!(ctx.available_rect().width() > 400.);
        assert!(ctx.available_rect().height() > 400.);
    }
    #[test]
    fn held_pointer_drag_transfers_palette_part_to_viewport() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut source = egui::Rect::NOTHING;
        let mut dropped = None;
        {
            let mut frame = |events: Vec<egui::Event>| {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::SidePanel::left("test_palette")
                            .exact_width(200.0)
                            .show(ctx, |ui| {
                                source = palette_button(
                                    ui,
                                    0,
                                    "Bulkhead",
                                    builder::DropPart::Extra(PartKind::Bulkhead),
                                    None,
                                )
                                .rect;
                            });
                        if let Some(payload) = release_part(ctx, ctx.available_rect()) {
                            dropped = Some(payload);
                        }
                    },
                );
            };
            frame(vec![]);
        }
        let origin = source.center();
        let destination = egui::pos2(500.0, 300.0);
        let edge = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [
            vec![egui::Event::PointerMoved(origin), edge(origin, true)],
            vec![egui::Event::PointerMoved(origin + egui::vec2(20.0, 0.0))],
            vec![egui::Event::PointerMoved(destination)],
            vec![edge(destination, false)],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::SidePanel::left("test_palette")
                        .exact_width(200.0)
                        .show(ctx, |ui| {
                            palette_button(
                                ui,
                                0,
                                "Bulkhead",
                                builder::DropPart::Extra(PartKind::Bulkhead),
                                None,
                            );
                        });
                    if let Some(payload) = release_part(ctx, ctx.available_rect()) {
                        dropped = Some(payload);
                    }
                },
            );
        }
        assert!(matches!(
            dropped,
            Some(builder::DropPart::Extra(PartKind::Bulkhead))
        ));
        assert!(!egui::DragAndDrop::has_any_payload(&ctx));
    }
}

#[cfg(test)]
mod parts_tests {
    use super::*;
    #[test]
    fn catalogue_has_the_core_openrocket_families_without_duplicates_or_stage_card() {
        let cards = palette_entries();
        assert_eq!(cards.len(), 22);
        let labels: std::collections::HashSet<_> = cards.iter().map(|c| c.1).collect();
        assert_eq!(labels.len(), cards.len());
        assert!(
            labels
                .iter()
                .all(|label| !label.to_lowercase().contains("booster"))
        );
        for label in [
            "Elliptical fins",
            "Freeform fins",
            "Tube fins",
            "Launch lug",
            "Engine block",
            "Streamer",
        ] {
            assert!(labels.contains(label), "missing {label}");
        }
        for category in 0..4 {
            assert!(cards.iter().any(|c| c.0 == category));
        }
    }
}
