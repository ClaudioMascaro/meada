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

Meada only writes inside `.meada/`, which ignores itself in git. Every query first refreshes
the index with what changed in the files, including what is not committed yet. Command-line
messages follow `LANG` (Portuguese or English); MCP messages are always in English.
