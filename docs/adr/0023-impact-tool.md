# 23. A fourth tool, `impact`, and when a capability gets a tool

- Status: proposed
- Date: 2026-10-30

## Context

ADR 0008 fixed the MCP server at three tools, `search`, `context` and `trace`, because agents
pay for every tool description on every turn and comparable servers ship 13 to 19 tools. Since
then, R1 bounded that cost directly: the tool list stays within 1,000 tokens, and it costs 602
today.

`trace` up answers "what does a change here affect?" with a depth-first chain cut by the budget.
On a symbol many things reach, the cut keeps one deep branch and drops the direct callers after
the first (on this repository, `meada-core/src/render.rs::budgeted` up gives 6,857 lines, and a
600-token answer shows 20 of them, all on one branch). The research in
`docs/research/agent-tool-surface.md` maps the capabilities GitNexus documents for agents
against Meada's and finds that impact analysis, from a symbol or from a diff, is the gap no
`search` kind or `trace` parameter fills without changing what `trace` answers.

## Decision

- The MCP server gains a fourth read-only tool, `impact`: what reaches a symbol, a route, a call
  site or the symbols a diff touches (`uncommitted`, or `since:<ref>` from the merge base with a
  ref), breadth first. Its answer lists the symbols by depth with counts per level, the tests
  apart, the processes and repositories affected, and the open items on the upstream links
  with their `meada decide --item` command, then a footer. The budget cuts from the deepest
  depth back.
- A capability gets a new tool when, and only when, its answer needs a grammar of its own.
  An answer that fits an existing grammar is a `search` kind or an optional parameter. Every
  new tool keeps the tool list within 1,000 tokens.
- `impact` never chooses between candidates, never hides an ambiguous link and never writes:
  open items are shown with the command that closes them (ADR 0006).
- `trace` keeps every parameter and value it has, `changes: uncommitted` included.

This amends ADR 0008's "three tools". Invariant 4 of ADR 0002 holds: no tool writes, the server
stays small, and every answer has a budget.

## Considered options

- **A `summary` or `mode` parameter on `trace`.** One name, two grammars: the overloaded tool
  ADR 0008 rejected, and the agent learns the shape of the answer only after the call.
- **A separate `detect_changes` tool beside `impact`.** The same grammar from a different start:
  about 150 tokens on every turn for no new answer.
- **A risk level in the answer.** It needs thresholds someone tunes; the counts are the answer.
- **A level filter.** Hiding `ambiguous` and `inferred` hides exactly what `meada decide`
  exists for.

## Consequences

- The tool list grows by about 160 tokens.
- The output grammar reference gains `impact`'s line kinds, and its goldens join the grammar
  check.
- The stability document lists adding a tool, within the tool list's limit, as a minor change.
  Without that, `impact` waits for 2.0.
- In server mode `impact` answers from `from` only; `changes` is refused there, as `trace`'s is.
