# Prior art

Meada builds on work others published first. This page credits them and records what each
project does, from its public documentation and from observing its shipped package, so that
Meada's specs can say where they agree and where they differ. Meada copies no code from any of
them (ADR 0001).

| Project | License | Core | Cross-repo |
| --- | --- | --- | --- |
| [GitNexus](https://github.com/abhigyanpatwari/GitNexus) | PolyForm Noncommercial 1.0.0 | TypeScript. tree-sitter into a graph database (LadybugDB); Leiden communities; execution "processes" traced from entry points; BM25 with optional local embeddings; 19 MCP tools, Claude Code hooks, generated skills; optional CFG/taint analysis | "Groups": HTTP, gRPC, Thrift, topic and GraphQL contracts matched by normalized identifier, followed one hop between repositories |
| [CodeGraph](https://github.com/colbymchenry/codegraph) | MIT | Rust kernel with a TypeScript shell. tree-sitter into SQLite with FTS5; one default MCP tool (`codegraph_explore`) returning symbols, calls and blast radius | Queries several indexed projects in one session; no links between them |
| [codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp) | MIT | C, single static binary. tree-sitter into SQLite; Louvain communities; BM25, structural and semantic search; 17 MCP tools; git diff to affected symbols | Cross-repo edges for HTTP, gRPC, GraphQL, tRPC, async calls and channels |
| [kivgraph](https://github.com/Luqueee/kivgraph) | Apache-2.0 | Go. Type checkers (go/types, TypeScript checker, rust-analyzer) into LadybugDB; 13 read-only MCP tools | Package-level and explicitly declared provider edges; no HTTP, gRPC or queue discovery |

## Where Meada wants to differ

These are the gaps observed in the projects above that Meada's first specs will weigh:

1. **Chains across repositories.** Follow a call or a change through A → B → C, not only one hop.
2. **Contract matching that understands real systems.** Base URLs, gateway prefixes, API
   versions, hosts read from environment variables, OpenAPI and AsyncAPI documents as contract
   sources, each link carrying an explicit confidence.
3. **Shape drift between consumer and provider.** A response field the consumer reads but the
   provider in another repository no longer returns.
4. **Cross-repo configuration in version control,** found by the agent without a special flag.
5. **Incremental recompute scoped by dependency,** with source spans read from disk instead of
   source text copied into the index.
6. **Low context cost.** Few tools, compact output, a default token budget, nothing injected
   into every agent turn.
7. **Nothing written outside Meada's own index** without an explicit command.
8. **Staleness that sees uncommitted changes.**

## Measurement

On 2026-10-11, on a 633-file TypeScript repository: CodeGraph indexed in 7.4 s into 37 MB;
GitNexus indexed in 85.9 s into 158 MB and wrote about 1.2k tokens of instructions and skill
descriptions that load on every agent turn.
