# Receipt: index, export and MCP answers (search, context, trace) over corpus entry `meada`, CLI + MCP stdio + MCP HTTP
Verdict: BROKEN
Checker: fresh sub-agent
Scope: flow "index, export and MCP answers over meada" (no diff read, as instructed)
Changed files: n/a for a flow
Surfaces driven: CLI (index, export, refresh), MCP stdio, MCP HTTP (--http)
Instance: base bin meada-base (392e09c, default features, prebuilt, not rebuilt); head bin meada-head 1.1.0 (dev profile, working tree at 392e09c, uncommitted: 1 path)
## What the change touches
- index/export/MCP answers over `meada` -> existing, copies nothing -> base binary (rung 1)
- MCP tool list -> existing -> tests/golden/mcp.tools.json + base tools/list (rung 1)
- MCP over HTTP -> copies stdio -> byte parity (rung 1)
## Steps
1. [PASS] Determinism: cold index twice and refresh equal, base and head (compare.md line 9)
2. [PASS] Budgets: index 45.7s -> 43.3s, RSS 236700 -> 236036 KiB, export 28672668 -> 29136567 bytes (x1.01)
3. [FAIL] Export vs base (existing): calls ~1629 changed (candidates None -> [N items] appears on many; 762 new or changed resolutions), providers -4, communities +52/-45, processes +25/-29. Providers lost: http::GET::/admin/dashboard, /more/extras, /status, /tools/audit (tests/fixtures/conformance-rust/crates/actix/src/admin.rs). No issue or spec predicts any of it; the existing flow differs from the base, so BROKEN.
4. [FAIL] Sampled resolutions: `shape.source.at()` at meada-core/src/context.rs:139 went exact -> inferred (a level lost). Several newly inferred name matches look wrong: `hits.push(..)` (a Vec) in engine.rs:667, `facts.imports.push`, `parts.push`, `paths.push` all now "inferred" to trace.rs Walk.push; `node.kind()` (swift/manifest.rs:125) now inferred to http/rust.rs Web.kind. trace from Walk.push now shows a self cycle, chain none -> inferred.
5. [FAIL] MCP stdio: 7/11 answers byte-equal to base; context/trace of Web.kind and Walk.push differ (see compare.md), e.g. trace Web.kind subtree rewritten, Code.files gained "[inferred]".
6. [PASS, weak] MCP tool list: compare.md prints no tool-list difference (no positive "same" line for stdio; HTTP says "tools same"); not diffed against tests/golden/mcp.tools.json by hand
7. [PASS] MCP HTTP vs stdio (head): tools same, 11/11 answers byte-equal
## Probes
- [PASS] search budget 1 -> same cut message on both binaries (probes.txt)
- [PASS] search kind=nonsense -> same isError message on both
- [PASS] trace from unknown symbol; context with no symbol; trace direction=sideways -> equal on both
- [PASS] cold start (no .meada/): done by the run's cold-twice index, deterministic
## Retries and fixes
- none; head built in 46s (dev profile, so index took ~44s instead of the 7-9s release timing in the skill)
## Not covered
- other corpus entries, console, deploy -> out of the named flow
- Confirmation of the lost providers and exact->inferred cause in source -> no spec/issue given and diff reading was off limits -> maintainer/issue needed to approve or fix
## Left behind
- $RUN/work (corpus copies), $RUN/http, probe.sh, probes.txt; also an older run folder .prove-it/runs/20261103T044303Z-audit-baseline/work was already there. Not cleaned.
