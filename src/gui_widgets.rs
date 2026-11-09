use crate::{
    base_num::{BaseNumConsts, BaseNumT},
    num_interval::NumInterval,
    schemas_value::{Clipping, ClippingIface},
};
use eframe::egui;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct ClippingIndicator {
    interval: NumInterval<BaseNumT>,
    last_value: BaseNumT,
    last_change: Instant,
    blink_start: Instant,
    clip: Option<Clipping>,
}

impl ClippingIndicator {
    pub fn new(interval: NumInterval<BaseNumT>) -> Self {
        let now = Instant::now();
        Self {
            interval,
            last_value: 0.0,
            last_change: now - Duration::from_secs(10),
            blink_start: now,
            clip: None,
        }
    }

    pub fn update(&mut self, value: BaseNumT, clipped: Option<Clipping>) {
        let now = Instant::now();

        if (value - self.last_value).abs() > f32::EPSILON {
            self.last_change = now;
            self.blink_start = now;
            self.last_value = value;
        }

        if value < self.interval.from() {
            self.clip = Clipping::Underflow.into();
        } else if value > self.interval.to() {
            self.clip = Clipping::Overflow.into();
        } else {
            self.clip = clipped
        }
    }

    pub fn reset(&mut self) {
        self.clip = None;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let size = egui::vec2(16.0, 16.0);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

        if response.clicked() {
            self.reset();
        }

        let value = self.last_value;
        let now = Instant::now();
        let time_since_change = now.duration_since(self.last_change).as_secs_f32();
        let is_changing = time_since_change < 0.25;
        let is_under = matches!(self.clip, Some(Clipping::Underflow));
        let is_over = matches!(self.clip, Some(Clipping::Overflow));
        let is_clipping = is_under || is_over;

        let color = if !self.clip.is_not_clipping() {
            egui::Color32::from_rgb(230, 50, 50)
        } else if is_changing || is_clipping {
            let t = if is_under || is_over {
                1.0
            } else {
                let center = self.interval.midpoint();
                let half_range = self.interval.span() / 2.0;
                ((value - center).abs() / half_range).clamp(0.0, 1.0)
            };
            egui::Color32::from_rgb((255.0 * t) as u8, 220, 0)
        } else {
            egui::Color32::from_gray(200)
        };

        let alpha = if is_changing || is_clipping {
            let blink = ((now.duration_since(self.blink_start).as_secs_f32() * 10.0).sin() + 1.0) / 2.0;
            0.35 + 0.65 * blink
        } else {
            1.0
        };

        let fill = color.gamma_multiply(alpha);
        let painter = ui.painter_at(rect);
        let center = rect.center();
        let r = rect.width().min(rect.height()) / 2.0 - 2.0;

        match self.clip {
            Some(Clipping::Underflow) => draw_half_circle(&painter, center, r, fill, true),
            Some(Clipping::Overflow) => draw_half_circle(&painter, center, r, fill, false),
            None => {
                painter.circle_filled(center, r, fill);
            }
        };

        painter.circle_stroke(center, r, egui::Stroke::new(1.5, egui::Color32::from_gray(220)));
        response
    }
}

fn draw_half_circle(painter: &egui::Painter, center: egui::Pos2, radius: f32, fill: egui::Color32, left: bool) {
    let (start, end) = if left {
        (BaseNumConsts::FRAC_PI_2, 3.0 * BaseNumConsts::FRAC_PI_2)
    } else {
        (-BaseNumConsts::FRAC_PI_2, BaseNumConsts::FRAC_PI_2)
    };

    let segments = 8;
    let mut points = Vec::with_capacity(segments + 2);
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let angle = start + t * (end - start);
        points.push(egui::pos2(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ));
    }
    points.push(center);

    painter.add(egui::Shape::convex_polygon(points, fill, egui::Stroke::NONE));
}
