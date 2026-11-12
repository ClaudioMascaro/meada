# 28. CI in the graph

- Status: accepted
- Date: 2026-11-12

## Context

The 1.3.0 feedback (item 7) showed that a reusable GitHub Actions workflow called from many
repositories is invisible as a contract. Its `uses:` lines are config values only, a hand-declared
`[[resource]]` gives one repository-level edge per repository, and `trace` cannot start from it.

Today Meada reads `.github/**` as config values (ADR 0025), derives directionless uses from them
(#661, ADR 0026), reads three image builder forms (`docker/build-push-action`,
`docker/metadata-action`, `ko`) and accepts `[[resource]] via` on their keys (#641). It reads no
call, no job, no step and no `permissions`.

This ADR records how CI configuration enters the graph: its nodes, the contract that carries a
call between files and repositories, and what CI gives to the kinds and mechanisms that exist. v1
reads GitHub Actions only. The decisions come from the grill of 2026-11-12. ADR 0002 holds:
indexing uses no network and no model, so Meada fetches no action and resolves no Git ref on a
host, and a provider is a file in the local checkout of a peer.

## Decision

### Model

- Pipelines, actions, CI jobs and CI steps are IaC nodes (ADR 0025) of a new format, GitHub
  Actions. Identity, file, span, config values, references and `context` come from the node model.
- One new contract kind, `ci`, carries only the call from one file to another, across
  repositories or inside one. Its sides sit on IaC nodes, as the alert kind's do.
- Every other contract CI takes part in belongs to the kind that exists (package, image, resource,
  channel, HTTP), with the CI node as the site of its side.

### Nodes

- A file under `.github/workflows/` that holds `jobs` is a pipeline, of type `workflow`. A file
  named `action.yml` or `action.yaml` that holds `runs` is an action, of type `action`. When both
  names exist in one folder the action is `action.yml`, the one the runner loads.
- The format's reader sits in the IaC module, on the bounded event reader of ADR 0025. Only the
  first YAML document of a pipeline or an action file is read.
- An address is the key path of the block in the grammar of ADR 0025 ("Key path"), as for Compose
  and Serverless nodes. A CI job (type `job`) is `jobs.build`, and `jobs["publish-prd"]` when its
  id is not a plain identifier, quoted as in every key path. A CI step (type `step`) is
  `jobs.build.steps[2]` or `runs.steps[0]`. The file node's address is `.`, the root key path, and
  a query names it by its file path.
- The identity is `<repository identity>::<file>::<address>`, as for any IaC node.
- Each pipeline file and each action manifest is one evaluation unit. References stay inside it:
  a pipeline to its CI jobs, a CI job or a composite action to its CI steps, a CI job to the CI
  jobs its `needs` names.
- A CI step's `id`, `name` and `uses` are searchable and are selectors (`steps[id=login]`), never
  identity.
- `strategy.matrix` does not multiply nodes: a CI job with a matrix is one node and one consumer.
- `search` finds CI nodes with the kind `ci`, and the MCP tool's kind list gains the value. The
  kind `iac` keeps its answers and the default search includes no CI node.
- `meada status` counts CI nodes among the IaC nodes it already counts.

### The CI contract

- The identifier is `ci::<repository identity>/<path>`: the repository identity of ADR 0005, then
  the path the call names. That is the file of a reusable pipeline, the folder of an action, and
  nothing for an action at the root of its repository. Owner and repository are lower-cased as the
  identity is; the path keeps its case.
- No segment names the CI system. Repository and path already name one file, and a provider must
  build its identifier alone at index time.
- A `uses` never writes a host. The host is the one of the consumer's repository identity. A
  consumer in a repository without `origin` links only its local calls.
- The provider is the file node of a reusable pipeline or of an action. A pipeline is reusable
  when its `on` holds `workflow_call`, in any of the three shapes `on` takes: a string, a list, a
  map. A pipeline without it holds no provider side.
- The consumer is the CI job whose `uses` names a reusable pipeline, or the CI step whose `uses`
  names an action. `uses` holds no expression, in a job or in a step, so a call is always literal.
- A consumer records its node, the Git ref and the keys of `with` and `secrets` as written
  (`secrets: inherit` as that word).
- The kind implements the contract kind interface that exists and links at link time, by
  identifier, across the repositories of the workspace.

| Form of `uses` | Where | Reading |
| --- | --- | --- |
| `<owner>/<repo>/.github/workflows/<file>@<ref>` | job | the peer `<host>/<owner>/<repo>`, `exact` |
| `<owner>/<repo>[/<folder>]@<ref>` | step | the peer `<host>/<owner>/<repo>`, `exact` |
| `./.github/workflows/<file>` | job | the own repository at the same commit, `exact` |
| `$/<path>` | job, step | the repository of the file, also inside a reusable pipeline or a composite action, `exact` |
| `./<folder>` | step | the workspace of the CI runner: see below |
| `docker://<image>` | step | no CI contract |

A step's `./<folder>` resolves against what the CI runner checked out, not against the repository
of the file. It links `exact` to the own repository only in a pipeline that is not reusable and
whose job checks out its own repository at the default path. In a reusable pipeline, in a composite
action, and in a job with no checkout or whose checkout names `repository` or `path`, it is
unresolved.

An unresolved consumer holds no candidate. It is an open item as any kind's unresolved consumer
is, with the cause in its reason.

### Git ref

- The Git ref is an attribute of the consumer, never part of the identifier: one contract per
  provider file, whatever refs its consumers name.
- The reason of a link reads `@<ref> (indexed <short commit>)`. When the ref is a full commit id
  (40 or 64 hexadecimal digits) that differs from the provider's indexed commit, the reason ends
  with `, ref mismatch`. A branch or a tag is shown as written, with no flag. A local call and a
  `$/` call write no ref and carry no such text.
- The level of a link does not depend on the ref. Every comparison with the provider's file reads
  the indexed commit.
- Meada never resolves a ref with the local git of the peer: the answer would depend on what the
  clone last fetched.

### External files, orphans and absent files

- A `uses` whose repository is not in the workspace is external. It holds no side, so it is never
  an orphan nor an open item, in `status`, `context` or `search`.
- A reusable pipeline or an action with no consumer is an orphan under the scope rule of ADR 0026:
  only when the scope holds at least one provider and one consumer of the kind.
- A consumer whose repository is in the workspace and whose path names no provider there is marked
  absent at the indexed commit, on the side, never broken: the file may exist at the ref. A folder
  that holds only a `Dockerfile`, a valid action for GitHub, is no provider in v1 and raises no
  such mark: the mark needs the folder to be missing, or to hold no action file and no
  `Dockerfile`.

### Queries

- `context` on a provider lists every consumer of the workspace with its repository, file,
  address, Git ref, keys and, for a CI job, declared permissions.
- `trace` accepts a CI contract identifier, a CI node or a pipeline's file path as its start.
- Upward, `trace` goes from a provider to its consumers (a link), from a consumer to the pipeline
  or action of its file (a reference), and on to that file's own consumers when it is a provider.
  Downward it goes the other way.
- This needs one walk that follows links and references together across repositories. Today the
  shared walk follows upward only a consumer that is a call or a declarative client, and
  references are walked inside one repository.
- GitHub caps a chain of reusable pipelines at ten levels. The walk keeps its own cycle detection
  and token budget and adds no cap.

### Declared permissions

- Each side is indexed in its own repository. For a consumer: the block that governs the CI job,
  its own, else the pipeline's, else `not declared`. A block is closed: a permission it does not
  name is `none`, so a job's block does not add to the pipeline's. For a provider: the block that
  governs each of its CI jobs, by the same rule.
- `read-all`, `write-all` and `{}` are read as the blocks they stand for. The levels order as
  `none`, `read`, `write`.
- The comparison happens at the link. When a CI job of the provider declares a permission above
  what the consumer's block holds, the reason ends with `permissions mismatch` and names the
  permission and both levels.
- A side that is `not declared` is shown so and is not compared: the default lives in settings
  outside the repository.
- Only a link whose consumer is a CI job compares permissions: an action declares none.
- Meada never says a permission is granted. The repository default, a pull request from a fork and
  the consumers further up can only reduce, so no mismatch proves nothing and a mismatch is enough.
- The flag states the fact, not its consequence, and it is about the provider's indexed commit.

### The `run` script and the command table

The `run` script of a CI step is read through the command table, under one gate:

- **Shell.** `bash`, `sh` or none declared.
- **Unit.** Each line of the script, a trailing `\` joined. A script that holds a heredoc is not
  read.
- **Command.** The first word of a line, or the one after `&&` or `;`. Text inside `echo`, a string
  or a comment never gives a side. A command inside `if` or a loop counts: the step may run it.
- **Word.** A literal, a quoted literal, or an expression that static evaluation resolves.
- **Unresolved.** `secrets.X`, `vars.X`, `github.*`, `matrix.*`, a `$VAR` no literal `env` gives,
  `$(...)` and backticks. In part of a name they make a pattern; as the whole name they make an
  unresolved side that names the variable. In an option's value they change nothing.

A script the reader cannot hold whole (past the value length ceiling) is not read. The command
table evaluates no shell: an assignment, `cd`, a function and a called script are not followed. A
command outside the table gives no side.

The rows of v1:

| Command | Side |
| --- | --- |
| `pip install`, `pip3 install`, `python -m pip install`, `uv pip install`, `pipx install`, `uv tool install` | the CI step is a consumer of `package::pypi::<name>`, the range as written |
| a command that is a console script of a package of the workspace | the CI step starts that program (below) |
| `docker push`, `docker buildx build --push` | the CI step is the site of an image builder side |
| `ko build`, `ko publish` with `--bare` | as today, with the CI step as the site |

### Programs of the workspace

- A package manifest keeps the name of each console script, beside its module and function. The
  stored summary changes, so the schema version rises and indexes rebuild (ADR 0004).
- A CI step whose command is that name links to the program entry point the script names, through
  the package contract: no new kind. The arguments are recorded as text.
- `trace` upward from the program reaches the CI step, its job, its pipeline and that pipeline's
  consumers; downward from a CI job it enters the program's process.

| Evidence | Level |
| --- | --- |
| An earlier line or step of the same CI job installs the package that declares the script | `exact` |
| The command names the package itself (`uvx <package>`, `pipx run <package>`) | `exact` |
| No install; one package of the workspace declares a script of that name | `inferred` |
| Several packages of the workspace declare a script of that name | `ambiguous` |
| No package of the workspace declares it | no side: an external command |

An install by name follows the package kind's rule as it stands: one provider of that name in the
workspace is `exact`, whatever index serves it.

### Expressions

A `${{ }}` expression is evaluated statically where a literal gives its value (an `env` of the
step, the job or the pipeline), and is otherwise a pattern or unresolved, as a Terraform expression
is under ADR 0025. `secrets.X` and `vars.X` have no value in the repository (ADR 0005): they stay
unresolved and name the variable, and are never matched by name across repositories. v1 reads no
provisioner for them.

### The CI reader owns its files

In a file that became a pipeline or an action node:

- A structured identifier in a value (an ARN, a queue URL, `s3://`, `gs://`) stays an `exact` use,
  and its site becomes the CI step or CI job that holds the key.
- A typed final key (`queue`, `topic`, `bucket`, `table` and the Azure and GCP aliases) gives no
  use. A key of `with` belongs to the vocabulary of the action or the reusable pipeline, not to
  the cloud's. Such a use returns per action, with a meaning and a direction, through the table of
  known actions.
- A role ARN in `with` is a grant, never a use, even if IAM enters the type table later.
- The image builder forms read today (`docker/build-push-action`, `docker/metadata-action`, `ko`)
  keep their rule; their site becomes the CI step, with slice F2.
- Every scalar stays a config value, `[[resource]] via` on a key stays valid, and the other files
  of `.github/**` and of other CI systems stay under the generic rule.

This removes `inferred` uses and orphan lines that came from pipelines. These are the only
existing answers that change, and the stability document does not freeze which links Meada finds.

The rule on a file a reader owns is the one #796 decides for ADR 0026 (its cause 4): a document
that the reader of a contract kind recognizes is not read by the key-name rule, and a structured
identifier in it is still an `exact` use. #796 also says that IaC files keep the key-name rule,
because an IaC reader takes nodes out of them and gives no meaning to their other values. The
GitHub Actions format is the exception to that statement: its reader gives a meaning to the keys
of `with`, the inputs of the action or the reusable pipeline the node calls. #796's own amendment
was not on main when this ADR was accepted, so this ADR carries the sentence alone until that
amendment repeats it. #796's level rule (#797) already keeps an `inferred` use out of orphan
lines; this section also removes the use itself. Its role `grant` (#798) is for IAM policy
statements: a role ARN in `with` gives no side here, whatever that role becomes.

### Other CI systems

v1 reads GitHub Actions. Four points keep the model fit for GitLab's `include` and Azure
Pipelines' templates:

- A consumer is the node where the reference is written, at any level: a step, a job or the file.
- A reference has optional parts: a host (the consumer's when absent), a repository, a path or a
  name, a Git ref (absent means the provider's default) and the keys it passes.
- A link finds its provider by repository and path in the peer's index. A mark in the provider's
  file is a GitHub Actions fact, not a rule of the kind.
- Glossary terms carry no vendor name; node types follow each format.

### Export, server mode, console and skill

- The sides and links enter `providers`, `consumers` and `links` as every kind's do, and a CI node
  that holds a side enters `iac_nodes`. Git ref, keys and declared permissions are optional
  fields, each left out when empty. The export format stays 1 under ADR 0004's rule.
- The console graph and query page show the kind through the shared rows, with the same answers
  as the CLI and MCP.
- In server mode and named workspaces the new rows follow the existing grants, generations and
  partial export.
- The shipped skill and the documentation in the three languages (the capabilities page, the user
  guide, the changelog) change in the PR of each slice. The user guide keeps the hand declaration
  (`[[resource]]` with `via` on the calling key) for the CI systems Meada does not read, and a
  workspace that declares its pipelines that way keeps those sides and links at `declared`,
  beside the CI contract.

### Limits and safety

- The reader adds its row to the hostile probes of ADR 0025: a pipeline at the per-file size
  ceiling with deep nesting, thousands of steps and one huge `run` script is read in bounded time
  and size.
- Nodes per evaluation unit and references per node are the named ceilings that exist.
- A path read from a pipeline (a local `uses`) stays inside the repository root, by the path rule
  that exists.

### Slices

| Slice | Holds | Accepted by |
| --- | --- | --- |
| F1 | nodes and references; `search` kind `ci`; the CI contract with Git ref and keys; `trace` through an intermediate pipeline; external files; orphans and absent files; the CI reader owning its files; export, console, server mode, skill, docs | the reproduction of the feedback's item 7 (#816) |
| F2 | the command table's gate; `pip install` as a package consumer; programs of the workspace; `docker push`; builder sites on CI steps | a chain of three contracts: an HTTP route, the package whose program calls it, the reusable pipeline that runs the program, and the CI jobs that call the pipeline |
| F3 | declared permissions and `permissions mismatch` | a consumer without the permission its provider declares; a consumer with `not declared` |

F1 and F2 ship in the same minor release, F3 in that one or the next. Each slice is additive.

Afterwards, by value over noise and reordered by the measured precision:

1. `secrets.X` and `vars.X` against their provisioners (`github_actions_secret`,
   `github_actions_variable` and their `organization` and `environment` forms), and drift of
   `with` against `inputs`.
2. The evaluator for `${{ }}`: an input's default as an overridable value, `matrix` as a pattern,
   `outputs` and artifacts as references.
3. The table of known actions and the mapping of a private action in `meada.toml`; the channel of
   `repository_dispatch` and `workflow_dispatch`.
4. The image runner in `container` and `services`; a CI job as a workload with its `env`.
5. The command table's other rows (`aws s3`, `aws sqs`, `curl`), a client wrapper across
   repositories and `changes: uncommitted`.

Out, with no slice: a publisher role for packages (`npm publish`, `twine`), alerts, provider
documents, a trigger of `on` as an entry point, duplicates of CI jobs, effective permissions,
console scripts of ecosystems other than PyPI, a link across hosts (GitHub Connect),
`Dockerfile`-only actions as providers, and other CI systems (GitLab, Azure Pipelines).

### Ceilings

Each enters `docs/agents/ceilings` with its slice:

- Only the first YAML document of a pipeline or an action file is read.
- A CI step's identity is positional: a step inserted above renames the ones below.
- A CI job with a matrix is one consumer.
- Only a ref of 40 or 64 hexadecimal digits is a commit id; a branch of that shape reads as one.
- A file absent at the indexed commit may exist at the ref.
- A folder that holds only a `Dockerfile` is no provider.
- A call from one host to an action on another (GitHub Connect) has no link.
- The command table evaluates no shell: a variable the script assigns, `cd`, a function and a
  called script (`./deploy.sh`, `make`, `npm run`) are not followed.
- A CI runner on Windows defaults to `pwsh`; the line is read as `bash`.
- A command links to its script's function, not to the handler of a subcommand.
- An install outside the CI job (a container image, a setup action, a self-hosted CI runner)
  leaves the program link `inferred`.
- A name the workspace provides wins over a public package of the same name.
- Only PyPI console scripts are programs with a command name.
- Declared permissions are not effective permissions.

### Amendments to ADRs 0005, 0025 and 0026

This ADR amends three ADRs, each carrying its dated amendments (2026-11-12):

- ADR 0005: its contract kinds gain `ci`, and `secrets.X` and `vars.X` fall under its rule on
  environment variables.
- ADR 0025: its IaC formats, node addresses, references, evaluation units, static evaluation and
  search kinds gain GitHub Actions.
- ADR 0026: its use sides, image builder forms, queries, orphans and export change in pipelines
  and actions as the sections above say.

## Considered options

- **A curated resource kind for pipelines.** Rejected: an external action would be an orphan and
  `trace` would end at the use.
- **A package ecosystem with a language pack.** Rejected: it opens `.github/**` to packs and
  manifests, a pack disputes `.yml` with config values, and workloads, patterns, overridable
  values and orphans stay out, being IaC node machinery.
- **The Git ref in the identifier.** Rejected: the consumers of one file would spread over
  several contracts, and the checkout would have to match a ref Meada does not resolve.
- **A segment for the CI system.** Rejected: it repeats the host in the common case, and a
  provider that does not know which system includes it could not build its identifier.
- **A consumer as a `<file>#<key path>` location with no node.** Rejected: a location ends a
  `trace`, so a chain would stop at the first intermediate pipeline.
- **A name-stable identity for a CI step** (`<job>::<action>#<ordinal>`, as a call site).
  Rejected: a second address grammar beside the key path its values use.
- **Effective permissions.** Rejected: the default, the fork rule and the consumers further up
  are not in the files.
- **A shell reader.** Rejected: the largest reader and the largest risk of a false side.
- **Keeping the typed key rule in pipelines.** Rejected: the noise stays, and the CI reader and
  the generic rule would give two sides for one use.

## Not confirmed at draft time

- What GitHub does when a called pipeline declares more than its consumer grants. The
  documentation says the permissions "can be only downgraded"; that the run is refused at start is
  from memory.
- Whether GitHub still accepts an abbreviated commit id in `uses`. If it does, it reads as a name.
- The spelling `template: <file>@<alias>` of Azure Pipelines; its schema confirms the alias and
  the fields of a repository resource.
- The address `.` for a file node: it follows the key path rule and was not put to the maintainer.
