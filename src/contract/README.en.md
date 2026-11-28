# Contract identities and extraction

Physical identity rules and extraction mechanisms shared by contract kinds.

## Interface

The GCP and Azure normalizers turn supported names into identities used by configuration, IaC and SDKs. Shared extraction combines clients, operations and arguments. `[gcp.rs:pubsub]` `[azure.rs:servicebus]` `[sdk.rs:extract]`

A persisted receiver identity records either an SDK kind and its arguments or the slash removals and suffix of an HTTP base URL. `[receiver.rs:ReceiverIdentity]`

## Invariants

A Pub/Sub subscription identifies the subscription itself. When combining environment values, the scope identifies the workload, root and container. `[gcp.rs:pubsub]` `[receiver.rs:EnvironmentScope]`

ARM identifier normalization accepts 1 to 1,024 bytes. `[azure.rs:arm]` `[azure.rs:AZURE_IDENTIFIER_MAX]`

## Side Effects

Extractors consume code facts and return the sides found in memory. `[sdk.rs:extract]`

## Components

- [Azure SDK extraction](./azure/README.en.md)
- [Google Cloud SDK extraction](./gcp/README.en.md)
- [SDK client origins and request arguments](./sdk/README.en.md)

## Verification

From the repository root, run `cargo test -p meada --test it -- aws_sdk:: azure_sdk:: gcp_sdk::`.
