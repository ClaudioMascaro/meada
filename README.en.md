# Meada

[Português (Brasil)](README.md) · **English** · [Español](README.es.md)

> This is a translation. The Brazilian Portuguese [README](README.md) is the source; if they
> disagree, it wins, and a fix to this page is welcome.

Meada reads your repositories, maps who calls whom, and stitches one repository to the next
through the contracts they share: HTTP routes, queues, packages, shared resources. Coding
agents, and people too, query that map over MCP. No step of indexing uses an LLM.

## Installation

Pick one of three ways.

- **Cargo**: `cargo install meada`.
- **Prebuilt binary**: download the file for your system from the
  [GitHub releases](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS and
  Windows, on x86_64 and arm64) and put `meada` on your `PATH`.
- **Installer** (Linux and macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.

## Configuration

Each repository can have a `meada.toml` at its root, versioned in git, saying who it talks to.
A repository that consumes a route names its peer:

```toml
[[peer]]
repo = "example.com/acme/api"            # origin git remote, no scheme and no .git
base_url_env = ["API_URL"]               # variable the client reads its base URL from
# path = "../api"                        # optional: a hint of where the clone is
```

A repository that serves the route declares the hosts it answers on:

```toml
[serve]
hosts = ["api.example.com"]
```

Meada finds peers by their git remote, even when the clone sits at a different path on each
machine. An unknown or misspelled key gives an error naming the file, the line and the key.
Without a `meada.toml`, the repository declares no peers.

## Usage

At the root of the repository:

- `meada index` builds the index, in `.meada/`, of the repository and its peers;
- `meada status` shows the set of repositories, the state of each index and any peer that was
  not found;
- `meada export` prints the whole index as sorted JSON, good for diffing two runs;
- `meada mcp` serves the tools below over MCP.

Meada only writes inside `.meada/`, which ignores itself in git. Every query first refreshes
the index with what changed in the files, including what is not committed yet. Command-line
messages follow `LANG` (Portuguese or English); MCP messages are always in English.

## Agents (MCP)

`meada mcp` speaks MCP over stdio and must be started inside the repository. In your agent's
configuration (for example Claude Code's `.mcp.json`):

```json
{
  "mcpServers": {
    "meada": { "command": "meada", "args": ["mcp"] }
  }
}
```

There are three tools, always in English, and none of them writes:

- `search` finds a symbol, a route or a contract by name;
- `context` describes one symbol: its code read from disk, its callers, its callees and the
  contracts it touches;
- `trace` follows a chain of calls across repositories, downstream (`down`) or upstream (`up`);
  with `changes: uncommitted`, it starts from the working-tree diff, which covers the impact
  analysis of what was just edited.

Answers are compact lines under a token budget (2,000 by default, adjustable per call). Each
link between repositories carries its level and reason (`→[level: reason]`), and `trace` ends
with a footer: the weakest level of the chain, the number of hops, the repositories involved
and the tokens used.

## Status

Version 0.1, the first. It indexes TypeScript and JavaScript repositories (`.ts`, `.tsx`, `.js`,
`.jsx`, `.mjs`, `.cjs`) and links repositories through HTTP contracts: `fetch` on the consuming
side, Next.js (app router) and Express on the serving side. Each link has the level `exact`,
`inferred` or `ambiguous`; a call whose URL cannot be resolved shows up as unresolved, never as
a link. Not there yet: other languages, HTTP clients other than `fetch`, queues, packages, the
`meada decide` command and the npm package. The scope and order of the work are in
[ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md); the decisions are in
[`docs/adr`](docs/adr). See the [CHANGELOG](CHANGELOG.md).

## Why another tool

Anyone who works across several services knows the bug rarely lives in a single repository. The
frontend calls a route that changed in the backend; the worker consumes a queue another team
renamed. Among today's code graph tools, some look at one repository at a time, some link
repositories only when the route text matches exactly, and some forbid commercial use.

Meada wants to follow the thread end to end, across as many repositories as it takes, and say
how confident it is in each link.

## Principles

- **Your code stays with you.** It runs on your machine, with no account, no API key and no
  telemetry.
- **No LLM to index.** The map comes from the code's syntax tree. No per-token bill in dollars;
  it works offline and fits on an ordinary laptop.
- **Deterministic.** The same code with the same configuration always gives the same map. When
  a link is ambiguous, the decision is recorded in a file versioned in git, made by a person
  or, if you choose, by a language model in a separate command. Indexing only re-reads that
  decision.
- **Portuguese first.** Documentation is born in Brazilian Portuguese; English and Spanish are
  translations. Issues and PRs are welcome in any of the three languages. The code is written
  in English so that anyone can read it.
- **Gather without blending.** "Not everything that comes together blends," taught Antônio
  Bispo dos Santos, known as Nêgo Bispo. Each repository keeps its own index; Meada only
  stitches the ends together.
- **Little noise for the agent.** Meada does not write to your `CLAUDE.md`, install hooks or
  touch your editor configuration unless you run a command for it. Answers are short.
- **Credit to those who came first.** Meada learns from GitNexus, CodeGraph,
  codebase-memory-mcp and kivgraph.

## How to take part

The project is just starting. The v1 scope and the order of work are in
[ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md). You can already help:

- open an issue describing your cross-repository use case, in Portuguese, Spanish or English;
- review the translations: if the English or the Spanish sounds off, a fix is welcome;
- read and question the ADRs.

## The name

A *meada* is a skein of thread. *Puxar o fio da meada*, "pulling the skein's thread", means
finding the end that undoes the knot, which is what the tool tries to do when it follows a call
from one repository to the next.

## License

[Apache-2.0](LICENSE).
