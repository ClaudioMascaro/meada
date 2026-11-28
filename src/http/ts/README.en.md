# JavaScript and TypeScript routes and consumers

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Relates `fetch` calls to routes and to Backstage plugin and proxy configuration.

## Interface

`consumers` reads URLs, methods and response fields. `plugins` and `discovered` identify plugin
mounts and discovery. `Config` resolves configuration reads and proxy targets.
[clients.rs:consumers](./clients.rs) [backstage.rs:plugins](./backstage.rs) [backstage.rs:discovered](./backstage.rs) [config.rs:Config](./config.rs)

`Config` reads each distinct base from `app-config*.yaml` and its includes. A dynamic object key
can yield multiple destinations. Expanding `fetch` preserves the request site when a helper method
returns the discovery base. A `pluginId` on `createBackendPlugin` or `createBackendModule` can
also come from a constant in the same file.
[config.rs:decided](./config.rs) [backstage.rs:discovered](./backstage.rs) [backstage.rs:plugins](./backstage.rs) [clients.rs:consumers](./clients.rs)

`Config::literals` returns the distinct values of a key in deterministic order. `whole` reports
whether all roots and includes were read. A value consisting entirely of `${VAR}` retains its
variable name for base resolution against the workload configuration.
[config.rs:literals](./config.rs) [config.rs:whole](./config.rs) [config.rs:configured](./config.rs)

- [Backstage configuration includes](./config/README.en.md)

## Invariants

`fetch` counts only where the language pack binds it globally, including parameters defaulting to
`fetch`. A plugin with a literal ID has the mount `/api/<id>`.
[clients.rs:consumers](./clients.rs) [backstage.rs:mount](./backstage.rs)

Multiple bases, incomplete configuration or a dynamic record selection cap links at `inferred`. If
a proxy target cannot be read, the consumer retains the endpoint name and marks its target as
unread.
[config.rs:decided](./config.rs) [config.rs:configured](./config.rs) [clients.rs:consumers](./clients.rs) [backstage.rs:proxied](./backstage.rs)

## Side effects

Configuration is parsed on demand. Includes that no root followed become records carrying the
reason.
[config.rs:Config](./config.rs) [config.rs:unfollowed](./config.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- http::ts urls:: http_bases::
```
