# Console API operations

Console operations for reading platform state, applying changes and administering access and workspaces.

## Interface

Read handlers query the fleet, storage and versions. Other routes expose the catalog, directory, local sign-in and workspace operations when their modules are enabled. `[read.rs:operational]` `[catalog.rs:catalog]` `[directory.rs:directory_refusal]` `[accounts.rs:login]` `[workspaces.rs:workspaces]`

## Invariants

Operational reads have deadlines and a concurrency limit. With Postgres auditing, a validated write records pending status before changing Valkey or storage. `[read.rs:READERS_MAX]` `[read.rs:operational]` `[write.rs:pending]` `[write.rs:write]`

Local sign-in returns HTTP 413 when the request body exceeds 4 KiB. `[accounts.rs:LOGIN_MAX]`

## Side Effects

Routes use services enabled by the manifest, including Postgres, Valkey and object storage. Audited operations record the outcome of the attempt. `[read.rs:operational]` `[write.rs:operation]` `[write.rs:finish]`

## Components

- [Console account administration](./accounts/README.en.md)
- [Workspaces and proposals in the console](./workspaces/README.en.md)

## Verification

From the repository root, run `cargo test -p meada --features cloud --test it ui::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
