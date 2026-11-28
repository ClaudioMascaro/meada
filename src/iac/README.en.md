# Infrastructure as code

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Turns infrastructure declarations into nodes and references that search, context and trace can
query.

## Interface

Readers evaluate Terraform/OpenTofu, Compose, CloudFormation/SAM, Serverless Framework and GitHub
Actions. The embedded table maps types to contracts. Queries use evaluated names to link
provisioners to uses.
[json.rs:read](./json.rs) [compose.rs:evaluate](./compose.rs) [cloudformation.rs:evaluate](./cloudformation.rs) [serverless.rs:evaluate](./serverless.rs) [actions.rs:evaluate](./actions.rs) [types.rs:entry](./types.rs) [provision.rs:links](./provision.rs)

Azure and GCP projections use declared resource types. Workload and subscription readers evaluate
references within the corresponding instance. Orphan reporting considers the exports visible to
the query.
[azure.rs:azure_projection](./azure.rs) [gcp.rs:gcp_projection](./gcp.rs) [workload.rs:workload](./workload.rs) [subscription.rs:prepare_subscriptions](./subscription.rs) [orphans.rs:Scope](./orphans.rs)

- [Reading GitHub Actions commands](./actions/README.en.md)
- [API Gateway REST routes](./http/README.en.md)
- [Mapped Terraform modules](./mapping/README.en.md)
- [ECS containers in Terraform](./workload/README.en.md)

## Invariants

Each Terraform root is evaluated in its own context. Registered pure functions use only their
arguments. Uses derived from configuration require complete, unredacted values to identify the
contract.
[../iac.rs:Eval](../iac.rs) [functions.rs:declare](./functions.rs) [config_uses.rs:read](./config_uses.rs)

A link from a use to its provisioner respects the confidence cap on an environment-derived use.
During CloudFormation evaluation, an intrinsic beyond 32 levels leaves that part of the name
unknown.
[provision.rs:directed_uses](./provision.rs) [../contract.rs:use_level](../contract.rs) [cloudformation.rs:INTRINSIC_DEPTH_MAX](./cloudformation.rs)

## Side effects

Refresh persists nodes and references in the index through a SQLite transaction. For Actions it
first removes the file’s previous rows. Provisioning links are derived during queries.
[../iac.rs:insert](../iac.rs) [actions.rs:refresh](./actions.rs) [provision.rs:links](./provision.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- iac:: custom_iac:: provision:: helm_env::
```
