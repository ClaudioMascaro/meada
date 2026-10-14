//! Meada: an LLM-free code graph. The query engine (`engine`) is the seam the CLI, the MCP
//! server and the tests share.

pub mod engine;
mod index;
mod lang;
pub mod mcp;
mod render;
pub mod repo;
mod resolve;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
