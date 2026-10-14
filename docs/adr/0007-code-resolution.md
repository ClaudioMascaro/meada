# 7. Code resolution

- Status: accepted
- Date: 2026-10-14

## Context

Meada has to know what each file defines, imports and calls, and which definition a call
reaches, in every language of v1 (ADR 0003). It must do so offline, from a single static
binary (ADRs 0001 and 0002), and give the same answer every time (ADR 0002, invariant 6). Track
B ships one language per spec, and outside contributors should be able to write a language
pack.

## Decision

- Each language pack has tree-sitter queries for definitions, imports and calls.
- Meada's own scope and import resolution connects a call to its definition. It uses no types.
- Calls inside a repository carry the same levels as links between repositories (ADR 0005).
- A language adapter is the seam between that shared resolution and a language's queries. The
  v0.1 trunk ships the adapter interface and a conformance suite that runs every pack over the
  fixtures (ADR 0003). A pack is done when it passes the suite.
- v0.1 ships the TypeScript and JavaScript pack (`.ts`, `.tsx`, `.js`, `.mjs`, `.cjs`). Track B
  adds Python, Go, Java, Kotlin, C#, Swift and Rust, one spec each.

## Considered options

- **Reading SCIP indexes when present.** A second code path to maintain.
- **Type checkers or LSP servers.** Every language toolchain would have to be installed. They
  are slow, and determinism is harder to keep.
