# 13. GitHub App credentials for the indexing job

- Status: proposed
- Date: 2026-10-22

## Context

In server mode (ADR 0011) the operator's indexing job fetches every mirror of the workspace
manifest with its own git credentials, then runs `meada index --workspace` (and, with the
snapshot module, `meada snapshot publish`). For an organisation on GitHub those credentials are
usually a GitHub App's installation tokens, which last an hour and which the operator mints and
renews with a script of their own. Issue #289 asks Meada to do that part.

What was found on 2026-10-22, from GitHub's and git's documentation:

- GitHub documents no per-installation limit on git fetches over HTTPS. The git limits it
  documents are per repository (shared by every App) or, on GitHub Enterprise Server, per
  repository network and per user, off by default. The limits that matter are the secondary
  rate limits, answered with 403 or 429, sometimes with `retry-after`, else with
  `x-ratelimit-remaining: 0` and `x-ratelimit-reset`.
- git calls a credential helper's `erase` only after a 401 sent with a username and password;
  403 and 429 never reach the helper, and no field of the credential protocol carries a status,
  `Retry-After` or `x-ratelimit-*`. `store` runs only after a success. `password_expiry_utc` is
  honoured right after `get`.
- An installation token is minted with `POST /app/installations/{id}/access_tokens` and the
  App's JWT (RS256, `exp` at most 10 minutes ahead, `iss` the App id), lasts an hour, and is
  used by git as the password of the user `x-access-token`. An App's installations are listed
  with `GET /app/installations`. An App has at most one installation per account.

The gain is lower friction, not more throughput: no operator script, tokens minted and renewed
by Meada, the load spread over several Apps and installations, and a throttled installation
left aside until it resets.

## Decision

- A cloud module (ADR 0012): feature `github-app`, off by default, which needs the internal
  `valkey` feature and joins `cloud`, with zero or more `[[github_app]]` blocks in the workspace
  manifest. Each names `host` (the git host it serves), `api` (the REST base URL, `https` unless
  loopback), `app_id` and exactly one of `private_key_file` and `private_key_env` (the App's PEM
  key). `[[github_app]]` without `[valkey]` refuses the start, naming the section.
- **Meada answers git, git fetches.** `meada github-app credential --workspace <manifest>
  <get|store|erase>` is git's credential helper for the job
  (`credential.https://<host>.helper` and `credential.useHttpPath = true`). `get` for a
  repository `<owner>/<repo>` some installation covers answers `username=x-access-token`, an
  installation token and its `password_expiry_utc`. A repository no installation covers gets no
  answer and exit 0, so git tries its next helper, and one stderr line naming the host and the
  owner. `store` does nothing; `erase` drops the cached token git was refused with.
- **Installations are discovered, not declared.** Per App, listed with its JWT and kept in
  Valkey as owner to installation, again at most every 10 minutes; a new block's installations
  are listed on the next `get`. There is no `owners` key and no installation id in the manifest.
- **Selection.** For a host and an owner, the covering installations in a stable order (App id,
  then installation id), those marked throttled skipped, one picked round robin through one
  Valkey counter. The same Valkey state and the same request give the same choice. When every
  one is marked, all are tried: a mark without `retry-after` is an estimate, so refusing would
  turn a guess into an outage, and a fetch that fails again doubles the mark.
- **Tokens in Valkey.** A token is minted only when Valkey holds none for that installation with
  at least 10 minutes left, under a per-installation lock so concurrent `get`s mint once, and
  cached with a TTL that ends 10 minutes before it expires. Whoever can read that Valkey can read
  the sources for that long; the `[valkey]` user and its TLS are what protect them.
- **Throttling is reported, since nothing tells the helper.** Each `get` records which
  installation answered for the repository, for an hour. The job runs `meada github-app
  throttled --workspace <manifest> <repository URL> [--retry-after <seconds>]` after a fetch
  that failed with 403 or 429, which marks that installation throttled until `retry-after`, or
  for 1 minute doubled on each repeat while it holds. The module's own REST calls mark their
  installation (a mint) or their App (a listing) the same way, from `retry-after`, else from
  `x-ratelimit-reset` when `x-ratelimit-remaining` is 0, else for 1 minute. A `get` whose mint is
  throttled takes the next installation, and fails when every mint was.
- **Only the job runs the module.** `meada mcp --listen` reads the blocks with the manifest and
  never calls GitHub. (Amended 2026-10-22; #319: ADR 0016 adds the module's second REST use,
  `meada github-app roles`, which the job runs at every refresh: it lists each repository's
  collaborators with their `role_name` through the installation the selection above picks, under
  the same throttle marks, token cache and round robin, and needs no permission beyond the
  repository permission Metadata (read). The console still never calls GitHub.)
- HTTP goes through the binary's ureq client with the OIDC module's policy (no redirect, no
  proxy, a timeout and a size cap per answer, the system's roots). No private key, JWT or token
  reaches stderr or an error, and every Valkey key lives under the section's `key_prefix`.

## Considered options

- **A documented recipe instead of a module.** No code in Meada, but every operator writes the
  same minting, caching and renewal script, and none spreads load or skips a throttled
  installation.
- **Installation ids, or an `owners` list, in the manifest.** Drifts from GitHub as Apps are
  installed and removed; the API already lists them.
- **Tokens only in memory.** Every helper run is a new process, so every fetch would mint, and
  GitHub's mint is itself rate limited.
- **Reading the throttle from git.** git gives a helper no status and no header, and GitHub
  offers no way to read the secondary-limit state ahead of time, so the job reports it.
- **Meada fetching mirrors itself.** Still rejected (ADR 0011): git fetches, Meada answers its
  credential request.
- **Personal access tokens, OAuth or other forges.** Out of scope for this module.

## Consequences

- ADR 0002, invariant 4, ADR 0011's rejected option "Meada fetching mirrors itself" and ADR
  0012's rule on outbound connections carry inline amendments pointing here.
- On GitHub Enterprise Cloud (and github.com), the configured `api` lies outside the operator's
  boundary. That is accepted because the job's fetches already go to that host; on GitHub
  Enterprise Server it is inside.
- The `github-app` build carries `jsonwebtoken` with `use_pem` (the `pem` and `simple_asn1`
  crates) and signs with RustCrypto's `rsa`, whose RUSTSEC-2023-0071 (Marvin timing) applies to
  private-key operations. The JWT is signed in the job's own process, at most once per App per
  run, where no remote party times it.
- The default build is unchanged and carries none of the module's code or crates.
