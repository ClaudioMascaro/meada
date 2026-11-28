# TypeScript and JavaScript references, receivers and uses

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

The pack records receiver origins, non-call references and result uses that reveal data shapes.

## Interface

`References` collects names used as types, values and JSX elements. `inferred` recognizes
`z.infer<typeof S>`. `Uses::of` follows what a function does with each call result.
[references.rs:References](./references.rs) [types.rs:inferred](./types.rs) [uses.rs:of](./uses.rs)

The pack records a call receiver’s class from typed parameters, local variables, instance fields
and declared return types. A `Promise<T>` supplies its inner type only after `await`, when
`Promise` denotes the global type.
[receivers.rs:local](./receivers.rs) [receivers.rs:definition](./receivers.rs) [receivers.rs:result](./receivers.rs)

## Invariants

Calls in a file share a usage budget. Once it is exhausted, later calls have empty uses. A generic
type hidden by a class, interface or alias remains unknown.
[uses.rs:USES_BUDGET](./uses.rs) [types.rs:reference](./types.rs)

Global constructors and namespaces such as `Map` and `JSON` count only where no declaration or
assignment shadows the name. The search for a type parameter that hides a reference checks up to
64 ancestors.
[globals.rs:constructor](./globals.rs) [globals.rs:namespace](./globals.rs) [globals.rs:Bindings](./globals.rs) [types.rs:HIDING_CLIMB](./types.rs)

## Side effects

Collection deduplicates references and caches uses by scope while the syntax tree is available.
[references.rs:push](./references.rs) [uses.rs:Uses](./uses.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::ts uses:: ts_receivers:: ts_library_receivers:: field_flow:: http_bases::
```
