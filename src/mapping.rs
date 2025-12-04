use anyhow::{bail, Context, Result};
use atomic_float::AtomicF32;
use log::{debug, info, warn};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use std::usize;
use tokio::select;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::common::NumInterval;
use crate::config::{ConfigManager, ControlReference, ResolvedMapping};
use crate::interpolation::{InterpolationCurve, ValueFilter};
use crate::joystick::VirtualJoystickManager;
use crate::midi::{MidiManager, MidiMessage};
use crate::mouse::{MouseEvent, MouseManager};
use crate::schemas::{
    IntegrateTransform, ResolvedHoldFactor, ResolvedPedalSmootherTransform,
    ResolvedSteeringTransform, ResolvedTransformationStep, StepRuntimeStateId,
};

struct TransformStepState {
    time1: HashMap<StepRuntimeStateId, Instant>,
    time2: HashMap<StepRuntimeStateId, Instant>,
    #[allow(unused)]
    usize_1: HashMap<StepRuntimeStateId, usize>,
    f32_1: HashMap<StepRuntimeStateId, f32>,
    f32_2: HashMap<StepRuntimeStateId, f32>,
    #[allow(unused)]
    vec_deq_1: HashMap<StepRuntimeStateId, VecDeque<f32>>,
    #[allow(unused)]
    vec_deq_timestamped: HashMap<StepRuntimeStateId, VecDeque<(Instant, f32)>>,
}

impl TransformStepState {
    fn new() -> Self {
        Self {
            time1: HashMap::new(),
            time2: HashMap::new(),
            usize_1: HashMap::new(),
            f32_1: HashMap::new(),
            f32_2: HashMap::new(),
            vec_deq_timestamped: HashMap::new(),
            vec_deq_1: HashMap::new(),
        }
    }
}

pub(crate) struct MappingEngine<'cfg> {
    config_manager: &'cfg ConfigManager,
    midi_manager: MidiManager,
    mouse_manager: MouseManager,
    joystick_manager: &'cfg VirtualJoystickManager,
    debug: bool,
    debug_idle_tick: bool,
    update_rate: u32,
    // _latency_mode: String,
    running: bool,
    moving_average_step_data: RefCell<TransformStepState>,
    transform_step_data: RefCell<TransformStepState>,
    router: HashMap<String, Vec<&'cfg ResolvedMapping>>,
    idle_tick_mappings: Vec<&'cfg ResolvedMapping>,
    enable_steering_indicator_window: bool,
    steering_indicator_pos: Arc<AtomicF32>,
    steering_indicator_hold: Arc<AtomicF32>,
    overlay_thread_cancellation_token: CancellationToken,
    overlay_thread_join_handle: Option<tokio::task::JoinHandle<()>>,
}

impl<'cfg> MappingEngine<'cfg> {
    pub(crate) fn new(
        config_manager: &'cfg ConfigManager,
        midi_manager: MidiManager,
        mouse_manager: MouseManager,
        joystick_manager: &'cfg VirtualJoystickManager,
        debug: bool,
        debug_idle_tick: bool,
        enable_steering_indicator_window: bool,
    ) -> Result<Self> {
        Ok(Self {
            config_manager,
            midi_manager,
            mouse_manager,
            joystick_manager,
            debug,
            debug_idle_tick,
            update_rate: config_manager.get_config().global.idle_tick_update_rate,
            // _latency_mode: "normal".to_string(),
            running: false,
            moving_average_step_data: TransformStepState::new().into(),
            transform_step_data: TransformStepState::new().into(),
            router: HashMap::new(),
            idle_tick_mappings: Vec::new(),
            enable_steering_indicator_window,
            steering_indicator_pos: Arc::new(0.0.into()),
            steering_indicator_hold: Arc::new(0.0.into()),
            overlay_thread_cancellation_token: CancellationToken::new(),
            overlay_thread_join_handle: None,
        })
    }

    pub(crate) fn set_update_rate(&mut self, rate: u32) {
        self.update_rate = rate.clamp(10, 10000);
    }

    // pub(crate) fn _set_latency_mode(&mut self, mode: &str) {
    //     self._latency_mode = mode.to_string();
    // }

    pub(crate) fn active_mapping_count(&self) -> usize {
        self.router.values().map(|v| v.len()).sum()
    }

    pub(crate) async fn initialize(&mut self) -> Result<()> {
        // let config = self.config_manager.get_config();
        let all_mappings = self.config_manager.get_mappings();

        info!("Initializing Mapping Engine...");

        // NB: Order of iteration for BTreeSet is stable,
        // NB: we need it to spawn joysticks in same order,
        // NB: faster alternatives could be OrderSet or IndexSet...
        let mut required_dst_device_keys: BTreeSet<String> = BTreeSet::new();
        let mut required_src_device_keys: HashSet<String> = HashSet::new();

        for mapping in all_mappings {
            if mapping.enabled {
                required_dst_device_keys.insert(mapping.destination.device_key.clone());
                required_src_device_keys.insert(mapping.source.device_key.clone());
            } else {
                warn! {"Mapping {:?} is not enabled. Ignoring.", self.mapping_to_string(mapping)};
            }
        }

        let mut opened_virtual_joysticks: BTreeSet<String> = BTreeSet::new();
        for vjoy_key in required_dst_device_keys {
            let resolved_device = self
                .config_manager
                .get_resolved_virtual_joystick(&vjoy_key)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Virtual joystick '{}' not found in resolved config",
                        vjoy_key
                    )
                })?;

            if !resolved_device.enabled {
                warn!(
                    "Joystick device {} is not enabled, ignoring it \
                    and all the associated mappings.",
                    resolved_device.name
                );
                continue;
            }

            opened_virtual_joysticks.insert(vjoy_key.clone());
            if self.debug {
                debug!("Using Virtual Joystick: {}", vjoy_key);
            }
        }

        let mut runtime_device_name_to_config_device_key: HashMap<String, String> = HashMap::new();

        let available_midi = self.midi_manager.enumerate_devices();
        for src_device_key in &required_src_device_keys {
            if let Some(resolved_device) =
                self.config_manager.get_resolved_midi_device(src_device_key)
            {
                if !resolved_device.enabled {
                    warn!(
                        "MIDI devices with name regex pattern {:?} are not enabled, ignoring config entry '{}' \
                     and all the associated mappings.",
                        resolved_device.match_name_regex, src_device_key
                    );
                    continue;
                }

                if let Some(pattern) = &resolved_device.match_name_regex {
                    let matched = self.midi_manager.match_device(pattern, &available_midi);

                    for device_name in matched {
                        match self.midi_manager.open_device(&device_name) {
                            Ok(_) => {
                                if self.debug {
                                    debug!(
                                        "Opened Source MIDI: {} (for key: {})",
                                        device_name, src_device_key
                                    );
                                }
                                runtime_device_name_to_config_device_key
                                    .insert(device_name, src_device_key.clone());
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to open matched MIDI device '{}': {}",
                                    device_name, e
                                );
                            }
                        }
                    }
                }
            }
        }

        let available_mice = self.mouse_manager.enumerate_devices()?;
        for src_device_key in &required_src_device_keys {
            if let Some(resolved_device) = self
                .config_manager
                .get_resolved_mouse_device(src_device_key)
            {
                if !resolved_device.enabled {
                    warn!(
                        "Mouse devices with name regex pattern {:?} are not enabled, ignoring config entry '{}' \
                     and all the associated mappings.",
                        resolved_device.match_name_regex, src_device_key
                    );
                    continue;
                }

                if let Some(pattern) = &resolved_device.match_name_regex {
                    let matched = self.mouse_manager.match_device(pattern, &available_mice);
                    for device_info in matched {
                        match self.mouse_manager.open_device(&device_info, src_device_key) {
                            Ok(_) => {
                                if self.debug {
                                    info!(
                                        "Opened Source Mouse: {} (as {})",
                                        device_info.name, src_device_key
                                    );
                                }
                                runtime_device_name_to_config_device_key
                                    .insert(src_device_key.clone(), src_device_key.clone());
                            }
                            Err(e) => {
                                warn!("Failed to open mouse '{}': {}", device_info.name, e)
                            }
                        }
                    }
                }
            }
        }

        for mapping in all_mappings {
            if !mapping.enabled {
                continue;
            }

            if !opened_virtual_joysticks.contains(&mapping.destination.device_key) {
                continue;
            }

            for (runtime_device_name, config_device_key) in
                &runtime_device_name_to_config_device_key
            {
                if *config_device_key == mapping.source.device_key {
                    self.router
                        .entry(runtime_device_name.clone())
                        .or_default()
                        .push(mapping);

                    if self.requires_idle_tick(mapping) {
                        self.idle_tick_mappings.push(mapping);
                    }
                }
            }
        }

        info!("Router built. Active Source Devices: {}", self.router.len());
        Ok(())
    }

    pub(crate) async fn run(&mut self) -> Result<()> {
        self.running = true;

        // Spawn the UI thread for mouse steering indicator window.
        // TODO: spawn a window for every steering transfrom
        if self.enable_steering_indicator_window {
            let steering_pos = self.steering_indicator_pos.clone();
            let steering_hold = self.steering_indicator_hold.clone();
            let cancellation_token = self.overlay_thread_cancellation_token.clone();
            self.overlay_thread_join_handle = Some(tokio::task::spawn_blocking(move || {
                crate::overlay::run_steering_indicator_window_overlay(
                    steering_pos,
                    steering_hold,
                    cancellation_token,
                );
            }));
        }

        let mut ticker = interval(Duration::from_secs_f64(1.0 / self.update_rate as f64));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        info!("Mapping engine running at {} Hz", self.update_rate);

        while self.running {
            select! {
                Some(midi_msg) = self.midi_manager.get_message() => {
                    self.process_midi_message(midi_msg).await?;
                }
                Some(mouse_event) = self.mouse_manager.get_event() => {
                    self.process_mouse_event(mouse_event).await?;
                }
                _ = ticker.tick() => {
                    self.process_idle_tick().await?;
                }
            }
        }

        Ok(())
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        self.running = false;
        if let Some(overlay_handle) = &self.overlay_thread_join_handle {
            self.overlay_thread_cancellation_token.cancel();
            overlay_handle.abort();
        }
        // NB: joysticks are stopped/started/restarted externally to mapping engine
        // NB: to support persistence.
        let midi_stop_result = self
            .midi_manager
            .stop()
            .context("Failed to stop Midi Manager.");
        let mouse_stop_result = self
            .mouse_manager
            .stop()
            .context("Failed to stop Mouse Manager.");

        let errors: Vec<String> = [midi_stop_result, mouse_stop_result]
            .into_iter()
            .filter_map(|res| res.err().map(|e| format!("- {}", e)))
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            let error_message = format!(
                "One or more managers failed to stop:\n{}",
                errors.join("\n")
            );
            bail!(error_message)
        }
    }

    fn set_idle_tick_enabled_on_device_control_for_mapping(&self, mapping: &ResolvedMapping) {
        let idle_tick_required = self.requires_idle_tick(mapping);
        let prev = mapping
            .destination
            .control
            .idle_tick_enabled_flag
            .swap(idle_tick_required, std::sync::atomic::Ordering::Relaxed);
        if idle_tick_required != prev && self.debug {
            debug!(
                "Idle-tick update for {}/{}: {} -> {}",
                mapping.destination.device_key,
                mapping.destination.control_key,
                prev,
                idle_tick_required
            );
        }
    }

    async fn process_midi_message(&self, msg: MidiMessage) -> Result<()> {
        let device_mappings = match self.router.get(&msg.device_name) {
            Some(m) => m,
            None => return Ok(()),
        };
        for mapping in device_mappings {
            if self.midi_manager.midi_message_matches_spec(&msg, mapping) {
                let value = self.midi_manager.extract_midi_value(&msg);
                self.execute_mapping_on_active_input(
                    msg.device_name.as_str(),
                    mapping,
                    value as f32,
                )?;
            }
        }
        Ok(())
    }

    async fn process_mouse_event(&self, event: MouseEvent) -> Result<()> {
        let device_mappings = match self.router.get(&event.device_key) {
            Some(m) => m,
            None => return Ok(()),
        };
        for mapping in device_mappings {
            if let ControlReference::Mouse(mouse_ctrl) = &mapping.source.control {
                if mouse_ctrl.r#type == event.control_type {
                    self.execute_mapping_on_active_input(
                        event.device_key.as_str(),
                        mapping,
                        event.value as f32,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn execute_mapping_on_active_input(
        &self,
        runtime_input_device_name: &str,
        mapping: &'cfg ResolvedMapping,
        input_value: f32,
    ) -> Result<()> {
        let final_value =
            self.apply_transformation(runtime_input_device_name, mapping, input_value, false)?;
        self.joystick_manager.set_control_value(
            &mapping.destination.device_key,
            &mapping.destination.control_key,
            final_value,
            false,
        )?;
        self.set_idle_tick_enabled_on_device_control_for_mapping(mapping);
        if self.debug {
            debug!(
                "Mapped {}/{} -> {}/{}: {} -> {}",
                mapping.source.device_key,
                mapping.source.control_key,
                mapping.destination.device_key,
                mapping.destination.control_key,
                input_value,
                final_value
            );
        }
        Ok(())
    }

    async fn process_idle_tick(&self) -> Result<()> {
        for mapping in &self.idle_tick_mappings {
            if !mapping
                .destination
                .control
                .idle_tick_enabled_flag
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                continue;
            }

            let final_value =
                self.apply_transformation("<-- idle tick device -->", mapping, 0.0, true)?;

            self.joystick_manager.set_control_value(
                &mapping.destination.device_key,
                &mapping.destination.control_key,
                final_value,
                /*silent:*/ !self.debug_idle_tick,
            )?;
        }
        Ok(())
    }

    fn requires_idle_tick(&self, mapping: &ResolvedMapping) -> bool {
        let mut idle_tick_requirement_info = mapping.idle_tick_requirement_info__.lock().unwrap();
        // TODO: must move to config parsing stage.
        if idle_tick_requirement_info.is_required.is_none() {
            idle_tick_requirement_info.is_required = Some(mapping.transformation.iter().any(|s| {
                matches!(
                    s,
                    ResolvedTransformationStep::Steering { .. }
                        | ResolvedTransformationStep::PedalSmoother { .. }
                )
            }));
        };
        idle_tick_requirement_info.is_required.unwrap_or_default()
    }

    fn apply_transformation(
        &self,
        runtime_input_device_name: &str,
        mapping: &'cfg ResolvedMapping,
        value: f32,
        is_idle_tick: bool,
    ) -> Result<f32> {
        // TODO: simplify: both either optional or not.
        let src_range = match &mapping.source.control {
            ControlReference::Mouse(mouse_control) => Some(mouse_control.range),
            ControlReference::Midi(midi_control) => midi_control.range,
        }
        .unwrap_or(NumInterval::new(0, 127))
        .cast::<f32>()
        .unwrap();

        let dst_range = mapping
            .destination
            .control
            .range
            .cast::<f32>()
            .unwrap_or(NumInterval::new(i32::MIN as f32, i32::MAX as f32));

        let mut current_value = value;
        let mut current_range = src_range;

        if !src_range.contains_inclusive(current_value) {
            warn!(
                "The value (={current_value}) read from device {runtime_input_device_name} \
            is out of configured range ({current_range:?}), clamping it."
            );
            current_value = current_range.clamp(current_value);
        }

        for step in mapping.transformation.iter() {
            (current_value, current_range) = self.apply_transformation_step(
                mapping,
                step,
                current_value,
                current_range,
                dst_range,
                is_idle_tick,
            )?;
        }

        if current_range != dst_range {
            current_value = dst_range.map_from(current_value, &current_range, false);
        }

        // log::error!("{current_range:?} {src_range:?} {dst_range:?}
        //         {current_value} {}", current_value as i32);

        Ok(dst_range.clamp(current_value))
    }

    fn apply_transformation_step(
        &self,
        mapping: &'cfg ResolvedMapping,
        step: &ResolvedTransformationStep,
        value: f32,
        current_range: NumInterval<f32>,
        dst_range: NumInterval<f32>,
        is_idle_tick: bool,
    ) -> Result<(f32, NumInterval<f32>)> {
        match step {
            ResolvedTransformationStep::Invert { invert } => {
                Ok(self.apply_invert_transform(invert, value, current_range))
            }
            ResolvedTransformationStep::Integrate {
                runtime_state_id,
                integrate,
            } => {
                if is_idle_tick {
                    return Ok((value, current_range));
                }
                Ok(self.apply_integrate_transform(
                    mapping,
                    *runtime_state_id,
                    integrate,
                    value,
                    current_range,
                ))
            }
            ResolvedTransformationStep::Clamp { clamp } => {
                let clamp_range = NumInterval::new(
                    clamp.from.unwrap_or(current_range.from as i32) as f32,
                    clamp.to.unwrap_or(current_range.to as i32) as f32,
                );
                Ok((
                    clamp_range.clamp(value),
                    if clamp.override_range {
                        clamp_range
                    } else {
                        current_range
                    },
                ))
            }
            ResolvedTransformationStep::Steering {
                runtime_state_id,
                steering,
            } => Ok(self.apply_steering_transform(
                mapping,
                *runtime_state_id,
                steering,
                value,
                dst_range,
            )),
            ResolvedTransformationStep::PedalSmoother {
                runtime_state_id,
                pedal_smoother,
            } => Ok(self.apply_pedal_smoother_transform(
                mapping,
                *runtime_state_id,
                pedal_smoother,
                value,
                current_range,
                is_idle_tick,
            )?),
            ResolvedTransformationStep::EmaFilter {
                runtime_state_id,
                ema_filter: moving_average,
            } => {
                if is_idle_tick && !moving_average.on_idle.unwrap_or(true) {
                    let _ = // State update.
                        self.apply_ema(*runtime_state_id, moving_average, value);
                    return Ok((value, current_range));
                }
                Ok((
                    self.apply_ema(*runtime_state_id, moving_average, value),
                    current_range,
                ))
            }
            ResolvedTransformationStep::Linear { linear } => {
                if is_idle_tick && !linear.on_idle.unwrap_or(true) {
                    return Ok((value, current_range));
                }
                Ok(self.apply_linear_transform(linear, value, current_range))
            }
            ResolvedTransformationStep::Quadratic {
                quadratic_curve: quadratic,
            } => {
                if is_idle_tick && !quadratic.on_idle.unwrap_or(true) {
                    return Ok((value, current_range));
                }
                Ok(self.apply_quadratic_transform(value, current_range))
            }
            ResolvedTransformationStep::Cubic { cubic_curve: cubic } => {
                if is_idle_tick && !cubic.on_idle.unwrap_or(true) {
                    return Ok((value, current_range));
                }
                Ok(self.apply_cubic_transform(value, current_range))
            }
            ResolvedTransformationStep::Smoothstep {
                smoothstep_curve: smoothstep,
            } => {
                if is_idle_tick && !smoothstep.on_idle.unwrap_or(true) {
                    return Ok((value, current_range));
                }
                Ok(self.apply_smoothstep_transform(value, current_range))
            }
            ResolvedTransformationStep::SCurve { s_curve } => {
                if is_idle_tick && !s_curve.on_idle.unwrap_or(true) {
                    return Ok((value, current_range));
                }
                Ok(self.apply_s_curve_transform(s_curve, value, current_range))
            }
            ResolvedTransformationStep::Exponential { exp_curve: exp } => {
                if is_idle_tick && !exp.on_idle.unwrap_or(true) {
                    return Ok((value, current_range));
                }
                Ok(self.apply_exponential_transform(exp, value, current_range))
            }
            ResolvedTransformationStep::SymmetricPower {
                symmetric_power_curve: symmetric_power,
            } => {
                if is_idle_tick && !symmetric_power.on_idle.unwrap_or(true) {
                    return Ok((value, current_range));
                }
                Ok((
                    self.apply_symmetric_power_transform(symmetric_power, value, &current_range),
                    current_range,
                ))
            }
            ResolvedTransformationStep::Power { power_curve: power } => {
                if is_idle_tick && !power.on_idle.unwrap_or(true) {
                    return Ok((value, current_range));
                }
                Ok(self.apply_power_transform(power, value, current_range))
            }
            ResolvedTransformationStep::LowPass {
                runtime_state_id,
                lowpass,
            } => {
                if is_idle_tick && !lowpass.on_idle.unwrap_or(true) {
                    // Update the state on idle.
                    let _ = self.apply_low_pass_transform(*runtime_state_id, lowpass, value);
                    return Ok((value, current_range));
                }
                Ok((
                    self.apply_low_pass_transform(*runtime_state_id, lowpass, value),
                    current_range,
                ))
            }
            ResolvedTransformationStep::HighPass {
                runtime_state_id: _runtime_state_id,
                highpass,
            } => {
                if is_idle_tick && !highpass.on_idle.unwrap_or(true) {
                    // Update the state on idle.
                    let _ = self.apply_high_pass_transform(step, highpass, value);
                    return Ok((value, current_range));
                }
                Ok((
                    self.apply_high_pass_transform(step, highpass, value),
                    current_range,
                ))
            }
        }
    }

    fn apply_ema(
        &self,
        runtime_state_id: StepRuntimeStateId,
        moving_average: &crate::schemas::EmaFilterTransform,
        value: f32,
    ) -> f32 {
        let mut data = self.moving_average_step_data.borrow_mut();
        let now = Instant::now();
        let prev_time = data.time1.entry(runtime_state_id).or_insert(now).clone();
        let prev_val = data.f32_1.entry(runtime_state_id).or_insert(value).clone();
        ValueFilter::ema(
            prev_val,
            value,
            (now - prev_time).as_secs_f32(),
            moving_average.tau,
        )
    }

    fn apply_low_pass_transform(
        &self,
        runtime_state_id: StepRuntimeStateId,
        lowpass: &crate::schemas::LowPassTransform,
        current_input: f32,
    ) -> f32 {
        let mut data = self.transform_step_data.borrow_mut();

        let now = Instant::now();
        let prev_time = data.time1.entry(runtime_state_id).or_insert(now);
        let dt = (now - *prev_time).as_secs_f32();
        *prev_time = now;

        let prev_val = *data.f32_1.entry(runtime_state_id).or_insert(current_input);
        let time_constant = lowpass.time_constant.unwrap_or(0.1);

        let out = ValueFilter::lowpass(prev_val, current_input, dt, time_constant);
        let _ = data.f32_1.insert(runtime_state_id, out);
        out
    }

    fn apply_invert_transform(
        &self,
        invert: &crate::schemas::InvertTransform,
        value: f32,
        range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        if invert.is_relative {
            (-value, range)
        } else {
            (range.clamp_and_invert(value), range)
        }
    }

    fn apply_linear_transform(
        &self,
        linear: &crate::schemas::LinearTransform,
        value: f32,
        range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        (
            range.denormalize_from_unit(
                InterpolationCurve::linear(
                    range.normalize_to_unit(value),
                    linear.slope.unwrap_or(1.),
                    linear.shift_x.unwrap_or(0.),
                    linear.shift_y.unwrap_or(0.),
                ),
                false,
            ),
            range,
        )
    }

    fn apply_quadratic_transform(
        &self,
        value: f32,
        range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        (
            range.denormalize_from_unit(
                InterpolationCurve::quadratic(range.normalize_to_unit(value)),
                false,
            ),
            range,
        )
    }

    fn apply_cubic_transform(
        &self,
        value: f32,
        range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        (
            range.denormalize_from_unit(
                InterpolationCurve::cubic(range.normalize_to_unit(value)),
                false,
            ),
            range,
        )
    }

    fn apply_smoothstep_transform(
        &self,
        value: f32,
        range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        (
            range.denormalize_from_unit(
                InterpolationCurve::smoothstep(range.normalize_to_unit(value)),
                false,
            ),
            range,
        )
    }

    fn apply_s_curve_transform(
        &self,
        s_curve: &crate::schemas::SCurveTransform,
        value: f32,
        range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        (
            range.denormalize_from_unit(
                InterpolationCurve::s_curve(
                    range.normalize_to_unit(value),
                    s_curve.steepness.unwrap_or(10.),
                ),
                false,
            ),
            range,
        )
    }

    fn apply_exponential_transform(
        &self,
        exponential: &crate::schemas::ExponentialTransform,
        value: f32,
        range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        (
            range.denormalize_from_unit(
                InterpolationCurve::exponential(
                    range.normalize_to_unit(value),
                    exponential.base.unwrap_or(2.),
                ),
                false,
            ),
            range,
        )
    }

    fn apply_symmetric_power_transform(
        &self,
        symmetric_power: &crate::schemas::SymmetricPowerTransform,
        value: f32,
        range: &NumInterval<f32>,
    ) -> f32 {
        range.denormalize_from_unit(
            InterpolationCurve::symmetric_power(
                range.normalize_to_unit(value),
                symmetric_power.power.unwrap_or(2.),
            ),
            false,
        )
    }

    fn apply_power_transform(
        &self,
        power: &crate::schemas::PowerTransform,
        value: f32,
        range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        (
            range.denormalize_from_unit(
                InterpolationCurve::power(
                    range.normalize_to_unit(value),
                    power.power.unwrap_or(2.),
                ),
                false,
            ),
            range,
        )
    }

    // fn apply_low_pass_transform(
    //     &self,
    //     _step: StepRuntimeStateId,
    //     _low_pass: &crate::schemas::LowPassTransform,
    //     _value: f32,
    // ) -> f32 {
    //     todo!()
    // }

    fn apply_high_pass_transform(
        &self,
        _step: &ResolvedTransformationStep,
        _high_pass: &crate::schemas::HighPassTransform,
        _value: f32,
    ) -> f32 {
        todo!()
    }

    fn apply_integrate_transform(
        &self,
        _mapping: &'cfg ResolvedMapping,
        runtime_state_id: StepRuntimeStateId,
        integrate: &IntegrateTransform,
        mut delta_value: f32,
        _current_range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        let integration_range = integrate.range.unwrap_or(NumInterval::new(0.0, 750.0));

        let deadzone = integrate.deadzone_norm.unwrap_or(0.0).max(0.0);
        if delta_value.abs() < deadzone * integration_range.span() {
            delta_value = 0.0;
        }

        let mut data = self.transform_step_data.borrow_mut();
        let prev = *data
            .f32_1
            .entry(runtime_state_id)
            .or_insert((integration_range.from + integration_range.to) * 0.5);

        let out_val = integration_range.clamp(prev + delta_value);
        *data.f32_1.entry(runtime_state_id).or_insert(0.0) = out_val;

        (out_val, integration_range)
    }

    fn apply_steering_transform(
        &self,
        mapping: &'cfg ResolvedMapping,
        runtime_state_id: StepRuntimeStateId,
        steering: &ResolvedSteeringTransform,
        value: f32,
        dst_range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        let last_pipeline_out = self.joystick_manager.get_control_state(
            &mapping.destination.device_key,
            &mapping.destination.control_key,
        );

        let mut pos_in_symm_unit = crate::common::SYMM_UNIT_INTERVAL.map_from(
            last_pipeline_out,
            &mapping.destination.control.range.cast().unwrap(),
            false,
        );

        self.steering_indicator_pos
            .store(pos_in_symm_unit, std::sync::atomic::Ordering::Relaxed);

        let mut data = self.transform_step_data.borrow_mut();

        let now = Instant::now();
        let dt = match data.time1.get(&(runtime_state_id)) {
            Some(prev) => (now - *prev).as_secs_f32(),
            _ => 0.0,
        };
        data.time1.insert(runtime_state_id, now);

        let counts_to_lock = steering.counts_to_lock.max(1.0);
        let delta = value / (counts_to_lock / 2.0);

        let last_pos_in_symm_unnit = pos_in_symm_unit;
        pos_in_symm_unit += delta;

        // NB: this augments user_input_ema_filter with a simpler smoothing technique
        //     in case  we don't use the EMA below.
        pos_in_symm_unit = (1.0 - steering.smoothing_alpha) * (last_pos_in_symm_unnit as f32)
            + steering.smoothing_alpha * pos_in_symm_unit;

        if delta != 0.0 {
            if let Some(symmetric_power) = &steering.user_input_power_curve {
                pos_in_symm_unit = self.apply_symmetric_power_transform(
                    symmetric_power,
                    pos_in_symm_unit,
                    &crate::common::SYMM_UNIT_INTERVAL,
                );
            }

            if let Some(moving_average) = &steering.user_input_ema_filter_average {
                pos_in_symm_unit = self.apply_ema(
                    steering.user_input_ema_filter_runtime_state_id,
                    moving_average,
                    pos_in_symm_unit,
                );
            }
        }

        {
            let hold_factor_unit = match &steering.hold_factor {
                Some(ResolvedHoldFactor::Value(v)) => *v,
                Some(ResolvedHoldFactor::Reference {
                    device,
                    control,
                    range,
                }) => {
                    let val = self.joystick_manager.get_control_state(device, control);
                    crate::common::UNIT_INTERVAL.map_from(val as f32, &range.cast().unwrap(), false)
                }
                None => 0.0,
            };

            self.steering_indicator_hold
                .store(hold_factor_unit, std::sync::atomic::Ordering::Relaxed);

            let ff_force_norm = if let Some(ff_config) = &steering.force_feedback {
                if ff_config.enabled {
                    let raw_force = self
                        .joystick_manager
                        .get_ff_constant_force_norm(&mapping.destination.device_key);
                    let scaled_force = raw_force * ff_config.constant_force_scale;
                    if ff_config.constant_force_invert {
                        -scaled_force
                    } else {
                        scaled_force
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            if ff_force_norm.abs() > 0.001 {
                let influence = steering
                    .force_feedback
                    .as_ref()
                    .map(|f| f.constant_force_influence)
                    .unwrap_or(0.7);
                let ff_position_offset = ff_force_norm * (1.0 - hold_factor_unit) * influence * dt;
                pos_in_symm_unit += ff_position_offset;

                if self.debug && self.debug_idle_tick && ff_force_norm.abs() > 0.1 {
                    debug!(
                        "FF active: force={:.3} offset={:.3}",
                        ff_force_norm, ff_position_offset
                    );
                }
            }

            if steering.auto_center_halflife > 0.0
                && dt > 0.0
                && ff_force_norm.abs() < 0.001
                && delta == 0.0
            {
                let k = 1.0 - (2.0_f32).powf(-dt / steering.auto_center_halflife);
                let k = k * (1.0 - hold_factor_unit).clamp(0.0, 1.0);
                pos_in_symm_unit += (0.0 - pos_in_symm_unit) * k;
            }
        }

        let out = dst_range.map_from(
            crate::common::SYMM_UNIT_INTERVAL.clamp(pos_in_symm_unit),
            &crate::common::SYMM_UNIT_INTERVAL,
            false,
        );

        (out, dst_range)
    }

    fn apply_pedal_smoother_transform(
        &self,
        _mapping: &'cfg ResolvedMapping,
        runtime_state_id: StepRuntimeStateId,
        pedal_smoother: &ResolvedPedalSmootherTransform,
        value: f32,
        current_range: NumInterval<f32>,
        is_idle_tick: bool,
    ) -> Result<(f32, NumInterval<f32>)> {
        let mut data = self.transform_step_data.borrow_mut();

        let initial_value = current_range.from;
        let prev_out = *data.f32_2.entry(runtime_state_id).or_insert(initial_value);
        let last_target = *data.f32_1.entry(runtime_state_id).or_insert(initial_value);

        let now = Instant::now();
        let dt = if let Some(prev) = data.time1.get(&(runtime_state_id)) {
            (now - *prev).as_secs_f32()
        } else {
            0.0
        };

        let dt_user_input = if let Some(prev) = data.time2.get(&(runtime_state_id)) {
            (now - *prev).as_secs_f32()
        } else {
            0.0
        };

        data.time1.insert(runtime_state_id, now);

        let target = if !is_idle_tick {
            data.f32_1.insert(runtime_state_id, value);
            value
        } else {
            last_target
        };

        let mut final_out = prev_out;
        if is_idle_tick {
            if dt > 0.0 {
                let delta_v = target - prev_out;
                let rate_limit = if delta_v > 0.0 {
                    pedal_smoother.rise_rate
                } else {
                    let fall_gentling_factor = match &pedal_smoother.fall_gentling_factor {
                        Some(ResolvedHoldFactor::Reference {
                            device,
                            control,
                            range,
                        }) => {
                            let control_value =
                                self.joystick_manager.get_control_state(device, control);
                            range.normalize_to_unit(range.try_invert_value(control_value).unwrap())
                        }
                        Some(ResolvedHoldFactor::Value(v)) => *v,
                        None => 1.0,
                    };

                    if let Some(fall_delay) = pedal_smoother.fall_delay {
                        if fall_delay < dt_user_input {
                            pedal_smoother.fall_rate * fall_gentling_factor
                        } else {
                            0.0
                        }
                    } else {
                        pedal_smoother.fall_rate * fall_gentling_factor
                    }
                };
                let max_delta = rate_limit * dt;
                let actual_delta = delta_v.clamp(-max_delta, max_delta);
                final_out = prev_out + actual_delta;
            }

            let smoothing_alpha = pedal_smoother.smoothing_alpha;
            final_out = (smoothing_alpha) * final_out + (1.0 - smoothing_alpha) * prev_out;

            final_out = current_range.clamp(final_out);
            data.f32_2.insert(runtime_state_id, final_out);
        } else {
            data.time2.insert(runtime_state_id, now);
        }

        Ok((final_out, current_range))
    }

    fn mapping_to_string(&self, mapping: &ResolvedMapping) -> String {
        mapping.name.clone().unwrap_or_else(|| {
            format!(
                "{}.{} -> {}.{}",
                mapping.source.device_key,
                mapping.source.control_key,
                mapping.destination.device_key,
                mapping.destination.control_key
            )
        })
    }
}
