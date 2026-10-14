# 4. Index storage and freshness

- Status: accepted
- Date: 2026-10-14

## Context

Meada keeps one index per repository (ADR 0002, invariant 5) and answers across several of them.
Prior art stores source text on every node, which inflates the index, and goes stale as soon as
the user edits a file (`docs/prior-art.md`, gaps 5 and 8). Decisions and links also have to
survive unrelated edits (ADRs 0005 and 0006). This ADR fixes where the index lives, how it stays
fresh, and how things in it are named.

## Decision

### Storage

- The index is a SQLite database (rusqlite, bundled) in `<repo>/.meada/`.
- `.meada/` holds its own `.gitignore` containing `*`. Meada never edits the user's
  `.gitignore` (invariant 3), and tools that respect ignore files skip the folder.
- Each worktree has its own index.
- A cross-repo query opens the peers' indexes with `ATTACH DATABASE`. Several indexes are read
  together and never merged (invariant 5).
- The index holds no source text. A span is stored as a location and a content hash. Its text is
  read from disk when shown, and the hash is checked first.

### Freshness

- Freshness is on demand, with no watcher. On every MCP call, Meada stats (mtime and size) the
  files of the repositories involved, including uncommitted and untracked ones, and hashes only
  the files that changed.
- It then reindexes those files and their dependents, which are the importers of their exports
  and the contract links that touch them, before it answers.
- Refreshing is a write inside Meada's own index. It is the only write an MCP call makes
  (ADR 0002, invariants 3 and 4).

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
  harder to find.
- **tantivy or a custom format.** No SQL, no `ATTACH`, not inspectable with `sqlite3`.
- **A watcher in the MCP server.** A long-lived process, and inotify limits in large
  monorepos.
- **Manual refresh only.** It leaves the staleness gap half open.
