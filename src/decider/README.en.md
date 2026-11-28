# Model-assisted decisions

Model-assisted choices for the explicit `meada decide --decider` option. Adapters implement the `chat` and `decisions` protocols.

## Interface

Execution selects eligible items, builds their context excerpts and passes accepted decisions to the callback. The chat protocol sends one question per request and the decisions protocol batches questions. `[mod.rs:run]` `[chat.rs:request]` `[decisions.rs:request]`

Automatic mode reports and skips items with zero candidates or more than 50. `[mod.rs:MAX_CANDIDATES]` `[mod.rs:run]`

## Invariants

Each answer must name an offered key, a probability between zero and one, and a valid model identifier. `[validate.rs:proposal]`

## Side Effects

Reads environment configuration and repository excerpts. Execution can send those excerpts over HTTP and write the result to the supplied output. Request preview returns before sending. `[mod.rs:from_env]` `[mod.rs:run]`

## Verification

From the repository root, run `cargo test -p meada --test it decider::`.
