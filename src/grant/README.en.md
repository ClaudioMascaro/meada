# GitHub access grants

Read grants derived from the latest GitHub permissions set in Postgres.

## Interface

The reader keeps permissions by login and returns the grant and set status for admission. With workspaces it also exposes the catalog available for selection. `[permissions.rs:Permissions]` `[permissions.rs:grant_and_state]` `[permissions.rs:catalog]`

## Invariants

Before the first valid read and when the set expires, the GitHub grant is empty. Age combines the Postgres clock at read time with elapsed local monotonic time. `[permissions.rs:grant_and_state]` `[permissions.rs:admission]`

By default the reader polls for new sets every 60 seconds. `max_age_seconds` determines how long the last set can grant access. `[permissions.rs:PERMISSIONS_POLL]` `[permissions.rs:grant_and_state]`

## Side Effects

Periodic refresh queries Postgres in a read transaction and replaces in-memory state. Admission reads that local state. `[permissions.rs:poll]` `[permissions.rs:grant_and_state]`

## Verification

From the repository root, run `cargo test -p meada --features workspaces --test it github_app::roles::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
