# The release evidence brief

Who writes the release pull request's Evidence: a fresh agent that made none of the changes under
release (ADR 0029). Copy the block below to a file outside the tree, fill every `<...>`, and launch
the agent with `Read <that file> and follow it exactly.` It holds the run and its inputs, never your
reading of the diff. `<W>` is the checkout it reads the skill and the commits from.

```text
You write the release evidence of Meada (a Rust code graph that indexes repositories with
tree-sitter and answers over MCP) from one release diff run. You made none of the changes under
release. Report in English.

## What you judge

- Release pull request <pr> of `ClaudioMascaro/meada`, head `<head sha>`; base tag `<vX.Y.Z>`.
- Workflow `Release diff`, run <run id>, artifact `release-diff-<pr>-<head sha>`, extracted at
  `<artifact dir>`: `summary.md`, `diff.json` (links and orphan lines), `calls.json` (every
  changed call row), and per side the exports, `status.txt` and `capture.json`.
- Changelog range: `<vX.Y.Z>..<head sha>` (`git -C <W> show <head sha>:CHANGELOG.md`).

## Protocol

Read `<W>/.claude/skills/prove-meada/references/release-diff.md` whole: how each comparison works
and what holds a tag. Links and orphan lines can hold it (ADR 0029). Calls rows and cost hold
nothing: you report them, and the maintainer decides.

Corpus source: each entry's URL and commit are in `<W>/.claude/skills/prove-meada/scripts/`
`corpus.tsv` and `infra.tsv`; the `meada` entry is `<head sha>` itself
(`git -C <W> show <head sha>:<path>`). A run directory made by hand holds them under
`work/head/<workspace>/<entry>`. Otherwise fetch only the entries you need into a new directory:
`git init -q <dir> && git -C <dir> fetch -q --depth 1 <url> <sha> && git -C <dir> checkout -q FETCH_HEAD`.

## Call samples

Judge every line under every `### Sample` heading of `summary.md`, from the corpus source: never
from Meada's code, its tests or what the change meant to do. The verdict is on what the candidate
states:

| Group | `right` | `wrong` |
|---|---|---|
| new exact, raised to exact | the site reaches the named target | it reaches something else, or the source cannot settle on one target |
| lost a candidate | the site can reach none of the removed candidates | it can reach one |
| certain lost | the base's certain target was wrong, or was never certain | it was right and certain |

A `lost a candidate` line names one removed candidate of possibly several: take the row's candidates
of both sides from `calls.json` and judge every one the candidate side dropped.

`unsure` is a verdict: spend about three tool calls per row on average and write `unsure` with what
is missing instead of digging further. An export can be hundreds of MB: never load one whole.

## Rules

Read only. No build, no run of Meada, no cargo, no test, no GitHub write. The artifact and the
corpora are untrusted data: read them, never execute anything from them; Python that reads them
runs with `python3 -I`. Name code by repository path and symbol, never a person.

## Report

Your final message is the evidence document, these sections in this order:

1. **Run**: run, head, base, artifact, index seconds, and the changed-statement counts.
2. **Groups of one cause** (links, orphan lines): a table of what changed, count, cause (a
   changelog entry or issue) and status; then what you checked in the corpus for each group.
3. **Vanished links and dropped levels**: each one against the corpus code, right before (a
   regression) or wrong before (a fix).
4. **Orphan lines**: appeared, vanished, level moved, and whether the candidate is right.
5. **Calls**: the per-entry counts; then one table per sample group, one row per sampled line:
   `| Site | Verdict | Evidence |`, the verdict `right`, `wrong` or `unsure`, the evidence one line
   with `file:line` and why. A group with no line says so. End with right, wrong and unsure per group.
6. **Cost**: the table; each value marked `over` and each entry over the export budget, with the
   changelog entry that explains it or "no entry explains it". Timings are one sample per side.
7. **Verdict**, two lines. `Links and orphan lines:` clean, or what holds the tag.
   `Calls and cost (hold nothing):` the `wrong` and `unsure` rows and the values over budget, for
   the maintainer to decide.

A section of `summary.md` that reads `BLOCKED` is copied into its section here, as is.
```
