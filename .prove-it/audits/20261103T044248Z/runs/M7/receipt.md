# Receipt: index, export and MCP answers (search, context, trace) over corpus entry meada, CLI, MCP stdio, MCP HTTP
Verdict: PROVED
Checker: fresh sub-agent
Scope: flow "index, export and MCP answers over meada" (no diff read)
Changed files: n/a
Surfaces driven: CLI (index, export), MCP stdio, MCP HTTP
Instance: bin/meada-base (392e09c, debug build, not rebuilt), bin/meada-head (1.1.0, this working tree, uncommitted: 1 path), no ports kept
## What the change touches
- index/export/MCP over meada -> existing, copies nothing -> base binary on same input (rung 1)
- tool list -> existing -> tests/golden/mcp.tools.json (rung 1/2)
- MCP HTTP -> copies stdio -> byte parity (rung 1)
## Steps
1. [PASS] index+export, cold twice and refresh (existing): determinism true both sides; export identical base vs head (28672668 bytes)
2. [PASS] budgets: index 46.2s -> 44.0s, RSS 236552 -> 236580 KiB, export size equal (compare.md)
3. [PASS] MCP stdio 11/11 answers byte-equal base vs head
4. [PASS] MCP HTTP 11/11 byte-equal to stdio, tools same
5. [PASS] head tools/list equals tests/golden/mcp.tools.json (3 tools)
## Probes
- [PASS] search budget 1, search kind zz_nokind, trace budget 1, trace direction sideways, context empty symbol -> prove.py run in probe/: 5/5 equal base/head and stdio/HTTP; errors well formed (isError true, lists valid kinds / directions)
- [PASS] cold start: prove.py run starts from removed .meada/ with no meada.toml; no network used
## Retries and fixes
- none
## Not covered
- other corpus entries, console, deploy -> out of the named flow -> prove.py run --repos all
- debug builds, so timings are not release timings
- probe answers file shows only 3 of 5 lines in my hand re-run (truncated by my tail/cut); parity run covered all 5
## Left behind
- run folder only; work copies cleaned
