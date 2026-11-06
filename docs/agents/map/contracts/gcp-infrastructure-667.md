# GCP infrastructure and Cloud Run (#667)

`contract/gcp.rs` owns shared GCP physical identities, used by type-table projection, config
uses and SDK readers. Topics retain their id; subscription targets are `subscriptions/<id>`.
Structured project identifiers must fit their complete kind-specific shape. Bucket URIs drop
only their opaque object suffix, Secret Manager names their optional version. The final typed
aliases in ADR 0026 remain inferred; generic AWS aliases retain their meanings.

`iac/gcp.rs` populates `Node.provisioned` with the canonical name, pattern and overridable
evidence while `Node.name` stays physical. An explicit projection without a name refuses
legacy fallback. Cloud SQL databases require complete instance evidence and qualify their
name as `<instance>/<database>`, retaining instance provenance. `provision::Side::pattern`
and `overridable` are the neutral evidence accessors used by matching, levels and choices.
An early unresolved marker prevents budget cuts from creating short-name provisioners.

`iac/subscription.rs` follows a Pub/Sub subscription's `topic` through the existing bounded
endpoint walker: exact resources, locals, module inputs/outputs and selected instances.
Missing/empty filters are exact; nonempty/unknown filters and overridable evidence weaken
delivery. Targets are always subscription-qualified. Pub/Sub AsyncAPI sides use the same
channel kind and supply a public routing seam independent of SDK extraction.

Cloud Run v1 services read `template.spec.containers`, v2 services `template.containers`, and
v2 jobs `template.template.containers`. Retained HCL bodies use the existing evaluator/root/
instance budgets. Each container keeps `WorkloadImage` and image-scoped environment entries.
`Eval::tf_environment` preserves typed node references; secret environment sources retain
only readable targets. Duplicate entries remain unresolved. Image matching, executions,
context, trace and orphans use #664's common projection. Environment entries are not exported.
Structural scans spend the existing evaluation work budget even for empty blocks. A workload
past its environment byte budget discards those entries while retaining its known images.

`tests/it/gcp.rs` uses `gcp-infra`, `gcp-app` and `gcp-run` sibling fixtures. The prove-meada
GCP flow drives CLI/MCP over neutral generated sources. #668 owns GCP SDK operations.
