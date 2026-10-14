# 2. Product invariants

- Status: proposed (to be confirmed while scoping v0.1)
- Date: 2026-10-14

## Context

A measurement on a 633-file TypeScript repository (2026-10-11) found GitNexus taking 86 s and
158 MB to index, adding roughly 1.2k tokens to every agent turn through instructions and skills
it writes into the repository, and editing the user's `CLAUDE.md`, `AGENTS.md` and editor
configuration without asking. A second tool on the same repository indexed in 7 s and answered
the same navigation question in one call. These invariants name what Meada keeps true so that
it never drifts into that cost profile.

## Decision

1. Indexing never calls an LLM or the network.
2. Local-first: no account, no API key, no telemetry; source code never leaves the machine.
3. Meada writes only inside its own index directory. Changes to the user's files (agent
   instructions, editor configuration, hooks) happen only through an explicit command the user
   runs.
4. The MCP server is read-only by default, exposes few tools, and returns compact output under
   a default token budget.
5. Each repository keeps its own index; cross-repo context links indexes without merging them.

## Consequences

- Features that need a model (summaries, naming clusters with prose) live outside indexing, as
  opt-in commands, or not at all.
- Agent integrations ship as opt-in commands rather than install-time side effects.
- Cross-repo queries read several indexes at once, so the index format needs a stable identity
  for symbols and contracts across versions.
