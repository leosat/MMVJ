use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use deserialize_untagged_verbose_error::DeserializeUntaggedVerboseError;
use num_traits::Zero;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::base_num::BaseNumT;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ObjId(usize);

impl std::fmt::Debug for ObjId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // if let Some(str) = get_interned_str(self.0) {
        //     f.write_str(str)
        //         .inspect_err(|e| log::error!("Error while debug-printing ObjId {}", self.0));
        // }
        f.debug_tuple("ObjId").field(&self.0).finish()
    }
}

impl DerefMut for ObjId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ObjId {
    #[allow(unused)]
    pub(crate) fn invalid() -> Self {
        Self::from(usize::MAX)
    }
}

impl std::fmt::Display for ObjId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

impl Default for ObjId {
    fn default() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        if crate::config::MORE_DEBUG {
            dbg!(format!("New id: {}", COUNTER.load(Ordering::Relaxed)));
        }
        Self(COUNTER.fetch_add(1, Ordering::Relaxed) as usize)
    }
}

impl From<usize> for ObjId {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl Deref for ObjId {
    type Target = usize;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub(crate) struct CfgOpjectIdPath(Vec<ObjId>);

// ---------------------------------------
#[derive(Debug, Clone, JsonSchema)]
pub(crate) struct IdleTickEnabledFlag(Arc<AtomicBool>);
impl Default for IdleTickEnabledFlag {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::from(true)))
    }
}

impl Deref for IdleTickEnabledFlag {
    type Target = AtomicBool;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ---------------------------------------
pub(crate) fn is_none_or_default<T: Default + PartialEq>(opt: &Option<T>) -> bool {
    opt.as_ref().is_none_or(|val| val == &T::default())
}

pub(crate) fn is_false(b: &bool) -> bool {
    !b
}

pub(crate) fn is_true(b: &bool) -> bool {
    *b
}

pub(crate) fn is_zero(v: &BaseNumT) -> bool {
    v.is_zero()
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) const fn default_false() -> bool {
    false
}

pub(crate) const fn default_one() -> BaseNumT {
    1.0
}

pub(crate) trait MarkedAsFromPredefinedControl {
    fn set_from_predefined_control_marker(&mut self, v: String);
}

#[derive(JsonSchema, DeserializeUntaggedVerboseError)]
#[serde(untagged)]
enum DeviceControlCfgVariations<T> {
    FullSpec(T),
    PredefinedControlRef(String),
}
pub(crate) fn deserialize_device_controls<
    'de,
    D: Deserializer<'de>,
    T: MarkedAsFromPredefinedControl + Default + Deserialize<'de>,
>(
    deserializer: D,
) -> Result<BTreeMap<String, T>, D::Error> {
    let tmp = BTreeMap::deserialize(deserializer)?;
    let mut res: BTreeMap<String, T> = Default::default();
    for v in tmp {
        match v.1 {
            DeviceControlCfgVariations::FullSpec(cm) => {
                let _ = res.insert(v.0, cm);
            }
            DeviceControlCfgVariations::PredefinedControlRef(s) => {
                let mut cm = T::default();
                cm.set_from_predefined_control_marker(s);
                let _ = res.insert(v.0, cm);
            }
        }
    }

    Ok(res)
}

pub(crate) trait WithRuntimeId {
    fn get_id(&self) -> ObjId;
    fn assign_new_id(&mut self);
}
