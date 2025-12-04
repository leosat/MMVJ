// #![cfg_attr(all(not(feature = "default"), not(debug_assertions)), allow(warnings))]
#![cfg_attr(all(not(debug_assertions)), allow(warnings))]
// #![macro_use]

pub mod config;
pub mod debug;
pub mod driver;
pub use debug::DebugLevel;

// --------------------------
pub mod cli;

// --------------------------
#[cfg(test)]
mod functional_tests;
#[cfg(test)]
pub(crate) mod test_utils;

//--------------------------------
pub(crate) mod base_num;
pub(crate) mod interner;

#[macro_use]
pub(crate) mod num_interval;
pub(crate) mod relativity;
//--------------------------------
pub(crate) mod hid_device;
pub(crate) mod hid_owned_and_ffb;
//--------------------------------
pub(crate) mod hid_manager;
#[cfg(feature = "midi")]
pub(crate) mod midi;
//--------------------------------
pub(crate) mod device_and_device_manager;
#[macro_use]
pub(crate) mod mapped_controls_macro;
pub(crate) mod mapped_controls;
pub(crate) mod mapping;
pub mod tfm_exec;
//--------------------------------
pub(crate) mod tracing;
//--------------------------------
pub(crate) mod schemas_cfg;
pub(crate) mod schemas_common;
pub(crate) mod schemas_control_matcher;
pub(crate) mod schemas_hid;
pub(crate) mod schemas_mapping;
#[cfg(feature = "midi")]
pub(crate) mod schemas_midi;
pub(crate) mod schemas_predefined;
pub(crate) mod schemas_transform;
pub(crate) mod schemas_ui;
pub(crate) mod schemas_value_port;

#[macro_use]
pub(crate) mod schemas_value;
//--------------------------------
pub(crate) mod curves_and_linear;
pub(crate) mod filters;
//--------------------------------
#[cfg(feature = "gui")]
pub(crate) mod gui_common;
#[cfg(feature = "gui")]
pub(crate) mod gui_config;
#[cfg(feature = "gui")]
pub(crate) mod gui_device;
#[cfg(feature = "gui")]
pub(crate) mod gui_hid;
#[cfg(feature = "gui")]
pub(crate) mod gui_main;
#[cfg(feature = "gui")]
pub(crate) mod gui_mapping;
#[cfg(feature = "gui")]
#[cfg(feature = "midi")]
pub(crate) mod gui_midi;
#[cfg(feature = "gui")]
pub(crate) mod gui_monitors;
#[cfg(feature = "gui")]
pub(crate) mod gui_style;
#[cfg(feature = "gui")]
pub(crate) mod gui_telemetry_graph;
#[cfg(feature = "gui")]
pub(crate) mod gui_transform_step;
#[cfg(feature = "gui")]
pub(crate) mod gui_value;
