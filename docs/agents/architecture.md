# Architecture map

Which module owns what, the seams between them, and what the code relies on. Verified against
`src/` and `tests/` of the v0.1 trunk; the code wins over this file, and a change to a seam
updates it. Terms are `GLOSSARY.md`'s; decisions are in `docs/adr/`.

## Flow

`Engine::open(dir)` finds the **workspace**, creates or recreates each **repository**'s **index**
in its own `.meada/index.db`, and ATTACHes every **peer** to the first repository's connection
as `peer<n>`. Every query (`export`, `search`, `context`, `trace`) calls `refresh()` first, then
reads each index through `index::export`, adds the contracts each `meada.toml` declares
(`ContractKind::declare`), derives **links** from both, and renders.
Nothing outside `.meada/` is written. Everything the engine returns is sorted: the same files
give byte-identical output.

## Modules (`src/`)

- `engine.rs`: `Engine` is the one query seam. The CLI, the MCP server and almost every test call
  it: `open`, `refresh`, `export`, `search`, `context`, `trace`, `open_items`, `local_items`,
  `local_item`, `decision_report`, `local_decisions`, `root`, plus `status(dir)` (never creates
  an index; when every repository is already indexed at the current schema it refreshes them
  like a query, to evaluate decisions, and writes
  only inside `.meada/`; otherwise, or when that fails, it prints the states and no decision
  lines). It owns the `context` and `search` rendering; `trace` lives in `trace.rs`. `search`
  kind `resource` is the contracts under `resource::` (matched by kind and id), the same lines
  `contract` and no kind list; `context` on `resource::<kind>::<id>` lists the writers and
  readers across the workspace (`contract_sides`, shared with channels). `search` kind
  `channel` is the contracts under `channel::`, matched by broker kind and name (an unresolved
  side by its broker kind, printed `channel::kafka (unresolved)`); `context` on
  `channel::<broker>::<name>` (or `channel::<broker>`, the unresolved sides of that broker, also
  accepted as `search` prints it, through `channel::typed`) lists the producers (the symbol
  around the producing call) and consumers (the handler) across the workspace, those linked
  through another name included with `[<level>: <reason>]` after them: the producers a
  wildcard consumer matches (`channel::fits`), the wildcard consumers that match the channel,
  and the publishes its bindings route to a queue (`Routes::reached`). `context` on a symbol adds
  `produces <channel>` (at the producing call), `publishes <exchange> <key>` followed by
  `produces <queue> [<level>: <reason>]` for each queue the bindings route it to, or `consumes
  <channel>` (the handler) to its `contracts:`. Which contracts `context` lists by side, and
  the words for them, is `ContractKind::sides` (`writer`/`reader`, `producer`/`consumer`). After
  a `provides <route>` line with a shape come `<repository> <document> response fields: <paths>`
  and, when some are open, `<repository> <document> response open: <paths>`
  (`shape_lines(identity, shape, what)`; a channel side in `contract_sides` gets the same two
  lines with `payload` instead of `response`, indented under its side, and a side only a
  document lists is `<repository> <role> <document> (documented only)`). `context` on an HTTP
  route (`http::GET::/x`, as `search` prints it) goes through the same `contract_sides` with
  the role `provider` and no consumer role: one `<repository> provider <path>:<line> <handler>`
  or `<repository> provider <document> (documented only)` line per provider, each followed by
  its `response` shape lines (which name the document); its consumers are calls, left to
  `trace` and to the `contracts:` of their symbols. Only `ContractKind::sides` makes a start
  point to `context` in `trace`, so a route stays a `trace` start. `status` also prints,
  under each repository, the lines `Engine::status_report` gathers (resource lines,
  then document lines from `document::report`; same indexed-only rule as
  decisions). `search` shows a route only a document lists at `<document path> (documented
  only)` (`Export::document_of`). `search` kind `document` (`search_documents`, an early branch
  like the open-item kinds) lists the documents read across the workspace, matched by path and
  `info.title` (exact, prefix, substring), as `<repository> <path> (<Summary::shown()>)
  <title>`; `shown` is `openapi 3.1.0, routes: 3` or `asyncapi 3.0.0, channels: 2` (the count
  follows the format, so OpenAPI lines never gain a `channels: 0`).
- `items.rs`: open items (`OpenItem`, `Candidate`, `ItemKind`, re-exported by `engine`) and `list`,
  the one place that derives them. `list` takes the exports and the core's links (before any
  decision), so an item is an ambiguous link or an unresolved consumer, in id order, whose
  candidates sort by repository, method, path, as if undecided. The routes a dynamic URL may
  reach come from `http::route_fits` (method and ordered literal fragments, see `Found::method`
  / `fragments`). `Candidate::describe` is the one candidate line (repository identity, method,
  path) shared by the prompt and the excerpt. `Engine::items` pairs each item with whether a
  decision applies (its call has a `Level::Decided` link after replay); `open_items` and
  `local_items` drop those, so `search` with kind `ambiguous` or `unresolved` and `meada
  decide` never offer a decided call, while `local_item(id)` keeps it, for `--item` to redo.
- `decide.rs`: `meada decide`. `items(engine, only)` (`Engine::local_items`: the open items
  whose call lives in the repository where Meada runs, id order; with `--item`,
  `Engine::local_item`, which also takes a decided call, offered with every candidate so the
  new pick replaces its entry; an id that is neither is an error),
  `ask(items, input, out) -> Vec<Decision>` (human mode: prints each item and its numbered
  candidates, reads plain lines, `s` or empty skips, end of input ends), `finish(engine, picks,
  dry_run, out)` (one write at the end through `decisions::canonical` + `save`; nothing picked
  writes nothing), `list` (`--list`: open items, then the repository's stale and waiting
  decisions from `Engine::local_decisions`) and `prune` (`--prune`: `decisions::pruned` drops
  exactly the stale entries and `save` rewrites the file; waiting ones stay; no write when none
  is stale) and `run`, the public entry. Human mode reads no environment variable and
  opens no connection. With `--decider` (and neither `--list` nor `--prune`), `run` reads
  `decider::Config` first (and validates `--min-p` with `decider::min_p`, before the
  variables), then hands the items to `decider::run`, which returns an `Outcome {decisions,
  failed}`; the decisions go to `finish` and, when `failed > 0`, `run` then returns an error
  (`meada: N item(s) failed`, exit 1) so a script sees the failure while the other items are
  already written. `--show-request` stops after printing.
- `decider/`: decider mode, the **only network path of Meada**. `mod.rs` owns
  `Config::from_env` (the four `MEADA_DECIDER_*` variables, read nowhere else: errors name the
  variable, `decisions` defaults to OpenRouter and `typesafe/jev-1.13` and needs the key, `chat`
  needs URL and model; HTTPS except to a loopback host), `Question` (the excerpt: call
  expression, the enclosing span read from disk only when the pack's parse of the file says
  that symbol is a function, method or inline function (`function_like`), so a call in a class
  field initializer goes alone and the top level says so; `offers: Vec<Offer {key, description,
  target}>`, keyed `"1".."n"`, described by `Candidate::describe`), `MAX_CANDIDATES` (50: more
  is skipped in decider mode only), `Config::requests` (the protocol dispatch), `show`
  (`--show-request`, the
  `Authorization` value printed as `<hidden>`) and `send` (`ureq`, rustls, fixed `TIMEOUT`, no
  proxy to a loopback host, no redirects, no retries; a non-success status is the error
  `Msg::HttpStatus` (`http status: <code>`, plus `: <message>` when the redacted body has
  `{"error": {"message"}}`)). `Config::shown(text)` is the one gate for every decoded upstream
  string printed or recorded (error message, model id, serde error text, type, keys, choice):
  control characters dropped, then the key replaced by `<hidden>`, since a JSON escape hides
  it from the raw-body redaction in `send`. `Config` has no `Debug`; the key
  leaves it only as the bearer header.
  Protocol adapters are files beside it that build requests and read answers: `chat.rs`
  (`request(config, question)`, one `POST <url>/chat/completions` per item; `answer(config,
  question, body) -> Result<Proposal>` reads `choices[0].message.content` as `{choice,
  probability}`, `by` = the response `model` or the configured one). `decisions.rs`
  (OpenRouter's alpha API: `request(config, questions) -> Option<Request>`, one `POST
  <url>/api/alpha/decisions` carrying every eligible item, `state {task, repository, items:
  name -> excerpt}` (upstream: `state` is the content to evaluate), `questions` named
  `item1..` zero-padded so the sorted maps keep item order, each `{type: choice,
  instructions: "... the call in items.<name> ...", criteria: key -> describe}`, `provider
  {zdr: true, data_collection: deny}`; `answers(config, asked, body)` parses `{model, answers, id?, provider?,
  usage?}` and each `{type, choice, confidence, probabilities}` with `deny_unknown_fields`: a
  shape error or an answer to an unknown question fails every item, an answer's values (type
  `choice`, choice and probability keys offered, each in [0,1], sum 1 within `SUM_TOLERANCE` =
  0.01, `confidence` in [0,1]) fail its item; `p` = probability of the choice, `by` = response
  `model`, `confidence` printed, not recorded). A refusal of the data-retention preferences
  arrives as a non-success status: every item of the request fails, nothing is written,
  nothing retries. A new protocol adds a file and one arm each in `Config::requests` and
  `Config::answers`.
  `validate.rs` is shared by every protocol: `proposal(config, question, choice, p, by) ->
  Result<Proposal>` (choice among the offered keys through `offered(config, question, choice)
  -> Result<&Offer>`, `p` in [0,1] through `probability(p)`, `by` not empty and not `human`; a
  `Proposal {decision, p, candidate, confidence}`) and `invalid(reason)` (the `invalid answer:
  <reason>` error text). `run`
  prints per item: `<id>` then `proposed: ...` (`p >= min_p`, returned to be written),
  `suggestion, not written: ...` (below it, `MIN_P` = 0.5 unless `--min-p`), both with
  `confidence` when the protocol gives one, or the error (`request failed: ...` /
  `invalid answer: ...`, counted in `Outcome::failed`).
- `main.rs`: clap, with `index`, `status`, `export`, `mcp`, `decide` (`--item`, `--dry-run`,
  `--list`, `--prune`, the last excluding the first three; `--decider`, and `--show-request`,
  and `--min-p`, which require `--decider`). `search`, `context` and `trace`
  are reachable through MCP and the library, not the CLI.
- `mcp.rs`: three tools (`search`, `context`, `trace`) over rmcp, each a thin call to `Engine`
  that reopens it per call (so it refreshes). Stdout is the protocol: log to stderr. No tool writes.
- `workspace.rs`, `config.rs`: `Workspace::discover` finds `Repository { root, identity, config }`
  from the `meada.toml` files (peers by `path` hint, then sibling clones; siblings that name a
  repository already in join, transitively; a sibling with an invalid `meada.toml` or
  `meada.decisions.toml` is skipped). Peers not found go to `Workspace::missing`. `config` parses
  `[[peer]]`, `[serve] hosts`, `[serve] prefix` and `strip` (each must start with `/`, normalized
  by `gateway_key`), `[serve] documents` (`repo_paths`: repository-relative, none absolute or
  with a `..` segment, `./` and doubled `/` dropped; added to the documents discovery finds;
  `Serve::declared` is set when the `[serve]` table is present, even empty or with only
  `documents`, which makes the repository's documents provider sources) and `[[resource]]`
  (`kind` checked as a lowercase identifier, `id` not empty,
  `role` read, write or read-write, optional `via`) with `deny_unknown_fields`. `config::gateway_path(prefix,
  strip, path) -> Option<(gateway path, reason)>` is the one gateway rule (spec #19); `Serve::
  gateway_path(path)` applies it with `[serve]` prefix and strip, `Http::declare` with a
  document's server path as the prefix. `config::read<T>(root, name)` is the shared TOML reader (file, line and key in
  every error), also used for the decisions file; `Repository` carries both `config` and
  `decisions`, loaded together at discovery.
- `decisions.rs`: `meada.decisions.toml` (ADR 0006). `Decisions { items: Vec<Decision> }`,
  `Decision { kind (ambiguous-link | dynamic-url), call, target, by, p }`, `load(root)` and
  `replay(links, workspace, exports) -> (links, Vec<Evaluation>)`, called by
  `Engine::links_and_decisions` at the end. Replay is a pure function of links, exports and the
  files, so nothing is stored and a changed file needs no reindex. `evaluate` is the one place
  that decides a decision's `Fate`, in this order: call gone (`Cause::CallGone`), target
  repository a missing peer (`Waiting`, neither applied nor stale, whatever else holds), item no
  longer open (the core resolves the call on its own, or the item is not of the decision's kind:
  `ItemNotOpen`), target gone (`TargetGone`), target not among the ambiguous link's candidates
  (`NotCandidate`); else it applies. A stale or waiting decision changes no link. `Evaluation {
  repository, stale: Vec<Stale {call, target, cause}>, waiting: Vec<Waiting {call, target, peer}>
  }` is per repository, in file order; `Engine::decision_report()` returns them for the workspace
  (`Engine::local_decisions()` the one of the repository where Meada runs,
  `Engine::status_report()` them and the resource lines in one pass), `export` and `status` render
  them, and `Evaluation::lines` is the one rendering of the stale and waiting lines (`status`,
  `meada decide --list`); `--prune` reuses the same call. `probability(p)` is the one [0,1] check,
  for the file's `p`, a decider's answer and `--min-p`. `load` rejects a file that repeats a
  `call` (parse error `<file>:<line>: `call`: ...` at the repeat): one entry per call is a file
  invariant, so replay and the writers never see duplicates. The writer is `canonical(root, new)`
  (existing entries minus those of the same `call`, plus `new`, sorted by `call`, no comments or
  timestamps; `Decision::human(item, candidate)` builds a human pick) and `save(root, text)`.
- `repo.rs`: root, identity (normalized `origin`, else `local/<dir>`) and `uncommitted()`, all by
  shelling out to `git`.
- `index.rs`: SQLite schema, `refresh`, `export` and the export types. Freshness is a pipeline in
  `refresh`: stat (mtime/size, trusted only if older than `RACY` = 2 s), hash with BLAKE3, parse
  only changed files, resolve every file's calls in memory, rewrite rows only when a file's
  `resolved` digest changed, re-extract contracts for the whole repository. `refresh(db, root,
  identity, serve)` first brings the `documents` table (path, hash, mtime, size, `summary`) up
  to date the same way (`refresh_documents`, sharing `stat`), before the early exit and apart
  from the code rows: a document edit re-extracts no code. Without `serve.declared` the
  repository has no documents and stored rows are deleted. The walk yields every file the
  ignore rules keep; `lang::pack_for` and `document::is_document` split it, and
  `documents_found` adds each `[serve] documents` path that is a file, stat'ed outside the walk
  so `.gitignore` does not hide it; a listed path that is not a file, or that has a symbolic
  link in any segment (`plain_file`; the walk follows none either), has no row. `export` loads
  `Export::documents` (`Document {path, summary}`, path order). `sort_providers` and
  `sort_consumers` put the lists back in `export`'s order after a kind's `declare` added sides.
- `document/` (spec #21): provider documents. `mod.rs`: `is_document(path)` (discovery by file
  name: `{openapi,asyncapi}.{yaml,yml,json}` and `*.{openapi,asyncapi}.{yaml,yml,json}`),
  `read(path, bytes) ->
  Summary` (never fails: JSON by extension through `serde_json`, else YAML; what cannot be read
  becomes a `Problem`, with the line clamped to the file's last line; the content decides,
  since a listed document can have any name: a top-level `openapi` field goes to `openapi.rs`,
  an `asyncapi` one to `asyncapi.rs`, and with neither the name only picks which format it is
  reported as not being, `asyncapi.rs` for an `asyncapi` name, `openapi.rs` for any other),
  `Summary {format: Option<Format>,
  version, title, routes: Vec<Route {path, method, shape}>, servers: Vec<Server {host, path}>,
  channels: Vec<Side {broker, name, role, shape}>, problems: Vec<Problem>}` (stored as JSON; `Route::contract`, `Route::label` = `GET
  /users/{}`; `servers` are the root `servers[].url` entries with variables replaced by their
  defaults in one pass over the URL (a default naming a variable stays as written), a URL
  still holding one dropped, a relative URL giving only its path, host lower
  case, path normalized with no trailing `/`; `Side::contract`, `Side::label` = `kafka send
  orders.created`, `Role::{Producer, Consumer}`; sorted by broker kind, name, role),
  `Problem::Unreadable {line, error}`,
  `Problem::Format {found}`, `Problem::AsyncFormat {found}`, `Problem::Protocol {protocol}`,
  `Problem::Parameters {channel}`, `Problem::NoServers`, `Problem::NoProtocol {server}`,
  `Problem::UnknownServer {channel, server}`, `Problem::ExternalRef {reference}` and
  `Problem::ExternalPathItem {reference}` (structured, rendered through `Msg`), `Format::{Openapi,
  Asyncapi}` (stored as `openapi` and `asyncapi`; `Msg::DocumentRead` takes it with the count),
  `Summary::shown()` (what `search` prints of a document read), `Summary::count(format)`
  (routes for OpenAPI, distinct channels of the sides for AsyncAPI) and
  `report(repo, export)`: the status lines, per document in path order (`Msg::DocumentRead`
  for one read, each problem of it, then each route a later document by path lists again:
  `Msg::DuplicateRoute`), then `Msg::DocumentMissing` for each `[serve] documents` path with
  no row. `openapi.rs`: `read(path,
  value)`: a top-level `openapi` of `3.0` or `3.1` (any patch; a YAML number counts), else
  `Problem::Format` naming the `openapi` or `swagger` field it found; every `paths` key starting
  with `/` and every method of `get put post delete options head patch trace` is a route (a
  path item that is a local `$ref`, such as `#/components/pathItems/...`, is read where it
  points; an external one is a `Problem::ExternalPathItem` and its routes are not read), its
  templates (`{id}`) written as `{}` before `http::normalize_path`, sorted and deduplicated (of
  two templates of one route, the first key gives the shape). A route's `shape` is the
  `application/json` schema of its lowest explicit 2xx response that has one, else of `2XX`
  (a response may be a local `$ref`; `application/*+json` is not read); every external `$ref`
  met, once, sorted, is a `Problem::ExternalRef`. `schema.rs`: `read(doc, schema, source) ->
  (Shape, external refs)`, the schema-to-shape reader: `properties` give fields, `items` the
  `[]` elements (an array with no `items` has open elements), `allOf`, `oneOf` and `anyOf` add
  their members' fields (an `allOf` member with no structure keyword adds nothing; anywhere
  else such a schema, or `true`, is open); `additionalProperties` `true` or a schema opens the
  object, and an object type that lists no fields, no members and no `$ref` is open unless
  `additionalProperties` is `false`. Local `$ref`s (JSON pointers into the document) resolve; a
  reference already being read (a cycle), a missing target and an external reference leave
  that part open. Reading one shape visits at most 10,000 schemas (`BUDGET`) and goes at most
  `MAX_DEPTH` (128) levels deep, a property, element, member or reference being one level; all
  the shapes of one document share one `schema::Work` (`WORK`, 64 MiB: 32 per schema visited
  plus the bytes of every field path listed), which the reader of each document creates and
  passes to every `read`. Past any of them, every part not read is open, so a document's shapes
  have bounded memory and the recursion bounded depth. To read several schemas as one shape
  (AsyncAPI messages), read `{"oneOf": [...]}` of them. `asyncapi.rs`: `read(path, value)`: a top-level `asyncapi` of `2.x`
  or `3.0` (any patch), else `Problem::AsyncFormat` naming the `asyncapi`, `openapi` or `swagger`
  field it found. 2.x: the key of a channel is its name, `subscribe` is a producer and `publish`
  a consumer (the application's side, as the specification reads them), the payloads those of
  `message` (or each member of its `oneOf`), a message a local `$ref`; the channel's `servers`
  are server names. 3.0: the name is the channel's `address` (a channel with none is skipped),
  `action: send` is a producer and `receive` a consumer, the payloads those of the operation's
  `messages` when it lists them, else every message of its channel; the channel's `servers` are
  `$ref`s to `#/servers/<name>`. A channel uses the servers it lists, else every server of the
  document; the lower-cased `protocol` gives the broker kind (`kafka`, `kafka-secure` ->
  `kafka`; `amqp`, `amqps` -> `amqp`; `nats`; `redis`; `sqs`), any other is a `Problem::Protocol`
  once and its channels are ignored. A document with channels and no root `servers` is a
  `Problem::NoServers` (every channel ignored), a server a channel uses without `protocol` a
  `Problem::NoProtocol`, and a server name a channel lists that the document lacks a
  `Problem::UnknownServer` (the channel is ignored on it). A name holding `{` or `}` is a `Problem::Parameters` and
  ignored (AMQP bindings are not read). Sides with the same broker kind, name and role (several
  operations, several servers of one kind) are one, their payloads read together as one
  `oneOf` shape. `yaml.rs`: `parse(text) -> Result<Value,
  (line, reason)>` over `yaml-rust2`'s event parser, taken one event at a time with
  `Parser::next_token` (never `YamlLoader`, nor `Parser::load`, which recurses per level of
  nesting), the collections being read on an explicit stack, at most `MAX_NESTING` (128, as
  `serde_json`) deep (deeper, the document is unreadable at that line): first document only, keys as written, plain scalars typed by the YAML 1.2 core schema,
  quoted and block scalars strings, a non-scalar key an error, and every node built, aliases
  included, charged to a budget of 4 times the source length plus 64 KiB (over it, the
  document is unreadable). Readers only see `serde_json::Value`.
- `shape.rs` (spec #21, ADR 0009): `Shape {source, fields, open}`, the one form of what a
  contract carries, whatever it is read from, so shape drift (spec #23) compares shapes without
  caring about their source. `fields` and `open` are sorted sets of field paths: dotted, `[]`
  for array elements (`items[].id`), `[]` first for a payload that is an array (`[].id`); every
  node below the root is a field (`items`, `items[]`, `items[].id`); the empty path is the root
  (`shape::shown` prints it `(root)`). An open path is a part whose fields Meada cannot list:
  anything below it may be present. Presence only: no types, formats, `required` or
  `nullable`. `Source::Document(path)` (exported `{"document": "openapi.yaml"}`) is the only
  source yet; spec #23 adds its code sources as variants. `Source::at` names it in `context`.
- `lang/mod.rs`: the **language adapter**, `LanguagePack` (extensions, `parse` to `Facts`,
  `config_files`, `modules`, `messages`), and the `Facts`/`Value` types. `lang/ts.rs` is the only
  pack (`.ts .tsx .js .jsx .mjs .cjs`). `parse` sees one file at a time; no pack method touches
  the index. `Value` has `New(callee, args)` (`new X(...)` as a value) and `List` (an array literal);
  `property(object, key)` (the last key, through spreads; `Err(Depends)` when a spread of a
  parameter may set it) and `literal(parts)` are shared by kinds and packs. An anonymous function
  written as a property of an options object passed as an argument (`run({ eachMessage: () =>
  {} })`, or an object method there) is a `Kind::Inline` definition and a `Value::Function`,
  like a function argument. `messages(code) -> Vec<Message>` (default: none) is the **producer
  and consumer output** (spec #20): `Message {broker, name: Option<String>, path, call, role}`,
  `role` `Producer` or `Consumer(Handler)`, `Handler::Def {path, name}` (a definition; a name
  that folds to nothing is the top level, so a pack passes a call's `scope` for "the enclosing
  function") or `Handler::Inline {path, def}`; `Role::Publish {exchange}` (a publish to a named
  exchange, `name` the routing key) and `Role::Binding {exchange, pattern}` (`name` the queue)
  are the RabbitMQ roles that are neither a plain producer nor a consumer. `name` is `None`
  when it cannot be evaluated (an unresolved side). The pack reads facts through `Code` (values, `Code::origin`,
  `Code::expand`) and reports; the channel kind turns the output into contracts. TS/JS library
  forms live in `lang/ts/messages.rs`: kafkajs `send({topic})` on a `new Kafka().producer()`,
  `subscribe({topic})` or `({topics})` on a `new Kafka().consumer()`, with the handler of the
  first `run({eachMessage|eachBatch})` on the same consumer binding (else the function whose
  call subscribes); AWS SDK v3 `SendMessageCommand` / `ReceiveMessageCommand` (the queue is the
  last segment of the `QueueUrl`, so only that tail has to evaluate; the receive's handler is
  its enclosing function); `sqs-consumer` `Consumer.create({queueUrl, handleMessage})`; NATS
  `publish(subject)` / `subscribe(subject, {callback})` on a `connect()` connection (the
  handler is the `callback`, else the enclosing function); BullMQ `add` on a binding that holds
  `new Queue(name)` (the queue is named by the binding, read with `Code::expand_constructed`:
  the `add` call is the producer, or, when `name` is a parameter of the function that makes the
  binding, each call site of that wrapper) and `new Worker(name,
  processor)` (the processor, a definition or a function in place, is the handler); ioredis
  `xadd(stream, ...)` and `xreadgroup(..., 'STREAMS', s1, s2, id1, id2)` (every stream of the
  first half after `STREAMS`; the enclosing function handles) on a `new Redis()` or default
  import; node-redis `xAdd(stream, ...)` and `xReadGroup(group, consumer, {key} | [{key}])` on a
  `createClient()` binding; and amqplib on a channel that
  traces to `connect().createChannel()` or `createConfirmChannel()` (default import, namespace
  or named `connect`): `sendToQueue(queue)`, `consume(queue, handler)` (a function in place or
  a definition; else the enclosing function), `publish(exchange, key)` and `bindQueue(queue,
  exchange, pattern)`, each name read through `Code::expand` like a topic. `new X(...)` is a
  call fact like `X(...)` (the `new_expression` arm of `ts.rs`'s `calls`, callee `X`), so a
  form can start at a constructor, and `value` reads through `await`, so `const nc = await
  connect()` or `const channel = await conn.createChannel()` is a traced binding. Both are in
  every export (`Kafka#0` and the like: unresolved calls, no target).
- `resolve.rs`: the shared resolution behind the adapter. `Resolver` connects calls to definitions
  (scope, imports, re-exports, `this.method`, member chains) and gives each a `Resolution`. `Code`
  folds constants and client wrapper arguments into values for contract kinds, a destructured
  object parameter reading its names (`Param::fields`) off the literal object the caller passes.
  `Code::expand` climbs only callers that resolved exact: a call found only by a name match
  (`Caller::by_name`: `data.get(...)` matched to a wrapper method, or a bare name no import
  binds) may be on any object or reach another function, so a wrapper with no exact caller is
  read where it stands.
  `Code::origin(path, scope, chain) -> Option<Origin {package, expr, binding}>` is the
  **receiver tracing** every message library form relies on: it follows a name or member chain
  through `const` bindings (same file and imported) down to an import whose module is outside
  the repository (`Binding::External` keeps the specifier and imported name), and spells what
  the code did with it from that name: `new Kafka().producer()` for `kafkajs`. A namespace import
  adds no name, a default import is `default`; `binding` is the constant the chain's head names
  (how a pack pairs calls on the same client). `Code::expand_constructed(binding, at, read)` is
  `expand` over the arguments of the `new` a binding holds, on behalf of the call `at` made on
  it. `Code::pack()` is the pack of the files.
- `contract.rs`: `ContractKind` trait (`prefix`, `extract`, `declare`, `sides`, `link`),
  `KINDS`, `Level`, `Link`, `Names`, `Role`. A `Found` carries its `role` (`Provider`,
  `Consumer`, `Binding` or `Publish`),
  stored in `contracts.role`; the site does not imply it (a channel producer is a `Site::Call`, a channel
  consumer a `Site::Def` or `Site::Inline`). A `Site::Def` name is folded through `Names::scope`
  in the index: unnamed inline segments drop, and an empty result is the file's top level. `http.rs`: `fetch` consumers (also a call to a parameter defaulting to
  `fetch`), Next.js app-router and Express providers, `normalize_path`, and the link rules (host,
  `base_url_env`, unique provider). `declare` first adds the routes of the repository's
  documents (spec #21): a document route whose contract a code provider already has merges
  into it (one provider, the code's handler site); any other becomes a provider of its own at
  `<repository>::<document path>::<METHOD> <path>` (no handler), and a route an earlier
  document by path listed adds nothing. Each provider of a documented contract, merged or not,
  takes `Provider::shape` from the first document by path that lists it (none when that
  document's route has no shape). A document provider is a written contract: it lowers
  no level (a host match is `exact`). A route also matches by its gateway paths: `declare`
  fills `Provider::gateways` (`Gateway {path, reason}`, not exported; the `[serve]` form, which
  is also the exported `Provider::gateway`, first, then the forms of each document listing the
  route: one per server path of the document, with `[serve] strip`, or the `[serve]` form when
  the document has no server path; a code route always has its `[serve]` form, a route only
  documents list has it only through a document without server paths), `Provider::hosts` (not
  exported: `[serve] hosts` plus the host of every `servers` URL of the repository's documents,
  checked by `link` for a `host:` consumer). `shape`, `gateways` and `hosts` are set on HTTP
  providers only; `http::gateway_contracts(provider)` yields each as
  `(contract, reason)` and `http::serves(provider, contract)` is the one check of any path (also
  used by open items and decision replay, so a gateway candidate carries the provider's own
  contract). That link is `inferred` with the form's reason (`gateway prefix <p>`, `gateway
  strip <s>` or both), read from the provider, not recomputed from the configuration; within
  one repository a code path match wins over gateway path matches. `resource.rs` (spec
  #19): reads no code; `declare` turns each `[[resource]]` `via` symbol the repository has into a
  provider (writer) or consumer (reader) of `resource::<kind>::<id>`, and `link` joins every
  reader to every writer of another repository at `declared`, reason the contract. A `via` that
  names no symbol is dropped. An entry without `via`, or with none matched, makes the repository
  the side: the `site` is the bare repository identity (no `::`), the **repository-level** side.
  `report` gives the status lines per repository: each unmatched `via`, the fallback to repository
  level, and a resource with no link in the workspace (unlinked). `channel.rs` (spec #20):
  `extract` calls `code.pack().messages(code)`; a producer is a provider at its call site, a
  consumer a consumer at its handler (an inline handler is named `<broker> <names in order>`,
  such as `kafka orders.created`). `contract(broker, name)` is `channel::<broker>::<name>`, or
  `channel::<broker>` for an unresolved side (`channel::parse`, `is_unresolved`, and `shown`, which
  prints it with `(unresolved)`), which `link` skips: it is in the export and never
  linked, and since its contract is not `None` it is no open item (`search` kind `unresolved`
  and `meada decide` never see it). `link` joins every consumer to every producer that
  `reaching(contract, providers, routes)` gives, in any repository including its own. Name
  matches are `fits(produced, consumed)`: the same contract at `exact`, reason the contract,
  and a consumer whose name is a wildcard pattern to the producers its pattern matches
  (`wildcard(broker, pattern, name)`, the one place a broker's matching rules live: NATS `*` one
  token and `>` the rest, a subject holding a wildcard matching nothing, table-tested), at
  `inferred`, reason `<producer contract> wildcard <pattern>`. RabbitMQ (`amqp`): a binding is a
  `Found` with role `Binding`, stored in `contracts` (`contract` the queue's channel, `via` the
  exchange, `method` the pattern, `site` the `bindQueue` call) and exported as `bindings`
  (absent when empty). A publish to a named exchange holds no channel: it is a `Found` with
  role `Publish` and no contract (`via` the exchange, `method` the routing key, `site` the
  `publish` call), exported as `publishes: [{exchange, key, site}]` (absent when empty), so no
  queue named like its exchange or its key ever equals it; a publish to the empty exchange is
  a plain producer of the queue `key`, and one whose key cannot be evaluated an unresolved
  producer `channel::amqp`. `Routes` holds the workspace's bindings and publishes (the broker
  is shared); `routed(publish, binding)` is the one routing rule: same exchange and the
  pattern `pattern_matches` the key (topic-exchange rules whatever the exchange type), `exact`
  when the pattern is the key, else `inferred`, reason `<queue contract> binding <exchange>
  <pattern>`. `reaching` adds those publishes to a queue's producers, and `Routes::reached`
  gives the queues one publish reaches; of several matches to one producer the strongest wins.
  `ContractKind::link` takes the workspace's `Routes` beside its providers. `declare` adds the
  channel sides of the repository's AsyncAPI documents (`Summary::channels`, in path order): a
  document side whose contract equals a code side's of the same direction merges into it (every
  such code provider or consumer keeps its site and takes the document's shape unless it has
  one), any other is a side without a handler, a provider at `<repository>::<document>::<broker>
  send <name>` or a consumer (`via` null) at `<repository>::<document>::<broker> receive
  <name>`; a side an earlier document by path already added merges the same way, so adds
  nothing. Both carry `shape` (`Consumer::shape` is new, exported when present). `link` is
  unchanged by this: the documented sides are ordinary providers and consumers, so a documented
  producer reaches the code consumers of other repositories at `exact`, and a code producer
  reaches the documented consumer (an `amqp` side of a document is a queue name, never an
  exchange, so it is never a publish). It sorts `providers` and `consumers` into the index's
  order afterwards.
- `trace.rs`: `Graph` over all exports and links, depth-first `Walk`, chain footer. A link
  whose consumer is a symbol (a resource reader) is crossed from provider to consumer going
  down and back going up; a call-site consumer the other way. A channel link (producer call,
  handler symbol) goes down from the producing call (`call_down` reads `into[call]`) into the
  handler, and up from the handler (`out[handler]`, a provider that is a call) to the producing
  call and its caller (a publish to a named exchange is such a call, linked through the bindings).
  An unresolved channel side has no link, so `Graph::unresolved_sides` (by site) gives
  the walk one row for it under the producing call (down, and from a call start) or the handler symbol:
  `<repository> <path>:<line> produces|consumes channel::<broker> (unresolved)`, no level and no
  hop. A repository-level side (`Graph::is_repo`) prints as `<repository>
  (repository-level)`, and a route or channel side only a document lists (`Graph::documented`,
  from `Export::document_of` over providers and consumers) as `<repository> <document> GET /x
  (documented only)` or `<repository> <document> kafka send orders.created (documented only)`;
  both end their branch (`Graph::ends`): `Walk::reach`, the one way a link crossing writes its
  row, pushes them and enters anything else. Starts take only HTTP routes, symbols and call
  sites; a start in a kind with `ContractKind::sides` (`resource::...`, `channel::...`) gets an
  answer pointing to `context`.
- `render.rs`: token budget (`budgeted`, `cut`, `tokens`; 1 token = 4 bytes; `DEFAULT_BUDGET`
  2000) and the line helpers `Files`, `label`, `callee`, `unresolved`, plus the markers
  `REPOSITORY_LEVEL` and `DOCUMENTED_ONLY`. Display only.
- `msg.rs`: the CLI message catalog (pt-BR and English, one arm per `Msg`). Only `main.rs` calls
  `msg::init`; `meada mcp` forces English, so the MCP surface and library tests are English.

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
- Document route: `<repository>::<document path>::<METHOD> <path>`, the site of a route only a
  document lists, such as `example.com/chain/gateway::openapi.yaml::GET /api/shipments/{}`
  (`Export::document_of` recognises it). Document channel side: `<repository>::<document
  path>::<broker> send|receive <name>`, a provider (`send`) or a consumer (`receive`) only a
  document lists, such as `example.com/asyncapi/events::asyncapi.yaml::kafka send orders.created`.
- Contract: `<prefix>::...`, `http::GET::/users/{}`, `resource::postgres::orders`,
  `channel::kafka::orders.created` (name kept as evaluated; `channel::kafka` alone for an
  unresolved side). A path parameter of any spelling is `{}`.
- Export JSON: `{repository, files, symbols, calls, providers, consumers, bindings, documents,
  publishes, links}`; a provider has
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
  whole workspace's providers. A kind's rules stay in its own file.
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
- `SCHEMA_VERSION` (`index.rs`) bumps on any change to `SCHEMA` or to the facts a pack stores. An
  index with another version, or an unreadable one, is deleted and rebuilt, and `status` says so.
- The index holds no source text and no values, apart from the literal URL path fragments and the
  method of an unresolved consumer (`contracts.fragments`, `contracts.method`), and what a
  provider document states (`documents.summary`: format, version, title, normalized route paths
  and methods, channel sides (broker kind, name, role), the field paths of their response and
  payload shapes and the external references met, the
  hosts and server paths of the root `servers` URLs, and the parser's reason and line for a
  document it could not read). `Call::args`,
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
- Test files by subject: `conformance` (pack), `index` (export, schema rebuild, writes only in
  `.meada/`), `query` (search/context), `workspace` (peers, status, config errors), `gateway`
  (gateway prefixes), `urls` (constants, wrappers, Express, inferred links), `trace`, `decisions`
  (replay, parse errors), `stale` (stale and waiting decisions in `status` and `export`), `decide`
  (the binary in human mode over scripted stdin), `decider` (decider mode against the stub),
  `freshness` (cold vs refreshed), `items` (open items and their `search` goldens, fixtures
  `open-few` and `open-generated`), `cli` (the binary under pt_BR, en_US and de_DE `LANG`), `mcp`
  (the binary over stdio), `resources` (resource `search`, `context`, `trace` start and `status`
  over the chain), `channels` (the `channels-api`, `channels-worker`, `channels-billing`
  fixture: kafkajs forms, fan-out, same-repository links, an unrelated `send`; amqplib queues,
  publishes through a binding of another repository, the empty exchange, queues named like the
  exchange and like an unbound key that no publish reaches, and a Kafka topic that shares a
  queue's name; the `channels-queues-api` and `channels-queues-worker` fixtures: SQS by queue
  name and NATS wildcards at `inferred`, also in `context` on a subject and on a pattern),
  `channel_queries` (`search`, `context` and `trace` on channels over the first fixture),
  `channels_jobs` (the `channels-jobs-api` and `channels-jobs-worker` fixtures: BullMQ queues,
  a `new Queue(name)` wrapper, and Redis streams in both client styles),
  `documents` (provider documents over `documented_chain`: export, `trace` to "documented
  only", `search` (routes and kind `document`), `status`, listed documents, `context` on the
  merged route with its documented fields, `context` on a route (documented only and merged);
  inline repositories written by the test (`repo`, `client`, `openapi` helpers): each document
  giving its own gateway forms under `[serve] prefix` and `strip`, a `[serve]` table with only
  `documents` and an unlisted gitignored document left out, a listed path through a symbolic
  link left unread; over
  the `documents-shapes` fixture, a repository with no code whose OpenAPI 3.0 JSON document
  has local `$ref`s into `components`, `allOf`, `oneOf`, an array response,
  `additionalProperties: true`, a cycle and an external `$ref`, plus a later
  `v2.openapi.json` listing one route again: the export with every shape and `status` with
  the external reference; freshness of an edited document and of a dropped `[serve]` is in
  `freshness`), `asyncapi` (AsyncAPI documents over `asyncapi-events`, a repository with a
  `[serve]` table whose `asyncapi.yaml` (2.6, `kafka-secure`), `billing.asyncapi.yaml` (3.0,
  `amqp`), `sensors.asyncapi.yml` (`mqtt`), `routing.asyncapi.yaml` (a server without
  `protocol`, a channel naming a server the document lacks), `serverless.asyncapi.yaml` (no root
  `servers`) and `legacy.asyncapi.json` (1.2) sit beside two
  code sides that merge with a documented one, and `asyncapi-services`, which names it as a
  peer and holds the code on the other side of each documented one: both exports with the
  channel contracts, shapes and links, the merge of a documented and a code side, `status`
  (protocol, parameterized channel, version), `trace` down to a documented consumer and up
  from a handler to a documented producer, `context` on documented channels with their payload
  fields, and `search` of channels and of documents). Table tests for discovery, format detection, routes, response selection, the
  schema-to-shape reader (`schema.rs`) and the YAML reader are unit tests in `src/document/`,
  with the hostile inputs each limit stops (server variables naming each other, 200,000 nested
  block sequences, a 10,000-long reference chain, one wide schema read for many routes).
- **The loopback decider stub** (`tests/common/stub.rs`): `Stub::start(script: Vec<Reply>)`
  listens on `127.0.0.1:<free port>` (`Stub::url`, the value for `MEADA_DECIDER_URL`), answers
  the n-th request to `.../chat/completions` or `.../api/alpha/decisions` with `script[n]` (500
  once it runs out, 404 on other paths) and records every request it receives
  (`requests() -> Vec<Request {method, path, headers, body}>`, `header(name)`, `json()`).
  `Reply::chat(model, content)`, `Reply::ok(json)`, `Reply::status(code, body)`.
  `tests/decider.rs` runs the binary with only the decider variables it sets and asserts the
  key is in no output; `--show-request`, `--list`, human mode, `index`, `export`, `status` and
  the MCP server leave the stub with zero requests. Answer cases (malformed, threshold,
  `--min-p`, `--dry-run`) script the stub per test; a malformed first answer must leave the
  second item written.
- A new behavior gets a fixture repo plus a golden; reuse a fixture before adding one. Fixture
  `meada.toml` files declare the peers and `serve.hosts`.

## Known ceilings

Each is marked `ponytail:` at its site unless noted.

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
- `resolve.rs`: wrapper expansion walks every simple path up the callers, and does not climb a
  call found only by a name match, so a wrapper method called on a receiver whose class Meada
  cannot tell (in TypeScript any receiver but `this`, a `const api = new Api()` included), or a
  function called by a bare name no import binds, is read where it stands; a
  parameter captured by a closure is unknown; a nested or rest pattern in a destructured
  parameter binds nothing, and a spread in the object a caller passes makes the names it may
  override unknown.
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
