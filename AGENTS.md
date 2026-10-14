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
ADRs and specs. GitNexus source, its npm `dist/` and its tuned
constants stay out of this repo and out of your reading.

## Invariants

The product invariants (no LLM or network in indexing; local-first, with a model only in the
opt-in `meada decide` command; writes only inside its own index; a compact MCP server with no
write tools; one index per repository; determinism) live in
`docs/adr/0002-product-invariants.md`. A change that breaks one starts with a new ADR.

## Workflow

- Conventional Commits.
- Gate before every commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`.
- `.githooks/` runs part of the gate locally: `cargo fmt --check` and no em dash in staged
  Markdown on commit; on push, a check against the maintainer's private-term list. Enable it
  once per clone with `git config core.hooksPath .githooks`.
- Prose separates clauses with commas, colons or parentheses rather than em dashes.
- Decisions are recorded as ADRs in `docs/adr/`.
- Domain terms are defined in `GLOSSARY.md`. Use them in code, docs and issues.
- Repository map in `docs/agents/architecture.md`: read it before exploring or changing code.

## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues on `ClaudioMascaro/meada`. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Vendored skills

The `mattpocock/skills` set lives in `.agents/skills/` (linked from `.claude/skills/`), pinned by
`.agents/skills-lock.json`: a skill whose `tree` differs from `upstreamTree` carries a local
adjustment. After staging an edit to a skill, set its `tree` to
`git rev-parse $(git write-tree):.agents/skills/<name>`.
