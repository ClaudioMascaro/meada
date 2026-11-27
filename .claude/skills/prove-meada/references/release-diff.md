# The release diff (#852, ADR 0029)

Before a tag, the links, orphan lines and calls rows of the candidate are compared with those of the
previous tag over this skill's pinned public corpora, and the cost of both sides is measured. This
file is the public half; the private half is a line of the maintainer's dogfooding pass (#809).

## Where it runs

Workflow `Release diff` (`.github/workflows/release-diff.yml`), job `release-diff`, on a pull
request whose title starts with `chore(release): ` (the release notes' checklist names it so).
Every other pull request skips the job without a runner. It runs on `opened`, `reopened`,
`synchronize` and `ready_for_review`, drafts included: a pull request retitled after it opened runs
it on its next push.

1. The previous tag is the highest `vX.Y.Z` tag below the version in `Cargo.toml`; its release
   binary (`meada-x86_64-unknown-linux-gnu.tar.xz`, checked against its `.sha256`) is the base.
2. The candidate is `cargo build --locked --profile dist` at the pull request's head, the profile
   and default features of the release workflow.
3. `$S/release-diff.py run` takes each group of entries as one workspace: `corpus.tsv`
   (`code`), the A7 rows of `infra.tsv` (`infra`) and its CI rows (`ci`, A9's corpus). The probes
   are this skill's own inputs, not corpora: they enter only when `--repos` names them.

## What it leaves

| Artifact | Holds |
|---|---|
| `meada-candidate-<head SHA>` | the candidate `meada` and `meada.from` (commit, version, build command): the binary the release's smoke slices run |
| `release-diff-<PR number>-<head SHA>` | `summary.md` (also the job summary), `diff.json` (every changed link and orphan line), `calls.json` (every changed call row), and per side each workspace's manifest, exports, `status.txt`, stderr and `capture.json` (version; per workspace index seconds, peak RSS KiB, index bytes and each export's bytes) |

## How it compares

- A link is the same link when `prove.py`'s link key is equal: consumer, provider, contract and
  root. A link of two repositories of a workspace shows in both exports and counts once.
- Link changes: **new**, **vanished**, **raised** and **dropped**, by the order of meada-core's
  `Level` (exact, declared, inferred, decided, ambiguous; a name it does not know is weakest). A
  change of `reason` alone at the same level is no change.
- An orphan line (`meada status` in English) is the same line when its repository, contract and
  place are equal. Changes: **appeared**, **vanished**, and **level moved** for a `no provisioner`
  line whose level changed.
- The summary counts per pair (the contract kind, the first segment of the contract id, and the
  statement type) and per level; a level move shows as `old -> new`.

`release-diff.py --selftest` needs no Meada: equal sides give no diff, one planted change of each
type lands in its count, and the calls sample and the cost marks are checked on planted data. `scripts/check` runs it whenever the script changes.

## Calls

The `calls` rows of each entry's export, one entry at a time. A row is the same row when its `id`
is equal; `level`, `target` and `candidates` are watched, so a change of `narrowed`, `decorates`,
`holder` or `span` alone is no change. A row can show several of: **new**, **vanished**, **raised**,
**dropped** (no level is the weakest, `unresolved`), **retargeted** (same level, other target),
**candidates removed**, **candidates gained**, and **certain lost** (`exact` or `declared` before,
and now absent, weaker or on another target). The summary counts them per entry and per use kind
(`call` when the row has none) and level.

It then samples at most 20 rows of each of four groups, pooled over all entries, with seed 852, so
the same captures give the same sample: **new exact**, **raised to exact**, **lost a candidate**
(without the raises to `exact`, judged in their own group, and without counting a candidate that
became the row's target) and **certain lost**. Each line gives the
source of the call site and of its target; `?` where the target is in another repository, or when
the diff runs again without the work copies. `--sample N` changes the 20.

## Cost

Per workspace, base against candidate: index seconds, peak RSS of the `index` and `export`
commands, bytes of `indexes/`, and bytes of the exports. ` over` marks a value past `prove.py`'s
budgets (time x1.25 where the base took 2 s or more, RSS x1.25, export x1.50, also listed per
entry); index bytes have none. An artifact without these fields shows `-`. One run per side, base
first, on one runner: a single sample, and a warning, not a failure.

## Reading it

Calls and cost are reported and judged by sample; they hold nothing, even a `wrong` row or a value
over budget, which the maintainer decides case by case. A comparison that fails leaves `BLOCKED:` in
its section and the job still passes. A change of the `id` scheme between versions shows as a mass
of vanished plus new rows of the same sites.

For links and orphan lines, the counts are not a verdict. The release pull request's Evidence names the run, and ADR 0029 asks
for an explanation per group of changes of one cause, tied to a changelog entry or an issue,
written by an agent that did not make the changes, and a verdict on each vanished link and dropped
level against the corpus code: right before (a regression, which holds the tag) or wrong before (a
fix). A change nothing explains holds the tag.

That agent's brief, calls and cost included, is [release-diff-judge.md](release-diff-judge.md).

To diff a run again, or two captures made by hand, lay them out as `run` does
(`<dir>/{base,head}/<workspace>/{<entry>.export.json,status.txt}`) and run
`python3 $S/release-diff.py diff --run <dir>`.
