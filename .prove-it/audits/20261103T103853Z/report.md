# Prove It audit re-check: meada, 2026-11-03
Target: flow "index, export and MCP answers over the `meada` entry, CLI + MCP stdio + MCP HTTP" at 392e09c75333aefd73d1e13fb6bdfa005376b496, with the skill fixes of 2026-11-03 applied (uncommitted patch of .claude/skills/prove-meada/)
Score: 2/3 caught (2 by the scripts alone), 0 false alarms (the clean tree fails the new content-refresh check on both sides, but that is a product finding, see below)
Timings: head build 132 s (cold target dir; 14 s incremental), base build 68 s, one prove.py run --repos meada --http 484 s (was 343 s before the fixes; the final clean run took 498 s)

| # | Flow | Layer | Planted bug | Before the fixes | Now | Caught by | Evidence (compare.md lines) |
|---|---|---|---|---|---|---|---|
| M7 | index, export, MCP over `meada` | `src/index.rs`, `refresh` walk | mtime shortcut reduced to `k.size == size`: a same-size edit is never re-indexed | missed | caught by luck (BROKEN, but for the clean-tree finding; the checker's evidence says the opposite of the symptom) | nothing: the scripts print the clean tree's line byte for byte | `Determinism: ... content refresh equal base False, head False (edit meada-core/src/authorize.rs: Change -> ChangQ); restore equal base False, head False  NOT DETERMINISTIC` (identical to the clean tree; no `STALE`) |
| M1 | same | `src/mcp.rs`, `SearchArgs::ask` | `kind` dropped: `engine.search(&self.query, None, budget)` | caught by the checker's own probe | caught | the scripts | `MCP search {"query": "kind", "kind": "contract"}: differs` (base `no matches for \`kind\``, head 99 symbol lines); `MCP search {"query": "zz_prove_missing", "kind": "zz_prove_kind"}: differs` (base `TOOL ERROR \`kind\` takes ...`, head `no matches`); `MCP stdio: 14/16 answers byte-equal` |
| M2 | same | `src/mcp.rs`, `ContextArgs::ask` | `budget` dropped: `engine.context(&self.symbol, None)` | caught by the checker's own probe | caught | the scripts | `MCP context {"symbol": "...src/http/rust.rs::Web.kind", "budget": 300}: differs` (base `… cut after 13 of 685 lines (budget 300 tokens)`, head the full answer); `MCP stdio: 15/16 answers byte-equal` |

## Still missed
### M7: a same-size in-place edit is never re-indexed
What the run actually recorded (head, planted): the warm export after the rename still holds `meada-core/src/authorize.rs::Change` (no `ChangQ`), so the edit never reached the index. After the cold re-index of the edited tree and the restore, the warm export holds `ChangQ` while the file says `Change` again. Both are the planted symptom, twice.

Why it still slipped through:
1. `STALE` is `warm == cold1` on raw bytes, and the export carries two fields that come from the checkout, not the index: `commit` and `dirty` (`src/engine.rs`, `checkout_commit`). Under M7 the warm export has `commit: None, dirty: None` (the freshness check `index::fresh` still compares mtime and size and says "not fresh", so the commit is left unknown), while `export1.json` has `commit: 7cd0d27e..., dirty: false`. The bodies are equal, the bytes are not, so `content_refresh_stale` is False and the word `STALE` never prints. On the clean tree `dirty` alone flips to `true` in the warm export, so the same comparison would also be defeated by `dirty` whenever an index is stale but fresh.
2. The clean tree already fails `content refresh equal` and `restore equal` on both sides (finding below). M7 turns both head values from False to False, so the Determinism line, the `differences found: 2` count and the exit code are identical to a clean run. Nothing in `compare.md` separates the planted head from the base.
3. The blind checker then read the head's `export4.json` against `export5.json` and reported "differ only in the header ... hashes and ids otherwise equal", and "the edit reached the index". The same pair differs by `symbols: +1 -1` (`Change` vs `ChangQ`) and `files: +1 -1`. It read a header and stopped; the skill asks it to diff with `export_diff`, it did not.
4. The MCP queries run on the restored tree, and none of the 16 touches `Change`, so the stale `ChangQ` in the head's index never shows in an answer.

Fix to the skill:
- In `drive`, compare exports with `commit` and `dirty` removed for `content_refresh_stale`, `content_refresh_equal` and `restore_equal` (they are checkout facts read at export time, ADR 0004, #512), and add a direct check on the edit itself: the renamed id (`<path>::<name[:-1]>Q`) must be in the warm export's `symbols` and the original id must be back after the restore. Checked offline on this run's saved exports: with the two fields stripped, `stale` is True only for the M7 head (`20261103T105144Z-recheck-M7 head stale True | new name in warm False | new name after restore True`) and False for both sides of the clean baseline and the M7 base.
- In `cmd_run`, when a Determinism check fails, print the `export_diff` summary of `export4` vs `export5` and of `export1` vs `export6` for each side (a few lines, `cap=4`) into `compare.md`, so the checker reads the diff the script already knows instead of diffing 28 MB by hand, and a head-only difference shows next to a failure both sides share.
- In SKILL.md, state that a Determinism failure present on base and head alike is pre-existing and does not end the check: the checker must still compare the head's diff with the base's and report a difference between them.
- Add one MCP query on the edited name (`search` of `<name[:-1]>Q`) run between the warm export and the cold re-index, so the stale index also shows through MCP.

## Found on the clean tree by the new check
Main at 392e09c7 still does what the maintainer's installed `meada 0.1.0` did, on both the base and the head binary, and the restore makes it persist:
- Edit chosen: `meada-core/src/authorize.rs: Change -> ChangQ`.
- Warm export (refresh path) vs cold index of the edited tree, both sides: `calls: ~3`, three Swift fixture calls in `tests/fixtures/conformance-swift-app` resolve to `exact` warm but not cold: `Page+Show.swift::Page.build()::Item#0` (`exact -> None`, `target: set -> None`), `Page+Show.swift::Page.show(_:)::item.label#0` and `Scopes.swift::Shelf.stock(_:)::item.label#0` (`exact -> inferred`); `communities: +1 -1`.
- Original vs warm: the expected `symbols: +1 -1` (`Change` to `ChangQ`), `files: +1 -1`, `dirty: False -> True`, plus the same three calls going to `exact` and the same community move.
- Original vs restored (`export1` vs `export6`), both sides: the three calls stay `exact` and the community stays moved after the file is restored, so the warm index never comes back to the cold answer. This is why `restore equal` is False on the clean tree.
- The mtime-only refresh (`refresh equal`) stays True: the file is hashed and skipped, nothing is re-parsed. Whatever re-parses a file through the warm path resolves those Swift calls differently from a cold index of the same tree. Full output: `runs/baseline/clean-diff.txt`.
- Consequence for the skill: until this is fixed, every run on `meada` ends with `differences found: 2` and `NOT DETERMINISTIC`, and every honest checker reports BROKEN, which is what hid M7 (above).

## False alarms of the fix
None. No query errors on both sides, no `EDIT CHANGED NOTHING`; the final clean run (`runs/final/compare.md`) is line for line the baseline's apart from timings. No change to `scripts/prove.py` was needed, so there is no `scripts-fix.patch`.

## What the Determinism line and the new queries showed on the clean tree
- Determinism: `cold twice equal base True, head True; refresh equal base True, head True; content refresh equal base False, head False (edit meada-core/src/authorize.rs: Change -> ChangQ); restore equal base False, head False  NOT DETERMINISTIC`, no `STALE`, the product finding above.
- The extra work costs about 80 s per side: a warm export after the edit (19 s), a third cold index (41 s), a warm export after the restore (20 s).
- MCP: 16 queries (3 symbols `Web.kind`, `Walk.push`, `lang::text` through 3 tools, 4 argument queries on `Web.kind`, `search channel::amqp kind: contract`, 2 error-parity probes), 16/16 byte-equal base vs head and HTTP vs stdio.
- The argument queries change the answers, so they can catch a dropped argument: `search kind budget: 300` 1166 bytes vs 7991 at the default budget; `context Web.kind budget: 300` 1159 vs 7984 (cut after 13 of 685 lines); `trace up budget: 300` 1127 vs 7920; `search kind kind: contract` gives `no matches for \`kind\``; an unknown `kind` gives `TOOL ERROR \`kind\` takes \`symbol\`, \`contract\`, ...`.
- Side observation by the M7 checker, not verified here: `meada export | head -c` panics with a backtrace on a broken pipe (`src/main.rs:1177`).
