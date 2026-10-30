# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · [Español](README.es.md)

Meada reads your repositories, maps who calls whom, and stitches one repository to the next
through the contracts they share: HTTP routes, queues, packages, shared resources. Coding
agents, and people too, query that map over MCP. No step of indexing uses an LLM.

## Installation

Pick the channel that fits your environment. All of them deliver the same `meada` binary.

```sh
# Cargo (builds with the Rust toolchain)
cargo install meada

# Shell installer (Linux and macOS)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh

# npm: downloads the binary for your platform and runs it, with nothing installed first
npx @meada/meada --version
```

- **Prebuilt binary**: download the archive for your system (Linux, macOS or Windows, on x86_64
  or arm64) from the [GitHub releases](https://github.com/ClaudioMascaro/meada/releases) and put
  `meada` on your `PATH`.
- **Container**: `ghcr.io/claudiomascaro/meada:<version>`, for Linux amd64 and arm64.

The npm package's version is always the release's. To index, Meada needs only the binary and git.
The details of each channel, the console and the container are in
[Installation](https://claudiomascaro.github.io/meada/en/installation/) and in the
[user guide](docs/user-guide.en.md) (section 1, Installation).

## First steps

In the root of a repository (here `web`, which consumes a route of `api`):

```sh
meada index     # builds the index in .meada/, for the repository and its peers
meada status    # the state of each index and the peers not found
```

For Meada to link `web` to `api`, the `meada.toml` of `web` names the peer:

```toml
[[peer]]
repo = "example.com/acme/api"       # origin's git remote, without scheme or .git
base_url_env = ["API_URL"]          # variable the client reads the base URL from
```

The full `meada.toml` reference is in the
[user guide](docs/user-guide.en.md) (section 4, `meada.toml` reference).

## Connect an agent (MCP)

`npx @meada/meada mcp` is the launch line of the MCP server: the client runs it and talks to Meada over
stdio. In your agent's configuration (for example Claude Code's `.mcp.json`):

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["@meada/meada", "mcp"] }
  }
}
```

In Claude Code, the same in one line: `claude mcp add meada -- npx @meada/meada mcp`. With `meada`
already on your `PATH`, use `"command": "meada", "args": ["mcp"]`.

The agent gets three tools, `search`, `context` and `trace`, which only read. See
[Agents and MCP](https://claudiomascaro.github.io/meada/en/mcp/) and the
[user guide](docs/user-guide.en.md) (section 2, Connect an agent).

## Status

Meada 1.0 delivers the whole v1 scope of [ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md):

- **Contracts**: HTTP routes, declared resources, OpenAPI and AsyncAPI documents, queues and
  topics, packages.
- **Levels**: a level on every link, chains that cross repositories and decisions with an
  optional decider (`meada decide`).
- **Analyses**: shape drift, communities and processes.
- **Languages in 1.0**: TypeScript and JavaScript, Python, Go and Rust. Java, Kotlin, C# and
  Swift are planned Release 2 work and arrive later as 1.x versions (#26 to #29, ADR 0003).

**Stability promise.** From 1.0 on, semver holds: a breaking change to any of these items needs
a major version.

- the names, parameters and output grammar of the MCP tools (`search`, `context` and `trace`
  today);
- the CLI commands and flags;
- the schemas of `meada.toml` and `meada.decisions.toml`;
- the format of `meada export`.

Adding an MCP tool, an optional parameter, an accepted value to a parameter, a search kind, a
field at the end of an output line, a configuration key, an export field or a language is a minor
change. Not covered: the wording of CLI messages, the text of tool descriptions, the
index file and the layout of `.meada/` (rebuilt when the schema changes), and which links Meada
finds in a code base (improving resolution is a fix, not a break). What 1.x freezes, and what it
does not, is detailed in [Stability](https://claudiomascaro.github.io/meada/en/stability/).

## Feed a knowledge base

`meada export` prints one repository's index as sorted JSON, good for `diff` and for feeding a
knowledge base. Per repository it carries the symbols (stable identity and span), the calls with
their levels, providers and consumers, links, imports, communities, processes and entry points.
It carries no source text: a reader who needs the snippet reads the span from the file.

One record per symbol, with its community, callers and callees, contracts and processes:

```sh
meada export > web.export.json
jq -c '. as $e | $e.symbols[] | .id as $id | {
  id, hash, span,
  community: ([$e.communities[]? | select(.members | index($id)) | .name] | first),
  callers: [$e.calls[] | select(.target == $id) | .caller],
  callees: [$e.calls[] | select(.caller == $id and .target) | .target],
  contracts: ([($e.providers[]?, $e.consumers[]?) | select(.site | startswith($id + "::")) | .contract] | unique),
  processes: [$e.processes[]? | select(any(.steps[]; .node == $id)) | .entry_point]
}' web.export.json

# duplicates: symbols that share a hash
jq '[.symbols[] | {id, hash}] | group_by(.hash) | map(select(length > 1))' web.export.json
```

A record per community (`communities`) or per contract (`providers`, `consumers`, `links`)
follows the same lists.

- **Fan-in** across repositories: count, per target, the `calls`, the `links` (by `provider`) and
  the `imports` of all the exports together.
- **Exposure**: the entry points in `processes` (`route`, `export`, `channel`, `program`), the
  exported names and the package consumers (`packages`, `dependencies`, `imports`).

```sh
jq -s '[.[] | (.calls[]? | select(.target) | {site: .id, target, kind: "calls"}),
              (.links[]? | {site: .consumer, target: .provider, kind: "links"}),
              (.imports[]? | {site, target, kind: "imports"})]
  | unique_by([.kind, .site, .target])
  | group_by(.target)
  | map(. as $g | {target: $g[0].target}
      + ([$g[].kind] | group_by(.) | map({(.[0]): length}) | add))' *.export.json
```

The export is per repository: an organisation runs `meada export` on each one and joins the
results by symbol identity. Embeddings and any model step live downstream, outside Meada
(invariant 1, [ADR 0002](docs/adr/0002-product-invariants.md)).

## Documentation

- [User guide](docs/user-guide.en.md)
- [Configuration](https://claudiomascaro.github.io/meada/en/configuration/)
- [Agents and MCP](https://claudiomascaro.github.io/meada/en/mcp/)
- [Server mode](https://claudiomascaro.github.io/meada/en/server/)
- [Console](https://claudiomascaro.github.io/meada/en/console/)
- [Capabilities and limits](https://claudiomascaro.github.io/meada/en/capabilities/)
- [Stability](https://claudiomascaro.github.io/meada/en/stability/)
- [Contribute](https://claudiomascaro.github.io/meada/en/contributing/)

[Apache-2.0](LICENSE).
