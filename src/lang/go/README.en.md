# Go file selection and values

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Determines which files and packages enter Go analysis and which values an expression supplies to
the graph.

## Interface

`builds` and `listed` filter files and directories. `Modules` resolves imports through `go.mod`,
including `replace`. `value` extracts literals, calls and string parts.
[build.rs:builds](./build.rs) [build.rs:listed](./build.rs) [modfile.rs:Modules](./modfile.rs) [values.rs:value](./values.rs)

`env_fields` records the environment variable named by an `env` or `envconfig` field tag,
including field paths in nested anonymous structs. `fmt.Sprintf` accepts a file-level string
constant as its format when that name remains visible.
[values.rs:env_fields](./values.rs) [values.rs:env_tag](./values.rs) [values.rs:format](./values.rs)

## Invariants

The target is always `linux/amd64` with `gc`. An unreadable constraint excludes the file. `./...`
skips `testdata` and directories whose names start with `.` or `_`.
[build.rs:builds](./build.rs) [build.rs:listed](./build.rs)

A build constraint that reaches 64 levels is rejected. A deferred call that may write the sole
named result leaves its return value unknown.
[build.rs:CONSTRAINT_DEPTH](./build.rs) [../go.rs:deferred_write](../go.rs)

## Side effects

The readers build the module table from supplied configuration bytes and evaluate expressions from
the syntax tree.
[modfile.rs:Modules](./modfile.rs) [values.rs:value](./values.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::go gopkg:: uses:: base_url:: kafka_env::
```
