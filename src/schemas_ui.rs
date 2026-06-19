use deserialize_untagged_verbose_error::DeserializeUntaggedVerboseError;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use traversable::Traversable;
use traversable::TraversableMut;

use crate::schemas_common::ObjId;
use crate::schemas_common::default_true;
use crate::schemas_value::DescriptionCfg;
use crate::schemas_value::DynValueRefs;

#[derive(Debug, Clone, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiCfg {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) monitors: Vec<UiMonitorsCfg>,
}

impl UiCfg {
    pub(crate) fn _is_default(&self) -> bool {
        self.monitors.is_empty()
    }
}

// -------------------------------------------------
#[derive(
    Debug, Clone, Serialize, DeserializeUntaggedVerboseError, TraversableMut, Traversable, JsonSchema, PartialEq,
)]
#[serde(untagged)]
pub(crate) enum UiMonitorsCfg {
    Axis(UiAxisMonitorCfg),
}

#[derive(Debug, Clone, Serialize, Deserialize, TraversableMut, Traversable, JsonSchema, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiAxisMonitorCfg {
    #[allow(unused)]
    #[serde(skip)]
    #[traverse(skip)]
    pub(crate) id: ObjId,
    // -----------------------
    #[serde(default)]
    #[traverse(skip)]
    pub(crate) name: String,
    #[serde(default)]
    #[traverse(skip)]
    pub(crate) desc: DescriptionCfg,
    #[serde(default = "default_true")]
    #[traverse(skip)]
    pub(crate) enabled: bool,
    pub(crate) position: Option<DynValueRefs>,
    pub(crate) hold: Option<DynValueRefs>,
}
