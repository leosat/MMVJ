use anyhow::{Result, bail};
use evdev::EvdevEnum;
use num_traits::Zero;
use tokio::time::MissedTickBehavior;
use tokio_util::future::FutureExt;

use crate::num_interval::SYMM_UNIT_INTERVAL;

use crate::base_num::BaseNumT;
use crate::device_and_device_manager::DeviceEvent;
use crate::filters::OneEuroFilter;
use crate::hid_device::{
    DeviceControlStates, DeviceThreadCmd, HidDeviceEvent, OwnedVirtualHIDDeviceThreadIO,
    set_hid_control_virtual_owned_device,
};
use crate::hid_device::{HID_AXIS_MAX_INTERVAL, HidEvent};
use crate::num_interval::{NumInterval, from_type_interval_to_symm_unit_clamping, from_type_interval_to_unit_clamping};
use crate::schemas_common::ObjId;
use std::collections::BTreeMap;
use std::ops::Neg;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[allow(dead_code)]
type FfGainT = i16;
type FfLevelT = i16;
type FfDirectionT = u16;
type FfIndexT = usize;
type PlayedEffects = BTreeMap<FfIndexT, FfEffectPlaybackInfo>;

const FF_LEVEL_HALFSPAN: BaseNumT = FfLevelT::MAX as BaseNumT;
// const MIN_SLEEP_ON_IDLE_MILLIS: u64 = 0;
// const MAX_SLEEP_ON_IDLE_MILLIS: u64 = 3;
pub(crate) const X_AXIS_IDX: usize = 0;
pub(crate) const Y_AXIS_IDX: usize = 1;
pub(crate) const AXIS_IDX_LIST: [usize; 2] = [X_AXIS_IDX, Y_AXIS_IDX];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FfEffectEnvelope {
    attack_length: std::time::Duration,
    attack_level: u16,
    fade_length: std::time::Duration,
    fade_level: u16,
}

struct FfEffectPlaybackInfo {
    play_counter: i32,
    played_since: Instant,
    #[allow(dead_code)]
    prev_out: BaseNumT,
    after_delay_start_time: Instant,
    end_time: Instant,
    after_delay_length: std::time::Duration,
}

impl FfEffectPlaybackInfo {
    fn new(play_count: i32, now: Instant, replay: &FfEffectReplayInfo) -> Self {
        Self {
            play_counter: play_count,
            played_since: now,
            prev_out: 0.0,
            after_delay_start_time: now + replay.delay,
            end_time: now + replay.delay + replay.length,
            after_delay_length: replay.length,
        }
    }

    fn set_new_play_iteration(&mut self, now: Instant, delay: std::time::Duration) {
        self.played_since = now;
        self.after_delay_start_time = now + delay;
        self.end_time = self.after_delay_start_time + self.after_delay_length;
    }
}

impl From<evdev::FFEnvelope> for FfEffectEnvelope {
    fn from(src: evdev::FFEnvelope) -> Self {
        Self {
            attack_length: std::time::Duration::from_millis(src.attack_length as u64),
            attack_level: src.attack_level,
            fade_length: std::time::Duration::from_millis(src.fade_length as u64),
            fade_level: src.fade_level,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FfEffectReplayInfo {
    pub length: std::time::Duration,
    pub delay: std::time::Duration,
}

impl From<evdev::FFReplay> for FfEffectReplayInfo {
    fn from(src: evdev::FFReplay) -> Self {
        Self {
            // USB PID std: "To sustain an effect until explicitly stopped with the Stop method, set Duration to INFINITE (Null)".
            // NB: We also consider u16::MAX as infinity, based on observations of its usage in ffcfstress and ffmvforce.
            length: std::time::Duration::from_millis(if src.length.is_zero() || src.length == u16::MAX {
                u64::MAX
            } else {
                src.length as u64
            }),
            delay: std::time::Duration::from_millis(src.delay as u64),
        }
    }
}

#[derive(Debug)]
pub(crate) struct FfEffectCondition {
    saturation_interval_symm_norm: NumInterval<BaseNumT>,
    right_coeff_symm_norm: BaseNumT,
    left_coeff_symm_norm: BaseNumT,
    half_deadband_norm: BaseNumT,
    center_symm_norm: BaseNumT,
}

impl From<evdev::FFCondition> for FfEffectCondition {
    fn from(cond: evdev::FFCondition) -> Self {
        Self {
            saturation_interval_symm_norm: NumInterval::new(
                from_type_interval_to_unit_clamping(cond.left_saturation).neg(),
                from_type_interval_to_unit_clamping(cond.right_saturation),
            ),
            left_coeff_symm_norm: from_type_interval_to_symm_unit_clamping(cond.left_coefficient),
            right_coeff_symm_norm: from_type_interval_to_symm_unit_clamping(cond.right_coefficient),
            // considering deadband to be "half" here as source is of unsigned type...
            // there might be interpretetions though.
            half_deadband_norm: from_type_interval_to_unit_clamping(cond.deadband),
            center_symm_norm: from_type_interval_to_symm_unit_clamping(cond.center),
        }
    }
}

enum FfEffect {
    ConstantForce {
        replay: FfEffectReplayInfo,
        envelope: FfEffectEnvelope,
        level: FfLevelT,
        direction: FfDirectionT,
    },
    RampForce {
        replay: FfEffectReplayInfo,
        envelope: FfEffectEnvelope,
        start_level: FfLevelT,
        end_level: FfLevelT,
        direction: FfDirectionT,
    },
    Spring {
        replay: FfEffectReplayInfo,
        condition_norm: [FfEffectCondition; 2],
        _direction: FfDirectionT,
    },
    Friction {
        replay: FfEffectReplayInfo,
        condition_norm: [FfEffectCondition; 2],
        _direction: FfDirectionT,
    },
    _Damper {
        replay: FfEffectReplayInfo,
        condition_norm: [FfEffectCondition; 2],
        _direction: FfDirectionT,
    },
    _Inertia {
        replay: FfEffectReplayInfo,
        condition_norm: [FfEffectCondition; 2],
        _direction: FfDirectionT,
    },
    _Periodic,
    NopEffect,
}

const NO_REPLAY: FfEffectReplayInfo = FfEffectReplayInfo {
    length: std::time::Duration::MAX, // Notice that 0 duration is infinity in accordance to USB PID std.
    delay: std::time::Duration::ZERO,
};

impl FfEffect {
    fn get_replay_info(&self) -> &FfEffectReplayInfo {
        match &self {
            FfEffect::ConstantForce { replay, .. } => replay,
            FfEffect::RampForce { replay, .. } => replay,
            FfEffect::Spring { replay, .. } => replay,
            FfEffect::Friction { replay, .. } => replay,
            FfEffect::_Damper { replay, .. } => replay,
            FfEffect::_Inertia { replay, .. } => replay,
            FfEffect::_Periodic => &NO_REPLAY,
            FfEffect::NopEffect => &NO_REPLAY,
        }
    }
}

fn convert_level_to_xy_components_symm_norm(level_symm_denorm: BaseNumT, direction: FfDirectionT) -> [BaseNumT; 2] {
    let angle_degrees = (direction as BaseNumT) * 360.0 / FfDirectionT::MAX as BaseNumT;
    let angle_radians = angle_degrees.to_radians();
    let x_component = -angle_radians.sin();
    let y_component = -angle_radians.cos(); // TODO: this is negated after ffmvtest test, recheck with other sources.
    // NB: currently we provide separate inversion knowbs for X and Y to satisfy any user needs.
    let level_symm_norm = level_symm_denorm / FF_LEVEL_HALFSPAN;
    [level_symm_norm * x_component, level_symm_norm * y_component]
}

fn play(
    played_effects: &mut PlayedEffects,
    uploaded_effects: &[Option<FfEffect>],
    jk_axis_pos_symm_norm: [BaseNumT; 2],
    jk_axis_vel_symm_norm: [BaseNumT; 2],
    debug_ff: bool,
) -> [BaseNumT; 2] {
    let now = Instant::now();
    let mut play_sum_symm_norm = [0.0 as BaseNumT, 0.0];

    for (effect_index, &mut ref mut played_effect_data) in played_effects.iter_mut() {
        if played_effect_data.play_counter == 0 || now < played_effect_data.after_delay_start_time {
            continue;
        }

        let elapsed_playing_duration = now.duration_since(played_effect_data.after_delay_start_time);

        if let Some(effect) = &uploaded_effects[*effect_index] {
            match effect {
                FfEffect::ConstantForce {
                    level,
                    direction,
                    replay: _,
                    envelope,
                } => {
                    let level_out = (*level as BaseNumT)
                        * envelope_gain(
                            *level as BaseNumT,
                            envelope,
                            elapsed_playing_duration,
                            played_effect_data.after_delay_length,
                        );
                    let c = convert_level_to_xy_components_symm_norm(level_out, *direction);
                    for axis_idx in AXIS_IDX_LIST {
                        play_sum_symm_norm[axis_idx] += c[axis_idx];
                    }
                }
                FfEffect::RampForce {
                    start_level,
                    end_level,
                    direction,
                    replay: _,
                    envelope,
                } => {
                    let fraction = (elapsed_playing_duration.as_secs_f32()
                        / played_effect_data.after_delay_length.as_secs_f32())
                    .clamp(0.0, 1.0) as BaseNumT;
                    let level_pre_envelope =
                        (*start_level as BaseNumT) + ((*end_level as BaseNumT - *start_level as BaseNumT) * fraction);
                    let level_out = level_pre_envelope
                        * envelope_gain(
                            level_pre_envelope,
                            envelope,
                            elapsed_playing_duration,
                            played_effect_data.after_delay_length,
                        );
                    let c = convert_level_to_xy_components_symm_norm(level_out, *direction);
                    for axis_idx in AXIS_IDX_LIST {
                        play_sum_symm_norm[axis_idx] += c[axis_idx];
                    }
                }
                FfEffect::Spring {
                    condition_norm,
                    _direction: _,
                    replay: _,
                } => {
                    //dbg!(condition_norm);
                    for axis_idx in AXIS_IDX_LIST {
                        play_sum_symm_norm[axis_idx] +=
                            condition_force_symm_norm(jk_axis_pos_symm_norm[axis_idx], condition_norm, axis_idx);
                    }
                }
                FfEffect::Friction {
                    condition_norm,
                    _direction: _,
                    replay: _,
                } => {
                    for axis_idx in AXIS_IDX_LIST {
                        play_sum_symm_norm[axis_idx] +=
                            condition_force_symm_norm(jk_axis_vel_symm_norm[axis_idx], condition_norm, axis_idx);
                    }
                }
                FfEffect::_Damper { .. } => {}
                FfEffect::_Inertia { .. } => {}
                FfEffect::_Periodic => {}
                FfEffect::NopEffect => {}
            };

            if now >= played_effect_data.end_time && played_effect_data.play_counter > 0 {
                played_effect_data.play_counter -= 1;
                if played_effect_data.play_counter != 0 {
                    played_effect_data.set_new_play_iteration(now, effect.get_replay_info().delay);
                } else {
                    // In play cycle we just ignore such effects. We could remove them from hash after this loop, but...
                    if debug_ff {
                        log::debug!("[(o)] STOP effect index {effect_index} after play counter went to 0.",);
                    }
                }
            }
        } else {
            log::error!(
                "Trying to play effect that's not found in uploaded effects set. Effect index: {}",
                effect_index
            );
        }
    }
    for axis_idx in AXIS_IDX_LIST {
        play_sum_symm_norm[axis_idx] = SYMM_UNIT_INTERVAL.clamp(play_sum_symm_norm[axis_idx]);
    }
    // dbg!(&play_sum_symm_norm);
    play_sum_symm_norm
}

fn upload_effect(
    maybe_effect_index_to_update: Option<usize>,
    eff: FfEffect,
    uploaded_effects: &mut [Option<FfEffect>],
    debug_ff: bool,
) -> Result<usize> {
    if let Some(effect_index_to_update) = maybe_effect_index_to_update {
        let max_effects = uploaded_effects.len();
        if effect_index_to_update >= max_effects {
            bail!("[e] Requested to upload effect with index out of bounds (max effects is {max_effects})");
        }
        if debug_ff && uploaded_effects[effect_index_to_update].is_none() {
            log::warn!("[iw] Updating yet not uploaded effect index {effect_index_to_update}.");
        }
        uploaded_effects[effect_index_to_update] = Some(eff);
        return Ok(effect_index_to_update);
    } else {
        for (index, option_item) in uploaded_effects.iter_mut().enumerate() {
            if option_item.is_none() {
                *option_item = eff.into();
                return Ok(index);
            }
        }
    }
    bail!("[e] Effect upload failed, no more free slots!")
}

fn set_played(
    effect_index: FfIndexT,
    play_count: i32,
    uploaded_effects_buffer: &[Option<FfEffect>],
    played_effects: &mut PlayedEffects,
    debug_ff: bool,
) {
    if uploaded_effects_buffer.len() > effect_index {
        let now = Instant::now();
        if let Some(effect) = &uploaded_effects_buffer[effect_index] {
            played_effects.insert(
                effect_index,
                FfEffectPlaybackInfo::new(play_count, now, effect.get_replay_info()),
            );
            if debug_ff {
                log::debug!("[|>] PLAY effect index {effect_index}.",);
            }
        }
    } else if debug_ff {
        log::error!(
            "Requested to play effect index {effect_index}, but max effects is {}",
            uploaded_effects_buffer.len()
        )
    }
}

fn set_stopped(
    effect_index: FfIndexT,
    uploaded_effects_buffer: &[Option<FfEffect>],
    played_effects: &mut PlayedEffects,
    debug_ff: bool,
) {
    if uploaded_effects_buffer.len() > effect_index {
        played_effects.retain(|i, _| *i != effect_index);
        if debug_ff {
            log::debug!("[(o)] STOP effect index {effect_index}. (by a user request)",);
        }
    } else {
        log::error!(
            "Requested to stop effect index {effect_index}, but max effects is {}",
            uploaded_effects_buffer.len()
        )
    }
}

fn envelope_gain(
    base_level: BaseNumT,
    envelope: &FfEffectEnvelope,
    elapsed_playing_duration: std::time::Duration,
    effect_duration: std::time::Duration,
) -> BaseNumT {
    let mut gain = 1.0 as BaseNumT;
    if !envelope.attack_length.is_zero() && elapsed_playing_duration < envelope.attack_length {
        let attack_level_norm = (envelope.attack_level as BaseNumT) / base_level.abs();
        let frac =
            (elapsed_playing_duration.as_secs_f32() / envelope.attack_length.as_secs_f32()).clamp(0.0, 1.0) as BaseNumT;
        gain = attack_level_norm + (1.0 - attack_level_norm) * frac;
    }
    if !envelope.fade_length.is_zero() {
        let fade_start = effect_duration.saturating_sub(envelope.fade_length);
        if elapsed_playing_duration >= fade_start {
            let into_fade = elapsed_playing_duration.saturating_sub(fade_start);
            let frac = (into_fade.as_secs_f32() / envelope.fade_length.as_secs_f32()).clamp(0.0, 1.0) as BaseNumT;
            let fade_level_norm = (envelope.fade_level as BaseNumT) / base_level.abs();
            gain = 1.0 + (fade_level_norm - 1.0) * frac;
        }
    }
    gain.clamp(0.0, 1.0)
}

fn condition_force_symm_norm(
    value_symm_norm: BaseNumT,
    condition_slice: &[FfEffectCondition; 2],
    component_index: usize,
) -> BaseNumT {
    let condition = &condition_slice[component_index];
    let delta_to_center = value_symm_norm - condition.center_symm_norm;
    if delta_to_center.abs() <= condition.half_deadband_norm {
        return 0.0;
    }

    // dbg!(&delta_to_center.abs());
    let delta_to_center_no_deadband = (delta_to_center.abs() - condition.half_deadband_norm) * delta_to_center.signum();

    let coefficient = if delta_to_center_no_deadband > 0.0 {
        condition.right_coeff_symm_norm
    } else {
        condition.left_coeff_symm_norm
    };

    condition
        .saturation_interval_symm_norm
        .clamp(delta_to_center_no_deadband * -coefficient)
}

// ===========================================================================================
// ===========================================================================================
// ===========================================================================================
// ===========================================================================================
// ===========================================================================================
// ===========================================================================================

#[allow(clippy::too_many_arguments)]
pub(crate) async fn owned_hid_device_thread(
    platform_device: evdev::uinput::VirtualDevice,
    stop_token: CancellationToken,
    virtual_hid_name: String,
    max_effects: usize,
    fake_accepting_unsupported_effects: bool,
    owned_virtual_device_thread_io: Arc<OwnedVirtualHIDDeviceThreadIO>,
    mut app_comm: tokio::sync::mpsc::UnboundedReceiver<DeviceThreadCmd>, // TODO: move to owned_virtual_device_thread_io
    virtual_device_id: ObjId,
    ctl_states: Arc<DeviceControlStates>, // TODO: move to owned_virtual_device_thread_io
    debug_ff: bool,
) {
    // NB: acquiring here so that AsyncFd would associate with current runtime.
    let mut platform_device_stream = platform_device.into_event_stream().unwrap();

    let mut external_notification_tx: Option<tokio::sync::mpsc::UnboundedSender<HidDeviceEvent>> = None;
    let mut uploaded_effects_buffer: Vec<Option<FfEffect>> = Vec::new();
    uploaded_effects_buffer.resize_with(max_effects, || None);
    let mut played_effects = PlayedEffects::new();

    const JK_AXIS_VEL_SENS_FACTOR: BaseNumT = 0.5; // TODO: make configurable.
    let mut jk_axis_pos_filters = [
        OneEuroFilter::new(0.0 /* TODO: Should be relevant axis init value */, Instant::now()),
        OneEuroFilter::new(0.0 /* TODO: Should be relevant axis init value */, Instant::now()),
    ];
    let mut jk_axis_velocities = [0.0 as BaseNumT, 0.0];
    let mut jk_axis_pos_filtered = [0.0 as BaseNumT, 0.0];

    let mut ticker = tokio::time::interval(Duration::from_millis(16));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    while !stop_token.is_cancelled() {
        tokio::select! {
            _ = ticker.tick().with_cancellation_token(&stop_token) => {
                // dbg!("A");

                for axis_idx in AXIS_IDX_LIST {
                    // dbg!(owned_virtual_device_thread_io.axis_pos_symm_norm[axis_idx].load(Ordering::Relaxed));
                    jk_axis_pos_filtered[axis_idx] = jk_axis_pos_filters[axis_idx].filter(
                        owned_virtual_device_thread_io.axis_pos_symm_norm[axis_idx].load(Ordering::Relaxed) as BaseNumT,
                        Instant::now(),
                        40.0,
                        0.007,
                        2.0,
                    ) ;
                    jk_axis_velocities[axis_idx] = jk_axis_pos_filters[0].get_dx_prev() * JK_AXIS_VEL_SENS_FACTOR;
                }

                let play_sums = play(
                    &mut played_effects,
                    &uploaded_effects_buffer,
                    jk_axis_pos_filtered,
                    jk_axis_velocities,
                    debug_ff,
                );

                for axis_idx in AXIS_IDX_LIST {
                    if let Some(tx) = &mut external_notification_tx
                        && play_sums[axis_idx] != owned_virtual_device_thread_io.force_sum[axis_idx].load(Ordering::Relaxed) as BaseNumT
                    {
                        let _ = tx.send(DeviceEvent {
                            device_id: virtual_device_id,
                            data: HidEvent {
                                control_type: if axis_idx == X_AXIS_IDX {
                                    crate::mapped_controls::MappedCtls::ForceFeedbackX
                                } else {
                                    crate::mapped_controls::MappedCtls::ForceFeedbackY
                                },
                                value: HID_AXIS_MAX_INTERVAL.map_from(
                                    play_sums[axis_idx],
                                    &SYMM_UNIT_INTERVAL,
                                    crate::num_interval::OutOfRangePolicy::WarnIfDebugAndClamp,
                                ),
                            },
                        });
                    }
                    // --------------------------
                    owned_virtual_device_thread_io.force_sum[axis_idx].store(play_sums[axis_idx] as BaseNumT, Ordering::Release);
                    // --------------------------
                    ctl_states[if axis_idx == X_AXIS_IDX {
                        crate::mapped_controls::MappedCtls::ForceFeedbackX
                    } else {
                        crate::mapped_controls::MappedCtls::ForceFeedbackY
                    } as usize]
                        .store(
                            HID_AXIS_MAX_INTERVAL.map_from(
                                play_sums[axis_idx],
                                &SYMM_UNIT_INTERVAL,
                                crate::num_interval::OutOfRangePolicy::WarnIfDebugAndClamp,
                            ) as BaseNumT,
                            Ordering::Relaxed,
                        );
                }
            },
            Some(Some(cmd)) = app_comm.recv().with_cancellation_token(&stop_token)  => {
                // dbg!("B");
                match cmd {
                    DeviceThreadCmd::SetExternalNotification(tx) => external_notification_tx = Some(tx),
                    DeviceThreadCmd::SetControlValue(control_type, control_value) => {
                        set_hid_control_virtual_owned_device(platform_device_stream.device_mut(), control_type, control_value);
                    }
                }
            },
            Some(Ok(event)) =  platform_device_stream.next_event().with_cancellation_token(&stop_token) => {
                // dbg!("C");
                match event.destructure() {
                    evdev::EventSummary::ForceFeedbackStatus(ffevent, ffeffect_status_code, i32val) => {
                        if debug_ff {
                            log::warn!(
                                "[w] FFB STATUS event, ignoring, unmplemented: {:?}, STATUS CODE: {:?}. VAL: {}",
                                ffevent,
                                ffeffect_status_code,
                                i32val
                            );
                        }
                    }
                    evdev::EventSummary::Repeat(repeat_event, repeat_code, i32val) => {
                        if debug_ff {
                            log::warn!(
                                "[w] FFB REPEAT event, ignoring, unmplemented: {:?}, STATUS CODE: {:?}. VAL: {}",
                                repeat_event,
                                repeat_code,
                                i32val
                            );
                        }
                    }
                    evdev::EventSummary::ForceFeedback(
                        ffevent,
                        ffeffect_code,
                        /* "the ``code'' actually needs to be interpreted for this type of message
                        as ``uploaded effect id/index''. Are your towels still with you?*/
                        i32val, /*
                                    0 is stop.
                                    >= 1 && < i32::MAX is play this number of times.
                                    i32::MAX is used to play "indefinitely".
                                */
                    ) => {
                        if debug_ff {
                            log::debug!("[i] FF PLAY|STOP REQUEST: {:?}", ffevent);
                        }
                        match evdev::FFStatusCode(i32val as u16) {
                            evdev::FFStatusCode::FF_STATUS_STOPPED => {
                                set_stopped(
                                    ffeffect_code.to_index() as FfIndexT,
                                    &uploaded_effects_buffer,
                                    &mut played_effects,
                                    debug_ff,
                                );
                            }
                            _ if i32val >= 1 => {
                                if debug_ff {
                                    if i32val == i32::MAX {
                                        log::debug!("[i] FF PLAY REQUEST: client uses i32::MAX as a count.");
                                    } else {
                                        log::debug!(
                                            "[i] FF PLAY REQUEST: client uses 1 <= {i32val} < i32::MAX as a count."
                                        );
                                    }
                                }
                                set_played(
                                    ffeffect_code.to_index() as FfIndexT,
                                    i32val,
                                    &uploaded_effects_buffer,
                                    &mut played_effects,
                                    debug_ff,
                                );
                            }
                            _ => {
                                log::error!(
                                    "[e] FF PLAY|STOP REQUEST: unhandled value \
                                (not >=0 ! Expected: 0 is ``stop'', >= 1 is ``play this count of times'')): {event:?}"
                                );
                            }
                        }
                    }
                    evdev::EventSummary::UInput(uinput_event, uinput_code, _i32val) => {
                        match uinput_code {
                            evdev::UInputCode::UI_FF_UPLOAD => {
                                let mut eff = platform_device_stream
                                    .device_mut()
                                    .process_ff_upload(uinput_event)
                                    .unwrap();

                                let maybe_updated_effect_id = if eff.effect_id() == -1 {
                                    None
                                } else {
                                    Some(eff.effect_id() as usize)
                                };

                                if debug_ff {
                                    log::debug!(
                                        "[^^^] UPLOADING ({}): {:?}",
                                        if maybe_updated_effect_id.is_some() {
                                            "updating"
                                        } else {
                                            "new"
                                        },
                                        eff.effect()
                                    );
                                }
                                let assigned_effect_index = match eff.effect().kind {
                                    evdev::FFEffectKind::Constant { level, envelope } => upload_effect(
                                        maybe_updated_effect_id,
                                        FfEffect::ConstantForce {
                                            level,
                                            direction: eff.effect().direction,
                                            replay: eff.effect().replay.into(),
                                            envelope: envelope.into(),
                                        },
                                        &mut uploaded_effects_buffer,
                                        debug_ff,
                                    ),
                                    evdev::FFEffectKind::Ramp {
                                        start_level,
                                        end_level,
                                        envelope,
                                    } => upload_effect(
                                        maybe_updated_effect_id,
                                        FfEffect::RampForce {
                                            start_level,
                                            end_level,
                                            direction: eff.effect().direction,
                                            replay: eff.effect().replay.into(),
                                            envelope: envelope.into(),
                                        },
                                        &mut uploaded_effects_buffer,
                                        debug_ff,
                                    ),
                                    evdev::FFEffectKind::Friction { condition } => upload_effect(
                                        maybe_updated_effect_id,
                                        FfEffect::Friction {
                                            condition_norm: condition.map(|evdev_cond| evdev_cond.into()),
                                            _direction: eff.effect().direction,
                                            replay: eff.effect().replay.into(),
                                        },
                                        &mut uploaded_effects_buffer,
                                        debug_ff,
                                    ),
                                    evdev::FFEffectKind::Spring { condition } => upload_effect(
                                        maybe_updated_effect_id,
                                        FfEffect::Spring {
                                            condition_norm: condition.map(|evdev_cond| evdev_cond.into()),
                                            _direction: eff.effect().direction,
                                            replay: eff.effect().replay.into(),
                                        },
                                        &mut uploaded_effects_buffer,
                                        debug_ff,
                                    ),
                                    // NB: waiting for evdev to apply the patch that propagates
                                    // NB: condition info for those effects.
                                    // evdev::FFEffectKind::Damper { condition } => upload_effect(
                                    //     maybe_updated_effect_id,
                                    //     FfEffect::Damper {
                                    //         condition_norm: condition
                                    //             .map(|evdev_cond| evdev_cond.into()),
                                    //         _direction: eff.effect().direction,
                                    //         replay: eff.effect().replay.into(),
                                    //     },
                                    //     &mut uploaded_effects_buffer,
                                    //     debug_ff,
                                    // ),
                                    // evdev::FFEffectKind::Inertia { condition } => upload_effect(
                                    //     maybe_updated_effect_id,
                                    //     FfEffect::Inertia {
                                    //         condition_norm: condition
                                    //             .map(|evdev_cond| evdev_cond.into()),
                                    //         _direction: eff.effect().direction,
                                    //         replay: eff.effect().replay.into(),
                                    //     },
                                    //     &mut uploaded_effects_buffer,
                                    //     debug_ff,
                                    // ),
                                    // evdev::FFEffectKind::Periodic { waveform, period, magnitude, offset, phase, envelope } |
                                    // evdev::FFEffectKind::Rumble { strong_magnitude, weak_magnitude } => {
                                    _ => {
                                        if fake_accepting_unsupported_effects {
                                            if debug_ff {
                                                log::warn!("[i] FAKING UPLOAD for unsupported effect {:?}", eff.effect());
                                            }
                                            upload_effect(
                                                maybe_updated_effect_id,
                                                FfEffect::NopEffect,
                                                &mut uploaded_effects_buffer,
                                                debug_ff,
                                            )
                                        } else {
                                            Err(anyhow::anyhow!(
                                                "[i] IGNORING UPLOAD for unsupported effect {:?}",
                                                eff.effect()
                                            ))
                                        }
                                    }
                                };

                                match assigned_effect_index {
                                    Ok(assigned_effect_index) => {
                                        if debug_ff {
                                            log::debug!(
                                                "[***] UPLOADED to index {}: {:?}",
                                                assigned_effect_index,
                                                eff.effect()
                                            );
                                        }
                                        eff.set_effect_id(assigned_effect_index as i16);
                                        eff.set_retval(0);
                                    }
                                    Err(e) => {
                                        log::error!("{e}");
                                        eff.set_retval(-1);
                                    }
                                }
                            }
                            evdev::UInputCode::UI_FF_ERASE => {
                                let eff = platform_device_stream.device_mut().process_ff_erase(uinput_event).unwrap();

                                let effect_index_to_erase = eff.effect_id() as FfIndexT;

                                if debug_ff {
                                    log::debug!("[<<<] ERASING at index {}", effect_index_to_erase);
                                }

                                if uploaded_effects_buffer.len() > effect_index_to_erase {
                                    uploaded_effects_buffer[effect_index_to_erase] = None;
                                } else {
                                    log::error!(
                                        "Requested to erase effect index {effect_index_to_erase}, but max effects is {}",
                                        uploaded_effects_buffer.len()
                                    )
                                }
                            }
                            _ => {
                                if debug_ff {
                                    log::warn!("[w] UNHANDLED UINPUT EVENT: {:?}", uinput_event);
                                }
                            }
                        }
                    }
                    event => {
                        if debug_ff {
                            log::error!("[w] UNHANDLED Virtual HID event: {:?}", event);
                        }
                    }
                }
            },
            else => break
        }
    }

    log::info!("Stopping thread for owned virtual HID {virtual_hid_name} (this thread handles FF effects)");
}
