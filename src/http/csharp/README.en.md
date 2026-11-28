# C# HTTP routes and clients

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Identifies HttpClient and Refit request senders and ASP.NET Core Minimal API routes.

## Interface

`providers` links `MapGroup` and `MapMethods` to routes. The two `consumers` readers distinguish
HttpClient calls from methods declared on Refit interfaces.
[minimal.rs:providers](./minimal.rs) [clients.rs:consumers](./clients.rs) [refit.rs:consumers](./refit.rs)

## Invariants

In HttpClient a path beginning with `/` replaces the base path. In Refit the annotated route
retains the base path. A partially unknown `MapMethods` verb list prevents registration.
[clients.rs:rebase](./clients.rs) [refit.rs:url](./refit.rs) [minimal.rs:methods](./minimal.rs)

When the Minimal API group graph exceeds 4,096 nodes, the reader omits its routes to avoid
publishing an incomplete set of prefixes.
[minimal.rs:MINIMAL_NODES](./minimal.rs)

## Side effects

`Reads` caches indexed settings in memory and loads values on demand. Reading an environment
variable preserves its name.
[reads.rs:Reads](./reads.rs) [reads.rs:read](./reads.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http::csharp nugetpkg::
```
