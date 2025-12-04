use anyhow::{bail, Context, Result};
use evdev::uinput::{VirtualDevice, VirtualEventStream};
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EvdevEnum, FFEffectCode, InputEvent, KeyCode,
    UinputAbsSetup,
};
use num_traits::Zero;

use crate::schemas::ResolvedVirtualJoystick as JoystickConfig;
use atomic_float::AtomicF32;
use log::{debug, info, warn};
use std::cell::RefCell;
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

type FfPlayedSummT = AtomicF32;
pub(crate) struct VirtualJoystick {
    config_key: String,
    name: String,
    platform_device_api: Arc<Mutex<evdev::uinput::VirtualEventStream>>,
    control_states: HashMap<String, i32>,
    control_info: HashMap<String, crate::common::ControlType>,
    ff_enabled: bool,
    ff_played_summ_norm: Arc<crate::joystick::FfPlayedSummT>,
    ff_input_join_handle: Option<JoinHandle<()>>,
    ff_input_cancellation_token: Option<CancellationToken>,
    debug: bool,
    is_persistent: bool,
}

impl VirtualJoystick {
    pub(crate) fn new(
        joystick_config_key: &str,
        config: &crate::schemas::ResolvedVirtualJoystick,
        debug: bool,
        debug_ff: bool,
        is_persistent: bool,
    ) -> Result<Self> {
        let mut evdev_builder = VirtualDevice::builder()
            .context("Failed to create virtual device builder")?
            .name(&config.name)
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
            if control_config.r#type.is_button() {
                let evdev_key_code_int = control_config.r#type.into();
                keys.insert(KeyCode::new(evdev_key_code_int));
                control_states.insert(control_name.clone(), control_config.initial_value);
                control_info.insert(control_name.clone(), control_config.r#type);
            } else if control_config.r#type.is_absolute() {
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

                let evdev_axis_code_int = control_config.r#type.into();
                let evdev_axis_code = evdev::AbsoluteAxisCode(evdev_axis_code_int);
                let abs_setup = UinputAbsSetup::new(evdev_axis_code, abs_info);
                evdev_builder = evdev_builder
                    .with_absolute_axis(&abs_setup)
                    .context("Failed to setup joystick with absolute axis")?;
                abs_axes.insert(evdev_axis_code);
                control_states.insert(control_name.clone(), control_config.initial_value);
                control_info.insert(control_name.clone(), control_config.r#type);
            }
        }

        evdev_builder = evdev_builder.with_keys(&keys)?;

        if config.is_ff_enabled() {
            info!("Enabling Force Feedback for '{joystick_config_key}'");
            let mut ff_effects = AttributeSet::<FFEffectCode>::new();
            ff_effects.insert(FFEffectCode::FF_CONSTANT);
            evdev_builder = evdev_builder
                .with_ff(&ff_effects)
                .context("Failed to to create virtual joystick with FF effects {ff_effects:?}")?
                .with_ff_effects_max(VJ_FF_MAX_EFFECTS as u32);
        }

        let mut vj = Self {
            name: config.name.clone(),
            config_key: joystick_config_key.to_string(),
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
            is_persistent,
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
                    debug_ff,
                )
            }));
        }

        info!(
            "Created virtual joystick: {} ({}){}",
            config.name,
            joystick_config_key,
            if config.is_ff_enabled() {
                ", force feedback ENABLED."
            } else {
                "."
            }
        );

        Ok(vj)
    }

    fn stop_ff_thread(&self) {
        if self.ff_enabled {
            if let (Some(token), Some(handle)) = (
                &self.ff_input_cancellation_token,
                &self.ff_input_join_handle,
            ) {
                if self.debug {
                    log::debug!("Cancelling joystick {} FFB consumer thread", self.name);
                }
                handle.abort();
                token.cancel();
            } else {
                unreachable!("FFB flag set but no handle found for {}", self.name);
            }
        }
    }

    //-------------
    // TODO: many parallel effects if other types supported.
    //-------------
    // TODO: Proper sampling during play accounting for envelope (and maybe trigger).
    //-------------
    // TODO: support events beyond contant force.
    //-------------
    // TODO: support external gain control, Gain is u16 (0 to 65535, where 65535 = 100%)
    //         const GAIN_TOTAL_RANGE: u32 = u16::MAX as u32 + 1;
    //         let normalized_gain = gain as f32 / GAIN_TOTAL_RANGE as f32;
    //-------------
    // TODO: account for update_rate in Hz. Presently we don't limit it, only sleeping if no events.
    //-------------
    fn ff_consumer_thread(
        ff_played_summ_norm: Arc<AtomicF32>,
        platform_device_api: Arc<Mutex<VirtualEventStream>>,
        stop_token: CancellationToken,
        virtual_joystick_name: String,
        debug_ff: bool,
    ) {
        let mut collected_events: Vec<InputEvent>;
        let ff_working_state = RefCell::new(crate::joystick::FfWorkingState::default());
        let ff_value_halfspan_f32 =
            (crate::common::NumInterval::new(FfLevelT::MIN, FfLevelT::MAX).span_w() as f32) / 2.0;
        // -----------------------------------------------------
        // FFB player routine.
        // -----------------------------------------------------
        let play = || {
            let ff_working_state = ff_working_state.borrow();
            let mut summ: f32 = 0.0;
            if let Some((effect_id, _count)) = ff_working_state.played_effect {
                if let Some(crate::joystick::FfEffect::ConstantForce { level }) =
                    ff_working_state.uploaded_effects.get(&effect_id)
                {
                    summ += *level as f32 / ff_value_halfspan_f32;
                }
            }
            ff_played_summ_norm.store(summ, Ordering::Relaxed);
        };

        let mut sleep_millis = 0;

        loop {
            if stop_token.is_cancelled() {
                log::info!(
                    "Joystick {virtual_joystick_name} FFB consumer thread \
                received request to stop. Thread run finished."
                );
                return;
            }

            std::thread::sleep(std::time::Duration::from_millis(sleep_millis));

            //------------------------------------------------------------------
            play();

            // NB: file descriptor is set to O_NONBLOCK in device stream mode.
            // NB: so we have forward progress here.
            // NB: if in sync mode, this routine would be split in two parallel routines:
            // NB: one for events consumption and another for events play.
            // NB: Current implementation is sufficient.
            if let Ok(events) = platform_device_api
                .lock()
                .expect(
                    "Hold on to your towels, couldn't lock the mutex \
                on joystick device due to cosmic rays influence!",
                )
                .device_mut()
                .fetch_events()
            {
                collected_events = events.collect();
                sleep_millis = 0;
            } else {
                if sleep_millis < 10 {
                    sleep_millis += 1;
                }
                continue;
            }
            //------------------------------------------------------------------

            let mut ff_working_state = ff_working_state.borrow_mut();

            for event in collected_events {
                match event.destructure() {
                    evdev::EventSummary::ForceFeedback(ffevent, ffeffect_code, i32val) => {
                        if debug_ff {
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
                                log::warn!("Unhandled FF status for event {event:?}");
                            }
                        }
                    }
                    evdev::EventSummary::ForceFeedbackStatus(
                        ffstatus_event,
                        _ffstatus_code,
                        _i32val,
                    ) => {
                        if debug_ff {
                            log::debug!("{:?}", ffstatus_event);
                        }
                    }
                    evdev::EventSummary::UInput(uinput_event, uinput_code, _i32val) => {
                        if debug_ff {
                            log::debug!("{:?}", uinput_event);
                        }
                        match uinput_code {
                            evdev::UInputCode::UI_FF_UPLOAD => {
                                let mut platform_device_api_ = platform_device_api.lock().unwrap();
                                let mut eff = platform_device_api_
                                    .device_mut()
                                    .process_ff_upload(uinput_event)
                                    .unwrap();

                                // TODO: support more than 1.
                                let effect_id: crate::joystick::FfIndexT = 0;
                                eff.set_effect_id(effect_id as i16);

                                if let evdev::FFEffectKind::Constant { level, envelope: _ } =
                                    eff.effect().kind
                                {
                                    ff_working_state
                                        .uploaded_effects
                                        .insert(effect_id, FfEffect::ConstantForce { level });
                                    // ff_state.played_effect = Some((0,30000));
                                } else {
                                    if debug_ff {
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
                                let mut platform_device_api_ = platform_device_api.lock().unwrap();
                                let eff = platform_device_api_
                                    .device_mut()
                                    .process_ff_erase(uinput_event)
                                    .unwrap();
                                if debug_ff {
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
                        if debug_ff {
                            log::error!("Unhandled FF event: {:?}", event);
                        }
                    }
                }
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
            if let Some(control_type) = self.control_info.get(control_name) {
                if control_type.is_button() {
                    // We may map arbitrary floating point values to button and we count any
                    // non-zero one as "button on", that is value 1.
                    *current_value = (!value.is_zero()).into();
                    let event = InputEvent::new(
                        evdev::EventType::KEY.0,
                        (*control_type).into(),
                        *current_value,
                    );
                    self.platform_device_api
                        .lock()
                        .unwrap()
                        .device_mut()
                        .emit(&[event])
                        .context("Failed to emit button event")?;
                } else if control_type.is_absolute() {
                    *current_value = value as i32;
                    let event = InputEvent::new(
                        evdev::EventType::ABSOLUTE.0,
                        (*control_type).into(),
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
        self.ff_played_summ_norm.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
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
        is_persistent: bool,
    ) -> Result<()> {
        let mut joysticks = self.joysticks.lock().unwrap();

        if let Some(existing) = joysticks.get_mut(name) {
            if existing.is_persistent != is_persistent {
                if self.debug {
                    debug!(
                        "Updating persistence for joystick '{}': {} -> {}",
                        name, existing.is_persistent, is_persistent
                    );
                }
                existing.is_persistent = is_persistent;
            }
            if self.debug {
                debug!(
                    "Virtual joystick '{}' already exists, skipping creation.",
                    name
                );
            }
            return Ok(());
        }

        let joystick =
            VirtualJoystick::new(name, config, self.debug, self.debug_ff, is_persistent)?;
        joysticks.insert(name.to_string(), joystick);
        Ok(())
    }

    pub(crate) fn destroy_virtual_joystick_if_exists(&self, joystick_config_key: &str) {
        let mut joysticks = self.joysticks.lock().unwrap();
        joysticks.retain(|_, joystick| {
            if joystick_config_key != joystick.config_key {
                return true;
            }
            joystick.stop_ff_thread();
            false
        });
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

    pub(crate) fn stop(&self, full_shutdown: bool) -> Result<()> {
        let mut joysticks = self.joysticks.lock().unwrap();

        // NB: On not a full_shutdown (when hot-reloading due to config changes),
        // NB: we keep persistent joysticks.
        joysticks.retain(|name, joystick| {
            if !full_shutdown && joystick.is_persistent {
                if self.debug {
                    info!("Keeping persistent joystick: {}", name);
                }
                return true;
            }

            joystick.stop_ff_thread();
            false // Removing the joystick.
        });

        if full_shutdown && self.debug {
            info!("All virtual joysticks closed.");
        }

        Ok(())
    }
}
