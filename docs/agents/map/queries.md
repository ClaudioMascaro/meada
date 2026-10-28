# Queries and rendering

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams, invariants and Known ceilings. The code wins over this file, and a change to these
modules updates it.

- `engine.rs`: `Engine` is the one query seam. The CLI, the MCP server and almost every test call
  it: `open`, `refresh`, `export`, `search`, `context`, `trace`, `open_items`, `local_items`,
  `local_item`, `decision_report`, `local_decisions`, `root`, plus `status(dir)` (never creates
  an index; when every repository is already indexed at the current schema it refreshes them
  like a query, to evaluate decisions, and writes
  only inside `.meada/`; otherwise, or when that fails, it prints the states and no decision
  lines). It builds the results of `context` and `search` (each line placed: a `path:line` reads
  the file) and `meada-core` turns them into the answer: `search::answer` sorts and cuts the hits,
  `context::Symbol::answer` cuts a symbol's context; `trace` lives in `trace.rs`. `export` writes
  through `meada_core::export` (`index::Export::written`, then `Export::write`). It imports no kind
  module: where a rule belongs to a kind it asks `ContractKind` (through the seam's free
  functions or `KINDS`), in the phase order `Engine::exports` and `core_links` fix (see
  "New contract kind" in the architecture map). The valid `kind` words of `search` are derived
  from the kinds' `search_word`, and a test asserts that the MCP `search` doc string names each
  of them. `search`
  kind `resource` is the contracts under `resource::` (matched by kind and id), the same lines
  `contract` and no kind list; `context` on `resource::<kind>::<id>` lists the writers and
  readers across the workspace (`contract_sides`, shared with channels). `search` kind
  `channel` is the contracts under `channel::`, matched by broker kind and name (an unresolved
  side by its broker kind, printed `channel::kafka (unresolved)`); `context` on
  `channel::<broker>::<name>` (or `channel::<broker>`, the unresolved sides of that broker, also
  accepted as `search` prints it, through the seam's `typed`) lists the producers (the symbol
  around the producing call) and consumers (the handler) across the workspace, those linked
  through another name included with `[<level>: <reason>]` after them: the producers a
  wildcard consumer matches (`channel::fits`, private to the kind), the wildcard consumers that match the channel,
  and the publishes its bindings route to a queue (`Channel::related_sides`, through
  `Routes::reached`). `context` on a symbol adds
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
  follows the format, so OpenAPI lines never gain a `channels: 0`). `search` kind `package`
  (`Packages::own_search`, an early branch too) lists the `packages` rows whose name matches, as
  `<repository> <directory> <name>@<version> (consumers: <n>)` (no `@<version>` for a manifest
  without one; a package of another ecosystem than npm's adds it after the name, as in
  `shared-lib@1.2.0 (pypi)`, and so does its `packages` row, as `ecosystem`); the consumers are the links whose provider is the package's manifest
  (`Package::site`), that is the dependency entries at any level, not the call hops, which link to
  symbols. A dependency entry's site (`<repository>::<path>::<field>::<name>`) and a manifest's
  print as `package.json dependencies` and `kit/package.json` in `search` and in `context` on
  `package::npm::<name>` (`Packages::place`); that `context` goes through `contract_sides` with the
  roles `package` and `dependent`, and `trace` on it points to `context`. `context` on a symbol
  adds a `consumers:` section after `callees:`, only when non-empty: the calls that reach it
  through a package import (`Packages::context_sections`, through `reached`, from the links whose provider is the symbol and
  whose contract starts with `package::`), from any repository, as `<repository> <path>:<line>
  <caller>` then ` [<level>, <flag>]` when the link is not `exact` or the dependency's range does
  not plainly accept the provider's version (`flag` on the entry's range and the version
  of the manifest the hop went through; the reason is never parsed). A `workspace:` range is never
  plain: a consumer through one always shows `range unchecked`, even inside its own repository,
  which `pkg-mono.context-add.txt` pins (`b` through `workspace:*`; `c`, ambiguous with `fork`).
- `drift_view.rs` and `meada-core`'s `drift.rs`: shape drift (spec #23, ADR 0009) as `context`,
  `search` and `status` show it, with no drift computed of its own: `entries` (binary) takes
  every link with a `Link::drift` or, but for an ambiguous one, whose consumer has a shape in the
  exports (`drift::shown_link`), and finds its consumer call by scanning every repository's calls
  (Known ceilings); `consumers`, `matches` and `counts` (core) render. `Drift` and
  `shape::drift` live in `meada-core`'s `shape.rs`. `search` kind `drift` ranks against the route path the seam's `parts` gives (its
  second part). `context` on a contract its kind lists by its providers alone (`View::consumer`
  is `None`, as an HTTP route) appends, after the providers, one
  `<repository> consumer <path>:<line> <caller>` per linked consumer that read fields of the
  carried shape (ambiguous links apart), sorted by repository, file and numeric line, with `drift:
  missing <path> (<level>), ...` and `not checked: <path> under <open path>, ...` lines when it
  has any. The provider's shape lines name the source: `<file> (document)`, `(zod)` or
  `(literals)` (`Source::at`). `search` kind `drift`
  (`search_drift`, an early branch) gives one line per missing path, `<repository>
  <path>:<line> <caller> <contract> missing <path> (<level>)`, matched by the route path or the
  field path; the reads not checked are never listed. `status` prints `Msg::Drift(n)` under a
  repository whose consumers have `n` > 0 missing reads, link and path pairs
  (`Engine::status_report`), nothing
  otherwise, so no repository without drift changes.
- `items.rs`: open items (`OpenItem`, `Candidate`, `ItemKind`, re-exported by `engine`) and `list`,
  the one place that derives them. `list` takes the exports and the core's links (before any
  decision), so an item is an ambiguous link or an unresolved consumer, in id order, whose
  candidates sort by repository, method, path, as if undecided. The routes a dynamic URL may
  reach come from each provider's kind (`ContractKind::candidate`; for HTTP, `route_fits`: method,
  or a route of every method, and ordered literal fragments, see `Found::method` /
  `fragments`). `Candidate::describe` is the one candidate line (repository identity, method,
  path) shared by the prompt and the excerpt. `Engine::items` pairs each item with whether a
  decision applies (its call has a `Level::Decided` link after replay); `open_items` and
  `local_items` drop those, so `search` with kind `ambiguous` or `unresolved` and `meada
  decide` never offer a decided call, while `local_item(id)` keeps it, for `--item` to redo.
  `list` skips the ambiguous links of a kind that says `decidable` is false (packages): several
  providers of a package name are no call for a person to decide. Decision replay asks the
  seam's `serves` whether a provider serves a decided contract.
- Communities and processes in `search` and `context` (spec #31): `search` kind `community`
  (`search_communities`, an early branch) lists `<repository> <name> (members: <n>)`, matched by
  name; kind `process` (`search_processes`) lists `<repository> <label> process::<identity>
  (<kind>)`, matched by the entry point's label, the identity being what `trace` starts from.
  `context` on a symbol opens with `<repository> <path>:<start>-<end> <name>` (`<path>:<start>`
  when the span text is unavailable, `<path>` when the file cannot be read), and puts two lines
  right after it, so the budget cuts the tail and not them: `community: <name>` (or `none`, read
  from `Export::communities`) and `processes: <summary>: <label> (<repository>), ...` (the
  summary below, or `none`), sorted by entry point identity.
  `Graph::processes_of` finds them by walking upstream, inverting `Graph::edges` over every
  node, so membership is the reachability the process walk follows. `processes_line`
  (`meada-core`'s `context.rs`) opens
  with a summary whenever the symbol belongs to a process, `<n> (<k> route, <k> export; <r>
  repositories)` (kinds in `EntryKind::ALL` order, only those with a count), then the entries
  that fit its room, then `+<m> more` for the rest; when not even one fits, the summary is the
  line. The room is what the budget leaves after the header and community, less the note of
  the final cut. `context::Symbol::answer` cuts in this order: the span text first (`render::cut`, with
  its own note; left out when not even the note fits, so the agent reads it from the lines the
  header's range names), then the tail (callers, callees, annotations, consumers, contracts) by
  the usual cut (`annotations:` lists, only when non-empty, the symbol's calls with `CallSite::decorates`
  set, each as its callee line ending in ` on <decorated definition>`, apart from the callees); the header, community and processes lines survive it. An answer that fits
  names every process and is cut nowhere. Two processes of one repository
  with the same label show as their `process::<identity>`.
- `trace.rs` process walk (spec #31): `Graph::process(entry)` is the one walk (`Descent`, its own
  variant beside `Walk`, which `trace` down keeps as is) and returns `Vec<Step>`: depth, the
  repository, how it was reached (`Via`: the entry, a call, a link), the level, the node and a
  `Mark` (in `meada-core`'s `export.rs`: `cycle`, `seen`, `ambiguous`, `unresolved`,
  `no-provider`, `produces` or `consumes`
  for an unresolved channel side, `depth-limit`; `tests/it/docs.rs` checks every variant is here).
  `Walk::show` renders a step in the chain's line grammar (`trace` from `process::<identity>`,
  found in `Graph::entry_points`: first line `process <label>`, then the steps, then the
  footer), and `Graph::export_process` turns the same steps into `Export::processes`
  (`meada_core::export::Process`).
  Children of a step are `Graph::edges` (calls in source order, then identity, then what each
  call reaches); an ambiguous call or link is one step listing its candidates; a symbol is expanded at its first
  occurrence (`(see above)` after), a step already on the branch is `(cycle)`. The entry
  points themselves are in `entry.rs`.
- `trace.rs`: `Graph` over all exports and links, depth-first `Walk`; `meada-core`'s `trace.rs`
  holds the result (`Row`, `Chain`) and its answer (`Chain::answer`: the rows that fit, the cut
  note, the chain footer, the missing peers), with `level_mark` and `arrow`. A link
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
  row, pushes them and enters anything else. A line reached through a link with drift ends
  with ` drift: missing <paths>` (comma-separated, after any `(cycle)` or `(see above)`):
  `Row::link` (core) reads `Link::drift` and `Walk::push` appends it. Starts take only HTTP routes, symbols and call
  sites; a start in a kind with `ContractKind::sides` (`resource::...`, `channel::...`) gets an
  answer pointing to `context`. A package hop is a call-site consumer linked to a symbol, like an
  HTTP call: `call_down` crosses it from the call into the provider's function and on through its
  callees, and `trace` up reaches the call through `into[symbol]`; a hop into another repository
  counts in the footer's hops and repositories. A consumer at a symbol of a kind with
  `ContractKind::calls_from_definition` (HTTP: a Feign or Retrofit interface method, #90) is
  followed like a call: `Walk::consumers_down` crosses it from the method into the provider (down
  from the callers, after the callees), and up the provider reaches the method and its callers;
  the `calls_on_link` filters keep a reader-style crossing from looping back.
- `meada-core`'s `render.rs`: token budget (`budgeted`, `cut`, `tokens`; 1 token = 4 bytes;
  `DEFAULT_BUDGET` 2000) and the line helpers `label`, `callee`, `unresolved`, `mark` (` [<level>:
  <reason>]` after a call that is not exact, in `trace` and in `context`'s callers and callees;
  the reason is derived when rendering, never stored: `one method named render`, `2 functions or
  classes named render`), plus the markers `REPOSITORY_LEVEL` and `DOCUMENTED_ONLY`. Display
  only. The binary's `render.rs` re-exports them beside `Files` and `source`, which read a file.
- `mcp.rs`: three tools (`search`, `context`, `trace`) over rmcp, each a thin call to `Engine`
  that reopens it per call. `Meada::source` says where: over stdio (`serve`), on the client's
  first `file://` root (`roots/list`, asked per call when the client declared the capability),
  or on the cwd without one, refreshing; in server mode (`listen`), `Engine::open_listed` with
  the endpoint's identity first, read-only, with no request to the client. `listen` checks every
  repository first and keeps the manifest it read (`engine::served`); a call checks again only
  the named repository's mirror (`Repository::recheck`), and the indexes as it opens them.
  It binds a hyper HTTP/1 listener and routes `/<identity>` (percent-decoded, a trailing slash
  tolerated) to that repository's rmcp `StreamableHttpService`: stateless (no
  session), JSON responses, no `Host` check (a reverse proxy forwards its own name) and any
  `Origin` refused with 403 before routing, on every path (a browser, as on a DNS rebinding
  page, `browser`); other paths get a `text/plain` 404
  with one identity per line. `trace` from `uncommitted` is refused there, and `context` on a
  span whose file moved says `index behind the mirror: run meada index --workspace` instead of
  refreshing. Stdout is the protocol over stdio: log to stderr. No tool writes.
- `main.rs`: clap, with `index`, `status`, `export`, `mcp`, `decide` (`--item`, `--dry-run`,
  `--list`, `--prune`, the last excluding the first three; `--decider`, and `--show-request`,
  and `--min-p`, which require `--decider`; `mcp --listen` and `--workspace`, which require
  each other). `search`, `context` and `trace`
  are reachable through MCP and the library, not the CLI.
- `msg.rs`: the CLI message catalog (pt-BR and English, one arm per `Msg`). The messages of the
  checks `meada-core` holds are `meada_core::msg`'s (a few arms delegate to it), and so is the
  language flag: `msg::init` sets it for both. Only `main.rs` calls
  `msg::init`; `meada mcp` forces English, so the MCP surface and library tests are English.
