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
prefix = "/shop"                         # optional: what the gateway adds in front of paths
strip = "/v1"                            # optional: what it removes from their start
```

With `prefix` and `strip`, a call to `/shop/orders` reaches the code route `/v1/orders`, at the
level `inferred` with the reason `gateway prefix /shop, gateway strip /v1`. A version segment
goes only when you write it in `strip`, and callers of the code path keep linking as before.

A shared resource no code names, such as a table or a bucket, goes in as `[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # lowercase identifier
id = "orders"
role = "write"                           # read, write or read-write
via = ["src/orders.ts::cancelOrder"]     # optional: the symbols that do the access
```

Every writer links to every reader of the same resource in another repository, at the level
`declared`. Without `via`, or when no `via` matches a symbol, the side is the whole repository.

Meada finds peers by their git remote, even when the clone sits at a different path on each
machine. An unknown or misspelled key gives an error naming the file, the line and the key.
Without a `meada.toml`, the repository declares no peers.

## Usage

At the root of the repository:

- `meada index` builds the index, in `.meada/`, of the repository and its peers;
- `meada status` shows the set of repositories, the state of each index and any peer that was
  not found; under each repository, the unlinked resources and each `via` that matches no
  symbol;
- `meada export` prints the whole index as sorted JSON, good for diffing two runs;
- `meada decide` resolves ambiguous links and dynamic URLs, see Decisions;
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

- `search` finds a symbol, a route or a contract by name; with the kind `resource`, a resource
  by its kind or id; with the kind `ambiguous` or `unresolved`, it lists the open items (see
  Decisions);
- `context` describes one symbol: its code read from disk, its callers, its callees and the
  contracts it touches; on a resource (`resource::postgres::orders`), it lists who writes and
  who reads it, with the symbol and line, or `(repository-level)` for the whole repository;
- `trace` follows a chain of calls across repositories, downstream (`down`) or upstream (`up`);
  with `changes: uncommitted`, it starts from the working-tree diff, which covers the impact
  analysis of what was just edited.

Answers are compact lines under a token budget (2,000 by default, adjustable per call). Each
link between repositories carries its level and reason (`→[level: reason]`), and `trace` ends
with a footer: the weakest level of the chain, the number of hops, the repositories involved
and the tokens used.

## Decisions

An ambiguous link (several providers serve the route) or a dynamic URL (the call builds its URL
at run time) stays an open item. The MCP `search` tool, with the kind `ambiguous` or
`unresolved`, lists these items with how many candidates each has, not the candidates;
`meada decide --list` lists the items with their candidates. You resolve them in human mode, or
a model does if you ask.

- `meada decide` shows each open item of the repository, with numbered candidates; you pick a
  number, or `s` to skip. At the end it writes the picks to `meada.decisions.toml`, at the
  root. Nothing picked, nothing written.
- `--item <id>` works on that item only (the id `meada decide --list` or the `search` tool
  prints), even one already decided, to redo its decision; `--dry-run` prints the file that
  would be written and writes nothing; `--list` prints the open items plus the stale
  decisions and asks nothing; `--prune` removes the stale decisions (it does not combine with
  the previous three).
- `--decider` swaps the person for a model (see below); `--show-request` (only with
  `--decider`) prints every request that would be sent and sends none; `--min-p <p>` (only with
  `--decider`, from 0 to 1) is the least probability for the model to write a decision.

`meada.decisions.toml` is a file versioned in git, like a lockfile: its diff is what you
review. Each `[[decision]]` holds the call (`call`), the chosen provider (`target`), who decided
(`by`: `human` or the model id) and, for a model, the probability (`p`). Indexing only reads
the file back: the decided link gets the level `decided` and its reason says who decided. When
the code changes and a decision no longer fits (the call is gone, the item is no longer open,
the provider is gone or is no longer a candidate), `meada status` reports it as stale, under
its repository, and `meada decide --prune` removes it. A decision waiting for a peer that has
not been found yet is shown apart and kept.

### Model decider

`meada decide --decider` is the only path from Meada to the network, and it runs only when you
ask. Four environment variables configure it (nothing comes from a file):

| Variable | Role | Default |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` or `chat` | `decisions` |
| `MEADA_DECIDER_URL` | base URL | `https://openrouter.ai` (`decisions`); required with `chat` |
| `MEADA_DECIDER_KEY` | API key, sent only in the `Authorization` header | required with `decisions`; optional with `chat` |
| `MEADA_DECIDER_MODEL` | model | `typesafe/jev-1.13` (`decisions`); required with `chat` |

- **`decisions`** (the default) uses OpenRouter's decisions API, with Jev: a single request
  carries every item, and the response gives the probability of each candidate. The request
  asks for zero data retention (`zdr: true`, `data_collection: deny`); if OpenRouter refuses
  that preference, the request fails and nothing is written.
- **`chat`** talks to any endpoint compatible with OpenAI's chat API
  (`<URL>/chat/completions`), a local model included, for example through Ollama:
  `MEADA_DECIDER_PROTOCOL=chat MEADA_DECIDER_URL=http://localhost:11434/v1
  MEADA_DECIDER_MODEL=<model> meada decide --decider`. One request per item. The probability is
  the one the model reports about itself, not a calibrated measure.

Any gateway that speaks either protocol works: just change the URL. HTTPS anywhere; HTTP only
to `localhost` or a loopback address.

What leaves the machine, per item: the call, the function that contains it (read from disk) and
the list of candidates (repository, method and path), plus the id of the repository that makes
the call. No provider code, no other file. `--show-request` shows all of it first, with the key
replaced by `<hidden>`. Each answer is validated per item; an invalid one, or a request that
failed, is reported without writing that item, and the command exits with an error.

Fixed constants:

- `--min-p` defaults to 0.5: below it the proposal is printed as a suggestion and not written.
  It changes only through a PR with data.
- At most 50 candidates per item: with more, the item is skipped in decider mode (human mode
  still offers it).
- 120 seconds per request, from connection to last byte, with no retry.
- With the `decisions` protocol, the probabilities of an answer must sum to 1 within a
  tolerance of 0.01.

## Status

Version 0.1, the first. It indexes TypeScript and JavaScript repositories (`.ts`, `.tsx`, `.js`,
`.jsx`, `.mjs`, `.cjs`) and links repositories through HTTP contracts (`fetch` on the consuming
side, Next.js (app router) and Express on the serving side, also behind a gateway), through
the resources declared in `meada.toml` and through channels: `kafkajs` Kafka topics, with the
producer linked to the handler of each consumer, in the same repository too. Each link has the
level `exact`, `declared`, `inferred`, `decided` or `ambiguous`; a call whose URL cannot be
resolved shows up as unresolved and becomes a link only through a decision, at the level
`decided`. Broker clusters are not modelled: two clusters that each have a topic with the same
name link as if they were one. Not there yet: other languages, HTTP clients other than `fetch`,
brokers other than Kafka, packages and the npm package. The scope and order of the work are in
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
