# Meada

LLM-free code graph for agents and people: indexes repositories with tree-sitter, links them
through the contracts they share, answers over MCP. Rust, Apache-2.0.

## Language

- Code, identifiers, comments, commit messages, ADRs, specs and maintainer-written issues and
  PRs are in English.
- Prose for people (README, CONTRIBUTING, community docs) is written in Brazilian Portuguese
  first; `*.en.md` and `*.es.md` are its translations, in that order of priority. A change to a
  pt-BR doc updates both translations in the same PR, or opens an issue labelled `translation`
  for each one left behind.
- Contributors may write issues, PRs and reviews in pt-BR, Spanish or English; reply in the
  language they used.

## Clean room

Meada covers ground that GitNexus (PolyForm Noncommercial) also covers. Build from Meada's own
ADRs and specs and from `docs/prior-art.md`. GitNexus source, its npm `dist/` and its tuned
constants stay out of this repo and out of your reading.

## Invariants

The product invariants (no LLM or network in indexing, local-first, writes only inside its own
index, read-only and compact MCP) live in `docs/adr/0002-product-invariants.md`. A change that
breaks one starts with a new ADR.

## Workflow

- Conventional Commits.
- Gate before every commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`.
- Decisions are recorded as ADRs in `docs/adr/`.
