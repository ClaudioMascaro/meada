# Mapped Terraform modules

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Interprets calls to known or repository-mapped modules as provisioned resources and workloads.

## Interface

`mapped_module` evaluates the input in the calling instance. `module_workload` uses the mapping to
select Lambda or ECS.
[modules.rs:mapped_module](./modules.rs) [modules.rs:module_workload](./modules.rs)

## Invariants

Creation conditions must allow the resource. A name generated from a prefix prevents
identification of the provisioned physical name.
[modules.rs:module_created](./modules.rs) [modules.rs:mapped_module](./modules.rs)

An omitted Boolean argument takes the module default. If an argument is present but cannot be
evaluated as a Boolean, the condition remains unknown.
[modules.rs:module_bool](./modules.rs)

## Side effects

The evaluator updates the node projection, its name or pattern and the sources that may override
it, within the evaluation budget.
[modules.rs:mapped_module](./modules.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- custom_iac::
```
