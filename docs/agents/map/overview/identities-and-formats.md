# Identities and formats

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
