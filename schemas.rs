use crate::common::Interval;
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GlobalSettings {
    #[serde(default = "default_update_rate")]
    pub(crate) update_rate: u32,
    #[serde(default = "default_latency_mode")]
    pub(crate) latency_mode: String,
    #[serde(default = "default_true")]
    pub(crate) auto_reconnect: bool,
    #[serde(default = "default_scan_interval")]
    pub(crate) device_scan_interval: f32,
    #[serde(default = "default_true")]
    pub(crate) joystick_persistent: bool,
    #[serde(default = "default_log_level")]
    pub(crate) log_level: String,
    #[serde(default)]
    pub(crate) log_midi_events: bool,
    #[serde(default)]
    pub(crate) log_ff_events: bool,
    #[serde(default)]
    pub(crate) log_joystick_events: bool,
    #[serde(default = "default_max_joysticks")]
    pub(crate) max_virtual_joysticks: u32,
    #[serde(default = "default_max_mappings")]
    pub(crate) max_mappings_per_device: u32,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            update_rate: default_update_rate(),
            latency_mode: default_latency_mode(),
            auto_reconnect: default_true(),
            device_scan_interval: default_scan_interval(),
            joystick_persistent: default_true(),
            log_level: default_log_level(),
            log_midi_events: false,
            log_ff_events: false,
            log_joystick_events: false,
            max_virtual_joysticks: default_max_joysticks(),
            max_mappings_per_device: default_max_mappings(),
        }
    }
}

fn default_update_rate() -> u32 {
    1000
}
fn default_latency_mode() -> String {
    "normal".to_string()
}
fn default_true() -> bool {
    true
}
fn default_scan_interval() -> f32 {
    5.0
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_max_joysticks() -> u32 {
    10
}
fn default_max_mappings() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Profiles {
    #[serde(default)]
    pub(crate) current_profile: String,
    #[serde(default)]
    pub(crate) available_profiles: HashMap<String, ProfileInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProfileInfo {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Presets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) midi_controls: Option<HashMap<String, MidiControlDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mouse_controls: Option<HashMap<String, MouseControlDef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MidiDevice {
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    pub(crate) match_criteria: MatchCriteria,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) metadata: Option<DeviceMetadata>,
    pub(crate) controls: HashMap<String, MidiControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MatchCriteria {
    #[serde(with = "serde_regex")]
    pub(crate) name_regex: Option<regex::Regex>,
    pub(crate) name_exact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DeviceMetadata {
    pub(crate) friendly_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MidiControl {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) predefined_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) midi_message: Option<MidiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) range: Option<Interval<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MidiMessage {
    #[serde(rename = "type")]
    pub(crate) msg_type: MidiMessageType,
    #[serde(
        default = "default_channel",
        deserialize_with = "deserialize_midi_channel"
    )]
    pub(crate) channel: MidiChannel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) number: Option<MidiNumber>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MidiMessageType {
    PitchWheel,
    ControlChange,
    Note,
    NoteOn,
    NoteOff,
    Aftertouch,
    ProgramChange,
}

#[derive(Debug, Clone)]
pub(crate) enum MidiChannel {
    Any,
    Number(u8),
}

impl Default for MidiChannel {
    fn default() -> Self {
        MidiChannel::Any
    }
}

fn deserialize_midi_channel<'de, D>(deserializer: D) -> Result<MidiChannel, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MidiChannelHelper {
        String(String),
        Number(u8),
    }

    match MidiChannelHelper::deserialize(deserializer)? {
        MidiChannelHelper::String(s) => {
            if s.to_lowercase() == "any" {
                Ok(MidiChannel::Any)
            } else {
                Err(D::Error::custom(format!(
                    "invalid channel string: '{}', expected 'any' or a number",
                    s
                )))
            }
        }
        MidiChannelHelper::Number(n) => {
            if n <= 15 {
                Ok(MidiChannel::Number(n))
            } else {
                Err(D::Error::custom(format!(
                    "channel number {} out of range (0-15)",
                    n
                )))
            }
        }
    }
}

impl Serialize for MidiChannel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            MidiChannel::Any => serializer.serialize_str("any"),
            MidiChannel::Number(n) => serializer.serialize_u8(*n),
        }
    }
}

fn default_channel() -> MidiChannel {
    MidiChannel::Any
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum MidiNumber {
    Single(u8),
    Multiple(Vec<u8>),
    Special(String), // "any", "lowest", "highest"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MouseDevice {
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    pub(crate) match_criteria: MatchCriteria,
    pub(crate) controls: HashMap<String, MouseControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MouseControl {
    #[serde(rename = "type")]
    pub(crate) control_type: String,
    #[serde(with = "crate::common::mmvj_event_code_serde")]
    pub(crate) code: crate::common::MmVjEventCode,
    pub(crate) range: Interval<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
}

// Virtual Joystick
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VirtualJoystick {
    pub(crate) properties: JoystickProperties,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) capabilities: Option<JoystickCapabilities>,
    pub(crate) controls: HashMap<String, JoystickControl>,
}

impl VirtualJoystick {
    pub(crate) fn is_ff_enabled(&self) -> bool {
        self.capabilities
            .as_ref()
            .and_then(|c| c.ff.as_ref())
            .map(|ff| ff.enabled)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JoystickProperties {
    pub(crate) name: String,
    #[serde(default = "default_vendor_id")]
    pub(crate) vendor_id: u16,
    #[serde(default = "default_product_id")]
    pub(crate) product_id: u16,
    #[serde(default = "default_version")]
    pub(crate) version: u16,
}

fn default_vendor_id() -> u16 {
    0x1234
}
fn default_product_id() -> u16 {
    0x5678
}
fn default_version() -> u16 {
    0x0100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JoystickCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ff: Option<FFCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FFCapabilities {
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) effects: Vec<String>,
    #[serde(default = "default_max_effects")]
    pub(crate) max_effects: u32,
    #[serde(default = "default_gain")]
    pub(crate) gain: f32,
    #[serde(default)]
    pub(crate) autocenter: f32,
}

fn default_max_effects() -> u32 {
    1
}
fn default_gain() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JoystickControl {
    #[serde(rename = "type")]
    pub(crate) control_type: ControlType,
    pub(crate) code: String,
    pub(crate) range: Interval<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) properties: Option<AxisProperties>,
    #[serde(default)]
    pub(crate) initial_value: i32,
    #[serde(skip_serializing)]
    #[serde(skip_deserializing)]
    pub(crate) idle_tick_enabled__: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ControlType {
    Axis,
    Button,
    Hat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AxisProperties {
    #[serde(default = "default_resolution")]
    pub(crate) resolution: u32,
    #[serde(default)]
    pub(crate) fuzz: u32,
    #[serde(default)]
    pub(crate) flat: u32,
}

fn default_resolution() -> u32 {
    1
}

// Mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Mapping {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    pub(crate) source: MappingSource,
    pub(crate) destination: MappingDestination,
    #[serde(default)]
    pub(crate) transformation: Transformation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MappingSource {
    pub(crate) device: String,
    pub(crate) control: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MappingDestination {
    pub(crate) joystick: String,
    pub(crate) control: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Transformation {
    List(Vec<TransformationStep>),
}

impl Default for Transformation {
    fn default() -> Self {
        Transformation::List(Vec::new())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum TransformationStep {
    Invert {
        invert: InvertTransform,
    },
    Integrate {
        integrate: IntegrateTransform,
    },
    Curve {
        curve: Curve,
    },
    Autocenter {
        autocenter: AutocenterTransform,
    },
    ForceFeedback {
        force_feedback: ForceFeedbackTransform,
    },
    Steering {
        steering: SteeringTransform,
    },
    Clamp {
        clamp: ClampTransform,
    },
    PedalSmoother {
        pedal_smoother: PedalSmootherTransform,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct InvertTransform {
    #[serde(default)]
    pub(crate) is_relative: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Curve {
    #[serde(rename = "type")]
    pub(crate) curve_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parameters: Option<HashMap<String, f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IntegrateTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) range: Option<Interval<f32>>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) deadzone_norm: Option<f32>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) smoothing_alpha: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutocenterTransform {
    pub(crate) halflife: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) hold_factor: Option<HoldFactor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ForceFeedbackTransform {
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default = "default_ff_influence")]
    pub(crate) constant_force_influence: f32,
    #[serde(default = "default_ff_scale")]
    pub(crate) constant_force_scale: f32,
    #[serde(default)]
    pub(crate) constant_force_invert: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) hold_factor: Option<HoldFactor>,
}

fn default_ff_influence() -> f32 {
    0.7
}

fn default_ff_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClampTransform {
    pub from: Option<i32>,
    pub to: Option<i32>,
    #[serde(default = "clamp_override_transform")]
    pub override_range: bool,
}

fn clamp_override_transform() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedalSmootherTransform {
    pub rise_rate: f32, // e.g., units per second (e.g., 200.0/sec)
    pub fall_rate: f32, // e.g., units per second
    #[serde(default = "default_smoothing_alpha")]
    pub initial_smoothing_alpha: f32,
    pub fall_delay: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fall_gentling_factor: Option<HoldFactor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SteeringTransform {
    #[serde(default = "default_counts_to_lock")]
    pub(crate) counts_to_lock: f32,
    #[serde(default)]
    pub(crate) deadzone_norm: f32,
    #[serde(default)]
    pub(crate) deadzone_counts: f32,
    #[serde(default = "default_smoothing_alpha")]
    pub(crate) smoothing_alpha: f32,
    #[serde(default = "default_auto_center_halflife")]
    pub(crate) auto_center_halflife: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) wheel_hold_factor: Option<HoldFactor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) force_feedback: Option<ForceFeedbackTransform>,
    #[serde(default)]
    pub(crate) invert: bool,
    #[serde(default = "default_true")]
    pub(crate) clamp: bool,
}

fn default_counts_to_lock() -> f32 {
    600.0
}
fn default_smoothing_alpha() -> f32 {
    1.0
}
fn default_auto_center_halflife() -> f32 {
    0.3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum HoldFactor {
    Value(f32),
    Reference { device: String, control: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct InternalControls {
    pub(crate) midi_controls: HashMap<String, MidiControlDef>,
    pub(crate) joystick_controls: HashMap<String, JoystickControlDef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mouse_controls: Option<HashMap<String, MouseControlDef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MidiControlDef {
    pub(crate) midi_message: MidiMessage,
    pub(crate) range: Option<Interval<i32>>,
    pub(crate) description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JoystickControlDef {
    #[serde(rename = "type")]
    pub(crate) control_type: String,
    pub(crate) description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MouseControlDef {
    #[serde(rename = "type")]
    pub(crate) control_type: String,
    pub(crate) code: String,
    pub(crate) range: Interval<i32>,
    pub(crate) description: String,
}
