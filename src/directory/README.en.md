# SCIM directory

Directory of SCIM-provisioned users and groups, queried by the console for admission and owner selection.

## Interface

The SCIM interface handles users, groups and metadata under `/scim/v2/`. The console reader checks deprovisioning, group membership and counts. `[scim.rs:Scim]` `[scim.rs:answer]` `[read.rs:Directory]` `[read.rs:deprovisioned]` `[read.rs:counts]`

## Invariants

SCIM admission validates the provisioning credential. Request body and page sizes are bounded. `[scim.rs:admits]` `[scim.rs:SCIM_BODY_MAX]` `[scim.rs:SCIM_PAGE_MAX]`

Each change must leave the stored resource JSON within 64 KiB. Exceeding that limit returns HTTP 413 even for a small PATCH. `[scim.rs:candidate]` `[scim.rs:SCIM_RESOURCE_MAX]`

## Side Effects

Provisioning changes user, group and membership records in Postgres. Console reads open a connection to retrieve current state. `[scim.rs:answer]` `[read.rs:groups]`

## Verification

From the repository root, run `cargo test -p meada --features ui,oidc,directory --test it ui::oidc::directory::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
