# CLI and MCP: the detail behind `prove.py run`

These product execution commands are CI-only recipes under `AGENTS.md`. Locally, prepare
inputs or read the matching run artifact; do not build or start the product.


## What one run does

For each corpus entry (`scripts/corpus.tsv`), with the head binary first and then the base:

1. A copy of the entry under `$RUN/work/<side>/<entry>`: `self` is `git archive HEAD` of this
   checkout, committed with fixed dates under the origin `github.com/ClaudioMascaro/meada`; the
   others are shallow fetches of the pinned sha, cached in `~/.cache/prove-meada/corpus/`
   (`PROVE_MEADA_CACHE` moves it).
2. `meada index` twice from cold (`.meada/` removed), each followed by `meada export`: wall time
   and peak RSS of each, and whether the two exports are byte-equal (determinism, ADR 0002).
3. An mtime touch of the first tracked file and `meada export` again: the refresh path must give
   the cold export's bytes. Then one symbol's name is rewritten in place with the same size (the
   first symbol by id whose name is an identifier of 3 or more characters, its last character
   replaced by `Q`), `meada export` warm, `.meada/` removed, `meada index` and `meada export`
   cold: the warm and cold exports must be equal (ignoring `commit` and `dirty`, checkout facts
   absent while an index is not fresh), differ from the first one, and the warm one must contain
   the renamed id. The file is restored and `meada export` warm must give the first export again,
   with the original id back; the queries below run on that restored tree. When one of these
   fails, `compare.md` prints the `export_diff` of warm against cold and of the first export
   against the restored one, per side.
4. `meada mcp` over stdio, `initialize` (2025-11-25), `tools/list`, then every query of
   `$RUN/queries/<entry>.jsonl`.

Then `compare.md`, per entry: the budget lines, the export diff per section (`golden_diff`'s keys:
symbols and calls by id, links by consumer, provider and contract), up to `--sample` (10) new or
changed resolutions with the line of the call site and of the target, the tool list, and a
unified diff of every MCP answer that differs.

Exit 0 means no difference at all, 1 differences or budget overruns, 2 BLOCKED.

The stdio client flushes each request, saves every sent and received JSON message beside its
stderr log as `*.wire.jsonl`, and bounds each response wait at 120 seconds. A timeout is
BLOCKED with the last request visible in that transcript, never an empty successful answer.
The drive checkpoints its measured index/export results before MCP and saves each completed
answer as it arrives.

After an interrupted run, retain every partial capture and audit the exact frozen input,
binary contents, queries and options before any run-local recovery. There is no generic
resume flag. A manually audited recovery may reuse completed measured phases, but its script,
identity audit and previous attempts stay with the receipt. Replaying a partial MCP phase
archives its old stderr, wire transcript and partial answers under `mcp-attempts/` first.
Record the interruption and every retry in the receipt.

## Queries

Without `--queries`, the head's first export picks them: the three symbols with the most
resolved callers, each through `search` (its short name), `context` and `trace`; on the first of
them, every tool argument once (`search` with `budget: 300` and with `kind: contract`, `context`
with `budget: 300`, `trace` with `direction: up` and `budget: 300`); one `search` of the first
provided contract with `kind: contract`; and the error parity probes, `search` with
`kind: zz_prove_kind` and `context` of `zz_prove_missing`. The base reuses the same file.
`trace`'s `changes` has no automatic query: pass one with `--queries` when Scope reaches it.

For a change about a named symbol, route or channel, write the queries before the run, one JSON
object per line, and pass the file with `--queries`:

```bash
cat >"$RUN/queries.jsonl" <<'EOF'
{"tool": "context", "arguments": {"symbol": "github.com/cashapp/turbine::src/commonMain/kotlin/app/cash/turbine/flow.kt::app.cash.turbine.Flow.test(Duration?,String?,TurbineTestContext<T>.()->Unit)"}}
{"tool": "trace", "arguments": {"from": "github.com/cashapp/turbine::src/commonMain/kotlin/app/cash/turbine/flow.kt::app.cash.turbine.Flow.test(Duration?,String?,TurbineTestContext<T>.()->Unit)", "direction": "up", "budget": 4000}}
{"tool": "search", "arguments": {"query": "http::ANY::/", "kind": "contract"}}
EOF
```

The same file then reaches every entry of the run: name the entries one run at a time when the
queries belong to one of them. Parameters are the tools' own (`tests/golden/mcp.tools.json`):
`search` takes `query`, `kind`, `budget`; `context` takes `symbol`, `budget`; `trace` takes
`from`, `direction`, `changes`, `budget`.

## Reading the export diff

- `calls ... level: None -> exact` or `ambiguous -> exact`: a new resolution. Confirm each sampled
  one: the site line must call the target line's definition, in that language's rules, for every
  receiver the site can have. A trait or interface method chosen for a `dyn` or interface receiver
  passes when the docs at the base promise it (the capabilities page, `GLOSSARY.md` "Family") or
  the issue does; otherwise it is BROKEN.
- `target: set -> None` or `exact -> ambiguous`: a resolution lost. Existing flow: BROKEN unless
  the issue predicts it.
- `communities` and `processes` move with any call change (spec #31): they need no line of the
  issue of their own when the calls that moved them are predicted.
- `format`, `commit`, `dirty` differences between two releases come from issue #512 (PR #585);
  within one release they must not move.

## Release binaries instead of builds

Only when the base is a release tag and this machine must not build. The head is then the
release whose tag is HEAD, or a build of the head elsewhere:

```bash
for v in v1.0.0 v1.1.0; do  # the base's tag, then the head's
  d="$RUN/dl-$v"; mkdir -p "$d"
  gh release download "$v" -R ClaudioMascaro/meada -p 'meada-x86_64-unknown-linux-gnu.tar.xz*' -D "$d"
  (cd "$d" && sha256sum -c meada-x86_64-unknown-linux-gnu.tar.xz.sha256 && tar -xJf meada-x86_64-unknown-linux-gnu.tar.xz)
done
mkdir -p "$RUN/bin"
cp "$RUN/dl-v1.0.0/meada-x86_64-unknown-linux-gnu/meada" "$RUN/bin/meada-base"
cp "$RUN/dl-v1.1.0/meada-x86_64-unknown-linux-gnu/meada" "$RUN/bin/meada-head"
echo "release v1.0.0 (commit $(git rev-parse 'v1.0.0^{commit}')), sha256-checked, features: default" >"$RUN/bin/meada-base.from"
echo "release v1.1.0 (commit $(git rev-parse 'v1.1.0^{commit}')), sha256-checked, features: default" >"$RUN/bin/meada-head.from"
```

## Timings measured on 2026-11-02 (release builds, 7 GB machine)

| Entry | Index | Peak RSS | Export |
|---|---|---|---|
| meada | 7 to 9 s | 205 MB | 28 MB |
| click, pflag, petclinic, turbine, serilog, swiftlog | under 1 s each | 30 to 45 MB | 1 to 10 MB |

The time budget applies only from 2 s of base time, so it bites on `meada` alone; RSS and export
size apply to every entry.

## By hand

The scripts only wrap these; use them to look closer at one answer:

```bash
cd "$RUN/work/head/meada"
"$RUN/bin/meada-head" status
"$RUN/bin/meada-head" export | head -c 2000
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"x","version":"1"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search","arguments":{"query":"Engine"}}}' \
  | "$RUN/bin/meada-head" mcp | tail -1 | head -c 1500
```
