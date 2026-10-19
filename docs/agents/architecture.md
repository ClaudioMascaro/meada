# Architecture map

Which module owns what, the seams between them, and what the code relies on. Verified against
`src/` and `tests/` of the v0.1 trunk; the code wins over this file, and a change to a seam
updates it. Per-area detail lives in `map/`, one file per area, each named from the module
list below. Terms are `GLOSSARY.md`'s; decisions are in `docs/adr/`.

## Flow

`Engine::open(dir)` finds the **workspace**, creates or recreates each **repository**'s **index**
in its own `.meada/index.db`, and ATTACHes every **peer** to the first repository's connection
as `peer<n>`. Every query (`export`, `search`, `context`, `trace`) calls `refresh()` first, then
reads each index through `index::export`, adds the contracts each `meada.toml` declares
(`ContractKind::declare`), derives **links** from both, and renders.
Nothing outside `.meada/` is written. Everything the engine returns is sorted: the same files
give byte-identical output.

## Modules (`src/`)

Each line names what a module owns; the file in `map/` it points to has the detail. Read the
one for the area a change touches.

- `engine.rs`: `Engine`, the one query seam (`open`, `refresh`, `export`, `search`, `context`,
  `trace`, open items, decisions, `status`); it renders `search` and `context`. `items.rs`: open
  items, derived in one place. `trace.rs`: the `Graph`, the walk, the chain footer and the
  process walk (`Graph::process`, the step list `trace` renders and the export carries).
  `entry.rs`: entry points (spec #31), found when an index is written.
  `drift_view.rs`: how `context`, `search` and `status` show `Link::drift`.
  `render.rs`: token budget and line helpers. `mcp.rs`: three read-only tools. `main.rs`: clap.
  `msg.rs`: the CLI message catalog. Detail: [map/queries.md](map/queries.md).
- `decide.rs`, `decisions.rs`, `decider/`: `meada decide`, the decisions file and its replay, and
  decider mode (the only network path). Detail: [map/decisions.md](map/decisions.md).
- `workspace.rs`, `config.rs`, `repo.rs`: repositories, peers, `meada.toml` and identities.
  `index.rs`: the SQLite schema, the freshness pipeline in `refresh`, and `export`. `lang/`: the
  language adapter and the TS/JS, Python and Rust packs. `resolve.rs`: calls to definitions (`Resolver`), values
  and receivers (`Code`). `community.rs`: the deterministic Leiden behind communities (spec
  #31). Detail: [map/indexing.md](map/indexing.md).
- `contract.rs`, `http.rs`, `http/rust.rs`, `resource.rs`, `channel.rs`: the `ContractKind` trait
  and the HTTP (`http/rust.rs`: the actix-web and Rocket providers), resource and channel kinds. `package.rs`: npm, PyPI and Cargo packages as contracts (specs #22,
  #24 and #30). Detail:
  [map/contracts.md](map/contracts.md).
- `document/`, `shape.rs`, `literals.rs`, `zod.rs`: OpenAPI and AsyncAPI provider documents, the
  shape a contract carries, and the shapes read from a handler's returned literals and zod
  schemas (`lang/ts/types.rs` finds the schema a `z.infer` annotation names). Detail:
  [map/documents.md](map/documents.md).

## Identities and formats

- Repository identity: `github.com/acme/web` (lower-cased host and path, no scheme, user, port
  or `.git`) or `local/<dir>`.
- Symbol: `<repository>::<path>::<qualified name>` (`Class.method`, `outer.inner`).
- Call site: `<caller>::<normalized callee>#<ordinal>`; a top-level call's caller is
  `<repository>::<path>`. The normalized callee keeps identifier and member chains only; any
  other expression becomes `(<node kind>)`. A Rust path is dotted like a chain (`crate::a::f`
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
  `Engine::exports` drops, after `package::import`, each one that is the target of an import
  from its own repository (a monorepo), at any level.
- Package: `package::npm::<name>`, `package::pypi::<name>` (the PEP 503 normalized name) or
  `package::cargo::<name>` (the `[package] name`; a dependent's site names the key it writes, so
  a renamed crate has its own site),
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
- Export JSON: `{repository, files, symbols, calls, providers, consumers, bindings, documents,
  publishes, packages, dependencies, imports, communities, links, processes}`;
  `communities` (absent when empty) lists each community as `{name, members}`, by name, with
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

- New **contract kind**: implement `ContractKind`, add it to `KINDS`. `extract` reads `Code`
  (never the syntax tree) and returns `Found` with a `Site` (`Def`, `Inline`, `File`, `Call`);
  a kind fed by `meada.toml` instead overrides `declare(repo, export)`, which `Engine::exports`
  calls at query time (nothing stored, so a config edit needs no reindex); `link` receives the
  whole workspace's providers. A kind's rules stay in its own file. A kind whose consumers
  need providers from other repositories (a package depends on one the workspace holds) adds a
  workspace step: `Engine::exports` runs it after every `declare`, as `package::consume` (the
  dependency entries) and `package::import` (the calls through an import) are, over a
  `Providers` index built once, and `link` reads what the step left on the consumer. Such a
  consumer is no open item when ambiguous: `items.rs` (`decidable`) skips the contracts of the
  kind explicitly, since several providers of a name are no call for a person to decide. Its
  sites show in `context` and `search` through hooks in `engine.rs`: `package::place` names where
  a site sits (`<path> <field>`), the `consumers:` section of `context` on a symbol adds the
  calls that reach it through a hop (`package::reached`), and `contract_sides` lists the sides
  of the contract itself, with the roles `sides` names.
- A **builder** language (spec #30 #129: a call whose method returns its receiver, such as
  `Router::new().route(..)`): the pack sets `LanguagePack::fluent` and gives each call its
  `Call::receiver` (a value emitted from the syntax, not stored in the index, like `args`).
  `Code::origins` then walks a chain down its receivers, by `call_at` (a call found by its byte
  range; `Code::call_index` for a kind) and `Code::receiver` (the value a call is made on).
  `Code::chain` is that walk, memoized by `chain_tail`: the calls from one down to where the
  flow stops (a call a function of the repository answers, or one made on no call). A kind
  that reads a chain step by step (an axum method router, an actix-web `ServiceConfig`, a
  Kafka record, an SQS request, redis read options) goes through it, never its own walk.
  A step no function of the repository answers passes the receiver on; one only a name match
  found does when the chain starts at a constructor of an outside crate (whose method it then
  is), else the flow ends. Which methods pass the object on is listed per object, with its
  constructors: `FRAMEWORKS` in `lang/rust/frameworks.rs` for the web frameworks and HTTP
  clients (`passes`, and `frameworks::made`, which the HTTP kind reads), `LIBRARIES` in
  `lang/rust/messages.rs` for the brokers. A kind reads a route off each call of the chain (`http::axum`, and `http::rust`: actix-web
  `App`, `web::scope`, `web::resource`, `.configure(f)` and Rocket's `build().mount(..)`, whose
  `routes![..]` the Rust pack reads as a list of the handlers' paths).
  `Code::returned` and `Code::constant` give a kind the values a call may return and a
  constant holds, for what it must not leave unmounted.
- New **package ecosystem** (spec #22 user story 26, track B packs): a variant of
  `package::Ecosystem` (npm and PyPI today) plugs in another manifest format (`Cargo.toml`,
  `go.mod`) without a new kind of contract. `Ecosystem::of` names the manifest files by file
  name, which `package::is_manifest` makes `manifests_found` (`index.rs`) stamp into
  `manifests`; `Ecosystem::read` (behind `package::read_manifest`) turns the bytes into the same
  `Summary` (name, version, dependency entries, entry fields, source roots, console scripts) and never fails;
  `Ecosystem::prefix` gives the contract `package::<ecosystem>::<name>`, so `PREFIX` still
  routes it to the same `declare`, `consume`, `link`, `place` and `reached`; `Providers` is
  keyed by ecosystem and name, so two ecosystems never link; and the reason grammar
  (`reason`) stays one function, with `Ecosystem::flag` checking the range (npm semver through
  `flag`; PyPI's and Cargo's are never evaluated, `range unchecked`) and `Ecosystem::reaches` holding a
  range rule like npm's `workspace:`. Which ecosystem a call through an import belongs to is
  its file's (`Ecosystem::of_source`; Cargo's for a `.rs` file), and resolving the import into the provider is the
  ecosystem's own rule in `resolutions` (npm: `split`, `ways`, `Provided::resolve`; PyPI:
  `pypi_resolutions`, `Provided::module`; Cargo: `cargo::names_crate`, the `use` head matched to
  the key of a renamed dependency or to the provider's library name (`cargo::lib_head`), then
  `Provided::resolve` from the crate's library root, `How::CrateName`). A step outside
  `ContractKind` fills what a manifest inherits: `cargo::inherit`, run in
  `index::read_manifests` over the whole manifest list after the reads, fills the `workspace =
  true` entries and `version.workspace = true` of each `Cargo.toml` from the nearest one above it,
  so a refresh equals a cold export. `Ecosystem::reaches` takes the repositories' roots (`Export::root`, set by
  `Engine::exports`) for a Cargo `path` that leaves its repository: it folds both locations as
  paths, never opening one, so a symlink changes nothing. A change to what is stored bumps `SCHEMA_VERSION`.
- New **message library** (spec #20, tickets after #45 and track B packs): a form in the pack's
  `messages` (TS/JS: `lang/ts/messages.rs`; Python: `lang/python/messages.rs`; Rust: `lang/rust/messages.rs`), matching a call's
  method and the `Code::origin` of its receiver (package and `expr`; Python: the constructors
  `Code::origins` finds, named by the file's imports), reading the name through `Code::expand` (a reader that
  answers `None` when the name holds a parameter makes the enclosing function a client wrapper)
  and giving the handler. A wildcard match (`inferred`, reason `<producer contract> wildcard
  <p>`) is an arm of `channel.rs`'s `wildcard(broker, pattern, name)`, which `fits` calls; a
  binding match (reason `<queue contract> binding <exchange> <p>`) is `routed`. `reaching`
  joins both for `link` (with the whole workspace's providers and `Routes`) and
  `Engine::contract_sides` uses `fits` and `Routes::reached`, so `context` on a channel shows
  every side a link would join. Something that is neither side, like a binding or a publish to
  a named exchange, is a `Found` with its own `Role`, an export list of its own
  (`Export::bindings`, `Export::publishes`) and, to show in `context` on a channel, a line in
  `Engine::contract_sides`. A form whose site is a
  constructor (`new SendMessageCommand(...)`, `new Worker(...)`) reads the call fact of the
  `new`, with its arguments, like any other call.
- New **document format or document data** (spec #21 tickets after #50): what is read from the
  bytes goes in `document::Summary` (bump `SCHEMA_VERSION`), a reader beside `openapi.rs` and
  `asyncapi.rs` sees
  only `serde_json::Value`, a problem is a `Problem` variant rendered in `document::report`
  through `Msg`, and contracts come from `Summary` at query time in the kind's `declare`
  (HTTP: `Http::declare`; channels: `Channel::declare`). A new gateway form is one more `Gateway` in `Provider::gateways`, a
  new host one more entry in `Provider::hosts`. A payload schema becomes a shape through
  `schema::read`; a new kind of shape source is a `shape::Source` variant, never a new shape
  type.
- New **shape source** (spec #23): a provider's is a `shape::Source` variant, never a new shape
  type, with an arm in `Source::at` and `Source::declared`, in the order `http::provider_shape`
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
  for it is a shape fact: an optional part of the pack's output, `serde(skip)` like
  `Call::args` and read through `Code` (`Code::uses`), so a pack without it is unchanged. The
  TS/JS pack's are in `lang/ts/uses.rs` (`Call::uses`, the `Step`s the enclosing function
  applies to a call's result). The kind turns them into a `Found::shape`, stored as
  `contracts.shape` and exported on the side; `Http::declare` keeps a code shape unless a
  document gives the route one. Drift is one pure function, `shape::drift(provider,
  consumer)`, which `http::drift` runs over the workspace's links in
  `Engine::links_and_decisions`, after the decisions replay, setting `Link::drift`: `trace`
  (`Row::link`), the export and every other reader of links see the same value;
  `drift_view.rs` renders it for `context` (a route's consumers that read fields, with their
  drift), `search` (kind `drift`) and `status` (a count per repository): it computes no drift
  of its own, reading `Link::drift`, the exports' consumer shapes (which consumers read fields)
  and their calls (where each consumer is).
- **Shapes through package imports** (issue #175): a source reading a name that `Code::imported`
  says comes from outside the repository records a `shape::Import` on the shape (its path, the
  importing file, the specifier and name, and the `Want` view: `zod`, `infer`, `literals`,
  `type`, or `opens` for a provider typed with a package type, which takes that type's index
  signatures alone) instead of opening it or dropping it. A repository keeps, beside its
  contracts, the shapes of the module-level constants and types the files of its packages'
  public surface export (`package::surface`: the entries of each named manifest, as an import
  resolves them, and what their `exports` rows lead to; `package::exported_shapes`, a step
  `index::refresh` runs after the kinds' `extract`, as
  `Role::Shape` rows; each source reads its own: `Types::exported`, `Zod::exported`,
  `Literals::exported`). The workspace step `package::shapes`, after `package::import` in
  `Engine::exports`, resolves each import as a call through it resolves (`Provided::resolve`,
  over the `exports` rows, which name constants and types too) and fills it with
  `shape::fill`, the one recursion and the one place its limits live, carrying the
  resolution's level: through one that is not `exact` (an `outDir` mapping) a provider's part is
  `guessed` and a consumer's paths are not declared, so no drift on them is `exact`. A new source that reads
  an exported name adds a `Want`, a reader's `exported` and an arm in `exported_shapes`.
- New **match rule** (a path rewrite, a new provider source): the kind's `link`, the open-item
  candidates in `items.rs` (`http::route_fits`) and each candidate's contract must agree.
  `meada decide` writes the candidate's contract as the decision's target and
  `decisions::evaluate` checks it against the link's candidates, so a candidate carrying the
  consumer's contract instead of the provider's own makes every decision on it stale.
- New **entry point kind** (spec #31): a variant of `entry::EntryKind` (its `name` and `parse`
  arms and the list `parse` searches, and its name in the `processes` kinds of the export JSON
  above, which `tests/it/docs.rs` checks), a finder beside `routes`, `channels`, `programs` and
  `exports` in `entry::find`, and the `site` its process starts from (a symbol or a code file;
  `holds` checks it). The finder reads `entry::Local`: `identity`, `files` (every file's
  `Facts`), `names`, `symbols`, `called`, `exported`, `providers` and `consumers` (`entry::Side`,
  a `site` and a `contract`) and `programs` (`entry::Program`, a `file` and a definition name). A
  kind that needs a fact not there adds a field to `Local` and fills it in `index::refresh`,
  where `Local` is built after the contracts and the `exports` rows are written, just before
  `entry::find` stores its result; a new fact a pack declares is a `Facts` field first (as
  `programs` is). A kind the stored rows already name needs no schema change; a new column does
  (`SCHEMA_VERSION`). Export entry points are filtered again at query time
  (`entry::drop_imported_exports`): only `export` is dropped, never another kind. The walk (`Graph::process`),
  `trace` from `process::<identity>` and `Export::processes` need nothing else: they read the
  stored entry points. A process is walked on demand from the stored calls and the links of the
  query, never stored; a new thing the walk must follow is one more `Edge` in `Graph::edges` (the
  one place that says which edges a step expands, which `Graph::processes_of` inverts for `context`) and its arm
  in `Descent::follow`, with its `Mark` (named in the `Mark` list of `map/queries.md`, which
  `tests/it/docs.rs` checks) and its line in `Walk::show`. A change to what is stored bumps
  `SCHEMA_VERSION`.
- New **level**: add the variant to `Level` in `contract.rs` and keep the order strongest first,
  since `Ord` makes a chain's weakest link the max. `Level::name`/`parse` are the only place names
  live; `engine.rs` and `trace.rs` render through them.
- New **language pack**: add it to `PACKS` in `lang/mod.rs`, a fixture
  `tests/fixtures/conformance-<language>/` and one `conform(...)` test in `tests/it/conformance.rs`.
  A pack is done when that suite passes. A pack whose facts depend on its config files
  (Rust's default features) also gives `settings` (a string per file path, stored as
  `files.setting` and `Facts::setting`) and takes it in `parse_with`; a file whose setting
  changed is parsed again, whatever its content. Besides `parse` and `modules`, it says what the
  shared resolution cannot guess: `receiver` and `base_receiver` (keywords for the enclosing
  class and its base), `globals` (names bound everywhere), `Def::bases` (a class's bases, for
  its family), `Facts::receivers` (the class each binding gives a name), `Facts::star` (what a
  star import takes), `stars_bind` (whether a star import rebinds names in statement order, as
  Python's does, or only re-exports, as an ES `export *` does) and `Param::binds` (which
  argument binds each parameter). A pack that
  writes none of them (TypeScript writes no bases or receivers) gets name matches where those
  would have resolved. A language whose files are not all built, or whose imports name files by
  a tree spread over several files (Rust's crates and `mod` declarations), says so through
  three more: `config_files` names with a `**/` prefix (every `Cargo.toml` the walk finds, in
  path order, each read as a root config is (`read_config`) and folded into the configs digest on
  every refresh) and `layout`, which `refresh` calls once per pack and refresh with the configs
  and every file the pack parsed, and which answers four things from one build (Rust's crate
  `Tree`, so the manifests are parsed and the module trees walked once): `Layout::members` (the
  files it builds; the rest keep their facts in `files` with an empty `resolved` and no rows,
  and leave the export), `Layout::modules` (by default the pack's `modules`), `Layout::published`
  and `Layout::programs` (both below). A language whose imports name a path rather than a file (Rust's
  `use a::b as c`) writes `Imported::Path` (what the dotted callee-like name gives, resolved
  from the scope of the import's own `local`, which a pack qualifies by the scope that holds
  it: `inner.c`, `f.c`) and `Imported::Glob` (every name the module the path gives takes:
  `inside` it, the module's private items too; `public`, a `pub use` also re-exports them),
  both found by `Resolver::lexical` at each enclosing scope (`imported_at`) and, for the file
  itself, by `imported` and `export`; explicit imports beat globs, and an alias or a glob
  that reaches itself gives nothing (`Seen`: the names being resolved, the globs by file, import
  and name, and the depth of links followed, bounded by `LINK_DEPTH`). A call name is never read
  as a `Kind::Module` (`Reading::Value`); a use path is (`Reading::Alias`). `Facts::externs` names the first segments of
  paths that nothing in the file binds (the crates outside it), resolved last in `imported`:
  a glob beats them, another crate of the repository answers through `Modules` and the rest
  is `Binding::External`. `Facts::private` lists the `exports` names `Resolver::exported`
  leaves out of the exports table (Rust's non-`pub` items: they still resolve inside the
  crate), and `Layout::published` (default: none) the files whose exports a pack counts
  at all (`Resolver::publishing`, called by `refresh`): Rust's are the files reached from a
  library root through `pub mod` declarations, so a binary crate exports nothing. A block that adds members to a type written elsewhere (Rust's
  `impl`) is a `Facts::impls` entry (`Impl {scope, ty, of, within}`), which `Resolver::member`
  (`block_member`, before the class's own names: the inherent blocks' definitions, else the
  trait blocks' or their traits' default bodies; several at that step are `Binding::Several`,
  an `ambiguous` call with exactly those candidates) and `Resolver::class` (`Self` inside a
  block) read. A module is `Kind::Module`: its functions are name-match candidates like
  top-level ones, a `Kind::Module` that an import of its own name binds to a file (Rust's `mod
  store;`) has that file's exports as members, and `Resolver::lexical` does not climb past
  one (the names around a module are not its own; the file's imports still are). A call
  written as a path sets `Call::by_path`: its head is read among types and modules alone
  (`lexical_as`, no local or parameter hides it), and a failed resolution matches functions as
  well as methods by name. A pack whose types are closed (`LanguagePack::strict`, Rust) has a
  member its blocks lack on a type the receiver or the path head writes resolve to nothing
  (`Resolver::lacks`: a derive, a blanket or outside trait), not by name, and its attribute
  calls (`Call::decorates`) resolve to nothing, read only through `Code::external_of`. A pack that has program entry points fills
  `Facts::programs` in `parse` (it sees the path and the source; default: none) with the
  `Def::name` of each definition that starts a program; each becomes an entry point of kind
  `program`, labelled by its qualified name, and the conformance test lists them. Where the
  entry point is top-level code, `parse` yields a definition for that code (the calls it makes
  belong to it) and names it. A name with no symbol is dropped silently: the pack's conformance
  golden catches a wrong one. A form declared outside the code file (Python `[project.scripts]`,
  which names a function of another file) needs a repository-level step feeding
  `entry::Local::programs` with `entry::Program`s: for Python, `package::python_programs` maps the
  `Summary::scripts` of each `pyproject.toml` (stored with the manifest rows, whose digest makes an
  edit re-extract) to files through `python_roots`. A form only the whole repository tells
  (Rust `fn main`, a program only in a binary target's root, which one file's `parse` cannot
  see) is `Layout::programs`, which `refresh` takes from each pack's `layout` beside
  `python_programs`; Rust asks the crate `Tree` for its binary roots (`Tree::bins`). The forms per language are spec #31 "Program entry points per language
  pack": Python `if __name__ == "__main__":` as symbol `__main__` plus each `[project.scripts]`
  target; Go `func main` in package `main`; Java `public static void main(String[])`; Kotlin a
  top-level `fun main`; C# a `static Main` and, for top-level statements, a symbol `Main`; Swift
  an `@main` type and `main.swift` as symbol `main`; Rust `fn main` in `src/main.rs` and
  `src/bin/`. TS/JS declares none.
- New **config key**: add the field to `Config`. Parse errors name file,
  line and dotted key by re-parsing the TOML for spans (`config::describe`).
- New **MCP tool**: `mcp.rs`, one `Engine` method, and watch `tests/it/mcp.rs`: the tool list
  (descriptions plus parameter schemas) must stay within 1,000 tokens.
- New **CLI text**: a `Msg` variant with both languages; command descriptions are set at runtime
  in `main.rs` from `Msg`.

## Invariants the code relies on

- Determinism: all export lists are sorted, hashes are BLAKE3, no clock or random value reaches
  an answer. A refreshed index must equal a cold index of the same files (`tests/it/freshness.rs`),
  communities and processes included: `refresh` recomputes both at its end from the stored calls
  whenever anything changed. The test-only `with_reversed_walk` (`index.rs`, hidden from the
  docs, no CLI flag or setting) reverses the walk order, the parse order and the insertion order
  of the `symbols`, `calls` and `candidates` rows, and the chain's export must stay
  byte-identical. The in-memory order is normalized by path (`BTreeMap`s), so the switch covers
  nothing beyond that. Indexing runs on one thread, so there is no worker count to vary.
- `SCHEMA_VERSION` (`index.rs`) bumps on any change to `SCHEMA`, to the facts a pack stores or to
  a document's or manifest's stored `summary`. A unit test in `index.rs` keeps all of them in
  `tests/golden/schema.v<version>.txt` (the facts of each pack's
  `tests/fixtures/schema/sample.<extension>`, the summaries of `sample.openapi.yaml`,
  `sample.asyncapi.yaml` and `sample.package.json` there) and never re-blesses it in place: a change
  fails until the bump, then `MEADA_BLESS=1` writes the new version's file and removes the old.
  An index with another version, or an unreadable one, is deleted and rebuilt, and `status` says
  so. One whose stored facts do not parse is rebuilt by the first refresh that reads them (when
  a file changed), which `status` cannot tell.
- The index holds no source text and no values, apart from the literal URL path fragments and the
  method of an unresolved consumer (`contracts.fragments`, `contracts.method`), the field paths
  of the shape the code gives a side and its source file (`contracts.shape`: the fields an HTTP
  consumer reads from the parsed response, and those of the zod schema or object literals a
  route handler returns, spec #23, with the parts a package import gives as the specifier, the
  name imported, the importing file and the view, issue #175), the shapes a repository with a
  named manifest keeps for the constants and types its files export (`contracts` rows of role
  `shape`: the exported name in `method`, the view (`shape::Want`) in `via`, and field paths, open
  parts, scalar leaves, declared and optional paths and the imports they leave to fill, issue
  #175; what crosses repositories is this shape of an exported name,
  never its source text), the names of the Cargo features a Rust crate enables by default
  (`files.setting`, a comma-separated list per file, from `[features] default`), and what a
  provider document states (`documents.summary`: format, version, title, normalized route paths
  and methods, channel sides (broker kind, name, role), the field paths of their response and
  payload shapes (fields, open parts, scalar leaves) and the external references met, the
  hosts and server paths of the root `servers` URLs, and the parser's reason and line for a
  document it could not read), and what spec #22 stores of the build and package files: the
  `manifests` summaries of each `package.json` (name, version, the dependency ranges as
  written, `exports` as written, `types`, `module` and `main`) and, spec #24, of each
  `pyproject.toml` (normalized name, version, the dependency specifiers as written, the source
  roots and the console scripts it declares) and `requirements*.txt` (the dependency specifiers as written), of
  each `Cargo.toml` (spec #30: name, version or the flag that it inherits one, the library root
  as a path of the repository and the `[lib] name` when it differs from the package's, the
  entries with the version requirement as written, the `alias` when the key renames the crate
  and the `via`: a `path` folded from the manifest's directory, which keeps its leading `../`
  when it leaves the repository, or the normalized `git` URL; `workspace = true` as a flag; the
  `[workspace.dependencies]` entries; the `[workspace.package]` version), the `tsconfig.json` rows
  (`outDir` and `rootDir`, as written) and the `exports` table (each file's exported names and
  the definition, constant, type or file they name), the `entry_points` table (spec #31:
  each entry point's identity, kind, label and start symbol, rewritten with the contracts), and
  each community's name and member identities (`communities`, spec #31). `Call::args`,
  `Call::kwargs`, `Call::uses`, `Call::arg_types`, `Def::returns`, `Def::return_values`, `Def::type_params`, `Param::default`,
  `Field::default`, `Const::value` and `Const::ty` are `serde(skip)`, so unchanged files come
  back from SQLite without them. An `interface` or `type` declaration is a `Const` too, so its
  name is stored (its members are not). `Code` records every file it needs but lacks as a miss, and
  `refresh` re-parses those and extracts again until none is missed. A contract kind that reads a
  value from a new place relies on this loop.
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
  fails; an import from outside the repository is never inferred. A file its pack leaves out
  (`Layout::members`) resolves nothing and has no symbol, call or `files` row in the export.

## Tests (`tests/`)

- The integration tests are one binary, `tests/it/main.rs`, so `cargo test` links it once: a
  module per subject (`tests/it/<subject>.rs`) and the harness in `tests/it/common/`. A new
  subject is a file there and its `mod` line in `main.rs` (`every_file_here_is_a_module` checks
  it); `cargo test --test it <subject>::` runs one.
- `tests/it/common/mod.rs`: `fixture_repo(case, fixture, origin)` copies `tests/fixtures/<fixture>`
  to `$CARGO_TARGET_TMPDIR/<case>/<fixture>` and makes it a git repository (`init`, `origin`,
  one commit). `case` must be unique per test across every subject, since they run in parallel in
  one process. `chain(case)` builds
  the three-repository chain as siblings, plus `chain-worker` (`worker(sibling)`), which reads
  the `postgres` `orders` resource the gateway writes (the catalog reads it at repository
  level) and handles the Kafka topic `orders.created` the gateway's `createOrder` (`PUT
  /api/orders/:id`, which the web's `createOrder` calls) produces through its `publish` wrapper. `loose_chain(case)` adds an unlinked bucket and a `via` naming no symbol to the
  catalog. `git()` pins config, author and dates; `age()` pushes
  mtimes into the past so a later edit always changes them. `decisions(repo, name)` copies
  `tests/fixtures/decisions/<name>.toml` to `<repo>/meada.decisions.toml`; the base chain fixture
  has none. `documented_chain(case)` lays `tests/fixtures/chain-documents/{gateway,web}/` over
  the chain (and copies `generated-openapi.yaml` to the gateway's gitignored
  `generated/openapi.yaml`, which Meada's own git would ignore inside the overlay), then commits:
  the gateway gains `openapi.yaml` (3.1: `GET /api/orders`, merged with the Express route and
  given a response shape with an open part, and
  the documented-only `GET /api/shipments/{id}`; a `servers` URL on `api.example.com` with base
  path `/gw`), `orders.openapi.json` (3.0, the same shipments route again: a duplicate; servers
  with variable defaults `https://{region}.example.com/{version}` and a relative `/relative/base`,
  and one URL with a variable without default, which gives nothing), a Swagger 2.0 `legacy.openapi.yaml` and an invalid
  `broken.openapi.yaml`, `docs/api-contract.yaml` (3.0, `GET /api/carriers`) and the
  gitignored `generated/openapi.yaml` (`GET /api/generated`), both listed, with a path that does
  not exist, in the gateway's `[serve] documents` (the helper writes that line); the web app gains `src/shipments.ts` (the shipments route through
  `GATEWAY_URL` and through the gateway's host, plus one call per server case) and a vendored `vendor/openapi.yaml`, ignored
  since it has no `[serve]`. Later spec #21 tickets extend this overlay, so the base chain
  goldens never change. `drift_chain(case)`
  (spec #23) lays `tests/fixtures/chain-drift/web/` over the documented chain and commits:
  `src/drift.ts` holds consumers of the documented `GET /api/orders` reading a field the
  document lacks, a present one, one under its open `[].product`, one through the client
  wrapper `src/api/body.ts` (`get(path)`, which returns the parsed body), and one whose name a
  `catch` rebinds (no reads), plus a read below a documented scalar (`[].id.length`), a wrapper
  `load` fetching two routes whose two consumers share their caller's site, and a wrapper `both`
  calling `get` twice, which reaches one consumer site by two paths. Declared types (#64) add
  the catalog route `app/api/profiles/route.ts` (a zod page of profiles) and, in the web app,
  `src/declared.ts` (casts, type arguments of the wrapper `src/api/typed.ts` and of
  `res.json<T>()`, an annotation, `z.infer`, a package type and an optional field),
  `src/api/types.ts` (an interface another file extends) and the golden
  `chain-drift.trace-declared.txt`; the catalog route `app/api/labels/route.ts` returns a literal
  typed by an index signature, and the web's `src/labels.ts` reads under that open part (golden
  `chain-drift.trace-labels.txt`). Shapes through packages (#175) add the catalog's package
  `packages/contracts/` (`@chain/contracts`: an entry `src/index.ts` re-exporting by name, with
  `export *` and as types, plus a default zod schema; `quote.ts`, `shelf.ts` and `receipt.ts`
  hold a zod schema, constants, an interface, a union and a type with an index signature), a
  root `package.json` in the catalog, the gateway and the web app depending on it, the catalog
  routes `app/api/quotes`, `app/api/tickets` (the default export) and `app/api/offers` (a
  namespace import) parsed with the shared schemas, `app/api/tags` (a package return type)
  and `app/api/receipts`, the gateway's `src/shelves.ts` (spreading the shared
  constants) and the web's `src/shared.ts` (casts, an annotation, a type argument and `z.infer`
  of the package's names; golden `chain-drift.trace-shared.txt`). Error answers in other forms
  (the gateway's `GET /api/inventory/:sku/price`, the catalog's `app/api/reviews`), a `GET
  /api/ping` both serve (ambiguous) and the web's `src/open.ts` (a dynamic URL the decisions file
  `chain-drift-web` links) cover the rest of the drift rules. Later spec
  #23 tickets extend this overlay, so the `chain` and
  `chain-documents` goldens never change.
- **Golden files** live in `tests/golden/<fixture>.<what>.{txt,json}` and are compared with
  `assert_golden(name, actual)`. `MEADA_BLESS=1 cargo test` rewrites them; the diff is the
  review. `.gitattributes` marks fixtures and goldens `-text`, since BLAKE3 hashes and byte
  ranges are in them. An export golden of a test about something else goes through
  `without_processes(export)`, which drops `processes`: only the chain's own exports
  (`chain-{web,gateway,catalog,worker}.export.json`), the conformance export and the
  `*.processes.json` goldens carry them (spec #31), so a change to the process walk re-blesses
  those alone.
- Test files by subject, the fixtures each uses and the loopback decider stub: [map/tests.md](map/tests.md).
- A new behavior gets a fixture repo plus a golden; reuse a fixture before adding one. Fixture
  `meada.toml` files declare the peers and `serve.hosts`.
- `scripts/edges <dir>[=<origin url>] [<peer dir>...]` is the repro harness for reviewing a
  resolution or contract change: it copies the directories as sibling repositories, exports the
  first and lists each call's level and target, its providers, consumers and links (needs `jq`).

## Known ceilings

- `engine.rs`: one connection attaches every peer, so a workspace reads at most 10 peers
  (`MAX_PEERS`, SQLite's attach limit). `search` scans every symbol.
- `index.rs`: every contract is extracted again and every call resolved again on any change
  (rows rewritten only when they differ). An unreadable `contracts.fragments` value is read as
  no fragments, since only `refresh` writes it. A manifest or pack config file (`package.json`,
  `pyproject.toml`, `requirements*.txt`, `Cargo.toml`, `tsconfig.json`, `jsconfig.json`) is read up to one
  byte past `package::MAX_BYTES`, and a provider document up to one byte past
  `document::MAX_BYTES` (`read_up_to`), which their readers take as invalid whatever the rest
  holds, so the `manifests` and `documents` hashes and the config digest are of what was read. A
  pack config, at the root or at any depth (`**/`), is read through no symbolic link
  (`read_config`), as the walk follows none.
- `workspace.rs`: a sibling with an invalid `meada.toml` or `meada.decisions.toml` is skipped
  silently.
- `decisions.rs`: a repository serving a decision's contract from several sites links to the
  first by site; the target names no site.
- `decider/decisions.rs`: the upstream documentation gives no range for `confidence`; [0,1] is
  assumed.
- `resolve.rs`: wrapper expansion (HTTP clients, message producers and consumers) walks every
  simple path up the callers, and does not climb a call found only by a name match, so a wrapper
  method called on a receiver whose class Meada cannot tell (in TypeScript any receiver but
  `this`, a `const api = new Api()` included), or a function called by a bare name no import
  binds, is read where it stands; value flow (`Code::origins`, for a parameter's arguments) does
  not climb such a call either; a parameter captured by a closure is unknown (`Code::origins`
  does not follow it either; nor a name its scope binds more than once, or an attribute not
  assigned a call once in its class); a nested or rest
  pattern in a destructured parameter binds nothing, and a spread in the object a caller passes
  makes the names it may override unknown. `FAMILY_DEPTH` (128): a class family (the class and
  its ancestors in C3 order) longer than that is absent, so every member of the class is
  unknown (a name match at most), and the linearization recurses no deeper down the bases. The
  bases count from where the walk started, a family looked up for a member a base names
  (`class Z(K.D)` linearizes `K`'s) included.
  `LINK_DEPTH` (64): a name followed through more links than that, each into the next (a file
  re-exporting it from another: `export { x } from`, `export * from`, a Python `from m import x`
  or `import *`, a Rust `pub use`; a Rust alias, `use a as b`, or glob, `use m::*`), is unknown
  (a name match at most), and no other import, `export *`, glob or submodule binds it in its
  place. A star import from outside the repository (`from numpy import *`) is taken to bind
  nothing (`ponytail:` in `Resolver::imported`): a name it rebinds stays linked `exact` to the
  import or definition before it, as with the standard library's accelerator modules (`from
  _heapq import *` after `def heappush`). Where a Python star import sits is not compared with
  the code that reads a name it rebinds (`ponytail:` in `Resolver::rebound`): code run while
  the module loads (its top level, a class body there, its main guard) gets the name unknown, as
  it may run before the star import, and a function's code gets the star import's, as for every
  binding a function reads. `RECEIVER_DEPTH` (64): a receiver whose class is named through
  more receivers than that, each through the next (`x: y.C`, `x = y.C()`, or a Rust `let` that
  takes the class of a call's result on the next, `let x = y.f()`, `Class::Returned`), holds an
  unknown class.
  The receivers count from where the walk started. When `FAMILY_DEPTH` or `RECEIVER_DEPTH` cuts
  a walk, the families being linearized give up and none is kept: each is linearized again when
  asked for, and does not depend on the walk that first reached it.
- `lang/rust.rs` (types): `written` and `parameter` read a type and climb to the items that
  declare a type parameter within `PATH_DEPTH` (256) steps (references, `A + B + 'c` bounds and
  ancestors); past it a type names nothing and a parameter is taken as declared with no bound,
  so no repository type is guessed.
- `lang/ts.rs`: tsconfig `extends` and nested configs are not followed. The root
  `tsconfig.json` (else `jsconfig.json`) is read as `package::read_tsconfig` reads one
  (`package::tsconfig_root`, `MAX_BYTES`), so one past the size limit, not UTF-8, not JSON or
  not an object declares no `paths` or `baseUrl` and the refresh carries on, with no status
  line. `require` and
  `module.exports` are not imports (no `ponytail:` marker). `new X()` is a call fact (callee `X`)
  and a `Value::New(callee, args)`. `DEFINITION_DEPTH` (128): a function or class nested deeper
  is not a definition (its body belongs to the one around it, and its name is a local that hides outer
  ones), since qualified names and `Resolver::lexical` cost time cubic in the nesting; a function
  written in place there is `Value::Unknown`. `VALUE_DEPTH` (64): a value nested deeper in
  arguments, objects, arrays and the receivers of method chains is `Unknown` from there.
  `FOLD_DEPTH` (256): a `+` chain or a chain of string method calls folds only that deep (the
  rest is a hole), and a member chain in a callee is cut to the node kind. The parent of every node is read once (`Parents`, which
  `lang/ts/uses.rs` and `lang/ts/types.rs` climb too), as `Node::parent` is linear in the
  tree's depth.
- `resolve.rs` (`Code::origin`): only `const` bindings with a plain name are followed; a client
  held by `let`, a class field, `this.x`, a destructuring or a parameter, or chained without a
  binding (`new Kafka().producer().send`), is not recognized (no marker).
- `lang/ts/messages.rs`: a kafkajs handler passed through a wrapper's parameter is not followed
  (the function that subscribes handles); only the first `run` on a consumer binding counts. An
  amqplib `bindQueue` whose queue, exchange or pattern is not a literal is dropped.
  A BullMQ `Queue` is named by its own `new Queue(name)` binding: `new Queue(name).add(...)` with
  no binding, a `Queue` that is a class field or a parameter, and `addBulk` are not recognized. A
  node-redis client made as `await createClient().connect()` is not traced (the `connect` chain
  has no binding); a BullMQ processor given as a file path is not followed (the function that
  makes the `Worker` handles).
- `lang/python/messages.rs`: a client is traced to its constructor by `Code::origins` (a name its
  scope binds once, an attribute assigned a call once, a parameter every exact caller passes, a
  value a function returns, or a call written in place: `boto3.client("sqs").send_message(...)`,
  `pika.BlockingConnection(p).channel().basic_publish(...)`), with one level of `channel()` or
  `client()` above it (a pika channel of a connection, a boto3 client of a session); deeper
  factories, a client held in a container and a module imported under another path
  (`kafka.producer.KafkaProducer`) are not recognized. A boto3 client counts for SQS when its
  service is the literal `"sqs"` (a name or constant is not followed). A topic that starts with
  `^` is a regular expression, so it is an unresolved name. A pika `queue_bind` whose queue, exchange or routing key is left out
  or not a literal is dropped. A callback is a handler when it is a definition of the repository
  (`on_message_callback=handler`, `cb=self.on_job`); a lambda or anything else leaves the
  function that subscribes. The streams of an `xreadgroup` are read from a dict literal (or a
  constant folded to one); a dict built otherwise is an unresolved stream.
- `lang/rust/messages.rs`: a client is recognized when its receiver traces back to the library's
  constructor (`Code::value_origins`, then `Code::external`, one constructor per `LIBRARIES`
  entry); a client held in a struct field, passed as `&mut con` to a parameter, or rebound by
  assignment is not followed. `lapin` `set_delegate` closures are not followed (the function
  that holds `basic_consume` handles). A Kafka topic starting with `^` is unresolved; an
  `queue_bind` that cannot be fully evaluated is dropped; an SQS `queue_url` on a builder held
  in a `let` is not recognized. `Code::mutable` takes the value of a `let mut` bound once and
  never assigned (`Facts::mutable`, keyed by qualified name); a closure parameter, a `let` in a
  closure given to a call, a nested function's parameter or an assignment in such a closure hides it.
  `CHAIN` (64): a builder chain (`FutureRecord::to(t).payload(p)`, an SQS `send_message()...`,
  redis `StreamReadOptions`) is read to that many calls below the form (`Code::chain`, itself
  bounded by `FLUENT_CHAIN`); a longer one is not a producer or a consumer.
- `resolve.rs` (`Resolver::owned_outside`): a Rust method called on a call chain
  (`client.get(u).send()`, `Call::root` = `client`) resolves to nothing when `client.send` would
  be external, and so does one on a chain that starts at a call (`rocket::build().mount(..)`,
  `Client::new().get(u)`, no `Call::root`) when that first call, the innermost one starting
  where the chain does, is external (`Resolver::rooted_outside`, Rust only); a root that is a
  repository type, a parameter, or a local with no written class (a client a repository
  function returns) is name-matched as before. `self` and `Self` in an `impl` of a type from
  outside the repository (`impl ApiExt for reqwest::Client`) are opaque too
  (`Resolver::outside`): `self.clone()` there resolves to nothing.
- `channel.rs`: broker clusters are not modelled (one name of one broker kind is one channel
  across the workspace), and an exchange's type is not read: every binding pattern follows
  topic-exchange rules.
- `package.rs`: a manifest never fails indexing, so its reader has fixed limits. `MAX_BYTES`
  (1 MiB; a larger file reads as empty, but is read whole first, as code and documents are) and
  `MAX_NAME` (214 bytes; a longer name is no package). A range is stored as written, with no
  length limit beyond `MAX_BYTES`; `flag` checks only one of at most `MAX_RANGE` (256 bytes) and
  calls a longer one `range unchecked`. PyPI (`package/pypi.rs`, spec #24): `MAX_NAME` and
  `MAX_BYTES` hold for `pyproject.toml` and `requirements*.txt` too (a file past it, not UTF-8
  or not TOML reads as empty; the toml crate's own depth limit stops nested input); a
  requirements file is read line by line, its option lines (`-r`, `-e`, `-c`, `--index-url`),
  URLs and paths skipped (a Poetry `git` or `url` source too), and what `-r` names is not
  followed; environment markers and extras
  are dropped and a direct reference (`name @ url`) is no entry; Poetry groups and
  `[project.optional-dependencies]` are not read; a range is never evaluated (no marker). A
  Python import is looked up in the distributions every PyPI manifest of a directory holding
  the file depends on (the nearest naming one gives the range), among the top-level modules under each provider's roots (its directory, then the roots its
  `pyproject.toml` declares, or `src`); a namespace package (a directory without
  `__init__.py`) exposes nothing, and an import several distributions expose is `ambiguous` for
  each, its reason naming none of the others (no marker). Cargo (`package/cargo.rs`, spec #30):
  `MAX_NAME` and `MAX_BYTES` hold for `Cargo.toml` too (a file past it, not UTF-8 or not TOML
  reads as empty); a name outside Cargo's alphabet makes no provider and no entry; an entry that
  is neither a version nor a table, or whose `path`, `git` or `package` is empty or not a string,
  or that gives both `path` and `git`, or whose `path` is absolute, is dropped, never read as a
  version dependency; of two entries of one table and key (a plain one and a `[target.*]` one) the
  first stays (a set keyed by table and key, no quadratic scan). A range is never evaluated,
  `registry`, `optional` and features are not read, and neither `Cargo.lock` nor a registry is
  opened (no marker). A `path` is folded lexically from the manifest's directory: one that
  leaves its repository (`../`) links only to the workspace repository whose root, joined with
  the manifest of a provider, folds to the same location, and one that lands in no workspace
  repository adds nothing; nothing outside a repository is read and no symlink is followed, so
  a symlinked checkout is matched by the paths as they are written (no marker). `[lib] name` is
  followed (a `use` writes it, unless the key renames the crate); `version.workspace = true`
  reads the nearest `[workspace.package] version`, and no other inherited field is read. A `use`
  follows `pub mod` and `pub use` rows alone, so an item behind a private module is not
  reached (no marker). Import resolution: `exported` follows at most
  `MAX_CONDITIONS` (32) nested conditions and `lookup` at most `MAX_REEXPORTS` (64) `export *`
  levels; deeper is no target. An array of fallbacks in `exports` and a key with two `*` are not
  read (one `*` in a target keeps every expansion the size of the subpath). `read_tsconfig` reads
  like `read`: past `MAX_BYTES` or `serde_json`'s depth limit, not UTF-8 or not an object it is
  invalid (`None`, no config; an option of another type is absent in a valid one), and `roots` voids the mapping for an absolute `outDir` or `rootDir`, one with an empty or
  `..` segment, or an `outDir` that is the config's own directory (a path read from a config stays
  inside the repository; nothing is opened on disk, the mapped file must be one the walk indexed,
  and the walk follows no symlink). `extends` is not followed (`ponytail:` in `Tsconfig`; the
  trunk follows none for `paths` either): a base config's `outDir` counts as absent, so such an
  entry is `ambiguous`; `declarationDir` is not read, and a package with no `rootDir` maps
  `dist/index.js` to `index.ts` at its own directory. An entry with no output extension never
  maps (`main: "./dist"`, `"dist/index"`, `types: "dist/index.d.mts"` or `.d.cts`): only `.d.ts`,
  `.js`, `.mjs`, `.cjs` and `.jsx` are stripped (no marker). The ambiguous candidates of an
  import have no cap: every file of the package exporting the name is listed, so a name exported
  by many files gives as many hops (the spec lists them all; no marker). Only a call whose callee's head is the import crosses: a `const` alias or a
  client bound from it (`const c = new Client(); c.run()`) does not (no marker).
- `lang/ts/uses.rs`: following call results (`Call::uses`) charges every node it visits and
  every step it copies into a `const`'s uses to `USES_BUDGET` (2^20 per file); past it, the
  file's remaining calls have no uses, so their consumers read nothing and give no drift. A
  value is followed only in the function making the call, through the names a `const`
  declaration, a `for...of` variable or an array method's callback parameter binds (patterns
  included), when nothing else in that function binds or assigns the name; a `let`
  declaration, `.then(...)`, a callback given by name, `reduce` and a value passed to a
  function end it (no marker). A destructuring pattern nested deeper than `PATTERN_DEPTH` (32)
  reads nothing below that depth (`ponytail:`; a unit test feeds it a deeper pattern). A wrapper's caller is read only when the
  wrapper returns one thing (the response, the body or one path below it) from the function
  holding its call; whether the body was awaited is not checked (no marker).
- Shape readers (`zod.rs`, `declared.rs`, `literals.rs`): each per-file budget below is one
  `shape::Budget` per extraction (`Http::extract`, `package::exported_shapes`), shared by every
  reader of that extraction, so a file spends each at most once per extraction.
- `zod.rs`: reading the schemas of a file's handlers charges every schema expression examined
  to `ZOD_BUDGET` (2^14, over all the handlers of the file, and the `z.infer` schemas its
  types name); past it, the remaining handlers
  give no shape from a schema. Nesting, constants followed and methods chained count together
  against `ZOD_DEPTH` (32): a part deeper is open. A schema is read only when every 2xx body a
  handler sends is typed with one; otherwise the literals are read (no marker). A name imported
  from a package is a schema the query fills (issue #175) where a schema stands (the receiver
  of `parse`, a field, an element, a member, a `z.infer`), or under the methods `KEEPS` and
  `SCHEMA_METHODS` list; `.pick`, `.omit`, `.partial` and `.required` on it open the part,
  since its fields are not known here (no marker). `.passthrough`, `.catchall`, `z.record`, `z.any`, `z.unknown`, `.transform`, `z.lazy`
  and any call Meada does not read open the part; so does a `pick` / `omit` with a computed
  key and a `z.union([])`. A schema or a parse result held in a constant of the handler (a
  function-local `const out = Schema.parse(..)`) is followed, unlike in `literals.rs`, and
  nothing records a later write to it (`Object.assign(out, { extra: 1 })`), so it is still
  read as the schema's shape (no marker).
- Declared types (spec #23, `declared.rs`, `lang/ts/types.rs`): a type is read to at most
  `TYPE_DEPTH` (32) levels, both when it is written (`lang/mod.rs`, the nesting of an object or
  array type) and when a consumer's cast, type argument or annotation is followed through
  `extends`, aliases and intersections; a level deeper adds no fields. Reading the types of one
  file (its consumers' and its handlers') charges every type examined to `TYPES_BUDGET` (2^14); past it, the type being
  read adds no fields on a consumer, and opens the value on a provider. A declaration that leads
  back to itself (`interface A extends B`, `interface B extends A`) adds what it did the first
  time. A type parameter hides a declared type of its name only when it sits within
  `HIDING_CLIMB` (64) levels of the use; one declared farther up leaves the name unknown, so
  it adds no fields. Only interfaces (with `extends`), aliases to object types, intersections,
  arrays and `z.infer<typeof S>` give fields; a union, a mapped or conditional type
  add none (no marker); a type from a package is filled at query time (issue #175). A generic of the repository
  (`Paged<Row>`) adds its own fields, its type arguments not substituted (its `T` adds none).
  An optional field (`?`) adds no drift, nor does what lies below it; the fields of a
  `z.infer` schema that `.optional()`, `.nullish()` or `.partial()` make may be absent are
  optional too (`.required()` undoes it), and a schema union adds no fields. A type argument
  given to a wrapper types the body only when the wrapper's return type is its own type
  parameter (`Promise<T>`) or it casts the body to it (`as T`, `json<T>()`); `json<T>()` and
  the like type their result when they have exactly one type argument. A cast or an annotation is read at the top of a value (not
  inside an object literal), and a value that goes through a function, a `let` or a rebinding
  is not followed (no marker). On a provider, an index signature in the type of a returned
  constant, a cast body or the handler's return annotation opens that part of the shape.
- `literals.rs`: reading a handler's returned literals charges every call examined and every
  value read or constant followed, over all the handlers of a file, to `LITERALS_BUDGET` (2^16);
  past it, the file's remaining handlers give their routes no shape. Nested literals and
  constant hops count together against `LITERALS_DEPTH` (32): a part deeper is present and open,
  and a constant chain longer is unknown. Only a body Meada reads (an object or array literal,
  a constant naming one) gives fields; a call, a `map` over rows, a parameter or a spread of
  anything but a known object is open (no marker). The handler is the function `res.json` and
  its kin sit in or in a callback inside it. A response with a literal status outside 2xx is
  left out whatever its form (`res.status(404).json(..)`, `res.status(404).end()`,
  `res.sendStatus(404)`, `new Response(null, { status: 404 })`), and so is Express's error path,
  `next(err)` (`next` called with an argument other than `'route'` or `'router'`). A response
  sent another way opens the root of the shape (no marker): in Next.js, a call whose result the
  handler itself returns and that is no `NextResponse.json` / `Response.json` (`return
  ok(user)`, `new Response(..)` with no such status, `redirect`); in Express, `res` passed to
  another call, a `res` method other than `json`, `send` and the ones that only set a header or
  the status (`end`, `render`, `redirect`, `sendFile`, `sendStatus`, ...) with no such status,
  or `next()`, `next('route')` and `next('router')`. A `res` stored in another name or a status set in a separate
  statement (`res.status(404);` then `res.json(...)`) is not followed, so such a send counts as
  2xx (no marker). Only a module-level constant is followed: nothing records a later write to
  a function-local one (`body.next = cursor`, `Object.assign(body, x)`), so it is unknown and
  open; a module-level object mutated after its declaration is still read as written. A constant
  held by another repository (a package import the repository's `Resolver` does not map to one
  of its files) is filled at query time (issue #175), and so are the index signatures of a
  package type a returned value, a cast or the return annotation names.
- Shapes through package imports (issue #175, `shape.rs`, `package.rs`): `shape::fill` follows
  an import into the shape the package keeps, and the imports of that shape in turn, at most
  `PACKAGE_HOPS` (8) packages deep, and charges every import looked up and every path grafted
  to `GRAFT_BUDGET` (2^16) per side; past either, the import is unresolved: open on a provider,
  nothing on a consumer. So is an import that resolves nowhere, `ambiguous`, to several names,
  or in two providers to different shapes, and a name no reader keeps a shape for (a call, a
  union type). Only a module-level constant or type a file of a package's public surface
  (`package::surface`) exports itself keeps a shape (a re-export reaches it through the
  `exports` rows): the files the entries of each named manifest (the bare name and each
  `exports` subpath without a `*`, as an import resolves them) lead to through the `exports`
  rows. Those are re-read, their files parsed again, on every refresh that extracts, so the
  cost is bounded by the packages' surface, not the repository. A subpath pattern (`./x/*`) and
  a deep import into a package without `exports` (`pkg/src/x`) reach files off the surface,
  which keep no shape, so such an import is unresolved (no marker). An edited `package.json` or
  `tsconfig.json` extracts again. A type through a namespace import (`C.Receipt`) is not read
  (no marker); a schema through one (`C.Quote`) is. Since constants and types are `exports`
  targets, two `export *` that give a name differently (a function and a constant) export
  neither, as in ES modules.
- `lang/ts.rs` (`Facts::locals`): a local binding hides the name in its whole enclosing
  definition, whatever block holds it, the definition's own definitions, constants and
  parameters of that name included, and a function that is no definition (an object literal
  method, `module.exports = (db) => ...`, an IIFE, an arrow kept in an object) lends its
  parameters and locals to the definition around it; at the top level that is the whole file.
  A call to the hidden name elsewhere in that scope loses its exact link or its value, never
  gains a wrong one (no marker).
- `index.rs` (`refresh_stamped`): a `package.json` or document that cannot be statted or read
  (it vanished after the walk, or is unreadable) has no row instead of failing the refresh; a
  `[serve] documents` entry in that state is then reported as missing.
- `document/`: a document never fails indexing, so its readers have fixed limits; deeper or
  larger parts are unreadable or open. `mod.rs`: `MAX_BYTES` (16 MiB, above the largest public
  single-file descriptions, GitHub's REST API and Stripe's SDK spec at 10 to 15 MB): a larger
  document is unreadable at its first line. `yaml.rs`: `MAX_NESTING` (128 nested collections, as
  `serde_json` has for JSON) and the alias budget (`EXPANSION` times the source plus `SLACK`, no
  marker). `schema.rs`: `MAX_DEPTH` (128 levels per shape), `WORK` (64 MiB per document) and
  `BUDGET` (10,000 schemas per shape, no marker). `openapi.rs`: a path item reference behind
  another one that is external is dropped without a status line (no marker).
- `drift_view.rs`: `context`, `search` and `status` find each shown link's consumer call by
  scanning the calls of every repository, once per link (no marker); an index by call id waits
  for a workspace that makes this slow.
- `lang/python.rs`: `VALUE_DEPTH` (64): a list, tuple, dict literal, parenthesized expression or
  `await` nested deeper in a value is `Value::Unknown`, so a hostile file cannot overflow the
  stack. `FOLD_DEPTH` (256): a `+` or `or` chain, a chain of string methods or `getenv`
  defaults and nested f-strings fold only that deep (the rest is a hole), and an attribute
  chain in a callee is cut to the node kind; each call's arguments are folded again, so a file
  of nested calls costs up to `FOLD_DEPTH` steps per call. `DEFINITION_DEPTH` (128): a
  function or class nested deeper is not a definition (its body belongs to the one around it,
  its name and parameters are locals that hide outer ones there, and its `return`s are no
  one's), since qualified names and `Resolver::lexical` cost time cubic in the nesting.
  `TARGET_DEPTH` (64 nodes): a name deeper in an unpacking target or a `case` pattern is not
  bound, so it hides nothing. An annotation reads `Optional[...]` and `... | None` to
  `TYPE_DEPTH` levels; deeper is no class. For imports inside the repository only the root `pyproject.toml` is read
  (`lang/python/pyproject.rs`, which the PyPI reader shares), and only the setuptools and
  Poetry root declarations; it is read as a PyPI manifest is (`package::read_manifest`, `MAX_BYTES`), so one past the size limit, not UTF-8 or not TOML declares no roots and the refresh carries on, with no status line. `src` is a source root when nothing is declared, whether or not it holds
  packages (a pack sees no file list). An import binds its names for the whole file, wherever
  it sits. A comprehension's targets are bound in the enclosing scope, so they hide a name
  there for the whole scope (the safe side: a name match instead of `exact`). A write to an
  attribute from outside the class (`obj.x = ...` in another function) is not seen, so a
  class-body `x = C()` still types `self.x`. `__all__` is read only as one literal list or
  tuple, grown by literal names in a statement of the module itself (`+=`, `.extend`,
  `.append`); any other change to it, one under `if`, `try` or a loop, in a lambda or a
  function, and an item set or deleted (`__all__[:] = ...`, `del __all__[0]`) make it computed. A name `del` unbinds (at the top level,
  unless bound again), and a definition another binding of its scope follows (at the top level
  or in a function, where a nested function's `nonlocal` binding counts as following), and one
  under `if`, `try`, a loop, `with` or `match` after another binding of its name, which it may
  leave in place, are hidden in their whole scope, wherever the call sits (the safe side: a name match instead of
  `exact`). In a class body a binding after a method hides nothing: `self.m()` reads the method
  through its class, right for `m = staticmethod(m)` and as `@property` is read for `m =
  property(m)`, but a class-body `m = None` leaves `self.m()` linked to `def m`. A `**k` splat in a call is not read; a `*a` splat is read as one positional argument.
  Console scripts (`Pyproject::scripts`) are read as `"module:function"` strings only: the Poetry
  table form (`{ callable = "...", extras = [...] }`) is left out, and so are
  `[project.gui-scripts]` and `entry-points`. The block of an `if __name__ == "__main__":` is the
  scope `__main__`, so a name it assigns is a local of that symbol and no longer rebinds the
  module's global of that name; only the first `__main__` of a file (a guard or a
  `def __main__()`) counts.
- `lang/rust/cfg.rs` (spec #30): `PREDICATE_DEPTH` (64): a `cfg` predicate nesting `all`,
  `any` and `not` deeper is false; `FEATURES` (4096): the default features of a crate followed
  through features that enable others (a cycle ends sooner, by the visited set), the rest is
  not followed.
- `lang/rust.rs` (spec #30): `DEFINITION_DEPTH` (128) as in the other packs; `BINDING_DEPTH`
  (64): a pattern or a `use` tree deeper binds nothing; `PATH_DEPTH` (256): a longer path or
  member chain in a callee is its node kind, and an `impl` type behind more references names
  no type; `USE_BINDINGS` (4096): the names the `use` declarations of a file bind, the rest
  bind nothing; `VALUE_DEPTH` (64): a value nested deeper in wrappers (`(x)`, `&x`, `x?`) and
  arrays is `Unknown` from there. A call inside a value is a stub (its callee and span), so a
  builder chain costs one value per call. A closure written as an argument of a call is a
  `Kind::Inline` definition; any other closure is not, and neither is a `let mut`, a name bound
  twice in its scope or assigned again, or a destructuring a constant. A string literal with an escape sequence is
  `Unknown`. A `Cargo.toml` past `package::MAX_BYTES` (read up to one byte past it, as every
  pack config is), not UTF-8 or not TOML declares no crate, with no status line. Every `Cargo.toml` with
  a `[package]`, at any depth, is a crate (a workspace's `members` and `exclude` add or remove
  none, and a vendored or fixture crate the walk finds is indexed too); `tests/`, `examples/`,
  `benches/` and `build.rs` are no crate roots (no marker). The default features a file is
  parsed with (`settings`) are those of the nearest `Cargo.toml` with a `[package]` above it,
  read before any module tree exists, so a file another package's `#[path]` reaches is parsed
  with its nearest package's features, not those of the crate that builds it (no marker). A file two crates reach (`mod
  util;` in both `lib.rs` and `main.rs`) belongs to the first by root order (the library
  first), so its `crate::` names that root (no marker). `#[path]` on a module written in place
  is not read, and `super::` out of a file declared inside a module written in place of its
  parent reaches nothing (no marker). The `mod` imports and the synthetic ones (`crate`,
  `super`, `mod`) bind for the whole file, so they are seen inside every inline module too;
  a `use` binds in the scope that holds it, and the `mod` import of a declared module is not
  seen inside a module written in place. A `use` path whose first segment nothing in the
  file binds is a crate outside it (`Facts::externs`, no marker, never name-matched), also when
  the file holds a glob that brings no such name. A glob of a module written in place from
  outside it, and of an enum or a type, bring nothing (the visibility of the items of a module
  written in place is not kept; no marker), and a glob takes `pub` items alone, so a
  `pub(crate)` item reached through one falls to a name match. `pub use` names are `pub`;
  a crate in the workspace is reached only from a crate whose `Cargo.toml` lists it in
  `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]` (by key, or by `package =`;
  `workspace = true` through `[workspace.dependencies]`), a binary naming the library of its own
  package; the same tables under `[target.*]` count, and `optional` and features are not read
  (a listed dependency counts as present; no marker). Any other crate name is external. A module declared inside a module
  written in place is not followed for exports (no marker). A chain of `use` aliases, globs and
  `pub use` longer than `LINK_DEPTH` (see `resolve.rs` above) is unknown, and no other glob
  binds the name in its place.
  `self::` and `super::` read inside a module that sits in a function are
  `(scoped_identifier)`, unresolved, and a `use` there binds nothing. A file's exports table
  rows are its `pub` items, `pub use` names, `pub` modules and, inside `pub` modules written
  in place, the dotted names of their `pub` items (`entries.add`), and a `pub use` glob of a
  file gives that file's public names; whether the file itself is reachable from the crate's
  root is for the reader to follow through the `pub mod` rows. Calls inside macro invocations
  are not indexed.
- `lang/rust.rs` (spec #30, actix-web and Rocket): `ROUTES_PER_FUNCTION` (16): a function with
  more route attributes (`#[get("/x")]`, `#[route(..)]`) than that has the first ones as calls
  that decorate it (`Call::decorates`), the rest nothing. `ROUTE_ITEMS` (1024, `lang/rust/values.rs`):
  a `routes![..]` listing more handlers is `Unknown`; `ROUTE_METHODS` (16): the `method = ".."`
  of a `#[route]` past that are `Unknown`, so the attribute gives no provider. `MENTIONS`
  (16384, `lang/rust/mentions.rs`): a file referring to more distinct names has no
  `Facts::mentions`, so none of the handlers of its crate is taken as unmentioned.
  `Facts::mentions` is not kept in the index (see `Call::args`); the providers read it from the
  files parsed again, all of the pack's when a handler is registered by no call they read. A
  mention is a name in a reference position (an expression, a receiver, a path, a `use`, a
  macro's tokens); a pattern, a parameter or a field declaring it is none, and a local hides a
  reference from the end of its `let` to the end of the block, a parameter in its function's
  or closure's body, a `match`, `if let`, `while let` or `for` binding in its arm or block (an
  `else` sees none). An item declared in a block (`const`, `static`, `fn` inside a function)
  hides nothing, and a pattern deeper than `BINDING_DEPTH` binds nothing, so the reference
  still counts (no marker).
- `http/rust.rs` (spec #30): `ITEM_WORK` (16384): the values the expansion of constants and
  returned lists adds, over the whole repository, to the handlers a call names; a list past it
  is read no further, and a handler it left out is taken as mentioned. `macro_use`: the crate
  of a bare route attribute is read from the `#[macro_use] extern crate x;` of its crate's root
  (the `Tree`'s `crate`, wherever `[lib] path` puts it), and it needs exactly one such crate
  (actix-web or Rocket). A handler is served at its own path only when no file of its crate
  mentions its name (`mentions.rs` above), or of any crate when its file is one another crate
  may import (`Code::published`, a library's `pub mod` files); any other mention Meada does not
  read as a registration (`services![..]`, `vec![..]`, `concat()`, `cfg_if!`, an alias, a use
  before a `let` of the name) means it may be registered where Meada cannot tell, and then it is
  served nowhere. A file two crates reach belongs to the first (see above), so a mention in the
  second, of a handler not published, is not counted (no marker). A service configuration is followed into the functions of the
  repository it is passed to (`f(cfg)`, answered exactly), not through methods or closures.
- `http.rs` (axum): `METHOD_CHAIN` (64): an axum method router chain (`get(a).post(b)`) is read
  to that many calls, a longer one gives its route no provider. `FLUENT_CHAIN` (1024,
  `resolve.rs`): a builder chain (`Router::new().route(..)`, an actix-web `ServiceConfig`'s
  `cfg.service(a).route(..)`) is read to that many calls down to what starts it (`Code::chain`),
  and a call past it has no origin, so its route is no provider (the walk is memoized per call,
  linear in the chain). `PREFIX_DEPTH` (64) and `PREFIX_WORK` (4096):
  `prefixes`, for every kind that mounts (Express, FastAPI, Flask, axum, actix-web, Rocket), reads a router
  nested deeper than the first, or through more mounts than the second, as served nowhere
  rather than under part of its prefixes. A name bound again in its scope (`let mut`,
  shadowing) or assigned again holds no known value, and the routers its `let` holds
  (`Facts::rebound`) are served nowhere; `on(MethodFilter, h)` and a method router held in a
  variable are not followed. A router nothing mounts is served at its root. A router
  given to a call Meada does not read (`route_service`, `fallback_service`, an outside
  function wrapping it, a method only a name match found) is mounted nowhere, so it is not
  served at the root; so is one whose `nest` prefix cannot be read, or that is mounted on a
  router Meada cannot read. A router handed to a call that is neither a mount nor a wrapper
  of one in its arguments (a router put in a struct, a closure capture) is not told from one
  nothing mounts.
- Rust values (`lang/rust/values.rs`, `lang/rust/builders.rs`, spec #30 #131): `BUILDER_BUDGET`
  (2^20 syntax nodes per file, over every `let mut`), `BUILDER_DEPTH` (64 builders read within
  one another), `BUILDER_SIZE` (4096, bytes of text plus parts) and `BUILDER_WORK` (2^22 size
  units per file, over every prefix folded and every read): a `let mut` String past the budget
  is unknown, and one read past the depth, the size or the work is a hole (`push_str(&s)` of
  itself doubles). `FORMAT_ARGS` (64), `FORMAT_PLACEHOLDERS` (256) and the size above bound
  one `format!` or `concat!`; past any the value is unknown. A `let mut` String is a builder
  only when its block reads it, `push_str`s, `push`es and `+=`s it in statements of that block
  (a statement `#[cfg]` leaves out, decided where the block is read, is not there); a push in
  a branch or a loop, `&mut s`, `write!(s, ..)`, an index or deref use (`s[..]`, `*s`), another
  binding of the name, a method that changes it (`insert`, `clear`) or a second builder of
  that name in the scope leave it unknown. `format!` and `concat!` read
  a literal format string and arguments that are a literal or a plain name; a spec such as
  `{:.*}` or `{:1$}`, a count that does not match or a `{`/`}` left alone are unknown; a macro
  of another name or one the file defines (`macro_rules!`, in this file only) is not read.
  `Method::GET` is text only through a `use` of one name from `reqwest`, `http`, `hyper`,
  `axum` or `actix_web`, or written in full from one of them; a glob import or an alias is not
  followed. `a + b` is a string only when one side is a string literal or a conversion: two
  names may add numbers. Rust clients: a client held in a field of a struct (`self.client`), in
  a `static` or a `OnceLock`, and a client from a crate other than `reqwest` and `ureq`
  (`hyper`, `surf`, `isahc`, `attohttpc`) are not read; a client parameter is followed through
  the callers that pass a constructor, as in Python; `reqwest::Request::new` and
  `RequestBuilder::build` are not followed; a `Url::parse(..)?` or `join` URL is unknown.
  A value derived from a client (a response, a header map, a JSON value, the result of an
  extension trait's method) is no client: a method passes the object on only when the
  object's entry lists it (`FRAMEWORKS` in `lang/rust/frameworks.rs`, the constructors named
  exactly: what a name of the crate that is no listed constructor makes passes nothing), so a
  setter missing from the list ends the chain and its client (or its router, app, scope,
  resource or Rocket instance) is not read.
- `http.rs`: `router.route(path)` chains and `require` are not followed. A route path or prefix
  that depends on a parameter is not read through callers (it yields no route). A FastAPI
  mount whose child cannot be read hides every other app of its framework that nothing else
  mounts. Django: one `include()` whose argument cannot be read leaves no root URLconf; the
  walk reads at most `URLCONF_BUDGET` (2^15) entries over every root and `include()` (a
  module included under several prefixes is read once for each), so the entries past it yield
  no provider; what
  an extension of `urlpatterns` adds other than a list literal is left out (marked in
  `lang/python.rs`), and a single `urlpatterns = [...] + other` (no list literal as a whole)
  yields no entry; `django.conf.urls.url`, a view wrapped in a call in the URLconf
  (`require_POST(view)`), a handler method a class-based view inherits and `as_view()` on a
  class outside the repository yield nothing; a `re_path()` pattern is read as written, so a
  non-raw string with doubled backslashes yields nothing. Python clients: a receiver that is the
  result of a call (`Session().get(...)`, `make().get(...)`) or an attribute set from anything
  but a call (`self.http = http`, marked in `lang/python.rs`) is not read; a parameter given a
  client takes the base of every caller that passes one, and several different bases make it a
  hole.
- `entry.rs`: an export entry point is a function, or a method of an exported class (a
  constructor excepted), whose exported name the exporting file's `Facts::star` lets through when
  its pack names a public set (Python: no leading `_`, or a literal `__all__`; TypeScript names
  none, so all pass), that the `exports` rows name and no call reaches in its repository,
  an ambiguous candidate counting as a caller and a self-call not; a call through a package
  import of its own repository counts too, but only at query time (`Engine::exports`), so the
  stored rows over-list in a monorepo. A method of a class that is not exported, a
  default-exported anonymous function and an export the resolver cannot place are not found (no
  marker). A route handler Meada could not find (an Express handler passed as a parameter) has
  no entry point.
- `trace.rs` (process walk): an ambiguous call or link is one unexpanded step listing its
  candidates; `inferred` and `decided` links expand. A process is walked down only; `trace`
  up from `process::...` answers with a note. `PROCESS_DEPTH` (256) caps the recursion of one
  branch: the step at the limit is marked `depth-limit` (`(depth limit)` in `trace`) and not
  expanded, so a longer call chain is cut. `Graph::processes_of` (the processes `context` lists)
  walks upstream with no depth limit, so on chains deeper than `PROCESS_DEPTH` membership and
  trees can disagree: a node past depth 256 is no step of the tree, and the limit can hide a
  node shallower than that, through `Seen`. E reaches X at depth 255, so X expands and each of
  its callees, Y among them, is a `depth-limit` step at 256 whose own callee Z is no step; a
  later direct E to X shows X as seen, so Y is not expanded there either, yet `processes_of(Z)`
  lists E.
- `community.rs`: Leiden has no iteration cap (spec #31 asks for none). Each pass that changes
  anything either moves a node to a community with a strictly positive modularity gain, which
  raises the modularity, an integer here (`i128`, no floating point), so no partition repeats,
  or merges subcommunities and shrinks the graph, which cannot happen forever; so the loop ends.
  The work grows with the repository's calls and passes (no marker).
- `engine.rs`: `context` builds `trace::Graph` and calls `Graph::processes_of` for every symbol
  it answers about: a reverse map of every edge of the workspace (every call and link) is built
  per call, with no cache, so the cost follows the workspace's size, not the symbol's (no
  marker).
- `repo.rs`: a path git still quotes (tab, newline or quote in the name) is skipped by
  `uncommitted()`.
- No `busy_timeout`: two concurrent refreshes of one repository may hit `SQLITE_BUSY` (no marker).
