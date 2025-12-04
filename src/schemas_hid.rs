use crate::{
    base_num::{BaseAtomicT, BaseNumT},
    device_and_device_manager::{DeviceClassification, WithDeviceClassification},
    hid_device::HID_AXIS_MAX_INTERVAL,
    mapped_controls::MappedCtls,
    num_interval::NumInterval,
    schemas_common::{
        IdleTickEnabledFlag, MarkedAsFromPredefinedControl, ObjId, WithRuntimeId, deserialize_device_controls,
        is_false, is_none_or_default, is_zero,
    },
    schemas_predefined::HidControlPredefined,
    schemas_value::{
        _WithDstRefCount, WithLastKnownIO, WithLastKnownIOSettable, WithNumericValue, WithNumericValueSettable,
    },
};
use crossbeam_utils::CachePadded;
use evdev::FFEffectCode;
use garde::Validate;
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::Relaxed},
    },
};
use strum_macros::{Display, EnumIter, EnumString};
use traversable::{Traversable, TraversableMut};

use schemars::JsonSchema;
// use serde_valid::Validate;

pub(crate) fn default_vendor_id() -> u16 {
    0x1234
}

pub(crate) fn default_product_id() -> u16 {
    0x5678
}

pub(crate) fn default_version() -> u16 {
    0x0100
}

pub(crate) fn default_max_effects() -> usize {
    16
}

pub(crate) fn default_gain() -> BaseNumT {
    1.0
}

pub(crate) fn default_resolution() -> BaseNumT {
    1.0
}

pub(crate) fn deserialize_jk_device_controls<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, HidControlMatcherCfg>, D::Error> {
    deserialize_device_controls::<'de, D, HidControlMatcherCfg>(deserializer)
}

#[derive(Debug, Clone, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct HidMatcherParamsCfg {
    #[serde(with = "serde_regex")]
    #[schemars(with = "Option<String>")]
    #[traverse(skip)]
    pub(crate) match_name_regex: regex::Regex,
}

impl PartialEq for HidMatcherParamsCfg {
    fn eq(&self, other: &Self) -> bool {
        self.match_name_regex.as_str() == other.match_name_regex.as_str()
    }
}

impl Default for HidMatcherParamsCfg {
    fn default() -> Self {
        Self {
            match_name_regex: regex::Regex::new(".*").unwrap(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct HidVirtualParamsCfg {
    #[serde(default)]
    // #[serde(skip_serializing_if = "String::is_empty")]
    // TODO: sanitize_hid_name !!! (e.g. it gets truncated before spawhing a device, but need to sanitize it here)
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) persistent: bool,
    #[traverse(skip)]
    #[serde(default)]
    #[serde(alias = "properties")]
    // #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bus: Option<HidDeviceBusSpecCfg>,
    #[traverse(skip)]
    pub(crate) force_feedback: Option<HIDDeviceForceFeedbackCfg>,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    TraversableMut,
    deserialize_untagged_verbose_error::DeserializeUntaggedVerboseError,
    Traversable,
    JsonSchema,
    PartialEq,
)]
#[serde(untagged)]
pub(crate) enum HidVirtualOrMatcherParamsCfg {
    DeviceMatcher(HidMatcherParamsCfg),
    VirtualDevice(HidVirtualParamsCfg),
}

impl Default for HidVirtualOrMatcherParamsCfg {
    fn default() -> Self {
        Self::DeviceMatcher(Default::default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct HidDeviceClassificationCfg(pub(crate) DeviceClassification);

#[derive(Debug, Clone, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema, Default, PartialEq)]
// NB/TODO?: this will not work along with variable params__ deserialization... find out, why #[serde(deny_unknown_fields)]
pub(crate) struct HidDeviceCfg {
    pub(crate) enabled: bool,
    #[serde(default)]
    #[schemars(default)]
    pub(crate) description: String,
    #[serde(flatten)]
    pub(crate) params__: HidVirtualOrMatcherParamsCfg,
    #[schemars(skip)]
    #[traverse(skip)]
    #[serde(skip)]
    pub(crate) classification: Option<HidDeviceClassificationCfg>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_jk_device_controls")]
    pub(crate) controls: BTreeMap<String, HidControlMatcherCfg>,
}

impl HidDeviceCfg {
    pub(crate) fn new_matcher() -> Self {
        Self {
            params__: HidVirtualOrMatcherParamsCfg::DeviceMatcher(HidMatcherParamsCfg::default()),
            ..Default::default()
        }
    }

    pub(crate) fn new_virtual(name: &str) -> Self {
        Self {
            params__: HidVirtualOrMatcherParamsCfg::VirtualDevice(HidVirtualParamsCfg {
                name: name.to_string(),
                persistent: false,
                bus: Some(Default::default()),
                force_feedback: Default::default(),
            }),
            ..Default::default()
        }
    }

    #[allow(unused)]
    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn is_persistent(&self) -> bool {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(ref p) => p.persistent,
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => false,
        }
    }

    pub(crate) fn virtual_device_persistent_mut(&mut self) -> Option<&mut bool> {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(ref mut p) => Some(&mut p.persistent),
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => {
                // log::error!("Persistency parameter is only available for virtual device config  {self:?}.");
                None
            }
        }
    }

    #[allow(unused)]
    pub(crate) fn description_ref(&self) -> &str {
        &self.description
    }

    pub(crate) fn virtual_device_name_ref(&self) -> Option<&str> {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(ref p) => Some(&p.name),
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => {
                // log::error!(
                //     "Name parameter is only available for virtual device config, calling it on device matcher config {self:#?}."
                // );
                None
            }
        }
    }

    pub(crate) fn virtual_device_name_mut(&mut self) -> Option<&mut String> {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(ref mut p) => Some(&mut p.name),
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => {
                // log::error!(
                //     "Name parameter is only available for virtual device config, calling it on device matcher config {self:#?}"
                // );
                None
            }
        }
    }

    pub(crate) fn matcher_name_regex_mut(&mut self) -> Option<&mut regex::Regex> {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(_) => {
                // log::error!(
                //     "Name regex is only available for device matcher config, calling it on virtual device config {self:#?}"
                // );
                None
            }
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(ref mut p) => Some(&mut p.match_name_regex),
        }
    }

    pub(crate) fn matcher_name_regex_ref(&self) -> Option<&regex::Regex> {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(_) => {
                // log::error!(
                //     "Name regex is only available for device matcher config, calling it on virtual device config {self:#?}"
                // );
                None
            }
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(ref p) => Some(&p.match_name_regex),
        }
    }

    pub(crate) fn virtual_device_bus_info_ref(&self) -> Option<&HidDeviceBusSpecCfg> {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(ref p) => p.bus.as_ref(),
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => {
                // log::error!(
                //     "Bus info parameters are only available for virtual device config, calling it on device matcher config {self:#?}"
                // );
                None
            }
        }
    }

    pub(crate) fn virtual_device_bus_info_mut(&mut self) -> Option<&mut HidDeviceBusSpecCfg> {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(ref mut p) => p.bus.as_mut(),
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => {
                log::error!(
                    "Bus info parameters are only only available for virtual device config, calling it on device matcher config {self:#?}"
                );
                None
            }
        }
    }

    pub(crate) fn virtual_device_force_feedback_info_ref(&self) -> Option<&HIDDeviceForceFeedbackCfg> {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(ref p) => p.force_feedback.as_ref(),
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => {
                log::error!(
                    "Force feedback parameters are only available for virtual device config, calling it on device matcher config {self:#?}"
                );
                None
            }
        }
    }

    pub(crate) fn add_virtual_device_force_feedback_params(&mut self) {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(ref mut p) => {
                p.force_feedback = Some(Default::default());
            }
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => {
                log::error!(
                    "Force feedback parameters are only available for virtual device config, calling it on device matcher config {self:#?}"
                );
            }
        }
    }

    pub(crate) fn virtual_device_force_feedback_info_mut(&mut self) -> Option<&mut HIDDeviceForceFeedbackCfg> {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::VirtualDevice(ref mut p) => p.force_feedback.as_mut(),
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => {
                log::error!(
                    "Name parameter is only available for virtual device config, calling it on device matcher config {self:#?}"
                );
                None
            }
        }
    }

    pub(crate) fn add_special_force_feedback_controls(&mut self) {
        if self.is_a_virtual() && self.virtual_device_force_feedback_info_ref().is_some() {
            for ctl_type in [MappedCtls::ForceFeedbackX, MappedCtls::ForceFeedbackY] {
                if !self.controls.iter().any(|v| v.1.r#type == ctl_type) {
                    self.controls.insert(
                        ctl_type.to_string(),
                        HidControlMatcherCfg {
                            r#type: ctl_type,
                            range: HID_AXIS_MAX_INTERVAL,
                            properties: Default::default(),
                            initial_value: 0.0,
                            id: Default::default(),
                            idle_tick_enabled: Default::default(),
                            last_known_io_value: Default::default(),
                            from_predefined: Default::default(),
                            _dst_refs_count: Default::default(),
                            current_value: Default::default(),
                        },
                    );
                }
            }
        }
    }

    pub(crate) fn virtual_device_is_ff_enabled(&self) -> bool {
        if !self.is_a_virtual() {
            log::error!(
                "Trying to get count of FF effects-related info on a device matcher. Only actual for a virtual device."
            );
            return false;
        }
        self.virtual_device_force_feedback_info_ref()
            .as_ref()
            .map(|c| c.enabled)
            .unwrap_or(false)
    }

    pub(crate) fn virtual_device_fake_accepting_all_effects(&self) -> bool {
        if !self.is_a_virtual() {
            log::error!(
                "Trying to get count of FF effects-related info on a device matcher. Only actual for a virtual device."
            );
            return false;
        }
        self.virtual_device_force_feedback_info_ref()
            .as_ref()
            .map(|c| c.fake_accepting_all_effects)
            .unwrap_or(false)
    }

    pub(crate) fn virtual_device_get_ff_max_effects(&self) -> usize {
        if !self.is_a_virtual() {
            log::error!(
                "Trying to get count of suuported FF effects on a device matcher. Only actual for a virtual device."
            );
            return 0;
        }
        self.virtual_device_force_feedback_info_ref()
            .as_ref()
            .map(|c| c.max_effects)
            .unwrap_or(default_max_effects())
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Validate)]
pub(crate) struct AxisProperties {
    #[serde(default = "default_resolution")]
    #[garde(range(min = 0.0))]
    /// Resolution must be positive
    pub(crate) resolution: BaseNumT,
    #[serde(default)]
    #[garde(range(min = 0.0))]
    /// Fuzz/Flat thresholds must be positive
    pub(crate) fuzz: BaseNumT,
    #[serde(default)]
    #[garde(range(min = 0.0))]
    pub(crate) flat: BaseNumT,
}

impl Default for AxisProperties {
    fn default() -> Self {
        Self {
            resolution: 1.0,
            fuzz: Default::default(),
            flat: Default::default(),
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, Default, EnumIter, EnumString, PartialEq, PartialOrd)]
pub(crate) enum HidDeviceBusType {
    #[default]
    Virtual,
    Usb,
    IsaPnp,
    Isa,
    Gameport,
}

#[derive(
    JsonSchema, Debug, Display, Clone, Serialize, Deserialize, Default, EnumIter, EnumString, PartialEq, PartialOrd,
)]
#[strum(serialize_all = "PascalCase")]
#[strum(ascii_case_insensitive)]
pub(crate) enum HidFfEffect {
    #[default]
    #[serde(alias = "constant")]
    Constant,
    #[serde(alias = "periodic")]
    Periodic,
    #[serde(alias = "rumble")]
    Rumble,
    #[serde(alias = "spring")]
    Spring,
    #[serde(alias = "friction")]
    Friction,
    #[serde(alias = "damper")]
    Damper,
    #[serde(alias = "inertia")]
    Intertia,
    #[serde(alias = "ramp")]
    Ramp,
    #[serde(alias = "square")]
    Square,
    #[serde(alias = "triangle")]
    Triangle,
    #[serde(alias = "sine")]
    Sine,
    #[serde(alias = "saw_up")]
    SawUp,
    #[serde(alias = "saw_down")]
    SawDown,
    #[serde(alias = "custom")]
    Custom,
    #[serde(alias = "gain")]
    Gain,
    #[serde(alias = "autocenter")]
    Autocenter,
}

impl HidFfEffect {
    pub(crate) fn is_periodic(&self) -> bool {
        matches!(
            self,
            HidFfEffect::Periodic
                | HidFfEffect::Rumble
                | HidFfEffect::Square
                | HidFfEffect::Triangle
                | HidFfEffect::Sine
                | HidFfEffect::SawUp
                | HidFfEffect::SawDown
                | HidFfEffect::Custom
        )
    }
}

impl From<evdev::FFEffectCode> for HidFfEffect {
    fn from(value: evdev::FFEffectCode) -> Self {
        match value {
            FFEffectCode::FF_RUMBLE => Self::Rumble,
            FFEffectCode::FF_PERIODIC => Self::Periodic,
            FFEffectCode::FF_CONSTANT => Self::Constant,
            FFEffectCode::FF_SPRING => Self::Spring,
            FFEffectCode::FF_FRICTION => Self::Friction,
            FFEffectCode::FF_DAMPER => Self::Damper,
            FFEffectCode::FF_INERTIA => Self::Intertia,
            FFEffectCode::FF_RAMP => Self::Ramp,
            FFEffectCode::FF_SQUARE => Self::Square,
            FFEffectCode::FF_TRIANGLE => Self::Triangle,
            FFEffectCode::FF_SINE => Self::Sine,
            FFEffectCode::FF_SAW_UP => Self::SawUp,
            FFEffectCode::FF_SAW_DOWN => Self::SawDown,
            FFEffectCode::FF_CUSTOM => Self::Custom,
            FFEffectCode::FF_GAIN => Self::Gain,
            FFEffectCode::FF_AUTOCENTER => Self::Autocenter,
            _ => unreachable!(),
        }
    }
}

impl From<HidFfEffect> for evdev::FFEffectCode {
    fn from(value: HidFfEffect) -> Self {
        match value {
            HidFfEffect::Rumble => FFEffectCode::FF_RUMBLE,
            HidFfEffect::Periodic => FFEffectCode::FF_PERIODIC,
            HidFfEffect::Constant => FFEffectCode::FF_CONSTANT,
            HidFfEffect::Spring => FFEffectCode::FF_SPRING,
            HidFfEffect::Friction => FFEffectCode::FF_FRICTION,
            HidFfEffect::Damper => FFEffectCode::FF_DAMPER,
            HidFfEffect::Intertia => FFEffectCode::FF_INERTIA,
            HidFfEffect::Ramp => FFEffectCode::FF_RAMP,
            HidFfEffect::Square => FFEffectCode::FF_SQUARE,
            HidFfEffect::Triangle => FFEffectCode::FF_TRIANGLE,
            HidFfEffect::Sine => FFEffectCode::FF_SINE,
            HidFfEffect::SawUp => FFEffectCode::FF_SAW_UP,
            HidFfEffect::SawDown => FFEffectCode::FF_SAW_DOWN,
            HidFfEffect::Custom => FFEffectCode::FF_CUSTOM,
            HidFfEffect::Gain => FFEffectCode::FF_GAIN,
            HidFfEffect::Autocenter => FFEffectCode::FF_AUTOCENTER,
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, Default, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct HidDeviceBusSpecCfg {
    #[serde(default)]
    pub(crate) r#type: HidDeviceBusType,
    #[serde(default = "default_vendor_id")]
    pub(crate) vendor_id: u16,
    #[serde(default = "default_product_id")]
    pub(crate) product_id: u16,
    #[serde(default = "default_version")]
    pub(crate) version: u16,
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, Validate, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct HIDDeviceForceFeedbackCfg {
    #[serde(skip)]
    #[garde(skip)]
    pub(crate) state_xy: Arc<[CachePadded<BaseAtomicT>; 2]>,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) effects: Vec<HidFfEffect>,
    #[serde(default = "default_max_effects")]
    #[garde(range(min = 1))]
    /// Must support at least 1 effect if enabled
    pub(crate) max_effects: usize,
    #[serde(default = "default_gain")]
    #[serde(skip)]
    #[allow(unused)]
    #[garde(range(min = 0.0))]
    pub(crate) gain: BaseNumT,
    #[serde(default)]
    #[serde(skip)]
    #[allow(unused)]
    #[garde(skip)]
    pub(crate) autocenter: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    pub(crate) fake_accepting_all_effects: bool,
}

impl Default for HIDDeviceForceFeedbackCfg {
    fn default() -> Self {
        Self {
            state_xy: Default::default(),
            enabled: true,
            effects: Default::default(),
            max_effects: 16,
            gain: 1.0,
            autocenter: false,
            fake_accepting_all_effects: false,
        }
    }
}

impl MarkedAsFromPredefinedControl for HidControlMatcherCfg {
    fn set_from_predefined_control_marker(&mut self, v: String) {
        self.from_predefined = v
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema, Default)]
pub(crate) struct HidControlMatcherCfg {
    #[traverse(skip)]
    pub(crate) r#type: MappedCtls,
    #[serde(skip)]
    #[traverse(skip)]
    #[serde(default)]
    pub(crate) from_predefined: String,
    #[traverse(skip)]
    pub(crate) range: NumInterval<BaseNumT>,
    #[traverse(skip)]
    #[serde(skip_serializing_if = "is_none_or_default")]
    pub(crate) properties: Option<AxisProperties>,
    #[serde(default)]
    #[serde(skip_serializing_if = "is_zero")]
    pub(crate) initial_value: BaseNumT,
    #[serde(skip)]
    #[traverse(skip)]
    pub(crate) id: ObjId,
    #[serde(skip)]
    #[traverse(skip)]
    pub(crate) idle_tick_enabled: IdleTickEnabledFlag,
    #[serde(skip)]
    #[traverse(skip)]
    last_known_io_value: Arc<CachePadded<BaseAtomicT>>,
    #[serde(skip)]
    #[traverse(skip)]
    current_value: Arc<CachePadded<BaseAtomicT>>,
    #[serde(skip)]
    #[traverse(skip)]
    _dst_refs_count: Arc<CachePadded<AtomicUsize>>,
}

impl WithLastKnownIOSettable<BaseNumT> for HidControlMatcherCfg {
    fn set_last_known_io(&self, v: BaseNumT) {
        self.last_known_io_value.store(v, Relaxed);
    }
}

impl WithNumericValue for HidControlMatcherCfg {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        self.current_value.load(Relaxed)
    }
}

impl WithNumericValueSettable for HidControlMatcherCfg {
    fn set_numeric_value(&self, v: Self::ValueT) {
        self.current_value.store(v, Relaxed);
    }
}

impl WithLastKnownIO<BaseNumT> for HidControlMatcherCfg {
    fn get_last_known_io(&self) -> BaseNumT {
        self.last_known_io_value.load(Relaxed) as BaseNumT
    }
}

impl From<HidControlPredefined> for HidControlMatcherCfg {
    fn from(predef: HidControlPredefined) -> Self {
        Self {
            r#type: predef.r#type,
            from_predefined: Default::default(),
            range: predef.range,
            properties: predef.properties,
            initial_value: predef.initial_value,
            id: Default::default(),
            idle_tick_enabled: Default::default(),
            last_known_io_value: Arc::new(CachePadded::new(BaseAtomicT::from(predef.initial_value))),
            _dst_refs_count: Default::default(),
            current_value: Default::default(),
        }
    }
}

impl From<String> for HidControlMatcherCfg {
    fn from(value: String) -> Self {
        Self {
            from_predefined: value,
            ..Default::default()
        }
    }
}

impl PartialEq for HidControlMatcherCfg {
    fn eq(&self, other: &Self) -> bool {
        self.r#type == other.r#type
            && self.range == other.range
            && self.properties == other.properties
            && self.initial_value == other.initial_value
    }
}

impl HidControlMatcherCfg {
    pub(crate) fn get_interval(&self) -> NumInterval<BaseNumT> {
        self.range
    }
}

impl _WithDstRefCount for HidControlMatcherCfg {
    fn _get_dst_refs_count(&self) -> usize {
        self._dst_refs_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn _set_dst_refs_count(&mut self, refs_count: usize) {
        self._dst_refs_count
            .store(refs_count, std::sync::atomic::Ordering::Relaxed)
    }
}

impl WithRuntimeId for HidControlMatcherCfg {
    fn get_id(&self) -> ObjId {
        self.id
    }

    fn assign_new_id(&mut self) {
        self.id = Default::default()
    }
}
