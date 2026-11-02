# Usage

At the root of the repository:

- `meada index` builds the index, in `.meada/`, of the repository and its peers;
- `meada status` shows the set of repositories, the state of each index and any peer that was
  not found; under each repository, the unlinked resources and each `via` that matches no
  symbol, and each source file a pack dropped for passing a limit (`too broad` for a qualified-name budget, or a project
  that gives its files too many usings);
- `meada export` prints the whole index as sorted JSON, good for diffing two runs;
- `meada decide` resolves ambiguous links and dynamic URLs, see Decisions;
- `meada mcp` serves the [MCP tools](mcp.md).

The export starts with three fields that say where it comes from. `format` names the export format (1
today): it rises when a reader of the previous format would misread the new one (a
field renamed or removed, a meaning changed), never for an added optional field, and it is not
the index schema version. `commit` is the commit `HEAD` named when the index was read, and
`dirty` says whether the checkout differed from it (changed, deleted or untracked files; ignored
ones, such as `.meada/`, do not count). Both are left out together outside git, before the first
commit and when `HEAD` or the files change during the refresh. In server mode they come from what
`meada index --workspace` recorded, or from the generation, never from the mirror's current
`HEAD`, and there `dirty` is always `false`.

Meada only writes inside `.meada/`, which ignores itself in git. Every query first refreshes
the index with what changed in the files, including what is not committed yet. Command-line
messages follow `LANG` (Portuguese or English); MCP messages are always in English.
