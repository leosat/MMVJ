use crate::base_num::BaseAtomicT;
use crate::base_num::BaseNumT;
use crate::config::WithSelfSanitize;
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

use schemars::JsonSchema;
use traversable::{Traversable, TraversableMut};
// use serde_valid::Validate;

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
