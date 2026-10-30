# Meada with Docker Compose

[Português](README.md) | [Español](README.es.md)

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
URLs carry no credentials (give git a credential helper). A failed fetch names the identity and
ends the refresh before `index` and `publish`, keeping the current generation (#316).
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
explicit, one-time operator action; deployment never reapplies a version automatically:

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
