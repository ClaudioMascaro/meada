# Language adapters

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Each adapter turns source code into facts shared by resolution and contract extraction.

## Interface

`LanguagePack` defines per-file parsing, configuration, fact joining and module resolution.
`pack_for` selects by path. The registry includes JavaScript/TypeScript, Python, Rust, Go, Java,
Kotlin, C# and Swift.
[mod.rs:LanguagePack](./mod.rs) [mod.rs:pack_for](./mod.rs) [mod.rs:PACKS](./mod.rs)

The C# adapter joins project-wide `global using` directives before resolution. Java and Kotlin
read the JVM layout. Other adapters retain their own module discovery logic.
[csharp.rs:join](./csharp.rs) [java.rs:layout](./java.rs) [kotlin.rs:resolves_with](./kotlin.rs)

Go records named type uses in signatures and declarations. It skips predeclared types and type
parameters. The resolver drops Go references without a repository target.
[go.rs:type_uses](./go.rs) [go.rs:PREDECLARED](./go.rs) [go.rs:drops_external_uses](./go.rs) [../resolve.rs:reference](../resolve.rs)

`Facts` carries per-file receivers and their classes (`TsLocal`, `TsClass`), Go fields tied to
environment variables (`EnvField`) and observed assignments to private fields (`FieldWrites`).
Cross-file resolution and contract extraction use these facts.
[mod.rs:Facts](./mod.rs) [mod.rs:TsLocal](./mod.rs) [mod.rs:TsClass](./mod.rs) [mod.rs:EnvField](./mod.rs) [mod.rs:FieldWrites](./mod.rs)

- [Go file selection and values](./go/README.en.md)
- [Python project metadata](./python/README.en.md)
- [Rust modules, macros and values](./rust/README.en.md)
- [Swift modules and values](./swift/README.en.md)
- [TypeScript and JavaScript references, receivers and uses](./ts/README.en.md)

## Invariants

A pack receives file bytes and a repository-relative path. Configuration and facts from other
files come from the indexer. Kotlin shares Java’s resolution group.
[mod.rs:LanguagePack](./mod.rs) [mod.rs:resolver_of](./mod.rs) [kotlin.rs:resolves_with](./kotlin.rs)

The C# pack tracks up to 128 enclosing types when extracting nested definitions. Java and Kotlin
mark private methods so the resolver can identify calls that only run that body.
[csharp.rs:DEFINITION_DEPTH](./csharp.rs) [java.rs:parse](./java.rs) [kotlin.rs:parse](./kotlin.rs) [../resolve.rs:runs_alone](../resolve.rs)

## Side effects

Parsing produces `Facts`. `hash_spans` fills hashes from the supplied bytes. `with_values`
temporarily enables value capture on the thread and restores the state on exit.
[mod.rs:Facts](./mod.rs) [mod.rs:hash_spans](./mod.rs) [mod.rs:with_values](./mod.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- lang:: xlang:: uses:: ts_receivers:: ts_library_receivers:: field_flow:: http_bases::
```
