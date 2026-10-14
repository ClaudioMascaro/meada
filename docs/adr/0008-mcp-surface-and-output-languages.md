# 8. MCP surface and output languages

- Status: accepted
- Date: 2026-10-14

## Context

Agents pay for every tool description on every turn, and comparable code-graph
servers ship 13 to 19 tools. Meada is also a Portuguese-first project (ADR 0001),
and its output is read both by agents and by people. This ADR fixes how many tools the MCP
server has, what they return, and in which language.

## Decision

### Tools

The MCP server has three tools, and none of them writes (ADR 0002, invariant 4).

- `search` finds a symbol, a route or a contract by name.
- `context` describes one symbol: its span read from disk, its callers, its callees and its
  contracts.
- `trace` follows a chain across repositories, upstream or downstream (ADR 0005). With
  `changes: uncommitted`, it starts from the diff of the working tree, which covers impact
  analysis.

### Output

- Output is compact, line-oriented text, not indented JSON.
- Every call has a default token budget and accepts a per-call override.
- A chain footer closes the answer: the weakest level, the number of hops, the repositories
  involved and the tokens used.

### Languages

- The MCP surface is always in English: tool names, descriptions and output. It is a contract
  with agents, like code.
- CLI messages follow `LANG`. pt-BR and English come first, and any other value falls back to
  English.
- Spanish arrives as an issue labelled `translation` and `good first issue`.

## Considered options

- **One overloaded tool.** Agents pick the wrong mode.
- **About six tools.** The tool list costs tokens on every turn.
- **English-only CLI.** Contradicts the Portuguese-first principle.
- **CLI always in pt-BR.** Shuts out people who do not read Portuguese.
- **pt-BR MCP descriptions.** More tokens and less portable.
