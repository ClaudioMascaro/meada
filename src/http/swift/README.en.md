# Swift HTTP routes and clients

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Reads Vapor and Hummingbird routes and Swift client calls.

## Interface

`providers` uses a dialect to read groups and handlers in both frameworks. `consumers` recognizes
URLSession, Alamofire and AsyncHTTPClient.
[vapor.rs:providers](./vapor.rs) [hummingbird.rs:HUMMINGBIRD](./hummingbird.rs) [client.rs:consumers](./client.rs)

## Invariants

Prefixes belong to the group registering the route. On the client side, receiver origin must
identify the library.
[vapor.rs:providers](./vapor.rs) [client.rs:consumers](./client.rs)

The search for a client’s origin follows at most eight levels of locals and constants.
[client.rs:CLIENT_DEPTH](./client.rs)

## Side effects

The readers traverse the available Swift facts and return routes and consumers in memory.
[vapor.rs:providers](./vapor.rs) [client.rs:consumers](./client.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http::swift swiftpkg::
```
