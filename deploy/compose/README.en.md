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
