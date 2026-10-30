# 4. Index storage and freshness

- Status: accepted
- Date: 2026-10-14

## Context

Meada keeps one index per repository (ADR 0002, invariant 5) and answers across several of them.
Prior art stores source text on every node, which inflates the index, and goes stale as soon as
the user edits a file. Decisions and links also have to
survive unrelated edits (ADRs 0005 and 0006). This ADR fixes where the index lives, how it stays
fresh, and how things in it are named.

## Decision

### Storage

- The index is a SQLite database (rusqlite, bundled) in `<repo>/.meada/`.
- `.meada/` holds its own `.gitignore` containing `*`. Meada never edits the user's
  `.gitignore` (invariant 3), and tools that respect ignore files skip the folder.
- Each worktree has its own index.
- (Amended 2026-10-21; in server mode, ADR 0011, the index lives at
  `<index_dir>/<identity>/index.db`, keyed by remote identity, with no `.gitignore`, and nothing
  is written into the checkout.)
- A cross-repo query opens the peers' indexes. Several indexes are read together and never
  merged (invariant 5). (Amended 2026-10-28; each index is read through its own connection
  instead of `ATTACH DATABASE`, since SQLite attaches at most ten databases to one connection
  and a workspace has no such bound.)
- The index holds no source text. A span is stored as a location; the content hash is per file
  (`files.hash`), which freshness compares and which `context` checks before showing a span. Each
  symbol also stores the BLAKE3 of its span's exact bytes (`symbols.hash`, no normalisation),
  which `search` with kind `duplicate` groups by. Text is read from disk when shown.

### Freshness

- Freshness is on demand, with no watcher. On every MCP call, Meada stats (mtime and size) the
  files of the repositories involved, including uncommitted and untracked ones, and hashes only
  the files that changed.
- It then reindexes those files and their dependents, which are the importers of their exports
  and the contract links that touch them, before it answers.
- Refreshing is a write inside Meada's own index. It is the only write an MCP call makes
  (ADR 0002, invariants 3 and 4).
- (Amended 2026-10-21; in server mode, ADR 0011, freshness is by command: the server stats
  nothing per call and writes nothing, and the operator runs `meada index` after a mirror
  moves. Freshness on demand stays the default.)

### Stable identities

Identities never use line numbers. Lines are for display only.

- A symbol is `<repo remote>::<path>::<qualified name>`.
- A call site is its enclosing symbol, the normalized call expression and an ordinal.

Links and decisions refer to things by these identities, so an edit above a call does not
orphan them.

### Canonical export

`meada export` writes the index's content as sorted JSON. It is the artifact that determinism
(ADR 0002, invariant 6) is checked against. The `.db` file does not need to be byte-identical.

(Amended 2026-10-30; #512.) Three top-level fields say which shape the export has and which
state of the repository it describes:

- `format`: an integer naming the export's shape, 1 for the first versioned one. It rises when
  a reader of the previous shape would misread the new one: a field renamed or removed, a
  meaning changed. An added optional field does not raise it. It is not the index schema
  version. It is always written, and an export without it predates it: `meada_core::export`
  refuses to parse one.
- `commit`: the indexed commit, the commit `HEAD` named when the index's content was read. It
  is left out outside git, before the first commit, and when nothing trustworthy is known.
- `dirty`: `true` when the checkout differed from that commit (changed, deleted or untracked
  files; ignored files do not count, so `.meada/` never does). It is left out with `commit`.

Where they are read:

- Locally, `meada export` refreshes the index before reading it, so indexing and export are one
  call: it reads `HEAD` before and after the refresh and the checkout's status after it. A
  `HEAD` that moves, or files that change under the refresh, leave both fields out. A query
  never reads git for this.
- In server mode the index may lag its mirror, and the console exports past generations, so the
  fields come from what indexing recorded (the indexed commit kept beside the index, or a
  generation's `head`), never from the mirror's live `HEAD`. Server-mode indexing records a
  commit only for a clean checkout, so there `dirty` is `false` whenever `commit` is present.

## Considered options

- **An index in the user's cache directory.** Keyed by path, orphaned when a repository moves,
  harder to find. (Amended 2026-10-21; still rejected for local use. Server mode, ADR 0011,
  keeps indexes outside the checkout, keyed by remote identity in a directory the manifest
  names, which answers both objections.)
- **tantivy or a custom format.** No SQL, no `ATTACH`, not inspectable with `sqlite3`.
- **A watcher in the MCP server.** A long-lived process, and inotify limits in large
  monorepos.
- **Manual refresh only.** It leaves the staleness gap half open.
