//! Meada: an LLM-free code graph. The query engine (`engine`) is the seam the CLI, the MCP
//! server and the tests share.

pub mod engine;
mod index;
pub mod repo;
mod ts;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
