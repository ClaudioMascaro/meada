# Receipt: index, export and MCP answers (search, context, trace) over corpus entry meada, CLI + MCP stdio + MCP HTTP
Verdict: BROKEN
Checker: fresh sub-agent
Scope: flow "index, export and MCP answers over meada" (existing flow, against base binary)
Changed files: n/a (flow)
Surfaces driven: CLI (index/export), MCP stdio, MCP HTTP
Instance: bin/meada-head (meada 1.1.0, working tree on 392e09c, uncommitted 1 path, dev build) vs bin/meada-base (392e09c)
## What the change touches
- index, export, search, context, trace -> existing, copies nothing -> base binary (rung 1); tool list tests/golden/mcp.tools.json; HTTP parity with stdio (rung 1)
## Steps
1. [PASS] index+export determinism (existing): cold twice and refresh equal, base and head (compare.md)
2. [PASS] export base vs head: identical, 28672668 bytes both; index 43.5s->43.4s, RSS 237112->237920 KiB (compare.md)
3. [PASS] MCP stdio 11/11 default queries byte-equal to base; tools same
4. [PASS] MCP HTTP: tools same, 11/11 byte-equal to stdio (head)
## Probes
- [FAIL] context with budget 1 on Walk.push -> base: "... cut after 0 of 414 lines (budget 1 tokens); raise budget for more"; head: full 80+ line answer, budget ignored (probe/compare.md). Cause seen by grep: src/mcp.rs ContextArgs::ask calls engine.context(&self.symbol, None), dropping self.budget (search and trace pass it).
- [PASS] search budget 1, search unknown kind, trace budget 1, trace direction sideways, trace changes on unknown, unknown tool, search with no args: base == head; HTTP == stdio 8/8
## Retries and fixes
- none (head built with build.sh; prove.py selftest passed)
## Not covered
- cold start without .meada/ beyond what prove.py does (it indexes from cold); other corpus entries, console, deploy -> out of the named flow
## Left behind
- run copies under work/ and http/ (not cleaned yet at write time)
