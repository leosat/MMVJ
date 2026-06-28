use crate::hid_device::HidDeviceKind;
use crate::hid_manager::WithDeviceClassification;
use crate::schemas_hid::HidDeviceCfg;
use crate::schemas_mapping::Mapping;
#[cfg(feature = "midi")]
use crate::schemas_midi::MidiMatcherCfg;
use crate::schemas_predefined::ControlsPredefinedCfg;
use crate::schemas_ui::UiCfg;
use crate::schemas_value::VariableState;
use anyhow::Result;
use doc_for::*;
use garde::Validate;
use schemars::JsonSchema;
use serde::de::IntoDeserializer;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use traversable::{Traversable, TraversableMut};

#[derive(
    Debug, Clone, Default, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema, Validate, PartialEq,
)]
pub(crate) struct DevicesCfgLegacy {
    #[cfg(feature = "midi")]
    #[serde(rename = "midi_devices")]
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[garde(skip)]
    pub(crate) midi: BTreeMap<String, MidiMatcherCfg>,
    #[serde(rename = "mouse_devices")]
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[garde(skip)]
    pub(crate) mice: BTreeMap<String, HidDeviceCfg>,
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[garde(skip)]
    pub(crate) virtual_joysticks: BTreeMap<String, HidDeviceCfg>,
}

#[derive(
    Debug, Clone, Default, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema, Validate, PartialEq,
)]
pub(crate) struct DevicesCfgNew {
    #[cfg(feature = "midi")]
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[garde(skip)]
    pub(crate) midi: BTreeMap<String, MidiMatcherCfg>,
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[garde(skip)]
    pub(crate) hid: BTreeMap<String, HidDeviceCfg>,
}

#[derive(Debug, Clone, Serialize, TraversableMut, Traversable, JsonSchema)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
enum ConfigVariants {
    New(ConfigNew),
    Old(ConfigOld),
}

impl<'de> Deserialize<'de> for ConfigVariants {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let value: serde_value::Value = Deserialize::deserialize(deserializer)?;
        match ConfigNew::deserialize(value.clone().into_deserializer()) {
            Ok(new_cfg) => Ok(ConfigVariants::New(new_cfg)),
            Err(new_err) => match ConfigOld::deserialize(value.into_deserializer()) {
                Ok(old_cfg) => Ok(ConfigVariants::Old(old_cfg)),
                Err(old_err) => Err(D::Error::custom(format!(
                    "Configuration parse error.\nIf using new format: {}\nIf using old format: {}\n",
                    new_err, old_err
                ))),
            },
        }
    }
}

impl From<DevicesCfgLegacy> for DevicesCfgNew {
    fn from(value: DevicesCfgLegacy) -> Self {
        Self {
            #[cfg(feature = "midi")]
            midi: value.midi,
            hid: {
                let mut hid = value.mice;
                hid.append(&mut value.virtual_joysticks.clone());
                hid
            },
        }
    }
}

impl From<DevicesCfgNew> for DevicesCfgLegacy {
    fn from(value: DevicesCfgNew) -> Self {
        Self {
            #[cfg(feature = "midi")]
            midi: value.midi,
            mice: value
                .hid
                .iter()
                .filter(|cfg| cfg.1.get_classification().contains(HidDeviceKind::Mouse))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
            virtual_joysticks: value
                .hid
                .iter()
                .filter(|cfg| cfg.1.get_classification().contains(HidDeviceKind::Joystick))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
        }
    }
}

impl From<ConfigVariants> for Config {
    fn from(value: ConfigVariants) -> Self {
        match value {
            ConfigVariants::Old(c) => Self {
                cfg_file: c.cfg_file,
                description: c.description,
                predef_controls: c.predef_controls,
                global: c.global,
                devices: c.devices.into(),
                variables: c.variables,
                mappings: c.mappings,
                ui: Default::default(),
            },
            ConfigVariants::New(c) => Self {
                cfg_file: c.cfg_file,
                description: c.description,
                predef_controls: c.predef_controls,
                global: c.global,
                devices: c.devices,
                variables: c.variables,
                mappings: c.mappings,
                ui: c.ui,
            },
        }
    }
}

// WHYMACRO: To do it with generics and type aliases would need to have a tuple struct for final config struct
// WHYMACRO: to specifically implement serde from only on it. Attribute can't be set on type alias.
// WHYMACRO: Since I don't want to have a tuple struct for config, I go with macro.
macro_rules! config_struct_tpl {
  ($n:ident,$( $devices_meta:meta )?, $devices:ident) => {
    config_struct_tpl!(, $( $devices_meta )?, $n, $devices);
  };
  ($( $m:meta )*, $( $devices_meta:meta )?, $n:ident, $devices:ident) => {
    #[derive(PartialEq, Debug, Clone, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema, Validate)]
    $( #[$m] )*
    #[serde(deny_unknown_fields)]
    pub(crate) struct $n {
        #[traverse(skip)]
        #[serde(skip)]
        #[garde(skip)]
        pub(crate) cfg_file: PathBuf,
        #[serde(default)]
        #[garde(skip)]
        pub(crate) description: String,
        #[serde(skip_serializing)]
        #[traverse(skip)]
        #[serde(default = "crate::config::load_predefined_controls")]
        #[garde(skip)]
        pub(crate) predef_controls: ControlsPredefinedCfg,
        #[traverse(skip)]
        #[garde(skip)]
        pub(crate) global: GlobalSettingsCfg,
        $( #[ $devices_meta ] ) ?
        #[garde(skip)]
        pub(crate) devices: $devices,
        #[serde(default)]
        #[garde(skip)]
        pub(crate) variables: VariablesCfg,
        #[serde(default)]
        #[garde(skip)]
        pub(crate) mappings: Vec<Mapping>,
        #[serde(default)]
        #[garde(skip)]
        pub(crate) ui: UiCfg,
    }
  };
}

config_struct_tpl!(
    serde(from = "ConfigVariants"),
    ,
    Config,
    DevicesCfgNew
);
config_struct_tpl!(ConfigOld, serde(flatten), DevicesCfgLegacy);
config_struct_tpl!(ConfigNew, , DevicesCfgNew);

// --------------------------------------

pub(crate) type VariablesCfg = BTreeMap<String, VariableState>;

// --------------------------------------

pub(crate) fn default_update_rate() -> u32 {
    200
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, Default, Validate, PartialEq)]
#[serde(deny_unknown_fields)]
#[doc_impl]
pub(crate) struct GlobalSettingsCfg {
    /// The program will run idle tick with this rate in Hz.
    #[serde(default = "default_update_rate")]
    #[garde(range(min=crate::config::MIN_BASE_FREQ_HZ))]
    pub(crate) idle_tick_rate: u32,
    /// If true, all virtual joysticks will be persistent (not destroyed on hot-reload) by default.
    #[serde(default)]
    #[garde(skip)]
    pub(crate) persistent_joysticks: bool,
}
