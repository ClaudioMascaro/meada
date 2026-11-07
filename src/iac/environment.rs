//! Stored workload environment entries. Secret values never enter these summaries.

use serde::{Deserialize, Serialize};

/// Bound reference expansion per workload, including overwritten entries.
// ponytail: 1 MiB exceeds ordinary container environments and stops envFrom fan-out.
pub(crate) const ENVIRONMENT_BYTES_MAX: usize = crate::lang::MAX_BYTES;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub container: String,
    pub image: Option<String>,
    pub name: String,
    pub target: String,
    pub value: Option<String>,
    /// A known resource target, without pretending to know its deployment-time URL or ARN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
}

impl Entry {
    pub(crate) fn size(&self) -> usize {
        self.container.len()
            + self.image.as_ref().map_or(0, String::len)
            + self.name.len()
            + self.target.len()
            + self.value.as_ref().map_or(0, String::len)
            + self.contract.as_ref().map_or(0, String::len)
    }
}

/// A cut value cannot identify a contract. Kubernetes expansion is not a literal either.
pub(crate) fn literal(value: &str) -> Option<String> {
    (value.len() <= crate::config_values::VALUE_MAX && !value.contains("$("))
        .then(|| value.to_string())
}
