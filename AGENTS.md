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
- Local gate before every commit: `scripts/check` selects checks from the staged diff or, on a
  clean tree, the last commit. Docs, agent instructions and harness
  configuration get pertinent static checks and harness self-checks, without rebuilding the
  product, running its full suite or producing a `.prove-it` receipt. Unknown or product paths
  conservatively run the full applicable gate: fmt, clippy and tests, plus the relevant cloud
  feature checks (ADR 0012). Heavy commands run through `bounded` when it is on PATH.
- Product E2E runs only in GitHub Actions, for a relevant large implementation marked with the
  `e2e` label on a non-draft PR. Locally, `/prove-meada` prepares the scope and answer keys or
  reads CI evidence; it does not build or start the product. Docs-only follow-up commits also
  skip E2E. Existing artifacts retain their original SHA; pending evidence stays pending.
- For E2E, the PR Evidence section identifies the workflow run, tested SHA and artifact or
  receipt. Capture is not judgment: `CAPTURED` is never `PROVED`, and a relevant surface is not
  done on a `BROKEN` verdict. Without the required CI evidence, report what remains pending.
  For docs, instructions or harness-only validation, report E2E `N/A` in the ordinary validation
  summary, without creating a `.prove-it` receipt.
- Mutation audits are explicit manual GitHub Actions `workflow_dispatch` runs, never an
  automatic task-completion step or a consequence of editing a skill.
- For product integration handoff, also run `scripts/clippy-features` locally; it reads the CI
  feature matrix (ADR 0012). Documentation and harness-only follow-ups do not repeat it.
- GitHub Actions owns the full integration, UI and deployment checks. CI for the exact PR
  head must be green before marking the PR ready for review or merging into main.
- `.githooks/` runs part of the gate locally: `cargo fmt --check` and no em dash in staged
  Markdown on commit; on push, a check against the maintainer's private-term list. Enable it
  once per clone with `git config core.hooksPath .githooks`.
- Prose separates clauses with commas, colons or parentheses rather than em dashes.
- Decisions are recorded as ADRs in `docs/adr/`.
- Domain terms are defined in `GLOSSARY.md`. Use them in code, docs and issues.
- Repository routing index in `docs/agents/architecture.md`: read it before exploring or changing
  code, then follow its area index and read the topics relevant to the change. A split index is
  navigation, not a request to read every leaf. Read the relevant extension and invariant details
  it names when changing a seam. The limits of the code are in `docs/agents/ceilings.md`: follow
  it to the relevant limit topic when a limit is touched. A
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
