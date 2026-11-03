# Receipt: index, export, MCP search/context/trace over corpus entry meada (CLI, stdio, HTTP)
Verdict: BROKEN
Checker: fresh sub-agent
Scope: flow "index, export and MCP answers over meada"
Changed files: n/a (flow)
Surfaces driven: CLI (index/export), MCP stdio, MCP HTTP
Instance: bin/meada-head (meada 1.1.0, debug build of working tree, uncommitted 5 paths), bin/meada-base (commit 392e09c, prebuilt)
## What the change touches
- index/export/MCP answers -> existing, copies nothing -> base binary (rung 1)
- tool list -> existing -> tests/golden/mcp.tools.json + base tools/list (tools same)
- MCP over HTTP -> copies stdio -> byte parity
## Steps
1. [PASS] export base vs head: identical (28672668 bytes both); budgets: index 41.6s->42.1s, RSS 236320->236872 KiB.
2. [PASS] tool list unchanged; HTTP: 16/16 answers byte-equal to stdio.
3. [FAIL] MCP context with budget 300 (Web.kind): base cut after 13 of 685 lines; head returns all ~685 lines. Cause: src/mcp.rs ContextArgs::ask calls engine.context(&self.symbol, None), dropping budget (SearchArgs passes it). 15/16 stdio answers equal.
4. [FAIL] Determinism: "content refresh equal" and "restore equal" False on BOTH base and head. Warm export after in-place edit (export4) differs from cold index of edited tree (export5) at line 578014: target Item (swift fixture call) warm vs null cold; restore likewise reversed (export1 null vs export6 Item). Same on base, so pre-existing refresh nondeterminism, not a head regression, but the skill rules False = BROKEN.
## Probes
- [PASS] search budget:1 -> identical on both ("cut after 0 of 104 lines").
- [FAIL] context budget -> head ignores it (step 3).
- unknown kind / missing symbol: in automatic queries, equal.
## Retries and fixes
- none
## Not covered
- trace `changes`, other corpus entries, release build timings (debug build), console.
## Left behind
- work/ copies and http indexes under this run folder (prove.py clean not run)
