# Google Cloud SDK calls by language

Google Cloud SDK forms in C#, Go, Java, Python and JS/TS for Pub/Sub, Cloud Storage and Secret Manager.

## Interface

Readers identify the client and return the role, target kind and naming argument. In Go, topics and subscriptions are identified from the Pub/Sub client. `[csharp.rs:read]` `[go.rs:step]` `[java.rs:read]` `[python.rs:read]` `[ts.rs:read]`

## Invariants

In Node, a subscription’s `on` method represents consumption only for the literal `message` event. Each language checks the receiver stage before assigning direction. `[ts.rs:read]` `[go.rs:read]`

## Side Effects

The readers query code facts in memory. `[java.rs:constructed]` `[python.rs:read]`

## Verification

From the repository root, run `cargo test -p meada --test it gcp_sdk::`.
