# Crates

The repository is a Cargo workspace (ADR 0014): the `meada` binary's crate at the root (`src/`,
`tests/`), and `meada-core/`, the deterministic part the binary and the console's front end
share. Both take one version from the workspace, and `meada` depends on `meada-core` at exactly
it. `meada-core` has no rusqlite, tree-sitter, filesystem, process or network code and no async
runtime; it builds for `wasm32-unknown-unknown`, its `wasm-bindgen` feature (never the binary's)
exporting `init`, `validate`, `workspaceVersionId`, `workspaceDiff`, `entriesText`,
`entriesJson`, `authorizeEntries`, `composeEntries`, `touchedOwners`, `parseExport` and the named-workspace set
(`namedEntriesJson`, `namedEntriesText`, `namedVersionId`, `namedAuthorize`, `namedVisible`,
`namedEffective`, `namedDiff`, `namedChanged`, `namedNameProblem`, `namedDescriptionProblem`,
`namedMessageProblem`, `resourceRoles`). CI's `core` job builds it
with wasm-pack and checks both trees.

Development builds optimize the `meada` package at level 1 to bound the resident code footprint
of the indexing and contract readers. Cargo's test profile inherits that setting. The corpus
proof keeps its original time, memory and export-size budgets against the historical builds.

Every tree-sitter grammar is a crate except Kotlin's: `vendor/tree-sitter-kotlin-ng/` holds
`tree-sitter-kotlin-ng` 1.1.0 with upstream PRs #21 and #13 and a local scanner patch
(`PROVENANCE.md`), which the root's `build.rs` compiles and `lang/kotlin.rs` binds (ADR 0017). No
other Kotlin grammar may enter the dependency tree: they all export `tree_sitter_kotlin`.

The modules of `meada-core`:

- `meada-core/src/manifest.rs`: a workspace manifest's `[[repository]]` entries and every check their text decides (`validate`, `validate_import`, identities, `owner`, `added_by`), with the `[serve]`, `[[resource]]` and `[[peer]]` shapes `meada.toml` shares. [core](../core.md)
- `meada-core/src/ids.rs`: bundle, generation and workspace version ids (ADR 0012, 0014), `is_id`, `fence`, `entry_blocks`. [core](../core.md)
- `meada-core/src/workspace.rs`: `diff` of two workspace versions and `entries_text`, the JSON-to-TOML conversion `snapshot::versions` and WASM share. [core](../core.md)
- `meada-core/src/authorize.rs`: what a subject may change in a workspace version (ADR 0015): `authorize_entries`, `compose_entries`. [core](../core.md)
- `meada-core/src/msg.rs`, `meada-core/src/http.rs`: the messages of those checks and the language flag; `normalize_path` and `PARAM` of the HTTP kind. [core](../core.md)
- `meada-core/src/export.rs`: the export's types as `meada export` writes them, and `parse`. [core](../core.md)
- `meada-core/src/shape.rs`, `drift.rs`: shapes and shape drift (ADR 0009), and how `context`, `search` and `status` show a link's drift. [core](../core.md)
- `meada-core/src/render.rs`, `search.rs`, `context.rs`, `trace.rs`: the token budget and line helpers every answer shares, and the results and text answers of `search`, `context` and `trace`. [core](../core.md)
- `meada-core/src/named.rs`, `effective.rs`: the pure rules of named workspaces (ADR 0020, 0021) and the effective configuration of their entries. [workspaces](../workspaces.md)
- `meada-core/src/lib.rs`: the crate root, with `qualify` and `parent` (dotted names). [core](../core.md)
- `meada-core/src/wasm.rs`: the browser's bindings (feature `wasm-bindgen`), the same functions as the CLI's checks. [core](../core.md)
