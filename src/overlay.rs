use atomic_float::AtomicF32;
use eframe::egui;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use winit::platform::x11::EventLoopBuilderExtX11;
// // TODO: support Wayland also.
// use winit::platform::wayland::EventLoopBuilderExtWayland;

pub fn run_steering_indicator_window_overlay(
    steering_val: Arc<AtomicF32>,
    steering_hold_val: Arc<AtomicF32>,
    cancellation_token: CancellationToken,
) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_transparent(true)
            //.with_always_on_top()
            .with_movable_by_background(true)
            .with_visible(true)
            .with_decorations(true)
            .with_active(true)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_inner_size([1920.0, 100.0]),
        event_loop_builder: Some(Box::new(|builder| {
            {
                EventLoopBuilderExtX11::with_any_thread(builder, true);
                // EventLoopBuilderExtWayland::with_any_thread(builder, true);
            }
        })),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "<.> MMVJ Steering Indicator <.>",
        options,
        Box::new(|_cc| {
            Ok(Box::new(SteeringIndicatorWindow {
                cancellation_token,
                steering_pos_val: steering_val,
                steering_hold_val: steering_hold_val,
            }))
        }),
    );
}

struct SteeringIndicatorWindow {
    cancellation_token: CancellationToken,
    steering_pos_val: Arc<AtomicF32>,
    steering_hold_val: Arc<AtomicF32>,
}

impl eframe::App for SteeringIndicatorWindow {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.cancellation_token.is_cancelled() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // let screen_width = ctx.content_rect().width();
        // let target_width = screen_width * 0.7;
        // let target_height = 100.0;
        // ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(target_width, target_height)));

        // let screen_rect = ctx.screen_rect();
        // let x_pos = (screen_rect.width() - target_width) / 2.0;
        // let y_pos = (screen_rect.height() - target_height) / 2.0;

        // ctx.send_viewport_cmd(egui::ViewportCommand::Position(egui::pos2(x_pos, y_pos)));

        // ctx.send_viewport_cmd(egui::ViewportCommand::Focus);

        ctx.request_repaint();

        let panel_frame = egui::Frame::new().fill(egui::Color32::from_black_alpha(150));
        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                let center_x = rect.center().x;

                let border_stroke = egui::Stroke::new(5.0, egui::Color32::WHITE);
                painter.line_segment([rect.left_top(), rect.left_bottom()], border_stroke);
                painter.line_segment([rect.right_top(), rect.right_bottom()], border_stroke);

                painter.line_segment(
                    [
                        egui::pos2(center_x, rect.min.y),
                        egui::pos2(center_x, rect.max.y),
                    ],
                    egui::Stroke::new(10.0, egui::Color32::WHITE),
                );

                let quarter_offset = rect.width() / 4.0;
                let marker_height = rect.height() * 0.5;
                let marker_y_top = rect.center().y - (marker_height / 2.0);
                let marker_y_bottom = rect.center().y + (marker_height / 2.0);
                let marker_stroke = egui::Stroke::new(2.0, egui::Color32::GOLD);

                for x in [center_x - quarter_offset, center_x + quarter_offset] {
                    painter.line_segment(
                        [egui::pos2(x, marker_y_top), egui::pos2(x, marker_y_bottom)],
                        marker_stroke,
                    );
                }

                let hold_val = self
                    .steering_hold_val
                    .load(Ordering::Relaxed)
                    .clamp(0.0, 1.0);
                let steer_val = self.steering_pos_val.load(Ordering::Relaxed);

                let color_intensity = (255.0 * ((1.0 - hold_val).clamp(0., 1.))) as u8;
                let cursor_color = egui::Color32::from_rgb(255 - color_intensity, 100, 100);

                let x_pos = rect.min.x + (steer_val + 1.0) / 2.0 * rect.width();
                let cursor_width = (rect.width() * 0.02).max(8.0);
                let cursor_rect = egui::Rect::from_center_size(
                    egui::pos2(x_pos, rect.center().y),
                    egui::vec2(cursor_width, rect.height() * 0.8),
                );

                painter.rect_stroke(
                    cursor_rect,
                    0.0,
                    egui::Stroke::new(6.0, egui::Color32::WHITE),
                    egui::StrokeKind::Middle,
                );

                painter.rect_filled(cursor_rect, 0.0, cursor_color);
            });
    }
}
