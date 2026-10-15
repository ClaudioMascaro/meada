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
  readers across the workspace (`resource_sides`). `status` also prints, under each
  repository, `Engine::resource_report` (resource lines, same indexed-only rule as decisions).
- `items.rs`: open items (`OpenItem`, `Candidate`, `ItemKind`, re-exported by `engine`) and `list`,
  the one place that derives them. `list` takes the exports and the core's links (before any
  decision), so an item is an ambiguous link or an unresolved consumer, in id order, whose
  candidates sort by repository, method, path, as if undecided. The routes a dynamic URL may
  reach come from `http::route_fits` (method, or a route of every method, and ordered literal
  fragments, see `Found::method` / `fragments`). `Candidate::describe` is the one candidate line (repository identity, method,
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
  by `gateway_key`) and `[[resource]]` (`kind` checked as a lowercase identifier, `id` not empty,
  `role` read, write or read-write, optional `via`) with `deny_unknown_fields`; keys later track A
  specs add are listed in `LATER` so the error says "not in v0.1". `Serve::gateway_path(code path)
  -> Option<(gateway path, reason)>` is the one gateway rule (spec #19), written for A4's document
  routes to reuse. `config::read<T>(root, name)` is the shared TOML reader (file, line and key in
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
  `resolved` digest changed, re-extract contracts for the whole repository.
- `lang/mod.rs`: the **language adapter**, `LanguagePack` (extensions, `parse` to `Facts`,
  `config_files`, `modules`, `receiver`: a keyword naming the enclosing class's instance,
  `this` for TS/JS, none for Python, `base_receiver`: a keyword naming its base, whose members
  are not resolved, `super` for TS/JS, none for Python, whose `super()` is a call, and
  `globals`: names bound everywhere that contract kinds read, `fetch` for TS/JS, none for
  Python), and the `Facts`/`Value` types, plus the tree-sitter helpers
  (`named`, `children`, `text`) every pack shares. `PACKS` lists `lang/ts.rs` (`.ts .tsx .js
  .jsx .mjs .cjs`, reads `tsconfig.json`) and `lang/python.rs` (`.py`, reads `pyproject.toml`:
  `.pyi` and notebooks are not read). A pack sees one file at a time and never touches the index.
  `python.rs` yields `Kind::Method` for a function directly in a class body, keeps the first of
  two definitions of one name, spans a decorated definition with its decorators, and exports
  every module-level name (imports included), as an explicit import or `m.x` sees them; what
  `import *` takes is `Facts::star`: the public names, or a literal `__all__` alone.
  Its imports are the module as written (`.`, `..pkg`, `a.b`): `import a.b.c` binds `a`, `a.b`
  and `a.b.c` (dotted `Import::local`s); `from p import m` is an `Imported::Member`, a name
  `p` exports or else the submodule `p.m`; `from p import *` is `Imported::All`. `modules` resolves them against the repository root, then the roots
  `pyproject.toml` declares (setuptools `package-dir` and `packages.find.where`, Poetry
  `packages.from`), or `src` when none is declared; a package comes before a module of the
  same name.
  It also yields the facts receivers resolve through: `Def::bases` (a class's bases as
  written, `B[T]` read as `B`, any other expression `(<kind>)`) and `Facts::locals`, one
  `Local {scope, name, class}` per name a scope binds: a method's first parameter unless
  `@staticmethod` (`Class::Enclosing`), an annotated parameter or name, `x = C(...)`
  (`Class::Named("C")`, whatever `C` turns out to be), `self.x: T` in a method (an attribute,
  at the class's scope), and `Class::Unknown`, which hides an outer binding, for any other
  binding: other assignments, `for` and comprehension targets, `with` and `except ... as`,
  `:=`, `case` captures, a lambda's parameters (in the enclosing scope, which owns its calls)
  and a parameter the body assigns. A name its scope binds only once, by `name = value`, is
  also a `Const` holding that value; so is the name of `with C(...) as x` (also
  `Class::Named("C")`, as `x = C(...)` is) and a `self.x = C(...)` attribute (a `Const` of the
  class, only for a call). `xs += ...` and `xs = xs + ...` on a name whose latest
  value in that scope is a list literal do not bind it again: a list literal added joins the
  `Const`'s items, anything else added (`+= static(...)`) is left out. Calls carry their positional `args` and keyword `kwargs`
  as `Value`s (strings, f-strings and `+` folded into parts, a name or a member chain in them,
  `config.BASE`, a `Part::Ref`; `[...]`/`(...)` a `Value::List`, a call
  a `Value::Call` at its range), a decorator call names the definition it decorates
  (`Call::decorates`), a decorator written as a bare name is in `Def::decorators` (as a
  callee, `http.require_safe`), and a function lists the values it returns (`Def::returns`).
  A function's parameters carry their defaults as written (`Param::default`) and which
  argument binds them (`Param::binds`, a `Binds`): `*args` and `**kwargs` are `Rest` (no single
  argument), the parameters after them or after the bare `*` (which is no parameter) are
  `Keyword`, a method's first one is `Instance` (`Class` for a `@classmethod`, neither for a
  `@staticmethod`), any other `Any`. A target that is `self.x`, also inside unpacking, a `for`
  or an `as`, and also written in a function nested in the method, binds the class's attribute
  `x`. An import in a function or a class is `Class::Imported`: it hides an outer binding there
  and resolves through the file's imports. An annotation decides; other bindings that
  disagree make the class unknown. `global x` sends the function's bindings of `x` to the
  module, `nonlocal x` to the nearest enclosing function that binds `x` (class bodies
  skipped). Annotations are read as written: a name, a string holding one, `Optional[...]`
  and `... | None`.
- `resolve.rs`: the shared resolution behind the adapter. `Resolver` connects calls to definitions (scope,
  imports, re-exports, `this.method`, member chains) and gives each a `Resolution`. A name may be
  bound by several imports (the latest in source order decides, whatever it resolves to), a
  callee may start with a dotted import, and an `Imported::All` import counts as one binding
  of each name its module's `Facts::star` takes. `lexical` skips a class's scope unless the
  lookup starts in it (a class body's names are not in scope for its methods). A member of a
  class, or of a parameter or local whose `Class` resolves to a class of the repository
  (`Binding::Instance`), is looked up through the class's family: the class and the ancestors
  its written bases give, in C3 order (Python's method resolution order), linearized once per
  class in `Resolver::new`. A base that is not a class of the repository is an opaque
  ancestor, named by what the file imports for it (`abc.ABC`) or as written (`object`), so
  classes that share it give C3 one entry; a base that is no name (`mixin()`) is its own
  entry: reaching it before a class that has the member leaves the member unknown, and so
  does a hierarchy C3 rejects or bases in a cycle. The first class that defines the member, or
  binds it as an attribute local, wins, `exact`. A receiver whose class (or whose value's
  callee) is outside the repository is unresolved; any other member call falls back to the
  name match. `Busy` guards locals and families that resolve through themselves. A single
  module-level binding is read from functions as it is; a function that rebinds it through
  `global` makes it unknown. A pack that writes `Def::bases` and `Facts::locals` gets this
  without resolver changes (TypeScript writes none yet; its `this` is the pack's `receiver`
  keyword). `Code` folds constants and client wrapper arguments into values for contract kinds,
  a destructured object parameter reading its names (`Param::fields`) off the literal object the
  caller passes. An argument binds a parameter as its `Param::binds` says: by position or by
  the parameter's name among the keyword arguments, only by name, or not at all (`Rest`); a
  method called on an instance (`x.m(a)`) passes nothing for an `Instance` receiver, one called
  on its class (`C.m(obj, a)`) passes it first, and a `Class` receiver is never passed.
  `Code::expand(path, index, read)` hands
  `read` the call's positional and keyword arguments, folded, and walks the wrappers' callers
  with them; a member call found only by a name match (`data.get(...)` matched to a wrapper
  method, `Caller::by_name`) is not climbed, since its receiver may be any object, so a
  wrapper with no other caller is read where it stands. `Code::origins(path, scope, name)` (and
  `value_origins` for a value as written) is the shared value flow: the call sites whose result
  a name may hold, followed through constants, imports and module attributes, the arguments
  every caller passes to a parameter, the values a function of the repository returns (its
  parameters then bound by that call only) and the items of a list or tuple; an attribute
  assigned a call once in its class (`self.client = Client()`) counts as a constant; a call that
  reaches no function of the repository is the origin. A constant, parameter or function met
  again on one path (a cycle) adds nothing. `Code::external(path, scope, callee)` names what an
  import from outside the repository binds a callee to (`fastapi.APIRouter`,
  `express.default`): contract kinds recognize framework constructors with it.
  `Code::definition` resolves a name or a member chain (`views.index`) to a definition,
  `Code::module` a name to the file it binds as a whole module, and `Code::import_file` an
  import specifier (`shop.orders.urls`) to its file.
- `contract.rs`: `ContractKind` trait (`prefix`, `extract`, `declare`, `link`), `KINDS`, `Level`,
  `Link`, `Names`. `http.rs`: `fetch` consumers (also a call to a parameter defaulting to
  `fetch`; only in a pack whose `globals` has it), Python client consumers (`clients`, below),
  Next.js app-router, Express, FastAPI, Flask and Django providers, `normalize_path`, and the link rules (host,
  `base_url_env`, unique provider). A provider that serves every method has the method `ANY`
  (`http::ANY::<path>`): `http::fits(provided, wanted)` matches it to a consumer of any method
  on its path, and its link's reason ends with `; any method`; `route_fits` and a `trace` start
  (`POST /health`) take it the same way. A route also matches by its gateway path: `declare`
  fills `Provider::gateway`, `http::gateway_contract` turns it into a contract and
  `http::serves(provider, contract)` is the one check of either path, through `fits` (also used
  by open items and decision replay, so a gateway candidate carries the provider's own
  contract). That link is
  `inferred` with the rewrite as its reason (`gateway prefix <p>`, `gateway strip <s>` or both);
  within one repository a code path match wins over gateway path matches. An app or router
  (Express `express()`, `Router()`; FastAPI `FastAPI()`, `APIRouter()`; Flask `Flask()`,
  `Blueprint()`) is the call creating it, reached through `Code::origins`: a route counts only
  on a receiver that traces back to its framework's constructor (`Code::external`; Express
  also takes an unimported or local name), so `@app.get` on a Flask app yields nothing. Each
  route is served under every prefix (`prefixes`): the segment of each mount (`use`,
  `include_router`, `mount`, `register_blueprint`; Python's in `Framework::mounts`) under every
  prefix of the router it is mounted on; FastAPI's mount segment is the given prefix (or
  `mount`'s path) then the router's own (`APIRouter(prefix=)`), Flask's given `url_prefix`
  replaces the own one (`Blueprint(url_prefix=)`). What nothing mounts is served at its root
  (`prefixes`' `root`): every Express app or router; a FastAPI or Flask app, unless a mount
  whose child cannot be read (no origin of the child is an app or router of the framework),
  called on another app or router of its framework and able to hold an app (FastAPI's
  `include_router(app.router)` and `mount`, not Flask's `register_blueprint`), may mount it;
  never an `APIRouter` or a `Blueprint`, which serves only where a mount Meada reads puts it,
  so a router mounted through an import that does not resolve is not guessed at the root. A
  prefix or a path with a part that cannot be read (an f-string hole, a parameter) adds no
  route; a path parameter written in the literal (`{id}`, `<int:id>`) stays one. FastAPI method
  decorators and `api_route`, and Flask `route`, are read from `Call::decorates`: the handler
  is the decorated function, the methods `methods=[...]` or `GET`. Django (`django`): what an
  `include()` argument names is an `Include`: a module of the repository (by dotted name or
  as a module), the entries of a list, either of them first in the tuple form `(module or
  list, "app_name")`, a dotted name no module answers to from the source roots (an import
  from outside the repository, or one that does not resolve: `Named`), or `Unknown` (anything
  else, or a value whose origins are not all `path()`/`re_path()` entries). A root URLconf is
  a file with a module-level `urlpatterns` that no `include()` may name: none resolves to it,
  no `Named` is its dotted path or a suffix of it (`may_be`: `backend/shop/urls.py` may be
  `shop.urls`), and no `include()` is `Unknown` (then there is no root). Its `path()` and
  `re_path()` entries (`Code::origins` of `urlpatterns`) are walked top-down, an `include()`
  view adding its route before the entries it holds, at any depth, an `include()` met again
  on the walk (a cycle) adding nothing. Routes join as Django joins them (plain concatenation). A
  `re_path()` pattern is read only when it holds literal text and `(?P<name>...)` groups
  (`regex_route`); any other yields nothing. A function view (`Code::definition`) serves what
  `require_http_methods([...])`, `require_GET`, `require_POST` and `require_safe` allow (from
  `Call::decorates` and `Def::decorators`, all of them when several restrict it), else `ANY`;
  `C.as_view()` serves each handler method `C` itself defines (`get`, `post`, ...), each one
  the provider's site. Python clients (`clients`, from `LIBRARIES`: `requests`, `httpx`,
  `aiohttp`): a call is a consumer when `Code::external` names it a module function of a
  library imported by the file (`requests.get`, `httpx.post`, also under an alias), or when it
  is `get`, `post`, ..., `request(method, url)` on a receiver that `Code::origins` traces back
  to a constructor of the library (`Session()`, `Client()`, `AsyncClient()`,
  `ClientSession()`): assigned in the scope, bound by `with`, a module name, a `self`
  attribute, or a parameter the callers pass one to, or a value a function of the repository
  returns. This is wider on purpose than the three binding forms spec #24 names (same scope,
  `with`, a `self` attribute), as the final review accepted: every way `Code::origins` follows
  a value counts (module constants, caller parameters, return values). A `get` on any other
  object is nothing.
  `client_base` reads the constructor's `base_url=` and `based` joins it, with one `/`, in
  front of a relative URL (an absolute one, or one starting with a hole, is left alone);
  different constructors with different bases make the base a hole. URL and method fold like
  any `Value` (`str.format` and `%` do not). `python.rs` folds an environment read into
  `Part::Env`, as `ts.rs` does (`env_read`/`single_env`): `os.environ["X"]`,
  `os.environ.get("X")`, `os.getenv("X")` and the bare `environ`/`getenv`, matched as written
  (not through the file's imports), alone or in an f-string hole, through `or` fallbacks (the
  read on the left, no other variable on the right), a default argument (ignored, unless it
  reads another variable) and string methods (`strip`, `rstrip`, `replace`, ...). A leading
  `Part::Env` is `via = env:X` in `http::locate`, linked through `base_url_env`; Meada never
  reads the value. `Code::expand` follows a function whose URL
  or method depends on its parameters, so each of its call sites is a consumer (an argument
  it leaves out takes the parameter's default). `resource.rs` (spec
  #19): reads no code; `declare` turns each `[[resource]]` `via` symbol the repository has into a
  provider (writer) or consumer (reader) of `resource::<kind>::<id>`, and `link` joins every
  reader to every writer of another repository at `declared`, reason the contract. A `via` that
  names no symbol is dropped. An entry without `via`, or with none matched, makes the repository
  the side: the `site` is the bare repository identity (no `::`), the **repository-level** side.
  `report` gives the status lines per repository: each unmatched `via`, the fallback to repository
  level, and a resource with no link in the workspace (unlinked).
- `trace.rs`: `Graph` over all exports and links, depth-first `Walk`, chain footer. A link
  whose consumer is a symbol (a resource reader) is crossed from provider to consumer going
  down and back going up; a call-site consumer the other way. A repository-level side
  (`Graph::is_repo`) prints as `<repository> (repository-level)` and ends its branch: it is
  pushed as a row, never entered. Starts take only HTTP routes, symbols and call sites; a
  `resource::...` start gets an answer pointing to `context`.
- `render.rs`: token budget (`budgeted`, `cut`, `tokens`; 1 token = 4 bytes; `DEFAULT_BUDGET`
  2000) and the line helpers `Files`, `label`, `callee`, `unresolved`, `mark` (` [<level>: <reason>]`
  after a call that is not exact, in `trace` and in `context`'s callers and callees; the reason
  is derived when rendering, never stored: `one method named render`, `2 functions or classes
  named render`). Display only.
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
- Contract: `<prefix>::...`, `http::GET::/users/{}`, `resource::postgres::orders`. A path
  parameter of any spelling (`:id`, `[id]`, `{id}`, `<id>`, `<int:id>`) is `{}`. A provider of
  every method is `http::ANY::<path>`.
- Export JSON: `{repository, files, symbols, calls, providers, consumers, links}`; a provider has
  `gateway` when `[serve]` rewrites its path. An unresolved consumer's method and URL fragments
  live in the index (`contracts.method`, `contracts.fragments`) and are left out of the export. A
  resource side is a provider or consumer whose `site` is the `via` symbol, or the repository
  identity for a repository-level side (a consumer's `via` is null). A call has `level` (`exact`,
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
  A pack is done when that suite passes. Besides `parse` and `modules`, it says what the
  shared resolution cannot guess: `receiver` and `base_receiver` (keywords for the enclosing
  class and its base), `globals` (names bound everywhere), `Def::bases` (a class's bases, for
  its family), `Facts::locals` (the class each binding gives a name), `Facts::star` (what a
  star import takes) and `Param::binds` (which argument binds each parameter). A pack that
  writes none of them (TypeScript writes no bases or locals) gets name matches where those
  would have resolved.
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
  method of an unresolved consumer (`contracts.fragments`, `contracts.method`). `Call::args`,
  `Call::kwargs`, `Def::returns`, `Param::default`, `Field::default` and `Const::value` are `serde(skip)`, so unchanged files come
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
  level). `loose_chain(case)` adds an unlinked bucket and a `via` naming no symbol to the
  catalog. `git()` pins config, author and dates; `age()` pushes
  mtimes into the past so a later edit always changes them. `decisions(repo, name)` copies
  `tests/fixtures/decisions/<name>.toml` to `<repo>/meada.decisions.toml`; the base chain fixture
  has none.
- **Golden files** live in `tests/golden/<fixture>.<what>.{txt,json}` and are compared with
  `assert_golden(name, actual)`. `MEADA_BLESS=1 cargo test` rewrites them; the diff is the
  review. `.gitattributes` marks fixtures and goldens `-text`, since BLAKE3 hashes and byte
  ranges are in them.
- Test files by subject: `conformance` (pack; for Python also its HTTP providers and consumers,
  over `conformance-python`, `python-mounts` and `python-django`), `xlang` (a TypeScript and a
  Python repository linked both ways: exports, `context`, `trace`), `index` (export, schema rebuild, writes only in
  `.meada/`), `query` (search/context), `workspace` (peers, status, config errors), `gateway`
  (gateway prefixes), `urls` (constants, wrappers, Express, inferred links, routes of every method), `trace`, `decisions`
  (replay, parse errors), `stale` (stale and waiting decisions in `status` and `export`), `decide`
  (the binary in human mode over scripted stdin), `decider` (decider mode against the stub),
  `freshness` (cold vs refreshed), `items` (open items and their `search` goldens, fixtures
  `open-few` and `open-generated`), `cli` (the binary under pt_BR, en_US and de_DE `LANG`), `mcp`
  (the binary over stdio), `resources` (resource `search`, `context`, `trace` start and `status`
  over the chain).
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
  member call found only by a name match, so a wrapper method called on a receiver whose class
  Meada cannot tell is read where it stands; a parameter captured by
  a closure is unknown (`Code::origins` does not follow it either; nor a name its scope binds
  more than once, or an attribute not assigned a call once in its class); a nested or rest pattern in a destructured parameter binds nothing, and a
  spread in the object a caller passes makes the names it may override unknown.
- `lang/ts.rs`: tsconfig `extends` and nested configs are not followed. `require` and
  `module.exports` are not imports, and `new X()` is not a call (no `ponytail:` marker).
- `lang/python.rs`: only the root `pyproject.toml` is read, and only the setuptools and Poetry
  root declarations. `src` is a source root when nothing is declared, whether or not it holds
  packages (a pack sees no file list). An import binds its names for the whole file, wherever
  it sits. A comprehension's targets are bound in the enclosing scope, so they hide a name
  there for the whole scope (the safe side: a name match instead of `exact`). A write to an
  attribute from outside the class (`obj.x = ...` in another function) is not seen, so a
  class-body `x = C()` still types `self.x`. `__all__` is read only as one literal list or tuple. A `**k` splat
  in a call is not read; a `*a` splat is read as one positional argument.
- `http.rs`: `router.route(path)` chains and `require` are not followed. A route path or prefix
  that depends on a parameter is not read through callers (it yields no route). A FastAPI
  mount whose child cannot be read hides every other app of its framework that nothing else
  mounts. Django: one `include()` whose argument cannot be read leaves no root URLconf; what
  an extension of `urlpatterns` adds other than a list literal is left out (marked in
  `lang/python.rs`), and a single `urlpatterns = [...] + other` (no list literal as a whole)
  yields no entry; `django.conf.urls.url`, a view wrapped in a call in the URLconf (`require_POST(view)`), a
  handler method a class-based view inherits and `as_view()` on a class outside the repository
  yield nothing; a `re_path()` pattern is read as written, so a non-raw string with doubled
  backslashes yields nothing. Python clients: a receiver that is the result of a call
  (`Session().get(...)`, `make().get(...)`) or an attribute set from anything but a call
  (`self.http = http`, marked in `lang/python.rs`) is not read; a parameter given a client
  takes the base of every caller that passes one, and several different bases make it a hole.
- `repo.rs`: a path git still quotes (tab, newline or quote in the name) is skipped by
  `uncommitted()`.
- No `busy_timeout`: two concurrent refreshes of one repository may hit `SQLITE_BUSY` (no marker).
