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
messages follow `LANG` (Portuguese, English or Spanish); MCP messages are always in English.

### From a workload to code

With the application and deployment repositories in one workspace, find the normalized image,
read who builds it and trace the code the workload runs, using the MCP tools:

```text
search(query="ghcr.io/acme/orders", kind="image")
context(symbol="image::ghcr.io/acme/orders")
context(symbol="Deployment.apps/shop/orders")
trace(from="Deployment.apps/shop/orders", direction="down")
trace(from="orders.py::orders", direction="up")
```

When two overlays use the same workload and image, export retains one link per root.
Qualify an IaC query address with its root, for example `Pod/common (root overlays/right)`.

Compose with `build: ./src` also reaches code without publishing an image. `status` uses the
visible scope for orphans; an isolated builder gets `no runner in the workspace` in queries.
In the console, contracts show images and their locations, and Workload code shows direct
paths. See supported forms in [Capabilities](capabilities.md).
