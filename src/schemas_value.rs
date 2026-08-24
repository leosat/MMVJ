// ----------------------------------
// Value types hier:
// ----------------------------------
// 1. Src or Dst
// 2. Static|(Src) or Dynamic(Src or Dst) or Void|(Dst)
// 3. VarRef(Dynamic) or DeviceControlMatcher(Dynamic)

use std::{
    cell::Cell,
    fmt::Display,
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
    },
};

use crate::num_interval::ZERO_INTERVAL;
use crate::relativity::Relativity;
use crate::{
    num_interval::UNIT_INTERVAL,
    schemas_value_port::{WithNumIntervalSanitizerStatic, WithNumericValueSanitizerStatic},
};
use crossbeam_utils::CachePadded;
use deserialize_untagged_verbose_error::DeserializeUntaggedVerboseError;
use enum_dispatch::enum_dispatch;
use garde::{Validate, rules::range::Bounds};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use traversable::{Traversable, TraversableMut};

use crate::{
    base_num::{BaseAtomicT, BaseNumT},
    num_interval::{NumInterval, NumIntervalValue},
    schemas_common::{ObjId, WithRuntimeId},
    schemas_control_matcher::ControlMatchers,
};

const XRC_SINK_VALUE_INTERVAL: NumInterval<BaseNumT> = NumInterval {
    from: -8675309.0,
    to: 8675309.0,
};

// --------------------------------------------------------

pub(crate) trait ValueIface:
    Clone
    + From<ValueTargets>
    + std::fmt::Debug
    + Default
    + PartialEq
    + PartialOrd
    + WithDeviceControlMatcherRef
    + WithNumInterval
    + WithNumIntervalSettable
    + WithNumericValue
    + WithNumericValueSettable
    + WithNumericValueSanitizerStatic
    + Serialize
    + for<'de> Deserialize<'de>
    + JsonSchema
{
    fn value_identity(&self) -> String;
    fn value_is_static(&self) -> bool;
}

pub(crate) trait _WithDstRefCount {
    fn _get_dst_refs_count(&self) -> usize;
    fn _set_dst_refs_count(&mut self, refs_count: usize);
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub(crate) struct TfmValue<ValueT: NumIntervalValue> {
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

impl<ValueT: NumIntervalValue> WithNumericValue for TfmValue<ValueT> {
    fn get_numeric_value(&self) -> ValueT {
        self.value
    }

    type ValueT = ValueT;
}

impl<ValueT: NumIntervalValue> WithRelativityRef for TfmValue<ValueT> {
    fn relativity_ref(&self) -> &Relativity {
        &self.relativity
    }
}

impl<ValueT: NumIntervalValue> _WithRelativityMut for TfmValue<ValueT> {
    fn relativity_mut(&mut self) -> &mut Relativity {
        &mut self.relativity
    }
}

impl<ValueT: NumIntervalValue> WithNumInterval for TfmValue<ValueT> {
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        self.interval
    }
}

/// The point of this trait vs WithNumericValue trait is to give access to memorized
/// last input/output value(s) which is different from giving access to the current one.
/// The difference takes place for relative values, where current (in-the-moment) value may be 0,
/// whereas last memorized input or output may be != 0. In other cases both traits if implemented
/// may return the same value.
#[enum_dispatch]
pub(crate) trait WithLastKnownIO<T> {
    fn get_last_known_io(&self) -> T;
}

#[enum_dispatch]
pub(crate) trait WithLastKnownIOSettable<T> {
    fn set_last_known_io(&self, value: T);
}

#[enum_dispatch]
pub(crate) trait WithRelativity {
    fn get_relativity(&self) -> Relativity;
}

#[enum_dispatch]
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

#[enum_dispatch]
pub(crate) trait WithNumericValue {
    type ValueT: NumIntervalValue;
    fn get_numeric_value(&self) -> Self::ValueT;
}

// #[allow(unused)]
// pub(crate) trait WithNumericValueClamped: WithNumericValue /*+ WithNumInterval*/ {
//     fn get_numeric_value_clamped(&self) -> <Self as WithNumericValue>::ValueT;
// }

// #[allow(unused)]
// pub(crate) trait WithNumericValueClampedPredicated: WithNumericValue /*+ WithNumInterval*/ {
//     type PredicationParamsT;
//     fn get_numeric_value_clamped_predicated(
//         &self,
//         params: Self::PredicationParamsT,
//     ) -> <Self as WithNumericValue>::ValueT;
// }

/// This trait sets a numeric value within configuration tree/transformation state cache.
/// NB: It does not actually write to any devices!
#[enum_dispatch]
pub(crate) trait WithNumericValueSettable: WithNumericValue {
    fn set_numeric_value(&self, value: Self::ValueT);
}

#[enum_dispatch]
pub(crate) trait WithNumInterval: WithNumericValue {
    fn get_interval(&self) -> NumInterval<Self::ValueT>;
}

#[enum_dispatch]
pub(crate) trait WithNumIntervalMut: WithNumericValue {
    fn interval_mut(&mut self) -> &mut NumInterval<Self::ValueT>;
}

//--------------------------------------------------
#[enum_dispatch]
pub(crate) trait WithNumIntervalSettable: WithNumInterval {
    fn set_interval(&mut self, interval: NumInterval<Self::ValueT>);
}

pub(crate) mod variable_value_serde {
    use super::*;

    pub(crate) fn serialize<S>(
        value: &AutoOrManual<Arc<CachePadded<BaseAtomicT>>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[cfg(feature = "base_num_f64")]
        return serializer.serialize_f64(value.load(std::sync::atomic::Ordering::Relaxed));
        #[cfg(not(feature = "base_num_f64"))]
        return serializer.serialize_f32(value.load(std::sync::atomic::Ordering::Relaxed));
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<AutoOrManual<Arc<CachePadded<BaseAtomicT>>>, D::Error>
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
    #[serde(default = "dummy_variable")]
    pub(crate) variable: VariableState,
}

pub(crate) fn dummy_variable() -> VariableState {
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

// -------------------------------------------------
// #[allow(unused)]
// pub(crate) type DeviceControlMatcherKey<'k> = (&'k str, &'k str);
// pub(crate) trait WithDeviceControlMatcherKey {
//     fn _get_device_control_matcher_key(&self) -> Option<DeviceControlMatcherKey<'_>>;
// }

pub(crate) trait WithDeviceControlMatcherRef {
    fn get_device_control_matcher_ref(&self) -> Option<&DeviceControlMatcherRef>;
}
// ----------------------------------------------------

impl WithDeviceControlMatcherRef for ValueSrcs {
    fn get_device_control_matcher_ref(&self) -> Option<&crate::schemas_value::DeviceControlMatcherRef> {
        if let Self::Dynamic(DynValueRefs::DeviceControlMatcher(d)) = self {
            Some(d)
        } else {
            None
        }
    }
}

impl WithDeviceControlMatcherRef for ValueDsts {
    fn get_device_control_matcher_ref(&self) -> Option<&crate::schemas_value::DeviceControlMatcherRef> {
        if let Self::Dynamic(DynValueRefs::DeviceControlMatcher(d)) = self {
            Some(d)
        } else {
            None
        }
    }
}

// -----------------------------------------

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
    pub(crate) control_matcher_key: String,
    #[serde(skip)]
    #[serde(default = "dummy_control_matcher")]
    pub(crate) control_matcher: ControlMatchers,
}

impl WithNumInterval for DeviceControlMatcherRef {
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        self.control_matcher.get_interval()
    }
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
            Some(core::cmp::Ordering::Equal) => {
                match self.control_matcher_key.partial_cmp(&other.control_matcher_key) {
                    Some(core::cmp::Ordering::Equal) => Some(core::cmp::Ordering::Equal),
                    ord => ord,
                }
            }
            ord => ord,
        }
    }
}

pub(crate) fn dummy_control_matcher() -> ControlMatchers {
    ControlMatchers::Hid(Default::default())
}

#[derive(PartialOrd, Ord, Eq, JsonSchema, Debug, Clone, Serialize, PartialEq, TraversableMut, Traversable)]
#[serde(untagged)]
pub(crate) enum DynValueRefs {
    DeviceControlMatcher(DeviceControlMatcherRef),
    Variable(VariableRef),
}

impl WithDeviceControlMatcherRef for DynValueRefs {
    fn get_device_control_matcher_ref(&self) -> Option<&DeviceControlMatcherRef> {
        if let DynValueRefs::DeviceControlMatcher(d) = self {
            Some(d)
        } else {
            None
        }
    }
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
            DynValueRefs::DeviceControlMatcher(d) => d.device_matcher_key.to_string() + "/" + &d.control_matcher_key,
            DynValueRefs::Variable(v) => v.variable_key.to_string(),
        }
    }
}

// -------
impl WithNumInterval for DynValueRefs {
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
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        match self {
            Self::Static(s) => *s.interval,
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

fn default_src_value_interval() -> NumInterval<BaseNumT> {
    UNIT_INTERVAL
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
enum StaticValueCfgSerdeHelper {
    ValueOnly(BaseNumT),
    Full {
        value: BaseNumT,
        #[serde(default = "default_src_value_interval")]
        #[serde(rename = "range")]
        #[serde(alias = "interval")]
        interval: NumInterval<BaseNumT>,
    },
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Default, Validate)]
#[serde(from = "StaticValueCfgSerdeHelper", into = "StaticValueCfgSerdeHelper")]
#[serde(deny_unknown_fields)]
pub(crate) struct StaticValueCfg {
    #[garde(skip)]
    pub(crate) value: Cell<BaseNumT>,
    #[serde(default = "default_unit_interval")]
    #[serde(rename = "range")]
    #[serde(alias = "interval")]
    #[garde(skip)]
    pub(crate) interval: AutoOrManual<NumInterval<BaseNumT>>,
}

impl WithNumIntervalSettable for StaticValueCfg {
    fn set_interval(&mut self, interval: NumInterval<Self::ValueT>) {
        self.interval = AutoOrManual::Manual(interval);
    }
}

impl From<StaticValueCfgSerdeHelper> for StaticValueCfg {
    fn from(helper: StaticValueCfgSerdeHelper) -> Self {
        match helper {
            StaticValueCfgSerdeHelper::ValueOnly(value) => Self {
                value: value.into(),
                interval: AutoOrManual::Auto(default_src_value_interval()),
            },
            StaticValueCfgSerdeHelper::Full { value, interval } => Self {
                value: value.into(),
                interval: AutoOrManual::Manual(interval),
            },
        }
    }
}

impl From<StaticValueCfg> for StaticValueCfgSerdeHelper {
    fn from(orig: StaticValueCfg) -> Self {
        if orig.interval.is_auto() {
            StaticValueCfgSerdeHelper::ValueOnly(orig.value.get())
        } else {
            StaticValueCfgSerdeHelper::Full {
                value: orig.value.get(),
                interval: *orig.interval,
            }
        }
    }
}

impl std::fmt::Display for StaticValueCfg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{} {}", self.value.get(), *self.interval))
    }
}

// -------------------------------------------------
#[derive(
    JsonSchema,
    Debug,
    Clone,
    Serialize,
    DeserializeUntaggedVerboseError,
    PartialEq,
    TraversableMut,
    Traversable,
    Validate,
)]
#[serde(untagged)]
pub(crate) enum ValueSrcs {
    // Rand { distr: ... , interval: ... },
    #[traverse(skip)]
    Static(#[garde(skip)] StaticValueCfg),
    Dynamic(#[garde(skip)] DynValueRefs),
}

// impl WithDeviceControlMatcherKey for ValueSrcs {
//     fn _get_device_control_matcher_key(&self) -> Option<DeviceControlMatcherKey<'_>> {
//         if let Self::Dynamic(DynValueRefs::DeviceControlMatcher(d)) = self {
//             Some((&d.device_matcher_key, &d.control_matcher_key))
//         } else {
//             None
//         }
//     }
// }

impl ValueIface for ValueSrcs {
    fn value_identity(&self) -> String {
        match self {
            ValueSrcs::Static(_) => egui_phosphor::bold::PENCIL.into(),
            ValueSrcs::Dynamic(d) => format!(
                "Src({}({}))",
                if d._is_device_control_matcher() { "CTL:" } else { "VAR:" },
                d.to_string()
            ),
        }
    }

    fn value_is_static(&self) -> bool {
        self.is_static()
    }
}

impl WithNumIntervalSettable for ValueSrcs {
    fn set_interval(&mut self, interval: NumInterval<Self::ValueT>) {
        match self {
            Self::Static(s) => {
                s.set_interval(interval);
                s.set_numeric_value(s.get_interval().clamp(s.get_numeric_value()));
            }
            Self::Dynamic(_) => {
                log::error!(
                    "Setting interval on dynamic value reference is not possible: modify the definition itself."
                )
            }
        }
    }
}

impl PartialOrd for ValueSrcs {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.get_numeric_value().partial_cmp(&other.get_numeric_value())
    }
}

impl Bounds for ValueSrcs {
    type Size = BaseNumT;
    const MIN: Self::Size = BaseNumT::MIN;
    const MAX: Self::Size = BaseNumT::MAX;
    fn validate_bounds(
        &self,
        lower_bound: Self::Size,
        upper_bound: Self::Size,
    ) -> Result<(), garde::rules::range::OutOfBounds> {
        let value = self.get_numeric_value();
        let expected_interval = NumInterval::new(lower_bound, upper_bound);
        debug_assert!(
            self.get_interval().contains_interval(expected_interval),
            "Interval expected in garde is not contained within the interval specified for the value source"
        );
        if value < expected_interval.from() {
            Err(garde::rules::range::OutOfBounds::Lower)
        } else if value > expected_interval.to() {
            Err(garde::rules::range::OutOfBounds::Upper)
        } else {
            Ok(())
        }
    }
}

pub(crate) const fn make_static_value_src(value: BaseNumT, interval: NumInterval<BaseNumT>) -> ValueSrcs {
    ValueSrcs::Static(StaticValueCfg {
        value: std::cell::Cell::new(value),
        interval: AutoOrManual::Auto(interval),
    })
}

impl From<BaseNumT> for ValueSrcs {
    fn from(value: BaseNumT) -> Self {
        make_static_value_src(value, UNIT_INTERVAL)
    }
}

impl WithNumericValue for StaticValueCfg {
    type ValueT = BaseNumT;
    fn get_numeric_value(&self) -> Self::ValueT {
        self.value.get()
    }
}

impl WithNumInterval for StaticValueCfg {
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        *self.interval
    }
}

// #[allow(unused)]
// pub(crate) enum ClampPred {
//     IfStatic,
//     IfDynamic,
// }

// impl WithNumericValueClampedPredicated for ValueSrcs {
//     type PredicationParamsT = ClampPred;

//     fn get_numeric_value_clamped_predicated(
//         &self,
//         params: Self::PredicationParamsT,
//     ) -> <Self as WithNumericValue>::ValueT {
//         match self {
//             Self::Static(s) => match params {
//                 ClampPred::IfStatic => s.get_interval().clamp(s.get_numeric_value()),
//                 ClampPred::IfDynamic => s.get_numeric_value(),
//             },
//             Self::Dynamic(d) => match params {
//                 ClampPred::IfDynamic => d.get_interval().clamp(d.get_numeric_value()),
//                 ClampPred::IfStatic => d.get_numeric_value(),
//             },
//         }
//     }
// }

impl WithNumericValueSettable for ValueSrcs {
    fn set_numeric_value(&self, value: Self::ValueT) {
        match self {
            Self::Static(s) => s.value.set(value),
            Self::Dynamic(d) => d.set_numeric_value(value),
        }
    }
}

impl WithNumericValueSettable for StaticValueCfg {
    fn set_numeric_value(&self, value: Self::ValueT) {
        self.value.set(value)
    }
}

impl WithNumericValueSettable for DynValueRefs {
    fn set_numeric_value(&self, value: Self::ValueT) {
        match self {
            Self::DeviceControlMatcher(d) => d.set_numeric_value(value),
            Self::Variable(v) => v.set_numeric_value(value),
        }
    }
}

impl WithNumericValueSettable for VariableRef {
    fn set_numeric_value(&self, value: Self::ValueT) {
        self.variable.set_numeric_value(value);
    }
}

impl WithNumericValue for VariableRef {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        self.variable.get_numeric_value()
    }
}

impl WithNumericValueSettable for VariableState {
    fn set_numeric_value(&self, value: Self::ValueT) {
        self.value.store(value, std::sync::atomic::Ordering::Relaxed);
    }
}

impl WithNumericValueSettable for DeviceControlMatcherRef {
    fn set_numeric_value(&self, value: Self::ValueT) {
        self.control_matcher.set_numeric_value(value);
    }
}

impl WithNumericValue for ValueSrcs {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        match self {
            ValueSrcs::Static(s) => s.value.get(),
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
            ValueSrcs::Static(v) => v.value.get(),
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

pub(crate) fn serialize_value_src_rt_ignore_interval<S>(srcs: &ValueSrcs, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match srcs {
        #[cfg(not(feature = "base_num_f64"))]
        ValueSrcs::Static(s) => serializer.serialize_f32(s.get_numeric_value()),
        #[cfg(feature = "base_num_f64")]
        ValueSrcs::Static(s) => serializer.serialize_f64(s.get_numeric_value()),
        ValueSrcs::Dynamic(d) => d.serialize(serializer),
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
                    format!("Src: {}.{}", d.device_matcher_key, d.control_matcher_key)
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
            ValueDsts::Void(..) => "Dst: void".into(),
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => {
                    format!("Dst: {}.{}", d.device_matcher_key, d.control_matcher_key)
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
                DynValueRefs::DeviceControlMatcher(d) => Some(&d.control_matcher_key),
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

    pub(crate) fn _get_id(&self) -> Option<ObjId> {
        match self {
            Self::Static { .. } => None,
            Self::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                DynValueRefs::DeviceControlMatcher(d) => Some(d.control_matcher.get_id()),
                DynValueRefs::Variable(v) => Some(v.variable.get_id()),
            },
        }
    }

    pub(crate) fn is_static(&self) -> bool {
        matches!(self, Self::Static(..))
    }

    pub(crate) fn _is_dynamic(&self) -> bool {
        matches!(self, Self::Dynamic(..))
    }

    pub(crate) fn _is_device_control_matcher(&self) -> bool {
        matches!(self, Self::Dynamic(d) if d._is_device_control_matcher() )
    }
}

// ----------------------------------------------------------

#[derive(
    JsonSchema,
    Debug,
    Clone,
    Serialize,
    DeserializeUntaggedVerboseError,
    PartialEq,
    PartialOrd,
    TraversableMut,
    Traversable,
)]
#[serde(untagged)]
pub(crate) enum ValueDsts {
    Dynamic(DynValueRefs),
    Void(Option<bool>),
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(from = "BaseNumT", into = "BaseNumT")]
pub(crate) struct XrcSink(#[schemars(with = "BaseNumT")] pub(crate) Arc<CachePadded<BaseAtomicT>>);

impl WithNumericValue for XrcSink {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        self.0.as_ref().load(Relaxed)
    }
}

impl WithNumericValueSettable for XrcSink {
    fn set_numeric_value(&self, value: Self::ValueT) {
        self.0.as_ref().store(value, Relaxed);
    }
}

impl WithNumInterval for XrcSink {
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        XRC_SINK_VALUE_INTERVAL
    }
}

impl From<XrcSink> for BaseNumT {
    fn from(value: XrcSink) -> Self {
        value.get_numeric_value()
    }
}

impl From<BaseNumT> for XrcSink {
    fn from(value: BaseNumT) -> Self {
        Self(Arc::new(CachePadded::new(BaseAtomicT::from(value))))
    }
}

impl PartialOrd for XrcSink {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.get_numeric_value().partial_cmp(&other.get_numeric_value())
    }
}

impl PartialEq for XrcSink {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Default for XrcSink {
    fn default() -> Self {
        Self(Arc::new(CachePadded::new(0.0.into())))
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd, TraversableMut, Traversable)]
#[serde(untagged)]
pub(crate) enum ValueXrcs {
    #[traverse(skip)]
    Sink(#[serde(skip)] XrcSink),
    Dynamic(DynValueRefs),
}

impl WithNumericValue for ValueXrcs {
    type ValueT = <DynValueRefs as WithNumericValue>::ValueT;

    fn get_numeric_value(&self) -> Self::ValueT {
        match self {
            Self::Dynamic(d) => d.get_numeric_value(),
            Self::Sink(v) => v.get_numeric_value(),
        }
    }
}
impl WithNumericValueSettable for ValueXrcs {
    fn set_numeric_value(&self, value: Self::ValueT) {
        match self {
            Self::Dynamic(d) => d.set_numeric_value(value),
            Self::Sink(s) => s.set_numeric_value(value),
        }
    }
}
impl WithNumInterval for ValueXrcs {
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        match self {
            Self::Dynamic(d) => d.get_interval(),
            Self::Sink(s) => s.get_interval(),
        }
    }
}
impl WithNumIntervalSettable for ValueXrcs {
    fn set_interval(&mut self, _: NumInterval<Self::ValueT>) {
        match self {
            Self::Dynamic(_) => log::error!("Can't set interval on a sink dynamic value ref."),
            Self::Sink(s) => log::error!(
                "Can't set interval other than the default one {} on a xrc sink value.",
                s.get_interval()
            ),
        }
    }
}

impl Default for ValueXrcs {
    fn default() -> Self {
        Self::Sink(Default::default())
    }
}

impl ValueIface for ValueXrcs {
    fn value_identity(&self) -> String {
        match self {
            Self::Dynamic(d) => d.to_string(),
            Self::Sink(_) => "XRC(sink value)".into(),
        }
    }

    fn value_is_static(&self) -> bool {
        false
    }
}

impl WithDeviceControlMatcherRef for ValueXrcs {
    fn get_device_control_matcher_ref(&self) -> Option<&DeviceControlMatcherRef> {
        match self {
            Self::Dynamic(d) => d.get_device_control_matcher_ref(),
            Self::Sink(_) => None,
        }
    }
}

impl WithNumericValueSanitizerStatic for ValueXrcs {
    fn sanitize_numeric_value_static(value: Self::ValueT) -> Self::ValueT {
        value
    }
}

impl Default for ValueDsts {
    fn default() -> Self {
        Self::Void(None)
    }
}

impl From<DynValueRefs> for ValueDsts {
    fn from(value: DynValueRefs) -> Self {
        ValueDsts::Dynamic(value)
    }
}

impl From<DynValueRefs> for ValueSrcs {
    fn from(value: DynValueRefs) -> Self {
        ValueSrcs::Dynamic(value)
    }
}

impl From<ValueXrcs> for ValueDsts {
    fn from(value: ValueXrcs) -> Self {
        match value {
            ValueXrcs::Dynamic(d) => ValueDsts::Dynamic(d),
            ValueXrcs::Sink(_) => ValueDsts::Void(None),
        }
    }
}

impl From<XrcSink> for StaticValueCfg {
    fn from(value: XrcSink) -> Self {
        Self {
            value: value.get_numeric_value().into(),
            interval: AutoOrManual::Auto(value.get_interval()),
        }
    }
}

impl From<ValueXrcs> for ValueSrcs {
    fn from(value: ValueXrcs) -> Self {
        match value {
            ValueXrcs::Dynamic(d) => ValueSrcs::Dynamic(d),
            ValueXrcs::Sink(x) => ValueSrcs::Static(x.into()),
        }
    }
}

// impl WithDeviceControlMatcherKey for ValueDsts {
//     fn _get_device_control_matcher_key(&self) -> Option<DeviceControlMatcherKey<'_>> {
//         if let Self::Dynamic(DynValueRefs::DeviceControlMatcher(d)) = self {
//             Some((&d.device_matcher_key, &d.control_matcher_key))
//         } else {
//             None
//         }
//     }
// }

impl ValueIface for ValueDsts {
    fn value_identity(&self) -> String {
        match self {
            Self::Void(_) => egui_phosphor::bold::EMPTY.into(),
            Self::Dynamic(d) => format!(
                "Dst({}({}))",
                if d._is_device_control_matcher() { "CTL:" } else { "VAR:" },
                d.to_string()
            ),
        }
    }

    fn value_is_static(&self) -> bool {
        matches!(self, Self::Void(..))
    }
}

impl WithNumInterval for ValueDsts
where
    ValueDsts: WithNumericValue<ValueT = <DynValueRefs as WithNumericValue>::ValueT>,
{
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        match self {
            Self::Dynamic(d) => d.get_interval(),
            Self::Void(_) => crate::num_interval::ZERO_INTERVAL,
        }
    }
}

impl WithNumIntervalSettable for ValueDsts {
    fn set_interval(&mut self, _: NumInterval<Self::ValueT>) {
        log::error!("Can't set interval on dynamic value ref.")
    }
}

impl WithNumericValue for ValueDsts {
    type ValueT = BaseNumT;
    fn get_numeric_value(&self) -> Self::ValueT {
        match self {
            Self::Dynamic(d) => d.get_numeric_value(),
            Self::Void(_) => Self::ValueT::default(),
        }
    }
}

impl WithNumericValueSettable for ValueDsts {
    fn set_numeric_value(&self, value: Self::ValueT) {
        match self {
            Self::Dynamic(d) => d.set_numeric_value(value),
            Self::Void(_) => {}
        }
    }
}

impl ValueDsts {
    #[allow(unused)]
    pub(crate) fn is_static(&self) -> bool {
        matches!(self, Self::Void(..))
    }

    pub(crate) fn get_idle_tick_enabled_flag(&self) -> Option<&AtomicBool> {
        if let ValueDsts::Dynamic(DynValueRefs::DeviceControlMatcher(d)) = self {
            Some(d.control_matcher.get_idle_tick_enabled_flag())
        } else {
            None
        }
    }

    pub(crate) fn _get_id(&self) -> Option<ObjId> {
        match self {
            ValueDsts::Void(..) => None,
            ValueDsts::Dynamic(d) => match d {
                DynValueRefs::DeviceControlMatcher(d) => Some(d.control_matcher.get_id()),
                DynValueRefs::Variable(v) => Some(v.variable.get_id()),
            },
        }
    }

    pub(crate) fn get_interval(&self) -> NumInterval<BaseNumT> {
        match self {
            ValueDsts::Void(..) => ZERO_INTERVAL,
            Self::Dynamic(d) => match d {
                DynValueRefs::DeviceControlMatcher(d) => d.control_matcher.get_interval(),
                DynValueRefs::Variable(v) => v.variable.get_interval(),
            },
        }
    }

    pub(crate) fn _get_relativity(&self) -> Relativity {
        match self {
            ValueDsts::Void(..) => Relativity::Abs,
            Self::Dynamic(d) => match d {
                DynValueRefs::DeviceControlMatcher(d) => d.control_matcher.get_relativity(),
                DynValueRefs::Variable(_) => Relativity::Abs, // TODO: Variables support: always Abs or not ?
            },
        }
    }

    pub(crate) fn _is_void(&self) -> bool {
        matches!(*self, Self::Void(..))
    }

    pub(crate) fn _is_dynamic(&self) -> bool {
        matches!(*self, Self::Dynamic(..))
    }

    pub(crate) fn _is_device_control_matcher(&self) -> bool {
        matches!(*self, Self::Dynamic(DynValueRefs::DeviceControlMatcher(..)))
    }
}

impl std::hash::Hash for DynValueRefs {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            Self::DeviceControlMatcher(d) => {
                d.device_matcher_key.hash(state);
                d.control_matcher_key.hash(state);
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
            ValueDsts::Void(..) => std::mem::discriminant(self).hash(state),
            Self::Dynamic(dynamic_value_ref_rt) => dynamic_value_ref_rt.hash(state),
        }
    }
}

// #[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) enum ValueTargets {
    Src(ValueSrcs),
    Dst(ValueDsts),
    Xrc(ValueXrcs),
}

impl TryFrom<ValueTargets> for DynValueRefs {
    type Error = String;

    fn try_from(value: ValueTargets) -> Result<Self, Self::Error> {
        value.try_into()
    }
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

//---------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub(crate) enum AutoOrManual<T: Default> {
    #[serde(skip)]
    Auto(T),
    Manual(T),
}

impl<T: Default + PartialOrd> PartialOrd for AutoOrManual<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.deref().partial_cmp(other.deref())
    }
}

#[test]
fn auto_or_manual_check_serialization() {
    assert!(
        !serde_saphyr::to_string(&AutoOrManual::Manual(1.0))
            .unwrap_or_default()
            .is_empty()
    );
    assert!(
        serde_saphyr::to_string(&AutoOrManual::Auto(1.0))
            .unwrap_or_default()
            .is_empty()
    );
}

impl<T: Default + Display> Display for AutoOrManual<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            AutoOrManual::Manual(m) => format!("Manual({})", m),
            AutoOrManual::Auto(a) => format!("Auto({})", a),
        })
    }
}

impl<T: Default> From<T> for AutoOrManual<T> {
    fn from(value: T) -> Self {
        Self::Auto(value)
    }
}

impl<T: Copy + Default> Copy for AutoOrManual<T> {}

impl<T: Default> AutoOrManual<T> {
    #[allow(unused)]
    pub(crate) fn inner_ref(&self) -> &T {
        match self {
            Self::Manual(m) => m,
            Self::Auto(a) => a,
        }
    }

    #[allow(unused)]
    pub(crate) fn inner_mut(&mut self) -> &mut T {
        match self {
            Self::Manual(m) => m,
            Self::Auto(a) => a,
        }
    }

    #[allow(unused)]
    pub(crate) fn make_auto(self) -> AutoOrManual<T> {
        match self {
            Self::Manual(m) => Self::Auto(m),
            Self::Auto(_) => self,
        }
    }

    #[allow(unused)]
    pub(crate) fn make_manual(self) -> AutoOrManual<T> {
        match self {
            Self::Manual(_) => self,
            Self::Auto(a) => Self::Manual(a),
        }
    }

    pub(crate) fn is_auto(&self) -> bool {
        matches!(self, Self::Auto(_))
    }

    #[allow(unused)]
    pub(crate) fn is_manual(&self) -> bool {
        matches!(self, Self::Manual(_))
    }

    #[allow(unused)]
    pub(crate) fn set_inner(&mut self, other: T) {
        match self {
            AutoOrManual::Manual(v) => *v = other,
            AutoOrManual::Auto(v) => *v = other,
        }
    }
}

impl<T: Default> Deref for AutoOrManual<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        match self {
            AutoOrManual::Manual(v) => v,
            AutoOrManual::Auto(v) => v,
        }
    }
}

impl<T: Default> DerefMut for AutoOrManual<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            AutoOrManual::Manual(v) => v,
            AutoOrManual::Auto(v) => v,
        }
    }
}

impl<T: Default> Default for AutoOrManual<T> {
    fn default() -> Self {
        Self::Auto(Default::default())
    }
}

// ---------------

impl WithNumericValueSanitizerStatic for ValueSrcs {
    fn sanitize_numeric_value_static(value: Self::ValueT) -> Self::ValueT {
        value
    }
}

impl WithNumIntervalSanitizerStatic for ValueSrcs {
    fn sanitize_interval_static(interval: NumInterval<Self::ValueT>) -> NumInterval<Self::ValueT> {
        interval
    }
}

impl WithNumIntervalSanitizerStatic for ValueXrcs {
    fn sanitize_interval_static(interval: NumInterval<Self::ValueT>) -> NumInterval<Self::ValueT> {
        interval
    }
}

impl WithNumericValueSanitizerStatic for ValueDsts {
    fn sanitize_numeric_value_static(value: Self::ValueT) -> Self::ValueT {
        value
    }
}

impl WithNumIntervalSanitizerStatic for ValueDsts {
    fn sanitize_interval_static(interval: NumInterval<Self::ValueT>) -> NumInterval<Self::ValueT> {
        interval
    }
}

impl From<ValueTargets> for ValueXrcs {
    fn from(value: ValueTargets) -> Self {
        match value {
            ValueTargets::Src(s) => match s {
                ValueSrcs::Static(s) => Self::Sink(XrcSink::from(s.value.get())),
                ValueSrcs::Dynamic(d) => Self::Dynamic(d),
            },
            ValueTargets::Dst(d) => match d {
                ValueDsts::Dynamic(d) => Self::Dynamic(d),
                ValueDsts::Void(_) => Self::Sink(Default::default()),
            },
            ValueTargets::Xrc(x) => x,
        }
    }
}
impl From<ValueTargets> for ValueSrcs {
    fn from(value: ValueTargets) -> Self {
        match value {
            ValueTargets::Src(s) => s,
            ValueTargets::Dst(d) => match d {
                ValueDsts::Dynamic(d) => d.into(),
                ValueDsts::Void(_) => Self::Static(Default::default()),
            },
            ValueTargets::Xrc(x) => match x {
                ValueXrcs::Sink(_) => Self::Static(Default::default()),
                ValueXrcs::Dynamic(d) => Self::Dynamic(d),
            },
        }
    }
}
impl From<ValueTargets> for ValueDsts {
    fn from(value: ValueTargets) -> Self {
        match value {
            ValueTargets::Src(s) => match s {
                ValueSrcs::Static(_) => Self::Void(None),
                ValueSrcs::Dynamic(d) => Self::Dynamic(d),
            },
            ValueTargets::Dst(d) => d,
            ValueTargets::Xrc(x) => match x {
                ValueXrcs::Sink(_) => Self::Void(None),
                ValueXrcs::Dynamic(d) => Self::Dynamic(d),
            },
        }
    }
}

// impl TryFrom<ValueTargets> for ValueXrcs {
//     type Error = String;
//     fn try_from(value: ValueTargets) -> Result<Self, Self::Error> {
//         match value {
//             ValueTargets::Src(s) => match s {
//                 ValueSrcs::Static(s) => Err("Can't convert a static value to a source-destination".into()),
//                 ValueSrcs::Dynamic(d) => Ok(Self::Dynamic(d)),
//             },
//             ValueTargets::Dst(d) => match d {
//                 ValueDsts::Dynamic(d) => Ok(Self::Dynamic(d)),
//                 ValueDsts::Void(_) => Err("Can't convert void to a source-detination".into()),
//             },
//             ValueTargets::Xrc(x) => Ok(x),
//         }
//     }
// }

// impl TryFrom<ValueTargets> for ValueDsts {
//     type Error = String;

//     fn try_from(value: ValueTargets) -> Result<Self, Self::Error> {
//         match value {
//             ValueTargets::Src(s) => match s {
//                 ValueSrcs::Static(_) => Err("Can't convert from a static source value target to a detination".into()),
//                 ValueSrcs::Dynamic(d) => Ok(ValueDsts::Dynamic(d)),
//             },
//             ValueTargets::Dst(d) => Ok(d),
//             ValueTargets::Xrc(x) => Ok(x.into()),
//         }
//     }
// }

// impl TryFrom<ValueTargets> for ValueSrcs {
//     type Error = String;

//     fn try_from(value: ValueTargets) -> Result<Self, Self::Error> {
//         match value {
//             ValueTargets::Src(s) => Ok(s),
//             ValueTargets::Dst(d) => match d {
//                 ValueDsts::Dynamic(d) => Ok(ValueSrcs::Dynamic(d)),
//                 ValueDsts::Void(_) => Err("Can't convert from a stvoidatic source value target to a source".into()),
//             },
//             ValueTargets::Xrc(x) => Ok(x.into()),
//         }
//     }
// }
