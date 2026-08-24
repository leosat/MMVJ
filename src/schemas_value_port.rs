use std::marker::PhantomData;

use crate::num_interval::{NumIntervalValue, OutOfRangePolicy};
use crate::schemas_value::{ValueIface, WithDeviceControlMatcherRef};
use crate::tfm_exec::TfmExecCtx;
use crate::{
    config::WithSelfSanitize,
    schemas_value::{WithNumericValueSettable, WithRelativity},
};
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
    WithMappingTriggerPredicate
    + WithSelfSanitize
    + WithNumericValue
    + WithNumericValueSettable
    + WithDeviceControlMatcherRef
{
    type InnerT: ValueIface;
    type RemapT: PortRemapPolicy<Self::ValueT>;
    type SanT: PortSanPolicy<Self::InnerT>;

    fn port_get_default_interval_from_inner(&self) -> NumInterval<Self::ValueT>;
    fn _port_get_identity_str(&self) -> String;

    fn port_set_numeric_value_and_flush_to_devices(&self, value: Self::ValueT, cxt: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>;
    fn port_flush_numeric_value_to_devices(&self, cxt: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>;

    fn port_set_remap_off(&mut self);
    fn port_set_remap_from_inner_default(&mut self);
    fn port_get_remap_interval(&self) -> Option<NumInterval<Self::ValueT>>;
    fn port_set_remap_interval(&mut self, ri: NumInterval<Self::ValueT>);

    fn port_inner_ref(&self) -> &Self::InnerT;
    fn port_inner_mut(&mut self) -> &mut Self::InnerT;
}

impl<InnerT, SanT, RemapT> WithDeviceControlMatcherRef for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    fn get_device_control_matcher_ref(&self) -> Option<&crate::schemas_value::DeviceControlMatcherRef> {
        self.value.get_device_control_matcher_ref()
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
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(alias = "remap_from_interval", alias = "remap_to_interval")]
        remap: Option<NumInterval<InnerT::ValueT>>,
        #[serde(skip_serializing_if = "crate::schemas_common::is_false")]
        #[serde(default)]
        triggers_mapping: bool,
        //#[serde(flatten)] // Fails when inner is serialized to a single number.
        #[serde(alias = "source", alias = "destination", alias = "target")]
        value: InnerT,
    },
    AsInner(InnerT),
}

impl<InnerT, SanT, RemapT> From<ValuePortSerdeHelper<InnerT>> for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    fn from(value: ValuePortSerdeHelper<InnerT>) -> Self {
        match value {
            ValuePortSerdeHelper::AsInner(i) => i.into(),
            ValuePortSerdeHelper::AsPort {
                remap,
                triggers_mapping,
                value,
            } => Self {
                remap,
                triggers_mapping,
                value,
                _san_policy: PhantomData,
                _remap_policy: PhantomData,
            },
        }
    }
}

impl<InnerT, SanT, RemapT> From<ValuePort<InnerT, SanT, RemapT>> for ValuePortSerdeHelper<InnerT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    fn from(port: ValuePort<InnerT, SanT, RemapT>) -> Self {
        if !port.value.value_is_static() {
            ValuePortSerdeHelper::AsPort {
                remap: port.remap,
                triggers_mapping: port.triggers_mapping,
                value: port.value,
            }
        } else {
            ValuePortSerdeHelper::AsInner(port.value)
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
#[derive(JsonSchema, Debug, Clone, PartialOrd, PartialEq, Deserialize, Serialize)]
#[serde(from = "ValuePortSerdeHelper<InnerT>", into = "ValuePortSerdeHelper<InnerT>")]
#[serde(bound(serialize = "
    InnerT: ValueIface,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    ValuePort<InnerT, SanT, RemapT>: WithNumericValue<ValueT = <InnerT as WithNumericValue>::ValueT>, 
    <InnerT as WithNumericValue>::ValueT: serde::Serialize
"))]
#[serde(bound(deserialize = "
    InnerT: ValueIface,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    ValuePort<InnerT, SanT, RemapT>: WithNumericValue<ValueT = <InnerT as WithNumericValue>::ValueT>, 
    <InnerT as WithNumericValue>::ValueT: serde::Deserialize<'de>
"))]
pub(crate) struct ValuePort<InnerT, SanT = SanPolicyUseFromPortInner, RemapT = RemapPolicyUserDefined>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    pub(super) remap: Option<NumInterval<<InnerT as WithNumericValue>::ValueT>>,
    pub(super) triggers_mapping: bool,
    pub(super) value: InnerT,
    #[serde(skip)]
    _remap_policy: PhantomData<RemapT>,
    #[serde(skip)]
    pub(super) _san_policy: PhantomData<SanT>,
}

// ----------------------------------------------

impl<InnerT, SanT, RemapT> Traversable for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface + Traversable,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    fn traverse<V: traversable::Visitor>(&self, visitor: &mut V) -> std::ops::ControlFlow<V::Break> {
        self.value.traverse(visitor)
    }
}

impl<InnerT, SanT, RemapT> TraversableMut for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface + TraversableMut,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    fn traverse_mut<V: traversable::VisitorMut>(&mut self, visitor: &mut V) -> std::ops::ControlFlow<V::Break> {
        self.value.traverse_mut(visitor)
    }
}

// ----------------------------------------------
impl<InnerT, SanT, RemapT> Default for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    fn default() -> Self {
        Self {
            remap: Default::default(),
            triggers_mapping: Default::default(),
            value: Default::default(),
            _san_policy: PhantomData,
            _remap_policy: PhantomData,
        }
    }
}

// ----------------------------------------------

impl<InnerT, SanT, RemapT> From<InnerT> for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    Self: WithNumericValue,
{
    fn from(value: InnerT) -> Self {
        Self {
            remap: None,
            triggers_mapping: false,
            value,
            _san_policy: PhantomData,
            _remap_policy: PhantomData,
        }
    }
}

impl<InnerT, SanT, RemapT> WithNumericValueSettable for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface,
    InnerT: WithNumericValueSanitizerStatic,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
    Self: WithNumericValue<ValueT = <InnerT as WithNumericValue>::ValueT>,
{
    fn set_numeric_value(&self, mut value: <Self as WithNumericValue>::ValueT) {
        value = SanT::san_policy_sanitize_numeric_value(value);
        if let Some(remap) = RemapT::get_remap_range().or(self.remap) {
            value = self
                .value
                .get_interval()
                .map_from(value, &remap, OutOfRangePolicy::Clamp);
        }
        self.value.set_numeric_value(value);
    }
}

impl<InnerT, SanT, RemapT> WithNumericValue for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface + WithNumericValueSanitizerStatic,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    type ValueT = <InnerT as WithNumericValue>::ValueT;
    fn get_numeric_value(&self) -> Self::ValueT {
        let mut value = self.value.get_numeric_value();
        if let Some(remap) = RemapT::get_remap_range().or(self.remap) {
            value = remap.map_from(value, &self.value.get_interval(), OutOfRangePolicy::Clamp);
        }
        SanT::san_policy_sanitize_numeric_value(value)
    }
}

impl<InnerT, SanT, RemapT> Bounds for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
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

// -----------------------------

impl<InnerT, SanT, RemapT> WithSelfSanitize for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    fn sanitize_inplace(&mut self) {
        if self.value.value_is_static() {
            self.remap = None;
        } else {
            if let Some(remap) = &mut self.remap {
                remap.from = SanT::san_policy_sanitize_numeric_value(remap.from);
                remap.to = SanT::san_policy_sanitize_numeric_value(remap.to);
            }
        }
    }
}

impl<InnerT, SanT, RemapT> ValuePortIface for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
{
    type InnerT = InnerT;
    type RemapT = RemapT;
    type SanT = SanT;

    fn port_get_default_interval_from_inner(&self) -> NumInterval<Self::ValueT> {
        InnerT::default().get_interval()
    }

    fn _port_get_identity_str(&self) -> String {
        self.value.value_identity()
    }

    fn port_set_numeric_value_and_flush_to_devices(&self, value: Self::ValueT, ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
    {
        self.set_numeric_value(value);
        self.port_flush_numeric_value_to_devices(ctx);
    }

    fn port_flush_numeric_value_to_devices(&self, ctx: &impl TfmExecCtx)
    where
        BaseNumT: From<<Self::InnerT as WithNumericValue>::ValueT>,
    {
        if let Some(dcm_ref) = self.get_device_control_matcher_ref() {
            #[allow(deprecated)]
            ctx.device_control_matcher_ref_write(dcm_ref, self.get_numeric_value().into());
        }
    }

    fn port_set_remap_off(&mut self) {
        self.remap = None
    }

    fn port_set_remap_from_inner_default(&mut self) {
        self.remap = Some(InnerT::default().get_interval())
    }

    fn port_get_remap_interval(&self) -> Option<NumInterval<Self::ValueT>> {
        self.remap
    }

    fn port_set_remap_interval(&mut self, ri: NumInterval<Self::ValueT>) {
        self.remap = Some(ri);
    }

    fn port_inner_ref(&self) -> &Self::InnerT {
        &self.value
    }

    fn port_inner_mut(&mut self) -> &mut Self::InnerT {
        &mut self.value
    }
}

// -----------------------------

impl<InnerT, SanT, RemapT> WithMappingTriggerPredicate for ValuePort<InnerT, SanT, RemapT>
where
    InnerT: ValueIface,
    SanT: PortSanPolicy<InnerT>,
    RemapT: PortRemapPolicy<InnerT::ValueT>,
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

impl<InnerT: ValueIface + WithNumIntervalSanitizerStatic> WithNumIntervalSanitizerStatic
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
                port.value
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
        }

        fn make_output_port_inner_dcm() -> ValueDsts {
            ValueDsts::Dynamic(make_device_control_matcher())
        }

        fn _make_input_port_inner_dcm() -> ValueSrcs {
            ValueSrcs::Dynamic(make_device_control_matcher())
        }

        // ----------------------------------
        {
            make_output_port_inner_nutype!(
                PortInnerDeviceSanEpsilonGtZero,
                default: make_output_port_inner_dcm(),
                san-doc: "Value must be > 0.0",
                san-exe: |v: BaseNumT| { if v <= BaseNumT::zero() {BaseNumT::EPSILON} else {v}}
            );

            let port_to_device_san_gt_zero = ValuePort::<PortInnerDeviceSanEpsilonGtZero>::default();

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

            port_to_device_san_gt_zero.set_numeric_value(42.0);
            assert!(exe_ctx.device_control_value_received.get() == BaseNumT::default());

            port_to_device_san_gt_zero.port_set_numeric_value_and_flush_to_devices(42.0, &exe_ctx);
            assert_eq!(port_to_device_san_gt_zero.get_numeric_value(), 42.0);
            assert_eq!(exe_ctx.device_control_value_received.get(), 42.0);

            port_to_device_san_gt_zero.port_set_numeric_value_and_flush_to_devices(-42.0, &exe_ctx);
            assert_eq!(port_to_device_san_gt_zero.get_numeric_value(), BaseNumT::EPSILON);
            assert_eq!(exe_ctx.device_control_value_received.get(), BaseNumT::EPSILON);
        }
    }
}
