use enumflags2::BitFlags;
use enumflags2::bitflags;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::base_num::BaseNumT;

use crate::num_interval::NumInterval;
use crate::schemas_common::ObjId;
use std::path::Path;

// #[derive(Debug, Clone, PartialEq, Default)]
// pub(crate) enum _DeviceOwnershipStatus {
//     Owned,
//     #[default]
//     Unowned,
// }

// #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
// pub(crate) enum _DeviceVirtuality {
//     Virtual,
//     #[default]
//     Physical,
// }

#[bitflags]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) enum DeviceKind {
    Keyboard = 1,
    Joystick = 1 << 1,
    Gamepad = 1 << 2,
    Mouse = 1 << 3,
    MiscMappable = 1 << 4,
    Misc = 1 << 5,
    Virtual = 1 << 6,
}

pub(crate) trait WithDeviceClassification {
    fn get_classification(&self) -> BitFlags<DeviceKind>;
    fn update_classification(&mut self);
    fn is_a_joystick(&self) -> bool;
    #[allow(unused)]
    fn is_a_keyboard(&self) -> bool;
    #[allow(unused)]
    fn is_a_gamepad(&self) -> bool;
    fn is_a_mouse(&self) -> bool;
    fn is_a_virtual(&self) -> bool;
    #[allow(unused)]
    fn is_a_misc_mappable(&self) -> bool;
}

impl WithDeviceClassification for BitFlags<DeviceKind> {
    fn get_classification(&self) -> BitFlags<DeviceKind> {
        *self
    }

    fn is_a_joystick(&self) -> bool {
        self.contains(DeviceKind::Joystick)
    }

    fn is_a_keyboard(&self) -> bool {
        self.contains(DeviceKind::Keyboard)
    }

    fn is_a_gamepad(&self) -> bool {
        self.contains(DeviceKind::Gamepad)
    }

    fn is_a_mouse(&self) -> bool {
        self.contains(DeviceKind::Mouse)
    }

    fn is_a_misc_mappable(&self) -> bool {
        self.contains(DeviceKind::MiscMappable)
    }

    fn is_a_virtual(&self) -> bool {
        self.contains(DeviceKind::Virtual)
    }

    fn update_classification(&mut self) {}
}

pub(crate) type DeviceClassification = BitFlags<DeviceKind>;

#[allow(unused)]
#[derive(Debug, Clone)]
pub(crate) struct OpenedDeviceInfo<AvailableDeviceInfoT> {
    pub(crate) opened_device_id: ObjId,
    pub(crate) available_device_info: AvailableDeviceInfoT,
}

pub(crate) trait AvailableDeviceInfoIface {
    fn get_name(&self) -> &str;
    fn get_classification(&self) -> DeviceClassification;
}

pub(crate) trait OpenedDeviceInfoIface {
    fn get_opened_device_id(&self) -> ObjId;
    #[allow(unused)]
    fn get_available_device_info(&self) -> &impl AvailableDeviceInfoIface;
}

pub(crate) trait DeviceManagerCommon {
    type AvailableDeviceInfoT: AvailableDeviceInfoIface;
    type DeviceCfgT;
    type DeviceKindFilterT: From<enumflags2::BitFlags<DeviceKind, u8>>;
    type DeviceEventT;
    type OpenedDeviceInfoT: OpenedDeviceInfoIface;
    fn open_device(
        &self,
        device_info: &Self::AvailableDeviceInfoT,
        device_matcher_key: &str,
        device_cfg: &Self::DeviceCfgT,
    ) -> anyhow::Result<Self::OpenedDeviceInfoT>;
    // NB: https://github.com/leosat/MMVJ/issues/72
    async fn consume_any_opened_device_event(&self) -> Option<Self::DeviceEventT>;
    async fn device_monitor(
        &self,
        match_name_regex: &regex::Regex,
        filter: Option<Self::DeviceKindFilterT>,
    ) -> anyhow::Result<()>;
    fn enumerate_available_devices(&self, filter: Option<Self::DeviceKindFilterT>) -> Vec<Self::AvailableDeviceInfoT>;
    fn set_control_matcher_and_broadcast(&self, dev_key: &str, ctl_key: &str, value: BaseNumT, silent: bool);
    fn stop(&self, full_shutdown: bool) -> anyhow::Result<()>;
}

pub(crate) trait DeviceManagerWithFfb {
    fn ff_set_x_axis_pos(&self, dev_key: &str, ctl_key: &str, control_interval: NumInterval<BaseNumT>);
    fn ff_set_y_axis_pos(&self, dev_key: &str, ctl_key: &str, control_interval: NumInterval<BaseNumT>);
    fn ff_get_x_sum_symm_norm(&self, dev_key: &str) -> BaseNumT;
    fn ff_get_y_sum_symm_norm(&self, dev_key: &str) -> BaseNumT;
}

#[derive(Debug, Clone)]
pub(crate) struct DeviceEvent<T> {
    pub(crate) device_id: ObjId,
    pub(crate) data: T,
}

pub(crate) trait Device {
    type EventsListener;
    fn get_id(&self) -> ObjId;
    fn attach_events_listener(&mut self, listener: Option<Self::EventsListener>);
    fn is_owning(&self) -> bool;
    fn close(&self) -> anyhow::Result<()>;
    fn get_name(&self) -> &str;
    #[allow(unused)]
    fn get_filesystem_path(&self) -> &Path;
}
