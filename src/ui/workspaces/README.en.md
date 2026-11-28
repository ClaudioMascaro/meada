# Workspaces and proposals in the console

Console operations for named workspaces and the proposal server for agents.

## Interface

The console offers version and history reads, effective configuration, previews and lifecycle changes. With the `proposals` feature the MCP server exposes catalog, discovery, lookup and proposal tools. `[effective.rs:configuration]` `[effective.rs:preview]` `[lifecycle.rs:operate]` `[lifecycle.rs:inspect]` `[proposals.rs:listen]`

## Invariants

The session determines the identity used for authorization. Reads filter entries by the user’s grant and a configuration preview does not save a version. `[lifecycle.rs:readable_entries]` `[effective.rs:preview]`

The proposal MCP server returns HTTP 413 for a request body larger than 80 KiB. `[proposals.rs:BODY_MAX]`

## Side Effects

Lifecycle operations delegate writes to workspace storage. The proposal server opens an HTTP listener and evaluates proposals without persisting them. `[lifecycle.rs:operate]` `[proposals.rs:listen]`

## Verification

From the repository root, run `cargo test -p meada --features cloud --test it ui::oidc::workspaces::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
