# Receipt: index, export and MCP answers (search, context, trace) over corpus entry `meada`, via CLI, MCP stdio, MCP HTTP
Verdict: BROKEN
Checker: fresh sub-agent
Scope: flow "index, export and MCP answers over meada" (existing flow, no diff read)
Changed files: n/a
Surfaces driven: CLI (index/export), MCP stdio, MCP HTTP (--http)
Instance: .prove-it/runs/20261103T110241Z-recheck-M1/bin/meada-head (1.1.0, working tree on 392e09c, uncommitted 5 paths, debug build) vs meada-base (392e09c, prebuilt)
## What the change touches
- index/export/MCP answers -> existing, copies nothing -> base binary (rung 1), tools.json, stdio/HTTP parity
## Steps
1. [PASS] export base vs head: identical (28672668 bytes both); cold index twice equal both sides.
2. [FAIL] MCP search {"query":"kind","kind":"contract"}: base "no matches for `kind`", head returns 99 symbol lines (kind filter dropped).
3. [FAIL] MCP search {"query":"zz_prove_missing","kind":"zz_prove_kind"}: base TOOL ERROR "`kind` takes ... not `zz_prove_kind`", head "no matches" (unknown kind accepted).
4. [PASS] 14/16 other stdio answers byte-equal to base; tool list same.
5. [PASS] HTTP: tools same, 16/16 byte-equal to stdio.
6. [FAIL] Determinism line: content refresh equal and restore equal False on BASE and HEAD alike (export4 vs export5 differ at the same byte, line 578014). Not a head regression; refresh/edit check in this environment fails on both sides, unexplained.
7. Budgets: time 40.2s -> 42.5s, RSS 236896 -> 236232 KiB, export size equal; all within limits (debug build).
## Probes
- [FAIL] hand-run `search {"query":"kind","kind":"bogus"}`: base error, head returns results (kind ignored).
- [PASS] `search budget:1`: both "cut after 0 of 104 lines", equal.
- [PASS] `trace from:zz_missing`: equal. `trace direction:sideways`: both error identically.
- Cold start: indexes ran from cold copies under work/, no network.
## Retries and fixes
- none; build.sh head OK (warning: 1 dead_code).
## Not covered
- other corpus entries, console, deploy: not asked.
- content-refresh failure on base: cause not investigated.
## Left behind
- run folder work/ copies (run `prove.py clean`), no zz-prove leftovers found.
