//! Meada: an LLM-free code graph. The query engine (`engine`) is the seam the CLI, the MCP
//! server and the tests share.

mod config;
mod contract;
pub mod engine;
mod http;
mod index;
mod lang;
pub mod mcp;
pub mod msg;
mod render;
pub mod repo;
mod resolve;
mod workspace;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
