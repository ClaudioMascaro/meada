//! The integration tests, one module per subject (`docs/agents/map/tests.md`), in one binary so
//! `cargo test` links it once instead of once per subject.

mod common;

mod asyncapi;
mod cargo;
mod channel_queries;
mod channels;
mod channels_jobs;
mod cli;
mod community_queries;
mod conformance;
mod decide;
mod decider;
mod decisions;
mod docs;
mod documents;
mod drift;
mod freshness;
mod gateway;
mod generations;
mod gopkg;
mod hostile;
mod index;
mod items;
mod manifest;
mod mcp;
mod packages;
mod processes;
mod pypi;
mod query;
mod resources;
mod self_index;
mod server;
mod serving;
mod snapshot;
mod stale;
mod trace;
mod urls;
mod workspace;
mod xlang;

/// A file or directory here left out of the list above would never compile or run.
#[test]
fn every_file_here_is_a_module() {
    let main = std::fs::read_to_string("tests/it/main.rs").unwrap();
    for entry in std::fs::read_dir("tests/it").unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let module = name.strip_suffix(".rs").unwrap_or(&name);
        if module != "main" {
            assert!(main.contains(&format!("\nmod {module};\n")), "{name}");
        }
    }
}
