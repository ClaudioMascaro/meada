# Packages in SwiftPM manifests

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Reads the portion of `Package.swift` that identifies the package, its library products and its
dependencies.

## Interface

`package` takes manifest bytes and returns a `Package` with products, library targets and
`Dependency` entries, including path or URL and version requirement.
[manifest.rs:package](./manifest.rs) [manifest.rs:Package](./manifest.rs) [manifest.rs:Dependency](./manifest.rs)

## Invariants

A dependency must specify exactly one of `url` and `path`. Computed parts are marked with `*` and
the `computed` flag.
[manifest.rs:package](./manifest.rs) [manifest.rs:Dependency](./manifest.rs)

## Side effects

The parser walks the local tree in source order and retains the first name found in
`Package(name:)`. The result stays in memory.
[manifest.rs:package](./manifest.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- package::swiftpm swiftpkg::
```
