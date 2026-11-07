# Split area files

The architecture, contract and resolution/index/shape ceiling indexes stay within 8 KiB
(`MAP_BUDGET`). Their overview and topic leaves stay within 24 KiB (`DETAIL_BUDGET`). Other
Markdown files under `docs/agents` keep their existing 64 KiB (`AREA_BUDGET`) until migrated;
`tests/it/docs.rs` checks every file. A file past its budget is split by its `## ` headings
with `cargo run --example split_agents -- <file>` in the same change, never trimmed by dropping
entries. Prepare meaningful headings first when a single section is too large. The file keeps
its introduction and one line per part, and each part is a file of its own in a directory
named after it. Read the index and the topics relevant to the change, including extension and
invariant details when changing a seam. Existing split families and their references follow:

- [map/tests.md](../tests.md): [harness](../tests/test-crates-harness-and-fixtures.md),
  [test modules by subject](../tests/test-modules-by-subject.md),
  [package, channel and gateway fixtures](../tests/package-channel-and-gateway-fixtures.md),
  [workspace editor and shared validation](../tests/workspace-editor-and-shared-validation-310.md),
  [Java declarative clients](../tests/java-declarative-clients-90.md),
  [Java conformance](../tests/java-conformance-84.md) and the named workspaces parts
  ([creation to query](../tests/named-workspaces-creation-to-query-342.md),
  [versions and overlap](../tests/named-workspaces-versions-and-overlap-343.md),
  [context configuration](../tests/named-workspaces-context-configuration-344.md),
  [acceptance](../tests/named-workspaces-acceptance-347.md)).
- [map/indexing.md](../indexing.md): [modules in brief](../indexing/modules-in-brief.md),
  [stored values](../indexing/stored-values.md),
  [discovery, the index, adapter](../indexing/detail-discovery-index-adapter.md),
  [message forms, Python and Rust](../indexing/detail-message-forms-python-rust.md),
  [Go, resolution and JVM builds](../indexing/detail-go-resolution-jvm-builds.md),
  [Java receivers and supertypes](../indexing/java-receivers-and-supertypes-85.md) and
  [Java source facts](../indexing/java-source-facts-84.md).
- [ceilings.md](../../ceilings.md): the limits of the code, by area
  ([resolution and the index](../../ceilings/resolution-the-index-and-shape-readers.md),
  [language packs](../../ceilings/language-packs.md),
  [HTTP: JVM, Go, Rust, TS](../../ceilings/http-clients-jvm-go-rust-ts.md),
  [HTTP: C# and Swift](../../ceilings/http-clients-csharp-swift.md),
  [channel clients](../../ceilings/channel-clients.md),
  [server, accounts, workspaces and deploy](../../ceilings/server-accounts-workspaces-and-deploy.md)).
