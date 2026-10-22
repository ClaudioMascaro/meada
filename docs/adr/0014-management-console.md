# 14. Management console

- Status: proposed
- Date: 2026-10-22

## Context

Server mode (ADR 0011) with the cloud modules (ADR 0012) is operated from the CLI and from the
operator's orchestrator. Spec S2 (#293) met what that leaves out:

- `meada status --workspace [--generations]` prints text, per node, so seeing the fleet means
  running it on every node.
- The fleet's state is spread over Valkey (the current generation, the retained set, the refresh
  lock and the fence) and the snapshot store (the generation documents and their bundles), and
  nothing shows it in one place. An operator with several nodes, a scheduled job and two external
  services cannot tell which node serves which generation, whether one is stuck, or what a
  token's grant admits.
- A stuck fleet (a fence lost in a Valkey flush or a failover) is diagnosed by reading `meada
  status` node by node and recovered by hand with a Valkey client: no command sets the fence or
  moves the pointer back to a retained generation.
- The workspace manifest is one file in an ops repository, read from the path `--workspace`
  names. Changing the repositories means a pull request and a redeploy of the job, and nothing
  records which list of repositories produced the generation a node serves.
- Reading the graph itself (communities, contracts, cross-repository links, drift, decisions)
  needs an MCP client, or `meada export` and a tool of one's own.
- The container image of #279 carries only the binary.

Triage on 2026-10-22 settled the console's shape with the maintainer, and writing the spec
settled three more points: `meada index` stays offline, operators retain and drop generations
but do not pin them, and a viewer's grant bounds every view. This ADR records those decisions;
the spec fixes the details the slices build.

## Decision

### The `ui` module and the console process

- The console is a cloud module in ADR 0012's shape: the cargo feature `ui`, off by default and
  enabled by `cloud`, plus a `[ui]` section that needs no other section. It brings no crate of
  its own: the listener is the hyper one `meada mcp --listen` uses and the API's JSON goes
  through `serde_json`. A `[ui]` section in a build without the feature is the error naming the
  file, the line, the section and the feature.
- `meada ui --listen <addr> --workspace <bootstrap manifest>` runs the console as a process of
  its own, in its own pod or compose service, on its own port and behind its own admission. The
  address takes the forms `meada mcp --listen` takes and binds the loopback interface without a
  host; the first stderr line is `listening on http://<host>:<port>`, and SIGINT and SIGTERM
  drain within the node's bound. The subcommand is absent from a build without `ui`.
- The console is never a route of a node and never an MCP server. Nodes change in one way only:
  their pull companion writes a heartbeat (below). They keep reading only and keep answering 403
  to any request carrying `Origin`.
- The console connects only to what its bootstrap manifest configures, under ADR 0012's rules:
  the snapshot store, Valkey, and with `[oidc]` the issuer and its `jwks_uri`. It never calls a
  node or GitHub, and discovers nothing.
- With `[snapshot]` the console is deployed like a node, with an init `meada snapshot pull` and a
  `meada snapshot pull --every` companion on a `local_dir` of its own, and answers the query
  playground and the graph views from the generations it holds, in process, through the
  engine's read-only open. Without `[snapshot]` it opens the indexes under `index_dir` as `meada
  mcp --listen --workspace` does.
- The console refuses to start, listing every problem, on a bootstrap manifest with errors, a
  `[ui] dir` that is missing or built for another Meada version (both named), or an issuer whose
  metadata or JWKS cannot be read. It starts without a servable generation or index, and the
  views that need one name the command that provides it. Its readiness is "the listener is up".

### The bootstrap manifest and `[ui]`

- The file `--workspace` names is the bootstrap manifest. With `[workspace] source = "file"`, the
  default and what a file without `[workspace]` means, it carries its `[[repository]]` entries
  and nothing changes. With `source = "store"` it carries `index_dir`, the module sections,
  `[ui]` and `[workspace]`, and the repositories come from the current workspace version.
- `[ui]` holds `dir` (the front end's files), `public_url` (the console's own origin, `https`
  unless loopback), `audience` (`public_url` by default), `client_id` (the provider's public
  client the browser logs in with, required with `[oidc]`), `operations` (`false` by default) and
  at most one of `operator_groups` and `operator_claim`. Unknown keys are errors named by file,
  line and key.
- `operations = true` without `[oidc]` is an error naming both, so every write carries an
  authenticated subject. An operator key without `[oidc]`, `operations = true` without an
  operator key, and `operator_groups` without `[oidc] groups_claim` are errors naming what is
  missing.
- `[snapshot] node_name` is new and optional: the name a node's heartbeat carries, the host name
  in lowercase by default, checked against a name pattern (lowercase letters, digits, `.` and
  `-`, 1 to 63 characters) before it becomes part of a key. Names must be unique, as pod names
  are.
- `[workspace]` is read in every build. `source = "store"` needs the `generation-pointer`
  feature and the `[snapshot]` and `[generations]` sections, and a file that carries both
  `[[repository]]` and `source = "store"` is an error naming both: one fact, one source.
- The console reads `[oidc]` as a node does, with `[ui] audience` as its audience. An operator
  deploys the console with its own bootstrap manifest, beside the nodes', sharing the module
  sections.

### The console's API and static files

- One listener serves both: paths under `/api/v1/` are a JSON API whose only client is the
  console's own front end, and every other path is a static file from `[ui] dir`. Each request
  passes, in order, a `Host` check against `[ui] public_url` (or the bound loopback address), an
  `Origin` check against the console's own origin (a write without `Origin` is refused too),
  admission and routing. The refusals are 403 with fixed text, which closes cross-site requests
  and DNS rebinding.
- The console sets and reads no cookie and sends no `Access-Control-*` header, so the same-origin
  rule is its whole CSRF surface.
- Static files are served only from `[ui] dir`: no listing, no `..` or empty segment, no symbolic
  link out of the directory, a fixed table of content types, `nosniff`, `no-referrer`, and a
  content security policy of `default-src 'self'`, `script-src 'self' 'wasm-unsafe-eval'`,
  `connect-src 'self'` plus the issuer's origin, and `frame-ancestors 'none'`. An unknown path
  outside `/api/` answers `index.html`, for the front end's own routes.
- The API reads the bootstrap manifest as loaded, the workspace versions, each repository's index
  state across the nodes, the fleet, the store, admission and the GitHub App credentials state,
  and answers the playground and the exports. Reads are `GET`, except the token check and the
  playground, which are `POST` so a token or a query never sits in a URL or an access log. Errors
  carry the CLI catalog's English text. Request bodies are capped per route.
- No answer carries a secret: no token, password, private key, JWT or secret file content, and
  no URL holding one. The manifest view names secrets' variables and files, never their values.
- `/api/v1/` is the contract between a console and the front end of the same release; scripts
  use the CLI.

### The heartbeat

- With `[generations]`, every round of `meada snapshot pull --every` ends by writing the node's
  heartbeat, whether the round moved the pointer, changed nothing or failed. A single `meada
  snapshot pull` writes none.
- The heartbeat is a JSON document at `{<key_prefix>}:node:<name>`: the node's name, its
  generation and fence (from its pointer file), the generations it holds, its readiness (the
  predicate `meada status --generations` exits on), the binary's version and the time. It
  expires after three `--every` periods, so a dead node disappears. The name also enters the
  sorted set `{<key_prefix>}:nodes`, scored by expiry, so the console lists nodes without
  scanning the keyspace.
- A heartbeat that cannot be written is one warning on stderr. The round's outcome, the node's
  pointer file, its exit code and its readiness do not change.
- The console only reads heartbeats and treats them as hostile input: names, ids and fences are
  checked against their patterns, and a heartbeat that fails a check is shown as malformed,
  never used. It names a node stuck when the node's fence is above the Valkey pointer's while its
  generation differs from the current one, and the fleet stuck when `{<key_prefix>}:fence` is
  below the highest fence recorded, each with its recovery.
- Without `[generations]` there are no heartbeats, and the fleet view says that a per-node view
  needs that section. A node that writes no heartbeat is not in the fleet view.

### Roles and admission

- Without `[oidc]`, the reverse proxy admits, as for the nodes, and the console is read only:
  every admitted request is a viewer's.
- With `[oidc]`, the console validates a bearer JWT with the nodes' module and its own audience.
  A subject in an `operator_groups` group, or whose `operator_claim` is `true`, is an operator;
  every other admitted subject is a viewer. Operators are managed in the identity provider, not
  in Meada. (Amended 2026-10-22; #294: ADR 0015 adds the team operator, a subject one of whose
  groups owns an entry of the current workspace version that its grant reads, who may change
  that group's entries and nothing else; this ADR's operator is the global operator.)
- A viewer reads every view and writes nothing. A write from a viewer, and any write while
  `operations` is off, is a 403; the front end hides what the role cannot do, but the API is what
  refuses.
- The token's grant (`identities_claim` and `[oidc.groups]`, computed by the nodes' code) bounds
  every view for a viewer, as #290 bounds a node's answers: the playground, the exports and the
  graph views read only granted repositories, and the operational views list no identity outside
  the grant. State that belongs to no repository (heartbeats, the pointer, the fence, the lock
  holder, the issuer's metadata, the manifest's sections) is shown whole. An operator sees the
  whole fleet.
- The browser logs in with the authorization code flow and PKCE against the operator's issuer,
  as the public client `[ui] client_id`, keeps the token in memory only and sends it as a bearer
  header. Meada issues no token, registers no client and forwards none.
- The token check takes a pasted token, runs the nodes' checks and grant code on it, and holds it
  only for that request: it is never written to stderr, Valkey, the store or a URL, and never
  sent to the issuer or any other host.
- Every write leaves one stderr line, `ui: <subject> <action> <arguments>: <outcome>`, with no
  token or part of one.

### Operations as CLI subcommands

- Every write the console makes is first a CLI subcommand taking `--workspace <bootstrap
  manifest>`, and the console calls the same code in process, so recovery and scripts need no
  browser and the tests live on the CLI:
  - `meada snapshot pointer set <generation>` moves the Valkey pointer to a generation of the
    retained set, a rollback or forward again;
  - `meada snapshot pointer retain <generation>` adds a stored generation to the retained set and
    keeps it by hand, outside the `keep` trim, until it is dropped;
  - `meada snapshot pointer drop <generation>` takes a generation that is not current out of the
    retained set;
  - `meada snapshot fence recover [--at-least <fence>]` raises `{<key_prefix>}:fence` to the
    highest fence recorded in the pointer, the retained set and every live heartbeat, or to the
    floor given, never lowering it;
  - `meada workspace list`, `show`, `import`, `apply` and `pull` (below).
- `meada snapshot pointer` and `meada snapshot fence` come with `refresh-lock` and need
  `[refresh_lock]`. Each acquires the refresh lock itself, with a new fence, makes its one write
  through a script that checks the lock still holds its token, and releases it. A lock already
  held refuses the operation, naming the holder.
- The holder is a hash `{<key_prefix>}:holder` beside the lock: the host name (or `node_name`),
  the process id, the start time and what holds it. `meada snapshot lock` and every operation
  write it with the lock's lifetime.
- A pointer write goes through the pointer script with the operation's fence, so `set` gives the
  pointer a fence above every node's and nodes swap on their next pull. A rollback holds until
  the next publish moves the pointer again; to stay back, the operator suspends the job.
- The generations kept by hand are the set `{<key_prefix>}:kept`. The pointer script keeps them
  out of its trim and every node's pull holds the whole retained set, so a kept generation stays
  answerable at `/<identity>@<generation>` on every node until it is dropped. "Pin" keeps its
  one meaning, the client's path.
- Each command prints one line on success and its errors come from the CLI's message catalog;
  every id on the command line is checked against its pattern first.
- No command and no console action releases a held lock (the renewed TTL frees a dead job's
  lock) or triggers a refresh (the job belongs to the orchestrator).

### Workspace versions in the store

- A workspace version is the `[[repository]]` entries of a workspace (identity, path, and the
  `serve`, `resource` and `peer` blocks) as one immutable document in the snapshot store, at
  `workspaces/<id>.json` under the store's prefix, with its author, time and message. It is
  uploaded with its size and BLAKE3 hash, as every object is, and never rewritten.
- Its id is 16 lowercase hex characters, the first 16 of a BLAKE3 hash of the entries' canonical
  text; author, time and message are not hashed. Same entries, same id, on any machine, and
  saving entries already stored uploads nothing.
- The current version is the hash `{<key_prefix>}:workspace`, `{id, fence}`. `meada workspace
  apply <version>` moves it under the refresh lock, through a script that writes only while the
  lock holds its token and the fence is above the current one. Applying an older version is the
  rollback.
- `meada workspace import <file>` stores a version from a manifest file's `[[repository]]`
  entries, and the console's save is the same function over the editor's text. The author is the
  OIDC subject in the console, and `--author` or `cli:<login>@<host>` on the CLI. Every save
  validates with `meada-core`; the checks that need a mirror stay in `meada index --workspace`.
- `meada workspace pull` brings the current version onto the machine, as `meada snapshot pull`
  brings a generation: it reads the Valkey pointer and the document, checks both as hostile
  input, writes the document under `<index_dir>/workspace/` and swaps
  `<index_dir>/workspace/current` by rename, never to a lower fence.
- With `source = "store"`, `meada index --workspace`, `meada status --workspace`, `meada export
  --workspace` without `--generations`, and `meada snapshot publish` read the pulled version from
  disk, once, and refuse without one, naming `meada workspace pull`. `meada index` opens no
  connection, whatever the build and the manifest. The job's refresh becomes `meada workspace
  pull`, the fetch of the mirrors `meada workspace show` lists, `meada index --workspace` and
  `meada snapshot publish`, under `meada snapshot lock`, so `apply` cannot run between the pull
  and the publish and the whole refresh sees one version.
- `meada snapshot publish` records the version id in the generation document's new optional
  `workspace` field. Generation and bundle ids do not change, a stored document is never
  rewritten, and older binaries ignore the field.
- One workspace per deployment: one bootstrap manifest, one key prefix, one current version.

### The front end

- Vite, React and TypeScript with `strict` on, under `ui/` in the repository, outside the Cargo
  build. `wasm-pack` builds `meada-core` for the browser. The output is static files; nothing of
  Node runs at serve time.
- One page per view: the bootstrap manifest, workspace versions with the editor and the diff,
  repositories, fleet, store, admission with the token check, credentials, the playground, and
  the graph views (communities, contracts, cross-repository links, shape drift as ADR 0009
  defines it, decisions). A view whose module or section is off says which one it needs.
- The workspace editor validates and diffs with `meada-core`, so its errors are the server's
  text; a save still validates on the server. The graph views fetch a repository's export once
  and derive every view in the browser.
- The copy is in Brazilian Portuguese and English, picked from the browser's language as the CLI
  picks from `LANG`; the playground's answers are the MCP surface's English text (ADR 0008).
  Pages are reachable by keyboard and labelled for screen readers.
- Operator actions show the CLI command they run and ask for confirmation; a refusal shows the
  CLI's message.

### The `meada-core` split

- The crate becomes a Cargo workspace with `meada-core` beside the binary's crate. `meada-core`
  holds the manifest and workspace schema and the validation a file's text decides, the ids and
  their patterns, the result and export types, the text renderers of `search`, `context` and
  `trace`, and drift. It has no `rusqlite`, `tree-sitter`, filesystem, process or network code
  and no async runtime, and builds for `wasm32-unknown-unknown`, with `wasm-bindgen` behind a
  feature the binary never enables.
- Behavior does not move: every export and trace golden stays byte for byte, and the default
  build's `cargo tree -e normal` lists the same third-party crates. The split lands first,
  before any console code, and `meada-core` is published with `meada` at the same version.

### Packaging

- The `cargo` build, install and gate stay free of Node, and the binary never embeds the front
  end: `[ui] dir` is a directory on disk. The front end and the WebAssembly bundle are built and
  tested in a CI job of their own.
- The image's `-cloud` tag (#279) gains a stage with Node and `wasm-pack` that puts the front
  end's files under `/usr/share/meada/ui`; the default tag carries nothing of the console. The
  container workflow attaches `meada-ui-<version>.tar.gz` to each GitHub release, for `cargo
  install --features ui` users.
- The front end's files carry the Meada version they were built for, which the console checks at
  start.
- The release binaries stay the default build (ADR 0010).

## Considered options

- **The console as a path on the serving nodes.** Nodes would gain a browser surface, the
  `Origin` refusal that protects them would need an exception, they would hold the credentials a
  write needs, and a console bug or outage would touch every agent's answer. Invariant 4 of ADR
  0002, as ADR 0011 reads it, has the server write nothing. A process of its own keeps all of it
  apart.
- **A status endpoint on the nodes instead of heartbeats.** A new route on every node, which the
  console would have to discover (through the orchestrator's API or a list kept by hand) and
  call, an outbound connection to endpoints no manifest configures, through each node's
  admission. A heartbeat uses the Valkey connection the pull companion already has, and nodes
  gain no HTTP surface.
- **Operator pins beside retention.** Two mechanisms to keep one generation, and "pin" already
  means the generation a client names in its path. A generation retained by hand is what such a
  pin needs, since every node holds the retained set.
- **`meada index` reading the workspace version from the store, amending invariant 1.** Puts the
  network in indexing. `meada workspace pull` keeps the read in a separate command, as `meada
  snapshot publish` keeps the write, and invariant 1 stands as written.
- **Git-backed workspace management through the GitHub App.** The console would commit to the
  ops repository: it would need write credentials to a repository, call GitHub against ADR
  0012's outbound rule, and tie the managed workspace to one forge, while ADR 0013's module only
  answers the job's fetches. The ops repository's file stays the default for teams that want
  pull-request review.
- **Next.js with server rendering.** Node at serve time in the pod, a second runtime to secure
  and patch, for an operator's tool that needs no rendering on the server. Static files from the
  console's own listener suffice.
- **Embedding the front end in the binary.** The `cargo` build would need Node, every Rust
  contributor a JavaScript toolchain, and every `ui` build would carry the assets. The image and
  the release tarball ship the files instead.
- **Writes behind the reverse proxy alone.** A write without an authenticated subject has no
  author for its log line or its workspace version, so operations need `[oidc]`.
- **Releasing the lock or triggering a refresh from the console.** The renewed TTL already frees
  a dead job's lock, and releasing a live one races it; the refresh belongs to the orchestrator,
  so the console shows the lock and the last publish.
- **Several workspaces per fleet.** Needs roles per workspace, a switcher and one key prefix per
  workspace; left to #294. A deployment has one bootstrap manifest, one key prefix and one
  current version. (Amended 2026-10-22; #294: ADR 0015 keeps one workspace per deployment, with
  several workspaces per fleet and multi-tenant isolation out of scope, and lets teams own the
  entries of the one workspace instead.)

## Consequences

- What each invariant of ADR 0002 keeps, with no new amendment of it:
  1. Indexing never touches the network: `meada index` reads the pulled workspace version from
     disk and opens no connection; `meada workspace pull` is a separate command.
  2. Source stays within the operator's boundary: the console serves span text and exports only
     to the subjects it admits, bounded by their grant, and connects only to endpoints the
     bootstrap manifest configures. No account with Meada or a vendor the operator did not
     choose.
  3. Meada writes only in its own state: the store's prefix (`workspaces/` beside the
     generations), the Valkey key prefix (`:node:<name>`, `:nodes`, `:holder`, `:kept`,
     `:workspace`), the console's `local_dir` and `<index_dir>/workspace/`. Never a mirror, a
     bootstrap manifest or a secret.
  4. The MCP server has no tool that writes: the nodes are unchanged and the console is no MCP
     server. Its writes are an operator's explicit commands on Meada's own state.
  5. One index per repository: unchanged; a workspace version with two entries of one identity
     is an error.
  6. Determinism: workspace version ids derive from the entries; generation and bundle ids do not
     change; no clock, host name or address reaches an answer or an export. Times live only in
     heartbeats, holders and version documents. The playground's answer for a repository and a
     generation equals the node's byte for byte.
- ADR 0011 carries inline amendments for the bootstrap manifest, the managed workspace as the
  alternative to the ops repository's file, and the job's refresh. ADR 0012 carries inline
  amendments for the `ui` row of its module table, what Valkey holds, `node_name` and the
  heartbeat, the operations on the pointer and the fence, and the workspace version beside the
  generations.
- ADR 0008 is unchanged: the nodes' endpoints, tools, descriptions and answers do not move. ADR
  0010 is unchanged: the release binaries are the default build. ADR 0013's module stays the
  job's: the console reads its state in Valkey and never calls GitHub.
- Everything is additive under Spec R1's stability rule (a feature, subcommands, keys, Valkey
  keys, a document field): the console is a minor change. Without `ui`, nothing changes; without
  `[workspace]`, the manifest in the ops repository works as today.
- The operator owns more: unique node names, a console reachable only through its admission (a
  console exposed beyond it exposes the graph), suspending the job to stay on a rollback, and the
  identity provider's operator group or claim. A node that writes no heartbeat is invisible to
  the fleet view.
- The tail is cut into eight slices against `main`, each blocked by the one before: the
  `meada-core` split; the heartbeat with `node_name`; the pointer and fence CLI with the holder
  and the kept set; workspace versions; the console's server and read API; operations and roles;
  the front end with WebAssembly and the image and release changes; documentation, with console
  profiles for the compose example (#280) and the Helm chart (#281).
- A new CI job builds `meada-core` with `wasm-pack` and the front end and runs Vitest against the
  WebAssembly bundle; the `cargo` gate is unchanged.
