# HCL values and configuration selectors

HCL configuration values and lookup of configuration locations referenced by declarations.

## Interface

The HCL reader turns attributes into rows carrying a key path and location. The resolver takes a selector and looks up matching sites in a repository index. `[hcl.rs:read]` `[sites.rs:locate]` `[sites.rs:Site]`

## Invariants

Null values are omitted. Selectors require complete scalar values and disregard truncated or redacted text. `[hcl.rs:read]` `[sites.rs:locate]`

The HCL reader rejects a file when braces, brackets and parentheses nest more than 64 levels deep. `[hcl.rs:NESTING_MAX]`

## Side Effects

The parser works on the supplied bytes. Site lookup queries the SQLite connection already opened by the caller. `[hcl.rs:read]` `[sites.rs:locate]`

## Verification

From the repository root, run `cargo test -p meada --test it config_values::`.
