//! Private, persisted receiver evidence. Kinds interpret it; indexing and workload lookup
//! only carry the pieces and preserve their scope. Public exports keep the written operation.

use serde::{Deserialize, Serialize};

use crate::names::Argument;

/// A kind's receiver subtype and the name arguments folded in its exact construction frame.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ReceiverIdentity {
    pub(crate) kind: String,
    pub(crate) parts: Vec<Argument>,
}

/// The execution scope in which companion environment values may be combined.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EnvironmentScope {
    pub(crate) workload: String,
    pub(crate) root: String,
    pub(crate) container: String,
}

/// A certain, stored environment value or typed target, with its execution scope.
pub struct EnvironmentValue {
    pub(crate) scope: EnvironmentScope,
    pub(crate) value: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) reason: String,
}
