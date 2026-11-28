# Python and Rust HTTP clients

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Shares HTTP call recognition across requests, httpx, aiohttp and the Rust tables for reqwest and
ureq.

## Interface

`consumers` takes code facts, identifies module functions or client methods and expands wrappers
at their call sites.
[clients.rs:consumers](./clients.rs)

## Invariants

The receiver’s origin identifies the library. In aiohttp a path starting with `/` replaces the
base path even if the base is unknown. A relative reference that depends on that base stays open.
[clients.rs:consumers](./clients.rs)

## Side effects

The result is a vector of consumers assembled in memory from code facts.
[clients.rs:consumers](./clients.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http::python urls::
```
