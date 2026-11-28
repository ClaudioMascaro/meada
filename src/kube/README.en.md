# Kubernetes objects and related resources

Kubernetes facts for Crossplane composition, custom resources, RabbitMQ topology, Ingress and container environments.

## Interface

The readers extract object facts, identify references and prepare physical names and targets for contract kinds to query. `[crossplane.rs:facts]` `[custom.rs:physical]` `[topology.rs:Prepared]` `[routing.rs:read]` `[environment.rs:resolve]`

## Invariants

Environment resolution looks up objects by namespace, kind and name in the supplied set. Ingress only assigns a Service to a literal `Prefix` path without rewrite or regex annotations. `[environment.rs:configs]` `[environment.rs:resolve]` `[routing.rs:read]`

A Crossplane composition contributes at most 64 composed resources. Additional resources are omitted from the index. `[crossplane.rs:COMPOSED_MAX]`

## Side Effects

Processing uses previously read objects and transforms environment references in memory when applying overlay prefixes, suffixes and namespace. `[environment.rs:transform]`

## Verification

From the repository root, run `cargo test -p meada --test it kubernetes::`.
