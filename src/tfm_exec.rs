use crate::config::WithSelfSanitize;
use crate::filters::OneEuroFilter;
use crate::filters::clamp_dt_by_min_and_max_period;
use crate::num_interval::SYMM_UNIT_INTERVAL;
use crate::num_interval::UNIT_INTERVAL;

use crate::base_num::BaseNumT;
use crate::curves_and_linear::*;
use crate::debug::get_debug_level;
#[cfg(feature = "gui")]
use crate::gui_transform_step::TfmStepTraceStage;
use crate::num_interval::{NumInterval, OutOfRangePolicy};
use crate::relativity::Relativity;

use crate::schemas_common::WithRuntimeId;

use crate::schemas_transform::ArithCfg;
use crate::schemas_transform::ArithOpType;
use crate::schemas_transform::TfmCfgDuplicateWithNewState;
use crate::schemas_transform::VelocityToDisplacementCfg;
use crate::schemas_transform::WithCommonState;
use crate::schemas_transform::{
    ClampCfg, EmaCfg, ForceFeedbackComponent, IntegrateCfg, InvertCfg, LinearCfg, NormExpCfg, OneEuroFilterCfg,
    RaiseFallCfg, SCurveCfg, ScriptCfg, SignedPowerCfg, SmoothstepCfg, SteeringCfg, TfmSeqCfg, TfmStepCfg,
};

use crate::schemas_value::DeviceControlMatcherRef;
use crate::schemas_value::WithLastKnownIOSettable;
use crate::schemas_value::{DynValueRefs, ValueDsts, WithNumInterval, WithRelativity};
use crate::schemas_value::{TfmValue, WithNumericValue};

use crate::schemas_value_port::ValuePortIface;
#[cfg(feature = "gui")]
use crate::tracing::GraphDisplayStyle;
#[cfg(feature = "gui")]
use eframe::egui::Color32;
use log::debug;
use mlua::ErrorContext;
use mlua::{FromLua, Lua};
use std::ops::Add;
use std::time::Instant;

pub(crate) trait TfmExeState {
    type StateMutT<'a>
    where
        Self: 'a;
    type ResetInput<'b>;
    fn exe_state_mut(&self) -> Self::StateMutT<'_>;
    fn exe_state_reset(&self, reset_with: Self::ResetInput<'_>);
}

impl TfmExecCtx for () {}

#[allow(unused)]
pub(crate) trait TfmExecCtx {
    fn get_dt(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }

    fn is_idle_tick(&self) -> bool {
        false
    }

    fn get_idle_tick_rate(&self) -> u32 {
        crate::config::MIN_BASE_FREQ_HZ
    }

    fn get_main_dst(&self) -> Option<&ValueDsts> {
        None
    }

    #[deprecated = "Raw usage of this API is deprecated, replace with ValuePort usage when API is complete."]
    fn device_control_matcher_ref_write(&self, dcm_ref: &DeviceControlMatcherRef, value: BaseNumT) {}

    fn get_lua(&self) -> Option<&mlua::Lua> {
        None
    }

    fn get_ff_x(&self, device_matcher_key: &str) -> BaseNumT {
        Default::default()
    }

    fn get_ff_y(&self, device_matcher_key: &str) -> BaseNumT {
        Default::default()
    }

    fn set_ff_x_axis_pos(
        &self,
        device_matcher_key: &str,
        device_control_matcher_key: &str,
        normalize_from: NumInterval<BaseNumT>,
    ) {
    }

    fn set_ff_y_axis_pos(
        &self,
        device_matcher_key: &str,
        device_control_matcher_key: &str,
        normalize_from: NumInterval<BaseNumT>,
    ) {
    }
}

pub(crate) trait WithTfmExec: TfmCfgDuplicateWithNewState + WithSelfSanitize {
    type InputT;
    type OutputT;
    fn exec(&self, input: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT;
}

impl WithTfmExec for TfmSeqCfg {
    type InputT = TfmValue<BaseNumT>;
    type OutputT = Self::InputT;
    fn exec(&self, mut input: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        let in_interval = self.get_in_interval();

        if input.interval != in_interval {
            input.value = in_interval.map_from(input.value, &input.interval, OutOfRangePolicy::Clamp);
            input.interval = in_interval;
        }

        let mut value = input.value;

        self.set_last_known_io((Some(value), None));
        for step in &self.steps {
            value = step.exec(value, ctx);
        }
        self.set_last_known_io((None, Some(value)));

        input.interval = self.get_out_interval();
        input.relativity = self.get_out_relativity();
        input.value = input.interval.clamp(value);

        input
    }
}

impl WithTfmExec for TfmStepCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        let common_step_data = self.common_state_ref();
        common_step_data.set_last_known_io((Some(value), None));

        #[cfg(feature = "gui")]
        common_step_data.gui_trace(
            TfmStepTraceStage::In,
            value,
            common_step_data.get_in_interval(),
            Instant::now(),
        );

        value = match self {
            TfmStepCfg::Sum(s) => s.exec((ArithOpType::Sum, value), ctx),
            TfmStepCfg::Sub(s) => s.exec((ArithOpType::Sub, value), ctx),
            TfmStepCfg::Mul(s) => s.exec((ArithOpType::Mul, value), ctx),
            TfmStepCfg::Div(s) => s.exec((ArithOpType::Div, value), ctx),
            TfmStepCfg::VelocityToDisplacement(s) => s.exec(value, ctx),
            TfmStepCfg::Nop(_) => value,
            TfmStepCfg::Invert(s) => s.exec(value, ctx),
            TfmStepCfg::Integrate(s) => s.exec(value, ctx),
            TfmStepCfg::Steering(s) => s.exec(value, ctx),
            TfmStepCfg::Clamp(s) => s.exec(value, ctx),
            TfmStepCfg::RaiseFall(s) => s.exec(value, ctx),
            TfmStepCfg::Ema(s) => s.exec(value, ctx),
            TfmStepCfg::Linear(s) => s.exec(value, ctx),
            TfmStepCfg::Smoothstep(s) => s.exec(value, ctx),
            TfmStepCfg::SCurve(s) => s.exec(value, ctx),
            TfmStepCfg::Exp(s) => s.exec(value, ctx),
            TfmStepCfg::SignedPower(s) => s.exec(value, ctx),
            TfmStepCfg::OneEuro(s) => s.exec(value, ctx),
            TfmStepCfg::Script(s) => s.exec(value, ctx),
        };

        if !common_step_data.get_out_interval().contains_value_closed(value) {
            branches::mark_unlikely();
            if get_debug_level().is_mid_or_above() {
                branches::mark_unlikely();
                log::warn!(
                    "Value {} must fit in interval {} after transformation step ``{}'' (ID: {}). \
                         Each step must ensure it, clamping!",
                    value,
                    common_step_data.get_out_interval(),
                    self,
                    self.get_id()
                );
            }
            value = common_step_data.get_out_interval().clamp(value);
        }

        #[cfg(feature = "gui")]
        self.common_state_ref().gui_trace(
            TfmStepTraceStage::Out,
            value,
            common_step_data.get_out_interval(),
            Instant::now(),
        );

        self.common_state_ref().set_last_known_io((None, Some(value)));

        value
    }
}

impl WithTfmExec for ArithCfg {
    type InputT = (ArithOpType, BaseNumT);
    type OutputT = BaseNumT;
    fn exec(&self, mut input: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if branches::unlikely(!*self.enabled) {
            return input.1;
        }
        self.sources.iter().for_each(|src| {
            let v = src.port_get_numeric_value(Some(ctx));
            match input.0 {
                ArithOpType::Sum => input.1 += v,
                ArithOpType::Sub => input.1 -= v,
                ArithOpType::Mul => input.1 *= v,
                ArithOpType::Div => {
                    if v.is_normal() {
                        input.1 /= v
                    }
                }
            }
        });
        input.1
    }
}

impl WithTfmExec for VelocityToDisplacementCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, _ctx: &impl TfmExecCtx) -> Self::OutputT {
        if branches::unlikely(!*self.enabled) {
            return value;
        }
        value = self.out_interval.map_from(
            value * self.multiplier * (std::time::Instant::now() - self.last_time.get()).as_secs_f32() as BaseNumT,
            &self.common_state_ref().get_in_interval(),
            OutOfRangePolicy::Clamp,
        );
        self.last_time.set(std::time::Instant::now());
        value
    }
}

impl WithTfmExec for ClampCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, value: Self::InputT, _ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !*self.enabled {
            return value;
        }
        self.get_clamping_interval().clamp(value)
    }
}

impl TfmExeState for OneEuroFilterCfg {
    type StateMutT<'a>
        = std::sync::MutexGuard<'a, OneEuroFilter>
    where
        Self: 'a;
    type ResetInput<'b> = BaseNumT;

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.lock().unwrap()
    }

    fn exe_state_reset(&self, reset_with: Self::ResetInput<'_>) {
        self.exe_state_mut().reset(reset_with);
    }
}

impl WithTfmExec for OneEuroFilterCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if *self.enabled
            && (!ctx.is_idle_tick()
                || self.common_state_ref().get_in_relativity().is_absolute()
                || self.on_relative_input_feed_on_idle)
        {
            value = self.exe_state_mut().filter(
                value,
                Instant::now(),
                self.min_cutoff_hz.port_get_numeric_value(Some(ctx)),
                // .get_numeric_value_clamped_predicated(ClampPred::IfDynamic),
                self.beta.port_get_numeric_value(Some(ctx)), //get_numeric_value_clamped_predicated(ClampPred::IfDynamic),
                self.d_cutoff_hz.port_get_numeric_value(Some(ctx)), //.get_numeric_value_clamped_predicated(ClampPred::IfDynamic),
            );
        } else if self.on_relative_input_reset_on_idle {
            self.exe_state_reset(value);
        }
        value
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RaiseFallExeState {
    pub(crate) prev_out: BaseNumT,
    pub(crate) last_target: BaseNumT,
    pub(crate) prev_out_time: Instant,
    pub(crate) prev_user_input_time: Instant,
}

impl Default for RaiseFallExeState {
    fn default() -> Self {
        Self {
            prev_out: Default::default(),
            last_target: Default::default(),
            prev_out_time: Instant::now(),
            prev_user_input_time: Instant::now(),
        }
    }
}

impl TfmExeState for RaiseFallCfg {
    type StateMutT<'a>
        = std::sync::MutexGuard<'a, RaiseFallExeState>
    where
        Self: 'a;

    type ResetInput<'b> = Option<RaiseFallExeState>;

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.lock().unwrap()
    }

    fn exe_state_reset(&self, reset_with: Self::ResetInput<'_>) {
        *self.exe_state_mut() = reset_with.unwrap_or_default();
    }
}

impl WithTfmExec for Box<RaiseFallCfg> {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !*self.enabled {
            return value;
        }
        // TODO: warn application for non-abs values, but only once while sanitizing.
        // {
        //     log::warn!("Raise-fall transform should only be applied to absolute inputs.");
        // }

        let now = Instant::now();
        let mut filter_data = self.exe_state_mut();

        // NB: we do not clamp dt here as usecase is different from steering transform and filters.
        let dt = (now - filter_data.prev_out_time).as_secs_f32() as BaseNumT;
        let dt_user_input = (now - filter_data.prev_user_input_time).as_secs_f32() as BaseNumT;

        filter_data.prev_out_time = now;

        let target = if !ctx.is_idle_tick() {
            filter_data.last_target = value;
            value
        } else {
            filter_data.last_target
        };

        let mut final_out = filter_data.prev_out;
        if ctx.is_idle_tick() {
            let delta_v = target - filter_data.prev_out;
            let rate_limit = if delta_v > 0.0 {
                self.raise_rate
            } else {
                let mut fall_hold_factor = UNIT_INTERVAL.map_from(
                    self.fall_hold_factor.get_numeric_value(),
                    &self.fall_hold_factor.get_interval(),
                    OutOfRangePolicy::WarnIfDebugAndClamp,
                );

                if self.invert_fall_hold_factor {
                    fall_hold_factor = UNIT_INTERVAL.clamp_and_invert(fall_hold_factor);
                }

                if self.fall_delay > 0.0 {
                    if self.fall_delay < dt_user_input {
                        self.fall_rate * (1.0 - fall_hold_factor)
                    } else {
                        0.0
                    }
                } else {
                    self.fall_rate * (1.0 - fall_hold_factor)
                }
            };
            let max_delta = rate_limit * dt;
            let actual_delta = delta_v.clamp(-max_delta, max_delta);
            final_out = filter_data.prev_out + actual_delta;

            let smoothing_alpha = self.smoothing_alpha;
            final_out = (smoothing_alpha) * final_out + (1.0 - smoothing_alpha) * filter_data.prev_out;

            final_out = self.common_state_ref().get_out_interval().clamp(final_out);
            filter_data.prev_out = final_out;
        } else {
            filter_data.prev_user_input_time = now;
        }

        final_out
    }
}

impl TfmExeState for EmaCfg {
    type StateMutT<'a>
        = std::sync::MutexGuard<'a, crate::filters::EmaFilter>
    where
        Self: 'a;

    type ResetInput<'b> = BaseNumT;

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.lock().unwrap()
    }

    fn exe_state_reset(&self, reset_with: Self::ResetInput<'_>) {
        self.exe_state_mut().reset(reset_with);
    }
}

impl WithTfmExec for EmaCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if *self.enabled
            && (!ctx.is_idle_tick()
                || self.common_state_ref().get_in_relativity().is_absolute()
                || self.on_relative_input_feed_on_idle)
        {
            value = self
                .exe_state_mut()
                .filter(value, Instant::now(), self.tau.port_get_numeric_value(Some(ctx)));
        } else if self.on_relative_input_reset_on_idle {
            self.exe_state_reset(value);
        }
        value
    }
}

impl WithTfmExec for SignedPowerCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !(*self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return value;
        }

        let interval = self.common_state_ref().get_in_interval();
        value = if self.center_symmetric {
            apply_center_symmetric_with_abs_value(
                value,
                interval,
                |v_abs| signed_power(v_abs, self.power),
                OutOfRangePolicy::WarnIfDebugAndClamp,
            )
        } else {
            interval.map_from_unit(
                signed_power(
                    interval.map_to_unit(value, OutOfRangePolicy::WarnIfDebugAndClamp),
                    self.power,
                ),
                OutOfRangePolicy::WarnIfDebugAndClamp,
            )
        };
        value
    }
}

impl WithTfmExec for NormExpCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !(*self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return value;
        }

        let interval = self.common_state_ref().get_in_interval();

        value = if self.center_symmetric {
            apply_center_symmetric_with_abs_value(
                value,
                interval,
                |v_abs| exp_curve(v_abs, self.base),
                OutOfRangePolicy::WarnIfDebugAndClamp,
            )
        } else {
            interval.map_from_unit(
                exp_curve(
                    interval.map_to_unit(value, OutOfRangePolicy::WarnIfDebugAndClamp),
                    self.base,
                ),
                OutOfRangePolicy::WarnIfDebugAndClamp,
            )
        };
        value
    }
}

impl WithTfmExec for SCurveCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !(*self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return value;
        }

        let interval = self.common_state_ref().get_in_interval();
        value = interval.map_from_unit(
            s_curve(
                interval.map_to_unit(value, OutOfRangePolicy::WarnIfDebugAndClamp),
                self.steepness,
            ),
            OutOfRangePolicy::WarnIfDebugAndClamp,
        );
        value
    }
}

impl WithTfmExec for SmoothstepCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !(*self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return value;
        }

        let interval = self.common_state_ref().get_in_interval();
        value = interval.map_from_unit(
            smoothstep(interval.map_to_unit(value, OutOfRangePolicy::WarnIfDebugAndClamp)),
            OutOfRangePolicy::WarnIfDebugAndClamp,
        );
        value
    }
}

impl WithTfmExec for LinearCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !(*self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return value;
        }

        let interval = self.common_state_ref().get_in_interval();
        value = if self.center_symmetric {
            apply_center_symmetric_with_abs_value(
                value,
                interval,
                |abs_v| {
                    linear(
                        abs_v,
                        self.slope,
                        interval.map_to_symm_unit(self.shift_x, OutOfRangePolicy::Clamp),
                        interval.map_to_symm_unit(self.shift_y, OutOfRangePolicy::Clamp),
                    )
                },
                OutOfRangePolicy::Clamp,
            )
        } else {
            interval.clamp(linear(value, self.slope, self.shift_x, self.shift_y))
        };
        value
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ScriptExeState {
    #[cfg(feature = "gui")]
    pub(super) edit_epoch: usize,
    pub(crate) compiled: mlua::Function,
}

impl ScriptExeState {
    fn new(lua: &mlua::Lua) -> Self {
        let compiled = lua
            .load(" ")
            .into_function()
            .inspect_err(|e| log::error!("{e}"))
            .unwrap();
        Self {
            compiled,
            #[cfg(feature = "gui")]
            edit_epoch: Default::default(),
        }
    }
}

pub(crate) enum ScriptExeStateResetMode {
    #[cfg(feature = "gui")]
    Recompile {
        new_epoch: usize,
    },
    Init,
}

impl TfmExeState for ScriptCfg {
    type StateMutT<'a>
        = std::sync::MutexGuard<'a, Option<ScriptExeState>>
    where
        Self: 'a;

    type ResetInput<'b> = (
        std::sync::MutexGuard<'b, Option<ScriptExeState>>,
        &'b mlua::Lua,
        ScriptExeStateResetMode,
    );

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.lock().unwrap()
    }

    fn exe_state_reset(&self, mut args: Self::ResetInput<'_>) {
        let (mut exe_state_guard, lua, ref mut reset_mode) = args;
        let script_compile = |exe_state: &mut ScriptExeState| {
            exe_state.compiled = lua
                .load(&self.script)
                .into_function()
                .inspect_err(|e| log::error!("Failed to compile lua script. Error was:\n {e}"))
                .unwrap_or(lua.load("").into_function().unwrap());
        };

        || -> mlua::Result<()> {
            #[cfg(feature = "gui")]
            if let ScriptExeStateResetMode::Recompile { new_epoch } = reset_mode
                && let Some(exe_state) = exe_state_guard.as_mut()
            {
                log::info!("Re-compiling lua script!");
                let env = exe_state
                    .compiled
                    .environment()
                    .ok_or_else(|| mlua::Error::RuntimeError("Can't get script environment".into()))?;
                script_compile(exe_state);
                exe_state
                    .compiled
                    .set_environment(env)
                    .context("Can't set script environment")?;
                #[cfg(feature = "gui")]
                {
                    exe_state.edit_epoch = *new_epoch
                }
            } else {
                *reset_mode = ScriptExeStateResetMode::Init;
            }

            if let ScriptExeStateResetMode::Init = reset_mode {
                log::info!("Initializing lua script exe state!");
                let mut exe_state = ScriptExeState::new(lua);
                script_compile(&mut exe_state);
                let env = lua.create_table().context("Can't create environment table.")?;
                let meta = lua.create_table().context("Can't create environment metatable.")?;
                meta.set("__index", lua.globals())
                    .context("Can't set environment metadata.")?;
                env.set_metatable(meta.into())
                    .context("Can't set environment metatable.")?;
                exe_state
                    .compiled
                    .set_environment(env.clone())
                    .context("Can't set environment.")?;
                *exe_state_guard = Some(exe_state);
            }

            Ok(())
        }()
        .with_context(|_| {
            #[cfg(feature = "gui")]
            if let ScriptExeStateResetMode::Recompile { .. } = reset_mode {
                "while recompiling lua script"
            } else {
                "while creating the new lua script exe state"
            }

            #[cfg(not(feature = "gui"))]
            {
                "while creating the new lua script exe state"
            }
        })
        .expect("Lua script exe state creation failed");
    }
}

impl WithTfmExec for ScriptCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !(*self.enabled) {
            return value;
        }

        let lua = ctx.get_lua();
        if lua.is_none() {
            return value;
        }

        let lua = lua.unwrap();

        let mut stats_post_closure_setup: f64 = 0.0;
        let mut stats_post_scope_setup: f64 = 0.0;
        let mut stats_post_env_setup: f64 = 0.0;
        let mut stats_post_exec: f64 = 0.0;

        const NAIVE_BENCH: bool = false;

        'BEGIN: {
            let now = Instant::now();
            match self.lang {
                crate::schemas_transform::ScriptLanguage::Luau => {
                    let mut exe_state_guard = self.exe_state_mut();

                    if branches::unlikely(exe_state_guard.is_none()) {
                        self.exe_state_reset((exe_state_guard, lua, ScriptExeStateResetMode::Init));
                        break 'BEGIN;
                    }

                    let exe_state = exe_state_guard.as_mut().unwrap();

                    #[cfg(feature = "gui")]
                    if branches::unlikely(self.edit_epoch != exe_state.edit_epoch) {
                        self.exe_state_reset((
                            exe_state_guard,
                            lua,
                            ScriptExeStateResetMode::Recompile {
                                new_epoch: self.edit_epoch,
                            },
                        ));
                        break 'BEGIN;
                    }

                    let env = exe_state.compiled.environment().expect(
                        // NB: we can do a recovery with supplied Lua instance here, but generally, it must not happen, hence leaving as hard error.
                        "Can't get lua script environment! Execution context references other Lua instance?",
                    );

                    #[derive(Clone)]
                    enum SrcOrDstKey {
                        Str(String),
                        Num(i64),
                    }

                    impl FromLua for SrcOrDstKey {
                        fn from_lua(value: mlua::prelude::LuaValue, _lua: &Lua) -> mlua::prelude::LuaResult<Self> {
                            match value {
                                mlua::Value::String(s) => Ok(SrcOrDstKey::Str(s.to_str()?.to_owned())),
                                mlua::Value::Integer(i) => Ok(SrcOrDstKey::Num(i)),
                                mlua::Value::Number(n) => Ok(SrcOrDstKey::Num(n as i64)),
                                _ => Err(mlua::Error::FromLuaConversionError {
                                    from: value.type_name(),
                                    to: "SrcOrDstKey".to_string(),
                                    message: Some("expected string or number".to_string()),
                                }),
                            }
                        }
                    }

                    let transform_closure = move |_lua: &mlua::Lua,
                                                  args: (String, BaseNumT)|
                          -> std::result::Result<BaseNumT, mlua::Error> {
                        if let Some(tfm) = self.aux_transformations.get(&args.0) {
                            let ret = tfm.exec(
                                TfmValue {
                                    value: args.1,
                                    interval: tfm.get_interval(),
                                    relativity: tfm.get_relativity(),
                                },
                                ctx,
                            );
                            Ok(ret.value)
                        } else {
                            Err(mlua::Error::RuntimeError(format!(
                                "Can't find transformation with key {}",
                                args.0
                            )))
                        }
                    };

                    let is_idle_closure = move |_lua: &mlua::Lua, _: ()| -> std::result::Result<bool, mlua::Error> {
                        Ok(ctx.is_idle_tick())
                    };

                    let base_tick_closure = move |_lua: &mlua::Lua, _: ()| -> std::result::Result<u32, mlua::Error> {
                        Ok(ctx.get_idle_tick_rate())
                    };

                    let read_src_closure = {
                        move |_lua: &mlua::Lua, key: SrcOrDstKey| -> std::result::Result<BaseNumT, mlua::Error> {
                            match key {
                                SrcOrDstKey::Num(0) => Ok(value),
                                SrcOrDstKey::Str(s) => {
                                    if let Some(src) = self.aux_srcs.get(&s) {
                                        Ok(src.port_get_numeric_value(Some(ctx)))
                                    } else {
                                        Err(mlua::Error::RuntimeError(format!("Can't find source with key {s}")))
                                    }
                                }
                                SrcOrDstKey::Num(n) => self
                                    .aux_srcs
                                    .iter()
                                    .nth(n as usize - 1)
                                    .map(|v| v.1.port_get_numeric_value(Some(ctx)))
                                    .ok_or_else(|| {
                                        mlua::Error::RuntimeError(format!("Can't find source with key {n}"))
                                    }),
                            }
                        }
                    };

                    let write_dst_closure = {
                        let input_ref = &mut value;
                        move |_lua: &mlua::Lua,
                              (key, value): (SrcOrDstKey, BaseNumT)|
                              -> std::result::Result<(), mlua::Error> {
                            match key {
                                SrcOrDstKey::Num(0) => {
                                    let _: () = *input_ref = value;
                                    Ok(())
                                }
                                SrcOrDstKey::Str(key) => {
                                    if let Some(dst) = self.aux_dsts.get(&key) {
                                        dst.port_set_numeric_value_and_flush_to_devices(value, ctx);
                                        Ok(())
                                    } else {
                                        Err(mlua::Error::RuntimeError(format!(
                                            "Can't find destination with key {key}",
                                        )))
                                    }
                                }
                                SrcOrDstKey::Num(n) => self
                                    .aux_dsts
                                    .iter()
                                    .nth(n as usize - 1)
                                    .map(|v| v.1.port_set_numeric_value_and_flush_to_devices(value, ctx))
                                    .ok_or_else(|| {
                                        mlua::Error::RuntimeError(format!("Can't find destination with key {n}"))
                                    }),
                            }
                        }
                    };

                    if NAIVE_BENCH {
                        stats_post_closure_setup = (Instant::now() - now).as_secs_f64();
                    }

                    let _ = lua.scope(|s| {
                        if NAIVE_BENCH {
                            stats_post_scope_setup = (Instant::now() - now).as_secs_f64();
                        }

                        // TODO: consider using Lua userdata ref and setup those routines only once on state reset.
                        let _ = env.set("transform", s.create_function(transform_closure).unwrap());
                        let _ = env.set("is_idle", s.create_function(is_idle_closure).unwrap());
                        let _ = env.set("base_rate", s.create_function(base_tick_closure).unwrap());
                        let _ = env.set("read", s.create_function(read_src_closure).unwrap());
                        let _ = env.set("write", s.create_function_mut(write_dst_closure).unwrap());

                        if NAIVE_BENCH {
                            stats_post_env_setup = (Instant::now() - now).as_secs_f64();
                        }

                        if let Err(e) = exe_state.compiled.call::<()>(()) {
                            log::error!("{e} ");
                        }

                        if NAIVE_BENCH {
                            stats_post_exec = (Instant::now() - now).as_secs_f64();
                        }

                        Ok(())
                    });

                    // -----------------------------------
                    if NAIVE_BENCH {
                        println!(
                            " Script execution naive perf stats -----
                  post closures    {stats_post_closure_setup}
                  post scope setup {stats_post_scope_setup}
                  post env setup   {stats_post_env_setup}
                  post script exe  {stats_post_exec}
                  post exec total  {}\n",
                            (Instant::now() - now).as_secs_f64()
                        );
                    }
                }
            }
        }

        value
    }
}

impl WithTfmExec for InvertCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, value: Self::InputT, _ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !*self.enabled {
            return value;
        }
        match self.common_state_ref().get_in_relativity() {
            Relativity::Rel => -value,
            Relativity::Abs => self.common_state_ref().get_in_interval().clamp_and_invert(value),
        }
    }
}

impl IntegrateCfg {
    pub(crate) fn get_delta_acc_norm(&self, value: BaseNumT) -> BaseNumT {
        value.signum()
            * self.accumulator.port_get_interval().map_from(
                self.common_state_ref()
                    .get_in_interval()
                    .map_to_symm_unit::<BaseNumT>(value, OutOfRangePolicy::Clamp)
                    .abs(),
                &UNIT_INTERVAL,
                OutOfRangePolicy::Clamp,
            )
    }
}

impl WithTfmExec for IntegrateCfg {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, mut value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !*self.enabled {
            return value;
        }

        // dbg!(acc_interval);
        let acc_value = self.accumulator.port_get_numeric_value(Some(ctx));
        let acc_interval = self.accumulator.port_get_interval();

        value *= self.in_gain;

        let acc_delta = self.get_delta_acc_norm(value);
        let acc_out = acc_interval.clamp(acc_value + acc_delta);

        self.accumulator
            .port_set_numeric_value_and_flush_to_devices(acc_out, ctx);

        self.common_state_ref()
            .get_out_interval()
            .map_from(acc_out, &acc_interval, OutOfRangePolicy::Clamp)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SteeringExeState {
    pub(crate) last_time: Instant,
    pub(crate) pre_filter: BaseNumT,
}

impl Default for SteeringExeState {
    fn default() -> Self {
        Self {
            last_time: Instant::now(),
            pre_filter: Default::default(),
        }
    }
}

impl WithTfmExec for Box<SteeringCfg> {
    type InputT = BaseNumT;
    type OutputT = Self::InputT;
    fn exec(&self, value: Self::InputT, ctx: &impl TfmExecCtx) -> Self::OutputT {
        if !*self.enabled {
            return value;
        }
        let now = Instant::now();
        let state = &mut self.exe_state_mut();

        let auto_center_along_force_feedback = self.auto_center_along_force_feedback.port_get_numeric_value(Some(ctx));

        let dt = clamp_dt_by_min_and_max_period((now - state.last_time).as_secs_f32() as BaseNumT);

        let delta: BaseNumT = self
            .common_state_ref()
            .get_in_interval()
            .map_to_symm_unit::<BaseNumT>(value, OutOfRangePolicy::Clamp)
            * self.in_gain.port_get_numeric_value(Some(ctx));

        let mut post_filter: BaseNumT;

        if let Some(acc) = &self.accumulator {
            state.pre_filter = acc.port_get_numeric_value(Some(ctx));
        }

        state.pre_filter = SYMM_UNIT_INTERVAL.clamp(state.pre_filter.add(delta));

        #[cfg(feature = "gui")]
        if delta != 0.0 {
            use crate::schemas_transform::WithCommonState;

            self.common_state_ref().gui_trace(
                TfmStepTraceStage::Custom(
                    GraphDisplayStyle::as_filled()
                        .with_color(Color32::BROWN.gamma_multiply(0.7))
                        .with_width(1.2),
                ),
                delta,
                SYMM_UNIT_INTERVAL,
                now,
            );
        }

        #[cfg(feature = "gui")]
        self.common_state_ref().gui_trace(
            TfmStepTraceStage::Custom(GraphDisplayStyle::as_filled().with_color(Color32::BLUE).with_width(1.5)),
            state.pre_filter,
            SYMM_UNIT_INTERVAL,
            now,
        );

        '_User_input_filtering_and_curving_pre_FFB_and_autocentering: {
            if !self.integrated_user_input_transform.steps.is_empty() {
                post_filter = self
                    .integrated_user_input_transform
                    .exec(
                        TfmValue {
                            value: state.pre_filter,
                            interval: SYMM_UNIT_INTERVAL,
                            relativity: Relativity::Abs,
                        },
                        ctx,
                    )
                    .value;
            } else {
                post_filter = state.pre_filter;
            }
        }

        #[cfg(feature = "gui")]
        self.common_state_ref().gui_trace(
            TfmStepTraceStage::Custom(
                GraphDisplayStyle::default()
                    .with_color(Color32::MAGENTA)
                    .with_width(1.2),
            ),
            post_filter,
            SYMM_UNIT_INTERVAL,
            now,
        );

        let hold_factor_unit = self.hold_factor.port_get_numeric_value(Some(ctx));

        '_FFB_and_autocentering: {
            let ff_force_symm_norm = if let Some(ff_config) = &self.force_feedback {
                if *ff_config.enabled {
                    let raw_force = if let Some(custom_src) = &ff_config.custom_source {
                        SYMM_UNIT_INTERVAL.map_from(
                            custom_src.port_get_numeric_value(None::<&()>),
                            &custom_src.port_get_interval(),
                            OutOfRangePolicy::WarnIfDebugAndClamp,
                        )
                    } else {
                        ctx.get_main_dst()
                            .and_then(|dst| {
                                if let ValueDsts::Dynamic(DynValueRefs::DeviceControlMatcher(d)) = dst {
                                    match ff_config.component {
                                        ForceFeedbackComponent::X => {
                                            ctx.set_ff_x_axis_pos(
                                                &d.device_matcher_key,
                                                &d.control_matcher_key,
                                                dst.get_interval(),
                                            );
                                            ctx.get_ff_x(&d.device_matcher_key).into()
                                        }
                                        ForceFeedbackComponent::Y => {
                                            ctx.set_ff_y_axis_pos(
                                                &d.device_matcher_key,
                                                &d.control_matcher_key,
                                                dst.get_interval(),
                                            );
                                            ctx.get_ff_y(&d.device_matcher_key).into()
                                        }
                                    }
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_default()
                    };

                    let filtered_force = if !ff_config.transformation.steps.is_empty() {
                        let ret = ff_config.transformation.exec(
                            TfmValue {
                                value: raw_force,
                                interval: SYMM_UNIT_INTERVAL,
                                relativity: Relativity::Rel,
                            },
                            ctx,
                        );
                        // debug_assert!(ret.relativity == Relativity::Rel); TODO: check once on metadata recalculation
                        ret.value
                    } else {
                        raw_force
                    };

                    let filtered_and_scaled_force =
                        SYMM_UNIT_INTERVAL.clamp(filtered_force * ff_config.gain.port_get_numeric_value(Some(ctx)));

                    if ff_config.invert {
                        -filtered_and_scaled_force
                    } else {
                        filtered_and_scaled_force
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            if ff_force_symm_norm.abs() > 1e-4 {
                let ff_position_offset = ff_force_symm_norm * (1.0 - hold_factor_unit) * dt;

                state.pre_filter += ff_position_offset;
                post_filter += ff_position_offset;

                if get_debug_level().is_hi() && ff_force_symm_norm.abs() > 0.1 {
                    debug!(
                        "FF active: force={:.3} offset={:.3}",
                        ff_force_symm_norm, ff_position_offset
                    );
                }

                #[cfg(feature = "gui")]
                self.common_state_ref().gui_trace(
                    TfmStepTraceStage::Custom(
                        #[allow(clippy::unnecessary_cast)]
                        GraphDisplayStyle::default()
                            .with_color(Color32::GREEN.gamma_multiply((1.0 - hold_factor_unit as f32).max(0.4)))
                            .with_width(1.7),
                    ),
                    ff_force_symm_norm,
                    SYMM_UNIT_INTERVAL,
                    now,
                );
            }

            let autocentering_halflife = self.auto_center_halflife.port_get_numeric_value(Some(ctx)).abs();

            let ffb_is_small = ff_force_symm_norm.abs() < 1e-4;

            if autocentering_halflife > 0.0
                && (auto_center_along_force_feedback > 0.0 || ffb_is_small)
                && delta.abs() <= BaseNumT::EPSILON
            {
                let mut centerwize_decay_factor =
                    (1.0 - (-dt / autocentering_halflife).exp2()) * (1.0 - hold_factor_unit);

                if !ffb_is_small {
                    centerwize_decay_factor *= auto_center_along_force_feedback;
                }

                state.pre_filter -= state.pre_filter * centerwize_decay_factor;
                post_filter -= post_filter * centerwize_decay_factor;
            }
        };

        state.pre_filter = SYMM_UNIT_INTERVAL.clamp(state.pre_filter);
        post_filter = SYMM_UNIT_INTERVAL.clamp(post_filter);

        state.last_time = now;

        if let Some(acc) = self.accumulator.as_ref() {
            acc.port_set_numeric_value_and_flush_to_devices(state.pre_filter, ctx);
        }

        post_filter
    }
}
