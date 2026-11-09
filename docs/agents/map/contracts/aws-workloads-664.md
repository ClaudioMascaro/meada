# AWS workloads (#664)

`iac/workload.rs` and `iac/workload/ecs.rs` evaluate Terraform Lambda/task inputs in the same root/module/instance as its
physical name. `Block.nested` retains workload blocks; `Eval::tf_environment(instance, each,
expression)` returns the source description, optional literal and optional typed contract
through the existing evaluator and endpoint walker. Identity attributes include the catalog's
physical-name argument and SQS.url; secret value/SecretString attributes and GCP .arn stay
unread. Explicit unresolved composite projections never fall back to a physical name. A precise archive-file output traversal
follows exactly one declared local source directory/file input; empty or ambiguous inputs
refuse execution, and literal ZIPs stay opaque paths.
`iac/cloudformation.rs` retains SAM CodeUri, Handler, ImageUri and environment
entries, merging Globals first and function entries last. Serverless merges provider entries
before function entries, resolves one existing handler source and fills typed references from
the existing CloudFormation resource-node evaluation. It reserves the bounded target before
copying it, without evaluating resource names twice. Known resource references carry
typed contract targets without inventing a deployment-time ARN or URL. The curated AWS
CloudFormation/SAM aliases share `types::provision_entry` and `Entry::defining_key`, so a
resource's defining property is its provisioner rather than an automatic config use.
Lambda environment keys also use the existing evaluator; an unread or duplicate key retains
an unknown environment scope instead of partial literals, without discarding image/code facts.

`Node.images` retains each container's evaluated image and override evidence; `image.rs`
projects it with legacy Compose and Kubernetes images through one runner inventory. The
stored environment associates each entry with its container image. `Environments::values`
still returns literal values for HTTP; typed resource targets use the same scope index and
resolve only a matching unresolved kind. Secret contents and unread targets stay absent.
ECS reads literal jsonencode fields through the same evaluator; computed encoded JSON shares
that evaluator's limits. Each container retains its own image and environment, including
unread secret entries. A budget failure discards the workload's environment while preserving
its independently known images; overwritten entries and temporary map keys consume room.

`ContractKind::config_summary` receives neutral `ConfigFacts`, grouped by file once after IaC
evaluation. `image/code.rs` checks evaluated direct paths during indexing, including existence
and every symlink component, each through `contract::probe`: a refresh summarizes the file again
when only a variable, a module or what is at a probed path changed. Queries use the stored summary. Compose keeps its earlier path
normalization; new AWS paths reject raw parent components before joining.

The shared cloud fact slots are `Node.provisioned: Option<Projection>` and `Node.ingress`.
A projection preserves the physical name: no projection uses the ordinary type-table name,
an explicitly absent projected name forbids fallback, and a known projection retains its own
pattern and overridable evidence. Literal Ingress host/prefix/Service evidence follows the
transformed Service address (empty for an unread backend). Each reader charges these fields to its existing node/root/repository budget.
