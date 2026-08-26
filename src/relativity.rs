use std::fmt::Display;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Copy, PartialEq, JsonSchema, Default)]
pub(crate) enum Relativity {
    Rel,
    #[default]
    Abs,
}

impl Display for Relativity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Relativity::Rel => "Rel",
            Relativity::Abs => "Abs",
        })
    }
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
