use std::f32::consts::PI;

use crate::num_interval::ZERO_INTERVAL;

use crate::base_num::BaseNumT;
use crate::num_interval::{NumInterval, OutOfRangePolicy};
use crate::tracing::{TelemetryEvent, TraceGraphHandle};
use circular_buffer::CircularBuffer;
use eframe::egui::{self, Color32, Pos2, Shape, Stroke};
use tokio::sync::mpsc;

// =====================================================
pub(crate) const MAX_DATA_POINTS_PER_TICK_PER_GRAPH: usize = 6;
pub(crate) const MIN_TIME_WINDOW: f32 = 0.1;
pub(crate) const MAX_TIME_WINDOW: f32 = 10.0;
pub(crate) const MAX_EST_FREQUENCY: u32 = 100;
const DEFAULT_GRAPH_RESOLUTION_FACTOR: f32 = 0.5;
const DEFAULT_GRAPH_TIME_WINDOW_SEC: f32 = 3.0;
const SAMPLES_PER_GRAP_RESOLUTION_BUCKET: usize = 7; // TODO: make per-graph configurable...
// TODO: or make graph drawing caching and get rid of it.
const GRAPH_STATIC_BUF_SIZE: usize = (crate::gui_telemetry_graph::MAX_DATA_POINTS_PER_TICK_PER_GRAPH as f32
    * crate::gui_telemetry_graph::MAX_TIME_WINDOW
    * (crate::gui_telemetry_graph::MAX_EST_FREQUENCY as f32)
    * (SAMPLES_PER_GRAP_RESOLUTION_BUCKET as f32)) as usize;
// const GRAPH_STATIC_BUF_SIZE: usize = 30000;

// =====================================================
pub(crate) struct GuiGraphState {
    pub(crate) legend: String,
    pub(crate) receiver: mpsc::Receiver<TelemetryEvent>,
    pub(crate) data: Box<CircularBuffer<GRAPH_STATIC_BUF_SIZE, TelemetryEvent>>,
    pub(crate) y_interval: Option<NumInterval<BaseNumT>>,
    pub(crate) _in_interval: NumInterval<BaseNumT>,
    pub(crate) _out_interval: NumInterval<BaseNumT>,
    pub(crate) time_window_sec: f32,
    pub(crate) resolution_factor: f32,
    pub(crate) dot_width_factor: f32,
    pub(crate) buckets_per_graph: usize,
    pub(crate) dt_per_bucket: f32,
    pub(crate) per_bucket_counter: Vec<usize>,
}

pub(crate) type GuiTelemetryGraphStates = std::collections::HashMap<String, GuiGraphState>;
// pub(crate) type GuiTelemetryGraphStatesMt = Arc<Mutex<GuiTelemetryGraphStates>>;
// pub(crate) type GuiThreadLocalGraphStatesRegistry = Rc<TelemetryGraphStates>;

pub(crate) fn make_trace_graph_2d(
    title: &str,
    legend: &str,
    interval: Option<NumInterval<BaseNumT>>,
) -> (TraceGraphHandle, GuiGraphState) {
    let (tx, rx) = mpsc::channel(1000);
    (
        TraceGraphHandle {
            title: title.to_string(),
            tx,
        },
        GuiGraphState {
            legend: legend.to_string(),
            receiver: rx,
            data: CircularBuffer::boxed(),
            y_interval: interval,
            buckets_per_graph: 0,
            dt_per_bucket: 0.0,
            per_bucket_counter: Vec::new(),
            // ------------------------------------
            time_window_sec: DEFAULT_GRAPH_TIME_WINDOW_SEC,
            resolution_factor: DEFAULT_GRAPH_RESOLUTION_FACTOR,
            dot_width_factor: 1.0,
            _in_interval: ZERO_INTERVAL,
            _out_interval: ZERO_INTERVAL,
        },
    )
}

impl GuiGraphState {
    fn set_resolution_state(&mut self, max_buckets: f32) {
        self.buckets_per_graph = (self.resolution_factor * max_buckets) as usize;
        self.per_bucket_counter.fill(0);
        self.per_bucket_counter.resize(self.buckets_per_graph, 0);

        self.dt_per_bucket = self.time_window_sec / self.buckets_per_graph as f32;
    }

    pub(crate) fn consume_input_queue_and_draw_gui(&mut self, ui: &mut egui::Ui) {
        self.consume_input_queue();
        self.draw_gui(ui);
    }

    pub(crate) fn consume_input_queue(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            self.data.push_back(event);
        }
    }

    pub(crate) fn draw_gui(&mut self, ui: &mut egui::Ui) {
        let (rect, _response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 150.0), egui::Sense::hover());

        self.set_resolution_state(rect.width().abs().trunc());

        // TODO: cache the painted graph to a texture and shift it,
        // TODO: instead of drawing every frame.
        let painter = ui.painter_at(rect);
        ui.separator();

        painter.rect_filled(rect, 0.0, Color32::WHITE);
        // painter.rect_filled(rect, 0.0, Color32::BLACK);

        // for interval in [self._in_interval, self._out_interval] {
        //     let font = egui::FontId::monospace(9.0);
        //     painter.text(
        //         rect.left_top(),
        //         egui::Align2::LEFT_TOP,
        //         format!("{:.1}", interval.to()),
        //         font.clone(),
        //         Color32::YELLOW.linear_multiply(0.6),
        //     );
        //     painter.text(
        //         rect.left_bottom(),
        //         egui::Align2::LEFT_BOTTOM,
        //         format!("{:.1}", interval.from()),
        //         font,
        //         Color32::YELLOW.linear_multiply(0.4),
        //     );
        // }

        if let Some(interval) = self.y_interval
            && interval.contains_value_closed(0.0 as BaseNumT)
        {
            let zero_y = rect.max.y
                - (interval.map_to_unit::<f32>(0.0 as BaseNumT, OutOfRangePolicy::WarnIfDebugAndClamp) * rect.height());
            painter.line_segment(
                [egui::pos2(rect.min.x, zero_y), egui::pos2(rect.max.x, zero_y)],
                Stroke::new(1.0_f32, Color32::from_white_alpha(180)),
            );
        }

        painter.text(
            rect.right_top(),
            egui::Align2::RIGHT_TOP,
            &self.legend,
            egui::FontId::proportional(16.0),
            Color32::GRAY,
        );

        if self.data.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_TOP.flip(),
                "Waiting for data...",
                egui::FontId::proportional(20.0),
                Color32::MAGENTA,
            );
            return;
        }

        let mut prev_event = self.data.front();

        let now = std::time::Instant::now();

        ui.horizontal(|ui| {
            ui.label("Time wind.:");
            ui.add(
                egui::DragValue::new(&mut self.time_window_sec)
                    .range(MIN_TIME_WINDOW..=MAX_TIME_WINDOW)
                    .suffix(" s")
                    .speed(0.1),
            );
            ui.separator();
            ui.label("Resolution:");
            ui.add(
                egui::DragValue::new(&mut self.resolution_factor)
                    .range(0.1..=1.0)
                    .speed(0.1),
            );
            ui.separator();
            ui.label("Dot width");
            ui.add(
                egui::DragValue::new(&mut self.dot_width_factor)
                    .range(0.5..=PI)
                    .speed(0.1),
            );
        });

        for event in &*self.data {
            let age = now.duration_since(event.timestamp).as_secs_f32();
            if age > self.time_window_sec {
                continue;
            }

            let bucket_index = ((age / self.dt_per_bucket) as usize).min(self.buckets_per_graph - 1);

            if self.per_bucket_counter[bucket_index] < SAMPLES_PER_GRAP_RESOLUTION_BUCKET {
                self.per_bucket_counter[bucket_index] += 1;
            } else {
                continue;
            }

            let x = rect.max.x - (age / self.time_window_sec) * rect.width();
            let norm = event
                .interval
                .map_to_unit::<f32>(event.value, OutOfRangePolicy::WarnIfDebugAndClamp)
                .clamp(0.0, 1.0);
            let y = rect.max.y - (norm * rect.height()); // screen coords, 0 is top left, Y points down.
            let current_pos = Pos2::new(x, y);

            let (base_color, is_filled, width) = (
                event.style.get_color(),
                event.style.is_filled(),
                event.style.get_point_width() * self.dot_width_factor,
            );

            if is_filled && let Some(prev_event) = prev_event {
                let baseline_y = rect.max.y - (0.5 * rect.height());
                let stroke_w = rect.width().abs()
                    * ((event.timestamp.duration_since(prev_event.timestamp)).as_secs_f32() / self.time_window_sec)
                        .min(0.001);
                painter.add(Shape::line_segment(
                    [
                        Pos2 {
                            x: current_pos.x,
                            y: baseline_y,
                        },
                        Pos2 {
                            x: current_pos.x,
                            y: current_pos.y,
                        },
                    ],
                    Stroke::new(stroke_w, base_color.gamma_multiply(0.25)),
                ));
            }

            painter.circle_filled(current_pos, width, base_color);

            prev_event = Some(event);
        }
    }
}
