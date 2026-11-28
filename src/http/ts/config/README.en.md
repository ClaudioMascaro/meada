# Backstage configuration includes

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Expands `$include` and `$env` so URLs can be read from configuration files.

## Interface

`tree` takes root bytes, a repository file reader, watched paths and a shared budget. It returns
the tree and reasons for refused includes.
[includes.rs:tree](./includes.rs) [includes.rs:Reason](./includes.rs)

## Invariants

Only JSON or YAML files inside the repository can be included. Expansion rejects cycles and limits
depth to eight and includes to 64 per root.
[includes.rs:tree](./includes.rs) [includes.rs:DEPTH](./includes.rs) [includes.rs:INCLUDES](./includes.rs)

Expansion also charges the bytes grafted per root and across the shared read. Including the same
file twice consumes the budget twice.
[includes.rs:GRAFTED_MAX](./includes.rs) [includes.rs:READING_MAX](./includes.rs)

## Side effects

The reader requests files through the supplied callback and charges reads to the shared budget.
`$env` retains the variable name.
[includes.rs:tree](./includes.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http::ts::config urls:: http_bases::
```
