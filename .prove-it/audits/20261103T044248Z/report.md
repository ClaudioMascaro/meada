# Prove It audit: meada, 2026-11-03
Target: flow "index, export and MCP answers over the `meada` entry, CLI + MCP stdio + MCP HTTP" at 392e09c75333aefd73d1e13fb6bdfa005376b496 (tree equals main; the Compose surface was audited by the maintainer's session)
Score: 4/5 caught (2 by the scripts alone), 0 false alarms
Timings: head build 99 s (dev profile, cold target dir; 11 s incremental), base build 54 s (dependencies reused), one prove.py run --repos meada --http 343 s (baseline) and 332 s (final); each blind check took 7 to 11 minutes end to end (4 vCPU, 15 GB, debug builds)

Checker for every bug: a fresh sub-agent (general-purpose, sonnet), given only the brief's prompt. Plants were checked with `cargo check` before each blind run; all compiled as written (M1 and M2 add one unused-field warning).

| # | Flow | Layer | Planted bug | Result | Caught by | Evidence |
|---|---|---|---|---|---|---|
| M5 | index, export, context/trace | Rust language pack (`src/lang/rust.rs`) | typed `let` receivers lose their class (`Class::Unknown`) | caught (BROKEN) | scripts | export diff: calls `level: exact -> inferred: 290`, `exact -> ambiguous: 344`; sampled `shape.source.at()` at `meada-core/src/context.rs:139` fell exact -> inferred; `hits.push` on a Vec now inferred to `Walk.push`; 4/11 MCP answers differ (`runs/M5/compare.md`) |
| M1 | MCP search | MCP argument plumbing (`src/mcp.rs`) | `search` drops `kind` | caught (BROKEN) | checker's probe | main run "checks passed", 11/11 equal; probe `search {"query":"kind","kind":"zz_nokind"}`: base refuses (`kind` takes ...), head answers 99 lines of symbols (`runs/M1/probe/compare.md`) |
| M2 | MCP context | MCP argument plumbing (`src/mcp.rs`) | `context` drops `budget` | caught (BROKEN) | checker's probe | main run "checks passed", 11/11 equal; probe `context Walk.push budget: 1`: base "cut after 0 of 414 lines (budget 1 tokens)", head the full default-budget answer (`runs/M2/probe/compare.md`) |
| M8 | every answer that hits its budget | shared renderer (`meada-core/src/render.rs`, `cut`) | one line too few shown, note says N-1 | caught (BROKEN) | scripts | 5/11 stdio answers differ; `search "kind"` "cut after 98 of 104" -> "cut after 97 of 104"; `trace Web.kind` 1982 -> 1944 tokens (`runs/M8/compare.md`) |
| M7 | refresh (export and MCP after an edit) | index freshness (`src/index.rs`, `refresh`) | mtime shortcut keys on size only: a same-size in-place edit is never re-indexed | missed (PROVED) | none | main run "checks passed"; checker's five probes were all query-shaped; nothing edited a file. Confirmed after the run that the bug is real: on a two-function crate, renaming `alpha` to `gamma` in place (same 47 bytes) left the head's export with `alpha` and the base's with `gamma` (`runs/M7/receipt.md`) |

## Misses
### M7: a same-size in-place edit is never re-indexed
Why it slipped through: the only freshness check in `prove.py run` (`drive`, "refresh") touches the mtime of the first tracked file without changing its bytes and asks that the export stay equal. That check can only catch a refresh that changes output when nothing changed; it cannot catch a refresh that fails to change output when something did. Both cold indexes delete `.meada/` first, so every other export is cold and the refresh path is never asked to pick up new content. The skill's "Try to break it" list for this flow names query probes (unknown symbol, `budget: 1`, unknown `kind`) and a cold start, but no edit-then-requery probe, so the checker never edited a file.
Fix to the skill: in `drive`, after the mtime touch, add a content-refresh check: rewrite one tracked source file in place with a same-length change that alters a symbol (for example swap one identifier for another of the same length, or flip a character inside a function name), keep its size, set a new mtime, run `meada export` (warm), then remove `.meada/`, run `meada index` and `meada export` (cold), and require the warm and cold exports to be byte-equal (record `content_refresh_equal` next to `refresh_equal` on the Determinism line). Add the same as a named probe under "Try to break it": "edit a file in place without changing its size, then `export` and one MCP query must show the new code".
Fix to the answer key: none needed; the cold index of the edited tree is the answer key (ADR 0002 determinism, and the refresh path promises the cold result).

## Near misses (caught only by the checker's own probe)
M1 and M2 passed the scripted run (11/11 equal on stdio and HTTP) and were caught only because the checker chose a matching probe. Different checkers picked different probes: the M7 and M8 checkers asked `budget: 1` of `search` and `trace` and an unknown `kind`, but not `budget` on `context`; a checker that skips `context` with a budget would miss M2. Suggested script changes (not applied, the maintainer decides):
- `auto_queries`: add `context` of the top symbol with a small budget (`budget: 300`), and `search` of the top symbol's short name with `budget: 300` and with `kind: contract`; add an unknown-kind error-parity query (`search {"query": <top name>, "kind": "zz_prove_kind"}`) beside `zz_prove_missing`.
- Why the existing automatic `kind: contract` query did not catch M1: its text is a contract id (`channel::amqp`) that matches no symbol name, so a search without `kind` returns exactly the same contract rows (`src/engine.rs`, `search`: with no kind both symbols and contracts are searched). A kind query only discriminates when its text matches something of another kind.

## Skill drift found by the baseline
None. The first baseline run on the clean tree came back "checks passed" with determinism true on both sides (cold twice and refresh), every budget within its limit (index 47.4 s -> 47.7 s, RSS 236088 -> 236128 KiB, export 28672668 bytes both), export identical, 11/11 stdio answers byte-equal and 11/11 HTTP answers byte-equal to stdio. No `scripts/` file was changed. `prove.py selftest` passed.

Smaller gap noticed while reading `prove.py` (not exercised by a plant): the export's `imports` section (39 rows on `meada`) is in neither `KEYED` nor `OTHER`, so `export_diff` never compares it; a change that only moves `imports` would read as "identical". Fix: add `"imports"` to `OTHER`.

## False alarms
None. The final run on the clean tree after the last revert (`audit-final`, head rebuilt from the clean tree) came back "checks passed" with the same determinism, budgets, identical export and 11/11 on both transports.

## What the automatic queries covered
From `queries/meada.jsonl` (picked by the head's first export): `search`, `context` and `trace` of `src/http/rust.rs::Web.kind`, `src/trace.rs::Walk.push` and `src/lang/mod.rs::text` (searches `kind`, `push`, `text`); `search {"query": "channel::amqp", "kind": "contract"}` (the self export has 619 `providers`, so a contract query was asked); and `context zz_prove_missing` (error parity).
- Budget cuts: `search "kind"` and the three `context` answers end with "... cut after" lines (98 of 104, 83 of 685, 79 of 414, 117 of 433). No `trace` answer is cut: `trace Web.kind` ends at 1982 tokens, under the cut, and both other traces are "chain: none, 0 hops" (two lines each), so two of the three traces exercise almost nothing.
- No query passes `budget`, `direction` or `changes`, and the only `kind` used matches nothing but contracts; argument plumbing in `src/mcp.rs` is therefore covered only by whatever the checker probes by hand (see Near misses).
- No query or check follows an edit to the indexed tree (see M7).

What the score means: five planted bugs show the misses this run found, not that the verifier catches everything. The scripts reliably catch changes to resolution and rendering that the export or the default-budget answers show; argument handling and the warm refresh path depend on the checker's own probes, and the warm refresh path had none.
