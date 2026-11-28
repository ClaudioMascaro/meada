# Workspace persistence and lifecycle

Named workspace persistence and administrative commands for creation, editing, applying versions and history.

## Interface

Console and CLI operations share creation and lifecycle transactions. Creation records the workspace, its first operator and initial version. Apply and rollback change the current version. `[store.rs:create]` `[lifecycle.rs:transact]` `[cli.rs:create]` `[cli.rs:apply]`

## Invariants

Creation checks eligibility against the current permissions set before recording SQL rows. Lifecycle changes revalidate the base version and authorization under a row lock. `[store.rs:eligible]` `[store.rs:create]` `[lifecycle.rs:transact]`

An operator change must leave the workspace with at least one operator and at most 20. `[lifecycle.rs:OPERATORS_MAX]` `[lifecycle.rs:transact]`

## Side Effects

State and history tables reside in Postgres. Immutable version documents use object storage. The CLI audits writes and reports the outcome on the terminal. `[store.rs:create]` `[store.rs:put_document]` `[lifecycle.rs:transact]` `[cli.rs:apply]`

## Verification

From the repository root, run `cargo test -p meada --features workspaces --test it ui::oidc::workspaces::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
