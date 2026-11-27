---
name: meada
description: Use when querying Meada's MCP tools (`search`, `context`, `trace`), locating a symbol, finding its callers or other uses, assessing which files a change or lockfile affects, or tracing routes, channels, packages, shared resources and CI pipelines across repositories.
---

# Meada

The local MCP tools answer about the client's first local root (the server's directory if
the client supplies none) and its peers. Each local query refreshes their indexes from disk.
Server mode reads indexed data.

## Steps

1. **Find** with `search` when you have only a name. For a method, use
   `{"query":"Client.send","kind":"symbol"}`. Match the repository and path to the definition
   you want. Use its identity (`<repository>::<path>::Client.send`) or a contract such as
   `http::GET::/users/{}` in the next query. Search is case-insensitive. Other kinds include
   `route`, `contract`, `channel`, `resource`, `package`, `process`, `duplicate` and `ci`.
2. **Describe** one symbol with `context` (`symbol` is its identity). Read `callers:` and
   `callees:` for calls, `used by:` and `uses:` for other indexed uses. These include TS/JS
   types, JSX, values, members, inheritance, re-exports and dynamic imports, and Go type uses.
   The answer also names the symbol's community, processes and contracts and shows its source.
3. **Follow** with `trace` from that identity. Use `direction: "down"` for what it reaches
   and `"up"` for what depends on it, including non-call uses. For local edits, pass
   `{"changes":"uncommitted","direction":"up"}` to trace dependents of the symbols the diff
   touches. For affected files, use the CLI below. Stop tracing when the footer's weakest
   level supports the action you need to take.
4. **Read** the lines the answer names before you edit them or state a claim about them.

Every MCP tool takes `budget` (tokens, 2000 by default). Raise it when the answer is cut short.

## Changed files on the CLI

For a text chain, use `meada trace 'path/to/file.ts::Client.send' --up`. For changes, choose
one start: `--changes uncommitted`, `--changes main..HEAD` (clean tree at the range's head),
or `--files path1 path2` (whole files, relative to the repository root).

- **One affected set.** `meada trace --affected --changes main..HEAD` gives JSON. Each file
  carries the level of the weakest link in its strongest chain. Read `packages` for the
  packages by tier and by manifest, and `reads` for what the change reads. Report each
  `open` path and reason. With `all-packages`, every package in that workspace may be affected.
- **Many file lists.** `meada trace --affected --batch changes.txt` reads one block per
  change: a line such as `@@candidate-1`, then one repository-relative file path per line.
  It prints one JSON object per line with the block's label.
- **Changed lockfile.** Use a range or uncommitted changes to compare a Yarn Berry
  `yarn.lock` with its base. In `lockfiles`, inspect the moved entries and the direct
  dependencies leading to them. Meada starts from files loading those dependencies, or the
  whole affected package when none does. File lists and batches supply no base: for an
  unread workspace-root lockfile, expect `open` with `all-packages`. For the resolved
  versions of a package's direct dependencies, use `context` on its package contract.

## Reading an answer

A link between repositories reads `→[level: reason]`. The level is a name, from firmest to
weakest:

| Level | Meaning | What to do |
| --- | --- | --- |
| `exact` | the code or configuration proves it | act on it |
| `declared` | a declaration in `meada.toml` | as firm as that declaration |
| `inferred` | a match by name or convention | read both ends to confirm |
| `decided` | recorded in `meada.decisions.toml`; the reason names who decided | act on it, citing the decision |
| `ambiguous` | several candidates fit by name | read the call site and candidates |

- **Weakest hop.** A text chain's footer, `chain: <level> · N hops · N repos · N tokens`,
  reports the weakest level among the rows shown. For example, a `decided` hop is weaker
  than an `inferred` one. Cite the footer's level with your conclusion.
- **TypeScript methods.** A receiver built with `new Client()`, or annotated with a class
  whose method Meada can pin, can resolve `exact` even when another class has a method of
  the same name. A known platform receiver such as a local built with `new Map()` can rule
  out same-named repository methods. An unknown receiver or a possible override can leave
  name matches. In `context`, name matches carry their level and reason. An exact call has
  no suffix. To inspect its stored level, read the call's row in `meada export`.
- **Unproven absence.** A missing caller is unproven. For example, Rust calls inside
  `assert!`, `format!` and `vec!` arguments escape indexing. Before deleting or renaming
  a symbol on that evidence, search the workspace text for other uses.
- **Orphan lines.** An orphan line of `meada status`
  (`orphan <contract>: no use in the workspace; <place>`) states an absence in the scope it names:
  the workspace, or the repository of a workspace of one. A use in a repository the workspace does
  not select is not seen. An orphan line is a lead for review, never by itself a reason to delete
  a resource.
- **`(unresolved)`.** When a URL or channel name is computed at run time and Meada cannot
  resolve it, `context` shows it without a link. Read the call site to trace its destination.
- **Open items.** Search by enclosing symbol or call expression with `kind: "ambiguous"` or
  `"unresolved"` for open links or dynamic URLs. Inspect the candidates or the reason an item
  cannot be decided. Propose the displayed `meada decide` command to the user when a choice
  is available: accepting it writes `meada.decisions.toml`. An unresolved channel side
  has no link and is outside this decision list.
- **No matches.** The name may differ, or the language may not be indexed. `meada status` in a
  shell shows each repository of the workspace and its index state.

## Routes and channels from configuration

- **HTTP.** When several services serve a path, trace down from the consumer and read the
  link reason. In `meada.toml`, `[[base_url]]` maps the variable names in `env` to a `dir`
  in that repository. A direct match there can link `declared`. A relative URL prefers a
  matching route in its own package.
- **Kafka.** Search with `kind: "channel"` by broker or topic name. Use `context` on
  `channel::kafka::orders.created` for producers and consumers, then trace down from the
  producer for links and reasons. In `meada.toml`, `[[channel_client]]` names the call
  (including a TS class method), its role, broker and topic argument. `[[helm_env]]` names
  the chart values key holding the environment. Check the values files named in the reason:
  agreement can yield `declared`, a missing or differing value can yield `inferred`.
  For kafkajs, Meada also recognizes clients held in private fields and reads topics from
  Backstage's `app-config*.yaml`. To list affected consumer files across peers after editing
  a producer, use `meada trace --affected --changes uncommitted`.

## CI (GitHub Actions)

A pipeline or an action that other files call with `uses` provides a CI contract, named as a
`uses` names it without the ref: `ci::<repository>/<file path>` for a pipeline,
`ci::<repository>[/<folder>]` for an action. Each CI job or CI step of the workspace that calls
it consumes it.

- **Find** a pipeline, an action, a CI job or a CI step with `search` and `kind: "ci"`, by type
  (`workflow`, `action`, `job`, `step`), address (`jobs.publish`, `jobs.build.steps[2]`), id,
  `name` or `uses`. A search with no `kind` leaves them out.
- **Callers.** `context` on the file's path (`.github/workflows/publish.yml`) lists one
  `consumer:` line per caller in the workspace: repository, file, address, the link, and the
  keys of `with` and `secrets` it passes. A missing CI caller is unproven. Callers outside
  the workspace are invisible to Meada.
- **Chain.** `trace` with `direction: "up"` follows callers through intermediate reusable
  pipelines and composite actions. Three starts give the same chain: the file's path, its node
  (`<repository>::<file path>::.`) and the contract (`ci::...`).
- **Git ref.** A link reads `[exact: @main (indexed 1a2b3c4)]`: `@main` is the ref the caller
  wrote, `1a2b3c4` the commit Meada read the called file at. Meada resolves no ref, so `exact`
  says the caller names that file, not that it runs the indexed version. `, ref mismatch` marks
  a caller pinned to a full commit id other than the indexed one: read the file at that commit
  before you state what that caller runs. `(indexed commit unknown)` never carries
  `ref mismatch` because Meada has no indexed commit to compare with the ref.
- **Permissions.** A `consumer:` line of a CI job ends with the `permissions` block that governs
  it: its own, else its pipeline's, else `not declared`. The `permissions:` line under
  `provides` is what the CI jobs of the called pipeline declare together. A reason that ends
  with `permissions mismatch: id-token write against none` marks a caller whose block holds
  less than the called pipeline declares (`write` there, `none` here): name that caller before
  the pipeline comes to depend on the permission. Say "declares", as the files do at the
  indexed commit: the effective permissions of a run also follow settings Meada does not read,
  so `not declared` is unknown, never `none`.
- **Programs.** A CI step whose command is a console script of a PyPI package of the workspace
  links to the function the script names, so `trace` with `direction: "up"` from a symbol or a
  route reaches the CI jobs that run it, and `direction: "down"` from a CI job enters the
  program. Read the level and its reason together: the link is `inferred` at best, since an
  install, `uvx <package>` and `pipx run <package>` name the package by its name alone and a
  fork of a public package may hold that name. The reason says whether an earlier command of
  the same CI job installed the package, the command names it, or no install was seen. When
  several packages declare a script and the job installed just one of them earlier, that
  package is picked at `inferred`. Several remaining candidates give `ambiguous`. A `pip install` by name also
  links at `inferred`. The target is the script's function. Check subcommand dispatch, script
  paths, `python -m`, `uv run` and calls between scripts in the source.
- **`(absent at the indexed commit)`.** The call names a path with no indexed CI provider at
  the indexed commit, or, with `: the file is no reusable pipeline there`, a pipeline without
  `workflow_call`. The file may exist at the caller's ref: report a call to check, not a
  broken one.
