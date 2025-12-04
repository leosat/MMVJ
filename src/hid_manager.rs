use crate::base_num::BaseNumT;
use crate::interner::get_interned_str;

use crate::debug::DebugLevel;
use crate::device_and_device_manager::{
    AvailableDeviceInfoIface, Device, DeviceClassification, DeviceEvent, DeviceKind, DeviceManagerCommon,
    DeviceManagerWithFfb, OpenedDeviceInfo, OpenedDeviceInfoIface, WithDeviceClassification,
};
use crate::hid_device::{HidDevice, HidDeviceEvent, HidVirtualDeviceCreationSpec};
use crate::hid_owned_and_ffb::{X_AXIS_IDX, Y_AXIS_IDX};
use crate::num_interval::{NumInterval, OutOfRangePolicy};
use crate::schemas_common::ObjId;
use crate::schemas_hid::{HidDeviceCfg, HidDeviceClassificationCfg, HidVirtualOrMatcherParamsCfg};
use anyhow::{Result, bail};
use enumflags2::BitFlags;
use evdev::{BusType, EventType};
use std::collections::HashMap;
use std::future::poll_fn;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use unchecked_refcell::UncheckedRefCell;

// ---------------------------

#[derive(Debug, Clone)]
pub(crate) struct AvailableHIDDeviceInfo {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) classification: DeviceClassification,
}

// ----------------------------

pub(crate) struct HidManager {
    debug: DebugLevel,
    debug_ff: bool,
    #[allow(clippy::type_complexity)]
    device_key_to_devices: UncheckedRefCell<HashMap<String, Vec<(Rc<UncheckedRefCell<HidDevice>>, HidDeviceCfg)>>>,
    per_device_event_notification_tx: tokio::sync::mpsc::UnboundedSender<HidDeviceEvent>,
    all_devices_rx: UncheckedRefCell<tokio::sync::mpsc::UnboundedReceiver<HidDeviceEvent>>,
}

impl HidManager {
    pub(crate) fn set_control_matcher_and_broadcast(
        &self,
        device_key: &str,
        ctl_key: &str,
        value: BaseNumT,
        _silent: bool,
    ) {
        if self.debug.is_hi() {
            log::debug!("Request to set device matcher control {device_key} {ctl_key}");
        }
        self.device_key_to_devices
            .borrow_mut()
            .get_mut(device_key)
            .map(|devices| {
                for device in devices {
                    if let Some(c) = &device.1.controls.get(ctl_key) {
                        if self.debug.is_hi() {
                            log::debug!(
                                "Setting device control {} ({})/{}",
                                device.0.borrow().get_name(),
                                device.0.borrow().get_filesystem_path().to_string_lossy(),
                                ctl_key
                            );
                        }
                        device.0.borrow_mut().set_control_value(c.r#type, value)
                    }
                }
            })
            .or_else(|| {
                log::warn!("Trying to set control value for {device_key} / {ctl_key:?} with no associated devices.");
                Some(())
            });
    }

    pub(crate) fn get_control_value(&self, device_key: &str, ctl_key: &str) -> BaseNumT {
        if let Some(devices) = self.device_key_to_devices.borrow().get(device_key)
            && let Some(ctl) = devices[0].1.controls.get(ctl_key)
        {
            return devices[0].0.borrow().get_control_value_cached(ctl.r#type);
        }
        log::warn!("Trying to get control value of unopened device  {device_key} / {ctl_key:?}");
        0.0
    }

    pub(crate) fn _get_device_id_by_cfg_key(&self, key: &str) -> ObjId {
        self.device_key_to_devices.borrow().get(key).unwrap()[0]
            .0
            .borrow()
            .get_id()
    }

    pub(crate) fn new(debug: DebugLevel, debug_ff: bool) -> Result<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HidDeviceEvent>();
        Ok(Self {
            debug,
            debug_ff,
            device_key_to_devices: Default::default(),
            per_device_event_notification_tx: tx,
            all_devices_rx: UncheckedRefCell::new(rx),
        })
    }

    pub(crate) fn ff_update_axis_pos(
        &self,
        device_key: &str,
        ctl_key: &str,
        control_interval: NumInterval<BaseNumT>,
        axis_idx: usize,
    ) {
        match self.device_key_to_devices.borrow().get(device_key) {
            Some(d) if d[0].0.borrow_mut().ff_is_a_condition_effect_enabled() => {
                // dbg!("Updating axis position due to conditional effect enabled on virtual HID");
                if let Some(io) = d[0].0.borrow().owned_virtual_device_thread_io() {
                    io.axis_pos_symm_norm[axis_idx].store(
                        control_interval.map_to_symm_unit(
                            self.get_control_value(device_key, ctl_key),
                            OutOfRangePolicy::WarnIfDebugAndClamp,
                        ),
                        Ordering::Relaxed,
                    );
                }
            }
            Some(_) => (),
            _ => log::warn!(
                "Device '{}' not found among opened, is it enabled in config?",
                device_key
            ),
        }
    }

    pub(crate) fn ff_set_x_axis_pos(&self, device_key: &str, ctl_key: &str, control_interval: NumInterval<BaseNumT>) {
        self.ff_update_axis_pos(device_key, ctl_key, control_interval, X_AXIS_IDX);
    }

    pub(crate) fn ff_set_y_axis_pos(&self, device_key: &str, ctl_key: &str, control_interval: NumInterval<BaseNumT>) {
        self.ff_update_axis_pos(device_key, ctl_key, control_interval, Y_AXIS_IDX);
    }

    pub(crate) fn ff_get_x_sum_symm_norm(&self, device_key: &str) -> BaseNumT {
        if let Some(d) = self.device_key_to_devices.borrow().get(device_key) {
            d[0].0.borrow().ff_get_x_sum_symm_norm()
        } else {
            0.0
        }
    }

    pub(crate) fn ff_get_y_sum_symm_norm(&self, device_key: &str) -> BaseNumT {
        if let Some(d) = self.device_key_to_devices.borrow().get(device_key) {
            d[0].0.borrow().ff_get_y_sum_symm_norm()
        } else {
            0.0
        }
    }

    pub(crate) fn create_virtual_device(
        &self,
        device_key: &str,
        device_cfg: &HidDeviceCfg,
        is_persistent: bool,
    ) -> Result<()> {
        if let Some(existing) = self.device_key_to_devices.borrow_mut().get_mut(device_key)
            && !existing.is_empty()
        {
            if existing[0].0.borrow().is_persistent() != is_persistent {
                log::info!(
                    "Updating persistence for virtual device '{}': {} -> {}",
                    device_key,
                    existing[0].0.borrow().is_persistent(),
                    is_persistent
                );
                existing[0].0.borrow_mut().set_persistent(is_persistent);
            }
            log::info!("Virtual device '{}' already exists, skipping creation.", device_key);
            return Ok(());
        }

        let mut d = HidDevice::create_virtual_device(
            device_key,
            HidVirtualDeviceCreationSpec {
                cfg_spec: device_cfg.clone(),
                debug: self.debug,
                debug_ff: self.debug_ff,
                is_persistent,
            },
            self.debug.is_on(),
        )?;

        d.attach_events_listener(Some(self.per_device_event_notification_tx.clone()));

        self.device_key_to_devices
            .borrow_mut()
            .entry(device_key.to_string())
            .or_default()
            .push((Rc::new(UncheckedRefCell::new(d)), device_cfg.clone()));

        Ok(())
    }

    pub(crate) fn destroy_virtual_device_if_exists(&self, device_key: &str) {
        self.device_key_to_devices.borrow_mut().retain(|_, device| {
            if !device.is_empty() {
                if device_key != device[0].0.borrow().get_cfg_key() {
                    return true;
                }
                if let Err(e) = device[0].0.borrow().close() {
                    log::error!("Error while closing a virtual device {device_key}: {e}");
                };
            }
            false
        });
    }
}

impl WithDeviceClassification for HidDeviceCfg {
    fn get_classification(&self) -> BitFlags<DeviceKind> {
        if let Some(classification) = &self.classification {
            return classification.0;
        }

        let mut flags: BitFlags<DeviceKind> = BitFlags::empty();
        if self.controls.iter().any(|c| c.1.r#type.is_a_joystick_control()) {
            flags.insert(DeviceKind::Joystick);
        }

        if self.controls.iter().any(|c| c.1.r#type.is_a_gamepad_control()) {
            flags.insert(DeviceKind::Gamepad);
        }

        if self.controls.iter().any(|c| c.1.r#type.is_a_keyboard_control()) {
            flags.insert(DeviceKind::Keyboard);
        }

        if self.controls.iter().any(|c| c.1.r#type.is_a_mouse_control()) {
            flags.insert(DeviceKind::Mouse);
        }

        if flags.is_empty() && !self.controls.is_empty() {
            flags.insert(DeviceKind::MiscMappable);
        }

        if self.is_a_virtual() {
            flags.insert(DeviceKind::Virtual);
        }

        flags
    }

    fn is_a_joystick(&self) -> bool {
        self.get_classification().is_a_joystick()
    }

    fn is_a_keyboard(&self) -> bool {
        self.get_classification().is_a_keyboard()
    }

    fn is_a_gamepad(&self) -> bool {
        self.get_classification().is_a_gamepad()
    }

    fn is_a_mouse(&self) -> bool {
        self.get_classification().is_a_mouse()
    }

    fn is_a_misc_mappable(&self) -> bool {
        self.get_classification().is_a_misc_mappable()
    }

    fn is_a_virtual(&self) -> bool {
        match self.params__ {
            HidVirtualOrMatcherParamsCfg::DeviceMatcher(_) => false,
            HidVirtualOrMatcherParamsCfg::VirtualDevice(_) => true,
        }
    }

    fn update_classification(&mut self) {
        self.classification = None;
        self.classification = Some(HidDeviceClassificationCfg(self.get_classification()));
    }
}

impl WithDeviceClassification for evdev::Device {
    fn get_classification(&self) -> BitFlags<DeviceKind> {
        let mut flags: BitFlags<DeviceKind> = BitFlags::empty();
        if self.supported_absolute_axes().is_some()
            || self
                .supported_keys()
                .is_some_and(|k| k.contains(evdev::KeyCode::BTN_TRIGGER))
        {
            flags.insert(DeviceKind::Joystick);
        }

        if let Some(keys) = self.supported_keys()
            && keys.contains(evdev::KeyCode::BTN_SOUTH)
        {
            flags.insert(DeviceKind::Gamepad);
        }

        if let Some(keys) = self.supported_keys()
            && (keys.contains(evdev::KeyCode::KEY_A)
                || keys.contains(evdev::KeyCode::KEY_SPACE)
                || keys.contains(evdev::KeyCode::KEY_ENTER))
        {
            flags.insert(DeviceKind::Keyboard);
        }

        if let Some(rel) = self.supported_relative_axes()
            && rel.contains(evdev::RelativeAxisCode::REL_X)
            && rel.contains(evdev::RelativeAxisCode::REL_Y)
        {
            flags.insert(DeviceKind::Mouse);
        }

        if flags.is_empty()
            && (self.supported_absolute_axes().is_some()
                || self.supported_relative_axes().is_some()
                || self.supported_keys().is_some())
            // TODO: provide mapped control enum values and predefines for the below:
            || self.supported_leds().is_some()
            // Let's make some noize
            || self.supported_sounds().is_some()
        {
            flags.insert(DeviceKind::MiscMappable);
        }

        if self.input_id().bus_type() == BusType::BUS_VIRTUAL
            || self.supported_events().contains(EventType::UINPUT)
            || self.physical_path().is_some_and(|p| {
                std::fs::canonicalize(p)
                    .unwrap_or_default()
                    .to_string_lossy()
                    .contains("virtual")
            })
        //  || self.unique_name().is_some()
        {
            flags.insert(DeviceKind::Virtual);
        }

        flags
    }

    fn is_a_joystick(&self) -> bool {
        self.get_classification().is_a_joystick()
    }

    fn is_a_keyboard(&self) -> bool {
        self.get_classification().is_a_keyboard()
    }

    fn is_a_gamepad(&self) -> bool {
        self.get_classification().is_a_gamepad()
    }

    fn is_a_mouse(&self) -> bool {
        self.get_classification().is_a_mouse()
    }

    fn is_a_misc_mappable(&self) -> bool {
        self.get_classification().is_a_misc_mappable()
    }

    fn is_a_virtual(&self) -> bool {
        self.get_classification().is_a_virtual()
    }

    fn update_classification(&mut self) { /* NOP */
    }
}

impl AvailableDeviceInfoIface for AvailableHIDDeviceInfo {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn get_classification(&self) -> DeviceClassification {
        self.classification
    }
}

impl OpenedDeviceInfoIface for OpenedDeviceInfo<AvailableHIDDeviceInfo> {
    fn get_opened_device_id(&self) -> ObjId {
        self.opened_device_id
    }

    fn get_available_device_info(&self) -> &impl AvailableDeviceInfoIface {
        &self.available_device_info
    }
}

impl DeviceManagerCommon for HidManager {
    type AvailableDeviceInfoT = AvailableHIDDeviceInfo;
    type DeviceCfgT = HidDeviceCfg;
    type DeviceKindFilterT = BitFlags<DeviceKind>;
    type DeviceEventT = HidDeviceEvent;
    type OpenedDeviceInfoT = OpenedDeviceInfo<Self::AvailableDeviceInfoT>;
    fn open_device(
        &self,
        device_info: &Self::AvailableDeviceInfoT,
        device_matcher_key: &str,
        device_cfg: &HidDeviceCfg,
    ) -> Result<OpenedDeviceInfo<Self::AvailableDeviceInfoT>> {
        let mut devices = self.device_key_to_devices.borrow_mut();

        let opened_device: Option<Rc<UncheckedRefCell<HidDevice>>> = {
            let mut out = None;
            for v in &*devices {
                for v in v.1 {
                    if v.0.borrow().get_name() == device_info.name
                        && v.0.borrow().get_filesystem_path() == device_info.path
                    {
                        out = Some(Rc::clone(&v.0));
                        break;
                    }
                }
            }
            out
        };

        if let Some(opened_device) = opened_device {
            log::info!(
                "Device {} (matched config key {device_matcher_key}) already opened.",
                device_info.name
            );

            if opened_device
                .borrow()
                .get_classification()
                .intersects(device_cfg.get_classification())
            {
                devices
                    .entry(device_matcher_key.to_string())
                    .or_default()
                    .push((Rc::clone(&opened_device), device_cfg.clone()));
            }

            Ok(OpenedDeviceInfo {
                opened_device_id: opened_device.borrow().get_id(),
                available_device_info: device_info.clone(),
            })
        } else if let Ok(mut d) = HidDevice::open_from_path(device_info.path.to_str().unwrap(), None, self.debug) {
            log::info!(
                "Opened device {} (matched config key {})",
                device_info.name,
                device_matcher_key
            );
            let id = d.get_id();
            d.attach_events_listener(Some(self.per_device_event_notification_tx.clone()));
            devices
                .entry(d.get_cfg_key().to_string())
                .or_default()
                .push((Rc::new(UncheckedRefCell::new(d)), device_cfg.clone()));
            Ok(OpenedDeviceInfo {
                opened_device_id: id,
                available_device_info: device_info.clone(),
            })
        } else {
            bail!("Can't open device {device_info:?}")
        }
    }

    async fn consume_any_opened_device_event(&self) -> Option<Self::DeviceEventT> {
        poll_fn(|cx| self.all_devices_rx.borrow_mut().poll_recv(cx)).await
    }

    fn enumerate_available_devices(&self, filter: Option<Self::DeviceKindFilterT>) -> Vec<Self::AvailableDeviceInfoT> {
        let Ok(rd) = std::fs::read_dir("/dev/input").inspect_err(|e| log::error!("{e}")) else {
            return Vec::new();
        };

        rd.filter_map(|entry| {
            let path = entry.inspect_err(|e| log::error!("{e:?}")).ok()?.path();
            let name = path.file_name()?;

            if !name.to_string_lossy().starts_with("event") {
                return None;
            }

            let device = evdev::Device::open(&path).inspect_err(|e| log::error!("{e}")).ok()?;

            let device_kind = device.get_classification();

            if let Some(filter) = filter
                && !filter.intersects(device_kind)
            {
                return None;
            }

            if device_kind.is_empty() {
                return None;
            }

            Some(AvailableHIDDeviceInfo {
                name: device.name().unwrap_or("Unnamed device.").to_string(),
                path,
                classification: device.get_classification(),
            })
        })
        .collect()
    }

    fn stop(&self, full_shutdown: bool) -> Result<()> {
        for (device_key, devices) in &mut *self.device_key_to_devices.borrow_mut() {
            devices.retain(|d| {
                if !full_shutdown && d.0.borrow().is_persistent() {
                    log::info!("Keeping persistent virtual HID device: {}", device_key);
                    return true;
                }
                if let Err(e) = d.0.borrow().close() {
                    log::error!(
                        "Error while closing HID device {}, cfg key: {}: {e}",
                        device_key,
                        d.0.borrow().get_name()
                    );
                }
                false
            });
        }
        if full_shutdown {
            log::info!("All HID devices closed.");
        }
        Ok(())
    }

    async fn device_monitor(
        &self,
        match_name_regex: &regex::Regex,
        filter: Option<Self::DeviceKindFilterT>,
    ) -> anyhow::Result<()> {
        let devices = self.enumerate_available_devices(filter);
        let matched = devices
            .iter()
            .filter(|d| match_name_regex.is_match(&d.name))
            .collect::<Vec<_>>();

        if matched.is_empty() {
            bail!("No devices found matching '{}'", match_name_regex);
        }

        println!(
            "Monitoring HID devices matchig kind {} and name pattern {match_name_regex}",
            filter.unwrap_or_default()
        );

        for device_info in matched {
            println!("  - {} @ {}", device_info.name, device_info.path.display());
            self.open_device(&device_info.clone(), &device_info.name, &Default::default())?;
        }

        println!("Press Ctrl+C to stop monitoring...");

        loop {
            if let Some(DeviceEvent {
                device_id,
                data: control_state,
            }) = self.consume_any_opened_device_event().await
            {
                log::info!(
                    "[{}, {}] {} = {}",
                    get_interned_str(*device_id).unwrap_or_default(),
                    device_id,
                    control_state.control_type,
                    control_state.value
                );
            }
        }
    }

    fn set_control_matcher_and_broadcast(&self, device_key: &str, ctl_key: &str, value: BaseNumT, silent: bool) {
        self.set_control_matcher_and_broadcast(device_key, ctl_key, value, silent);
    }
}

impl DeviceManagerWithFfb for HidManager {
    fn ff_set_x_axis_pos(&self, device_key: &str, ctl_key: &str, control_interval: NumInterval<BaseNumT>) {
        self.ff_set_x_axis_pos(device_key, ctl_key, control_interval);
    }

    fn ff_set_y_axis_pos(&self, device_key: &str, ctl_key: &str, control_interval: NumInterval<BaseNumT>) {
        self.ff_set_y_axis_pos(device_key, ctl_key, control_interval);
    }

    fn ff_get_x_sum_symm_norm(&self, device_key: &str) -> BaseNumT {
        self.ff_get_x_sum_symm_norm(device_key)
    }

    fn ff_get_y_sum_symm_norm(&self, device_key: &str) -> BaseNumT {
        self.ff_get_y_sum_symm_norm(device_key)
    }
}
