# 11. Server mode

- Status: proposed
- Date: 2026-10-21

## Context

Issue #252 asks to run Meada once on a shared server for an organisation, so every coding
agent there reads the same graph over MCP instead of each developer indexing its peers locally.
Meada was built for one machine:

- The MCP server speaks stdio only and answers about the client's first `file://` root, a path
  on the client's machine (ADR 0008).
- Each index lives in `<repo>/.meada/`, and opening it creates the directory and its
  `.gitignore` (ADR 0004). On a server the checkouts are mirrors Meada should not write into.
- Every query refreshes first, which stats every file of every repository of the workspace
  (ADR 0004). That does not scale to an organisation-wide workspace.
- Peers are declared in each repository's `meada.toml` and found next to it (ADR 0005). An
  organisation with hundreds of repositories will not add one to each.

Serving spans to remote agents also changes the reading of ADR 0002, invariant 2 ("source
code never leaves the machine"), and rejected options of ADRs 0004 and 0005 come back. Triage
on 2026-10-21 settled four decisions with the maintainer; this ADR records them and fixes the
design points the slices need.

## Decision

### The four triage decisions

1. **Server mode lives in the binary, behind a flag.** `meada mcp --listen <addr>` serves the
   same three tools over MCP streamable HTTP (rmcp's `transport-streamable-http-server`),
   bound to the loopback interface by default. Authentication belongs to the operator's reverse
   proxy in this phase. (Amended 2026-10-21; ADR 0012: the proxy stays the default, and the
   opt-in OIDC module is the alternative inside Meada.)
2. **A workspace manifest, read in server mode only.** A versioned file in an ops repository
   lists the repositories (remote identity and local path), plus the `[serve]` and
   `[[resource]]` declarations of those with no `meada.toml`. A repository's own `meada.toml`
   wins.
3. **The knowledge-base feed is a documented use of `meada export`.** Chunking by symbol,
   community and contract happens downstream; embeddings stay outside Meada (ADR 0002,
   invariant 1). A format of its own waits until the export proves insufficient.
4. **Exact duplicate symbols are a signal inside Meada.** A content hash per symbol and a
   `search` kind. Fan-in across repositories and exposure already derive from the export and
   are documented, not computed anew. Churn, owners and registry data stay with the agent.

### Server mode

Server mode is `meada mcp --listen <addr>` and `meada index`, each given the workspace manifest
with `--workspace <path>`; `meada status` and `meada export` take it too (see Open). Without that flag every command behaves as
before: the stdio server, discovery and freshness on demand are unchanged.

### Where source may go (invariant 2)

- Source stays within the operator's boundary. The stdio server still never sends code off the
  machine. The HTTP server returns span text, as `context` does today, to the clients the
  operator admits, and to no one else.
- An `<addr>` with no host binds the loopback interface. Binding another interface is the
  operator's explicit choice, made with a reverse proxy in front.
- Meada does no authentication in this phase: the reverse proxy admits clients (TLS, tokens,
  SSO, network policy). rmcp's `auth` feature is the later option to weigh when an operator
  needs Meada itself to check credentials (in rmcp 3.5.1 its OAuth support is mostly the
  client's side), and that needs its own decision. (Amended 2026-10-21; ADR 0012: the opt-in
  OIDC module validates a bearer JWT inside Meada, as an OAuth resource server; the reverse
  proxy stays the documented default admission.)
- Nothing else changes: no account, no telemetry, no key, and indexing still never calls a model
  or the network. Accepting connections opens no outbound one. (Amended 2026-10-21; ADR 0012:
  unless an opt-in cloud module is configured, and then only to its configured endpoints.)

### Naming the repository

- A remote client names the repository it asks about in the endpoint path: the repository's
  identity is the path, as in `http://meada.internal:7333/github.com/acme/web`. Each repository
  is one MCP endpoint, so a client configures one URL per repository it works on, and the tools
  and their descriptions do not change (ADR 0008 is untouched). (Amended 2026-10-21; ADR 0012:
  with the generation pointer module, `/<identity>@<generation>` pins a generation the node
  holds, the generation being what follows the last `@`, while `/<identity>` serves the current
  one.)
- The named repository plays the part of the repository where Meada runs: `search`, `context`
  and `trace` answer from it, across the workspace.
- A request that names no repository, or one the manifest does not list, is refused with HTTP
  404, and the body lists the identities the server serves. There is no default repository: a
  default would answer about the wrong repository without saying so. (Amended 2026-10-21;
  ADR 0012: a pin naming a generation the node does not hold is a 404 too, naming the requested
  generation, the current one and the held ones. With the OIDC module a 401 precedes any 404: a
  caller without a valid token learns no identity or generation, and a valid token's 404 lists
  only what it may read. The protected resource metadata,
  `/.well-known/oauth-protected-resource/<identity>` and its pinned form, is served with that
  module, without a token.)
- `trace` with `changes: uncommitted` is refused in server mode: the working tree it would diff
  is on the client's machine, not the mirror's. The stdio server keeps it.

### The workspace manifest

- The file is `meada.workspace.toml`. It lives in an ops repository, versioned and reviewed in
  PRs, and Meada reads it only from the path `--workspace` gives. It is never discovered by
  walking up from a directory or by looking at sibling clones.
- Its schema:

  ```toml
  index_dir = "/var/lib/meada"

  [[repository]]
  repo = "github.com/acme/web"
  path = "/srv/mirrors/web"

  [[repository]]
  repo = "github.com/acme/api"
  path = "/srv/mirrors/api"

  [repository.serve]
  hosts = ["api.acme.com"]
  prefix = "/api"

  [[repository.resource]]
  kind = "postgres"
  id = "orders"
  role = "write"
  ```

  `index_dir` is required (see below). Each `[[repository]]` has `repo`, the remote identity,
  and `path`, absolute or relative to the manifest's directory. `serve` and `resource` take the
  shapes of `meada.toml`'s `[serve]` and `[[resource]]`.
- The identity read from the mirror's git remote must equal `repo`; a mismatch is an error
  naming both. Two entries with one identity are an error (one index per repository, ADR 0002,
  invariant 5).
- A repository with its own `meada.toml` uses that file whole. The manifest's `serve` and
  `resource` for it are ignored, not merged, and `meada status` names them. A merge would give
  one fact two sources.
- In server mode the workspace of every query is every repository the manifest lists, the named
  one first. Peers named in a `meada.toml` but absent from the manifest are reported missing,
  as today; Meada does not look for them by path hint or sibling clone.
- Each repository's `meada.decisions.toml` is still read from its mirror. Server mode never runs
  `meada decide`.
- Locally nothing changes: discovery by `[[peer]]` hints and sibling clones works as ADR 0005
  says, and a central repository or a configuration outside version control stays rejected
  there.

### Indexes outside the checkout

- In server mode every index lives under the manifest's `index_dir`, at
  `<index_dir>/<identity>/index.db` (`/var/lib/meada/github.com/acme/web/index.db`). Nothing is
  written into a mirror, so mirrors may be read-only mounts.
- The key is the remote identity, not the path, which answers ADR 0004's objection to a cache
  directory: a mirror that moves keeps its identity and its index, and the manifest says where
  the indexes are. A remote that changes is a new identity, which already renames every symbol
  and starts the index over; the old directory is the operator's to delete.
- Meada writes no `.gitignore` there: the directory is not inside a repository.
- A `local/<dir>` identity (no remote) is refused in server mode. It is not stable across
  machines, two mirrors may share a directory name, and the identity is what local users name
  the repository with in `[[peer]] repo`.
- An identity whose segments would leave `index_dir` (`..`, an empty segment) is refused.
- Invariant 3 holds unchanged: `index_dir` is Meada's own index directory in server mode.

### Refresh by command

- In server mode the server only reads. It opens indexes read-only, stats nothing per call and
  writes nothing, not even a refresh. (Amended 2026-10-21; ADR 0012: with the session-store
  module the server also writes MCP session state to Valkey, under its key prefix; it still
  never writes an index or a file.)
- The operator refreshes: after a mirror moves, the operator's fetch job or a webhook runs
  `meada index --workspace <path>`, which refreshes every repository of the manifest the way a
  local refresh does (a stat per file, a hash of the changed ones). Meada still never clones or
  fetches (ADR 0002, invariant 4).
- Each repository's refresh is one SQLite transaction (`index::refresh`), so a query sees either
  the last committed state of an index or the new one, never a half-written index. A query that
  meets a commit in progress waits for it rather than failing. A workspace commits repository
  by repository, so an answer that crosses repositories during a refresh may join one
  repository's new state to another's old one; links derive at query time, so nothing stored
  across repositories goes inconsistent, and this is the same gap as a mirror fetched between
  two pushes.
- An index that is missing, or of another schema version, is not rebuilt by the server: the
  answer names `meada index`. After installing a new binary, the operator runs `meada index`
  before restarting the server.
- `context` shows span text only while the file's hash still equals the indexed one. When a
  mirror moved before `meada index` ran, the server shows the location without the text and
  says the index is behind, where the stdio server refreshes and retries.
- Freshness on demand stays the default everywhere else (ADR 0004).

### Mirrors

A mirror is a checkout with a working tree. Meada walks the working tree, so a bare clone has
nothing to index and is refused. Untracked files in a mirror are indexed like any others, so
mirrors are kept clean by the operator's job.

## Considered options

- **A separate server distribution built on Meada.** Two binaries to release and keep in step,
  for a flag's worth of difference.
- **A bot that commits a `meada.toml` to every repository.** ADR 0005 unchanged, but hundreds
  of PRs to keep in sync, and repositories the organisation does not control stay out.
- **Authentication inside Meada now** (rmcp `auth`, a bearer token flag, mTLS). The operator's
  proxy already does it, and credentials checked by Meada are a security surface of its own,
  with a decision of its own. Left for later. (Amended 2026-10-21; ADR 0012 is that decision:
  the opt-in OIDC module validates a bearer JWT inside Meada, as an OAuth resource server, and
  the reverse proxy stays the default.)
- **Binding every interface by default.** Serves source to the network by accident.
- **The repository in a header.** Not every MCP client lets a configuration set headers, and
  a proxy may drop them; a path is visible in every log and routable by the proxy.
- **The repository as a tool parameter.** Every tool gains a parameter its description must
  explain, on every turn, against the tool list's token budget (ADR 0008), and an agent may
  pass the wrong one mid-session.
- **Mapping the client's MCP roots.** A root is a path on the client's machine; the server
  cannot read it or its remote.
- **A default repository when none is named.** Answers about the wrong repository silently.
- **Manifest entries merged field by field with `meada.toml`.** Two sources for one fact, and
  the repository's owners cannot tell from their file what the server sees.
- **Discovering the manifest by walking up, or reading it locally too.** A file outside the
  repository would change local answers, which ADR 0005 rejects.
- **`index_dir` as a command-line flag.** `meada mcp --listen` and `meada index` would have to
  agree on it every time; in the manifest they read one value.
- **A default `index_dir`.** Any default lands in a checkout or a home directory the operator
  did not choose.
- **Indexes keyed by mirror path.** ADR 0004's objection: orphaned when a mirror moves.
- **`local/<dir>` identities in server mode, or an identity set in the manifest.** The first
  collides between mirrors; the second names symbols differently from what local users write
  in `[[peer]] repo`.
- **A stat per call in server mode.** Statting an organisation's files on every call is the
  cost this ADR removes.
- **A watcher in the server.** ADR 0004 rejected it, and the fetch job already knows when a
  mirror moves.
- **Meada fetching mirrors itself.** Puts the network in Meada (invariant 4).
- **A snapshot of the whole workspace per refresh.** Needs one database or a generation scheme
  across indexes (invariant 5); per-repository consistency suffices while links derive at query
  time. (Amended 2026-10-21; ADR 0012: the snapshot module adopts it as opt-in, for several
  nodes serving the same commits. A generation is stored as one bundle per repository, never
  merged, so invariant 5 holds.)
- **Bare clones as mirrors.** Reading blobs from the object store is a second walk to keep
  equal to the first.

## Consequences

- ADR 0002, invariants 2 and 4, ADR 0004 (Storage, Freshness and the cache directory option)
  and ADR 0005 (the central repository and global configuration options) carry inline
  amendments pointing here. ADR 0008 is unchanged: three tools, the same shapes.
- After this ADR merges, four slices are cut against `main`, each a sub-issue of #252:
  - the index directory outside the checkout, from `index_dir`, keyed by remote identity;
  - the workspace manifest, `--workspace`, server mode only;
  - `meada mcp --listen <addr>`: streamable HTTP, loopback by default, the repository named in
    the path, 404 with the served identities otherwise;
  - refresh by command: a read-only server with no stat per call, `meada index --workspace`,
    and a query that waits on a commit in progress.
- Three slices need no ADR and go first: #253 (a workspace of any size, lifting the limit of ten
  attached peers that an organisation hits before anything else), #254 (exact duplicate symbols:
  a content hash per symbol, a `search` kind, an export field) and #255 (`meada export` as a
  knowledge-base feed).
- Answers on the server lag the mirrors by as long as the operator's job takes to run
  `meada index`. The operator owns that lag, and `context` says when a span is behind.
- The operator owns admission. A misconfigured proxy exposes source, which the documentation of
  server mode states first.

### Open

Both points were closed by spec #259 (the manifest slice, #261):

- **`base_url_env` for a repository with no `meada.toml`.** The manifest makes every listed
  repository part of the workspace, so `[[peer]]` is not needed for membership, but the
  variable names that tell Meada which peer a call built from them goes to come only from a
  consumer's `[[peer]]` entries. Closed: a manifest entry may carry `[[repository.peer]]` with
  `repo` and `base_url_env` only, the shape of `meada.toml`'s `[[peer]]` without `path`. A
  `path` there is an error: the manifest already locates every repository, and membership comes
  from the manifest, not from peers. Like `serve` and `resource`, the block is ignored when the
  mirror has its own `meada.toml`.
- **Which local commands take `--workspace`.** Closed: `meada index`, `meada status`,
  `meada export` (with `--repo <identity>`, required, since server mode has no "here") and
  `meada mcp` (with `--listen`; either flag alone is an error). `meada decide` never takes it.
