use crate::common::BaseNumT;
use crate::schemas_value::{DescriptionCfg, InputValueMetadata, WithDescriptionMut};
use crate::schemas_value::{
    DeviceControlMatcherRef, DynValueRefs, ValueDsts, VariableRef, WithNumInterval, WithRelativityRef,
    serialize_value_src_rt_ignore_interval,
};
use crate::{
    common::{Relativity, SYMM_UNIT_INTERVAL, UNIT_INTERVAL},
    num_interval::NumInterval,
    schemas_common::*,
    schemas_value::{StaticValueCfg, ValueSrcs},
    tracing::TraceChannel,
};
use atomic_float::AtomicF32;
use bitflags::bitflags;
use crossbeam_utils::CachePadded;
use doc_for::*;
use garde::Validate;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use schemars::JsonSchema;
use serde::de::IntoDeserializer;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::{str::FromStr, sync::Arc};
use strum::IntoEnumIterator;
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

const fn default_1euro_beta() -> BaseNumT {
    0.007
}

const fn default_1euro_d_cutoff_hz() -> BaseNumT {
    1.0
}

const fn default_1euro_min_cutoff_hz() -> BaseNumT {
    1.0
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

const fn default_ema_tau() -> BaseNumT {
    0.04
}

// ============================================================

pub(crate) trait DuplicateTfmTree
where
    Self: Clone + TraversableMut + WithRuntimeId,
{
    fn duplicate_tfm_tree_with_new_state(&self) -> Self {
        struct RuntimeInfoRenewVisitor {}
        impl traversable::VisitorMut for RuntimeInfoRenewVisitor {
            type Break = ();

            fn enter_mut(&mut self, this: &mut dyn core::any::Any) -> std::ops::ControlFlow<Self::Break> {
                if let Some(v) = this.downcast_mut::<TfmStepCfg>() {
                    v.assign_new_state();
                } else if let Some(v) = this.downcast_mut::<TfmSeqCfg>() {
                    v.assign_new_id();
                }
                std::ops::ControlFlow::Continue(())
            }
        }
        let mut duplicate = self.clone();
        let _ = duplicate.traverse_mut(&mut RuntimeInfoRenewVisitor {});
        duplicate
    }
}

// ============================================================
pub(crate) struct TfmStepState {
    id: ObjId,
    intervals: (NumInterval<BaseNumT>, NumInterval<BaseNumT>),
    relativity: (Relativity, Relativity),
    #[cfg(feature = "gui")]
    pub(crate) gui_trace_graph_opened: bool,
    pub(crate) last_in: CachePadded<AtomicF32>,
    pub(crate) last_out: CachePadded<AtomicF32>,
    pub(crate) trace_channel: Option<Arc<TraceChannel>>,
}

impl WithRuntimeState for TfmStepCfg {
    fn assign_new_state(&mut self) {
        *self.get_state_arc_mut() = Default::default()
    }

    type StateT = TfmStepState;
}

impl Default for TfmStepState {
    fn default() -> Self {
        Self {
            id: Default::default(),
            intervals: (NumInterval::default(), NumInterval::default()),
            relativity: (Relativity::Abs, Relativity::Abs),
            #[cfg(feature = "gui")]
            gui_trace_graph_opened: false,
            trace_channel: None,
            last_in: CachePadded::new(AtomicF32::new(0.0)),
            last_out: CachePadded::new(AtomicF32::new(0.0)),
        }
    }
}

impl WithRuntimeId for TfmStepState {
    fn get_id(&self) -> ObjId {
        self.id
    }

    fn assign_new_id(&mut self) {
        self.id = Default::default()
    }
}

// ===========================================================

impl TfmStepState {
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

impl std::fmt::Debug for TfmStepState {
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
        TfmStepCfg::Nop {
            state: TfmStepStateShared::new(),
            nop: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TfmStepStateShared(pub(crate) Arc<RwLock<TfmStepState>>);
impl TfmStepStateShared {
    pub(crate) fn new() -> Self {
        Default::default()
    }
}

impl PartialEq for TfmStepStateShared {
    fn eq(&self, other: &Self) -> bool {
        self.0.read().get_id() == other.0.read().get_id()
    }
}

#[derive(
    JsonSchema, Display, Debug, Serialize, EnumString, EnumIter, Traversable, TraversableMut, Clone, PartialEq, Validate,
)]
#[strum(serialize_all = "snake_case")]
#[serde(untagged)]
pub(crate) enum TfmStepCfg {
    Nop {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        nop: bool,
    },
    Invert {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        invert: InvertCfg,
    },
    Integrate {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        integrate: IntegrateCfg,
    },
    Steering {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[garde(skip)]
        steering: Box<SteeringCfg>,
    },
    Clamp {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        clamp: ClampCfg,
    },
    RaiseFall {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[garde(skip)]
        raise_fall: RaiseFallCfg,
    },
    Ema {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        ema: EmaFilterCfg,
    },
    Linear {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        linear: LinearCfg,
    },
    Smoothstep {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        smoothstep: SmoothstepCfg,
    },
    SCurve {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        s_curve: SCurveCfg,
    },
    #[strum(serialize = "exp")]
    NormExp {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        exp: NormExpCfg,
    },
    SignedPower {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        signed_power: SignedPowerCfg,
    },
    OneEuro {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        one_euro: OneEuroFilterCfg,
    },
    Script {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[garde(skip)]
        script: ScriptCfg,
    },
    #[strum(disabled)]
    _HighPass {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        highpass: HighPassCfg,
    },
    #[strum(disabled)]
    _ForceFeedback {
        #[serde(skip)]
        #[traverse(skip)]
        #[garde(skip)]
        state: TfmStepStateShared,
        #[traverse(skip)]
        #[garde(skip)]
        force_feedback: ForceFeedbackCfg,
    },
}

impl TfmStepCfg {
    pub(crate) fn get_enabled_ref_mut(&mut self) -> &mut bool {
        match self {
            Self::Nop { nop, .. } => nop,
            Self::Invert { invert, .. } => &mut invert.enabled,
            Self::Integrate { integrate, .. } => &mut integrate.enabled,
            Self::Steering { steering, .. } => &mut steering.enabled,
            Self::Clamp { clamp, .. } => &mut clamp.enabled,
            Self::RaiseFall { raise_fall, .. } => &mut raise_fall.enabled,
            Self::Ema { ema, .. } => &mut ema.enabled,
            Self::Linear { linear, .. } => &mut linear.enabled,
            Self::Smoothstep {
                smoothstep: smoothstep_curve,
                ..
            } => &mut smoothstep_curve.enabled,
            Self::SCurve { s_curve, .. } => &mut s_curve.enabled,
            Self::NormExp {
                exp: norm_exp_curve, ..
            } => &mut norm_exp_curve.enabled,
            Self::SignedPower {
                signed_power: signed_power_curve,
                ..
            } => &mut signed_power_curve.enabled,
            Self::OneEuro { one_euro, .. } => &mut one_euro.enabled,
            Self::Script { script, .. } => &mut script.enabled,
            Self::_HighPass { highpass, .. } => &mut highpass.enabled,
            Self::_ForceFeedback { force_feedback, .. } => &mut force_feedback.enabled,
        }
    }

    pub(crate) fn get_state_arc(&self) -> &Arc<RwLock<TfmStepState>> {
        match self {
            Self::Nop { state, .. }
            | Self::Invert { state, .. }
            | Self::Integrate { state, .. }
            | Self::Steering { state, .. }
            | Self::Clamp { state, .. }
            | Self::RaiseFall { state, .. }
            | Self::Ema { state, .. }
            | Self::Linear { state, .. }
            | Self::Smoothstep { state, .. }
            | Self::SCurve { state, .. }
            | Self::NormExp { state, .. }
            | Self::SignedPower { state, .. }
            | Self::OneEuro { state, .. }
            | Self::Script { state, .. }
            | Self::_HighPass { state, .. }
            | Self::_ForceFeedback { state, .. } => &state.0,
        }
    }

    pub(crate) fn get_state_arc_mut(&mut self) -> &mut Arc<RwLock<TfmStepState>> {
        match self {
            Self::Nop { state, .. }
            | Self::Invert { state, .. }
            | Self::Integrate { state, .. }
            | Self::Steering { state, .. }
            | Self::Clamp { state, .. }
            | Self::RaiseFall { state, .. }
            | Self::Ema { state, .. }
            | Self::Linear { state, .. }
            | Self::Smoothstep { state, .. }
            | Self::SCurve { state, .. }
            | Self::NormExp { state, .. }
            | Self::SignedPower { state, .. }
            | Self::OneEuro { state, .. }
            | Self::Script { state, .. }
            | Self::_HighPass { state, .. }
            | Self::_ForceFeedback { state, .. } => &mut state.0,
        }
    }

    pub(crate) fn get_state(&self) -> RwLockReadGuard<'_, TfmStepState> {
        self.get_state_arc().read()
    }

    pub(crate) fn get_state_as_mut(&self) -> RwLockWriteGuard<'_, TfmStepState> {
        self.get_state_arc().write()
    }

    pub(crate) fn clone_with_new_state_no_recurse(&self) -> Self {
        let mut cloned = self.clone();
        *cloned.get_state_arc_mut() = TfmStepStateShared::new().0.clone();
        cloned
    }
}

// ===========================================================

fn step_parse_result(key: &str) -> anyhow::Result<TfmStepCfg> {
    let r = TfmStepCfg::from_str(key).map_err(|_| {
        anyhow::anyhow!(
            "Unexpected transformation step \"{key}\" found. Supported steps are: {:?}",
            TfmStepCfg::iter()
                .map(|c: TfmStepCfg| c.to_string())
                .collect::<Vec<_>>()
        )
    })?;

    Ok(r)
}

impl<'de> Deserialize<'de> for TfmStepCfg {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;
        struct StepVisitor {}

        impl<'de> Visitor<'de> for StepVisitor {
            type Value = TfmStepCfg;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map with exactly one transformation step key (e.g. { clamp: {...} }) or `invert`")
            }

            fn visit_str<E>(self, key: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let step: TfmStepCfg = step_parse_result(key).map_err(|e| de::Error::custom(e))?;

                match step {
                    TfmStepCfg::Invert { state, .. } => Ok(TfmStepCfg::Invert {
                        state,
                        invert: InvertCfg { enabled: true },
                    }),
                    _ => Err(de::Error::custom("Expected invert")),
                }
            }

            fn visit_string<E>(self, key: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(&key)
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let Some(key) = map.next_key::<String>()? else {
                    return Err(de::Error::custom("empty transformation step object"));
                };

                type S = TfmStepCfg;
                let step = match step_parse_result(key.as_str()).map_err(de::Error::custom)? {
                    S::Nop { .. } => S::Nop {
                        nop: map.next_value()?,
                        state: Default::default(),
                    },
                    S::Invert { .. } => S::Invert {
                        invert: map.next_value()?,
                        state: Default::default(),
                    },
                    S::Integrate { .. } => S::Integrate {
                        integrate: map.next_value()?,
                        state: Default::default(),
                    },
                    S::Steering { .. } => S::Steering {
                        steering: map.next_value()?,
                        state: Default::default(),
                    },
                    S::Clamp { .. } => S::Clamp {
                        clamp: map.next_value()?,
                        state: Default::default(),
                    },
                    S::RaiseFall { .. } => S::RaiseFall {
                        raise_fall: map.next_value()?,
                        state: Default::default(),
                    },
                    S::Ema { .. } => S::Ema {
                        ema: map.next_value()?,
                        state: Default::default(),
                    },
                    S::Linear { .. } => S::Linear {
                        linear: map.next_value()?,
                        state: Default::default(),
                    },
                    S::Smoothstep { .. } => S::Smoothstep {
                        smoothstep: map.next_value()?,
                        state: Default::default(),
                    },
                    S::SCurve { .. } => S::SCurve {
                        s_curve: map.next_value()?,
                        state: Default::default(),
                    },
                    S::NormExp { .. } => S::NormExp {
                        exp: map.next_value()?,
                        state: Default::default(),
                    },
                    S::SignedPower { .. } => S::SignedPower {
                        signed_power: map.next_value()?,
                        state: Default::default(),
                    },
                    S::OneEuro { .. } => S::OneEuro {
                        one_euro: map.next_value()?,
                        state: Default::default(),
                    },
                    S::Script { .. } => S::Script {
                        script: map.next_value()?,
                        state: Default::default(),
                    },
                    S::_HighPass { .. } => todo!(),
                    S::_ForceFeedback { .. } => todo!(),
                };

                // Enforce "exactly one key" (catch typos like { clamp: {...}, foo: 1 })
                if let Some(extra) = map.next_key::<String>()? {
                    return Err(de::Error::custom(format!(
                        "transformation step must contain exactly one key; found extra key '{}' after '{}'",
                        extra, key
                    )));
                }

                Ok(step)
            }
        }

        deserializer.deserialize_any(StepVisitor {})
    }
}

// pub(crate) type TfmStep = TfmStepGen<ForceFeedbackCfg, SteeringCfg, RaiseFallCfg>;

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

#[derive(
    JsonSchema, Debug, Clone, Traversable, TraversableMut, Serialize, Deserialize, PartialEq, Default, Validate,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct ForceFeedbackCfg {
    #[traverse(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[serde(default = "default_ff_gain")]
    #[garde(range(min = 0.0))] // Gain magnitude (invert is handled via a separate boolean)
    pub(crate) gain: BaseNumT,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) invert: bool,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) component: ForceFeedbackComponent,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "wheel_hold_factor")]
    #[garde(skip)]
    pub(crate) hold_factor: Option<ValueSrcs>,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) transformation: TfmSeqCfg,
    #[serde(default)]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    pub(crate) external_gain_control_enabled: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    pub(crate) external_autocentering_control_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub(crate) constant: Option<Box<ForceFeedbackCfg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub(crate) spring: Option<Box<ForceFeedbackCfg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub(crate) friction: Option<Box<ForceFeedbackCfg>>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub(crate) custom_source: Option<ValueSrcs>,
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClampCfg {
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,
    pub(crate) from: Option<BaseNumT>,
    pub(crate) to: Option<BaseNumT>,
    #[serde(default = "default_clamp_transform_override_interval")]
    pub(crate) override_range: bool,
}

impl Default for ClampCfg {
    fn default() -> Self {
        Self {
            desc: Default::default(),
            enabled: default_step_enabled(),
            from: Default::default(),
            to: Default::default(),
            override_range: Default::default(),
        }
    }
}

impl ClampCfg {
    pub(crate) fn get_clamping_interval(&self, in_interval: NumInterval<BaseNumT>) -> NumInterval<BaseNumT> {
        NumInterval::new(
            self.from.unwrap_or(in_interval.from()),
            self.to.unwrap_or(in_interval.to()),
        )
    }
    pub(crate) fn get_out_interval(&self, in_interval: NumInterval<BaseNumT>) -> NumInterval<BaseNumT> {
        if self.override_range {
            self.get_clamping_interval(in_interval)
        } else {
            in_interval
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvertCfg {
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,
}

impl Default for InvertCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmaFilterCfg {
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[serde(default = "default_false")]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    pub(crate) on_relative_input_feed_on_idle: bool,
    #[serde(default = "default_false")]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    pub(crate) on_relative_input_reset_on_idle: bool,
    #[serde(default = "default_ema_tau")]
    #[garde(range(min = 0.0))]
    pub(crate) tau: BaseNumT,
}

impl Default for EmaFilterCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            on_relative_input_feed_on_idle: default_false(),
            tau: 0.01,
            on_relative_input_reset_on_idle: default_false(),
            desc: Default::default(),
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct OneEuroFilterCfg {
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[serde(default = "default_false")]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    pub(crate) on_relative_input_feed_on_idle: bool,
    #[serde(default = "default_false")]
    #[serde(skip_serializing_if = "is_false")]
    #[garde(skip)]
    pub(crate) on_relative_input_reset_on_idle: bool,
    #[serde(default = "default_1euro_beta")]
    #[garde(range(min = 0.0))]
    pub(crate) beta: BaseNumT,
    #[serde(default = "default_1euro_min_cutoff_hz")]
    #[garde(range(min = 0.0))]
    pub(crate) min_cutoff_hz: BaseNumT,
    #[serde(default = "default_1euro_d_cutoff_hz")]
    #[garde(range(min = 0.0))]
    pub(crate) d_cutoff_hz: BaseNumT,
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
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinearCfg {
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
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SmoothstepCfg {
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
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct SCurveCfg {
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
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignedPowerCfg {
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
        }
    }
}

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Default, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct HighPassCfg {
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

#[derive(JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntegrateCfg {
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

impl Default for IntegrateCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            range: NumInterval::new(-100.0, 100.0),
            deadzone_norm: 0.0,
            smoothing_alpha: default_smoothing_alpha(),
            on_idle: default_on_idle(),
            desc: Default::default(),
        }
    }
}

// ==================================================================
impl TfmStepCfg {
    #[allow(unused)]
    fn get_input_interval(&self) {}

    #[allow(unused)]
    fn get_output_interval(&self) {}

    #[allow(unused)]
    fn set_current_interval(&mut self, interval: NumInterval<BaseNumT>) {
        todo!()
    }
}

impl TfmSeqCfg {
    #[cfg(feature = "gui")]
    pub(crate) fn disable_gui_tracing(&mut self) {
        for step in &mut self.steps {
            step.get_state_as_mut().disable_gui_tracing();
        }
    }

    pub(crate) fn recompute_metadata_with_known_inputs(&mut self) {
        self.recompute_metadata(self.in_meta.clone());
    }

    pub(crate) fn recompute_metadata(&mut self, input: AutoOrManual<InputValueMetadata<BaseNumT>>) {
        self.in_meta = input;
        let mut in_relativity = self.in_meta.relativity;
        let mut in_interval = self.in_meta.interval;
        for step in &mut self.steps {
            let (out_interval, out_relativity) = match step {
                TfmStepCfg::Script { script, .. } => {
                    script
                        .aux_transformations
                        .iter_mut()
                        .for_each(|t| t.1.recompute_metadata(t.1.in_meta));
                    (
                        script.output_interval.unwrap_or(in_interval),
                        script.output_relativity.unwrap_or(in_relativity),
                    )
                }
                TfmStepCfg::Integrate { integrate, .. } if integrate.enabled => (integrate.range, Relativity::Abs),
                TfmStepCfg::Steering { steering, .. } if steering.enabled => {
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
                                relativity: Relativity::Abs,
                            }));
                    };
                    (SYMM_UNIT_INTERVAL, Relativity::Abs)
                }
                TfmStepCfg::_ForceFeedback { force_feedback, .. } if force_feedback.enabled => {
                    (SYMM_UNIT_INTERVAL, Relativity::Abs)
                }
                TfmStepCfg::Clamp { clamp, .. } if clamp.enabled => {
                    (clamp.get_out_interval(in_interval), in_relativity)
                }
                TfmStepCfg::Nop { .. }
                | TfmStepCfg::Invert { .. }
                | TfmStepCfg::Integrate { .. }
                | TfmStepCfg::Steering { .. }
                | TfmStepCfg::Clamp { .. }
                | TfmStepCfg::RaiseFall { .. }
                | TfmStepCfg::Ema { .. }
                | TfmStepCfg::Linear { .. }
                | TfmStepCfg::Smoothstep { .. }
                | TfmStepCfg::SCurve { .. }
                | TfmStepCfg::NormExp { .. }
                | TfmStepCfg::SignedPower { .. }
                | TfmStepCfg::OneEuro { .. }
                | TfmStepCfg::_HighPass { .. }
                | TfmStepCfg::_ForceFeedback { .. } => (in_interval, in_relativity),
            };

            step.get_state_as_mut()
                .set_input_relativity(in_relativity)
                .set_input_interval(in_interval)
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
                    "Configuration mismatch:\n\nIf using steps list only: {}\n\nIf using steps + input spec: {}\n\n",
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
    (vis: $v:vis, name: $name:ident, meta: $( $m:meta ),*) => {
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

tfm_seq_tpl!(vis:, name: TfmSeqFull, meta: );
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

impl<T: Copy + Default> Copy for AutoOrManual<T> {}

impl<T: Default> AutoOrManual<T> {
    pub(crate) fn is_auto(&self) -> bool {
        if let Self::Auto(_) = self { true } else { false }
    }

    pub(crate) fn _set(&mut self, other: T) {
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

#[derive(Debug, Clone, Serialize, Traversable, TraversableMut, Deserialize, JsonSchema, PartialEq, Validate)]
pub(crate) struct SteeringCfg {
    #[traverse(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    #[garde(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    #[garde(skip)]
    pub(crate) enabled: bool,
    #[garde(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) accumulator: Option<DynValueRefs>,
    #[allow(dead_code)]
    #[serde(default)]
    #[garde(range(min = 0.0))]
    pub(crate) deadzone_counts: BaseNumT,
    //#[garde(range(min = 0.0, max = 1.0))]
    #[garde(skip)]
    #[serde(alias = "smoothing_alpha")]
    #[serde(alias = "input_sensitivity")]
    pub(crate) input_gain: ValueSrcs,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) auto_center_halflife: ValueSrcs,
    #[serde(default)]
    #[garde(skip)]
    #[serde(deserialize_with = "deserialize_bool_or_value_src")]
    pub(crate) auto_center_along_force_feedback: ValueSrcs,
    #[serde(default)]
    #[serde(serialize_with = "serialize_value_src_rt_ignore_interval")]
    #[garde(skip)]
    pub(crate) hold_factor: ValueSrcs,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) force_feedback: Option<ForceFeedbackCfg>,
    #[serde(default)]
    #[garde(skip)]
    pub(crate) integrated_user_input_transform: TfmSeqCfg,
}

impl Default for SteeringCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            deadzone_counts: 0.0,
            input_gain: ValueSrcs::Static(StaticValueCfg {
                value: default_steering_smoothing_alpha(),
                interval: UNIT_INTERVAL,
            }),
            auto_center_halflife: ValueSrcs::Static(StaticValueCfg {
                value: default_steering_transform_auto_center_halflife(),
                interval: UNIT_INTERVAL,
            }),
            auto_center_along_force_feedback: ValueSrcs::Static(StaticValueCfg {
                value: 0.0,
                interval: UNIT_INTERVAL,
            }),
            hold_factor: ValueSrcs::Static(StaticValueCfg {
                value: 0.0,
                interval: UNIT_INTERVAL,
            }),
            force_feedback: None,
            integrated_user_input_transform: TfmSeqCfg::default(),
            desc: Default::default(),
            accumulator: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Deserialize, JsonSchema, Traversable, TraversableMut, Validate)]
pub(crate) struct RaiseFallCfg {
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

impl Default for RaiseFallCfg {
    fn default() -> Self {
        Self {
            enabled: default_step_enabled(),
            raise_rate: Default::default(),
            fall_rate: Default::default(),
            smoothing_alpha: Default::default(),
            fall_delay: Default::default(),
            fall_hold_factor: ValueSrcs::Static(StaticValueCfg {
                value: 1.0,
                interval: UNIT_INTERVAL,
            }),
            invert_fall_hold_factor: false,
            desc: Default::default(),
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

// -------------------------------------

#[derive(JsonSchema, Display, Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub(crate) enum ScriptLanguage {
    #[default]
    Luau,
}

#[derive(Clone, Serialize, Deserialize, JsonSchema, Debug, TraversableMut, Traversable, PartialEq)]
pub(crate) struct ScriptCfg {
    #[traverse(skip)]
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_step_enabled")]
    pub(crate) enabled: bool,
    #[serde(default)]
    #[traverse(skip)]
    pub(crate) lang: ScriptLanguage,
    #[serde(default)]
    #[traverse(skip)]
    pub(crate) script: String,
    #[traverse(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_interval: Option<NumInterval<BaseNumT>>,
    #[traverse(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_relativity: Option<Relativity>,

    #[serde(default)]
    #[serde(alias = "sources")]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_btree_or_vec")]
    pub(crate) aux_srcs: BTreeMap<String, ScriptSourceCfg>,

    #[serde(default)]
    #[serde(alias = "destinations")]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_btree_or_vec")]
    pub(crate) aux_dsts: BTreeMap<String, ScriptDestinationCfg>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_btree_or_vec")]
    pub(crate) aux_transformations: BTreeMap<String, TfmSeqCfg>,
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
            value: if b { 1.0 } else { 0.0 },
            interval: UNIT_INTERVAL,
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

#[derive(Clone, Serialize, Deserialize, JsonSchema, Debug, TraversableMut, Traversable, Default, PartialEq)]
pub(crate) struct ScriptSourceCfg {
    #[traverse(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remap_to_interval: Option<NumInterval<BaseNumT>>,
    // #[serde(default)]
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub(crate) transformation: Option<TfmSeqCfg>,
    #[serde(default)]
    pub(crate) source: ValueSrcs,
}

#[derive(Clone, Serialize, Deserialize, JsonSchema, Debug, TraversableMut, Traversable, Default, PartialEq)]
pub(crate) struct ScriptDestinationCfg {
    #[traverse(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remap_from_interval: Option<NumInterval<BaseNumT>>,
    // #[serde(default)]
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub(crate) transformation: Option<TfmSeqCfg>,
    #[serde(default)]
    pub(crate) destination: ValueDsts,
}

impl DuplicateTfmTree for TfmStepCfg {}

impl WithRuntimeId for TfmStepCfg {
    fn get_id(&self) -> ObjId {
        self.get_state().get_id()
    }

    fn assign_new_id(&mut self) {
        self.assign_new_state();
    }
}

impl WithDescriptionMut for TfmStepCfg {
    fn description_mut(&mut self) -> Option<&mut DescriptionCfg> {
        match self {
            TfmStepCfg::Nop { .. } => None,
            TfmStepCfg::Invert { .. } => None,
            TfmStepCfg::Integrate { integrate, .. } => Some(&mut integrate.desc),
            TfmStepCfg::Steering { steering, .. } => Some(&mut steering.desc),
            TfmStepCfg::Clamp { clamp, .. } => Some(&mut clamp.desc),
            TfmStepCfg::RaiseFall { raise_fall, .. } => Some(&mut raise_fall.desc),
            TfmStepCfg::Ema { ema, .. } => Some(&mut ema.desc),
            TfmStepCfg::Linear { linear, .. } => Some(&mut linear.desc),
            TfmStepCfg::Smoothstep { smoothstep, .. } => Some(&mut smoothstep.desc),
            TfmStepCfg::SCurve { s_curve, .. } => Some(&mut s_curve.desc),
            TfmStepCfg::NormExp { exp, .. } => Some(&mut exp.desc),
            TfmStepCfg::SignedPower { signed_power, .. } => Some(&mut signed_power.desc),
            TfmStepCfg::OneEuro { one_euro, .. } => Some(&mut one_euro.desc),
            TfmStepCfg::Script { script, .. } => Some(&mut script.desc),
            TfmStepCfg::_HighPass { highpass, .. } => Some(&mut highpass.desc),
            TfmStepCfg::_ForceFeedback { force_feedback, .. } => Some(&mut force_feedback.desc),
        }
    }
}

mod tests {
    #[allow(unused)]
    use super::*;

    #[test]
    fn default_on_idle_is_true() {
        assert!(is_true(&default_on_idle()));
    }
}
