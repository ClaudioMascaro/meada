# Prove It audit re-check: meada, 2026-11-03
Target: flow "index, export and MCP answers over the `meada` entry, CLI + MCP stdio + MCP HTTP" at aaded2f4d0c26c13a9aa4886c231b207e3a9612d (main, one commit past 392e09c7: #604, tests and `src/resolve.rs`), with the skill fixes of 2026-11-03 applied (uncommitted patch of .claude/skills/prove-meada/)
Score: 1/1 caught (1 by the scripts alone), 0 false alarms
Timings: head build 105 s, base build 49 s, one prove.py run --repos meada --http 534 s (545 s on the final clean run; 343 s before the fixes, 484 s after the first round)

| # | Flow | Layer | Planted bug | First audit | First re-check | Now | Caught by | Evidence (compare.md lines: planted run vs baseline) |
|---|---|---|---|---|---|---|---|---|
| M7 | index, export, MCP over `meada` | `src/index.rs`, `refresh` walk, mtime shortcut | `k.mtime.is_some() && k.mtime == mtime && k.size == size` reduced to `k.size == size`: a same-size in-place edit is never re-indexed | missed | missed | caught (BROKEN, blind sonnet checker) | the scripts (`prove.py run` Determinism line and diff blocks) | planted: `renamed symbol in the warm export base True, head False`; `original symbol back base True, head False`; `STALE: the edit never reached the index (base False, head True)`; head diff blocks name `meada-core/src/authorize.rs::ChangQ` / `::Change` instead of the Swift calls. Baseline: `renamed ... base True, head True`, `original symbol back base True, head True`, no `STALE`, head diff blocks equal to the base's |

## Still missed
None. M7 is now separated from the pre-existing failure by the scripts alone: three independent signals (`renamed ... head False`, `original symbol back ... head False`, `STALE ... head True`) flip on the head only, and the head's diff blocks differ from the base's.

Observations worth keeping:
- The planted head's `content refresh equal` and `restore equal` are `False` as on the clean tree, so those two flags alone would not have separated it; the direct flags and `STALE` did. Their being shared with the pre-existing failure is why the first re-check could not score it.
- The `head original vs restored` block shows `ChangQ` still present after the restore: the cold re-index of the edited tree stored `ChangQ`, and the restore (same size again) is skipped by the planted shortcut. The bug is visible in both directions.
- The head's stale warm export lacks `commit` and `dirty` (`commit: None -> 0f5a2bed...`, `dirty: None -> True`), confirming why the raw-bytes comparison of the first re-check never matched; `body()` stripping them is what lets `STALE` fire.
- `Export, base -> head: identical` and `MCP stdio: 16/16 answers byte-equal` stay green under M7: the cross-binary export compares cold indexes, and no automatic query names `Change`. Only the Determinism line carries this bug, so a checker who reads only the Export and MCP lines would still miss it.

## The clean tree's Determinism line and diff blocks (the pre-existing failure)
On aaded2f4 the failure is byte-identical to the one the brief predicted for 392e09c7 (`#604` did not change it):
```
differences found: 2
Determinism: cold twice equal base True, head True; refresh equal base True, head True; content refresh equal base False, head False (edit meada-core/src/authorize.rs: Change -> ChangQ); renamed symbol in the warm export base True, head True; restore equal base False, head False; original symbol back base True, head True  NOT DETERMINISTIC
  base warm vs cold (edited tree):
    calls: +0 -0 ~3
      level: exact -> None: 1
        ...conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
      level: exact -> inferred: 2
        ...conformance-swift-app/Sources/Page+Show.swift::Page.show(_:)::item.label#0
        ...conformance-swift-app/Sources/Scopes.swift::Shelf.stock(_:)::item.label#0
      target: set -> None: 1
        ...conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
    communities: +1 -1
  base original vs restored: the same three calls and `communities: +1 -1`, reversed (None/inferred -> exact)
  head warm vs cold (edited tree): identical to the base block
  head original vs restored: identical to the base block
```
Full text in `runs/baseline/compare.md` (queries in `runs/baseline/meada.jsonl`). The skill's Known and deliberate entry names 392e09c7; it still holds at aaded2f4.

## False alarms of the fix
None. On the clean tree (baseline and final runs): no `STALE`, no `EDIT CHANGED NOTHING`, `renamed ...` and `original symbol back` `True` on both sides, no query erroring on both sides. The final run on the clean tree after the revert differs from the baseline only in index seconds and peak RSS (45.054 -> 43.895 s, 236536 -> 237008 KiB, within budget). No `scripts-fix.patch` was needed.

## What the Determinism line and the new queries showed on the clean tree
- The edit chosen: `meada-core/src/authorize.rs: Change -> ChangQ`, on both runs.
- Cold twice and the mtime touch are byte-equal on both sides; the content-refresh check fails only through the shared Swift fixture product bug above.
- The 16 automatic queries (3 symbols x 3 tools, the 4 argument probes `budget: 300`, `kind: contract`, `context budget: 300`, `trace direction: up budget: 300`, one contract search, the unknown kind and unknown symbol probes) are 16/16 byte-equal base vs head over stdio, and 16/16 HTTP vs stdio with the same tool list.
- Budget: index 45.18 -> 43.967 s, peak RSS 236724 -> 237068 KiB, export 28672345 bytes on both sides.

Process notes: the planted file was backed up before planting and the revert confirmed with `cmp` (exit 0) before anything else; the backup was then dropped from the audit folder since it equals `src/index.rs` at aaded2f4. `git status --porcelain` at the end equals the start (the four patched skill files).
