# Change impact in the graph

Computes the files, packages and contract sides affected by a change for `meada trace --affected`.

## Interface

Lockfile attribution identifies changed dependencies. Root selection maps changed paths to symbols, configuration sites, IaC nodes and document sides, and the answer aggregates the dependents reached. A lockfile in a subdirectory can only cover changed member manifests within that directory. `[affected.rs:attribute]` `[affected.rs:roots]` `[affected.rs:answer]`

## Invariants

A changed path with no known root appears in `open` with its reason. Response lists have stable ordering. `[affected.rs:roots]` `[affected.rs:answer]`

Each reached file is rated by the weakest edge of its strongest path (`certain`, `inferred`, `model` or `ambiguous`). A chart execution reaches only files owned by its package, excluding nested packages. `[affected.rs:Tier]` `[affected.rs:answer]` `[affected.rs:dependents]` `[../trace.rs:runs]`

## Side Effects

Dependency comparison can read files from the checkout and earlier versions through Git. The response is built in memory. `[affected.rs:attribute]` `[affected.rs:answer]`

## Verification

From the repository root, run `cargo test -p meada --test it -- affected:: kafka_lock::`.
