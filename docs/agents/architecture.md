# Architecture map

Which module owns what, the seams between them, and what the code relies on. Verified against
`src/` and `tests/` of the v0.1 trunk; the code wins over this file, and a change to a seam
updates it. Terms are `GLOSSARY.md`'s; decisions are in `docs/adr/`.

## Flow

`Engine::open(dir)` finds the **workspace**, creates or recreates each **repository**'s **index**
in its own `.meada/index.db`, and ATTACHes every **peer** to the first repository's connection
as `peer<n>`. Every query (`export`, `search`, `context`, `trace`) calls `refresh()` first, then
reads each index through `index::export`, derives **links** from the stored contracts, and renders.
Nothing outside `.meada/` is written. Everything the engine returns is sorted: the same files
give byte-identical output.

## Modules (`src/`)

- `engine.rs`: `Engine` is the one query seam. The CLI, the MCP server and almost every test call
  it: `open`, `refresh`, `export`, `search`, `context`, `trace`, `open_items`, `local_items`,
  `local_item`, `decision_report`, `local_decisions`, `root`, plus `status(dir)` (never creates
  an index; when every repository is already indexed at the current schema it refreshes them
  like a query, to evaluate decisions, and writes
  only inside `.meada/`; otherwise, or when that fails, it prints the states and no decision
  lines). It owns the `context` and `search` rendering; `trace` lives in `trace.rs`.
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
  `meada.decisions.toml` is skipped). Peers not found go to `Workspace::missing`. `config`
  parses the v0.1 subset (`[[peer]]`, `[serve] hosts`) with `deny_unknown_fields`; keys track A
  adds are listed in `LATER` so the error says "not in v0.1". `config::read<T>(root, name)` is the
  shared TOML reader (file, line and key in every error), also used for the decisions file;
  `Repository` carries both `config` and `decisions`, loaded together at discovery.
- `decisions.rs`: `meada.decisions.toml` (ADR 0006). `Decisions { items: Vec<Decision> }`,
  `Decision { kind (ambiguous-link | dynamic-url), call, target, by, p }`, `load(root)` and
  `replay(links, workspace, exports) -> (links, Vec<Evaluation>)`, called by
  `Engine::links_and_decisions` at the end. Replay is a pure function of links, exports and the
  files, so nothing is stored and a changed file needs no reindex. `evaluate` is the one place
  that decides a decision's `Fate`, in this order: call gone (`Cause::CallGone`), target repository a
  missing peer (`Waiting`, neither applied nor stale, whatever else holds), item no longer open
  (the core resolves the call on its own, or the item is not of the decision's kind:
  `ItemNotOpen`), target gone (`TargetGone`), target not among the ambiguous link's candidates
  (`NotCandidate`); else it applies. A stale or waiting decision changes no link. `Evaluation
  { repository, stale: Vec<Stale {call, target, cause}>, waiting: Vec<Waiting {call, target,
  peer}> }` is per repository, in file order; `Engine::decision_report()` returns them for the
  workspace (`Engine::local_decisions()` the one of the repository where Meada runs), `export`
  and `status` render them, and `Evaluation::lines` is the one rendering of the stale and
  waiting lines (`status`, `meada decide --list`); `--prune` reuses the same call.
  `probability(p)` is the one [0,1] check, for the file's `p`, a decider's answer and
  `--min-p`. `load` rejects a file that repeats a `call` (parse error `<file>:<line>: `call`: ...` at the repeat): one entry per call is a file invariant, so replay and the writers never see duplicates. The writer is `canonical(root, new)` (existing entries minus those of the same
  `call`, plus `new`, sorted by `call`, no comments or timestamps; `Decision::human(item,
  candidate)` builds a human pick) and `save(root, text)`.
- `repo.rs`: root, identity (normalized `origin`, else `local/<dir>`) and `uncommitted()`, all by
  shelling out to `git`.
- `index.rs`: SQLite schema, `refresh`, `export` and the export types. Freshness is a pipeline in
  `refresh`: stat (mtime/size, trusted only if older than `RACY` = 2 s), hash with BLAKE3, parse
  only changed files, resolve every file's calls in memory, rewrite rows only when a file's
  `resolved` digest changed, re-extract contracts for the whole repository.
- `lang/mod.rs`: the **language adapter**, `LanguagePack` (extensions, `parse` to `Facts`,
  `config_files`, `modules`), and the `Facts`/`Value` types, plus the tree-sitter helpers
  (`named`, `children`, `text`) every pack shares. `PACKS` lists `lang/ts.rs` (`.ts .tsx .js
  .jsx .mjs .cjs`, reads `tsconfig.json`) and `lang/python.rs` (`.py`, reads `pyproject.toml`:
  `.pyi` and notebooks are not read). A pack sees one file at a time and never touches the index.
  `python.rs` yields `Kind::Method` for a function directly in a class body, keeps the first of
  two definitions of one name, spans a decorated definition with its decorators, and exports
  every module-level name (imports included), as an explicit import or `m.x` sees them; what
  `import *` takes is `Facts::star`: the public names, or a literal `__all__` alone.
  Its imports are the module as written (`.`, `..pkg`, `a.b`): `import a.b.c` binds `a`, `a.b`
  and `a.b.c` (dotted `Import::local`s); `from p import m` binds `m` twice, as a name `p`
  exports and as the submodule `p.m` (the last that resolves wins); `from p import *` is
  `Imported::All`. `modules` resolves them against the repository root, then the roots
  `pyproject.toml` declares (setuptools `package-dir` and `packages.find.where`, Poetry
  `packages.from`), or `src` when none is declared; a package comes before a module of the
  same name.
- `resolve.rs`: the shared resolution behind the adapter. `Resolver` connects calls to definitions (scope,
  imports, re-exports, `this.method`, member chains) and gives each a `Resolution`. A name may be
  bound by several imports (the last that resolves wins), a callee may start with a dotted
  import, and a name no import binds is looked up in what the `Facts::star` of each
  `Imported::All` import takes, last first. `Code` folds constants and client wrapper arguments into values for contract kinds.
- `contract.rs`: `ContractKind` trait (`prefix`, `extract`, `link`), `KINDS`, `Level`, `Link`,
  `Names`. `http.rs` is the only kind: `fetch` consumers, Next.js app-router and Express
  providers, `normalize_path`, and the link rules (host, `base_url_env`, unique provider).
- `trace.rs`: `Graph` over all exports and links, depth-first `Walk`, chain footer.
- `render.rs`: token budget (`budgeted`, `cut`, `tokens`; 1 token = 4 bytes; `DEFAULT_BUDGET`
  2000) and the line helpers `Files`, `label`, `callee`, `unresolved`. Display only.
- `msg.rs`: the CLI message catalog (pt-BR and English, one arm per `Msg`). Only `main.rs` calls
  `msg::init`; `meada mcp` forces English, so the MCP surface and library tests are English.

## Identities and formats

- Repository identity: `github.com/acme/web` (lower-cased host and path, no scheme, user, port
  or `.git`) or `local/<dir>`.
- Symbol: `<repository>::<path>::<qualified name>` (`Class.method`, `outer.inner`).
- Call site: `<caller>::<normalized callee>#<ordinal>`; a top-level call's caller is
  `<repository>::<path>`. The normalized callee keeps identifier and member chains only; any
  other expression becomes `(<node kind>)`.
- Inline route handlers: a pack emits anonymous function arguments as `Kind::Inline` named
  `@<byte offset>`; `Names` renames the ones a kind claims to `<METHOD> <full path>` (`#<n>` from
  the second equal name) and folds the rest into the enclosing symbol.
- Contract: `<prefix>::...`, `http::GET::/users/{}`. A path parameter of any spelling is `{}`.
- Export JSON: `{repository, files, symbols, calls, providers, consumers, links}`. An unresolved
  consumer's method and URL fragments live in the index (`contracts.method`, `contracts.fragments`)
  and are left out of the export. A call has
  `level` (`exact`, `inferred`, `ambiguous` or null) and `target`; an ambiguous call has
  `target: null` and a sorted `candidates`. `export` keeps only the links that touch the
  repository where Meada runs. A repository with stale decisions also has `stale_decisions:
  [{call, target, cause}]` (`call-gone`, `item-not-open`, `target-gone`, `not-candidate`), absent
  when empty; decisions waiting on a missing peer are not exported. A `decided` link also has `by` (`human` or a model id) and, for
  a model, `p`; its reason is `decided by <by> (p=<p>)` (two decimals) or `decided by human`.
- Decision `target`: `<provider repository identity>::<contract>`, such as
  `example.com/chain/gateway::http::GET::/api/health`; split at the first `::`. The target names
  no site: if the repository serves the contract from several, the link goes to the first by site.
  Decision `call` is a consumer's call-site identity.

## Seams and what extends them

- New **contract kind**: implement `ContractKind`, add it to `KINDS`. `extract` reads `Code`
  (never the syntax tree) and returns `Found` with a `Site` (`Def`, `Inline`, `File`, `Call`);
  `link` receives the whole workspace's providers. A kind's rules stay in its own file.
- New **level**: add the variant to `Level` in `contract.rs` and keep the order strongest first,
  since `Ord` makes a chain's weakest link the max. `Level::name`/`parse` are the only place names
  live; `engine.rs` and `trace.rs` render through them.
- New **language pack**: add it to `PACKS` in `lang/mod.rs`, a fixture
  `tests/fixtures/conformance-<language>/` and one `conform(...)` test in `tests/conformance.rs`.
  A pack is done when that suite passes.
- New **config key**: add the field to `Config`, delete it from `LATER`. Parse errors name file,
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
  method of an unresolved consumer (`contracts.fragments`, `contracts.method`). `Call::args`, `Param::default` and `Const::value`
  are `serde(skip)`, so unchanged files come back from SQLite without them. `Code` records every
  file it needs but lacks as a miss, and `refresh` re-parses those and extracts again until none
  is missed. A contract kind that reads a value from a new place relies on this loop.
- Links are never stored. They derive from the stored provider and consumer contracts, then the
  decisions replay, when a query runs, so a change on either side, or to a decisions file, needs
  no relinking. `Level` order: `exact` < `inferred` < `decided` < `ambiguous` (`declared` joins
  with spec #19).
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
  the three-repository chain as siblings. `git()` pins config, author and dates; `age()` pushes
  mtimes into the past so a later edit always changes them. `decisions(repo, name)` copies
  `tests/fixtures/decisions/<name>.toml` to `<repo>/meada.decisions.toml`; the base chain fixture
  has none.
- **Golden files** live in `tests/golden/<fixture>.<what>.{txt,json}` and are compared with
  `assert_golden(name, actual)`. `MEADA_BLESS=1 cargo test` rewrites them; the diff is the
  review. `.gitattributes` marks fixtures and goldens `-text`, since BLAKE3 hashes and byte
  ranges are in them.
- Test files by subject: `conformance` (pack), `index` (export, schema rebuild, writes only in
  `.meada/`), `query` (search/context), `workspace` (peers, status, config errors), `urls`
  (constants, wrappers, Express, inferred links), `trace`, `decisions` (replay, parse
  errors), `stale` (stale and waiting decisions in `status` and `export`), `decide` (the binary
  in human mode over scripted stdin), `decider` (decider mode against the stub), `freshness`
  (cold vs refreshed), `items` (open items and their `search` goldens, fixtures `open-few` and
  `open-generated`), `cli` (the binary under pt_BR, en_US and de_DE
  `LANG`), `mcp` (the binary over stdio).
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
- `resolve.rs`: wrapper expansion walks every simple path up the callers; a parameter captured by
  a closure is unknown.
- `lang/ts.rs`: tsconfig `extends` and nested configs are not followed. `require` and
  `module.exports` are not imports, and `new X()` is not a call (no `ponytail:` marker).
- `lang/python.rs`: only the root `pyproject.toml` is read, and only the setuptools and Poetry
  root declarations. `src` is a source root when nothing is declared, whether or not it holds
  packages (a pack sees no file list). An import binds its names for the whole file, wherever
  it sits. `self`, `cls`, base classes and typed receivers are not resolved yet (a method call
  resolves by name), `__all__` is read only as one literal list or tuple, and call arguments
  and parameter defaults are not read (no `Value`s, so no contracts).
- `http.rs`: an Express app or router passed as a parameter, `router.route(path)` chains and
  `require` are not followed.
- `repo.rs`: a path git still quotes (tab, newline or quote in the name) is skipped by
  `uncommitted()`.
- No `busy_timeout`: two concurrent refreshes of one repository may hit `SQLITE_BUSY` (no marker).
