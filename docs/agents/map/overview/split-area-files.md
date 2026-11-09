# Split area files

Every Markdown file under `docs/agents` stays within its byte budget (`tests/it/docs.rs` checks
it): `MAP_BUDGET` is 8 KiB for `architecture.md`, `map/contracts.md` and
`ceilings/resolution-the-index-and-shape-readers.md`; `DETAIL_BUDGET` is 24 KiB for the topics
under `map/overview/`, `map/contracts/` and `ceilings/resolution-the-index-and-shape-readers/`.
`AREA_BUDGET` stays 64 KiB for the other files while those areas await their own split.
Read an index to choose the relevant topics, not every part. A file past its budget is split by
its `## ` headings with `cargo run --example split_agents -- <file>` in the same change, never
trimmed by dropping entries: the file keeps its introduction and one line per part, and each part
is a file of its own in a directory named after it. Prepare meaningful headings first when a
single section would still exceed its budget. The split families include:

- [architecture.md](../../architecture.md): flow, crates, module descriptions, identities,
  extension seams, invariants, builds, deployment and tests live in this `map/overview/`
  directory; the routing index retains every module's area.
- [contracts](../contracts.md): library forms, the kind interface, HTTP, resources, channels,
  packages and their ecosystems, AWS SDK sides and images.
- [resolution, the index and shape readers](../../ceilings/resolution-the-index-and-shape-readers.md):
  resolution, engine/index/query costs, package/build readers, shapes/documents, configuration,
  Terraform/provisioning, Kubernetes/Crossplane and workload configuration.

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
