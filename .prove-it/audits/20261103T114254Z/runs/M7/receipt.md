# Receipt: index, export and MCP answers (search, context, trace) over corpus entry meada, CLI + MCP stdio + MCP HTTP
Verdict: BROKEN
Checker: fresh sub-agent
Scope: flow "index, export and MCP answers over meada" (no diff read, as instructed)
Changed files: n/a for a flow
Surfaces driven: CLI (index, export), MCP stdio, MCP HTTP (--http)
Instance: head meada 1.1.0 (working tree at aaded2f, uncommitted 5 paths, default features, debug build); base meada-base (aaded2f, prebuilt)
## What the change touches
- index/export/MCP answers over meada -> existing -> answer key: base binary (rung 1), tools.json, stdio/HTTP parity
## Steps
1. [PASS] cold index twice: exports byte-equal, base and head
2. [PASS] mtime-touch refresh equals cold, base and head
3. [FAIL] content refresh (in-place same-size rename meada-core/src/authorize.rs Change -> ChangQ): head warm export lacks ChangQ (STALE: edit never reached the index), original symbol not back after restore. Base: renamed symbol present in warm export (True), original back True. Only base's known swift-fixture refresh diff (3 calls, communities) appears on base. Evidence: compare.md Determinism line and export_diff blocks.
4. [PASS] export base vs head: identical (28672345 bytes both)
5. [PASS] budgets: index 44.05s -> 42.49s, RSS 237240 -> 236944 KiB
6. [PASS] MCP stdio base vs head: 16/16 answers byte-equal; tool list same
7. [PASS] MCP HTTP vs stdio (head): tools same, 16/16 byte-equal
## Probes
- [FAIL] in-place same-size edit then warm export: head returns stale index (see step 3); this is the M7 symptom from the skill
## Retries and fixes
- none; run.out/compare.md are the raw outputs
## Not covered
- other corpus entries; console; deploy (not named)
- tests/golden/mcp.tools.json compared via prove.py's tool-list check ("tools same"), not separately
## Left behind
- work copies cleaned with prove.py clean; evidence kept in the run folder
