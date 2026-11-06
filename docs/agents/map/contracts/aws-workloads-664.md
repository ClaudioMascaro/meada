# AWS workloads (#664)

`iac/workload.rs` evaluates Terraform Lambda inputs in the same root/module/instance as its
physical name. `Block.nested` retains workload blocks; `Eval::tf_environment(instance, each,
expression)` returns the source description, optional literal and optional typed contract
through the existing evaluator and endpoint walker. A precise archive-file output traversal
follows only its declared local source directory/file input; literal ZIPs stay opaque paths.
`iac/cloudformation.rs` retains SAM CodeUri, Handler, ImageUri and environment
entries, merging Globals first and function entries last. Known resource references carry
typed contract targets without inventing a deployment-time ARN or URL. The curated AWS
CloudFormation/SAM aliases share `types::provision_entry` and `Entry::defining_key`, so a
resource's defining property is its provisioner rather than an automatic config use.

`Node.images` retains each container's evaluated image and override evidence; `image.rs`
projects it with legacy Compose and Kubernetes images through one runner inventory. The
stored environment associates each entry with its container image. `Environments::values`
still returns literal values for HTTP; typed resource targets use the same scope index and
resolve only a matching unresolved kind. Secret contents and unread targets stay absent.

`ContractKind::config_summary` receives neutral `ConfigFacts`, grouped by file once after IaC
evaluation. `image/code.rs` checks evaluated direct paths during indexing, including existence
and every symlink component. `config_reads_paths` rechecks them when only a variable, module or
filesystem path changed. Queries use the stored summary. Compose keeps its earlier path
normalization; new AWS paths reject raw parent components before joining.

The shared cloud fact slots are `Node.provisioned: Option<Projection>` and `Node.ingress`.
A projection preserves the physical name: no projection uses the ordinary type-table name,
an explicitly absent projected name forbids fallback, and a known projection retains its own
pattern and overridable evidence. Literal Ingress host/prefix/Service evidence follows the
transformed Service address (empty for an unread backend). Each reader charges these fields to its existing node/root/repository budget.
