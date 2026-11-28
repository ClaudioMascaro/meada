# Backstage npm plugin registration

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Identifies packages loaded as plugins in a Backstage backend or as features in a test backend.

## Interface

`added_plugin` returns the package passed to `backend.add(import(...))`. `test_features` extracts
imports from the `features` list passed to `startTestBackend`.
[backstage.rs:added_plugin](./backstage.rs) [backstage.rs:test_features](./backstage.rs)

## Invariants

The backend must have one constant origin bound to `createBackend` from the expected package. Only
a literal `import()` identifies the plugin.
[backstage.rs:made_by](./backstage.rs) [backstage.rs:added_plugin](./backstage.rs)

`added_plugin` requires a plain receiver name for `.add` and exactly one argument. It skips
`app.backend.add(...)` and calls with extra arguments.
[backstage.rs:added_plugin](./backstage.rs)

## Side effects

The readers inspect indexed imports and arguments and return the names found in memory.
[backstage.rs:added_plugin](./backstage.rs) [backstage.rs:test_features](./backstage.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- package::npm packages::
```
