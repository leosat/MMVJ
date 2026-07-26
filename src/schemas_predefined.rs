#[cfg(feature = "midi")]
use crate::schemas_midi::MidiMessageCfg;
use crate::{base_num::BaseNumT, mapped_controls::MappedCtls, num_interval::NumInterval, schemas_hid::AxisProperties};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// use serde_valid::Validate;
#[derive(JsonSchema, Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlsPredefinedCfg {
    #[cfg(feature = "midi")]
    #[serde(default)]
    pub(crate) midi_controls: BTreeMap<String, MidiControlPredefined>,
    #[serde(default)]
    pub(crate) hid_controls: BTreeMap<String, HidControlPredefined>,
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct HidControlPredefined {
    pub(crate) r#type: MappedCtls,
    pub(crate) range: NumInterval<BaseNumT>,
    #[serde(default)]
    pub(crate) properties: Option<AxisProperties>,
    #[serde(default)]
    pub(crate) initial_value: BaseNumT,
    #[serde(default)]
    pub(crate) description: String,
}

#[cfg(feature = "midi")]
#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct MidiControlPredefined {
    pub(crate) midi_message: MidiMessageCfg,
    pub(crate) range: NumInterval<BaseNumT>,
    pub(crate) description: String,
}
