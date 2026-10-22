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
#[cfg(feature = "github-app")]
pub mod github_app;
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
#[cfg(feature = "oidc")]
mod oidc;
mod package;
mod render;
pub mod repo;
mod resolve;
mod resource;
use meada_core::shape;
#[cfg(feature = "snapshot")]
pub mod snapshot;
mod trace;
#[cfg(feature = "ui")]
pub mod ui;
#[cfg(feature = "valkey")]
mod valkey;
mod workspace;
mod zod;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

use meada_core::{parent, qualify};
