use egui::Color32;
use serde::{Deserialize, Serialize};

// ----------------------------------------------------
pub(crate) const GUI_STYLE_DARK: GuiStyle = GuiStyle {
    kind: GuiStyleKind::Dark,
    //---
    value_identity_text_color: egui::Color32::CYAN,
    value_identity_bg_color: egui::Color32::DARK_GRAY,
    //---
    tfm_io_info_bg_color_enabled: egui::Color32::from_black_alpha(0),
    tfm_io_info_color_enabled: egui::Color32::from_rgb(0, 240, 240),
    tfm_title_bg_color_enabled: egui::Color32::LIGHT_GRAY,
    tfm_title_color_enabled: egui::Color32::BLACK,
    tfm_title_font_size: 16.0,
    tfm_title_bg_color_disabled: egui::Color32::LIGHT_GRAY,
    tfm_title_color_disabled: egui::Color32::DARK_GRAY,
};

// ----------------------------------------------------
pub(crate) const GUI_STYLE_LIGHT: GuiStyle = GuiStyle {
    kind: GuiStyleKind::Light,
    //---
    value_identity_text_color: egui::Color32::BLUE,
    value_identity_bg_color: egui::Color32::from_rgba_unmultiplied_const(0, 200, 0, 30),
    //---
    tfm_io_info_bg_color_enabled: egui::Color32::from_black_alpha(0),
    tfm_io_info_color_enabled: egui::Color32::from_rgb(0, 140, 140),
    tfm_title_bg_color_enabled: egui::Color32::DARK_GRAY,
    tfm_title_color_enabled: egui::Color32::LIGHT_GREEN,
    tfm_title_font_size: 16.0,
    tfm_title_bg_color_disabled: egui::Color32::LIGHT_GRAY,
    tfm_title_color_disabled: egui::Color32::DARK_GRAY,
};

// ----------------------------------------------------
pub(crate) const GUI_STYLE_GRAY_OUTLINED: GuiStyle = GuiStyle {
    kind: GuiStyleKind::GrayOutlined,
    //---
    value_identity_text_color: egui::Color32::BLUE,
    value_identity_bg_color: egui::Color32::from_rgba_unmultiplied_const(0, 200, 0, 30),
    //---
    tfm_io_info_bg_color_enabled: egui::Color32::from_black_alpha(0),
    tfm_io_info_color_enabled: egui::Color32::DARK_GREEN,
    tfm_title_bg_color_enabled: egui::Color32::DARK_GRAY,
    tfm_title_color_enabled: egui::Color32::LIGHT_GREEN,
    tfm_title_font_size: 16.0,
    tfm_title_bg_color_disabled: egui::Color32::LIGHT_GRAY,
    tfm_title_color_disabled: egui::Color32::DARK_GRAY,
};

fn setup_egui_ctx_light(ctx: &egui::Context) {
    // cc.egui_ctx..all_styles_mut(|s| s.visuals = get_visuals_high_contrast_light1());
    // cc.egui_ctx.all_styles_mut(|s| s.visuals = get_visuals_w95_1());
    ctx.set_theme(egui::Theme::from_dark_mode(false));
    setup_egui_cc_common(ctx);
    ctx.set_visuals(egui::Visuals::light());
}

fn setup_egui_ctx_gray_outlined(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::from_dark_mode(false));
    setup_egui_cc_common(ctx);
    ctx.set_visuals(get_visuals_gray_outlined());
}

fn setup_egui_ctx_dark(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::from_dark_mode(true));
    setup_egui_cc_common(ctx);
    ctx.set_visuals(egui::Visuals::dark());
}

fn setup_egui_cc_common(ctx: &egui::Context) {
    ctx.set_pixels_per_point(1.2);
    {
        let drag_value_style_name = "drag_value";
        ctx.all_styles_mut(|s| {
            s.text_styles.insert(
                egui::TextStyle::Name(drag_value_style_name.into()),
                egui::FontId::new(10.0, egui::FontFamily::Monospace),
            );
            s.drag_value_text_style = egui::TextStyle::Name(drag_value_style_name.into());
        });
    }
    ctx.set_fonts(font_definitions_with_phosphor_fonts());
}

fn font_definitions_with_phosphor_fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let name: &str = "phosphor";
    {
        let fonts = &mut fonts;
        let variant = egui_phosphor::Variant::Regular;
        {
            let bytes = variant.font_bytes();
            {
                fonts
                    .font_data
                    .insert(name.into(), egui::FontData::from_static(bytes).into());

                let mut keys = fonts
                    .families
                    .get(&egui::FontFamily::Proportional)
                    .cloned()
                    .unwrap_or_default();
                keys.insert(keys.len().min(1), name.into());
                fonts.families.insert(egui::FontFamily::Name(name.into()), keys);
            };
            if let Some(font_keys) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                // Second, so it does not shadow the text font -- but `Proportional` can
                // be empty when egui is built without `default_fonts`.
                let at = font_keys.len().min(1);
                font_keys.insert(at, name.into());
            }
        };

        if let Some(font_list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            font_list.push(name.to_owned());
        }
    }

    fonts
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub(crate) enum GuiStyleKind {
    #[default]
    Light,
    GrayOutlined,
    Dark,
}

// ----------------------------------------------------
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct GuiStyle {
    pub(crate) kind: GuiStyleKind,
    // ----
    pub(crate) value_identity_text_color: egui::Color32,
    pub(crate) value_identity_bg_color: egui::Color32,
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
    pub(crate) fn setup_egui_ctx(&self, ctx: &egui::Context) {
        match self.kind {
            GuiStyleKind::Light => setup_egui_ctx_light(ctx),
            GuiStyleKind::Dark => setup_egui_ctx_dark(ctx),
            GuiStyleKind::GrayOutlined => setup_egui_ctx_gray_outlined(ctx),
        }
    }
    pub(crate) fn value_identity_decorate(&self, vi: egui::RichText) -> egui::RichText {
        vi.color(self.value_identity_text_color)
            .background_color(self.value_identity_bg_color)
            .strong()
    }

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
pub fn get_visuals_gray_outlined() -> egui::Visuals {
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
    // visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, dark_gray);
    // visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, black);

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
