# Meada

[Português (Brasil)](README.md) · **English** · [Español](README.es.md)

> This is a translation. The Brazilian Portuguese [README](README.md) is the source; if they
> disagree, it wins, and a fix to this page is welcome.

Meada reads your repositories, maps who calls whom, and stitches one repository to the next
through the contracts they share: HTTP routes, queues, packages, shared resources. Coding
agents, and people too, query that map over MCP. No step of indexing uses an LLM.

## Installation

Pick one of five ways.

- **Cargo**: `cargo install meada`.
- **Prebuilt binary**: download the file for your system from the
  [GitHub releases](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS and
  Windows, on x86_64 and arm64) and put `meada` on your `PATH`.
- **Installer** (Linux and macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.
- **npm**: `npx meada <command>` downloads the prebuilt binary for your platform and runs it,
  with nothing to install first.
- **Container**: `ghcr.io/claudiomascaro/meada:<version>` for the default build, or
  `<version>-cloud` with every cloud module, both for Linux amd64 and arm64. `latest` and
  `latest-cloud` follow the latest stable release; prereleases get version tags only.
  The Debian bookworm-slim base (git and CA certificates) is refreshed only with a Meada release.
  The image runs without root and defines no command: pass the arguments explicitly, for
  example `docker run --rm ghcr.io/claudiomascaro/meada:latest --version`. To serve, mount
  the manifest, mirrors and indexes, and pass
  `mcp --listen 0.0.0.0:7333 --workspace /etc/meada/meada.workspace.toml`, following
  [Server mode](#server-mode) and its admission configuration.

## Configuration

Each repository can have a `meada.toml` at its root, versioned in git, saying who it talks to.
A repository that consumes a route names its peer:

```toml
[[peer]]
repo = "example.com/acme/api"            # origin git remote, no scheme and no .git
base_url_env = ["API_URL"]               # variable the client reads its base URL from
# path = "../api"                        # optional: a hint of where the clone is
```

A repository that serves the route declares the hosts it answers on:

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"                         # optional: what the gateway adds in front of paths
strip = "/v1"                            # optional: what it removes from their start
```

With `prefix` and `strip`, a call to `/shop/orders` reaches the code route `/v1/orders`, at the
level `inferred` with the reason `gateway prefix /shop, gateway strip /v1`. A version segment
goes only when you write it in `strip`, and callers of the code path keep linking as before.

A shared resource no code names, such as a table or a bucket, goes in as `[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # lowercase identifier
id = "orders"
role = "write"                           # read, write or read-write
via = ["src/orders.ts::cancelOrder"]     # optional: the symbols that do the access
```

Every writer links to every reader of the same resource in another repository, at the level
`declared`. Without `via`, or when no `via` matches a symbol, the side is the whole repository.

Meada finds peers by their git remote, even when the clone sits at a different path on each
machine. An unknown or misspelled key gives an error naming the file, the line and the key.
Without a `meada.toml`, the repository declares no peers.

## Usage

At the root of the repository:

- `meada index` builds the index, in `.meada/`, of the repository and its peers;
- `meada status` shows the set of repositories, the state of each index and any peer that was
  not found; under each repository, the unlinked resources and each `via` that matches no
  symbol;
- `meada export` prints the whole index as sorted JSON, good for diffing two runs;
- `meada decide` resolves ambiguous links and dynamic URLs, see Decisions;
- `meada mcp` serves the tools below over MCP.

Meada only writes inside `.meada/`, which ignores itself in git. Every query first refreshes
the index with what changed in the files, including what is not committed yet. Command-line
messages follow `LANG` (Portuguese or English); MCP messages are always in English.

## Agents (MCP)

`meada mcp` speaks MCP over stdio. It answers about the first root (`roots`) the client sends,
in Claude Code the session's directory; without roots, about the directory it was started in,
which then must be inside the repository. If the client fails or does not answer `roots/list`
within 5 s, the call uses the last root it sent on that connection, or returns an error saying
so; it never falls back to the server's directory for that (which, for a session in a worktree,
would be another checkout). An answer with no local root counts as no roots (the server's
directory). In your agent's configuration (for example Claude Code's `.mcp.json`), using `npx`:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["meada", "mcp"] }
  }
}
```

With `meada` already on your `PATH`, use `"command": "meada", "args": ["mcp"]`.

There are three tools, always in English, and none of them writes:

- `search` finds a symbol, a route or a contract by name; with the kind `resource`, a resource
  by its kind or id; with the kind `channel`, a channel (queue or topic) by broker or name;
  with the kind `ambiguous` or `unresolved`, it lists the open items (see
  Decisions);
- `context` describes one symbol: its code read from disk, its callers, its callees and the
  contracts it touches; on a resource (`resource::postgres::orders`), it lists who writes and
  who reads it, with the symbol and line, or `(repository-level)` for the whole repository; on
  a channel (`channel::kafka::orders.created`), it lists producers and consumers, those that
  reach it through a wildcard or a binding included, with the level and the reason, and a side
  whose name Meada cannot evaluate shows as `(unresolved)` and is never linked;
- `trace` follows a chain of calls across repositories, downstream (`down`) or upstream (`up`);
  with `changes: uncommitted`, it starts from the working-tree diff, which covers the impact
  analysis of what was just edited.

Answers are compact lines under a token budget (2,000 by default, adjustable per call). Each
link between repositories carries its level and reason (`→[level: reason]`), and `trace` ends
with a footer: the weakest level of the chain, the number of hops, the repositories involved
and the tokens used.

## Server mode

> **Warning.** In server mode Meada returns source code to whoever can connect, and by default
> it authenticates nobody: the operator's reverse proxy admits the clients (TLS, tokens, SSO,
> network policy); the OIDC module, below, is the opt-in alternative. With no host the server listens on the loopback interface only; listening
> beyond it is the operator's explicit choice, and a misconfigured proxy exposes source.
> Meada checks no `Host` header (a proxy with a real hostname works) and answers 403 to any
> request carrying `Origin` (browser and DNS-rebinding protection), on every path and before
> any other answer. The proxy must not expose Meada beyond the clients it admits.

Server mode runs Meada once for an organisation: `meada mcp --listen` serves the tools over
HTTP from mirrors of the repositories a workspace manifest lists, each index outside its
checkout. The manifest, `meada.workspace.toml`, lives in an ops repository, versioned, and is
read only from the path `--workspace` gives:

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

[[repository.peer]]
repo = "github.com/acme/web"
base_url_env = "WEB_URL"
```

`index_dir` is required; `path` is absolute or relative to the manifest's directory. The
`serve`, `resource` and `peer` blocks (a peer has no `path`, since the manifest locates every
repository) are optional and apply to a mirror with no `meada.toml`: a repository's own
`meada.toml` wins, whole, and the manifest's blocks for it are ignored (`meada status` names
them). A mirror is a checkout with a working tree; a bare clone has nothing to index and is
refused.

- `meada index --workspace <manifest>` refreshes every repository's index, at
  `<index_dir>/<identity>/index.db`. It is the operator's refresh, run by a fetch job or a
  webhook: Meada never fetches or clones, and the server only reads. After installing a new
  binary, run `meada index --workspace` before restarting the server.
- `meada mcp --listen <address> --workspace <manifest>` serves. The address is `host:port`,
  `:port` or `port`; the host is an IP literal or `localhost`, never resolved, and the default
  is loopback. The first stderr line is `listening on http://<host>:<port>`. SIGINT or SIGTERM
  stop the server: it stops accepting connections, lets the answers in flight finish and exits
  with code 0. The wait is bounded at 25 seconds, under Kubernetes' default 30-second grace
  period: past the bound the remaining connections are dropped and the exit is still 0.
- Each repository is one endpoint, `http://<host>:<port>/<identity>` (for example
  `http://meada.internal:7333/github.com/acme/web`); a client configures one URL per repository
  it works on. Any other path answers 404 with the identities served.
- The HTTP transport is stateless with JSON responses: any of several servers over the same
  indexes answers any call.
- The server reads the manifest and each mirror's `meada.toml` and `meada.decisions.toml` once,
  at start: a change to those files needs a server restart. A code change in a mirror needs only
  `meada index --workspace`; the next call sees the new index.
- `context` prints `index behind the mirror: run meada index --workspace` when a span's file
  changed since indexing. `trace` with `changes: uncommitted` is refused, since the working tree
  is on the client's machine.
- `meada status --workspace <manifest>` shows each index's state, and
  `meada export --workspace <manifest> --repo <identity>` prints one repository's index, for
  instance to feed a knowledge base.

### Several nodes

The [Docker Compose example](deploy/compose/README.en.md) provides both layouts below, with two
nodes, TLS and token admission at the proxy.

Several servers over the same indexes work with no cloud module, provided the operator keeps
to one writer per index.

- **Layout A, each node indexes its own mirrors.** Each node has a local `index_dir` and runs
  `meada index --workspace` over the mirrors it fetches. It is the simplest, but nodes drift:
  each sees the commit it fetched, and two calls to the same endpoint may answer differently
  until every node has fetched.
- **Layout B, one job builds and nodes receive.** One job fetches the mirrors and indexes all
  of them at one commit; nodes receive the finished mirrors and indexes and swap them in by
  `rename`. A rebuild uses the same swap (`index.db.new.<pid>` under `index.db.lock`, renamed
  over `index.db`), so a reader sees the old index or the whole new one. Rename an index in
  only while no `meada index` writes it: the live `index.db-journal` of an in-place refresh
  would be taken as the new file's journal.
- **Unsupported: `index_dir` on a network filesystem (NFS, EFS) shared by a writer and
  readers.** SQLite is not reliable over a network (see
  [sqlite.org/useovernet.html](https://sqlite.org/useovernet.html)). Give each node a local
  `index_dir`, or a volume only it writes.
- **Read-only.** `index_dir` and the mirrors may be read-only (a volume filled by an init step,
  an image layer): the server creates nothing there. A mirror owned by another user is trusted
  through `-c safe.directory`, for the manifest's mirrors only, which needs git 2.38 or later;
  Ubuntu 22.04 and RHEL 8 ship an older git, so set `safe.directory` in the system config there.
- **No session.** The transport is stateless: no sticky sessions are needed, and clients on
  protocol 2026-07-28 have no session at all.
- **Reverse proxy.** Forward `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` and every
  `Mcp-Param-*` header unchanged (SEP-2243: rmcp rejects a request whose headers disagree with
  its body); the proxy may route on them. Forward the `Meada-Generation` response
  header. Add no `Origin` header: Meada answers 403 to any request
  that carries one, so a browser page cannot call it directly. Meada checks no `Host`, and
  `--listen` takes an IP literal or `localhost`, never a name it resolves.
- **Readiness.** An init step first (`meada index --workspace` or the copy of the indexes). Then
  a TCP probe, or `meada status --workspace <manifest>` as an exec probe: it exits 0 when every
  repository can be served and 1 otherwise, waits at most 50 ms on a commit in progress and
  counts a locked index as ready. `--listen` binds only after its start checks, so an open port
  means the indexes are present. The 404 at `/` lists identities and is not a health check.
- **Single writer.** Run the indexing job with no overlap (for example a CronJob with
  `concurrencyPolicy: Forbid`). Set `terminationGracePeriodSeconds` above the 25 s SIGTERM drain
  bound, and a `preStop` delay matched to the load balancer's deregistration.

### Cloud modules

Opt-in modules, off by default; each is documented below when it lands. A module is a cargo
feature plus one section of `meada.workspace.toml`; the release binaries are the default build,
with none, and a section in a build without its feature is an error naming the feature. The
`cloud` feature turns them all on at once (`cargo install meada --features cloud`) and, like
`refresh-lock`, builds on Unix only.

**Snapshot** (feature `snapshot`, `cargo install meada --features snapshot`). One job indexes
and publishes; nodes receive the result whole, instead of each indexing its own mirrors. After
`meada index --workspace`, which stays offline, the job runs
`meada snapshot publish --workspace <manifest>`. It checks, reading only, that every index equals
its mirror and every mirror is clean (`git status` lists nothing, not even an ignored file), and
refuses listing each repository and the `meada index --workspace` command that fixes it. Then it
uploads a generation: one bundle per repository (a tar of the mirror, `.git` included, and its
`index.db`), a document listing them with their size and BLAKE3 hash, and the `current` pointer
last. Its only stdout line is the generation id. A bundle or a generation already stored is not
uploaded again, and nothing is written on the machine that publishes.

```toml
[snapshot]
endpoint = "https://s3.us-east-1.amazonaws.com"   # any S3-compatible endpoint
region = "us-east-1"
bucket = "example-meada"
prefix = "prod"                                    # optional
path_style = false                                 # true for most self-hosted stores
# ca_file = "ca.pem"                              # optional private CA, relative to the manifest
credentials = "static"                             # or "web-identity", "pod-identity", "instance"
access_key_id_env = "AWS_ACCESS_KEY_ID"            # defaults shown; or the *_file keys
secret_access_key_env = "AWS_SECRET_ACCESS_KEY"
session_token_env = "AWS_SESSION_TOKEN"            # optional
local_dir = "/var/lib/meada/generations"           # for pulling and serving generations
```

Publishing connects only to the `endpoint`, over TLS outside loopback, and to the credential
endpoint of the chosen source; no request follows a redirect or goes through a proxy.
Credentials come from one source, the one `credentials` names: `static` reads the key and the
secret from the variables or files the section names (never from the manifest); `web-identity`
(IRSA) reads `AWS_WEB_IDENTITY_TOKEN_FILE` and `AWS_ROLE_ARN` and exchanges the token with STS in
`region`, over https; `pod-identity` (EKS Pod Identity) calls the agent in
`AWS_CONTAINER_CREDENTIALS_FULL_URI`, only `http://169.254.170.23` or `http://[fd00:ec2::23]`,
with the token in `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`; `instance` uses the instance
metadata service (IMDSv2 over IPv4 only, `http://169.254.169.254`), and only when the section says so.
Credentials stay in memory, and no secret appears in an error.

`ca_file` adds PEM certificates to the system trust roots. The file holds at most 1 MiB;
invalid certificates and an empty file are refused, naming the file.

On each node, `meada snapshot pull --workspace <manifest>` reads the store's `current` pointer
and, when it names a generation the node lacks, downloads its bundles into `local_dir`, checks
each one's size and BLAKE3 hash against the document, extracts them and swaps the node's pointer
by rename. The only line on stdout is the current generation id. `--every <seconds>` repeats the
pull (printing only when the pointer moves); a second pull on the same `local_dir` exits 0 saying
one is running. The node keeps the current and the previous generation and deletes older ones.

Everything read from the store is treated as hostile input: ids and identities are checked before
they become paths; the pull refuses an archive entry with an absolute path or a `..`, a link that
leaves its repository, an entry written through a symbolic link, and devices or FIFOs, with byte
and entry ceilings per generation. The extracted `.git/config` is replaced by a minimal one Meada
writes (only `[core]` and the identity's `origin`), `.git/hooks` is removed, and Meada's `git`
runs in those mirrors without the system or global configuration. A pull that fails at any step
leaves the pointer and the held generations as they were.

```
<local_dir>/
  current                     # "<generation id> <fence>", fence 0 without the refresh lock
  pull.lock
  <generation id>/
    generation.json           # the document, as stored
    <bundle id>/index.db
    <bundle id>/mirror/       # the checkout, .git included
```

To serve what the node received, `meada mcp --listen <address> --workspace <manifest>
--generations`. The node's manifest contributes only its module sections (`[snapshot]`); the
repositories are the ones the current generation recorded, with mirrors and indexes inside
`<local_dir>/<generation id>/`. Each call reads `<local_dir>/current` and opens the engine over
the generation it names: a call already open finishes on the generation it started on, the next
one sees the new one, and `meada snapshot pull --every` swaps the generation without restarting
the server (a repository new in a new generation gets its endpoint at once). Nothing is written
per call, the indexes open read-only, and the server opens no connection to the store. Nodes
never run `meada index`: the job indexes and the node only receives. With no current
generation, or an index that is not at this binary's schema version, the server refuses to start
and names `meada snapshot pull` or the repository and the generation.

`meada status --workspace <manifest> --generations` is the node's readiness probe. It prints the
current generation id, the held ids and a line `store: reachable (current <id>)` or
`store: unreachable (<reason>)`. The exit code depends only on local state: 1 without a
servable current generation (the problems go to stderr), 0 otherwise, the store reachable or
not, so a stopped store does not take the node out of rotation. `meada export --workspace
<manifest> --repo <identity> --generations` exports the same way.

**Generation pointer and pins** (feature `generation-pointer`, which includes `snapshot`;
`cargo install meada --features generation-pointer`). With the `[generations]` section, which
needs `[snapshot]` and `[valkey]`, the current generation's pointer moves from the store to
Valkey, and the whole fleet holds the same set of generations.

```toml
[valkey]
url = "rediss://meada@cache.example.internal:6379"   # TLS outside loopback
password_file = "/run/secrets/valkey"                 # or password_env
key_prefix = "meada:prod"                             # default "meada"

[generations]
keep = 3                                              # generations retained fleet-wide, at least 2
```

`meada snapshot publish` ends by writing, in one script, `{<key_prefix>}:current` (a hash
`{id, fence}`) and `{<key_prefix>}:retained` (a list of `<id>:<fence>`, newest first, each id
once, trimmed to `keep`), and no longer writes the store's `current`; the fence is `0` without
the refresh lock. Each `meada snapshot pull` reads both from Valkey and checks every id (16
lowercase hex characters) and every fence (decimal) before it changes the node; it then
downloads every retained generation the node lacks and deletes the ones that left the list,
never the current one or the one it just replaced. The node's pointer file carries the fence
read with the id. A malformed entry, a missing pointer or an empty list stops the pull with an
error naming the key, and the node stays as it was. The current generation is downloaded and
swapped in first: a retained generation that fails after it becomes a warning on stderr naming
the id and the cause, blocks nothing, and the next pull retries it. Use the same `keep` on the
job and the nodes. `meada status --generations` reads the current generation from Valkey and
checks that the store holds its document; the `store: unreachable (...)` line says whether
Valkey or the store failed.

A workflow run pins the commits it started on in the path: `/<identity>@<generation>`, the
generation being what follows the last `@`, serves that held generation on any node while it
is retained, and `/<identity>` keeps serving the current one. A generation the node does not
hold answers 404 `text/plain`, naming the requested generation, the current one and the held
ones. Every HTTP answer names its generation in the `Meada-Generation` header (a 404 names the
current one) and every tool result in its `_meta`, `{"meada/generation": "<id>"}`: the run reads
the generation of its first answer and pins the next ones to it. Tools, parameters and the text
of the answers do not change. The reverse proxy forwards `Meada-Generation` in answers. A
pinned call still open on a generation that a pull's retention deletes may fail mid-call.

**Refresh lock** (feature `refresh-lock`, which includes `generation-pointer`, Unix only;
`cargo install meada --features refresh-lock`). With the `[refresh_lock]` section, which needs
`[generations]`, the whole refresh runs under a lock in Valkey, and each acquisition gets a
fence, so an overlapping or stalled job never replaces a newer generation with an older one.

```toml
[refresh_lock]
ttl_seconds = 300                                     # default; at least 10
```

```sh
meada snapshot lock --workspace ops/meada.workspace.toml -- \
  sh -c 'git -C api fetch && git -C api reset --hard origin/main &&
         meada index --workspace ops/meada.workspace.toml &&
         meada snapshot publish --workspace ops/meada.workspace.toml'
```

`meada snapshot lock` takes the lock with `SET {<key_prefix>}:lock <random token> NX PX <ttl>`,
adds one to `{<key_prefix>}:fence` and runs the command as its child, with `MEADA_REFRESH_LOCK`
(the token) and `MEADA_REFRESH_FENCE` in its environment. The lock is renewed every third of the
TTL (compare and extend) and released at the end (compare and delete); the exit code is the
child's (128 plus the signal when a signal ended it). A lock already held exits 1 naming the
key, running nothing. When a renewal finds another token, or none succeeded for a whole TTL, the
lock is lost: the child's process group gets SIGTERM (and SIGKILL 10 s later if it is still
alive), and `lock` exits 1 saying it lost it. A SIGTERM, SIGINT, SIGHUP or SIGQUIT to `lock`
reaches the group the same way; once the child exits, whatever is left in its process group gets
SIGKILL (a grandchild that calls `setsid` leaves the group and escapes), and on Linux the child
gets SIGTERM if `lock` dies first. Indexing stays network-free: the child's `meada index`
opens no connection.

With `[refresh_lock]`, `meada snapshot publish` needs both variables, and before each upload it
checks that the lock still holds the token: if not, it stops, uploading nothing more and writing
no pointer. The pointer script writes only when the lock holds the token and the fence is
greater than the one in `{<key_prefix>}:current`, in one operation. Nodes never swap to a
generation whose fence is lower than the one in their pointer file; without the file, they take
the current one whatever its fence. Meada never lowers or deletes `{<key_prefix>}:fence`. If a
Valkey flush or failover loses it, the next fences are lower than the recorded ones and the
fleet stops: the refused publish exits 1 naming `{<key_prefix>}:fence`, the pull of a node with
a higher fence exits 1 without swapping, and `meada status --generations` on that node prints a
`fence: stuck: ...` line with both fences (its exit code does not change). To recover, set
`{<key_prefix>}:fence` at or above the highest fence recorded (in Valkey or in any node's
`<local_dir>/current`), or delete the node's `<local_dir>/current`, which then takes the current
generation. Turning `[refresh_lock]` off after nodes recorded fences above 0 stops the fleet too,
since without the lock every fence is 0: delete each node's `<local_dir>/current`.

**OIDC** (feature `oidc`; `cargo install meada --features oidc`). The default admission is still
the reverse proxy, as the warning at the top of this section says; the OIDC module is the
alternative for operators who want Meada itself to check who calls. With an `[oidc]` section the
server becomes an OAuth 2.1 resource server, as the MCP authorization specification
(2026-07-28) describes: every request carries `Authorization: Bearer <JWT>` issued by the
operator's provider. Meada issues no token, registers no client and never forwards a token.

```toml
[oidc]
issuer = "https://id.example.internal/realms/agents"
public_url = "https://meada.example.internal"   # the URL clients use, behind the proxy
audience = "https://meada.example.internal"     # optional; default: the endpoint's canonical URL
identities_claim = "meada_repositories"          # optional with groups_claim
groups_claim = "groups"                          # optional, together with [oidc.groups]
scopes = ["meada:read"]                          # optional, announced and required
jwks_refresh_seconds = 3600                      # default; at least 60

[oidc.groups]                                    # a token's group: the identities it grants
"team-payments" = ["github.example.internal/acme/payments-api", "github.example.internal/acme/ledger"]
"platform" = ["*"]
```

In the provider: an audience equal to `audience` (or, without it, to each endpoint's URL,
`<public_url>/<identity>`); a claim mapper that puts in `identities_claim` the list of identities
the client may read (`"*"` grants all), or in `groups_claim` its groups; and client credentials
for headless agents and for a portal's backend, which ask the provider for their tokens. A browser
page cannot call Meada directly: with `[oidc]`, any request carrying `Origin` gets 403, on every
path, so a portal calls from its backend. The proxy forwards `Authorization` and the SEP-2243
headers (`MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`, `Mcp-Param-*`) unchanged.

With `groups_claim`, the manifest's `[oidc.groups]` table maps each group to the identities it
grants (`"*"` for all), and `identities_claim` becomes optional: the two keys come together, and
a section with neither claim refuses the start. A token reads the union of the identities its
`identities_claim` lists and those its groups map to. A missing claim or one that is not an array
of strings, a group the table does not map and an identity the server does not serve grant
nothing; a typo shows up to the caller as a 403.

At start, before it listens, Meada reads the issuer's metadata (RFC 8414 or OpenID Discovery, at
the paths and in the order the MCP specification gives, checking that the document's `issuer`
is the configured one) and the JWKS it names; a failure refuses the start. `issuer`,
`public_url` and the `jwks_uri` must be `https` unless loopback; neither request follows a
redirect or goes through a proxy, each has 10 s and a size cap, and certificates are verified
against the system's roots. The JWKS is read again every `jwks_refresh_seconds` and when a
token names an unknown `kid`, at most once a minute; when a refresh fails the cached keys stay
and stderr says so.

`GET /.well-known/oauth-protected-resource/<identity>` answers the resource's metadata (RFC
9728) without a token: `resource` is `<public_url>/<identity>`, `authorization_servers` is
`[issuer]`, `bearer_methods_supported` is `["header"]`, and `scopes_supported` is there when
`scopes` is set; `/.well-known/oauth-protected-resource/<identity>@<generation>` answers the
same with the pinned `resource`. A token passes with an asymmetric algorithm (RS256, RS384,
RS512, ES256, ES384, PS256) that fits its key, `iss` equal to `issuer`, an `aud` equal to
`audience` or, without it, to the URL of `/<identity>` (which pins accept too, so one token
reads a repository at every generation), `exp` present and `nbf` honoured with 60 s of leeway,
and every configured scope in its `scope`. A token in the query string is never read. `scope`
is read only as a space-separated string; `scp` and an array-valued `scope` are not read, so,
with `scopes` set, a provider that sends `scp` (Okta, for example) needs a claim mapper to
`scope`.

No token, or one that does not pass: 401 with `WWW-Authenticate: Bearer
resource_metadata="<public_url>/.well-known/oauth-protected-resource/<path>"`, plus
`error="invalid_token"` when a token was sent and `scope` when configured. A valid token that may
not read the endpoint's identity, on a pin too: 403 with `error="insufficient_scope"`; missing a
required scope, the same 403 with `scope`. Behind an endpoint the token may read, every answer
covers only the repositories it may read. Authentication runs before routing: the 404 at the root,
on an unknown path and for a generation the node does not hold needs a valid token for that path
(without `audience`, the root's `aud` is `public_url`) and lists only the identities it may read.
No token, and no part of one, appears on stderr or in an error.

**GitHub App** (feature `github-app`, which includes Valkey; `cargo install meada --features
github-app`). Meada still fetches nothing: the indexing job's git fetches the mirrors, and Meada
only answers git's credential request with installation tokens of one or more GitHub Apps, which
it mints and renews. Installations are discovered through the GitHub API, not declared; the load
is spread across the Apps and installations, and a throttled installation is left aside until it
resets. Only the job uses the module: `meada mcp --listen` reads the blocks with the manifest and
never calls GitHub.

```toml
[valkey]                                # required with [[github_app]]
url = "rediss://meada@cache.example.internal:6379"
password_file = "/run/secrets/valkey"

[[github_app]]
host = "github.com"                     # the git host it serves, as git names it
api = "https://api.github.com"          # the REST API base URL; on GHES, https://<host>/api/v3
app_id = 123456
private_key_file = "/run/secrets/github-app.pem"   # or private_key_env, never both
```

There may be several `[[github_app]]` blocks, one per App (two blocks of the same App on the same
host are an error). `api` must be `https` outside loopback, and a block without `[valkey]`, with
no key or both, or with an unknown key refuses to run, naming the key or the section. The private
key comes only from the file or the variable and never appears on stderr or in an error, nor do
the App's JWT and the tokens.

In the job, before the fetch, git uses Meada as the host's credential helper, with the
repository's path in the request (use the manifest's absolute path, since git runs the helper in
the mirror's directory):

```sh
git config --global credential.https://github.com.helper \
  "!meada github-app credential --workspace /etc/meada/meada.workspace.toml"
git config --global credential.useHttpPath true
```

For `https://github.com/<owner>/<repo>`, `get` answers `username=x-access-token`, a token of an
installation that covers `<owner>` and its `password_expiry_utc`. Each App's installations are
listed with its JWT (RS256, issued 60 s in the past, expiring in 9 minutes) and kept in Valkey,
again at most every 10 minutes; a new block joins at the next `get`, with no restart. Among the
installations covering the owner, in a stable order (App id, then installation id), Meada skips
those marked throttled and picks one round robin, through a counter in Valkey. A token is minted
only when Valkey holds none for that installation with at least 10 minutes left; a lock per
installation makes concurrent `get`s mint once, and the token stays cached until 10 minutes
before it expires. Whoever can read that Valkey can read the source for that long. A repository
no installation covers gets no answer, with exit 0 (git tries its next helper) and one stderr
line with the host and the owner. `store` does nothing; `erase`, which git sends after a 401,
drops the refused token, and the next `get` mints another.

Neither git nor GitHub tells the helper about a secondary rate limit (403 or 429). So, after a
fetch that failed that way, the job runs:

```sh
meada github-app throttled --workspace /etc/meada/meada.workspace.toml \
  https://github.com/<owner>/<repo> --retry-after <seconds>
```

This marks the installation that last answered for that repository (Meada remembers it for an
hour) as throttled until `--retry-after` or, without it, for 1 minute, doubling on each repeat
while the mark holds. The module's own REST calls that get 403 or 429 mark their installation (a
mint) or their App (the listing) the same way, from `retry-after`, else from `x-ratelimit-reset`
when `x-ratelimit-remaining` is 0, else for 1 minute; a `get` whose mint was throttled moves on
to the next installation. With git 2.54 or later, `http.maxRetries` makes git itself honour the
`Retry-After` of a 429 on its GET requests.

On GitHub.com and GitHub Enterprise Cloud, `api` lies outside the operator's boundary; that is
accepted because the job's fetches already go to that host (ADR 0013).

## Decisions

An ambiguous link (several providers serve the route) or a dynamic URL (the call builds its URL
at run time) stays an open item. The MCP `search` tool, with the kind `ambiguous` or
`unresolved`, lists these items with how many candidates each has, not the candidates;
`meada decide --list` lists the items with their candidates. You resolve them in human mode, or
a model does if you ask.

- `meada decide` shows each open item of the repository, with numbered candidates; you pick a
  number, or `s` to skip. At the end it writes the picks to `meada.decisions.toml`, at the
  root. Nothing picked, nothing written.
- `--item <id>` works on that item only (the id `meada decide --list` or the `search` tool
  prints), even one already decided, to redo its decision; `--dry-run` prints the file that
  would be written and writes nothing; `--list` prints the open items plus the stale
  decisions and asks nothing; `--prune` removes the stale decisions (it does not combine with
  the previous three).
- `--decider` swaps the person for a model (see below); `--show-request` (only with
  `--decider`) prints every request that would be sent and sends none; `--min-p <p>` (only with
  `--decider`, from 0 to 1) is the least probability for the model to write a decision.

`meada.decisions.toml` is a file versioned in git, like a lockfile: its diff is what you
review. Each `[[decision]]` holds the call (`call`), the chosen provider (`target`), who decided
(`by`: `human` or the model id) and, for a model, the probability (`p`). Indexing only reads
the file back: the decided link gets the level `decided` and its reason says who decided. When
the code changes and a decision no longer fits (the call is gone, the item is no longer open,
the provider is gone or is no longer a candidate), `meada status` reports it as stale, under
its repository, and `meada decide --prune` removes it. A decision waiting for a peer that has
not been found yet is shown apart and kept.

### Model decider

`meada decide --decider` is the only path from Meada to the network, and it runs only when you
ask. Four environment variables configure it (nothing comes from a file):

| Variable | Role | Default |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` or `chat` | `decisions` |
| `MEADA_DECIDER_URL` | base URL | `https://openrouter.ai` (`decisions`); required with `chat` |
| `MEADA_DECIDER_KEY` | API key, sent only in the `Authorization` header | required with `decisions`; optional with `chat` |
| `MEADA_DECIDER_MODEL` | model | `typesafe/jev-1.14` (`decisions`); required with `chat` |

- **`decisions`** (the default) uses OpenRouter's decisions API, with Jev: a single request
  carries every item, and the response gives the probability of each candidate. The request
  asks for zero data retention (`zdr: true`, `data_collection: deny`); if OpenRouter refuses
  that preference, the request fails and nothing is written.
- **`chat`** talks to any endpoint compatible with OpenAI's chat API
  (`<URL>/chat/completions`), a local model included, for example through Ollama:
  `MEADA_DECIDER_PROTOCOL=chat MEADA_DECIDER_URL=http://localhost:11434/v1
  MEADA_DECIDER_MODEL=<model> meada decide --decider`. One request per item. The probability is
  the one the model reports about itself, not a calibrated measure.

Any gateway that speaks either protocol works: just change the URL. HTTPS anywhere; HTTP only
to `localhost` or a loopback address.

What leaves the machine, per item: the call, the function that contains it (read from disk) and
the list of candidates (repository, method and path), plus the id of the repository that makes
the call. No provider code, no other file. `--show-request` shows all of it first, with the key
replaced by `<hidden>`. Each answer is validated per item; an invalid one, or a request that
failed, is reported without writing that item, and the command exits with an error.

Fixed constants:

- `--min-p` defaults to 0.5: below it the proposal is printed as a suggestion and not written.
  It changes only through a PR with data.
- At most 50 candidates per item: with more, the item is skipped in decider mode (human mode
  still offers it).
- 120 seconds per request, from connection to last byte, with no retry.
- With the `decisions` protocol, the probabilities of an answer must sum to 1 within a
  tolerance of 0.01.

## Status

Version 0.1, the first. It indexes TypeScript and JavaScript repositories (`.ts`, `.tsx`, `.js`,
`.jsx`, `.mjs`, `.cjs`) and Python ones (`.py`), reads the Rust code (`.rs`) of the
repository's crates (each `Cargo.toml` with a `[package]`, in any directory, each crate
through the module tree its `mod` declarations build, with no Rust toolchain: a file no `mod`
reaches stays out) with its functions, types, traits, modules and `impl` methods and the calls
written as paths (`crate::a::f()`, `Type::new()`, `Self::f()`), also through `use` (groups,
renames, globs, `pub use` and the other workspace crates by name; method calls (`x.m()`) go through the receiver's type when the code writes it (`let`, parameter, field, `self`, function return; a `dyn Trait` or trait-bounded generic goes to the trait's method; a method the `impl` blocks of a repository type lack, from a `derive` or an outside trait, links to nothing, and a route attribute never links to a function), else by name; calls inside macros are not
indexed, and the Rust contracts are packages, HTTP and channels (below); only each crate's default configuration is read, for a fixed x86_64 Linux target: an item under a false `#[cfg(..)]`, or under a feature outside the `Cargo.toml` `default`, stays out, and `mod` under `#[cfg(test)]` is in), reads Go code (`.go`, with no Go toolchain: an import reaches the package directory through the module path of each `go.mod`, also through a `replace` to a directory of the repository, and a nested module owns its subtree; the files of one directory with one `package`, `_test.go` included, share a scope; build constraints hold for a fixed linux/amd64 target, and a `_windows.go`, a false `//go:build` or a cgo file (`import "C"`) stays out) with its functions, types, interfaces and methods (`Type.Method`) and the calls to functions of the same package and through imports (`pkg.F()`, aliased or dot imports; the standard library and outside modules stay external) and to methods (`recv.M()`: `exact` when the code writes the receiver's type, in a parameter, field, `var`, composite literal or function result, and by name, `inferred` or `ambiguous`, when it does not; a call through an interface stops at the interface method), and links repositories,
also from one language to the other, through HTTP contracts (`fetch` and, in Python, `requests`, `httpx` and `aiohttp` and, in Rust, `reqwest` and `ureq` and, in Go, `net/http` and resty
on the consuming side; Next.js (app router) and Express and, in Python, FastAPI, Flask and
Django and, in Rust, axum, actix-web and Rocket and, in Go, `net/http`, gin, echo, chi and gorilla/mux on the serving side, also behind a gateway; in Rust, a base URL read from the environment with `std::env::var`, `env!` or `dotenvy::var` and, in Go, with `os.Getenv` or `os.LookupEnv` (with a default `if v == "" { v = ... }`, also with `len(v) == 0`, `!ok` or `!ok || v == ""` when `ok` comes from the same `os.LookupEnv`, and through `strings.TrimSuffix`, `TrimRight` or `TrimSpace`) links through `base_url_env`), through
the resources declared in `meada.toml` and through channels: Kafka topics (`kafkajs` and, in Python,
`confluent-kafka` and `kafka-python` and, in Rust, `rdkafka` and, in Go, `segmentio/kafka-go`, `confluent-kafka-go` and `sarama`), RabbitMQ queues (`amqplib` and, in Python, `pika` and, in Rust, `lapin` and, in Go, `amqp091-go`, with a
`publish` to an exchange reaching the queues its bindings tie to it), Amazon SQS queues (AWS SDK
v3, `sqs-consumer` and, in Python, `boto3` and, in Rust, `aws-sdk-sqs` and, in Go, `aws-sdk-go-v2`), NATS subjects (`nats` and, in Python, `nats-py` and, in Rust, `async-nats` and, in Go, `nats.go`, with
the `*` and `>` wildcards), BullMQ jobs and Redis streams (`ioredis`, `node-redis` and, in Python,
`redis-py` and, in Rust, the `redis` crate and, in Go, `go-redis`), with the producer linked
to the handler of each consumer, in the same repository too. A repository with a `[serve]` table
also reads its OpenAPI (3.0 and 3.1) and AsyncAPI (2.x and 3.0) documents, found by name or
listed in `[serve] documents`: the routes and channels they describe become contracts, with the
fields of each response or payload, and `meada status` says what it read and what it ignored.
Each named `package.json` becomes the contract `package::npm::<name>`, and each dependency
(`dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies`) that names a
package of the workspace (from any repository, its own included) links to it, with the version range checked
against the local version (`range mismatch` when it does not accept it, `range unchecked` when
it cannot be checked). A call to a function imported from that package crosses into the
provider's code, through the `exports`, `types`, `module` and `main` fields and through
re-exports, also when the entry point points into the build folder (the `outDir` of
`tsconfig.json`, mapped back to its `rootDir`). `search` with the kind `package` finds the
package by name, with its repository, folder, version and number of consumers, and `context` on
an exported function lists who calls it, in any repository of the workspace. In Python, each
named `pyproject.toml` (`[project]` or `[tool.poetry]`) becomes the contract
`package::pypi::<name>`, the name normalized as PEP 503 does, and each dependency in `[project]
dependencies`, `[tool.poetry.dependencies]` or a `requirements*.txt` that names a package of the
workspace links to it, with the range as written and `range unchecked` (PyPI ranges are not
checked); an `import` of one of that package's modules crosses into the provider's code,
`exact` when the module bears the distribution's name, `inferred` when it is only one of its
top-level modules and `ambiguous` when several expose it. An npm and a PyPI package of the same
name never link. In Rust, each `Cargo.toml` with a `[package] name` (every member of a Cargo
workspace is a provider) becomes the contract `package::cargo::<name>`, and each entry of
`[dependencies]`, `[dev-dependencies]` and `[build-dependencies]` links to it: by `path`, to the
crate in that directory, in the same repository or, when the path leaves it (`../other`), in
another repository of the workspace (the path is resolved from the text alone, against the
repositories' roots, reading nothing outside them and following no symlink; a path that lands in
no workspace repository appears nowhere); by `git`, to the repository whose identity is the
normalized URL; by version, to the provider of the same name in the workspace; `package =` gives
the real name of a renamed dependency, `workspace = true` reads `[workspace.dependencies]` and
`version.workspace = true` reads `[workspace.package]`. The link is `exact`, with the range as
written and `range unchecked` (Cargo ranges are not checked), and a `use` of the crate (by its
`[lib] name`, if any, or by the key of a renamed dependency) reaches the provider's public items
through its `pub use` re-exports, also `exact`, with `; import crate name` ending the reason. A
dependency outside the workspace appears nowhere. In Go, each `go.mod` with a `module` becomes
the contract `package::go::<module path>`, and each `require`, and each `replace` to a local
directory, links to that module's provider in the workspace: a `replace` that leaves the
repository (`../other`) is resolved from the text alone, like Cargo's `path`, and a `replace` by
another module leaves the module out. The link is `exact`, with the `require` version as the
range, `(local none)` (a `go.mod` has no version) and `range unchecked`, and an import of
`<module>/<directory>` reaches the exported functions, types and methods of that directory in the
provider, also `exact` when the directory exists, with `; import module path` ending the reason.
It reads neither `go.sum` nor the module proxy. It reads no lockfile (`Cargo.lock`
included) and queries neither the npm registry, PyPI nor crates.io. Meada also reports shape drift: the fields a `fetch` consumer reads from the parsed response that the route it calls does not return, compared with the route's shape (an OpenAPI document, a zod schema or the object literals of its handler). `trace` ends the link line with `drift: missing <path>`, `context` on a route names where its shape comes from and lists each consumer that reads fields of the response, with its drift and level and the reads not checked under an open part when there are any, `search` with the kind `drift` lists all the drift, and `meada status` counts the missing reads of each repository's consumers (the consumer's repository counts them; the provider's shows nothing). `meada export` carries each repository's communities: groups of symbols that call each other more than they call the rest, found with the Leiden algorithm with nothing random and integer arithmetic only, and named after the directory most of their members live in (`src`, `src#2`), the same on every machine. Each link has the level `exact`, `declared`, `inferred`, `decided` or `ambiguous`; a call whose URL cannot be
resolved shows up as unresolved and becomes a link only through a decision, at the level
`decided`, and a route Meada cannot place stays out of the map rather than being guessed. Broker
clusters are not modelled: two clusters that each have a topic with the same
name link as if they were one. Each route a handler serves, each channel a consumer handler consumes and each exported function or method nothing in its own repository calls is an entry point with its process: `trace` with `process::<entry point identity>` reads the call tree it sets in motion, across repositories, and `meada export` carries the process of each entry point. In Python, a file's `if __name__ == "__main__":` block (the symbol `__main__`) and each function a `[project.scripts]` entry of `pyproject.toml` names (`module:function`) are entry points too, and only public names (no leading `_`, or those of a literal `__all__`) count as exported. In Rust, the `fn main` of each binary (`src/main.rs`, `src/bin/`, `[[bin]] path`) is one too. In Go, each `func main` of a `package main` is one too. `search` with the kind `community` finds a community by name and with the kind `process` finds a process by its entry point's label, and `context` on a symbol names its community (or `community: none`) and the processes it belongs to, found by walking upstream over the same calls and links a process follows. Not there yet: other languages, HTTP clients other than these,
messaging clients other than these, or Meada's npm package. The scope and order of the work are in
[ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md); the decisions are in
[`docs/adr`](docs/adr). See the [CHANGELOG](CHANGELOG.md).

## Why another tool

Anyone who works across several services knows the bug rarely lives in a single repository. The
frontend calls a route that changed in the backend; the worker consumes a queue another team
renamed. Among today's code graph tools, some look at one repository at a time, some link
repositories only when the route text matches exactly, and some forbid commercial use.

Meada wants to follow the thread end to end, across as many repositories as it takes, and say
how confident it is in each link.

## Principles

- **Your code stays with you.** It runs on your machine, with no account, no API key and no
  telemetry.
- **No LLM to index.** The map comes from the code's syntax tree. No per-token bill in dollars;
  it works offline and fits on an ordinary laptop.
- **Deterministic.** The same code with the same configuration always gives the same map. When
  a link is ambiguous, the decision is recorded in a file versioned in git, made by a person
  or, if you choose, by a language model in a separate command. Indexing only re-reads that
  decision.
- **Portuguese first.** Documentation is born in Brazilian Portuguese; English and Spanish are
  translations. Issues and PRs are welcome in any of the three languages. The code is written
  in English so that anyone can read it.
- **Gather without blending.** "Not everything that comes together blends," taught Antônio
  Bispo dos Santos, known as Nêgo Bispo. Each repository keeps its own index; Meada only
  stitches the ends together.
- **Little noise for the agent.** Meada does not write to your `CLAUDE.md`, install hooks or
  touch your editor configuration unless you run a command for it. Answers are short.
- **Credit to those who came first.** Meada learns from GitNexus, CodeGraph,
  codebase-memory-mcp and kivgraph.

## How to take part

The project is just starting. The v1 scope and the order of work are in
[ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md). You can already help:

- open an issue describing your cross-repository use case, in Portuguese, Spanish or English;
- review the translations: if the English or the Spanish sounds off, a fix is welcome;
- read and question the ADRs.

## The name

A *meada* is a skein of thread. *Puxar o fio da meada*, "pulling the skein's thread", means
finding the end that undoes the knot, which is what the tool tries to do when it follows a call
from one repository to the next.

## License

[Apache-2.0](LICENSE).
