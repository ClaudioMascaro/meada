# meada-core · Rust

Types and operations for manifests, identities, permissions and exports.

## Interface

Validates TOML entries, compares versions, authorizes changes and parses index exports. The `wasm-bindgen` feature exposes these operations to the browser. `[manifest.rs:validate]` `[workspace.rs:diff]` `[authorize.rs:authorize_entries]` `[export.rs:Export]` `[lib.rs:wasm]`

The manifest also models an HTTP base selected by environment variable (`[[base_url]]`), the container environment path in Helm values (`[[helm_env]]`) and the channel client call that supplies the topic (`[[channel_client]]`). `[manifest.rs:BaseUrl]` `[manifest.rs:HelmEnv]` `[manifest.rs:ChannelClient]`

## Invariants

The browser uses import validation, which accepts other manifest sections when validating entries. The `owner` field accepts up to 256 characters. Authorization takes the role, grant and entries as inputs. `[wasm.rs:validate]` `[manifest.rs:validate_import]` `[manifest.rs:OWNER_MAX]` `[authorize.rs:authorize_entries]`

## Side effects

WASM initialization selects the message language. The caller supplies manifest and export text. `[wasm.rs:init]`

## Verification

From the repository root, run `cargo test -p meada-core --features wasm-bindgen`.
