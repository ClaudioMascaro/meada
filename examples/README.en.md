# Maintainer examples

Two executables support export review and maintenance of the repository map.

## Interface

`golden_diff` compares exports by section and identity. `split_agents` splits a document at level-two headings. `[golden_diff.rs:diff_docs]` `[split_agents.rs:split]`

## Invariants

The splitter refuses to overwrite an existing part with different content. Comparison with `A...B` requires the common ancestor to be available in the clone. `[golden_diff.rs:resolve]` `[split_agents.rs:main]`

## Side effects

The comparison reads Git revisions. The splitter writes the index and section files. `[golden_diff.rs:git]` `[split_agents.rs:main]`

## Verification

From the repository root, run `cargo test --examples`.
