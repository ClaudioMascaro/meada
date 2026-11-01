# Deploy

Three ways to run Meada, from a developer's machine to a cluster. The repository's CI checks
each example, with the script named in its section.

## Docker, no server

In the root of the repository:

```sh
docker run --rm --network none --user "$(id -u):$(id -g)" -v "${PWD%/*}:${PWD%/*}" -w "$PWD" ghcr.io/claudiomascaro/meada:1.0.0 index
docker run --rm --network none --user "$(id -u):$(id -g)" -v "${PWD%/*}:${PWD%/*}" -w "$PWD" ghcr.io/claudiomascaro/meada:1.0.0 status
```

- `--user` writes `.meada/` as your user, not the image's (uid 10001).
- `-v` mounts the folder that holds the repository at the same path: sibling peers join the
  workspace, and the path an MCP client sends as its root exists inside the container. For a
  repository without peers, `-v "$PWD:$PWD"` is enough.
- `--network none`: indexing and querying need no network.

The MCP server for your agent is the same line with `-i` and `mcp`, in the Docker tab of
[Agents and MCP](mcp.md). `scripts/container-smoke` runs that line with an MCP client that
sends its root, on every change to the image.

## Docker Compose

`deploy/compose` runs two nodes behind a Caddy with TLS and bearer-token admission, and
publishes only `127.0.0.1:8443`. The default profile, `local`, fetches and indexes the
repositories in `REPOSITORIES` into volumes of their own; the generations and console profiles
are optional.

```sh
git clone --depth 1 --branch v1.0.0 https://github.com/ClaudioMascaro/meada.git
cd meada/deploy/compose
cp .env.example .env    # version, a random token and, if you like, REPOSITORIES
docker compose up -d
docker compose ps       # wait for the servers to be healthy
```

For the proof of concept with [local accounts](console.md#local-accounts), copy `.env.accounts.example` to `.env` and run `docker compose up -d`: the same stack gains Postgres, the writable console with `[accounts]` and the Ed25519 key pair, which the `certificates` step generates once (the private key only in the console; the public one in the console and the nodes). Sign in as `admin` with `admin123` (or the value of `MEADA_ADMIN_PASSWORD`), change the password, generate an agent token on the profile and use it as the bearer on the nodes. The nodes admit that token instead of the proxy's shared bearer. Removing the two key volumes generates a new pair and ends every token.

The `curl` test through TLS, refreshing the indexes and the other profiles are in the
[Compose README](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/README.en.md).
`scripts/compose-check` validates every profile with `docker compose config`.

## Helm

The chart ships with each release at `oci://ghcr.io/claudiomascaro/charts/meada`, at Meada's
version. `values-minimal.yaml` runs one node, without cloud modules, which fetches and indexes a
public repository in its init container:

```sh
curl -fsSLO https://raw.githubusercontent.com/ClaudioMascaro/meada/v1.0.0/deploy/helm/meada/values-minimal.yaml
helm install meada oci://ghcr.io/claudiomascaro/charts/meada --version 1.0.0 -f values-minimal.yaml
kubectl rollout status deployment/meada-meada
kubectl port-forward service/meada-meada 7333:7333
```

In another terminal, a search:

```sh
curl -s http://127.0.0.1:7333/github.com/claudiomascaro/meada \
  -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-06-18' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search","arguments":{"query":"main"}}}'
```

Replace the two blocks of `values-minimal.yaml` with your repositories. Without OIDC or local accounts the server
admits whoever reaches the Service: configure admission ([Authentication and grants](admission.md))
before enabling the Ingress. Generations, the console, local accounts (a writable console with no identity provider,
`values-accounts.yaml`) and enterprise identity are in the
[chart README](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/README.en.md).
`scripts/helm-check` runs `helm lint` and checks what the chart renders with each values file,
this one included.
