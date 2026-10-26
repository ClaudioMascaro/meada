# Architecture map

Which module owns what, the seams between them, and what the code relies on. Verified against
`src/` and `tests/` of the v0.1 trunk; the code wins over this file, and a change to a seam
updates it. Per-area detail lives in `map/`, one file per area, each named from the module
list below. Terms are `GLOSSARY.md`'s; decisions are in `docs/adr/`.

## Flow

`Engine::open(dir)` finds the **workspace**, creates or recreates each **repository**'s **index**
where `Repository::index` says it lives, and ATTACHes every **peer** to the first repository's
connection as `peer<n>`. Locally the index is the checkout's own `.meada/index.db`. In
**server mode**, `Engine::open_manifest(manifest, repo)` builds the workspace from the
**workspace manifest** instead (`workspace::Manifest`: every listed repository, `repo` first,
then manifest order, nothing discovered; under an OIDC grant only the granted ones) and each
index lives at `<index_dir>/<identity>/index.db`; `engine::index_workspace` and
`engine::probe_workspace` are `meada index --workspace` and `meada status --workspace`.
The status report and readiness share one `Manifest::read_listed`, including a managed version
swap during the informational Valkey probe; `ready` also reuses one listed manifest across
its normal and locked-index checks. An
engine opened from a
manifest opens every index read-only on one connection (`index::open_read`: nothing created,
`trusted_schema` off and `SQLITE_DBCONFIG_DEFENSIVE` on, since a generation's index comes from
a store, one read transaction held until the engine drops, so an answer sees each index as one commit left
it and a commit waits for the answer, both up to a busy timeout) and its `refresh()` does
nothing, so only `meada index --workspace` writes; `open_manifest` is also what `meada export --workspace --repo`
opens. With the snapshot module (feature `snapshot`, `--generations`), `Engine::open_held`
builds the workspace from a generation the node holds instead
(`snapshot::serve::held` reads the pointer file and the generation document, and
`Manifest::of_generation` turns it into the same `Manifest`: per entry the mirror at
`<generation id>/<bundle id>/mirror`, the index beside it, the mirror's own `meada.toml` whole or
else the blocks the document recorded, each read up to `config::GENERATION_CONFIG_MAX`; no
`git` runs, the pull checked each mirror) and opens it read-only as `open_manifest` does, one
engine per call. The server keeps the last generation read (`serve::Current`, one `Arc<Held>`:
a generation's directory never changes once renamed into place), so a request reads only the
pointer file until it names another id, and the request's route hands the `Held` it routed by
to the call (`Source::Generation`): a swap between two calls changes the generation and one
during a call does not, and a call still answering keeps its own `Arc` once the server moved
on. The node's manifest contributes only its module
sections. `engine::served_generation` is its start check and the predicate `status
--generations` exits on (current generation present, every mirror rechecked, every index at the
schema version and holding exactly this build's schema objects, `index::vet`, the whole
workspace opens), `engine::status_generations` its text (the node's
current and held ids, and the store's current id with a short timeout, which never changes the
exit code), and `Engine::open_node` what `export --generations` opens. An index that cannot be
read in a generation names the generation and its fix (`msg::Fix`), not `meada index`: a
publish from this build for another schema version, else removing `<local_dir>/<id>` before
`meada snapshot pull` (a pull of the generation already current changes nothing) or a new
generation. Every query (`export`, `search`, `context`, `trace`) calls `refresh()` first, then
reads each index through `index::export`, adds the contracts each `meada.toml` declares
(`ContractKind::declare`), derives **links** from both, and renders.
Nothing outside the index's directory (`.meada/`, or `index_dir` in server mode) is written.
Server mode also runs on `index_dir` and mirrors mounted read-only, possibly owned by another
user: the open creates no file there (no `index.db.lock`, only a rebuild takes it; no
`.gitignore`; no journal), and a hot journal that only a write could roll back is reported as the
interrupted-commit error naming `meada index --workspace`. The open is SQLite's plain read-only
one, not the `immutable=1` URI. Trade-off: `immutable` skips locking and change detection, so it
would also skip the shared lock that makes a commit in progress wait for readers and readers for
a commit, and it would ignore a hot journal, serving a half-written file; it is safe only for
files that never change in place. A plain read-only open takes a shared `fcntl` lock, which works
on a read-only mount (the test locks directories `0555` and files `0444`) and keeps the busy
timeout and the interrupted error working. `meada index --workspace` refreshes an index of the
current schema in place, in one transaction (only a rebuild renames), so a server reading with
`immutable=1` while it runs would take no shared lock and not see the hot journal, and could
serve half-written pages. `immutable` would be safe only for an index that arrives whole and is
never refreshed in place on that node; that is why the plain read-only open stays.
For a mirror another user owns, `git` refuses it (`safe.directory`); `repo::command` is the one
place Meada runs `git`, and it adds `-c safe.directory=<mirror>` for the paths `Manifest::read`
registered with `repo::trust`: the manifest's mirrors only, in server mode only, never `*`. Git
honours `safe.directory` from the command line only from 2.38 (Ubuntu 22.04's 2.34 and RHEL 8's
2.31 ignore it on purpose), so a mirror owned by another user needs git 2.38 or later, or
`safe.directory` in the system configuration; on an older git the start check fails closed with
an error naming the mirror and that minimum (`Msg::DubiousOwner`).
Server mode reads the **workspace manifest**, and each mirror's `meada.toml` and
`meada.decisions.toml`, once at start (`engine::served`, which refuses to start unless every
repository can be served), so a change to those files needs a restart, while a code change in a
mirror needs only `meada index --workspace`. It refuses `trace` with `changes: uncommitted`, and
`context` prints `index behind the mirror: run meada index --workspace` for a span whose file
changed since indexing.
Everything the engine returns is sorted: the same files give byte-identical output.

## Crates

The repository is a Cargo workspace (ADR 0014): the `meada` binary's crate at the root (`src/`,
`tests/`), and `meada-core/`, the deterministic part the binary and the console's front end
share. Both take one version from the workspace, and `meada` depends on `meada-core` at exactly
it. `meada-core` has no rusqlite, tree-sitter, filesystem, process or network code and no async
runtime; it builds for `wasm32-unknown-unknown`, its `wasm-bindgen` feature (never the binary's)
exporting `init`, `validate`, `workspaceVersionId`, `workspaceDiff`, `entriesText` and
`parseExport`. CI's `core` job builds it
with wasm-pack and checks both trees.

Every tree-sitter grammar is a crate except Kotlin's: `vendor/tree-sitter-kotlin-ng/` holds
`tree-sitter-kotlin-ng` 1.1.0 with upstream PRs #21 and #13 and a local scanner patch
(`PROVENANCE.md`), which the root's `build.rs` compiles and `lang/kotlin.rs` binds (ADR 0017). No
other Kotlin grammar may enter the dependency tree: they all export `tree_sitter_kotlin`.

- `meada-core/src/manifest.rs`: a workspace manifest's `[[repository]]` (`EntryFile`,
  `EntryPeer`) and the `[serve]` and `[[resource]]` shapes `meada.toml` shares (`Serve`,
  `Resource`, `Role`, `declared`, `gateway_path`); `parse` and `error_at`, the errors naming
  the file, the line and the dotted key (`line_of`); `identity_problem` and `entry_problems`
  (an identity listed twice); `validate`, a workspace text's entries or its problems as the CLI
  prints them (a text holding only `[[repository]]` entries). `validate_import(file, src)`
  validates entries while ignoring other top-level keys, with CLI import's file prefix on
  identity errors; CLI, HTTP and WASM import validation share it, preserving empty workspaces and ignored
  bootstrap sections.
- `meada-core/src/ids.rs`: `bundle_id`, `generation_id` (ADR 0012), `workspace_version_id`
  (ADR 0014: the canonical JSON of the entries, `{"blocks","path","repo"}` in manifest order),
  `is_id`, `fence`, `entry_blocks`.
- `meada-core/src/workspace.rs`: `diff(file, before, after)` validates through `validate_import`
  and lists added/removed/changed identities and changed dotted keys in manifest order. Array
  tables retain each field's row position and absence; common entries reordered relative to
  one another report the `repository` array key. Formatting is ignored. `entries_text` is the
  pure JSON-repositories-to-TOML conversion shared with `snapshot::versions` and WASM.
- `meada-core/src/msg.rs`: the messages of those checks and the language flag the binary's
  `msg.rs` reads too; `meada-core/src/http.rs`: `normalize_path` and `PARAM`, which the HTTP
  kind re-exports. `qualify` and `parent` (dotted names) live in its `lib.rs`.
- `meada-core/src/export.rs`: the export's types as `meada export` writes them (`Export` and
  every piece, `Level`, `Link`, `Process` with its `Step`, `Via` and `Mark`, `Stale`, `Cause`,
  `EntryKind`, `Format`, `How`), `Serialize` and `Deserialize`, so a parsed export written again
  (`Export::write`) is the same bytes; `parse` reads one or names the field that does not fit
  (`calls[3].span: missing field ...`), which `parseExport` exports to the browser. The binary
  re-exports the shared pieces where they lived (`index`, `contract`, `package`, `decisions`,
  `community`, `entry`, `document`) and keeps its own `Export`, `Provider`, `Consumer`,
  `Document` and `Package` with what only the build needs, turned into these by
  `index::Export::written`.
- `meada-core/src/shape.rs`: shapes and shape drift (ADR 0009: `Drift`, `{missing, unchecked}`,
  `shape::drift`), which the binary reaches as `crate::shape`. `drift.rs`: how `context`,
  `search` and `status` show a link's drift (`consumers`, `matches`, `counts`, over the `Entry`s
  the binary's `drift_view.rs` places).
- `meada-core/src/render.rs`: the token budget and line helpers every answer shares (`budgeted`,
  `cut`, `note`, `tokens`, `label`, `callee`, `mark`, `rank`, `section`); `search.rs` (`Hit`,
  `answer`), `context.rs` (`Symbol::answer`, its cut order and `processes_line`, `shape_lines`)
  and `trace.rs` (`Row`, `Chain::answer` with the footer): the results of `search`, `context`
  and `trace` and their text answers. The engine walks the indexes and places each line (a
  `path:line` reads the file); these assemble and cut the answer.

## Console frontend (`ui/`)

The static console (ADR 0014) is a separate Vite/React/strict TypeScript build. `npm ci`,
`npm run wasm`, `npm run build`, `npm run check:version` and `npm test` run inside `ui/`, with
Node pinned by `.nvmrc` and wasm-pack by `scripts/wasm.mjs`. Cargo never invokes them. The
WASM bundle builds from `meada-core` into ignored `ui/wasm/`; Vite puts content-hashed scripts,
styles and WASM under `dist/assets/`, with `meada-ui.json` carrying the workspace version.

`Dockerfile` builds the frontend in a pinned build-platform stage. Only its `cloud` target
copies `dist/` to `/usr/share/meada/ui`; the default `runtime` target skips Node entirely.
`scripts/ui-tarball` packages that directory from the cloud image under one versioned root.
The container workflow checks both images, compares the archive's files and bytes with the
image, and attaches it to the published release. cargo-dist's default binary artifacts stay
in their separate release workflow.

`src/api.ts` owns the same-origin `/api/v1/` client; `auth.ts` owns OIDC discovery, PKCE and the
memory-only bearer. Authorization redirects retain the issuer endpoint's query parameters and
replace generated OAuth keys through `URL.searchParams.set`. Only the pending redirect's verifier and state enter sessionStorage, and
both are removed on callback. `main.tsx` handles `/auth/callback` before rendering the shell;
`App.tsx` keeps page navigation in process so changing pages does not discard the bearer.
`pages.tsx` provides semantic navigation and the configuration, session and bootstrap manifest
views, with view models in `models.ts` and navigation copy in `i18n.ts`. `PlaygroundPage.tsx`
sends the MCP-shaped query that `playground.ts` builds, displaying the answer unchanged.
`GraphPage.tsx` fetches an export through `Api.export`, and `graph.ts` checks it with WASM
`parseExport` before deriving communities, contracts, cross-repository links, drift and
applied/stale decisions. View changes are local; explicit generation exports are cached in the
page and a current-generation load reads again. `graph-i18n.ts` holds this area's pt-BR/en copy.
`OperationalPage.tsx` reads the fleet, store, repositories, credentials and admission routes;
`operations.ts` projects their checked DTOs, with bilingual copy in `operations-i18n.ts`.
Missing modules name their sections, errors remain local to the page, and the pasted token
exists only in the uncontrolled field and its request body, cleared on completion.
The API reader preserves large JSON integers as decimal strings instead of rounding fences.
`actions.ts` offers generation operations only to an operator while operations are enabled;
`ActionControls.tsx` displays their CLI commands and the optional fence floor without rounding
u64 values. `ConfirmDialog.tsx` uses a native modal dialog with labelled controls, Escape
cancellation and focus restoration. `OperationalPage.tsx` freezes the confirmed payload,
shows CLI results and refusals unchanged and reloads the fleet after success. A generation
pointer rollback lasts until the next publish, as every set control explains.
`WorkspacePage.tsx` reads workspace versions and their current fence, caches selected details
and compares them with the core's `workspaceDiff`. `workspace.ts` owns the editor projection
and captured save/apply payloads and commands; `workspace-i18n.ts` owns its pt-BR/en copy.
Every edit validates through WASM `validate_import`; JSON entries become TOML through the
same `entries_text` as `workspace show`. Saves require a message, accepted saves read the
immutable first metadata independently of read refresh failures, and older applies are labelled
as rollback for the next refresh. The shared confirmation dialog sends nothing before acceptance.
The frontend container stage copies the one shared validation fixture because TypeScript also
checks its tests; Rust/default stages gain no frontend input.
Vitest exercises the built WASM and the public client, view-model and DOM
seams, without a browser end-to-end suite.

## Deployment profiles

`deploy/compose/compose.yaml` supplies local and generations nodes; the explicit
`compose.console.yaml` overlay adds the cloud console, its initial pull and companion, its own
bootstrap/data volume and its own TLS proxy route. `deploy/helm/meada/` follows the same split:
`console.enabled` is false by default, with a separate ConfigMap, Deployment, Service and
optional Ingress when enabled. Neither profile mounts a node's data into the console. The
console's public assets, config and PKCE callback reach its listener before OIDC admission;
its proxy preserves Host, Origin and Authorization. MCP keeps its separate admission rules.

Managed writers pull the workspace before the external fetch, index and publish, inside one
refresh lock. Seeding and applying the first version are explicit operator actions. The fetch
list/script and mirror provisioning still belong to the operator: applying a new identity or
path does not clone it. `scripts/compose-check` and `scripts/helm-check` check the rendered
configuration, module dependencies, storage, selectors and refresh order.

## Modules (`src/`)

Each line names what a module owns; the file in `map/` it points to has the detail. Read the
one for the area a change touches.

- `engine.rs`: `Engine`, the one query seam (`open`, `refresh`, `export`, `search`, `context`,
  `trace`, open items, decisions, `status`); it builds the results of `search` and `context`,
  which `meada-core` renders. `items.rs`: open items, derived in one place. `trace.rs`: the
  `Graph`, the walk and the process walk (`Graph::process`, the step list `trace` renders and the
  export carries); `meada-core`'s `trace.rs` closes the chain with its footer.
  `entry.rs`: entry points (spec #31), found when an index is written.
  `drift_view.rs`: places the links `context`, `search` and `status` show with their drift
  (`entries`); it reads no kind (the stored links, the exports' consumer shapes, the calls and the
  seam's `parts`), and `meada-core`'s `drift.rs` turns them into lines.
  `render.rs`: the source files an answer reads (`Files`, `source`), re-exporting
  `meada-core`'s budget and line helpers. `mcp.rs`: three read-only tools, one definition over
  two transports: stdio, about the client's first `file://` root or the server's cwd without
  roots (also for an answer with no local root; a client that fails or does not answer
  `roots/list` within `ROOTS_TIMEOUT`, 5 s, gets the last root it sent, else a tool error),
  and in server mode streamable HTTP (`meada mcp --listen`,
  stateless, JSON responses on a hyper listener), one endpoint per repository of the manifest at `/<identity>`, a 404
  listing the identities served elsewhere (with `--generations`, `listen_generations` makes
  the endpoint per request from the current generation's identities, so a pull that adds a
  repository needs no restart, and a node with no readable generation answers 503 with the fixed body `no servable generation`, the problem on stderr; no `Host` check). A request carrying `Origin` gets 403
  before anything else, on every path and in every build (`mcp::browser`, called first in
  `route`): no listing, pin 404 or 503 reaches a browser. With `[generations]` on the node
  (`Routes::Generation { pins: true }`) the routing rule is: the decoded path, a trailing slash
  stripped, is `/<identity>@<generation>` when what follows its last `@` is an id
  (`snapshot::is_id`); that generation is served from `<local_dir>/<generation>/`
  (`Current::pinned`) when the node holds it, and is a 404 (`text/plain`: the requested
  generation, `current: <id>` and `held: <ids>`) when it does not; any other path routes as
  without pins, against the current generation. Every answer then carries `Meada-Generation:
  <id>` (the generation it was routed by; a 404 the current one or the held pin, a 503 none),
  and every `CallToolResult` `_meta` `{"meada/generation": "<id>"}` (`Meada::result`, tool
  errors included); tool names, schemas and text do not change. `listen` runs every start check (`engine::served`, the predicate `status --workspace` exits on) before it binds, so an open port means indexes present (the probe counts an index locked by a commit in progress as ready, after a 50 ms wait), then installs the SIGINT and SIGTERM
  handlers before it prints the address, then on a signal drops the listener and lets each
  connection finish its answer in flight (`graceful_shutdown`), for at most `DRAIN` (25 s; the
  hidden `with_drain_bound` shortens it, `main.rs` reads `MEADA_DRAIN_MS` for it in debug builds only, a second signal ends the wait), then cancels
  rmcp's `cancellation_token` (earlier it would end calls in flight with an error), drops what
  is left and exits 0 without waiting for the runtime's threads. Every request goes through
  `Gate` before routing, after the `Origin` check: in the default build, or with no `[oidc]`, it
  lets everything through; with `[oidc]` (feature `oidc`) it is `oidc::Oidc::admit`, which
  answers itself (the protected resource metadata, a 401) or hands routing a `Pass`, whose grant routing checks
  at an endpoint (`Pass::refused`, the 403) and filters every 404 listing through
  (`Pass::reads`). Routing then puts the `Pass` in the request's extensions, which rmcp's
  stateless transport hands the tool inside the request's `http::request::Parts`:
  `Meada::answer` opens the engine (`open_listed`, `open_held`) over `Manifest::workspace` with
  `Pass::reads`, which drops every entry outside the grant before the missing peers are
  computed, so no answer names one, even as missing (#290); a call with no `Pass` is a tool
  error. The pin is split from the path before the gate, so a pinned path is admitted
  as its identity is. The serving loop (bind, the address line, `Stop`, the drain) is
  `mcp::serve_http`, generic over what answers a request: the node's `run` hands it `route`
  over `Routes` and `Gate`, the console (`ui.rs`) its own answer. `main.rs`: clap.
  `msg.rs`: the CLI message catalog. Detail: [map/queries.md](map/queries.md).
- `modules.rs`, `valkey.rs`: the cloud modules (ADR 0012). `modules.rs` is the section table,
  in every build: each module section of `meada.workspace.toml` (`[valkey]`, `[snapshot]`,
  `[generations]`, `[refresh_lock]`, `[oidc]`, `[[github_app]]`, `[ui]`) with the cargo feature that
  owns it and whether this build has it. `Manifest::read` checks it before the typed parse, so a
  section whose feature is missing is an error naming the file, the line, the section and the
  feature, and a section the table lacks stays an unknown key. A module ticket adds its row and
  sets `built` to `cfg!(feature = ..)`. The `cloud` feature turns on every module at once and,
  as `refresh-lock` is, is Unix only. `modules.rs` also holds what the modules share, each
  behind the features that use it: `Problem` (a section key and its message) and `validated`,
  which turns one into the `file:line` error; `at_least` (an optional integer with a default
  and a minimum, `Msg::AtLeast`); `needs` (a section missing the sections it needs);
  `agent` (the ureq client of every module's outbound HTTP: no redirect, no proxy, a timeout, the
  platform verifier); `loopback` (`localhost`, `127.0.0.0/8` or `::1`, the one host test of every
  `http` exception); `reachable` (an endpoint URL a module may name: `https` or loopback `http`, a
  host, no user or fragment, and past the host what its `Shape` allows: a path, nothing, or a path
  and a query); `Secret` (a variable or a file, at most one of the pair, and `read_secret`, a
  bounded regular file whose error names only the file); `block_on` (a current-thread runtime shut
  down without waiting) and `within` (a `status` line's bounded wait). `valkey.rs` (feature
  `valkey`, internal; the only place
  the `redis` crate is used) is the one Valkey client the modules share: `File` is the
  `[valkey]` section, `File::validate` turns it into a `Config` (no password in the URL, `rediss`
  outside loopback, at most one of `password_env` and `password_file`, no `{` or `}` in
  `key_prefix`; each problem names its key), and `Valkey::connect` opens a connection manager or
  a cluster connection, reading the password from the variable or the file only when it
  connects. Every key goes through `Config::key(name, tagged)`: `<key_prefix>:<name>`, or
  `{<key_prefix>}:<name>` for the keys one script touches together (pointer, lock, fence). An
  error from the client has the password scrubbed. `status --workspace` prints `valkey:
  reachable` or `valkey: unreachable (<reason>)` after waiting at most `PING_TIMEOUT`; the
  line is informational and never changes the probe's exit code. With a `ca_file` the client
  trusts that file alone, not the system roots as well (redis-rs replaces them).
- `oidc.rs` (feature `oidc`; with `github_app.rs` the only places `jsonwebtoken` is used, with
  RustCrypto as its backend, never aws-lc-rs): server mode as an OAuth 2.1 resource server. `File`
  is the `[oidc]` section and `File::validate` its `Config` (`issuer` `https` or loopback `http`
  with no user, query or fragment, `public_url` the same with no path (`modules::reachable`),
  `identities_claim` and `audience` not empty, `groups_claim` (not empty) and its `[oidc.groups]`
  table together, at least one of the two claims, scopes RFC 6749 scope-tokens,
  `jwks_refresh_seconds` at least 60). `Oidc::start`,
  called by `mcp::Gate::new` before the listener binds, finds the metadata at the URLs the MCP
  specification tries in order (`metadata_urls`: RFC 8414 and OpenID discovery with the path
  inserted, then OpenID's appended, passing over a document that does not parse or names
  another issuer), checks its `jwks_uri` (which may carry a query), reads the JWKS (only
  signing keys: RSA, or EC on P-256 and P-384) and starts a thread that reads it again every
  `jwks_refresh_seconds` until the module is dropped; every request goes through ureq with no
  redirect, no proxy, `FETCH_TIMEOUT` and a cap, and the platform verifier for `https`.
  `Oidc::admit` (reached only without `Origin`, which `mcp::route` refuses first) serves the
  metadata of any path under
  `.well-known/oauth-protected-resource` (`resource` is `<public_url>/<path>`, percent-encoded),
  and otherwise reads the one `Authorization: Bearer` header (another scheme counts as no
  token): `ALGORITHMS` only, no `crit`, a cached key that `fits` the header (`kid` when both
  carry one, the key's own `alg`, its type and curve), the JWKS fetched again
  when none does (`refetch`, at most once per `KID_REFRESH_EVERY`), `exp` and `nbf` with
  `LEEWAY`, `iss`, `aud` against the configured audience or the path's and its identity's
  resources, then the scopes (403 when one is missing). The grant (`Grant::of`, once per
  request) is the union of `identities_claim` and what `[oidc.groups]` maps the groups of
  `groups_claim` to, a claim counting only as an array of strings, `*` from either for all;
  routing checks it and it bounds the workspace of each answer (`mcp.rs`). No token, error text
  of `jsonwebtoken` or claim value is ever printed. With feature `ui`, the console reaches the
  same checks through `admit_console` (one audience, `[ui] audience`; a `Bearer` challenge with
  no `resource_metadata`), `check` (a pasted token's outcome per check, signature, issuer,
  audience as a node checks it, lifetime and scopes, and its grant when every check passes,
  against the cached keys only, so it never fetches) and `admission` (the issuer's metadata as
  read at start, the cached key ids, the last read and a refresh that failed since, with its
  reason and never the answer's body, and the groups table bounded by the caller's grant).
- `ui.rs` (feature `ui`, no crate of its own; `cloud` enables it): the management console's
  process (ADR 0014), `meada ui --listen <addr> --workspace <bootstrap manifest>`,
  on the node's listener (`mcp::serve_http`, `mcp::address`, the same drain); never a node's
  route, never an MCP server. `File` is `[ui]` and `validated` lists every problem of it at
  once (`dir` and `public_url` required, `public_url` `https` or loopback `http` with nothing
  past the host through `modules::reachable`; `audience` defaults to `public_url`; `client_id`,
  `operations` defaults to false and requires `[oidc]` plus one operator key when true;
  `operator_groups` and `operator_claim` are mutually exclusive and require `[oidc]`, and the
  groups key also requires `oidc.groups_claim`). `Console::start` refuses, listing every problem: a manifest error, no `[ui]`, `[oidc]`
  without `[ui] client_id`, `[oidc]` whose issuer's metadata or JWKS cannot be read
  (`Oidc::start`, as a node), a `[ui] dir` that is not a directory, and the version check:
  `meada-ui.json` at the root of `[ui] dir` must be `{"version": "<this Meada version>"}`, and
  a missing, oversized (`VERSION_MAX`) or unreadable file counts as another version, both
  versions named. Each request goes, in order, through the `Host` check (403 unless `Host` is
  `public_url`'s host and port, or the bound address on loopback), the `Origin` check (403 for
  an `Origin` other than `public_url`, and for a write, any method but `GET` and `HEAD`, without
  one), admission (`Console::admit`), then routing; the 403 bodies are fixed text. Admission:
  behind the operator's proxy every request is a viewer's that reads everything; with `[oidc]`
  (feature `oidc`) a bearer token for `[ui] audience` through the nodes' module
  (`Oidc::admit_console`), a 401 with `WWW-Authenticate: Bearer` (and `error="invalid_token"`
  when a token was sent) or a 403 for a missing scope, both as JSON errors. The static files
  and `config` need no token, since the browser needs them to log in. A verified subject in
  `operator_groups`, or with the top-level `operator_claim` equal to JSON boolean true, is an
  operator; string `"true"` is a viewer. `Session::reads` and `whole` let operators see the
  entire fleet regardless of grant. The request's `Session` carries the token's grant, and
  `Session::reads` is the one check every view of a repository goes through, as #290 bounds a
  node's answers: a repository outside the grant is absent, state that belongs to no repository
  is shown whole, a `*` grant reads everything; the read routes (#304) and the playground
  (#305) bound theirs through it. `/api/v1/` is the JSON API (`config`, `session`, `manifest`,
  and with `[oidc]` `admission` and `POST admission/check`; errors `{"error": {"code",
  "message"}}`, an unknown path a 404 in that shape, as the admission routes are without
  `[oidc]`); `config` adds, with `[oidc]`, the issuer, `client_id`, the scopes and the audience
  the browser's authorization code flow with PKCE needs; `session` is the subject and role;
  `manifest` answers the file as loaded at start (a TOML table as JSON: secrets' variable and
  file names only), the module table with what this build has, and the errors of the file as it
  now stands on disk, read again per request, for a session that does not read everything
  without the `[[repository]]` entries, nested peers or OIDC group identities outside its grant
  (group wildcards remain visible), and with only the file and line of an error whose text
  could name one, including an identity known only through groups; `admission/check` reads `{"token": ...}` (`CHECK_MAX`) and answers each check's outcome and the identities, of the
  file's entries or the checked current workspace version's entries for `source = "store"`,
  that both the token's grant and the session's admit; the token lives
  only for that request, and a body that does not parse is a fixed 400, never serde's text.
  `ui/write.rs` owns four POST routes, all disabled unless `operations` is true and the
  admitted operator has a nonempty subject. Host, Origin, admission and role are checked before
  reading any body, carrying that same session into routing. `pointer` takes `{action,
  generation}` (`set`, `retain`, `drop`) and calls `snapshot::operation::pointer_set`,
  `pointer_retain` or `pointer_drop`; `fence/recover` takes `{at_least?}` (u64 or checked decimal
  string) and calls `fence_recover`; `workspace/versions` takes `{entries, message}` and calls
  `versions::save`; `workspace/apply` takes `{version}` and calls `versions::apply`. These
  owner functions keep the CLI checks, refresh lock and holder (operation plus escaped subject),
  with the admitted subject as author. Success is 200 `{result: "<CLI line>"}`; operational
  refusals are 409 with the CLI message; entry validation is 422 with CLI line/key diagnostics;
  malformed JSON, extra keys or malformed ids/fences are safe fixed 400s; caps are 413s. Every
  accepted or refused write logs one stderr line `ui: <subject> <action> <arguments>: <outcome>`,
  with controls escaped throughout. Pre-authentication refusals use absent subject `-` and safe
  fixed arguments, never body or token text. Save validation keeps its full CLI diagnostics in
  the 422 response but logs only the fixed outcome `workspace entries failed validation`, so
  invalid identities cannot copy editor contents into stderr. No route releases locks or
  triggers refresh.
  Operational reads live in `ui/read.rs`: `GET fleet` reads the checked pointer/retained/kept
  state (`generations::inspect`), live heartbeats (`heartbeat::read`), holder and remaining
  lock TTL, and fence counter from Valkey; workspace ids come from generation documents and
  last publish from the current document's modification time in the store. Malformed
  heartbeats are shown with fixed reasons, never used for a key or diagnostic. The two stuck
  cases name `meada snapshot fence recover`; the fleet maximum includes the current workspace
  pointer's fence, and a stuck node also names deleting its own
  `<local_dir>/current`. `GET store` lists every generation document name and fully reads
  current/retained documents only; `store/generations/<id>` reads one. Both use the pull's
  checked document reader, with validated ids, bundle keys, identities, hashes and blocks.
  `GET workspace`, `workspace/versions` and `workspace/versions/<id>` read the optional
  Valkey pointer and checked versions from the store (`versions::stored`, `documents`, the
  listing shared with the CLI), newest first, malformed documents listed as such. Repository
  entries and nested peers are bounded by `Session::reads`; malformed reasons cannot name
  hidden repositories. `GET repositories` lists commits per live served generation, schema
  versions from the console's own held indexes, unknown elsewhere; without snapshot it
  compares read-only index freshness and optional indexed commit with mirror `HEAD`.
  `index_workspace` atomically records an optional `index.commit.json` beside each index
  after a clean indexing pass; its commit is accepted only while its fingerprint of the
  index's existing metadata and file/document/manifest hashes matches. Older indexes,
  interrupted writes and stale sidecars give an unknown commit; the SQLite schema is unchanged.
  `GET credentials` uses `github_app::inspect` to read installations, token expiry and
  throttle expiry times from Valkey only, filtering owners by the granted workspace identities;
  expiry is extracted beside the token in Valkey, so no token or private key is read by the
  console. All operational service reads share a two-second request deadline and report
  the service that failed; permit waits share that deadline. At most `READERS_MAX` operational
  readers run at once, and a worker retains its permit until blocking I/O actually finishes,
  even after its request timed out. The managed-workspace identities of `admission/check` use
  this same worker and deadline boundary, including the store connection and runtime teardown.
  Config/session/manifest use no such permit. Missing module sections are explicit `requires`
  responses; without `[generations]` there is no per-node fleet view.
  The playground and the exports (#305) answer in process from what the console holds
  (`Holds`), never from a node: with `[snapshot]`, the generations its own `meada snapshot pull`
  (an init pull and a `pull --every` companion on a `local_dir` of its own) keeps, read per
  request as `--generations` serving reads them (`serve::Current`, `Engine::open_held`), the one
  `<local_dir>/current` names by default; without it, the bootstrap manifest's `[[repository]]`
  entries as read at start, each index under `index_dir`, as `meada mcp --listen --workspace`
  opens them (`Engine::open_listed`). `POST query` reads `{"repository", "generation", "tool",
  "arguments"}` (`QUERY_MAX`), the arguments as the MCP tool takes them and checked by the
  tools' own code (`mcp::served_call`, so `trace` with `changes: uncommitted` is refused with
  the node's text), and answers `{"text", "generation"}` (`generation` null without
  `[snapshot]`); `GET repositories/<identity>/export?generation=<id>` answers the bytes
  `meada export` prints, the identity being everything between `repositories/` and the last
  `/export`. Both open one engine per request whose workspace holds only what `Session::reads`
  admits (peers outside the grant dropped before the answer is built); a repository outside
  the grant is the 404 of one not served, a generation not held a 404 naming the held ones, and
  nothing servable yet (no pull, no index) a 503 naming `meada snapshot pull` or `meada index
  --workspace`, every other view answering, and the next request after the pull or the index
  answering with no restart. A tool error is a 422 with the node's text.
  Every other path is a file of `[ui] dir`: percent-decoded, a 404 for a
  `..`, `.` or empty segment, a NUL or a backslash, a resolved path (symbolic links followed)
  outside the canonical `[ui] dir`, or an extension outside the fixed table (`content_type`); a
  path that names no file, a directory included (none is listed), answers `index.html` unless it
  is under `/api/`. Files under `assets/` are cacheable for a year and immutable, every other one
  `no-cache`. Every answer carries `nosniff`, `Referrer-Policy: no-referrer` and the content
  security policy (`csp`, whose `connect-src` adds the issuer's origin with `[oidc]`); none sets
  a cookie or an `Access-Control-*` header.
- `github_app.rs` (feature `github-app`, which needs `valkey`; `jsonwebtoken` with `use_pem`
  only here): git's credential helper for the indexing job (ADR 0013). `File` is one
  `[[github_app]]` block and `validated` checks them all (`host` a git host name, lowercase; `api`
  `https` or loopback `http` with no user, query or fragment (`modules::reachable`); `app_id` at
  least 1; one of `private_key_file` and `private_key_env`; one App once per host; `[valkey]`
  required through `modules::needs`), each problem at the line of its own block (`error_in`).
  `workspace::Manifest::github_apps` reads only `[valkey]` and the blocks, never a mirror, since
  the helper runs on every git request. `credential` reads git's request strictly (`request`:
  `key=value` lines to a blank line, at most `INPUT_MAX`, a fixed error that echoes nothing) and
  the path as `<owner>/<repo>` (`repository`, lowercase); `get` lists each App's installations
  when its `listed` mark has expired (`Run::refresh`, under a lock, at most every
  `REFRESH_EVERY`, replaced in one `EVAL`), takes the owner's installations ordered by App id
  and installation id, skips the throttled ones (all of them are tried when every one is),
  picks one through an `INCR` counter, and answers its cached token or mints one under a
  per-installation lock (`Run::token`), caching it until `TOKEN_MARGIN` before it expires; a
  throttled mint passes to the next installation, and a `get` fails when every mint is throttled.
  `erase` drops the cached token equal to the password git sends; `store` does nothing. `throttled`
  marks the installation the repository's `last` key names; a URL it refuses is never echoed, since
  it may carry a token. A 403 or 429 marks for `retry-after`, else until `x-ratelimit-reset` when
  `x-ratelimit-remaining` is 0, else for a minute doubled while the mark holds (`Run::mark`).
  Every key is `<key_prefix>:github-app:<host>:...` through `valkey::Config::key` (`Run::key`,
  `app_key`, `installation_key`, `last_key`, one helper per shape): `app:<id>:`
  `installations` (a hash, owner to installation id), `listed`, `listing`, `throttled`;
  `installation:<id>:` `token` (`<expiry> <token>`), `mint`, `throttled`;
  `owner:<owner>:next` (the counter, the only key with no TTL); `repo:<owner>/<repo>:last`. A
  token must be visible ASCII before it is cached or answered, so the API cannot inject a
  protocol line. Errors carry an HTTP status or a URL, never a body, and the JWT is scrubbed
  from ureq's (`valkey::scrub`).
- `snapshot.rs`, `snapshot/store.rs`, `snapshot/pull.rs` (feature `snapshot`; the only places
  `object_store`, `reqwest` as a client of Meada's own and `tar` are used): the snapshot store. `File` is the
  `[snapshot]` section and `File::validate` its `Config` (an `https` endpoint, or `http` on
  loopback, with no user or path, and `path_style` for an `http`, IP or `localhost` one;
  `region`, `bucket` and `prefix` shapes; one credential source,
  the static keys only with `static`, at most one of each `*_env`/`*_file` pair). `bundle_id`
  and `generation_id` are the ids of ADR 0012 (`is_id` their shape); `entry_blocks` gives each
  manifest entry's blocks as canonical JSON (`Listed::blocks`); all four, and `fence`, are
  `meada_core::ids`'s, re-exported. `publish` is `meada snapshot
  publish`: per repository `index::open_read` plus `index::fresh`, `repo::unclean` and
  `repo::head`, every problem listed; then `store_generation` uploads the missing bundles (a
  first pass measures the tar's size and hash, a second streams it as a multipart upload with
  them as metadata, aborted if they differ), reads each back from metadata, writes the document
  and `current` last; an existing document is verified instead (`verify`) and never rewritten.
  `Document::workspace`, optional, is the id of the pulled workspace version with `source =
  "store"` (`Manifest::version`), absent with `source = "file"`, and part of no id: a stored
  generation keeps the version that first produced it, and older binaries ignore the field.
  The tar is `index.db`, then the mirror under `mirror/` in path order, symbolic links as links.
  `store::Store` is the client `meada snapshot pull` (#273) reuses: object_store's S3 client
  over `Guard`, a reqwest client with no redirect and no proxy that refuses any origin but the
  bucket's and the chosen credential source's (STS in `region`, the Pod Identity agent, IMDSv2),
  whose TLS is `store::tls` (rustls with ring and the system's roots, optionally augmented by
  the manifest-relative `snapshot.ca_file` PEM certificates, handed to a reqwest built
  with `rustls-no-provider`; object_store signs with ring too). Ring is the only rustls provider
  in any build (ureq brings it, and redis reads it as the process default): a second one, as
  object_store's `aws` feature would add with aws-lc-rs, makes rustls panic on the first TLS
  connection of a build with both modules;
  credentials come only from that source (static ones built from the variables or files, the
  others through object_store's providers given exactly those inputs, IMDSv2 over IPv4 only),
  stay in memory and are scrubbed from every error: the static secret is replaced, and an S3
  error body loses its `CanonicalRequest`, `StringToSign` and their `*Bytes` echoes. The
  secret files are read through `modules::read_secret`, as `valkey.rs` reads its own. Each
  bundle keeps its `open_read` connection, the read transaction its freshness check ran in,
  until its upload ends. Every object carries `meada-size` and `meada-blake3` metadata.
  `snapshot/pull.rs` is `meada snapshot pull` (`pull`, once; `main.rs` loops for `--every`),
  which treats everything it reads from the store as hostile. Under `local_dir`: `current`
  (the node pointer file, one line `<generation id> <fence>\n`, fence `0` from the store's
  pointer and the fence read with the id from Valkey with `[generations]`; `pull::current`
  reads it and checks the id and the fence), `pull.lock` (an advisory lock taken with
  `try_lock`: a second pull returns `Pulled::Running`), and per held generation `<generation
  id>/` with `generation.json` (the document as stored) and per repository `<bundle
  id>/index.db` and `<bundle id>/mirror/`; a pull in progress writes only `<generation
  id>.partial/` and `current.new`. Steps: the store's `current` must be an id and a newline
  (`store_pointer`); `checked` refuses a document before any path is built from it (its
  generation id, a `workspace` field that is a version id when present
  (`Msg::SnapshotBadWorkspace`), `identity_problem` (`meada_core::manifest`) per identity plus no duplicate and no character
  a git remote URL cannot hold, each bundle key `repos/<identity>/<bundle id>.tar` under the
  prefix, hash and `HEAD` shapes, the size sum, the generation id derived again); per bundle,
  `Store::download` streams it to a file checked against the document's size and hash, and
  `extract` checks every entry itself (no `unpack`: `index.db` first, then only `mirror/`; only
  normal components; regular files created `create_new`, `0644` or `0755`; directories made
  without following a link; symbolic links only to relative targets that fold inside the
  mirror, and after the last entry none through another link; hard links only to an earlier
  regular file of the mirror; no nested repository, that is no `.git` component, in any case,
  below the mirror's own `.git` and nothing under its `.git/modules`, since git runs a
  submodule's own configuration when `git status` descends into it; any other type refused);
  `index::vet` requires the extracted `index.db` to hold exactly the schema objects this build
  creates (type, name, table and SQL against a fresh in-memory index: no view, trigger, table
  or index more, none altered) and its header to name the rollback journal (bytes 18 and 19
  both 1, so no read-only open makes `-wal` or `-shm` files), naming the repository and the
  object;
  `settle` replaces `.git/config` with `[core]` and the identity's `origin`, removes `hooks`,
  `commondir` and `objects/info/alternates`, and checks the checkout, identity, `HEAD` and a
  clean `git status` that does not descend into submodules (`repo::unclean(_, false)`; the
  publish check on the operator's own mirrors keeps descending). `repo::isolate(local_dir)`
  makes every `git` call under it run with `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL` set to
  the null device and `-c core.fsmonitor=false`. Then the partial
  directory is renamed to `<generation id>/`, `current.new` is written, synced and renamed over
  `current`, and every id-named directory but the new current and the one just replaced is
  deleted. A failure removes the partial directory and leaves the pointer and every held
  generation as they were.
  `snapshot/serve.rs` is what serving reads from a node (`--generations`): `node` (the
  manifest's `[snapshot]` section and `local_dir`), `held` (the current generation: the
  pointer file through `pull::current`, then `generation.json`, bounded and checked as the
  generation's own), `Current` (the last generation `held` read, kept while the pointer names
  it; `Current::pinned` keeps one `Held` per pinned id, read once while held and dropped once
  `ids` no longer lists it, so what it keeps is bounded by what the node holds; a read that fails
  because a pull deleted the directory after the listing is `None`, the pin's 404, through
  `unless_gone`), `ids` (the generations held) and `reach` (the store's current id within `CURRENT_ID_TIMEOUT`, two
  seconds, for the probe; with `[generations]`, `reach_pointer`: Valkey's current id, then a
  HEAD of its document in the store, within the same bound, the reason prefixed `valkey:` or
  `store:`). It only reads; the pull is the only writer of `local_dir`. A pinned call still open
  on a generation that a pull's retention deletes may fail mid-call: the pull does not wait for
  what the server is still reading.
  `snapshot/generations.rs` (feature `generation-pointer`, which implies `snapshot` and
  `valkey`; no crate of its own) is the generation pointer, section `[generations]` (`keep`,
  default 3, at least 2; `Manifest::read` refuses it without `[snapshot]` and `[valkey]` through
  `modules::needs`). It owns the Valkey keys, both under the `[valkey]` prefix's hash tag:
  `{<key_prefix>}:current`, a hash `{id, fence}`, `{<key_prefix>}:retained`, a list of
  `<id>:<fence>` newest first, each id once, and `{<key_prefix>}:kept` (`kept`), the set of ids
  an operator keeps by hand (`meada snapshot pointer retain`), at most `KEPT_MAX`. `point` runs
  the one script that writes the pointer (`EVAL`; KEYS current, retained, `{<key_prefix>}:lock`
  and kept, ARGV id, fence, keep and the refresh lock's token, empty without one): with a token it returns 0 unless the lock
  holds it (`Msg::LockNotHeld`) and -1 unless the fence is above `current`'s
  (`Msg::FenceRefused`, naming `{<key_prefix>}:fence` and `meada snapshot fence recover`, as
  `Msg::FenceBelowNode` and `Msg::FenceStuck` do with `--at-least`; the two fences compare
  as decimal text, leading zeros dropped, so none loses precision as a Lua number, and -2 for a
  stored fence that is no decimal); then it removes every entry of the id, pushes
  `<id>:<fence>`, trims (the oldest entries whose id is not in kept go until at most `keep` of
  those remain; a kept entry never goes) and sets the hash. `publish` connects to Valkey before any upload and
  calls `point` last, with fence `0` and no token without `[refresh_lock]`, instead of writing
  the store's `current`; `meada snapshot pointer set` (`operation::pointer_set`) calls it with
  its own token and fence to move the pointer to a retained generation, a rollback that the
  next publish undoes. `read` is the pull's side: `HMGET current id fence` and the whole
  retained list (`LRANGE retained 0 keep+KEPT_MAX`), every id checked by `is_id` and every fence
  by `snapshot::fence` before anything else; a missing pointer, an empty list, a list longer
  than `keep` plus `KEPT_MAX` (`Msg::GenerationsTooLong`) or a malformed entry is an error
  naming the key. With
  it, `pull_locked` (through `Source::wanted`) fetches the current generation if the node lacks
  it and swaps the pointer file to `<id> <fence>` as read (a failure there fails the pull and
  changes nothing), then fetches every retained one the node lacks, each failure a warning
  (`Msg::SnapshotRetainedSkipped`, naming the id and the cause, on stderr) that blocks nothing
  and that the next pull retries, then deletes every held id outside the current, the retained
  and the one just replaced. A pull that re-fetched the current generation (its directory gone)
  reports `Pulled::Swapped`, so `--every` prints its id. Before any fetch, a current generation
  whose fence is below the one in the node pointer file fails the pull with
  `Msg::FenceBelowNode` and changes nothing; with no pointer file the node takes it whatever
  its fence. `engine::status_generations` adds a `fence: stuck: ...` line (`Msg::FenceStuck`)
  when the node's fence is above the one `reach_pointer` read, with no effect on the exit code.
  `snapshot/heartbeat.rs` (feature `generation-pointer`, crate `nix` for the host name; ADR
  0014) is the node heartbeat. `main.rs`'s `pull` loop calls `heartbeat::beat` at the end of
  every round of `pull --every` (never of a single pull), whether the round swapped, changed
  nothing or failed, and `heartbeat::check` once before the loop: with `[generations]`, a host
  name that is no node name, with no `[snapshot] node_name`, refuses to start
  (`Msg::NodeNameHost`, naming `[snapshot] node_name`). Without `[generations]` neither runs
  and no connection opens. The key `{<key_prefix>}:node:<name>` holds a JSON object (`name`;
  `generation` and `fence`, from the node pointer file, null without one; `held`, `serve::ids`;
  `ready`, `engine::served_generation` succeeding, the predicate `status --generations` exits
  on; `version`; `time`, RFC 3339 UTC to the second), and the `WRITE` script sets it with a TTL
  of three `--every` periods, adds the name to `{<key_prefix>}:nodes` scored by the expiry in
  Unix seconds by Valkey's own clock (`TIME`), and removes every name whose score is at or
  below now, so the set never keeps the names of pods long gone and the console lists nodes
  without scanning. `[snapshot] node_name` (`snapshot::is_node_name`: lowercase letters,
  digits, `.` and `-`, 1 to 63 characters; an error naming the file, the line and the key
  otherwise) defaults to the host name in lowercase; names must be unique, as pod names are,
  since two nodes with one name overwrite each other's heartbeat. A write that fails, or that
  Valkey does not answer within `WRITE_TIMEOUT` (two seconds, the bound the probe waits for
  Valkey), is one stderr warning for the round (`Msg::HeartbeatNotWritten`, the key and the
  scrubbed reason): the round's outcome, the pointer file, the exit code and the readiness do
  not change. `heartbeat::read`, shared by recovery and the console, selects live names with
  Valkey `TIME`, reads their keys and applies the complete `heartbeat::checked` validator.
  Malformed names never form keys and malformed documents never contribute fences;
  `Malformed` supplies the CLI warnings and console rows.
  `snapshot/versions.rs` (feature `generation-pointer`, ADR 0014) holds the workspace
  versions: the `[[repository]]` entries of a workspace as immutable documents in the store, at
  `workspaces/<version id>.json` under `[snapshot] prefix` (`Version`: `id`, `author`, `time`
  in RFC 3339 UTC, `message`, and `repositories` in manifest order, each entry its `repo` and
  `path` as written plus its blocks as `entry_blocks` gives them), stored with the size and
  hash metadata every object carries and never rewritten. The id is
  `meada_core::ids::workspace_version_id` of the entries alone. The current version is
  `{<key_prefix>}:workspace`, a hash `{id, fence}` under the pointer's hash tag (`pointer_key`,
  read by `current`, moved by `meada workspace apply`, #302). `import` (`meada workspace import
  <file> --message <text> [--author <name>] --workspace <bootstrap manifest>`) reads the file's
  `[[repository]]` entries through `meada_core::manifest::validate_import`, names each other
  top-level key on stderr as not imported (`Msg::WorkspaceNotImported`), and validates before
  `Store::connect`; entries whose document is stored upload nothing and answer it with its first
  author and message (`Msg::WorkspaceStored`). Shared `store_version` checks the serialized
  document against `VERSION_MAX`, then `Store::create` uses conditional create with size/hash
  metadata: concurrent imports preserve the first author, time and message, and a losing
  writer reads the checked original. HTTP `save` shares the preparation and storage under the
  refresh lock; CLI `import` requires only `generation-pointer` and takes no refresh lock.
  `list` reads and checks the pointer first (a malformed one, `Msg::WorkspaceBadPointer`, refuses the listing), then every object under
  `workspaces/` (`Store::list`), sorted newest first by time, then id; a name that is not an id,
  or a document that fails `checked`, is a `!` line. `show [<version>]` checks the argument
  first (`Msg::BadVersionArg`), takes the current version without one
  (`Msg::WorkspaceNoCurrent`, naming `meada workspace apply`) and prints the entries as
  `[[repository]]` tables, which `import` reads back to the same id. `checked` treats a
  document as hostile: at most `VERSION_MAX`, its id the one its key names, its time's shape,
  its entries rendered as `[[repository]]` text and validated as a workspace's
  (`meada_core::manifest::validate`), and the id derived from them again. The commands need
  `[generations]` in the bootstrap manifest (`Msg::WorkspaceNoSections`), not `source =
  "store"`, so an operator imports before switching. The author without `--author` is
  `cli_author`, `cli:<login>@<host>` (from the environment in a build without `refresh-lock`).
  `apply` (`meada workspace apply <version>`, feature `refresh-lock`) checks the id first
  (`Msg::BadVersionArg`), needs `[refresh_lock]` (`operation::sections`), reads and checks the
  version's document in the store (missing, `Msg::WorkspaceNoVersion` naming its key), then runs
  under `operation::locked` (holder `workspace apply <author>`) the `APPLY` script: it sets
  `{<key_prefix>}:workspace` to `{id, fence}` only while the lock holds the token and the
  operation's fence is above the stored one (decimal text, as `POINT` compares;
  `Msg::FenceRefused` otherwise); it prints `<id> <fence>`. Rolling back is applying an older
  version. `pull` (`meada workspace pull`) reads the pointer (`current`; none is
  `Msg::WorkspaceNoCurrent`, naming `meada workspace apply`) and the document (at most
  `VERSION_MAX`, `checked`) before anything on disk changes, then, under `<index_dir>/workspace/`
  (`DIR`) and its `pull.lock` (an advisory lock, one pull at a time), writes `<id>.json` (the
  document as stored, through `<id>.json.new` and a rename) and swaps `current` (one line `<id>
  <fence>`, `pull::swap`); a pointer whose fence is below the current file's, or equal with
  another id, is `Msg::WorkspaceFenceBelow` naming both fences and the file, and the same id and
  fence is `Pulled::Current` (the id on stdout, `Msg::WorkspaceAlreadyCurrent` on stderr). Only
  the current version's document and the one it replaced stay; a failure leaves the current
  file and the documents as they were. `pulled` reads the version the current file names from
  disk (`pull::pointer_file`, `Msg::WorkspaceCurrentBad` for a malformed one), bounded and
  checked as a pull checks it, and opens no connection: `Manifest::read_listed` uses it.
  The refresh job of a managed workspace runs, under `meada snapshot lock`, `meada workspace
  pull`, the fetch of the mirrors `meada workspace show` lists, `meada index --workspace` and
  `meada snapshot publish`: `apply` needs the lock, so it cannot run between the pull and the
  publish, and the whole refresh sees one version.
  `snapshot/lock.rs` (feature `refresh-lock`, which implies `generation-pointer`; crate `nix`,
  Unix only, a `compile_error!` elsewhere) is the refresh lock, section `[refresh_lock]` (`ttl_seconds`, default 300, at
  least 10; `Manifest::read` refuses it without `[generations]`). Keys under the same hash tag:
  `{<key_prefix>}:lock`, `{<key_prefix>}:fence`, which Meada only `INCR`s and `fence recover`
  raises, never lowers, and `{<key_prefix>}:holder`, a hash beside the lock with its TTL
  (`host`, the lowercased host name; `pid`; `started`, RFC
  3339 in UTC to the second; `holds`: `lock <program>`, the child's program without its
  arguments, or `<operation> cli:<login>@<host>`). `acquire` is shared by `lock` and the
  operations: `TAKE` (`SET lock <token> NX PX <ttl>` and, when taken, the holder written with
  the same TTL, in one script; else the holder's fields, so a lock already held is
  `Msg::LockHeld` naming its host, process, start time and what holds it, or an unknown holder
  for a lock without one, every field escaped; a token of 16 bytes of `/dev/urandom` in hex; a
  script that errs releases the token anyway, since the server may have taken it), then
  `INCREMENT`: under the same token, compare the counter as decimal text against the caller's
  ceiling and issue native `INCR` once, returning `GET` to preserve precision beyond 2^53.
  The signed INCR ceiling is `LAST_FENCE` (9223372036854775807); recovery reserves one more
  acquisition for publishing, so its own acquisition cannot exceed 9223372036854775806.
  A ceiling refusal is `Msg::FenceLimit`; a result that is not a positive `u64` is
  `Msg::LockBadFence`. Wire values and recorded fences remain full `u64`. `RENEW` extends the
  holder with the lock and `RELEASE` deletes both, each only while the lock holds the token.
  `run` is `meada snapshot lock`: `acquire`, then the child, spawned on the main thread in a
  process group of its own (`process_group(0)`) with `MEADA_REFRESH_LOCK` and
  `MEADA_REFRESH_FENCE`; on Linux its `pre_exec` arms `PR_SET_PDEATHSIG` (SIGTERM) and refuses
  to run if `lock` already died, the crate's one `unsafe`. A thread waits for the child; the
  `select!` loop renews every `ttl/3` (`RENEW`, compare-and-extend, each try bounded by the next
  deadline and polled as a branch of its own, so no signal or exit waits for Valkey), counts a renewal from the instant before it was sent, and calls the lock lost when
  a renewal finds another token or none succeeded for a whole TTL. Lost, or a SIGTERM, SIGINT,
  SIGHUP or SIGQUIT to `lock`: SIGTERM to the group, SIGKILL after `KILL_GRACE` (10 s). Once the child exits, the
  group gets SIGKILL (nothing is left in it; a grandchild that calls `setsid` escapes), `RELEASE` (compare-and-delete) runs within `RELEASE_TIMEOUT`,
  and `run` returns the child's code (128 plus the signal) or `Msg::LockLost`. `from_env` is
  `publish`'s side: with `[refresh_lock]` it needs both variables (`Msg::LockEnv`), `holds` (`GET
  lock` equals the token) runs before each bundle upload and before the document's (a token
  with no `[generations]` is an error, never a pass), and the
  token and fence go to `point`.
  `snapshot/operation.rs` (feature `refresh-lock`, ADR 0014) holds the operations, which the
  console calls in process too: each checks its arguments first (an id by `is_id`, a fence by
  `snapshot::fence`), needs `[refresh_lock]` (`Msg::LockNoSection`), runs under `acquire` with
  a holder naming it and its author, makes one write through a script that checks the token,
  and calls `release` whatever the write did, so it leaves no lock and no holder (a release
  Valkey misses expires with the TTL). `pointer_set` (`meada snapshot pointer set <generation>`)
  refuses an id outside `retained` (`Msg::PointerNotRetained`), then
  calls `point` with its token and fence, so the moved pointer's fence is above every node's
  and nodes swap on their next pull; it prints `<id> <fence>`. The rollback holds until the
  next publish moves the pointer past it: to stay back, the operator suspends the refresh job.
  `pointer_retain` (`meada snapshot pointer retain <generation>`) first reads the generation's
  document from the store and checks it as a pull does (`pull::stored_document`; missing or
  malformed, the error names its key, before the lock), then `RETAIN`: an id already kept
  changes nothing, a kept set already holding `KEPT_MAX` is `Msg::KeptFull`, an id with no
  entry is appended at the list's oldest end as `<id>:<fence>` with the operation's fence
  (which `fence recover` counts), and the id joins kept; it prints the id. `pointer_drop`
  (`meada snapshot pointer drop <generation>`) runs `DROP`: the current id is
  `Msg::DropCurrent` naming `current`, one with no entry `Msg::DropNotRetained` naming
  `retained`; else its entries leave the list and the id leaves kept, and the next pull deletes
  it; it prints the id.
  `fence_recover` (`meada snapshot fence recover [--at-least <fence>]`) reads the pointer, the
  workspace pointer (`versions::current`: `apply` takes its fence from the same counter), every
  `retained` entry and the heartbeat (`{<key_prefix>}:node:<name>`, its `fence` a `u64` or
  null) of every name of `{<key_prefix>}:nodes` scored above Valkey's `TIME`, as hostile input: a malformed
  pointer, entry, node name or heartbeat is a `Msg::FenceNotCounted` line on stderr and does not
  count, a name whose key expired is skipped. The floor and recorded maximum are checked
  against `LAST_FENCE - 1` before acquisition; records are read again under the lock to cover
  a publisher between the preflight and acquisition. `RAISE` checks the token and ceiling and
  sets `fence` to the highest of those and the floor only when above the stored one (decimal
  text, as `POINT` compares); it prints the fence it leaves, including its own `INCR`.
  Recovery refuses a counter already at that ceiling without consuming the last acquisition;
  it never lowers or clamps fences. The next locked publish can still use `LAST_FENCE`.
- `decide.rs`, `decisions.rs`, `decider/`: `meada decide`, the decisions file and its replay, and
  decider mode (the only network path). Detail: [map/decisions.md](map/decisions.md).
- `workspace.rs`, `config.rs`, `repo.rs`: repositories, peers, `meada.toml` and identities.
  `workspace.rs` also reads the workspace manifest (`Manifest`) and refuses the entries server
  mode cannot serve; an entry's `[repository.serve]`, `[[repository.resource]]` and
  `[[repository.peer]]` reuse `config.rs`'s shapes (the peer without `path`) and are the
  configuration of a mirror with no `meada.toml`. The entry's schema, those shapes and every
  check the text decides (identities included, `entry_problems`) are `meada_core::manifest`'s;
  `mirror` keeps the checks that need the mirror, after them. A mirror's own `meada.toml` wins whole: the
  entry's blocks are ignored, not merged, and `Listed::ignored` names them for `status`.
  The file `--workspace` names is the bootstrap manifest: its `[workspace]` section, read in
  every build, has `source`, `"file"` (the default, and what a file without the section means:
  its own `[[repository]]` entries) or `"store"` (the current workspace version,
  `snapshot/versions.rs`). `source = "store"` without `generation-pointer` is an error naming
  the feature (`Msg::WorkspaceStoreNeedsFeature`), without `[snapshot]` or `[generations]` one
  naming them (`modules::needs`), and beside a `[[repository]]` one naming both
  (`Msg::WorkspaceStoreBeside`); `Manifest::store` records it, with no entries from `read`.
  `Manifest::read_listed`, which `index`, `status`, `export` without `--generations` and
  `snapshot publish` call, reads the entries of the version `meada workspace pull` left under
  `<index_dir>/workspace/` (`versions::pulled`, from disk, once per read, `Manifest::version`
  its id), its paths relative to the bootstrap manifest's directory, and goes on as for a
  file's entries (the checks that need a mirror run in `mirror`); without one it refuses naming
  `meada workspace pull` (`Msg::WorkspaceNotPulled`). `mcp --listen` without `--generations`
  refuses such a manifest naming `--generations` (`Msg::WorkspaceStoreRefused`): a managed
  workspace's nodes serve generations, which take their repositories from the generation.
  `Manifest::index_dir` is the directory every index lives under.
  `index.rs`: the SQLite schema, the freshness pipeline in `refresh` (with `fresh`, its
  read-only twin for `meada snapshot publish`: the same walk, `packs_of` and stamps, and a file
  whose stat moved but whose hash did not is no difference), and
  `export`; `open` and `state` take the index's path, `<root>/.meada/index.db` locally (`path`,
  with a `.gitignore` of `*`) or `<index_dir>/<identity>/index.db` in server mode (no
  `.gitignore`). A rebuild (a missing index, or one of another schema) is a rename under the
  rebuild lock: `open` builds the whole new index in `index.db.new.<pid>` while it holds
  `index.db.lock` and renames it over `index.db`, so a reader sees the old index or the new
  one, never a missing or empty file; `refresh` stays one transaction in place. An operator
  renames an index in (built elsewhere) only while no `meada index` writes that index, one
  writer per index: a refresh's live `index.db-journal` would be taken for the new file's. `lang/`: the
  language adapter and the TS/JS, Python, Rust, Go, Java and Kotlin packs. `resolve.rs`: calls to definitions (`Resolver`), values
  and receivers (`Code`: `Code::origins`, the one origin question, with three evidence policies
  (`Values`, `Constants` for the TypeScript forms, `Declarations` for Go's typed clients and
  forms), `Code::outside` for what an import from outside binds a name to, `Code::extensions`
  for the extension functions a Kotlin call has in scope, and `Code::expand`
  for wrappers, `Expand::Chain` reading a call whole). `community.rs`: the deterministic Leiden behind communities (spec
  #31). Detail: [map/indexing.md](map/indexing.md).
- `contract.rs`, `http.rs` and its `http/` submodules, `resource.rs`, `channel.rs` and its `channel/` submodules
  (`channel/message.rs`: the message types and broker helpers; `channel/{ts,python,rust,go,java}.rs`:
  the message libraries of each language): the
  `ContractKind` trait and the HTTP kind (`http.rs`: the contract and path rules, the mount
  machinery and the shared consumer readers; one submodule per language: `http/ts.rs` and
  `http/ts/clients.rs`, Express and Next.js route-file providers and `fetch` consumers;
  `http/python.rs` and `http/python/clients.rs`, FastAPI, Flask and Django providers and the
  requests, httpx and aiohttp consumers, whose rows also list reqwest and ureq; `http/rust.rs`
  and `http/rust/axum.rs`, actix-web, Rocket and axum providers; `http/go.rs` and
  `http/go/clients.rs`, net/http, gin, echo, chi and gorilla/mux providers and their consumers
  with resty; `http/java.rs` and `http/java/clients.rs`, Spring MVC providers and the
  RestTemplate, WebClient, `java.net.http` and OkHttp consumers, whose bases come from the
  Spring configuration `jvm/spring.rs` reads for the JVM packs' kinds; `http/kotlin.rs`, Ktor
  providers), resource and channel
  kinds. `package.rs` and `package/`: npm, PyPI, Cargo, Go and Maven packages as
  contracts (specs #22, #24, #30, #25 and #26), the shared steps and one module per ecosystem behind
  the `Ecosystem` trait (spec #225). Detail:
  [map/contracts.md](map/contracts.md).
- `document/`, `literals.rs`, `zod.rs` (and `meada-core`'s `shape.rs`): OpenAPI and AsyncAPI
  provider documents, the shape a contract carries, and the shapes read from a handler's returned literals and zod
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
  `Engine::exports` drops (in the package kind's `workspace_step`, after `import`), each one that is the target of an import
  from its own repository (a monorepo), at any level.
- Package: `package::npm::<name>`, `package::pypi::<name>` (the PEP 503 normalized name) or
  `package::cargo::<name>` (the `[package] name`; a dependent's site names the key it writes, so
  a renamed crate has its own site) or `package::go::<module path>` (the `module` of a `go.mod`), or
  `package::maven::<group>:<artifact>` (a Maven or Gradle artifact),
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

- New **contract kind**: implement `ContractKind` (`contract.rs`) and add it to `KINDS`, at the
  end: the order of the list is the order of the steps and of the output. Nothing in
  `engine.rs`, `items.rs`, `decisions.rs`, `trace.rs`, `entry.rs` or the extraction loop of
  `index.rs` names a kind, and a kind's rule stays in its own file, with the library forms it reads in each
  language (one module per language: `http/<language>.rs`, `channel/<language>.rs`), never in
  the pack. Three questions are
  required: `prefix`, `extract` (receives `Code`, never the syntax tree, and `Indexed`, what the
  index read of the repository besides: the manifests and tsconfigs, the exported rows, the
  files of every pack and the bytes of the config files the packs list (`Indexed::configs`,
  never stored: Java's Spring `application.properties` and `application.yml`), which a kind
  reads when its contracts depend on what the manifests or a configuration say;
  it returns `Found` with a `Site`: `Def`, `Inline`, `File`, `Call`) and
  `link` (receives the whole workspace's providers and exports). The others have a default
  body, and a kind overrides only what it answers differently:

  | Question | The kind says |
  |---|---|
  | `entry` | which entry point kind a side of a contract makes (a pure function of the contract: entry points are stored) |
  | `declare` | the sides `meada.toml` declares, run at query time (a config edit needs no reindex) |
  | `workspace_step` | a step over every repository after every `declare` (the package provider index, dependency entries, imports, shapes) |
  | `after_links` | a pass over the links after the decisions replay (HTTP: drift) |
  | `unresolved`, `label`, `parts` | whether an identifier is an unresolved side, how it is labelled, the parts `search` ranks |
  | `place` | where a site sits when it is neither a symbol nor a call (packages: manifest and field) |
  | `search_word`, `own_search` | the word of `search`'s `kind` and, for an own renderer, its hits |
  | `sides`, `view`, `related_sides` | the role words and the carried word of `context` on a contract, and the sides that reach it by another name; a contract listed by its providers alone is followed by the linked consumers that read fields, with their drift (the engine's rule: `View::follows_drift`) |
  | `at_call`, `context_sections` | the lines a call adds to a symbol's `contracts:`, and the sections `context` adds after the callees (packages: `consumers:`) |
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
  constructors: `FRAMEWORKS` in `http/rust/frameworks.rs` for the web frameworks and HTTP
  clients (`passes_on`, and `frameworks::made`), `LIBRARIES` in `channel/rust.rs` for the
  brokers. The tables live under the kinds, never in the pack: the kind hands the question its
  predicate (`Follow::Values { passes }`, `passes_nothing` for a reader with no table) and the
  filter reads it. A kind reads a route off each call of the chain (`http/rust/axum.rs`, and `http/rust.rs`: actix-web
  `App`, `web::scope`, `web::resource`, `.configure(f)` and Rocket's `build().mount(..)`, whose
  `routes![..]` the Rust pack reads as a list of the handlers' paths).
  `Code::returned` and `Code::constant` give a kind the values a call may return and a
  constant holds, for what it must not leave unmounted.
  `Expand::Chain` is `Code::expand` for a request built by a chain (`http/java/clients.rs`; in
  `channel/java.rs` an SQS request, the stream maps of a Redis group read and a
  `BindingBuilder` chain):
  the reader gets the call as one `Value::Call`, its receivers and every argument along the
  chain folded with the wrapper's callers bound, and a name an import from outside binds as a
  `Value::External`, spelled (`Outside`, candidates included) in the file that writes it. An
  enum member passed through a wrapper so keeps the caller's import evidence, and a type of
  the caller's repository by that name is no member. Only this reading folds a call's
  arguments or makes a `Value::External`: `Expand::Call` and `Code::value` leave them as
  written, so no other reader sees a change.
  `Code::expand_with` is `Code::expand` with an injector (`resolve::Inject`): what a kind reads
  off an annotation written on a field or a parameter (`Facts::annotated`, transient, read
  from the full facts so a file the index kept is parsed again through `Code::misses`). The
  shared side asks it for a field no file assigns (`Code::field_written`, the `.f` marker
  constants the Java pack writes for every `f = v` and `x.f = v`) and for a parameter no frame
  binds of a function no call of the repository may reach (`Code::may_be_called`: an exact
  caller, or any callee ending in its name, so a name match, a call on an opaque receiver, a
  `new` inside an anonymous class and `this(..)` or `super(..)`, which the Java pack emits as
  constructor calls, all count); a function some exact call reaches climbs to its callers as
  before, so the injected value never stands in for what a caller passes. A
  constructor's parameter a `final` field holds (`Const::scope`, the constructor the value is
  written in) is no wrapper's parameter: it is injected or unknown, never climbed. The values
  an injector gives are transient like the arguments they stand for, so the index stays free of
  them. `jvm/spring.rs` is the one injector (Spring's `@Value`, `spring::inject`, which is given
  the annotation with what it marks), shared by the JVM packs; `spring::resolved` is the one
  rule for an annotation's text Spring resolves (`@Value`, a Feign `url` or `path`, a listener's
  topic or queue): literal text with its placeholders resolved, an environment read kept, any
  other part unknown. The list of Spring's config files is the reader's too
  (`spring::CONFIG_FILES`), which the Java pack returns as its `config_files`. `jvm/names.rs`
  holds the name questions the JVM kinds share: `one` (exactly one spelling in the kind's
  table), `known` (a name an import binds), `spellings` and `typed` (the outside type the source
  writes for a name), and the Java readers' shared filters `java_files`, `jvm_files` (Java and
  Kotlin files) and `is_method` (an annotation on a method of the file).
- New **Go router** (spec #25; net/http, gin, echo, chi and gorilla): a row of `ROUTERS` in
  `http/go.rs` (the package, the functions that make a router, the methods that make a group
  (a `Form`: `Group(prefix)`, chi's `Group(fn)` and `Route(prefix, fn)`, gorilla's
  `PathPrefix(prefix).Subrouter()`) or mount a router (chi's `Mount`), the route methods with
  the methods each serves, where the handler sits, which path spellings are parameters);
  `Go::provide` reads the routers through `Code::origins` and `prefixes`, as the Rust kinds do.
  The function a `Route` or `Group` call is given (`Go::callbacks`) binds its first parameter to
  that group, which `Code::origins` does not see; `Go::reach` follows it from there through the
  exact callers that pass the parameter on and the names bound once to it (`Code::param_of`,
  `Code::param_args`, `Code::constant`). The pack gives it `Call::args`,
  `Call::receiver`, the constants a function binds once (`Facts::consts`) and what it returns
  (`Def::return_values`), read off the syntax only (`lang/go/values.rs`).
- New **Go HTTP client** (spec #25, #79): `http/go/clients.rs` reads `net/http` calls (on the
  package, a `Client`, `DefaultClient`), `NewRequest(WithContext)` and resty requests as
  consumers; a call is a function of a library when `Code::outside` says so (`direct`: its head is the
  import itself, the one check the Go routers, clients and channels share), and a receiver is a
  client of a library when `Code::origins` with `Follow::Declarations` answers the form `Type`
  (the type a typed parameter, field or `:=` names, qualified by its import path). The URL is folded in
  `lang/go/values.rs` at parse time (`+`, `fmt.Sprintf` verbs as holes, `net/http` method
  constants, package-level `const` and a `var` nothing assigns, in the package or in another one
  by import), and a resty base is the value of the `SetBaseURL` argument on the chain (or the
  statement) that built the client. A base belongs to one client, not to the call that makes
  clients: a client is the `resty.New()` call with the calls of the repository's functions it
  came through (`Code::origins` with `Follow::Values`, each route's climbed calls), so two clients one factory makes keep their own
  bases, and a `SetBaseURL` inside the factory reaches every client it makes. A client held in a
  struct field (`c.rc`, `Code::field`) holds the clients of the values every composite literal
  of the repository gives that field of that type (`Code::field_values`, read from the marker
  constants `{field}` the Go pack writes for each field a literal sets, `{*}` by position, the
  type `?` when it is not read): a literal by position, of a type not read or not known, or of
  a type that declares no such field, a value that is not traced, a field written through a
  selector anywhere (`Code::field_written`), or one met again while it is followed, makes the
  base unknown, and so does a `SetBaseURL` called through a field. Wrapper climbing is
  `Code::expand`, as for every kind. A package `var` another file of the package assigns, or any
  file of the repository writes through a selector (`pkg.X = ..`, `&pkg.X`, `pkg.X++`: any write
  to a field called `X`, whatever it belongs to), is `Unknown` (`Resolver::redeclared`, read from
  the markers `Name.var` and `.Name` the Go pack stores as constants of no value); a `const` is
  never one.
- **Go environment reads** (spec #25, #80): `os.Getenv("X")` is `Str([Env(X)])` and
  `os.LookupEnv("X")` is so for the first name of a `:=` or `var` (`values::lookup_env`), through
  `strings.TrimSuffix`/`TrimRight`/`TrimSpace` (`values::env_value`) of the read or of a name
  `Std::note` saw bound to one earlier in the file (source order; a package-level `var` counts).
  `v = strings.TrimX(v, ..)` leaves `v` a constant when it holds a read, and so does
  `if v == "" { v = .. }` when the default is plain (`values::keeps_env`, `values::plain`,
  decided in `Walk::finish_values`); the test may also be `len(v) == 0` (no binding of the file
  hides `len`; a package-level `len` in another file is not seen), `!ok` or `!ok || v == ""`
  (`values::Guard`: `ok` is the second name of the `v, ok := os.LookupEnv(..)` that bound `v`,
  bound once and never assigned): text, or names that are constants of the same file holding
  plain values; a call, a name bound to a read, a loop variable, a package `var` or a name from
  another file leaves `v` no constant, so the base is absent. `http/go/clients.rs` `one_read`
  makes every read of a URL a hole when it holds two. `http.rs` `locate` and `link` map the
  head `Env` as for every language. The `xlang-go-*` fixtures are the cross-language chain: a
  repository of another language joins by one more `common::xlang_go` entry.
- **Go channels** (spec #25, #82): `channel/go.rs` lists each library (`LIBRARIES`: the
  import path without its module's major version, `modfile::unversioned`, which the Go routers
  and clients use too, and the constructors that make its client)
  and reads a call when `Code::origins` with `Follow::Declarations` traces its receiver to one of them (the
  same question as the written-type read of the clients, with the constructor's steps: it also follows `x, err := f()`, a repository function that returns a
  library type, and a method on a value the library made, such as `conn.Channel()`). The
  channel name goes through `Code::expand`, struct fields of a composite literal included:
  `values::composite` gives `Value::New(Type, [Object(fields)])` for a struct written with its
  type and `Value::List` for a slice (`Code::eval` descends into `New`); `aws.String(x)` is `x`.
  A callback is the handler (`Handler::Inline`, or `Def` for a function or a method value
  `a.onAudit`, through `Code::definition`); a polling client's handler is the function containing
  the call that names the topic, queue or stream (the `consume` default); a sarama group's is
  the `ConsumeClaim` of the type its `Consume` is given (`Code::method`, which serves nothing
  else). A writer that names a topic (`kafka.Writer{Topic:}`, or the argument of kafka-go's own
  `NewWriter`, never a repository function of that name) stands for its messages, read from
  `Code::constant` of the writer's name, and a `Topic` field written anywhere in the repository
  (`Code::field_written`, the markers of #79) leaves it unresolved; a writer a function made, or one with no topic, is read from each message. The
  `xlang-go-*` fixtures carry a TypeScript producer to the Go consumer group.
- **Java channels** (spec #26, #92): `channel/java.rs` lists each client by the outside types
  the source may write for its receiver (`CLIENTS`: `KafkaTemplate`, `RabbitTemplate`,
  `JmsTemplate`, the SQS clients, jnats `Connection` and `Dispatcher`, the Jedis clients and
  Lettuce's `RedisCommands`), read through `jvm::names::typed` and `one`, and the listener
  annotations (`@KafkaListener`, `@RabbitListener`, `@JmsListener`) through `jvm::names::known`
  on a method's `Call::decorates`. Names fold through `Code::expand_with` with
  `jvm::spring::inject` (`Expand::Chain` for an SQS request, a Redis group read and a
  `BindingBuilder` chain); an annotation's values fold through `Code::value` and its placeholders
  resolve through `jvm::spring::resolved`. An annotated listener's handler is the method
  (`Handler::Def`); every other client takes `consume`'s default, the method holding the call
  that names the topic or queue (the Java pack keeps a lambda's calls in the method that writes
  it, so a jnats callback is that method too). The overload of `RabbitTemplate.convertAndSend`
  and `JmsTemplate.convertAndSend` is told by the type of the argument a `MessagePostProcessor`
  would take (`of`, through `jvm::names::declared`): a name the source types as one, or a `new`
  of one, picks the routing-key form, and so does a `CorrelationData` there
  (`(routingKey, message, correlation)`) unless a `MessagePostProcessor` before it makes it
  `(message, postProcessor, correlation)`; an argument whose type the source does not tell (a
  lambda, a call, a name with no written type, anything the pack cannot read) leaves the
  producer unresolved, and a literal or a name of another written type is the message. The two-argument AMQP form publishes to the
  template's exchange: `spring.rabbitmq.template.exchange` of the module's configuration, else
  the default exchange, a producer of the queue the key names. An `@Exchange` is `direct`
  unless its `type` (a string, or a member of `ExchangeTypes` bound by an import) says
  otherwise: a direct or fanout exchange keeps only the keys with no wildcard word, a headers or
  custom one none, a topic one, or one whose type is not read, every key; a `DirectExchange` in
  `BindingBuilder` keeps only a key with no wildcard. The listener annotation is asked first
  (`jvm::names::known`), and only then whether it is on a method (a set of the file's methods,
  built once), so a file of many `@Override` methods is read in linear time. The Java pack reads an
  annotation written as an element value as `Value::New(name, [Object(elements)])`, its unnamed
  element as `value` (the shape of a Go composite literal; `Code::eval` folds it), only inside
  an annotation's arguments, which the index never keeps; `channel/java.rs` reads a nested
  `@QueueBinding`, `@Queue`, `@Exchange` or `@TopicPartition` by its element's key, since Java
  types an element by the annotation that declares it. `jms` is a broker kind of its own (also
  AsyncAPI's `jms` protocol), which the Kotlin pack reuses.
- **Kotlin channels** (spec #27, issue #104): `channel/java.rs` reads `jvm_files` (`.java`,
  `.kt`, `.kts`) with no pack gate, since both packs share one `Resolver` and one `Code`. The
  Kotlin pack gives it the same shapes: `kotlin::element` reads an annotation element value
  (a nested `QueueBinding(..)` as `Value::New`, `[..]` and a nested `arrayOf(..)` as
  `Value::List`; a top-level `arrayOf(..)` stays a `Value::Call` that `channel::array` accepts
  only when `known` binds `kotlin.arrayOf`), and an infix `a to b` is a `Call` on `a`. The
  Kotlin `Call::on` is always empty, so a `BindingBuilder` chain is found by the callee shape
  `(call_expression).with` and its start call by range in a map built once per file. A stream
  map written `mapOf(key to id)` counts when `known` binds `kotlin.collections.mapOf`; a
  constructor with no receiver (`Queue("q")`) counts as `new`. `of()` reads a lambda argument
  as an untold post-processor overload, so `convertAndSend("x") { m -> m }` keeps `x` as the
  destination. A call on an implicit receiver (an extension function on the client, `apply`,
  `with`) is not read. The `conformance-kotlin` fixture holds each channel form and the
  `xlang-kotlin` one a TypeScript producer reaching a Kotlin `@KafkaListener`.
- New **package ecosystem** (spec #22 user story 26, spec #225; track B packs): implement the
  `Ecosystem` trait of `package.rs` in a module of its own under `package/` (`npm`, `pypi`,
  `cargo`, `go`, `maven` today) and add it to `ECOSYSTEMS`; the package kind, the index writer
  and the packs learn no name. An arm for one ecosystem added to the shared code is a finding. It plugs
  in another manifest format (`Cargo.toml`, `go.mod`) without a new kind of contract: the
  contract `package::<name>::<package>` (`Ecosystem::prefix`, derived from `name`) still routes
  to the same `Packages` steps (`declare`, `consume`, `import`, `shapes`, `link`, `place`,
  `reached`), `Providers` is keyed by ecosystem name and package name, so two ecosystems never
  link, and the reason grammar (`reason`) stays one function that asks the ecosystem.
  The questions, each with its default (an adapter overrides only what its ecosystem answers
  differently):
  - Required: `name` (the contract prefix and the export's `ecosystem` field spell it);
    `is_manifest(file name)`; `read(path, bytes)` (the same `Summary` for every ecosystem: name,
    version, dependency entries, entry fields, source roots, console scripts; it never fails,
    hostile bytes read as empty); `sources()` (the extensions of the source files whose imports
    name its packages); `names_package(specifier)`; `entries(..)` (the dependency entries an
    import of a file may name, each with the subpath inside the package). `imports(..)` (the
    `(specifier, chain)` pairs a call goes through) has a default, `Code::imported`; Maven
    overrides it (the class and `name(arity)`, from the head's import or the receiver's type,
    one pair per on-demand candidate, `Outside::spellings`). `declares(..)` (whether a provider
    declares the specifier itself) defaults to `true`: `import` lets none of one site's pairs
    enter when two are declared, so only an ecosystem with several pairs per call answers it.
  - `fits`: literal package-name equality; Maven matches the literal fragments of a computed
    coordinate. `uncertainty`: none; Maven's `computed` source keeps even one match ambiguous.
  - `flag`: the range is unchecked (`range unchecked`); npm checks semver.
  - `reaches`: every provider; npm's `workspace:` keeps the dependent's own repository, Cargo
    and Go share `sourced`, in the seam (a `path` by the folded location, a `git` by the
    remote's identity).
  - `owns`: every manifest of the ecosystem bounds the files under it; PyPI only a
    `pyproject.toml`, Cargo only a manifest with a `[package]`.
  - `ways` and `resolve`: Node's rule (`exports`, else `types`, `module` and `main`, else the
    subpath as a path, or `index.<ext>` for the bare name when none of the three exists, `exact`;
    then the candidates among the files the manifest owns); Cargo follows the crate's library root alone
    (`How::CrateName`, no candidates, and the library-head check of an entry that does not
    rename its crate); Go looks in the one directory the subpath names (`How::ModulePath`); Maven
    looks the class up by package declaration in the files the artifact owns that are no test
    sources and picks the method by name and argument count among the overloads of the class's
    family (`How::Package`, `Maven::resolutions`; the provider's `families` rows, an opaque
    ancestor keeping the target from `exact`). `member_of`: none; Maven's constructors and
    methods of a class, so `context` on a public class lists, once per call, the calls that
    reach its members (`reached`, at the best level of the call's links into the class).
  - `resolutions`: the single-entry loop (the entries, each provider the entry `reaches`,
    `resolve` in it); PyPI replaces it whole, since its imports name a module and several
    distributions may expose it.
  - `shapes`: false; npm's packages carry them. `programs`: none; PyPI's console scripts.
    `inherit`: nothing; Cargo's workspace entries and version.
  Callers use the seam's free functions, never the adapters: `of` (the adapter whose manifest a
  file is), `of_source` (the adapter whose sources a file is, none for a file no ecosystem
  claims, so a pack whose ecosystem has not landed imports nothing), `of_contract` (by prefix),
  `is_manifest` and `read_manifest`, `programs` (every adapter's, sorted, without repeats) and
  `inherit` (every adapter's, in list order). `package::is_manifest` makes `manifests_found`
  (`index.rs`) stamp the manifest files into `manifests`. The list order fixes the order `of`
  and `of_contract` search in and the order `inherit` runs in; a new ecosystem appends. The
  deliberate extras: `is_npm`, the export's compatibility rule (it names the ecosystem only when
  it is not npm, so the goldens that predate the field stay); a reader in a language pack when the manifest is also a pack config (as
  `go.mod`, read by `lang::go::modfile`, and `Cargo.toml`, read by `lang::rust::crates`, are;
  the adapter calls the pack's reader, never the other way); and, for a user-visible ecosystem,
  the README state line and the CHANGELOG.
  Which ecosystem a call through an import belongs to is its file's (`of_source`; Cargo's for a
  `.rs` file, Go's for a `.go` one), and resolving the import into the provider is the
  ecosystem's own rule behind `resolutions` (npm: `split`, then `ways` and `resolve` by Node's
  rule; PyPI: the module found under the provider's roots; Cargo: `cargo::names_crate`, the
  `use` head matched to the key of a renamed dependency or to the provider's library name
  (`cargo::lib_head`), then the crate's library root; Go: any import path that is a valid
  contract name (`valid_name`), matched to the longest module path of the nearest `go.mod`'s
  entries (`modfile::inside`), then the one directory of the provider the rest names; `Go::read`
  takes `require` and `replace` from `lang::go::modfile`, the Go pack's one `go.mod` reader, and
  gives a local `replace` the Cargo `path` form in `Entry::via`). A step outside `ContractKind`
  fills what a manifest inherits: the seam's `inherit`, run in `index::read_manifests` over the
  whole manifest list after the reads, has Cargo fill the `workspace = true` entries and
  `version.workspace = true` of each `Cargo.toml` from the nearest one above it, so a refresh
  equals a cold export. `Ecosystem::reaches` takes the repositories' roots (`Export::root`, set
  by `Engine::exports`) for a Cargo `path` that leaves its repository: it folds both locations
  as paths, never opening one, so a symlink changes nothing. A change to what is stored
  re-blesses `tests/golden/schema.txt`.
- New **message library** (spec #20, tickets after #45 and track B packs): a form in the
  channel kind's module for that language (TS/JS: `channel/ts.rs`; Python: `channel/python.rs`; Rust: `channel/rust.rs`; Go: `channel/go.rs`; Java: `channel/java.rs`), never in the pack, matching a call's
  method and the `Code::origins` of its receiver, asked with the policy its language needs (TS/JS:
  `Constants`, the form of the call that made the value, outside name and steps; Go: `Declarations`; Java: the written type, `jvm::names::typed`; Python and Rust:
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
  for it is a shape fact: an optional part of the pack's output, `serde(skip)` like
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
- **Shapes through package imports** (issue #175): a source reading a name that `Code::imported`
  says comes from outside the repository records a `shape::Import` on the shape (its path, the
  importing file, the specifier and name, and the `Want` view: `zod`, `infer`, `literals`,
  `type`, or `opens` for a provider typed with a package type, which takes that type's index
  signatures alone) instead of opening it or dropping it. A repository keeps, beside its
  contracts, the shapes of the module-level constants and types the files of its packages'
  public surface export (`surface`: the entries of each named manifest, as an import
  resolves them, and what their `exports` rows lead to; `exported_shapes`, a step the package
  kind's `extract` runs after its imports, from `Indexed`, as
  `Role::Shape` rows; each source reads its own: `Types::exported`, `Zod::exported`,
  `Literals::exported`). The workspace step `package::shapes`, after `import` in the package
  kind's `workspace_step` (which `Engine::exports` runs), resolves each import as a call through it resolves (the ecosystem's
  `resolutions`, over the `exports` rows, which name constants and types too) and fills it with
  `shape::fill`, the one recursion and the one place its limits live, carrying the
  resolution's level: through one that is not `exact` (an `outDir` mapping) a provider's part is
  `guessed` and a consumer's paths are not declared, so no drift on them is `exact`. A new source that reads
  an exported name adds a `Want`, a reader's `exported` and an arm in `exported_shapes`.
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
- New **language pack**: add it to `PACKS` in `lang/mod.rs`, a fixture
  `tests/fixtures/conformance-<language>/` and one `conform(...)` test in `tests/it/conformance.rs`.
  A pack is done when that suite passes. A pack ships syntax and value evaluation only: no
  table of libraries and no form of one. The forms of its web frameworks, HTTP clients and message
  libraries are separate tickets, each a module of the pack's language in the kind that reads
  them (`http/<language>.rs`, `channel/<language>.rs`). A pack whose facts depend on its config files
  (Rust's default features) also gives `settings` (a string per file path, stored as
  `files.setting` and `Facts::setting`) and takes it in `parse_with`; a file whose setting
  changed is parsed again, whatever its content. Besides `parse` and `modules`, it says what the
  shared resolution cannot guess: `members_in_scope` (bare names may name class members),
  `unordered_globs` (combine wildcard candidates, with an inside glob taking priority),
  `qualified_types` (an unbound path head may name a repository type through its layout),
  `Facts::overloads` and `Facts::constructors` (separate method and constructor families with
  written signatures), `Call::arity` and `Call::constructs`
  (the argument count and constructor call form, kept in cached facts), `Facts::statics` (members a class wildcard
  import takes), `Imported::Qualified` (a fully qualified definition import),
  with Java parameter types spelled from syntax tokens, excluding annotations and comments,
  and `#method` after a class-named method's signature to distinguish it from a constructor,
  `receiver` and `base_receiver` (keywords for the enclosing
  class and its base), `globals` (names bound everywhere), `Def::bases` (a class's bases, for
  its family), `Facts::receivers` (the class each binding gives a name), `Facts::star` (what a
  star import takes), `stars_bind` (whether a star import rebinds names in statement order, as
  Python's does, or only re-exports, as an ES `export *` does) and `Param::binds` (which
  argument binds each parameter; `Binds::Optional`, a Kotlin parameter with a default value, and
  `Binds::Rest` widen the argument counts an overload takes, `Arity` in `Resolver::narrow`). A pack
  whose files resolve with another pack's says so through `LanguagePack::resolves_with` (Kotlin
  with Java): `refresh` keys its resolution groups by `lang::resolver_of`, so one `Resolver` and
  one `Code` built from that pack's `layout` and flags take both packs' files, and calls between
  the two languages resolve by fully qualified name. The joining pack's own flags and `layout` are
  not read, and its files reach the contract kinds through that pack's `Code` (the Java kinds read
  `.java` files alone, but the Spring MVC providers, which read Kotlin too). This keeps one
  resolver per repository and pack group, with no new invariant (ADR 0002). Java's `layout` therefore also maps a Kotlin
  file's top-level overload families and constants by fully qualified name, and a Kotlin class
  joins its own name's overload family with its primary constructor's parameters, so calling the
  class constructs it. A companion object is written as its class's last base: `Resolver::lineage`
  keeps a class's own nested class out of its family, so the companion's bases lend nothing to
  instances, and `Resolver::companion` reads it after the family, for a bare call in the class
  (`inherited`) and a member of the class (`member`, so `Type.create()`); `Resolver::member` finds
  a class nested in a class while that class's family is still being linearized, which resolving
  such a base needs. A bare Kotlin call is `Call::by_path`, as Java's, so it reads the class family
  first, unless a local, parameter, property or constant of its name in a scope around it holds
  what it invokes (`bare_calls`). Java's `layout` gives a package's files before a Kotlin
  top-level declaration of the same fully qualified name, and leaves `.kts` scripts out of every
  package, since a script's declarations are its own (the Kotlin pack exports none of them). A
  Kotlin `object` has an `INSTANCE` property of its own type, which Java calls go through.
  Kotlin writes its receivers as Java does: a property, parameter or local typed by its written
  type (`Class::Type`) or initialized with a constructor call `T(..)` (`Class::Named`), where
  the one binding of its name says so; `Facts::externs` lists the type heads written in the
  file, so a type nothing binds is outside the repository (Opaque outside). An extension
  function or property is a `Facts::extensions` block of its receiver's type, named as the
  function is (`pkg.Receiver.f()`): it tells what `this` is in the extension's body, and its
  functions are no members of the type, so Java calls never reach them. A Kotlin call no member
  answers (`LanguagePack::extension_functions`) reads `Resolver::extended`: the extensions of its
  name in scope of the file, first to last a scope around the call, an explicit import, the
  file's package, a star import (`visible`), narrowed by argument count. On a receiver of a class
  of the repository whose family is all known, the first scope that holds one of a class of the
  family gives the one of the nearest class, `exact`. An extension of a type from outside the
  repository or of a type parameter may apply to any receiver (`Any`, `T`, `CharSequence`), and
  an outside receiver's own members are unknown, so those are name matches over the extensions in
  scope that may apply; a call on a type's name (`Outside.create()`) reaches no extension. A
  lambda passed to `apply`, `run` or `with` is a `Kind::Inline` definition of its own
  (`@<offset>`, folded into the one around it) and an `Impl` block of the receiver's type that
  declares no member (`impls` leaves it out, so a class's lookups never read one per lambda):
  `this` and a bare call there read a local function around the block first, then that type's
  family (`lexical_as` through `blocks`), then the names around it, or stay a name match when
  the block's type is no class of the repository (`unread`). The lambda of `let` or `also` is
  such a scope too, with its parameter (`it`) typed and no block; a lambda in it that declares no
  parameter is a scope of its own, whose `it` hides the typed one only inside it. Any other
  lambda written as an argument (trailing or in parentheses) is a `Kind::Inline` scope with no
  block, as the TypeScript and Rust packs write their closures: the call's argument is its
  `Value::Function`, so a kind can name it (a Ktor route handler), and it folds otherwise. A
  function declared where it is no definition (in a lambda, an object literal or an initializer)
  is such a scope too, so its body is not the code around it.
  `LanguagePack::is_code` drops from the walk the files of a pack's
  extensions that are no code (Kotlin's `build.gradle.kts` and `settings.gradle.kts`, which the
  package ecosystems read as manifests). A pack that
  writes none of them (TypeScript writes no bases or receivers) gets name matches where those
  would have resolved. A language whose files are not all built, or whose imports name files by
  a tree spread over several files (Rust's crates and `mod` declarations), says so through
  three more: `config_files` names with a `**/` prefix (every `Cargo.toml` the walk finds, in
  path order, each read as a root config is (`read_config`) and folded into the configs digest on
  every refresh) and `layout`, which `refresh` calls once per pack and refresh with the configs
  and every file the pack parsed, and which answers five things from one build (Rust's crate
  `Tree`, so the manifests are parsed and the module trees walked once): `Layout::members` (the
  files it builds; the rest keep their facts in `files` with an empty `resolved` and no rows,
  and leave the export), `Layout::modules` (by default the pack's `modules`), `Layout::published`
  and `Layout::programs` (both below), and `Layout::reaches` (default: none, every file), the
  files a name match may reach from a caller (`Resolver::reaching`, called by `refresh`):
  Rust's are its crate's and those of the repository's libraries it depends on. A language whose
  files share one scope (a Go package: the files of one directory that declare one package; Java and Kotlin packages, which share the Java layout; C# packages next) says which through `Layout::shared`, groups of files that
  `Resolver::with_shared` takes: a name a file and its imports leave unbound is looked up
  among the top-level definitions and constants of the others in its group, private ones
  included (`Resolver::sibling`, at the end of `lexical_as`), and an import that names any
  file of a group (`Modules` gives the first candidate, which stands for them all) exports what
  any file of the group that names it in its `Facts::exports` gives (`Resolver::exports`, over
  `file_exports`); a method declared in another file of the group is found for its type too
  (`Resolver::sibling_method`, in `member`); files that define one name apart are `Binding::Several`, an `ambiguous`
  call. The group is built from `Facts::package`, the package each file declares. A member
  lookup (`Resolver::member`) reads the class's own file, then the group's others for a method
  (`sibling_method`). A name a class family declares as an overload family (`Facts::overloads`)
  is read per signature instead (`Resolver::overloaded`): in C3 order, each written signature
  from the first class that declares it, so an override replaces its base's signature and an
  inherited overload of another arity stays; a class past an opaque base that declares the name
  leaves it unknown. A pack with `members_in_scope` reads a bare called name with
  `Reading::Method`, which at each class body takes that family's overloads
  (`Resolver::inherited`) and stops, unknown, at a class whose family is absent or has an
  opaque base and declares none, or that does not declare a name every class has
  (`LanguagePack::implicit_members`); a bare field name reads the family's fields first
  (`Resolver::inherited_field`). Overloads merge across the family, so an opaque base anywhere
  in it keeps them from `exact` (Opaque outside, strict; #353 tracks a relaxation).
  `LanguagePack::unordered_bases` drops the written base list from the C3 merge (Java's
  `implements I, J` with `J extends I`), and a `base_receiver` call reads the overloads of the
  family past the class (`Resolver::base_member`). A call made on a call's result names it
  (`Call::on`), as a local may (`Class::Result`): the receiver is the class the declared result
  of that call's exact target names (`Resolver::result_of`, bounded by `RECEIVER_DEPTH`). A name
  match of a call narrowed by `Call::arity` (`Resolver::matched`) reads the family that owns it:
  a typed receiver, `this` or a bare call match within that family alone, a bare call past an
  opaque base falls back to what its scope binds, and any other receiver takes, from several,
  the one declared in the family of the class around the call other than the caller; a match
  narrowed from several is `Resolution::Narrowed`, stored as `inferred` with `calls.narrowed`. A language whose imports name a path rather than a file (Rust's
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
  library root through `pub mod` declarations, so a binary crate exports nothing.
  `Facts::hidden` lists the methods (`Class.method`) that are no export door although their
  class is one (Go's lower-case methods and interface methods of an exported type):
  `entry::exports` skips them, read as a set per file. A block that adds members to a type written elsewhere (Rust's
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
  calls (`Call::decorates`) resolve to nothing, read only through `Code::outside` with the path flag. A pack that has program entry points fills
  `Facts::programs` in `parse` (it sees the path and the source; default: none) with the
  `Def::name` of each definition that starts a program; each becomes an entry point of kind
  `program`, labelled by its qualified name, and the conformance test lists them. Where the
  entry point is top-level code, `parse` yields a definition for that code (the calls it makes
  belong to it) and names it. A name with no symbol is dropped silently: the pack's conformance
  golden catches a wrong one. A form declared outside the code file (Python `[project.scripts]`,
  which names a function of another file) needs a repository-level step feeding
  `entry::Local::programs` with `entry::Program`s: for Python, the seam's `package::programs` has the
  PyPI adapter map the `Summary::scripts` of each `pyproject.toml` (stored with the manifest
  rows, whose digest makes an edit re-extract) to files through the package's source roots. A form only the whole repository tells
  (Rust `fn main`, a program only in a binary target's root, which one file's `parse` cannot
  see) is `Layout::programs`, which `refresh` takes from each pack's `layout` beside
  the seam's `programs`; Rust asks the crate `Tree` for its binary roots (`Tree::bins`). The forms per language are spec #31 "Program entry points per language
  pack": Python `if __name__ == "__main__":` as symbol `__main__` plus each `[project.scripts]`
  target; Go `func main` in package `main`; Java `public static void main(String[])`; Kotlin a
  top-level `fun main`; C# a `static Main` and, for top-level statements, a symbol `Main`; Swift
  an `@main` type and `main.swift` as symbol `main`; Rust `fn main` in `src/main.rs` and
  `src/bin/`. TS/JS declares none.
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
- The schema version (`index::schema_version`) is derived from `tests/golden/schema.txt`, the
  stored format: `SCHEMA`, the facts of each pack's `tests/fixtures/schema/sample.<extension>`
  and the summaries of the document and manifest samples there. A unit test in `index.rs`
  (`the_stored_format_matches_its_golden`) fails on any change to them until `MEADA_BLESS=1`
  rewrites the golden, which changes the version; no number is kept by hand, so two branches
  that change the format conflict only in the golden. An index with another version, or an
  unreadable one, is rebuilt beside it and renamed over it, and `status` says so. One whose stored facts do not
  parse is rebuilt by the first refresh that reads them (when a file changed), which `status`
  cannot tell.
- The index holds no source text and no values, apart from the argument count and constructor
  form of an overloaded call (`Call::arity`, `Call::constructs`), the place of the call a
  chained call is made on (`Call::on`), whether an `inferred` call was narrowed from several
  definitions of its name (`calls.narrowed`), callable-to-signature and
  static-member names (`Facts::overloads`, `Facts::constructors` and
  `Facts::statics`), the literal URL path fragments and the
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
  `[workspace.dependencies]` entries; the `[workspace.package]` version), and of JVM build files
  (spec #26: literal Maven coordinates and versions, dependency coordinate fragments with `*`
  for computed parts, declared ranges and a `computed` source flag, source roots with the test ones marked, and Gradle's
  literal group, root project name and included project names/directories in `Summary::build`),
  the `tsconfig.json` rows
  (`outDir` and `rootDir`, as written) and the `exports` table (each file's exported names and
  the definition, constant, type or file they name), the `families` table (spec #26: each public
  Java class's ancestors past it, `Resolver::ancestries`, a repository class by file and name and
  an opaque one as `null`), the `entry_points` table (spec #31:
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
  fails, among the files the caller reaches (`Layout::reaches`); an import from outside the
  repository is never inferred. A file its pack leaves out
  (`Layout::members`) resolves nothing and has no symbol, call or `files` row in the export.

## Deployment (`deploy/`)

- `deploy/helm/meada/` renders a module-free, two-node Deployment by default. Its init
  container fetches mirrors outside Meada and indexes its own `emptyDir` before MCP starts.
  `values-generations.yaml` enables the cloud image, an initial pull and pull companion per
  pod, plus a `Forbid` refresh CronJob with separate mirrors and indexes under the refresh lock.
- The chart's ConfigMap owns the workspace manifest. Disabled modules emit no TOML section;
  external store and Valkey endpoints require TLS, credentials come from Secrets or platform
  identity, and the optional Ingress leaves admission and header forwarding to the controller.
- `scripts/helm-check` checks public renders with Helm and parses the resulting TOML. The
  Helm workflow runs it without a cluster; a local kind smoke checks MCP search after install.

## Tests (`tests/`)

- `meada-core/tests/` tests that crate at its public seam, and `cargo test` runs them with the
  binary's (both crates are default members): `ids.rs`, the bundle and generation ids against
  their canonical texts (the cases the `snapshot` module held), `is_id`, `fence`,
  `entry_blocks`, and the workspace version id (its canonical text, the same for the same
  entries however written, changed by a swap, a path or a block); `manifest.rs`, `validate`
  (the entries in order, text errors with file, line and dotted key, every unservable identity
  named); `export.rs`, every `*.export.json` golden read with `export::parse` and written
  again byte for byte, and `parse` refusing a missing field or one of the wrong type, naming it.
  The binary's manifest errors stay in `tests/it/manifest.rs`.
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

- `lang/java.rs`: `DEFINITION_DEPTH` (128) bounds nested named types; deeper definitions and
  their bindings stay with the enclosing symbol. `VALUE_DEPTH` (64) bounds nested values and
  `PATH_DEPTH` (256) bounds callee segments; deeper values or callees are unknown.
  `QUALIFICATION_BYTES` (4 MiB) bounds enclosing-name bytes across syntax visits, before each
  visit's bounded metadata fanout. Pending visits share their scope strings. A file that
  exceeds this budget contributes no facts or package layout, so an incomplete overload
  family cannot produce an exact link; other files still index. The Java hostile-depth and
  long-prefix engine fixtures exceed these limits.

- `lang/kotlin.rs`: `DEFINITION_DEPTH` (128), `VALUE_DEPTH` (64), `PATH_DEPTH` (256) and
  `QUALIFICATION_BYTES` (4 MiB) bound the Kotlin pack as they bound the Java pack's, with the same
  outcome past each; the Kotlin hostile-depth and long-prefix engine fixtures exceed them. The
  budget charges each syntax node the qualified name up to its last named definition, and the
  `.@<offset>` segments of the lambdas and local functions after it only at a node that stores
  the name (a call, a definition, a binding), so nested lambdas cost what the facts keep: a file
  of deeply nested lambdas (a Compose UI, a Ktor routing file) reaches the budget at about 92%
  of the size it did before lambdas were scopes. Beyond
  the limits, what the pack leaves out (spec #27, #95, #96): a bare or `this` call inside a lambda
  other than the five scope functions', an object literal or an enum entry's body may reach an
  implicit receiver no syntax names, so it is read as a name match (`(anonymous).f`), never
  `exact`; so is one in a lambda nested in an `apply`, `run` or `with` block. Those five are
  told by name (a function of the repository called `apply` is not told apart), and a receiver
  is known when it is a name with a written type, a constructor call `T(..)` or an `apply` or
  `also` chain of them; any other (a call's result, a property chain) leaves the lambda as any
  other. A function's result type is not read (`Def::result` is empty), so `make().m()` and a
  local initialized with `make()` are name matches. A typealias is no class of the repository:
  a receiver typed by one is outside it, and resolves to nothing. A name bound twice in one
  scope with different types, or named like a property, holds an unknown type. A call `X.f(..)`
  is read as a constructor only when `f` is capitalized, as Kotlin names classes. An import alias
  of an extension (`import a.slug as s`) brings nothing in scope for it. An extension written in its
  class's own file is named as a member would be (`pkg.Foo.x()`), so a call reaches it as a
  member, a Java call too (the naming of #95). A bare call a top-level function of its name answers is
  exact to that function, though Kotlin prefers an extension of the implicit receiver. A
  companion is
  its class's last base, so `Type.m()` reads the class's own instance member first when both
  declare `m` with one signature. A star import of a class (`import a.C.*`) reads every export of
  the class's file. A fully qualified function call (`example.helper()`) is a name match. A class
  with a secondary constructor is not in its name's overload family, so calling it resolves to
  nothing. A single import of a top-level declaration named like a package of the repository
  (`import app.config` beside package `app.config`) binds nothing, since the package keeps the
  name. A property of function type that a base class declares, invoked by bare name, is read as
  a method call. A companion's outside base leaves the companion's members unknown, so
  `Type.create()` then resolves to nothing (Opaque outside, strict, as Java's #353). Kotlin files
  resolve with Java's flags, so `java.lang.Object`'s members are the implicit members of a Kotlin
  class too: a bare `notify()` in a class is unknown. A star import of a Java package from Kotlin sees its
  package-private classes. The Kotlin pack lists the same Spring configuration files as the Java
  pack (`jvm::spring::CONFIG_FILES`, spec #27, #100), so a Kotlin-only repository gets them
  through `Indexed::configs` (a mixed one lists each twice, which `Configs::read` takes as once).
  A declaration the grammar reads as an
infix call (a hard keyword as an operand, `misparsed`) leaves the whole file out, as the
qualification budget does (ADR 0017).

- `snapshot/pull.rs`: a generation's bundles hold at most `GENERATION_BYTES_MAX` (256 GiB)
  together, as the document records their sizes (checked before any download) and as the
  regular files extracted from them add up; at most `GENERATION_ENTRIES_MAX` (4,000,000) archive
  entries together; and each GNU long name or link, or pax header, at most
  `HEADER_EXTENSION_MAX` (64 KiB), which a scan of the headers checks before the tar crate,
  which reads them into memory whole, sees the archive. Past any of them the pull fails and
  changes nothing. A symbolic link whose target goes through another symbolic link is refused
  even when both stay inside the mirror. A sparse file is refused. The node pointer file holds at
  most `POINTER_MAX` (64 bytes); past it the pointer file is malformed.
- `snapshot/generations.rs`: `{<key_prefix>}:kept` holds at most `KEPT_MAX` (8) generations,
  since every node holds every retained generation on disk: `pointer retain` refuses past it,
  and a pull refuses a retained list longer than `keep` plus `KEPT_MAX`, naming the key and
  changing nothing.
- `snapshot.rs`: a generation document holds at most `DOCUMENT_MAX` (16 MiB), in the store and
  as a node holds it, and the store's `current` at most `CURRENT_MAX` (64 bytes); past either
  the object is refused, naming its key or file, and a pull changes nothing.
- `snapshot/store.rs`: a credential file (key id, secret, token) holds at most `SECRET_MAX`
  (16 KiB), and `ca_file` at most `CA_MAX` (1 MiB); past either the file is refused, naming it.
  An empty CA file or invalid certificate is also refused before connecting.
- `valkey.rs`: `password_file` holds at most `PASSWORD_MAX` (4096 bytes) and `ca_file` at most
  `CA_MAX` (1 MiB); past either the connection fails, naming the file.
- `oidc.rs`: each metadata and JWKS request takes at most `FETCH_TIMEOUT` (10 seconds), a
  metadata document at most `METADATA_MAX` (64 KiB) and a JWKS at most `JWKS_MAX` (256 KiB);
  past any of them the request fails (at start, the start is refused; on a refresh, the cached
  keys stay). An identity whose path starts with `.well-known/oauth-protected-resource` is
  unreachable with `[oidc]`, since that path serves the metadata. The `scope` claim is read as a
  space-separated string; `scp` and arrays are not read. With `audience` unset, the root's 404
  takes a token whose `aud` is `public_url` itself. The caps are inclusive: a body of exactly
  the cap is read. The `oidc` build carries `rsa`, whose RUSTSEC-2023-0071 (Marvin timing) hits
  private-key operations; Meada only verifies with public keys, so it does not apply.
- `github_app.rs`: each REST request takes at most `REQUEST_TIMEOUT` (10 seconds) and reads at
  most `ANSWER_MAX` (4 MiB); an App's installations are listed over at most `PAGES_MAX` pages of
  100 (10,000 installations), the rest unseen; a `private_key_file` holds at most `KEY_MAX` (16
  KiB) and git's request at most `INPUT_MAX` (64 KiB). The time margins are fixed, not
  configured: `TOKEN_MARGIN` and `REFRESH_EVERY` (10 minutes each), the JWT's `JWT_LIFE` (9
  minutes) and `JWT_BACKDATE` (60 s), `INSTALLATIONS_TTL` (the listed installations stay usable
  an hour while listing fails), `LAST_TTL` (`throttled` finds the installation of a `get` within
  an hour), `LOCK_TTL` (30 s, a listing or mint lock and the wait for another run's) and
  `THROTTLE_MAX` (a mark lasts at most an hour). A lock that expires under its holder lets a
  second run list or mint too. The `github-app` build signs with RustCrypto's `rsa`, whose
  RUSTSEC-2023-0071 (Marvin timing) applies to private-key operations; the JWT is signed in the
  job's process, at most once per App per run, where no remote party times it.
- `mcp.rs`: the stop signal drains for at most `DRAIN` (25 seconds, no flag, under Kubernetes'
  30-second default grace period); past it the connections left are dropped, an answer still
  waiting on an index lock included, and the exit is 0.
- `ui.rs`: `meada-ui.json` is read up to `VERSION_MAX` (4 KiB); a larger file counts as front
  end files built for another version, and the console refuses to start. A token check's body
  is read up to `CHECK_MAX` (16 KiB), and a playground query's up to `QUERY_MAX` (16 KiB); past
  either the answer is a 413. Without `[snapshot]` the playground and the exports serve the
  `[[repository]]` entries as read at start (a mirror missing then stays unservable until a
  restart, as on a node). Operational service reads and managed-workspace token checks share
  a two-second request bound, including the wait for one of `READERS_MAX` (4) workers;
  timed-out workers retain their slots until I/O completes, so overload starts no additional
  work. `index.rs` reads optional `index.commit.json` up to
  `PROVENANCE_MAX` (4 KiB); anything larger has no trusted indexed commit.
- `ui/write.rs`: `POINTER_MAX`, `FENCE_MAX` and `APPLY_MAX` bound their respective POST bodies
  to 1 KiB; `SAVE_MAX` bounds a version save's JSON body to 4 MiB. Over-cap authorized uploads
  get 413 before any operation; unauthorized uploads are never consumed.
- `snapshot/versions.rs`: a workspace version document holds at most `VERSION_MAX` (4 MiB),
  checked both before immutable creation and while reading;
  past it the document is refused (on import, apply and pull, and read from
  `<index_dir>/workspace/`) or listed as malformed, naming its key or file.
- `render.rs`: a source file an answer reads (`render::source`, for `path:line` and a symbol's
  source) is read only through plain relative parts of its path, through no symbolic link, as a
  regular file of at most `SOURCE_MAX` (16 MiB); anything else renders as unavailable, the path
  without its line, as a missing file does. An answer keeps each file it reads, so several
  large files add up.
- `engine.rs`: one connection attaches every peer, so a workspace reads at most 10 peers
  (`MAX_PEERS`, SQLite's attach limit). `search` scans every symbol.
- `index.rs`: every contract is extracted again and every call resolved again on any change
  (rows rewritten only when they differ). An unreadable `contracts.fragments` value is read as
  no fragments, since only `refresh` writes it. A manifest or pack config file (`package.json`,
  `pyproject.toml`, `requirements*.txt`, `Cargo.toml`, `go.mod`, `tsconfig.json`, `jsconfig.json`) is read up to one
  byte past `lang::MAX_BYTES`, and a provider document up to one byte past
  `document::MAX_BYTES` (`read_up_to`), which their readers take as invalid whatever the rest
  holds, so the `manifests` and `documents` hashes and the config digest are of what was read. A
  pack config, at the root or at any depth (`**/`), is read through no symbolic link
  (`read_config`), as the walk follows none.
- `package/go.rs`: a `replace` applies to every version of the module it names (its old version is
  read and dropped, as `lang/go/modfile.rs` does for imports); a `go.work` is not read; a `vendor/`
  directory and an `internal/` package are no different from any other directory of the provider;
  a `replace` to an absolute directory leaves the module outside.
- `workspace.rs`: a sibling with an invalid `meada.toml` or `meada.decisions.toml` is skipped
  silently. In a pulled generation, a mirror's `meada.toml` and `meada.decisions.toml` are read
  only as regular files of at most `GENERATION_CONFIG_MAX` (1 MiB, `config.rs`); past it the
  generation is not servable, the error naming the file.
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
  makes the names it may override unknown. `WRAPPER_DEPTH` (64): a wrapper more calls up than
  that is read where the climb stops, its parameters unknown. `EVAL_BUDGET` (2^16 per file):
  folding values (`Code::expand`, `Code::value`) and tracing them to their origins
  (`Code::origins`) charge each step to the file whose code it reads (a value folded, a part
  copied, a call a wrapper climb reaches, a constant, parameter or return value followed, a
  composite literal `Code::field_values` reads, past which the field's value is open);
  past it, that file's values are unknown, the climb stops at its calls and no origin is found
  through it, so a string doubled through forty constants (`const b = a + a`), or a lattice of
  functions each calling or returning the next twice, ends. `EVAL_DEPTH` (256): a value
  folded deeper than that, through the items of lists and objects, the constants it names and
  the arguments callers pass, is unknown, and a value traced through more constants,
  parameters and return values than that, each into the next, has no origin.
  `FAMILY_DEPTH` (128): a class family (the class and its ancestors in C3 order) longer than
  that is absent, so every member of the class is unknown (a name match at most), and the
  linearization recurses no deeper down the bases. The bases count from where the walk
  started, a family looked up for a member a base names (`class Z(K.D)` linearizes `K`'s)
  included.
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
- `lang/go/values.rs`: `VALUE_DEPTH` (64): a value (an argument, a returned or held expression)
  nested deeper (`f(g(h(..)))`) is unknown. A name is a constant only when it is bound once in
  the function (`:=` or `var`, one name, never assigned, in a statement of the function body, not
  in a block, an `if` or a loop), so a name bound again, assigned or shadowed holds no value
  (the `Hidden` binding wins); a function returns a value only for a `return` of one expression.
  A name whose address is taken (`&p`) is no constant, except `&p` as the `Topic:` of a
  `TopicPartition` literal (confluent-kafka-go's `*string`; `ponytail:` in `Walk::body`,
  `Keep::Pointer`), unless the function, or one around or inside it, writes through a
  dereference (`*x = ..`, `*x.Topic = ..`): no alias analysis, so `*q = ..` of another pointer
  counts too. A struct literal nests as deep as `VALUE_DEPTH`; one written by position, a map and
  a literal of any other type are `Unknown`. A name bound in a block or an `if` hides
  the name in the whole function for constants (absence only: a router or path written outside
  the block after such a binding yields no provider; the receivers of the pack hold a name for a
  region, constants do not).
- `channel/go.rs` (spec #25, #82): a receiver that does not trace back to a library's
  constructor or type makes no side (a type of the repository with a `Publish` or
  `WriteMessages` method is never one). A confluent `Produce` takes the topic from
  `TopicPartition.Topic` of the message literal written in the call or held by one name; a
  message built elsewhere is unresolved. A topic starting with `^` is a pattern and names no
  topic. A Redis read without a consumer group (`XRead`) is no consumer, and `XReadGroup` gives
  the first half of `Streams` as the names (the ids are the second). A sarama `Consume` handler
  is found when its third argument is a literal or a name that holds one; any other leaves the
  consumer on the function that calls `Consume`. A queue name is the last segment of the SQS
  queue URL.
- `channel/java.rs` (spec #26, #92): a client counts only by the type the source writes for its
  receiver: one a call returns in place (`connection.sync().xadd(..)`), an interface the table
  does not list (`KafkaOperations`, `AmqpTemplate`, `RedisStreamCommands`) and a `var` with no
  readable type make no side. A listener annotation on a class (`@KafkaHandler`,
  `@RabbitHandler` methods), a composed annotation, `@KafkaListeners`, `topicPattern` and an
  `id`-only listener read no name (one unresolved consumer when the annotation is on a method).
  A placeholder that resolves to a comma-separated list stays one name. A `RabbitTemplate`
  whose exchange or routing key the code sets (`setExchange`, `setRoutingKey`) is not read:
  only the module's `spring.rabbitmq.template.exchange` replaces the default exchange; the
  one-argument forms and a JMS default destination are unresolved. `BindingBuilder` is read
  only as `bind(q).to(e).with(key)` with the queue and the exchange written as a `new Queue`,
  `new TopicExchange` or `new DirectExchange` (through constants and `final` fields); a `@Bean`
  method's parameter, `QueueBuilder`, `ExchangeBuilder` and a fanout `to(..)` bind nothing, and
  neither does a `@QueueBinding` without a key. An SQS request built in a lambda
  (`sendMessage(b -> b.queueUrl(..))`) or held in a local, and Redis streams held in a map
  variable, are unresolved. The same file reads Kotlin (see Kotlin channels above), with the
  same limits.
- `http/java.rs` (spec #26, #86): a Spring MVC route is read from `Call::decorates` on a method
  of a `@RestController` or `@Controller` class, under the class's `@RequestMapping` prefix
  (nested classes do not inherit the outer prefix). An annotation or `RequestMethod.X` counts
  only when `Code::outside` says an import from Spring binds its name: single-type, static, or
  the one on-demand candidate in the kind's table (`Outside::candidates`: `...bind.annotation.*`
  alone or beside `java.util.*`). A type of the repository with the same name (a class, an
  interface or an annotation type, which the Java pack indexes as an interface; a nested,
  same-package or repository-wildcard `GetMapping` or `RequestMethod`), a single-type import of
  another package and two candidates in the table yield no route; the kind reads the enum
  member off the written `Value::Ref`, with no table in the pack. A path folds constants and
  `+` through `Code::value`; one that does not fold, a `value` with a `path`, a class-level
  `method`, a `method` with no readable element, or a pattern with a `/` inside braces
  (`{id:a/b}`) serves nothing. An unreadable element beside readable ones of a `method` or a
  path array is dropped: the others still serve. Interface-declared mappings, composed annotations
  and `server.servlet.context-path` are not read.
  Kotlin (spec #27, #97): the Spring reader reads Kotlin files too, through `jvm_files` (`.java`,
  `.kt`, `.kts`, in the shared `Code`), with the same annotations and rules: the Kotlin pack
  writes `["/a", "/b"]` as a `Value::List` and `method = [RequestMethod.X]` as a keyword list, and
  a bare constant of a class's companion folds (`Resolver::inherited_field` reads the companion
  after the family, unless its own bases are unknown: a ceiling, since a same-named name outside
  then wins over the companion's inherited member). Several unnamed paths (`@GetMapping("/a",
  "/b")`) fill the `value` array, and `arrayOf(..)` counts as `[..]` in a path or a `method`
  when `known` says the name is `kotlin.arrayOf`. The pack writes a star import as an `inside`
  glob (it sees `internal` members), so `Resolver::on_demand` takes the outside source of an
  `inside` glob too (never the repository's names), and an outside star import
  (`...bind.annotation.*`) gives `known` its candidate, as Java's does.
  JAX-RS and Feign keep `java_files`; the `RestTemplate`, `WebClient`, OkHttp and Retrofit readers
  take `jvm_files` (spec #27, #101): Kotlin writes the same clients without `new`
  (`RestTemplate()`, `Request.Builder()`, a `Call::receiver` of `Retrofit` and no `Call::on`),
  so the `uri` and `build` steps of `clients.rs` match a chain step spelled
  `(call_expression).name`, `constructs` takes a `RestTemplate(..)` call for the written type, and
  `made` falls back, after typed and origin lookups, to `jvm::names::local` (Kotlin's
  `Facts::bound`, shared with the Ktor reader) up to `LOCAL_DEPTH`. The pack binds every local
  `val` with an initializer, whatever its class (a factory call `WebClient.create(..)` or a
  `.build()` has none), and drops the bound of one whose name a loop variable or a `catch`
  parameter also declares in the scope; `local` then does not take the `val`'s own entry in
  `Facts::locals` for another binding (a typed one still has its receiver). A client held by such a
  local gets its base from `folded_local` (`http/java.rs`): the call it is bound to, folded by
  `Expand::Chain` where it is made, replaces the evaluated `Ref` that `sends` (`Client::Web`) and
  `declarative::base` would read as an unknown base. The Kotlin pack reads `X::class.java` as
  `Value::Member(X, "class")`, the Java class literal, and gives a call's one type argument
  (`create<Api>()`) as the same member under the keyword `lang::kotlin::TYPE_ARGUMENT`, which no
  name matches; `retrofit_clients` takes it for `create` only when `retrofit2.create` is what
  `known` says the name is. In `declarative.rs`, `builds`/`base` take the Kotlin constructor
  `Retrofit.Builder` as the chain start, and the bases of a `.kt` file expand with Spring's
  `@Value` inject (Java Retrofit bases stay as they were).
  JAX-RS (#87, `jax_rs` in the same file): a method with a verb annotation (`GET` ... `OPTIONS`)
  of a class with `@Path`, from `javax.ws.rs` or `jakarta.ws.rs`, joined to the method's own
  `@Path`; both packages through the same `known`, so `import javax.ws.rs.*` beside
  `jakarta.ws.rs.*` is two candidates and serves nothing. A `@Path`-only method and
  `@ApplicationPath` are not routes, and neither is a method of an interface (`Facts::interfaces`):
  a `@Path` interface is a MicroProfile or Quarkus client. A controller or resource class whose
  family has an outside base (`extends com.corp.Base`, top-level or nested) gives no provider
  (#354), since its annotations are read in the class scope, where an inherited member type
  could shadow the import. A fully qualified annotation without an import (`@javax.ws.rs.GET`)
  and annotations an implementation inherits from an interface are not read.
- `http/kotlin.rs` (spec #27, #98): Ktor routes, through the same `Mounts` and `prefixes` as
  the other routers, whose nodes are definitions (file, qualified name): the lambda of each
  `routing { }` call (a root), the lambda of each `route(prefix) { }` call (mounted under its
  prefix on the routing scope around the call) and each extension function on Ktor's `Route` or
  `Routing` (mounted with no segment on each routing scope that calls it through the implicit
  receiver, so one called under two prefixes serves under both; one nothing calls is served
  nowhere). The routing scope of a call is the nearest such node around it through lambdas
  passed as arguments (`Kind::Inline` scopes inside a call of the scope around them, by the
  calls' and definitions' ranges), stopping at a route's handler and at a local function's
  body. `get`, `post`, `put`, `patch`, `delete`,
  `head` and `options` with a path or none (the block's path), and `route(path, HttpMethod.X)`,
  register a route whose handler is the trailing lambda (`handler_site`); the path and the method
  are read positionally or by name (`path =`, `method =`). The extension called
  is the nearest one in scope (`Code::extensions`, Kotlin's lookup order: around the call, an
  explicit import, the package, a star import, after the locals and parameters that hide the
  name) whose receiver is Ktor's `Route`, or `Routing` in a scope whose receiver is one:
  the routing block tells the implicit receiver, which the resolver's own answer for the call
  (a name match inside a lambda) does not know. A Ktor name counts only when `Code::outside`
  says an import from `io.ktor.server.routing` (or Ktor 1's `io.ktor.routing`) binds it, and
  `HttpMethod` from `io.ktor.http`; a name a star import of `io.ktor.server.resources` may bind
  too is no route, so type-safe routing is not read. Ktor parameters (`{id}`, `{id?}`, `{...}`)
  fold as actix-web's (`actix_path`).
- `http/kotlin/client.rs` (spec #27, #99): Ktor client calls (`get`, `post`, `put`, `patch`,
  `delete`, `head`, `options` and `request` of `io.ktor.client.request`, counted by the import
  `Code::outside` says binds the verb) on a receiver that is an `io.ktor.client.HttpClient`: one
  `Code::origins` (`Follow::Values`) traces to a call to it (counted by the import of its
  callee as `jvm::names::spellings` reads it, so `import io.ktor.client.*` counts), a local the
  pack recorded (`Facts::bound`: a `val x = Callee(..)` with no written type, and the parameter
  of a `let`, `also` or `use` lambda, whose value is the receiver; `Clients::local` takes the
  nearest binding of the name and none when another binding of it shares its scope, so a
  parameter, a loop variable, a `var`, a local named like a property of the file or one declared
  twice hides it) followed through `origins` again, or one whose written type is it
  (`jvm::names::typed`); a map, another library's client and a name a local hides are none. The
  call is read whole (`Expand::Chain`, which also gives the reader the call's keyword arguments),
  so an enum member a wrapper passes keeps the evidence of the caller's imports. The URL (first
  argument, or `urlString`) goes through `Code::expand`, as the Go clients do, so a wrapper is
  read at each call site of the function that determines the URL or the method, through any
  number of levels, with cycle detection; a call with no URL argument (`get { url { .. } }`) is
  an unresolved consumer. The base of a client made by a call is the `url(..)` of the
  `defaultRequest { }` in the configuration lambda of that call (Ktor 2 and 3
  `io.ktor.client.plugins.defaultRequest`, Ktor 1 `io.ktor.client.features.defaultRequest`),
  folded in its scope: a hole when it is set more than once or not by a string, none for a client
  known by its type alone. `request` sends the method its block assigns: the pack records
  `method = v` and `this.method = v` written as a statement of a lambda argument as the
  `method=` key of that call's `Call::kwargs` (a key no Kotlin name matches; `given` in
  `http/kotlin.rs` ignores keys ending in `=`), `Unknown` where the statement may not run (inside
  an `if`, `when`, `try` or loop) and, for the call that owns each lambda around a nested
  assignment (`headers { method = X }`), because the inner lambda may not run, and when the block
  assigns it twice. `GET` when it assigns none and the call is given no other argument, a `Ref`
  while it depends on a parameter of a wrapper, unknown for anything else (a block passed as a
  value, `HttpMethod.parse(..)`, a method that is no `HttpMethod` member of Ktor).
  The Kotlin pack folds a string literal's templates into parts (`$name`, `${name}` as names,
  any other `${expr}` as a hole); the grammar reads `$name` as text, so the pack finds the name
  in the text. A literal with an escape other than `\$` is unknown; `\$` is a `$` that starts no
  template, so Spring's `@Value("\${key}")` is the text `${key}` (#100).
  Base URLs (spec #27, #100): the client reader expands with `Code::expand_with` and
  `jvm::spring::inject` (as `http/java/clients.rs`, `Configs` read the first time an annotation
  is injected). The Kotlin pack records `Facts::annotated` for a property (a field of its class,
  `static` at the top level or in an object or a companion, where Spring injects nothing), a
  constructor parameter (`Marked::Param` of the class, and the `Marked::Field` of the property
  a `val` or `var` declares, whose `Const` is dropped when annotated, as a `lateinit var` has
  none, so the annotation is what it holds) and a function's parameter, by use-site target: none,
  `field`, `set` and `setparam` for a property, none and `param` for a parameter; `get`,
  `property` and the others inject nothing. Every assignment to a name (`base = ..`,
  `this.base += ..`, `o.base = ..`) is a `.name` constant, as Java's, which `Code::field_written`
  reads: a field some code assigns holds no injected value. `value()` reads
  `System.getenv("X")`, `java.lang.System.getenv("X")` and `System.getenv()["X"]` as `Part::Env`,
  through `!!`, parentheses, `?:` fallbacks and the string methods of `STRING_METHODS`
  (`trimEnd`, `removeSuffix`, ...), unless the file binds `System` to another type
  (`SYSTEM_BOUND`, as Java's); each node is read once, a string method passing on the
  `Part::Env` its receiver already evaluated to. A fallback that reads the environment too (a
  `System.getenv` call, or a property of the file whose declaration holds one, `ENV_NAMES`) makes
  the term a hole: exactly one read. Adjacent literal parts of a `+` join, so `"\$" + "{key}"`
  is a placeholder. The variable maps through `base_url_env` as in the trunk. A function-local
  `val` is no constant in the Kotlin pack, so the read must stand in a property or the URL
  expression.
- `http/java/clients.rs` (spec #26, #88): a RestTemplate or a WebClient counts by the type the
  source writes for the receiver (`Follow::Declarations`), or, for a WebClient, by the
  `WebClient.create` or `WebClient.builder()` chain it traces to (`Follow::Values`); a name
  imported on demand counts through its one candidate in the kind's table (`Outside::candidates`,
  now also for a head only the file's outside names bind). A WebClient's base is read off the
  folded chain (`Expand::Chain`), through constants, `final` fields and a wrapper parameter's
  arguments; a client held in a local has an unknown base, so the consumer links by path. The
  Java pack keeps a `final` instance field's one value (its initializer, or the single `this.f =
  v` or `f = v` of all its constructors; two give none) as a `Const` of its class, outside
  `statics`, a constructor's value read in that constructor (`Const::scope`): its parameters
  are what Spring injects (#89), else unknown. A builder
  (`HttpRequest.newBuilder()`, `new Request.Builder()`) is read only as one chain ending in
  `build()`; one held in a variable between steps is no consumer, and a chain folded short of
  its start names no method. `String.format` puts each `%s` or `%d` argument in place (a
  parameter keeps the wrapper climbing, anything else is a hole) and reads `%%`; any other
  conversion leaves the URL unread. A `HttpMethod.X` names a method only when one spelling
  (`Outside`) is Spring's.
  Bases from configuration (#89): the reader expands with `jvm::spring::inject`, so a field or a
  constructor parameter with Spring's `@Value("${key}")` (bound by an import, as for the
  annotations of #86) holds `key` from the `application.properties`, `application.yml` or
  `application.yaml` of its module's `src/main/resources`, in that order of precedence (a file
  past `lang::MAX_BYTES` is not read, nor a `.properties` past its first `#---`). A source's
  module is that of the deepest main source root a Maven or Gradle build declares for it
  (`Indexed::manifests`, a Gradle source set's `srcDirs` included), else the one whose
  `<module>/src/main/` it is under. The pack records every annotated field with whether it is
  `static`, and `spring::inject` gives a `static` one nothing; a field some file assigns, and
  one a parameter, a local or a same-name binding hides, hold nothing too. A placeholder no key fills is the variable it
  names (`Part::Env`), and `${X:default}` keeps both (`Part::EnvOr`): `http::fetches` reads
  such a URL through `X` and, when the default names a host, through that host too, the twins
  marked in their `via` (`env:X|host:<host>` and `host:<host>|default`). `Http::workspace_step`
  (`keep_applicable`), after every `declare` filled `Provider::hosts`, keeps the one that
  applies, its mark dropped: the default's when a provider of the workspace serves its host,
  else the variable's. Links, `context`, `trace` and the open items see one consumer per call.
  The Java pack reads `System.getenv("X")` (or `java.lang.System.getenv`) as `Part::Env`,
  unless the file imports or may declare another `System` (`binds_system`, a word after
  `class` in a comment counts; `import java.lang.System;` does not). Profiles, relaxed binding,
  YAML lists and a SpEL `#{...}` value are not read. The configurations are read once per
  extraction, the first time an annotation is injected.
- `http/java/declarative.rs` (spec #26, #90): a Feign or Retrofit interface method is a consumer
  sited at the method (`Site::Def`, built by `http::consumer_at`), not at a call: its callers
  reach it as any call to a definition. Feign: `@FeignClient` from
  `org.springframework.cloud.openfeign` (bound as the Spring annotations of #86, through `known`)
  on an interface; `url` (empty is none; without a scheme it gets `http://`, as Spring Cloud does) and `path` resolve with `jvm::spring::resolved`
  over the module's configuration, the methods' mappings are `request_methods` and `paths` of
  `http/java.rs`, a `@RequestMapping` with no `method` meaning GET, as Feign sends it. Without
  `url` the consumer's `via` is `client:Feign client <name>`: `http::CLIENT_VIA` is the prefix of
  a client that names itself, the rest the words the reader writes, and `Http::link` words the
  unresolved host as `host unresolved (<words>)`, so the shared code names no library; a name with
  `|` is refused, it would collide with the twin marks. A `@RequestMapping` on the interface,
  `@RequestLine` and other contracts give nothing. Retrofit: the verbs of `retrofit2.http`
  (their names collide with JAX-RS, so `known` accepts both spellings and the result must be
  Retrofit's); the base is the folded `Retrofit.Builder().baseUrl(..)` chain of each
  `retrofit.create(Api.class)` (`Expand::Chain`, climbing wrapper callers while the base holds a
  `Part::Ref`), the receiver proved Retrofit by its type or by its `new Retrofit.Builder()`
  chain; one consumer per creation, none gives an unknown base. Its annotations name the path by
  `value` alone, and a route with a brace open past its segment is not read (`balanced`, as for
  Spring, JAX-RS and Feign). A route from `/` replaces the
  base's path (read over the base's leading text parts, so `HOST + "/v1/"` counts), and an empty
  route is Retrofit's dynamic `@Url`: a consumer with no contract. A consumer at a method is an
  open item like a call's (`items::list`): its location is the method, its expression the method
  on one line, and its enclosing symbol the method itself, so `meada decide` sends the method's
  lines as the minimal excerpt (ADR 0006). The Java pack reads `Api.class` as `Value::Member("Api", "class")`.
  `ContractKind::calls_from_definition` (true for HTTP) tells `trace` that a consumer at a
  symbol is a call to follow, unlike a channel handler, where the link ends.
- `http/go.rs` (spec #25): a route is read only with a literal path and a literal prefix; a
  group prefix or a pattern Meada cannot read serves nothing. `net/http` reads `{name}`,
  `{name...}` and `{$}` alone: a `:name` segment is a literal no contract can spell, so its route
  has no provider, and `*` stays literal; gin and echo read `:name` and `*rest` as parameters.
  A `ServeMux` is mounted only as the handler of a `Handle(pattern, mux)` (or a function
  returning it) on a router Meada reads; it keeps its own paths, served for the pattern's
  subtree (a trailing `/` or `{name...}`, else the exact path) and method alone, so a route
  outside the pattern, or of another method, has no provider. A mux held by any other argument of
  a route call (`http.StripPrefix`, `echo.WrapHandler`, `gin.WrapH`, any middleware), or given
  to a router held in a field, a map or a slice, is an unread mount: served nowhere. A mux
  nested deeper than `PREFIX_DEPTH` or through more mounts than `PREFIX_WORK` is served nowhere
  too. `http.StripPrefix` and other middleware that rewrites paths are not modelled (spec #25).
  Middleware (`Use`, extra handlers) has no effect on a gin or echo path. A router held in a
  struct field, a map or a slice is not followed (no marker).
  chi: `*` is a parameter segment, `:name` a literal no contract can spell (no provider);
  `Mount(prefix, router)` follows the router like a group, any router inside a call of the
  handler argument (a middleware), and a `ServeMux` given to `Mount`, are unread mounts (chi's
  `Mount` leaves the request path alone, so the mux would see its full path under the mount's
  prefix: combining those chains with the router's prefixes is not modelled); `With(..)` is not
  a group. A `Route` or `Group` function binds its first parameter to the group of that call,
  and the group is followed from there through exact calls (`users(r)`) and names bound once
  (`sub := r`), at most `PREFIX_DEPTH` hops and `PREFIX_WORK` parameters (past it the group is
  served nowhere); a call found only by a name passes nothing on, and a parameter
  rebound anywhere in the function hides it in the whole function. gorilla: `.Methods(..)`
  restricts the call it is chained on through `Name`, `Schemes`, `Headers`, `Queries` and
  `Host`; arguments Meada cannot read, no argument and a second `Methods` serve nothing;
  `Path(..).HandlerFunc`, `NewRoute`, `Host` subrouters and `StrictSlash(..)` chains are not
  read (a router made by a call that is not `NewRouter` has no provider).
- `http/go/clients.rs` (spec #25, #79): `CLIENT_DEPTH` (64): a client or a request is followed
  back through that many calls, names and struct fields (`New().SetX().R().SetY()`, `b := a`,
  `c.rc`); past it the base is unknown and the request is no consumer (the chain is no longer
  known to be resty's). A request on a client only written as a type (a parameter, or a field
  no composite literal sets) is a consumer with no host, and so is one on a parameter some
  caller passes an untraced client to (the base is unknown). A field's literals are those of
  its type: a literal of an alias or a defined type (`type B = T`, `type B T`) written by
  position, and a conversion from another struct type (`T(other)`), are not seen as setting
  the fields of `T` (`Code::field_values`), and a literal nested deeper than `VALUE_DEPTH`
  inside literals written without their type sets the fields of a type not read. Only the
  libraries of the table (`net/http`, resty v2 and v3, `resty.dev`) are
  clients; a client assigned more than once, one held in a map, a slice or a channel, or an
  interface, is untraced and its requests are no consumer. A `fmt.Sprintf` format with a verb
  other than `%s`, `%d` and `%v`, a count of verbs and values that differ, a `var` assigned
  anywhere but its declaration (or written through a selector), fold to nothing: the consumer is
  unresolved (no marker). A verb with flags, width or precision is a hole, and so is `%d` of
  anything but an integer literal, and `%s` of an integer literal. `values::Std` folds
  `fmt`, `net/http`, `os`, `strings` and `aws` only where nothing else in the file spells the
  name they are imported by (`hidden`: a parameter, a local or a function of that name in any
  function of the file leaves every call of the package in the file unfolded, whatever its
  scope).
- `lang/go.rs`: `PATH_DEPTH` (256): a callee keeps that many selectors of a chain (a longer
  one is its node kind) and a receiver's type is read through fewer `*`, type arguments and
  parentheses (past it the method is no definition, and a parameter, field, `var` or result
  type read through more names no class). Receivers (`Walk::bound`, `finish`): a name is a
  `Facts::receivers` row only when every binding of it in its function (parameters, the method
  receiver, named results, `var`, `:=`) writes the same class, one scope per function and per
  literal, so a name bound again to another type (in any block) or by `range`, a type switch or
  a second result is a local that hides, and so is a name whose type a local of the function
  hides (a local `type`, a type parameter; a type declaration's own parameters hide only in
  the declaration, not in its methods). A `:=` or `var` holds from the end of its statement to
  the end of its block, clause or statement, and the name is a receiver only if every call that
  starts at the name in its function (and in the literals in it) lies inside the regions of its
  bindings; a call before it, or after the block, may mean an outer name, so the name stays a
  hiding local. A struct's named fields are receivers in the type's
  scope; an embedded field, and so a promoted method, is not followed (a name match).
  `Class::Called` is a call read as a value (`h.Counter()`, `shapes.NewCircle()`), the first
  result only. `DEFINITION_DEPTH` (128): a function
  literal nested deeper is no definition (its parameters and calls are the enclosing one's).
  `CONSTRAINT_DEPTH` (64, `lang/go/build.rs`): a `//go:build` expression nested deeper (`!`,
  parentheses) is not read, and the file is left out, as one that cannot be read (two
  `//go:build` lines, not UTF-8) is. Build constraints hold for one target (`linux`, `amd64`,
  `unix`, `gc`, every `go1.N`; `cgo` and any other tag are false, and a file that imports `"C"`
  is left out); a legacy `// +build` line
  alone is not read. A plain import binds the name the Go tools assume for its path
  (`assumed_name`: the last element, the one before a `vN`, without `go-`, up to the first
  character no identifier holds), not the package clause of the directory it names, so a
  package whose clause differs from its directory is reached by its alias alone, and a name two
  imports of a file give is unknown (`unbind_clashes`: an import of it from the empty
  specifier, which names nothing), so a call through it links to nothing. An import of two
  modules of the repository that declare one path names nothing unless a `replace` picks one; a
  `replace` by a module or a directory outside the repository makes the replaced module an
  outside one; a `vendor/` directory is not followed. A `go.mod` past `lang::MAX_BYTES`
  (read up to one byte past it, as every pack config is, so its last line may be cut) declares no
  module for the imports either (`modfile::Modules::new`).
- `lang/rust.rs` (types): `written` and `parameter` read a type and climb to the items that
  declare a type parameter within `PATH_DEPTH` (256) steps (references, `A + B + 'c` bounds and
  ancestors); past it a type names nothing and a parameter is taken as declared with no bound,
  so no repository type is guessed. The walk in `parse` carries each node's depth and the items
  around it, so `parameter` asks for no parent (each lookup walks down from the root of the
  tree).
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
- `resolve.rs` (`Code::origins` with `Follow::Constants`): only `const` bindings with a plain name are followed; a client
  held by `let`, a class field, `this.x`, a destructuring or a parameter, or chained without a
  binding (`new Kafka().producer().send`), is not recognized (no marker).
- `channel/ts.rs`: a kafkajs handler passed through a wrapper's parameter is not followed
  (the function that subscribes handles); only the first `run` on a consumer binding counts. An
  amqplib `bindQueue` whose queue, exchange or pattern is not a literal is dropped.
  A BullMQ `Queue` is named by its own `new Queue(name)` binding: `new Queue(name).add(...)` with
  no binding, a `Queue` that is a class field or a parameter, and `addBulk` are not recognized. A
  node-redis client made as `await createClient().connect()` is not traced (the `connect` chain
  has no binding); a BullMQ processor given as a file path is not followed (the function that
  makes the `Worker` handles).
- `channel/python.rs`: a client is traced to its constructor by `Code::origins` (a name its
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
- `channel/rust.rs`: a client is recognized when its receiver traces back to the library's
  constructor (`Code::origins` with `Follow::Values`, then `Code::outside`, one constructor per `LIBRARIES`
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
- `package.rs`: a manifest never fails indexing, so its reader has fixed limits. `MAX_NAME`
  (214 bytes; a longer name is no package) and `MAX_BYTES` (under `lang/mod.rs`). A range is
  stored as written, with no length limit beyond `MAX_BYTES`; npm's `flag` (`package/npm.rs`) checks only one of at most
  `MAX_RANGE` (256 bytes) and calls a longer one `range unchecked`. PyPI (`package/pypi.rs`, spec #24): `MAX_NAME` and
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
  reached (no marker). Import resolution: `exported` (`package/npm.rs`) follows at most
  `MAX_CONDITIONS` (32) nested conditions and `lookup` (`package.rs`) at most `MAX_REEXPORTS` (64) `export *`
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
  every step it copies into a `const`'s uses or past a method call to `USES_BUDGET` (2^20 per
  file), so a long method chain spends it; past it, the
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
  `shape::Budget` per extraction (`Http::extract`, `exported_shapes`), shared by every
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
- `lang/mod.rs`: `MAX_BYTES` (1 MiB, beside the packs that read their configs by it) bounds every
  manifest and pack config the package readers and the packs take; a larger file reads as empty,
  and the index reads it up to one byte past the limit (`read_up_to`).
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
  (`surface`) exports itself keeps a shape (a re-export reaches it through the
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
  there for the whole scope (the safe side: a name match instead of `exact`); at the top level
  they, and a lambda's parameters, hide nothing, so `[h() for h in hs]` beside `def h()` stays
  `exact` to the definition (hiding `h` there alone needs a scope of the comprehension's own). A write to an
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
  `Unknown`. A `Cargo.toml` past `lang::MAX_BYTES` (read up to one byte past it, as every
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
  more route attributes (`#[get("/x")]`, `#[route(..)]`) than that has the last ones (nearest the
  function) as calls that decorate it (`Call::decorates`), the rest nothing. `ROUTE_ITEMS` (1024, `lang/rust/values.rs`):
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
  still counts (no marker). The walk over mentions carries each node's parent, the token
  before it and where an `if let` binds, and `built` reads the attributes before a function
  with the one read of its parent's children, so neither asks for a parent or sibling.
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
- `http/rust/axum.rs`: `METHOD_CHAIN` (64): an axum method router chain (`get(a).post(b)`) is read
  to that many calls, a longer one gives its route no provider. `FLUENT_CHAIN` (1024,
  `resolve.rs`): a builder chain (`Router::new().route(..)`, an actix-web `ServiceConfig`'s
  `cfg.service(a).route(..)`) is read to that many calls down to what starts it (`Code::chain`),
  and a call past it has no origin, so its route is no provider (the walk is memoized per call,
  linear in the chain). `PREFIX_DEPTH` (64) and `PREFIX_WORK` (4096):
  `prefixes`, for every kind that mounts (Express, FastAPI, Flask, axum, actix-web, Rocket, Ktor), reads a router
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
- `http/kotlin.rs` (spec #27, #98): a scope holding a `route` call whose lambda is not read (one
  nested past the Kotlin pack's `DEFINITION_DEPTH`, or a block passed as a function reference,
  `route(path, Route::orders)`) registers nothing: its routes and routing extension calls, the
  block's siblings included, are served nowhere, never under part of a prefix. The implicit
  receiver of a routing block is taken as Ktor's `Route` (`Routing` in `routing { }`), kept
  through any lambda argument inside the block (`authenticate { }`) but a handler's or a local
  function's: a member of `Route` named like an extension of the repository, and an extension of
  a supertype (`Any`, a type parameter) nearer in scope, are not weighed, and an extension on
  `Routing` called in a `route` block (which Kotlin runs on the `Routing` around it) mounts
  nothing. A local or a parameter of the name hides an extension (`Code::extensions`), and a
  local function of the name mounts nothing. Routes nested in `route(path, HttpMethod.X) { }`
  (its lambda is the handler), `method(HttpMethod.X) { }`, `install(Routing)`, a lambda passed
  by name (`body = { }`), any named argument other than `path` and `method`, and a path with a
  string template that does not fold are not read. A Ktor verb inside a lambda (`get`, `put`) is a call the
  resolver reads as a name match, as any call there is, so it may match a repository method of
  the same name: that method then has a caller and loses its export door.
- `http/kotlin/client.rs` (spec #27, #99): `url { path(..) }`, `url(..)` and `takeFrom` inside a
  request block, `HttpRequestBuilder` values, `submitForm`, the `prepare*` forms, an aliased
  import (`import .. as k`) and a client the call reaches through `with(client) { get(..) }`
  are not read. A `defaultRequest` base given by a parameter of a client factory is a hole. A
  local client is followed only when its `val` is written with the constructor call (or a
  `let`, `also` or `use` receiver): `val b = a` and a `var` are not, and a local named like a
  property of its file is typed unknown by the pack, so it is not either. A nested lambda's
  `method = X` makes the outer request's method unknown even when the inner lambda always runs
  (`headers { }`).
- Kotlin base URLs (spec #27, #100): `STRING_METHODS` also lets `replace`, `lowercase` and the
  others of the trunk packs' lists pass an environment read through, beyond `trimEnd` and
  `removeSuffix`. A fallback that reads the environment through a name of another file, a
  function or a local is not seen (`ENV_NAMES` is the file's properties). An environment read
  kept in a local `val` (`val raw = System.getenv(..) ?: ..; raw.trimEnd('/')`) or in a
  function stays open: the consumer is open, never a wrong link. Parity with Java, not
  regressions: a constructor parameter without `val` used in an initializer stays open; a
  call that constructs the class in the source (`ExplicitClient("http://localhost:1")`) does not
  stop injection into the constructor's property; `@Value` on a plain function's parameter is
  injected as if the function were a bean's. A secondary constructor's `@Value` parameters are
  not read, and a `@set:Value` property is taken as injected into its field.
- Kotlin HTTP consumers (spec #27, #101): a named argument on a nested builder step
  (`.url(url = ..)`) is dropped by `Invocation`, so the consumer is open, never a wrong link; an
  OkHttp `Request.Builder` held by a local `val` is not followed (the chain starts at the local's
  name, as in Java); a local that a wrapper's callers fold to several bases has an unknown one;
  Java Retrofit bases from `@Value` stay unread; Feign remains Java-only.
- Rust values (`lang/rust/values.rs`, `lang/rust/builders.rs`, spec #30 #131): `BUILDER_BUDGET`
  (2^20 syntax nodes per file, over every `let mut`, block by block in the order the blocks
  open), `BUILDER_DEPTH` (64 builders read within
  one another), `BUILDER_SIZE` (4096, bytes of text plus parts) and `BUILDER_WORK` (2^22 size
  units per file, over every prefix folded and every read): a `let mut` String past the budget
  is unknown, and one read past the depth, the size or the work is a hole (`push_str(&s)` of
  itself doubles). `FORMAT_ARGS` (64), `FORMAT_PLACEHOLDERS` (256) and the size above bound
  one `format!` or `concat!`; past any the value is unknown. A `let mut` String is a builder
  only when its block reads it, `push_str`s, `push`es and `+=`s it in statements of that block
  (a statement `#[cfg]` leaves out, decided where the block is read, is not there); a push in
  a branch or a loop, `&mut s`, `write!(s, ..)`, an index or deref use (`s[..]`, `*s`), another
  binding of the name, a method that changes it (`insert`, `clear`) or a second builder of
  that name in the scope leave it unknown. A builder takes the statements after its `let` from
  the one read of the block in `parse`, and its walk asks for no parent or sibling (each lookup
  walks down from the root of the tree), so every node the budget counts costs the same at any
  depth. `format!` and `concat!` read
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
  object's entry lists it (`FRAMEWORKS` in `http/rust/frameworks.rs`, the constructors named
  exactly: what a name of the crate that is no listed constructor makes passes nothing), so a
  setter missing from the list ends the chain and its client (or its router, app, scope,
  resource or Rocket instance) is not read.
- `http/ts.rs`: `router.route(path)` chains and `require` are not followed. A route path or
  prefix that depends on a parameter is not read through callers (it yields no route;
  `http.rs` `path_literal`, for every framework). `http/python.rs`: a FastAPI
  mount whose child cannot be read hides every other app of its framework that nothing else
  mounts. Django: one `include()` whose argument cannot be read leaves no root URLconf; the
  walk reads at most `URLCONF_BUDGET` (2^15) entries over every root and `include()` (a
  module included under several prefixes is read once for each), so the entries past it yield
  no provider, and follows at most `INCLUDE_DEPTH` (64) `include()` calls, each of the next
  module, so the routes of a module included deeper yield none; what
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

- `jvm/spring.rs` (spec #26, #89): `PLACEHOLDER_DEPTH` (32) bounds how many keys deep a
  placeholder is resolved, a cycle included, and `PLACEHOLDER_BUDGET` (64 KiB) what one value
  may cost (each placeholder visited, each byte copied); past either the rest is an unknown
  part. The YAML file goes through the provider documents' reader (`document/yaml.rs`), with its
  budget and nesting limit; a `.properties` file is bounded by `lang::MAX_BYTES` like every pack
  config. A same-package `System` type in another file is not seen by the Java pack's
  `getenv`.
- `jvm/maven.rs` (spec #26): `XML_DEPTH` (64 nested elements), `XML_NAMESPACES` (16 `xmlns`
  occurrences in the file) and `XML_ATTRIBUTES` (64 per tag) are checked by one linear pass
  before parsing, since roxmltree copies the enclosing namespaces into each element that
  declares one and checks a tag's attributes pairwise; `XML_NODES` (65,536) bounds the parsed
  tree, in addition to `lang::MAX_BYTES`. `roxmltree` rejects DTDs and never loads external
  entities. An over-limit, unterminated or malformed manifest reads as an empty summary;
  indexing continues. `jvm/gradle.rs`: `GRADLE_DEPTH` (64) bounds nested blocks; a deeper build
  or settings file reads as an empty summary. The Gradle scanner does not read Groovy slashy
  strings, and reads a triple-quoted string as quoted pairs. It keeps at most `MAX_ROOTS` (64)
  source directories. The Groovy and the Kotlin DSL files (`*.gradle`, `*.gradle.kts`) go
  through this one scanner (spec #27: `tests/it/kotlinpkg.rs`); a `project(":x")` dependency
  declares no coordinate. A block comment nests, as Kotlin's does (Groovy's does not, so a
  nested opener there turns a coordinate into a miss, never a wrong `exact`). Ceilings, all
  misses: a version catalog (`libs.x`), a `dependencies` block nested deeper than one block
  (`subprojects`, `constraints`, `sourceSets { commonMain { dependencies } }`), a configuration
  called as a string (`"implementation"("g:a:1")`), a positional `implementation("g", "a",
  "1")`, a `group` set only in `subprojects`/`allprojects`, and a top-level `val group`. A
  `group` reset in `afterEvaluate` is not seen, and `publishing { artifactId = .. }` is
  ignored, so the announced package may differ from the published one. JVM coordinate names share
  `package::MAX_NAME` (214 bytes). These readers never run Maven, Gradle or a JDK.
