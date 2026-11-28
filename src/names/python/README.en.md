# Python constructor origins

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Determines whether a Python call receiver can be attributed to an identifiable external
construction.

## Interface

`Factories::read` follows the receiver and returns a `Construction` with a name and optional
argument. It allows up to one external factory level.
[arguments.rs:Factories](./arguments.rs) [arguments.rs:read](./arguments.rs) [arguments.rs:Construction](./arguments.rs)

## Invariants

All alternatives must support the construction. An opaque branch or a cycle prevents attributing
the receiver to a known constructor from another branch.
[arguments.rs:values](./arguments.rs) [arguments.rs:read](./arguments.rs)

## Side effects

The reader caches results and charges work per query. On a file change it clears result caches and
resets the file budget.
[arguments.rs:read](./arguments.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- names::python aws_sdk::
```
