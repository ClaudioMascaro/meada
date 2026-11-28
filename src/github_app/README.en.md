# GitHub installation catalog

Discovery of candidate repository identities for the GitHub App role check.

## Interface

Given installations for configured owners and the existing selection, produces an ordered set of identities for the authorization stage. `[catalog.rs:candidates]`

## Invariants

Catalog membership only makes the repository a candidate. Pagination or lookup failures abort listing so a partial result cannot replace the previous set. `[catalog.rs:candidates]` `[catalog.rs:repositories]`

If the last permitted page is full, listing fails because more repositories may remain. `[catalog.rs:repositories]`

## Side Effects

Queries the installation repository API using its token and uses the client’s shared throttling marks. `[catalog.rs:repositories]` `[catalog.rs:candidates]`

## Verification

From the repository root, run `cargo test -p meada --features workspaces --test it github_app::roles::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
