# SDK client origins and request arguments

Shared SDK client construction evidence and request argument extraction.

## Interface

Each service policy supplies rules to recognize the client. Language readers interpret C# initializers, Go composite literals, Java builders and map mutations. `[origins.rs:Policy]` `[csharp.rs:field_of]` `[go.rs:field_of]` `[java.rs:Builders]` `[mutations.rs:writes]`

## Invariants

All alternatives must yield the same evidence. An opaque alternative prevents confirmation of the origin. The budget limits new expansions and the reader can reuse previously computed evidence. `[origins.rs:Evidence]`

## Side Effects

Evidence and builder analysis results are kept in memory during extraction. `[origins.rs:Evidence]` `[java.rs:Builders]`

## Verification

From the repository root, run `cargo test -p meada --test it aws_sdk::`.
