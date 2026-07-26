use crate::num_interval::SYMM_UNIT_INTERVAL;
use crate::num_interval::UNIT_INTERVAL;

use crate::base_num::BaseNumT;
use crate::curves::Curves;
use crate::debug::get_debug_level;
#[cfg(feature = "gui")]
use crate::gui_transform_step::TfmStepTraceStage;
use crate::num_interval::{NumInterval, OutOfRangePolicy};
use crate::relativity::Relativity;

use crate::schemas_common::WithRuntimeId;
use crate::schemas_transform::WithCommonState;
use crate::schemas_transform::{
    ClampCfg, EmaFilterCfg, ForceFeedbackComponent, IntegrateCfg, InvertCfg, LinearCfg, NormExpCfg, OneEuroFilterCfg,
    RaiseFallCfg, SCurveCfg, ScriptCfg, SignedPowerCfg, SmoothstepCfg, SteeringCfg, TfmSeqCfg, TfmStepCfg,
};
use crate::schemas_value::{DynValueRefs, ValueDsts, WithNumInterval, WithRelativity};
use crate::schemas_value::{MappedValue, WithNumericValue};

#[cfg(feature = "gui")]
use crate::tracing::GraphDisplayStyle;
#[cfg(feature = "gui")]
use eframe::egui::Color32;
use log::debug;
use mlua::{FromLua, Lua};
use std::ops::{Add, DerefMut};
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

use std::cell::UnsafeCell;

#[derive(Debug, Default)]
pub(crate) struct UncheckedIMStorage<T: Clone> {
    inner: UnsafeCell<T>,
}

impl<T: Clone> Clone for UncheckedIMStorage<T> {
    fn clone(&self) -> Self {
        Self {
            inner: UnsafeCell::new(self.get().clone()),
        }
    }
}

// SAFETY: We guarantee that only one thread accesses this at a time
unsafe impl<T: Clone> Send for UncheckedIMStorage<T> {}
unsafe impl<T: Clone> Sync for UncheckedIMStorage<T> {}

impl<T: Clone> UncheckedIMStorage<T> {
    #[allow(unused)]
    fn new(value: T) -> Self {
        Self {
            inner: UnsafeCell::new(value),
        }
    }

    #[allow(unused)]
    pub(crate) fn get(&self) -> &T {
        unsafe { &*self.inner.get() }
    }

    #[allow(clippy::mut_from_ref)]
    pub(crate) fn get_mut(&self) -> &mut T {
        unsafe { &mut *self.inner.get() }
    }
}

pub(crate) trait TfmExeState {
    type StateMutT<'a>: DerefMut
    where
        Self: 'a;
    type ResetInput;
    fn exe_state_mut(&self) -> Self::StateMutT<'_>;
    fn exe_state_reset(&self, reset_with: Self::ResetInput);
}

pub(crate) trait TfmExecCtx {
    fn is_idle_tick(&self) -> bool;
    #[allow(unused)]
    fn get_idle_tick_rate(&self) -> u32;

    fn get_main_dst(&self) -> &ValueDsts;
    fn set_dyn_value(&self, dst: &DynValueRefs, v: BaseNumT);

    fn get_lua(&self) -> &mlua::Lua;
    fn get_ff_x(&self, dk: &str) -> BaseNumT;
    fn get_ff_y(&self, dk: &str) -> BaseNumT;
    fn set_ff_x_axis_pos(&self, dk: &str, ck: &str, ivl: NumInterval<BaseNumT>);
    fn set_ff_y_axis_pos(&self, dk: &str, ck: &str, ivl: NumInterval<BaseNumT>);
}

pub(crate) trait WithTfmExec {
    fn exec(&self, input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT>;
}

impl WithTfmExec for TfmSeqCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        for step in &self.steps {
            input = step.exec(input, ctx);
            if !input.interval.contains_value_closed(input.value) {
                log::warn!(
                    "Value {} must fit in interval {} after transformation step ``{}'' (ID: {}). 
            Each step must ensure it, clamping!",
                    input.value,
                    input.interval,
                    step,
                    step.get_id()
                );
                input.value = input.interval.clamp(input.value);
            }
        }
        input
    }
}

impl WithTfmExec for ClampCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, _ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !self.enabled {
            return input;
        }
        input.value = self.get_clamping_interval().clamp(input.value);
        input.interval = self.get_out_interval();
        // NB: clamping interval is ensured to be contained wihin input interval,
        // NB: so no more need for input.value = input.interval.clamp(input.value);
        input
    }
}

impl TfmExeState for OneEuroFilterCfg {
    type StateMutT<'a>
        = parking_lot::ArcMutexGuard<parking_lot::RawMutex, crate::filters::OneEuroFilter>
    where
        Self: 'a;
    type ResetInput = BaseNumT;

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.lock_arc()
    }

    fn exe_state_reset(&self, reset_with: Self::ResetInput) {
        self.exe_state_mut().reset(reset_with);
    }
}

impl WithTfmExec for OneEuroFilterCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if self.enabled
            && (!ctx.is_idle_tick() || input.relativity == Relativity::Abs || self.on_relative_input_feed_on_idle)
        {
            input.value = self.exe_state_mut().filter(
                input.value,
                Instant::now(),
                self.min_cutoff_hz,
                self.beta,
                self.d_cutoff_hz,
            );
        } else if self.on_relative_input_reset_on_idle {
            self.exe_state_reset(input.value);
        }
        input
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct RaiseFallExeState {
    pub(crate) prev_out: BaseNumT,
    pub(crate) last_target: BaseNumT,
    pub(crate) prev_out_time: Option<Instant>,
    pub(crate) prev_user_input_time: Option<Instant>,
}

impl TfmExeState for RaiseFallCfg {
    type StateMutT<'a>
        = parking_lot::ArcMutexGuard<parking_lot::RawMutex, RaiseFallExeState>
    where
        Self: 'a;

    type ResetInput = Option<RaiseFallExeState>;

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.lock_arc()
    }

    fn exe_state_reset(&self, reset_with: Self::ResetInput) {
        *self.exe_state_mut() = reset_with.unwrap_or_default();
    }
}

impl WithTfmExec for RaiseFallCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !self.enabled {
            return input;
        }
        if input.relativity != Relativity::Abs {
            log::warn!("Raise-fall transform should only be applied to absolute inputs.");
        }
        let now = Instant::now();
        let mut filter_data = self.exe_state_mut();

        let dt = if let Some(prev) = filter_data.prev_out_time {
            (now - prev).as_secs_f32()
        } else {
            0.0
        } as BaseNumT;

        let dt_user_input = if let Some(prev) = filter_data.prev_user_input_time {
            (now - prev).as_secs_f32()
        } else {
            0.0
        } as BaseNumT;

        filter_data.prev_out_time = Some(now);

        let target = if !ctx.is_idle_tick() {
            filter_data.last_target = input.value;
            input.value
        } else {
            filter_data.last_target
        };

        let mut final_out = filter_data.prev_out;
        if ctx.is_idle_tick() {
            if dt > 0.0 {
                let delta_v = target - filter_data.prev_out;
                let rate_limit = if delta_v > 0.0 {
                    self.raise_rate
                } else {
                    let mut fall_hold_factor = UNIT_INTERVAL.map_from(
                        self.fall_hold_factor.get_numeric_value(),
                        &self.fall_hold_factor.get_interval(),
                        OutOfRangePolicy::WarnAndClamp,
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
            }

            let smoothing_alpha = self.smoothing_alpha;
            final_out = (smoothing_alpha) * final_out + (1.0 - smoothing_alpha) * filter_data.prev_out;

            final_out = input.interval.clamp(final_out);
            filter_data.prev_out = final_out;
        } else {
            filter_data.prev_user_input_time = Some(now);
        }

        input.value = final_out;
        input
    }
}

impl TfmExeState for EmaFilterCfg {
    type StateMutT<'a>
        = parking_lot::ArcMutexGuard<parking_lot::RawMutex, crate::filters::EmaFilter>
    where
        Self: 'a;

    type ResetInput = BaseNumT;

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.lock_arc()
    }

    fn exe_state_reset(&self, reset_with: Self::ResetInput) {
        self.exe_state_mut().reset(reset_with);
    }
}

impl WithTfmExec for EmaFilterCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if self.enabled
            && (!ctx.is_idle_tick() || input.relativity == Relativity::Abs || self.on_relative_input_feed_on_idle)
        {
            input.value = self.exe_state_mut().filter(input.value, Instant::now(), self.tau);
        } else if self.on_relative_input_reset_on_idle {
            self.exe_state_reset(input.value);
        }
        input
    }
}

impl WithTfmExec for TfmStepCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        self.common_state_ref().last_in.store(input.value, Relaxed);
        #[cfg(feature = "gui")]
        self.common_state_ref()
            .gui_trace(TfmStepTraceStage::In, &input, Instant::now());

        input = match self {
            TfmStepCfg::Nop(_) => input,
            TfmStepCfg::Invert(s) => s.exec(input, ctx),
            TfmStepCfg::Integrate(s) => s.exec(input, ctx),
            TfmStepCfg::Steering(s) => s.exec(input, ctx),
            TfmStepCfg::Clamp(s) => s.exec(input, ctx),
            TfmStepCfg::RaiseFall(s) => s.exec(input, ctx),
            TfmStepCfg::Ema(s) => s.exec(input, ctx),
            TfmStepCfg::Linear(s) => s.exec(input, ctx),
            TfmStepCfg::Smoothstep(s) => s.exec(input, ctx),
            TfmStepCfg::SCurve(s) => s.exec(input, ctx),
            TfmStepCfg::Exp(s) => s.exec(input, ctx),
            TfmStepCfg::SignedPower(s) => s.exec(input, ctx),
            TfmStepCfg::OneEuro(s) => s.exec(input, ctx),
            TfmStepCfg::Script(s) => s.exec(input, ctx),
            TfmStepCfg::_HighPass(_) => input,
            TfmStepCfg::_ForceFeedback(_) => input,
        };

        self.common_state_ref().last_out.store(input.value, Relaxed);
        #[cfg(feature = "gui")]
        self.common_state_ref()
            .gui_trace(TfmStepTraceStage::Out, &input, Instant::now());

        input
    }
}

impl WithTfmExec for SignedPowerCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !(self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return input;
        }
        input.value = if self.center_symmetric {
            Curves::apply_center_symmetric_with_abs_value(
                input.value,
                input.interval,
                |v_abs| Curves::signed_power(v_abs, self.power),
                OutOfRangePolicy::WarnAndClamp,
            )
        } else {
            input.interval.map_from_unit(
                Curves::signed_power(
                    input.interval.map_to_unit(input.value, OutOfRangePolicy::WarnAndClamp),
                    self.power,
                ),
                OutOfRangePolicy::WarnAndClamp,
            )
        };
        input
    }
}

impl WithTfmExec for NormExpCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !(self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return input;
        }
        input.value = if self.center_symmetric {
            Curves::apply_center_symmetric_with_abs_value(
                input.value,
                input.interval,
                |v_abs| Curves::exp_curve(v_abs, self.base),
                OutOfRangePolicy::WarnAndClamp,
            )
        } else {
            input.interval.map_from_unit(
                Curves::exp_curve(
                    input.interval.map_to_unit(input.value, OutOfRangePolicy::WarnAndClamp),
                    self.base,
                ),
                OutOfRangePolicy::WarnAndClamp,
            )
        };
        input
    }
}

impl WithTfmExec for SCurveCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !(self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return input;
        }
        input.value = input.interval.map_from_unit(
            Curves::s_curve(
                input.interval.map_to_unit(input.value, OutOfRangePolicy::WarnAndClamp),
                self.steepness,
            ),
            OutOfRangePolicy::WarnAndClamp,
        );
        input
    }
}

impl WithTfmExec for SmoothstepCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !(self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return input;
        }
        input.value = input.interval.map_from_unit(
            Curves::smoothstep(input.interval.map_to_unit(input.value, OutOfRangePolicy::WarnAndClamp)),
            OutOfRangePolicy::WarnAndClamp,
        );
        input
    }
}

impl WithTfmExec for LinearCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !(self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return input;
        }
        input.value = if self.center_symmetric {
            Curves::apply_center_symmetric_with_abs_value(
                input.value,
                input.interval,
                |abs_v| {
                    Curves::linear(
                        abs_v,
                        self.slope,
                        input.interval.map_to_symm_unit(self.shift_x, OutOfRangePolicy::Clamp),
                        input.interval.map_to_symm_unit(self.shift_y, OutOfRangePolicy::Clamp),
                    )
                },
                OutOfRangePolicy::Clamp,
            )
        } else {
            input
                .interval
                .clamp(Curves::linear(input.value, self.slope, self.shift_x, self.shift_y))
        };
        input
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ScriptExeState {
    // pub(crate) inputs: mlua::Table,
    // pub(crate) outputs: mlua::Table,
    pub(crate) compiled: mlua::Function,
}

impl ScriptExeState {
    fn new(lua: &mlua::Lua) -> Self {
        Self {
            // inputs: lua.create_table().unwrap(),
            // outputs: lua.create_table().unwrap(),
            compiled: lua
                .load(" ")
                .into_function()
                .inspect_err(|e| log::error!("{e}"))
                .unwrap(),
        }
    }
}

impl TfmExeState for ScriptCfg {
    type StateMutT<'a>
        = &'a mut ScriptExeState
    where
        Self: 'a;

    type ResetInput = mlua::Lua;

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.get_mut().as_mut().unwrap()
    }

    fn exe_state_reset(&self, lua: Self::ResetInput) {
        let mut state = ScriptExeState::new(&lua);

        if get_debug_level().is_on() {
            log::debug!("Compiling Luau script!");
        }

        state.compiled = lua
            .load(&self.script)
            .into_function()
            .inspect_err(|e| log::error!("{e}"))
            .unwrap_or(lua.load(" ").into_function().unwrap());

        // COMPAT
        // let aux_tfm_idx = lua.create_table().unwrap();
        // for (name, _) in self.aux_transformations.iter() {
        //     let _ = aux_tfm_idx.set(name.as_str(), name.to_string());
        // }
        // let _ = lua
        //     .globals()
        //     .set("aux_tfm_idx", aux_tfm_idx)
        //     .inspect_err(|e| log::error!("{e}"));

        *self.exe_state.get_mut() = Some(state);
    }
}

impl WithTfmExec for ScriptCfg {
    #[inline]
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !self.enabled {
            return input;
        }

        let mut stats_post_closure: f64 = 0.0;
        let mut stats_pre_scope_setup: f64 = 0.0;
        let mut stats_post_scope_setup: f64 = 0.0;

        const NAIVE_BENCH: bool = false;

        if NAIVE_BENCH {
            println!("Script execution naive perf stats -----");
        }

        let now = Instant::now();
        match self.lang {
            crate::schemas_transform::ScriptLanguage::Luau => {
                let transform_closure =
                    move |_lua: &mlua::Lua, args: (String, BaseNumT)| -> std::result::Result<BaseNumT, mlua::Error> {
                        if let Some(tfm) = self.aux_transformations.get(&args.0) {
                            let ret = tfm.exec(
                                MappedValue {
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

                let is_idle_closure =
                    move |_lua: &mlua::Lua, _: ()| -> std::result::Result<bool, mlua::Error> { Ok(ctx.is_idle_tick()) };

                let base_tick_closure = move |_lua: &mlua::Lua, _: ()| -> std::result::Result<u32, mlua::Error> {
                    Ok(ctx.get_idle_tick_rate())
                };

                if self.exe_state.get().is_none() {
                    self.exe_state_reset(ctx.get_lua().clone());
                }

                let exe_state = self.exe_state_mut();

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

                let read_src_closure = {
                    move |_lua: &mlua::Lua, key: SrcOrDstKey| -> std::result::Result<BaseNumT, mlua::Error> {
                        match key {
                            SrcOrDstKey::Num(0) => Ok(input.value),
                            SrcOrDstKey::Str(s) => {
                                if let Some(src) = self.aux_srcs.get(&s) {
                                    let raw = src.source.get_numeric_value();
                                    Ok(if let Some(remap_interval) = src.remap_to_interval {
                                        remap_interval.map_from(
                                            raw,
                                            &src.source.get_interval(),
                                            OutOfRangePolicy::Clamp,
                                        )
                                    } else {
                                        raw
                                    })
                                } else {
                                    Err(mlua::Error::RuntimeError(format!("Can't find source with key {s}")))
                                }
                            }
                            SrcOrDstKey::Num(n) => self
                                .aux_srcs
                                .iter()
                                .nth(n as usize - 1)
                                .map(|v| {
                                    let raw = v.1.source.get_numeric_value();
                                    if let Some(remap_interval) = v.1.remap_to_interval {
                                        remap_interval.map_from(
                                            raw,
                                            &v.1.source.get_interval(),
                                            OutOfRangePolicy::Clamp,
                                        )
                                    } else {
                                        raw
                                    }
                                })
                                .ok_or_else(|| mlua::Error::RuntimeError(format!("Can't find source with key {n}"))),
                        }
                    }
                };

                let write_dst_closure = {
                    let input_ref = &mut input.value;
                    move |_lua: &mlua::Lua,
                          (key, mut value): (SrcOrDstKey, BaseNumT)|
                          -> std::result::Result<(), mlua::Error> {
                        match key {
                            SrcOrDstKey::Num(0) => {
                                let _: () = *input_ref = value;
                                Ok(())
                            }
                            SrcOrDstKey::Str(key) => {
                                if let Some(dst) = self.aux_dsts.get(&key) {
                                    match dst.destination {
                                        ValueDsts::Dynamic(ref d) => {
                                            if let Some(remap_interval) = dst.remap_from_interval {
                                                value = dst.destination.get_interval().map_from(
                                                    value,
                                                    &remap_interval,
                                                    OutOfRangePolicy::Clamp,
                                                )
                                            }
                                            ctx.set_dyn_value(d, value)
                                        }
                                        ValueDsts::Void => {}
                                    };
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
                                .map(|v| match v.1.destination {
                                    ValueDsts::Dynamic(ref d) => {
                                        if let Some(remap_interval) = v.1.remap_from_interval {
                                            value = v.1.destination.get_interval().map_from(
                                                value,
                                                &remap_interval,
                                                OutOfRangePolicy::Clamp,
                                            )
                                        }
                                        ctx.set_dyn_value(d, value);
                                    }
                                    ValueDsts::Void => {}
                                })
                                .ok_or_else(|| {
                                    mlua::Error::RuntimeError(format!("Can't find destination with key {n}"))
                                }),
                        }
                    }
                };

                if NAIVE_BENCH {
                    stats_post_closure = (Instant::now() - now).as_secs_f64();
                }

                if NAIVE_BENCH {
                    stats_pre_scope_setup = (Instant::now() - now).as_secs_f64();
                }

                let _ = ctx.get_lua().scope(|s| {
                    let globals = ctx.get_lua().globals();
                    let _ = globals.set("transform", s.create_function(transform_closure).unwrap());
                    let _ = globals.set("is_idle", s.create_function(is_idle_closure).unwrap());
                    let _ = globals.set("base_rate", s.create_function(base_tick_closure).unwrap());
                    let _ = globals.set("read", s.create_function(read_src_closure).unwrap());
                    let _ = globals.set("write", s.create_function_mut(write_dst_closure).unwrap());
                    if let Err(e) = exe_state.compiled.call::<()>(()) {
                        if NAIVE_BENCH {
                            stats_post_scope_setup = (Instant::now() - now).as_secs_f64();
                        }
                        log::error!("{e} ");
                    }
                    Ok(())
                });

                // -----------------------------------
                input.relativity = self.output_relativity.unwrap_or(input.relativity);
            }
        }

        if let Some(intvl) = self.output_interval {
            input.interval = intvl;
        }

        if NAIVE_BENCH {
            println!(
                "post closure {},\n pre scope setup {},\n post scope setup {},\n post exec {}",
                stats_post_closure,
                stats_pre_scope_setup,
                stats_post_scope_setup,
                (Instant::now() - now).as_secs_f64()
            );
        }
        input
    }
}

impl WithTfmExec for InvertCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, _ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !self.enabled {
            return input;
        }
        input.value = match input.relativity {
            Relativity::Rel => -input.value,
            Relativity::Abs => input.interval.clamp_and_invert(input.value),
        };
        input
    }
}

#[derive(Default, Debug, Clone, Copy, PartialOrd, PartialEq)]
pub(crate) struct IntegrateExeState {
    pub(crate) prev_val: BaseNumT, //  prev_val: (self.range.from() + self.range.to()) * 0.5,
}

impl TfmExeState for IntegrateCfg {
    type StateMutT<'a>
        = parking_lot::ArcMutexGuard<parking_lot::RawMutex, IntegrateExeState>
    where
        Self: 'a;

    type ResetInput = ();

    fn exe_state_mut(&self) -> Self::StateMutT<'_> {
        self.exe_state.lock_arc()
    }

    fn exe_state_reset(&self, _: Self::ResetInput) {
        *self.exe_state_mut() = Default::default()
    }
}

impl WithTfmExec for IntegrateCfg {
    fn exec(&self, mut input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !(self.enabled && (!ctx.is_idle_tick() || self.on_idle)) {
            return input;
        }
        let mut state = self.exe_state_mut();
        if input.value.abs() < self.deadzone_norm * self.range.span() {
            input.value = 0.0;
        }
        state.prev_val = self.range.clamp(state.prev_val + input.value * self.smoothing_alpha);
        MappedValue::<BaseNumT> {
            value: state.prev_val,
            interval: self.range,
            relativity: Relativity::Abs,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SteeringExeState {
    pub(crate) last_time: Instant,
    pub(crate) pre_filter: BaseNumT,
    pub(crate) post_filter: BaseNumT,
}

impl Default for SteeringExeState {
    fn default() -> Self {
        Self {
            last_time: Instant::now(),
            pre_filter: Default::default(),
            post_filter: Default::default(),
        }
    }
}

impl WithTfmExec for SteeringCfg {
    fn exec(&self, input: MappedValue<BaseNumT>, ctx: &impl TfmExecCtx) -> MappedValue<BaseNumT> {
        if !self.enabled {
            return input;
        }
        let now = Instant::now();
        let state = &mut self.exe_state_mut();
        let value = input.value
            * UNIT_INTERVAL.map_from(
                self.input_gain.get_numeric_value(),
                &self.input_gain.get_interval(),
                OutOfRangePolicy::WarnAndClamp,
            );

        let auto_center_along_force_feedback = UNIT_INTERVAL.map_from(
            self.auto_center_along_force_feedback.get_numeric_value(),
            &self.auto_center_along_force_feedback.get_interval(),
            OutOfRangePolicy::WarnAndClamp,
        );

        let dt = (now - state.last_time).as_secs_f32() as BaseNumT;
        let delta: BaseNumT = input.interval.map_to_symm_unit(value, OutOfRangePolicy::Clamp);

        if let Some(acc) = &self.accumulator {
            state.pre_filter = SYMM_UNIT_INTERVAL.map_from(
                acc.get_numeric_value(),
                &acc.get_interval(),
                OutOfRangePolicy::WarnAndClamp,
            );
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
                &MappedValue::<BaseNumT> {
                    value: delta,
                    interval: SYMM_UNIT_INTERVAL,
                    relativity: Relativity::Rel,
                },
                now,
            );
        }

        #[cfg(feature = "gui")]
        self.common_state_ref().gui_trace(
            TfmStepTraceStage::Custom(GraphDisplayStyle::as_filled().with_color(Color32::BLUE).with_width(1.5)),
            &MappedValue::<BaseNumT> {
                value: state.pre_filter,
                interval: SYMM_UNIT_INTERVAL,
                relativity: Relativity::Abs,
            },
            now,
        );

        '_User_input_filtering_and_curving_pre_FFB_and_autocentering: {
            if !self.integrated_user_input_transform.steps.is_empty() {
                state.post_filter = self
                    .integrated_user_input_transform
                    .exec(
                        MappedValue {
                            value: state.pre_filter,
                            interval: SYMM_UNIT_INTERVAL,
                            relativity: Relativity::Abs,
                        },
                        ctx,
                    )
                    .value;
            } else {
                state.post_filter = state.pre_filter;
            }
        }

        #[cfg(feature = "gui")]
        self.common_state_ref().gui_trace(
            TfmStepTraceStage::Custom(
                GraphDisplayStyle::default()
                    .with_color(Color32::MAGENTA)
                    .with_width(1.2),
            ),
            &MappedValue::<BaseNumT> {
                value: state.post_filter,
                interval: SYMM_UNIT_INTERVAL,
                relativity: Relativity::Abs,
            },
            now,
        );

        let hold_factor_unit = UNIT_INTERVAL.map_from(
            self.hold_factor.get_numeric_value(),
            &self.hold_factor.get_interval(),
            OutOfRangePolicy::WarnAndClamp,
        );

        '_FFB_and_autocentering: {
            let ff_force_symm_norm = if let Some(ff_config) = &self.force_feedback {
                if ff_config.enabled {
                    let raw_force = if let Some(custom_src) = &ff_config.custom_source {
                        SYMM_UNIT_INTERVAL.map_from(
                            custom_src.get_numeric_value(),
                            &custom_src.get_interval(),
                            OutOfRangePolicy::WarnAndClamp,
                        )
                    } else {
                        match &ctx.get_main_dst() {
                            ValueDsts::Void => 0.0,
                            ValueDsts::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                                DynValueRefs::DeviceControlMatcher(d) => match ff_config.component {
                                    ForceFeedbackComponent::X => {
                                        ctx.set_ff_x_axis_pos(
                                            &d.device_matcher_key,
                                            &d.control_key,
                                            ctx.get_main_dst().get_interval(),
                                        );
                                        ctx.get_ff_x(&d.device_matcher_key)
                                    }
                                    ForceFeedbackComponent::Y => {
                                        ctx.set_ff_y_axis_pos(
                                            &d.device_matcher_key,
                                            &d.control_key,
                                            ctx.get_main_dst().get_interval(),
                                        );
                                        ctx.get_ff_y(&d.device_matcher_key)
                                    }
                                },
                                DynValueRefs::Variable(_) => 0.0,
                            },
                        }
                    };

                    let filtered_force = if !ff_config.transformation.steps.is_empty() {
                        let ret = ff_config.transformation.exec(
                            MappedValue {
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

                    let filtered_and_scaled_force = SYMM_UNIT_INTERVAL.clamp(filtered_force * ff_config.gain);

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
                state.post_filter += ff_position_offset;
                state.pre_filter += ff_position_offset;

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
                    &MappedValue::<BaseNumT> {
                        value: ff_force_symm_norm,
                        interval: SYMM_UNIT_INTERVAL,
                        relativity: Relativity::Abs,
                    },
                    now,
                );
            }

            let autocentering_halflife = self.auto_center_halflife.get_numeric_value().abs();

            let ffb_is_small = ff_force_symm_norm.abs() < 1e-4;

            if autocentering_halflife > 0.0
                && (auto_center_along_force_feedback > 0.0 || ffb_is_small)
                && delta.abs() < 1e-4
            {
                let mut centerwize_decay_factor =
                    (1.0 - (-dt / autocentering_halflife).exp2()) * (1.0 - hold_factor_unit);

                if !ffb_is_small {
                    centerwize_decay_factor *= auto_center_along_force_feedback;
                }

                state.post_filter -= state.post_filter * centerwize_decay_factor;
                state.pre_filter -= state.pre_filter * centerwize_decay_factor;
            }
        };

        state.pre_filter = SYMM_UNIT_INTERVAL.clamp(state.pre_filter);
        state.post_filter = SYMM_UNIT_INTERVAL.clamp(state.post_filter);

        let out = MappedValue::<BaseNumT> {
            value: state.post_filter,
            interval: SYMM_UNIT_INTERVAL,
            relativity: Relativity::Abs,
        };

        state.last_time = now;

        if let Some(acc) = &self.accumulator {
            ctx.set_dyn_value(
                acc,
                acc.get_interval()
                    .map_from_symm_unit(state.pre_filter, OutOfRangePolicy::Clamp),
            );
        }

        out
    }
}
