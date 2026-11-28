# ECS containers in Terraform

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Extracts images and environment entries from ECS container definitions.

## Interface

`ecs_workload` reads `container_definitions` from `jsonencode` or evaluated JSON. `ecs_module`
reads the ECS module’s map keyed by container name.
[ecs.rs:ecs_workload](./ecs.rs) [ecs.rs:ecs_module](./ecs.rs)

## Invariants

An incomplete or over-budget environment invalidates the collected entries. Secrets retain only a
redacted reference. Images are independent of this invalidation.
[ecs.rs:unread](./ecs.rs) [ecs.rs:ecs_environment](./ecs.rs)

## Side effects

After charging the cost, the evaluator replaces the in-memory node’s images and environment
entries.
[ecs.rs:ecs_workload](./ecs.rs) [ecs.rs:ecs_module](./ecs.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- aws_workloads::
```
