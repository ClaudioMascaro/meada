# Index value cache

Value cache that avoids reanalyzing unchanged files during index refresh.

## Interface

The index opens its cache on demand, looks up facts by path and key, queues new facts, and retains the needed entries after extraction. `[values.rs:beside]` `[values.rs:get]` `[values.rs:put]` `[values.rs:keep]`

## Invariants

The key includes file bytes, language settings, pack and Meada build. A missing or unreadable cache yields a miss so the file can be parsed again. `[values.rs:key]` `[values.rs:get]`

An entry larger than 8 MiB of uncompressed JSON cannot be reused. The index reparses the file when those values are needed. `[values.rs:ENTRY_BYTES]` `[values.rs:facts_of]`

## Side Effects

Writes and prunes compressed entries in `values.db` beside the SQLite index. An in-memory index does not create that file. `[values.rs:FILE]` `[values.rs:beside]` `[values.rs:keep]`

## Verification

From the repository root, run `cargo test -p meada --test it freshness::a_refresh_over_the_values_cache`.
