//! Display conversions only. Mission data, solvers and raw CSVs remain in SI.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) enum System {
    Metric,
    #[default]
    Imperial,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Conversion {
    pub unit: &'static str,
    pub scale: f64,
    pub offset: f64,
}
impl Conversion {
    pub fn display(self, value: f64) -> f64 {
        value * self.scale + self.offset
    }
    pub fn stored(self, value: f64) -> f64 {
        (value - self.offset) / self.scale
    }
}
impl System {
    pub fn conversion(self, label: &str, source: &str) -> Conversion {
        let feet = label.contains("altitude")
            || label.contains("Altitude")
            || label.contains("AGL")
            || label.contains("MSL")
            || label.contains("ceiling")
            || label.contains("Apogee")
            || label.contains("Rail length")
            || label.contains("axis")
            || label.contains("position / m")
            || label.contains("station / m")
            || label.contains("Station aft vehicle")
            || label.contains("Lateral position")
            || label.contains("Crossflow")
            || label.contains("Fin span from root")
            || label.contains("Aft of root");
        let wind = label.contains("Wind")
            || label.contains("wind")
            || label.contains("Mean speed")
            || label.contains("Gust");
        let (unit, scale, offset) = match (self, source) {
            (Self::Imperial, "m") if feet => ("ft", 1. / 0.3048, 0.),
            (Self::Imperial, "m") => ("in", 1. / 0.0254, 0.),
            (Self::Imperial, "mm") => ("in", 1. / 25.4, 0.),
            (Self::Imperial, "km") if feet => ("ft", 1000. / 0.3048, 0.),
            (Self::Imperial, "km") => ("mi", 1. / 1.609344, 0.),
            (Self::Imperial, "1/km") => ("1/mi", 1.609344, 0.),
            (Self::Imperial, "m/s") if wind => ("mph", 1. / 0.44704, 0.),
            (Self::Imperial, "m/s") => ("ft/s", 1. / 0.3048, 0.),
            (Self::Imperial, "m/s²") => ("ft/s²", 1. / 0.3048, 0.),
            (Self::Imperial, "m²") => ("ft²", 1. / 0.3048_f64.powi(2), 0.),
            (Self::Imperial, "m³") => ("ft³", 1. / 0.3048_f64.powi(3), 0.),
            (Self::Imperial, "kg") => ("lb", 1. / 0.45359237, 0.),
            (Self::Metric, "lb") => ("kg", 0.45359237, 0.),
            (Self::Imperial, "kg/m³") => ("lb/ft³", 0.3048_f64.powi(3) / 0.45359237, 0.),
            (Self::Metric, "lb/ft³") => ("kg/m³", 0.45359237 / 0.3048_f64.powi(3), 0.),
            (Self::Imperial, "Pa") => ("psi", 1. / 6894.757293168, 0.),
            (Self::Imperial, "kPa") => ("psi", 1000. / 6894.757293168, 0.),
            (Self::Imperial, "hPa") => ("inHg", 100. / 3386.389, 0.),
            (Self::Imperial, "MPa") => ("ksi", 1e3 / 6894.757293168, 0.),
            (Self::Imperial, "GPa") => ("Msi", 1e3 / 6894.757293168, 0.),
            (Self::Imperial, "N") => ("lbf", 1. / 4.4482216152605, 0.),
            (Self::Imperial, "N·s" | "N s") => ("lbf·s", 1. / 4.4482216152605, 0.),
            (Self::Imperial, "N·m") => ("lbf·ft", 1. / 1.3558179483314, 0.),
            (Self::Imperial, "kg·m²") => ("lb·ft²", 1. / (0.45359237 * 0.3048_f64.powi(2)), 0.),
            (Self::Imperial, "°C") => ("°F", 1.8, 32.),
            (Self::Imperial, "K") => ("°F", 1.8, -459.67),
            (Self::Imperial, "ΔK") => ("Δ°F", 1.8, 0.),
            (Self::Imperial, "W/(m·K)") => {
                ("Btu/(h·ft·°F)", 3600. * 0.3048 / (1055.05585262 * 1.8), 0.)
            }
            (Self::Imperial, "J/(kg·K)") => {
                ("Btu/(lb·°F)", 0.45359237 / (1055.05585262 * 1.8), 0.)
            }
            (Self::Imperial, "J/K") => ("Btu/°F", 1. / (1055.05585262 * 1.8), 0.),
            (Self::Imperial, "J") => ("Btu", 1. / 1055.05585262, 0.),
            (Self::Imperial, "W") => ("Btu/h", 3600. / 1055.05585262, 0.),
            (Self::Imperial, "Wh") => ("Btu", 3600. / 1055.05585262, 0.),
            (Self::Imperial, "W/m²") => (
                "Btu/(h·ft²)",
                3600. * 0.3048_f64.powi(2) / 1055.05585262,
                0.,
            ),
            (_, "m") => ("m", 1., 0.),
            (_, "mm") => ("mm", 1., 0.),
            (_, "km") => ("km", 1., 0.),
            (_, "1/km") => ("1/km", 1., 0.),
            (_, "m/s") => ("m/s", 1., 0.),
            (_, "m/s²") => ("m/s²", 1., 0.),
            (_, "m²") => ("m²", 1., 0.),
            (_, "m³") => ("m³", 1., 0.),
            (_, "kg") => ("kg", 1., 0.),
            (_, "lb") => ("lb", 1., 0.),
            (_, "kg/m³") => ("kg/m³", 1., 0.),
            (_, "lb/ft³") => ("lb/ft³", 1., 0.),
            (_, "Pa") => ("Pa", 1., 0.),
            (_, "kPa") => ("kPa", 1., 0.),
            (_, "hPa") => ("hPa", 1., 0.),
            (_, "MPa") => ("MPa", 1., 0.),
            (_, "GPa") => ("GPa", 1., 0.),
            (_, "N") => ("N", 1., 0.),
            (_, "N·s" | "N s") => ("N·s", 1., 0.),
            (_, "N·m") => ("N·m", 1., 0.),
            (_, "kg·m²") => ("kg·m²", 1., 0.),
            (_, "°C") => ("°C", 1., 0.),
            (_, "K") => ("K", 1., 0.),
            (_, "ΔK") => ("ΔK", 1., 0.),
            (_, "W/(m·K)") => ("W/(m·K)", 1., 0.),
            (_, "J/(kg·K)") => ("J/(kg·K)", 1., 0.),
            (_, "J/K") => ("J/K", 1., 0.),
            (_, "J") => ("J", 1., 0.),
            (_, "W") => ("W", 1., 0.),
            (_, "Wh") => ("Wh", 1., 0.),
            (_, "W/m²") => ("W/m²", 1., 0.),
            (_, "deg") => ("°", 1., 0.),
            (_, "deg/s") => ("°/s", 1., 0.),
            (_, "s") => ("s", 1., 0.),
            (_, "Hz") => ("Hz", 1., 0.),
            (_, "g") => ("g", 1., 0.),
            _ => ("", 1., 0.),
        };
        Conversion {
            unit,
            scale,
            offset,
        }
    }
    pub fn field(self, label: &str) -> (String, Conversion) {
        let (name, source) = label.split_once(" / ").unwrap_or((label, ""));
        (name.into(), self.conversion(label, source))
    }
    pub fn label(self, label: &str) -> String {
        let (name, c) = self.field(label);
        if c.unit.is_empty() {
            label.into()
        } else {
            format!("{name} / {}", c.unit)
        }
    }
}
pub(super) fn system(ui: &egui::Ui) -> System {
    ui.ctx().data_mut(|data| {
        *data.get_temp_mut_or_insert_with(egui::Id::new("display_units"), || {
            std::fs::read_to_string("output/ui-units.json")
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        })
    })
}
pub(super) fn selector(ui: &mut egui::Ui) {
    let mut selected = system(ui);
    let previous = selected;
    egui::ComboBox::from_id_salt("unit_system").selected_text(match selected {System::Imperial=>"Imperial",System::Metric=>"Metric"}).width(94.).show_ui(ui,|ui| {
        ui.selectable_value(&mut selected,System::Imperial,"Imperial");
        ui.selectable_value(&mut selected,System::Metric,"Metric");
    }).response.on_hover_text("Display units for properties, weather, telemetry, charts and analysis images. Raw mission files and solver CSVs retain their labeled SI units.");
    if selected != previous {
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("display_units"), selected));
        if std::fs::create_dir_all("output").is_ok() {
            let _ = std::fs::write(
                "output/ui-units.json",
                serde_json::to_string(&selected).unwrap(),
            );
        }
    }
}
pub(super) fn value(
    ui: &egui::Ui,
    label: &str,
    value: f64,
    source: &str,
    decimals: usize,
) -> String {
    let c = system(ui).conversion(label, source);
    format!(
        "{:.*} {}",
        decimals,
        c.display(value),
        if c.unit.is_empty() { source } else { c.unit }
    )
}
pub(super) fn formatted(ui: &egui::Ui, label: &str, value: &str) -> (String, String) {
    let sys = system(ui);
    let (name, label_conversion) = sys.field(label);
    let (numbers, source) = value.rsplit_once(' ').unwrap_or((value, ""));
    let mut c = sys.conversion(label, source);
    let numbers = if c.unit.is_empty() && !label_conversion.unit.is_empty() {
        c = label_conversion;
        value
    } else {
        numbers
    };
    if c.unit.is_empty() {
        return (label.into(), value.into());
    }
    let tokens: Vec<_> = numbers
        .split_whitespace()
        .map(|token| {
            token
                .parse::<f64>()
                .map(|v| {
                    format!(
                        "{:.*}",
                        token.split_once('.').map_or(0, |(_, d)| d.len()),
                        c.display(v)
                    )
                })
                .unwrap_or_else(|_| token.into())
        })
        .collect();
    (name, format!("{} {}", tokens.join(" "), c.unit))
}
/// Scrub, type or step; write back only after a real edit to prevent conversion drift.
pub(super) fn edit(ui: &mut egui::Ui, label: &str, stored: &mut f64, speed: f64) -> bool {
    edit_with_unit_font(ui, label, stored, speed, egui::TextStyle::Small.resolve(ui.style()))
}
pub(super) fn edit_launch(ui: &mut egui::Ui, label: &str, stored: &mut f64, speed: f64) -> bool {
    edit_with_unit_font(ui, label, stored, speed, egui::FontId::proportional(20.))
}
fn edit_with_unit_font(ui: &mut egui::Ui, label: &str, stored: &mut f64, speed: f64, unit_font: egui::FontId) -> bool {
    let (name, c) = system(ui).field(label);
    let mut shown = c.display(*stored);
    let raw_step = (speed * c.scale.abs()).max(1e-8);
    let step = if system(ui) == System::Imperial {
        let power = 10f64.powf(raw_step.log10().floor());
        [1., 2., 5., 10.]
            .into_iter()
            .map(|v| v * power)
            .min_by(|a, b| (a - raw_step).abs().total_cmp(&(b - raw_step).abs()))
            .unwrap()
    } else {
        raw_step
    };
    let mut changed = false;
    ui.push_id(label,|ui| {
        ui.horizontal(|ui| {
            let unit_galley = ui.fonts_mut(|fonts| {
                fonts.layout_no_wrap(c.unit.to_string(), unit_font.clone(), ui.visuals().text_color())
            });
            let unit_width = unit_galley.size().x.ceil() + 2.;
            let name_width = (ui.available_width() - unit_width - ui.spacing().item_spacing.x).floor().max(20.);
            ui.allocate_ui_with_layout(egui::vec2(name_width, 0.), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_min_width(name_width);
                ui.add(egui::Label::new(egui::RichText::new(name).strong()).wrap());
            });
            let (unit_rect, response) = ui.allocate_exact_size(egui::vec2(unit_width, unit_galley.size().y), egui::Sense::hover());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), c.unit));
            ui.painter().galley(unit_rect.right_center() - egui::vec2(unit_galley.size().x, unit_galley.size().y * 0.5), unit_galley, ui.visuals().text_color());
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding.x = 4.;
            let button_width=36.0_f32.max(ui.fonts_mut(|f|f.glyph_width(&egui::TextStyle::Button.resolve(ui.style()),'−'))+2.*ui.spacing().button_padding.x);
            if ui.add_sized([button_width,32.],egui::Button::new("−")).on_hover_text("Decrease one step").clicked() {shown-=step;changed=true;}
            changed |= ui.add_sized([(ui.available_width()-button_width-ui.spacing().item_spacing.x).max(50.),32.],egui::DragValue::new(&mut shown).speed(step).max_decimals(4)).on_hover_text("Drag to adjust · click to type · Shift for fine adjustment · arrow keys to step").changed();
            if ui.add_sized([button_width,32.],egui::Button::new("+")).on_hover_text("Increase one step").clicked() {shown+=step;changed=true;}
        });
    });
    if changed && shown.is_finite() {
        *stored = c.stored(shown);
    } else {
        changed = false;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimensional_conversions_and_absolute_temperatures_are_reversible() {
        let sys = System::Imperial;
        for (label, source, value, expected) in [
            ("Length", "m", 0.0254, 1.),
            ("Altitude AGL", "m", 0.3048, 1.),
            ("Wind", "m/s", 0.44704, 1.),
            ("Speed", "m/s", 0.3048, 1.),
            ("Mass", "kg", 0.45359237, 1.),
            ("Pressure", "Pa", 6894.757293168, 1.),
            ("Temperature", "°C", 0., 32.),
            ("Temperature", "K", 273.15, 32.),
            ("Density", "kg/m³", 0.45359237 / 0.3048_f64.powi(3), 1.),
            ("Shear", "1/km", 1. / 1.609344, 1.),
        ] {
            let c = sys.conversion(label, source);
            assert!((c.display(value) - expected).abs() < 1e-9);
            assert!((c.stored(expected) - value).abs() < 1e-9);
        }
        for source in [
            "MPa",
            "GPa",
            "N",
            "N·s",
            "N·m",
            "m²",
            "m³",
            "kg·m²",
            "W/(m·K)",
            "J/(kg·K)",
            "J/K",
            "W",
            "Wh",
            "W/m²",
        ] {
            let c = sys.conversion("Property", source);
            assert!(!c.unit.is_empty());
            assert!((c.stored(c.display(123.456)) - 123.456).abs() < 1e-9);
        }
        assert!((sys.conversion("Temperature", "ΔK").display(1.) - 1.8).abs() < 1e-9);
    }
    #[test]
    fn switching_units_and_rendering_does_not_mutate_any_stored_value() {
        let ctx = egui::Context::default();
        let mut length = 0.15674323456789;
        let original = length;
        for i in 0..100 {
            ctx.data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("display_units"),
                    if i % 2 == 0 {
                        System::Metric
                    } else {
                        System::Imperial
                    },
                )
            });
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(!edit(ui, "Diameter / m", &mut length, 0.001));
                });
            });
        }
        assert_eq!(length.to_bits(), original.to_bits());
    }
    #[test]
    fn imperial_step_button_edits_stored_si_without_affecting_other_values() {
        let ctx = egui::Context::default();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("display_units"), System::Imperial));
        let mut diameter = 0.0254;
        let mut frame = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(420., 240.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        edit(ui, "Diameter / m", &mut diameter, 0.001);
                    });
                },
            )
        };
        let out = frame(vec![]);
        let position = out
            .shapes
            .iter()
            .find_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    if t.galley.job.text == "+" {
                        return Some(t.pos + t.galley.size() * 0.5);
                    }
                    None
                } else {
                    None
                }
            })
            .expect("step button");
        let edge = |pressed| egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(position), edge(true)]);
        frame(vec![edge(false)]);
        assert!((diameter - 1.05 * 0.0254).abs() < 1e-12);
    }
    #[test]
    fn themed_property_controls_keep_the_chosen_sidebar_width_across_frames() {
        for width in [190., 265., 400.] {
            let ctx = egui::Context::default();
            theme::Theme::Dark.apply(&ctx);
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("display_units"), System::Imperial));
            let mut diameter = 0.1524;
            let mut density = 1850.;
            let mut modulus = 50.;
            for _ in 0..100 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1440., 900.),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        let panel = egui::SidePanel::left("property_width")
                            .default_width(width)
                            .width_range(190. ..=600.)
                            .resizable(true)
                            .show(ctx, |ui| {
                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        edit(ui, "Diameter / m", &mut diameter, 0.001);
                                        edit(ui, "Density / kg/m³", &mut density, 1.);
                                        edit(ui, "Young’s modulus / GPa", &mut modulus, 0.1);
                                    });
                            });
                        assert!(
                            (panel.response.rect.width() - width).abs() < 0.5,
                            "{} grew to {}",
                            width,
                            panel.response.rect.width()
                        );
                    },
                );
            }
        }
    }
}
