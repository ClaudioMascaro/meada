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
  `cargo test`; for a change under a cloud module's feature `<f>` (ADR 0012), also
  `cargo clippy --all-targets --features <f> -- -D warnings` and `cargo test --features <f>` (CI's
  `clippy-features` matrix runs the clippy half for every feature alone and for the pairs the code
  gates on).
- Before a change is called done, `/prove-meada` runs on it and the PR body's Evidence section
  names its receipt (`.prove-it/runs/`, kept out of git); a change whose diff reaches a surface of
  the skill's Scope table is not done on a BROKEN verdict.
- `.githooks/` runs part of the gate locally: `cargo fmt --check` and no em dash in staged
  Markdown on commit; on push, a check against the maintainer's private-term list. Enable it
  once per clone with `git config core.hooksPath .githooks`.
- Prose separates clauses with commas, colons or parentheses rather than em dashes.
- Decisions are recorded as ADRs in `docs/adr/`.
- Domain terms are defined in `GLOSSARY.md`. Use them in code, docs and issues.
- Repository map in `docs/agents/architecture.md`: read it before exploring or changing code, then
  the area file in `docs/agents/map/` that the map names for the area being changed. The
  limits of the code are in `docs/agents/ceilings.md`: read it when a limit is touched. A
  `docs/agents` file past its size budget (`tests/it/docs.rs` names it) is split in the same PR
  with `cargo run --example split_agents -- <file>`, never trimmed by dropping entries.
- Coding standards in `CODING_STANDARDS.md`: read them before writing or reviewing resolution, a
  contract kind or a language pack.

## Code graph

Meada indexes this repository too: `.mcp.json` serves it as the `meada` MCP server (`search`,
`context`, `trace`). Reach for it first to find where a symbol lives, who calls it and what a
change touches, then read the files it names.

- Install the binary once per machine with `cargo install --path . --locked`, and again after a
  change to a language pack or a contract kind: the server runs the installed binary, while the
  index follows the working tree on every query. After a kind change, also delete `.meada/`: a
  refresh keeps the rows of the language groups no file changed in while Meada's version holds.
- The server answers about the first MCP root the client sends (Claude Code sends the session's
  directory first), so a session in a worktree under `.claude/worktrees/` gets its own tree even
  though the desktop app starts the server in the main checkout. A client without roots gets the
  directory the server started in. Subagents share their parent's server and its roots (#192).
- Read its answers as Meada's own levels say: `exact` is certain, `inferred` and `ambiguous` are
  name matches, and a missing caller is unproven, since calls inside macros (`assert!`,
  `format!`, `vec!`) are not indexed.
- The index follows the default features (ADR 0012 keeps the cloud modules behind features) and
  covers each crate's module tree from `src/lib.rs` and `src/main.rs`. For code under `accounts`,
  `directory`, `github-app`, `oidc`, `postgres`, `proposals`, `snapshot`, `ui`, `valkey` or
  `workspaces`, and for anything under `tests/`, `examples/` or `benches/`, go straight to grep: an
  empty answer there is expected, not a stale index.
- Results under `tests/fixtures/` are test data.
- A wrong answer on Meada's own code is a bug: open an issue with the query, the answer and the
  expected one.

## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues on `ClaudioMascaro/meada`. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Vendored skills

The `mattpocock/skills` set and the prove-it trio of `ray-amjad/skills` live in `.agents/skills/`
(linked from `.claude/skills/`), pinned by `.agents/skills-lock.json`. Per skill the lock records
`path` in the upstream repository, `upstreamTree` and `tree`; the upstream is the skill's own
`upstream` (`repo`, `commit`) or, without one, the top-level `upstream`. A skill whose `tree`
differs from `upstreamTree` carries a local adjustment. After staging an edit to a skill, set its
`tree` to `git rev-parse $(git write-tree):.agents/skills/<name>`. The plugins that ship the same
skills stay disabled in `.claude/settings.json`, so a session lists each skill once.
