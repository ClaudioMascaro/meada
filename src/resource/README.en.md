# Resources used by code or declared in configuration

Resource contracts from AWS SDK calls and locations declared in indexed configuration.

## Interface

The operation table gives the service, naming key and direction. boto3 and JS/TS readers identify the client and arguments. The declaration reader resolves configuration selectors to indexed sites. `[aws.rs:OPERATIONS]` `[aws.rs:extract]` `[python.rs:sides]` `[ts.rs:sides]` `[via.rs:read]`

Looking up a site shows the resources it provides or consumes, the matching readers or writers, and the evidence for each link. `[via.rs:context]`

## Invariants

In JS/TS the client and command must come from the same expected SDK package for the operation. In Python the client factory must match the service. `[ts.rs:sides]` `[python.rs:sides]`

When linking a reader to a writer, the link keeps the weaker evidence level of the two sides. A name obtained from the workload environment caps the link at `inferred` when the side carries no other cap. `[../resource.rs:link]` `[../contract.rs:environment_cap]`

## Side Effects

Declaration resolution queries SQLite and appends matching sites to the in-memory export. SDK analysis uses previously extracted facts. `[via.rs:read]` `[aws.rs:extract]`

## Verification

From the repository root, run `cargo test -p meada --test it resources::`.
