use anyhow::{bail, Context, Result};
use evdev::uinput::{VirtualDevice, VirtualEventStream};
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EvdevEnum, FFEffectCode, InputEvent, KeyCode,
    UinputAbsSetup,
};

use crate::schemas::{ControlType, VirtualJoystick as JoystickConfig};
use atomic_float::AtomicF32;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

type FfIndexT = usize;
const VJ_FF_MAX_EFFECTS: crate::joystick::FfIndexT = 1; // TODO: VjFfIndexT::MAX - 1;

type FfGainT = i16;
type FfLevelT = i16;
enum FfEffect {
    ConstantForce {
        level: crate::joystick::FfLevelT, // NB: simplistic form. Envelop, trigger is skipped as not needed currently.
                                          // trigger: ...
                                          // envelop: ...
                                          // repeats: ...
    },
    #[allow(dead_code)]
    None,
}

#[derive(Default)]
struct FfWorkingState {
    uploaded_effects: HashMap<crate::joystick::FfIndexT, crate::joystick::FfEffect>,
    played_effect: Option<(crate::joystick::FfIndexT, crate::joystick::FfGainT)>,
}

#[derive(Clone)]
struct VjControlInfo {
    control_type: ControlType,
    evdev_code: u16,
}

type FfPlayedSummT = AtomicF32;
pub(crate) struct VirtualJoystick {
    name: String,
    platform_device_api: Arc<Mutex<evdev::uinput::VirtualEventStream>>,
    control_states: HashMap<String, i32>,
    control_info: HashMap<String, VjControlInfo>,
    ff_enabled: bool,
    ff_played_summ_norm: Arc<crate::joystick::FfPlayedSummT>,
    ff_input_join_handle: Option<JoinHandle<()>>,
    ff_input_cancellation_token: Option<CancellationToken>,
    debug: bool,
}

impl VirtualJoystick {
    pub(crate) fn new(name: &str, config: &JoystickConfig, debug: bool) -> Result<Self> {
        let mut evdev_builder = VirtualDevice::builder()
            .context("Failed to create virtual device builder")?
            .name(&config.properties.name)
            .input_id(evdev::InputId::new(
                evdev::BusType::BUS_USB,
                config.properties.vendor_id,
                config.properties.product_id,
                config.properties.version,
            ));

        let mut keys = AttributeSet::<KeyCode>::new();
        let mut abs_axes = AttributeSet::<AbsoluteAxisCode>::new();
        let mut control_states = HashMap::new();
        let mut control_info = HashMap::new();

        for (control_name, control_config) in &config.controls {
            match control_config.control_type {
                ControlType::Button => {
                    let key_code = if let Ok(parsed_code) = control_config.code.parse::<u16>() {
                        parsed_code
                    } else {
                        match control_config.code.as_str() {
                            "BTN_A" => KeyCode::BTN_SOUTH.code(),
                            "BTN_B" => KeyCode::BTN_EAST.code(),
                            "BTN_X" => KeyCode::BTN_NORTH.code(),
                            "BTN_Y" => KeyCode::BTN_WEST.code(),
                            "BTN_TL" => KeyCode::BTN_TL.code(),
                            "BTN_TR" => KeyCode::BTN_TR.code(),
                            "BTN_SELECT" => KeyCode::BTN_SELECT.code(),
                            "BTN_START" => KeyCode::BTN_START.code(),
                            _ => {
                                debug!("Unknown button code: {}", control_config.code);
                                continue;
                            }
                        }
                    };

                    keys.insert(KeyCode::new(key_code));
                    control_states.insert(control_name.clone(), control_config.initial_value);
                    control_info.insert(
                        control_name.clone(),
                        VjControlInfo {
                            control_type: control_config.control_type.clone(),
                            evdev_code: key_code,
                        },
                    );
                }
                ControlType::Axis | ControlType::Hat => {
                    let (axis_type, axis_code) = match control_config.code.as_str() {
                        "ABS_X" => (Some(AbsoluteAxisCode::ABS_X), AbsoluteAxisCode::ABS_X.0),
                        "ABS_Y" => (Some(AbsoluteAxisCode::ABS_Y), AbsoluteAxisCode::ABS_Y.0),
                        "ABS_Z" => (Some(AbsoluteAxisCode::ABS_Z), AbsoluteAxisCode::ABS_Z.0),
                        "ABS_RX" => (Some(AbsoluteAxisCode::ABS_RX), AbsoluteAxisCode::ABS_RX.0),
                        "ABS_RY" => (Some(AbsoluteAxisCode::ABS_RY), AbsoluteAxisCode::ABS_RY.0),
                        "ABS_RZ" => (Some(AbsoluteAxisCode::ABS_RZ), AbsoluteAxisCode::ABS_RZ.0),
                        "ABS_WHEEL" => (
                            Some(AbsoluteAxisCode::ABS_WHEEL),
                            AbsoluteAxisCode::ABS_WHEEL.0,
                        ),
                        "ABS_HAT0X" => (
                            Some(AbsoluteAxisCode::ABS_HAT0X),
                            AbsoluteAxisCode::ABS_HAT0X.0,
                        ),
                        "ABS_HAT0Y" => (
                            Some(AbsoluteAxisCode::ABS_HAT0Y),
                            AbsoluteAxisCode::ABS_HAT0Y.0,
                        ),
                        _ => (None, 0),
                    };

                    if let Some(axis) = axis_type {
                        let abs_info = AbsInfo::new(
                            control_config.initial_value,
                            control_config.range.from,
                            control_config.range.to,
                            control_config
                                .properties
                                .as_ref()
                                .map(|p| p.fuzz as i32)
                                .unwrap_or(0),
                            control_config
                                .properties
                                .as_ref()
                                .map(|p| p.flat as i32)
                                .unwrap_or(0),
                            control_config
                                .properties
                                .as_ref()
                                .map(|p| p.resolution as i32)
                                .unwrap_or(1),
                        );

                        let abs_setup = UinputAbsSetup::new(axis, abs_info);
                        evdev_builder = evdev_builder.with_absolute_axis(&abs_setup)?;
                        abs_axes.insert(axis);
                        control_states.insert(control_name.clone(), control_config.initial_value);
                        control_info.insert(
                            control_name.clone(),
                            VjControlInfo {
                                control_type: control_config.control_type.clone(),
                                evdev_code: axis_code,
                            },
                        );
                    }
                }
            }
        }

        evdev_builder = evdev_builder.with_keys(&keys)?;

        if config.is_ff_enabled() {
            info!("Enabling Force Feedback for '{name}'");
            let mut ff_effects = AttributeSet::<FFEffectCode>::new();
            ff_effects.insert(FFEffectCode::FF_CONSTANT);
            evdev_builder = evdev_builder
                .with_ff(&ff_effects)
                .context("Failed to to create virtual joystick with FF effects {ff_effects:?}")?
                .with_ff_effects_max(VJ_FF_MAX_EFFECTS as u32);
        }

        let mut vj = Self {
            name: name.to_string(),
            platform_device_api: Arc::new(Mutex::new(
                evdev_builder
                    .build()
                    .context("Evdev builder failed to build a virtual joystick device.")?
                    .into_event_stream()
                    .context(
                        "Failed to convert evdev virtual joystick device \
                    to event non-blocking stream.",
                    )?,
            )),
            control_states,
            control_info,
            ff_enabled: config.is_ff_enabled(),
            ff_played_summ_norm: Arc::new(0.0.into()),
            ff_input_join_handle: None,
            ff_input_cancellation_token: None,
            debug,
        };

        if config.is_ff_enabled() {
            let platform_device_api = vj.platform_device_api.clone();
            let virtual_joystick_name = vj.name.clone();
            let ff_played = vj.ff_played_summ_norm.clone();

            let cancellation_token = CancellationToken::new();
            vj.ff_input_cancellation_token = Some(cancellation_token.clone());

            vj.ff_input_join_handle = Some(tokio::task::spawn_blocking(move || {
                Self::ff_consumer_thread(
                    ff_played,
                    platform_device_api,
                    cancellation_token,
                    virtual_joystick_name,
                    debug.clone(),
                )
            }));
        }

        if debug {
            info!(
                "Created virtual joystick: {} ({}){}",
                config.properties.name,
                name,
                if config.is_ff_enabled() {
                    ", force feedback ENABLED."
                } else {
                    "."
                }
            );
        }

        Ok(vj)
    }

    // TODO: many parallel effects if other types supported.
    // TODO: support external gain control, Gain is u16 (0 to 65535, where 65535 = 100%)
    //         const GAIN_TOTAL_RANGE: u32 = u16::MAX as u32 + 1;
    //         let normalized_gain = gain as f32 / GAIN_TOTAL_RANGE as f32;
    // TODO: Proper sampling during play.
    fn ff_consumer_thread(
        ff_played_summ: Arc<AtomicF32>,
        platform_device_api: Arc<Mutex<VirtualEventStream>>,
        stop_token: CancellationToken,
        virtual_joystick_name: String,
        debug: bool,
    ) {
        let mut ff_working_state = crate::joystick::FfWorkingState::default();
        let mut collected_events: Vec<InputEvent>;
        loop {
            if stop_token.is_cancelled() {
                log::info!(
                    "Joystick {virtual_joystick_name} FFB consumer threadq \
                received request to stop. Thread run finished."
                );
                return;
            }

            let mut platform_device_api = platform_device_api.lock().unwrap();

            // NB: it is O_NOBLOCKING in device stream mode, so we can do it.
            if let Ok(events) = platform_device_api.device_mut().fetch_events() {
                collected_events = events.collect();
            } else {
                continue;
            }

            for event in collected_events {
                match event.destructure() {
                    evdev::EventSummary::ForceFeedback(ffevent, ffeffect_code, i32val) => {
                        if debug {
                            log::debug!("{:?}", ffevent);
                        }
                        match evdev::FFStatusCode(i32val as u16) {
                            evdev::FFStatusCode::FF_STATUS_PLAYING => {
                                let effect_index =
                                    ffeffect_code.to_index() as crate::joystick::FfIndexT;
                                ff_working_state.played_effect =
                                    Some((effect_index, i32val.try_into().unwrap()));
                            }
                            evdev::FFStatusCode::FF_STATUS_STOPPED => {
                                ff_working_state.played_effect = None;
                            }
                            _ => {
                                panic! {""};
                            }
                        }
                    }
                    evdev::EventSummary::ForceFeedbackStatus(
                        ffstatus_event,
                        _ffstatus_code,
                        _i32val,
                    ) => {
                        if debug {
                            log::debug!("{:?}", ffstatus_event);
                        }
                    }
                    evdev::EventSummary::UInput(uinput_event, uinput_code, _i32val) => {
                        if debug {
                            log::debug!("{:?}", uinput_event);
                        }
                        match uinput_code {
                            evdev::UInputCode::UI_FF_UPLOAD => {
                                let mut eff = platform_device_api
                                    .device_mut()
                                    .process_ff_upload(uinput_event)
                                    .unwrap();

                                // TODO: support more than 1.
                                let effect_id: crate::joystick::FfIndexT = 0;
                                eff.set_effect_id(effect_id as i16);

                                if let evdev::FFEffectKind::Constant { level, envelope: _ } =
                                    eff.effect().kind
                                {
                                    ff_working_state.uploaded_effects.insert(effect_id, {
                                        FfEffect::ConstantForce { level: level }
                                    });
                                    // ff_state.played_effect = Some((0,30000));
                                } else {
                                    if debug {
                                        log::debug!(
                                            "Effect upload request for {:?} ignored: \
                                        only Constant force effect supported.",
                                            eff.effect()
                                        );
                                    }
                                    eff.set_retval(-1); // Ignoring other effects.
                                }
                                // log::error!("Uploading {:?}", eff.retval());
                            }
                            evdev::UInputCode::UI_FF_ERASE => {
                                let eff = platform_device_api
                                    .device_mut()
                                    .process_ff_erase(uinput_event)
                                    .unwrap();
                                if debug {
                                    log::debug!("Erasing uploaded effect id {}", eff.effect_id());
                                }
                                let effect_id_to_erase =
                                    eff.effect_id() as crate::joystick::FfIndexT;
                                ff_working_state
                                    .uploaded_effects
                                    .remove(&effect_id_to_erase);
                            }
                            _ => {}
                        }
                    }
                    event => {
                        log::error!("Some other event from JS: {:?}", event);
                    }
                }
            }

            // -----------------------------------------------------
            // Player phase.
            // TODO: it's very simplified now, single effect played, no envelop application.
            // -----------------------------------------------------
            if let Some((effect_id, _count)) = ff_working_state.played_effect {
                if let Some(crate::joystick::FfEffect::ConstantForce { level }) =
                    ff_working_state.uploaded_effects.get(&effect_id)
                {
                    const LEVEL_SYMMETRIC_HALF_RANGE: i16 = i16::MAX; // level is i16 (-32768 to 32767)
                    let normalized_level = *level as f32 / LEVEL_SYMMETRIC_HALF_RANGE as f32;

                    ff_played_summ.store(normalized_level, Ordering::Relaxed);
                } else {
                    ff_played_summ.store(0.0, Ordering::Relaxed);
                }
            } else {
                ff_played_summ.store(0.0, Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn set_control_value(
        &mut self,
        control_name: &str,
        value: f32,
        silent: bool,
    ) -> Result<()> {
        if let Some(current_value) = self.control_states.get_mut(control_name) {
            if let Some(info) = self.control_info.get(control_name) {
                match info.control_type {
                    ControlType::Button => {
                        // We may map arbitrary floating point values to button and we count any
                        // non-zero one as "button on", that is value 1.
                        *current_value = if value != 0. { 1 } else { 0 };
                        let event = InputEvent::new(
                            evdev::EventType::KEY.0,
                            info.evdev_code,
                            *current_value,
                        );
                        self.platform_device_api
                            .lock()
                            .unwrap()
                            .device_mut()
                            .emit(&[event])
                            .context("Failed to emit button event")?;
                    }
                    ControlType::Axis | ControlType::Hat => {
                        *current_value = value as i32;
                        let event = InputEvent::new(
                            evdev::EventType::ABSOLUTE.0,
                            info.evdev_code,
                            *current_value,
                        );
                        self.platform_device_api
                            .lock()
                            .unwrap()
                            .device_mut()
                            .emit(&[event])
                            .context("Failed to emit axis event")?;
                    }
                }
            }

            if self.debug && !silent {
                debug!("[{}][{}] = {}", self.name, control_name, value);
            }

            Ok(())
        } else {
            bail!(
                "Control '{}' not found in joystick '{}'",
                control_name,
                self.name
            )
        }
    }

    pub(crate) fn get_control_state(&self, control_name: &str) -> i32 {
        self.control_states.get(control_name).copied().unwrap_or(0)
    }

    // Get the current constant force feedback level.
    pub(crate) fn get_ff_played_summ_norm(&self) -> f32 {
        if !self.ff_enabled {
            return 0.0;
        }
        return self.ff_played_summ_norm.load(Ordering::Relaxed);
    }
}

pub(crate) struct VirtualJoystickManager {
    debug: bool,
    #[allow(dead_code)]
    debug_ff: bool,
    joysticks: Arc<Mutex<HashMap<String, VirtualJoystick>>>,
}

impl VirtualJoystickManager {
    pub(crate) fn new(debug: bool, debug_ff: bool) -> Result<Self> {
        Ok(Self {
            debug,
            debug_ff,
            joysticks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub(crate) fn create_virtual_joystick(
        &self,
        name: &str,
        config: &JoystickConfig,
    ) -> Result<()> {
        let joystick = VirtualJoystick::new(name, config, self.debug)?;
        self.joysticks
            .lock()
            .unwrap()
            .insert(name.to_string(), joystick);
        Ok(())
    }

    pub(crate) fn set_control_value(
        &self,
        joystick_name: &str,
        control_name: &str,
        value: f32,
        silent: bool,
    ) -> Result<()> {
        let mut joysticks = self.joysticks.lock().unwrap();
        if let Some(joystick) = joysticks.get_mut(joystick_name) {
            joystick.set_control_value(control_name, value, silent)
        } else {
            if self.debug {
                warn!("Joystick '{}' not found", joystick_name);
            }
            Ok(())
        }
    }

    pub(crate) fn get_control_state(&self, joystick_name: &str, control_name: &str) -> i32 {
        let joysticks = self.joysticks.lock().unwrap();
        if let Some(joystick) = joysticks.get(joystick_name) {
            joystick.get_control_state(control_name)
        } else {
            0
        }
    }

    pub(crate) fn get_ff_constant_force_norm(&self, joystick_name: &str) -> f32 {
        let joysticks = self.joysticks.lock().unwrap();
        if let Some(joystick) = joysticks.get(joystick_name) {
            joystick.get_ff_played_summ_norm()
        } else {
            0.0
        }
    }

    pub(crate) fn stop(&self) -> Result<()> {
        let mut joysticks = self.joysticks.lock().unwrap();
        for joystick in joysticks.iter() {
            if joystick.1.ff_enabled {
                if let (Some(token), Some(handle)) = (
                    &joystick.1.ff_input_cancellation_token,
                    &joystick.1.ff_input_join_handle,
                ) {
                    if self.debug {
                        log::debug!(
                            "Cancelling joystick {} FFB consumer thread",
                            joystick.1.name
                        );
                    }
                    handle.abort(); // NB: For a blocking task (our case) this is most of the time a no-op.
                                    // NB: But can prevent task from starting if was not started.
                    token.cancel();
                } else {
                    unreachable!(
                        "FFB flag is set on virtual joystick, however no associated \
                    cancellation token or task handle found."
                    );
                }
            }
        }
        joysticks.clear();
        if self.debug {
            info!("All virtual joysticks closed.");
        }
        Ok(())
    }
}
