//! Meada: an LLM-free code graph. The query engine (`engine`) is the seam the CLI, the MCP
//! server and the tests share.

mod channel;
mod community;
mod config;
mod contract;
pub mod decide;
mod decider;
mod decisions;
mod declared;
mod document;
mod drift_view;
pub mod engine;
mod entry;
mod http;
mod index;
#[doc(hidden)]
pub use index::{with_busy_timeout, with_reversed_walk};
mod items;
mod lang;
mod literals;
pub mod mcp;
mod modules;
pub mod msg;
mod package;
mod render;
pub mod repo;
mod resolve;
mod resource;
mod shape;
#[cfg(feature = "snapshot")]
pub mod snapshot;
mod trace;
#[cfg(feature = "valkey")]
mod valkey;
mod workspace;
mod zod;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// `name` under the dotted `scope`: `outer.inner`, or `name` alone at the top.
fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}.{name}")
    }
}

/// The scope enclosing the dotted `scope`, empty at the top: `outer` for `outer.inner`.
fn parent(scope: &str) -> &str {
    scope.rsplit_once('.').map_or("", |(parent, _)| parent)
}
