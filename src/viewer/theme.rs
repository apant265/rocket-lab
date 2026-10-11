//! Shared neutral palettes for egui, custom charts and the inspection viewport.
use super::*;
#[derive(Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) enum Theme {
    #[default]
    Dark,
    Light,
}
pub(super) struct Palette {
    pub panel: egui::Color32,
    pub surface: egui::Color32,
    pub raised: egui::Color32,
    pub hover: egui::Color32,
    pub border: egui::Color32,
    pub text: egui::Color32,
    pub muted: egui::Color32,
    pub accent: egui::Color32,
    pub on_accent: egui::Color32,
    pub warning: egui::Color32,
    pub danger: egui::Color32,
    pub viewport: [f32; 3],
}
impl Theme {
    pub fn load() -> Self {
        std::fs::read_to_string("output/ui-theme.json")
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(self) -> crate::config::Result<()> {
        std::fs::create_dir_all("output")?;
        std::fs::write("output/ui-theme.json", serde_json::to_string(&self)?)?;
        Ok(())
    }
    pub fn palette(self) -> Palette {
        let c = egui::Color32::from_rgb;
        match self {
            Self::Dark => Palette {
                panel: c(25, 27, 26),
                surface: c(20, 22, 21),
                raised: c(35, 38, 36),
                hover: c(46, 51, 48),
                border: c(52, 58, 54),
                text: c(245, 248, 245),
                muted: c(180, 190, 183),
                accent: c(151, 199, 169),
                on_accent: c(19, 36, 25),
                warning: c(232, 181, 116),
                danger: c(244, 137, 124),
                viewport: [0.052, 0.061, 0.055],
            },
            Self::Light => Palette {
                panel: c(246, 248, 246),
                surface: c(255, 255, 255),
                raised: c(235, 240, 236),
                hover: c(220, 230, 223),
                border: c(209, 220, 212),
                text: c(19, 29, 22),
                muted: c(66, 82, 72),
                accent: c(49, 106, 70),
                on_accent: c(251, 255, 252),
                warning: c(137, 79, 28),
                danger: c(167, 44, 29),
                viewport: [0.90, 0.93, 0.91],
            },
        }
    }
    pub fn apply(self, ctx: &egui::Context) {
        let p = self.palette();
        let mut style = (*ctx.style()).clone();
        let v = &mut style.visuals;
        *v = if self == Self::Dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.panel_fill = p.panel;
        v.window_fill = p.surface;
        v.faint_bg_color = p.raised;
        v.extreme_bg_color = p.surface;
        v.override_text_color = Some(p.text);
        v.hyperlink_color = p.accent;
        v.selection.bg_fill = p.accent.gamma_multiply(0.22);
        v.selection.stroke = egui::Stroke::new(1.0, p.accent);
        v.window_corner_radius = egui::CornerRadius::same(12);
        v.window_stroke = egui::Stroke::new(1.0, p.border);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.bg_fill = p.raised;
            w.weak_bg_fill = p.raised;
            w.bg_stroke = egui::Stroke::new(1.0, p.border);
            w.fg_stroke = egui::Stroke::new(1.0, p.text);
            w.corner_radius = egui::CornerRadius::same(15);
        }
        v.widgets.noninteractive.bg_fill = p.surface;
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, p.border);
        v.widgets.noninteractive.fg_stroke.color = p.muted;
        v.widgets.hovered.bg_fill = p.hover;
        v.widgets.hovered.weak_bg_fill = p.hover;
        v.widgets.hovered.bg_stroke.color = p.accent.gamma_multiply(0.55);
        v.widgets.active.bg_fill = p.hover;
        v.widgets.active.weak_bg_fill = p.hover;
        v.widgets.active.bg_stroke.color = p.accent;
        v.widgets.active.fg_stroke.color = p.accent;
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(11.0, 6.0);
        style.spacing.interact_size.y = 32.0;
        style.spacing.window_margin = egui::Margin::same(16);
        // Allocate a gutter instead of painting the handle over labels and values.
        style.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 8.0,
            bar_inner_margin: 6.0,
            bar_outer_margin: 2.0,
            foreground_color: true,
            ..egui::style::ScrollStyle::solid()
        };
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(18.0));
        ctx.set_style(style);
    }
}
pub(super) fn palette(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode {
        Theme::Dark
    } else {
        Theme::Light
    }
    .palette()
}
