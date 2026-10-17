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
  language adapter and the TS/JS pack. `resolve.rs`: calls to definitions (`Resolver`), values
  and receivers (`Code`). `community.rs`: the deterministic Leiden behind communities (spec
  #31). Detail: [map/indexing.md](map/indexing.md).
- `contract.rs`, `http.rs`, `resource.rs`, `channel.rs`: the `ContractKind` trait and the HTTP,
  resource and channel kinds. `package.rs`: npm packages as contracts (spec #22). Detail:
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
  other expression becomes `(<node kind>)`.
- Inline handlers: a pack emits anonymous function arguments (and functions written in an
  options object argument) as `Kind::Inline` named `@<byte offset>`; `Names` renames the ones a
  kind claims, to `<METHOD> <full path>` for a route or `<broker> <channel names>` for a channel
  handler (`#<n>` from the second equal name), and folds the rest into the enclosing symbol.
- Entry point (spec #31, `entry.rs`): a route is `<handler symbol>::<contract>`, such as
  `example.com/chain/gateway::src/orders.ts::listOrders::http::GET::/api/orders` (the handler's
  identity, then the contract identifier; an inline handler keeps its synthetic symbol, as in
  `example.com/chain/gateway::src/server.ts::GET /api/health::http::GET::/api/health`); an export
  is the exported function or method's own symbol identity. Its label is `<METHOD> <path>` for a
  route and the qualified name for an export. A process is addressed as `process::<entry point
  identity>` and named by the label; only a handler that is a symbol is a route entry point (a
  route served by a file has none). A handler serving several routes has an entry point per
  contract.
- Package: `package::npm::<name>`, provided at `<repository>::<manifest path>` (such as
  `example.com/pkg/shared::package.json`) and consumed at `<repository>::<manifest
  path>::<field>::<name>`. Both sites hold a `::`, which `Engine::export` needs to keep their links.
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
  unresolved side). A path parameter of any spelling is `{}`.
- Export JSON: `{repository, files, symbols, calls, providers, consumers, bindings, documents,
  publishes, packages, dependencies, imports, communities, links, processes}`;
  `communities` (absent when empty) lists each community as `{name, members}`, by name, with
  its sorted member identities; `processes` (absent when
  empty) lists the repository's entry points by identity as `{entry_point, kind, label, steps}`,
  `kind` being `route` or `export`, each step `{depth, level, repository, via, node, ...}`: `via`
  is `entry`, `call` or `link` (a link step has the link's `reason`), `node` the symbol, file
  or side reached (a note's call site), and `mark` (`cycle`, `seen`, `ambiguous` with its
  `candidates`, `unresolved`, `no-provider` with its `contract`, `produces` or `consumes` of an
  unresolved channel) when the step is not a plain node; steps are in walk order and cross
  repositories; `imports` (absent when empty) lists each
  call resolved into a package as `{site, specifier, name, level, how, target}`, by site then
  target, where `level` is the import's own level (how certain the entry and the name were), not
  the hop's: the link from that call carries the hop's level, the weaker of it and the package
  link's. `how` is the field or rule that named the entry (`exports`, `types`, `module`,
  `main`, `path`, or `outDir` when the output-to-source mapping found the file); for an
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
- New **package ecosystem** (spec #22 user story 26, track B packs): a pack for another manifest
  format (`Cargo.toml`, `pyproject.toml`, `go.mod`) plugs in beside npm without a new kind of
  contract. Discovery is a file name in `manifests_found` (`index.rs`), which stamps the file
  into `manifests` like `package.json`; a reader per ecosystem, beside `package::read`, turns
  its bytes into the same `Summary` (name, version, dependency entries, entry fields) and never
  fails; the contract is `package::<ecosystem>::<name>` (`package::npm::<name>` is the first),
  so `PREFIX` still routes it to the same `declare`, `consume`, `link`, `place` and `reached`;
  and the reason grammar and `flag` stay the one function each, with the ecosystem's range
  syntax checked there (a range it cannot check is `range unchecked`). Resolving an import
  into the provider is the ecosystem's own rule (`ways`, `Provided`). A change to what is
  stored bumps `SCHEMA_VERSION`.
- New **message library** (spec #20, tickets after #45 and track B packs): a form in the pack's
  `messages` (TS/JS: `lang/ts/messages.rs`), matching a call's method and the `Code::origin` of
  its receiver (package and `expr`), reading the name through `Code::expand` (a reader that
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
- New **entry point kind** (spec #31): a variant of `entry::EntryKind` (`name`, `parse`), a finder
  beside `routes` and `exports` in `entry::find`, which reads the repository's own facts
  (`entry::Local`), and the `site` symbol its process starts from. The walk (`Graph::process`),
  `trace` from `process::<identity>` and `Export::processes` need nothing else: they read the
  stored entry points. A process is walked on demand from the stored calls and the links of the
  query, never stored; a new thing the walk must follow is one more arm in `Descent::enter` or
  `Descent::call`, with its `Mark` and its line in `Walk::show`. A change to what is stored bumps
  `SCHEMA_VERSION`.
- New **level**: add the variant to `Level` in `contract.rs` and keep the order strongest first,
  since `Ord` makes a chain's weakest link the max. `Level::name`/`parse` are the only place names
  live; `engine.rs` and `trace.rs` render through them.
- New **language pack**: add it to `PACKS` in `lang/mod.rs`, a fixture
  `tests/fixtures/conformance-<language>/` and one `conform(...)` test in `tests/conformance.rs`.
  A pack is done when that suite passes.
- New **config key**: add the field to `Config`. Parse errors name file,
  line and dotted key by re-parsing the TOML for spans (`config::describe`).
- New **MCP tool**: `mcp.rs`, one `Engine` method, and watch `tests/mcp.rs`: the tool list
  (descriptions plus parameter schemas) must stay within 1,000 tokens.
- New **CLI text**: a `Msg` variant with both languages; command descriptions are set at runtime
  in `main.rs` from `Msg`.

## Invariants the code relies on

- Determinism: all export lists are sorted, hashes are BLAKE3, no clock or random value reaches
  an answer. A refreshed index must equal a cold index of the same files (`tests/freshness.rs`).
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
  never its source text), and what a
  provider document states (`documents.summary`: format, version, title, normalized route paths
  and methods, channel sides (broker kind, name, role), the field paths of their response and
  payload shapes (fields, open parts, scalar leaves) and the external references met, the
  hosts and server paths of the root `servers` URLs, and the parser's reason and line for a
  document it could not read), and what spec #22 stores of the build and package files: the
  `manifests` summaries of each `package.json` (name, version, the dependency ranges as
  written, `exports` as written, `types`, `module` and `main`), the `tsconfig.json` rows
  (`outDir` and `rootDir`, as written) and the `exports` table (each file's exported names and
  the definition, constant, type or file they name), the `entry_points` table (spec #31:
  each entry point's identity, kind, label and start symbol, rewritten with the contracts), and
  each community's name and member identities (`communities`, spec #31). `Call::args`,
  `Call::uses`, `Call::arg_types`, `Def::returns`, `Def::type_params`, `Param::default`,
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
  fails; an import from outside the repository is never inferred.

## Tests (`tests/`)

- `tests/common/mod.rs`: `fixture_repo(case, fixture, origin)` copies `tests/fixtures/<fixture>`
  to `$CARGO_TARGET_TMPDIR/<case>/<fixture>` and makes it a git repository (`init`, `origin`,
  one commit). `case` must be unique per test, since tests run in parallel. `chain(case)` builds
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
  ranges are in them.
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
  no fragments, since only `refresh` writes it.
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
  binds, is read where it stands; a parameter captured by a closure is unknown; a nested or rest
  pattern in a destructured parameter binds nothing, and a spread in the object a caller passes
  makes the names it may override unknown.
- `lang/ts.rs`: tsconfig `extends` and nested configs are not followed. `require` and
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
- `channel.rs`: broker clusters are not modelled (one name of one broker kind is one channel
  across the workspace), and an exchange's type is not read: every binding pattern follows
  topic-exchange rules.
- `package.rs`: a manifest never fails indexing, so its reader has fixed limits. `MAX_BYTES`
  (1 MiB; a larger file reads as empty, but is read whole first, as code and documents are) and
  `MAX_NAME` (214 bytes; a longer name is no package). A range is stored as written, with no
  length limit beyond `MAX_BYTES`; `flag` checks only one of at most `MAX_RANGE` (256 bytes) and
  calls a longer one `range unchecked`. Import resolution: `exported` follows at most
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
  larger parts are unreadable or open. `yaml.rs`: `MAX_NESTING` (128 nested collections, as
  `serde_json` has for JSON) and the alias budget (`EXPANSION` times the source plus `SLACK`, no
  marker). `schema.rs`: `MAX_DEPTH` (128 levels per shape), `WORK` (64 MiB per document) and
  `BUDGET` (10,000 schemas per shape, no marker). `openapi.rs`: a path item reference behind
  another one that is external is dropped without a status line (no marker).
- `drift_view.rs`: `context`, `search` and `status` find each shown link's consumer call by
  scanning the calls of every repository, once per link (no marker); an index by call id waits
  for a workspace that makes this slow.
- `http.rs`: an Express app or router passed as a parameter, `router.route(path)` chains and
  `require` are not followed.
- `entry.rs`: an export entry point is a function, or a method of an exported class (a
  constructor excepted), that the `exports` rows name and no call reaches in its repository,
  an ambiguous candidate counting as a caller; a method of a class that is not exported, a
  default-exported anonymous function and an export the resolver cannot place are not found (no
  marker). A route handler that is a file, not a symbol (`export const GET = withAuth(...)`,
  an Express handler Meada could not find), has no entry point.
- `trace.rs` (process walk): a link at the `ambiguous` level is followed as `trace` down
  follows it (each provider a step, expanded); only an ambiguous call is one unexpanded step.
  A process is walked down only; `trace` up from `process::...` answers with a note.
- `repo.rs`: a path git still quotes (tab, newline or quote in the name) is skipped by
  `uncommitted()`.
- No `busy_timeout`: two concurrent refreshes of one repository may hit `SQLITE_BUSY` (no marker).
