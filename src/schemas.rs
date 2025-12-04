use crate::common::NumInterval;
use doc_for::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
#[doc_impl]
pub(crate) struct GlobalSettings {
    /// The program will run idle tick with this rate in Hz.
    #[serde(default = "default_update_rate")]
    pub(crate) idle_tick_update_rate: u32,
    /// If true, all virtual joysticks will be persistent (not destroyed on hot-reload) by default.
    #[serde(default)]
    pub(crate) persistent_joysticks: bool,
}

fn default_update_rate() -> u32 {
    1000
}

fn default_true() -> bool {
    true
}

// ----------------
// Control Entry Types - Support both shorthand string and full object syntax
// ----------------
/// Wrapper for control definitions that supports both:
/// - Shorthand: `my_control: "predefined_name"`
/// - Full: `my_control: { merge_from: ..., ... }`
#[derive(Debug, Clone)]
pub(crate) enum ControlEntry<T> {
    Shorthand(String),
    Full(T),
}

impl<T: Serialize> Serialize for ControlEntry<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ControlEntry::Shorthand(s) => serializer.serialize_str(s),
            ControlEntry::Full(t) => t.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ControlEntry<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;
        use std::marker::PhantomData;

        struct ControlEntryVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ControlEntryVisitor<T> {
            type Value = ControlEntry<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string (predefined name) or an object (control definition)")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(ControlEntry::Shorthand(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(ControlEntry::Shorthand(value))
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let t = T::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(ControlEntry::Full(t))
            }
        }

        deserializer.deserialize_any(ControlEntryVisitor(PhantomData))
    }
}

// ----------------
// MIDI Types
// ----------------
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MidiDevice {
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(with = "serde_regex")]
    pub(crate) match_name_regex: Option<regex::Regex>,
    pub(crate) controls: HashMap<String, ControlEntry<MidiControl>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct MidiControl {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) merge_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) midi_message: Option<MidiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) range: Option<NumInterval<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub(crate) enum MidiMessageType {
    PitchWheel,
    ControlChange,
    Note,
    NoteOn,
    NoteOff,
    Aftertouch,
    ProgramChange,
}

#[derive(Debug, Clone, Default)]
pub(crate) enum MidiChannel {
    #[default]
    Any,
    Number(u8),
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
#[serde(deny_unknown_fields)]
pub(crate) enum MidiNumber {
    Single(u8),
    Multiple(Vec<u8>),
    Special(String),
}

// ----------------
// Mouse Types
// ----------------
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MouseDevice {
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(with = "serde_regex")]
    pub(crate) match_name_regex: Option<regex::Regex>,
    pub(crate) controls: HashMap<String, ControlEntry<MouseControl>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct MouseControl {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) merge_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) r#type: Option<crate::common::ControlType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) range: Option<NumInterval<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
}

/// Resolved mouse control with all required fields populated
#[derive(Debug, Clone)]
pub(crate) struct ResolvedMouseControl {
    pub(crate) r#type: crate::common::ControlType,
    pub(crate) range: NumInterval<i32>,
    pub(crate) _description: Option<String>,
}

// ----------------
// Virtual Joystick Types
// ----------------
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VirtualJoystick {
    pub(crate) name: String,
    pub(crate) enabled: Option<bool>,
    /// If set, overrides the global persistence setting for this specific joystick.
    pub(crate) persistent: Option<bool>,
    pub(crate) properties: JoystickProperties,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) force_feedback: Option<FFCapabilities>,
    pub(crate) controls: HashMap<String, ControlEntry<JoystickControl>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoystickProperties {
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub(crate) struct JoystickControl {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) merge_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) r#type: Option<crate::common::ControlType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) range: Option<NumInterval<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) properties: Option<AxisProperties>,
    #[serde(default)]
    pub(crate) initial_value: i32,
    #[serde(skip_serializing)]
    #[serde(skip_deserializing)]
    pub(crate) idle_tick_enabled_flag: Arc<AtomicBool>,
}

impl Default for JoystickControl {
    fn default() -> Self {
        Self {
            merge_from: None,
            r#type: None,
            range: None,
            properties: None,
            initial_value: 0,
            idle_tick_enabled_flag: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Resolved joystick control with all required fields populated
#[derive(Debug, Clone)]
pub(crate) struct ResolvedJoystickControl {
    pub(crate) r#type: crate::common::ControlType,
    pub(crate) range: NumInterval<i32>,
    pub(crate) properties: Option<AxisProperties>,
    pub(crate) initial_value: i32,
    pub(crate) idle_tick_enabled_flag: Arc<AtomicBool>,
}

// ----------------
// Resolved Device Structures
// ----------------
#[derive(Debug, Clone)]
pub(crate) struct ResolvedVirtualJoystick {
    pub(crate) enabled: bool,
    pub(crate) persistent: bool,
    pub(crate) name: String,
    pub(crate) properties: JoystickProperties,
    pub(crate) controls: HashMap<String, ResolvedJoystickControl>,
    pub(crate) force_feedback: Option<FFCapabilities>,
}

impl ResolvedVirtualJoystick {
    pub(crate) fn is_ff_enabled(&self) -> bool {
        self.force_feedback
            .as_ref()
            .map(|c| c.enabled)
            .unwrap_or(false)
    }
}

/// Fully resolved MIDI device configuration - all controls expanded
#[derive(Debug, Clone)]
pub(crate) struct ResolvedMidiDevice {
    pub(crate) enabled: bool,
    pub(crate) match_name_regex: Option<regex::Regex>,
    pub(crate) controls: HashMap<String, MidiControl>,
}

/// Fully resolved mouse device configuration - all controls expanded
#[derive(Debug, Clone)]
pub(crate) struct ResolvedMouseDevice {
    pub(crate) enabled: bool,
    pub(crate) match_name_regex: Option<regex::Regex>,
    pub(crate) controls: HashMap<String, ResolvedMouseControl>,
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

// ----------------
// Mapping Types
// ----------------
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mapping {
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
#[serde(deny_unknown_fields)]
pub(crate) struct MappingSource {
    pub(crate) device: String,
    pub(crate) control: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MappingDestination {
    pub(crate) joystick: String,
    pub(crate) control: String,
}

pub(crate) type Transformation = Vec<TransformationStep>;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) enum TransformationStep {
    Invert {
        invert: InvertTransform,
    },
    Integrate {
        integrate: IntegrateTransform,
    },
    // Curve {
    //     curve: CurveTransform,
    // },
    Steering {
        steering: SteeringTransform,
    },
    Clamp {
        clamp: ClampTransform,
    },
    PedalSmoother {
        pedal_smoother: PedalSmootherTransform,
    },
    EmaFilter {
        ema_filter: EmaFilterTransform,
    },
    Linear {
        linear: LinearTransform,
    },
    Quadratic {
        quadratic_curve: QuadraticTransform,
    },
    Cubic {
        cubic_curve: CubicTransform,
    },
    Smoothstep {
        smoothstep_curve: SmoothstepTransform,
    },
    SCurve {
        s_curve: SCurveTransform,
    },
    Exponential {
        exp_curve: ExponentialTransform,
    },
    Power {
        power_curve: PowerTransform,
    },
    SymmetricPower {
        symmetric_power_curve: SymmetricPowerTransform,
    },
    LowPass {
        lowpass: LowPassTransform,
    },
    _HighPass {
        highpass: HighPassTransform,
    },
}

impl<'de> Deserialize<'de> for TransformationStep {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct StepVisitor;

        impl<'de> Visitor<'de> for StepVisitor {
            type Value = TransformationStep;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(
                    "a map with exactly one transformation step key (e.g. { clamp: {...} })",
                )
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let Some(key) = map.next_key::<String>()? else {
                    return Err(de::Error::custom("empty transformation step object"));
                };

                let step = match key.as_str() {
                    "invert" => TransformationStep::Invert {
                        invert: map.next_value()?,
                    },
                    "integrate" => TransformationStep::Integrate {
                        integrate: map.next_value()?,
                    },
                    "steering" => TransformationStep::Steering {
                        steering: map.next_value()?,
                    },
                    "clamp" => TransformationStep::Clamp {
                        clamp: map.next_value()?,
                    },
                    "pedal_smoother" => TransformationStep::PedalSmoother {
                        pedal_smoother: map.next_value()?,
                    },
                    "ema_filter" => TransformationStep::EmaFilter {
                        ema_filter: map.next_value()?,
                    },
                    "linear" => TransformationStep::Linear {
                        linear: map.next_value()?,
                    },
                    "quadratic" => TransformationStep::Quadratic {
                        quadratic_curve: map.next_value()?,
                    },
                    "cubic" => TransformationStep::Cubic {
                        cubic_curve: map.next_value()?,
                    },
                    "smoothstep" => TransformationStep::Smoothstep {
                        smoothstep_curve: map.next_value()?,
                    },
                    "s_curve" => TransformationStep::SCurve {
                        s_curve: map.next_value()?,
                    },
                    "exp" => TransformationStep::Exponential {
                        exp_curve: map.next_value()?,
                    },
                    "power" => TransformationStep::Power {
                        power_curve: map.next_value()?,
                    },
                    "symmetric_power" => TransformationStep::SymmetricPower {
                        symmetric_power_curve: map.next_value()?,
                    },
                    "lowpass" => TransformationStep::LowPass {
                        lowpass: map.next_value()?,
                    },
                    // "highpass" => TransformationStep::HighPass {
                    //     highpass: map.next_value()?,
                    // },
                    other => {
                        return Err(de::Error::custom(format!(
                            "unknown transformation step type '{}'. Expected one of: \
invert, integrate, steering, clamp, pedal_smoother, ema_filter, linear, quadratic, cubic, \
smoothstep, s_curve, exp, power, symmetric_power, lowpass",
                            other
                        )));
                    }
                };

                // Enforce "exactly one key" (catch typos like { clamp: {...}, foo: 1 })
                if let Some(extra) = map.next_key::<String>()? {
                    return Err(de::Error::custom(format!(
                        "transformation step must contain exactly one key; found extra key '{}' after '{}'",
                        extra, key
                    )));
                }

                Ok(step)
            }
        }

        deserializer.deserialize_map(StepVisitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinearTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) slope: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) shift_x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) shift_y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuadraticTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CubicTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SmoothstepTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SCurveTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) steepness: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExponentialTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) base: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SymmetricPowerTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) power: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PowerTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) power: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct LowPassTransform {
    pub(crate) time_constant: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct HighPassTransform {
    pub(crate) cutoff: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvertTransform {
    #[serde(default)]
    pub(crate) is_relative: bool,
}

// #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
// #[serde(rename_all = "snake_case")]
// pub(crate) enum CurveType {
//     Linear,
//     Quadratic,
//     Cubic,
//     Smoothstep,
//     SCurve,
//     Exponential,
//     SymmetricPower,
//     Power,
// }

// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
// #[serde(deny_unknown_fields)]
// pub(crate) struct CurveTransform {
//     #[serde(rename = "type")]
//     pub(crate) curve_type: CurveType,
//     #[serde(default)]
//     #[serde(alias = "parameters")]
//     pub(crate) params: Option<HashMap<String, f32>>,
// }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntegrateTransform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) range: Option<NumInterval<f32>>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) deadzone_norm: Option<f32>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) smoothing_alpha: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
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
    #[serde(alias = "wheel_hold_factor")]
    pub(crate) hold_factor: Option<HoldFactor>,
}

fn default_ff_influence() -> f32 {
    0.7
}

fn default_ff_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClampTransform {
    pub from: Option<i32>,
    pub to: Option<i32>,
    #[serde(default = "clamp_override_transform")]
    pub override_range: bool,
}

fn clamp_override_transform() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PedalSmootherTransform {
    pub rise_rate: f32,
    pub fall_rate: f32,
    #[serde(default = "default_smoothing_alpha")]
    pub smoothing_alpha: f32,
    pub fall_delay: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fall_gentling_factor: Option<HoldFactor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmaFilterTransform {
    pub on_idle: Option<bool>,
    pub tau: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SteeringTransform {
    #[serde(default = "default_counts_to_lock")]
    pub(crate) counts_to_lock: f32,
    #[serde(default)]
    pub(crate) deadzone_counts: f32,
    #[serde(default = "default_smoothing_alpha")]
    pub(crate) smoothing_alpha: f32,
    #[serde(default = "default_auto_center_halflife")]
    pub(crate) auto_center_halflife: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "wheel_hold_factor")]
    pub(crate) hold_factor: Option<HoldFactor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) force_feedback: Option<ForceFeedbackTransform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) user_input_power_curve: Option<SymmetricPowerTransform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) user_input_ema_filter: Option<EmaFilterTransform>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
pub(crate) enum HoldFactor {
    Value(f32),
    Reference { device: String, control: String },
}

// ----------------
// Predefined Control Definitions (for predefines file)
// ----------------
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlsPredefined {
    #[serde(default)]
    pub(crate) midi_controls: HashMap<String, MidiControlPredefined>,
    #[serde(default)]
    pub(crate) joystick_controls: HashMap<String, JoystickControlPredefined>,
    #[serde(default)]
    pub(crate) mouse_controls: HashMap<String, MouseControlPredefined>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MidiControlPredefined {
    pub(crate) midi_message: MidiMessage,
    pub(crate) range: NumInterval<i32>,
    pub(crate) description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoystickControlPredefined {
    pub(crate) r#type: crate::common::ControlType,
    pub(crate) range: NumInterval<i32>,
    pub(crate) properties: Option<AxisProperties>,
    pub(crate) initial_value: i32,
    pub(crate) description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MouseControlPredefined {
    pub(crate) r#type: crate::common::ControlType,
    pub(crate) range: NumInterval<i32>,
    pub(crate) description: String,
}

// ----------------
// Resolved Transformation Types
// ----------------
pub(crate) type ResolvedTransformation = Vec<ResolvedTransformationStep>;

pub(crate) type StepRuntimeStateId = usize;

#[derive(Debug, Clone)]
pub(crate) enum ResolvedTransformationStep {
    Invert {
        invert: InvertTransform,
    },
    Integrate {
        runtime_state_id: StepRuntimeStateId,
        integrate: IntegrateTransform,
    },
    // Curve {
    //     curve: CurveTransform,
    // },
    Steering {
        runtime_state_id: StepRuntimeStateId,
        steering: ResolvedSteeringTransform,
    },
    Clamp {
        clamp: ClampTransform,
    },
    PedalSmoother {
        runtime_state_id: StepRuntimeStateId,
        pedal_smoother: ResolvedPedalSmootherTransform,
    },
    EmaFilter {
        runtime_state_id: StepRuntimeStateId,
        ema_filter: EmaFilterTransform,
    },
    Linear {
        linear: LinearTransform,
    },
    Quadratic {
        quadratic_curve: QuadraticTransform,
    },
    Cubic {
        cubic_curve: CubicTransform,
    },
    Smoothstep {
        smoothstep_curve: SmoothstepTransform,
    },
    SCurve {
        s_curve: SCurveTransform,
    },
    Exponential {
        exp_curve: ExponentialTransform,
    },
    Power {
        power_curve: PowerTransform,
    },
    SymmetricPower {
        symmetric_power_curve: SymmetricPowerTransform,
    },
    LowPass {
        runtime_state_id: StepRuntimeStateId,
        lowpass: LowPassTransform,
    },
    HighPass {
        runtime_state_id: StepRuntimeStateId,
        highpass: HighPassTransform,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedSteeringTransform {
    pub(crate) counts_to_lock: f32,
    #[allow(dead_code)]
    pub(crate) deadzone_counts: f32,
    pub(crate) smoothing_alpha: f32,
    pub(crate) auto_center_halflife: f32,
    pub(crate) hold_factor: Option<ResolvedHoldFactor>,
    pub(crate) force_feedback: Option<ForceFeedbackTransform>,
    pub(crate) user_input_power_curve: Option<SymmetricPowerTransform>,
    pub(crate) user_input_ema_filter_runtime_state_id: StepRuntimeStateId,
    pub(crate) user_input_ema_filter_average: Option<EmaFilterTransform>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedPedalSmootherTransform {
    pub(crate) rise_rate: f32,
    pub(crate) fall_rate: f32,
    pub(crate) smoothing_alpha: f32,
    pub(crate) fall_delay: Option<f32>,
    pub(crate) fall_gentling_factor: Option<ResolvedHoldFactor>,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedHoldFactor {
    Value(f32),
    Reference {
        device: String,
        control: String,
        range: NumInterval<i32>,
    },
}
