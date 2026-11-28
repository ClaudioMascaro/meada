# Entra ID groups from the directory

Resolves Entra ID groups when a token indicates group overage and the manifest selects the directory.

## Interface

The detector checks that the group claim is missing and is referenced in `_claim_names`. Lookup uses the user claim to retrieve the active user’s groups from the directory. `[overage.rs:in_overage]` `[overage.rs:groups]`

## Invariants

Lookup uses only the configured directory. A missing or inactive user receives zero groups and a failed lookup is not cached. `[overage.rs:ask]` `[overage.rs:groups]`

## Side Effects

Queries Postgres and caches successful results in memory for five minutes by default, including empty results. A failed lookup emits a diagnostic. `[overage.rs:groups]` `[overage.rs:OVERAGE_TTL]`

## Verification

From the repository root, run `cargo test -p meada --features oidc,ui,directory --test it oidc::overage::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
