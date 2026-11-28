# Swift modules and values

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Relates Swift files to SwiftPM targets and extracts values written in expressions and calls.

## Interface

`targets` reads `Package.swift` with the Swift grammar. `value` interprets expressions and `fill`
records arguments and the receiver on a call.
[manifest.rs:targets](./manifest.rs) [values.rs:value](./values.rs) [values.rs:fill](./values.rs)

## Invariants

A target name must be literal. A computed or absolute explicit path, or one containing `..` as a
path segment, excludes the target. Declarations inside `dependencies` do not create targets.
[manifest.rs:targets](./manifest.rs)

Value extraction limits expression depth to 32. A deeper expression leaves the excess part
unknown.
[values.rs:VALUE_DEPTH](./values.rs)

## Side effects

The parser returns targets ordered by directory and name. Value extraction fills the call record
in memory.
[manifest.rs:targets](./manifest.rs) [values.rs:fill](./values.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::swift swiftpkg::
```
