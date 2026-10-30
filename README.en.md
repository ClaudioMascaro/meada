# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · [Español](README.es.md)

Meada reads your repositories, maps who calls whom, and stitches one repository to the next
through the contracts they share: HTTP routes, queues, packages, shared resources. Coding
agents, and people too, query that map over MCP. No step of indexing uses an LLM.

## Installation

Pick the row that matches what you already have. Every channel delivers the same `meada`
binary, and the last column is the command your agent's MCP configuration runs.

| You have | Install | MCP server command |
| --- | --- | --- |
| Node.js | nothing: `npx -y @meada/meada --version` | `npx -y @meada/meada mcp` |
| Rust | `cargo install meada` | `meada mcp` |
| Linux or macOS | the shell installer below | `meada mcp` |
| Any system | the archive for your system from the [GitHub releases](https://github.com/ClaudioMascaro/meada/releases), with `meada` on your `PATH` | `meada mcp` |
| Docker | `docker pull ghcr.io/claudiomascaro/meada:1.0.0` | the `docker run` line in [Connect an agent](#connect-an-agent-mcp) |

```sh
# Shell installer (Linux and macOS)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh
```

The prebuilt archives cover Linux, macOS and Windows on x86_64 and arm64; the image covers Linux
amd64 and arm64. The npm package's version is always the release's. To index, Meada needs only
the binary and git. The details of each channel, the console and the container are in
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

The client runs the MCP server command and talks to Meada over stdio. In your agent's
configuration (for example Claude Code's `.mcp.json`), with npx:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["-y", "@meada/meada", "mcp"] }
  }
}
```

In Claude Code, the same in one line: `claude mcp add meada -- npx -y @meada/meada mcp`. With
`meada` already on your `PATH` (Cargo, installer or archive), use
`"command": "meada", "args": ["mcp"]`.

With Docker (Linux and macOS):

```json
{
  "mcpServers": {
    "meada": {
      "command": "sh",
      "args": ["-c", "exec docker run -i --rm --network none --user \"$(id -u):$(id -g)\" -v \"${PWD%/*}:${PWD%/*}\" -w \"$PWD\" ghcr.io/claudiomascaro/meada:1.0.0 mcp"]
    }
  }
}
```

The client starts the server in your repository. Meada answers about the path the client sends
as its root, so the container mounts the folder that holds the repository, and its sibling
peers, at that same path; it runs as your user, so `.meada/` stays yours, and without a network,
which neither indexing nor stdio needs.

The agent gets three tools, `search`, `context` and `trace`, which only read. See
[Agents and MCP](https://claudiomascaro.github.io/meada/en/mcp/) and the
[user guide](docs/user-guide.en.md) (section 2, Connect an agent).

### Agent skill

[`skills/meada/SKILL.md`](skills/meada/SKILL.md) teaches an agent when to reach for each tool
and how to read the levels in an answer. For Claude Code:

```sh
mkdir -p ~/.claude/skills/meada
curl -fsSL https://raw.githubusercontent.com/ClaudioMascaro/meada/v1.0.0/skills/meada/SKILL.md -o ~/.claude/skills/meada/SKILL.md
```

Other agents: see [Agents and MCP](https://claudiomascaro.github.io/meada/en/mcp/#agent-skill).

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
- [Deploy with Docker, Compose or Helm](https://claudiomascaro.github.io/meada/en/deploy/)
- [Console](https://claudiomascaro.github.io/meada/en/console/)
- [Capabilities and limits](https://claudiomascaro.github.io/meada/en/capabilities/)
- [Stability](https://claudiomascaro.github.io/meada/en/stability/)
- [Contribute](https://claudiomascaro.github.io/meada/en/contributing/)

[Apache-2.0](LICENSE).
