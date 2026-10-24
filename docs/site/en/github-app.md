# GitHub App credentials

**GitHub App** (feature `github-app`, which includes Valkey; `cargo install meada --features
github-app`). Meada still fetches nothing: the indexing job's git fetches the mirrors, and Meada
only answers git's credential request with installation tokens of one or more GitHub Apps, which
it mints and renews. Installations are discovered through the GitHub API, not declared; the load
is spread across the Apps and installations, and a throttled installation is left aside until it
resets. Only the job uses the module: `meada mcp --listen` reads the blocks with the manifest and
never calls GitHub.


<!-- meada-graph:app-credentials -->

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

## Apps and installations

There may be several `[[github_app]]` blocks, one per App (two blocks of the same App on the same
host are an error). `api` must be `https` outside loopback, and a block without `[valkey]`, with
no key or both, or with an unknown key refuses to run, naming the key or the section. The private
key comes only from the file or the variable and never appears on stderr or in an error, nor do
the App's JWT and the tokens.

## Configure the helper

In the job, before the fetch, git uses Meada as the host's credential helper, with the
repository's path in the request (use the manifest's absolute path, since git runs the helper in
the mirror's directory):

```sh
git config --global credential.https://github.com.helper \
  "!meada github-app credential --workspace /etc/meada/meada.workspace.toml"
git config --global credential.useHttpPath true
```

<details markdown="1" class="reference-details">
<summary>Installation selection, token caching and renewal</summary>

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

</details>

## Rate limits

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
