# API Gateway REST routes

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Associates public methods and integrations declared in Terraform with paths on an AWS REST API.

## Interface

`rest_routes` matches methods to AWS and AWS_PROXY integrations using previously evaluated blocks.
[rest.rs:rest_routes](./rest.rs)

## Invariants

The API, resource and public method must match. A path is dropped if an ancestor belongs to
another API, if there is a cycle or if a segment contains `/`.
[rest.rs:rest_identity](./rest.rs) [rest.rs:rest_path](./rest.rs) [rest.rs:rest_routes](./rest.rs)

The recursive lookup of a declared attribute also returns no result when it exceeds the evaluation
depth limit.
[rest.rs:declared_attribute](./rest.rs)

## Side effects

Accepted routes are added to evaluator output with their target and an indication of overridable
values.
[rest.rs:rest_routes](./rest.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- iac_http_routes:: http_iac::
```
