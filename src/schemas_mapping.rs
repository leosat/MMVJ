use crate::base_num::BaseAtomicT;
use crate::base_num::BaseNumT;
use crate::config::WithSelfSanitize;
use crate::config::default_base_freq_hz;
use crate::schemas_common::EnabledFlagCfg;
use crate::schemas_common::ObjId;
use crate::schemas_common::WithRuntimeId;
use crate::schemas_transform::DynValFilter;
use crate::schemas_transform::TfmSeqCfg;
use crate::schemas_transform::TfmStepCfg;
use crate::schemas_transform::collect_dynamic_value_matchers;
use crate::schemas_value::AutoOrManual;
use crate::schemas_value::WithLastKnownIO;
use crate::schemas_value::WithLastKnownIOSettable;
use crate::schemas_value::WithNumInterval;
use crate::schemas_value::WithRelativity;
use crate::schemas_value::{ValueDsts, ValueSrcs};
use crossbeam_utils::CachePadded;
use garde::Validate;
use serde::{Deserialize, Serialize};
use std::ops::Not;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use strum_macros::EnumIter;
use strum_macros::EnumString;

use schemars::JsonSchema;
use traversable::{Traversable, TraversableMut};
// use serde_valid::Validate;

// ---------------------------------------------
#[derive(Copy, Clone, Debug, Serialize, Deserialize, EnumString, strum_macros::Display, JsonSchema)]
pub(crate) enum MapperModeKind {
    Reactive,
    Capped,
    Stable,
}

impl From<&MapperMode> for MapperModeKind {
    fn from(value: &MapperMode) -> Self {
        match value {
            MapperMode::Reactive { .. } => MapperModeKind::Reactive,
            MapperMode::Capped { .. } => MapperModeKind::Capped,
            MapperMode::Stable { .. } => MapperModeKind::Stable,
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum MapperModeSerdeHelper {
    Full {
        mode: MapperModeKind,
        #[serde(
            rename = "idle_rate",
            alias = "idle_tick_rate",
            default = "crate::config::default_base_freq_hz"
        )]
        idle_rate: u32,
        #[serde(alias = "mapping_cap_rate", default = "crate::config::default_base_freq_hz")]
        mapping_rate: u32,
    },
    Simple {
        #[serde(
            rename = "idle_rate",
            alias = "idle_tick_rate",
            default = "crate::config::default_base_freq_hz"
        )]
        idle_rate: u32,
    },
}

#[derive(
    Copy,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    Validate,
    PartialEq,
    JsonSchema,
    EnumString,
    strum_macros::Display,
    EnumIter,
)]
#[serde(from = "MapperModeSerdeHelper", into = "MapperModeSerdeHelper")]
pub(crate) enum MapperMode {
    Reactive {
        #[garde(range(min=crate::config::MIN_BASE_FREQ_HZ,max=crate::config::MAX_BASE_FREQ_HZ))]
        idle_rate: u32,
    },
    Capped {
        #[garde(range(min=crate::config::MIN_BASE_FREQ_HZ,max=crate::config::MAX_BASE_FREQ_HZ))]
        idle_rate: u32,
        #[garde(range(min=crate::config::MIN_BASE_FREQ_HZ,max=crate::config::MAX_BASE_FREQ_HZ))]
        mapping_rate: u32,
    },
    Stable {
        #[garde(range(min=crate::config::MIN_BASE_FREQ_HZ,max=crate::config::MAX_BASE_FREQ_HZ))]
        idle_rate: u32,
        #[garde(range(min=crate::config::MIN_BASE_FREQ_HZ,max=crate::config::MAX_BASE_FREQ_HZ))]
        mapping_rate: u32,
    },
}

impl From<MapperModeSerdeHelper> for MapperMode {
    fn from(value: MapperModeSerdeHelper) -> Self {
        let mut ret = match value {
            MapperModeSerdeHelper::Simple { idle_rate } => MapperMode::Reactive { idle_rate },
            MapperModeSerdeHelper::Full {
                mode,
                idle_rate,
                mapping_rate,
            } => match mode {
                MapperModeKind::Reactive => MapperMode::Reactive { idle_rate },
                MapperModeKind::Capped => MapperMode::Capped {
                    idle_rate,
                    mapping_rate,
                },
                MapperModeKind::Stable => MapperMode::Stable {
                    idle_rate,
                    mapping_rate,
                },
            },
        };
        ret.sanitize_inplace(());
        ret
    }
}

impl From<MapperMode> for MapperModeSerdeHelper {
    fn from(value: MapperMode) -> Self {
        match value {
            MapperMode::Reactive { idle_rate } => MapperModeSerdeHelper::Simple { idle_rate },
            MapperMode::Capped {
                idle_rate,
                mapping_rate,
            } => MapperModeSerdeHelper::Full {
                mode: MapperModeKind::Capped,
                idle_rate,
                mapping_rate,
            },
            MapperMode::Stable {
                idle_rate,
                mapping_rate,
            } => MapperModeSerdeHelper::Full {
                mode: MapperModeKind::Stable,
                idle_rate,
                mapping_rate,
            },
        }
    }
}

impl Default for MapperMode {
    fn default() -> Self {
        Self::Reactive {
            idle_rate: crate::config::DEFAULT_BASE_FREQ_HZ,
        }
    }
}

impl WithSelfSanitize for MapperMode {
    type SanInputT = ();
    fn sanitize_inplace(&mut self, _input: Self::SanInputT) {
        self.set_idle_rate_inner(
            self.get_idle_tick_rate()
                .clamp(crate::config::MIN_BASE_FREQ_HZ, crate::config::MAX_BASE_FREQ_HZ),
        );
        self.set_mapping_rate_inner(
            self.get_mapping_rate()
                .unwrap_or(crate::config::MAX_BASE_FREQ_HZ)
                .max(self.get_idle_tick_rate())
                .clamp(crate::config::MIN_BASE_FREQ_HZ, crate::config::MAX_BASE_FREQ_HZ),
        );
    }
}

impl MapperMode {
    pub(crate) fn make_reactive(&mut self) {
        *self = Self::Reactive {
            idle_rate: self.get_idle_tick_rate(),
        }
        .to_sanitized(());
    }
    pub(crate) fn make_capped(&mut self) {
        *self = Self::Capped {
            idle_rate: self.get_idle_tick_rate(),
            mapping_rate: self.get_mapping_rate().unwrap_or(default_base_freq_hz()),
        }
        .to_sanitized(());
    }
    pub(crate) fn make_stable(&mut self) {
        *self = Self::Stable {
            idle_rate: self.get_idle_tick_rate(),
            mapping_rate: self.get_mapping_rate().unwrap_or(default_base_freq_hz()),
        }
        .to_sanitized(());
    }
    pub(crate) fn calc_idle_tick_period(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(
            1.0 / (self.get_idle_tick_rate() as f64).max(crate::config::MIN_BASE_FREQ_HZ as f64),
        )
    }
    pub(crate) fn calc_mapping_tick_period(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(
            1.0 / (self
                .get_mapping_rate()
                .unwrap_or(crate::config::MAX_BASE_FREQ_HZ)
                .max(self.get_idle_tick_rate()) as f64),
        )
    }
    pub(crate) fn is_reactive(&self) -> bool {
        matches!(self, Self::Reactive { .. })
    }
    pub(crate) fn is_capped(&self) -> bool {
        matches!(self, Self::Capped { .. })
    }
    pub(crate) fn is_stable(&self) -> bool {
        matches!(self, Self::Stable { .. })
    }
    pub(crate) fn get_idle_tick_rate(&self) -> u32 {
        match self {
            MapperMode::Reactive { idle_rate } => *idle_rate,
            MapperMode::Capped { idle_rate, .. } => *idle_rate,
            MapperMode::Stable { idle_rate, .. } => *idle_rate,
        }
    }
    pub(crate) fn get_mapping_rate(&self) -> Option<u32> {
        match self {
            MapperMode::Reactive { .. } => None,
            MapperMode::Capped { mapping_rate, .. } => Some(*mapping_rate),
            MapperMode::Stable { mapping_rate, .. } => Some(*mapping_rate),
        }
    }
    fn set_idle_rate_inner(&mut self, new_rate: u32) -> &mut Self {
        match self {
            MapperMode::Reactive { idle_rate } => *idle_rate = new_rate,
            MapperMode::Capped { idle_rate, .. } => *idle_rate = new_rate,
            MapperMode::Stable { idle_rate, .. } => *idle_rate = new_rate,
        };
        self
    }
    pub(crate) fn set_idle_rate(&mut self, new_rate: u32) -> &mut Self {
        self.set_idle_rate_inner(new_rate);
        self.sanitize_inplace(());
        self
    }
    fn set_mapping_rate_inner(&mut self, new_rate: u32) {
        match self {
            MapperMode::Reactive { .. } => {}
            MapperMode::Capped { mapping_rate, .. } => *mapping_rate = new_rate,
            MapperMode::Stable { mapping_rate, .. } => *mapping_rate = new_rate,
        };
    }
    pub(crate) fn set_mapping_rate(&mut self, new_rate: u32) {
        self.set_mapping_rate_inner(new_rate);
        self.sanitize_inplace(());
    }
}

// -------------------------------------------------

impl std::fmt::Display for Mapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = format!("{} -> {}", self.src, self.dst,);
        f.write_str(&s)
    }
}

// -------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema, Validate)]
pub(crate) struct Mapping {
    #[serde(skip)]
    #[traverse(skip)]
    #[allow(unused)]
    #[garde(skip)]
    id: ObjId,
    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    last_in: Arc<CachePadded<BaseAtomicT>>,
    #[serde(skip)]
    #[traverse(skip)]
    #[garde(skip)]
    last_out: Arc<CachePadded<BaseAtomicT>>,
    // -----------------------
    #[serde(default)]
    #[garde(skip)]
    pub(crate) name: String,
    #[garde(skip)]
    #[traverse(skip)]
    #[serde(default, skip_serializing_if = "EnabledFlagCfg::is_default")]
    pub(crate) enabled: EnabledFlagCfg,
    #[serde(rename = "src", alias = "source")]
    #[garde(skip)]
    pub(crate) src: ValueSrcs,
    #[serde(rename = "dst", alias = "destination")]
    #[garde(skip)]
    pub(crate) dst: ValueDsts,
    #[serde(default, rename = "tfm", alias = "transformation")]
    #[garde(dive)]
    #[serde(skip_serializing_if = "TfmSeqCfg::skip_serializing")]
    pub(crate) transformation: TfmSeqCfg,
    #[serde(skip)]
    #[garde(skip)]
    pub(crate) requires_idle_tick: bool,
}

impl WithLastKnownIO for Mapping {
    type LastKnownIOValueT = (BaseNumT, BaseNumT);
    fn get_last_known_io(&self) -> Self::LastKnownIOValueT {
        (self.last_in.load(Relaxed), self.last_out.load(Relaxed))
    }
}

impl WithLastKnownIOSettable for Mapping {
    type LastKnownIOSettableValueT = (Option<BaseNumT>, Option<BaseNumT>);
    fn set_last_known_io(&self, v: Self::LastKnownIOSettableValueT) {
        v.0.inspect(|v| self.last_in.store(*v, Relaxed));
        v.1.inspect(|v| self.last_out.store(*v, Relaxed));
    }
}

impl Default for Mapping {
    fn default() -> Self {
        let mut m = Self {
            id: Default::default(),
            last_in: Default::default(),
            last_out: Default::default(),
            name: "New mapping".to_string(),
            enabled: Default::default(),
            src: Default::default(),
            dst: Default::default(),
            transformation: Default::default(),
            requires_idle_tick: Default::default(),
        };
        m.sanitize_inplace(());
        m
    }
}

impl WithSelfSanitize for Mapping {
    fn sanitize_inplace(&mut self, _input: Self::SanInputT) {
        if self.name.is_empty() {
            self.name = format!("{} -> {}", self.src, self.dst);
        }

        self.transformation
            .recompute_metadata_and_sanitize_recursive(Some(AutoOrManual::Auto(
                crate::schemas_value::InputValueMetadata {
                    interval: self.src.get_interval(),
                    relativity: self.src.get_relativity(),
                },
            )));

        self.requires_idle_tick = self.requires_idle_tick();
    }

    type SanInputT = ();
}

impl Mapping {
    pub(crate) fn _set_src(&mut self, src: ValueSrcs) {
        self.src = src;
        self.sanitize_inplace(());
    }

    pub(crate) fn _set_dst(&mut self, dst: ValueDsts) {
        self.dst = dst;
        self.sanitize_inplace(());
    }

    pub(crate) fn requires_idle_tick(&self) -> bool {
        self.transformation.steps.iter().any(|s| {
            matches!(
                s,
                TfmStepCfg::Steering { .. }
                    | TfmStepCfg::RaiseFall { .. }
                    | TfmStepCfg::Ema { .. }
                    | TfmStepCfg::OneEuro { .. }
                    | TfmStepCfg::Script { .. }
            )
        }) || collect_dynamic_value_matchers(self, |ctx| {
            ctx.contains(DynValFilter::Var) /*&& ctx.contains(DynValFilter::Src)*/
        })
        .is_empty()
        .not()
            || self.src.is_static()
    }
}

impl PartialEq for Mapping {
    fn eq(&self, other: &Self) -> bool {
        self.src == other.src && self.dst == other.dst
    }
}

impl Eq for Mapping {}

impl std::hash::Hash for Mapping {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.get_id().hash(state);
        // self.src.hash(state);
        // self.dst.hash(state);
    }
}

impl WithRuntimeId for Mapping {
    fn get_id(&self) -> ObjId {
        self.id
    }

    fn assign_new_id(&mut self) {
        self.id = Default::default()
    }
}
