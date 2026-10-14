# 3. v1 scope and roadmap

- Status: accepted
- Date: 2026-10-14

## Context

ADR 0001 founded the project and ADR 0002 fixed its invariants. Neither says what Meada has to
do, in what order, or when it counts as ready. Scoping only the first release would hide the
work that makes Meada different from the projects in `docs/prior-art.md`: chains across
repositories, decided links and shape drift. The scope was therefore set for v1 as a whole, and
this ADR records it with its order and its acceptance numbers.

## Decision

### Scope of v1

- Contract kinds: HTTP routes, declared shared resources, OpenAPI and AsyncAPI documents,
  queues and topics, packages (ADR 0005).
- Levels on every link, chains across repositories, and recorded decisions with optional
  deciders (ADRs 0005 and 0006).
- Shape drift between consumer and provider (ADR 0009).
- Languages: TypeScript and JavaScript first, then Python, Go, Java, Kotlin, C#, Swift and Rust
  (ADR 0007).
- Communities and processes, computed deterministically.
- The MCP surface of ADR 0008, distribution and community practice of ADR 0010.

### Releases and 1.0

- Public 0.x releases ship along the way. Every merged spec becomes the next 0.x. A version
  number promises no content.
- 1.0 is the semver stability promise: MCP tool names and shapes, CLI flags and the config
  format. It is tagged only when the whole v1 scope exists and the acceptance numbers below are
  met.
- The announced launch happens when every differentiator exists for TypeScript and JavaScript,
  which is the end of track A. Earlier releases are public but quiet.

### Order of work

- **v0.1 trunk**, alone. It builds the seams: the language adapter with its conformance suite
  over fixtures (ADR 0007), the contract-kind interface, the level vocabulary and the three MCP
  tools (ADR 0008).
- **Track A, differentiators**, in this order: decisions, then resources and gateway, then
  OpenAPI and AsyncAPI, queues and packages, then shape drift.
- **Track B, languages**: one small spec per language pack, in any order. Packs open as
  `good first issue` after the announcement.
- **Track C**: deterministic communities and processes.
- At most two runs go at once, usually A and B. Each has its own integration branch and draft
  PR. Tracks touch disjoint directories.
- Planning is pipelined: the next spec is grilled and ticketed while the current one is
  implemented. A spec builds only on merged `main`.
- The roadmap lives in this ADR. Each spec is written when its work starts. The v0.1 trunk is
  the first one.

### Benchmark and acceptance

The maintainer's private dogfood repositories are the local benchmark. They cannot be shared,
so the public benchmark is `tests/fixtures/`: three or four synthetic mini-repositories forming
an A → B → C chain, built from patterns seen in the dogfood set (a `post(path)` helper wrapping
`fetch`, paths held in constants, a base URL read from the environment, a gateway prefix). CI
runs on them, so anyone can reproduce the results.

The contracts of the dogfood set form a star around one provider. A real A → B → C chain there
crosses a declared shared resource. That is one reason the fixtures exist.

Acceptance:

- The fixtures reach 100% of the expected links at the expected levels, checked against a golden
  file.
- On the dogfood repositories, v0.1 links the five consumer calls of one repository and the
  three of another. A call whose URL comes from a runtime value is dynamic, so it waits for the
  decisions of track A.
- On the 633-file TypeScript repository measured in `docs/prior-art.md`:

  | Metric | Target |
  | --- | --- |
  | Cold index time | ≤ 7 s |
  | Cold index size | ≤ 37 MB |
  | Single-file reindex | ≤ 1 s |
  | Tool list | ≤ 1k tokens |
  | Default answer | ≤ 2k tokens |

  The baseline is the measurement in `docs/prior-art.md` (CodeGraph: 7.4 s and 37 MB), plus
  about 6k tokens per CodeGraph answer, measured the same day.

## Considered options

- **1.0 freezes the API and languages arrive in 1.x.** Semver allows additive minors, so this
  would work. The maintainer chose a feature-complete 1.0 instead.
- **One sequential queue of milestones.** Slower, and no parallelism between differentiators and
  languages.
- **Breadth-first languages.** The differentiators would arrive late.
- **Announcing at v0.1.** A thin first impression.
- **Announcing at 1.0.** Too long without outside users.

## Consequences

- 1.0 is far from the first release. Until then the 0.x series carries no stability promise.
- Tracks A, B and C all build on the seams of the v0.1 trunk, so a change to a seam after the
  trunk touches every track.
