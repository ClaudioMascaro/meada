# Console account administration

Console administration of local accounts and linking a verified GitHub login.

## Interface

The global operator lists, creates, changes or suspends accounts. Each person can link or remove their own GitHub login when the integration is configured. `[administration.rs:administer]` `[github.rs:verify_github]` `[github.rs:remove_own_github]`

## Invariants

GitHub identity comes from the response to `GET /user`. The administrative listing omits password hashes. `[github.rs:Verifier]` `[administration.rs:administer]`

Requests to create or change accounts are limited to 4 KiB and return HTTP 413 if they exceed it. `[administration.rs:ADMIN_BODY_MAX]`

## Side Effects

Verification calls the configured API with the supplied token. Changes update accounts in Postgres and pass through console auditing. `[github.rs:verify_github]` `[administration.rs:administer]`

## Verification

From the repository root, run `cargo test -p meada --features accounts,github-app --test it ui::accounts::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
