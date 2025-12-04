#[cfg(feature = "midi")]
use crate::schemas_value::{WithLastKnownIO, WithLastKnownIOSettable, WithNumericValueSettable};
use crate::{
    base_num::{BaseAtomicT, BaseNumT},
    mapped_controls::{MappedCtls, MappedCtlsMidi},
    num_interval::NumInterval,
    schemas_common::{
        IdleTickEnabledFlag, MarkedAsFromPredefinedControl, ObjId, WithRuntimeId, deserialize_device_controls,
    },
    schemas_predefined::MidiControlPredefined,
    schemas_value::{_WithDstRefCount, WithNumericValue},
};
use crossbeam_utils::CachePadded;
// use lasso::Key;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
#[cfg(feature = "midi")]
use std::sync::atomic::Ordering::Relaxed;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicUsize},
};
use strum_macros::{Display, EnumIter, EnumString};
use traversable::{Traversable, TraversableMut};

pub(crate) fn default_channel() -> MidiChannelCfg {
    MidiChannelCfg::Any
}

#[repr(u8)]
#[derive(EnumString, strum_macros::Display, EnumIter)]
pub(crate) enum MidiControlCode {
    Modulation = 1,
    BreathController = 2,
    Volume = 7,
    Pan = 10,
    Expression = 11,
    Sustain = 64,
    Portamento = 65,
    Sostenuto = 66,
    SoftPedal = 67,
    FilterResonance = 71,
    ReleaseTime = 72,
    AttackTime = 73,
    FilterCutoff = 74,
    DecayTime = 75,
}

impl TryFrom<u8> for MidiControlCode {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Modulation),
            2 => Ok(Self::BreathController),
            7 => Ok(Self::Volume),
            10 => Ok(Self::Pan),
            11 => Ok(Self::Expression),
            64 => Ok(Self::Sustain),
            65 => Ok(Self::Portamento),
            66 => Ok(Self::Sostenuto),
            67 => Ok(Self::SoftPedal),
            71 => Ok(Self::FilterResonance),
            72 => Ok(Self::ReleaseTime),
            73 => Ok(Self::AttackTime),
            74 => Ok(Self::FilterCutoff),
            75 => Ok(Self::DecayTime),
            _ => Err("Misc CC code {value}".to_string()),
        }
    }
}

// --------------------------------------------------

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct MidiMessageCfg {
    #[serde(rename = "type")]
    pub(crate) r#type: MappedCtlsMidi,
    #[serde(default = "default_channel", deserialize_with = "deserialize_midi_channel")]
    pub(crate) channel: MidiChannelCfg,
    #[serde(default)]
    pub(crate) number: MidiNumberCfg,
}

#[derive(Debug, Clone, Default, JsonSchema, PartialEq)]
pub(crate) enum MidiChannelCfg {
    #[default]
    Any,
    Number(u8),
}

#[derive(JsonSchema, Debug, Clone, Display, Serialize, Deserialize, PartialEq, EnumString, Default)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
pub(crate) enum MidiNumberSpecial {
    #[default]
    Any,
}

#[derive(JsonSchema, Debug, Clone, Display, Serialize, Deserialize, PartialEq, EnumString)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
pub(crate) enum MidiNumberCfg {
    Single(u16),
    Multiple(Vec<u16>),
    Special(MidiNumberSpecial),
}

impl Default for MidiNumberCfg {
    fn default() -> Self {
        Self::Single(0)
    }
}

// ---------------------------------------------------
#[derive(JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Display)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub(crate) enum MidiMessageType {
    PitchWheel,
    ControlChange,
    #[serde(rename = "note")]
    NoteOn,
    #[serde(skip)]
    NoteOff,
    #[default]
    Aftertouch,
    PolyAftertouch,
    ProgramChange,
}

impl From<MappedCtlsMidi> for MidiMessageType {
    fn from(value: MappedCtlsMidi) -> Self {
        match value {
            MappedCtlsMidi::PitchWheel => MidiMessageType::PitchWheel,
            MappedCtlsMidi::Note => MidiMessageType::NoteOn,
            MappedCtlsMidi::ControlChange => MidiMessageType::ControlChange,
            MappedCtlsMidi::ProgramChange => MidiMessageType::ProgramChange,
            MappedCtlsMidi::Aftertouch => MidiMessageType::Aftertouch,
            MappedCtlsMidi::PolyAftertouch => MidiMessageType::PolyAftertouch,
            MappedCtlsMidi::Unhandled => unreachable!(),
        }
    }
}

impl From<MidiMessageType> for MappedCtls {
    fn from(value: MidiMessageType) -> Self {
        match value {
            MidiMessageType::NoteOn => MappedCtls::Note,
            MidiMessageType::NoteOff => MappedCtls::Note,
            MidiMessageType::ControlChange => MappedCtls::ControlChange,
            MidiMessageType::PitchWheel => MappedCtls::PitchWheel,
            MidiMessageType::Aftertouch => MappedCtls::Aftertouch,
            MidiMessageType::PolyAftertouch => MappedCtls::PolyAftertouch,
            MidiMessageType::ProgramChange => MappedCtls::ProgramChange,
        }
    }
}

// --------------------------------------------

fn deserialize_midi_channel<'de, D>(deserializer: D) -> Result<MidiChannelCfg, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(JsonSchema, Deserialize)]
    #[serde(untagged)]
    enum MidiChannelHelper {
        String(String),
        Number(u8),
    }

    match MidiChannelHelper::deserialize(deserializer)? {
        MidiChannelHelper::String(s) => {
            if s.to_lowercase() == "any" {
                Ok(MidiChannelCfg::Any)
            } else {
                Err(D::Error::custom(format!(
                    "invalid channel string: '{}', expected 'any' or a number",
                    s
                )))
            }
        }
        MidiChannelHelper::Number(n) => {
            if n <= 15 {
                Ok(MidiChannelCfg::Number(n))
            } else {
                Err(D::Error::custom(format!("channel number {} out of interval (0-15)", n)))
            }
        }
    }
}

impl Serialize for MidiChannelCfg {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            MidiChannelCfg::Any => serializer.serialize_str("any"),
            MidiChannelCfg::Number(n) => serializer.serialize_u8(*n),
        }
    }
}

// ------------------------------------------------------

pub(crate) fn deserialize_midi_device_controls<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, MidiControlMatcherCfg>, D::Error> {
    deserialize_device_controls::<'de, D, MidiControlMatcherCfg>(deserializer)
}

#[derive(Debug, Clone, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema)]
pub(crate) struct MidiMatcherCfg {
    pub(crate) enabled: bool,
    #[serde(with = "serde_regex")]
    #[schemars(with = "Option<String>")]
    #[traverse(skip)]
    pub(crate) match_name_regex: regex::Regex,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_midi_device_controls")]
    pub(crate) controls: BTreeMap<String, MidiControlMatcherCfg>,
}

impl PartialEq for MidiMatcherCfg {
    fn eq(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.match_name_regex.as_str() == other.match_name_regex.as_str()
            && self.controls == other.controls
    }
}

impl Default for MidiMatcherCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            match_name_regex: regex::Regex::new(".+").unwrap(),
            controls: Default::default(),
        }
    }
}

impl MarkedAsFromPredefinedControl for MidiControlMatcherCfg {
    fn set_from_predefined_control_marker(&mut self, v: String) {
        self.from_predefined = v
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, Default, TraversableMut, Traversable)]
#[serde(deny_unknown_fields)]
pub(crate) struct MidiControlMatcherCfg {
    // #[serde(skip)]
    // #[traverse(skip)]
    // pub(crate) r#type: MappedCtls,
    #[serde(skip)]
    #[traverse(skip)]
    pub(crate) from_predefined: String,
    #[traverse(skip)]
    pub(crate) midi_message: MidiMessageCfg,
    #[traverse(skip)]
    pub(crate) range: NumInterval<BaseNumT>,
    pub(crate) description: Option<String>,
    #[serde(skip)]
    #[traverse(skip)]
    #[allow(unused)]
    pub(crate) id: ObjId,
    #[serde(skip)]
    #[traverse(skip)]
    pub(crate) idle_tick_enabled: IdleTickEnabledFlag,
    #[serde(skip)]
    #[traverse(skip)]
    current_value: Arc<CachePadded<BaseAtomicT>>,
    #[serde(skip)]
    #[traverse(skip)]
    _dst_refs_count: Arc<CachePadded<AtomicUsize>>,
}

#[cfg(feature = "midi")]
impl WithNumericValueSettable for MidiControlMatcherCfg {
    type ValueT = BaseNumT;
    fn set_numeric_value(&self, v: Self::ValueT) {
        self.current_value.store(v, Relaxed);
    }
}

impl WithLastKnownIO<BaseNumT> for MidiControlMatcherCfg {
    fn get_last_known_io(&self) -> BaseNumT {
        self.current_value.load(Relaxed) as BaseNumT
    }
}

impl WithLastKnownIOSettable<BaseNumT> for MidiControlMatcherCfg {
    fn set_last_known_io(&self, v: BaseNumT) {
        self.current_value.store(v, Relaxed);
    }
}

impl WithNumericValue for MidiControlMatcherCfg {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        self.get_last_known_io()
    }
}

impl PartialEq for MidiControlMatcherCfg {
    fn eq(&self, other: &Self) -> bool {
        self.midi_message == other.midi_message && self.range == other.range && self.description == other.description
    }
}

impl From<MidiControlPredefined> for MidiControlMatcherCfg {
    fn from(value: MidiControlPredefined) -> Self {
        Self {
            midi_message: value.midi_message,
            range: value.range,
            description: Some(value.description), // TODO: remove Option.
            id: Default::default(),
            idle_tick_enabled: Default::default(),
            current_value: Default::default(),
            from_predefined: Default::default(),
            _dst_refs_count: Default::default(),
        }
    }
}

impl _WithDstRefCount for MidiControlMatcherCfg {
    fn _get_dst_refs_count(&self) -> usize {
        self._dst_refs_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn _set_dst_refs_count(&mut self, refs_count: usize) {
        self._dst_refs_count
            .store(refs_count, std::sync::atomic::Ordering::Relaxed)
    }
}

impl WithRuntimeId for MidiControlMatcherCfg {
    fn get_id(&self) -> ObjId {
        self.id
    }

    fn assign_new_id(&mut self) {
        self.id = Default::default()
    }
}
