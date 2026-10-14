//! Meada: an LLM-free code graph. The query engine (`engine`) is the seam the CLI, the MCP
//! server and the tests share.

mod config;
mod contract;
pub mod decide;
mod decider;
mod decisions;
pub mod engine;
mod http;
mod index;
mod items;
mod lang;
pub mod mcp;
pub mod msg;
mod render;
pub mod repo;
mod resolve;
mod resource;
mod trace;
mod workspace;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// `name` under the dotted `scope`: `outer.inner`, or `name` alone at the top.
fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}.{name}")
    }
}
