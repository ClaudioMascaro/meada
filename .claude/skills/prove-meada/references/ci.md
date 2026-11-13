# A9: CI in the graph (spec #820)

The flows of #820's "End-to-end seams", one slice per run of `ci.py`:

- **F1** (the default): CI nodes, the CI contract with its Git ref and keys, `trace` through an
  intermediate pipeline, external files, orphans, absent files, `ref mismatch` and the files the
  CI reader owns.
- **F2**: the gate and the command table of a `run` script, the unresolved parts, programs of the
  workspace with their level, the builder sites and the chain of three contracts.
- **F3**: declared permissions and `permissions mismatch`.

```bash
python3 $S/ci.py --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" --run "$RUN/ci" --http
python3 $S/ci.py --slice f2 --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" --run "$RUN/ci-run" --http
python3 $S/ci.py --slice f3 --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" --run "$RUN/ci-permissions" --http
python3 $S/ci.py --corpus --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" --run "$RUN/ci-corpus"
python3 $S/ci.py --selftest   # no Meada: the cases, their controls and this driver's reading
```

## Inputs

`ci.py` writes the workspaces of its slice (`SLICES`) as sibling repositories under
`github.com/acme`, each with a `hub` that lists the others as peers, and copies them once per
binary (`input/`, `head/work/`, `base/work/`). Nothing comes from a feature test, a golden or the
private workspace of #816's report.

| Workspace | Holds |
|---|---|
| `calls` | the reproduction of #816 (`workflows`, `wrapper`, `app-a`, `app-b`, `app-c`, the callers with `actions/checkout` and `docker/build-push-action` steps and #816's hand declaration), `actions` (a root action, a folder with both file names, one with `action.yaml` alone, a `Dockerfile`-only folder) and `forms` (one case per form of `uses`, the five positions of a step's `./<folder>`, the three shapes of `on`, a pipeline without `workflow_call`, the owner and path case, the config uses of #828) |
| `refs` | a provider and a consumer pinned to the provider's commit, to another 40-digit id, to a 64-digit id and to an abbreviated id |
| `orphan` | a reusable pipeline nobody calls beside one that is called |
| `orphan-control` | a reusable pipeline nobody calls in a scope without consumers |
| `solo` | a repository without `origin` that calls its own pipeline and a peer's, whose pipeline nobody links to |
| `run` (F2) | `libs`, nine Python packages in folders (`PACKAGES`: two plain, one whose only console script has another name, one with a script, one with a script named like it beside another, two that declare the same script, the one package the pattern of the forms fits, one named like a variable of the forms) and `steps`: four generated pipelines of one CI job per case (`PROBES`) and a composite action. `run.yml` holds each row of the command table, each rule of the gate with its negative edge (`||`, a pipe, a command behind another), the arguments that are no name, the shells and the three places of a literal `env`; `defaults.yml` a pipeline-level `defaults.run.shell`; `levels.yml` each row of the level table; `forms.yml` each unresolved form as part of a name, as the whole name and in an option's value. A case of `levels.yml` may hold a helper step that only installs: a consumer of its package, keyed by `wanted()` beside its case |
| `chain` (F2) | the callers of `calls` (`wrapper`, `app-a`, `app-b`, `app-c`, unmarked), `workflows` whose `publish.yml` installs `core-sdk` and runs its console script, `core-sdk` (the package; its program posts to a base URL read from `CORE_API_URL`, a `base_url_env` of its peer) and `core-api` (the route it posts to, beside a route no CI step reaches) |
| `permissions` (F3) | `perm-provider` (reusable pipelines with a pipeline block, a job block, both, none, and two CI jobs with a block each; an action) and `perm-consumer` (one calling CI job per case of `CALLERS`, in a pipeline with a block and in one without) |
| `calls` again (F3) | its five callers of #816, the F3 columns of #820's table: three with a block of their own, one under its pipeline's, one `not declared` |

A case is one row of `CASES` and one node of a generated file, marked there by `# case: <id>` and,
on the next line, `# control: <id>`. The control is another case whose expected answer differs, so
an answer given without reading the case (nothing read, everything linked, one ref for all) fails
one of the two. `--selftest` fails when a case has no `control:` line, when its control expects
the same answer (it cannot fail) and when this driver's reading of the generated YAML disagrees
with #820's table of the four consumers of `publish.yml`.

## Answer keys

| Flow | Kind | Key | Check |
|---|---|---|---|
| CI nodes and references | new | #820, and `nodes()`: this driver's reading of the generated YAML | `search` kind `ci` by id, step `id`, `uses` and type; `context` on a pipeline, a CI job (`needs` both ways), a CI step, a selector and a composite action |
| Consumers of a provider | new | #820's table (`PUBLISH_CONSUMERS`) and `CASES` | the whole sets of `ci::` providers, consumers and links of each workspace; each link's reason (`@<ref> (indexed <commit>)`); the `ref`, `with` and `secrets` fields; `context` on `publish.yml` |
| `trace` through `wrapper` | new | #820 | up from the contract, the file path and a CI node; down from `app-c`; a control from `on-string.yml` |
| External files | new | #820 | no contract of any export section, no line of any `meada status`, no answer of `search` kinds `ambiguous` and `unresolved` from any repository names one; controls: their `uses` lines are config values, and the hub sees a peer's open item |
| Orphan, absent file, `ref mismatch` | new | #820, each with its control | `orphan ci::` lines; `absent at the indexed commit` in `context` on the consumer; the end of the reason in the export and in `context` |
| The files the CI reader owns | existing | the base binary; allowed: the typed-key uses removed, the site of an `exact` use and the side of its provisioning link; a role ARN is no use on either binary | `uses` rows of `forms` on both binaries |
| `uses` lines as config values, `[[resource]] via`, `search` kind `iac`, the default search | existing | the base binary | byte-equal answers from the hub |
| Every other row of every export | existing | the base binary | `parity`: all sections but `ci::` rows, CI nodes, unresolved consumers on a CI node and uses read from a CI file. Another kind's side or link sited on a pipeline file (an image builder) is the base's |
| MCP over HTTP | copies stdio | the same binary's stdio answers | every answer asked in the `calls` workspace, again at the same repository, byte for byte. The server indexes the seeded `input/` copy: server mode records a commit only for a clean checkout, and a reason names the indexed commit |
| The pinned corpus | existing, plus five links read here from the source | the base binary; the source at the pin | `--corpus` |
| F2: the sides of a CI step | new | #820's command table, gate and level table, as `PROBES` writes them beside each script; `wanted()` turns a case into the contracts its step must hold, may hold, builds and starts | per case, the whole set of consumer rows, image provider rows and links sited on its CI step (`fits`): an installed name links to its package's manifest at `exact`, a pattern to each package it fits and never at `exact`, a program to its entry point at the level of the table; the range as written; `search` kind `unresolved` by variable name, one name per form; `context` on a package lists the installing steps and no other step of a case; each helper step is a consumer of its package, and no CI node outside the cases and their helper steps holds a side |
| F2: the builder forms of today | existing | the base binary; allowed: the site, which becomes the CI step | the head's provider row equals the base's but for `site`; controls: the base reads each form at the repository, and reads no builder from `docker push` or `docker buildx build --push` |
| F2: the chain | new, over an existing link | #820's acceptance of F2; the base binary for the HTTP link | the HTTP links of the workspace equal the base's (control: the base links the program to the route); `trace` up from the route reaches the program, the CI step that runs it and the five calling CI jobs; `trace` down from `app-c` reaches both pipelines, the program and the route; control: `trace` up from the other route reaches no pipeline |
| F3: declared permissions | new | #820's rules and `declared()`: this driver's reading of the blocks it generated (`governing`, `held`, `short`), which `--selftest` holds equal to the `DECLARED` table | per case, the link's reason (`permissions mismatch` or its absence, the permission and both levels named, no other permission named), the consumer row's block, and the consumer's line in `context` on the provider (`not declared`, each declared name and level, the flag); a call to an action carries none; no answer says "granted" |
| MCP over HTTP, F2 and F3 | copies stdio | the same binary's stdio answers | every answer asked in each workspace of the slice, again at the same repository of one server per workspace (`http/<workspace>/answers.json`) |

Both binaries are required: half of these keys are the base.

## What #820 does not state

The driver had to choose where the spec is silent. Each choice is a guess until the ticket that
builds it lands, and that ticket changes the driver, never the reverse:

- The export's field names (`REF`, `WITH`, `SECRETS`, #824), an unresolved consumer as a
  `consumers` row with `contract: null`, the `orphan ci::` status line, the words of the
  absent mark (`ABSENT`) and the `absent` field of a marked consumer row (#827).
- The shape of an answer: one consumer per line in `context`, one hop per line in `trace`, the
  name `<file>::<address>` for a CI node, a type word (`job`, `step`) as a `search` query.
- Three cases. `job_not_reusable` carries the absent mark and `step_docker_only` holds no side
  at all (#827 settled both). `solo_remote` holds no side at all, and its peer's pipeline is an
  orphan in the scope seen from `solo`.
- F3 (#834 confirmed each): the consumer row's field is `PERMISSIONS`, and any shape
  that holds each declared name and level passes, so a shorthand (`read-all`, `write-all`, `{}`)
  is keyed by its flags alone; `flagged()` takes the flags in either order and fixes no wording
  beyond #820's (`permissions mismatch`, the permission, both levels as whole words). #834
  writes `, permissions mismatch: <permission> <the provider's level> against <the consumer's>`
  after `, ref mismatch`, the consumer's block at the end of its `consumer:` line, and the
  provider's side as one `permissions:` line under `provides`, which is not keyed.
- F2 (#831 to #833), where a row is recorded: the range (searched in every row that names the CI
  step), the unresolved variable (through `search` kind `unresolved`, by its bare name: `FLAVOR`
  for `vars.FLAVOR`) and a program's arguments (not keyed at all).
- F2, what a form gives: a pattern is spelled `package::pypi::acme-*-suite` on its consumer row
  and its links, as a computed coordinate of the package kind is; a CI step that starts a
  program may also be a consumer of its package (the program link carries the package's
  contract either way); `-t` and `--tag` both name an image, and two tags give two builder
  sides; an unresolved tag after a literal name leaves the builder side; `--index-url` and
  `--no-binary` take a value; an argument of a program that holds an expression changes neither
  its side nor its level; the variable an unresolved `github.*`, `matrix.*`, `$(...)` or
  backtick form names is not keyed (#820 names the variable for `secrets.X` and `vars.X`, and
  the maintainer decides the rest); an unresolved builder side has no keyed shape, so no case
  pushes `$IMAGE`.
- F2, the letter of the gate where shell semantics would run the command: nothing is read after
  `||` or a pipe, nor behind `sudo`. A later line or a later CI step that installs is no
  evidence: the program link stays `inferred`.
- What `packages` exports for a console script (the base parity shows it as a difference to
  judge), and the paths of the modules that build F2 (`CI_RUN` in `scripts/prove-ci` guesses
  `src/iac/actions/`, `src/package*` and `src/image*`).
- Four readings settled in the tickets: a call to a pipeline file without `on.workflow_call`
  carries the absent mark (#827); a step's shell is its own, else `defaults.run.shell` of the
  CI job, else of the pipeline, and a `pip install` argument that is no name (`-r`, `-e`, a
  path, a URL) gives no side (#831); `uvx <package>` and `pipx run <package>` start the console
  script named like the package, and with no such script the CI step is only a consumer (#832).

## The pinned corpus

In `infra.tsv`, flow `ci`, all Apache-2.0. `--corpus` exports each alone with both binaries
(predicted differences: CI rows for local calls, uses removed in CI files), then serves the three
as one workspace and saves every `ci::` link in `workspace/ci-links.json` with the count per level,
the input of #835's precision check.

| Entry | Why |
|---|---|
| `ci-slsa` (`slsa-framework/slsa-github-generator`) | a repository of reusable pipelines: ten with `workflow_call`, four of them intermediate (a builder calls a delegator), called locally and by full name from its own pipelines, and 28 composite actions |
| `ci-osv-scanner` (`google/osv-scanner`) | a caller across repositories: it calls `ci-slsa` at a tag and `ci-osv-action` at two branches, and holds composite actions in folders |
| `ci-osv-action` (`google/osv-scanner-action`) | a root action beside folder actions, and a pipeline that calls its own reusable pipelines by full name at a commit id other than the pin: `ref mismatch` in the wild |

## Done looks like

`checks.json` with every check passed, `receipt.md` saying so, and each raw export, status and
MCP answer under `head/captures/` and `base/captures/`. A script pass says checks passed; a fresh
checker reads `input/`, #820 and the captures and writes the verdict.

## Could fool you

- The driver and the product were written by the same agent in one session. The keys above come
  from #820 and from the generated YAML; a check that only repeats a format the product chose
  (one consumer per line in `context`) proves the format, not the fact. The fresh checker reads
  the raw answers.
- `meada status` is compared by hand: `status/<workspace>-<repo>.diff` holds the lines that differ
  from the base, and #820 predicts only CI nodes in the count and `orphan ci::` lines.
- A consumer's site is positional. A step added above a case moves its address, and the driver
  reads the index from the YAML, so a wrong index in the product shows as a missing and an
  unexpected consumer, not as one wrong row.
- The identity of a repository without `origin` is read from its own export: the driver cannot
  know it, so `solo_local` proves the link, not the identity.
- Three pairs (`a_publish`, `c_release`, `w_release`) are link against link at `@main`: they
  fail on a wrong provider only. The `refs` workspace and `b_publish_prd` catch one ref for all.
- `trace` through `wrapper` is presence on a line, not order, and `trace` down has no negative
  control.
- `--selftest` checks the cases and the reading, and runs each slice over an export with no rows
  to see it fail there. It never runs a check against an answer that should pass: a check that
  is wrong about the product's shape shows only in the first CI capture.
- F2 silence is a pair: a case that expects no side passes on any product that reads nothing, so
  it proves something only beside its control and the two whole-workspace checks.
- Each package of the F2 cases has one provider, in another repository than the CI steps: the
  cases never see the package kind's `inferred` for a package of the consumer's own repository,
  nor its `ambiguous` for two providers of a name.
- `echo` and the first line of `comment` are words outside the table: they catch a reader that
  scans for the text, no more. `chain_install` and `chain_run` are each other's control and
  differ by the side, not by a rule.
- F3 looks for names and levels as words in a line or a field, never for a structure: `read`
  inside another word passes. The cases use three permission names so a line that lists every
  permission fails the "no other permission named" half.
- The chain's `trace` checks are presence on a line of the repository, as F1's.

## Not covered

- GitHub Enterprise Server (a call linked on the consumer's own host): no workspace on another
  host.
- The console graph and query page: they copy the CLI and MCP through the shared rows, and the
  console drivers of the Scope table run on the paths that reach them.
- Hostile pipeline files, freshness after an edit, a second YAML document, a matrix, a file
  without `jobs` or `runs`, a pipeline in a subfolder and a local `uses` that leaves the
  repository: `tests/it` and the reader's unit tests hold them (#823, #824).
- An orphan action, a folder with neither an action file nor a `Dockerfile` beside one that
  exists, typed keys other than `bucket`, a use sited on a CI job or in an action file.
- Time, memory and index size (#835 measures them), and the count of CI nodes in `meada status`.
- F2: `pwsh` as the default on Windows runners, a script past the value length ceiling, a
  function, a called script, a subcommand's handler, an install outside the CI job, a workspace
  name that shadows a public package, console scripts outside PyPI manifests: ceilings of ADR
  0028, held by `tests/it` where they are built (#831 to #833).
- F2, rules of #820 with no case: an unresolved form in a builder's name or in
  `KO_DOCKER_REPO`, an `env` whose own value is unresolved, a `while` loop, `uv tool install` as
  the evidence of `exact`, and the weakest level in the footer of the chain's `trace`.
- F3: effective permissions (the token's defaults, an organisation's policy), `read-all`,
  `write-all` and `{}` on the provider's side or as a pipeline's block, a provider with one CI
  job declared beside one that is not, both sides `not declared` at once, a local call, a
  permission above the consumer's inside an intermediate pipeline of a chain.
