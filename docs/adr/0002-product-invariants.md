# 2. Product invariants

- Status: accepted
- Date: 2026-10-14

## Context

A measurement on a 633-file TypeScript repository (2026-10-11) found GitNexus taking 86 s and
158 MB to index, adding roughly 1.2k tokens to every agent turn through instructions and skills
it writes into the repository, and editing the user's `CLAUDE.md`, `AGENTS.md` and editor
configuration without asking. A second tool on the same repository indexed in 7 s and answered
the same navigation question in one call. These invariants name what Meada keeps true so that
it never drifts into that cost profile.

Scoping v1 (ADR 0003) added one more aim: the same map every time, with the few judgments a
deterministic core cannot make recorded in git instead of recomputed. Two invariants were
amended for it (2 and 4) and one was added (6).

## Decision

1. Indexing never calls an LLM or the network.
2. Local-first: no account, no telemetry, and no key needed to run. Source code never leaves
   the machine, with one exception: the opt-in `meada decide` command may send the minimal code
   excerpt a decision needs to a provider the user configured with their own key (ADR 0006).
   Indexing and the MCP server never do. (Amended 2026-10-21; in server mode, ADR 0011, "the
   machine" is the operator's boundary: the HTTP server returns span text to the clients the
   operator admits, the stdio server is unchanged.)
3. Meada writes only inside its own index directory. Changes to the user's files (agent
   instructions, editor configuration, hooks) happen only through an explicit command the user
   runs.
4. The MCP server has no tool that writes: it never changes the user's files, configuration or
   decisions. Its only write is refreshing Meada's own index before it answers (invariant 3,
   ADR 0004). (Amended 2026-10-21; in server mode, ADR 0011, the server writes nothing: the
   operator refreshes with `meada index`.) It exposes few tools and returns compact output under a default token budget. A
   write tool needs a new ADR. Peers are local paths only: Meada never clones or fetches a peer,
   so the network stays out of indexing.
5. Each repository keeps its own index; cross-repo context links indexes without merging them.
6. Determinism: the same inputs (the source tree, `meada.toml` and `meada.decisions.toml`) give
   the same answers and the same canonical export (`meada export`, sorted JSON). The index file
   does not need to be byte-identical.

## Consequences

- Features that need a model (summaries, naming clusters with prose) live outside indexing, as
  opt-in commands, or not at all.
- A model's output enters Meada only as a recorded decision in a versioned file, never as a step
  of the pipeline. Indexing replays the file, so invariants 1 and 6 hold with deciders in use
  (ADR 0006).
- Agent integrations ship as opt-in commands rather than install-time side effects.
- Cross-repo queries read several indexes at once, so the index format needs a stable identity
  for symbols and contracts across versions (ADR 0004).
- Results never depend on timeouts or machine speed. This makes golden-file tests possible and
  keeps answers diffable between runs and between machines.
- The determinism guarantee covers answers and the canonical export, not the bytes of the index
  file. Tying it to the file would bind it to the SQLite version and page layout.
- "Read-only by default" was rejected for invariant 4: it lets write tools in quietly, as prior
  art does by enabling them by default.
