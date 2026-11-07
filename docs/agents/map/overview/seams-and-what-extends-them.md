# Seams and what extends them

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
  | `use_level` | the strength of a directed side before an evaluated provisioner name weakens it (resources distinguish SDK and declared sides) |
  | `entry` | which entry point kind a side of a contract makes (a pure function of the contract: entry points are stored) |
  | `read_declarations` | indexed locations needed by a declaration, loaded immediately before its `declare`, without opening source files |
  | `declare` | the sides `meada.toml` declares, run at query time (a config edit needs no reindex) |
  | `symbols_read` | the symbols `declare`, `workspace_step`, `workspace_ready`, `link` and `status` look up, which a partial export reads before declaring (resources: their `via` symbols; packages: every symbol of a repository with a named manifest) |
  | `workspace_step` | a step over every repository after every `declare` (the package provider index, dependency entries, imports, shapes) |
  | `workspace_ready` | a prepared view after all workspace steps finish (HTTP routing through image-scoped workloads) |
  | `workspace_links` | optional whole-workspace links instead of the kind's per-consumer hook, with `None` preserving its original order |
  | `after_links` | a pass over the links after the decisions replay (HTTP: drift) |
  | `unresolved`, `label`, `parts` | whether an identifier is an unresolved side, how it is labelled, the parts `search` ranks |
  | `place` | where this kind's site sits when it is neither a symbol nor a call (packages: manifest and field); dispatch uses only the contract's kind |
  | `search_word`, `own_search` | the word of `search`'s `kind` and, for an own renderer, its hits |
  | `sides`, `view`, `related_sides` | the role words and the carried word of `context` on a contract, and the sides that reach it by another name; a contract listed by its providers alone is followed by the linked consumers that read fields, with their drift (the engine's rule: `View::follows_drift`) |
  | `workspace_related_sides` | optional workspace grouping using prepared links, replacing per-repository `related_sides` without repeating routing |
  | `at_call`, `context_sections` | the lines a call adds to a symbol's `contracts:`, and the sections `context` adds after the callees (packages: `consumers:`) |
  | `explain_inferred` | whether a symbol's `context` explains a call consumer's inferred link, given its repository, link and prepared exports (HTTP: a declared `base_url_env` peer or resolved infrastructure/workload); false by default |
  | `context_reads` | the call sites `context_sections` looks up, which `context` reads first (packages: the consumers of the links into the symbol) |
  | `starts` | the providers a `trace` start text names |
  | `calls_from_definition` | whether a consumer at a symbol (a Feign or Retrofit interface method) calls on its provider, so `trace` follows from the method to it (HTTP; a channel handler is where a link ends) |
  | `flows_to_consumer` | whether data travels provider to consumer (resource writes to reads, channel producers to handlers), including consumers at call sites |
  | `status`, `scope_status` | the lines `meada status` prints per repository, optionally considering the visible workspace |
  | `workspace_status` | optional per-query status rows in repository order, with `None` preserving the ordinary status hooks and call order |
  | `decidable`, `consumer_item`, `link_candidate`, `serves`, `candidate` | whether a consumer ambiguity remains an open item, whether a provider remains a current link candidate, alias matching and candidates for a consumer with no contract |

  The engine fixes the phase order: each kind's `read_declarations` then `declare`, followed by every `workspace_step`, every `workspace_ready`, then `link`,
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
  `modules`, and the builder form (`LanguagePack::fluent`): [map/languages.md](../languages.md).
- New **package ecosystem** (spec #22 user story 26, spec #225; track B packs): implement the
  `Ecosystem` trait of `package.rs` in a module of its own under `package/` (`npm`, `pypi`,
  `cargo`, `go`, `maven`, `nuget`, `swiftpm` today) and add it to `ECOSYSTEMS`; the package kind,
  the index writer and the packs learn no name. An arm for one ecosystem added to the shared code
  is a finding. The questions and their defaults: [map/seams.md](../seams.md).
- Entries per library and shared reader, in [map/seams.md](../seams.md): `dotnet/` (what the
  .NET pack and ecosystems share, as `jvm/` does for the JVM), a Go router (`ROUTERS` in
  `http/go.rs`), a Go HTTP client (`http/go/clients.rs`), the Go, Java, Kotlin and C# channels
  (`channel/<language>.rs`) and shapes through package imports (`Code::imported`). Go environment
  reads are in [map/languages.md](../languages.md).
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
- **Configuration and IaC source locations** use `document::events::read_located` (JSON alone:
  `json_spanned`) when a scalar's location is insufficient. The shared reader lends completed
  collections beside their key/end lines before a repeated property replaces one. Format
  interpretation stays with the caller (`iac/json.rs`); retained location or marker metadata
  carries paths and lines, with its budget charged before copying, never values.
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
- New **match rule** (a path rewrite, a new provider source): the kind's `link`, `link_candidate`,
  `serves` and `candidate` and each candidate's contract must agree. Item listing and ordinary
  replay share `consumer_item` eligibility and the query-aware `link_candidate` rule.
  `meada decide` writes the candidate's contract as the decision's target and
  `decisions::evaluate` checks it against the link's candidates, so a candidate carrying the
  consumer's contract instead of the provider's own makes every decision on it stale.
- New **entry point kind** (spec #31): a variant of `entry::EntryKind` (its `name` and `parse`
  arms and the list `parse` searches, and its name in the `processes` kinds of the export JSON
  in [Identities and formats](identities-and-formats.md), which `tests/it/docs.rs` checks).
  A door that is a side of a contract needs no finder:
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
