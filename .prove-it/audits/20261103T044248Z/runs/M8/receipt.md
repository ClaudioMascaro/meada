# Receipt: index, export and MCP answers (search, context, trace) over corpus entry meada, CLI + MCP stdio + MCP HTTP
Verdict: BROKEN
Checker: fresh sub-agent
Scope: flow "index, export and MCP answers over meada" (head = working tree on 392e09c, base binary 392e09c)
Changed files: n/a (flow named; diff not read per instructions)
Surfaces driven: CLI (index, export), MCP stdio, MCP HTTP (--http)
Instance: .prove-it/runs/20261103T052309Z-audit-M8/bin/meada-head (1.1.0, debug build, 1 uncommitted path) vs meada-base (392e09c)
## What the change touches
- index/export over meada -> existing, copies nothing -> base binary (rung 1)
- MCP search/context/trace -> existing -> base binary, tools.json
- MCP HTTP -> copies stdio -> byte parity (rung 1)
## Steps
1. [PASS] index+export determinism: cold twice and refresh equal on both; export identical to base (28672668 bytes) (compare.md)
2. [PASS] budgets: RSS 236120->236772 KiB, time 45.2->46.8 s (debug, x1.25 ok), export size equal
3. [FAIL] MCP stdio answers vs base: 6/11 byte-equal. 5 differ (search "kind", context Web.kind, trace Web.kind, context Walk.push, context lang/mod.rs::text), all at default 2000-token budget. Head cuts the output one line earlier than base (e.g. "cut after 97 of 104 lines" vs "98 of 104"); trace footer 1982 -> 1944 tokens. No issue predicts it. (compare.md)
4. [PASS] tools/list unchanged; HTTP: tools same, 11/11 byte-equal to stdio
## Probes
- [PASS] budget 1, unknown kind "zzbogus", budget 100000 for search and context: 4/4 byte-equal base vs head, HTTP equal stdio (probe/compare.md)
- Reading: with a large budget the full answers are equal, so the content is the same; only the budget accounting/truncation point moved (head counts tokens slightly higher per line or the cut rule changed).
## Retries and fixes
- none
## Not covered
- other corpus entries, console, deploy: not in the named flow
- debug build only: time budget measured on debug binaries
## Left behind
- none (prove.py clean run)
