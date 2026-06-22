use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use traversable::{Traversable, TraversableMut};

#[cfg(feature = "midi")]
use crate::schemas_midi::MidiControlMatcherCfg;
use crate::{
    common::{BaseNumT, Relativity},
    mapped_controls::MappedCtls,
    num_interval::NumInterval,
    schemas_common::WithRuntimeId,
    schemas_hid::HidControlMatcherCfg,
    schemas_value::{WithLastKnownIO, WithLastKnownIOSettable, WithNumericValue, WithNumericValueSettable},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TraversableMut, Traversable)]
#[serde(untagged)]
pub(crate) enum ControlMatchers {
    #[cfg(feature = "midi")]
    Midi(MidiControlMatcherCfg),
    Hid(HidControlMatcherCfg),
}

impl WithNumericValueSettable for ControlMatchers {
    type ValueT = BaseNumT;

    fn set_numeric_value(&mut self, v: Self::ValueT) {
        match self {
            ControlMatchers::Midi(m) => m.set_last_known_io(v),
            ControlMatchers::Hid(h) => h.set_numeric_value(v),
        }
    }
}

impl WithNumericValue for ControlMatchers {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(m) => m.get_last_known_io(),
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
    fn set_last_known_io(&self, val: BaseNumT) {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(m) => m
                .last_known_value
                .store(val as BaseNumT, std::sync::atomic::Ordering::Relaxed),
            ControlMatchers::Hid(h) => h
                .last_known_io_value
                .store(val as BaseNumT, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

#[cfg(feature = "midi")]
impl WithNumericValue for MidiControlMatcherCfg {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        self.get_last_known_io()
    }
}

impl WithNumericValue for HidControlMatcherCfg {
    type ValueT = BaseNumT;

    fn get_numeric_value(&self) -> Self::ValueT {
        self.current_value.load(Relaxed)
    }
}

impl WithNumericValueSettable for HidControlMatcherCfg {
    type ValueT = BaseNumT;

    fn set_numeric_value(&mut self, v: Self::ValueT) {
        self.current_value.store(v, Relaxed);
    }
}

#[cfg(feature = "midi")]
impl WithLastKnownIO<BaseNumT> for MidiControlMatcherCfg {
    fn get_last_known_io(&self) -> BaseNumT {
        self.last_known_value.load(std::sync::atomic::Ordering::Relaxed) as BaseNumT
    }
}

#[cfg(feature = "midi")]
impl WithLastKnownIOSettable<BaseNumT> for MidiControlMatcherCfg {
    fn set_last_known_io(&self, v: BaseNumT) {
        self.last_known_value.store(v, std::sync::atomic::Ordering::Relaxed);
    }
}

impl WithLastKnownIO<BaseNumT> for HidControlMatcherCfg {
    fn get_last_known_io(&self) -> BaseNumT {
        self.last_known_io_value.load(std::sync::atomic::Ordering::Relaxed) as BaseNumT
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

    #[allow(dead_code)]
    pub(crate) fn get_interval(&self) -> NumInterval<BaseNumT> {
        match &self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(cm) => cm.range,
            ControlMatchers::Hid(cm) => cm.range,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn get_type(&self) -> MappedCtls {
        match self {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(cm) => cm.midi_message.r#type.into(),
            ControlMatchers::Hid(cm) => cm.r#type,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn is_joystick_control(&self) -> bool {
        if let Self::Hid(..) = self {
            return true;
        }
        false
    }

    #[allow(dead_code)]
    pub(crate) fn is_mouse_control(&self) -> bool {
        if let Self::Hid(..) = self {
            return true;
        }
        false
    }

    #[allow(dead_code)]
    pub(crate) fn is_midi_control(&self) -> bool {
        #[cfg(feature = "midi")]
        if let Self::Midi(..) = self {
            return true;
        }
        false
    }
}
