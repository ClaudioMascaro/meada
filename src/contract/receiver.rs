use serde::{Deserialize, Serialize};

use crate::names::Argument;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReceiverIdentity {
    Sdk {
        kind: String,
        parts: Vec<Argument>,
    },
    HttpBase {
        strips: Vec<crate::lang::SlashStrip>,
        suffix: String,
    },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EnvironmentScope {
    pub(crate) workload: String,
    pub(crate) root: String,
    pub(crate) container: String,
}

pub struct EnvironmentValue {
    pub(crate) scope: EnvironmentScope,
    pub(crate) value: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) reason: String,
}
