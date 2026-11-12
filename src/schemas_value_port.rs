use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use crate::base_num::BaseAtomicT;
use crate::num_interval::NumIntervalValue;
use crate::schemas_transform::TfmSeqCfg;
use crate::schemas_value::{
    AutoOrManual, InputValueMetadata, TfmValueData, ValueDsts, ValueIface, ValueSrcs, ValueXrcs,
    WithDeviceControlMatcherRef, WithLastKnownIO, WithLastKnownIOSettable,
};
use crate::tfm_exec::{TfmExecCtx, WithTfmExec, get_out_of_range_policy};
use crate::{config::WithSelfSanitize, schemas_value::WithRelativity};
use crate::{relativity::Relativity, schemas_value::WithNumericValue};
use garde::rules::range::Bounds;
use num_traits::ToPrimitive;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use traversable::{Traversable, TraversableMut};

use crate::{base_num::BaseNumT, num_interval::NumInterval};

// -------------------------------------------------
pub(crate) trait WithMappingTriggerPredicate {
    type PredicateT;
    fn _eval_triggers_mapping_pred(&self) -> bool;
    fn _set_triggers_mapping_pred(&mut self, pred: Self::PredicateT);
    fn _get_triggers_mapping_pred(&mut self) -> Self::PredicateT;
}

impl<InnerT, SanT, RemapT, TfmT> WithLastKnownIO for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    type LastKnownIOValueT = BaseNumT;

    fn get_last_known_io(&self) -> Self::LastKnownIOValueT {
        self.last_known_io.load(Relaxed).into()
        // {
        //     // if let Some(tfm) = self.port_transformation_ref() {
        //     //     tfm.get_last_known_io().0.into()
        //     // } else {
        //     //     self.inner.get_last_known_io().into()
        //     // }
        // }
    }
}

impl<InnerT, SanT, RemapT, TfmT> WithLastKnownIOSettable for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    type LastKnownIOSettableValueT = BaseNumT;

    fn set_last_known_io(&self, value: Self::LastKnownIOSettableValueT) {
        self.last_known_io.store(value, Relaxed);
    }
}

// --------------------------------------------------------
pub(crate) trait ValuePortIface:
    WithMappingTriggerPredicate + WithSelfSanitize + WithDeviceControlMatcherRef
{
    type InnerT: ValueIface;
    type RemapT: PortRemapPolicy<<Self::InnerT as WithNumericValue>::ValueT>;
    type SanT: PortSanPolicy<Self::InnerT>;

    fn port_is_transformable(&self) -> bool;

    fn port_transformation_ref(&self) -> Option<&TfmSeqCfg>;
    fn port_transformation_mut(&mut self) -> Option<&mut TfmSeqCfg>;

    fn port_transformation_on(&mut self);
    fn port_transformation_off(&mut self);

    fn port_get_numeric_value(&self, ctx: Option<&impl TfmExecCtx>) -> <Self::InnerT as WithNumericValue>::ValueT
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
        <Self::InnerT as WithNumericValue>::ValueT: From<BaseNumT>;

    // fn port_get_last_known_io(&self) -> <Self::InnerT as WithLastKnownIO>::LastKnownIOValueT
    // where
    //     BaseNumT: From<<Self::InnerT as WithLastKnownIO>::LastKnownIOValueT>,
    //     <Self::InnerT as WithLastKnownIO>::LastKnownIOValueT: From<BaseNumT>;

    fn port_set_numeric_value(&self, value: <Self::InnerT as WithNumericValue>::ValueT, ctx: Option<&impl TfmExecCtx>)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>;

    fn port_get_interval(&self) -> NumInterval<<Self::InnerT as WithNumericValue>::ValueT>;
    fn port_get_relativity(&self) -> Relativity;

    fn port_get_default_interval_from_inner(&self) -> NumInterval<<Self::InnerT as WithNumericValue>::ValueT>;
    // fn _port_get_identity_str(&self) -> String;

    fn port_set_tfm_value_and_flush_to_devices(&self, value: TfmValueData<BaseNumT>, ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
        <Self::InnerT as WithNumericValue>::ValueT: From<BaseNumT>;

    fn port_set_numeric_value_and_flush_to_devices(
        &self,
        value: <Self::InnerT as WithNumericValue>::ValueT,
        ctx: &impl TfmExecCtx,
    ) where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
        <Self::InnerT as WithNumericValue>::ValueT: From<BaseNumT>;

    fn port_flush_numeric_value_to_devices(&self, ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
        <Self::InnerT as WithNumericValue>::ValueT: From<BaseNumT>;

    fn port_set_remap_off(&mut self);
    fn port_set_remap_from_inner_default(&mut self);
    fn port_user_defined_remapping_allowed(&self) -> bool;
    fn port_get_remap_interval(&self) -> Option<NumInterval<<Self::InnerT as WithNumericValue>::ValueT>>;
    fn port_set_remap_interval(&mut self, ri: NumInterval<<Self::InnerT as WithNumericValue>::ValueT>);

    fn port_inner_ref(&self) -> &Self::InnerT;
    fn port_inner_mut(&mut self) -> &mut Self::InnerT;
}

impl<InnerT, SanT, RemapT, TfmT> WithDeviceControlMatcherRef for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    fn get_device_control_matcher_ref(&self) -> Option<&crate::schemas_value::DeviceControlMatcherRef> {
        self.inner.get_device_control_matcher_ref()
    }
}

// ---------------------------------

#[derive(JsonSchema, Debug, Clone, PartialOrd, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
#[serde(bound(serialize = "InnerT: ValueIface, <InnerT as WithNumericValue>::ValueT: serde::Serialize"))]
#[serde(bound(deserialize = "InnerT: ValueIface, <InnerT as WithNumericValue>::ValueT: serde::Deserialize<'de>"))]
enum ValuePortSerdeHelper<InnerT: ValueIface> {
    AsPort {
        //#[serde(flatten)] // Fails when inner is serialized to a single number.
        #[serde(alias = "source", alias = "destination", alias = "target")]
        value: InnerT,
        #[serde(skip_serializing_if = "Option::is_none", rename = "tfm", alias = "transformation")]
        transformation: Option<TfmSeqCfg>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(alias = "remap_from_interval", alias = "remap_to_interval")]
        remap: Option<NumInterval<InnerT::ValueT>>,
        #[serde(skip_serializing_if = "crate::schemas_common::is_false")]
        #[serde(default)]
        triggers_mapping: bool,
    },
    AsInner(InnerT),
}

impl<InnerT, SanT, RemapT, TfmT> From<ValuePortSerdeHelper<InnerT>> for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface, //<TfmPolicyT = TfmT>,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    fn from(value: ValuePortSerdeHelper<InnerT>) -> Self {
        let mut port = match value {
            ValuePortSerdeHelper::AsInner(i) => i.into(),
            ValuePortSerdeHelper::AsPort {
                remap,
                triggers_mapping,
                value,
                transformation,
            } => Self {
                triggers_mapping,
                inner: value,
                san_policy: PhantomData,
                remap_policy: RemapT::new(remap),
                tfm_policy: {
                    let mut tfm_pol: TfmT = Default::default();
                    if let Some(tfm_cfg) = transformation {
                        tfm_pol.transformation_on();
                        if let Some(tfm) = tfm_pol.transformation_mut() {
                            *tfm = tfm_cfg;
                        };
                    }
                    tfm_pol.into()
                },
                _port_effective_interval: Default::default(),
                _port_effective_relativity: Default::default(),
                last_known_io: Default::default(),
            },
        };
        port.sanitize_inplace(());
        port
    }
}

impl<InnerT, SanT, RemapT, TfmT> From<ValuePort<InnerT, SanT, RemapT, TfmT>> for ValuePortSerdeHelper<InnerT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    fn from(port: ValuePort<InnerT, SanT, RemapT, TfmT>) -> Self {
        if port.inner.value_is_static()
            || ((port.port_get_remap_interval().is_none() || !port.remap_policy.remap_pol_user_defined_allowed())
                && port.tfm_policy.transformation_ref().is_none())
        {
            ValuePortSerdeHelper::AsInner(port.inner)
        } else {
            ValuePortSerdeHelper::AsPort {
                remap: port.remap_policy.remap_pol_get_interval(),
                triggers_mapping: port.triggers_mapping,
                value: port.inner,
                transformation: port.tfm_policy.transformation_ref().cloned(),
            }
        }
    }
}

// -------------------------------------------
pub(crate) trait PortSanPolicy<SanProviderT>:
    std::fmt::Debug + Copy + Clone + PartialOrd + PartialEq + 'static
{
    #[allow(unused)]
    fn san_policy_doc_str() -> &'static str;
    fn san_policy_sanitize_numeric_value(value: SanProviderT::ValueT) -> SanProviderT::ValueT
    where
        SanProviderT: WithNumericValueSanitizerStatic;
}

#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub(crate) struct SanPolicyUseFromPortInner;
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub(crate) struct SanPolicyNone;

impl<SanProviderT> PortSanPolicy<SanProviderT> for SanPolicyUseFromPortInner {
    fn san_policy_sanitize_numeric_value(value: SanProviderT::ValueT) -> SanProviderT::ValueT
    where
        SanProviderT: WithNumericValueSanitizerStatic,
    {
        SanProviderT::sanitize_numeric_value_static(value)
    }

    fn san_policy_doc_str() -> &'static str {
        "Values read from this port are strictly sanitized for particular parameter"
    }
}

impl<SanProviderT> PortSanPolicy<SanProviderT> for SanPolicyNone {
    fn san_policy_sanitize_numeric_value(value: SanProviderT::ValueT) -> SanProviderT::ValueT
    where
        SanProviderT: WithNumericValue,
    {
        value
    }

    fn san_policy_doc_str() -> &'static str {
        "No predefined sanitization logic applied to value read from this port!"
    }
}

// -------------------------------------------
pub(crate) trait PortRemapPolicy<ValueT: NumIntervalValue>:
    std::fmt::Debug + Clone + PartialOrd + PartialEq + 'static + Default
{
    fn new(interval: Option<NumInterval<ValueT>>) -> Self;
    fn remap_pol_user_defined_allowed(&self) -> bool;
    fn remap_pol_off(&self);
    fn remap_pol_get_interval(&self) -> Option<NumInterval<ValueT>>;
    fn remap_pol_set_interval(&self, interval: NumInterval<ValueT>) -> Option<NumInterval<ValueT>>;
}

#[derive(Clone, PartialEq, PartialOrd, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct RemapPolicyUserDefined {
    remap: std::cell::Cell<Option<NumInterval<BaseNumT>>>,
}

impl<ValueT: NumIntervalValue> PortRemapPolicy<ValueT> for RemapPolicyUserDefined {
    fn new(interval: Option<NumInterval<ValueT>>) -> Self {
        Self {
            remap: std::cell::Cell::new(interval.and_then(|interval| interval.cast::<BaseNumT>()).into()),
        }
    }

    fn remap_pol_off(&self) {
        self.remap.set(None);
    }

    fn remap_pol_get_interval(&self) -> Option<NumInterval<ValueT>> {
        self.remap.get().map(|interval| interval.cast::<ValueT>().unwrap())
    }

    fn remap_pol_set_interval(&self, interval: NumInterval<ValueT>) -> Option<NumInterval<ValueT>> {
        self.remap.set(Some(interval.cast::<BaseNumT>().unwrap()));
        Some(interval)
    }

    fn remap_pol_user_defined_allowed(&self) -> bool {
        true
    }
}

#[derive(Copy, Clone, PartialEq, PartialOrd, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct RemapPolicyNone;
impl<ValueT: NumIntervalValue> PortRemapPolicy<ValueT> for RemapPolicyNone {
    fn remap_pol_off(&self) {}

    fn remap_pol_get_interval(&self) -> Option<NumInterval<ValueT>> {
        None
    }

    fn remap_pol_set_interval(&self, _interval: NumInterval<ValueT>) -> Option<NumInterval<ValueT>> {
        None
    }

    fn new(_interval: Option<NumInterval<ValueT>>) -> Self {
        Self {}
    }

    fn remap_pol_user_defined_allowed(&self) -> bool {
        false
    }
}

// -------------------------------------------
pub(crate) trait TfmPolicyIface:
    Clone + Traversable + TraversableMut + JsonSchema + Default + PartialOrd
{
    fn is_transformable() -> bool;
    fn transformation_ref(&self) -> Option<&TfmSeqCfg>;
    fn transformation_mut(&mut self) -> Option<&mut TfmSeqCfg>;
    fn transformation_on(&mut self);
    fn transformation_off(&mut self);
}

pub(crate) trait TfmPolicyDefaultChoice {
    type TfmPolicyT: TfmPolicyIface + std::fmt::Debug + Clone + Traversable + TraversableMut + PartialOrd + Default;
}

#[derive(Default, Copy, Clone, PartialEq, PartialOrd, Debug, Traversable, TraversableMut, JsonSchema)]
pub(crate) struct TfmPolicyDisabled {}
impl TfmPolicyIface for TfmPolicyDisabled {
    fn transformation_ref(&self) -> Option<&TfmSeqCfg> {
        None
    }

    fn transformation_mut(&mut self) -> Option<&mut TfmSeqCfg> {
        None
    }

    fn transformation_on(&mut self) { /* NOP */
    }

    fn transformation_off(&mut self) { /* NOP */
    }

    fn is_transformable() -> bool {
        false
    }
}

#[derive(Default, Clone, PartialEq, PartialOrd, Debug, Traversable, TraversableMut, JsonSchema)]
pub(crate) struct TfmPolicyEnabled {
    transformation: Option<TfmSeqCfg>,
}

impl TfmPolicyIface for TfmPolicyEnabled {
    fn transformation_ref(&self) -> Option<&TfmSeqCfg> {
        self.transformation.as_ref()
    }

    fn transformation_mut(&mut self) -> Option<&mut TfmSeqCfg> {
        self.transformation.as_mut()
    }

    fn transformation_on(&mut self) {
        self.transformation = Some(Default::default())
    }

    fn transformation_off(&mut self) {
        self.transformation = None
    }

    fn is_transformable() -> bool {
        true
    }
}

impl TfmPolicyDefaultChoice for ValueSrcs {
    type TfmPolicyT = TfmPolicyEnabled;
}

impl TfmPolicyDefaultChoice for ValueDsts {
    type TfmPolicyT = TfmPolicyDisabled;
}

impl TfmPolicyDefaultChoice for ValueXrcs {
    type TfmPolicyT = TfmPolicyDisabled;
}

// -------------------------------------------
#[derive(JsonSchema, Debug, Clone, PartialEq, Deserialize, Serialize, Traversable, TraversableMut)]
#[serde(from = "ValuePortSerdeHelper<InnerT>", into = "ValuePortSerdeHelper<InnerT>")]
#[serde(bound(serialize = "
    TfmT: TfmPolicyIface,
    InnerT: ValueIface,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    <InnerT as WithNumericValue>::ValueT: serde::Serialize
"))] // <TfmPolicyT = TfmT>
#[serde(bound(deserialize = "
    TfmT: TfmPolicyIface,
    InnerT: ValueIface,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    <InnerT as WithNumericValue>::ValueT: for<'q> serde::Deserialize<'q>
"))] // <TfmPolicyT = TfmT>
pub(crate) struct ValuePort<
    InnerT,
    SanT = SanPolicyUseFromPortInner,
    RemapT = RemapPolicyUserDefined,
    TfmT = <InnerT as TfmPolicyDefaultChoice>::TfmPolicyT,
> where
    TfmT: TfmPolicyIface,
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    #[serde(skip)]
    #[traverse(skip)]
    last_known_io: Arc<BaseAtomicT>,
    inner: InnerT,
    #[serde(skip)]
    #[traverse(skip)]
    san_policy: PhantomData<SanT>,
    #[serde(flatten)]
    #[traverse(skip)]
    remap_policy: RemapT,
    tfm_policy: Box<TfmT>,
    #[traverse(skip)]
    triggers_mapping: bool,
    #[serde(skip)]
    #[traverse(skip)]
    _port_effective_interval: NumInterval<<InnerT as WithNumericValue>::ValueT>,
    #[serde(skip)]
    #[traverse(skip)]
    _port_effective_relativity: Relativity,
}

impl<InnerT, SanT, RemapT, TfmT> PartialOrd for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.inner.partial_cmp(&other.inner) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match self.san_policy.partial_cmp(&other.san_policy) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match self.remap_policy.partial_cmp(&other.remap_policy) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match self.tfm_policy.partial_cmp(&other.tfm_policy) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match self.triggers_mapping.partial_cmp(&other.triggers_mapping) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match self
            ._port_effective_interval
            .partial_cmp(&other._port_effective_interval)
        {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        self._port_effective_relativity
            .partial_cmp(&other._port_effective_relativity)
    }
}

// ----------------------------------------------
impl<InnerT, SanT, RemapT, TfmT> Default for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    fn default() -> Self {
        let mut tmp = Self {
            triggers_mapping: Default::default(),
            inner: Default::default(),
            san_policy: PhantomData,
            remap_policy: Default::default(),
            tfm_policy: Default::default(),
            _port_effective_interval: Default::default(),
            _port_effective_relativity: Default::default(),
            last_known_io: Default::default(),
        };
        tmp.sanitize_inplace(());
        tmp
    }
}

// ----------------------------------------------

impl<InnerT, SanT, RemapT, TfmT> From<InnerT> for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    fn from(value: InnerT) -> Self {
        Self {
            triggers_mapping: false,
            inner: value,
            san_policy: PhantomData,
            remap_policy: RemapT::default(),
            tfm_policy: Default::default(),
            _port_effective_interval: Default::default(),
            _port_effective_relativity: Default::default(),
            last_known_io: Default::default(),
        }
    }
}

impl<InnerT, SanT, RemapT, TfmT> Bounds for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
    BaseNumT: From<<InnerT as WithNumericValue>::ValueT>,
    <InnerT as WithNumericValue>::ValueT: From<BaseNumT>,
{
    type Size = BaseNumT;
    const MIN: Self::Size = Self::Size::MIN;
    const MAX: Self::Size = Self::Size::MAX;

    fn validate_bounds(
        &self,
        lower_bound: Self::Size,
        upper_bound: Self::Size,
    ) -> Result<(), garde::rules::range::OutOfBounds> {
        let value = self.port_get_numeric_value(None::<&()>);
        if value.to_f64().unwrap() < lower_bound.to_f64().unwrap() {
            Err(garde::rules::range::OutOfBounds::Lower)
        } else if value.to_f64().unwrap() > upper_bound.to_f64().unwrap() {
            Err(garde::rules::range::OutOfBounds::Upper)
        } else {
            Ok(())
        }
    }
}

// -----------------------------

impl<InnerT, SanT, RemapT, TfmT> WithSelfSanitize for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    type SanInputT = ();
    fn sanitize_inplace(&mut self, _: Self::SanInputT) {
        if self.inner.value_is_static() {
            self._port_effective_interval = self.inner.get_interval();
            self._port_effective_relativity = self.inner.get_relativity();
        } else {
            if let Some(mut remap) = self.remap_policy.remap_pol_get_interval() {
                remap.from = SanT::san_policy_sanitize_numeric_value(remap.from);
                remap.to = SanT::san_policy_sanitize_numeric_value(remap.to);
                self.remap_policy.remap_pol_set_interval(remap);
            }

            let inner_interval = self.port_inner_ref().get_interval();
            let inner_rel = self.port_inner_ref().get_relativity();

            if let Some(ref mut tfm) = self.port_transformation_mut() {
                tfm.recompute_metadata_and_sanitize_recursive(
                    AutoOrManual::Auto(InputValueMetadata {
                        interval: inner_interval.cast().unwrap(),
                        relativity: inner_rel,
                    })
                    .into(),
                );

                self._port_effective_relativity = tfm.get_out_relativity();
            } else {
                self._port_effective_relativity = inner_rel;
            }

            self._port_effective_interval = self
                .port_get_remap_interval()
                .map(|interval| interval.cast().unwrap())
                .or(self
                    .port_transformation_ref()
                    .as_ref()
                    .map(|tfm| tfm.get_out_interval().cast().unwrap()))
                .or(inner_interval.into())
                .unwrap();
        }
    }
}

impl<InnerT, SanT, RemapT, TfmT> ValuePortIface for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    type InnerT = InnerT;
    type RemapT = RemapT;
    type SanT = SanT;

    fn port_get_numeric_value(&self, ctx: Option<&impl TfmExecCtx>) -> InnerT::ValueT
    where
        BaseNumT: From<<InnerT as WithNumericValue>::ValueT>,
        <InnerT as WithNumericValue>::ValueT: From<BaseNumT>,
    {
        let (mut value, interval) = if let Some(tfm) = self.port_transformation_ref() {
            if let Some(ctx) = ctx {
                (
                    tfm.exec(self.inner.get_numeric_value().into(), ctx),
                    tfm.get_out_interval(),
                )
            } else {
                (tfm.get_last_known_io().1, tfm.get_out_interval())
            }
        } else {
            (
                self.inner.get_numeric_value().into(),
                self.inner.get_interval().cast().unwrap(),
            )
        };

        if let Some(remap) = self.port_get_remap_interval() {
            value = remap
                .map_from(
                    value,
                    &interval,
                    get_out_of_range_policy(
                        self.port_get_relativity(),
                        ctx.map(|ctx| ctx.is_reactive_mode()).unwrap_or(false),
                    ),
                )
                .into();
        }

        let value = SanT::san_policy_sanitize_numeric_value(value.into());

        self.set_last_known_io(value.into());

        value
    }

    fn port_set_numeric_value(&self, mut value: InnerT::ValueT, ctx: Option<&impl TfmExecCtx>)
    where
        BaseNumT: From<<InnerT as WithNumericValue>::ValueT>,
    {
        value = SanT::san_policy_sanitize_numeric_value(value);

        self.set_last_known_io(value.into());
        self.inner.set_numeric_value(self.inner.get_interval().map_from(
            value,
            &self.port_get_interval(),
            get_out_of_range_policy(
                self.port_get_relativity(),
                ctx.map(|ctx| ctx.is_reactive_mode()).unwrap_or(true),
            ),
        ));
    }

    fn port_get_default_interval_from_inner(&self) -> NumInterval<InnerT::ValueT> {
        InnerT::default().get_interval()
    }

    // fn _port_get_identity_str(&self) -> String {
    //     self.inner.value_identity()
    // }
    fn port_set_tfm_value_and_flush_to_devices(&self, input: TfmValueData<BaseNumT>, ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
        <Self::InnerT as WithNumericValue>::ValueT: From<BaseNumT>,
    {
        let value = tfm_value_port_writeback_praelude(
            WritebackPraeludeInput {
                from: input,
                to_interval: self.port_get_interval().cast::<BaseNumT>().unwrap(),
                to_relativity: self.port_get_relativity(),
            },
            ctx,
        );
        self.port_set_numeric_value_and_flush_to_devices(value.into(), ctx);
    }

    fn port_set_numeric_value_and_flush_to_devices(&self, value: InnerT::ValueT, ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
        <InnerT as WithNumericValue>::ValueT: From<BaseNumT>,
    {
        self.port_set_numeric_value(value, Some(ctx));
        self.port_flush_numeric_value_to_devices(ctx);
    }

    fn port_flush_numeric_value_to_devices(&self, ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
        <InnerT as WithNumericValue>::ValueT: From<BaseNumT>,
    {
        if let Some(dcm_ref) = self.get_device_control_matcher_ref() {
            #[allow(deprecated)]
            ctx.device_control_matcher_ref_write(dcm_ref, self.inner.get_numeric_value().into(), ctx);
        }
    }

    fn port_set_remap_off(&mut self) {
        self.remap_policy.remap_pol_off();
        self.sanitize_inplace(());
    }

    fn port_set_remap_from_inner_default(&mut self) {
        self.port_set_remap_interval(InnerT::default().get_interval());
        // self.sanitize_inplace(());
    }

    fn port_get_remap_interval(&self) -> Option<NumInterval<InnerT::ValueT>> {
        self.remap_policy.remap_pol_get_interval()
    }

    fn port_set_remap_interval(&mut self, interval: NumInterval<InnerT::ValueT>) {
        self.remap_policy.remap_pol_set_interval(interval);
        self.sanitize_inplace(());
    }

    fn port_inner_ref(&self) -> &Self::InnerT {
        &self.inner
    }

    fn port_inner_mut(&mut self) -> &mut Self::InnerT {
        &mut self.inner
    }

    fn port_get_interval(&self) -> NumInterval<<Self::InnerT as WithNumericValue>::ValueT> {
        self._port_effective_interval
    }

    fn port_get_relativity(&self) -> Relativity {
        self._port_effective_relativity
    }

    fn port_transformation_mut(&mut self) -> Option<&mut TfmSeqCfg> {
        self.tfm_policy.transformation_mut()
    }

    fn port_transformation_on(&mut self) {
        self.tfm_policy.transformation_on();
        self.sanitize_inplace(());
    }

    fn port_transformation_off(&mut self) {
        self.tfm_policy.transformation_off();
        self.sanitize_inplace(());
    }

    fn port_transformation_ref(&self) -> Option<&TfmSeqCfg> {
        self.tfm_policy.transformation_ref()
    }

    fn port_is_transformable(&self) -> bool {
        TfmT::is_transformable()
    }

    fn port_user_defined_remapping_allowed(&self) -> bool {
        self.remap_policy.remap_pol_user_defined_allowed()
    }
}

// -----------------------------

impl<InnerT, SanT, RemapT, TfmT> WithMappingTriggerPredicate for ValuePort<InnerT, SanT, RemapT, TfmT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    TfmT: TfmPolicyIface,
{
    type PredicateT = bool;

    fn _eval_triggers_mapping_pred(&self) -> bool {
        self.triggers_mapping
    }

    fn _set_triggers_mapping_pred(&mut self, pred: Self::PredicateT) {
        self.triggers_mapping = pred;
    }

    fn _get_triggers_mapping_pred(&mut self) -> Self::PredicateT {
        self.triggers_mapping
    }
}

impl<InnerT: ValueIface + WithNumIntervalSanitizerStatic + TfmPolicyDefaultChoice> WithNumIntervalSanitizerStatic
    for ValuePort<InnerT, SanPolicyUseFromPortInner>
where
    Self: WithNumIntervalSanitizerStatic,
    Self: WithNumericValue<ValueT = <InnerT as WithNumericValue>::ValueT>,
{
    fn sanitize_interval_static(interval: NumInterval<Self::ValueT>) -> NumInterval<Self::ValueT> {
        InnerT::sanitize_interval_static(interval)
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

        impl $crate::schemas_value_port::WithNumericValueSanitizerStatic for $name {
            fn sanitize_numeric_value_static(value: Self::ValueT) -> Self::ValueT {
                $value_sanitize(value)
            }

            fn get_value_sanitizer_policy_doc_str() -> &'static str {
                $sandoc
            }
        }

        impl $crate::schemas_value_port::WithNumIntervalSanitizerStatic for $name {
            fn sanitize_interval_static(mut interval: NumInterval<Self::ValueT>) -> NumInterval<Self::ValueT> {
                use $crate::schemas_value_port::WithNumericValueSanitizerStatic;
                interval.from = Self::sanitize_numeric_value_static(interval.from);
                interval.to = Self::sanitize_numeric_value_static(interval.to);
                interval
            }
        }


        impl $crate::schemas_value_port::TfmPolicyDefaultChoice for $name {
            type TfmPolicyT = $crate::schemas_value_port::TfmPolicyDisabled;
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new($inner_default)
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

        impl $crate::schemas_value::WithDeviceControlMatcherRef for $name {
            fn get_device_control_matcher_ref(&self) -> Option<&$crate::schemas_value::DeviceControlMatcherRef> {
                self.as_ref().get_device_control_matcher_ref()
            }
        }

        impl ::core::convert::From<$crate::schemas_value::ValueTargets> for $name  {
            fn from(value: $crate::schemas_value::ValueTargets) -> Self {
                $name::new($inner::from(value))
            }
        }

        impl $crate::schemas_value::ValueIface for $name {
            fn value_is_xrc_sink(&self) -> bool {
                self.as_ref().value_is_xrc_sink()
            }

            fn value_identity(&self) -> String {
                self.as_ref().value_identity()
            }

            fn value_is_static(&self) -> bool {
                self.as_ref().is_static()
            }

            fn value_is_void(&self) -> bool {
                self.as_ref().value_is_void()
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

        impl<SanT: $crate::schemas_value_port::PortSanPolicy<$name>> ::core::convert::From<ValuePort<$name, SanT>> for $name {
            fn from(port: ValuePort<$name, SanT>) -> Self {
                use $crate::schemas_value_port::ValuePortIface;
                port.port_inner_ref().clone()
            }
        }

        impl $crate::schemas_value::WithRelativity for $name {
            fn get_relativity(&self) -> $crate::relativity::Relativity {
                self.as_ref().get_relativity()
            }
        }

        impl $crate::schemas_value::WithLastKnownIO for $name {
            type LastKnownIOValueT = BaseNumT;

            fn get_last_known_io(&self) -> Self::LastKnownIOValueT {
                self.as_ref().get_last_known_io()
            }
        }

        impl $crate::schemas_value::WithNumericValue for $name {
            type ValueT = BaseNumT;

            fn get_numeric_value(&self) -> Self::ValueT {
                self.as_ref().get_numeric_value()
            }
        }

        impl $crate::schemas_value::WithNumInterval for $name {
            fn get_interval(&self) -> NumInterval<Self::ValueT> {
                self.as_ref().get_interval()
            }
        }

        impl $crate::schemas_value::WithNumericValueSettable for $name {
            fn set_numeric_value(&self, value: Self::ValueT) {
                self.as_ref().set_numeric_value(value);
            }
        }

        impl $crate::schemas_value::WithNumIntervalSettable for $name {
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
        $crate::make_port_inner_nutype!(
            name:     $name,
            inner:    ValueDsts,
            inner_default:  $inner_default,
            nutype_san: |s| { s },
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
        $crate::make_port_inner_nutype!(
            name:     $name,
            inner:    ValueSrcs,
            inner_default:  $inner_default,
            nutype_san: |mut value| {
                // Within the context of a value port the sanitized static value will guide associated interval bounds
                // (interval automatically expands to fit it).
                use $crate::schemas_value_port::WithNumIntervalSanitizerStatic;
                use $crate::schemas_value_port::WithNumericValueSanitizerStatic;
                use $crate::schemas_value::WithNumericValueSettable;
                if let ValueSrcs::Static(ref mut value) = value {
                    let mut interval_sanitized = Self::sanitize_interval_static(value.get_interval());
                    let default_interval = $inner_default.get_interval();
                    value.set_numeric_value(Self::sanitize_numeric_value_static(value.get_numeric_value()));
                    if interval_sanitized == default_interval ||
                            (value.interval.is_auto() && default_interval.contains_value_closed(value.get_numeric_value()))
                                                      /* when deserialized from single value format, the interval is set to
                                                        default interval (unit), which must be reset here to default making sense.*/ {
                        value.interval = AutoOrManual::Auto(default_interval);
                    } else {
                        let numeric_value = value.get_numeric_value();
                        if !default_interval.contains_value_closed(numeric_value) {
                            interval_sanitized = $crate::interval_grow_to_fit!(interval_sanitized, numeric_value);
                            value.set_numeric_value(interval_sanitized.clamp(numeric_value));
                        }
                        value.interval = AutoOrManual::Manual(interval_sanitized);
                    }
                }; value
            },
            value_sanitize: $value_sanitize,
            sandoc:         $sandoc
        );

        #[cfg(feature = "gui")]
        impl<'s> $crate::gui_common::DrawEgui<'s> for $name {
            type In = $crate::gui_value::GuiInValue<'s>;
            type Out = Option<$crate::gui_common::GuiCmd>;

            fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
                $crate::gui_value::draw_egui_for_a_value(self, gui_in, ui)
            }
        }
    };
}

// =================================================================

#[cfg(test)]
mod testing {
    use super::*;
    use crate::schemas_value::WithNumInterval;
    use crate::{
        num_interval::{UNIT_INTERVAL, ZERO_INTERVAL},
        schemas_control_matcher::ControlMatchers,
        schemas_value::{DeviceControlMatcherRef, DynValueRefs, ValueDsts, ValueSrcs, VariableRef},
        tfm_exec::TfmExecCtx,
    };
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
        p_san_epsilon_for_zero.port_set_numeric_value(100.0, None::<&()>);
        assert_eq!(
            p_san_epsilon_for_zero.port_get_numeric_value(None::<&()>),
            BaseNumT::EPSILON
        ); // At port level 0 is sanitized to epsilon
        p_san_epsilon_for_zero.port_set_numeric_value(-100.0, None::<&()>);
        assert_eq!(
            p_san_epsilon_for_zero.port_get_numeric_value(None::<&()>),
            BaseNumT::EPSILON
        );

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
                p.port_set_numeric_value(1.0, None::<&()>);
                assert_eq!(p.port_get_numeric_value(None::<&()>), 1.0);
                assert_eq!(p.port_inner_ref().get_numeric_value(), 1.0);
                p.port_set_numeric_value(-1.0, None::<&()>);
                assert_eq!(p.port_get_numeric_value(None::<&()>), BaseNumT::EPSILON);
            }

            {
                use std::ops::{Div, Mul};

                use crate::{
                    num_interval::{OutOfRangePolicy, SYMM_UNIT_INTERVAL},
                    test_utils::fp_approx_eq,
                };

                p_san_ge_epsilon.port_set_remap_interval(SYMM_UNIT_INTERVAL);
                // dbg!(&p_san_ge_epsilon.port_get_interval());
                p_san_ge_epsilon.port_set_numeric_value(-100.0, None::<&()>);

                assert_eq!(p_san_ge_epsilon.port_get_numeric_value(None::<&()>), BaseNumT::EPSILON);
                assert!(p_san_ge_epsilon.port_get_interval() == (BaseNumT::EPSILON..1.0).into());
                assert!(p_san_ge_epsilon.port_inner_ref().get_interval() == UNIT_INTERVAL);
                assert_eq!(
                    p_san_ge_epsilon.port_inner_ref().get_numeric_value(),
                    UNIT_INTERVAL.map_from(
                        BaseNumT::EPSILON,
                        &(BaseNumT::EPSILON..1.0).into(),
                        OutOfRangePolicy::Clamp
                    )
                );

                let mut p_no_san = ValuePort::<PortInnerDeviceSanEpsilonGtZero, SanPolicyNone>::default();
                p_no_san.port_set_remap_interval(SYMM_UNIT_INTERVAL);
                assert!(p_no_san.port_inner_ref().get_interval() == UNIT_INTERVAL);
                p_no_san.port_set_numeric_value(-0.5, None::<&()>);
                assert!(fp_approx_eq(p_no_san.port_get_numeric_value(None::<&()>), -0.5));
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
        }

        fn make_output_port_inner_dcm() -> ValueDsts {
            ValueDsts::Dynamic(make_device_control_matcher())
        }

        fn _make_input_port_inner_dcm() -> ValueSrcs {
            ValueSrcs::Dynamic(make_device_control_matcher())
        }

        dbg!(make_device_control_matcher());

        // ----------------------------------
        {
            make_output_port_inner_nutype!(
                PortInnerDeviceSanEpsilonGtZero,
                default: make_output_port_inner_dcm(),
                san-doc: "Value must be > 0.0",
                san-exe: |v: BaseNumT| { if v <= BaseNumT::zero() {BaseNumT::EPSILON} else {v}}
            );

            let mut port_to_device_san_gt_zero = ValuePort::<PortInnerDeviceSanEpsilonGtZero>::default();

            struct MockExeCtx {
                device_control_value_received: std::cell::Cell<BaseNumT>,
            }

            impl TfmExecCtx for MockExeCtx {
                fn device_control_matcher_ref_write(
                    &self,
                    _dcm_ref: &DeviceControlMatcherRef,
                    value: BaseNumT,
                    _ctx: &impl TfmExecCtx,
                ) {
                    self.device_control_value_received.set(value);
                }
            }

            let exe_ctx = MockExeCtx {
                device_control_value_received: Default::default(),
            };

            port_to_device_san_gt_zero.port_set_numeric_value(0.42, None::<&()>);
            assert!(exe_ctx.device_control_value_received.get() == BaseNumT::default());

            port_to_device_san_gt_zero.port_set_numeric_value_and_flush_to_devices(0.42, &exe_ctx);
            assert_eq!(port_to_device_san_gt_zero.port_get_numeric_value(None::<&()>), 0.42);
            assert_eq!(exe_ctx.device_control_value_received.get(), 0.42);

            port_to_device_san_gt_zero.port_set_numeric_value_and_flush_to_devices(-0.42, &exe_ctx);
            assert_eq!(
                port_to_device_san_gt_zero.port_get_numeric_value(None::<&()>),
                BaseNumT::EPSILON
            );
            assert_eq!(exe_ctx.device_control_value_received.get(), BaseNumT::EPSILON);

            port_to_device_san_gt_zero.port_set_remap_interval((-100.0..100.0).into());
            assert_eq!(
                port_to_device_san_gt_zero.port_get_remap_interval().unwrap(),
                (BaseNumT::EPSILON..100.0).into()
            );
            port_to_device_san_gt_zero.port_set_numeric_value_and_flush_to_devices(50.0, &exe_ctx);

            dbg!(&port_to_device_san_gt_zero);
            assert_eq!(port_to_device_san_gt_zero.port_get_numeric_value(None::<&()>), 50.0);
            assert_eq!(exe_ctx.device_control_value_received.get(), 0.5);
        }
    }
}

// -----------------------------
struct WritebackPraeludeInput {
    from: TfmValueData<BaseNumT>,
    to_interval: NumInterval<BaseNumT>,
    to_relativity: Relativity,
    /* TODO: writeback mode */
}

fn tfm_value_port_writeback_praelude(params: WritebackPraeludeInput, exe_ctx: &impl TfmExecCtx) -> BaseNumT {
    let mut value = params.from.value;

    if params.from.interval != params.to_interval {
        value = params.to_interval.map_from(
            value,
            &params.from.interval,
            get_out_of_range_policy(params.from.relativity, exe_ctx.is_reactive_mode()),
        );
    }

    if !exe_ctx.is_reactive_mode() && params.from.relativity.is_relative() && params.to_relativity.is_absolute() {
        value = params.to_interval.clamp(value);
    }

    value
}
