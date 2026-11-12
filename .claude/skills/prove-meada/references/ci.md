# A9: CI in the graph, slice F1 (spec #820)

The F1 flows of #820's "End-to-end seams": CI nodes, the CI contract with its Git ref and keys,
`trace` through an intermediate pipeline, external files, orphans, absent files, `ref mismatch`
and the files the CI reader owns. `ci.py` drives them; F2 and F3 add theirs here (#830).

```bash
python3 $S/ci.py --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" --run "$RUN/ci" --http
python3 $S/ci.py --corpus --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" --run "$RUN/ci-corpus"
python3 $S/ci.py --selftest   # no Meada: the cases, their controls and this driver's reading
```

## Inputs

`ci.py` writes five workspaces of sibling repositories under `github.com/acme`, each with a `hub`
that lists the others as peers, and copies them once per binary (`input/`, `head/work/`,
`base/work/`). Nothing comes from a feature test, a golden or the private workspace of #816's
report.

| Workspace | Holds |
|---|---|
| `calls` | the reproduction of #816 (`workflows`, `wrapper`, `app-a`, `app-b`, `app-c`, the callers with `actions/checkout` and `docker/build-push-action` steps and #816's hand declaration), `actions` (a root action, a folder with both file names, one with `action.yaml` alone, a `Dockerfile`-only folder) and `forms` (one case per form of `uses`, the five positions of a step's `./<folder>`, the three shapes of `on`, a pipeline without `workflow_call`, the owner and path case, the config uses of #828) |
| `refs` | a provider and a consumer pinned to the provider's commit, to another 40-digit id, to a 64-digit id and to an abbreviated id |
| `orphan` | a reusable pipeline nobody calls beside one that is called |
| `orphan-control` | a reusable pipeline nobody calls in a scope without consumers |
| `solo` | a repository without `origin` that calls its own pipeline and a peer's |

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
| External files | new | #820 | no row of any export, no line of any `meada status`, no answer of `search` kinds `ambiguous` and `unresolved` names one; a control reads their `uses` lines as config values |
| Orphan, absent file, `ref mismatch` | new | #820, each with its control | `orphan ci::` lines; `absent at the indexed commit` in `context` on the consumer; the end of the reason |
| The files the CI reader owns | existing | the base binary; allowed: the typed-key uses removed, the site of an `exact` use | `uses` rows of `forms` on both binaries |
| `uses` lines as config values, `[[resource]] via`, `search` kind `iac`, the default search | existing | the base binary | byte-equal answers from the hub |
| Every other row of every export | existing | the base binary | `parity`: all sections but `ci::` rows, rows on a CI node and uses read from a CI file |
| MCP over HTTP | copies stdio | the same binary's stdio answers | the hub's answers, byte for byte |
| The pinned corpus | existing, plus five links read here from the source | the base binary; the source at the pin | `--corpus` |

The names of the consumer row's optional fields (`ref`, `with`, `secrets`) are the driver's
`REF`, `WITH` and `SECRETS`: #824 names them in the export and changes them here if it differs.

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

## Not covered

- GitHub Enterprise Server (a call linked on the consumer's own host): no workspace on another
  host.
- The console graph and query page: they copy the CLI and MCP through the shared rows, and the
  console drivers of the Scope table run on the paths that reach them.
- Hostile pipeline files and freshness after an edit: `tests/it` holds them (#823, #824).
- Declared permissions and the `run` script: F3 and F2 (#830).
