use enumflags2::BitFlags;

use crate::base_num::BaseNumT;
use crate::hid_device::HidDeviceKind;
use crate::mapped_controls::MappedCtls;
#[cfg(feature = "midi")]
use crate::midi::MappedMidiMessage;
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

#[allow(unused)]
#[derive(Debug, Clone)]
pub(crate) struct OpenedDeviceInfo<AvailableDeviceInfoT> {
    pub(crate) id: ObjId,
    pub(crate) info: AvailableDeviceInfoT,
}
pub(crate) trait MappedDeviceManager {
    type AvailableDeviceInfo;
    type DeviceCfg;
    type DeviceKindFilter;
    type DeviceEvent;
    type EventsListener;
    fn open(
        &self,
        device_info: Self::AvailableDeviceInfo,
        device_matcher_key: &str,
        device_cfg: &Self::DeviceCfg,
    ) -> anyhow::Result<OpenedDeviceInfo<Self::AvailableDeviceInfo>>;
    // NB/TODO: API: this consumes any message and that's it... which suggests one consumer.
    // NB/TODO: API: but if one consumer, why to keep the rx channel end, maybe just make an API
    // NB/TODO: API: to attach any external channel and not bother about serving rx end from here.
    // NB/TODO: API: this API is not mut for interior mutability, any impl. must be clever
    //          to keep borrows etc across await points...
    // https://github.com/leosat/MMVJ/issues/72
    async fn consume_any_opened_device_event(&self) -> Option<Self::DeviceEvent>;
    async fn monitor(&self, match_name_regex: &regex::Regex, filter: Self::DeviceKindFilter) -> anyhow::Result<()>;
    // TODO: https://github.com/leosat/MMVJ/issues/72
    fn _set_events_listenter(&self, tx: Self::EventsListener);
    fn enumerate_available_devices(&self, filter: Self::DeviceKindFilter) -> Vec<Self::AvailableDeviceInfo>;
    #[allow(unused)]
    fn _set_control_value(
        &self,
        device_key: &str,
        device_name: Option<&str>,
        ctl_key: &str,
        value: BaseNumT,
        silent: bool,
    ) {
        todo!()
    }
    #[allow(unused)]
    fn _get_control_value(&self, device_key: &str, device_name: Option<&str>, ctl_key: &str) -> BaseNumT {
        todo!()
    }
    fn stop(&self, full_shutdown: bool) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum MappedDeviceClassification {
    #[cfg(feature = "midi")]
    Midi,
    Hid(BitFlags<HidDeviceKind>),
    #[default]
    Unsupported,
}

#[derive(Debug, Clone)]
pub(crate) struct MappedHidEvent {
    pub(crate) control_type: MappedCtls,
    pub(crate) value: BaseNumT,
}

#[derive(Debug, Clone)]
pub(crate) enum MappedEvents {
    Hid(MappedHidEvent),
    #[cfg(feature = "midi")]
    _Midi(MappedMidiMessage),
}

#[derive(Debug, Clone)]
pub(crate) struct MappedDeviceEvent {
    pub(crate) device_id: ObjId,
    pub(crate) event: MappedEvents,
}

pub(crate) trait MappedDevice {
    type EventsListener;
    fn get_id(&self) -> ObjId;
    fn attach_events_listener(&mut self, listener: Option<Self::EventsListener>);
    fn is_owning(&self) -> bool;
    fn close(&self) -> anyhow::Result<()>;
    fn get_name(&self) -> &str;
    #[allow(unused)]
    fn get_filesystem_path(&self) -> &Path;
}
