use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
