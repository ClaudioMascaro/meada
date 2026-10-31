# Snapshots

**Snapshot** (feature `snapshot`, `cargo install meada --features snapshot`). One job indexes and publishes; nodes receive the prepared generation. Explore the sequence below, then configure the store to publish and receive bundles.


<!-- meada-graph:snapshot-lifecycle -->

<details markdown="1" class="reference-details">
<summary>Pre-publication checks and generation format</summary>

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

</details>

## Configure the store

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

## Credentials and transport

<details markdown="1" class="reference-details">
<summary>Choose the credential source</summary>

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

</details>

`ca_file` adds PEM certificates to the system trust roots. The file holds at most 1 MiB;
invalid certificates and an empty file are refused, naming the file.

## Receive a generation

On each node, `meada snapshot pull --workspace <manifest>` reads the store's `current` pointer
and, when it names a generation the node lacks, downloads its bundles into `local_dir`, checks
each one's size and BLAKE3 hash against the document, extracts them and swaps the node's pointer
by rename. The only line on stdout is the current generation id. `--every <seconds>` repeats the
pull (printing only when the pointer moves); a second pull on the same `local_dir` exits 0 saying
one is running. The node keeps the current and the previous generation and deletes older ones.

## Validate bundles

<details markdown="1" class="reference-details">
<summary>Bundle validation and isolation rules</summary>

Everything read from the store is treated as hostile input: ids and identities are checked before
they become paths; the pull refuses an archive entry with an absolute path or a `..`, a link that
leaves its repository, an entry written through a symbolic link, and devices or FIFOs, with byte
and entry ceilings per generation. The extracted `.git/config` is replaced by a minimal one Meada
writes (only `[core]` and the identity's `origin`), `.git/hooks` is removed, and Meada's `git`
runs in those mirrors without the system or global configuration. A pull that fails at any step
leaves the pointer and the held generations as they were. When serving, and when a
server-mode manifest lists a mirror, its `meada.toml` and `meada.decisions.toml` are read only as
regular files of at most 1 MiB: a symbolic link is refused unopened, even one into the mirror, and
the generation or the entry is not servable, the error naming the file. A local checkout, which
the developer owns, still follows links.

</details>

```
<local_dir>/
  current                     # "<generation id> <fence>", fence 0 without the refresh lock
  pull.lock
  <generation id>/
    generation.json           # the document, as stored
    <bundle id>/index.db
    <bundle id>/mirror/       # the checkout, .git included
```

## Serve without indexing

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

## Readiness

`meada status --workspace <manifest> --generations` is the node's readiness probe. It prints the
current generation id, the held ids and a line `store: reachable (current <id>)` or
`store: unreachable (<reason>)`. The exit code depends only on local state: 1 without a
servable current generation (the problems go to stderr), 0 otherwise, the store reachable or
not, so a stopped store does not take the node out of rotation. `meada export --workspace
<manifest> --repo <identity> --generations` exports the same way.
