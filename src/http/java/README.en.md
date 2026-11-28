# JVM HTTP clients

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Recognizes imperative Java clients and declarative interfaces used to send requests.

## Interface

One reader follows RestTemplate, WebClient, `java.net.http` and OkHttp. The other records
OpenFeign and Retrofit methods at the interface definition, where the graph can reach the
consumer.
[clients.rs:consumers](./clients.rs) [declarative.rs:consumers](./declarative.rs)

## Invariants

Imports and types identify the framework. In Retrofit each `create` can supply a base. A relative
path with an unknown base stays open.
[clients.rs:consumers](./clients.rs) [declarative.rs:consumers](./declarative.rs)

For `@FeignClient`, a URL beginning with literal text without `://`, such as `localhost:8080`,
gets the `http://` prefix.
[declarative.rs:schemed](./declarative.rs)

## Side effects

The readers consult code facts and indexed Spring settings to assemble results in memory.
[clients.rs:consumers](./clients.rs) [declarative.rs:consumers](./declarative.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http::java jvmpkg::
```
