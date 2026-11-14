# Workload images (#662)

```bash
python3 $S/images.py --base-bin "$RUN/bin/meada-base" --head-bin "$RUN/bin/meada-head" \
  --run "$RUN/images" --http --public \
  --ui-bin "$RUN/bin/meada-head-ui" --ui-dir ui/dist
```

The answer keys are issue #662, its parent #642, and ADRs 0025/0026 at the exact required
base. Read the issues through the connected GitHub tools at run time. The driver imports no
product readers and uses no feature-test fixture or new golden as an answer key. Its neutral
repositories, SHA256 inventory, complete exports and every raw CLI/MCP/HTTP response remain
attached even on failure. All indexing runs on copies under the run's `work/` directory.

The generated inputs cover docker/build-push-action and docker/metadata-action, Compose,
Skaffold, checked-in ko publishing destinations, and Maven/Gradle Jib. They check Docker Hub
expansion, lowercase, tags/digests, literal and evaluated levels, both container lists, Helm's
singular `image.repository`, Kustomize images, direct Compose root/directory contexts, rejected
escaping and symlink contexts, bounded runner/builder patterns, ambiguous literal candidates
(including two image names built by one repository), pattern-to-pattern and interior-wildcard
rejection, candidate visibility, both trace directions, scope markers/orphans and same-size
image refresh. Deliberately contradictory synthetic process values must never resolve names.
A deploy-only server manifest must not follow a local peer to a hidden builder.
An entirely unresolved `${IMAGE}` must gain no literal evidence from Docker Hub expansion,
even beside a literal Docker Hub builder and a contradictory synthetic process `IMAGE` value.

The decision probe reads open-item identities from the running MCP search answer, writes only
manual `by = "human"` decisions in its disposable copy, and never calls a model. A Pod's
application and init image ambiguities must close independently. Two Kustomize roots sharing
the same workload path/address but overriding its image differently must retain separate
context, trace, candidate sets and replay: deciding one leaves the other unchanged. Removing
the decisions must restore the complete initial export. Optional consumer/link root fields
remain intact in all raw captures and same-mode parity.

`--public` fetches `boutique` at infra.tsv's existing pin. Its independent source key reads
both Skaffold documents: eleven application artifacts and the second document's loadgenerator.
The Kubernetes short-image workloads link to those builders; full Artifact Registry names in
the Kustomize base do not match short names by guess. Busybox init containers remain runners.
The chart's plural `images.repository` is not the singular Helm key, and its templates are not
rendered. The driver retains a full-field JSON delta and unified export diff against the exact
base, checks every existing provider/consumer/link row, and checks complete unchanged fields
outside the new image model. The checker audits every remaining delta against the source and
spec, including newly exported workload nodes and optional direct `executions`.

With both UI options, console query text and errors are compared to stdio in the same scope,
and each export is compared byte for byte to the same UI binary's server-mode CLI export;
no fields are removed. `images-graph.mjs` loads real exports in Chromium and retains rendered
contracts, builder locations, links with their levels, and the direct Workload code view with
screenshots. This supplements all-corpus base/stdio/HTTP/console parity and the existing viewer
page budgets when shared or launch/version code changes. Skipping the UI options leaves the
new graph flow unproved, even if the CLI and MCP checks pass.

A script pass is input to the independent checker's verdict. It does not establish authenticated
grant enforcement beyond the explicit server manifest scope; use the existing grants seams
when those modules change. Environment-variable binding to workload code remains a later flow.

## Combined image/AWS and literal ko regressions

When the baseline already supports #658, also drive the shared stored-row and trace seams:

```bash
python3 $S/images-mixed.py --base-bin "$RUN/bin/meada-base" \
  --head-bin "$RUN/bin/meada-head" --run "$RUN/images-mixed"
python3 $S/image-ko.py --head-bin "$RUN/bin/meada-head" --run "$RUN/image-ko"
```

These independent probes use #658/#662, parent #642, exact-base ADRs 0025/0026 and literal
neutral inputs. They import only existing transport/git mechanics, never product readers or
feature fixtures. Both require a new evidence directory and retain complete inputs, SHA256
inventories, oracles, exports and raw commands. A failed expectation exits 1; an unavailable
command/transport exits 2 with its error retained. An existing run is never overwritten.

`images-mixed.py` always drives CLI, persistent MCP stdio and MCP HTTP. One infrastructure
repository contains Compose image workloads and an SNS-to-SQS subscription; its peer both
builds the image and receives the queue, with a direct Compose build context. It requires two
distinct image links, mirrored whole-row in both endpoints' exports, plus the separate AWS
route and direct execution. The direct Compose workload has no self image link. Every AWS
provisioner/provision/subscription array and non-image contract row equals the supplied
AWS-capable baseline. Reopening preserves bytes. Same-size image and subscription edits each
remove their own links while preserving the other feature's complete arrays; restoration
returns the complete arrays and the persistent mixed trace. HTTP copies all three combined
context/trace answers byte for byte.

The unresolved registry runner `${REGISTRY}/mixed-proof/worker` must remain
`image::*/mixed-proof/worker` and link to the literal `ghcr.io` builder at `inferred`.
Docker Hub expansion must add no registry evidence. A contradictory synthetic process
`REGISTRY` must have no effect.

`image-ko.py` requires the exact set of eight literal publications: simple bare build/publish,
explicit bash/sh, an omitted package target, and step/job shell overrides of a Python workflow
default. Heredoc text, compound/multiline commands, semicolons, substitutions, quoted command
text, non-bare mode, help flags, unknown flags, repeated `--bare`, multiple targets and
Python/custom shells create no guessed publication. It checks each complete repository-level
contract context separately from its exact exported `at` key; contract context does not print
the builder location. Only the checked-in shell/configuration is read; no shell or ko runs.

For a fix, add `--before-bin "$RUN/bin/meada-before"` with an immutable image binary predating
the shell/help fixes. This retains its complete export and requires the original heredoc,
help and non-shell false publications before accepting the final exact builder set. Omit the
option for ordinary regression proof when no pre-fix binary is applicable.

These local probes do not prove actual image publication or cloud delivery. Keep the main
image/public/UI proof, AWS SecureString modes, generic IaC/value oracles and scoped corpus/API
checks alongside them. Preserve strict generic oracle reds and compare their exact answers to
the required baseline before judging them; never trim optional fields or change an oracle to
match a result.
