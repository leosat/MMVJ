use crate::base_num::BaseNumT;
use crate::num_interval::NumInterval;
use std::time::Instant;

#[cfg(feature = "gui")]
use eframe::epaint;
#[cfg(feature = "gui")]
use epaint::Color32;
use std::{
    fs::File,
    io::{BufWriter, Write},
    sync::{Arc, Mutex},
};

#[cfg(feature = "gui")]
use tokio::sync::mpsc;

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub(crate) enum GraphDisplayStyleLineVariant {
    Dashed,
    Dotted,
    #[default]
    Solid,
}

// #[derive(Clone, Debug, Default)]
// pub(crate) enum GraphDisplayStyleLineInterpolation {
//     #[default]
//     Linear,
// }

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub(super) struct GraphDisplayStyleDetails {
    #[cfg(feature = "gui")]
    color: Color32,
    point_width: f32,
    // line_width: f32,
    // line_variant: GraphDisplayStyleLineVariant,
    // line_interpolation: GraphDisplayStyleLineInterpolation,
}

#[cfg(feature = "gui")]
#[derive(Clone, Debug)]

pub(crate) enum GraphDisplayStyle {
    Line(GraphDisplayStyleDetails),
    Filled(GraphDisplayStyleDetails),
}

#[cfg(feature = "gui")]
impl GraphDisplayStyle {
    pub(crate) fn as_line() -> Self {
        Self::Line(GraphDisplayStyleDetails::default())
    }

    pub(crate) fn as_filled() -> Self {
        Self::Filled(GraphDisplayStyleDetails::default())
    }

    pub(crate) fn is_filled(&self) -> bool {
        matches!(self, Self::Filled(_))
    }

    fn details_mut(&mut self) -> &mut GraphDisplayStyleDetails {
        match self {
            Self::Line(d) | Self::Filled(d) => d,
        }
    }

    fn details(&self) -> &GraphDisplayStyleDetails {
        match self {
            Self::Line(d) | Self::Filled(d) => d,
        }
    }

    #[cfg(feature = "gui")]
    pub(crate) fn with_color(mut self, c: Color32) -> Self {
        self.details_mut().color = c;
        self
    }

    #[cfg(feature = "gui")]
    pub(crate) fn get_color(&self) -> Color32 {
        self.details().color
    }

    pub(crate) fn get_point_width(&self) -> f32 {
        self.details().point_width
    }

    // #[allow(dead_code)]
    // pub(crate) fn get_line_width(&self) -> BaseNumericT {
    //     self.details().line_width
    // }

    pub(crate) fn with_width(self, w: f32) -> Self {
        self.with_point_width(w) //.with_line_width(w)
    }

    pub(crate) fn with_point_width(mut self, w: f32) -> Self {
        self.details_mut().point_width = w;
        self
    }

    // pub(crate) fn with_line_width(mut self, w: BaseNumericT) -> Self {
    //     self.details_mut().line_width = w;
    //     self
    // }
}

#[cfg(feature = "gui")]
impl Default for GraphDisplayStyle {
    fn default() -> Self {
        Self::as_line()
    }
}

#[derive(Clone, Debug)]
#[cfg(feature = "gui")]
pub(crate) struct TelemetryEvent {
    pub(crate) timestamp: Instant,
    pub(crate) value: BaseNumT,
    pub(crate) interval: NumInterval<BaseNumT>,
    #[cfg(feature = "gui")]
    pub(crate) style: GraphDisplayStyle,
}

#[cfg(feature = "gui")]
#[derive(Clone)]
pub(crate) struct TraceGraphHandle {
    #[allow(dead_code)]
    pub(crate) title: String,
    pub(crate) tx: mpsc::Sender<TelemetryEvent>,
    // pub(crate) registry: Arc<Mutex<crate::overlay::GraphRegistry>>,
}

// impl Drop for GraphHandle {
//     fn drop(&mut self) {
//         if let Ok(mut reg) = self.registry.lock() {
//             reg.remove(&self.title);
//         }
//     }
// }

pub(crate) enum TraceTarget {
    #[cfg(feature = "gui")]
    Graph(TraceGraphHandle),
    #[allow(unused)]
    File(String),
}

#[derive(Default)]
pub(crate) struct TraceChannel {
    #[cfg(feature = "gui")]
    graph_senders: Vec<tokio::sync::mpsc::Sender<TelemetryEvent>>,
    file_loggers: Vec<Arc<Mutex<BufWriter<File>>>>,
}

impl TraceChannel {
    pub(crate) fn trace(
        &self,
        value: BaseNumT,
        #[allow(unused)] interval: NumInterval<BaseNumT>,
        timestamp: Instant,
        #[cfg(feature = "gui")] style: GraphDisplayStyle,
    ) {
        #[cfg(feature = "gui")]
        for tx in &self.graph_senders {
            let event = TelemetryEvent {
                timestamp,
                value,
                interval,
                #[cfg(feature = "gui")]
                style: style.clone(),
            };
            let _ = tx.try_send(event);
        }

        if !self.file_loggers.is_empty() {
            let log_line = format!("{:.6}, {:?}\n", value, timestamp);

            for logger in &self.file_loggers {
                if let Ok(mut guard) = logger.lock() {
                    let _ = guard.write_all(log_line.as_bytes()); // No per-line flush.
                }
            }
        }
    }
}

pub(crate) fn make_trace_channel(targets: Vec<TraceTarget>) -> TraceChannel {
    #[cfg(feature = "gui")]
    let mut graph_senders = Vec::new();
    let mut file_loggers = Vec::new();

    for target in targets {
        match target {
            #[cfg(feature = "gui")]
            TraceTarget::Graph(handle) => {
                graph_senders.push(handle.tx.clone());
            }
            TraceTarget::File(path) => match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                Ok(file) => {
                    file_loggers.push(Arc::new(Mutex::new(BufWriter::new(file))));
                }
                Err(e) => log::error!("Failed to open trace file {}: {}", path, e),
            },
        }
    }

    TraceChannel {
        #[cfg(feature = "gui")]
        graph_senders,
        file_loggers,
    }
}
