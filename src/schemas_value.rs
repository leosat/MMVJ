// ----------------------------------
// Value types hier:
// ----------------------------------
// 1. Src or Dst
// 2. Static|(Src) or Dynamic(Src or Dst) or Void|(Dst)
// 3. VarRef(Dynamic) or DeviceControlMatcher(Dynamic)

use std::{
    cell::Cell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize},
    },
};

use crate::relativity::Relativity;
use crate::{config::WithSelfSanitize, num_interval::ZERO_INTERVAL};
use crate::{
    num_interval::{OutOfRangePolicy, UNIT_INTERVAL},
    tfm_exec::TfmExecCtx,
};
use crossbeam_utils::CachePadded;
use deserialize_untagged_verbose_error::DeserializeUntaggedVerboseError;
use garde::{Validate, rules::range::Bounds};
use num_traits::ToPrimitive;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use traversable::{Traversable, TraversableMut};

use crate::{
    base_num::{BaseAtomicT, BaseNumT},
    num_interval::{NumInterval, NumIntervalValue},
    schemas_common::{ObjId, WithRuntimeId},
    schemas_control_matcher::ControlMatchers,
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
pub(crate) trait WithLastKnownIO<T> {
    fn get_last_known_io(&self) -> T;
}

pub(crate) trait WithLastKnownIOSettable<T> {
    fn set_last_known_io(&self, value: T);
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
    type ValueT: NumIntervalValue; // RRRRR
    fn get_numeric_value(&self) -> Self::ValueT;
}

#[allow(unused)]
pub(crate) trait WithNumericValueClamped: WithNumericValue /*+ WithNumInterval*/ {
    fn get_numeric_value_clamped(&self) -> <Self as WithNumericValue>::ValueT;
}

#[allow(unused)]
pub(crate) trait WithNumericValueClampedPredicated: WithNumericValue /*+ WithNumInterval*/ {
    type PredicationParamsT;
    fn get_numeric_value_clamped_predicated(
        &self,
        params: Self::PredicationParamsT,
    ) -> <Self as WithNumericValue>::ValueT;
}

/// This trait sets a numeric value within configuration tree/transformation state cache.
/// NB: It does not actually write to any devices!
pub(crate) trait WithNumericValueSettable: WithNumericValue {
    fn set_numeric_value(&self, value: Self::ValueT);
}

pub(crate) trait WithNumInterval: WithNumericValue {
    fn get_interval(&self) -> NumInterval<Self::ValueT>;
}

pub(crate) trait WithNumIntervalMut {
    type ValueT: NumIntervalValue;
    fn interval_mut(&mut self) -> &mut NumInterval<Self::ValueT>;
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
    pub(crate) control_matcher_key: String,
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

pub(crate) fn dummy_control_matcher_rt() -> ControlMatchers {
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
impl WithTriggersMapping for ValueSrcs {
    fn _get_triggers_mapping(&self) -> bool {
        true
    }

    fn _set_triggers_mapping(&mut self, _flag: bool) {}
}

impl WithTriggersMapping for ValueDsts {
    fn _get_triggers_mapping(&self) -> bool {
        false
    }

    fn _set_triggers_mapping(&mut self, _flag: bool) {}
}

impl<SanT: PortSanPolicy<PortInnerT>, PortInnerT: PortInnerIface> WithTriggersMapping for ValuePort<PortInnerT, SanT> {
    fn _get_triggers_mapping(&self) -> bool {
        self.triggers_mapping
    }

    fn _set_triggers_mapping(&mut self, flag: bool) {
        self.triggers_mapping = flag
    }
}

impl<SanT: PortSanPolicy<PortInnerT>, PortInnerT: PortInnerIface> ValuePortIface for ValuePort<PortInnerT, SanT>
where
    Self: WithNumericValue<ValueT = <PortInnerT as WithNumericValue>::ValueT>,
{
    type InnerT = PortInnerT;

    fn port_get_identity_str(&self) -> String {
        self.target.port_inner_identity()
    }

    fn port_inner_ref(&self) -> &Self::InnerT {
        &self.target
    }

    fn port_inner_mut(&mut self) -> &mut Self::InnerT {
        &mut self.target
    }

    fn port_get_default_interval_from_inner(&self) -> NumInterval<Self::ValueT> {
        Self::InnerT::default().get_interval()
    }

    fn port_set_remap_interval(&mut self, ri: NumInterval<Self::ValueT>) {
        self.remap = Some(ri);
    }

    fn port_get_remap_interval(&self) -> Option<NumInterval<Self::ValueT>> {
        self.remap
    }

    fn port_set_remap_off(&mut self) {
        self.remap = None
    }

    fn port_set_remap_from_inner_default(&mut self) {
        self.remap = Some(PortInnerT::default().get_interval())
    }

    fn port_write_to_device(&self, exe_ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<Self::ValueT>,
    {
        if let Some(dcm_key) = self.port_inner_ref()._get_device_control_matcher_key() {
            exe_ctx.set_device_control_matcher(dcm_key, self.get_numeric_value().into());
        }
    }
}

// -------------------------------------------------
pub(crate) type DeviceControlMatcherKey<'k> = (&'k str, &'k str);
pub(crate) trait WithDeviceControlMatcherKey {
    fn _get_device_control_matcher_key(&self) -> Option<DeviceControlMatcherKey<'_>>;
}

// -------------------------------------------------
pub(crate) trait WithTriggersMapping {
    fn _get_triggers_mapping(&self) -> bool;
    fn _set_triggers_mapping(&mut self, flag: bool);
}

// --------------------------------------------------------
pub(crate) trait ValuePortIface: WithTriggersMapping + From<Self::InnerT> + WithNumericValue {
    type InnerT: PortInnerIface;
    fn port_get_default_interval_from_inner(&self) -> NumInterval<Self::ValueT>;
    fn port_get_identity_str(&self) -> String;

    fn port_set_remap_off(&mut self);
    fn port_set_remap_from_inner_default(&mut self);
    fn port_get_remap_interval(&self) -> Option<NumInterval<Self::ValueT>>;
    fn port_set_remap_interval(&mut self, ri: NumInterval<Self::ValueT>);

    fn port_inner_ref(&self) -> &Self::InnerT;
    fn port_inner_mut(&mut self) -> &mut Self::InnerT;

    #[allow(unused)]
    fn port_write_to_device(&self, exe_ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<Self::ValueT>;
}

pub(crate) trait WithNumIntervalSettable: WithNumInterval {
    fn set_interval(&mut self, interval: NumInterval<Self::ValueT>);
}

// --------------------------------------------------------

pub(crate) trait PortInnerIface:
    Clone
    + Default
    + PartialEq
    + PartialOrd
    + JsonSchema
    + WithDeviceControlMatcherKey
    + WithNumInterval
    + WithNumIntervalSettable
    + WithNumericValue
    + WithNumericValueSettable
    + Serialize
    + for<'de> Deserialize<'de>
{
    fn port_inner_identity(&self) -> String;
    fn port_inner_is_static(&self) -> bool;
    #[allow(unused)]
    fn port_inner_get_device_control_matcher_key(&self) -> Option<(&str, &str)>;
}

impl<T: Default + PartialOrd> PartialOrd for AutoOrManual<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.deref().partial_cmp(other.deref())
    }
}

#[derive(JsonSchema, Debug, Clone, PartialOrd, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[serde(bound(serialize = "PortInnerT: PortInnerIface, <PortInnerT as WithNumericValue>::ValueT: serde::Serialize"))]
#[serde(bound(
    deserialize = "PortInnerT: PortInnerIface, <PortInnerT as WithNumericValue>::ValueT: serde::Deserialize<'de>"
))]
enum ValuePortSerdeHelper<PortInnerT: PortInnerIface> {
    AsPort {
        remap: Option<NumInterval<PortInnerT::ValueT>>,
        #[serde(skip_serializing_if = "crate::schemas_common::is_false")]
        #[serde(default)]
        triggers_mapping: bool,
        //#[serde(flatten)] // Fails when inner is serialized to a single number.
        target: PortInnerT,
    },
    AsInner(PortInnerT),
}

impl<SanT: PortSanPolicy<PortInnerT>, PortInnerT: PortInnerIface> From<ValuePortSerdeHelper<PortInnerT>>
    for ValuePort<PortInnerT, SanT>
where
    Self: WithNumericValue<ValueT = <PortInnerT as WithNumericValue>::ValueT>,
{
    fn from(value: ValuePortSerdeHelper<PortInnerT>) -> Self {
        match value {
            ValuePortSerdeHelper::AsInner(i) => i.into(),
            ValuePortSerdeHelper::AsPort {
                remap,
                triggers_mapping,
                target,
            } => Self {
                remap,
                triggers_mapping,
                target,
                _san_tag: PhantomData,
            },
        }
    }
}

impl<SanT: PortSanPolicy<PortInnerT>, PortInnerT: PortInnerIface> From<ValuePort<PortInnerT, SanT>>
    for ValuePortSerdeHelper<PortInnerT>
{
    fn from(value: ValuePort<PortInnerT, SanT>) -> Self {
        if !value.target.port_inner_is_static() {
            ValuePortSerdeHelper::AsPort {
                remap: value.remap,
                triggers_mapping: value.triggers_mapping,
                target: value.target,
            }
        } else {
            ValuePortSerdeHelper::AsInner(value.target)
        }
    }
}

// -------------------------------------------
pub(crate) trait PortSanPolicy<SanProviderT = ()>:
    std::fmt::Debug + Copy + Clone + PartialOrd + PartialEq + 'static
{
    const _TAG_DESC: &'static str;

    fn san_policy_get_value_san_doc_str() -> &'static str
    where
        SanProviderT: WithNumericValueSanitizerStatic,
    {
        Self::_TAG_DESC
    }

    fn san_policy_sanitize_numeric_value(value: SanProviderT::ValueT) -> SanProviderT::ValueT
    where
        SanProviderT: WithNumericValueSanitizerStatic;

    fn _san_policy_sanitize_interval(interval: NumInterval<SanProviderT::ValueT>) -> NumInterval<SanProviderT::ValueT>
    where
        SanProviderT: WithNumIntervalSanitizerStatic;

    fn san_policy_sanitize_this_inplace<SelfSanitizedT: WithSelfSanitize>(this: &mut SelfSanitizedT);
    fn _san_policy_sanitize_this<SelfSanitizedT: WithSelfSanitize>(this: SelfSanitizedT) -> SelfSanitizedT;
}

#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub(crate) struct SanPolicyUseFromPortInner;
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub(crate) struct SanPolicyNone;

impl<SanProviderT> PortSanPolicy<SanProviderT> for SanPolicyUseFromPortInner {
    const _TAG_DESC: &'static str = "\
       Values read from this port are strictly sanitized for particular parameter";

    fn san_policy_sanitize_numeric_value(value: SanProviderT::ValueT) -> SanProviderT::ValueT
    where
        SanProviderT: WithNumericValueSanitizerStatic,
    {
        SanProviderT::sanitize_numeric_value_static(value)
    }

    fn _san_policy_sanitize_interval(interval: NumInterval<SanProviderT::ValueT>) -> NumInterval<SanProviderT::ValueT>
    where
        SanProviderT: WithNumIntervalSanitizerStatic,
    {
        SanProviderT::sanitize_interval_static(interval)
    }

    fn san_policy_sanitize_this_inplace<SelfSanitizedT: WithSelfSanitize>(this: &mut SelfSanitizedT) {
        this.sanitize_inplace()
    }

    fn _san_policy_sanitize_this<SelfSanitizedT: WithSelfSanitize>(this: SelfSanitizedT) -> SelfSanitizedT {
        this.sanitize_self()
    }

    fn san_policy_get_value_san_doc_str() -> &'static str
    where
        SanProviderT: WithNumericValueSanitizerStatic,
    {
        <SanProviderT as WithNumericValueSanitizerStatic>::get_value_sanitizer_policy_doc_str()
    }
}

impl<SanProviderT> PortSanPolicy<SanProviderT> for SanPolicyNone {
    const _TAG_DESC: &'static str = "\
       No predefined sanitization logic applied to value read from this port!";

    fn san_policy_sanitize_numeric_value(value: SanProviderT::ValueT) -> SanProviderT::ValueT
    where
        SanProviderT: WithNumericValue,
    {
        value
    }

    fn _san_policy_sanitize_interval(interval: NumInterval<SanProviderT::ValueT>) -> NumInterval<SanProviderT::ValueT>
    where
        SanProviderT: WithNumericValue,
    {
        interval
    }

    fn san_policy_sanitize_this_inplace<SelfSanitizedT: WithSelfSanitize>(_this: &mut SelfSanitizedT) {}
    fn _san_policy_sanitize_this<SelfSanitizedT: WithSelfSanitize>(this: SelfSanitizedT) -> SelfSanitizedT {
        this
    }
}

// -------------------------------------------
#[derive(JsonSchema, Debug, Clone, PartialOrd, PartialEq, Deserialize, Serialize)]
#[serde(from = "ValuePortSerdeHelper<PortInnerT>", into = "ValuePortSerdeHelper<PortInnerT>")]
#[serde(bound(serialize = "
    PortInnerT: PortInnerIface,
    ValuePort<PortInnerT, SanT>: WithNumericValue<ValueT = <PortInnerT as WithNumericValue>::ValueT>, 
    <PortInnerT as WithNumericValue>::ValueT: serde::Serialize
"))]
#[serde(bound(deserialize = "
    PortInnerT: PortInnerIface,
    ValuePort<PortInnerT, SanT>: WithNumericValue<ValueT = <PortInnerT as WithNumericValue>::ValueT>, 
    <PortInnerT as WithNumericValue>::ValueT: serde::Deserialize<'de>
"))]
pub(crate) struct ValuePort<PortInnerT: PortInnerIface, SanT: PortSanPolicy<PortInnerT> = SanPolicyUseFromPortInner> {
    pub(super) remap: Option<NumInterval<<PortInnerT as WithNumericValue>::ValueT>>,
    pub(super) triggers_mapping: bool,
    pub(super) target: PortInnerT,
    #[serde(skip)]
    pub(super) _san_tag: PhantomData<SanT>,
}

// ----------------------------------------------
impl<PortInnerT: PortInnerIface, SanT: PortSanPolicy<PortInnerT>> Default for ValuePort<PortInnerT, SanT> {
    fn default() -> Self {
        Self {
            remap: Default::default(),
            triggers_mapping: Default::default(),
            target: Default::default(),
            _san_tag: PhantomData,
        }
    }
}

impl<SanT: PortSanPolicy<ValueSrcs>> ::traversable::Traversable for ValuePort<ValueSrcs, SanT>
where
    Self: WithNumericValue,
{
    fn traverse<V: traversable::Visitor>(&self, visitor: &mut V) -> std::ops::ControlFlow<V::Break> {
        self.target.traverse(visitor)
    }
}

impl<SanT: PortSanPolicy<ValueSrcs>> ::traversable::TraversableMut for ValuePort<ValueSrcs, SanT>
where
    Self: WithNumericValue,
{
    fn traverse_mut<V: traversable::VisitorMut>(&mut self, visitor: &mut V) -> std::ops::ControlFlow<V::Break> {
        self.target.traverse_mut(visitor)
    }
}
// ----------------------------------------------

impl<SanT: PortSanPolicy<PortInnerT>, PortInnerT: PortInnerIface> From<PortInnerT> for ValuePort<PortInnerT, SanT>
where
    Self: WithNumericValue,
{
    fn from(value: PortInnerT) -> Self {
        Self {
            remap: if !value.port_inner_is_static() {
                Some(PortInnerT::default().get_interval())
            } else {
                None
            },
            triggers_mapping: false,
            target: value,
            _san_tag: PhantomData,
        }
    }
}

impl<SanT, PortInnerT> WithNumericValueSettable for ValuePort<PortInnerT, SanT>
where
    PortInnerT: PortInnerIface,
    PortInnerT: WithNumericValueSanitizerStatic,
    SanT: PortSanPolicy<PortInnerT>,
    Self: WithNumericValue<ValueT = <PortInnerT as WithNumericValue>::ValueT>,
{
    fn set_numeric_value(&self, mut value: <Self as WithNumericValue>::ValueT) {
        value = SanT::san_policy_sanitize_numeric_value(value);
        let value = self
            .remap
            .map(|remap| {
                self.target
                    .get_interval()
                    .map_from(value, &remap, OutOfRangePolicy::Clamp)
            })
            .unwrap_or(value);
        self.target.set_numeric_value(value);
    }
}

impl<SanT, PortInnerT> WithNumericValue for ValuePort<PortInnerT, SanT>
where
    PortInnerT: PortInnerIface + WithNumericValueSanitizerStatic,
    SanT: PortSanPolicy<PortInnerT>,
{
    type ValueT = <PortInnerT as WithNumericValue>::ValueT;
    fn get_numeric_value(&self) -> Self::ValueT {
        let mut value = self.target.get_numeric_value();
        value = self
            .remap
            .map(|r| r.map_from(value, &self.target.get_interval(), OutOfRangePolicy::Clamp))
            .unwrap_or(value);
        SanT::san_policy_sanitize_numeric_value(value)
    }
}

impl<SanT, PortInnerT> Bounds for ValuePort<PortInnerT, SanT>
where
    PortInnerT: PortInnerIface,
    SanT: PortSanPolicy<PortInnerT>,
    Self: WithNumericValue,
{
    type Size = BaseNumT;
    const MIN: Self::Size = Self::Size::MIN;
    const MAX: Self::Size = Self::Size::MAX;

    fn validate_bounds(
        &self,
        lower_bound: Self::Size,
        upper_bound: Self::Size,
    ) -> Result<(), garde::rules::range::OutOfBounds> {
        let value = self.get_numeric_value();
        if value.to_f64().unwrap() < lower_bound.to_f64().unwrap() {
            Err(garde::rules::range::OutOfBounds::Lower)
        } else if value.to_f64().unwrap() > upper_bound.to_f64().unwrap() {
            Err(garde::rules::range::OutOfBounds::Upper)
        } else {
            Ok(())
        }
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

impl WithDeviceControlMatcherKey for ValueSrcs {
    fn _get_device_control_matcher_key(&self) -> Option<DeviceControlMatcherKey<'_>> {
        if let Self::Dynamic(DynValueRefs::DeviceControlMatcher(d)) = self {
            Some((&d.device_matcher_key, &d.control_matcher_key))
        } else {
            None
        }
    }
}

impl PortInnerIface for ValueSrcs {
    fn port_inner_identity(&self) -> String {
        match self {
            ValueSrcs::Static(_) => egui_phosphor::bold::PENCIL.into(),
            ValueSrcs::Dynamic(d) => format!(
                "Src({}({}))",
                if d._is_device_control_matcher() { "CTL:" } else { "VAR:" },
                d.to_string()
            ),
        }
    }

    fn port_inner_is_static(&self) -> bool {
        self.is_static()
    }

    fn port_inner_get_device_control_matcher_key(&self) -> Option<(&str, &str)> {
        if let Self::Dynamic(DynValueRefs::DeviceControlMatcher(dcm)) = self {
            Some((&dcm.device_matcher_key, &dcm.control_matcher_key))
        } else {
            None
        }
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

#[allow(unused)]
pub(crate) enum ClampPred {
    IfStatic,
    IfDynamic,
}

impl WithNumericValueClampedPredicated for ValueSrcs {
    type PredicationParamsT = ClampPred;

    fn get_numeric_value_clamped_predicated(
        &self,
        params: Self::PredicationParamsT,
    ) -> <Self as WithNumericValue>::ValueT {
        match self {
            Self::Static(s) => match params {
                ClampPred::IfStatic => s.get_interval().clamp(s.get_numeric_value()),
                ClampPred::IfDynamic => s.get_numeric_value(),
            },
            Self::Dynamic(d) => match params {
                ClampPred::IfDynamic => d.get_interval().clamp(d.get_numeric_value()),
                ClampPred::IfStatic => d.get_numeric_value(),
            },
        }
    }
}

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

    pub(crate) fn get_id(&self) -> Option<ObjId> {
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

impl Default for ValueDsts {
    fn default() -> Self {
        Self::Void(None)
    }
}

impl WithDeviceControlMatcherKey for ValueDsts {
    fn _get_device_control_matcher_key(&self) -> Option<DeviceControlMatcherKey<'_>> {
        if let Self::Dynamic(DynValueRefs::DeviceControlMatcher(d)) = self {
            Some((&d.device_matcher_key, &d.control_matcher_key))
        } else {
            None
        }
    }
}

impl PortInnerIface for ValueDsts {
    fn port_inner_identity(&self) -> String {
        match self {
            Self::Void(_) => egui_phosphor::bold::EMPTY.into(),
            Self::Dynamic(d) => format!(
                "Dst({}({}))",
                if d._is_device_control_matcher() { "CTL:" } else { "VAR:" },
                d.to_string()
            ),
        }
    }

    fn port_inner_is_static(&self) -> bool {
        matches!(self, Self::Void(..))
    }

    fn port_inner_get_device_control_matcher_key(&self) -> Option<(&str, &str)> {
        if let Self::Dynamic(DynValueRefs::DeviceControlMatcher(dcm)) = self {
            Some((&dcm.device_matcher_key, &dcm.control_matcher_key))
        } else {
            None
        }
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

    pub(crate) fn get_id(&self) -> Option<ObjId> {
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
    // TODO: Xrc(DynValueRefs),
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
    Auto(#[serde(skip)] T),
    Manual(T),
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

impl<T: Default + ToString> ToString for AutoOrManual<T> {
    fn to_string(&self) -> String {
        match self {
            AutoOrManual::Manual(m) => format!("Manual({})", m.to_string()),
            AutoOrManual::Auto(a) => format!("Auto({})", a.to_string()),
        }
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

// --------------------------------------------

pub(crate) trait _WithRelativitySanitizerStatic: WithRelativity {
    fn sanitize_relativity_static(rel: Relativity) -> Relativity;
}

pub(crate) trait WithNumericValueSanitizerStatic: WithNumericValue {
    fn sanitize_numeric_value_static(value: Self::ValueT) -> Self::ValueT;
    fn get_value_sanitizer_policy_doc_str() -> &'static str {
        "Policy unknown"
    }
}

pub(crate) trait WithNumIntervalSanitizerStatic: WithNumericValueSanitizerStatic {
    fn sanitize_interval_static(interval: NumInterval<Self::ValueT>) -> NumInterval<Self::ValueT>;
}

// --------------------------------------------

trait _WithNumericValueSanitized: WithNumericValue {
    fn get_numeric_value_sanitized(&self) -> <Self as WithNumericValue>::ValueT;
}

// --------------------------------------------
impl<PortInnerT: PortInnerIface + WithNumIntervalSanitizerStatic> WithSelfSanitize
    for ValuePort<PortInnerT, SanPolicyUseFromPortInner>
where
    Self: WithNumericValue<ValueT = <PortInnerT as WithNumericValue>::ValueT>,
{
    fn sanitize_inplace(&mut self) {
        if self.port_inner_ref().port_inner_is_static() {
            self.remap = None;
        } else {
            self.port_get_remap_interval()
                .map(|ri| self.port_set_remap_interval(PortInnerT::sanitize_interval_static(ri)));
        }
    }
}

impl<PortInnerT: PortInnerIface> WithSelfSanitize for ValuePort<PortInnerT, SanPolicyNone>
where
    Self: WithNumericValue<ValueT = <PortInnerT as WithNumericValue>::ValueT>,
{
    fn sanitize_inplace(&mut self) {
        if self.port_inner_ref().port_inner_is_static() {
            self.remap = None;
        } else {
            self.port_get_remap_interval()
                .map(|ri| self.port_set_remap_interval(ri));
        }
    }
}

impl<PortInnerT: PortInnerIface + WithNumIntervalSanitizerStatic> WithNumIntervalSanitizerStatic
    for ValuePort<PortInnerT, SanPolicyUseFromPortInner>
where
    Self: WithNumIntervalSanitizerStatic,
    Self: WithNumericValue<ValueT = <PortInnerT as WithNumericValue>::ValueT>,
{
    fn sanitize_interval_static(interval: NumInterval<Self::ValueT>) -> NumInterval<Self::ValueT> {
        PortInnerT::sanitize_interval_static(interval)
    }
}

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

// --------------------------------------------
#[macro_export]

macro_rules! make_port_inner_nutype {
    (
        name:           $name:ident,
        inner:          $inner:ident,
        inner_default:  $inner_default:expr,
        nutype_san:     $nutype_san:stmt,
        value_sanitize: $value_sanitize:expr,
        sandoc:         $sandoc:literal
    ) => {
        #[nutype::nutype(
                                constructor(visibility = pub(crate)),
                                default = $inner_default ,
                                derive( From, Debug, Clone, AsRef, Serialize, Deserialize, PartialEq, PartialOrd),
                                sanitize(with = $nutype_san)
                                )]
        pub(crate) struct $name($inner);

        impl crate::schemas_value::WithNumericValueSanitizerStatic for $name {
            fn sanitize_numeric_value_static(value: Self::ValueT) -> Self::ValueT {
                $value_sanitize(value)
            }

            fn get_value_sanitizer_policy_doc_str() -> &'static str {
                $sandoc
            }
        }

        impl crate::schemas_value::WithNumIntervalSanitizerStatic for $name {
            fn sanitize_interval_static(mut interval: NumInterval<Self::ValueT>) -> NumInterval<Self::ValueT> {
                use crate::schemas_value::WithNumericValueSanitizerStatic;
                interval.from = Self::sanitize_numeric_value_static(interval.from);
                interval.to = Self::sanitize_numeric_value_static(interval.to);
                interval
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new($inner_default)
            }
        }

        impl<SanT: crate::schemas_value::PortSanPolicy<$name>> ::traversable::Traversable for ValuePort<$name, SanT>
        where
            Self: WithNumericValue,
        {
            fn traverse<V: traversable::Visitor>(&self, visitor: &mut V) -> std::ops::ControlFlow<V::Break> {
                self.target.as_ref().traverse(visitor)
            }
        }

        impl<SanT: crate::schemas_value::PortSanPolicy<$name>> ::traversable::TraversableMut for ValuePort<$name, SanT>
        where
            Self: WithNumericValue,
        {
            fn traverse_mut<V: traversable::VisitorMut>(&mut self, visitor: &mut V) -> std::ops::ControlFlow<V::Break> {
                let mut tmp = self.target.clone().into_inner();
                let ret = tmp.traverse_mut(visitor);
                self.target = $name::new(tmp);
                ret
            }
        }

        impl ::traversable::Traversable for $name {
            fn traverse<V: traversable::Visitor>(&self, visitor: &mut V) -> std::ops::ControlFlow<V::Break> {
                self.as_ref().traverse(visitor)
            }
        }

        impl ::traversable::TraversableMut for $name {
            fn traverse_mut<V: traversable::VisitorMut>(&mut self, visitor: &mut V) -> std::ops::ControlFlow<V::Break> {
                let mut tmp = self.clone().into_inner();
                let ret = tmp.traverse_mut(visitor);
                *self = $name::new(tmp);
                ret
            }
        }


        impl crate::schemas_value::WithDeviceControlMatcherKey for  $name  {
            fn _get_device_control_matcher_key(&self) -> Option<crate::schemas_value::DeviceControlMatcherKey<'_>> {
                self.as_ref()._get_device_control_matcher_key()
            }
        }


        impl crate::schemas_value::PortInnerIface for $name {
            fn port_inner_identity(&self) -> String {
                self.as_ref().port_inner_identity()
            }
            fn port_inner_is_static(&self) -> bool {
                self.as_ref().is_static()
            }
            fn port_inner_get_device_control_matcher_key(&self) -> std::option::Option<(&str, &str)> {
                self.as_ref().port_inner_get_device_control_matcher_key()
            }
        }

        impl ::schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                $inner::json_schema(generator)
            }
        }

        impl<SanT: crate::schemas_value::PortSanPolicy<$name>> ::core::convert::From<ValuePort<$name, SanT>> for $name {
            fn from(value: ValuePort<$name, SanT>) -> Self {
                value.target
            }
        }

        impl crate::schemas_value::WithNumericValue for $name {
            type ValueT = BaseNumT;

            fn get_numeric_value(&self) -> Self::ValueT {
                self.as_ref().get_numeric_value()
            }
        }

        impl crate::schemas_value::WithNumInterval for $name {
            fn get_interval(&self) -> NumInterval<Self::ValueT> {
                self.as_ref().get_interval()
            }
        }

        impl crate::schemas_value::WithNumericValueSettable for $name {
            fn set_numeric_value(&self, value: Self::ValueT) {
                self.as_ref().set_numeric_value(value);
            }
        }

        impl crate::schemas_value::WithNumIntervalSettable for $name {
            fn set_interval(&mut self, interval: NumInterval<Self::ValueT>) {
                let mut tmp = self.clone().into_inner();
                tmp.set_interval(interval);
                *self = Self::new(tmp)
            }
        }
    };
}

#[macro_export]
macro_rules! make_output_port_inner_nutype {
    (
        $name:ident,
        default:  $inner_default:expr,
        san-doc:  $sandoc:literal,
        san-exe:  $value_sanitize:expr
    ) => {
        crate::make_port_inner_nutype!(
            name:     $name,
            inner:    ValueDsts,
            inner_default:  $inner_default,
            nutype_san: |mut s| { s },
            value_sanitize: $value_sanitize,
            sandoc:         $sandoc
        );
    };
}
#[macro_export]
macro_rules! make_input_port_inner_nutype {
    (
        $name:ident,
        default:  $inner_default:expr,
        san-doc:  $sandoc:literal,
        san-exe:  $value_sanitize:expr
    ) => {
        crate::make_port_inner_nutype!(
            name:     $name,
            inner:    ValueSrcs,
            inner_default:  $inner_default,
            nutype_san: |mut s| {
                use crate::schemas_value::WithNumIntervalSanitizerStatic;
                if let ValueSrcs::Static(ref mut s) = s {
                    // s.interval = AutoOrManual::Auto($inner_default.get_interval());
                    if s.get_interval() == $inner_default.get_interval() {
                        s.interval = AutoOrManual::Auto(Self::sanitize_interval_static(s.get_interval()));
                    } else {
                        s.interval = AutoOrManual::Manual(Self::sanitize_interval_static(s.get_interval()));
                    }
                }; s
            },
            value_sanitize: $value_sanitize,
            sandoc:         $sandoc
        );

        impl<'s> crate::gui_common::DrawEgui<'s> for $name {
            type In = crate::gui_value::GuiInValue<'s>;
            type Out = bool;

            fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
                let mut changed = false;
                changed |= crate::gui_value::draw_egui_for_input_port_inner(self, gui_in, ui);
                changed
            }
        }
    };
}

// =================================================================

#[cfg(test)]
mod testing {
    use super::*;
    use num_traits::Zero;

    #[test]
    #[allow(unused)]
    #[allow(non_local_definitions)]
    fn port_to_variable() {
        fn make_variable() -> DynValueRefs {
            DynValueRefs::Variable(VariableRef {
                variable_key: "test".into(),
                variable: Default::default(),
            })
        };

        fn make_output_port_inner_variable() -> ValueDsts {
            ValueDsts::Dynamic(make_variable())
        };

        fn make_input_port_inner_variable() -> ValueSrcs {
            ValueSrcs::Dynamic(make_variable())
        };
        //--------------------------------------

        make_output_port_inner_nutype!(
            PortInnerSanEpsilonForZero,
            default: ValueDsts::default(),
            san-doc: "Value must not be 0.0",
            san-exe: |v: BaseNumT| { if v.is_zero() {BaseNumT::EPSILON} else {v}}
        );

        let p_san_epsilon_for_zero = ValuePort::<PortInnerSanEpsilonForZero>::default();
        assert!(p_san_epsilon_for_zero.port_get_remap_interval().is_none());
        assert!(p_san_epsilon_for_zero.port_inner_ref().get_interval() == ZERO_INTERVAL); // Values written to [0,0] will be clamped to 0
        p_san_epsilon_for_zero.set_numeric_value(100.0);
        assert_eq!(p_san_epsilon_for_zero.get_numeric_value(), BaseNumT::EPSILON); // At port level 0 is sanitized to epsilon
        p_san_epsilon_for_zero.set_numeric_value(-100.0);
        assert_eq!(p_san_epsilon_for_zero.get_numeric_value(), BaseNumT::EPSILON);

        // --------------------------------------------------

        {
            make_output_port_inner_nutype!(
                PortInnerDeviceSanEpsilonGtZero,
                default: make_output_port_inner_variable(),
                san-doc: "Value must be > 0.0",
                san-exe: |v: BaseNumT| { if v <= BaseNumT::zero() {BaseNumT::EPSILON} else {v}}
            );

            let mut p_san_ge_epsilon = ValuePort::<PortInnerDeviceSanEpsilonGtZero>::default();
            {
                let p = &p_san_ge_epsilon;
                assert!(p.port_get_remap_interval().is_none());
                assert!(p.port_inner_ref().get_interval() == PortInnerDeviceSanEpsilonGtZero::default().get_interval());
                assert!(p.port_inner_ref().get_interval() == UNIT_INTERVAL);
                p.set_numeric_value(1.0);
                assert_eq!(p.get_numeric_value(), 1.0);
                assert_eq!(p.port_inner_ref().get_numeric_value(), 1.0);
                p.set_numeric_value(-1.0);
                assert_eq!(p.get_numeric_value(), BaseNumT::EPSILON);
            }

            {
                use std::ops::{Div, Mul};

                use crate::{
                    num_interval::{OutOfRangePolicy, SYMM_UNIT_INTERVAL},
                    test_utils::fp_approx_eq,
                };

                p_san_ge_epsilon.port_set_remap_interval(SYMM_UNIT_INTERVAL);
                assert!(p_san_ge_epsilon.port_inner_ref().get_interval() == UNIT_INTERVAL);

                p_san_ge_epsilon.set_numeric_value(-100.0);
                assert_eq!(p_san_ge_epsilon.get_numeric_value(), BaseNumT::EPSILON);
                assert!(
                    p_san_ge_epsilon.port_inner_ref().get_numeric_value()
                        == UNIT_INTERVAL.map_from(BaseNumT::EPSILON, &SYMM_UNIT_INTERVAL, OutOfRangePolicy::Clamp)
                );

                let mut p_no_san = ValuePort::<PortInnerDeviceSanEpsilonGtZero, SanPolicyNone>::default();
                p_no_san.port_set_remap_interval(SYMM_UNIT_INTERVAL);
                assert!(p_no_san.port_inner_ref().get_interval() == UNIT_INTERVAL);
                p_no_san.set_numeric_value(-0.5);
                assert!(fp_approx_eq(p_no_san.get_numeric_value(), -0.5));
                assert!(fp_approx_eq(p_no_san.port_inner_ref().get_numeric_value(), 0.25));
            }
        }
    }

    #[test]
    fn port_to_device() {
        fn make_device_control_matcher() -> DynValueRefs {
            DynValueRefs::DeviceControlMatcher(DeviceControlMatcherRef {
                device_matcher_key: "test".into(),
                control_matcher_key: "test".into(),
                control_matcher: ControlMatchers::Hid(Default::default()),
            })
        };

        fn make_output_port_inner_dcm() -> ValueDsts {
            ValueDsts::Dynamic(make_device_control_matcher())
        };

        fn make_input_port_inner_dcm() -> ValueSrcs {
            ValueSrcs::Dynamic(make_device_control_matcher())
        };

        // ----------------------------------
        {
            make_output_port_inner_nutype!(
                PortInnerDeviceSanEpsilonGtZero,
                default: make_output_port_inner_dcm(),
                san-doc: "Value must be > 0.0",
                san-exe: |v: BaseNumT| { if v <= BaseNumT::zero() {BaseNumT::EPSILON} else {v}}
            );

            let port_to_devie_san_gt_zero = ValuePort::<PortInnerDeviceSanEpsilonGtZero>::default();

            struct MockExeCtx {
                device_control_value_received: std::cell::Cell<BaseNumT>,
            }

            impl TfmExecCtx for MockExeCtx {
                fn set_device_control_matcher(&self, _dcm_key: DeviceControlMatcherKey, value: BaseNumT) {
                    self.device_control_value_received.set(value);
                }
            }

            let exe_ctx = MockExeCtx {
                device_control_value_received: Default::default(),
            };

            port_to_devie_san_gt_zero.set_numeric_value(42.0);
            assert!(exe_ctx.device_control_value_received.get() == BaseNumT::default());
            port_to_devie_san_gt_zero.port_write_to_device(&exe_ctx);
            assert_eq!(exe_ctx.device_control_value_received.get(), 42.0)
        }
    }
}
