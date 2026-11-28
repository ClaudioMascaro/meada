# Rust modules, macros and values

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Reconstructs the Cargo module tree and the syntax facts used to resolve values and references in
Rust code.

## Interface

Readers reconstruct modules and `cfg`, recognize item-preserving item macros, identify mentions
and extract values including strings built by successive appends.
[crates.rs:Tree](./crates.rs) [cfg.rs:holds](./cfg.rs) [macros.rs:bodies](./macros.rs) [mentions.rs:mentions](./mentions.rs) [values.rs:Reader](./values.rs) [builders.rs:builder](./builders.rs)

## Invariants

Configuration uses x86_64 Linux and the crate’s default features. Malformed or excessively deep
predicates are false. Module lookup uses only files present in the inventory.
[cfg.rs:holds](./cfg.rs) [cfg.rs:defaults](./cfg.rs) [crates.rs:Tree](./crates.rs)

String builders share a per-file work budget. Once it is exhausted, the remaining constructions
yield unknown values.
[builders.rs:BUILDER_BUDGET](./builders.rs) [builders.rs:builder](./builders.rs)

## Side effects

`record` adds the macro definitions and invocations relevant to a new analysis of items to the
file’s facts.
[macros.rs:record](./macros.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::rust cargo::
```
