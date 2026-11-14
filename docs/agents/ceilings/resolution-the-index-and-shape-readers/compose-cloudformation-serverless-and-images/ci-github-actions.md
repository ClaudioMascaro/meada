# CI: GitHub Actions

- `iac/actions.rs` (#823, ADR 0028): a pipeline or an action file reuses `NODES_MAX` (the file,
  then its CI jobs in the order of their ids, each with its CI steps; past it the rest of the file
  is left out), `REFERENCES_MAX` (per node: a pipeline of more than 64 CI jobs, or a CI job whose
  `needs` and CI steps pass 64 together, keeps the first ones, `needs` before steps; a CI step past
  it is still a node `search` finds, with no reference from its CI job) and `ROOT_STORED_MAX` (per
  file: addresses, names, `id` and `uses`, and references; a CI step's address repeats its CI
  job's id). A `name`, an `id` or a `uses` longer than `config_values::VALUE_MAX` is kept neither
  as a name nor as a selector. A CI step's identity is positional (`jobs.build.steps[2]`): a CI
  step inserted before it renumbers it; `id`, `name` and `uses` are selectors, never identity, and
  a selector's value is compared whole and case-sensitively. A `strategy.matrix` multiplies
  nothing: a CI job with one is one node. Only the first YAML document of a file is read. A file
  past `lang::MAX_BYTES`, one that is not UTF-8 and one the event reader refuses give no node. A
  CI job or a CI step that came by a YAML alias has no lines of its own: a step sits at its CI
  job's first line and a job at line 1, since the reader hands a location only where a mapping is
  written. A pipeline is a file directly under `.github/workflows/`, and with `action.yml` and
  `action.yaml` in one folder the action is `action.yml`, whatever each of them holds. The
  extension is `.yml` or `.yaml` in lower case. The file node spans the whole file, the documents
  after the first included. A CI job id written twice is the last block, while the config values
  keep both. A merge key (`<<`) is not expanded: what it would bring gives no CI step. The config
  values have their own budget for what aliases repeat: past it the file counts as unreadable
  there and stores no value, and its nodes stay, with `values: none` (found by #823's
  verification).
- `iac/actions/run.rs` (#831, ADR 0028): the command table evaluates no shell. A variable the
  script assigns, `cd`, a function call and a called script (`./deploy.sh`, `make`, `npm run`)
  are not followed; the lines of a function's body are read as lines of the script, but for a
  body on the definition's own line and a subshell body (`f() ( ... )`). A command
  is the first word of a line or the one after `&&` or `;`, also after a bare `if`, `then`,
  `else`, `elif`, `do`, `while`, `until`, `!` and `{` (quoted, escaped or given by a variable,
  such a word is the command); a prefix (`sudo`, `env`, `time`, an assignment) makes it another
  command, and what follows `||`, a pipe, `&`, a redirection or a parenthesis is not read up
  to the next line, `&&` or `;`. A parenthesis that opens a command is a subshell, `((` an
  arithmetic command with no command inside, and any other one (an array, a function's `()`)
  is passed over to the one that closes it. The name of a function before its `()` and a
  pattern of `case` are no command (#832's verification); the command on a pattern's own line
  (`a) cmd ;;`) is not read, and after `;&` a pattern that opens with `(` is taken for a
  subshell. Between parentheses (an array, a command
  substitution) a `#` after a blank or the parenthesis starts a comment to the end of its line and a `${...}` is
  passed whole; a backtick there is no quote, and a `#` after a blank inside the parentheses of
  a `[[ =~ ]]` pattern is taken for a comment, which may leave the script unread. A heredoc (`<<`, also the left shift of an
  arithmetic expansion or command) leaves the whole script unread, and so does a quote, a
  parenthesis, an expansion or an expression left open: no side and no open item. The script is
  the stored config value, so one past `config_values::VALUE_MAX` (1024 bytes) is not read. The
  shell is the CI step's `shell`, else `defaults.run.shell` of its CI job, then of the
  pipeline: only `bash`, `sh` or none declared is read, never a template (`bash -e {0}`) or an
  expression. A CI runner on Windows defaults to `pwsh`: `runs-on` is not read, and the script
  is read as `bash`. `${{ env.X }}`, `$X` and `${X}` take the literal string of the nearest
  `env` (CI step, CI job, pipeline) that defines the name (a number or a boolean stays open),
  when it holds only letters, digits, `.`, `_`, `=`, `-`, `/`, `:` and `@` (the last three
  since #833, for an image name), and when neither the script nor
  another script of the file that names the environment file (`GITHUB_ENV`, `github.env`,
  `github[`) mentions the name other than by reading it. A line that names the environment
  file and is no `echo` of `NAME=` (#836: a file appended to it, `cat vars.env >> $GITHUB_ENV`;
  a name written through a variable, `echo "$NAME=x" >> $GITHUB_ENV`; `printf`; a backslash,
  which `sh` prints as a new line; a `{ }` group
  or a continued line redirected to it; two writes on one line) may write a name the script
  does not show, so every literal `env` of that file stays open, whatever the order of the CI
  steps: such a file gives its `pip install $PKG` no side where a narrower reading would. A
  write to it by an action, a called script or a script past the length ceiling is not seen,
  and neither is the environment file named through another variable (`ENV_FILE: ${{
  github.env }}`) or a second name the expansion of a value carries on a new line: the literal
  `env` of that name still resolves, and can link `exact` to a package the CI step does not
  install. An alias (`env: *vars`) is the map it repeats. A merge key (`<<: *vars`) is not
  expanded: an `env` that holds one gives no literal for any name and hides the `env` above
  it, so its `pip install $PKG` has no side where the merged map would name the package.
  The literals read add at most the
  script's own length and `VALUE_MAX`; past it a variable stays open.
- `ci.rs` (#824, ADR 0028): a CI contract links what is written, and Meada resolves no Git ref.
  Only a ref of 40 or 64 hexadecimal digits is a commit id: an abbreviated one reads as a name
  and never gets `ref mismatch`, and a branch or a tag of that shape reads as a commit id and
  gets it. A CI job with a `strategy.matrix` is one consumer. A `uses` writes no host, so a call
  from one host to another (GitHub Connect) has no link, and a repository without `origin` names
  no peer. Owner and repository are lower-cased; the path is compared as written, so a call that
  differs from the file's name by case is a consumer with no link. The keys of `with` and
  `secrets` are kept sorted, not in file order, each up to `config_values::VALUE_MAX` and within
  the file's `ROOT_STORED_MAX`; a `secrets` that is neither a mapping nor the word `inherit` gives
  none. The indexed commit is the checkout's `HEAD` even when the checkout is dirty, read with
  one `git` call per repository that holds a reusable pipeline on each query; a provider whose
  repository records no commit reads `@<ref> (indexed commit unknown)`, with no flag: none made
  yet, or a server-mode index, whose commit `meada index --workspace` records only for a clean
  checkout (#512: `git status` lists nothing, not even an ignored file such as a `.meada/` left
  there), so over such a checkout MCP over HTTP differs from stdio in the reason and flags no
  `ref mismatch`, and `index --workspace` says nothing of it. A `uses` that holds a
  control character names no pipeline, and a key of `with` or `secrets` that holds one is left
  out: GitHub takes neither, and each would break a line of `context`. On that line a key named
  `inherit` reads like the word, and a key that holds `, ` or `; ` like several; `meada export`
  tells them apart. On a CI node the side lines of every kind come above its values, the last block of
  `context`, so the `budget` cuts the values first; on any other IaC node they close the answer,
  as before. A path and address that two repositories of the workspace hold fit two IaC nodes,
  even asked at the endpoint of one of them: the answer asks to pick one by its identity. A
  `uses` with two `@` has the ref after the first. `trace` (#825) follows a CI call only between CI nodes of the
  workspace and adds no cap for GitHub's ten levels of nesting: a chain ends at a cycle mark or
  at the budget, and the walk from an IaC node has no depth limit of its own. It reads the
  whole chain before the budget cuts the answer, so a chain of thousands of pipelines costs
  seconds. A CI job past the 64 references of its pipeline (`REFERENCES_MAX` of
  `iac/actions.rs`) has no file node above it: `trace` up ends at that CI job, and down from
  the pipeline never reaches what it calls. A contract identifier is matched as written: one
  with the owner in another case, or a trailing `/`, starts nothing.
- `ci.rs` (#826, ADR 0028): a CI step's `./<folder>` is read in the CI runner's workspace, which
  Meada knows to be the file's repository in one position only: a pipeline that is not
  reusable, after an `actions/checkout` step of the same CI job, with no `actions/checkout`
  before the CI step that names `repository` or `path` in `with` (the key in any case, as
  GitHub reads an input). It is unresolved in a reusable
  pipeline (whatever other triggers it has), where a checkout is of the caller's repository, and
  in a composite action, which runs in its caller's workspace; with no checkout before it; and
  after a checkout that names one of the two keys, also when the key names the own repository (`${{ github.repository }}`) or a
  path the folder is not under. Only `actions/checkout` at any ref counts, owner and name in
  any case: a fork of it, a wrapper action or a `git clone` in a `run` is no checkout, a
  checkout under an `if` counts whatever its condition, and a
  step that removes the files afterwards is not seen. A checkout's `ref` is not read: the link
  is to the own repository at its indexed commit. An unresolved CI step holds no candidate and
  `meada decide` offers nothing for it. A decision written by hand for it (`kind =
  "dynamic-url"`, its `target` a CI provider) is accepted for any CI provider of the workspace,
  an action or a reusable pipeline alike: it adds a `decided` link that `trace` and the
  provider's `context` follow and closes the item, while `context` on the CI step still says
  `consumes (unresolved)`. The export row of an unresolved CI step holds no `with` and no
  `secrets`: they are copied to a consumer with a CI contract only. On the pinned corpus (#835)
  all 32 unresolved CI steps are of this kind, 10 of them after a literal checkout of the own
  repository into a named `path` (`./<path>/<folder>`), which the value of `path` would resolve.
  `$/<path>` reads on every host, though GitHub Enterprise Server does not have it. A path is
  normalised by the path rule of `iac::joined` in every form (`.`, `..`, inner and trailing
  empty segments), which GitHub may refuse in a call by owner and name; one that leaves the
  root, or starts with an empty segment (`.//x`), holds no side. A
  CI step that names a pipeline file and a CI job that names anything else hold no side, so a
  CI job's `./<file>@<ref>` holds none; a CI step's `./<folder>@<ref>`, which GitHub calls
  invalid, reads as a folder whose name holds the `@`. A `$/<path>` that holds an `@` names
  nothing: GitHub refuses a Git ref on that form. A `uses` that holds
  `${{` names nothing: GitHub evaluates no expression there. A call by owner and name is cut
  at its first `@` (`<folder>@v1@v2` reads the ref `v1@v2`), and a CI job's ref written
  `refs/heads/<branch>`, which GitHub does not allow, links as any other. An action in a folder named as a pipeline file
  (`.github/workflows/x.yml/action.yml`) provides nothing.
- `ci.rs` (#827, ADR 0028): the absent mark says what the index of the provider's repository
  holds, never what the Git ref of the call holds: Meada resolves no ref, so a file that exists
  only at the ref is marked, and one removed there is not. The index follows the checkout, a
  dirty one included, and the mark reads the same for a peer with no commit. A file Meada
  could not read (past `lang::MAX_BYTES`, not UTF-8, refused by the event reader) gives no node
  and reads as absent, and so do a path that differs from the file's by case and a path the
  walk skips though the commit holds it (a hidden folder outside `.github`, `node_modules`, an
  ignored path, a symlink). The second reason (`the file is no reusable pipeline there`) needs
  a pipeline's file node: a file under `.github/workflows/` with no `jobs` is no pipeline and
  gets the plain mark, and so does a CI step that names an action's file. A folder that holds
  a file named `Dockerfile` or `dockerfile` (the two names the runner builds an action from)
  and no action file is no provider, and a resolved call to it holds no side at all: no mark,
  no orphan line, no open item. `Dockerfile.dev`, `DOCKERFILE` and a `Dockerfile` the walk
  skips do not count, and the call is then marked absent. An unresolved `./<folder>` stays an
  open item whatever the own repository holds in that folder. The set of those folders is
  stored per repository (table `dockerfiles`), every folder of the walk, with no ceiling of its
  own. The mark reads the providers and the IaC nodes of the repository its contract names
  once per listed consumer, so `search` with the kind `contract` over thousands of marked
  consumers costs seconds. An orphan line needs a consumer with a contract in the scope: a
  marked consumer opens it, an external `uses` and an unresolved CI step do not, a provider
  consumed only by a call to itself is no orphan, and one whose only caller is an unresolved
  CI step is an orphan once another consumer opens the scope. A peer `meada.toml` names and
  the disk does not hold is outside the workspace: a call to it holds no side and no mark.
  `meada status` reports and does not index: a repository never indexed prints `not indexed`
  and no orphan line. The `absent` field of `meada export` follows the links of that same
  export: a consumer row of the kind with no link.
- `ci.rs`, `iac/actions.rs` (#834, ADR 0028): permissions are the declared ones, as the files
  of the checkout hold them (an uncommitted edit is read, beside the commit the reason shows).
  The default of a repository's or an organization's settings
  is not read, so `not declared` is unknown, never `none`, and a side that declares nothing is
  never compared. What GitHub does when a called pipeline declares more than its caller grants
  is not confirmed: `permissions mismatch` states the fact only, and no answer says a
  permission is granted or effective. `read-all` reads as `read` and `write-all` as `write` for
  every permission, whether or not GitHub offers that level for it. A block is read whole or
  not at all: one that holds `${{`, a level other than `none`, `read` or `write`, a name past
  `VALUE_MAX` or with a byte other than a letter, a digit, `-` or `_`, or any other shape reads
  as `not declared`, on the CI job's own block too (it does not fall back to its pipeline's). A
  name GitHub does not know is shown as written, and names are compared byte for byte. The provider's side is the
  highest level its CI jobs declare, each by its own block, else its pipeline's: a CI job past
  `NODES_MAX` is not read and not compared, and a CI job of the called pipeline that declares
  nothing adds nothing. A link to an action, or from a CI step, carries no permissions text.
  A block's named keys count in the stored size of the CI job's node, so a block of thousands
  of names over many CI jobs drops the jobs past `ROOT_STORED_MAX`, and `status` says nothing.
  A name or a `permissions` key written twice reads as the YAML reader gives it (the last
  one), where GitHub refuses the file; `1` and `true` read as names. The links are derived in
  one pass over the workspace on every query (#836, `workspace_links`): the CI nodes are looked
  up by site and the blocks of a called pipeline are folded once, so the cost is the CI nodes
  and the callers, never their product. That pass writes the site of every CI node of the
  workspace once it meets a call to a provider the workspace holds. A `with`
  key is shown as written, so one may hold the text `permissions:`; the true block is the last
  `permissions:` of the line, before the absent mark, and the export holds it apart. A `uses`
  whose Git ref holds a space names nothing.
