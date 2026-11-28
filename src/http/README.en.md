# HTTP contract extraction

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Brings served routes, consumer calls and paths published by infrastructure under the identity
`http::<METHOD>::<path>`.

## Interface

Language adapters return providers and consumers from code facts. The IaC reader declares routes
from evaluated infrastructure. `prepare` combines Services, Ingresses and workloads to explain
links.
[../http.rs:Http](../http.rs) [iac.rs:declare](./iac.rs) [infrastructure.rs:prepare](./infrastructure.rs)

Providers include Express and Next.js route files, FastAPI and Flask, Go routers, Spring
MVC/JAX-RS, Ktor, ASP.NET controllers, actix-web/Rocket and Vapor/Hummingbird.
[ts.rs:providers](./ts.rs) [python.rs:providers](./python.rs) [go.rs:providers](./go.rs) [java.rs:providers](./java.rs) [kotlin.rs:providers](./kotlin.rs) [csharp.rs:providers](./csharp.rs) [rust.rs:providers](./rust.rs) [swift.rs:providers](./swift.rs)

- [C# HTTP routes and clients](./csharp/README.en.md)
- [Go HTTP clients](./go/README.en.md)
- [JVM HTTP clients](./java/README.en.md)
- [Ktor HTTP clients](./kotlin/README.en.md)
- [Python and Rust HTTP clients](./python/README.en.md)
- [Rust HTTP routes and libraries](./rust/README.en.md)
- [Swift HTTP routes and clients](./swift/README.en.md)
- [JavaScript and TypeScript routes and consumers](./ts/README.en.md)

## Invariants

Identity uses the method and normalized path. Relative URLs use RFC 3986 reference resolution for
clients that adopt it. Infrastructure links depend on evaluated routing information.
[../http.rs:contract](../http.rs) [join.rs:resolve](./join.rs) [infrastructure.rs:prepare](./infrastructure.rs)

A `[[base_url]]` variable restricts providers to the declared directory in the same repository.
For a relative URL, if the consumer’s package serves the route, only providers in that package
remain. A link that depends on a gateway rewrite is at most `inferred`.
[../http.rs:link](../http.rs) [../http.rs:under](../http.rs)

If the serialized IaC route list exceeds the file’s budget, the reader discards the entire
summary. For URLs supplied by a workload environment, preparation reapplies any explicit
trailing-slash removal before concatenating the suffix written by the client.
[iac.rs:IAC_HTTP_STORED_MAX](./iac.rs) [infrastructure.rs:workload_url](./infrastructure.rs)

## Side effects

Preparation builds links and their explanations in memory for the query.
[infrastructure.rs:Prepared](./infrastructure.rs) [infrastructure.rs:prepare](./infrastructure.rs)

`prepare` also replaces consumers with their workload-resolved variants, sorts and deduplicates
the results, and adds gateway routes to providers.
[infrastructure.rs:prepare](./infrastructure.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http:: http_iac:: urls:: test_routes:: base_url:: http_bases:: helm_env::
```
