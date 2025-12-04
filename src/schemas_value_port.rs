use std::marker::PhantomData;

use crate::num_interval::{NumIntervalValue, OutOfRangePolicy};
use crate::schemas_transform::TfmSeqCfg;
use crate::schemas_value::{
    AutoOrManual, InputValueMetadata, TfmValue, ValueDsts, ValueIface, ValueSrcs, ValueXrcs,
    WithDeviceControlMatcherRef, WithLastKnownIO,
};
use crate::tfm_exec::{TfmExecCtx, WithTfmExec};
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

    fn port_set_numeric_value(&self, value: <Self::InnerT as WithNumericValue>::ValueT)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>;

    fn port_get_interval(&self) -> NumInterval<<Self::InnerT as WithNumericValue>::ValueT>;

    fn port_get_default_interval_from_inner(&self) -> NumInterval<<Self::InnerT as WithNumericValue>::ValueT>;
    fn _port_get_identity_str(&self) -> String;

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
        #[serde(skip_serializing_if = "Option::is_none")]
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
                remap,
                triggers_mapping,
                inner: value,
                _san_policy: PhantomData,
                _remap_policy: PhantomData,
                _port_effective_interval: Default::default(),
                _tfm_policy: {
                    let mut tfm_pol: TfmT = Default::default();
                    if let Some(tfm_cfg) = transformation {
                        tfm_pol.transformation_on();
                        if let Some(tfm) = tfm_pol.transformation_mut() {
                            *tfm = tfm_cfg;
                        };
                    }
                    tfm_pol.into()
                },
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
        if !port.inner.value_is_static() {
            ValuePortSerdeHelper::AsPort {
                remap: port.remap,
                triggers_mapping: port.triggers_mapping,
                value: port.inner,
                transformation: port._tfm_policy.transformation_ref().cloned(),
            }
        } else {
            ValuePortSerdeHelper::AsInner(port.inner)
        }
    }
}

// -------------------------------------------
pub(crate) trait PortSanPolicy<SanProviderT>:
    std::fmt::Debug + Copy + Clone + PartialOrd + PartialEq + 'static
{
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
    std::fmt::Debug + Copy + Clone + PartialOrd + PartialEq + 'static + Default
{
    #[inline(always)]
    fn get_remap_range() -> Option<NumInterval<ValueT>> {
        None
    }
}

#[derive(Copy, Clone, PartialEq, PartialOrd, Debug, Default)]
pub(crate) struct RemapPolicyUserDefined;
impl<ValueT: NumIntervalValue> PortRemapPolicy<ValueT> for RemapPolicyUserDefined {}

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
#[derive(JsonSchema, Debug, Clone, PartialOrd, PartialEq, Deserialize, Serialize, Traversable, TraversableMut)]
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
    inner: InnerT,
    #[traverse(skip)]
    remap: Option<NumInterval<<InnerT as WithNumericValue>::ValueT>>,
    #[serde(skip)]
    #[traverse(skip)]
    _san_policy: PhantomData<SanT>,
    #[serde(skip)]
    #[traverse(skip)]
    _remap_policy: PhantomData<RemapT>,
    _tfm_policy: Box<TfmT>,
    #[traverse(skip)]
    triggers_mapping: bool,
    #[serde(skip)]
    #[traverse(skip)]
    _port_effective_interval: NumInterval<<InnerT as WithNumericValue>::ValueT>,
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
            remap: Default::default(),
            triggers_mapping: Default::default(),
            inner: Default::default(),
            _san_policy: PhantomData,
            _remap_policy: PhantomData,
            _port_effective_interval: Default::default(),
            _tfm_policy: Default::default(),
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
            remap: None,
            triggers_mapping: false,
            inner: value,
            _san_policy: PhantomData,
            _remap_policy: PhantomData,
            _port_effective_interval: Default::default(),
            _tfm_policy: Default::default(),
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
            self.remap = None;
            self._port_effective_interval = self.inner.get_interval();
        } else {
            if let Some(remap) = &mut self.remap {
                remap.from = SanT::san_policy_sanitize_numeric_value(remap.from);
                remap.to = SanT::san_policy_sanitize_numeric_value(remap.to);
            }

            let inner_interval = self.port_inner_ref().get_interval();
            let inner_rel = self.port_inner_ref().get_relativity();

            if let Some(ref mut tfm) = self.port_transformation_mut() {
                let (_, _) = tfm.recompute_steps_metadata_get_out_interval_and_relativity(AutoOrManual::Auto(
                    InputValueMetadata {
                        interval: inner_interval.cast().unwrap(),
                        relativity: inner_rel,
                    },
                ));
            }

            self._port_effective_interval = self
                .remap
                .or(RemapT::get_remap_range().map(|r| r.cast().unwrap()))
                .or(self
                    .port_transformation_ref()
                    .as_ref()
                    .map(|tfm| tfm.get_out_interval().cast().unwrap()))
                .or(inner_interval.into())
                .unwrap()
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
        let mut value = self.inner.get_numeric_value();
        let post_tfm_value = if let Some(tfm) = self.port_transformation_ref() {
            if let Some(ctx) = ctx {
                tfm.exec(
                    TfmValue {
                        value: value.into(),
                        interval: self.inner.get_interval().cast().unwrap(),
                        relativity: self.inner.get_relativity(),
                    },
                    ctx,
                )
            } else {
                TfmValue {
                    value: tfm.get_last_known_io().1,
                    interval: self.inner.get_interval().cast().unwrap(),
                    relativity: self.inner.get_relativity(),
                }
            }
        } else {
            TfmValue {
                value: value.into(),
                interval: self.inner.get_interval().cast().unwrap(),
                relativity: self.inner.get_relativity(),
            }
        };

        if let Some(remap) = RemapT::get_remap_range().or(self.remap) {
            value = remap.map_from(post_tfm_value.value, &post_tfm_value.interval, OutOfRangePolicy::Clamp);
        }

        SanT::san_policy_sanitize_numeric_value(value)
    }

    fn port_set_numeric_value(&self, mut value: InnerT::ValueT) {
        value = SanT::san_policy_sanitize_numeric_value(value);

        self.inner.set_numeric_value(self.inner.get_interval().map_from(
            value,
            &self.port_get_interval(),
            OutOfRangePolicy::Clamp,
        ));
    }

    fn port_get_default_interval_from_inner(&self) -> NumInterval<InnerT::ValueT> {
        InnerT::default().get_interval()
    }

    fn _port_get_identity_str(&self) -> String {
        self.inner.value_identity()
    }

    fn port_set_numeric_value_and_flush_to_devices(&self, value: InnerT::ValueT, ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
        <InnerT as WithNumericValue>::ValueT: From<BaseNumT>,
    {
        self.port_set_numeric_value(value);
        self.port_flush_numeric_value_to_devices(ctx);
    }

    fn port_flush_numeric_value_to_devices(&self, ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
        <InnerT as WithNumericValue>::ValueT: From<BaseNumT>,
    {
        if let Some(dcm_ref) = self.get_device_control_matcher_ref() {
            #[allow(deprecated)]
            ctx.device_control_matcher_ref_write(dcm_ref, self.inner.get_numeric_value().into());
        }
    }

    fn port_set_remap_off(&mut self) {
        self.remap = None;
        self.sanitize_inplace(());
    }

    fn port_set_remap_from_inner_default(&mut self) {
        self.remap = Some(InnerT::default().get_interval());
        self.sanitize_inplace(());
    }

    fn port_get_remap_interval(&self) -> Option<NumInterval<InnerT::ValueT>> {
        self.remap
    }

    fn port_set_remap_interval(&mut self, ri: NumInterval<InnerT::ValueT>) {
        self.remap = Some(ri);
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

    fn port_transformation_mut(&mut self) -> Option<&mut TfmSeqCfg> {
        self._tfm_policy.transformation_mut()
    }

    fn port_transformation_on(&mut self) {
        self._tfm_policy.transformation_on();
        self.sanitize_inplace(());
    }

    fn port_transformation_off(&mut self) {
        self._tfm_policy.transformation_off();
        self.sanitize_inplace(());
    }

    fn port_transformation_ref(&self) -> Option<&TfmSeqCfg> {
        self._tfm_policy.transformation_ref()
    }

    fn port_is_transformable(&self) -> bool {
        TfmT::is_transformable()
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
            fn value_identity(&self) -> String {
                self.as_ref().value_identity()
            }
            fn value_is_static(&self) -> bool {
                self.as_ref().is_static()
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
                                                        default interval (unit), which must be reset here to default making sense.
                                                        TODO: need to enable interval optionality for static values and use
                                                        None in case of deserialization from single-value format*/ {
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
        p_san_epsilon_for_zero.port_set_numeric_value(100.0);
        assert_eq!(
            p_san_epsilon_for_zero.port_get_numeric_value(None::<&()>),
            BaseNumT::EPSILON
        ); // At port level 0 is sanitized to epsilon
        p_san_epsilon_for_zero.port_set_numeric_value(-100.0);
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
                p.port_set_numeric_value(1.0);
                assert_eq!(p.port_get_numeric_value(None::<&()>), 1.0);
                assert_eq!(p.port_inner_ref().get_numeric_value(), 1.0);
                p.port_set_numeric_value(-1.0);
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
                p_san_ge_epsilon.port_set_numeric_value(-100.0);

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
                p_no_san.port_set_numeric_value(-0.5);
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
                fn device_control_matcher_ref_write(&self, _dcm_ref: &DeviceControlMatcherRef, value: BaseNumT) {
                    self.device_control_value_received.set(value);
                }
            }

            let exe_ctx = MockExeCtx {
                device_control_value_received: Default::default(),
            };

            port_to_device_san_gt_zero.port_set_numeric_value(0.42);
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
