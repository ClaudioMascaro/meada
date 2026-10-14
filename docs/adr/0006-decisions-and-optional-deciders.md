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
  shape drift triage are not decision kinds in v1.
- Decisions apply as soon as they are written. There is no offline evaluation gate. Safety comes
  from three things: the deterministic core stays primary, the command is opt-in, and git
  history plus PR review cover the file. How decisions with a low `p` are treated is left to the
  track A spec (ADR 0003).

### Decider endpoint

A decider speaks one of two protocols. Its URL, key and model come from the environment.

- **Decisions**: OpenRouter's decisions API, where each open item is a typed `choice` question
  and the answer carries a choice, a confidence and a probability per candidate. This is the
  documented default, with Jev, Typesafe's model for typed judgments, which is cheap per token,
  reached with zero-data-retention routing. The API is labelled alpha upstream, so its adapter
  is isolated and validates request and response shapes strictly.
- **Chat**: any OpenAI-compatible chat completions endpoint, answering a JSON schema with the
  chosen candidate and a self-reported probability. A local model, for example through Ollama,
  works here for zero-cost, fully local use. Economic access is a project principle.
- Any gateway that speaks either protocol works, including regional ones that bill in local
  currency. The documentation lists them once they are public.

(Amended 2026-10-14. The first version said any OpenAI-compatible endpoint with Jev as the
default, but Jev answers through the decisions API, not chat completions.)

## Considered options

- **Jev through OpenRouter only.** Vendor lock-in, and no local option.
- **OpenAI-compatible chat only.** Jev does not speak it, so the typed judgment with
  probabilities that motivated the default would be lost.
- **The vendor's direct API only.** It offers no zero-data-retention outside enterprise
  contracts.

## Consequences

- A decision made by a model is as reviewable as one made by a person: it is a diff in a file.
  The review is where a wrong decision gets caught.
- The excerpt `meada decide` sends is the minimal code needed for the judgment (invariant 2).
  What counts as minimal belongs to the track A spec.
