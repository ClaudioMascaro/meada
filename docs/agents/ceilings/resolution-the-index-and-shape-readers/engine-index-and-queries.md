# Engine, index and queries

- `engine.rs` (`Engine::refresh`, #443): the peers' connections are opened once, with the engine, and
  every query refreshes and reads through them. A peer whose index schema changes while the
  process runs (another Meada version rebuilding it) is not rebuilt mid-session: the engine keeps
  the file it opened, and the rebuilt one is read from the next process start. Reopening a peer per
  query is what #443 removed (it split the refresh from the read); a mid-session reopen would
  have to swap the handle `exports` reads, and the ticket left it out.

- `engine.rs`: `search` scans every symbol. Kind `duplicate` folds no nested groups (a duplicated
  class and each of its methods are groups of their own) and has no minimum span size (#254).

- `index.rs`: a refresh keeps the rows of a group of packs that resolve together only while no
  file of it changed and nothing every group reads moved (config files and manifests, Meada's
  version, the file list, the exports of every group): a file added or deleted anywhere, or an
  edit that changes what a file exports, extracts and resolves every group again. A group holding
  a changed file is extracted and resolved whole, over the values of every file of it the last
  extraction read (`valued`): the HTTP client reader (`http/python/clients.rs`, which reads
  reqwest and ureq too) walks the value flow of the receiver of every call named like an HTTP
  method (`map.get(k)` included), about 120 of the 222 Rust files here, and the actix-web and
  Rocket reader reads every call's arguments (`Web::read`, under one `ITEM_WORK` budget spent in
  walk order) and every file's mentions (`Web::mentioned`). The values cache (ADR 0027) gives
  those back for every unchanged file. On this repository a one-line edit of a Rust file
  refreshes in 1.02 s, a median of eleven (1.52 s before #610 on the same machine, 3.4 s
  without the cache). What is left, in the order of its cost: building the Rust group's resolver, which
  resolves every call (about 170 ms), reading the cached values back as JSON (about 150 ms),
  reading every kept file's stored facts, which the entry points read (about 110 ms), the http
  kind over the Rust group (axum's routers and the actix reader's argument scan, about 120 ms),
  the package kind's import of every call (about 80 ms) and writing the contract rows (about
  60 ms). Narrowing a group per file needs each kind to say what its contracts read, and the
  client reader's walk depends on resolver answers over the whole group (exact callers,
  definitions) that no read set per file captures (#391, #610).

- `index/values.rs`: the values cache holds one entry per file of `valued` (pruned on every
  extraction), each the file's parse as JSON, values included, deflated (#839): on this
  repository, as plain JSON, 18.7 MB beside a 60.8 MB index, 616 entries, the largest 1.6 MB. An entry is keyed by the file's bytes, setting
  and pack and by Meada's build (version, stored format, the executable's path, size and mtime),
  so a rebuilt or moved binary misses every entry once. An entry is at most `ENTRY_BYTES`
  (8 MiB) of JSON before it is deflated, so the cache is at most that per `valued` file: a file
  whose parse serializes past it (a C# read's shared scope is written once per read) is parsed on
  every refresh. A read inflates a stored blob to `ENTRY_BYTES` at most, whatever the blob asks
  for, so one that would inflate past it is a miss. An entry nested past `serde_json`'s depth
  (128) does not deserialize and is parsed again on every refresh, as is anything else wrong with
  the cache (unreadable, locked past the busy timeout, a blob that is no zlib stream, a write that
  failed): a miss costs a parse, never a different index. An extraction that leaves more than half
  of the file's pages free vacuums it. A changed file is parsed twice per refresh, once for its stored facts
  and once for its values, since the first parse is under the setting `settle` may still change.
  A refresh that changes nothing or keeps every group leaves the cache as it is. A dev build that changes a kind
  without changing the version keeps the rows an older build gave the groups it does not touch.
  An entry point that asks the name of an inline definition of a kept file makes the refresh
  roll back and run again keeping no group, a second full pass.
  An unreadable `contracts.fragments` value is read as
  no fragments, since only `refresh` writes it. A manifest or pack config file (`package.json`,
  `pyproject.toml`, `requirements*.txt`, `Cargo.toml`, `go.mod`, `tsconfig.json`, `jsconfig.json`) is read up to one
  byte past `lang::MAX_BYTES`, and a provider document up to one byte past
  `document::DOCUMENT_BYTES_MAX` (`read_up_to`), which their readers take as invalid whatever the rest
  holds, so the `manifests` and `documents` hashes and the config digest are of what was read. A
  pack config, at the root or at any depth (`**/`), is read through no symbolic link
  (`read_config`), as the walk follows none.

- `index.rs` (`refresh_stamped`): a `package.json` or document that cannot be statted or read
  (it vanished after the walk, or is unreadable) has no row instead of failing the refresh; a
  `[serve] documents` entry in that state is then reported as missing.

- `entry.rs`: an export entry point is a function, or a method of an exported class (a
  constructor excepted), whose exported name the exporting file's `Facts::star` lets through when
  its pack names a public set (Python: no leading `_`, or a literal `__all__`; TypeScript names
  none, so all pass), that the `exports` rows name and no call reaches in its repository,
  an ambiguous candidate counting as a caller and a self-call not; a call through a package
  import of its own repository counts too, but only at query time (`Engine::exports`), so the
  stored rows over-list in a monorepo. A method of a class that is not exported, a
  default-exported anonymous function and an export the resolver cannot place are not found (no
  marker). A route handler Meada could not find (an Express handler passed as a parameter) has
  no entry point.

- `trace.rs` (walks): `WALK_DEPTH` (256) caps the recursion of one branch of every `trace`,
  down or up, from any start, and of a process: the line at the limit is written, marked
  `(depth limit)` (a process step `depth-limit`), and nothing hangs from it, so a longer chain is
  cut. A node cut there is not counted as followed: a shorter path that meets it later follows
  it. Where an IaC trace continues into code (`Graph::attached_rows`), the limit counts from the
  IaC node, so code behind a nested IaC node is cut deeper than 256 (the IaC walk keeps its own
  stack). Without it a debug build (`28f1a4d7`, MCP over stdio, a runtime worker's 2 MiB stack)
  aborted on a call chain between 1,972 and 1,992 hops down and between 3,144 and 3,164 up, and
  on a chain of HTTP links between 900 and 1,200 hops down (#732): the limit stays more than
  seven, twelve and three times under them.
- `trace.rs` (cost, #745): a `trace` writes every line of the tree it reaches before the budget
  cut (`Chain::answer` keeps the lines that fit), so its work follows the reachable graph, not the
  lines it returns. `render::Files` counts the newlines of each file it reads once per answer,
  per `LINE_BLOCK` (1 KiB), so placing a line on its file's line reads one block, not the file
  before it. A line still costs its depth: its indent (two spaces a level) and the cycle check,
  which scans the branch's ancestors, each at most `WALK_DEPTH`. Reading the reachable graph
  (`trace::read`) also grows faster than it: the index is asked once per frontier and `Partial`
  sorts every row it holds at each merge, so the merges cost the frontiers times the rows. On the
  issue's generated Python graph in one file (each function calling 3 of the next 20; debug
  build, `7518ebe7`, MCP over stdio) the merges took 6, 14, 25 and 79 ms at 1,200, 2,000, 3,000
  and 6,000 functions (75 to 357 frontiers), of a read of 51 to 342 ms; at 6,000 the walk
  without the line lookups took 35 ms for its 3,904 lines, down to depth 256.
- `trace.rs` (process walk): an ambiguous call or link is one unexpanded step listing its
  candidates; `inferred` and `decided` links expand. A process is walked down only; `trace`
  up from `process::...` answers with a note. `Graph::processes_of` (the processes `context` lists)
  walks upstream with no depth limit, so on chains deeper than `WALK_DEPTH` membership and
  trees can disagree: a node past depth 256 is no step of the tree, and the limit can hide a
  node shallower than that, through `Seen`. E reaches X at depth 255, so X expands and each of
  its callees, Y among them, is a `depth-limit` step at 256 whose own callee Z is no step; a
  later direct E to X shows X as seen, so Y is not expanded there either, yet `processes_of(Z)`
  lists E.

- `community.rs`: Leiden has no iteration cap (spec #31 asks for none). Each pass that changes
  anything either moves a node to a community with a strictly positive modularity gain, which
  raises the modularity, an integer here (`i128`, no floating point), so no partition repeats,
  or merges subcommunities and shrinks the graph, which cannot happen forever; so the loop ends.
  The work grows with the repository's calls and passes (no marker).

- `engine.rs`: `context` builds `trace::Graph` and calls `Graph::processes_of` for every symbol
  it answers about: a reverse map of every edge of the workspace (every call and link) is built
  per call, with no cache, so the cost follows the workspace's size, not the symbol's (no
  marker).

- `repo.rs`: a path git still quotes (tab, newline or quote in the name) is skipped by
  `uncommitted()`.

- Concurrent refreshes of one index (#704). In one process, one runs at a time and a call that
  comes meanwhile waits for it with no timer, then shares the next run: a batch after a change
  costs about one refresh more than an unchanged one, whatever its size (debug build, 4 cores:
  Meada's own checkout, 50 calls 21 s against 15 s, 200 calls 70 s against 64 s; 5,000 files,
  500 calls 11.0 s against 9.2 to 10.9 s). A run reads the `[serve]` table it finds documents by
  as each of its attempts starts, whichever call started it (#758). A statement of another call
  that meets the run's lock (the first read of an open, a query, `forget`'s write) waits for the
  run, past the busy timeout (`wait` in `index.rs`): no call fails on a timer while a refresh of
  its process is in flight, however long it takes, nor while the tree keeps changing. Two things
  stay on the 10 s timer in one process. The run's own commit waits that long for the reads the
  other calls have
  in flight: one statement that reads for longer fails the run, and the calls that share it,
  with `database is locked`. And `wait` is a plain function that knows no connection: a run in
  flight on any index of the process extends the wait, so a statement behind another process's
  lock waits until the runs of its own process end, then a whole busy timeout more before it
  fails: the time it waits behind a run of its own process does not count, so a short lock
  another process takes as the run ends is waited for (#757). One stdio server answers
  `CALLS_IN_FLIGHT` (4) calls at once (#756): with its root known (the `roots/list` round trip
  holds no turn), a call waits for a turn at a fair semaphore, in the order the calls ask for
  one (rmcp starts a task per request, so two that arrive together may swap), and keeps it
  through its engine and its answer. A batch holds at most four engines and four answers on top
  of what the run allocated, and a run's commit waits for the reads of three other calls of its
  process at most. Four, by the memory one answer holds (about 26 MB waiting on Meada's own
  checkout, up to about 90 MB after a refresh that added 100,000 functions): four of the
  largest stay near 360 MB. Before the bound, under a 2 GiB cap, debug build, Meada's
  own checkout, the server was killed with 100 calls in flight when nothing changed (2,040 MB
  resident after 20 replies; one run of four survived, at a 2,044 MB peak), with 16 to 21
  during an edit every 2 s at one call a second (86 at four), and with 11 behind a run that adds
  100,000 functions to one file, which alone peaks at about 940 MB; with it the 100 calls all
  answer, in 36 to 38 s, at 249 to 274 MB. The bound costs time where answers are heavy: 30
  calls after an edit of that checkout took 18.6 s with it and 12.5 s without, at a 485 MB peak
  against 1,199 MB.
  Across processes (two servers, a CLI command beside a server) the wait is SQLite's: a refresh
  with nothing to write takes no write lock, and one with something to write waits up to the
  busy timeout (10 s) for another process's writer to commit, then walks again; after
  `DEFERRED_ATTEMPTS` such waits it takes the write lock first and holds it through its walk. A
  writer's commit waits up to 10 s too, for the read lock each other process's refresh in flight
  holds during its walk (one walk per process). The calls of a process wait behind its run
  through all of that, with no timer (three waits of 10 s, a walk per attempt and the commit's
  10 s at most), may span two runs, and share a failed run's error. A client sees `database is
  locked` on the calls of the process whose wait passes 10 s: the one behind another process's
  writer that holds the lock longer (`meada index` or another server writing a large change to
  an existing index; a missing index is built beside it and renamed, holding no lock on it), or
  the writer itself when the other processes' walks outlast it. Two servers on Meada's own
  checkout after one change, 20 calls each, answer all 40, the slowest at 13.4 s (a call waits
  past 10 s there because a refresh of its own server is in flight); a connection of another
  process that holds the write lock for 25 s gets every call `database is locked` at 10 s.
