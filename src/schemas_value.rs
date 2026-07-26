// ----------------------------------
// Value types hier:
// ----------------------------------
// 1. Src or Dst
// 2. Static|(Src) or Dynamic(Src or Dst) or Void|(Dst)
// 3. VarRef(Dynamic) or DeviceControlMatcher(Dynamic)

use std::{
    // fmt::Display,
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize},
    },
};

use crate::num_interval::UNIT_INTERVAL;
use crate::num_interval::ZERO_INTERVAL;
use crate::relativity::Relativity;
use crossbeam_utils::CachePadded;
use deserialize_untagged_verbose_error::DeserializeUntaggedVerboseError;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::IntoDeserializer};
use traversable::{Traversable, TraversableMut};

use crate::{
    base_num::{BaseAtomicT, BaseNumT},
    num_interval::{NumInterval, NumIntervalValue},
    schemas_common::{ObjId, WithRuntimeId},
    schemas_control_matcher::ControlMatchers,
    schemas_transform::AutoOrManual,
};

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub(crate) struct DescriptionCfg(pub(crate) String);

pub(crate) trait WithDescriptionMut {
    fn description_mut(&mut self) -> Option<&mut DescriptionCfg>;
}

impl Deref for DescriptionCfg {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DescriptionCfg {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub(crate) trait _WithDstRefCount {
    fn _get_dst_refs_count(&self) -> usize;
    fn _set_dst_refs_count(&mut self, refs_count: usize);
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub(crate) struct MappedValue<ValueT: NumIntervalValue> {
    pub(crate) value: ValueT,
    pub(crate) interval: NumInterval<ValueT>,
    pub(crate) relativity: Relativity,
}

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub(crate) struct InputValueMetadata<ValueT: NumIntervalValue> {
    #[serde(rename = "in_range")]
    pub(crate) interval: NumInterval<ValueT>,
    #[serde(rename = "in_relativity")]
    pub(crate) relativity: Relativity,
}

impl<ValueT: NumIntervalValue> WithNumericValue for MappedValue<ValueT> {
    fn get_numeric_value(&self) -> ValueT {
        self.value
    }

    type ValueT = ValueT;
}

impl<ValueT: NumIntervalValue> WithRelativityRef for MappedValue<ValueT> {
    fn relativity_ref(&self) -> &Relativity {
        &self.relativity
    }
}

impl<ValueT: NumIntervalValue> _WithRelativityMut for MappedValue<ValueT> {
    fn relativity_mut(&mut self) -> &mut Relativity {
        &mut self.relativity
    }
}

impl<ValueT: NumIntervalValue> WithNumInterval for MappedValue<ValueT> {
    type ValueT = ValueT;
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        self.interval
    }
}

/// The point of this trait vs WithNumericValue trait is to give access to memorized
/// last input/output value(s) which is different from giving access to the current one.
/// The difference takes place for relative values, where current (in-the-moment) value may be 0,
/// whereas last memorized input or output may be != 0. In other cases both traits if implemented
/// may return the same value.
pub(crate) trait WithLastKnownIO<T> {
    fn get_last_known_io(&self) -> T;
}

pub(crate) trait WithLastKnownIOSettable<T> {
    fn set_last_known_io(&self, v: T);
}

pub(crate) trait WithRelativity {
    fn get_relativity(&self) -> Relativity;
}

pub(crate) trait WithRelativityRef {
    fn relativity_ref(&self) -> &Relativity;
}

impl<T: WithRelativityRef> WithRelativity for T {
    fn get_relativity(&self) -> Relativity {
        *(self.relativity_ref())
    }
}

pub(crate) trait _WithRelativitySettable {
    fn with_relativity(&mut self, relativity: Relativity) -> &mut Self;
    fn set_relativity(&mut self, relativity: Relativity);
}

impl<T: _WithRelativityMut> _WithRelativitySettable for T {
    fn with_relativity(&mut self, relativity: Relativity) -> &mut Self {
        self.set_relativity(relativity);
        self
    }
    fn set_relativity(&mut self, relativity: Relativity) {
        *self.relativity_mut() = relativity
    }
}

pub(crate) trait _WithRelativityMut {
    fn relativity_mut(&mut self) -> &mut Relativity;
}

pub(crate) trait WithNumericValue {
    type ValueT;
    fn get_numeric_value(&self) -> Self::ValueT;
}

pub(crate) trait WithNumericValueSettable {
    type ValueT;
    fn set_numeric_value(&self, v: Self::ValueT);
}

pub(crate) trait WithNumInterval {
    type ValueT: NumIntervalValue;
    fn get_interval(&self) -> NumInterval<Self::ValueT>;
}

pub(crate) trait WithNumIntervalMut {
    type ValueT: NumIntervalValue;
    fn interval_mut(&mut self) -> &mut NumInterval<Self::ValueT>;
}

pub mod variable_value_serde {
    use super::*;

    pub fn serialize<S>(value: &AutoOrManual<Arc<CachePadded<BaseAtomicT>>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[cfg(feature = "base_num_f64")]
        return serializer.serialize_f64(value.load(std::sync::atomic::Ordering::Relaxed));
        #[cfg(not(feature = "base_num_f64"))]
        return serializer.serialize_f32(value.load(std::sync::atomic::Ordering::Relaxed));
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AutoOrManual<Arc<CachePadded<BaseAtomicT>>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[cfg(feature = "base_num_f64")]
        let value = f64::deserialize(deserializer)?;
        #[cfg(not(feature = "base_num_f64"))]
        let value = f32::deserialize(deserializer)?;
        Ok(AutoOrManual::Manual(Arc::new(CachePadded::new(BaseAtomicT::new(
            value,
        )))))
    }
}

#[derive(JsonSchema, Debug, Clone, Deserialize, Serialize, TraversableMut, Traversable, Default)]
pub(crate) struct VariableState {
    #[traverse(skip)]
    #[serde(alias = "range")]
    #[serde(rename = "range")]
    pub(crate) interval: NumInterval<BaseNumT>,
    #[traverse(skip)]
    #[serde(serialize_with = "variable_value_serde::serialize")]
    #[serde(deserialize_with = "variable_value_serde::deserialize")]
    #[serde(skip_serializing_if = "AutoOrManual::is_auto")]
    #[schemars(skip)] // TODO: implement schema!
    #[serde(default)]
    pub(crate) value: AutoOrManual<Arc<CachePadded<BaseAtomicT>>>,
    // NB: Currently variables are Abs-only.
    // NB: Supporting Rel semantic will require adding reactive mappings run on Rel variables updates.
    // #[traverse(skip)]
    // pub(crate) relativity: Relativity,
    #[serde(skip)]
    #[traverse(skip)]
    pub(crate) id: ObjId,
    #[serde(skip)]
    #[traverse(skip)]
    _dst_refs_count: Arc<CachePadded<AtomicUsize>>,
}

impl WithNumIntervalMut for VariableState {
    type ValueT = BaseNumT;
    fn interval_mut(&mut self) -> &mut NumInterval<Self::ValueT> {
        &mut self.interval
    }
}

impl PartialEq for VariableState {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl VariableState {
    pub(crate) fn new(interval: NumInterval<BaseNumT> /*, relativity: Relativity */) -> Self {
        Self {
            interval,
            value: Default::default(),
            // relativity,
            id: Default::default(),
            _dst_refs_count: Default::default(),
        }
    }
}

impl WithRuntimeId for VariableState {
    fn get_id(&self) -> ObjId {
        self.id
    }

    fn assign_new_id(&mut self) {
        self.id = Default::default()
    }
}

// impl Display for VariableState {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&self.to_string())
//     }
// }

impl WithNumericValue for VariableState {
    fn get_numeric_value(&self) -> BaseNumT {
        self.value.load(std::sync::atomic::Ordering::Relaxed) as BaseNumT
    }

    type ValueT = BaseNumT;
}

impl WithRelativity for VariableState {
    fn get_relativity(&self) -> Relativity {
        Relativity::Abs
    }
}

impl WithNumInterval for VariableState {
    type ValueT = BaseNumT;
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        self.interval
    }
}

#[derive(JsonSchema, Debug, Clone, Deserialize, Serialize, PartialEq, TraversableMut, Traversable)]
pub(crate) struct VariableRef {
    #[traverse(skip)]
    #[serde(alias = "var")]
    #[serde(rename = "var")]
    pub(crate) variable_key: String,
    #[serde(skip)]
    #[serde(default = "dummy_variable_rt")]
    pub(crate) variable: VariableState,
}

pub(crate) fn dummy_variable_rt() -> VariableState {
    VariableState::new(ZERO_INTERVAL /*, Relativity::Abs*/)
}

impl Eq for VariableRef {}

impl Ord for VariableRef {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other)
            .expect("Variable refs can always be compared based on variable name.")
    }
}

#[allow(clippy::non_canonical_partial_ord_impl)]
impl PartialOrd for VariableRef {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.variable_key.partial_cmp(&other.variable_key) {
            Some(core::cmp::Ordering::Equal) => Some(std::cmp::Ordering::Equal),
            ord => ord,
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, TraversableMut, Traversable)]
pub(crate) struct DeviceControlMatcherRef {
    #[serde(rename = "dev")]
    #[serde(alias = "device")]
    #[serde(alias = "device-matcher")]
    #[traverse(skip)]
    pub(crate) device_matcher_key: String,
    #[serde(rename = "ctl")]
    #[serde(alias = "control")]
    #[serde(alias = "control-matcher")]
    #[traverse(skip)]
    pub(crate) control_key: String,
    #[serde(skip)]
    #[serde(default = "dummy_control_matcher_rt")]
    pub(crate) control_matcher: ControlMatchers,
}

impl Eq for DeviceControlMatcherRef {}

impl Ord for DeviceControlMatcherRef {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Less)
    }
}

#[allow(clippy::non_canonical_partial_ord_impl)]
impl PartialOrd for DeviceControlMatcherRef {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.device_matcher_key.partial_cmp(&other.device_matcher_key) {
            Some(core::cmp::Ordering::Equal) => match self.control_key.partial_cmp(&other.control_key) {
                Some(core::cmp::Ordering::Equal) => Some(core::cmp::Ordering::Equal),
                ord => ord,
            },
            ord => ord,
        }
    }
}

pub fn dummy_control_matcher_rt() -> ControlMatchers {
    ControlMatchers::Hid(Default::default())
}

#[derive(PartialOrd, Ord, Eq, JsonSchema, Debug, Clone, Serialize, PartialEq, TraversableMut, Traversable)]
#[serde(untagged)]
pub(crate) enum DynValueRefs {
    DeviceControlMatcher(DeviceControlMatcherRef),
    Variable(VariableRef),
}

impl WithRelativity for DynValueRefs {
    fn get_relativity(&self) -> Relativity {
        match self {
            DynValueRefs::DeviceControlMatcher(d) => d.control_matcher.get_relativity(),
            DynValueRefs::Variable(v) => v.variable.get_relativity(),
        }
    }
}

impl<'de> Deserialize<'de> for DynValueRefs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        use serde::de::IntoDeserializer;
        let raw: serde_value::Value = Deserialize::deserialize(deserializer)?;
        match DeviceControlMatcherRef::deserialize(raw.clone().into_deserializer()) {
            Ok(matcher) => Ok(DynValueRefs::DeviceControlMatcher(matcher)),
            Err(err_matcher) => match VariableRef::deserialize(raw.clone().into_deserializer()) {
                Ok(variable) => Ok(DynValueRefs::Variable(variable)),
                Err(err_variable) => Err(D::Error::custom(format!(
                    "Dynamic value ref config error.\n\
                        Expected either a DeviceControlMatcher or a Variable reference.\n\n\
                        Received input: {:?}\n\n\
                        DeviceControlMatcher error: {}\n\
                        Variable error: {}",
                    raw, err_matcher, err_variable
                ))),
            },
        }
    }
}

#[allow(clippy::to_string_trait_impl)] // TODO: impl. Display
impl ToString for &DynValueRefs {
    fn to_string(&self) -> String {
        match self {
            DynValueRefs::DeviceControlMatcher(d) => d.device_matcher_key.to_string() + "/" + &d.control_key,
            DynValueRefs::Variable(v) => v.variable_key.to_string(),
        }
    }
}

// -------
impl WithNumInterval for DynValueRefs {
    type ValueT = BaseNumT;

    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        match self {
            DynValueRefs::DeviceControlMatcher(d) => d.control_matcher.get_interval(),
            DynValueRefs::Variable(v) => v.variable.get_interval(),
        }
    }
}

impl WithRelativity for ValueSrcs {
    fn get_relativity(&self) -> Relativity {
        match self {
            Self::Static(..) => Relativity::Abs,
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => d.control_matcher.get_relativity(),
                DynValueRefs::Variable(v) => v.variable.get_relativity(),
            },
        }
    }
}

impl WithNumInterval for ValueSrcs {
    type ValueT = BaseNumT;
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        match self {
            Self::Static(s) => s.interval,
            Self::Dynamic(dvr) => match dvr {
                DynValueRefs::DeviceControlMatcher(d) => d.control_matcher.get_interval(),
                DynValueRefs::Variable(v) => v.variable.interval,
            },
        }
    }
}

impl DynValueRefs {
    pub(crate) fn _is_var(&self) -> bool {
        if let Self::Variable { .. } = *self {
            return true;
        }
        false
    }

    pub(crate) fn _is_device_control_matcher(&self) -> bool {
        if let DynValueRefs::DeviceControlMatcher { .. } = *self {
            return true;
        }
        false
    }
}

fn default_unit_interval() -> NumInterval<BaseNumT> {
    UNIT_INTERVAL
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
enum StaticValueRtHelper {
    Simple(BaseNumT),
    Full {
        value: BaseNumT,
        #[serde(default = "default_unit_interval")]
        #[serde(rename = "range")]
        #[serde(alias = "interval")]
        interval: NumInterval<BaseNumT>,
    },
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(from = "StaticValueRtHelper", into = "StaticValueRtHelper")]
#[serde(deny_unknown_fields)]
pub(crate) struct StaticValueCfg {
    pub(crate) value: BaseNumT,
    #[serde(default = "default_unit_interval")]
    #[serde(rename = "range")]
    #[serde(alias = "interval")]
    pub(crate) interval: NumInterval<BaseNumT>,
}

impl From<StaticValueRtHelper> for StaticValueCfg {
    fn from(helper: StaticValueRtHelper) -> Self {
        match helper {
            StaticValueRtHelper::Simple(v) => Self {
                value: v,
                interval: default_unit_interval(),
            },
            StaticValueRtHelper::Full { value, interval } => Self { value, interval },
        }
    }
}

impl From<StaticValueCfg> for StaticValueRtHelper {
    fn from(orig: StaticValueCfg) -> Self {
        if orig.interval == default_unit_interval() {
            StaticValueRtHelper::Simple(orig.value)
        } else {
            StaticValueRtHelper::Full {
                value: orig.value,
                interval: orig.interval,
            }
        }
    }
}

impl std::fmt::Display for StaticValueCfg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{} {}", self.value, self.interval))
    }
}

// -------------------------------------------------
#[derive(
    JsonSchema, Debug, Clone, Serialize, DeserializeUntaggedVerboseError, PartialEq, TraversableMut, Traversable,
)]
#[serde(untagged)]
pub(crate) enum ValueSrcs {
    // Rand { distr: ... , interval: ... },
    #[traverse(skip)]
    Static(StaticValueCfg),
    Dynamic(DynValueRefs),
}

impl WithNumericValue for ValueSrcs {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        match self {
            ValueSrcs::Static(s) => s.value,
            ValueSrcs::Dynamic(d) => d.get_numeric_value(),
        }
    }
}

impl WithNumericValue for DynValueRefs {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        match self {
            DynValueRefs::DeviceControlMatcher(d) => d.get_numeric_value(),
            DynValueRefs::Variable(v) => v.variable.get_numeric_value(),
        }
    }
}

impl WithNumericValue for DeviceControlMatcherRef {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        self.control_matcher.get_numeric_value()
    }
}

impl WithLastKnownIO<BaseNumT> for ValueSrcs {
    fn get_last_known_io(&self) -> BaseNumT {
        match self {
            ValueSrcs::Static(v) => v.value,
            ValueSrcs::Dynamic(d) => d.get_last_known_io(),
        }
    }
}

impl WithLastKnownIO<BaseNumT> for DynValueRefs {
    fn get_last_known_io(&self) -> BaseNumT {
        match self {
            DynValueRefs::DeviceControlMatcher(cm) => cm.get_last_known_io(),
            DynValueRefs::Variable(v) => v.variable.value.load(std::sync::atomic::Ordering::Relaxed) as BaseNumT,
        }
    }
}

impl WithLastKnownIO<BaseNumT> for DeviceControlMatcherRef {
    fn get_last_known_io(&self) -> BaseNumT {
        self.control_matcher.get_last_known_io()
    }
}

pub(crate) fn serialize_value_src_rt_ignore_interval<S>(v: &ValueSrcs, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match v {
        #[cfg(not(feature = "base_num_f64"))]
        ValueSrcs::Static(v) => serializer.serialize_f32(v.value),
        #[cfg(feature = "base_num_f64")]
        ValueSrcs::Static(v) => serializer.serialize_f64(v.value),
        ValueSrcs::Dynamic(v) => v.serialize(serializer),
    }
}

impl Default for ValueSrcs {
    fn default() -> Self {
        Self::Static(StaticValueCfg {
            value: Default::default(),
            interval: Default::default(),
        })
    }
}

// -------------------------------------------------

impl std::fmt::Display for ValueSrcs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match &self {
            Self::Static(v) => format!("Static value: {v}"),
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => {
                    format!("Src: {}.{}", d.device_matcher_key, d.control_key)
                }
                DynValueRefs::Variable(v) => format!("Src var: {}", v.variable_key),
            },
        };
        f.write_str(&s)
    }
}

impl std::fmt::Display for ValueDsts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match &self {
            Self::Void => "Dst: void".into(),
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => {
                    format!("Dst: {}.{}", d.device_matcher_key, d.control_key)
                }
                DynValueRefs::Variable(v) => format!("Dst var: {}", v.variable_key),
            },
        };
        f.write_str(&s)
    }
}

impl ValueSrcs {
    pub(crate) fn _get_device_matcher_key(&self) -> Option<&String> {
        match &self {
            ValueSrcs::Static(_) => None,
            ValueSrcs::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => Some(&d.device_matcher_key),
                DynValueRefs::Variable(_) => None,
            },
        }
    }

    pub(crate) fn _get_control_key(&self) -> Option<&String> {
        match &self {
            ValueSrcs::Static(_) => None,
            ValueSrcs::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => Some(&d.control_key),
                DynValueRefs::Variable(_) => None,
            },
        }
    }

    pub(crate) fn _get_idle_tick_enabled_flag(&self) -> Option<&AtomicBool> {
        match self {
            Self::Static(..) => None,
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => Some(d.control_matcher.get_idle_tick_enabled_flag()),
                DynValueRefs::Variable(_) => None,
            },
        }
    }

    pub(crate) fn _get_control_matcher(&self) -> Option<&ControlMatchers> {
        match self {
            Self::Static { .. } => None,
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => Some(&d.control_matcher),
                DynValueRefs::Variable(_) => None,
            },
        }
    }

    pub(crate) fn get_id(&self) -> Option<ObjId> {
        match self {
            Self::Static { .. } => None,
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => Some(d.control_matcher.get_id()),
                DynValueRefs::Variable(v) => Some(v.variable.get_id()),
            },
        }
    }

    pub(crate) fn _is_static(&self) -> bool {
        if let Self::Static(..) = *self {
            return true;
        }
        false
    }

    pub(crate) fn _is_dynamic(&self) -> bool {
        if let Self::Dynamic(..) = *self {
            return true;
        }
        false
    }

    pub(crate) fn _is_device_control_matcher(&self) -> bool {
        if let Self::Dynamic(v) = self {
            return v._is_device_control_matcher();
        }
        false
    }
}

// ----------------------------------------------------------

#[derive(JsonSchema, Debug, Clone, Serialize, PartialEq, Default, TraversableMut, Traversable)]
#[serde(untagged)]
pub(crate) enum ValueDsts {
    Dynamic(DynValueRefs),
    #[default]
    Void,
}

impl<'de> Deserialize<'de> for ValueDsts {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let raw: serde_value::Value = Deserialize::deserialize(deserializer)?;
        match DynValueRefs::deserialize(raw.clone().into_deserializer()) {
            Ok(dynamic) => Ok(ValueDsts::Dynamic(dynamic)),
            Err(dynamic_err) => {
                let is_void = match <()>::deserialize(raw.clone().into_deserializer()) {
                    Ok(()) => true,
                    Err(_) => {
                        if let serde_value::Value::Map(ref m) = raw {
                            m.is_empty()
                        } else {
                            false
                        }
                    }
                };
                if is_void {
                    return Ok(ValueDsts::Void);
                }
                Err(D::Error::custom(format!("  {:?}  {}", raw, dynamic_err)))
            }
        }
    }
}

impl ValueDsts {
    pub(crate) fn get_idle_tick_enabled_flag(&self) -> Option<&AtomicBool> {
        match self {
            ValueDsts::Void => None,
            ValueDsts::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => Some(d.control_matcher.get_idle_tick_enabled_flag()),
                DynValueRefs::Variable(_) => None,
            },
        }
    }

    pub(crate) fn get_id(&self) -> Option<ObjId> {
        match self {
            ValueDsts::Void => None,
            ValueDsts::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => Some(d.control_matcher.get_id()),
                DynValueRefs::Variable(v) => Some(v.variable.get_id()),
            },
        }
    }

    pub(crate) fn get_interval(&self) -> NumInterval<BaseNumT> {
        match self {
            Self::Void => ZERO_INTERVAL,
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => d.control_matcher.get_interval(),
                DynValueRefs::Variable(v) => v.variable.interval,
            },
        }
    }

    pub(crate) fn _get_relativity(&self) -> Relativity {
        match self {
            Self::Void => Relativity::Abs,
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => d.control_matcher.get_relativity(),
                DynValueRefs::Variable(_) => Relativity::Abs, // TODO: Variables support: always Abs or not ?
            },
        }
    }

    pub(crate) fn _is_void(&self) -> bool {
        Self::Void == *self
    }
    pub(crate) fn _is_dynamic(&self) -> bool {
        if let Self::Dynamic(..) = *self {
            return true;
        }
        false
    }
    pub(crate) fn _is_device_control_matcher(&self) -> bool {
        if let Self::Dynamic(v) = self {
            return v._is_device_control_matcher();
        }
        false
    }
}

impl std::hash::Hash for DynValueRefs {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            Self::DeviceControlMatcher(d) => {
                d.device_matcher_key.hash(state);
                d.control_key.hash(state);
            }
            Self::Variable(v) => v.variable_key.hash(state),
        }
    }
}

impl std::hash::Hash for ValueSrcs {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match &self {
            Self::Static(_) => {
                (self as *const _ as usize).hash(state);
            }
            Self::Dynamic(dynamic_value_ref_rt) => dynamic_value_ref_rt.hash(state),
        }
    }
}

impl std::hash::Hash for ValueDsts {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match &self {
            Self::Void => std::mem::discriminant(self).hash(state),
            Self::Dynamic(dynamic_value_ref_rt) => dynamic_value_ref_rt.hash(state),
        }
    }
}

// #[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) enum ValuesRt {
    Src(ValueSrcs),
    Dst(ValueDsts),
}

impl _WithDstRefCount for VariableState {
    fn _get_dst_refs_count(&self) -> usize {
        self._dst_refs_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn _set_dst_refs_count(&mut self, refs_count: usize) {
        self._dst_refs_count
            .store(refs_count, std::sync::atomic::Ordering::Relaxed)
    }
}
