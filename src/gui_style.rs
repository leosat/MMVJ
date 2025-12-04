use eframe::CreationContext;
use egui::Color32;

// ----------------------------------------------------
pub(crate) const GUI_STYLE_LIGHT: GuiStyle = GuiStyle {
    tfm_io_info_bg_color_enabled: egui::Color32::from_black_alpha(0),
    tfm_io_info_color_enabled: egui::Color32::from_rgb(0, 140, 140),
    tfm_title_bg_color_enabled: egui::Color32::DARK_GRAY,
    tfm_title_color_enabled: egui::Color32::LIGHT_GREEN,
    tfm_title_font_size: 16.0,
    tfm_title_bg_color_disabled: egui::Color32::LIGHT_GRAY,
    tfm_title_color_disabled: egui::Color32::DARK_GRAY,
    setup_creation_context: &|cc: &'_ CreationContext| {
        // cc.egui_ctx..all_styles_mut(|s| s.visuals = get_visuals_high_contrast_light1());
        // cc.egui_ctx.all_styles_mut(|s| s.visuals = get_visuals_w95_1());

        cc.egui_ctx.set_theme(egui::Theme::from_dark_mode(false));
        cc.egui_ctx.set_pixels_per_point(1.2);

        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);
    },
};

// ----------------------------------------------------
pub(crate) struct GuiStyle {
    pub(crate) setup_creation_context: &'static dyn Fn(&'_ CreationContext),
    // ----
    pub(crate) tfm_io_info_bg_color_enabled: egui::Color32,
    pub(crate) tfm_io_info_color_enabled: egui::Color32,
    // ----
    pub(crate) tfm_title_font_size: f32,
    // ----
    pub(crate) tfm_title_bg_color_enabled: egui::Color32,
    pub(crate) tfm_title_color_enabled: egui::Color32,
    // ----
    pub(crate) tfm_title_bg_color_disabled: egui::Color32,
    pub(crate) tfm_title_color_disabled: egui::Color32,
}

impl GuiStyle {
    pub(crate) fn tfm_io_info_decorate(&self, t: egui::RichText, is_enabled: bool) -> egui::RichText {
        t.background_color(if is_enabled {
            self.tfm_io_info_bg_color_enabled
        } else {
            egui::Color32::GRAY
        })
        .color(if is_enabled {
            self.tfm_io_info_color_enabled
        } else {
            egui::Color32::LIGHT_GRAY
        })
        .size(10.0)
        .monospace()
    }

    pub(crate) fn tfm_big_title_decorate(&self, t: egui::RichText) -> egui::RichText {
        t.background_color(self.tfm_title_bg_color_enabled)
            .size(self.tfm_title_font_size)
            .color(self.tfm_title_color_enabled)
            .strong()
    }

    pub(crate) fn tfm_title_decorate_enabled(&self, t: egui::RichText) -> egui::RichText {
        t.background_color(self.tfm_title_bg_color_enabled)
            .color(self.tfm_title_color_enabled)
            .size(self.tfm_title_font_size * 0.9)
            .strong()
    }

    pub(crate) fn tfm_title_decorate_disabled(&self, t: egui::RichText) -> egui::RichText {
        t.background_color(self.tfm_title_bg_color_disabled)
            .color(self.tfm_title_color_disabled)
            .size(self.tfm_title_font_size * 0.9)
            .strong()
    }
}

// --------------------------------------------------------------------------

#[allow(unused)]
pub fn get_visuals_w95_1() -> egui::Visuals {
    use egui::{Stroke, epaint::Shadow};

    let mut visuals = egui::Visuals::light();

    let w95_gray = Color32::from_rgb(192, 192, 192);
    let dark_gray = Color32::from_rgb(128, 128, 128);
    // let white = Color32::from_rgb(255, 255, 255);
    let black = Color32::from_rgb(0, 0, 0);

    visuals.dark_mode = false;
    visuals.window_fill = w95_gray;
    visuals.panel_fill = w95_gray;
    visuals.window_stroke = Stroke::new(1.0_f32, black);
    visuals.window_shadow = Shadow::NONE;
    visuals.window_corner_radius = 0.0.into();
    visuals.menu_corner_radius = 0.0.into();

    visuals.override_text_color = Some(black);

    visuals.widgets.noninteractive.bg_fill = w95_gray;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, dark_gray);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, black);

    visuals.widgets.inactive.bg_fill = w95_gray;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, dark_gray);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, black);

    visuals.widgets.hovered.bg_fill = w95_gray;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.1_f32, black);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.2_f32, black);

    visuals.widgets.active.bg_fill = w95_gray;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, black);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, black);

    visuals
}

#[allow(unused)]
fn get_visuals_high_contrast_light1() -> egui::Visuals {
    let mut visuals = egui::Visuals::light();

    visuals.override_text_color = Some(egui::Color32::BLACK);
    visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::BLACK;
    visuals.widgets.inactive.fg_stroke.color = egui::Color32::BLACK;
    visuals.widgets.hovered.fg_stroke.color = egui::Color32::BLACK;
    visuals.widgets.active.fg_stroke.color = egui::Color32::BLACK;
    visuals.widgets.open.fg_stroke.color = egui::Color32::BLACK;

    visuals.panel_fill = egui::Color32::WHITE;
    visuals.widgets.noninteractive.bg_fill = egui::Color32::WHITE;
    visuals.widgets.inactive.bg_fill = egui::Color32::WHITE;
    visuals.widgets.hovered.bg_fill = egui::Color32::WHITE;
    visuals.widgets.active.bg_fill = egui::Color32::WHITE;
    visuals.widgets.open.bg_fill = egui::Color32::WHITE;

    visuals
}
