use anyhow::{bail, Context, Result};
use log::{debug, info, warn};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::time::Instant;
use tokio::select;
use tokio::time::{interval, Duration, MissedTickBehavior};

use crate::common::NumInterval;
use crate::config::{ConfigManager, ControlReference, ResolvedMapping};
use crate::interpolation::{InterpolationCurve, ValueFilter};
use crate::joystick::VirtualJoystickManager;
use crate::midi::{MidiManager, MidiMessage};
use crate::mouse::{MouseEvent, MouseManager};
use crate::schemas::{HoldFactor, Transformation, TransformationStep};

struct AccumulatingTransformData<'cfg> {
    history: HashMap<*const TransformationStep, VecDeque<f32>>,
    out: HashMap<&'cfg ResolvedMapping, f32>,
    _last_in: HashMap<&'cfg ResolvedMapping, f32>,
    last_time: HashMap<&'cfg ResolvedMapping, Instant>,
    last_time_user_input: HashMap<&'cfg ResolvedMapping, Instant>,
    accumulator: HashMap<&'cfg ResolvedMapping, f32>,
}

impl<'cfg> AccumulatingTransformData<'cfg> {
    fn new() -> Self {
        Self {
            history: HashMap::new(),
            out: HashMap::new(),
            _last_in: HashMap::new(),
            last_time: HashMap::new(),
            last_time_user_input: HashMap::new(),
            accumulator: HashMap::new(),
        }
    }
}

pub(crate) struct MappingEngine<'cfg> {
    config_manager: &'cfg ConfigManager,
    midi_manager: MidiManager,
    mouse_manager: MouseManager,
    joystick_manager: VirtualJoystickManager,
    debug: bool,
    debug_idle_tick: bool,
    update_rate: u32,
    // _latency_mode: String,
    running: bool,

    moving_average_transform_data: RefCell<AccumulatingTransformData<'cfg>>,
    steering_transform_data: RefCell<AccumulatingTransformData<'cfg>>,
    integrator_transform_data: RefCell<AccumulatingTransformData<'cfg>>,
    pedal_smoother_transform_data: RefCell<AccumulatingTransformData<'cfg>>,

    router: HashMap<String, Vec<&'cfg ResolvedMapping>>,
    idle_tick_mappings: Vec<&'cfg ResolvedMapping>,
}

impl<'cfg> MappingEngine<'cfg> {
    pub(crate) fn new(
        config_manager: &'cfg ConfigManager,
        midi_manager: MidiManager,
        mouse_manager: MouseManager,
        joystick_manager: VirtualJoystickManager,
        debug: bool,
        debug_idle_tick: bool,
    ) -> Result<Self> {
        Ok(Self {
            config_manager,
            midi_manager,
            mouse_manager,
            joystick_manager,
            debug,
            debug_idle_tick,
            update_rate: config_manager.get_config().global.update_rate,
            // _latency_mode: "normal".to_string(),
            running: false,
            moving_average_transform_data: AccumulatingTransformData::new().into(),
            steering_transform_data: AccumulatingTransformData::new().into(),
            integrator_transform_data: AccumulatingTransformData::new().into(),
            pedal_smoother_transform_data: AccumulatingTransformData::new().into(),
            router: HashMap::new(),
            idle_tick_mappings: Vec::new(),
        })
    }

    pub(crate) fn set_update_rate(&mut self, rate: u32) {
        self.update_rate = rate.clamp(200, 10000);
    }

    // pub(crate) fn _set_latency_mode(&mut self, mode: &str) {
    //     self._latency_mode = mode.to_string();
    // }

    pub(crate) fn active_mapping_count(&self) -> usize {
        self.router.values().map(|v| v.len()).sum()
    }

    pub(crate) async fn initialize(&mut self) -> Result<()> {
        let config = self.config_manager.get_config();
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
            }
        }

        let mut opened_virtual_joysticks: BTreeSet<String> = BTreeSet::new();
        for vjoy_key in required_dst_device_keys {
            if let Some(vjoy_config) = config.virtual_joysticks.get(&vjoy_key) {
                match self
                    .joystick_manager
                    .create_virtual_joystick(&vjoy_key, vjoy_config)
                {
                    Ok(_) => {
                        info!("Opened Destination Virtual Joystick: {}", vjoy_key);
                        opened_virtual_joysticks.insert(vjoy_key);
                    }
                    Err(e) => {
                        bail!("Failed to open required joystick '{}': {}", vjoy_key, e);
                    }
                }
            } else {
                bail!("Mapping references undefined joystick: {}", vjoy_key);
            }
        }

        let mut runtime_device_name_to_config_device_key: HashMap<String, String> = HashMap::new();

        if let Some(midi_devices) = &config.midi_devices {
            let available_midi = self.midi_manager.enumerate_devices();
            for src_key in &required_src_device_keys {
                if let Some(device_config) = midi_devices.get(src_key) {
                    if let Some(pattern) = &device_config.match_name_regex {
                        let matched = self.midi_manager.match_device(pattern, &available_midi);

                        for device_name in matched {
                            match self.midi_manager.open_device(&device_name) {
                                Ok(_) => {
                                    if self.debug {
                                        info!(
                                            "Opened Source MIDI: {} (for key: {})",
                                            device_name, src_key
                                        );
                                    }
                                    runtime_device_name_to_config_device_key
                                        .insert(device_name, src_key.clone());
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
        }

        if let Some(mouse_devices) = &config.mouse_devices {
            let available_mice = self.mouse_manager.enumerate_devices()?;
            for src_device_key in &required_src_device_keys {
                if let Some(mouse_config) = mouse_devices.get(src_device_key) {
                    if !mouse_config.enabled {
                        continue;
                    }

                    if let Some(pattern) = &mouse_config.match_name_regex {
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

        let joy_stop_result = self
            .joystick_manager
            .stop()
            .context("Failed to stop Joysticks Manager.");
        let midi_stop_result = self
            .midi_manager
            .stop()
            .context("Failed to stop Midi Manager.");
        let mouse_stop_result = self
            .mouse_manager
            .stop()
            .context("Failed to stop Mouse Manager.");

        let errors: Vec<String> = [joy_stop_result, midi_stop_result, mouse_stop_result]
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
        if idle_tick_requirement_info.is_required.is_none() {
            idle_tick_requirement_info.is_required = {
                match &mapping.transformation {
                    Transformation::List(steps) => Some(steps.iter().any(|s| {
                        matches!(
                            s,
                            TransformationStep::Steering { .. }
                                | TransformationStep::PedalSmoother { .. }
                        )
                    })),
                }
            };
        }
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
            .unwrap_or(NumInterval::new(0.0, 0.0));

        let mut current_value = value;
        let mut current_range = src_range;

        if !src_range.contains_inclusive(current_value) {
            warn!(
                "The value (={current_value}) read from device {runtime_input_device_name} \
            is out of configured range ({current_range:?}), clamping it."
            );
            current_value = current_range.clamp(current_value);
        }

        match &mapping.transformation {
            Transformation::List(steps) => {
                for step in steps {
                    (current_value, current_range) = self.apply_transformation_step(
                        mapping,
                        step,
                        current_value,
                        current_range,
                        dst_range,
                        is_idle_tick,
                    )?;
                }
            }
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
        step: &TransformationStep,
        value: f32,
        current_range: NumInterval<f32>,
        dst_range: NumInterval<f32>,
        is_idle_tick: bool,
    ) -> Result<(f32, NumInterval<f32>)> {
        match step {
            TransformationStep::Invert { invert } => {
                Ok(self.apply_invert_transform(invert, value, current_range))
            }
            TransformationStep::Integrate { integrate } => {
                Ok(self.apply_integrate_transform(mapping, integrate, value, current_range)?)
            }
            TransformationStep::Curve { curve } => {
                Ok(self.apply_curve_transform(curve, value, current_range))
            }
            TransformationStep::Clamp { clamp } => {
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
            TransformationStep::Steering { steering } => {
                Ok(self.apply_steering_transform(mapping, steering, value, dst_range)?)
            }
            TransformationStep::PedalSmoother { pedal_smoother } => Ok(self
                .apply_pedal_smoother_transform(
                    mapping,
                    pedal_smoother,
                    value,
                    current_range,
                    is_idle_tick,
                )?),
            TransformationStep::MovingAverage { moving_average } => Ok((
                self.apply_moving_average_transform(step, moving_average, value),
                current_range,
            )),
        }
    }

    fn apply_moving_average_transform(
        &self,
        step: &crate::schemas::TransformationStep,
        moving_average: &crate::schemas::MovingAverage,
        value: f32,
    ) -> f32 {
        let mut data = self.moving_average_transform_data.borrow_mut();
        let history = data
            .history
            .entry(step as *const TransformationStep)
            .or_default();
        ValueFilter::moving_average(value, history, moving_average.samples)
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

    fn apply_curve_transform(
        &self,
        curve: &crate::schemas::Curve,
        value: f32,
        range: NumInterval<f32>,
    ) -> (f32, NumInterval<f32>) {
        (
            range.denormalize_from_unit_domain(
                InterpolationCurve::apply(
                    &curve.curve_type,
                    range.normalize_to_unit_domain(value),
                    curve.parameters.as_ref(),
                ),
                false,
            ),
            range,
        )
    }

    fn apply_integrate_transform(
        &self,
        mapping: &'cfg ResolvedMapping,
        integrate: &crate::schemas::IntegrateTransform,
        delta_value: f32,
        _current_range: NumInterval<f32>,
    ) -> Result<(f32, NumInterval<f32>)> {
        let integration_range = integrate.range.unwrap_or(NumInterval::new(0.0, 750.0));

        let dz_norm = integrate.deadzone_norm.unwrap_or(0.0).max(0.0);
        let alpha = integrate.smoothing_alpha.unwrap_or(0.0).clamp(0.0, 1.0);

        let mut data = self.integrator_transform_data.borrow_mut();

        let mut dx = delta_value;
        if dx.abs() < dz_norm * integration_range.span() {
            dx = 0.0;
        }

        let prev = *data
            .accumulator
            .entry(mapping)
            .or_insert((integration_range.from + integration_range.to) * 0.5);

        let new_pos = integration_range.clamp(prev + dx);

        let prev_out = *data.out.entry(mapping).or_insert(prev);
        let out_val = (1.0 - alpha) * prev_out + alpha * new_pos;

        data.accumulator.insert(mapping, new_pos);
        data.out.insert(mapping, out_val);
        data.last_time.insert(mapping, Instant::now());

        Ok((out_val, integration_range))
    }

    fn apply_steering_transform(
        &self,
        mapping: &'cfg ResolvedMapping,
        steering: &crate::schemas::SteeringTransform,
        value: f32,
        range: NumInterval<f32>,
    ) -> Result<(f32, NumInterval<f32>)> {
        let mut data = self.steering_transform_data.borrow_mut();
        let mut pos_symmetric_norm = *data.accumulator.entry(mapping).or_insert(0.0);

        let now = Instant::now();
        let dt = match data.last_time.get(mapping) {
            Some(prev) => (now - *prev).as_secs_f32(),
            _ => 0.0,
        };
        data.last_time.insert(mapping, now);

        let counts_to_lock = steering.counts_to_lock.max(1.0);
        let mut step = value / (counts_to_lock / 2.0);

        if step != 0.0 {
            if let Some(curve) = &steering.user_input_curve {
                let symmetric_norm_interval = NumInterval::new(-1.0, 1.0);
                let step_normalized_to_unit =
                    symmetric_norm_interval.normalize_to_unit_domain(step);
                // TODO: avoid excessive value translations between domains.
                let post_curve = InterpolationCurve::apply(
                    &curve.curve_type,
                    step_normalized_to_unit,
                    curve.parameters.as_ref(),
                );
                let post_denorm =
                    symmetric_norm_interval.denormalize_from_unit_domain(post_curve, false);
                step = post_denorm;
            }
        }

        pos_symmetric_norm += step;

        let halflife = steering.auto_center_halflife;

        // Calculate hold factor:
        // Returns 0.0 (no hold) to 1.0 (full hold/freeze)
        let hold_factor = self.resolve_and_normalize_hold_factor(&steering.wheel_hold_factor);

        //--------------------------------
        // Force Feedback:
        //--------------------------------
        // Retrieve FF force from the destination joystick
        let ff_force_norm = if let Some(ff_config) = &steering.force_feedback {
            if ff_config.enabled {
                let raw_force = self
                    .joystick_manager
                    .get_ff_constant_force_norm(&mapping.destination.device_key);
                let scaled_force = raw_force * ff_config.constant_force_scale;
                // log::error!("FF: {:?} -> scaled to {:?}", raw_force, scaled_force);
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
            // Apply force feedback as a position offset
            // ff_force is -1.0 to 1.0
            // influence is how much it affects the position per second
            let influence = steering
                .force_feedback
                .as_ref()
                .map(|f| f.constant_force_influence)
                .unwrap_or(0.7);
            // log::error!("FF INFLUENCE: {:?}", influence);
            let ff_position_offset = ff_force_norm * (1.0 - hold_factor) * influence * dt;
            pos_symmetric_norm += ff_position_offset;

            if self.debug && self.debug_idle_tick && ff_force_norm.abs() > 0.1 {
                debug!(
                    "FF active: force={:.3} offset={:.3}",
                    ff_force_norm, ff_position_offset
                );
            }
        }

        //--------------------------------
        // Autocentering:
        //--------------------------------
        if halflife > 0.0 && dt > 0.0 {
            // If FF is dominating (strong force), we might want to reduce auto-centering,
            // but the Python implementation only disables auto-centering if step == 0.0
            // AND (implied) it allows FF to fight centering.
            // Python: `if ff_force < 0.001 and step == 0.0 ...`
            // We replicate that check here:

            if ff_force_norm.abs() < 0.001 && step == 0.0 {
                let k = 1.0 - (2.0_f32).powf(-dt / halflife);
                // If hold_factor is 1.0, term becomes 0.0 -> no centering applied
                let k = k * (1.0 - hold_factor).clamp(0.0, 1.0);
                pos_symmetric_norm += (0.0 - pos_symmetric_norm) * k;
            }
        }

        // After all the increments above we could overflow. Saturating back home!
        pos_symmetric_norm = pos_symmetric_norm.clamp(-1.0, 1.0);

        let midpoint = range.midpoint();
        let half_span = range.span() / 2.0;
        let out_abs_unsmoothed = midpoint + pos_symmetric_norm * half_span;
        let prev_out = *data.out.entry(mapping).or_insert(out_abs_unsmoothed);
        let out_abs = (1.0 - steering.smoothing_alpha) * prev_out
            + steering.smoothing_alpha * out_abs_unsmoothed;
        data.accumulator.insert(mapping, pos_symmetric_norm);
        data.out.insert(mapping, out_abs);
        Ok((out_abs, range))
    }

    fn resolve_and_normalize_hold_factor(&self, hold_factor: &Option<HoldFactor>) -> f32 {
        match hold_factor {
            Some(HoldFactor::Value(v)) => *v,
            Some(HoldFactor::Reference { device, control }) => {
                if let Some(control_range) = self.config_manager.get_control_range(device, control)
                {
                    return NumInterval::new(0.0, 1.0).map_from(
                        self.joystick_manager.get_control_state(device, control) as f32,
                        &control_range.cast::<f32>().unwrap(),
                        false,
                    );
                }
                0.0
            }
            None => 0.0,
        }
    }

    fn resolve_and_normalize_fall_gentling_factor(
        &self,
        gentling_factor: &Option<HoldFactor>,
    ) -> f32 {
        match gentling_factor {
            Some(HoldFactor::Reference { device, control }) => {
                if let Some(control_range) = self.config_manager.get_control_range(device, control)
                {
                    let control_value = self.joystick_manager.get_control_state(device, control);
                    return control_range.normalize_to_unit_domain(
                        control_range.try_invert_value(control_value).unwrap(),
                    );
                }
                1.0
            }
            None => 1.0,
            _ => 1.0,
        }
    }

    fn apply_pedal_smoother_transform(
        &self,
        mapping: &'cfg ResolvedMapping,
        pedal_smoother: &crate::schemas::PedalSmootherTransform,
        value: f32,
        current_range: NumInterval<f32>,
        is_idle_tick: bool,
    ) -> Result<(f32, NumInterval<f32>)> {
        let mut data = self.pedal_smoother_transform_data.borrow_mut();

        let initial_value = current_range.from;
        let prev_out = *data.out.entry(mapping).or_insert(initial_value);
        let last_target = *data.accumulator.entry(mapping).or_insert(initial_value);

        let now = Instant::now();
        let dt = if let Some(prev) = data.last_time.get(mapping) {
            (now - *prev).as_secs_f32()
        } else {
            0.0
        };

        let dt_user_input = if let Some(prev) = data.last_time_user_input.get(mapping) {
            (now - *prev).as_secs_f32()
        } else {
            0.0
        };

        data.last_time.insert(mapping, now);

        let target = if !is_idle_tick {
            data.accumulator.insert(mapping, value);
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
                    let fall_gentling_factor = self.resolve_and_normalize_fall_gentling_factor(
                        &pedal_smoother.fall_gentling_factor,
                    );
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
            data.out.insert(mapping, final_out);
        } else {
            data.last_time_user_input.insert(mapping, now);
        }

        Ok((final_out, current_range))
    }

    // fn mapping_to_string(&self, mapping: &ResolvedMapping) -> String {
    //     mapping.id.clone().unwrap_or_else(|| {
    //         format!(
    //             "{}.{}_to_{}.{}",
    //             mapping.source.device_key,
    //             mapping.source.control_key,
    //             mapping.destination.joystick_key,
    //             mapping.destination.control_key
    //         )
    //     })
    // }
}
