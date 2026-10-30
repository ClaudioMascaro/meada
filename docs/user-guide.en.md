# User guide

> This is a translation. The Brazilian Portuguese [guide](user-guide.md) is the source; if they
> disagree, it wins, and a fix to this page is welcome.

This guide takes an English-speaking person from zero to a working Meada: install it, connect an
agent, configure a workspace, decide what the code leaves unclear, use the command line and read
the export. The examples use the repositories `web`, `api` and `worker` and the host
`api.example.com`.

The second half (sections 8 to 11) explains what Meada links and where it stops: each contract
kind, each language, shape drift, communities and processes. Use it to tell whether a missing link
is a known limit or a bug.

Contents:

1. [Installation](#1-installation)
2. [Connect an agent (MCP)](#2-connect-an-agent-mcp)
3. [The workspace and first use](#3-the-workspace-and-first-use)
4. [`meada.toml`: reference](#4-meadatoml-reference)
5. [Decisions and deciders](#5-decisions-and-deciders)
6. [CLI commands](#6-cli-commands)
7. [The export](#7-the-export)
8. [Contract kinds](#8-contract-kinds)
9. [Languages](#9-languages)
10. [Shape drift](#10-shape-drift)
11. [Communities and processes](#11-communities-and-processes)

## 1. Installation

There are five channels, and all of them deliver the same `meada` binary. Pick the one that fits
your environment.

### Cargo

```sh
cargo install meada
```

It builds from crates.io and needs the Rust toolchain.

### Prebuilt binary

Download the archive for your system from the
[GitHub releases](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS and Windows, on
x86_64 and arm64), extract it and put `meada` on your `PATH`.

### Shell installer

For Linux and macOS:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh
```

### npm (`npx meada`)

```sh
npx meada --version
npx meada index
```

The `meada` npm package holds only the installer: it downloads the binary for your platform from
the GitHub release and runs it with the arguments you passed. The package's version is always the
release's. The install downloads the binary on the spot, so it needs a network (there is no
offline install through npm). It is the channel that matters most to whoever configures an agent,
because `npx meada mcp` is the launch line of the MCP server (section 2).

### Container

```sh
docker run --rm ghcr.io/claudiomascaro/meada:latest --version
```

`ghcr.io/claudiomascaro/meada:<version>` is the default build and `<version>-cloud` carries every
cloud module, both for Linux amd64 and arm64. The image runs without root and defines no command:
pass the arguments explicitly. To serve an organisation, see
[Server mode](https://claudiomascaro.github.io/meada/en/server/).

### What Meada needs

Only the binary and `git`. Indexing reads the repository's files without running the code or the
language's toolchain, uses no LLM and opens no network. Check the install with `meada --version`.

The management console (an optional module, `--features ui`) and the other cloud modules have
their own installation, described in
[Installation](https://claudiomascaro.github.io/meada/en/installation/).

## 2. Connect an agent (MCP)

`meada mcp` speaks MCP over stdio. The client (the agent) runs the command and talks to it. The
documented launch line is `npx meada mcp`, which needs no separate install.

### Client configuration

In Claude Code's `.mcp.json`, or your client's equivalent file:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["meada", "mcp"] }
  }
}
```

In Claude Code, the same in one command: `claude mcp add meada -- npx meada mcp`.

With `meada` already on your `PATH` (through Cargo, the binary or the installer), swap the command
for `"command": "meada", "args": ["mcp"]`, which avoids the npm lookup on every start.

### Which repository the server answers about

The server answers about the first root (`roots`) the client sends. Claude Code sends the
session's folder, so opening a session in `web` makes Meada answer about `web`, even if the server
was started somewhere else. Without roots, it answers about the folder it was started in, which
must be inside the repository. If the client does not answer `roots/list` within 5 seconds, the
call uses the last root sent on that connection or returns an error that says so; it never falls
back to the server's folder for that reason.

Every query first refreshes the index with what changed in the files, including what is not yet
committed. There is no reindex step for the agent.

### The three tools

There are three, with names and descriptions in English, and none of them writes. Answers are
compact lines inside a token budget (`budget`, 2,000 by default, adjustable on each call; a token
is four bytes of output). When an answer goes over the budget, it ends with
`… cut after N of M lines (budget B tokens); raise budget for more`.

#### `search`: find the starting point

| Parameter | Type | Default | Role |
| --- | --- | --- | --- |
| `query` | text, required | | The name to look for, case-insensitive |
| `kind` | text | all kinds | Restricts the kind of result |
| `budget` | integer | 2000 | Token budget of the answer |

The values of `kind`:

| `kind` | Looks for |
| --- | --- |
| `symbol` | a symbol by name |
| `contract` | a contract by identifier (`http::GET::/orders`) |
| `route` | an HTTP contract |
| `resource` | a declared resource, by kind and id |
| `channel` | a queue or a topic, by broker and name |
| `document` | an OpenAPI or AsyncAPI file, by path or title |
| `package` | a package by name, with its consumers |
| `drift` | fields that consumers read and a route does not return, by route or field path |
| `community` | a community by name |
| `process` | a process by the label of its entry point, with the `process::` identity `trace` starts from |
| `duplicate` | symbols with identical text in two or more places, by name or path |
| `ambiguous`, `unresolved` | the open items, with the `meada decide` command for each |

Each result is one line: repository, `path:line` and name.

#### `context`: understand a symbol

| Parameter | Type | Default | Role |
| --- | --- | --- | --- |
| `symbol` | text, required | | A name, `<path>::<name>` or the full identity; a resource (`resource::postgres::orders`) or a channel (`channel::kafka::orders.created`) |
| `budget` | integer | 2000 | Token budget of the answer |

It returns the symbol's community and processes, the code excerpt read from disk, who calls it
(`callers:`), who it calls (`callees:`) and the contracts it touches (`contracts:`). For a
resource, it lists who writes and who reads; for a channel, the producers and consumers.

```text
example.com/acme/web src/status.ts:2-4 health
community: src#3
processes: 1 (1 export; 1 repository): dashboard (example.com/acme/web)
function health() {
  return fetch("/api/health");
}
callers:
example.com/acme/web src/dashboard.ts:5 dashboard
callees:
example.com/acme/web src/status.ts:3 (unresolved) fetch
contracts:
example.com/acme/web src/status.ts:3 consumes http::GET::/api/health →[exact: host api.example.com is served by example.com/acme/api] example.com/acme/api::src/server.ts::GET /api/health
```

#### `trace`: follow a chain

| Parameter | Type | Default | Role |
| --- | --- | --- | --- |
| `from` | text | | The start: a symbol, a route (`GET /users/{}` or the contract), a call site or a process (`process::<identity>`, downstream only) |
| `changes` | text | | `uncommitted`: starts from the symbols the working-tree diff touches |
| `direction` | `down` or `up` | `down` | `down` is what the start reaches, `up` is what reaches it |
| `budget` | integer | 2000 | Token budget of the answer |

Pass `from` or `changes: uncommitted`, never both. The second serves to measure the impact of
what was just edited, and works only in local Meada (server mode has no working tree).

```text
example.com/acme/web src/orders.ts:5 listOrders
  example.com/acme/web src/api/client.ts:4 request
  →[inferred: base URL from API_URL, a base_url_env of peer example.com/acme/api] example.com/acme/api src/orders.ts:5 listOrders
chain: inferred · 1 hops · 2 repos · 93 tokens
```

(Illustrative example: the footer writes `hops` even for one hop, and the token count varies with
the answer.)

#### How to read the levels

Every link between repositories carries `→[level: reason]`. The level says how firm the link is:

| Level | Meaning |
| --- | --- |
| `exact` | certain: the code or the configuration proves the link |
| `declared` | comes from a declaration of yours in `meada.toml`, such as a shared resource |
| `inferred` | a match by name or by convention |
| `decided` | comes from `meada.decisions.toml`; the reason says who decided |
| `ambiguous` | several candidates fit, and Meada lists all of them without choosing |

`trace` ends with a footer: the weakest level of the chain, the number of hops, the repositories
involved and the tokens used. A chain is worth as much as its weakest link. A missing caller is
"unproven", not "nonexistent": calls inside macros (such as `assert!` and `format!` in Rust) are
not indexed.

## 3. The workspace and first use

The **workspace** is the set of repositories a query sees. It gathers the repository where Meada
runs, the peers its `meada.toml` names (and their peers, transitively) and the sibling git
clones (in the same folder as a repository of the set) whose own `meada.toml` names a repository
already in it. Discovery reads only local files. A repository is identified by the git remote of
`origin`, without scheme or `.git` (`example.com/acme/api`), so the clone can sit at a different
path on each machine.

In the root of `web`:

```sh
meada index
meada status
```

- `meada index` builds the index of each repository in the workspace, in `.meada/` inside it. It
  is optional before querying, since every query refreshes the index, but it makes the first
  answer fast.
- `meada status` shows each repository, the state of its index and the peers that were not found
  (`missing peer: example.com/acme/api, named by example.com/acme/web`).

For `web` to see `api`, the `meada.toml` of `web` declares the peer (section 4). Indexing writes
only inside `.meada/`, which Meada keeps out of git; the only other file it writes is
`meada.decisions.toml`, and only when you run `meada decide`.

### What is indexed

The scan covers every file in the repository, tracked or not, except:

- **Hidden**: files and directories whose name starts with `.` are not read.
- **Always ignored**: `node_modules` and `Pods` directories, a `Checkouts` inside `Carthage`,
  `*.min.js` and `*.min.css` files and the .NET build output (`obj` or `bin` next to a `.csproj`).
- **`.gitignore` and `.ignore`**, respected with or without git, at the root and in
  subdirectories.
- **`.meadaignore`**, with the `.gitignore` syntax. It does not bring back what the always-ignored
  list above excludes; a `!` pattern in it, however, re-includes what `.gitignore` ignores.

`vendor`, `dist`, `build` and generated code are not ignored by default, because any of them may
be your own code. List in `.meadaignore` what you do not want indexed:

```gitignore
vendor/
dist/
Migrations/
*.Designer.cs
```

A file that a new rule excludes leaves the index on the next refresh, like a deleted file.

## 4. `meada.toml`: reference

Each repository may have a `meada.toml` at its root, versioned in git, that says who it talks to.
The file is optional: without it, the repository declares no peer. There are three kinds of
table, `[[peer]]`, `[serve]` and `[[resource]]`, and no other. An unknown or misspelled key is an
error that names the file, the line and the key.

```toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]

[serve]
hosts = ["web.example.com"]

[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

### `[[peer]]`: what this repository consumes from

Repeatable, one table per peer. Whoever consumes a route names the peer.

| Key | Type | Required | Role |
| --- | --- | --- | --- |
| `repo` | text | yes | The peer's identity: the git remote of `origin`, without scheme or `.git` |
| `path` | text | no | A hint of where the clone usually sits, relative to this repository's root |
| `base_url_env` | list of text | no (empty) | Variables this repository reads the peer's base URL from |

```toml
[[peer]]
repo = "example.com/acme/api"
path = "../api"
base_url_env = ["API_URL"]
```

Meada never reads the variables' values, only their names. When the code builds a URL from
`API_URL`, the peer that declares `API_URL` in `base_url_env` is the provider, with level
`inferred` and the reason `base URL from API_URL, a base_url_env of peer example.com/acme/api`.

### `[serve]`: what this repository serves

A single table. Whoever serves routes declares the hosts it answers on and how a gateway rewrites
the paths.

| Key | Type | Required | Role |
| --- | --- | --- | --- |
| `hosts` | list of text | no (empty) | The hosts this repository answers on |
| `prefix` | text | no | What a gateway puts in front of every served path |
| `strip` | text | no | What a gateway removes from the start of every served path |
| `documents` | list of text | no (empty) | OpenAPI and AsyncAPI documents besides those found by file name |

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"
strip = "/v1"
documents = ["docs/openapi.yaml"]
```

- **`hosts`**: a call to `https://api.example.com/...` links to the repository that lists that
  host, at level `exact`.
- **`prefix` and `strip`**: must start with `/`. Meada normalizes the value and drops the trailing
  slash; a lone `/` rewrites nothing. With the two above, a call to `/shop/orders` reaches the
  code's `/v1/orders` route, at level `inferred`, with the reason
  `gateway prefix /shop, gateway strip /v1`. `strip` matches whole segments (`/v1` does not strip
  `/v10`), and a version segment only comes off when you write it. A caller that uses the code's
  own path keeps linking as before.
- **`documents`**: paths relative to the repository root, with no `..` and no absolute path.
  Meada already finds OpenAPI and AsyncAPI documents by file name; use the key for those with
  another name. A listed document is read even if `.gitignore` ignores it.
- The mere presence of the `[serve]` table, even empty, makes the repository's documents count as
  a source of providers.

### `[[resource]]`: shared resources

Repeatable. A resource that no code names, such as a table or a bucket, goes here.

| Key | Type | Required | Role |
| --- | --- | --- | --- |
| `kind` | text | yes | The kind, a lowercase identifier (letters, digits and hyphens, starting with a letter) with no meaning to Meada: `postgres`, `s3-bucket` |
| `id` | text | yes | The resource's id, not empty |
| `role` | `read`, `write` or `read-write` | yes | What this repository does with it |
| `via` | list of text | no (empty) | The symbols that do the access, as `<path>::<qualified name>` |

```toml
[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]
```

Each writer links to each reader of the same resource in another repository, at level `declared`.
Without `via`, or if no `via` matches a symbol, the side is the whole repository. `meada status`
shows, under each repository, the unlinked resources and the `via` entries that match no symbol.
The resource's identity in `search` and `context` is `resource::postgres::orders`.

### A complete example: `web`, `api` and `worker`

`web` consumes the API:

```toml
# web/meada.toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]
```

`api` serves routes behind a gateway and writes to a table:

```toml
# api/meada.toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"
strip = "/v1"

[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]
```

`worker` consumes `api` and reads the same table:

```toml
# worker/meada.toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]

[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

Running `meada status` in `web`, Meada finds `api` through the git remote (or through `path`, if
there is one), and `worker` joins as a sibling clone of `web` or of `api`, because its
`meada.toml` names `api`, which is already in the workspace. `worker` reads the `orders` table
that `api` writes, and Meada links the two through the resource, at level `declared`.

### In server mode

In server mode, a mirror without a `meada.toml` receives the same `serve`, `resource` and `peer`
blocks (the last without `path`) through the `meada.workspace.toml` manifest. The repository's own
`meada.toml`, when it exists, applies in full and the manifest's blocks are ignored. See
[Server mode](https://claudiomascaro.github.io/meada/en/server/).

## 5. Decisions and deciders

An **ambiguous link** (several providers serve the route) or a **dynamic URL** (the call builds
the URL at run time) stays an open item. Meada does not guess: it lists the candidates and waits
for a decision. The one who decides is you, or a model, if you ask for it.

To see the open items:

- over MCP, `search` with `kind: ambiguous` or `kind: unresolved` lists each item with its number
  of candidates;
- on the CLI, `meada decide --list` lists the items with numbered candidates.

### Deciding as a person

`meada decide`, in the repository's root, shows each open item with numbered candidates. Pick a
number, or `s` to skip. At the end, the command writes the choices to `meada.decisions.toml`; if
you chose nothing, it writes nothing.

```text
example.com/acme/web::src/status.ts::health::fetch#0
src/status.ts:3  fetch("/api/health")
  1) example.com/acme/api GET /api/health
  2) example.com/acme/worker GET /api/health
```

### The `meada.decisions.toml` file

It sits at the root and is versioned in git like a lockfile, and its diff is what gets reviewed.
Indexing only rereads it: no decision goes through a model or the network at that point.

```toml
[[decision]]
kind = "ambiguous-link"
call = "example.com/acme/web::src/status.ts::health::fetch#0"
target = "example.com/acme/api::http::GET::/api/health"
by = "human"

[[decision]]
kind = "dynamic-url"
call = "example.com/acme/web::src/status.ts::track::fetch#0"
target = "example.com/acme/worker::http::POST::/api/events"
by = "example/model"
p = 0.62
```

| Key | Type | Required | Role |
| --- | --- | --- | --- |
| `kind` | `ambiguous-link` or `dynamic-url` | yes | The item's kind: several providers, or a URL Meada does not evaluate |
| `call` | text | yes | The call site, as `meada export` and `meada decide --list` name it |
| `target` | text | yes | `<provider repository identity>::<contract identifier>` |
| `by` | text | yes | `human`, or the model's id |
| `p` | number from 0 to 1 | only for a model | The probability the model declared; forbidden with `by = "human"` |

The file has one entry per `call`: repeating a call is an error that names the line. The decided
link gets level `decided`, and the reason shows who decided.

When the code changes and a decision no longer fits (the call is gone, the item is no longer open,
the provider is gone or is no longer a candidate), `meada status` shows it as **stale** under the
repository, and `meada decide --prune` removes it. A decision waiting on a peer not yet found
appears separately (`decision waiting on missing peer ...`) and is kept.

### Model decider

`meada decide --decider` swaps the person for a model. It is Meada's only path to a language
model, and it runs only when you ask for it with that flag. No other part of Meada, indexing and
querying included, uses an LLM or the network.

#### Environment variables

The decider is configured only through environment variables (nothing comes from a file):

| Variable | Role | Default |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` or `chat` | `decisions` |
| `MEADA_DECIDER_URL` | base URL | `https://openrouter.ai` (`decisions`); required with `chat` |
| `MEADA_DECIDER_KEY` | API key, sent only in the `Authorization` header | required with `decisions`; optional with `chat` |
| `MEADA_DECIDER_MODEL` | model | `typesafe/jev-1.14` (`decisions`); required with `chat` |

#### Protocols

- **`decisions`** (the default) uses OpenRouter's decisions API with the Jev model. A single
  request carries all the items, and the response brings the probability of each candidate. The
  request asks for zero data retention (`zdr: true`, `data_collection: deny`); if OpenRouter
  refuses that preference, the request fails and nothing is written.

  ```sh
  export MEADA_DECIDER_KEY=...
  meada decide --decider
  ```

- **`chat`** talks to any endpoint compatible with the OpenAI chat API
  (`<URL>/chat/completions`), a local model included, for example through Ollama. One request per
  item. The probability is the one the model itself states, not a calibrated measure.

  ```sh
  MEADA_DECIDER_PROTOCOL=chat \
  MEADA_DECIDER_URL=http://localhost:11434/v1 \
  MEADA_DECIDER_MODEL=<model> \
  meada decide --decider
  ```

Any gateway that speaks one of the two protocols works: just change the URL. HTTPS is required
everywhere; HTTP only for `localhost` or a loopback address.

#### What is sent

Per item, and nothing besides:

- the call (the code excerpt at the call site);
- the function that contains it, read from disk (without it, when the call is at the top level of
  the file or the enclosing symbol is not a function);
- the list of candidates (repository, method and path);
- the identity of the calling repository, the model id and a fixed sentence that describes the
  task.

No provider code and no other file leaves the machine. To see exactly what would be sent, run
`meada decide --decider --show-request`: it prints each request, with the key replaced by
`<hidden>`, and sends none.

#### Privacy

- Without `--decider`, no code leaves the machine. Indexing, querying over MCP and deciding as a
  person use no network. Two things escape that and are not decisions: `npx meada` downloads the
  binary, and `mcp --listen` opens an HTTP server (see server mode).
- With `--decider`, the code above goes to the endpoint you configured. To keep everything on the
  machine, use the `chat` protocol with a local model (`http://localhost:...`).
- The key travels only in the `Authorization` header and is never printed.
- Each response is validated per item. An invalid response or a failed request is reported
  without writing that item, and the command exits with an error.
- What the model proposes reaches only `meada.decisions.toml`, which is a versioned file: the
  decision goes through diff review like any other change.

#### Fixed limits

- `--min-p` (from 0 to 1) is 0.5 by default: below it, the proposal is printed as a suggestion
  and not written.
- At most 50 candidates per item: with more, the item is skipped in decider mode (human mode
  still offers it).
- 120 seconds per request, from connection to the last byte, with no retry.
- With the `decisions` protocol, the probabilities of a response must sum to 1, with a tolerance
  of 0.01.

In server mode, decisions are made in a pipeline, not by the server; see
[Decide in a pipeline](https://claudiomascaro.github.io/meada/en/server/#decide-in-a-pipeline).

## 6. CLI commands

```text
meada <COMMAND>
```

Messages follow `LANG` (Portuguese or English); MCP's are always in English. Every invocation
accepts `--help` and `--version`.

### The five commands of the default build

| Command | What it does |
| --- | --- |
| `meada index` | Builds the indexes of the workspace of the repository in the current directory |
| `meada status` | Shows the workspace: each repository, the state of its index and the peers not found |
| `meada export` | Prints the index as sorted JSON, with the links that touch it |
| `meada mcp` | Serves the repository in the current directory to agents over MCP (stdio) |
| `meada decide` | Resolves this repository's open items and records the choices in `meada.decisions.toml` |

#### `meada index`

```sh
meada index [--workspace <manifest>]
```

Refreshes the index of the current repository and its peers. With `--workspace`, it refreshes
that of every repository the `meada.workspace.toml` manifest lists (the operator's refresh, in
server mode).

#### `meada status`

```sh
meada status [--workspace <manifest>]
```

Shows the workspace's repositories and the state of each index. Under each repository, it shows
the unlinked resources, the `via` entries that match no symbol, the stale decisions or those
waiting on a peer, and each source file a language pack dropped for going over its budget. Peers
that were not found appear as `missing peer`.

#### `meada export`

```sh
meada export [--workspace <manifest> --repo <identity>]
```

Prints the current repository's index as sorted JSON (format in section 7). With `--workspace`,
the `--repo` flag is required and picks the manifest's repository; each requires the other.

#### `meada mcp`

```sh
meada mcp
meada mcp --listen <address> --workspace <manifest>
```

Without flags, it serves over MCP on stdio (section 2). With `--listen`, it serves over HTTP in
server mode, at `host:port`, `:port` or `port` (loopback without a host); `--listen` and
`--workspace` go together.

#### `meada decide`

```sh
meada decide [--item <id>] [--dry-run] [--list] [--prune]
             [--decider [--show-request] [--min-p <p>]]
```

| Flag | Role |
| --- | --- |
| `--item <id>` | Handles only that item (the id `--list` or `search` prints), even if already decided, to redo it |
| `--dry-run` | Prints the file that would be written and writes nothing |
| `--list` | Prints the open items, the stale decisions and those waiting on a missing peer; neither asks nor writes |
| `--prune` | Removes the stale decisions; keeps those waiting on a missing peer. Does not combine with `--item`, `--dry-run` or `--list` |
| `--decider` | Asks the model configured by the `MEADA_DECIDER_*` variables instead of a person |
| `--show-request` | Only with `--decider`: prints each request that would be sent and sends none |
| `--min-p <p>` | Only with `--decider`: the minimum probability, from 0 to 1, to write (default 0.5) |

### Cloud module commands

Builds with optional modules (the `-cloud` image carries them all) add commands to run Meada as an
organisation's service. They are not part of local use and are described in the operations pages.

| Command | Module | Role |
| --- | --- | --- |
| `meada snapshot` | `snapshot` | The snapshot store: `publish`, `pull`, `lock`, `pointer` and `fence` |
| `meada workspace` | `generation-pointer` | The workspace versions in the snapshot store |
| `meada workspaces` | `workspaces` | The named workspaces, with `mcp` in the `proposals` module |
| `meada ui` | `ui` | The management console |
| `meada github-app` | `github-app` | Git credentials from GitHub App tokens, for the indexing job |
| `meada postgres` | `postgres` | The data layer: `migrate` creates the schema or takes it to the binary's version |

The `--generations` flags of `status`, `export` and `mcp` exist only in the `snapshot` module. See
[Server mode](https://claudiomascaro.github.io/meada/en/server/) and
[Cloud modules](https://claudiomascaro.github.io/meada/en/cloud/).

## 7. The export

`meada export` prints a repository's index as JSON, ready for `diff` between two runs and for
feeding a knowledge base.

```sh
meada export > web.export.json
```

- **Deterministic**: the same file tree produces the same bytes. Every list comes sorted and the
  text ends with a newline.
- **No source text**: whoever needs the excerpt reads the `span` in the file.
- **One repository per export**: an organisation runs `meada export` on each repository and joins
  the results by symbol identity.
- **No format version field.** Readers must ignore the fields they do not know: a new field is a
  minor change.

`jq` recipes (one record per symbol, duplicates, fan-in across repositories) are in the
[README](../README.en.md#feed-a-knowledge-base).

### Top-level fields

Empty lists are omitted, except `files`, `symbols`, `calls`, `providers`, `consumers` and `links`,
which always appear (like `repository`, which is text).

| Field | Content |
| --- | --- |
| `repository` | The repository's identity (`example.com/acme/web`) |
| `files` | The files read: `path` and `hash` |
| `symbols` | The symbols: `id`, `span` (`path`, `start`, `end`, byte offsets) and `hash` (hexadecimal BLAKE3 of the span's bytes) |
| `calls` | The call sites: `id`, `caller`, `target` and `level` (empty until resolved), `candidates` (if ambiguous), `span`, `narrowed` and `decorates` |
| `providers` | The provider side of each contract: `contract`, `site`, and if present `gateway` and `shape` |
| `consumers` | The consumer side: `contract` (null if the URL was not evaluated), `site`, `via` (how it names the provider's repository, such as `host:api.example.com` or `env:API_URL`) and `shape` |
| `links` | The links: `consumer`, `provider`, `contract`, `level`, `reason`, and if present `by`, `p` and `drift` |
| `bindings`, `publishes` | The bindings (`contract`, `exchange`, `pattern`, `site`) and the publications (`exchange`, `key`, `site`) of RabbitMQ that the repository declares |
| `documents` | The OpenAPI and AsyncAPI documents read: `path`, `format`, `version` and `contracts` |
| `packages` | The packages the repository provides: `name`, `version`, `repository`, `directory` and `ecosystem` |
| `dependencies` | The dependency entries that name a workspace package: `manifest`, `field`, `name`, and if present `alias` and `range` |
| `imports` | The imports resolved to a workspace package: `site`, `specifier`, `name`, `level`, `how` and `target` |
| `communities` | The communities: `name` and `members` (sorted) |
| `processes` | One process per entry point: `entry_point`, `kind` (`route`, `export`, `channel` or `program`), `label` and `steps` |
| `stale_decisions` | The decisions that no longer apply: `call`, `target` and `cause` (`call-gone`, `item-not-open`, `target-gone` or `not-candidate`) |

### Recurring values

- **`level`**: `exact`, `declared`, `inferred`, `decided` or `ambiguous`, from firmest to weakest.
- **Contract identifiers**: always start with the kind, `http::GET::/api/health`,
  `channel::kafka::orders.created`, `package::...` or `resource::postgres::orders`.
- **Symbol identity**: `<repository>::<path>::<name>`, for example
  `example.com/acme/web::src/status.ts::health`. A call site adds the target and the index within
  the symbol: `...::health::fetch#0`.
- **`shape`** (in `providers` and `consumers`): the field paths that a response or a message
  carries, with `source` and `fields`, among other keys. Drift, on the `link`, lists in `missing`
  the fields the consumer reads and the provider does not return.
- **`steps`** of a process: `depth`, `repository`, `level`, `via` (`entry`, `call` or `link`),
  `node`, and if present `reason`, `mark`, `candidates` and `contract`.

### Example

```json
{
  "repository": "example.com/acme/web",
  "files": [{ "path": "src/status.ts", "hash": "d61eae8a..." }],
  "symbols": [
    {
      "id": "example.com/acme/web::src/status.ts::health",
      "span": { "path": "src/status.ts", "start": 31, "end": 99 },
      "hash": "5cc3a63c..."
    }
  ],
  "calls": [],
  "providers": [],
  "consumers": [],
  "links": []
}
```

(Illustrative excerpt: a real export carries all the repository's files, symbols and links.)

## 8. Contract kinds

A **contract** is what one repository exposes and another uses. Meada reads five kinds, and each
has its own linking rule. This section says what each kind links and where it stops, so you can
tell a link that is missing because of a known limit from one that is missing because of a bug.

Every contract identity starts with its kind, and that identity is what you pass to `context` and
`trace`:

| Kind | Identity | Example |
| --- | --- | --- |
| HTTP route | `http::<METHOD>::<path>` | `http::GET::/api/orders/{}` |
| Channel | `channel::<broker>::<name>` | `channel::kafka::orders.created` |
| Package | `package::<ecosystem>::<name>` | `package::npm::@example/shared` |
| Resource | `resource::<kind>::<id>` | `resource::postgres::orders` |

OpenAPI and AsyncAPI documents are not a separate kind: they give routes and channels to the kinds
above (section 8.3).

How to tell whether a missing link is a bug:

- If the code uses a library, a framework or a form that sections 8 and 9 do not list, the link is
  missing because of a known limit, not a bug.
- If the library is on the list and the code uses the described form, the link should exist. Run
  `meada status` and `search` with `kind=unresolved` and `kind=ambiguous` to see whether Meada
  read the side and did not link it, and then open an issue with the query, the answer and the
  expected one.
- A call inside a Rust macro is not indexed: a caller that is missing there is "unproven".
- A name that Meada cannot compute yields no link by itself. A URL built at run time is an
  unresolved consumer, which you decide with `meada decide` (section 5). A channel name read from
  the environment is an unresolved side, a known limit that no decision resolves (section 8.4).

### 8.1 HTTP routes

A route links whoever **serves** it (the provider, the code that handles it) to whoever **calls**
it (the consumer, an HTTP client call). In every language, Meada normalizes the path:
`/users/:id`, `/users/{id}` and `/users/<int:id>` become the same `/users/{}`, and the method is
uppercased. Path case and API versions are kept.

How the link is chosen, depending on what the URL shows:

1. **Host.** The URL has a host (`https://api.example.com/orders`) that some repository lists in
   `[serve] hosts`. The link is `exact`. If no repository serves that host, there is no link.
2. **Environment variable.** The URL starts with an environment read (`API_URL`) that a peer
   declares in `base_url_env`. The link is `inferred`, and Meada never reads the variable's value.
3. **Unknown host.** The URL does not say which host it targets (a relative path, for example)
   and only one repository in the workspace serves that method and that path. The link is
   `inferred`, with the reason `the only repository serving it is`.
4. **Several providers.** The link is `ambiguous`, and Meada lists all the candidates, one link per
   candidate. It is an open item that `meada decide` resolves.

A provider that serves every method has the method `ANY` (`http::ANY::/health`) and links to
consumers of any method, with `any method` in the reason. A consumer whose URL Meada cannot
compute, even after folding constants and the arguments of functions that wrap the client, is an
**unresolved consumer**: it appears in `search`, in `context` and in the export, never links, and
is an open item. A function that builds the URL from its parameters becomes a consumer at each
call of it, with the arguments substituted, at any depth.

Why a link may be missing:

- The path that a **provider** declares has a part Meada does not read (an interpolation, a
  parameter, an environment read in the middle of the path): the route does not exist for Meada.
  An unreadable prefix does not become the root, it leaves the route with no prefix at all. (On the
  consumer side, a hole in the middle of the URL becomes a `{}` parameter and still links.)
- The framework or the client is not on the language's list (section 9).
- The repository that serves the route is not among the peers, and the workspace does not have it.

### 8.2 Declared resources and gateway prefixes

**Shared resources.** What no code names, such as a table or a bucket, you declare in
`[[resource]]` (section 4). Meada does not read code for this kind: it links each writer to each
reader of the same `kind` and `id` in another repository, at level `declared`, with the contract
as the reason.

```toml
# api/meada.toml
[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]

# worker/meada.toml
[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

With `via`, the side is the symbol; without `via`, or if no symbol matches, the side is the whole
repository and appears as `(repository-level)`: a `trace` shows the link but does not enter that
side. `meada status` lists, under each repository, the unlinked resources and the `via` entries
with no symbol. Since Meada does not interpret `kind`, nothing guarantees that `postgres` and
`postgresql` are the same resource: write it the same way on both sides.

**Gateway prefixes.** When a gateway rewrites paths, the consumer calls a path the provider's code
does not have. You declare the rewrite in `[serve]` with `prefix` and `strip` (section 4), and
Meada also accepts the path of each `servers` URL of an OpenAPI document as that document's
prefix. A route links through its gateway path as well, and that link is `inferred`, with the
reason `gateway prefix /shop`, `gateway strip /v1` or both. Within one repository, the code's path
wins over the gateway's.

### 8.3 OpenAPI and AsyncAPI documents

A repository with the `[serve]` table (even empty) has its documents read as a source of
providers. Meada reads **OpenAPI 3.0 and 3.1** and **AsyncAPI 2.x and 3.0**, in JSON or YAML,
found by file name or listed in `[serve] documents`. Without `[serve]`, the documents are ignored.

- **Routes.** Each route in the document is a provider. If the code already has a route with the
  same contract, the two become a single provider, with the code's point as the site. A route that
  only the document lists appears as `<document> (documented only)` and no `trace` enters it. A
  provider coming from a document is a written contract: it does not lower the level (a matching
  host stays `exact`).
- **Channels.** Each AsyncAPI channel becomes a producer or consumer side, merged with the code's
  side when the contract is the same. In a document, the name of an `amqp` channel is a queue's,
  never an exchange's.
- **Data format.** What the document says about the body of a response or a message is the shape
  (section 10) of that side. Only a route's shape enters the drift comparison; a message's is
  recorded, but no drift is computed for channels.
- **Hosts.** The host of each `servers` URL counts as a `[serve] hosts`.

`meada status` shows, per repository, what was read and what was ignored. A part of the shape that
Meada cannot list (an object that accepts extra properties, a schema behind an external
reference) stays **open**: what lies below it may exist. When the document and the code disagree
about a route's shape, the document's shape wins.

### 8.4 Messaging channels

A **channel** is a broker's queue or topic. Each producer links to each consumer of the same
channel, also within the same repository. The message goes from the producer to the handler, so
`trace` descends in that direction.

The broker is part of the identity (`channel::<broker>::<name>`), and the same name on two
brokers is two channels:

| Broker | Value in `<broker>` | Note |
| --- | --- | --- |
| Kafka | `kafka` | |
| RabbitMQ | `amqp` | exchanges, queues and bindings |
| Amazon SQS | `sqs` | |
| NATS | `nats` | wildcards `*` and `>` |
| Redis streams | `redis` | |
| BullMQ | `bullmq` | TypeScript and JavaScript only |

The clients of each language are in section 9. JMS (`jms`) and Azure Service Bus (`servicebus`)
are brokers of Java, Kotlin and C#, planned Release 2 work (section 9), and are not part of 1.0
(an AsyncAPI document with that protocol still gives channel sides for those brokers).

Rules that apply to all of them:

- **Name computed like a URL.** Constants, arguments of functions that wrap the client and simple
  interpolations are folded. A name that cannot be computed (read from the environment, built at
  run time) is an **unresolved side**: contract `channel::kafka`, shown as `(unresolved)`,
  present in the export, never linked and never an open item. It is a known limit, and
  `meada decide` does not see it.
- **RabbitMQ.** A binding of a queue to an exchange (`bindQueue`, `QueueBind`, `queue_bind`) is a
  routing rule, not a side. A publish to a named exchange reaches the queues whose binding matches
  the routing key, coming from any repository in the workspace, on any exchange type: `exact` when
  the pattern is the key, `inferred` with a wildcard. A publish to the default (empty) exchange is
  a producer of the queue the key names.
- **NATS.** A consumer with `*` (one token) or `>` (the rest) links to the producers whose subject
  matches, at level `inferred`.
- **Clusters are not modelled.** A name on a broker kind is one channel across the whole
  workspace: topics of the same name in different clusters link as if they were in one cluster. If
  this produces a false link, that is the limitation, not a bug.
- **The handler** of a consumer is the function the subscription names.

### 8.5 Packages

A **package** links whoever declares it in a manifest (the provider) to whoever lists it as a
dependency (the consumer), and goes further: an `import` of a function of the package reaches the
provider's code, so the `context` of an exported function lists the callers from the whole
workspace and `trace` crosses the hop. Only packages that some repository in the workspace
provides enter; dependencies from outside the workspace appear nowhere, Meada queries no registry
and does not read lockfiles (`go.sum`, `Cargo.lock` and the like).

The reason of a package link carries the range as written and, where it applies, ends with
`range unchecked` (the range was not checked) or, on npm, `range mismatch` (the range rejects the
provider's local version). On npm, a range that accepts the local version carries no suffix. An
import that reaches the code adds `; import <what was used>` to the reason.

| Ecosystem | Manifest | Identity | Notes |
| --- | --- | --- | --- |
| npm | `package.json` with `name` | `package::npm::<name>` | Reads `dependencies`, `devDependencies`, `peerDependencies` and `optionalDependencies`. The range is checked (npm semver) |
| PyPI | `pyproject.toml` with a name in `[project]` or `[tool.poetry]` | `package::pypi::<name>` | Normalized by PEP 503. Also reads `requirements*.txt`. Ranges are not checked |
| Cargo | `Cargo.toml` with `[package] name` | `package::cargo::<name>` | Each member of the Cargo workspace is a provider. Ranges are not checked |
| Go | `go.mod` with `module` | `package::go::<module path>` | Reads `require` and `replace` to a local directory |

Details per ecosystem:

- **npm.** An `import` enters the provider's code through `exports`, `types`, `module`, `main` and
  re-exports. If the entry points at a build's output, the `outDir` of `tsconfig.json` leads back
  to the `rootDir`. A package with the same name on npm and on PyPI does not link. A `workspace:`
  range is only valid inside the repository itself. `search` with `kind=package` reports name,
  repository, directory, version and consumers.
- **PyPI.** The `import` reaches the code as `exact` when the module matches the distribution's
  name, `inferred` when it is one of its top-level modules and `ambiguous` when several
  distributions expose it.
- **Cargo.** A dependency by `path` resolves the crate in that directory, in the same repository or
  in another one of the workspace (the path is resolved textually, without leaving the repository
  roots or following symbolic links); by `git`, the repository with the normalized URL; by
  version, the provider of the same name. `package =` gives the real name of a renamed dependency,
  and `workspace = true` reads `[workspace.dependencies]`. A `use` follows `[lib] name` or the
  renamed name and the `pub use` chains down to the public items.
- **Go.** A `replace` that leaves the repository is resolved textually, like Cargo's `path`, and a
  replacement by another module is left out. An `import` of `<module>/<directory>` reaches the
  exported functions, types and methods of the directory.

When two providers serve the same name, the link is `ambiguous`, one per candidate, with the
reason starting with `<n> providers fit`.

Maven and Gradle, NuGet and SwiftPM are the package managers of Java and Kotlin, C# and Swift,
planned Release 2 work (section 9): they arrive with those languages, after 1.0.

## 9. Languages

Meada 1.0 reads **TypeScript and JavaScript, Python, Go and Rust**. Each section below lists the
files read, the frameworks and HTTP clients, the environment reads, the manifests, the channel
clients and the known limits. A library outside these lists produces no link.

Rules that apply to all four languages:

- Meada reads the text with tree-sitter. It runs no compiler, interpreter or package manager and
  opens no network, and it never reads the value of an environment variable.
- A call is `exact` when the code shows the receiver's type or the written path, and `inferred` or
  `ambiguous` when only the name decides (section 2, "How to read the levels").
- A library counts when the file imports it (JavaScript's global `fetch` is the exception).
- A decorator, an attribute or an annotation written on a definition is a call from the scope that
  encloses it. `context` lists them apart, under `annotations:`, and not among the callees.
- A URL or a channel name is folded: literals, concatenations, simple interpolations, constants
  (also from other files of the repository) and the arguments of functions that wrap the client,
  at any depth. What does not fold is an unresolved consumer (HTTP) or an unresolved side
  (channels).

**Java, Kotlin, C# and Swift** are planned Release 2 work of ADR 0003 and are not part of 1.0.
They arrive later, as 1.x minor versions, under issues #26 to #29. This guide and the 1.0
stability promise do not cover them: the current build already reads files in those languages,
but what Meada links in them may change in any version. In a repository in those languages, only
what it declares (a `meada.toml`, an OpenAPI or AsyncAPI document) is covered.

### 9.1 TypeScript and JavaScript

**Files.** `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs` and `.cjs`. An `import` uses the `paths` and
`baseUrl` of the nearest `tsconfig.json` (or `jsconfig.json`), in the file's directory or above,
relative to that configuration file's directory. `node_modules` is ignored.

**What becomes a definition.** Functions, classes and methods, the `const`, `let` and `var` of a
function or class and `export default`. At the top of a file, assigned functions count too:
`obj.y = function () {}` defines `obj.y`, `X.prototype.y = function () {}` defines the method
`X.y`, `exports.y = ...` and `module.exports.y = ...` define the export `y`, and
`module.exports = function () {}` defines the default export. A `let` or `var` is only a
definition if the file never assigns the name again. The functions of the object literal of a
top-level `const` define `o.m`.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | global `fetch`, and a call to a parameter whose default is `fetch` |
| Provider | Next.js (app router: route files) and Express |

In Express, a route holds under each prefix where the router is mounted (`use`, `Router()`). A
prefix or a path with an unreadable part (a template-literal hole, a parameter, an environment
read) generates no route, and a router mounted only under such a prefix is left with no prefix at
all. A literal `:id` is still a parameter. No client besides `fetch` is read: `axios`, `got` and
`XMLHttpRequest` generate no consumer.

**Environment.** `process.env.X`, `process.env["X"]` and `import.meta.env.X`, alone or passing
through text methods such as `trim()`, `replace(...)` and `toLowerCase()`. A URL that starts with
such a read links through the peer's `base_url_env`. `.env` files are not read.

**Manifests.** `package.json` (`name`, `version`, the four dependency fields, `exports`, `types`,
`module` and `main`) and `tsconfig.json` (`paths`, `baseUrl`, `outDir`, `rootDir`). See npm in
section 8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | `kafkajs` |
| RabbitMQ | `amqplib` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` |
| NATS | `nats` |
| Redis streams | `ioredis`, `node-redis` |
| BullMQ | `bullmq` |

**Data format.** It is the only language where Meada reads the format of a response from the code
itself, on both sides: what a `fetch` consumer reads from `.json()` and what a Next.js or Express
provider returns, through zod schemas or object literals. See section 10.

**Known limits.**

- `require` and `module.exports` are not imports between files, and `new F()` of a constructor
  function does not reach the methods of `F.prototype`.
- A `let` or `var` reassigned anywhere in the file does not count as a single definition.
- A module that `tsconfig.json` does not map and that lies outside the repository is external.
- Routes mounted by a value Meada does not read (a router coming from an outside call) are not
  served anywhere.

### 9.2 Python

**Files.** `.py`. Each `pyproject.toml` gives code roots (its directory, the roots it declares or
`src`) to the files below it, and it is through them that an `import` finds the module.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | `requests`, `httpx`, `aiohttp` |
| Provider | FastAPI, Flask, Django |

- **Consumers.** A module function (`requests.get`, `httpx.post`, also under an alias) or a method
  of a client that the code shows came from the library's constructor (`Session()`, `Client()`,
  `AsyncClient()`, `ClientSession()`), kept in a variable, in an attribute of `self`, opened by
  `with` or received as a parameter. The constructor's `base_url=` is the base of relative URLs. A
  `get` on any other object is nothing.
- **FastAPI and Flask.** Method decorators hold on an object that traces back to the framework's
  constructor (`FastAPI()`, `APIRouter()`, `Flask()`, `Blueprint()`). In FastAPI: `@app.get`,
  `@router.post` and the other methods, and `api_route`. In Flask: `@app.route(..., methods=[...])`
  (the `@app.get` shortcut generates no route). The prefix of `APIRouter(prefix=)`, of
  `include_router` and of `mount` is summed; in Flask, the `url_prefix` given to
  `register_blueprint` replaces that of the `Blueprint` itself. What nothing mounts is served at
  the root, except an `APIRouter` or `Blueprint`, which only serves where a mount that Meada reads
  puts it.
- **Django.** Meada starts from the root URLconf (a file with `urlpatterns` that no `include()`
  names) and descends through `path()`, `re_path()` and `include()`. A `re_path()` only holds with
  literal text and named groups (`(?P<name>...)`); any other pattern generates no route. A
  function view serves the methods that `require_http_methods`, `require_GET`, `require_POST` and
  `require_safe` allow, or all of them; `C.as_view()` serves the methods (`get`, `post`...) that
  `C` defines.

**Environment.** `os.environ["X"]`, `os.environ.get("X")` and `os.getenv("X")` (also bare
`environ` and `getenv`, matched as written and not through imports), alone or in an f-string hole,
with an `or` fallback, with a default argument and with text methods such as `strip` and
`replace`.

**Manifests.** `pyproject.toml` (`[project]` and `[tool.poetry]`, including `[project.scripts]`)
and `requirements*.txt`. See PyPI in section 8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | `confluent-kafka`, `kafka-python` |
| RabbitMQ | `pika` |
| Amazon SQS | `boto3` |
| NATS | `nats-py` |
| Redis streams | `redis-py` |

**Known limits.**

- A route decorator on an object that does not trace back to the framework's constructor generates
  no route: Meada does not guess by name.
- A prefix or a path with an unreadable part (an f-string hole, a parameter) generates no route.
  `str.format` and `%` do not fold a URL; f-strings and `+` do.
- A router mounted through an import that does not resolve is not presumed to sit at the root.
- In Django, without a determinable root URLconf (an `include()` that Meada does not read), no
  route is served.
- Python configuration files (`settings.py`, `.env`) are not read beyond the environment reads in
  the code.

### 9.3 Go

**Files.** `.go` included by the package's build rules. Each `go.mod`'s `module` resolves imports
to package directories, a `replace` to a directory of the repository included. A nested module
owns its subtree, and the files of a directory with the same `package` share scope (`_test.go`
too). Meada does not run Go's toolchain.

The analysis targets linux/amd64: `_windows.go` files, files with a false `//go:build` or that use
cgo (`import "C"`) are left out.

**What becomes a definition.** Functions, types, interfaces and methods (`Type.Method`). Calls
resolve within the package and through imports (`pkg.F()`, aliased or dot). The standard library
and external modules stay external.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | `net/http` (`Get`, `Head`, `Post`, `PostForm`, `NewRequest`, `NewRequestWithContext`), resty |
| Provider | `net/http`, gin, echo, chi, gorilla/mux |

A router is the call that creates it (`gin.Default()`, `echo.New()`, `chi.NewRouter()`,
`http.NewServeMux()`, `mux.NewRouter()`). Prefix groups (`Group(prefix)` of gin and echo,
`Route(prefix, fn)` and `Mount(prefix, router)` of chi, `PathPrefix(...).Subrouter()` of
gorilla/mux) sum the prefix, and gorilla/mux restricts methods with `.Methods(..)`. Each
framework's path parameters (`{name}`, `:name`, `*rest`) become `{}`. A resty client's base is the
argument of `SetBaseURL`.

**Environment.** `os.Getenv` and `os.LookupEnv`, including the pattern written with `if v == ""`,
`len(v) == 0`, `!ok` or `!ok || v == ""` when the `ok` comes from the same `os.LookupEnv`, and
then `strings.TrimSuffix`, `TrimRight` and `TrimSpace`.

**Manifests.** `go.mod` (`module`, `require`, `replace`). `go.sum` is not read. See Go in section
8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` |
| RabbitMQ | `amqp091-go` |
| Amazon SQS | `aws-sdk-go-v2` |
| NATS | `nats.go` |
| Redis streams | `go-redis` |

**Known limits.**

- A `recv.M()` call is `exact` when the code gives the receiver's type (a parameter, field, `var`,
  composite literal or function result). Without that, name resolution is `inferred` or
  `ambiguous`.
- A call through an interface stops at the interface's method: Meada does not choose the
  implementation.
- A handler wrapped in a way Meada does not read (`http.StripPrefix`, `echo.WrapHandler`, a
  middleware) mounts the router where it does not reach, and its routes do not appear.
- echo's `Add(method, path, handler)` is not read.
- Code with cgo, `_windows.go` and a false `//go:build` is not indexed.

### 9.4 Rust

**Files.** `.rs` reachable through each crate's module tree. Every `Cargo.toml` with `[package]`
defines a crate, in any directory, and Meada follows the `mod` declarations without running
`cargo` or `rustc`. A file that no `mod` reaches is left out.

**What becomes a definition.** Functions, types, traits, modules and `impl` methods. Calls such as
`crate::a::f()`, `Type::new()` and `Self::f()` follow the written path; resolution also follows
`use`, groups, renames, globs, `pub use` and other workspace crates by name. In `x.m()`, the
receiver's type comes from a `let`, parameter, field, `self` or a written function return; a call
on the result of another (`Router::new().route(..)`, also through `.await` or parentheses)
resolves by the written return type of the inner call; a `dyn Trait` or a trait-bounded generic
reaches the trait's method. Without a written type, resolution is by name.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | `reqwest` (including `blocking`), `ureq` |
| Provider | axum, actix-web, Rocket |

In axum, `route`, `nest` and `merge` sum the prefixes along the `Router::new()` chain. In actix-web
and Rocket, the function with the route attribute (`#[get]`, `#[route(...)]`) or named by
`.route(path, web::get().to(h))` is the handler, and `web::scope`, `web::resource`,
`.configure(f)` and the Rocket instance mount prefixes. The body of `format!` and `concat!` folds a
URL when the arguments are literals or, in `format!`, simple names; any other argument becomes a
parameter (`concat!` only accepts literals). A `let mut` String that is only read and appended to
by `push_str`, `push` and `+=` also folds.

**Environment.** `std::env::var("X")`, `dotenvy::var("X")` and `env!("X")` (also as an argument of
`concat!` or `format!`), with `unwrap`, `expect`, `?`, `trim`, `to_string` and the like.
`option_env!` is not a read.

**Manifests.** `Cargo.toml` (`[package]`, `[dependencies]`, `[dev-dependencies]`,
`[build-dependencies]`, `[workspace.dependencies]`, `[lib] name`). See Cargo in section 8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | `rdkafka` |
| RabbitMQ | `lapin` |
| Amazon SQS | `aws-sdk-sqs` |
| NATS | `async-nats` |
| Redis streams | `redis` crate |

**Known limits.**

- **Calls inside macros are not indexed** (`assert!`, `format!`, `vec!`, `println!`). A caller that
  only calls from inside a macro does not appear, and `context` treats it as "unproven". The only
  exception is the braced body of an item macro of the repository itself (a `macro_rules!` whose
  first rule takes `$($x:item)*` and rewrites each item with outer attributes only, like tokio's
  `cfg_rt!`): it is read as the module's items. Macros from other crates remain unread.
- Methods that are not in the type's `impl` blocks, including those generated by `derive` or
  supplied by external traits, produce no link.
- The analysis uses each crate's default configuration for x86_64 Linux: items under a false
  `#[cfg(..)]` and features outside `default` are left out; modules compiled in a test build
  enter, not just the `#[cfg(test)]` ones.
- An axum router handed to a call that Meada does not read (`route_service`,
  `fallback_service`, an outside wrapper) is not mounted anywhere.
- `option_env!` and an `env!` that the file itself defines are not reads.

## 10. Shape drift

A contract's **shape** is what it carries, recorded as a set of field paths (`items[].id`;
`[].id` when the body is a list). **Drift** is a field that a consumer reads and that the shape of
the provider linked to it does not contain: a field the route stopped returning. A link may stand
and still break, and drift is the warning for that.

Where each shape comes from:

| Side | Source | Field level |
| --- | --- | --- |
| Provider | OpenAPI document (any language) | `exact` |
| Provider | zod schema that an Express or Next.js handler's response goes through | `exact` |
| Provider | Object literals that an Express or Next.js handler returns | `inferred` |
| Consumer | Declared type or schema (`z.infer`) of the body read | `exact` |
| Consumer | Fields read from the `.json()` of a `fetch` | `inferred` |

Each missing path carries its level: `exact` when the provider's shape and the consumer's path are
both declared (document, schema, type), and `inferred` when one of the two comes from what the
code does (field reads, returned literals). The link's level does not change because of drift.
Today only TypeScript and JavaScript code generates a consumer's shape, and only `fetch`
consumers and Next.js and Express providers generate a provider's from code; providers in other
languages enter through the document.

How Meada shows drift:

| Query | Result |
| --- | --- |
| `trace` | Adds `drift: missing <path>` to the link's line |
| `context` on a route | Shows the shape's origin, the consumers, the drift, the level and the unchecked reads |
| `search` with `kind=drift` | Lists the drifts, one per missing path |
| `meada status` | Counts the missing reads, under the consumer's repository; the provider gets no count |

Example of a `trace` line:

```text
example.com/acme/api src/orders.ts:12 GET /api/orders
  →[inferred: base URL from API_URL, a base_url_env of peer example.com/acme/api] example.com/acme/web src/orders.ts:8 loadOrders drift: missing discount
```

(Illustrative example in `trace` format: `web` reads `discount` from the response of `GET
/api/orders`, and `api` does not return it.)

The limits of this analysis, so you do not mistake silence for absence of drift:

- **Presence only, never types.** Meada compares the presence of paths. It does not compare types,
  formats, `required` or `nullable`.
- **Open parts are not checked.** An object that accepts extra properties
  (`additionalProperties`, `.passthrough()`, `z.record`, `z.any`), a schema behind an external
  reference and a body that Meada cannot list leave the part **open**: everything below it may
  exist, and the read appears under "unchecked" instead of as drift.
- **Nothing below a scalar.** `name.length` is not a field, and `list.length` below an array is not
  drift.
- **Only the shallowest path.** Of each branch, only the first missing path is listed.
- **Ambiguous links have no drift.** Without a single provider, there is no shape to compare.
- **The handler must respond in a way Meada reads.** A body that comes from a value Meada does not
  follow (an outside call, a spread of something unknown) opens that path. Only 2xx responses
  count; a response with a literal status outside 2xx is ignored.
- **Without a document or schema, the shape is only what the code shows.** Literals and reads are
  a guess at what the code returns, hence `inferred`.

## 11. Communities and processes

Two analyses give a map of the code beyond point-to-point links. Neither uses a language model,
and both come out the same on any machine.

### Communities

A **community** is a group of symbols in a repository that call each other more than they call the
rest. Meada finds them from the calls of level `exact` or `inferred`, with the Leiden algorithm,
with no randomness and with integer arithmetic. The name comes from the directory with the most
members (`src`, and `src#2` when another community in the repository already has that name). Every
symbol that is in at least one of those calls, as caller or as callee, is a member of some
community; a symbol whose only call is to itself, or that is only called from the top of a file,
forms a community of its own; a symbol outside every call belongs to none.

- `search` with `kind=community` finds communities by name.
- `context` of a symbol states its community, or `community: none`.
- `meada export` includes the communities.

### Processes

An **entry point** is a door into a repository. Each one has its own process; no score or
threshold chooses which ones count.

| Kind | What it is |
| --- | --- |
| `route` | Each HTTP route the repository serves; the handler is the function |
| `channel` | Each channel that a handler consumes |
| `program` | A program start that the language or the manifest declares |
| `export` | Each exported function or method that nothing calls in the repository itself |

Program starts, per language:

- **Python:** `if __name__ == "__main__":` (symbol `__main__`) and the functions of
  `[project.scripts]` in the `module:function` form. The exported names are the public ones,
  without a leading `_`, or those of a literal `__all__`.
- **Rust:** the `fn main` of each binary, in `src/main.rs`, `src/bin/` or `[[bin]] path`.
- **Go:** `func main` in a `package main`.
- **TypeScript and JavaScript:** there is no `program` kind; the doors are the routes, the channel
  handlers and the exported functions that nobody calls.

A **process** is the call tree that an entry point sets in motion: the calls within the repository
at all levels and the links to other repositories, down to the leaves. It ends at an ambiguous
call or link (a step that lists the candidates), at an unresolved consumer, at a cycle or at a
missing peer. The process is not stored: it is walked at query time, from the calls and links
already stored.

- `trace` with `process::<entry point identity>` reads the tree. The identity is the handler's,
  plus the contract for a route or a channel, for example
  `process::example.com/acme/worker::src/consumer.ts::onOrderCreated::channel::kafka::orders.created`.
- `search` with `kind=process` finds processes by the entry point's label (`GET /users/{}` or the
  qualified name).
- `context` of a symbol lists the processes it is part of, walking the calls and links toward the
  callers.
- `meada export` includes each process.

### Duplicates

`search` with `kind=duplicate` lists the symbols whose text is identical in two or more places of
the workspace (in different repositories or in the same one), in groups ordered by size.
The export carries each symbol's BLAKE3 `hash`. Near copies (a whitespace difference) are not
duplicates.

## Where to go next

- [Capabilities and limits](https://claudiomascaro.github.io/meada/en/capabilities/): the detailed
  state of each language and contract.
- [Stability](https://claudiomascaro.github.io/meada/en/stability/): what 1.x freezes.
- [Server mode](https://claudiomascaro.github.io/meada/en/server/): running Meada for a team.
- A wrong result on Meada's own code is a bug: open an issue with the query, the answer and the
  expected one.
