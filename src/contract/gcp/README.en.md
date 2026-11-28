# Google Cloud SDK extraction

Converts Google Cloud client evidence into Pub/Sub, Cloud Storage and Secret Manager contracts.

## Interface

Extraction selects the language reader, follows the receiver and emits the side corresponding to the operation. Environment projection distinguishes topic and subscription names. `[sdk.rs:extract]` `[sdk.rs:environment_name]` `[sdk.rs:environment_target]`

## Invariants

Obtaining a client or topic handle does not establish a producer. The reader keeps the stage until the call that uses the target. `[sdk.rs:extract]`

## Side Effects

Produces an in-memory collection from previously analyzed files. `[sdk.rs:extract]`

## Components

- [Google Cloud SDK calls by language](./sdk/README.en.md)

## Verification

From the repository root, run `cargo test -p meada --test it gcp_sdk::`.
