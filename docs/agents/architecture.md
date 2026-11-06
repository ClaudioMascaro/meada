# Architecture map

Which module owns what, the seams between them, and what the code relies on: an index to read
whole, within 64 KB (`tests/it/docs.rs` fails above it). Verified against `src/` and `tests/` of
the v0.1 trunk; the code wins over this file, and a change to a seam updates it. Per-area detail
lives in `map/`, one file per area, each named from the module list below; the limits of the
code are in [ceilings.md](ceilings.md). Terms are `GLOSSARY.md`'s; decisions are in `docs/adr/`.

Read this file, then the area file of the area a change touches. A new module, seam or invariant
gets a line here; its detail goes to the area file, and a limit to `ceilings.md`.

## Flow

`Engine::open(dir)` finds the **workspace**, creates or recreates each **repository**'s **index**
where `Repository::index` says it lives, and keeps one connection per repository (`Engine::dbs`,
workspace order; SQLite attaches at most 10 databases to one connection, so none is attached):
`Engine::exports` reads each index through its own, so a workspace has any number of
repositories. Locally the index is the checkout's own `.meada/index.db`. In
**server mode**, `Engine::open_manifest(manifest, repo)` builds the workspace from the
**workspace manifest** instead (`workspace::Manifest`: every listed repository, `repo` first,
then manifest order, nothing discovered; under an OIDC grant only the granted ones) and each
index lives at `<index_dir>/<identity>/index.db`; `engine::index_workspace` and
`engine::probe_workspace` are `meada index --workspace` and `meada status --workspace`.
Every query (`export`, `search`, `context`, `trace`) calls `refresh()` first, reads the indexes
(`search`, `context` and `trace` a partial export, `Engine::partial`), adds the contracts each
`meada.toml` declares, derives the **links** and renders; nothing outside the index's directory
is written, and everything the engine returns is sorted, so the same files give byte-identical
output. The rest of the flow (the read-only open of server mode, generations, the SQL filters of
the partial export, `safe.directory`): [map/flow.md](map/flow.md).

## Crates

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

Every tree-sitter grammar is a crate except Kotlin's: `vendor/tree-sitter-kotlin-ng/` holds
`tree-sitter-kotlin-ng` 1.1.0 with upstream PRs #21 and #13 and a local scanner patch
(`PROVENANCE.md`), which the root's `build.rs` compiles and `lang/kotlin.rs` binds (ADR 0017). No
other Kotlin grammar may enter the dependency tree: they all export `tree_sitter_kotlin`.

The modules of `meada-core`:

- `meada-core/src/manifest.rs`: a workspace manifest's `[[repository]]` entries and every check their text decides (`validate`, `validate_import`, identities, `owner`, `added_by`), with the `[serve]`, `[[resource]]` and `[[peer]]` shapes `meada.toml` shares. [core](map/core.md)
- `meada-core/src/ids.rs`: bundle, generation and workspace version ids (ADR 0012, 0014), `is_id`, `fence`, `entry_blocks`. [core](map/core.md)
- `meada-core/src/workspace.rs`: `diff` of two workspace versions and `entries_text`, the JSON-to-TOML conversion `snapshot::versions` and WASM share. [core](map/core.md)
- `meada-core/src/authorize.rs`: what a subject may change in a workspace version (ADR 0015): `authorize_entries`, `compose_entries`. [core](map/core.md)
- `meada-core/src/msg.rs`, `meada-core/src/http.rs`: the messages of those checks and the language flag; `normalize_path` and `PARAM` of the HTTP kind. [core](map/core.md)
- `meada-core/src/export.rs`: the export's types as `meada export` writes them, and `parse`. [core](map/core.md)
- `meada-core/src/shape.rs`, `drift.rs`: shapes and shape drift (ADR 0009), and how `context`, `search` and `status` show a link's drift. [core](map/core.md)
- `meada-core/src/render.rs`, `search.rs`, `context.rs`, `trace.rs`: the token budget and line helpers every answer shares, and the results and text answers of `search`, `context` and `trace`. [core](map/core.md)
- `meada-core/src/named.rs`, `effective.rs`: the pure rules of named workspaces (ADR 0020, 0021) and the effective configuration of their entries. [workspaces](map/workspaces.md)
- `meada-core/src/lib.rs`: the crate root, with `qualify` and `parent` (dotted names). [core](map/core.md)
- `meada-core/src/wasm.rs`: the browser's bindings (feature `wasm-bindgen`), the same functions as the CLI's checks. [core](map/core.md)

## Modules (`src/`)

Each line names what a module owns and links the file in `map/` with its detail. Read the one
for the area a change touches.

- `engine.rs`: `Engine`, the one query seam (`open`, `refresh`, `export`, `search`, `context`, `trace`, open items, decisions, `status`); it builds the results `meada-core` renders. [queries](map/queries.md)
- `items.rs`: open items, derived in one place. [queries](map/queries.md)
- `trace.rs`: the `Graph`, the walk and the process walk (`Graph::process`). [queries](map/queries.md)
- `partial.rs`: the partial export a query reads (#396): the contract rows whole, the code rows the answer needs. [queries](map/queries.md)
- `drift_view.rs`: places the links `context`, `search` and `status` show with their drift. [queries](map/queries.md)
- `render.rs`: the source files an answer reads, and the budget and line helpers `meada-core` holds. [queries](map/queries.md)
- `mcp.rs`: three read-only MCP tools over stdio and streamable HTTP, the `Gate` before routing, `serve_http`. [queries](map/queries.md)
- `main.rs`: clap, the CLI's commands. [queries](map/queries.md)
- `msg.rs`: the CLI message catalog, pt-BR, English and Spanish. [queries](map/queries.md)
- `lib.rs`: the crate root, re-exporting the modules the CLI, the MCP server and the tests share. [queries](map/queries.md)
- `workspace.rs`: the workspace (`Workspace::discover`: repositories, peers, sibling clones) and server mode's workspace manifest (`Manifest`). [indexing](map/indexing.md)
- `config.rs`: `meada.toml` (`[[peer]]`, `[serve]`, `[[resource]]`) and the shared TOML reader. [indexing](map/indexing.md)
- `repo.rs`: repository root and identity, read through `git`. [indexing](map/indexing.md)
- `index.rs`: the SQLite index: schema, the freshness pipeline (`refresh`) and `export`; `index/values.rs`, the values cache beside it (ADR 0027). [indexing](map/indexing.md)
- `lang/`: the language adapter and the TS/JS, Python, Rust, Go, Java, Kotlin, C# and Swift packs. [indexing](map/indexing.md)
- `resolve.rs`: the shared resolution: calls to definitions (`Resolver`), values and receivers (`Code`). [indexing](map/indexing.md)
- `community.rs`: the deterministic Leiden behind communities (spec #31). [indexing](map/indexing.md)
- `entry.rs`: entry points (spec #31), found when an index is written. [indexing](map/indexing.md)
- `contract.rs`: the `ContractKind` trait, `KINDS` and the free functions the engine asks. [contracts](map/contracts.md)
- `http.rs`, `http/`: the HTTP kind: the contract and path rules, mounts, and one submodule per language for providers and consumers. [contracts](map/contracts.md)
- `resource.rs`: the resource kind, declared in `meada.toml`. [contracts](map/contracts.md)
- `image.rs`, `image/config.rs`: image builders/runners, normalized names, checked-in build configuration, workload execution and two-role orphans. [contracts](map/contracts.md)
- `channel.rs`, `channel/`: the channel kind (queues and topics), one module per language for message libraries. [contracts](map/contracts.md)
- `package.rs`, `package/`: packages as contracts, one module per ecosystem behind the `Ecosystem` trait. [contracts](map/contracts.md)
- `names.rs`: the questions the contract kinds ask of a name: what a library outside the repository is. [languages](map/languages.md)
- `jvm/`: what the JVM ecosystems and kinds share: the Maven and Gradle readers, Spring configuration, file readers. [languages](map/languages.md), [seams](map/seams.md)
- `dotnet/`: what the .NET pack and ecosystems share (spec #28): the MSBuild project reader and what builds on it. [seams](map/seams.md)
- `xml.rs`: the bounds the XML build readers of `jvm/` and `dotnet/` share. [seams](map/seams.md)
- `document/`: OpenAPI and AsyncAPI provider documents, read into a `Summary` (spec #21), and `events.rs`, the one YAML and JSON event reader every YAML or JSON reader goes through (#646). [documents](map/documents.md)
- `config_values.rs`, `config_values/hcl.rs`: config values (#639, #640, ADR 0025): every scalar of the YAML, JSON and Terraform files the walk visits and of the hidden CI paths, with its key path, `search` kind `value` and the `meada status` counts. [documents](map/documents.md)
- `iac.rs`, `iac/`: the embedded AWS type table, provisioning sides/links and subscription
  summaries (`iac/subscription.rs`, #656, #658), and `iac/orphans.rs`, scope-aware orphan
  reports and side markers (#657); IaC nodes (#649,
  ADR 0025): the Terraform resources, data blocks and module calls with their address and root
  module, the references between them, the static evaluation of their names (#650,
  `iac/functions.rs` the pure functions), `search` kind `iac`, and `context` and `trace` on an
  address; Compose, CloudFormation and Serverless services (#652 to #654) are nodes of the same
  shape. [documents](map/documents.md)
- `kube.rs`: Kubernetes objects and Kustomize overlays as IaC nodes (#651, ADR 0025): address `<Kind>[.<group>]/<namespace>/<name>`, an overlay as the root of its evaluation with prefix, suffix, namespace and images applied, the references between objects, and the `meada status` count of the entries not followed; `kube/crossplane.rs`, Crossplane's roles, composed resources and references (#655). [documents](map/documents.md)
- `literals.rs`, `zod.rs`, `declared.rs`: the shape of a route read from its handler's returned literals, zod schemas and declared types (spec #23); `lang/ts/types.rs` finds the schema a `z.infer` annotation names. [documents](map/documents.md)
- `decisions.rs`: `meada.decisions.toml` and its replay at query time. [decisions](map/decisions.md)
- `decide.rs`, `decider/`: `meada decide` and decider mode, the only network path. [decisions](map/decisions.md)
- `modules.rs`: the section table of `meada.workspace.toml` (ADR 0012): each cloud module's section, its cargo feature, and whether this build has it. [cloud](map/cloud.md)
- `valkey.rs`: the one Valkey client the cloud modules share. [cloud](map/cloud.md)
- `postgres.rs`: the data layer (feature `postgres`, ADR 0016): the durable mutable state of server mode. [cloud](map/cloud.md)
- `audit.rs`: the audit (ADR 0016, #328): a record in `<schema>.audit` per console write and per CLI twin of one. [cloud](map/cloud.md)
- `directory.rs`, `directory/`: the directory (feature `directory`): users and groups the identity provider pushes in over SCIM 2.0. [cloud](map/cloud.md)
- `oidc.rs`, `oidc/`: server mode as an OAuth 2.1 resource server (feature `oidc`, ADR 0016). [cloud](map/cloud.md)
- `grant.rs`, `grant/`: the grant providers (ADR 0016): what an admitted subject may read. [cloud](map/cloud.md)
- `accounts.rs`, `accounts/`: local accounts (feature `accounts`, ADR 0024): passwords, session tokens and agent tokens. [cloud](map/cloud.md)
- `github_app.rs`, `github_app/`: git's credential helper for the indexing job, from GitHub App installation tokens (ADR 0013). [cloud](map/cloud.md)
- `snapshot.rs`, `snapshot/`: the snapshot store (feature `snapshot`, ADR 0012): bundles, generations, publish, pull, workspace versions. [snapshot](map/snapshot.md)
- `ui.rs`, `ui/`: the management console's process (feature `ui`, ADR 0014): its API, its sessions and the front end's files. [console](map/console.md)
- `workspaces.rs`, `workspaces/`: named workspaces (spec S5, ADR 0020, 0021): the `[workspaces]` section, contexts, lifecycle and store. [workspaces](map/workspaces.md)
- `catalog.rs`: the `[catalog]` section (#341): the repositories the role check may list. [workspaces](map/workspaces.md)
- `proposals.rs`: the `[proposals]` section (ADR 0022): where agents reach the proposal server. [workspaces](map/workspaces.md)

## Console frontend (`ui/`)

The static console (ADR 0014) is a separate Vite/React/strict TypeScript build. `npm ci`,
`npm run wasm`, `npm run build`, `npm run check:version` and `npm test` run inside `ui/`, with
Node pinned by `.nvmrc` and wasm-pack by `scripts/wasm.mjs`. Cargo never invokes them. The
WASM bundle builds from `meada-core` into ignored `ui/wasm/`; Vite puts content-hashed scripts,
styles and WASM under `dist/assets/`, with `meada-ui.json` carrying the workspace version.

The pages, the auth and the build: [map/console.md](map/console.md).

## Named workspaces (spec S5)

Built and validated end to end (#347). ADR 0020 ([named workspaces and shared
indexing](../adr/0020-named-workspaces-and-shared-indexing.md)) holds the domain decision, ADR
0021 ([contracts](../adr/0021-named-workspaces-contracts.md)) the contracts and ADR 0022
([proposal server](../adr/0022-authenticated-workspace-proposals.md)) the proposal server; the
user-facing page is `docs/site/pt/named-workspaces.md`. What slices #341 to #347 delivered,
module by module: [map/workspaces.md](map/workspaces.md).

## Identities and formats

- Repository identity: `github.com/acme/web` (lower-cased host and path, no scheme, user, port
  or `.git`) or `local/<dir>`.
- Symbol: `<repository>::<path>::<qualified name>` (`Class.method`, `outer.inner`).
- Call site: `<caller>::<normalized callee>#<ordinal>`; a top-level call's caller is
  `<repository>::<path>`. The normalized callee keeps identifier and member chains only; any
  other expression becomes `(<node kind>)`, except Swift's implicit member `.f(..)`, which is
  `(implicit).f` (`lang::IMPLICIT_HEAD`: the resolver matches it among methods by name and labels,
  and leaves `init` unresolved). A Rust path is dotted like a chain (`crate::a::f`
  is `crate.a.f`, `Type::new` is `Type.new`); its `self::` and a `super::` that stays in the
  file are read from the file's top under the keyword `mod` (`self::f` in `inner` is
  `mod.inner.f`, `super::f` out of `inner` is `mod.f`).
- Rust qualified names use `.` as well: `Type.method` (an inherent `impl`), `Type.Trait.method`
  (a trait's `impl`), `Trait.method` (a default body), `inner.helper` (an inline module); a
  later block that gives a name already given a method is `Type.Trait#2.method` (`impl From<A>`
  then `impl From<B>`). A module, inline or declared with `mod x;`, is a symbol of its file.
- Inline handlers: a pack emits anonymous function arguments (and functions written in an
  options object argument) as `Kind::Inline` named `@<byte offset>`; `Names` renames the ones a
  kind claims, to `<METHOD> <full path>` for a route or `<broker> <channel names>` for a channel
  handler (`#<n>` from the second equal name), and folds the rest into the enclosing symbol.
- Entry point (spec #31, `entry.rs`): a route is `<handler symbol>::<contract>`, such as
  `example.com/chain/gateway::src/orders.ts::listOrders::http::GET::/api/orders` (the handler's
  identity, then the contract identifier; an inline handler keeps its synthetic symbol, as in
  `example.com/chain/gateway::src/server.ts::GET /api/health::http::GET::/api/health`); an export
  is the exported function or method's own symbol identity. Its label is `<METHOD> <path>` for a
  route and the qualified name for an export or a program entry point (kind `program`, the
  symbol's own identity too, declared by the language pack and a door even when called; it wins
  over an export of the same symbol). A process is addressed as `process::<entry point
  identity>` and named by the label. A route whose handler is a file with no function of its own
  (`export const GET = withAuth(...)`) is a route entry point at the file's identity
  (`<repository>::<path>`), its process starting from the file's top-level calls; a side only
  a document lists has none. A channel is `<handler symbol>::<channel contract>`
  (`example.com/chain/worker::src/consumer.ts::onOrderCreated::channel::kafka::orders.created`),
  labelled by the channel identifier (an unresolved side as `channel::kafka (unresolved)`), kind
  `channel`; a channel handler is no export entry point. A handler serving several routes or
  channels has an entry point per contract,
  and entry points are unique by identity. The stored export entry points are candidates:
  `Engine::exports` drops (in the package kind's `workspace_step`, after `import`), each one that is the target of an import
  from its own repository (a monorepo), at any level.
- Package: `package::npm::<name>`, `package::pypi::<name>` (the PEP 503 normalized name) or
  `package::cargo::<name>` (the `[package] name`; a dependent's site names the key it writes, so
  a renamed crate has its own site) or `package::go::<module path>` (the `module` of a `go.mod`), or
  `package::maven::<group>:<artifact>` (a Maven or Gradle artifact) or `package::nuget::<id>` (an
  SDK-style `.csproj`: its `PackageId`, else `AssemblyName`, else file name) or
  `package::swiftpm::<name>` (a `Package.swift`: the literal of `Package(name:)`),
  provided at `<repository>::<manifest path>` (such as `example.com/pkg/shared::package.json` or
  `example.com/pkg/pylib::pyproject.toml`) and consumed at `<repository>::<manifest
  path>::<field>::<name>` (PyPI fields: `project.dependencies`, `tool.poetry.dependencies`, and
  `requirements` for a `requirements*.txt`). Both sites hold a `::`, which `Engine::export` needs to keep their links.
  A call through a package import that resolves is also a consumer of the package, at its call
  site, linked to the provider's symbol (`<repository>::<path>::<name>`).
- Document route: `<repository>::<document path>::<METHOD> <path>`, the site of a route only a
  document lists, such as `example.com/chain/gateway::openapi.yaml::GET /api/shipments/{}`
  (`Export::document_of` recognises it). Document channel side: `<repository>::<document
  path>::<broker> send|receive <name>`, a provider (`send`) or a consumer (`receive`) only a
  document lists, such as `example.com/asyncapi/events::asyncapi.yaml::kafka send orders.created`.
- Community: its repository identity plus its name, the most common parent directory of its
  members' files (`src`, `.` for the root). The second and later communities of the
  repository with that name, by smallest member identity, take `#2`, `#3`, skipping any `#n`
  another community is already named (a directory `src#2`), so the name is unique.
- Contract: `<prefix>::...`, `http::GET::/users/{}`, `resource::postgres::orders`,
  `channel::kafka::orders.created` (name kept as evaluated; `channel::kafka` alone for an
  unresolved side). A path parameter of any spelling (`:id`, `[id]`, `{id}`, `<id>`, `<int:id>`)
  is `{}`. A provider of every method is `http::ANY::<path>`.
- Export JSON: `{format, repository, commit, dirty, files, symbols, calls, providers, consumers,
  bindings, documents, publishes, packages, dependencies, imports, communities, links, processes}`;
  `format` is `meada_core::export::FORMAT` (ADR 0004); `commit` and `dirty` (absent together) are
  the indexed commit and whether the checkout differed from it, read around the refresh locally
  and from the recorded commit or the generation's `head` in server mode (`Engine::export`);
  each `symbols` entry is `{id, span, hash}`, `hash` the hex BLAKE3 of the span's exact bytes (`search` kind
  `duplicate` groups by it); `communities` (absent when empty) lists each community as `{name, members}`, by name, with
  its sorted member identities; `processes` (absent when
  empty) lists the repository's entry points by identity as `{entry_point, kind, label, steps}`,
  `kind` being `route`, `export`, `channel` or `program`, each step `{depth, level, repository, via, node, ...}`: `via`
  is `entry`, `call` or `link` (a link step has the link's `reason`), `node` the symbol, file
  or side reached (a note's call site), and `mark` (`cycle`, `seen`, `depth-limit`, `ambiguous` with its
  `candidates`, `unresolved`, `no-provider` with its `contract`, `produces` or `consumes` of an
  unresolved channel) when the step is not a plain node; steps are in walk order and cross
  repositories; `imports` (absent when empty) lists each
  call resolved into a package as `{site, specifier, name, level, how, target}`, by site then
  target, where `level` is the import's own level (how certain the entry and the name were), not
  the hop's: the link from that call carries the hop's level, the weaker of it and the package
  link's. `how` is the field or rule that named the entry (`exports`, `types`, `module`,
  `main`, `path`, or `outDir` when the output-to-source mapping found the file; for a Python
  import, `moduleName` or `topLevel`, spec #24); for an
  `ambiguous` import it names the declared field or rule whose entry could not be followed to a
  source file; a provider has
  `gateway` when `[serve]` rewrites its path, and `shape` (`{source, fields, open, scalars,
  declared, optional, guessed}`, each of the last five absent when empty; `guessed` lists the
  parts a package import filled through a resolution that was not `exact`, issue #175) when a document gives its route or channel side one, or, for an HTTP route with
  no document, when its handler returns a zod schema's value (`{"source": {"zod": "<file>"}, ...}`)
  or object literals (`{"source": {"literals": "<file>"}, ...}`, spec #23); a consumer has the same
  `shape` for a channel side a document lists, and an HTTP call that reads fields of the parsed
  response has `shape` `{"source": {"reads": "<file>"}, fields}` (spec #23). A link of an HTTP
  route with a shape on both sides and a single provider has `drift` (`{missing, unchecked}`,
  each absent when empty: `missing` maps each missing path to its level, `unchecked` each
  consumer path below an open part to that open path) when some consumer path is missing or
  not checked. `documents` (absent when none was read) lists each
  document read, in path order, as `{path, format, version, contracts}` (route and channel
  contracts, sorted);
  ignored documents are left out (`meada status` names them). An unresolved consumer's method and URL fragments
  live in the index (`contracts.method`, `contracts.fragments`) and are left out of the export. A
  resource side is a provider or consumer whose `site` is the `via` symbol, or the repository
  identity for a repository-level side (a consumer's `via` is null). A channel producer is a
  provider whose `site` is a call site; a channel consumer is a consumer whose `site` is its
  handler symbol (or file, at the top level), `via` null. A RabbitMQ binding is neither: it is in
  `bindings: [{contract, exchange, pattern, site}]` (the queue's channel, the exchange, the
  pattern, the `bindQueue` call site), sorted, absent when empty; `context` on the queue lists
  it as `<repository> binding <exchange> <pattern> <path>:<line> <symbol>` after the sides. A
  RabbitMQ publish to a named exchange is in `publishes: [{exchange, key, site}]` (the `publish`
  call site), sorted, absent when empty: it is no channel's side by itself, and a link from a
  queue's consumer names its site as the provider. Every channel the export names has the form
  `channel::<broker>::<name>` (or `channel::<broker>`) of a v1 broker kind. A call has `level` (`exact`,
  `inferred`, `ambiguous` or null) and `target`; an ambiguous call has `target: null` and a sorted
  `candidates`. `export` keeps only the links that touch the repository where Meada runs. A
  repository with stale decisions also has `stale_decisions: [{call, target, cause}]`
  (`call-gone`, `item-not-open`, `target-gone`, `not-candidate`), absent when empty; decisions
  waiting on a missing peer are not exported. A `decided` link also has `by` (`human` or a model
  id) and, for a model, `p`; its reason is `decided by <by> (p=<p>)` (two decimals) or `decided by
  human`.
- Decision `target`: `<provider repository identity>::<contract>`, such as
  `example.com/chain/gateway::http::GET::/api/health`; split at the first `::`. The target names
  no site: if the repository serves the contract from several, the link goes to the first by site.
  Decision `call` is a consumer's call-site identity.

## Seams and what extends them

Each seam keeps its extension steps here, or their lead with a link to the whole entry in `map/`.

- New **contract kind**: implement `ContractKind` (`contract.rs`) and add it to `KINDS`, at the
  end: the order of the list is the order of the steps and of the output. Nothing in
  `engine.rs`, `items.rs`, `decisions.rs`, `trace.rs`, `entry.rs` or the extraction loop of
  `index.rs` names a kind, and a kind's rule stays in its own file, with the library forms it reads in each
  language (one module per language: `http/<language>.rs`, `channel/<language>.rs`), never in
  the pack, whose `&[&str]` tables are only `syntax`, `runtime` (the language's own library that
  value evaluation passes through) or a decided `exception`, each listed with its class by a test
  in `lang/mod.rs` (`CODING_STANDARDS.md`). Three questions are
  required: `prefix`, `extract` (receives `Code`, never the syntax tree, and `Indexed`, what the
  index read of the repository besides: the manifests and tsconfigs, the exported rows, the
  files of every pack and the bytes of the config files the packs list and of the inputs they
  name (`Indexed::configs`, never stored: Java's Spring `application.properties` and
  `application.yml`, Backstage's `$include` partials, #689, with `Indexed::symlinks`), which a kind
  reads when its contracts depend on what the manifests or a configuration say;
  it returns `Found` with a `Site`: `Def`, `Inline`, `File`, `Call`) and
  `link` (receives the whole workspace's providers and exports). The others have a default
  body, and a kind overrides only what it answers differently:

  | Question | The kind says |
  |---|---|
  | `entry` | which entry point kind a side of a contract makes (a pure function of the contract: entry points are stored) |
  | `declare` | the sides `meada.toml` declares, run at query time (a config edit needs no reindex) |
  | `symbols_read` | the symbols `declare`, `workspace_step`, `link` and `status` look up, which a partial export reads before declaring (resources: their `via` symbols; packages: every symbol of a repository with a named manifest) |
  | `workspace_step` | a step over every repository after every `declare` (the package provider index, dependency entries, imports, shapes) |
  | `after_links` | a pass over the links after the decisions replay (HTTP: drift) |
  | `unresolved`, `label`, `parts` | whether an identifier is an unresolved side, how it is labelled, the parts `search` ranks |
  | `place` | where a site sits when it is neither a symbol nor a call (packages: manifest and field) |
  | `search_word`, `own_search` | the word of `search`'s `kind` and, for an own renderer, its hits |
  | `sides`, `view`, `related_sides` | the role words and the carried word of `context` on a contract, and the sides that reach it by another name; a contract listed by its providers alone is followed by the linked consumers that read fields, with their drift (the engine's rule: `View::follows_drift`) |
  | `at_call`, `context_sections` | the lines a call adds to a symbol's `contracts:`, and the sections `context` adds after the callees (packages: `consumers:`) |
  | `context_reads` | the call sites `context_sections` looks up, which `context` reads first (packages: the consumers of the links into the symbol) |
  | `starts` | the providers a `trace` start text names |
  | `calls_from_definition` | whether a consumer at a symbol (a Feign or Retrofit interface method) calls on its provider, so `trace` follows from the method to it (HTTP; a channel handler is where a link ends) |
  | `status` | the lines `meada status` prints |
  | `decidable`, `serves`, `candidate` | whether an ambiguous link is an open item, whether a provider serves a contract (aliases included), whether a provider is a candidate for a consumer with no contract |

  The engine fixes the phase order: every `declare`, then every `workspace_step`, then `link`,
  then the decisions replay, then every `after_links`, each in `KINDS` order (decisions are
  kind-free and a pass over the links must see decided links). A side is unresolved when its
  contract is absent (skipped by `link`, an open item whose candidates come from each provider's
  kind) or when its kind says so (`unresolved`: never linked, never an open item, shown with the
  suffix). The deliberate extras a kind may still need outside its files: a phrase in the MCP
  `search` doc string (a test fails until it is there), a variant of `entry::EntryKind` when
  its doors need a stored name, stored fields on the export, provider and consumer rows, and a
  reader in `document` when it reads provider documents. The coding standard sends any other
  hook in the engine to review.
  The index writer still names the package module for the stored format alone: the manifest and
  tsconfig rows it reads and writes, the export types, and the ecosystem seam's `programs` and
  `inherit`, which stay with the engine (spec #205).
- New **language pack**: add it to `PACKS` in `lang/mod.rs`, a fixture
  `tests/fixtures/conformance-<language>/` and one `conform(...)` test in `tests/it/conformance.rs`.
  A pack is done when that suite passes. A pack ships syntax and value evaluation only: no
  table of libraries and no form of one. The forms of its web frameworks, HTTP clients and message
  libraries are separate tickets, each a module of the pack's language in the kind that reads
  them (`http/<language>.rs`, `channel/<language>.rs`). What a pack answers besides `parse` and
  `modules`, and the builder form (`LanguagePack::fluent`): [map/languages.md](map/languages.md).
- New **package ecosystem** (spec #22 user story 26, spec #225; track B packs): implement the
  `Ecosystem` trait of `package.rs` in a module of its own under `package/` (`npm`, `pypi`,
  `cargo`, `go`, `maven`, `nuget`, `swiftpm` today) and add it to `ECOSYSTEMS`; the package kind,
  the index writer and the packs learn no name. An arm for one ecosystem added to the shared code
  is a finding. The questions and their defaults: [map/seams.md](map/seams.md).
- Entries per library and shared reader, in [map/seams.md](map/seams.md): `dotnet/` (what the
  .NET pack and ecosystems share, as `jvm/` does for the JVM), a Go router (`ROUTERS` in
  `http/go.rs`), a Go HTTP client (`http/go/clients.rs`), the Go, Java, Kotlin and C# channels
  (`channel/<language>.rs`) and shapes through package imports (`Code::imported`). Go environment
  reads are in [map/languages.md](map/languages.md).
- New **message library** (spec #20, tickets after #45 and track B packs): a form in the
  channel kind's module for that language (TS/JS: `channel/ts.rs`; Python: `channel/python.rs`; Rust: `channel/rust.rs`; Go: `channel/go.rs`; Java: `channel/java.rs`; C#: `channel/csharp.rs`), never in the pack, matching a call's
  method and the `Code::origins` of its receiver, asked with the policy its language needs (TS/JS:
  `Constants`, the form of the call that made the value, outside name and steps; Go: `Declarations`; Java: the written type, `names::typed`; C#: the written type or the construction, `Code::written` and `names::spellings`; Python and Rust:
  `Values`, the constructors named by the file's imports), reading the name through `Code::expand` (a reader that
  answers `None` when the name holds a parameter makes the enclosing function a client wrapper)
  and giving the handler. A wildcard match (`inferred`, reason `<producer contract> wildcard
  <p>`) is an arm of `channel.rs`'s `wildcard(broker, pattern, name)`, which `fits` calls; a
  binding match (reason `<queue contract> binding <exchange> <p>`) is `routed`. `reaching`
  joins both for `link` (with the whole workspace's providers and exports, from which it builds `Routes`) and
  `Channel::related_sides` uses `fits` and `Routes::reached`, so `context` on a channel shows
  every side a link would join. Something that is neither side, like a binding or a publish to
  a named exchange, is a `Found` with its own `Role`, an export list of its own
  (`Export::bindings`, `Export::publishes`) and, to show in `context` on a channel, a line in
  `Channel::related_sides`. A form whose site is a
  constructor (`new SendMessageCommand(...)`, `new Worker(...)`) reads the call fact of the
  `new`, with its arguments, like any other call.
- New **document format or document data** (spec #21 tickets after #50): what is read from the
  bytes goes in `document::Summary` (re-bless `tests/golden/schema.txt`), a reader beside `openapi.rs` and
  `asyncapi.rs` sees
  only `serde_json::Value`, a problem is a `Problem` variant rendered in `document::report`
  through `Msg`, and contracts come from `Summary` at query time in the kind's `declare`
  (HTTP: `Http::declare`; channels: `Channel::declare`). A new gateway form is one more `Gateway` in `Provider::gateways`, a
  new host one more entry in `Provider::hosts`. A payload schema becomes a shape through
  `schema::read`; a new kind of shape source is a `shape::Source` variant, never a new shape
  type.
- New **shape source** (spec #23): a provider's is a `shape::Source` variant, never a new shape
  type, with an arm in `Source::at` and `Source::declared`, in the order `http::ts::provider_shape`
  and `Http::declare` apply (a document, then a zod schema, `zod.rs`, then the returned
  literals, `literals.rs`). A consumer's (field reads, destructuring, iteration, declared types)
  adds no variant: every consumer shape is `Source::Reads`, and a source adds its paths through
  `Shape::list`, which fills `Shape::declared` for the paths a type gives and `Shape::optional`
  for those it marks optional, keeping both within `fields` (a path is optional only while
  everything listing it says so). A source that knows scalar leaves fills `Shape::scalars`
  (documents do, from primitive `type`s), and a missing path is `exact` only when the
  provider's source is declared and the path is in the consumer's `declared`. A reader of code
  charges its per-file limit to a `shape::Budget`, and an extraction holds one reader of each
  source (`Literals` holds its `Types`, which hold their `Zod`), so each limit is spent once per
  file. What a pack reads from code
  for it is a shape fact: an optional part of the pack's output, kept out of the index like
  `Call::args` and read through `Code` (`Code::uses`), so a pack without it is unchanged. The
  TS/JS pack's are in `lang/ts/uses.rs` (`Call::uses`, the `Step`s the enclosing function
  applies to a call's result). The kind turns them into a `Found::shape`, stored as
  `contracts.shape` and exported on the side; `Http::declare` keeps a code shape unless a
  document gives the route one. Drift is one pure function, `shape::drift(provider,
  consumer)`, which `http::drift` runs over the workspace's links in
  `Engine::links_and_decisions`, after the decisions replay, setting `Link::drift`: `trace`
  (`Row::link`), the export and every other reader of links see the same value;
  `drift_view.rs` places it and `meada-core`'s `drift.rs` renders it for `context` (a route's consumers that read fields, with their
  drift), `search` (kind `drift`) and `status` (a count per repository): it computes no drift
  of its own and reads no kind: it reads `Link::drift`, the exports' consumer shapes (which
  consumers read fields), their calls (where each consumer is) and the seam's `parts` (the part
  of a contract `search` ranks against).
- New **match rule** (a path rewrite, a new provider source): the kind's `link`, its `serves`
  and its `candidate` (the questions `items.rs` and `decisions.rs` ask) and each candidate's
  contract must agree.
  `meada decide` writes the candidate's contract as the decision's target and
  `decisions::evaluate` checks it against the link's candidates, so a candidate carrying the
  consumer's contract instead of the provider's own makes every decision on it stale.
- New **entry point kind** (spec #31): a variant of `entry::EntryKind` (its `name` and `parse`
  arms and the list `parse` searches, and its name in the `processes` kinds of the export JSON
  above, which `tests/it/docs.rs` checks). A door that is a side of a contract needs no finder:
  `entry::doors` asks each side's kind (`ContractKind::entry(role, contract)`, which sides make
  an entry point of which variant, a function of the contract alone). Any other door is a
  finder beside `programs`, `doors` and `exports` in `entry::find`, and the `site` its process
  starts from (a symbol or a code file;
  `holds` checks it). The finder reads `entry::Local`: `identity`, `files` (every file's
  `Facts`), `names`, `symbols`, `called`, `exported`, `providers` and `consumers` (`entry::Side`,
  a `site` and a `contract`) and `programs` (`entry::Program`, a `file` and a definition name). A
  kind that needs a fact not there adds a field to `Local` and fills it in `index::refresh`,
  where `Local` is built after the contracts and the `exports` rows are written, just before
  `entry::find` stores its result; a new fact a pack declares is a `Facts` field first (as
  `programs` is). A kind the stored rows already name needs no schema change; a new column does
  (re-bless `tests/golden/schema.txt`). Export entry points are filtered again at query time
  (`entry::drop_imported_exports`): only `export` is dropped, never another kind. The walk (`Graph::process`),
  `trace` from `process::<identity>` and `Export::processes` need nothing else: they read the
  stored entry points. A process is walked on demand from the stored calls and the links of the
  query, never stored; a new thing the walk must follow is one more `Edge` in `Graph::edges` (the
  one place that says which edges a step expands, which `Graph::processes_of` inverts for `context`) and its arm
  in `Descent::follow`, with its `Mark` (named in the `Mark` list of `map/queries.md`, which
  `tests/it/docs.rs` checks) and its line in `Walk::show`. A change to what is stored re-blesses
  `tests/golden/schema.txt`.
- New **level**: add the variant to `Level` in `contract.rs` and keep the order strongest first,
  since `Ord` makes a chain's weakest link the max. `Level::name`/`parse` are the only place names
  live; `engine.rs` and `trace.rs` render through them.
- New **config key**: add the field to `Config`. Parse errors name file,
  line and dotted key by re-parsing the TOML for spans (`config::describe`).
- New **MCP tool**: `mcp.rs`, one `Engine` method that builds the tool's result, its result
  type and text answer in a module of `meada-core` beside `search.rs`, `context.rs` and
  `trace.rs` (no file, index or process access there), and watch `tests/it/mcp.rs` and
  `tests/it/server.rs`: the tool list (descriptions plus parameter schemas) must stay within
  1,000 tokens. A tool is added once for both transports: the `#[tool_router]` of `Meada` serves
  stdio and every HTTP endpoint.
- New **CLI text**: a `Msg` variant with both languages; command descriptions are set at runtime
  in `main.rs` from `Msg`.

## Invariants the code relies on

- Server mode writes nothing: the engine opens every index read-only, `refresh()` does nothing,
  and the HTTP listener opens no outbound connection. Only `meada index --workspace` writes, and
  only under `index_dir`.
- Determinism: all export lists are sorted, hashes are BLAKE3, no clock or random value reaches
  an answer. A refreshed index must equal a cold index of the same files (`tests/it/freshness.rs`),
  communities and processes included: `refresh` recomputes both at its end from the stored calls
  whenever anything changed. The test-only `with_reversed_walk` (`index.rs`, hidden from the
  docs, no CLI flag or setting) reverses the walk order, the parse order and the insertion order
  of the `symbols`, `calls` and `candidates` rows, and the chain's export must stay
  byte-identical. The in-memory order is normalized by path (`BTreeMap`s), so the switch covers
  nothing beyond that. Indexing runs on one thread, so there is no worker count to vary.
  A refresh keeps the rows of a group of packs that resolve together when nothing it reads
  moved (#391): that rests on a group's extraction reading only its own files and finding
  contracts only in them, which `refresh` checks with debug assertions
  ([map/indexing.md](map/indexing.md)).
- The schema version (`index::schema_version`) is derived from `tests/golden/schema.txt`, the
  stored format: `SCHEMA`, the facts of each pack's `tests/fixtures/schema/sample.<extension>`
  and the summaries of the document and manifest samples there. A unit test in `index.rs`
  (`the_stored_format_matches_its_golden`) fails on any change to them until `MEADA_BLESS=1`
  rewrites the golden, which changes the version; no number is kept by hand, so two branches
  that change the format conflict only in the golden. An index with another version, or an
  unreadable one, is rebuilt beside it and renamed over it, and `status` says so. One whose stored facts do not
  parse is rebuilt by the first refresh that reads them (when a file changed), which `status`
  cannot tell.
- The index holds no source text and no values of code, apart from the BLAKE3 of each symbol's span
  (`symbols.hash`, `Def::hash`) and the facts listed under "Stored values" in
  [map/indexing/stored-values.md](map/indexing/stored-values.md): argument counts and labels, names a file
  declares or imports, summaries of manifests, build files and provider documents, the field
  paths of shapes, entry points and communities, and the config values of YAML and JSON files
  (ADR 0004 amended, ADR 0025). A value the index now stores joins that list. A
  contract kind that reads a value from a new place relies on the loop of misses (`Code::read`,
  `valued`) that `refresh` runs until none is left. The values cache beside the index
  (`values.db`, ADR 0027) gives back what a pack parsed of a file, values included, by the file's
  bytes, setting, pack and Meada's build: no query reads it, and a hit is what a parse gives.
- Links are never stored. They derive from the stored provider and consumer contracts, then the
  decisions replay, when a query runs, so a change on either side, or to a decisions file, needs
  no relinking. `Level` order: `exact` < `declared` < `inferred` < `decided` < `ambiguous`.
- Network: `decider::Config::send` is the only code that opens a connection, and only
  `meada decide --decider` without `--show-request` or `--list` reaches it. Indexing, queries,
  `status` and the MCP server never read the `MEADA_DECIDER_*` variables. `ureq` is the only
  HTTP dependency.
- One index per identity: a second clone of a repository already in the workspace is skipped.
- `context` shows span text only while the file's BLAKE3 still equals the indexed hash; on a
  mismatch it forgets the file's mtime, refreshes and retries once.
- Facts resolve only among files of the same pack, and inference runs only when resolution
  fails, among the files the caller reaches (`Layout::reaches`); an import from outside the
  repository is never inferred. A file its pack leaves out
  (`Layout::members`) resolves nothing and has no symbol, call or `files` row in the export.

## Deployment (`deploy/`)

The compose profiles (`deploy/compose/`), the console overlay, the local accounts recipe, the
Helm chart (`deploy/helm/meada/`) and the order of the managed refresh job:
[map/deployment.md](map/deployment.md). `scripts/compose-check` and `scripts/helm-check` check
the rendered configuration.

## Tests (`tests/`)

- The integration tests are one binary, `tests/it/main.rs`, so `cargo test` links it once: a
  module per subject (`tests/it/<subject>.rs`) and the harness in `tests/it/common/`. A new
  subject is a file there and its `mod` line in `main.rs` (`every_file_here_is_a_module` checks
  it); `cargo test --test it <subject>::` runs one.
- **Golden files** live in `tests/golden/<fixture>.<what>.{txt,json}` and are compared with
  `assert_golden(name, actual)`. `MEADA_BLESS=1 cargo test` rewrites them; the diff is the
  review, read per section with `cargo run --example golden_diff` (`--range` picks the
  revisions; the default is `origin/main...HEAD`). `.gitattributes` marks fixtures and goldens `-text`, since BLAKE3 hashes and byte
  ranges are in them, and `CHANGELOG.md merge=union` (`docs::changelog_union_merge_keeps_both_bullets`), so same-heading entries from two branches merge without a conflict. An export golden of a test about something else goes through
  `without_processes(export)`, which drops `processes`: only the chain's own exports
  (`chain-{web,gateway,catalog,worker}.export.json`), the conformance export and the
  `*.processes.json` goldens carry them (spec #31), so a change to the process walk re-blesses
  those alone.
- Test files by subject, the fixtures each uses and the loopback decider stub: [map/tests.md](map/tests.md),
  split into parts (below).
- A new behavior gets a fixture repo plus a golden; reuse a fixture before adding one. Fixture
  `meada.toml` files declare the peers and `serve.hosts`.

## Split area files

Every Markdown file under `docs/agents` stays within its byte budget (`MAP_BUDGET` for this map,
`AREA_BUDGET` for the rest, both 64 KiB; `tests/it/docs.rs` checks it). A file past it is split by
its `## ` headings with `cargo run --example split_agents -- <file>` in the same change, never
trimmed by dropping entries: the file keeps its introduction and one line per part, and each part
is a file of its own in a directory named after it. Three are split so far:

- [map/tests.md](map/tests.md): [harness](map/tests/test-crates-harness-and-fixtures.md),
  [test modules by subject](map/tests/test-modules-by-subject.md),
  [package, channel and gateway fixtures](map/tests/package-channel-and-gateway-fixtures.md),
  [workspace editor and shared validation](map/tests/workspace-editor-and-shared-validation-310.md),
  [Java declarative clients](map/tests/java-declarative-clients-90.md),
  [Java conformance](map/tests/java-conformance-84.md) and the named workspaces parts
  ([creation to query](map/tests/named-workspaces-creation-to-query-342.md),
  [versions and overlap](map/tests/named-workspaces-versions-and-overlap-343.md),
  [context configuration](map/tests/named-workspaces-context-configuration-344.md),
  [acceptance](map/tests/named-workspaces-acceptance-347.md)).
- [map/indexing.md](map/indexing.md): [modules in brief](map/indexing/modules-in-brief.md),
  [stored values](map/indexing/stored-values.md),
  [discovery, the index, adapter](map/indexing/detail-discovery-index-adapter.md),
  [message forms, Python and Rust](map/indexing/detail-message-forms-python-rust.md),
  [Go, resolution and JVM builds](map/indexing/detail-go-resolution-jvm-builds.md),
  [Java receivers and supertypes](map/indexing/java-receivers-and-supertypes-85.md) and
  [Java source facts](map/indexing/java-source-facts-84.md).
- [ceilings.md](ceilings.md): the limits of the code, by area
  ([resolution and the index](ceilings/resolution-the-index-and-shape-readers.md),
  [language packs](ceilings/language-packs.md),
  [HTTP: JVM, Go, Rust, TS](ceilings/http-clients-jvm-go-rust-ts.md),
  [HTTP: C# and Swift](ceilings/http-clients-csharp-swift.md),
  [channel clients](ceilings/channel-clients.md),
  [server, accounts, workspaces and deploy](ceilings/server-accounts-workspaces-and-deploy.md)).

## Known ceilings

The limits of the code, each with its value and its reason, are in [ceilings.md](ceilings.md) and
the parts it lists: read the part of the area when a limit is touched. A limit marked `ponytail:` is
named in one of them (`tests/it/docs.rs` checks it).
