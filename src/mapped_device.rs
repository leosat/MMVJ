use enumflags2::BitFlags;

use crate::common::BaseNumT;
use crate::hid_device::HidDeviceKind;
use crate::mapped_controls::MappedCtls;
#[cfg(feature = "midi")]
use crate::midi::MappedMidiMessage;
use crate::schemas_common::ObjId;
use std::path::Path;

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

pub(crate) trait MappedDeviceIface {
    fn get_id(&self) -> ObjId;
    fn set_external_notification(
        &mut self,
        external_notification_comm: Option<tokio::sync::mpsc::UnboundedSender<MappedDeviceEvent>>,
    );
    fn is_owning(&self) -> bool;
    fn close(&self) -> anyhow::Result<()>;
    fn get_name(&self) -> &str;
    #[allow(unused)]
    fn get_filesystem_path(&self) -> &Path;
}
