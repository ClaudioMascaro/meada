# Ktor HTTP clients

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Extracts consumers from Kotlin calls to Ktor’s `HttpClient`.

## Interface

`consumers` identifies the imported verb, receiver origin and URL supplied by the caller. It also
considers the `defaultRequest` base.
[client.rs:consumers](./client.rs)

## Invariants

A `request` with no method assignment uses GET. A conditional assignment leaves the method
unknown. A URL defined only in the builder block stays unresolved.
[client.rs:consumers](./client.rs)

## Side effects

Injected values come from the module’s indexed configuration. The reader returns consumers in
memory.
[client.rs:consumers](./client.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http::kotlin kotlinpkg::
```
