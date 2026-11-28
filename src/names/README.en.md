# External name identification

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Identifies libraries and constructors from the names that each language’s imports resolve.

## Interface

`imported` identifies a qualified Python import name. `factory` and `constructor_argument` recover
the external origin of a TypeScript client and its first constructed argument.
[python.rs:imported](./python.rs) [ts.rs:factory](./ts.rs) [ts.rs:constructor_argument](./ts.rs)

`factory_of` reads a TypeScript value’s construction in its original scope. `field_factory`
identifies the client held in a `private` or `#field` when all non-null assignments agree on one
factory. It follows a method call on another private field and stops at cycles.
[ts.rs:factory_of](./ts.rs) [ts.rs:field_factory](./ts.rs) [ts.rs:field_factory_in](./ts.rs)

- [Python constructor origins](./python/README.en.md)

## Invariants

A TypeScript factory must reach an external import through constant bindings. The argument is
examined in its original scope before expansion.
[ts.rs:factory](./ts.rs) [ts.rs:constructor_argument](./ts.rs)

## Side effects

Option extraction calls the supplied reader with recovered fields and returns in-memory results,
preserving call sites.
[ts.rs:constructor_options](./ts.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- names:: aws_sdk:: gcp_sdk:: azure_sdk:: kafkajs_classes::
```
