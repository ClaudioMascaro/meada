# Invariants the code relies on

- Server mode writes nothing: the engine opens every index read-only, `refresh()` does nothing,
  and the HTTP listener opens no outbound connection. Only `meada index --workspace` writes, and
  only under `index_dir`.
- Determinism: all export lists are sorted, hashes are BLAKE3, no clock or random value reaches
  an answer. A refreshed index must equal a cold index of the same files (`tests/it/freshness.rs`),
  communities and processes included: `refresh` recomputes both at its end from the stored calls
  whenever anything changed. The test-only `with_reversed_walk` (`index.rs`, hidden from the
  docs, no CLI flag or setting) reverses the walk order, the parse order and the insertion order
  of the `symbols`, `calls` and `candidates` rows, and the chain's export must stay
  byte-identical. The in-memory order is normalized by path (`BTreeMap`s), so the switch covers
  nothing beyond that. Indexing runs on one thread, so there is no worker count to vary.
  A refresh keeps the rows of a group of packs that resolve together when nothing it reads
  moved (#391): that rests on a group's extraction reading only its own files and finding
  contracts only in them, which `refresh` checks with debug assertions
  ([map/indexing.md](../indexing.md)).
- The schema version (`index::schema_version`) is derived from `tests/golden/schema.txt`, the
  stored format: `SCHEMA`, the facts of each pack's `tests/fixtures/schema/sample.<extension>`
  and the summaries of the document and manifest samples there. A unit test in `index.rs`
  (`the_stored_format_matches_its_golden`) fails on any change to them until `MEADA_BLESS=1`
  rewrites the golden, which changes the version; no number is kept by hand, so two branches
  that change the format conflict only in the golden. An index with another version, or an
  unreadable one, is rebuilt beside it and renamed over it, and `status` says so. One whose stored facts do not
  parse is rebuilt by the first refresh that reads them (when a file changed), which `status`
  cannot tell.
- The index holds no source text and no values of code, apart from the BLAKE3 of each symbol's span
  (`symbols.hash`, `Def::hash`) and the facts listed under "Stored values" in
  [map/indexing/stored-values.md](../indexing/stored-values.md): argument counts and labels, names a file
  declares or imports, summaries of manifests, build files and provider documents, the field
  paths of shapes, entry points and communities, and the config values of YAML and JSON files
  (ADR 0004 amended, ADR 0025). A value the index now stores joins that list. A
  contract kind that reads a value from a new place relies on the loop of misses (`Code::read`,
  `valued`) that `refresh` runs until none is left. The values cache beside the index
  (`values.db`, ADR 0027) gives back what a pack parsed of a file, values included, by the file's
  bytes, setting, pack and Meada's build: no query reads it, and a hit is what a parse gives.
- Links are never stored. They derive from the stored provider and consumer contracts, then the
  decisions replay, when a query runs, so a change on either side, or to a decisions file, needs
  no relinking. `Level` order: `exact` < `declared` < `inferred` < `decided` < `ambiguous`.
- Network: `decider::Config::send` is the only code that opens a connection, and only
  `meada decide --decider` without `--show-request` or `--list` reaches it. Indexing, queries,
  `status` and the MCP server never read the `MEADA_DECIDER_*` variables. `ureq` is the only
  HTTP dependency.
- One index per identity: a second clone of a repository already in the workspace is skipped.
- `context` shows span text only while the file's BLAKE3 still equals the indexed hash; on a
  mismatch it forgets the file's mtime, refreshes and retries once.
- Facts resolve only among files of the same pack, and inference runs only when resolution
  fails, among the files the caller reaches (`Layout::reaches`); an import from outside the
  repository is never inferred. A file its pack leaves out
  (`Layout::members`) resolves nothing and has no symbol, call or `files` row in the export.
