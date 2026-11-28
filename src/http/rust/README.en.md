# Rust HTTP routes and libraries

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Recognizes axum routes and describes the Rust constructors and method chains accepted during HTTP
extraction.

## Interface

`providers` follows `Router::route` and the methods that bind verbs to handlers. The `FRAMEWORKS`
and `CLIENT_LIBRARIES` tables identify server objects and reqwest/ureq clients.
[axum.rs:providers](./axum.rs) [frameworks.rs:FRAMEWORKS](./frameworks.rs) [frameworks.rs:CLIENT_LIBRARIES](./frameworks.rs)

## Invariants

A chain preserves receiver identity only through methods registered for the originating object.
[frameworks.rs:passes_on](./frameworks.rs)

The axum reader limits the method chain used to identify a route to 64 steps.
[axum.rs:METHOD_CHAIN](./axum.rs)

## Side effects

Extraction assembles the routes and their handlers in memory from code facts.
[axum.rs:providers](./axum.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http::rust urls::
```
