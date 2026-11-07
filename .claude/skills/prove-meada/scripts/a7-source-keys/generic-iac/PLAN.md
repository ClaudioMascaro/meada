# Frozen independent Generic IaC and Serverless source keys

These are pre-answer source keys. No Meada binary, product evaluator, mutation plan or auditor
material was used. No product runtime verdict is claimed. Final-source public CI artifacts
still require independent whole-output judgment against these keys.

Use `capture-plan.json` for the six exact repository/query scopes. It names 460 source nodes,
217 native reference groups and 1,509 baseline queries: Boutique 283/147/841,
kube-prometheus 114/20/363, Crossplane AWS 10/1/41, neutral Kubernetes/Crossplane 34/31/180,
neutral Compose 10/16/41 and Serverless 9/2/43. The three numbers are nodes, references and
queries. Unknown kinds, including the two Skaffold Config objects, remain in the node key.
Generic format completeness concerns these YAML/JSON/Compose/Serverless readers; the existing
native Terraform and CloudFormation keys keep their own scopes.

Public pins are unchanged. GitHub GET supplied nontruncated trees of 458 Boutique entries,
47 Crossplane entries and 358 kube-prometheus entries. All 256 selected source/ignore blobs,
5,191,175 bytes, match their Git blob SHA1 and retained SHA256. `source-sha256.json` records
the complete selection and documented skips; `source-inventory.json` records parsing and
every recognized source object. The oversized Grafana dashboard source is not a node under
the 1 MiB file ceiling. Kubernetes `List` children, Helm templates and patch files are not
expanded. The public source bytes are retained under `source/` for independent review and
need not be copied into tracked product files, since CI fetches those exact pins.

Neutral source repositories are committed independently. Exact origins, commits, Git author
metadata, tracked file hashes and symlink targets are in `compose-repo-identity.json` and
`kube-repo-identity.json`. Copy their frozen tracked bytes and symlinks, initialize with the
stated identity and exclude `.git` from tracked product files. The controls contain two
Compose units, two overlays over a shared nested base, two plain units, explicit/no namespace,
cluster-scoped objects, two Crossplane units and complete positive/negative reference chains.

The node keys include type, address, physical name or pattern, source block/span/root,
overridable evidence, complete local references and reverse references. Native source paths
and lines identify every reference group; rendered `via` text must name the same site.
`kubernetes-kustomize.expected.json` includes all regular/init container image source facts,
the six neutral Kustomize path refusals and first-image-rule/outer-transform expectations.
`crossplane.expected.json` includes literal resources and the supported literal pipeline
input, external names, patched/unnamed patterns and absent custom-function outputs. The actual
pinned Crossplane composition has no literal composed resource and no claim; those roles are
covered only by the neutral controls. A repository XRD identifies the public Cluster role but
does not create cross-directory references.

Use only each row's per-source query file with its own repository. The per-format aggregate
`kubernetes-kustomize.jsonl` and `crossplane.jsonl` concatenate several source scopes and are
indexes, not valid inputs for one repository capture. `queries.jsonl` is the neutral combined
Kubernetes/Crossplane query set. Every neutral node has up/down traces, including empty chains.

`source_edits` uses the existing capture helper's baseline-only exact-replacement schema.
There is one Serverless service-name edit, three Compose edits and four neutral Kubernetes/
Crossplane edits. Their exact before/after hashes and independently expected field/reference
changes are fixed before runtime. Capture warm changed, cold changed, warm restored and cold
restored states with the same source-chosen queries and CLI export/status. Whole tracked bytes
and symlinks must restore to the identity manifests. The Compose override-clear phase gives
the same source node/reference disappearance as removing its body; actual file unlink is a
separate lifecycle case if CI elects to exercise it.

The documented Compose reader records a lexically bounded symlink build path without reading
it. Subsequent code/Dockerfile reading must refuse traversal. The generic key makes that
distinction explicit. The source absence of the oversized public dashboard source is fixed; its
missing/unreadable Kustomize reason wording is not fixed by ADR 0025 and requires final-source
judgment. Composition patches, generators, Helm rendering and arbitrary functions remain
unsupported, and neither this key nor a CAPTURED receipt accepts them.

`input-sha256.json` freezes all ready keys and query pointers. Final source/key-bound CI
comparison and the existing required proof groups remain UNPROVEN until judged independently.
