# 6. Decisions and optional deciders

- Status: accepted
- Date: 2026-10-14

## Context

A deterministic core cannot resolve every link. Two cases stay open:

- **Ambiguous link:** a call matches two or more providers.
- **Dynamic URL:** a call whose URL value the core cannot follow.

Something has to resolve them, once, and the answer must not change from run to run (ADR 0002,
invariant 6). A person can resolve them. A language model can too, and some users will want
that. Neither may enter indexing (invariant 1).

## Decision

- The core marks these links `ambiguous` (ADR 0005). It never guesses.
- `meada.decisions.toml`, at the repository root, records each resolution. It is versioned and
  reviewed in PRs like a lockfile.
- Only `meada decide` writes the file. Nothing else does, and the MCP server never does
  (invariant 4).
- Each decision records `by`, which is `human` or a model id, and `p`, the probability the
  decider stated. A decision refers to its call site and candidate by the stable identities of
  ADR 0004.
- Indexing replays the file and gives the link the `decided` level. A model's output is an
  input artifact, never a step of the pipeline, so invariants 1 and 6 hold.
- The decidable kinds through v1 are ambiguous links and dynamic URLs. Community names and
  shape drift triage are not decidable.
- Decisions apply as soon as they are written. There is no offline evaluation gate. Safety comes
  from three things: the deterministic core stays primary, the command is opt-in, and git
  history plus PR review cover the file. How decisions with a low `p` are treated is left to the
  track A spec (ADR 0003).

### Decider endpoint

- A decider is any OpenAI-compatible endpoint. Its URL and its key come from the environment.
- The documented default is Jev, Typesafe's model for typed judgments with probabilities, which
  is cheap per token. It is reached through OpenRouter with zero-data-retention routing.
- A local model, for example through Ollama, works for zero-cost, fully local use. Economic
  access is a project principle.
- Any compatible gateway works, including regional ones that bill in local currency. The
  documentation lists them once they are public.

## Considered options

- **Jev through OpenRouter only.** Vendor lock-in.
- **The vendor's direct API only.** It offers no zero-data-retention outside enterprise
  contracts.

## Consequences

- A decision made by a model is as reviewable as one made by a person: it is a diff in a file.
  The review is where a wrong decision gets caught.
- The excerpt `meada decide` sends is the minimal code needed for the judgment (invariant 2).
  What counts as minimal belongs to the track A spec.
