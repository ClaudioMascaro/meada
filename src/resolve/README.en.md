# Call and value resolution

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

The resolver combines scope, receiver class and writes to find call targets and determine which
value is available at a read site.

## Interface

`value_within_at` evaluates a value at the read site. `member_changed` consults source writes and
additional mutations recognized by the library reader.
[mutations.rs:value_within_at](./mutations.rs) [mutations.rs:member_changed](./mutations.rs)

For TypeScript the resolver selects the declaration visible at the call site, identifies the
receiver class and checks for overrides in subclasses. A method call on a known global value has
no repository candidate. A library annotation can still admit object-literal methods, test doubles
and classes whose heritage names that type.
[ts.rs:ts_local](./ts.rs) [ts.rs:receiver_class](./ts.rs) [ts.rs:ts_override](./ts.rs) [ts.rs:ts_library_call](./ts.rs)

- [TypeScript fields and object methods](./ts/README.en.md)

## Invariants

A write after a read may affect the next iteration of the same loop. An incomplete index or
exhausted budget prevents using the field as a known value.
[mutations.rs:looped](./mutations.rs) [mutations.rs:changed](./mutations.rs)

When evaluating a Python return value, a decorator outside `PY_KEEPS_BODY` on the function, its
class or a subclass that could replace the method leaves the result unknown. On the JVM, an exact
call to a private method retains that body even if another class declares the same name.
[../resolve.rs:py_decorated](../resolve.rs) [../resolve.rs:PY_KEEPS_BODY](../resolve.rs) [../resolve.rs:runs_alone](../resolve.rs)

## Side effects

The first query builds an in-memory index for the mutation reader. Later reads reuse this index
and charge work per file.
[mutations.rs:member_changed](./mutations.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- resolve:: uses:: ts_receivers:: ts_library_receivers:: field_flow:: http_bases:: base_url:: xlang::
```
