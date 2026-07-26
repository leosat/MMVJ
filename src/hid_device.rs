use crate::base_num::BaseAtomicT;
use crate::base_num::BaseNumT;
use crate::debug::DebugLevel;
use crate::hid_manager::WithDeviceClassification;
use crate::hid_owned_and_ffb::X_AXIS_IDX;
use crate::hid_owned_and_ffb::Y_AXIS_IDX;
use crate::interner::intern_str;
use crate::mapped_controls::MappedCtls;
use crate::mapped_device::MappedDevice;
use crate::mapped_device::MappedDeviceEvent;
use crate::mapped_device::MappedEvents;
use crate::mapped_device::MappedHidEvent;
use crate::num_interval::NumInterval;
use crate::num_interval::ZERO_INTERVAL;
use crate::schemas_common::ObjId;
use crate::schemas_hid::HIDDeviceForceFeedbackCfg;
use crate::schemas_hid::HidControlMatcherCfg;
use crate::schemas_hid::HidDeviceBusSpecCfg;
use crate::schemas_hid::HidDeviceBusType;
use crate::schemas_hid::HidDeviceCfg;
use crate::schemas_hid::HidDeviceClassificationCfg;
use crate::schemas_hid::HidFfEffect;
use crate::schemas_hid::HidVirtualParamsCfg;
use anyhow::Context;
use crossbeam_utils::CachePadded;
use enumflags2::BitFlags;
use enumflags2::bitflags;
use evdev::AbsInfo;
use evdev::AttributeSet;
use evdev::Device;
use evdev::FFEffectCode;
use evdev::InputEvent;
use evdev::KeyCode;
use evdev::RelativeAxisCode;
use evdev::SynchronizationCode;
use evdev::SynchronizationEvent;
use evdev::UinputAbsSetup;
use evdev::uinput::VirtualDevice;
use nix::libc::UINPUT_MAX_NAME_SIZE;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result::Ok;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use strum::IntoEnumIterator;
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::future::FutureExt;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub(crate) enum DeviceThreadCmd {
    SetExternalNotification(tokio::sync::mpsc::UnboundedSender<MappedDeviceEvent>),
    SetControlValue(MappedCtls, BaseNumT),
}

//----------------------------------------------------------
#[bitflags]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) enum HidDeviceKind {
    Keyboard = 1,
    Joystick = 1 << 1,
    Gamepad = 1 << 2,
    Mouse = 1 << 3,
    MiscMappable = 1 << 4,
    Misc = 1 << 5,
    Virtual = 1 << 6,
}

#[derive(Debug)]
#[allow(unused)]
pub(crate) struct HidVirtualDeviceCreationSpec {
    pub(crate) device_kind: HidDeviceKind,
    pub(crate) cfg_spec: HidDeviceCfg,
    pub(crate) debug: DebugLevel,
    pub(crate) debug_ff: bool,
    pub(crate) is_persistent: bool,
}

//----------------------------------------------------------

// https://manpages.debian.org/testing/joystick/evdev-joystick.1.en.html
// joysticks are expected to produce values between -32767 and 32767 for axes, with 0 meaning the joystick is centred
#[allow(dead_code)]
pub(crate) const HID_AXIS_MAX_INTERVAL: NumInterval<BaseNumT> = NumInterval::<BaseNumT> {
    from: i16::MIN as BaseNumT,
    to: i16::MAX as BaseNumT,
};

#[allow(dead_code)]
pub(crate) const HID_AXIS_MAX_RANGE: std::ops::RangeInclusive<BaseNumT> = HID_AXIS_MAX_INTERVAL.make_range_inclusive();

pub(crate) const _MOUSE_LOWRES_CONTROL_INTERVAL: NumInterval<BaseNumT> = NumInterval::<BaseNumT> {
    from: -127.0,
    to: 127.0,
};

pub(crate) const _MOUSE_LOWRES_CONTROL_RANGE: std::ops::RangeInclusive<BaseNumT> =
    _MOUSE_LOWRES_CONTROL_INTERVAL.make_range_inclusive();

pub(crate) const _MOUSE_HIRES_CONTROL_INTERVAL: NumInterval<BaseNumT> = NumInterval::<BaseNumT> {
    from: -360.0,
    to: 360.0,
};

pub(crate) const _MOUSE_HIRES_CONTROL_RANGE: std::ops::RangeInclusive<BaseNumT> =
    _MOUSE_HIRES_CONTROL_INTERVAL.make_range_inclusive();

#[derive(Debug)]
pub(crate) struct OwnedVirtualHIDDeviceThreadIO {
    pub(crate) force_sum: Arc<[CachePadded<BaseAtomicT>; 2]>,
    pub(crate) axis_pos_symm_norm: [CachePadded<BaseAtomicT>; 2],
}

impl OwnedVirtualHIDDeviceThreadIO {
    fn new(ff_cfg: Option<&HIDDeviceForceFeedbackCfg>) -> Self {
        Self {
            force_sum: if let Some(ff) = &ff_cfg {
                {
                    ff.state_xy
                        .iter()
                        .for_each(|v| v.store(0.0, std::sync::atomic::Ordering::Relaxed));
                    ff.state_xy.clone()
                }
            } else {
                Arc::default()
            },
            axis_pos_symm_norm: [CachePadded::default(), CachePadded::default()],
        }
    }
}

pub(crate) type DeviceControlStates = [CachePadded<BaseAtomicT>; MappedCtls::Unhandled as usize];
// Vec<CachePadded<AtomicF32>>;
pub(crate) type DeviceComm = (
    tokio::sync::mpsc::UnboundedReceiver<DeviceThreadCmd>,
    tokio::sync::mpsc::UnboundedSender<DeviceThreadCmd>,
);

// -----------------------------------------------------

impl WithDeviceClassification for HidDevice {
    fn get_classification(&self) -> BitFlags<HidDeviceKind> {
        self.classification
    }

    fn is_a_joystick(&self) -> bool {
        self.classification.is_a_joystick()
    }

    fn is_a_keyboard(&self) -> bool {
        self.classification.is_a_keyboard()
    }

    fn is_a_gamepad(&self) -> bool {
        self.classification.is_a_gamepad()
    }

    fn is_a_mouse(&self) -> bool {
        self.classification.is_a_mouse()
    }

    fn is_a_virtual(&self) -> bool {
        self.owned_virtual_device_thread_io().is_some()
    }

    fn is_a_misc_mappable(&self) -> bool {
        self.classification.is_a_misc_mappable()
    }

    fn update_classification(&mut self) { /* NOP */
    }
}

pub(crate) fn sanitize_hid_name(name: &str) -> &str {
    let max_bytes = UINPUT_MAX_NAME_SIZE - 2;

    if name.len() <= max_bytes {
        return name;
    }

    if let Some(s) = name.get(..max_bytes) {
        s
    } else {
        let mut end = max_bytes;
        while end > 0 && !name.is_char_boundary(end) {
            end -= 1;
        }
        &name[..end]
    }
}

// -----------------------------------------------------
#[derive(Debug)]
pub(crate) struct HidDevice {
    id: ObjId,
    is_owned_virtual_device: bool,
    owned_virtual_device: Option<VirtualDevice>,
    owned_virtual_device_cmd: Option<tokio::sync::mpsc::UnboundedSender<DeviceThreadCmd>>,
    cfg_key: String,
    classification: BitFlags<HidDeviceKind>,
    name: String,
    _client_side_path: PathBuf,
    client_side_thread_rx_tx: DeviceComm,
    events_listener: Option<tokio::sync::mpsc::UnboundedSender<MappedDeviceEvent>>,
    client_side_thread_cancellation: CancellationToken,
    ctl_states: Arc<DeviceControlStates>,
    is_owned_virtual_device_persistent: bool,
    //-------------------------
    ff_enabled: bool,
    owned_virtual_device_thread_io: Option<Arc<OwnedVirtualHIDDeviceThreadIO>>,
    owned_virtual_device_thread_cancellation: Option<CancellationToken>,
    ff_is_a_condition_effect_enabled: bool,
}

impl HidDevice {
    pub(crate) fn create_virtual_device(
        cfg_key: &str,
        creation_spec: HidVirtualDeviceCreationSpec,
        debug_creation: bool,
    ) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        assert!(
            creation_spec.cfg_spec.is_a_virtual(),
            "Attempting to create virtual device while specifying spec for a device matcher: {creation_spec:#?}"
        );

        let mut evdev_builder = VirtualDevice::builder().context("Failed to create virtual device builder")?;

        evdev_builder = creation_spec
            .cfg_spec
            .virtual_device_bus_info_ref()
            .or(Some(&HidDeviceBusSpecCfg::default()))
            .map(|businfo| {
                evdev_builder = evdev_builder.input_id(evdev::InputId::new(
                    match businfo.r#type {
                        HidDeviceBusType::Virtual => evdev::BusType::BUS_VIRTUAL,
                        HidDeviceBusType::Usb => evdev::BusType::BUS_USB,
                        HidDeviceBusType::IsaPnp => evdev::BusType::BUS_ISAPNP,
                        HidDeviceBusType::Isa => evdev::BusType::BUS_ISA,
                        HidDeviceBusType::Gameport => evdev::BusType::BUS_GAMEPORT,
                    },
                    businfo.vendor_id,
                    businfo.product_id,
                    businfo.version,
                ));
                evdev_builder
            })
            .unwrap();

        let device_name = sanitize_hid_name(
            if let Some(name) = creation_spec.cfg_spec.virtual_device_name_ref()
                && !name.is_empty()
            {
                name
            } else {
                log::warn!("Virtual HID name is empty for config key {cfg_key}, using config key for it");
                cfg_key
            },
        );

        evdev_builder = evdev_builder.name(&device_name);

        let mut keys = AttributeSet::<KeyCode>::new();
        let mut relative_axis = AttributeSet::<RelativeAxisCode>::new();
        let mut ctl_state = HashMap::new();
        let mut ctl_metadata = HashMap::new();

        for (ctl_name, ctl_cfg) in &creation_spec.cfg_spec.controls {
            // dbg!(&ctl_cfg);
            ctl_state.insert(ctl_name.clone(), ctl_cfg.range.clamp(ctl_cfg.initial_value));
            ctl_metadata.insert(
                ctl_name.clone(),
                (
                    ctl_cfg.r#type,
                    ctl_cfg
                        .range
                        .cast::<BaseNumT>()
                        .expect("Can't convert HID control range to BaseNumericT"),
                ),
            );

            if ctl_cfg.r#type.is_button() || ctl_cfg.r#type.is_key() {
                keys.insert(KeyCode::new(ctl_cfg.r#type.into()));
            } else if ctl_cfg.r#type.is_relative() {
                relative_axis.insert(evdev::RelativeAxisCode(ctl_cfg.r#type.into()));
            } else if ctl_cfg.r#type.is_absolute() {
                evdev_builder = evdev_builder
                    .with_absolute_axis(&UinputAbsSetup::new(
                        evdev::AbsoluteAxisCode(ctl_cfg.r#type.into()),
                        AbsInfo::new(
                            ctl_cfg.range.clamp(ctl_cfg.initial_value) as i32,
                            ctl_cfg.range.from() as i32,
                            ctl_cfg.range.to() as i32,
                            ctl_cfg.properties.as_ref().map(|p| p.fuzz as i32).unwrap_or(0),
                            ctl_cfg.properties.as_ref().map(|p| p.flat as i32).unwrap_or(0),
                            ctl_cfg.properties.as_ref().map(|p| p.resolution as i32).unwrap_or(1),
                        ),
                    ))
                    .context("Failed to setup HID with absolute axis")?;
            }
        }

        evdev_builder = evdev_builder.with_keys(&keys)?;
        evdev_builder = evdev_builder.with_relative_axes(&relative_axis)?;

        let mut fake_accepting_unsupported_user_configured_effects: bool = false;

        if creation_spec.cfg_spec.virtual_device_is_ff_enabled() {
            let ff_max_effects = creation_spec.cfg_spec.virtual_device_get_ff_max_effects();
            if creation_spec.debug_ff || debug_creation {
                log::debug!(
                    "Enabling Force Feedback for '{}' (cfg key {cfg_key}), max effects: {ff_max_effects}",
                    creation_spec.cfg_spec.virtual_device_name_ref().unwrap()
                );
            }
            let mut ff_effects = AttributeSet::<FFEffectCode>::new();

            let ff_config = creation_spec
                .cfg_spec
                .virtual_device_force_feedback_info_ref()
                .expect("FF enabled only when this is set and can not be None");

            if creation_spec.cfg_spec.virtual_device_fake_accepting_all_effects() {
                for effect in HidFfEffect::iter() {
                    ff_effects.insert(effect.into());
                }
            } else if ff_config.effects.is_empty() {
                ff_effects.insert(HidFfEffect::Constant.into());
            } else {
                for effect in ff_config.effects.iter() {
                    ff_effects.insert(effect.clone().into());
                    if effect.is_periodic() {
                        ff_effects.insert(HidFfEffect::Periodic.into());
                    }
                    match effect {
                        HidFfEffect::Constant | HidFfEffect::Ramp | HidFfEffect::Spring | HidFfEffect::Friction => {
                            log::info!("Enabling fully supported FF effect {effect} for {cfg_key}.")
                        }
                        _ => {
                            log::warn!(
                                "User config asks to enable {effect} force feedback effect, \
                                but it's not yet supported, we are still adding it to fake that we accept it. \
                                NB: If you wish to fake supporting all the other effects (e.g. for debuggig), \
                                consider setting fake_accepting_all_effects: bool in force feedback config section."
                            );
                            fake_accepting_unsupported_user_configured_effects = true;
                        }
                    }
                }
            }

            evdev_builder = evdev_builder
                .with_ff(&ff_effects)
                .context("Failed to to create virtual HID with FF effects {ff_effects:?}")?
                .with_ff_effects_max(ff_max_effects as u32);
        }

        let mut virtual_device = evdev_builder
            .build()
            .context("Evdev builder failed to build a virtual HID device.")?;

        let client_side_path = virtual_device.enumerate_dev_nodes_blocking()?.last().unwrap()?;

        let ff_enabled = creation_spec.cfg_spec.virtual_device_is_ff_enabled();
        let (tx1, rx1) = tokio::sync::mpsc::unbounded_channel::<DeviceThreadCmd>();

        // --- Initialized or based on whether thread is spawned or not ---
        // --- Owned virtual device thread is spawned to handle FFB events and to set controls values ---
        let mut owned_virtual_device_thread_io = None;
        let mut owned_virtual_device = None;
        let mut owned_virtual_device_thread_data: Option<CancellationToken> = None;
        let mut ctl_states = None;
        let virtual_device_id = ObjId::from(intern_str(&format!("{}/{}", cfg_key, device_name)));
        // ---
        if creation_spec.cfg_spec.virtual_device_is_ff_enabled() {
            // --- Objects remaining in local state ---
            let cancellation_token = CancellationToken::new();
            owned_virtual_device_thread_io = Some(Arc::new(OwnedVirtualHIDDeviceThreadIO::new(
                creation_spec.cfg_spec.virtual_device_force_feedback_info_ref(),
            )));
            ctl_states = Some(Arc::new(std::array::from_fn(|_| Default::default())));
            // --- Clones to be moved to the thread ---
            let thread_ctl_states = ctl_states.as_ref().unwrap().clone();
            let thread_device_name = creation_spec
                .cfg_spec
                .virtual_device_name_ref()
                .unwrap_or_default()
                .to_string();
            let thread_ff_max_effects = creation_spec.cfg_spec.virtual_device_get_ff_max_effects();
            let thread_fake_accepting_all_effects = creation_spec.cfg_spec.virtual_device_fake_accepting_all_effects();
            let thread_ff_thread_io = owned_virtual_device_thread_io.clone().unwrap();
            let thread_cancellation_token = cancellation_token.clone();
            owned_virtual_device_thread_data = Some(cancellation_token);
            std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .thread_name(format!("HID owned/ffb thread for {thread_device_name}"))
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(crate::hid_owned_and_ffb::owned_hid_device_thread(
                        virtual_device,
                        thread_cancellation_token,
                        thread_device_name,
                        thread_ff_max_effects,
                        thread_fake_accepting_all_effects || fake_accepting_unsupported_user_configured_effects,
                        thread_ff_thread_io,
                        rx1,
                        virtual_device_id,
                        thread_ctl_states,
                        creation_spec.debug_ff,
                    ));
            });
        } else {
            owned_virtual_device = Some(virtual_device);
        }

        log::info!(
            "Created virtual HID device: {} ({}){}",
            creation_spec.cfg_spec.virtual_device_name_ref().unwrap_or_default(),
            cfg_key,
            if creation_spec.cfg_spec.virtual_device_is_ff_enabled() {
                format!(
                    ", force feedback ON, max effects {}",
                    creation_spec.cfg_spec.virtual_device_get_ff_max_effects()
                )
            } else {
                ".".into()
            }
        );

        {
            let perms_wait_begin_at = Instant::now();
            let perms_wait_timeout_sec = 5.0;
            while Instant::now().duration_since(perms_wait_begin_at).as_secs_f32() <= perms_wait_timeout_sec {
                if std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&client_side_path)
                    .is_ok()
                {
                    break;
                }
                std::thread::sleep(Duration::from_secs_f32(0.1));
            }

            if Instant::now().duration_since(perms_wait_begin_at).as_secs_f32() > perms_wait_timeout_sec {
                unreachable!(
                    "Timed out while waiting to access newly created virtual device file {:?}... Check your udev settings/permissions!",
                    client_side_path
                );
            }
        }

        let mut device = Self::open_from_path(client_side_path.to_str().unwrap(), ctl_states, creation_spec.debug)?;

        device.classification.insert(HidDeviceKind::Virtual);
        device.is_owned_virtual_device = true;
        device.ff_enabled = ff_enabled;
        device.ff_is_a_condition_effect_enabled = creation_spec.cfg_spec.virtual_device_fake_accepting_all_effects()
            || creation_spec
                .cfg_spec
                .virtual_device_force_feedback_info_ref()
                .map(|ff| {
                    ff.effects.contains(&HidFfEffect::Intertia)
                        || ff.effects.contains(&HidFfEffect::Damper)
                        || ff.effects.contains(&HidFfEffect::Friction)
                        || ff.effects.contains(&HidFfEffect::Spring)
                })
                .unwrap_or_default();
        device.owned_virtual_device_thread_io = owned_virtual_device_thread_io;
        device.owned_virtual_device = owned_virtual_device;
        if ff_enabled {
            device.owned_virtual_device_cmd = Some(tx1);
        }
        device.owned_virtual_device_thread_cancellation = owned_virtual_device_thread_data;
        device.is_owned_virtual_device_persistent = creation_spec.is_persistent;
        device.cfg_key = cfg_key.into();
        device.id = virtual_device_id;
        anyhow::Ok(device)
    }

    pub(crate) fn open_from_path(
        path: &str,
        ctl_states: Option<Arc<DeviceControlStates>>,
        debug: DebugLevel,
    ) -> anyhow::Result<HidDevice>
    where
        Self: Sized,
    {
        anyhow::Ok(Self::init_with_opened_device(
            evdev::Device::open(path)?,
            PathBuf::from_str(path)?,
            ctl_states,
            debug,
        )?)
    }

    pub(crate) fn ff_get_x_sum_symm_norm(&self) -> BaseNumT {
        if let Some(io) = &self.owned_virtual_device_thread_io {
            io.force_sum[X_AXIS_IDX].load(std::sync::atomic::Ordering::Relaxed) as BaseNumT
        } else {
            0.0
        }
    }

    pub(crate) fn ff_get_y_sum_symm_norm(&self) -> BaseNumT {
        if let Some(io) = &self.owned_virtual_device_thread_io {
            io.force_sum[Y_AXIS_IDX].load(std::sync::atomic::Ordering::Relaxed) as BaseNumT
        } else {
            0.0
        }
    }

    pub(crate) fn ff_is_a_condition_effect_enabled(&self) -> bool {
        self.ff_is_a_condition_effect_enabled
    }

    pub(crate) fn owned_virtual_device_thread_io(&self) -> &Option<Arc<OwnedVirtualHIDDeviceThreadIO>> {
        &self.owned_virtual_device_thread_io
    }

    pub(crate) fn get_cfg_key(&self) -> &str {
        &self.cfg_key
    }

    pub(crate) fn is_persistent(&self) -> bool {
        self.is_owned_virtual_device_persistent
    }

    pub(crate) fn set_persistent(&mut self, p: bool) {
        self.is_owned_virtual_device_persistent = p;
    }

    fn evdev_event_to_hid_device_event(
        _device_name: &str,
        opened_device_id: ObjId,
        event: evdev::InputEvent,
        debug: DebugLevel,
    ) -> Option<MappedDeviceEvent> {
        let control_type = crate::mapped_controls::MappedCtls::from(event);
        if control_type.is_unhandled() {
            if debug.is_on() && event.event_type() != evdev::EventType::SYNCHRONIZATION {
                log::debug!("Event not associated with any control: {:?}", event);
            }
            None
        } else {
            MappedDeviceEvent {
                device_id: opened_device_id,
                event: crate::mapped_device::MappedEvents::Hid(MappedHidEvent {
                    control_type,
                    value: event.value() as BaseNumT,
                }),
            }
            .into()
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run_main_device_event_thread(
        opened_device_id: ObjId,
        platform_device: evdev::Device,
        cancellation_token: CancellationToken,
        device_name: &str,
        ctl_states: Arc<DeviceControlStates>,
        mut app_comm: DeviceComm,
        debug: DebugLevel,
    ) {
        let mut external_notification_tx = None;

        // NB: acquiring here so that AsyncFd would associate with current runtime.
        let mut platform_device_stream = platform_device.into_event_stream().unwrap();

        while !cancellation_token.is_cancelled() {
            tokio::select! {
                Some(Some(cmd)) = app_comm.0.recv().with_cancellation_token(&cancellation_token) => {
                    match cmd {
                        DeviceThreadCmd::SetExternalNotification(unbounded_sender) => {
                            external_notification_tx = Some(unbounded_sender)
                        }
                        DeviceThreadCmd::SetControlValue(control_type, control_value) => {
                            set_hid_control_unowned_device(platform_device_stream.device_mut(), control_type, control_value);
                        }
                    }
                },
                Some(Ok(evdev_event)) = platform_device_stream.next_event().with_cancellation_token(&cancellation_token) => {
                    let event =
                        Self::evdev_event_to_hid_device_event(device_name, opened_device_id, evdev_event, debug);
                    if let Some(MappedDeviceEvent {
                        device_id: _,
                        event: MappedEvents::Hid(MappedHidEvent { control_type, value }),
                    }) = event
                    {
                        ctl_states[control_type as usize].store(value, std::sync::atomic::Ordering::Relaxed);

                        if let Some(external_notification_tx) = &external_notification_tx
                            && let Err(e) = external_notification_tx.send(event.unwrap())
                        {
                            log::error!("{e}");
                        }
                    }
                },
                else => break
            }
        }

        log::info!(
            "Stopping thread for HID {:?} {}",
            platform_device_stream.device().input_id(),
            device_name
        );
    }
}

pub(crate) fn control_value_to_evdev_event(control_type: MappedCtls, control_value: BaseNumT) -> InputEvent {
    if control_type.is_button() || control_type.is_key() {
        InputEvent::new(
            evdev::EventType::KEY.0,
            control_type.into(),
            if control_value == 0.0 { 0 } else { 1 }, // We may map arbitrary floating point values to button and
                                                      // we count any non-zero one as "button on", that is value 1.
        )
    } else if control_type.is_absolute() {
        InputEvent::new(evdev::EventType::ABSOLUTE.0, control_type.into(), control_value as i32)
    } else if control_type.is_relative() {
        InputEvent::new(evdev::EventType::RELATIVE.0, control_type.into(), control_value as i32)
    } else {
        log::error!("Sending {control_type:?} control change event is not yet supported.");
        *SynchronizationEvent::new(SynchronizationCode::SYN_REPORT, 0)
    }
}

pub(crate) fn set_hid_control_unowned_device(device: &mut Device, control_type: MappedCtls, control_value: BaseNumT) {
    if device
        .send_events(&[
            control_value_to_evdev_event(control_type, control_value),
            *SynchronizationEvent::new(SynchronizationCode::SYN_REPORT, 0),
        ])
        .is_err()
    {
        log::error!("Failed to emit control change event on unowned HID device.");
    };
}

pub(crate) fn set_hid_control_virtual_owned_device(
    device: &mut VirtualDevice,
    control_type: MappedCtls,
    control_value: BaseNumT,
) {
    if device
        .emit(&[control_value_to_evdev_event(control_type, control_value)])
        .is_err()
    {
        log::error!("Failed to emit control change event on owned virtual HID device.");
    };
}

#[allow(unused)]
fn test_virtual_joystick_internal(with_ff: bool) {
    let mut vjk = HidDevice::create_virtual_device(
        "TestVJ",
        HidVirtualDeviceCreationSpec {
            cfg_spec: HidDeviceCfg {
                enabled: true,
                description: "Use with caution.".to_string(),
                controls: {
                    let mut controls = BTreeMap::new();
                    controls.insert(String::from("X"), {
                        let mut control = HidControlMatcherCfg::default();
                        control.r#type = MappedCtls::AbsX;
                        control.range = HID_AXIS_MAX_INTERVAL;
                        control.initial_value = 42.0;
                        control
                    });
                    controls
                },
                classification: Some(HidDeviceClassificationCfg(BitFlags::from_flag(HidDeviceKind::Joystick))),
                params__: crate::schemas_hid::HidVirtualOrMatcherParamsCfg::VirtualDevice(HidVirtualParamsCfg {
                    persistent: true,
                    name: "MMVJ Test Virtual Joystick".to_string(),
                    bus: Default::default(),
                    force_feedback: if with_ff {
                        Some(HIDDeviceForceFeedbackCfg {
                            state_xy: Default::default(),
                            enabled: true,
                            effects: vec![HidFfEffect::Constant],
                            max_effects: 16,
                            gain: 0.1,
                            fake_accepting_all_effects: false,
                            autocenter: false,
                        })
                    } else {
                        None
                    },
                }),
            },
            debug: DebugLevel::Low,
            debug_ff: true,
            is_persistent: true,
            device_kind: HidDeviceKind::Joystick,
        },
        true,
    )
    .unwrap();

    let (notification_tx, mut notification_rx) = unbounded_channel::<MappedDeviceEvent>();

    vjk.attach_events_listener(Some(notification_tx));

    dbg!(&vjk);
    dbg!("Sleeping 1 second before infinite loop...");
    std::thread::sleep(Duration::from_secs_f32(1.0));

    let scale = 1000.0;
    let mut dir = 1.0;
    let mut current = 0.0;
    loop {
        vjk.set_control_value(MappedCtls::AbsX, (current + dir * scale));
        if vjk.get_control_value_cached(MappedCtls::AbsX) <= HID_AXIS_MAX_INTERVAL.from
            || vjk.get_control_value_cached(MappedCtls::AbsX) >= HID_AXIS_MAX_INTERVAL.to
        {
            dir *= -1.0;
        }
        current = vjk.get_control_value_cached(MappedCtls::AbsX);
        if let Ok(v) = notification_rx.try_recv() {
            dbg!("External notification for control change received back from device:");
            dbg!(v);
        }
        std::thread::sleep(Duration::from_secs_f32(0.01));
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_virtual_joystick() {
    test_virtual_joystick_internal(true);
}

impl HidDevice {
    pub(crate) fn get_control_value_cached(&self, control: MappedCtls) -> BaseNumT {
        self.ctl_states[control as usize].load(std::sync::atomic::Ordering::Relaxed) as BaseNumT
    }

    pub(crate) fn set_control_value(&mut self, control: MappedCtls, value: BaseNumT) {
        self.ctl_states[control as usize].store(value, std::sync::atomic::Ordering::Relaxed);

        if branches::likely(self.is_owning()) {
            if let Some(owned_device) = &mut self.owned_virtual_device {
                set_hid_control_virtual_owned_device(owned_device, control, value);
            } else if let Some(owned_virtual_device_cmd) = &self.owned_virtual_device_cmd {
                if let Err(e) = owned_virtual_device_cmd.send(DeviceThreadCmd::SetControlValue(control, value)) {
                    branches::mark_unlikely();
                    unreachable!(
                        "Error while setting control value on owned HID device {} {:?}",
                        self.name, e
                    );
                }
            } else {
                branches::mark_unlikely();
                unreachable!(
                    "Owning virtual device abstraction doesn't have neither object instance \
                        nor a corresponding owning thread handle."
                );
            }
        } else if let Err(e) = self
            .client_side_thread_rx_tx
            .1
            .send(DeviceThreadCmd::SetControlValue(control, value))
        {
            branches::mark_unlikely();
            log::error!(
                "Error while setting/spoofing control value on unowned HID device{} {e:?}",
                self.name
            );
        }
    }

    fn init_with_opened_device(
        platform_device: evdev::Device,
        device_path: PathBuf,
        ctl_states: Option<Arc<DeviceControlStates>>,
        debug: DebugLevel,
    ) -> anyhow::Result<Self> {
        let device_name = platform_device.name().unwrap_or("Unknown device name").to_string();

        // device.grab();
        // device.ungrab();

        let ctl_states = if let Some(ctl_states) = ctl_states {
            ctl_states
        } else {
            Arc::new(std::array::from_fn(|_| Default::default()))
        };

        let mut ctl_intervals: [NumInterval<BaseNumT>; MappedCtls::Unhandled as usize] =
            std::array::from_fn(|_| ZERO_INTERVAL);

        if let Some(abs_axes) = platform_device.supported_absolute_axes() {
            for abs in abs_axes {
                if let Some(abs_info) = platform_device.get_abs_state()?.get(abs.0 as usize) {
                    if let Ok(ctl) = MappedCtls::try_from(abs) {
                        ctl_intervals[ctl as usize].from = abs_info.minimum as BaseNumT;
                        ctl_intervals[ctl as usize].to = abs_info.maximum as BaseNumT;
                        ctl_states[ctl as usize]
                            .store(abs_info.value as BaseNumT, std::sync::atomic::Ordering::Relaxed);
                        if debug.is_on() {
                            log::debug!(
                                "Abs info for {device_path:?} axis {} {:?}",
                                abs.0,
                                ctl_intervals[ctl as usize]
                            )
                        }
                    }
                } else {
                    log::error!(
                        "HID device {device_name} does not support the {abs:?} axis, although reported as supported."
                    );
                }
            }
        }

        let (tx1, rx1) = tokio::sync::mpsc::unbounded_channel::<DeviceThreadCmd>();
        let (tx2, rx2) = tokio::sync::mpsc::unbounded_channel::<DeviceThreadCmd>();
        let cancellation_token = CancellationToken::new();
        let opened_device_id = ObjId::from(intern_str(&device_name));

        let classification = platform_device.get_classification();

        {
            let device_name = device_name.clone();
            let cancellation_token = cancellation_token.clone();
            let ctl_states = ctl_states.clone();
            std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .thread_name(format!("HID general thread for {device_name}"))
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(Self::run_main_device_event_thread(
                        opened_device_id,
                        platform_device,
                        cancellation_token,
                        &device_name,
                        ctl_states,
                        (rx2, tx1),
                        debug,
                    ));
            });
        }

        anyhow::Ok(Self {
            id: opened_device_id,
            name: device_name,
            _client_side_path: device_path,
            is_owned_virtual_device: false,
            is_owned_virtual_device_persistent: false,
            owned_virtual_device: None,
            owned_virtual_device_cmd: None,
            owned_virtual_device_thread_cancellation: None,
            events_listener: None,
            client_side_thread_rx_tx: (rx1, tx2),
            client_side_thread_cancellation: cancellation_token,
            ctl_states,
            ff_enabled: false,
            owned_virtual_device_thread_io: None,
            ff_is_a_condition_effect_enabled: false,
            cfg_key: "no cfg key".into(),
            classification,
        })
    }
}

impl Drop for HidDevice {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl MappedDevice for HidDevice {
    type EventsListener = tokio::sync::mpsc::UnboundedSender<MappedDeviceEvent>;

    fn close(&self) -> anyhow::Result<()> {
        self.client_side_thread_cancellation.cancel();

        if let Some(ct) = self.owned_virtual_device_thread_cancellation.as_ref() {
            ct.cancel();
        }

        Ok(())
    }

    fn get_id(&self) -> ObjId {
        self.id
    }

    fn attach_events_listener(&mut self, listener: Option<tokio::sync::mpsc::UnboundedSender<MappedDeviceEvent>>) {
        if let Some(listener) = &listener {
            self.client_side_thread_rx_tx
                .1
                .send(DeviceThreadCmd::SetExternalNotification(listener.clone()))
                .expect("Can't set external notification with device client side thread.");
            if let Some(tx) = &self.owned_virtual_device_cmd {
                tx.send(DeviceThreadCmd::SetExternalNotification(listener.clone()))
                    .expect("Can't set external notification with oned virtual device thread.");
            }
        }
        self.events_listener = listener;
    }

    fn is_owning(&self) -> bool {
        self.is_owned_virtual_device
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn get_filesystem_path(&self) -> &Path {
        &self._client_side_path
    }
}

// ---------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    const SAFE_LEN: usize = UINPUT_MAX_NAME_SIZE - 2;

    #[test]
    fn test_empty_string() {
        assert_eq!(sanitize_hid_name(""), "");
    }

    #[test]
    fn test_short_ascii() {
        let input = "Virtual Joystick";
        assert_eq!(sanitize_hid_name(input), input);
    }

    #[test]
    fn test_exact_safe_len_ascii() {
        let input = "a".repeat(SAFE_LEN);
        assert_eq!(sanitize_hid_name(&input), input);
        assert_eq!(sanitize_hid_name(&input).len(), SAFE_LEN);
    }

    #[test]
    fn test_one_over_safe_len_ascii() {
        let input = "a".repeat(SAFE_LEN + 1);
        let truncated = sanitize_hid_name(&input);
        assert_eq!(truncated.len(), SAFE_LEN);
        assert_eq!(*truncated, input[..SAFE_LEN]);
    }

    #[test]
    fn test_long_ascii() {
        let input = "x".repeat(200);
        let truncated = sanitize_hid_name(&input);
        assert_eq!(truncated.len(), SAFE_LEN);
        assert!(truncated.chars().all(|c| c == 'x'));
    }

    #[test]
    fn test_utf8_fits_exactly() {
        // '€' is 3 bytes. 78 / 3 = 26 exactly.
        let input = "€".repeat(26);
        assert_eq!(input.len(), SAFE_LEN);
        let truncated = sanitize_hid_name(&input);
        assert_eq!(truncated, input);
        assert_eq!(truncated.len(), SAFE_LEN);
    }

    #[test]
    fn test_utf8_split_2byte_char() {
        // 'é' is 2 bytes (0xC3 0xA9)
        // Place it so the first byte lands at index SAFE_LEN-1
        let prefix = "a".repeat(SAFE_LEN - 1);
        let input = format!("{}é", prefix); // total len: 79
        let truncated = sanitize_hid_name(&input);

        // Should backtrack to exclude the incomplete 2-byte sequence
        assert_eq!(truncated.len(), SAFE_LEN - 1);
        assert_eq!(truncated, prefix);
    }

    #[test]
    fn test_utf8_split_3byte_char() {
        // '€' is 3 bytes (0xE2 0x82 0xAC)
        // Place it so first byte is at SAFE_LEN-2
        let prefix = "a".repeat(SAFE_LEN - 2);
        let input = format!("{}€", prefix); // total len: 79
        let truncated = sanitize_hid_name(&input);

        assert_eq!(truncated.len(), SAFE_LEN - 2);
        assert_eq!(truncated, prefix);
    }

    #[test]
    fn test_utf8_split_4byte_char() {
        // '🎮' is 4 bytes (0xF0 0x9F 0x8E 0xAE)
        // Place it so first byte is at SAFE_LEN-3
        let prefix = "a".repeat(SAFE_LEN - 3);
        let input = format!("{}🎮", prefix); // total len: 79
        let truncated = sanitize_hid_name(&input);

        assert_eq!(truncated.len(), SAFE_LEN - 3);
        assert_eq!(truncated, prefix);
    }

    #[test]
    fn test_mixed_unicode_long_string() {
        let input = "Player1 🎮 Joystick 🕹️ UTF-8 Test 🌍 ".repeat(10);
        let truncated = sanitize_hid_name(&input);

        // Core guarantees:
        assert!(truncated.len() <= SAFE_LEN);
        assert!(input.starts_with(truncated)); // Always a valid prefix
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok()); // Valid UTF-8

        // If truncated, it must end exactly at a character boundary
        if truncated.len() < input.len() {
            assert!(input.is_char_boundary(truncated.len()));
            // Next byte should be the start of a multi-byte sequence or ASCII
            let next_byte = input.as_bytes()[truncated.len()];
            assert!(!(next_byte & 0b1100_0000 == 0b1000_0000)); // Not a continuation byte
        }
    }

    #[test]
    fn test_all_multi_byte_just_over_limit() {
        // String of 26 '€' (78 bytes) + 1 more '€' (3 bytes) = 81 bytes
        let input = "€".repeat(27);
        let truncated = sanitize_hid_name(&input);

        // Should drop the 27th '€' entirely, leaving exactly 26
        assert_eq!(truncated.len(), SAFE_LEN);
        assert_eq!(truncated, "€".repeat(26));
    }
}
