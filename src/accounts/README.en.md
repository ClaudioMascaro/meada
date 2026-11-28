# Local accounts in Postgres

Persistence for local accounts used by sign-in, console administration and default account recovery.

## Interface

The sign-in attempt takes a hash verification function and returns a typed outcome. Creation, changes and status lookup use the username. `[store.rs:attempt]` `[store.rs:Attempt]` `[store.rs:create]` `[store.rs:change]` `[store.rs:standing]`

## Invariants

Verification and failure counting take place under the same row lock. An invalid, suspended or already locked row prevents sign-in. `[store.rs:attempt]`

## Side Effects

Reads and updates the `accounts` table in the configured Postgres schema. Administrative recovery resets the default account’s password. `[store.rs:attempt]` `[store.rs:reset_admin]`

After sign-in, the hash upgrade only writes if the row still holds the verified hash. A concurrent password change takes precedence. `[store.rs:rehash]`

## Verification

From the repository root, run `cargo test -p meada --features accounts --test it ui::accounts::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
