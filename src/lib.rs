//! Meada: an LLM-free code graph. The query engine (`engine`) is the seam the CLI, the MCP
//! server and the tests share.

#[cfg(any(feature = "ui", feature = "generation-pointer"))]
pub mod audit;
#[cfg(feature = "workspaces")]
mod catalog;
mod channel;
mod community;
mod config;
mod contract;
pub mod decide;
mod decider;
mod decisions;
mod declared;
#[cfg(feature = "directory")]
mod directory;
mod document;
mod dotnet;
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
mod jvm;
mod lang;
mod literals;
pub mod mcp;
mod modules;
pub mod msg;
mod names;
#[cfg(feature = "oidc")]
mod oidc;
mod package;
mod partial;
#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "proposals")]
mod proposals;
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
#[cfg(feature = "workspaces")]
pub mod workspaces;
mod xml;
mod zod;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

use meada_core::{parent, qualify};
