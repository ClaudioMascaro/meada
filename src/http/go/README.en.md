# Go HTTP clients

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Turns `net/http` and Resty calls into HTTP consumers.

## Interface

`consumers` takes code facts and returns the method and destination of each recognized call.
`is_package_send` identifies sends made by the package.
[clients.rs:consumers](./clients.rs) [clients.rs:is_package_send](./clients.rs)

## Invariants

The receiver needs an import, declared type or construction tying it to the library. A Resty base
URL follows its instance, including when one factory creates two clients.
[clients.rs:consumers](./clients.rs)

When resolving a client stored in a field, a production read excludes values supplied only by
tests. A read in a test file considers both. Origin tracing stops at 64 steps and can leave the
client unknown.
[clients.rs:held](./clients.rs) [clients.rs:CLIENT_DEPTH](./clients.rs) [../../resolve.rs:field_values](../../resolve.rs)

## Side effects

Extraction collects consumers in memory from calls and their bases.
[clients.rs:consumers](./clients.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http::go gopkg:: base_url::
```
