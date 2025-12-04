//-----------------------------------------------------------------
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum, Debug, Default)]
pub enum DebugLevel {
    #[default]
    Off,
    Low,
    Mid,
    Hi,
}

impl From<DebugLevel> for bool {
    fn from(value: DebugLevel) -> Self {
        value.is_on()
    }
}

impl From<bool> for DebugLevel {
    fn from(value: bool) -> Self {
        if value { Self::Low } else { Self::Off }
    }
}

impl DebugLevel {
    pub fn is_on(&self) -> bool {
        !matches!(self, Self::Off)
    }

    pub fn is_mid_or_above(&self) -> bool {
        matches!(self, Self::Hi | Self::Mid)
    }

    pub fn is_hi(&self) -> bool {
        matches!(self, Self::Hi)
    }
}

static mut DEBUG_LEVEL__: DebugLevel = DebugLevel::Off;

#[allow(unused)]
pub(crate) fn get_debug_level() -> DebugLevel {
    // SAFETY: ensure to run set_debug_level__ only once at initialization.
    unsafe { DEBUG_LEVEL__ }
}

pub(crate) fn set_debug_level__(debug: DebugLevel) {
    // SAFETY: ensure to run set_debug_level__ only once at initialization.
    unsafe { DEBUG_LEVEL__ = debug }
}
