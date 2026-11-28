# Snapshot and generation distribution

Distribution of index generations, version retention and coordination between publishers and read nodes.

## Interface

The storage client transfers objects. Pull installs generations on the node and serving reads the local generation. The corresponding features add a Valkey pointer and retention, heartbeat, a refresh lock and version operations. `[store.rs:Store]` `[pull.rs:pull]` `[serve.rs:Current]` `[generations.rs:point]` `[heartbeat.rs:beat]` `[lock.rs:acquire]` `[versions.rs:apply]`

## Invariants

Downloaded objects are validated before the local pointer is swapped. With the refresh lock, publishing requires the current token and a fencing counter greater than the one stored in the pointer to move the shared pointer. `[pull.rs:pull]` `[generations.rs:point]`

Reading the Valkey pointer validates IDs and fencing counters and rejects an empty retention list or one longer than `keep + 8` entries. If the mirrored `meada.toml` is invalid, the effective configuration lookup still returns `held = true` and `own = true`, with `documents = false`. `[generations.rs:read]` `[generations.rs:KEPT_MAX]` `[serve.rs:facts]` `[../config.rs:read_config_up_to]`

## Side Effects

Transfers objects over S3, writes local files and renames the new pointer file into place. Coordination features change Valkey keys and the lock command also runs the requested child process. `[store.rs:Store]` `[pull.rs:replace]` `[operation.rs:pointer_set]` `[heartbeat.rs:beat]` `[lock.rs:run]`

## Verification

From the repository root, run `cargo test -p meada --features refresh-lock --test it -- snapshot:: generations::`.

To exercise service-backed cases, load the variables printed by `scripts/test-services up` before testing.
