# Receipt: index, export and MCP answers (search, context, trace) over corpus entry meada, CLI + MCP stdio + MCP HTTP
Verdict: BROKEN
Checker: fresh sub-agent
Scope: flow "index, export and MCP answers over meada" (existing; uncommitted work in tree, 5 paths, not inspected)
Changed files: n/a
Surfaces driven: CLI, MCP stdio, MCP HTTP (--http)
Instance: bin/meada-head (meada 1.1.0, working tree at 392e09c, uncommitted 5 paths, default features) vs bin/meada-base (392e09c, prebuilt, not rebuilt)
## What the change touches
- index/export/MCP over meada -> existing, copies nothing -> base binary (rung 1), tools/list golden, HTTP==stdio parity
## Steps
1. [PASS] export base vs head (existing): identical, 28672668 bytes both.
2. [PASS] budgets: index 42.998s -> 41.11s, RSS 236968 -> 236652 KiB, export size equal.
3. [PASS] MCP stdio: 16/16 answers byte-equal base vs head; tool list same.
4. [PASS] MCP HTTP: tools same, 16/16 answers byte-equal to stdio.
5. [FAIL] Determinism: cold twice equal and mtime refresh equal (both True); "content refresh equal" False and "restore equal" False on BOTH base and head (edit meada-core/src/authorize.rs: Change -> ChangQ). compare.md says NOT DETERMINISTIC. Not STALE (edit reached the index).
   - head: export4 (warm) vs export5 (cold) differ only in the header: warm export omits "commit" and "dirty" that cold has; hashes/ids otherwise equal. Restored warm export6 also lacks commit/dirty, so != export1 (dirty:false).
   - base: export4 vs export5 differ in content (swift fixture call at line 578014: target set/level exact warm vs null cold, two exact vs inferred levels, a community member line), i.e. the base refresh path keeps different resolutions than a cold index.
   Evidence: head/meada/export{1,4,5,6}.json, base/meada/export{1,4,5,6}.json, record.json (content_refresh_equal False, content_refresh_stale False).
## Probes
- Not run beyond the automatic ones (budget 300, kind contract, direction up, unknown kind, missing symbol: all in the 16 queries, byte-equal). No --overlay, cold-start or sibling-pack probe was run.
- Side observation: `meada export | head -c` panics on broken pipe (stdout write) with a backtrace; seen on head.
## Retries and fixes
- none; single run.
## Not covered
- Other corpus entries (only meada requested); console; deploy; overlay/cold-start probes; the 2026-07-28 MCP era.
## Left behind
- run folder work/ copies (head/base/http); `prove.py clean` not run.
