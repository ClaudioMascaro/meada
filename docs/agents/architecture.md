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
  items, derived in one place. `trace.rs`: the `Graph`, the walk and the chain footer.
  `render.rs`: token budget and line helpers. `mcp.rs`: three read-only tools. `main.rs`: clap.
  `msg.rs`: the CLI message catalog. Detail: [map/queries.md](map/queries.md).
- `decide.rs`, `decisions.rs`, `decider/`: `meada decide`, the decisions file and its replay, and
  decider mode (the only network path). Detail: [map/decisions.md](map/decisions.md).
- `workspace.rs`, `config.rs`, `repo.rs`: repositories, peers, `meada.toml` and identities.
  `index.rs`: the SQLite schema, the freshness pipeline in `refresh`, and `export`. `lang/`: the
  language adapter and the TS/JS pack. `resolve.rs`: calls to definitions (`Resolver`), values
  and receivers (`Code`). Detail: [map/indexing.md](map/indexing.md).
- `contract.rs`, `http.rs`, `resource.rs`, `channel.rs`: the `ContractKind` trait and the HTTP,
  resource and channel kinds. `package.rs`: npm packages as contracts (spec #22). Detail:
  [map/contracts.md](map/contracts.md).
- `document/`, `shape.rs`: OpenAPI and AsyncAPI provider documents and the shape a contract
  carries. Detail: [map/documents.md](map/documents.md).

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
- Contract: `<prefix>::...`, `http::GET::/users/{}`, `resource::postgres::orders`,
  `channel::kafka::orders.created` (name kept as evaluated; `channel::kafka` alone for an
  unresolved side). A path parameter of any spelling is `{}`.
- Export JSON: `{repository, files, symbols, calls, providers, consumers, bindings, documents,
  publishes, packages, dependencies, imports, links}`; `imports` (absent when empty) lists each
  call resolved into a package as `{site, specifier, name, level, how, target}`, by site then
  target, where `level` is the import's own level (how certain the entry and the name were), not
  the hop's: the link from that call carries the hop's level, the weaker of it and the package
  link's. `how` is the field or rule that named the entry (`exports`, `types`, `module`,
  `main`, `path`, or `outDir` when the output-to-source mapping found the file); for an
  `ambiguous` import it names the declared field or rule whose entry could not be followed to a
  source file; a provider has
  `gateway` when `[serve]` rewrites its path, and `shape` (`{source, fields, open}`, `open`
  absent when empty) when a document gives its route or channel side one; a consumer has the same
  `shape` for a channel side a document lists. `documents` (absent when none was read) lists each
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
- New **match rule** (a path rewrite, a new provider source): the kind's `link`, the open-item
  candidates in `items.rs` (`http::route_fits`) and each candidate's contract must agree.
  `meada decide` writes the candidate's contract as the decision's target and
  `decisions::evaluate` checks it against the link's candidates, so a candidate carrying the
  consumer's contract instead of the provider's own makes every decision on it stale.
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
  method of an unresolved consumer (`contracts.fragments`, `contracts.method`), and what a
  provider document states (`documents.summary`: format, version, title, normalized route paths
  and methods, channel sides (broker kind, name, role), the field paths of their response and
  payload shapes and the external references met, the
  hosts and server paths of the root `servers` URLs, and the parser's reason and line for a
  document it could not read), and what spec #22 stores of the build and package files: the
  `manifests` summaries of each `package.json` (name, version, the dependency ranges as
  written, `exports` as written, `types`, `module` and `main`), the `tsconfig.json` rows
  (`outDir` and `rootDir`, as written) and the `exports` table (each file's exported names and
  the definition or file they name). `Call::args`,
  `Param::default`, `Field::default` and `Const::value` are `serde(skip)`, so unchanged files come
  back from SQLite without them. `Code` records every file it needs but lacks as a miss, and
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
  goldens never change.
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
  and a `Value::New(callee, args)`.
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
- `http.rs`: an Express app or router passed as a parameter, `router.route(path)` chains and
  `require` are not followed.
- `repo.rs`: a path git still quotes (tab, newline or quote in the name) is skipped by
  `uncommitted()`.
- No `busy_timeout`: two concurrent refreshes of one repository may hit `SQLITE_BUSY` (no marker).
