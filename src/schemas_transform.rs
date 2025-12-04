use crate::base_num::{BaseAtomicT, BaseNumT};
use crate::config::WithSanitize;
use crate::filters::OneEuroFilter;
use crate::relativity::Relativity;
use crate::schemas_value::{DescriptionCfg, InputValueMetadata, WithDescriptionMut, make_static_value_src};
use crate::schemas_value::{
    DeviceControlMatcherRef, DynValueRefs, ValueDsts, VariableRef, WithNumInterval, WithRelativityRef,
    serialize_value_src_rt_ignore_interval,
};
use crate::tfm_exec::{IntegrateExeState, RaiseFallExeState, ScriptExeState, SteeringExeState, TfmExeState};
use crate::{
    num_interval::NumInterval,
    num_interval::{SYMM_UNIT_INTERVAL, UNIT_INTERVAL},
    schemas_common::*,
    schemas_value::{StaticValueCfg, ValueSrcs},
    tracing::TraceChannel,
};
use ambassador::{Delegate, delegatable_trait};
use atomic_float::AtomicF32;
use bitflags::bitflags;
use crossbeam_utils::CachePadded;
// use documented::{Documented, DocumentedFields, docs_const};
use garde::Validate;
use schemars::JsonSchema;
use serde::de::IntoDeserializer;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
#[cfg(feature = "gui")]
use std::sync::atomic::AtomicBool;
use strum_macros::{Display, EnumIter, EnumString};
use traversable::Traversable;
use traversable::TraversableMut;

// =================================================
const fn default_step_enabled() -> bool {
    true
}

const fn default_on_idle() -> bool {
    true
}

const fn default_ff_gain() -> BaseNumT {
    1.0
}

const fn default_1euro_beta() -> ValueSrcs {
    make_static_value_src(0.007, UNIT_INTERVAL)
}

const fn default_1euro_d_cutoff_hz() -> ValueSrcs {
    make_static_value_src(1.0, NumInterval { from: 1e-6, to: 30.0 })
}

const fn default_1euro_min_cutoff_hz() -> ValueSrcs {
    make_static_value_src(1.0, NumInterval { from: 1e-6, to: 30.0 })
}

const fn default_clamp_transform_override_interval() -> bool {
    true
}

const fn default_steering_smoothing_alpha() -> BaseNumT {
    0.33
}

const fn default_steering_transform_auto_center_halflife() -> BaseNumT {
    0.3
}

const fn default_smoothing_alpha() -> BaseNumT {
    1.0
}

const fn default_linear_slope() -> BaseNumT {
    1.0
}

const fn default_scurve_steepness() -> BaseNumT {
    10.
}

pub(crate) const fn default_norm_exp_base() -> BaseNumT {
    1.001
}

const fn default_ema_tau() -> ValueSrcs {
    // InputPort {
    //     src,
    //     remap_to_interval: None,
    //     clamp_to_interval: Some(NumInterval {
    //         from: 1.0e-6,
    //         to: 100.0,
    //     }),
    //     triggers_mapping: false,
    // }
    ValueSrcs::Static(StaticValueCfg {
        value: std::cell::Cell::new(0.04),
        interval: AutoOrManual::Auto(NumInterval { from: 1.0e-6, to: 2.0 }),
    })
}

pub(crate) trait DuplicateWithNewState
where
    Self: Clone + TraversableMut + WithRuntimeId,
{
    fn duplicate_with_new_state(&self) -> Self {
        struct Visitor {}
        impl traversable::VisitorMut for Visitor {
            type Break = ();
            fn enter_mut(&mut self, this: &mut dyn core::any::Any) -> std::ops::ControlFlow<Self::Break> {
                if let Some(v) = this.downcast_mut::<TfmStepCfg>() {
                    v.common_state_assign_new();
                    match v {
                        TfmStepCfg::Integrate(s) => s.exe_state = Default::default(),
                        TfmStepCfg::Steering(s) => s.exe_state = Default::default(),
                        TfmStepCfg::RaiseFall(s) => s.exe_state = Default::default(),
                        TfmStepCfg::Ema(s) => s.exe_state = Default::default(),
                        TfmStepCfg::OneEuro(s) => s.exe_state = Default::default(),
                        TfmStepCfg::Script(s) => s.exe_state = Default::default(),
                        TfmStepCfg::Nop(_) | TfmStepCfg::Invert(_) | TfmStepCfg::Clamp(_) | TfmStepCfg::Linear(_) => {}
                        TfmStepCfg::Smoothstep(_) | TfmStepCfg::SCurve(_) => {}
                        TfmStepCfg::Exp(_) | TfmStepCfg::SignedPower(_) => {}
                        TfmStepCfg::_HighPass(_s) => {}
                        TfmStepCfg::_ForceFeedback(_s) => {}
                    }
                } else if let Some(v) = this.downcast_mut::<TfmSeqCfg>() {
                    v.assign_new_id();
                }
                std::ops::ControlFlow::Continue(())
            }
        }
        let mut duplicate = self.clone();
        let _ = duplicate.traverse_mut(&mut Visitor {});
        duplicate
    }
}

// ============================================================
#[derive(Clone)]
pub(crate) struct TfmStepCommonState {
    id: ObjId,
    intervals: (NumInterval<BaseNumT>, NumInterval<BaseNumT>),
    relativity: (Relativity, Relativity),
    #[cfg(feature = "gui")]
    pub(crate) gui_trace_graph_opened: Arc<AtomicBool>,
    pub(crate) last_in: Arc<CachePadded<BaseAtomicT>>,
    pub(crate) last_out: Arc<CachePadded<BaseAtomicT>>,
    pub(crate) trace_channel: Option<Arc<TraceChannel>>,
}

impl Default for TfmStepCommonState {
    fn default() -> Self {
        Self {
            id: Default::default(),
            intervals: (NumInterval::default(), NumInterval::default()),
            relativity: (Relativity::Abs, Relativity::Abs),
            #[cfg(feature = "gui")]
            gui_trace_graph_opened: Default::default(),
            trace_channel: None,
            last_in: Default::default(),
            last_out: Default::default(),
        }
    }
}

impl WithRuntimeId for TfmStepCommonState {
    fn get_id(&self) -> ObjId {
        self.id
    }
    fn assign_new_id(&mut self) {
        self.id = Default::default()
    }
}

// ===========================================================
impl TfmStepCommonState {
    #[allow(unused)]
    pub(crate) fn is_in_relative(&self) -> bool {
        self.relativity.0.into()
    }

    pub(crate) fn is_out_relative(&self) -> bool {
        self.relativity.1.into()
    }

    pub(crate) fn set_input_relativity(&mut self, is_relative: Relativity) -> &mut Self {
        self.relativity.0 = is_relative;
        self
    }

    pub(crate) fn set_output_relativity(&mut self, is_relative: Relativity) -> &mut Self {
        self.relativity.1 = is_relative;
        self
    }

    pub(crate) fn set_input_interval(&mut self, interval: NumInterval<BaseNumT>) -> &mut Self {
        self.intervals.0 = interval;
        self
    }

    pub(crate) fn set_output_interval(&mut self, interval: NumInterval<BaseNumT>) -> &mut Self {
        self.intervals.1 = interval;
        self
    }

    pub(crate) fn get_in_interval(&self) -> NumInterval<BaseNumT> {
        self.intervals.0
    }

    #[allow(unused)]
    pub(crate) fn get_out_interval(&self) -> NumInterval<BaseNumT> {
        self.intervals.1
    }

    #[allow(unused)]
    pub(crate) fn make_with_io_intervals(
        in_interval: NumInterval<BaseNumT>,
        out_interval: NumInterval<BaseNumT>,
    ) -> Self {
        Self {
            intervals: (in_interval, out_interval),
            ..Default::default()
        }
    }
}

impl std::fmt::Debug for TfmStepCommonState {
    #[cfg(feature = "gui")]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TfmStepState")
            .field("trace_graph_opened", &self.gui_trace_graph_opened)
            .field("id", &self.id)
            // .field("trace_channel", &self.trace_channel)
            .finish()
    }
    #[cfg(not(feature = "gui"))]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TfmStepState").finish()
    }
}

// ============================================================
impl Default for TfmStepCfg {
    fn default() -> Self {
        TfmStepCfg::Nop(Default::default())
    }
}

// #[derive(Debug, Clone, Default)]
// pub(crate) struct TfmStepCommonStateShared(pub(crate) Arc<RwLock<TfmStepCommonState>>);

pub type TfmStepCommonStateShared = TfmStepCommonState;

impl PartialEq for TfmStepCommonStateShared {
    fn eq(&self, other: &Self) -> bool {
        self.get_id() == other.get_id()
    }
}

#[derive(
    JsonSchema,
    Display,
    Debug,
    Serialize,
    Deserialize,
    EnumString,
    EnumIter,
    Traversable,
    TraversableMut,
    Clone,
    PartialEq,
    Validate,
    Delegate,
)]
#[delegate(WithCommonState)]
#[delegate(WithCommonStateMut)]
#[delegate(WithCommonStateAssignedNew)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub(crate) enum TfmStepCfg {
    #[traverse(skip)]
    Nop(#[garde(skip)] NopCfg),
    #[traverse(skip)]
    Invert(#[garde(skip)] InvertCfg),
    #[traverse(skip)]
    Integrate(#[garde(skip)] IntegrateCfg),
    Steering(#[garde(skip)] Box<SteeringCfg>),
    #[traverse(skip)]
    Clamp(#[garde(skip)] ClampCfg),
    RaiseFall(#[garde(skip)] Box<RaiseFallCfg>),
    Ema(#[garde(skip)] EmaFilterCfg),
    #[traverse(skip)]
    Linear(#[garde(skip)] LinearCfg),
    #[traverse(skip)]
    Smoothstep(#[garde(skip)] SmoothstepCfg),
    #[traverse(skip)]
    #[serde(alias = "s_curve")]
    SCurve(#[garde(skip)] SCurveCfg),
    #[traverse(skip)]
    Exp(#[garde(skip)] NormExpCfg),
    #[traverse(skip)]
    SignedPower(#[garde(skip)] SignedPowerCfg),
    OneEuro(#[garde(skip)] Box<OneEuroFilterCfg>),
    Script(#[garde(skip)] ScriptCfg),
    #[strum(disabled)]
    #[traverse(skip)]
    _HighPass(#[garde(skip)] HighPassCfg),
    #[strum(disabled)]
    #[traverse(skip)]
    _ForceFeedback(#[garde(skip)] Box<ForceFeedbackCfg>),
}

pub(crate) const DEFAULT_TRANSFORM_DESCRIPTION: &str = "No transform description available... yet.";

impl TfmStepCfg {
    pub(crate) const fn doc_str(&self) -> &'static str {
        match self {
            TfmStepCfg::Steering(s) => s.doc_str(),
            TfmStepCfg::Script(s) => s.doc_str(),
            TfmStepCfg::OneEuro(s) => s.doc_str(),
            TfmStepCfg::Ema(s) => s.doc_str(),
            TfmStepCfg::Nop(_)
            | TfmStepCfg::Invert(_)
            | TfmStepCfg::Integrate(_)
            | TfmStepCfg::Clamp(_)
            | TfmStepCfg::RaiseFall(_)
            | TfmStepCfg::Linear(_)
            | TfmStepCfg::Smoothstep(_)
            | TfmStepCfg::SCurve(_)
            | TfmStepCfg::Exp(_)
            | TfmStepCfg::SignedPower(_)
            | TfmStepCfg::_HighPass(_)
            | TfmStepCfg::_ForceFeedback(_) => DEFAULT_TRANSFORM_DESCRIPTION,
        }
    }
}

impl TfmStepCfg {
    pub(crate) fn get_enabled_ref_mut(&mut self) -> &mut bool {
        match self {
            Self::Nop(s) => &mut s.enabled,
            Self::Invert(s) => &mut s.enabled,
            Self::Integrate(s) => &mut s.enabled,
            Self::Steering(s) => &mut s.enabled,
            Self::Clamp(s) => &mut s.enabled,
            Self::RaiseFall(s) => &mut s.enabled,
            Self::Ema(s) => &mut s.enabled,
            Self::Linear(s) => &mut s.enabled,
            Self::Smoothstep(s) => &mut s.enabled,
            Self::SCurve(s) => &mut s.enabled,
            Self::Exp(s) => &mut s.enabled,
            Self::SignedPower(s) => &mut s.enabled,
            Self::OneEuro(s) => &mut s.enabled,
            Self::Script(s) => &mut s.enabled,
            Self::_HighPass(s) => &mut s.enabled,
            Self::_ForceFeedback(s) => &mut s.enabled,
        }
    }

    pub(crate) fn clone_with_new_state_no_recurse(&self) -> Self {
        let mut cloned = self.clone();
        cloned.common_state_assign_new();
        cloned
    }
}

// ===========================================================
#[derive(
    JsonSchema,
    Display,
    EnumIter,
    Debug,
    Copy,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Traversable,
    TraversableMut,
)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ForceFeedbackComponent {
    #[default]
    X,
    Y,
    // XY,
}

use with_doc_str::with_doc_str;

/// Force-feedback configuration for the steering transform.
///
/// Controls how FFB forces from the game (read via the virtual HID device)
/// are filtered, scaled, and applied as positional offsets to the emulated
/// steering wheel.
#[derive(
    // Documented,
    // DocumentedFields,
    JsonSchema,
    Debug,
    Clone,
    Traversable,
    TraversableMut,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Validate,
)]
#[serde(deny_unknown_fields)]
#[with_doc_str]
pub(crate) struct ForceFeedbackCfg {
    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    /// Internal state
    pub common_state: TfmStepCommonStateShared,

    /// Optional human-readable description.
    #[traverse(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,

    /// Enable/disable FFB processing. When `false`, no force is applied.
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,

    /// Multiplier applied to the (optionally filtered) FFB force.
    ///
    /// - `1.0` (default): force applied as-is.
    /// - `> 1.0`: amplifies FFB effect.
    /// - `< 1.0`: dampens FFB effect.
    /// - `0.0`: effectively disables FFB without removing the config block.
    #[serde(default = "default_ff_gain")]
    #[garde(range(min = 0.0))]
    pub(crate) gain: BaseNumT,

    /// Flips the sign of the FFB force.
    ///
    /// Use when the wheel turns the wrong way in response to game forces
    /// (e.g. assists the turn instead of resisting it).
    #[serde(default)]
    #[garde(skip)]
    pub(crate) invert: bool,

    /// Selects which FFB axis to read from the virtual device.
    ///
    /// - `X` (default): primary steering axis.
    /// - `Y`: secondary axis / separate effect channel.
    #[serde(default)]
    #[garde(skip)]
    pub(crate) component: ForceFeedbackComponent,

    /// Sub-pipeline applied to the raw FFB signal **before** `gain` and
    /// `invert`.
    ///
    /// Receives the force in [-1, +1] with relative semantics.
    /// Typical uses: `ema`/`one_euro` smoothing, `clamp` for peak
    /// limiting, curves for reshaping the force response.
    #[serde(default)]
    #[garde(skip)]
    pub(crate) transformation: TfmSeqCfg,

    /// Overrides the default FFB source.
    ///
    /// Instead of reading force from the destination virtual device's
    /// internal FFB state, reads from this arbitrary `ValueSrc` (variable,
    /// device control, etc.). The value is mapped from the source's
    /// interval to [-1, +1].
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub(crate) custom_source: Option<ValueSrcs>,
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClampCfgCompat__ {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) range: Option<NumInterval<BaseNumT>>,
    #[serde(skip_serializing)]
    #[serde(rename = "from")]
    from_deprecated__: Option<BaseNumT>,
    #[serde(skip_serializing)]
    #[serde(rename = "to")]
    to_deprecated__: Option<BaseNumT>,
    #[serde(default = "default_clamp_transform_override_interval")]
    pub(crate) override_range: bool,
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
#[serde(from = "ClampCfgCompat__")]
pub(crate) struct ClampCfg {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) range: NumInterval<BaseNumT>,
    #[serde(default = "default_clamp_transform_override_interval")]
    pub(crate) override_range: bool,
}

impl From<ClampCfgCompat__> for ClampCfg {
    fn from(value: ClampCfgCompat__) -> Self {
        let err =
            "Clamp transform parse error: full range must be specified either with range: ... or from: ... and to: ...";
        Self {
            common_state: value.common_state,
            desc: value.desc,
            enabled: value.enabled,
            range: NumInterval::new(
                value.from_deprecated__.unwrap_or_else(|| value.range.expect(err).from),
                value.to_deprecated__.unwrap_or_else(|| value.range.expect(err).to),
            ),
            override_range: value.override_range,
        }
    }
}

impl WithSanitize for ClampCfg {
    fn sanitize_inplace(&mut self) {
        let mut clamping_interval = self.get_clamping_interval();
        let in_interval = self.get_in_interval();
        let clamping_interval_saved = clamping_interval;
        clamping_interval.from = in_interval.clamp(clamping_interval.from);
        clamping_interval.to = in_interval.clamp(clamping_interval.to);
        if clamping_interval_saved != clamping_interval {
            log::warn!(
                "Sanitizing clamp transform: clamping interval {clamping_interval_saved:?} \
                was not fully contained within input interval {in_interval:?}, converted it to {clamping_interval:?}"
            );
            self.range = clamping_interval;
        }
    }
}

impl ClampCfg {
    pub(crate) fn get_clamping_interval(&self) -> NumInterval<BaseNumT> {
        self.range
    }
    pub(crate) fn get_out_interval(&self) -> NumInterval<BaseNumT> {
        if self.override_range {
            self.get_clamping_interval()
        } else {
            self.get_in_interval()
        }
    }
    pub(crate) fn get_in_interval(&self) -> NumInterval<BaseNumT> {
        self.common_state_ref().intervals.0
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[serde(from = "bool", into = "bool")]
pub(crate) struct NopCfg {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[garde(skip)]
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,
}

impl Default for NopCfg {
    fn default() -> Self {
        Self {
            common_state: Default::default(),
            enabled: default_step_enabled(),
        }
    }
}

impl From<NopCfg> for bool {
    fn from(value: NopCfg) -> Self {
        value.enabled
    }
}

impl From<bool> for NopCfg {
    fn from(value: bool) -> Self {
        Self {
            common_state: Default::default(),
            enabled: value,
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvertCfg {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[garde(skip)]
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,
}

impl Default for InvertCfg {
    fn default() -> Self {
        Self {
            common_state: Default::default(),
            enabled: default_step_enabled(),
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, Validate, Traversable, TraversableMut)]
#[serde(deny_unknown_fields)]
#[with_doc_str]
/// An Exponential Moving Average (EMA) filter is a recursive lowpass filter.
/// It reduces noise in real-time data by giving more weight to recent data points.
/// It reacts faster to sudden changes than a standard moving average
/// while smoothing out minor fluctuations.
///
/// a = 1 - (-dt / tau).exp()
/// y[i] = a * x[i] + (1 - a) * y[i-1]
pub(crate) struct EmaFilterCfg {
    /// ...
    #[traverse(skip)]
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[traverse(skip)]
    #[serde(skip)]
    #[garde(skip)]
    /// ...
    pub(super) exe_state: Arc<std::sync::Mutex<crate::filters::EmaFilter>>,
    #[traverse(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    /// ...
    pub(crate) desc: DescriptionCfg,
    #[traverse(skip)]
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    /// ...
    pub(crate) enabled: bool,
    #[traverse(skip)]
    #[serde(default = "default_false")]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    /// ...
    pub(crate) on_relative_input_feed_on_idle: bool,
    #[traverse(skip)]
    #[serde(default = "default_false")]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    pub(crate) on_relative_input_reset_on_idle: bool,
    #[traverse(skip)]
    #[serde(default = "default_ema_tau")]
    #[garde(range(min = 0.0))]
    /// The time constant
    /// Defines the duration required for the filter's step response
    /// to reach about 63.2% (1 - 1/e) of its final steady-state value.
    pub(crate) tau: ValueSrcs, // InputPort,
}

impl PartialEq for EmaFilterCfg {
    fn eq(&self, other: &Self) -> bool {
        self.desc == other.desc
            && self.enabled == other.enabled
            && self.on_relative_input_feed_on_idle == other.on_relative_input_feed_on_idle
            && self.on_relative_input_reset_on_idle == other.on_relative_input_reset_on_idle
            && self.tau == other.tau
    }
}

impl Default for EmaFilterCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            on_relative_input_feed_on_idle: default_false(),
            tau: default_ema_tau(), /*InputPort {
                                        src: 0.01.into(),
                                        remap_to_interval: None,
                                        clamp_to_interval: NumInterval::new(1e-6, 100.0).into(),
                                        triggers_mapping: false,
                                    }*/
            on_relative_input_reset_on_idle: default_false(),
            desc: Default::default(),
            common_state: Default::default(),
            exe_state: Default::default(),
        }
    }
}

/// Adaptive 1€ (One-Euro) low-pass filter transform step.
///
/// Implements the [1€ Filter](https://cristal.univ-lille.fr/~casiez/1euro/)
/// algorithm — a speed-adaptive low-pass filter that dynamically adjusts
/// its cutoff frequency based on the rate of change of the input signal.
///
/// - **Slow / stationary input** -> cutoff ~= `min_cutoff_hz` -> heavy
///   smoothing, jitter suppressed.
/// - **Fast input** -> cutoff ramps up via `β · |dx|` -> light smoothing,
///   low latency.
///
/// Ideal for smoothing noisy relative inputs (mouse,
/// trackball) before a steering step, as a final output smoother
/// after steering, or inside a force-feedback sub-pipeline.
#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, Validate, Traversable, TraversableMut)]
#[serde(deny_unknown_fields)]
#[with_doc_str]
pub(crate) struct OneEuroFilterCfg {
    #[serde(skip)]
    #[garde(skip)]
    #[traverse(skip)]
    common_state: TfmStepCommonStateShared,

    #[serde(skip)]
    #[garde(skip)]
    #[traverse(skip)]
    pub(super) exe_state: Arc<std::sync::Mutex<OneEuroFilter>>,

    /// Optional human-readable description shown in the GUI.
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    #[traverse(skip)]
    pub(crate) desc: DescriptionCfg,

    /// Master on/off switch for the entire one-euro step. When `false`,
    /// the input value passes through unchanged and no filter state is updated.
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    #[traverse(skip)]
    pub(crate) enabled: bool,

    /// When `true` and the input has **relative** semantics, the last
    /// known value is re-fed into the filter on idle ticks so the
    /// output continues to converge.
    ///
    /// Ignored for absolute inputs (always processed every tick).
    /// Mutually exclusive with `on_relative_input_reset_on_idle`.
    ///
    /// **Warning:** convergence speed depends on `global.idle_tick_rate`.
    #[serde(default = "default_false")]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    #[traverse(skip)]
    pub(crate) on_relative_input_feed_on_idle: bool,

    /// When `true` and the input has **relative** semantics, the filter
    /// state (`x̂_prev`, `dx̂_prev`) is **reset** to the current input
    /// on idle ticks.
    ///
    /// Use when the relative source may jump to a new baseline after a
    /// pause and you want to avoid a smoothing transient.
    /// Mutually exclusive with `on_relative_input_feed_on_idle`.
    #[serde(default = "default_false")]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    #[traverse(skip)]
    pub(crate) on_relative_input_reset_on_idle: bool,

    /// Speed coefficient (β). Controls how much the cutoff frequency
    /// increases in response to fast input movement.
    ///
    /// Adaptive cutoff formula: `cutoff = min_cutoff_hz + β · |dx̂|`
    ///
    /// - `0.0` — fixed-cutoff filter (no speed adaptation).
    /// - `0.001 – 0.01` — gentle adaptation (recommended starting range).
    /// - `0.1 – 10.0` — aggressive adaptation; near-passthrough during
    ///   fast sweeps, may re-introduce jitter.
    ///
    /// **Tuning heuristic:** start at `0.0`, increase until fast
    /// movements feel responsive, then back off slightly.
    #[serde(default = "default_1euro_beta")]
    #[garde(range(min = 0.0))]
    pub(crate) beta: ValueSrcs,

    /// Minimum cutoff frequency in Hz. The cutoff used when the input
    /// is stationary or moving very slowly.
    ///
    /// - `0.1 – 0.5` — very heavy smoothing at rest (strong jitter
    ///   suppression, noticeable lag onset).
    /// - `1.0` *(default)* — moderate smoothing; good starting point.
    /// - `5.0 – 50.0` — light smoothing; use when input is already
    ///   clean or minimal latency is critical.
    ///
    /// Think of this as the **noise floor** of the filter.
    #[serde(default = "default_1euro_min_cutoff_hz")]
    #[garde(range(min = 0.0))]
    pub(crate) min_cutoff_hz: ValueSrcs,

    /// Cutoff frequency in Hz for the derivative (speed) low-pass
    /// filter.
    ///
    /// The raw derivative `(x - x̂_prev) / dt` amplifies sensor jitter.
    /// This secondary filter smooths the derivative so the adaptive
    /// cutoff does not oscillate.
    ///
    /// - `0.01 – 0.1` — very smooth derivative; stable but slow to
    ///   react to speed changes.
    /// - `1.0` *(default)* — balanced; works well for most devices.
    /// - `10.0 – 100.0` — barely filtered derivative; fast tracking
    ///   but may oscillate on noisy inputs.
    ///
    /// Rarely needs adjustment from the default.
    #[serde(default = "default_1euro_d_cutoff_hz")]
    #[garde(range(min = 0.0))]
    pub(crate) d_cutoff_hz: ValueSrcs,
}

impl PartialEq for OneEuroFilterCfg {
    fn eq(&self, other: &Self) -> bool {
        self.desc == other.desc
            && self.enabled == other.enabled
            && self.on_relative_input_feed_on_idle == other.on_relative_input_feed_on_idle
            && self.on_relative_input_reset_on_idle == other.on_relative_input_reset_on_idle
            && self.beta == other.beta
            && self.min_cutoff_hz == other.min_cutoff_hz
            && self.d_cutoff_hz == other.d_cutoff_hz
    }
}

impl Default for OneEuroFilterCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            on_relative_input_feed_on_idle: default_false(),
            beta: default_1euro_beta(),
            min_cutoff_hz: default_1euro_min_cutoff_hz(),
            d_cutoff_hz: default_1euro_d_cutoff_hz(),
            on_relative_input_reset_on_idle: default_false(),
            desc: Default::default(),
            common_state: Default::default(),
            exe_state: Default::default(),
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinearCfg {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,
    #[serde(default = "default_linear_slope")]
    pub(crate) slope: BaseNumT,
    #[serde(default)]
    pub(crate) shift_x: BaseNumT,
    #[serde(default)]
    pub(crate) shift_y: BaseNumT,
    #[serde(default)]
    pub(crate) center_symmetric: bool,
    #[serde(default = "default_on_idle")]
    #[serde(skip_serializing_if = "is_true")]
    pub(crate) on_idle: bool,
}

impl Default for LinearCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            slope: default_linear_slope(),
            shift_x: Default::default(),
            shift_y: Default::default(),
            center_symmetric: Default::default(),
            on_idle: default_on_idle(),
            desc: Default::default(),
            common_state: Default::default(),
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SmoothstepCfg {
    #[serde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,
    #[serde(default = "default_on_idle")]
    #[serde(skip_serializing_if = "is_true")]
    pub(crate) on_idle: bool,
}

impl Default for SmoothstepCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            on_idle: default_on_idle(),
            desc: Default::default(),
            common_state: Default::default(),
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct SCurveCfg {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[serde(default = "default_scurve_steepness")]
    #[garde(range(min = 0.0))]
    pub(crate) steepness: BaseNumT,
    #[serde(default = "default_on_idle")]
    #[serde(skip_serializing_if = "is_true")]
    #[garde(skip)]
    pub(crate) on_idle: bool,
}

impl Default for SCurveCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            steepness: default_scurve_steepness(),
            on_idle: default_on_idle(),
            desc: Default::default(),
            common_state: Default::default(),
        }
    }
}

// #[nutype(
//     derive(Debug, Clone, Serialize, Deserialize),
//     validate(greater = 1.0, less = 40.0)
// )]
// pub(crate) struct NormExpBase(BaseNumericT);
#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct NormExpCfg {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[serde(default = "default_norm_exp_base")]
    #[garde(range(min = 0.0))]
    /// Base must be positive
    pub(crate) base: BaseNumT,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) center_symmetric: bool,
    #[serde(default = "default_on_idle")]
    #[serde(skip_serializing_if = "is_true")]
    #[garde(skip)]
    pub(crate) on_idle: bool,
}

impl Default for NormExpCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            base: default_norm_exp_base(),
            center_symmetric: Default::default(),
            on_idle: default_on_idle(),
            desc: Default::default(),
            common_state: Default::default(),
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignedPowerCfg {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[serde(default = "default_one")]
    #[garde(range(min = 0.0))]
    /// Power must be positive
    pub(crate) power: BaseNumT,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) center_symmetric: bool,
    #[serde(default = "default_on_idle")]
    #[serde(skip_serializing_if = "is_true")]
    #[garde(skip)]
    pub(crate) on_idle: bool,
}

impl Default for SignedPowerCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            power: 1.0,
            center_symmetric: Default::default(),
            on_idle: default_on_idle(),
            desc: Default::default(),
            common_state: Default::default(),
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Default, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct HighPassCfg {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[garde(range(min = 0.0))]
    pub(crate) cutoff: BaseNumT,
    #[serde(default = "default_on_idle")]
    #[serde(skip_serializing_if = "is_true")]
    #[garde(skip)]
    pub(crate) on_idle: bool,
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntegrateCfg {
    #[serde(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(skip)]
    #[garde(skip)]
    pub(super) exe_state: Arc<std::sync::Mutex<IntegrateExeState>>,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[garde(skip)]
    pub(crate) range: NumInterval<BaseNumT>,
    #[serde(default)]
    #[garde(range(min = 0.0))]
    pub(crate) deadzone_norm: BaseNumT,
    #[serde(default = "default_one")]
    #[garde(range(min = 0.0, max = 1.0))]
    pub(crate) smoothing_alpha: BaseNumT,
    #[serde(default = "default_on_idle")]
    #[serde(skip_serializing_if = "is_true")]
    #[garde(skip)]
    pub(crate) on_idle: bool,
}

impl PartialEq for IntegrateCfg {
    fn eq(&self, other: &Self) -> bool {
        self.common_state == other.common_state
            && self.desc == other.desc
            && self.enabled == other.enabled
            && self.range == other.range
            && self.deadzone_norm == other.deadzone_norm
            && self.smoothing_alpha == other.smoothing_alpha
            && self.on_idle == other.on_idle
    }
}

impl Default for IntegrateCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            range: NumInterval::new(-100.0, 100.0),
            deadzone_norm: 0.0,
            smoothing_alpha: default_smoothing_alpha(),
            on_idle: default_on_idle(),
            desc: Default::default(),
            common_state: Default::default(),
            exe_state: Default::default(),
        }
    }
}

// ==================================================================
impl TfmSeqCfg {
    #[cfg(feature = "gui")]
    pub(crate) fn disable_gui_tracing(&self) {
        for step in &self.steps {
            step.common_state_ref().disable_gui_tracing();
        }
    }

    pub(crate) fn recompute_metadata_with_known_inputs(&mut self) {
        self.recompute_metadata(self.in_meta);
    }

    pub(crate) fn recompute_metadata(&mut self, input: AutoOrManual<InputValueMetadata<BaseNumT>>) {
        self.in_meta = input;
        let mut in_relativity = self.in_meta.relativity;
        let mut in_interval = self.in_meta.interval;
        for step in &mut self.steps {
            step.common_state_mut()
                .set_input_relativity(in_relativity)
                .set_input_interval(in_interval);
            let (out_interval, out_relativity) = match step {
                TfmStepCfg::Script(script) => {
                    script
                        .aux_transformations
                        .iter_mut()
                        .for_each(|t| t.1.recompute_metadata(t.1.in_meta));
                    (
                        script.output_interval.unwrap_or(in_interval),
                        script.output_relativity.unwrap_or(in_relativity),
                    )
                }
                TfmStepCfg::Integrate(integrate) if integrate.enabled => (integrate.range, Relativity::Abs),
                TfmStepCfg::Steering(steering) if steering.enabled => {
                    steering
                        .integrated_user_input_transform
                        .recompute_metadata(AutoOrManual::Auto(InputValueMetadata {
                            interval: SYMM_UNIT_INTERVAL,
                            relativity: Relativity::Abs,
                        }));
                    if let Some(ff) = &mut steering.force_feedback {
                        ff.transformation
                            .recompute_metadata(AutoOrManual::Auto(InputValueMetadata {
                                interval: SYMM_UNIT_INTERVAL,
                                relativity: Relativity::Rel,
                            }));
                    };
                    (SYMM_UNIT_INTERVAL, Relativity::Abs)
                }
                TfmStepCfg::_ForceFeedback(force_feedback) if force_feedback.enabled => {
                    (SYMM_UNIT_INTERVAL, Relativity::Abs)
                }
                TfmStepCfg::Clamp(clamp) if clamp.enabled => {
                    clamp.sanitize_inplace();
                    (clamp.get_out_interval(), in_relativity)
                }
                TfmStepCfg::Nop(_)
                | TfmStepCfg::Invert(_)
                | TfmStepCfg::Integrate(_)
                | TfmStepCfg::Steering(_)
                | TfmStepCfg::Clamp(_)
                | TfmStepCfg::RaiseFall(_)
                | TfmStepCfg::Ema(_)
                | TfmStepCfg::Linear(_)
                | TfmStepCfg::Smoothstep(_)
                | TfmStepCfg::SCurve(_)
                | TfmStepCfg::Exp(_)
                | TfmStepCfg::SignedPower(_)
                | TfmStepCfg::OneEuro(_)
                | TfmStepCfg::_HighPass(_)
                | TfmStepCfg::_ForceFeedback(_) => (in_interval, in_relativity),
            };

            step.common_state_mut()
                .set_output_relativity(out_relativity)
                .set_output_interval(out_interval);

            in_interval = out_interval;
            in_relativity = out_relativity;
        }
    }
}

bitflags! {
    #[derive(Default, Debug, Clone, Copy)]
    pub struct DynValFilter: u8 {
        const Var = 1;
        const Control = 1 << 1;
        const Src = 1 << 2;
        const Dst = 1 << 3;
    }
}

pub(crate) fn collect_dynamic_value_matchers(
    root: &impl Traversable,
    filter: impl Fn(DynValFilter) -> bool,
) -> Vec<DynValueRefs> {
    struct Collect<'a> {
        filter: &'a dyn Fn(DynValFilter) -> bool,
        ctx: DynValFilter,
        collected: Vec<DynValueRefs>,
    }
    impl traversable::Visitor for Collect<'_> {
        type Break = ();

        fn enter(&mut self, this: &dyn core::any::Any) -> std::ops::ControlFlow<Self::Break> {
            if this.is::<ValueSrcs>() {
                self.ctx.insert(DynValFilter::Src);
            } else if this.is::<ValueDsts>() {
                self.ctx.insert(DynValFilter::Dst);
            } else if this.is::<VariableRef>() {
                self.ctx.insert(DynValFilter::Var);
            } else if this.is::<DeviceControlMatcherRef>() {
                self.ctx.insert(DynValFilter::Control);
            }
            std::ops::ControlFlow::Continue(())
        }

        fn leave(&mut self, this: &dyn core::any::Any) -> std::ops::ControlFlow<Self::Break> {
            if let Some(dv) = this.downcast_ref::<DynValueRefs>() {
                if (self.filter)(self.ctx) {
                    self.collected.push(dv.clone());
                }
                match dv {
                    DynValueRefs::DeviceControlMatcher(_) => self.ctx.remove(DynValFilter::Control),
                    DynValueRefs::Variable(_) => self.ctx.remove(DynValFilter::Var),
                }
            }
            if this.is::<ValueSrcs>() {
                self.ctx.remove(DynValFilter::Src);
            } else if this.is::<ValueDsts>() {
                self.ctx.remove(DynValFilter::Dst);
            }
            std::ops::ControlFlow::Continue(())
        }
    }

    let mut state = Collect {
        filter: &filter,
        ctx: Default::default(),
        collected: Vec::new(),
    };

    let _ = root.traverse(&mut state);
    state.collected.sort();
    state.collected.dedup();
    state.collected
}

// ==================================================================
#[derive(Debug, Clone, Traversable, TraversableMut, JsonSchema, PartialEq, Serialize)]
#[serde(untagged)]
enum TfmSeqVariants {
    Short(Vec<TfmStepCfg>),
    Full(TfmSeqFull),
}

impl<'de> Deserialize<'de> for TfmSeqVariants {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let vv: serde_value::Value = Deserialize::deserialize(deserializer)?;
        match Vec::<_>::deserialize(vv.clone().into_deserializer()) {
            Ok(v) => Ok(Self::Short(v)),
            Err(e1) => match TfmSeqFull::deserialize(vv.into_deserializer()) {
                Ok(v) => Ok(Self::Full(v)),
                Err(e2) => Err(D::Error::custom(format!(
                    "Configuration parse error.\nIf using steps list only: {}\nIf using steps + input spec: {}\n",
                    e1, e2
                ))),
            },
        }
    }
}

impl Default for TfmSeqVariants {
    fn default() -> Self {
        Self::Short(Default::default())
    }
}

impl From<TfmSeqVariants> for TfmSeqCfg {
    fn from(value: TfmSeqVariants) -> Self {
        match value {
            TfmSeqVariants::Short(s) => Self {
                id: Default::default(),
                steps: s,
                in_meta: AutoOrManual::Auto(Default::default()),
                last_io: Default::default(),
                desc: Default::default(),
            },
            TfmSeqVariants::Full(f) => Self {
                id: f.id,
                steps: f.steps,
                in_meta: AutoOrManual::Manual(InputValueMetadata {
                    interval: f.in_meta.interval,
                    relativity: f.in_meta.relativity,
                }),
                last_io: Default::default(),
                desc: f.desc,
            },
        }
    }
}

impl From<TfmSeqCfg> for TfmSeqVariants {
    fn from(value: TfmSeqCfg) -> Self {
        if let AutoOrManual::Manual(_) = value.in_meta {
            Self::Full(TfmSeqFull {
                id: value.id,
                steps: value.steps,
                in_meta: value.in_meta,
                last_io: Default::default(),
                desc: value.desc,
            })
        } else {
            Self::Short(value.steps)
        }
    }
}

macro_rules! tfm_seq_tpl {
    (vis: $v:vis, name: $name:ident, meta: $( $m:meta ),* ) => {
        #[derive(
            Debug, Clone, Traversable, TraversableMut, Deserialize, Serialize, JsonSchema, PartialEq, Default, Validate
        )]
        $( #[$m] )*
        $v struct $name {
            #[serde(default)]
            #[traverse(skip)]
            #[serde(skip_serializing_if = "String::is_empty")]
            #[garde(skip)]
            pub(crate) desc: DescriptionCfg,
            #[traverse(skip)]
            #[serde(skip)]
            #[garde(skip)]
            pub(crate) id: ObjId,
            #[traverse(skip)]
            #[serde(skip)]
            #[garde(skip)]
            pub(crate) last_io: Arc<CachePadded<AtomicF32>>,
            #[traverse(skip)]
            // #[serde(default)]
            #[serde(flatten)]
            #[garde(skip)]
            pub(crate) in_meta: AutoOrManual<InputValueMetadata<BaseNumT>>,
            #[garde(skip)]
            pub(crate) steps: Vec<TfmStepCfg>,
        }
    };
}

tfm_seq_tpl!(vis: , name: TfmSeqFull, meta: );
tfm_seq_tpl!(
    vis: pub(crate),
    name: TfmSeqCfg,
    meta: serde(from = "TfmSeqVariants", into = "TfmSeqVariants")
);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub(crate) enum AutoOrManual<T: Default> {
    Manual(T),
    Auto(T),
}

impl<T: Default> From<T> for AutoOrManual<T> {
    fn from(value: T) -> Self {
        Self::Auto(value)
    }
}

impl<T: Copy + Default> Copy for AutoOrManual<T> {}

impl<T: Default> AutoOrManual<T> {
    #[allow(unused)]
    pub fn inner_ref(&self) -> &T {
        match self {
            Self::Manual(m) => m,
            Self::Auto(a) => a,
        }
    }

    #[allow(unused)]
    pub fn inner_mut(&mut self) -> &mut T {
        match self {
            Self::Manual(m) => m,
            Self::Auto(a) => a,
        }
    }

    #[allow(unused)]
    pub fn make_auto(self) -> AutoOrManual<T> {
        match self {
            Self::Manual(m) => Self::Auto(m),
            Self::Auto(_) => self,
        }
    }

    pub fn make_manual(self) -> AutoOrManual<T> {
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

impl<T: Default> Default for AutoOrManual<T> {
    fn default() -> Self {
        Self::Auto(Default::default())
    }
}

impl WithDescriptionMut for TfmSeqCfg {
    fn description_mut(&mut self) -> Option<&mut DescriptionCfg> {
        Some(&mut self.desc)
    }
}

impl WithRelativityRef for TfmSeqCfg {
    fn relativity_ref(&self) -> &Relativity {
        &self.in_meta.relativity
    }
}

impl<T: std::default::Default> DerefMut for AutoOrManual<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            AutoOrManual::Manual(v) => v,
            AutoOrManual::Auto(v) => v,
        }
    }
}

impl WithNumInterval for TfmSeqCfg {
    type ValueT = BaseNumT;
    fn get_interval(&self) -> NumInterval<Self::ValueT> {
        self.in_meta.interval
    }
}

impl WithRuntimeId for TfmSeqCfg {
    fn get_id(&self) -> ObjId {
        self.id
    }
    fn assign_new_id(&mut self) {
        self.id = Default::default()
    }
}

// ==================================================================
impl PartialEq for SteeringCfg {
    fn eq(&self, other: &Self) -> bool {
        self.common_state == other.common_state
            && self.desc == other.desc
            && self.enabled == other.enabled
            && self.accumulator == other.accumulator
            && self.deadzone_counts == other.deadzone_counts
            && self.input_gain == other.input_gain
            && self.auto_center_halflife == other.auto_center_halflife
            && self.auto_center_along_force_feedback == other.auto_center_along_force_feedback
            && self.hold_factor == other.hold_factor
            && self.force_feedback == other.force_feedback
            && self.integrated_user_input_transform == other.integrated_user_input_transform
    }
}

#[delegatable_trait]
pub(crate) trait WithCommonState {
    fn common_state_ref(&self) -> &TfmStepCommonState;
}

#[allow(unused)]
#[delegatable_trait]
pub(crate) trait WithCommonStateMut {
    fn common_state_mut(&mut self) -> &mut TfmStepCommonState;
}

#[allow(unused)]
#[delegatable_trait]
pub(crate) trait WithCommonStateAssignedNew {
    fn common_state_assign_new(&mut self);
}

macro_rules! impl_with_common_state {
    ($($t:ty),* $(,)?) => {
        $(
            impl WithCommonState for $t {
                fn common_state_ref(&self) -> &TfmStepCommonState {
                    &self.common_state
                }
            }
            impl WithCommonStateMut for $t {
                fn common_state_mut(&mut self) -> &mut TfmStepCommonState {
                    &mut self.common_state
                }
            }
            impl WithCommonStateAssignedNew for $t {
                fn common_state_assign_new(&mut self) {
                    self.common_state = Default::default();
                }
            }
        )*
    }
}

impl_with_common_state!(
    ForceFeedbackCfg,
    Box<ForceFeedbackCfg>,
    ClampCfg,
    NopCfg,
    InvertCfg,
    EmaFilterCfg,
    Box<OneEuroFilterCfg>,
    LinearCfg,
    SmoothstepCfg,
    SCurveCfg,
    NormExpCfg,
    SignedPowerCfg,
    HighPassCfg,
    IntegrateCfg,
    SteeringCfg,
    Box<SteeringCfg>,
    Box<RaiseFallCfg>,
    ScriptCfg,
);

/// Steering wheel emulation transform.
///
/// Converts relative input (e.g. mouse movement) into an absolute wheel
/// position in [-1, +1], with force-feedback displacement, autocentering,
/// and a configurable "hold factor" simulating grip strength.
///
/// See the doc/steering.md for a full
/// guide, signal-flow diagram, and configuration examples.
#[derive(Debug, Clone, Serialize, Traversable, TraversableMut, Deserialize, JsonSchema, Validate)]
#[with_doc_str]
pub(crate) struct SteeringCfg {
    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    /// Internal state
    common_state: TfmStepCommonStateShared,

    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    /// Internal state
    exe_state: Arc<std::sync::Mutex<SteeringExeState>>,

    /// Optional human-readable description shown in the GUI.
    #[traverse(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,

    /// Master on/off switch. When `false` the input passes through unchanged.
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,

    /// Optional external variable (or device control) that persists the raw
    /// accumulated wheel angle (`pre_filter`) across ticks.
    ///
    /// Useful for sharing wheel state across mappings or for inspection.
    #[garde(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) accumulator: Option<DynValueRefs>,

    /// *(Reserved — currently unused.)*
    ///
    /// Intended deadzone in input counts below which movement is ignored.
    #[allow(dead_code)]
    #[serde(default)]
    #[garde(range(min = 0.0))]
    pub(crate) deadzone_counts: BaseNumT,

    /// Multiplier applied to the raw input **before**
    /// accumulation. Mapped from its own interval to [0, 1].
    ///
    /// - Low (0.05–0.15): heavy, slow steering.
    /// - High (0.3–0.5): quick, responsive steering.
    ///
    /// Accepts a static value or a dynamic reference (`var:` / `dev:`)
    /// for runtime adjustment.
    ///
    /// YAML aliases: `smoothing_alpha`, `input_sensitivity`.
    #[garde(skip)]
    #[serde(alias = "smoothing_alpha")]
    #[serde(alias = "input_sensitivity")]
    pub(crate) input_gain: ValueSrcs,

    /// Half-life (in seconds) of the exponential autocentering decay.
    ///
    /// - `0`: autocentering disabled.
    /// - `0.1`: snappy return.
    /// - `0.3` (default): moderate, natural return.
    /// - `1.0+`: slow drift to center.
    ///
    /// Decay per tick: `(1 - 2^(-dt / halflife)) * (1 - hold_factor)`.
    /// Only active when the user is idle and (FFB is negligible or
    /// `auto_center_along_force_feedback > 0`).
    #[serde(default)]
    #[garde(skip)]
    pub(crate) auto_center_halflife: ValueSrcs,

    /// Allows autocentering to operate **alongside** active force feedback,
    /// scaled by this factor in [0, 1].
    ///
    /// - `0.0` (default): autocentering suppressed while FFB is present.
    /// - `1.0`: autocentering at full strength regardless of FFB.
    ///
    /// Accepts a bare `true`/`false` in YAML (converted to 1.0/0.0).
    #[serde(default)]
    #[garde(skip)]
    #[serde(deserialize_with = "deserialize_bool_or_value_src")]
    pub(crate) auto_center_along_force_feedback: ValueSrcs,

    /// Simulated grip strength in [0, 1]. Scales both FFB displacement
    /// and autocentering by `(1 - hold_factor)`.
    ///
    /// - `0.0` (default): hands off — FFB and autocentering act freely.
    /// - `0.5`: moderate grip — half effect.
    /// - `1.0`: locked grip — wheel immovable except by direct input.
    ///
    /// Commonly mapped to mouse Y via a separate `integrate` + `clamp`
    /// mapping for dynamic grip control.
    #[serde(default)]
    #[serde(serialize_with = "serialize_value_src_rt_ignore_interval")]
    #[garde(skip)]
    pub(crate) hold_factor: ValueSrcs,

    /// Force-feedback sub-configuration.
    ///
    /// When present and enabled, the step reads FFB forces from the
    /// destination virtual device (or a `custom_source`), optionally
    /// filters them through `transformation`, scales by `gain`, and
    /// applies the result as a positional offset weighted by
    /// `(1 - hold_factor) * dt`.
    ///
    /// Omit or set `enabled: false` to disable FFB entirely.
    #[serde(default)]
    #[garde(skip)]
    pub(crate) force_feedback: Option<ForceFeedbackCfg>,

    /// Sub-pipeline applied to the accumulated user input **after**
    /// integration but **before** FFB and autocentering.
    ///
    /// Receives the wheel angle in [-1, +1] (absolute). Use this to
    /// reshape the steering response curve:
    ///
    /// - `exp` with `base > 1, center_symmetric: true` — progressive
    ///   ratio (less sensitive in center).
    /// - `exp` with `base < 1` — inverted progressive (more sensitive
    ///   in center).
    /// - Empty (default) — linear 1:1 passthrough.
    #[serde(default)]
    #[garde(skip)]
    pub(crate) integrated_user_input_transform: TfmSeqCfg,
}

impl TfmExeState for SteeringCfg {
    type StateMutT<'a>
        = std::sync::MutexGuard<'a, SteeringExeState>
    where
        Self: 'a;

    type ResetInput<'b> = Option<SteeringExeState>;

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.lock().unwrap()
    }

    fn exe_state_reset(&self, reset_with: Self::ResetInput<'_>) {
        *self.exe_state_mut() = reset_with.unwrap_or_default()
    }
}

impl Default for SteeringCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            deadzone_counts: 0.0,
            input_gain: ValueSrcs::Static(StaticValueCfg {
                value: default_steering_smoothing_alpha().into(),
                interval: UNIT_INTERVAL.into(),
            }),
            auto_center_halflife: ValueSrcs::Static(StaticValueCfg {
                value: default_steering_transform_auto_center_halflife().into(),
                interval: UNIT_INTERVAL.into(),
            }),
            auto_center_along_force_feedback: ValueSrcs::Static(StaticValueCfg {
                value: 0.0.into(),
                interval: UNIT_INTERVAL.into(),
            }),
            hold_factor: ValueSrcs::Static(StaticValueCfg {
                value: 0.0.into(),
                interval: UNIT_INTERVAL.into(),
            }),
            force_feedback: None,
            integrated_user_input_transform: TfmSeqCfg::default(),
            desc: Default::default(),
            accumulator: Default::default(),
            common_state: Default::default(),
            exe_state: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Traversable, TraversableMut, Validate)]
pub(crate) struct RaiseFallCfg {
    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    common_state: TfmStepCommonStateShared,
    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    pub(super) exe_state: Arc<std::sync::Mutex<RaiseFallExeState>>,
    #[traverse(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[garde(range(min = 0.0))] // Rate limits must be positive
    pub(crate) raise_rate: BaseNumT,
    #[garde(range(min = 0.0))]
    pub(crate) fall_rate: BaseNumT,
    #[serde(default = "default_smoothing_alpha")]
    #[garde(range(min = 0.0, max = 1.0))]
    pub(crate) smoothing_alpha: BaseNumT,
    #[serde(default)]
    #[garde(range(min = 0.0))]
    /// Time delay must be positive
    pub(crate) fall_delay: BaseNumT,
    #[serde(serialize_with = "serialize_value_src_rt_ignore_interval")]
    #[serde(default)]
    #[garde(skip)]
    pub(crate) fall_hold_factor: ValueSrcs,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) invert_fall_hold_factor: bool,
}

impl PartialEq for RaiseFallCfg {
    fn eq(&self, other: &Self) -> bool {
        self.desc == other.desc
            && self.enabled == other.enabled
            && self.raise_rate == other.raise_rate
            && self.fall_rate == other.fall_rate
            && self.smoothing_alpha == other.smoothing_alpha
            && self.fall_delay == other.fall_delay
            && self.fall_hold_factor == other.fall_hold_factor
            && self.invert_fall_hold_factor == other.invert_fall_hold_factor
    }
}

impl Default for RaiseFallCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            raise_rate: Default::default(),
            fall_rate: Default::default(),
            smoothing_alpha: Default::default(),
            fall_delay: Default::default(),
            fall_hold_factor: ValueSrcs::Static(StaticValueCfg {
                value: 1.0.into(),
                interval: UNIT_INTERVAL.into(),
            }),
            invert_fall_hold_factor: false,
            desc: Default::default(),
            common_state: Default::default(),
            exe_state: Default::default(),
        }
    }
}

// -----------------------------------
pub(crate) trait TfmStepIdleBehavior {
    fn relative_input_feed_on_idle_mut(&mut self) -> &mut bool;
    fn relative_input_reset_on_idle_mut(&mut self) -> &mut bool;
}

impl TfmStepIdleBehavior for EmaFilterCfg {
    fn relative_input_feed_on_idle_mut(&mut self) -> &mut bool {
        &mut self.on_relative_input_feed_on_idle
    }
    fn relative_input_reset_on_idle_mut(&mut self) -> &mut bool {
        &mut self.on_relative_input_reset_on_idle
    }
}

impl TfmStepIdleBehavior for OneEuroFilterCfg {
    fn relative_input_feed_on_idle_mut(&mut self) -> &mut bool {
        &mut self.on_relative_input_feed_on_idle
    }
    fn relative_input_reset_on_idle_mut(&mut self) -> &mut bool {
        &mut self.on_relative_input_reset_on_idle
    }
}

/// Scripting language selector for the [`ScriptCfg`] transform step.
///
/// Currently only [`Luau`](ScriptLanguage::Luau) is supported.
#[derive(JsonSchema, Display, Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub(crate) enum ScriptLanguage {
    /// Luau — a fast, sandboxed dialect of Lua 5.1 with gradual typing,
    /// executed via the `mlua` crate.
    #[default]
    Luau,
}

/// User-script transform step.
///
/// Embeds a Luau script that runs on **every tick** of the mapping
/// pipeline, giving full programmatic control over the signal. The script
/// can read the main pipeline input and any number of auxiliary data
/// sources, execute arbitrary logic (with persistent global state across
/// ticks), and write results back to the main pipeline output and/or
/// auxiliary destinations.
///
/// Five global API functions are injected into the Luau environment for
/// the duration of each tick:
///
/// | Function | Purpose |
/// |----------|---------|
/// | `read(key)` | Read the main input (`0`) or an auxiliary source (string / 1-based index). |
/// | `write(key, value)` | Write the main output (`0`) or an auxiliary destination. |
/// | `transform(name, value)` | Invoke a named sub-pipeline from `aux_transformations`. |
/// | `is_idle()` | `true` when the tick is an idle-clock tick (no user input). |
/// | `base_rate()` | The configured idle tick rate in Hz. |
///
/// See `doc/script.md` for a full guide, signal-flow diagram, API
/// reference, and configuration examples.
#[derive(Clone, Serialize, Deserialize, JsonSchema, Debug, TraversableMut, Traversable)]
#[with_doc_str]
pub(crate) struct ScriptCfg {
    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    /// Internal state
    common_state: TfmStepCommonStateShared,

    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    /// Internal state
    pub(super) exe_state: Arc<std::sync::Mutex<Option<ScriptExeState>>>,

    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    #[cfg(feature = "gui")]
    /// Internal state
    pub(super) edit_epoch: usize,

    /// Optional human-readable description shown in the GUI.
    #[traverse(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) desc: DescriptionCfg,

    /// Master on/off switch. When `false` the input passes through
    /// unchanged and the script is **not** executed.
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,

    /// Scripting language to use. Currently only `Luau` is supported.
    ///
    /// Defaults to `Luau`; may be omitted entirely.
    #[serde(default)]
    #[traverse(skip)]
    pub(crate) lang: ScriptLanguage,

    /// Luau source code executed on every tick.
    ///
    /// Compiled **once** (lazily, on first execution) into a callable
    /// function; subsequent ticks re-use the compiled bytecode. On
    /// compilation failure the error is logged and the step degrades to
    /// a no-op.
    ///
    /// Global variables **persist** between ticks — this is the primary
    /// mechanism for maintaining state (accumulators, timers, flags).
    /// On the first tick, uninitialized globals are `nil`.
    ///
    /// Use YAML block scalars (`|-`) for multi-line scripts.
    #[serde(default)]
    #[traverse(skip)]
    pub(crate) script: String,

    /// Overrides the **output interval metadata** of this step.
    ///
    /// When `None` (default), the output interval is inherited from the
    /// input. Set this when the script produces values in a different
    /// range and downstream steps need the correct interval.
    #[traverse(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_interval: Option<NumInterval<BaseNumT>>,

    /// Overrides the **output relativity metadata** of this step.
    ///
    /// When `None` (default), the output relativity is inherited from
    /// the input. Set to `Abs` or `Rel` when the script converts
    /// between relative and absolute semantics.
    #[traverse(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_relativity: Option<Relativity>,

    /// Auxiliary data **sources** readable from the script via
    /// `read("<key>")` or `read(<1-based index>)`.
    ///
    /// Accepts a YAML **map** (explicit string keys) or a **list**
    /// (auto-numbered `"1"`, `"2"`, …).
    ///
    /// YAML alias: `sources`.
    #[serde(default)]
    #[serde(alias = "sources")]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_btree_or_vec")]
    pub(crate) aux_srcs: BTreeMap<String, ScriptSourceCfg>,

    /// Auxiliary data **destinations** writable from the script via
    /// `write("<key>", value)` or `write(<1-based index>, value)`.
    ///
    /// Accepts a YAML **map** or a **list**, same as [`aux_srcs`](Self::aux_srcs).
    ///
    /// YAML alias: `destinations`.
    #[serde(default)]
    #[serde(alias = "destinations")]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_btree_or_vec")]
    pub(crate) aux_dsts: BTreeMap<String, ScriptDestinationCfg>,

    /// Named transformation **sub-pipelines** invocable from the script
    /// via `transform("<name>", value)`.
    ///
    /// Each entry is a standard transformation pipeline (list of
    /// transform steps), identical in format to the top-level
    /// `transformation:` field of a mapping. Each sub-pipeline
    /// maintains its own independent execution state.
    ///
    /// Accepts a YAML **map** or a **list**.
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_btree_or_vec")]
    pub(crate) aux_transformations: BTreeMap<String, TfmSeqCfg>,
}

impl PartialEq for ScriptCfg {
    fn eq(&self, other: &Self) -> bool {
        self.desc == other.desc
            && self.enabled == other.enabled
            && self.lang == other.lang
            && self.script == other.script
            && self.output_interval == other.output_interval
            && self.output_relativity == other.output_relativity
            && self.aux_srcs == other.aux_srcs
            && self.aux_dsts == other.aux_dsts
            && self.aux_transformations == other.aux_transformations
    }
}

impl Default for ScriptCfg {
    fn default() -> Self {
        Self {
            desc: Default::default(),
            enabled: default_step_enabled(),
            lang: Default::default(),
            script: Default::default(),
            output_interval: Default::default(),
            output_relativity: Default::default(),
            aux_srcs: Default::default(),
            aux_dsts: Default::default(),
            aux_transformations: Default::default(),
            common_state: Default::default(),
            exe_state: Default::default(),
            #[cfg(feature = "gui")]
            edit_epoch: Default::default(),
        }
    }
}

fn deserialize_bool_or_value_src<'de, D>(deserializer: D) -> Result<ValueSrcs, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Data {
        Bool(bool),
        ValueSrc(ValueSrcs),
    }
    match Data::deserialize(deserializer)? {
        Data::Bool(b) => Ok(ValueSrcs::Static(StaticValueCfg {
            value: if b { 1.0 } else { 0.0 }.into(),
            interval: UNIT_INTERVAL.into(),
        })),
        Data::ValueSrc(v) => Ok(v),
    }
}

fn deserialize_btree_or_vec<'de, D, T>(deserializer: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MapOrVec<T> {
        Map(BTreeMap<String, T>),
        Vec(Vec<T>),
    }
    match MapOrVec::deserialize(deserializer)? {
        MapOrVec::Map(m) => Ok(m),
        MapOrVec::Vec(v) => Ok(v
            .into_iter()
            .enumerate()
            .map(|(i, val)| ((i + 1).to_string(), val))
            .collect()),
    }
}

/// A single auxiliary data source entry within [`ScriptCfg::aux_srcs`].
///
/// Binds an external value (variable, device control, or static number)
/// to a key that the script references via `read()`.
#[derive(Clone, Serialize, Deserialize, JsonSchema, Debug, TraversableMut, Traversable, Default, PartialEq)]
#[with_doc_str]
pub(crate) struct ScriptSourceCfg {
    /// When set, the raw value read from [`source`](Self::source) is
    /// **remapped** from the source's native interval to this interval
    /// before the script sees it.
    ///
    /// Example: a force-feedback axis with native range \[-32768, 32767\]
    /// and `remap_to_interval: [-1.0, 1.0]` yields a normalized value
    /// in \[-1, +1\].
    ///
    /// When `None`, the raw value is passed through as-is.
    #[traverse(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remap_to_interval: Option<NumInterval<BaseNumT>>,

    /// The data source to read from.
    ///
    /// Accepts any valid `ValueSrc`:
    /// - Device control: `{ dev: <device>, ctl: <control> }`
    /// - Variable: `{ var: <name> }`
    /// - Static value: `{ value: <number>, range: [from, to] }`
    #[serde(default)]
    pub(crate) source: ValueSrcs,
}

/// A single auxiliary data destination entry within [`ScriptCfg::aux_dsts`].
///
/// Binds an external target (variable or device control) to a key that
/// the script references via `write()`.
#[derive(Clone, Serialize, Deserialize, JsonSchema, Debug, TraversableMut, Traversable, Default, PartialEq)]
#[with_doc_str]
pub(crate) struct ScriptDestinationCfg {
    /// When set, the value written by the script is **remapped** from
    /// this interval to the destination's native interval before being
    /// stored.
    ///
    /// Example: script output in \[-100, 100\] with
    /// `remap_from_interval: [-100.0, 100.0]` and a destination
    /// variable range of \[-14000, 14000\] scales automatically.
    ///
    /// When `None`, the value is written as-is.
    #[traverse(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remap_from_interval: Option<NumInterval<BaseNumT>>,

    /// The target to write to.
    ///
    /// Accepts any valid `ValueDst`:
    /// - Device control: `{ dev: <device>, ctl: <control> }`
    /// - Variable: `{ var: <name> }`
    /// - `null` — void destination (writes silently discarded).
    #[serde(default)]
    pub(crate) destination: ValueDsts,
}

impl DuplicateWithNewState for TfmStepCfg {}

impl WithRuntimeId for TfmStepCfg {
    fn get_id(&self) -> ObjId {
        self.common_state_ref().get_id()
    }
    fn assign_new_id(&mut self) {
        self.common_state_assign_new();
    }
}

impl WithDescriptionMut for TfmStepCfg {
    fn description_mut(&mut self) -> Option<&mut DescriptionCfg> {
        match self {
            TfmStepCfg::Nop(_) => None,
            TfmStepCfg::Invert(_) => None,
            TfmStepCfg::Integrate(integrate) => Some(&mut integrate.desc),
            TfmStepCfg::Steering(steering) => Some(&mut steering.desc),
            TfmStepCfg::Clamp(clamp) => Some(&mut clamp.desc),
            TfmStepCfg::RaiseFall(raise_fall) => Some(&mut raise_fall.desc),
            TfmStepCfg::Ema(ema) => Some(&mut ema.desc),
            TfmStepCfg::Linear(linear) => Some(&mut linear.desc),
            TfmStepCfg::Smoothstep(smoothstep) => Some(&mut smoothstep.desc),
            TfmStepCfg::SCurve(s_curve) => Some(&mut s_curve.desc),
            TfmStepCfg::Exp(exp) => Some(&mut exp.desc),
            TfmStepCfg::SignedPower(signed_power) => Some(&mut signed_power.desc),
            TfmStepCfg::OneEuro(one_euro) => Some(&mut one_euro.desc),
            TfmStepCfg::Script(script) => Some(&mut script.desc),
            TfmStepCfg::_HighPass(highpass) => Some(&mut highpass.desc),
            TfmStepCfg::_ForceFeedback(force_feedback) => Some(&mut force_feedback.desc),
        }
    }
}

mod tests {
    #[allow(unused)]
    use super::*;

    #[test]
    fn clamp_cfg_sanitize() {
        let mut clamp_cfg = ClampCfg::default();
        clamp_cfg.common_state_mut().set_input_interval((3.0..4.0).into());

        clamp_cfg.range = (-100.0..-100.0).into();
        clamp_cfg.sanitize_inplace();
        assert!(clamp_cfg.range.to == 3.0);

        clamp_cfg.range = (100.0..100.0).into();
        clamp_cfg.sanitize_inplace();
        assert!(clamp_cfg.range.from == 4.0);
    }

    #[test]
    fn default_on_idle_is_true() {
        assert!(is_true(&default_on_idle()));
    }
}

// =================================================================
