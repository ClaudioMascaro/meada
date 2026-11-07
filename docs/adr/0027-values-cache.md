# 27. A values cache beside the index

- Status: accepted
- Date: 2026-11-04

## Context

A contract extraction reads values the index does not keep: call arguments, constant values,
return values, the values a binding holds (`Call::args` and the other fields the index leaves
out of `Facts`). A refresh that extracts a group of packs again (#391) parses again, up front,
every file whose values the last extraction read (`valued`). #610 measured a one-line edit of a
Rust file of this repository: a 3.43 s refresh, of which 2.0 s parse the 222 `valued` Rust files
again, while only the edited file changed. A per-file read set does not remove that cost, since
the HTTP client reader follows value flow across the whole group. This ADR records how a
refresh reads those values back instead (#611).

## Decision

### Where it lives

- The values cache is one SQLite file, `values.db`, beside `index.db` in Meada's own index
  directory: `<repo>/.meada/` locally (whose `.gitignore` of `*` covers it), and
  `<index_dir>/<identity>/` in server mode (ADR 0011). Meada writes nothing else and nowhere
  else (ADR 0002, invariant 3). An index with no file (in memory) has no cache.
- It is not part of the index: no query reads it, `export` and `status` do not see it, a
  snapshot bundle (ADR 0012) carries `index.db` alone, and the stored format and its golden
  (`tests/golden/schema.txt`) do not cover it.

### What an entry is

- One row per file: its path, a key, and what the file's pack parsed (`parse_with`) as JSON,
  values included (`lang::with_values`). What `LanguagePack::join` sets afterwards (`unit`,
  `unit_references`, `internals`, `usings`) stays out: a refresh joins the entry as it joins a
  parse.
- The key is the BLAKE3 of the build of Meada (its version, the stored format's version, and the
  running executable's path, size and mtime), the pack, the setting the file is parsed under and
  the file's bytes. The executable's stat keeps two builds of one version (an installed binary
  and a working tree's) from reading each other's values. Each file has one row, so two builds
  refreshing one checkout in turn replace each other's entries: that costs only parses. An
  executable Meada cannot find or stat keeps no cache.

### How it is read and invalidated

- Where a refresh parses a file again for its values, it reads the file, computes the key and
  takes the entry when the stored key matches; otherwise it parses and keeps the result to
  write. A changed file's key differs, so it is parsed; an unchanged one is not.
- At the end of each extraction, the parsed entries are written and every entry of a file not in
  the new `valued` is deleted, in one transaction. The cache never holds more than one entry
  per file of `valued`, and no entry past `ENTRY_BYTES` (8 MiB of JSON; the serialization stops
  there): a pack's limits bound its parse, not the JSON of it, since a C# read's scope is one
  string in memory and written once per read. A file past it is parsed on every refresh.
- Anything wrong with the cache is a miss: a missing file, one SQLite cannot read or whose table
  has another shape (recreated when an entry is next written), an entry that does not
  deserialize (a value nested past `serde_json`'s depth), a lock held past the busy timeout. A
  write that fails is dropped. Deleting the cache costs only the parses.

### Size

On this repository at its trunk (#611): `index.db` 60.8 MB, `values.db` 18.7 MB with 616
entries, the largest 1.6 MB. A one-line edit of a Rust file refreshes in 1.4 s instead of 3.4 s:
the edited file is the only one parsed for its values (23 ms instead of 2.0 s), and reading the
221 others back takes 0.1 s.

### Determinism

A pack's parse is a function of the path, the bytes and the setting alone, all in the key with
the build that parses, so an entry is what a parse of the file would give. The JSON round trip
is lossless for every field of `Facts` (a unit test round-trips every fixture file of every pack).
After a hit, the refresh joins, extracts and resolves as it would after a parse, so a refreshed
index equals a cold index of the same files (`tests/it/freshness.rs`), and a cold index, which
has no `valued` to read up front, is unchanged. The hostile probes index each case a second
time without its index, over the cache the first run left, to the same export.

## Consequences

- The cache holds values: string literals, names and the shapes of expressions as the packs read
  them, which ADR 0004 keeps out of the index. It holds no source of a symbol or a call, is
  never read to answer, and leaves with the index directory.
- A changed file is still parsed twice per refresh (once for its stored facts, once for its
  values); the second parse could come from the first, which only the changed files pay.
- #610 resumes on top: what remains of a refresh is the extraction and resolution of the
  changed group.
