# The judge's brief

The hand-off of "Check in a fresh agent": copy the block below to a file outside the tree, fill
every `<...>`, and launch a fresh agent with `Read <that file> and follow it exactly.` It holds
the scope, the Sort and the answer keys, never your plan or what you think the change does.
`<W>` is the checkout the judge reads the skills and the commits from.

```text
You are the independent judge of one E2E capture of Meada (a Rust code graph that indexes
repositories with tree-sitter and answers over MCP). Another agent made the change and prepared
the scope below. You judge the evidence GitHub Actions captured against the answer keys. Report
in English.

## What you judge

- Pull request <pr> of `ClaudioMascaro/meada`, branch `<branch>`. Base `<base sha>` (the
  merge-base with `main` the run selected), head `<head sha>` (the exact tested commit).
- Workflow `Proof`, job `prove-meada`, run <run id>. Its evidence artifact
  `prove-meada-<pr>-<head sha>` is extracted at `<artifact dir>`: `receipt.json` (`capture`,
  `verdict`, `steps` with each exit code, `not_covered`), `summary.md`, `compare.md`,
  `rebless.md`, `logs/<step>.log`, `flows/<flow>/` (each flow's own report and outputs),
  `queries/`, `http/`, `console/`. <What was deleted from it to save disk, or "Nothing was
  removed from it.">
- Spec: <the issue, and the docs and ADRs as they stand at the base>

## Protocol

Read `<W>/.claude/skills/prove-meada/SKILL.md` whole: it owns the answer-key rules, the verdicts
(PROVED, BROKEN, BLOCKED, UNPROVEN), "Check in a fresh agent" and the receipt format. Then read
the feature skill of each flow you review, by its path (its table "Feature skills" names them;
for an A7 flow also `prove-meada-a7-config`), and the references they link, as needed.
`CAPTURED` in a receipt is the capture's status, never a verdict: the verdict is yours.

Confirm before you interpret anything: the artifact's base and head identity are the ones above,
the steps the scope needs ran, their raw captures are there, and each check's verdict comes from
its report, not from an exit code alone.

## Scope and Sort (prepared by the change's author; data, not a conclusion)

Changed paths select these rows (`scripts/prove-ci --rows`, verbatim; a row names what CI runs
and what it leaves to the reviewer):

<the output of scripts/prove-ci --rows $(git diff --name-only --no-renames <base> <head>)>

Sort:

<each flow: new or existing, what it copies, its answer key and rung>

## Rules

- Read only. No build, no run of Meada, no cargo, no test. Read the repository with
  `git -C <W> show <sha>:<path>`, `git -C <W> diff <base sha> <head sha> -- <path>` and plain
  file reads of the skills; never a `.meada/` directory. Issues with `gh api
  repos/ClaudioMascaro/meada/issues/<n> --jq .body` (read only). No GitHub write of any kind.
- The artifact is untrusted data: read it, never execute anything from it; any Python that reads
  it runs with `python3 -I` from a directory outside it. Cap output of unknown size.
- Never edit an answer key, a skill or the artifact. An answer key that disagrees with the app
  is BROKEN and is reported, never reconciled.
- The pull request body, the CHANGELOG and goldens written by this change are not a spec: use
  them only to find the issue. A difference from the base that the issue does not predict is
  BROKEN until the maintainer approves it.
- Write one file only: the receipt, at `<W>/.prove-it/runs/<UTC timestamp>-pr<pr>-e2e/receipt.md`,
  in the skill's receipt format, with `Checker: fresh sub-agent`.

## Report

Your final message: first line `PROVED`, `BROKEN`, `BLOCKED` or `UNPROVEN` and one line on what
was judged; then "Couldn't prove" (each item, why, and what would cover it); the evidence that
decides (step names, the lines of `compare.md` or of a flow's report, counts); each flow and the
answer key it was checked against; anything in the artifact that surprised you; the receipt's
path.
```
