# TypeScript fields and object methods

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

The resolver identifies which field values and object-literal methods are still usable at a read
site.

## Interface

`ts_construction` resolves the class where `new` was written. `ts_literal_method` looks up an
object-literal method. `ts_field_argument` recovers the argument supplied to a field at an exact
construction, including through a factory. `ts_record` selects the property for a literal key or
collects possible values for a dynamic key.
[fields.rs:ts_construction](./fields.rs) [fields.rs:ts_literal_method](./fields.rs) [fields.rs:ts_field_argument](./fields.rs) [fields.rs:ts_record](./fields.rs)

## Invariants

A write other than initialization or an incomplete write index can invalidate a field. Production
field reads use production writes. Test reads also consider test writes. A known subclass or a
reassigned parameter leaves a constructor argument unknown during propagation.
[fields.rs:ts_field_written](./fields.rs) [fields.rs:ts_field_argument](./fields.rs)

## Side effects

`Writes::index` scans repository facts and builds in-memory indexes by owner and field name. It
keeps the location of a single write so initialization can be distinguished from other writes.
[fields.rs:Writes](./fields.rs) [fields.rs:index](./fields.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- ts_receivers:: field_flow:: http_bases::
```
