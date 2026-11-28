# OpenAPI and AsyncAPI documents

OpenAPI and AsyncAPI documents as sources of contracts and payload shapes, with a shared YAML and JSON event reader.

## Interface

The entry point takes document bytes and produces a summary of HTTP routes or channel sides. The event readers also expose scalar locations for other configuration readers. `[mod.rs:read]` `[openapi.rs:read]` `[asyncapi.rs:read]` `[events.rs:read_located]`

## Invariants

Schema references resolve within the document. Reading bounds the work and leaves parts of the shape open when they cannot be enumerated. `[schema.rs:read]` `[schema.rs:Work]`

The event reader rejects documents nested more than 128 levels deep, counting alias nodes at the depth where they are inserted. `[events.rs:MAX_NESTING]`

## Side Effects

Readers build results in memory from the bytes and deliver events to supplied callbacks. `[mod.rs:read]` `[events.rs:read]`

## Verification

From the repository root, run `cargo test -p meada --test it documents::`.
