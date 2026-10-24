# Server mode

> **Warning.** In server mode Meada returns source code to whoever can connect, and by default
> it authenticates nobody: the operator's reverse proxy admits the clients (TLS, tokens, SSO,
> network policy); the [OIDC module](admission.md) is the opt-in alternative. With no host the server listens on the loopback interface only; listening
> beyond it is the operator's explicit choice, and a misconfigured proxy exposes source.
> The MCP server checks no `Host` header (a proxy with a real hostname works) and answers 403 to any
> request carrying `Origin` (browser and DNS-rebinding protection), on every path and before
> any other answer. The proxy must not expose Meada beyond the clients it admits.


<!-- meada-graph:server-request -->

## Manifest and mirrors

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

## Index and serve

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
