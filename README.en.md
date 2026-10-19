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
  by its kind or id; with the kind `channel`, a channel (queue or topic) by broker or name;
  with the kind `ambiguous` or `unresolved`, it lists the open items (see
  Decisions);
- `context` describes one symbol: its code read from disk, its callers, its callees and the
  contracts it touches; on a resource (`resource::postgres::orders`), it lists who writes and
  who reads it, with the symbol and line, or `(repository-level)` for the whole repository; on
  a channel (`channel::kafka::orders.created`), it lists producers and consumers, those that
  reach it through a wildcard or a binding included, with the level and the reason, and a side
  whose name Meada cannot evaluate shows as `(unresolved)` and is never linked;
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
| `MEADA_DECIDER_MODEL` | model | `typesafe/jev-1.14` (`decisions`); required with `chat` |

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
`.jsx`, `.mjs`, `.cjs`) and Python ones (`.py`), reads the Rust code (`.rs`) of the
repository's crates (each `Cargo.toml` with a `[package]`, in any directory, each crate
through the module tree its `mod` declarations build, with no Rust toolchain: a file no `mod`
reaches stays out) with its functions, types, traits, modules and `impl` methods and the calls
written as paths (`crate::a::f()`, `Type::new()`, `Self::f()`), also through `use` (groups,
renames, globs, `pub use` and the other workspace crates by name; method calls (`x.m()`) go through the receiver's type when the code writes it (`let`, parameter, field, `self`, function return; a `dyn Trait` or trait-bounded generic goes to the trait's method; a method the `impl` blocks of a repository type lack, from a `derive` or an outside trait, links to nothing, and a route attribute never links to a function), else by name; calls inside macros are not
indexed, and the Rust contracts are packages, HTTP and channels (below); only each crate's default configuration is read, for a fixed x86_64 Linux target: an item under a false `#[cfg(..)]`, or under a feature outside the `Cargo.toml` `default`, stays out, and `mod` under `#[cfg(test)]` is in), reads Go code (`.go`, with no Go toolchain: an import reaches the package directory through the module path of each `go.mod`, also through a `replace` to a directory of the repository, and a nested module owns its subtree; the files of one directory with one `package`, `_test.go` included, share a scope; build constraints hold for a fixed linux/amd64 target, and a `_windows.go` or a false `//go:build` stays out) with its functions, types, interfaces and methods (`Type.Method`) and the calls to functions of the same package and through imports (`pkg.F()`, aliased or dot imports; the standard library and outside modules stay external) and to methods (`recv.M()`: `exact` when the code writes the receiver's type, in a parameter, field, `var`, composite literal or function result, and by name, `inferred` or `ambiguous`, when it does not; a call through an interface stops at the interface method), and links repositories,
also from one language to the other, through HTTP contracts (`fetch` and, in Python, `requests`, `httpx` and `aiohttp` and, in Rust, `reqwest` and `ureq` and, in Go, `net/http` and resty
on the consuming side; Next.js (app router) and Express and, in Python, FastAPI, Flask and
Django and, in Rust, axum, actix-web and Rocket and, in Go, `net/http`, gin, echo, chi and gorilla/mux on the serving side, also behind a gateway; in Rust, a base URL read from the environment with `std::env::var`, `env!` or `dotenvy::var` and, in Go, with `os.Getenv` or `os.LookupEnv` (with a default `if v == "" { v = ... }` and through `strings.TrimSuffix`, `TrimRight` or `TrimSpace`) links through `base_url_env`), through
the resources declared in `meada.toml` and through channels: Kafka topics (`kafkajs` and, in Python,
`confluent-kafka` and `kafka-python` and, in Rust, `rdkafka`), RabbitMQ queues (`amqplib` and, in Python, `pika` and, in Rust, `lapin`, with a
`publish` to an exchange reaching the queues its bindings tie to it), Amazon SQS queues (AWS SDK
v3, `sqs-consumer` and, in Python, `boto3` and, in Rust, `aws-sdk-sqs`), NATS subjects (`nats` and, in Python, `nats-py` and, in Rust, `async-nats`, with
the `*` and `>` wildcards), BullMQ jobs and Redis streams (`ioredis`, `node-redis` and, in Python,
`redis-py` and, in Rust, the `redis` crate), with the producer linked
to the handler of each consumer, in the same repository too. A repository with a `[serve]` table
also reads its OpenAPI (3.0 and 3.1) and AsyncAPI (2.x and 3.0) documents, found by name or
listed in `[serve] documents`: the routes and channels they describe become contracts, with the
fields of each response or payload, and `meada status` says what it read and what it ignored.
Each named `package.json` becomes the contract `package::npm::<name>`, and each dependency
(`dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies`) that names a
package of the workspace (from any repository, its own included) links to it, with the version range checked
against the local version (`range mismatch` when it does not accept it, `range unchecked` when
it cannot be checked). A call to a function imported from that package crosses into the
provider's code, through the `exports`, `types`, `module` and `main` fields and through
re-exports, also when the entry point points into the build folder (the `outDir` of
`tsconfig.json`, mapped back to its `rootDir`). `search` with the kind `package` finds the
package by name, with its repository, folder, version and number of consumers, and `context` on
an exported function lists who calls it, in any repository of the workspace. In Python, each
named `pyproject.toml` (`[project]` or `[tool.poetry]`) becomes the contract
`package::pypi::<name>`, the name normalized as PEP 503 does, and each dependency in `[project]
dependencies`, `[tool.poetry.dependencies]` or a `requirements*.txt` that names a package of the
workspace links to it, with the range as written and `range unchecked` (PyPI ranges are not
checked); an `import` of one of that package's modules crosses into the provider's code,
`exact` when the module bears the distribution's name, `inferred` when it is only one of its
top-level modules and `ambiguous` when several expose it. An npm and a PyPI package of the same
name never link. In Rust, each `Cargo.toml` with a `[package] name` (every member of a Cargo
workspace is a provider) becomes the contract `package::cargo::<name>`, and each entry of
`[dependencies]`, `[dev-dependencies]` and `[build-dependencies]` links to it: by `path`, to the
crate in that directory, in the same repository or, when the path leaves it (`../other`), in
another repository of the workspace (the path is resolved from the text alone, against the
repositories' roots, reading nothing outside them and following no symlink; a path that lands in
no workspace repository appears nowhere); by `git`, to the repository whose identity is the
normalized URL; by version, to the provider of the same name in the workspace; `package =` gives
the real name of a renamed dependency, `workspace = true` reads `[workspace.dependencies]` and
`version.workspace = true` reads `[workspace.package]`. The link is `exact`, with the range as
written and `range unchecked` (Cargo ranges are not checked), and a `use` of the crate (by its
`[lib] name`, if any, or by the key of a renamed dependency) reaches the provider's public items
through its `pub use` re-exports, also `exact`, with `; import crate name` ending the reason. A
dependency outside the workspace appears nowhere. In Go, each `go.mod` with a `module` becomes
the contract `package::go::<module path>`, and each `require`, and each `replace` to a local
directory, links to that module's provider in the workspace: a `replace` that leaves the
repository (`../other`) is resolved from the text alone, like Cargo's `path`, and a `replace` by
another module leaves the module out. The link is `exact`, with the `require` version as the
range, `(local none)` (a `go.mod` has no version) and `range unchecked`, and an import of
`<module>/<directory>` reaches the exported functions, types and methods of that directory in the
provider, also `exact` when the directory exists, with `; import module path` ending the reason.
It reads neither `go.sum` nor the module proxy. It reads no lockfile (`Cargo.lock`
included) and queries neither the npm registry, PyPI nor crates.io. Meada also reports shape drift: the fields a `fetch` consumer reads from the parsed response that the route it calls does not return, compared with the route's shape (an OpenAPI document, a zod schema or the object literals of its handler). `trace` ends the link line with `drift: missing <path>`, `context` on a route names where its shape comes from and lists each consumer that reads fields of the response, with its drift and level and the reads not checked under an open part when there are any, `search` with the kind `drift` lists all the drift, and `meada status` counts the missing reads of each repository's consumers (the consumer's repository counts them; the provider's shows nothing). `meada export` carries each repository's communities: groups of symbols that call each other more than they call the rest, found with the Leiden algorithm with nothing random and integer arithmetic only, and named after the directory most of their members live in (`src`, `src#2`), the same on every machine. Each link has the level `exact`, `declared`, `inferred`, `decided` or `ambiguous`; a call whose URL cannot be
resolved shows up as unresolved and becomes a link only through a decision, at the level
`decided`, and a route Meada cannot place stays out of the map rather than being guessed. Broker
clusters are not modelled: two clusters that each have a topic with the same
name link as if they were one. Each route a handler serves, each channel a consumer handler consumes and each exported function or method nothing in its own repository calls is an entry point with its process: `trace` with `process::<entry point identity>` reads the call tree it sets in motion, across repositories, and `meada export` carries the process of each entry point. In Python, a file's `if __name__ == "__main__":` block (the symbol `__main__`) and each function a `[project.scripts]` entry of `pyproject.toml` names (`module:function`) are entry points too, and only public names (no leading `_`, or those of a literal `__all__`) count as exported. In Rust, the `fn main` of each binary (`src/main.rs`, `src/bin/`, `[[bin]] path`) is one too. In Go, each `func main` of a `package main` is one too. `search` with the kind `community` finds a community by name and with the kind `process` finds a process by its entry point's label, and `context` on a symbol names its community (or `community: none`) and the processes it belongs to, found by walking upstream over the same calls and links a process follows. Not there yet: other languages, HTTP clients other than these,
messaging clients other than these, or Meada's npm package. The scope and order of the work are in
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
