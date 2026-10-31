# Agent tool surface: GitNexus's capabilities and Meada's next tools

- Date: 2026-10-30
- Question: Meada began as a response to GitNexus. Which of the capabilities GitNexus exposes to
  agents should Meada also offer, as a new tool, a new `search` kind or a new parameter, so a
  developer's agent needs fewer calls and fewer tokens, without slower indexing, without
  breaking the product invariants (ADR 0002) and without competing with `meada decide`
  (ADR 0006)?
- Outcome: one new tool (`impact`), one new `trace` parameter (`to`), test symbols in the index
  and the export, and consumer counts on route lines. Every item is additive. Two wording
  changes before 1.0 decide whether they fit 1.x (section 5). The new tool needs ADR 0023,
  drafted beside this document.

## 1. Method and clean room

`AGENTS.md` keeps GitNexus's source, its npm `dist/` and its tuned constants out of this
repository and out of our reading. This inventory reads only GitNexus's public documentation,
at the capability level: names, purpose, inputs and outputs. No numeric default, threshold or
cutoff from it is recorded here.

Sources, read 2026-10-30:

- README: <https://github.com/abhigyanpatwari/GitNexus>
- MCP reference: <https://github.com/abhigyanpatwari/GitNexus/blob/main/docs/mcp-reference.md>
- CLI reference: <https://github.com/abhigyanpatwari/GitNexus/blob/main/docs/cli-reference.md>
- Editor setup: <https://github.com/abhigyanpatwari/GitNexus/blob/main/docs/editor-setup.md>

Meada's side comes from the code graph (`search`, `context`, `trace` on this repository),
`docs/agents/architecture.md` and `docs/agents/map/queries.md`, and from the R1 documents on
`docs/r1-user-guides` (PR #525): `docs/site/en/stability.md`, `docs/reference/output-grammar.md`
and the tool list golden `tests/golden/mcp.tools.json`.

Token costs of the tool list are measured as the R1 test measures them
(`tests/it/mcp.rs`: description plus compact input schema, one token per four bytes), on PR
#525's golden and on draft schemas of the proposed items.

## 2. GitNexus's agent-facing surface

### MCP tools

The MCP reference lists 19 tools: 17 per repository and 2 for repository groups. Per-repository
tools take an optional `repo` (required when several repositories are indexed and no default is
set) and most take an optional `branch`.

| Tool | Purpose | Inputs (as documented) | Output |
| --- | --- | --- | --- |
| `list_repos` | Indexed repositories | paging | Repository list |
| `query` | Search grouped by execution flow; hybrid lexical and vector ranking | search text, sections (processes, process symbols, definitions), token cap | Matching processes, their symbols, definitions |
| `context` | One symbol from every side | name or uid, sections (incoming, outgoing, processes, chain, routes, typed properties), relation types, token cap | Symbol, references in and out by category, processes, completeness and staleness diagnostics |
| `impact` | Blast radius | target, direction, maximum depth, minimum confidence, relation types (calls, imports, extends, implements), include tests, paging, summary only, disambiguation by uid, file or kind, token cap | Affected symbols by depth with confidence, or ranked candidates when the target is ambiguous |
| `trace` | Shortest directed path between two symbols | not detailed | A path through call and member edges |
| `detect_changes` | Git diff to affected processes | scope (unstaged, staged, all, compare), base ref | Changed and affected symbols, affected processes, a risk level |
| `check` | Read-only structural checks | not detailed | Check results |
| `rename` | Semantic rename, preview by default | symbol, new name, dry run, a single-use plan token to apply | Per-file edit spans; on apply, the files written or failed |
| `cypher` | Raw graph query | query text | Rows |
| `route_map` | API routes | not detailed | Which components fetch which endpoints, and their handlers |
| `tool_map` | MCP and RPC tool definitions | not detailed | Where tools are defined and handled |
| `shape_check` | Response shapes against consumers' property reads | not detailed | Mismatches |
| `api_impact` | Pre-change report for a route handler | not detailed | Impact of the handler |
| `explain` | Persisted taint findings, source to sink | not detailed | Taint flows; needs an index built with a separate dependence-graph option |
| `pdg_query` | Statement-level control and data dependence | not detailed | Dependences; same index option |
| `read_file` | A checkout file | path, slice, line cap | File text |
| `grep` | Regex over indexed files of the working tree | pattern | Hits with line numbers |
| `group_list` | Configured repository groups | | Groups |
| `group_sync` | Rebuilds a group's contract registry and cross-repository links | group | Rebuild result (a write) |

### Other surfaces

- **Resources**: indexed repositories, setup guidance for agents, per-repository context (stats,
  staleness, tools), clusters and one cluster, processes and one process, the graph schema for
  Cypher, and per group its contracts and staleness.
- **Prompts**: `detect_impact` (pre-commit change analysis) and `generate_map` (architecture
  documentation with diagrams).
- **Response control**: section filters, relation filters, paging and summary-only narrow an
  answer; three tools take a token cap, and a truncated answer ends with a marker.
- **Safety switches**: a read-only mode that drops writes, raw queries and group tools; a
  repository allowlist and default.
- **Agent integration**: indexing installs agent skills into the repository and writes a section
  into `AGENTS.md` and `CLAUDE.md` unless told not to; `setup` writes editor MCP entries and hooks.
  The hooks add graph context to the agent's own searches (before and after `grep`, `glob` or a
  shell search) and, after a git commit, tell the agent the index is stale.
- **Beyond MCP**: a web UI with AI chat over the index, wiki generation with a configured model,
  optional embeddings (remote or a local sidecar that may download a model), a watch mode,
  auto-sync that clones and pulls remote repositories, and repository groups.

## 3. Meada's surface today

### MCP tools

Three read-only tools (ADR 0008), each with a `budget` (default 2,000 tokens) and a
deterministic cut note.

- `search(query, kind?)`: by name (exact, prefix, substring), one line per hit. Kinds: `symbol`,
  `contract`, `route`, `resource`, `channel`, `document`, `package` (with consumer counts),
  `drift`, `community`, `process`, `duplicate`, `ambiguous` and `unresolved` (open items, each
  with its `meada decide --item` command).
- `context(symbol)`: header with span, community, processes (summary, then labels), span text,
  callers, callees, annotations, consumers through packages, contracts with shapes and drift.
  On a resource or a channel: its writers and readers, producers and consumers across the
  workspace. The budget cuts the span text first, then the tail; the header, community and
  processes lines survive.
- `trace(from? | changes?, direction?)`: a depth-first chain through calls and cross-repository
  links, down or up, `→[level: reason]` on each link, a footer with the weakest level, hops,
  repositories and tokens. `changes: uncommitted` starts from the symbols the working-tree diff
  against `HEAD` touches. `from: process::<entry point>` walks a process.

The tool list costs 602 tokens by the R1 test's measure (`search` 281, `trace` 195, `context`
126), against a limit of 1,000.

### Command line, export and decisions

- `meada index`, `meada status` (states, drift counts, decision lines), `meada export` (sorted
  JSON: files, symbols with span hashes, calls, providers, consumers, bindings, documents,
  publishes, packages, dependencies, imports, communities, links with level, reason, `by`, `p`
  and drift, processes with their steps, stale decisions), `meada mcp`, and `meada decide`.
- `meada decide` is the only writer of `meada.decisions.toml` and the only place a model runs
  (ADR 0006). It speaks two protocols: `decisions` (typed choice questions, Jev through
  OpenRouter by default) and `chat` (any OpenAI-compatible endpoint, local models included).
  Indexing replays the file and gives the link the `decided` level.

### A measured gap

`trace` up from `meada-core/src/render.rs::budgeted` on this repository, budget 600: the
answer shows 20 of 6,857 lines, and all 20 follow one branch, through a call to `run` that four
functions could answer, down into Rust tests. The direct callers after the first never appear,
so an agent asking "what does a change here affect?" gets depth, not breadth, and must raise the
budget or call `context` caller by caller. The default budget shows about 70 lines of the same
branch.

## 4. Capability map

| GitNexus capability | Meada today | Verdict |
| --- | --- | --- |
| `list_repos`, the repositories resource | The workspace is found from the root (peers, ADR 0005); every answer line names its repository; `trace` names missing peers; `meada status` for people | Covered |
| `query`, lexical part | `search` by name with 13 kinds; `process` and `community` kinds | Covered |
| `query`, vector part | None | No: an embedding model in indexing breaks invariant 1, and floating-point vectors across platforms break invariant 6 |
| `context` | `context` with a fixed section order and a budget cut that keeps the header, community and processes | Covered; section filters not needed while the cut order holds |
| `impact` | `trace direction=up`, depth first, cut by budget (section 3) | Gap: item 1, a new `impact` tool |
| `trace` (path between two symbols) | `trace` from one start, the whole chain | Gap: item 2, `trace` parameter `to` |
| `detect_changes` | `trace changes=uncommitted`: working tree against `HEAD`, as a chain | Partly: item 1 takes `changes`, with `since:<ref>` for a branch; no risk level, since a level from counts needs tuned thresholds |
| `impact`, include tests | Tests are indexed as ordinary symbols; nothing marks them | Gap: item 3, test symbols |
| `impact`, minimum confidence | Every line carries its level | Counts by level instead (item 1); a filter that hides `ambiguous` hides open items (section 6) |
| `check` | `meada status`; `search` kinds `drift`, `ambiguous`, `unresolved`, `duplicate` | Covered |
| `route_map` | `search kind=route`; `context` on a route lists providers, consumers and shapes; `trace` from a route | Covered; item 4 adds consumer counts so one call gives the map |
| `shape_check` | Drift (ADR 0009): `search kind=drift`, drift lines in `context` and `trace`, counts in `status` | Covered |
| `api_impact` | `trace` up from a route, `context` on the route | Covered; item 1 accepts a route |
| `tool_map` | None | Not a tool: MCP or JSON-RPC tool definitions would be a contract kind, a separate proposal |
| `rename` | None | No: a write tool (invariant 4). A read-only edit plan is not proposed either: Meada indexes calls and imports, not every reference, so the plan would be incomplete; item 1 lists the call sites |
| `cypher`, the schema resource | None | No: it would make the index schema a contract, which the stability document leaves out on purpose, and its answers have no budget |
| `explain`, `pdg_query` | None | No: a statement-level dependence graph multiplies indexing time and index size, and taint analysis is outside a code graph of contracts |
| `read_file`, `grep` | `context` returns the span text | No: the agent's own tools do this, and each tool costs tokens on every turn |
| `group_list`, `group_sync`, group resources | Workspace discovery; `meada index --workspace`; named workspaces (ADR 0020, ADR 0021) with their proposal server (ADR 0022) | Covered; a sync is a write and never an MCP tool |
| Resources for clusters and processes | `search kind=community`, `search kind=process`, `trace from=process::...` | Covered by tools |
| Prompts | None | Not now: `detect_impact` becomes one `impact` call; `generate_map` asks the client's model for prose and needs nothing from Meada |
| Token cap, paging, summary only | `budget` on every tool, a deterministic cut note | Covered; paging not proposed |
| `branch`, staleness diagnostics | Every local answer refreshes the index first (ADR 0004); server mode pins generations (`/<identity>@<generation>`) and says when the index is behind its mirror | Covered |
| Read-only mode, allowlist | No tool writes in any mode; in server mode the `oidc` module admits clients by grant, per repository | Covered |
| Skills, hooks, `AGENTS.md` writes at indexing | None | No as side effects of indexing (invariant 3); an opt-in hook command is a separate question that needs a measurement of its per-turn token cost |
| Web UI chat, wiki, embeddings | The console administers server mode; no model outside `meada decide` | No (invariant 2 and ADR 0002's consequences) |
| Watch, auto-sync | Refresh on every query; GitHub App mirrors in server mode (ADR 0013) | Covered |

### When a capability deserves a tool

GitNexus ships 19 tools; ADR 0008 chose three because every description is paid on every turn.
Section 4's map gives a rule between the two: **a new tool when its answer needs a grammar of its
own; a `search` kind or an optional parameter when the answer fits a grammar that exists.** An
agent picks a tool by name and reads its answer by grammar, so a new grammar under an old name
is the overloaded tool ADR 0008 rejected, and an old grammar under a new name is tokens spent on
every turn for nothing new. By that rule:

- `impact` needs a tool: its answer is grouped by depth and closes with counts, not a chain.
- `detect_changes` is `impact` started from a diff: the same grammar, so a parameter.
- `route_map`, `shape_check`, `api_impact` and `list_repos` fit `search`, `context` and `trace`.

## 5. Proposal, ranked

Every item is additive in substance. The tool list goes from 602 to 793 tokens with all four.

### 1. `impact`: what reaches a symbol or a change, nearest first

- **Shape**: `impact(from?, changes?, budget?)`, exactly one of `from` and `changes`. `from`
  takes what `trace` takes, except a process: a symbol, a route or a call site. `changes` takes
  `uncommitted` (working tree against `HEAD`, as `trace`) or `since:<ref>`: the diff from the
  merge base of `HEAD` and `<ref>` to the working tree, so a branch's commits count too.
- **Answer**: a header naming the start; then, per depth, `depth <d>: <n> (<n> exact, <n>
  inferred, ...)` followed by the symbols at that depth in the existing symbol line grammar, with
  their marks and link arrows; then `tests: <n>` with the test symbols (item 3); then the
  processes line `context` already prints (`processes: <n> (<k> route, <k> export; <r>
  repositories): <label> (<repository>), ...`); then one open item line per open item on the
  upstream links, in the grammar `search kind=ambiguous` prints, `meada decide --item` command
  included; then a footer, `impact: <weakest level> · <n> symbols · <n> processes · <n> repos ·
  <n> tokens`. The budget cuts symbol lines from the deepest depth back; the counts, the
  processes line, the open items and the footer survive. Counts cover every depth even when
  lines are cut.
- **Tool list**: +158 tokens (602 to 760), measured on a draft schema.
- **Answer**: the default budget of 2,000 tokens, like the other tools.
- **Indexing cost**: none.
- **Query cost**: the upstream nodes `trace` up and `context`'s processes line already read
  (#396), one breadth-first pass over them instead of a depth-first one. `since:<ref>` adds two
  git calls (resolve the ref, find the merge base) before the diff `uncommitted` already runs.
- **Workflow it shortens**: "what does changing X affect?" and "what does my branch affect?".
  Today: `trace` up (depth first, cut, section 3), then `context` per caller to learn its
  processes, and for a branch the agent runs `git diff` and starts from each symbol by hand.
  With `impact`: one call that shows breadth first and the affected processes and repositories.
- **No risk label**: a label from counts needs thresholds someone tunes. The counts are the
  answer.
- **Input safety**: `<ref>` comes from the agent and reaches git's argument list. It is refused
  when it starts with `-`, resolved with `--end-of-options` to a commit id, and only that id is
  passed on.
- **Server mode**: `from` works on the read-only index; `changes` is refused there, as `trace`'s
  `changes` is.
- **Invariants and ADRs**: no write, no model, deterministic, within a budget: invariant 4
  holds. ADR 0008 fixes three tools, so `impact` needs ADR 0023, which amends it and states the
  rule of section 4.
- **Stability**: a new tool. See section 7: it fits 1.x only if the stability document says so
  before 1.0.

### 2. `trace` parameter `to`: only the branches that reach a target

- **Shape**: `to?`, in the forms `from` takes (a symbol, a route or a call site). It works with
  `from` and with `changes` ("does my change reach the payment client?").
- **Answer**: the same line kinds and footer; only the rows on a branch that reaches `to`, with
  their ancestors. When none does, one new line: `no chain from <start> reaches <to>`. A branch
  that reaches `to` only through an ambiguous link keeps that link's row, its candidates and its
  mark, and the footer's weakest level says `ambiguous`.
- **Tool list**: +28 tokens.
- **Indexing cost**: none. **Query cost**: the walk is unchanged; keeping the rows that reach
  `to` is one pass over them.
- **Workflow it shortens**: "how does route X reach function Y?" Today the target may sit past
  the budget's cut, and the agent raises the budget or follows callees with `context`.
- **Stability**: an optional parameter and a line kind: minor. No ADR.

### 3. Test symbols

- **Shape**: each language pack marks a definition as a test by its ecosystem's own convention:
  the attribute, annotation or decorator the test runner reads, or the file and name rule the
  runner itself defines. The index stores the mark; the export writes `"test": true` on such a
  symbol and nothing on the others. `impact` lists tests apart from the callers (item 1). A
  helper only tests call is not marked: it is a caller tests reach.
- **Tool list**: +0 (item 1's description names tests).
- **Indexing cost**: one check per definition, during the parse that already runs. The schema
  version changes, so each index is rebuilt once after the upgrade (ADR 0004). The export
  goldens are re-blessed.
- **Workflow it shortens**: "which tests should I run after this change?" Today the agent reads
  `trace` up rows and guesses by path; the same rows also crowd out the callers that matter
  (section 3's example ends in tests).
- **Stability**: an export field and a line kind: minor for the JSON format. For the
  `meada-core` crate see section 7.
- **Rollout**: one ticket per language pack; the first sets the convention table every pack
  fills.

### 4. Consumer counts on route lines

- **Shape**: `search kind=route` (and `contract` lines of HTTP routes) end with ` (consumers:
  <n>)`, as `package` lines already do: the links in the workspace whose provider is the route.
- **Tool list**: +5 tokens (the kind's description).
- **Indexing cost**: none. **Query cost**: the links `search` already reads for package counts.
- **Workflow it shortens**: an API map in one call: routes no consumer in the workspace reaches,
  and the ones many do. Today it is a `context` call per route.
- **Caveat**: the count covers the workspace's repositories only, which the line's meaning says.
- **Stability**: a field at the end of an existing line kind. The output grammar allows it; the
  stability document should list it (section 7).

## 6. The deciders stay central

The rule every item follows: **a tool may show an open item and the command that closes it; it
never chooses between candidates, ranks them, or hides ambiguous links by default.**
`meada decide` stays the only path to the `decided` level, through either protocol
(`decisions`, with Jev through OpenRouter by default, or `chat`), and `meada.decisions.toml`
stays its only output.

- `impact` lists the open items on the upstream links with the same `meada decide --item` line
  `search kind=ambiguous` prints, so an agent reviewing a change sees the undecided links that
  make its impact uncertain, at the moment they matter. Ambiguous edges are counted apart, never
  dropped or picked.
- `trace` with `to` keeps an ambiguous link's row and candidates on the path.
- No level filter is proposed: hiding `ambiguous` and `inferred` lines would hide exactly what
  the deciders exist for.
- Open items come from one place (`items.rs`). When package links become decidable (#392),
  `impact` lists them with no change of its own.
- Nothing here calls a model, writes a decision or adds a decider protocol.

## 7. What fits 1.x and what waits for 2.0

All four items add to the surface and change nothing that exists. Two pre-1.0 edits decide
whether they can ship in 1.x minors:

1. **The stability document (PR #525).** It says "Three tools, always with these names", says
   `uncommitted` is the only accepted value of `trace`'s `changes`, and lists the minor changes
   without "adding a tool", "adding an accepted value to a parameter" or "adding a field at the
   end of a line kind" (which the output grammar already allows). As written, item 1 waits for
   2.0, and item 4 is arguable. Adding those three cases to the minor list, with the 1,000-token
   tool list as the guard on new tools, lets items 1 and 4 ship in 1.x.
2. **`meada-core`'s export types.** `Export`, `Symbol` and their siblings are public structs
   without `#[non_exhaustive]`, and the crate's version follows the workspace's. Once
   `meada-core` is 1.0, a new field breaks code that builds those structs, so item 3's field (and
   #512's) would need `meada-core` 2.0. Marking the export structs `#[non_exhaustive]` before
   1.0 keeps new fields minor for the crate as they are for the JSON.

Not proposed, and 2.0 material if ever wanted: making `trace` breadth first by default,
changing the default budget, retiring `trace`'s `changes` in favour of `impact`, or changing how
`search` matches names.

## 8. Next steps

1. The maintainer decides the two pre-1.0 edits of section 7.
2. ADR 0023 (`docs/adr/0023-impact-tool.md`, proposed) is accepted, amended or declined.
3. With the proposal accepted, `/to-tickets` drafts one ticket per item (item 3 one per language
   pack), each updating the output grammar, its goldens and the user guides in the three
   languages.
