use deserialize_untagged_verbose_error::DeserializeUntaggedVerboseError;
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use traversable::{Traversable, TraversableMut};

use crate::relativity::Relativity;
#[cfg(feature = "midi")]
use crate::schemas_midi::MidiControlMatcherCfg;
use crate::{
    base_num::BaseNumT,
    mapped_controls::MappedCtls,
    num_interval::NumInterval,
    schemas_common::WithRuntimeId,
    schemas_hid::HidControlMatcherCfg,
    schemas_value::{WithLastKnownIO, WithLastKnownIOSettable, WithNumericValue, WithNumericValueSettable},
};

#[derive(Debug, Clone, Serialize, DeserializeUntaggedVerboseError, PartialEq, TraversableMut, Traversable)]
#[serde(untagged)]
pub(crate) enum ControlMatchers {
    #[cfg(feature = "midi")]
    Midi(MidiControlMatcherCfg),
    Hid(HidControlMatcherCfg),
}

impl WithNumericValueSettable for ControlMatchers {
    type ValueT = BaseNumT;

    fn set_numeric_value(&self, v: Self::ValueT) {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(m) => m.set_numeric_value(v),
            ControlMatchers::Hid(h) => h.set_numeric_value(v),
        }
    }
}

impl WithNumericValue for ControlMatchers {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(m) => m.get_numeric_value(),
            ControlMatchers::Hid(h) => h.get_numeric_value(),
        }
    }
}

impl WithLastKnownIO<BaseNumT> for ControlMatchers {
    fn get_last_known_io(&self) -> BaseNumT {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(m) => m.get_last_known_io(),
            ControlMatchers::Hid(h) => h.get_last_known_io(),
        }
    }
}

impl WithLastKnownIOSettable<BaseNumT> for ControlMatchers {
    fn set_last_known_io(&self, v: BaseNumT) {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(m) => m.set_last_known_io(v),
            ControlMatchers::Hid(h) => h.set_last_known_io(v),
        }
    }
}

impl WithRuntimeId for ControlMatchers {
    fn get_id(&self) -> crate::schemas_common::ObjId {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(cm) => cm.get_id(),
            ControlMatchers::Hid(cm) => cm.get_id(),
        }
    }

    fn assign_new_id(&mut self) {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(cm) => cm.assign_new_id(),
            ControlMatchers::Hid(cm) => cm.assign_new_id(),
        }
    }
}

impl ControlMatchers {
    #[allow(dead_code)]
    pub(crate) fn get_idle_tick_enabled_flag(&self) -> &AtomicBool {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(cm) => &cm.idle_tick_enabled,
            ControlMatchers::Hid(cm) => &cm.idle_tick_enabled,
        }
    }

    pub(crate) fn get_relativity(&self) -> Relativity {
        match &self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(_) => Relativity::Abs,
            ControlMatchers::Hid(cm) => cm.r#type.get_relativity(),
        }
    }

    pub(crate) fn get_interval(&self) -> NumInterval<BaseNumT> {
        match &self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(cm) => cm.range,
            ControlMatchers::Hid(cm) => cm.range,
        }
    }

    pub(crate) fn _get_type(&self) -> MappedCtls {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(cm) => cm.midi_message.r#type.into(),
            ControlMatchers::Hid(cm) => cm.r#type,
        }
    }
}
