---
name: prove-meada-cli-mcp
description: "Meada proof flows for the CLI and the MCP server over stdio and HTTP, with the goldens. Read after /prove-meada."
disable-model-invocation: true
---

# Prove the CLI and the MCP server

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`.

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| Index, export and MCP answers over the corpus | existing | the base commit's binary on the same pinned input | 1 | `build.sh base "$BASE"`, then `prove.py run` | the code before the change |
| A golden the change re-blessed | existing | the golden at the base, and each changed line predicted by the issue | 2 + 4 | `prove.py rebless` | the base's tests; the maintainer's issue |
| A new or changed resolution in the export diff | new | the issue, and the call site and target in the indexed source | 4 | "Confirm in the source" in `compare.md` | the maintainer; the corpus authors |
| The MCP tool list | existing | the base binary's `tools/list` (frozen at 1.0 in `tests/golden/mcp.tools.json`) | 1 | `prove.py run` | the 1.0 surface |
| MCP over HTTP | copies stdio | the same binary's stdio answers, byte for byte | 1 | `prove.py run --http` | the stdio server |
| Index time, peak RSS, export size | existing | head at most x1.25, x1.25 and x1.5 of the base (time only when the base takes 2 s or more) | budget | `prove.py run` | the maintainer, 2026-11-02 |

## CLI and MCP over stdio (primary)

**CI drive**: `python3 $S/prove.py run --base-bin "$RUN/bin/meada-base" --head-bin "$RUN/bin/meada-head" --repos "$REPOS" --run "$RUN" [--http]`, and when Scope listed `tests/golden/`, `python3 $S/prove.py rebless --base "$BASE" --run "$RUN"`. Detail, queries of your own and timings: [references/cli-mcp.md](../prove-meada/references/cli-mcp.md).

`prove.py run` is BLOCKED by itself when an index or export fails or an export has no symbol, so an empty index never reads as "no difference".

**Copies**: nothing. Existing flows answer to the base binary; new resolutions to the issue and the source.

**Done looks like**: `compare.md` shows, per entry, the `Determinism` line all true, every budget within its limit, every export difference predicted by the issue, every sampled resolution confirmed in the source, the tool list unchanged, and every MCP answer equal or its difference predicted. `rebless.md` has each changed golden line matched to an issue line.

**Could fool you**:
- A difference in `compare.md` is not a failure and "identical" is not a pass for a new flow: a change that should resolve something and leaves the export identical did nothing. BROKEN.
- The corpus may not contain the construct the change handles. Then probe it with `--overlay` (Try to break it), and record that the corpus missed it under Not covered.
- `inferred` and `ambiguous` are name matches, and calls inside macros are not indexed: a missing caller proves nothing.
- An argument the server parses and drops (`kind`, `budget`, `direction`) leaves every default-budget answer identical. The automatic queries pass each argument once (`budget: 300`, `kind: contract` on a symbol name, `direction: up`, an unknown `kind`); a new argument needs a query of its own, or it is unproved (audit 2026-11-03, M1 and M2 passed the scripts).
- Two cold indexes and an mtime touch never ask the refresh path to pick up new content, so a refresh that keeps stale state passes them. `prove.py run` therefore edits one symbol's name in place (same size), requires the warm export to equal a cold index of the edited tree, then restores it: `content refresh equal` and `restore equal` on the Determinism line. `renamed symbol in the warm export` and `original symbol back` ask the direct question (is the renamed id in the warm export, is the original id back after the restore); the equalities ignore `commit` and `dirty`, checkout facts that go missing while an index is not fresh. A `False` there is BROKEN: with `STALE`, or `renamed ... False`, the edit never reached the index (the M7 symptom, audit 2026-11-03); without them the refresh path gave an answer the cold index does not, and `compare.md` prints the `export_diff` of warm against cold and of original against restored for each side. A failure that base and head share is pre-existing (see Known and deliberate) and does not end the check: compare the head's diff with the base's and report any line that differs between them. `EDIT CHANGED NOTHING` means the entry gave no indexed symbol to rename.

**Known and deliberate**: the self entry is this checkout's HEAD with fixed git dates, so its `commit` is the same for both binaries; base 1.0.0 against a later head shows `format`, `commit` and `dirty` appearing in every export (issue #512, PR #585). A base older than 1.2.0 still has #609 (a refresh resolved Swift calls through a type parameter that a cold index left unresolved, fixed in #637), so `content refresh equal` and `restore equal` may read `False` on the base side alone; on the head a `False` is BROKEN.

## MCP over HTTP (server mode)

**CI drive**: add `--http` to `prove.py run` whenever Scope reaches `src/mcp.rs` or server mode. Each entry is served alone (`meada mcp --listen --workspace`), asked the stdio queries again and compared byte for byte.

**Copies**: the stdio server of the same binary (read parity, error parity through the `zz_prove_missing` query).

**Done looks like**: "tools same, N/N answers byte-equal to stdio" for every entry.

**Could fool you**: one manifest of several repositories is one workspace, so `search` spans all of them; the script serves each alone for that reason. Keep it that way.

## Verifying a bug fix

The base binary of `prove.py run` is the unchanged code: its export line or MCP answer is the "before" value, so put the probe in `--queries` or name the export line.

## Try to break it

A probe of source code goes through both binaries with `--overlay DIR`: the files under `DIR` are copied over the entry and committed before either side indexes, so give the probe its own run folder (`--run "$RUN/probe"`). Wire the file in as its language needs: Rust indexes a file only when a `mod` reaches it, so overlay the entry's `src/lib.rs` with `mod zz_prove;` appended.

- the changed construct combined with the others it can meet (`?`, `.await`, chains, aliases, overloads) in one file, every resolution confirmed in the source (#593);

- a query the change should refuse or cut: an unknown symbol (already in the automatic queries), `budget: 1`, a `kind` the tool does not know;

- the same construct in the sibling pack (TypeScript and JavaScript, Java and Kotlin), or the same route through the other HTTP client the pack reads;

- a cold start: no `.meada/`, no `meada.toml`, no network (indexing never reaches it, ADR 0002).

- an edit in place that keeps the file's size, then `export` and one MCP query must show the new code: the scripts do one such edit per entry (the Determinism line); do another on the construct you changed when Scope reaches `src/index.rs`.

## Gotchas

- Linux carries a forked child's pre-exec peak into its `ru_maxrss`, so the script measures through a small fresh interpreter. Do not "simplify" that away: every entry would report the script's own size.

- `examples/golden_diff` panics without `origin/main` (#592); `prove.py rebless --base` takes the base explicitly and needs no build.

- `meada decide` has no scripted check: its replay is `tests/it/decisions.rs`'s. Put it under Not covered when Scope reaches `src/decide.rs` or `src/decider/`.
- Keep the corpus time and memory budgets unchanged. The whole corpus took 98 s with release builds on 2026-11-02; debug builds index several times slower.
