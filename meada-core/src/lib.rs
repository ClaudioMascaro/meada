//! The deterministic core of Meada (ADR 0014): the schema of a workspace manifest's entries and
//! the checks its text decides (`manifest`), the ids with their patterns (`ids`), workspace entry diffs (`workspace`), the export's
//! types (`export`), shapes and their drift (`shape`, `drift`), and the answers of `search`,
//! `context` and `trace` with the token budget they share (`search`, `context`, `trace`,
//! `render`). The `meada` binary and the console's front end, compiled to WebAssembly, share it.
//! No filesystem, process or network code and no async runtime: it builds for
//! `wasm32-unknown-unknown`.

pub mod context;
pub mod drift;
pub mod export;
pub mod http;
pub mod ids;
pub mod manifest;
pub mod msg;
pub mod render;
pub mod search;
pub mod shape;
pub mod trace;
#[cfg(feature = "wasm-bindgen")]
mod wasm;
pub mod workspace;

/// `name` under the dotted `scope`: `outer.inner`, or `name` alone at the top.
pub fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}.{name}")
    }
}

/// The scope enclosing the dotted `scope`, empty at the top: `outer` for `outer.inner`.
pub fn parent(scope: &str) -> &str {
    scope.rsplit_once('.').map_or("", |(parent, _)| parent)
}
