# Receipt: index, export and MCP answers (search, context, trace) over corpus entry meada, CLI + MCP stdio + MCP HTTP
Verdict: BROKEN
Checker: fresh sub-agent
Scope: flow "index, export and MCP answers over meada" (diff not read, per brief)
Changed files: n/a
Surfaces driven: CLI, MCP stdio, MCP HTTP
Instance: bin/meada-base (392e09c, 1.1.0), bin/meada-head (working tree on 392e09c, 1 uncommitted path, 1.1.0, default features, debug build)
## What the change touches
- index/export/MCP over meada -> existing -> answer key base binary (rung 1), tool list golden tests/golden/mcp.tools.json, HTTP == stdio (rung 1)
## Steps
1. [PASS] existing: prove.py run --repos meada --http -> determinism all true, export identical, 11/11 stdio answers byte-equal to base, HTTP tools same and 11/11 byte-equal to stdio; budgets: index 46.59s -> 48.97s, RSS 237592 -> 237404 KiB, export 28672668 -> 28672668 B (compare.md)
2. [PASS] tools/list of head equals tests/golden/mcp.tools.json (compared as sets by name)
## Probes
- [FAIL] search with unknown kind `zz_nokind` -> base: TOOL ERROR "`kind` takes `symbol`, `contract`, ... not `zz_nokind`"; head: returns a normal 99-line symbol result, kind silently ignored. Existing flow differs from base (probe/compare.md line 16). Same over HTTP (HTTP matched stdio).
- [PASS] search budget 1, trace budget 1, context budget 1, trace direction `sideways` on unknown symbol, unknown tool name -> base and head byte-equal
- [PASS] cold start (no .meada, no meada.toml): index exit 0, export valid JSON, nothing written outside repo's .meada
## Retries and fixes
- python golden compare first failed: golden is a list, not an object -> my script; fixed
- the first probe did not alter the run folder's main results
## Not covered
- other corpus entries; console; deploy -> not asked
- debug build timings only (budget check weaker than release)
## Left behind
- none (prove.py clean run on both folders); probe/ and cold/ evidence kept
