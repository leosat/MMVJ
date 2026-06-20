use crate::hid_device::HidDeviceKind;
use crate::mapped_device::MappedDeviceEvent;
use crate::schemas_common::ObjId;
use crate::schemas_hid::HidDeviceCfg;
use crate::schemas_mapping::Mapping;
use crate::{num_interval::NumInterval, schemas_cfg::Config};
use atomic_float::AtomicF32;
use crossbeam_utils::CachePadded;
use enumflags2::BitFlags;
use num_traits::{Float, Num, ToPrimitive};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, LazyLock};

// ================================================
cfg_if::cfg_if! {
    if #[cfg(feature="base_num_f64")] {
        pub(crate) type BaseNumT = f64;
        pub(crate) type BaseAtomicT = atomic_float::AtomicF64;
        pub(crate) use std::f64::consts as BaseNumConsts;
    } else {
        pub(crate) type BaseNumT = f32;
        pub(crate) type BaseAtomicT = atomic_float::AtomicF32;
        pub(crate) use std::f32::consts as BaseNumConsts;
    }
    // NB: supporting e.g. fixed-point will require more changes than just switching types.
}

// ================================================

use lasso::{Key, ThreadedRodeo};
static INTERNER: LazyLock<Arc<ThreadedRodeo>> = LazyLock::new(|| {
    let interner = Default::default();
    interner
});

pub(crate) fn get_interned_str(id: usize) -> Option<&'static str> {
    let key = lasso::Spur::try_from_usize(id).unwrap_or_default();
    if INTERNER.contains_key(&key) {
        Some(INTERNER.resolve(&key))
    } else {
        None
    }
}

#[allow(unused)]
pub(crate) fn intern_str(str: &str) -> usize {
    INTERNER.get_or_intern(str).into_usize()
}

//====================================================================================

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum _DeviceOwnershipStatus {
    Owned,
    #[default]
    Unowned,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub(crate) enum _DeviceVirtuality {
    Virtual,
    #[default]
    Physical,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub(crate) struct OpenedDeviceInfo<AvailableDeviceInfoT> {
    pub(crate) id: ObjId,
    pub(crate) info: AvailableDeviceInfoT,
}
pub(crate) trait DeviceManager {
    type AvailableDeviceInfo;
    type DeviceCfg;
    fn open(
        &self,
        device_info: Self::AvailableDeviceInfo,
        device_matcher_key: &str,
        device_cfg: &Self::DeviceCfg,
    ) -> anyhow::Result<OpenedDeviceInfo<Self::AvailableDeviceInfo>>;
    fn create_virtual_device(
        &self,
        device_key: &str,
        device_cfg: &HidDeviceCfg,
        is_persistent: bool,
    ) -> anyhow::Result<()>;
    fn destroy_virtual_device_if_exists(&self, device_key: &str);
    // NB/TODO: API: this consumes any message and that's it... which suggests one consumer.
    // NB/TODO: API: but if one consumer, why to keep the rx channel end, maybe just make an API
    // NB/TODO: API: to attach any external channel and not bother about serving rx end from here.
    // NB/TODO: API: this API is not mut for interior mutability, any impl. must be clever
    //          to keep borrows etc across await points...
    async fn consume_any_opened_device_event(&self) -> Option<MappedDeviceEvent>;
    async fn monitor(
        &self,
        match_name_regex: &regex::Regex,
        filter: Option<BitFlags<HidDeviceKind>>,
    ) -> anyhow::Result<()>;
    fn set_events_listenter(&self, tx: tokio::sync::mpsc::UnboundedSender<MappedDeviceEvent>);
    fn enumerate_available_devices(&self, filter: Option<BitFlags<HidDeviceKind>>) -> Vec<Self::AvailableDeviceInfo>;
    // fn set_control_value(&self, device_key: &str, ctl_key: &str, value: BaseNumericT, _silent: bool);
    // fn get_control_value(&self, device_key: &str, ctl_key: &str) -> BaseNumericT;
    fn stop(&self, full_shutdown: bool) -> anyhow::Result<()>;
}

//====================================================================================
type SharedStats = Arc<CachePadded<AtomicF32>>;
type SharedStats3 = (SharedStats, SharedStats, SharedStats);

#[allow(unused)]
#[derive(Debug, Default)]
pub(crate) struct SharedAtomicStateStats {
    pub(crate) mem_usage: SharedStats3,
    pub(crate) cpu_usage: SharedStats3,
    pub(crate) input_freq: SharedStats3,
    pub(crate) mapping_time_sec: SharedStats3,
}

#[allow(unused)]
#[derive(Debug, Default)]
pub(crate) struct Stats {
    median: SharedAtomicStateStats,
    mean: SharedAtomicStateStats,
}

// #[derive(Debug, Default)]
// pub(crate) struct SharedAtomicState {
//     pub(crate) _stats: Stats,
//     pub(crate) _vars: [SharedStats; 32],
//     pub(crate) map: papaya::HashMap<usize, AtomicF32>,
// }

// impl SharedAtomicState {
//     pub(crate) fn new() -> Self {
//         Self::default()
//     }
// }

//====================================================================================

#[derive(Clone, Debug, Serialize, Deserialize, Copy, PartialEq, JsonSchema, Default)]
pub(crate) enum Relativity {
    Rel,
    #[default]
    Abs,
}

impl From<Relativity> for bool {
    fn from(value: Relativity) -> Self {
        match value {
            Relativity::Rel => true,
            Relativity::Abs => false,
        }
    }
}

impl From<bool> for Relativity {
    fn from(value: bool) -> Self {
        match value {
            true => Relativity::Rel,
            false => Relativity::Abs,
        }
    }
}

//====================================================================================

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) enum MappingEngineCmd {
    _None,
    #[default]
    UpdateMappingRouterIdleTickOnly,
    UpdateMappingRouter,
    ResetScriptingCache,
}

#[derive(Debug, Clone)]
pub(crate) enum DriverCmd {
    ChangeConfigSimple {
        cfg: Config,
    },
    ChangeVirtualHids {
        cfg: Config,
        restart_persistent: bool,
        report_done_tx: std::sync::mpsc::Sender<()>,
    },
    ChangeMappings {
        mappings: Option<Vec<Mapping>>,
        action: MappingEngineCmd,
    },
    ChangeIdleTickRate {
        rate: u32,
    },
    #[cfg(feature = "gui")]
    StatusGuiClosed,
    #[allow(unused)]
    SaveCfg {
        cfg_file: Option<PathBuf>,
        cfg_suffix: Option<String>,
    },
    LoadCfg {
        cfg_file: PathBuf,
        resp_tx: Sender<Result<Config, String>>,
    },
    Reload,
    #[allow(unused)]
    ReloadWithInitialCfg,
    #[allow(unused)]
    Halt,
}

impl PartialEq for DriverCmd {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

//====================================================================================

pub(crate) const SYMM_UNIT_INTERVAL: NumInterval<BaseNumT> = crate::num_interval!(-1.0 as BaseNumT, 1.0 as BaseNumT);
pub(crate) const UNIT_INTERVAL: NumInterval<BaseNumT> = crate::num_interval!(0.0 as BaseNumT, 1.0 as BaseNumT);
#[allow(unused)]
pub(crate) const ZERO_INTERVAL: NumInterval<BaseNumT> = crate::num_interval!(0.0 as BaseNumT, 0.0 as BaseNumT);

//====================================================================================

#[allow(dead_code)]
pub trait MakeUnsigned {
    type Type: Num + PartialOrd;
}

macro_rules! impl_make_unsigned {
    ($($t:ty => $w:ty),* $(,)?) => {
        $(impl MakeUnsigned for $t {
            type Type = $w ;
        })*
    };
}

impl_make_unsigned! {
    i8 => u8,
    i16 => u16,
    i32 => u32,
    i64 => u64,
    i128 => u128,
    u8 => u8,
    u16 => u16,
    u32 => u32,
    u64 => u64,
    u128 => u128,
}

// ====================================================
// ====================================================
// ====================================================
// ====================================================
pub(crate) mod math_helpers {
    use super::*;

    #[allow(unused)]
    pub(crate) fn fp_round_to_n_decimal_places<T: Float>(value: T, places: i32) -> T {
        fp_round_to_n_places_of_base(value, places, 10)
    }

    #[allow(unused)]
    pub(crate) fn fp_round_to_n_places_of_base<T: Float, BaseT: ToPrimitive>(value: T, places: i32, base: BaseT) -> T {
        let factor = (T::from(base).unwrap()).powi(places);
        (value * factor).trunc() / factor
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn fp_round_test() {
            assert_eq!(fp_round_to_n_decimal_places(42.4242, 4), 42.4242);
            assert_eq!(fp_round_to_n_decimal_places(42.4242, 3), 42.424);
            assert_eq!(fp_round_to_n_decimal_places(42.4242, 2), 42.42);
            assert_eq!(fp_round_to_n_decimal_places(42.4242, 1), 42.4);
            assert_eq!(fp_round_to_n_decimal_places(42.4242, 0), 42.0);
        }
    }
}
