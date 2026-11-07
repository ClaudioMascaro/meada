# Flow of a query and server mode

Detail behind the [architecture map](../architecture.md), which routes flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

How a workspace opens and what `Engine` reads is in the
[flow overview](overview/flow.md). The rest:

The status report and readiness share one `Manifest::read_listed`, including a managed version
swap during the informational Valkey probe; `ready` also reuses one listed manifest across
its normal and locked-index checks. An
engine opened from a
manifest opens every index read-only, one connection each (`index::open_read`: nothing created,
`trusted_schema` off and `SQLITE_DBCONFIG_DEFENSIVE` on, since a generation's index comes from
a store, one read transaction per connection held until the engine drops, so an answer sees each index as one commit left
it and a commit waits for the answer, both up to a busy timeout) and its `refresh()` does
nothing, so only `meada index --workspace` writes; `open_manifest` is also what `meada export --workspace --repo`
opens. With the snapshot module (feature `snapshot`, `--generations`), `Engine::open_held`
builds the workspace from a generation the node holds instead
(`snapshot::serve::held` reads the pointer file and the generation document, and
`Manifest::of_generation` turns it into the same `Manifest`: per entry the mirror at
`<generation id>/<bundle id>/mirror`, the index beside it, the mirror's own `meada.toml` whole or
else the blocks the document recorded, each read up to `config::GENERATION_CONFIG_MAX` and
through no symbolic link; no `git` runs, the pull checked each mirror) and opens it read-only
as `open_manifest` does, one engine per call. The server keeps the last generation read
(`serve::Current`, one `Arc<Held>`:
a generation's directory never changes once renamed into place), so a request reads only the
pointer file until it names another id, and the request's route hands the `Held` it routed by
to the call (`Source::Generation`): a swap between two calls changes the generation and one
during a call does not, and a call still answering keeps its own `Arc` once the server moved
on. The node's manifest contributes only its module
sections. `engine::served_generation` is its start check and the predicate `status
--generations` exits on (current generation present, every mirror rechecked, every index at the
schema version and holding exactly this build's schema objects, `index::vet`, the whole
workspace opens), `engine::status_generations` its text (the node's
current and held ids, and the store's current id with a short timeout, which never changes the
exit code), and `Engine::open_node` what `export --generations` opens. An index that cannot be
read in a generation names the generation and its fix (`msg::Fix`), not `meada index`: a
publish from this build for another schema version, else removing `<local_dir>/<id>` before
`meada snapshot pull` (a pull of the generation already current changes nothing) or a new
generation. Every query (`export`, `search`, `context`, `trace`) calls `refresh()` first, then
reads each index, adds the contracts each `meada.toml` declares (`ContractKind::declare`),
derives **links** from both, and renders. `export` (and `status`) read every row
(`Engine::exports`, `index::export`); `search`, `context`, `trace` and the open items read a
**partial export** (#396, `Engine::partial`, `partial.rs`): each index's contract rows whole
(`index::contracts`, what links derive from), and of its code rows (symbols, calls with their
candidates, communities) only those of the answer, through SQL filters on the indexes
`symbols_id`, `calls_id`, `calls_caller`, `calls_target`, `candidates_target`, `symbols_hash`
and `communities_member`. Before declaring, a partial export reads the symbols each kind looks
up (`ContractKind::symbols_read`: a resource's `via` symbols, and every symbol of a repository
with a named manifest, which an import resolves into through `package::Provided`) and the calls
its decisions name. Then the query reads its rows: `search` scans the symbols one row at a time
(`index::each_symbol`) and looks a contract's site up by id; `context` reads the symbol, the
calls that reach it and that it makes, the symbols at their ends, what `ContractKind::
context_reads` names and what is upstream of it for its processes (`trace::read_processes`);
`trace` reads its start (`trace::starts`) and every node a walk may reach from it
(`trace::read`, a superset of the edges `Walk` and `Graph::edges` follow). The code that renders
then looks rows up in the partial export as in a whole one; nothing is kept between queries, so
memory follows the answer and an MCP call pays a few SQL statements per step of its walk.
`engine::with_every_row` (the one switch, in `partial.rs`) makes a query read every row instead:
`Partial::open` reads whole exports, and the readers that count or place rows without holding
them (`partial::each_symbol`, `symbols_hashed`, `spans`, `community_sizes`, `community_of`)
answer from the whole export as before #396. `tests/it/partial.rs` checks that both give the same
bytes over the fixtures, including traces from short names and from an uncommitted edit.
Nothing outside the index's directory (`.meada/`, or `index_dir` in server mode) is written.
Server mode also runs on `index_dir` and mirrors mounted read-only, possibly owned by another
user: the open creates no file there (no `index.db.lock`, only a rebuild takes it; no
`.gitignore`; no journal), and a hot journal that only a write could roll back is reported as the
interrupted-commit error naming `meada index --workspace`. The open is SQLite's plain read-only
one, not the `immutable=1` URI. Trade-off: `immutable` skips locking and change detection, so it
would also skip the shared lock that makes a commit in progress wait for readers and readers for
a commit, and it would ignore a hot journal, serving a half-written file; it is safe only for
files that never change in place. A plain read-only open takes a shared `fcntl` lock, which works
on a read-only mount (the test locks directories `0555` and files `0444`) and keeps the busy
timeout and the interrupted error working. `meada index --workspace` refreshes an index of the
current schema in place, in one transaction (only a rebuild renames), so a server reading with
`immutable=1` while it runs would take no shared lock and not see the hot journal, and could
serve half-written pages. `immutable` would be safe only for an index that arrives whole and is
never refreshed in place on that node; that is why the plain read-only open stays.
For a mirror another user owns, `git` refuses it (`safe.directory`); `repo::command` is the one
place Meada runs `git`, and it adds `-c safe.directory=<mirror>` for the paths `Manifest::read`
registered with `repo::trust`: the manifest's mirrors only, in server mode only, never `*`. Git
honours `safe.directory` from the command line only from 2.38 (Ubuntu 22.04's 2.34 and RHEL 8's
2.31 ignore it on purpose), so a mirror owned by another user needs git 2.38 or later, or
`safe.directory` in the system configuration; on an older git the start check fails closed with
an error naming the mirror and that minimum (`Msg::DubiousOwner`).
Server mode reads the **workspace manifest**, and each mirror's `meada.toml` and
`meada.decisions.toml`, once at start (`engine::served`, which refuses to start unless every
repository can be served), so a change to those files needs a restart, while a code change in a
mirror needs only `meada index --workspace`. It refuses `trace` with `changes: uncommitted`, and
`context` prints `index behind the mirror: run meada index --workspace` for a span whose file
changed since indexing.
Everything the engine returns is sorted: the same files give byte-identical output.
