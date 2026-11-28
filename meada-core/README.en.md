# meada-core

Deterministic library shared by the binary and the WebAssembly console.

## Components

- [meada-core · Rust](./src/README.en.md)

## Verification

From the repository root, run `cargo test -p meada-core`.

## Distribution

The library has its own version. For this release, binary 1.6.0 depends on library 2.0.0. The binary requires that exact version of `meada-core`. The library produces an `rlib` for the binary and a `cdylib` for WebAssembly, with `wasm-bindgen` enabled for the browser build. The development profile optimizes the `meada` package at level 1 to bound indexing memory use. `[../Cargo.toml:meada-core]` `[Cargo.toml:crate-type]`
