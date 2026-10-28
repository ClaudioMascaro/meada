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

## Considered options

- **An index in the user's cache directory.** Keyed by path, orphaned when a repository moves,
  harder to find. (Amended 2026-10-21; still rejected for local use. Server mode, ADR 0011,
  keeps indexes outside the checkout, keyed by remote identity in a directory the manifest
  names, which answers both objections.)
- **tantivy or a custom format.** No SQL, no `ATTACH`, not inspectable with `sqlite3`.
- **A watcher in the MCP server.** A long-lived process, and inotify limits in large
  monorepos.
- **Manual refresh only.** It leaves the staleness gap half open.
