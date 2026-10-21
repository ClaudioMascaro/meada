# 12. Cloud modules for server mode

- Status: proposed
- Date: 2026-10-21

## Context

ADR 0011 serves the graph from one process that the operator refreshes with `meada index`.
Spec #259 then met the deployment its users run: server mode as several containers on
Kubernetes, answering headless agents in orchestrated workflows, whose requests come from a
portal's backend or straight over MCP. Stories 34 to 53 of #259 describe it, and story 16 keeps
the default: no outbound connection in server mode unless the operator configures a module.

Stories 34 to 39 need nothing new in Meada, and #265 to #269 build them: stateless transport, an
index rebuilt beside the old one and renamed, read-only mounts, readiness, a bounded drain and
the supported layouts. Stories 40 to 53 do not fit ADRs 0002 and 0011 as written. The findings
behind them (read on 2026-10-21 against rmcp 3.5.1, SQLite's and AWS's documentation and
Valkey's):

- rmcp's streamable HTTP server keeps sessions in memory by default (`legacy_session_mode:
  true`, `session_store: None`), so a client that initialized on one pod and lands on another
  gets a 404. #262 runs stateless instead, which answers every client that needs no session.
- Protocol 2026-07-28 drops sessions (SEP-2567): rmcp serves it statelessly whatever the
  configuration says. Only clients that negotiate an older protocol and keep an
  `Mcp-Session-Id` still need state shared across pods.
- SQLite over a network filesystem shared by a writer and readers is unsupported: SQLite's own
  guidance reports broken locks and corrupted databases. N pods cannot read one `index_dir` on
  EFS or NFS while a job writes it, so the result of one indexing run has to reach each pod
  whole.
- Without shared state, N pods drift: each pulls or indexes on its own schedule, so two calls of
  one workflow run may read two commits, and nothing stops an overlapping or stalled job from
  replacing a newer result with an older one.
- ElastiCache for Valkey (7.2 and later) supports AUTH/RBAC users over TLS and cluster mode.
  Its IAM authentication signs a token with SigV4, valid for 15 minutes on connections that last
  at most 12 hours; neither redis-rs nor fred signs it.

Two options ADR 0011 rejected come back. **Authentication inside Meada now** was left for later
because the proxy already admits clients; a portal's backend calling on a user's behalf, and an
MCP client that discovers its authorization server, need Meada to tell an expired token from a
forbidden repository. **A snapshot of the whole workspace per refresh** was rejected because
per-repository consistency sufficed for one process; for N pods it is what makes them serve the
same commits. ADR 0011's **Meada fetching mirrors itself** stays rejected.

On 2026-10-21 the maintainer decided two things. ADRs are read by intent: an invariant states
what Meada protects, and its wording may change where the protection still holds. And a
cloud-native, multi-node deployment for headless agents gets opt-in modules, each a cargo
feature plus one manifest section, off by default, with the default build and local use
unchanged. This ADR records the modules and the amendments they need.

## Decision

### What the invariants keep

The intent of ADR 0002 holds for every module: determinism (invariant 6), source within the
operator's boundary (invariant 2), no account with Meada or with a vendor the operator did not
choose, indexing that never touches the network (invariant 1), one index per repository
(invariant 5), and Meada never touching the user's files (invariants 3 and 4). Their letter
changes only where a module is configured, through the inline amendments listed under
Consequences. With no module configured, every ADR reads as before.

### Modules

A **cloud module** is a cargo feature, off by default, plus one section of
`meada.workspace.toml` at the top level, beside `index_dir`.

| Feature | Implies | Section | Needs the sections | Adds |
| --- | --- | --- | --- | --- |
| `snapshot` | | `[snapshot]` | | `meada snapshot publish`, `meada snapshot pull`, `--generations` |
| `valkey` (internal) | | `[valkey]` | | the Valkey connection the next three share |
| `generation-pointer` | `snapshot`, `valkey` | `[generations]` | `[snapshot]`, `[valkey]` | the current and retained generations in Valkey, pins |
| `refresh-lock` | `generation-pointer` | `[refresh_lock]` | `[generations]` | `meada snapshot lock`, fences |
| `session-store` | `valkey` | `[sessions]` | `[valkey]` | MCP sessions in Valkey for older clients |
| `oidc` | | `[oidc]` | | Meada as an OAuth resource server |
| `cloud` | all of the above | | | |

- `valkey` is internal: it is enabled through the modules that need it, and `[valkey]` alone
  only makes `meada status` report the connection.
- A build with a feature and without its section behaves as the default build. The operator
  then does that part by hand, as ADR 0011 says.
- A section whose module needs another section is an error without it, naming both.
- A section in a build without its feature is an error naming the file, the line, the section
  and the feature (`[valkey] needs a Meada built with the valkey feature`). The default build
  holds the table from section to feature that this check reads (#271 builds it); a section the
  table does not list stays an unknown-key error.
- A flag or a subcommand a feature adds (`meada snapshot`, `--generations`) is absent from a
  build without it, so the default CLI help and the MCP surface, and their goldens, do not
  change.
- A build without a feature carries none of its crates, as `cargo tree -e normal` of the default
  build shows. The release binaries stay the default build (ADR 0010).

### Outbound connections

- A module connects only to the endpoints the operator configured, inside the operator's
  boundary: the store's `endpoint`, the Valkey `url`, the OIDC `issuer` and the `jwks_uri` its
  metadata names, plus the credential endpoint of the source `credentials` names (below). No
  client follows a redirect to another host, with one exception: a Valkey cluster client follows
  the cluster's `MOVED` and `ASK` to the nodes the cluster announces, always over TLS (the
  section requires a `rediss` seed when `cluster` is on).
- The store's credentials come from one source, the one `credentials` names; no other is read.
  Besides `static` (keys from the environment or files, below), three sources reach an endpoint
  the operator chose by choosing them, each a recorded exception:
  - `web-identity` (IRSA): the token file and role in `AWS_WEB_IDENTITY_TOKEN_FILE` and
    `AWS_ROLE_ARN`, exchanged with STS in the configured `region` over `https`.
  - `pod-identity` (EKS Pod Identity): the container credentials endpoint in
    `AWS_CONTAINER_CREDENTIALS_FULL_URI`, called with the token read from
    `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`. The URL must name the Pod Identity agent
    (`http://169.254.170.23` or `http://[fd00:ec2::23]`); any other is an error.
  - `instance` (the cloud host's instance metadata service, IMDSv2): `http://169.254.169.254`
    or `http://[fd00:ec2::254]`, used only when a section sets it.
- TLS is required for every endpoint whose host is not loopback (`localhost`, `127.0.0.0/8`,
  `::1`): `https` and `rediss`. A plain `http` or `redis` URL to any other host is an error. The
  only exceptions are the Pod Identity agent and the instance metadata service above, at those
  addresses, which the platform serves only over `http` on a link-local address inside the host;
  no other link-local or `http` endpoint is allowed. Certificates verify against the system
  roots, or only against the `ca_file` when the section names one.
- Secrets come only from environment variables or files a section names, through `*_env` and
  `*_file` keys (at most one of each pair, each with a documented default such as
  `AWS_SECRET_ACCESS_KEY`), with the credential sources above as the only exceptions: STS, the
  Pod Identity agent and the instance metadata service hand out short-lived credentials, which
  Meada keeps in memory only. Secrets never come from the manifest's text: a password inside a
  URL is refused. No secret, and no URL carrying one, appears on stderr or in an error.
- `meada index` opens no connection, whatever the build and the manifest.

### Valkey

- The server is ElastiCache for Valkey (7.2 and later) or a self-hosted Valkey of the same
  versions, reached with an AUTH/RBAC user and a password first. Cluster mode is supported.
- Every key goes through one function that adds the manifest's `key_prefix`, which may not
  contain `{` or `}`, so no prefix can change the hash tag below. The keys one script
  reads or writes together (the pointer, the retained list, the lock and the fence) share the
  hash tag `{<key_prefix>}`, so they land on one slot in cluster mode. Session keys need no tag.
- Valkey holds pointers, a lock and session state, never an index. Nodes learn a new generation
  by polling (`meada snapshot pull --every`), never by pub/sub, which is at most once.

### Generations

- A **generation** is the workspace's mirrors and indexes at one commit each. It is stored as one
  bundle per repository (the clean mirror, `.git` included, and its `index.db` as SQLite wrote
  it), listed by one generation document. Bundles are never merged into one database, so
  invariant 5 holds.
- Ids are 16 lowercase hex characters (`^[0-9a-f]{16}$`), the first 16 of a BLAKE3 hash. A
  bundle id hashes a canonical text of one repository's identity, its `HEAD` commit, the schema
  version, the binary version and its entry blocks (`serve`, `resource`, `peer`; not `path`). A
  generation id hashes the sorted lines `<identity> <bundle id>`. Same inputs give the same ids
  on any machine (invariant 6). Any id read from the store, from Valkey or from a pointer file is
  checked against the pattern before it becomes part of a path or a key.
- Publishing is a separate command, `meada snapshot publish --workspace <manifest>`, run after
  `meada index --workspace`. It checks, reading only, that every index equals its mirror and
  every mirror is clean, refuses otherwise naming `meada index --workspace`, uploads the bundles
  and the document, and writes the pointer last. It writes nothing on the machine it runs on.
- An id names inputs, not bytes: two pods publishing the same commits build tar files that
  differ (mtimes, SQLite pages) under the same bundle id. So each bundle is uploaded with its
  size and BLAKE3 hash as object metadata, and the generation document records the stored
  object's size and hash, read back from that metadata, never the local file's. A bundle id
  already stored is not uploaded again; the document takes that object's metadata. A generation
  document, once stored, is never rewritten: a publish whose generation id exists uploads
  nothing, checks that the document and every bundle it lists are stored with the recorded size
  and hash, and only moves the pointer to it when it is not already current. Two publishers at
  once are kept apart by the operator's scheduler or by the refresh lock.
- Nodes pull: `meada snapshot pull` treats everything it reads from the store as hostile input
  (ids, identities, archives, the mirrors' git configuration), verifies sizes and hashes against
  the document, extracts into its `local_dir`, and swaps the node's pointer file by rename, so a
  call in flight finishes on the generation it started on.
- With the snapshot module, nodes never run `meada index`. `--generations` serves the
  generation the node's pointer file names, read per call, with the repositories and blocks the
  generation recorded; the node's manifest contributes its module sections. Readiness depends
  only on what the node holds: a store or Valkey that cannot be reached changes no exit code and
  stops no answer outside a legacy session.
- Without the generation pointer, the store holds the current pointer and a node keeps its
  current and previous generations. With it, Valkey holds the current generation and a retained
  list that every node mirrors, so the fleet holds one set; with the refresh lock, each pointer
  write carries a fence, and neither the Valkey pointer nor a node's ever moves to a lower one.
- Meada never lowers `{<key_prefix>}:fence`. A Valkey flush or a failover that loses it makes
  the next fences lower than the ones recorded, and the fleet stops moving: the pointer script
  refuses them, or nodes refuse to swap. The operator recovers by setting
  `{<key_prefix>}:fence` at or above the highest fence recorded (in Valkey or in any node's
  pointer file), or by deleting a node's pointer file, after which that node takes the current
  generation whatever its fence. A refused publish names the fence key and that recovery, and
  `meada status` on a node names a node fence above Valkey's as the stuck case.
- A workflow run pins a generation in the path: `/<identity>@<generation>`, the generation being
  what follows the last `@`. Pins are served with the generation pointer, which guarantees every
  node holds the retained set. `/<identity>` keeps serving the current generation. A generation
  the node does not hold answers 404, naming the requested generation, the current one and the
  held ones. Each answer in `--generations` mode names its generation in a `Meada-Generation`
  header and in the result's `_meta` (`meada/generation`). Tools, parameters, descriptions and
  text output are unchanged (ADR 0008).

### Sessions

Stateless is the default (#262) and stays the transport for every client on protocol
2026-07-28, which rmcp serves with no session whatever the configuration. The session-store
module turns rmcp's stateful mode on with a Valkey store only for clients that negotiate an
older protocol and keep a session. It becomes dead weight as clients move, and its ticket runs
last of the tail, to be closed if no such client needs it by then.

### OIDC

- The OIDC module makes Meada an OAuth resource server as the MCP authorization specification
  (2026-07-28) describes: protected resource metadata (RFC 9728) per endpoint, served without a
  token at `/.well-known/oauth-protected-resource/<identity>` and at its pinned form; a bearer
  JWT validated for signature (asymmetric algorithms only, against the issuer's cached JWKS),
  issuer, audience (the endpoint's canonical URL by default, RFC 8707, which a pin of it also
  accepts) and lifetime; and one claim listing the identities a token may read.
- A missing or invalid token gets 401 with `WWW-Authenticate` naming the metadata; a valid token
  for a repository it may not read gets 403. Authentication runs before routing (after the
  `Origin` check of #262), so no 404 lists an identity or a generation to a caller without a
  valid token, and a valid token's 404 lists only what it may read.
- No token passthrough: Meada never forwards a token, issues none, registers no client and is no
  authorization server. No stderr line or error carries a token or any part of it.
- The issuer's metadata and JWKS are read at start, before the listener binds, through the HTTP
  client the binary already has; a failure refuses the start.
- The reverse proxy stays the documented default admission. The documentation states it first,
  with the OIDC module as the alternative.

### Tests and gate

- Each module is tested against local services: a Valkey and an S3-compatible server named by
  `MEADA_TEST_VALKEY_URL` and `MEADA_TEST_S3_ENDPOINT`. Without the variable a test prints why
  and skips; with `MEADA_TEST_SERVICES_REQUIRED=1` a missing service fails it. Each section is
  also tested in the default build, where it is the error naming its feature.
- CI has a `cloud` job that runs both services as containers and runs clippy and the tests with
  `--all-features`, plus a step that fails when `cargo tree -e normal` of the default build lists
  a crate a module added. Each module ticket adds its crates to that list.
- A change under a feature passes the gate with that feature too (`AGENTS.md`, Workflow).

### Open

- **ElastiCache IAM authentication.** A SigV4-signed token valid for 15 minutes, on connections
  limited to 12 hours, which neither redis-rs nor fred signs. It needs AWS SDK crates and a
  connection that renews itself, and gets its own ticket when an operator needs it; until then
  the AUTH/RBAC password is the way in.

## Considered options

- **Valkey only in the operator's wrapper, never in the binary.** Keeps Meada offline, but a
  wrapper cannot route a pin to a generation, fence a pointer the server reads, or keep the
  fleet on one retained set. The modules do it, opt-in.
- **Modules always compiled, enabled by their section.** Every user would carry the network
  crates and code a module needs, and a typo in a manifest could open a connection.
- **One `cloud` feature and no finer split.** An operator who wants only OIDC would build a
  store client and a Valkey client. `cloud` exists as the shorthand for all.
- **The index in Valkey, or SQLite read from the object store.** One key per index on one shard
  with a size cap, no streaming read, and network in every query; it replaces ADR 0004's
  storage.
- **`index_dir` on a network filesystem shared by every pod.** SQLite's guidance against it.
- **Publishing from inside `meada index`.** Puts the network in indexing (invariant 1).
- **Every node indexing its own mirrors.** Supported without modules (#269), at the cost of N
  fetches, N index runs and nodes that drift. The modules add the other layout.
- **The pointer in the store only, with no Valkey.** The snapshot module alone does that, but
  a fenced write needs a compare-and-set that S3-compatible stores do not all offer.
- **Valkey pub/sub to announce a generation.** At most once: a node that was disconnected
  misses it.
- **A bundle id from the tar's bytes.** Two pods at the same commits would name one bundle twice,
  so ids would stop being deterministic and every re-run would upload again.
- **A merged workspace bundle.** Breaks invariant 5 for no gain, since links derive at query
  time.
- **The pin in a header or a tool parameter.** ADR 0011's reasons for the repository hold for
  the generation, and a parameter changes ADR 0008's tools.
- **The generation in the tool's text output.** Changes every answer and every golden; the
  header and `_meta` carry it outside the text.
- **A session store by default, or sticky sessions required.** Stateless answers every client on
  2026-07-28 and every older one that keeps no session; sticky routing remains the operator's
  choice and needs nothing in Meada.
- **Instance metadata credentials by default.** A connection to an endpoint the operator did not
  configure.
- **Secrets in the manifest.** The manifest is reviewed in pull requests.
- **Token introspection (RFC 7662) instead of JWT validation.** An outbound call per request.
- **Meada as an authorization server, or with dynamic client registration.** A security surface
  the operator's provider already covers.
- **ElastiCache IAM authentication now.** Left open (above).
- **A container image with this ADR.** Distribution is ADR 0010's; an image is a separate
  decision.

## Consequences

- ADR 0002, invariants 1 to 4, carry inline amendments pointing here; invariants 5 and 6 hold as
  written. ADR 0011 carries inline amendments on outbound connections, authentication, the pin,
  the 404 rule and the snapshot option.
- ADR 0008 is unchanged: three tools with their parameters and descriptions, the pin in the
  path, the generation in a header and in `_meta`.
- ADR 0004 is unchanged: the SQLite file moves as is, built beside the old one and renamed
  (#265); there is no new format.
- ADR 0010 is unchanged: the release binaries are the default build. An image, or a release
  built with `cloud`, is a separate decision.
- Spec #259 story 16 holds with no module configured, in any build.
- The tail is cut as: the Valkey connection and the section table (#271); the snapshot store in
  three tickets, publish (#272), pull (#273) and serving (#274); the generation pointer and pins
  (#275); the refresh lock (#276); the session store (#277, last, closed if unneeded); OIDC
  (#278).
- New crates enter only behind a feature, and CI checks the default build for them. The gate of
  a change under a feature runs clippy and the tests with it.
- The operator owns the store, Valkey and the OIDC provider. The store's and Valkey's
  availability affects publishing and pulling: a node keeps serving what it holds. Two answers
  depend on a service: with the session-store module, a Valkey error during a session load
  answers 503 to that legacy session; with the OIDC module, a provider unreachable at start
  refuses the start, and a key rotation reaches Meada only while the provider's JWKS can be
  read.
