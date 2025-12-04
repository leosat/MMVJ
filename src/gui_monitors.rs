use eframe::egui::{self};

use crate::base_num::BaseNumT;
use crate::gui_common::DrawEgui;
use crate::num_interval::OutOfRangePolicy;
use crate::schemas_ui::UiAxisMonitorCfg;
use crate::schemas_value::{WithLastKnownIO, WithNumInterval};

//====================================================================================

impl<'s> DrawEgui<'s> for UiAxisMonitorCfg {
    type In = ();
    type Out = ();

    fn egui(&mut self, _gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        // if self.cancellation_token.is_cancelled() {
        //     ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        //     return;
        // }

        let panel_frame = egui::Frame::new().fill(egui::Color32::from_black_alpha(150));
        egui::CentralPanel::default().frame(panel_frame).show_inside(ui, |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter();
            let center_x = rect.center().x;

            let border_stroke = egui::Stroke::new(5.0_f32, egui::Color32::WHITE);
            painter.line_segment([rect.left_top(), rect.left_bottom()], border_stroke);
            painter.line_segment([rect.right_top(), rect.right_bottom()], border_stroke);

            painter.line_segment(
                [egui::pos2(center_x, rect.min.y), egui::pos2(center_x, rect.max.y)],
                egui::Stroke::new(1.0_f32, egui::Color32::WHITE),
            );

            let quarter_offset = rect.width() / 4.0;
            let marker_height = rect.height() * 0.5;
            let marker_y_top = rect.center().y - (marker_height / 2.0);
            let marker_y_bottom = rect.center().y + (marker_height / 2.0);
            let marker_stroke = egui::Stroke::new(2.0_f32, egui::Color32::GOLD);

            for x in [center_x - quarter_offset, center_x + quarter_offset] {
                painter.line_segment(
                    [egui::pos2(x, marker_y_top), egui::pos2(x, marker_y_bottom)],
                    marker_stroke,
                );
            }

            let (position, hold) = {
                (
                    self.position
                        .as_ref()
                        .map(|v| {
                            painter.text(
                                rect.right_top(),
                                egui::Align2::RIGHT_TOP,
                                v,
                                egui::FontId::proportional(14.0),
                                egui::Color32::GRAY,
                            );

                            v.get_interval()
                                .map_to_symm_unit::<BaseNumT>(v.get_last_known_io(), OutOfRangePolicy::Clamp)
                        })
                        .unwrap_or_default(),
                    self.hold
                        .as_ref()
                        .map(|v| {
                            painter.text(
                                rect.left_top(),
                                egui::Align2::LEFT_TOP,
                                v,
                                egui::FontId::proportional(14.0),
                                egui::Color32::GRAY,
                            );

                            v.get_interval()
                                .map_to_unit::<BaseNumT>(v.get_last_known_io(), OutOfRangePolicy::WarnIfDebugAndClamp)
                        })
                        .unwrap_or_default(),
                )
            };

            let color_intensity = (255.0 * ((1.0 - hold).clamp(0., 1.))) as u8;
            let cursor_color = egui::Color32::from_rgb(255 - color_intensity, 100, 100);

            #[allow(clippy::unnecessary_cast)]
            let x_pos = rect.min.x + (position as f32 + 1.0) / 2.0 * rect.width();
            let cursor_width = (rect.width() * 0.02).max(8.0);
            let cursor_rect = egui::Rect::from_center_size(
                egui::pos2(x_pos, rect.center().y),
                egui::vec2(cursor_width, rect.height() * 0.8),
            );

            painter.rect_stroke(
                cursor_rect,
                0.0,
                egui::Stroke::new(6.0_f32, egui::Color32::WHITE),
                egui::StrokeKind::Middle,
            );

            painter.rect_filled(cursor_rect, 0.0, cursor_color);
        });

        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }
}
