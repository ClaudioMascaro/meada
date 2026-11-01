# Meada with Docker Compose

[Português](README.md) | [Español](README.es.md)

## Proof of concept with local accounts

One command starts the generations fleet, Valkey, the store, Postgres with its migration, a
writable console with local accounts and nodes that admit agent tokens (ADR 0024). It needs no
identity provider. Only `127.0.0.1:8443` (nodes) and `127.0.0.1:8444` (console) are published.

```sh
cd deploy/compose
cp .env.accounts.example .env
# Edit .env: random passwords (letters and digits only for Postgres) and the store credentials.
docker compose up -d
docker compose ps
```

It needs Docker Compose v2.24 or newer (`!reset`) and images that already carry `[accounts]`
(ADR 0024, under `[Unreleased]` in the CHANGELOG; the published 1.0.0 does not). Until a release
has them, build the two images from this checkout and point `MEADA_VERSION` at the tag:

```sh
docker build --target runtime -t ghcr.io/claudiomascaro/meada:local ../..
docker build --target cloud -t ghcr.io/claudiomascaro/meada:local-cloud ../..
# In .env: MEADA_VERSION=local
```

`MEADA_TOKEN=unused` stays in `.env.accounts.example` only because `compose.yaml` requires it at
interpolation; this profile's proxy neither receives nor checks it.

The `.env` selects the recipe's files (`COMPOSE_FILE`) and profiles (`COMPOSE_PROFILES`), so
`docker compose up -d` is enough. `depends_on` fixes the order: certificates, Postgres,
`postgres-migrate`, store and Valkey, the `seed` step, the job, the console and the nodes. `seed`
imports and applies `console.repositories.toml` as the first workspace version, only while the
store has no current version (the console does not start before the job's first generation, and
the job needs a version); a restart never replaces what the console saved since.

The demo, from a clean checkout:

1. Copy the console proxy's CA and trust it in the browser, or use `curl --cacert`:
   `docker compose cp console-proxy:/data/caddy/pki/authorities/local/root.crt ./console-ca.crt`.
2. Open `https://localhost:8444` and sign in as `admin` / `admin123`. The default account can only
   change its password: the console sends you straight to the change (12 to 128 characters, not a
   common password).
3. In the profile, generate an agent token for the nodes (1 to 168 hours, 8 by default). It is
   shown once; copy it.
4. Query a node through the proxy, which forwards `Authorization` and holds no static bearer:

```sh
docker compose cp proxy:/data/caddy/pki/authorities/local/root.crt ./ca.crt
curl --cacert ca.crt -i https://localhost:8443/github.com/claudiomascaro/meada
# Expected: HTTP 401, no token.
printf 'Agent token: '; read -r -s MEADA_AGENT_TOKEN; printf '\n'; export MEADA_AGENT_TOKEN
curl --cacert ca.crt -i https://localhost:8443/github.com/claudiomascaro/meada \
  -H "Authorization: Bearer $MEADA_AGENT_TOKEN" -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' -H 'MCP-Protocol-Version: 2025-03-26' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search","arguments":{"query":"main"}}}'
```

Expected: HTTP 200 with the `search` result, over the repositories the account's grant reads (all
of them while the manifest has neither `[accounts.groups]` nor `[accounts.github]`). Do not record
the token in shell traces.

The recipe changes little else: `console.accounts.workspace.toml` (with `[accounts]`, `[postgres]`
and `operations = true`) and `generations.accounts.workspace.toml` (the nodes, with `[accounts]`
holding the public key only, `[postgres]` with the read role and the console's `[ui] public_url`,
the tokens' issuer) replace the manifests by mounts; the profiles below do not change.

### `MEADA_ADMIN_PASSWORD`

Empty (the `.env.accounts.example` default), the `admin` account starts as `admin123` with a
change required at the first sign-in. With a value (12 to 128 characters, not a common password),
that is the first password and no change is required: an automated install never exposes the
published default. The variable is read only when the console creates the account, with an empty
table; an invalid value refuses the start, naming `accounts.admin_password_env`.

### Recovering the default account: `meada ui reset-admin`

A lost password or a locked account (ten failures lock it for fifteen minutes) is recovered
without SQL:

```sh
docker compose run --rm --no-deps --entrypoint meada console ui reset-admin \
  --workspace /etc/meada/meada.workspace.toml
```

It creates or reactivates `admin` as an operator, clears the lock and sets a password with a
change required: the `MEADA_ADMIN_PASSWORD` value when set, else a random one printed once on
stdout. It leaves an audit record (`via` `cli`) and exits 1 when it cannot write it.

### The signing key pair and its rotation

The `certificates` job makes the Ed25519 pair once: the private key in the `accounts-private`
volume, mounted read-only **in the console alone**, and the public key in `accounts-public`,
mounted in the nodes and the console. It prints neither, and a restart keeps the pair. Rotating
means making a new pair, which ends every outstanding token at once (there is no revocation list):

```sh
docker compose down
docker volume rm meada_accounts-private meada_accounts-public
docker compose up -d
```

Everyone signs in again, and each agent needs a new token.

### Rate limiting the sign-in route

Meada locks an account after ten consecutive failures but does not limit by address: unknown
usernames still cost a hash each and grow the audit. Rate limit `POST /api/v1/login` by client
address at the proxy in front of the console (this recipe's Caddy has no rate limit module, so use
a build with `caddy-ratelimit` or an nginx with `limit_req`), keeping `Host`, `Origin` and
`Authorization` unchanged.

### Moving to an identity provider

Configuring `[oidc]` turns local sign-in off for every account. `[accounts]` beside `[oidc]`
needs `break_glass = true`, which keeps only the `admin` account signing in to the console. The
site's identity guides continue from there.

## Static bearer profiles

A local example with two nodes, Caddy TLS and bearer-token admission. Requires Docker
Compose v2 and images from the same published version. Only `127.0.0.1:8443` is published.
Services do not use OIDC and the console is disabled. For remote access, configure your own
DNS, certificate and admission rules in Caddy.

```sh
cd deploy/compose
cp .env.example .env
# Edit .env: published version, random token and, for generations, your own credentials.
docker compose config -q
docker compose up -d
docker compose logs -f local-indexer-one local-indexer-two
```

The default profile, `local`, uses the default image and layout A: each indexer fetches its
mirror and indexes into separate volumes; its server starts only after that job succeeds.
Both servers mount the manifest, mirrors and indexes read-only. They can serve different
commits. To refresh, run `docker compose run --rm local-indexer-one` followed by
`docker compose run --rm local-indexer-two`; servers detect the new indexes. Do not run two
indexers against the same volume simultaneously.

`REPOSITORIES` lists space-separated HTTP(S) Git URLs. When changing repositories, also edit
the `[[repository]]` blocks in the manifests: identity without `.git` and path under
`/srv/mirrors`. Fetching belongs to the job; `meada index` still performs no network requests.
URLs carry no credentials (give git a credential helper). A mirror cloned earlier from a URL
with a token keeps it in `origin` and is refused the same way (#445): run
`git remote set-url origin <URL without credentials>` in it. A failed fetch names the identity
and ends the refresh before `index` and `publish`, keeping the current generation (#316).
Data lives in Docker volumes; `docker compose down` preserves it, and `down -v` deletes it.

## TLS, admission and tools

Wait until servers are `healthy` in `docker compose ps`. Copy Caddy's local CA:

```sh
docker compose cp proxy:/data/caddy/pki/authorities/local/root.crt ./ca.crt
curl --cacert ca.crt -i https://localhost:8443/github.com/claudiomascaro/meada
# Expected: HTTP 401 without a token.
printf 'Bearer token: '; read -r -s MEADA_TOKEN; printf '\n'; export MEADA_TOKEN
curl --cacert ca.crt -i https://localhost:8443/github.com/claudiomascaro/meada \
  -H "Authorization: Bearer $MEADA_TOKEN" -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' -H 'MCP-Protocol-Version: 2025-03-26' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search","arguments":{"query":"main"}}}'
```

Expected: HTTP 200 with a `search` result. Repeated calls alternate nodes by round robin.
With a symbol identity from the result, call `trace` with `{"from":"<identity>",
"direction":"down"}`; expect a call chain and its certainty footer. Use a function that
calls another to see hops. Caddy preserves `Authorization`, MCP headers and
`Meada-Generation`; it does not create `Origin`. Do not use `curl -k` or record the token in
shell traces.

## Shared generations

This profile uses the cloud image and layout B. A periodic job fetches, indexes and publishes
under `snapshot lock`. Valkey holds the pointer, retention and fence; RustFS holds bundles and
documents. Both accept only TLS on the internal network, with no published ports. The
certificate job creates a private CA, discards its key when it exits and mounts only each
service's own key into that service. Clients verify the CA through `ca_file`. Passwords come
from `.env`, never the manifest.

```sh
docker compose down
docker compose --profile generations config -q
docker compose --profile generations up -d
docker compose --profile generations logs -f generation-indexer first-pull-one first-pull-two
docker compose --profile generations ps
```

Select one profile at a time; `--profile generations` overrides `COMPOSE_PROFILES=local` from
`.env`. Each node's first pull must finish before its server starts; a companion then runs
`snapshot pull --every 30` on that node's exclusive volume. The server mounts it read-only.
The healthcheck runs `status --generations`. `REFRESH_SECONDS` controls the job interval,
300 by default; the lock renews its TTL during refresh and receives SIGTERM on shutdown.
Refresh failures appear in the log and are retried on the next cycle.

To verify convergence, repeat the `search` above and record `Meada-Generation` as `G1`:
two alternating responses must name the same generation. Push a new commit to the configured
remote repository, wait for refresh and up to 30 seconds for pulls, then repeat: both nodes
must move to `G2`. Append `@G1` to the same path (`/.../meada@G1`): it must still respond with
`Meada-Generation: G1` while G1 is among the three retained generations. A `trace` pinned to
that URL must keep the same graph after the update. A generation outside retention returns
404 with the current and kept generations. This example does not publish artifacts: image
publication remains tied to the release workflow.

## Optional management console

The console overlay uses the cloud image and requires the `generations` profile. Without the
overlay, no console service, port or volume starts. This example admits a local viewer through
loopback, with operations disabled. Configure OIDC or your own proxy admission before remote
access; a console reachable beyond that admission exposes the graph.

Run from this example's directory with `.env` configured. Edit `console.repositories.toml`
for the initial seed, `REPOSITORIES` for fetching, and `job.workspace.toml` and
`console.workspace.toml` to match the nodes' store, Valkey, prefixes and CA. Seeding is an
explicit, one-time operator action; deployment never reapplies a version automatically (the exception is the local accounts proof of
concept's `seed` step, only while the store has no current version):

```sh
compose_console() {
  docker compose -f compose.yaml -f compose.console.yaml --profile generations --profile console "$@"
}
compose_console config -q
compose_console up -d bucket valkey
VERSION=$(compose_console run --rm --entrypoint meada generation-indexer workspace import \
  /etc/meada/repositories.toml --message "Initial managed workspace" --workspace /etc/meada/meada.workspace.toml)
compose_console run --rm --entrypoint meada generation-indexer workspace apply "$VERSION" \
  --workspace /etc/meada/meada.workspace.toml
compose_console up -d
compose_console logs -f generation-indexer console-first-pull console-pull console
```

The writer uses `[workspace] source = "store"`. The loop holds **one** `snapshot lock` across
`workspace pull`, external fetch, `index` and `snapshot publish`. Fetch still uses
`REPOSITORIES`; it does not derive arbitrary URLs or paths from the editor. Adding a repository
or changing its path requires updating that list, credentials and mirror before refreshing;
a missing mirror prevents publish without advancing the pointer. Applying a version prepares
the next refresh; it reaches a generation after publish and pull. Stop the writer to keep a
generation rollback beyond the next refresh.

The console has its own bootstrap, `index_dir`, `local_dir`, volume and `node_name = "console"`.
Its initial pull completes before startup; a `snapshot pull --every 30` companion updates only
that volume and writes its heartbeat. The two nodes retain separate volumes and identities.
`snapshot.ca_file` adds roots to platform trust; `valkey.ca_file` uses only the file's roots.

The separate TLS route `https://localhost:8444` does not use MCP port 8443's static bearer.
Caddy preserves `Host`, `Origin` and `Authorization`. Assets, `/api/v1/config` and `/auth/callback`
must reach the backend before PKCE login:

```sh
compose_console cp console-proxy:/data/caddy/pki/authorities/local/root.crt ./console-ca.crt
curl --cacert console-ca.crt https://localhost:8444/api/v1/config
curl --cacert console-ca.crt https://localhost:8444/auth/callback
```

For OIDC, uncomment and adjust the policy in `console.workspace.toml`: issuer, claims, groups
and scopes matching the nodes; `[oidc] public_url`/audience describe MCP. `[ui] public_url`,
`client_id` and `audience` belong to the console. Register a public client without a secret,
Authorization Code with PKCE S256 and the exact redirect URI `https://localhost:8444/auth/callback`.
The browser must reach issuer discovery and its token endpoint with CORS for that origin;
it exchanges the code directly with the issuer. Protected API routes require the issuer bearer.
For operations, enable `operations = true` and choose either `operator_groups` or
`operator_claim`; groups require `oidc.groups_claim`. An operator claim must be boolean true.

The local smoke uses a loopback viewer without simulating an IdP. OIDC and its refusals are
covered by HTTP/Rust tests; the chart's OIDC profile passes lint/template. Validate with
`python3 ../../scripts/compose-check` from this directory. Stop with `compose_console down`;
`down -v` also deletes data and requires a fresh seed.

## Postgres and enterprise identity, optional

`compose.postgres.yaml` adds to the console overlay a Postgres 15 with TLS and the one-shot
`meada postgres migrate` step (ADR 0016). Without that file no Postgres starts. Set
`POSTGRES_PASSWORD` and the three roles' passwords in `.env` (`POSTGRES_MIGRATE_PASSWORD`,
`POSTGRES_WRITE_PASSWORD`, `POSTGRES_READ_PASSWORD`; letters and digits only, since they sit in
URLs). The `certificates` job also issues Postgres's certificate (the CA is recreated once if the
`postgres-tls` volume does not exist yet: restart the services afterwards). The roles are created
on the volume's first start by `postgres-init.sh`.

```sh
compose_postgres() {
  docker compose -f compose.yaml -f compose.console.yaml -f compose.postgres.yaml \
    --profile generations --profile console "$@"
}
compose_postgres config -q
compose_postgres up -d postgres postgres-migrate
compose_postgres logs postgres-migrate   # wait for the step to exit 0
compose_postgres up -d
```

`postgres-migrate` runs `meada postgres migrate` as the `meada_migrate` role and finishes before
the job (`generation-indexer`), the console and the nodes, which depend on it. Each reads its URL
from a variable (`MEADA_POSTGRES_URL` with the write role in the job and the console,
`MEADA_POSTGRES_READ_URL` with the read role in the nodes), never from the manifest. Nothing
connects until you uncomment `[postgres]` and the sections that need it in the bootstraps:
`[oidc.github]` and `[[github_app]]` in `job.workspace.toml`, `[oidc.github]`, `[directory]` and
`[oidc.entra]` in `console.workspace.toml`, `[postgres]` and `[oidc.github]` in
`generations.workspace.toml`. With `[oidc.github]` in the job's bootstrap, `managed-refresh.sh`
runs `meada github-app roles` and fetches the mirrors that `meada workspace show --pulled` lists
(`fetch-pulled.sh`); without the section the fetch stays the `REPOSITORIES` one. For
`[directory]`, export `MEADA_SCIM_TOKEN` (at least 32 characters). The Entra ID, Okta and
Keycloak recipes are in the site's documentation.

### Named workspaces

Spec S5 (named workspaces over shared indexing) is opt in on top of the identity sections above:
`[catalog]` and `[workspaces]` (commented in `job.workspace.toml` and `console.workspace.toml`, with
the `[ui]` block the job also needs, since every process that reads `[workspaces]` reads `[ui]` with
`operations = true`). Upgrade every image, the nodes included, before the first refresh that records
a workspace: an older node refuses that generation and keeps its last one. `mirrors = "/srv/mirrors"`
is the job's mirror volume, where `fetch-pulled.sh` clones each repository only named workspaces
select. The proposal server (`meada workspaces mcp`) is not part of this example. The procedure and
its failure modes are in the site's named-workspaces page.
