# 25. Config and IaC in the index

- Status: accepted
- Date: 2026-11-04

## Context

Spec #642 (A7, config and IaC in the graph) has Meada read configuration and infrastructure as
code (IaC) and turn the cloud resources it creates into contracts. Its triage settled the
decisions in the grill of #639 on 2026-11-03. This ADR records what Meada reads and keeps: config
values, IaC nodes, the references between them and the static evaluation of the names they
build. ADR 0026 records the contracts built on top of them. ADR 0004 is amended so the index may
keep the text of config and IaC scalars; code stays a location only.

ADR 0002 does not change: reading and evaluation use only the repository, never the network or a
model, and the same inputs give the same answers.

## Decision

### Config values

- **Files read.** Every `*.yaml`, `*.yml` and `*.json` file the walk visits (it honours
  `.gitignore`, `.ignore` and `.meadaignore`) up to the per-file size ceiling `lang::MAX_BYTES`,
  plus the CI configuration of a fixed list of hidden paths, read although hidden:
  `.github/**`, `.gitlab-ci.yml`, `.gitlab/**`, `.circleci/**`, `.buildkite/**` and
  `.drone.yml`. Every other hidden path stays out. There is no opt-in key.
- **Skipped.** Lockfiles, by a fixed list of names (`package-lock.json`,
  `npm-shrinkwrap.json`, `pnpm-lock.yaml` and `packages.lock.json`, the lockfiles a YAML or JSON
  extension reaches), and files that do not parse are skipped and counted in `meada status`.
- **Reader.** One event reader for YAML and JSON yields each scalar with its line and reads
  every document of a stream. Alias expansion keeps today's budget (a node built is charged to a
  budget proportional to the source). The documents per file are a named ceiling. The Spring and
  .NET configuration readers keep their behaviour.
- **Row.** A config value is a row of: file, key path, value, scalar type (`string`, `number`
  or `boolean`; a null is left out) and line. A value longer than the value length ceiling
  (`VALUE_MAX`, 1 KiB to start) is stored cut, with a mark the answers show.
- **Storage.** Config values live in a stamped table refreshed like documents and manifests
  (path, hash, mtime, size), so a change to config alone refreshes config alone. The index
  schema version changes with the schema golden. A full-text index (an FTS5 table with the
  trigram tokenizer, compiled into the bundled SQLite) serves substring search over values and
  key paths, and a B-tree on their lowercased text serves exact and prefix search.
- **Export.** Config values stay out of `meada export`.

### Key path

A key path names a scalar inside its file, jq style:

- keys are joined by dots: `spec.template.spec.containers`;
- a list item is `[n]`, 0-based: `containers[0].image`;
- a key that is not a plain identifier (an ASCII letter or `_`, then ASCII letters, digits or
  `_`, as jq reads `.key`) is written `["..."]` with JSON string escaping:
  `metadata.annotations["app.kubernetes.io/name"]`, `env["log-level"]`;
- a file with more than one document starts with the document's index (`[2].spec.replicas`); a
  file with one document does not.

A key path into an HCL file follows the same grammar over its blocks and attributes, the block's
type and labels first (`resource.aws_sqs_queue.orders.name`).

### Secret markers

Where a format marks a value as secret with certainty, the row keeps its key path, type and line
with `redacted` in place of the value:

- Kubernetes `Secret` `data` and `stringData`;
- a Terraform variable with `sensitive = true`, and every value a `.tfvars` file gives it;
- a CloudFormation parameter with `NoEcho`;
- an `aws_ssm_parameter` of type `SecureString`.

Nothing else is filtered and no heuristic guesses at secrets: the index holds nothing the
checkout does not, and the docs point at `.meadaignore` for files that must stay out.
`meada status` counts the values redacted.

### IaC formats

- **Terraform and OpenTofu**: `.tf`, `.tf.json`, `.tofu`, `.tofu.json`, `.tfvars`,
  `*.auto.tfvars` and `.tfvars.json`. A `.tofu` file shadows the `.tf` file of the same base
  name in the same directory, and a `.tofu.json` file the `.tf.json` one, as OpenTofu does: its
  documentation, "Extension Precedence" in `website/docs/language/files/index.mdx`, says "When
  both `.tf` and `.tofu` files with the same base name are present in a directory, OpenTofu will
  prioritize the `.tofu` file and ignore the `.tf` file", and its loader
  (`filterTfPathsWithTofuAlternatives` in `internal/configs/parser_config_dir.go`) applies the
  same rule to `.tf.json` and `.tofu.json` (opentofu/opentofu at commit
  `c570669d0fed5787f18d2f017f56b2658e2e6cce`, read on 2026-11-04).
- **Kubernetes objects**: any YAML or JSON document with `apiVersion` and `kind`, and Kustomize
  trees (`kustomization.yaml` with its bases and overlays).
- **Compose** files.
- **Crossplane** compositions, composite resource definitions, claims and managed resources.
- **CloudFormation and SAM** templates.
- **Serverless Framework** service files.
- **Helm charts**, through their values files only: templates are not rendered and stay skipped.

IaC written in general-purpose code (CDK, Pulumi) is out.

### HCL reader

- HCL is read with `hcl-edit` (a span for every node) and `hcl-rs` (its expression evaluator),
  both MIT OR Apache-2.0. `.tf.json` and `.tofu.json` go through the JSON reader with
  Terraform's JSON syntax rules. A file that does not parse is skipped and counted.
- `hcl-rs` declares no function of its own. Meada registers its own implementations of these
  pure functions, and only these:
  - strings: `format`, `formatlist`, `join`, `split`, `lower`, `upper`, `title`, `trim`,
    `trimprefix`, `trimsuffix`, `trimspace`, `chomp`, `replace` (a literal substring only; a
    `/.../` pattern leaves the call unresolved), `substr`;
  - collections and conversions: `concat`, `coalesce`, `element`, `keys`, `values`, `lookup`,
    `merge`, `length`, `toset`, `tolist`, `tomap`, `tostring`, `tonumber`;
  - encoders: `jsonencode` (#650, as Terraform writes it: compact, `<`, `>` and `&` escaped).
- Any other call (`file`, `templatefile`, `timestamp`, `uuid`, the hash functions, the other
  encoding functions, the network functions, a provider function) is unresolved and becomes a
  wildcard. Adding a function to the list is a change to this ADR: it must be pure, total over its
  inputs and need no file.

### HCL values (#640)

- **Key path.** The grammar above over the block's type and labels and the attribute's name
  (`resource.aws_sqs_queue.orders.name`). A block repeated under the same type and labels in
  one body is indexed `[n]` from 0 (`ingress[0].from_port`), as the JSON syntax writes it; a
  block that is alone has no index. A list or an object with literal keys is read element by
  element; an object with a key that is no literal is one `expression`.
- **Type.** A literal keeps `string`, `number` or `boolean` (a null is left out); a heredoc of
  literals is the string HCL reads from it. Anything else is stored as written (the source
  text, cut at the value ceiling) under a type of its own until static evaluation resolves it:
  `expression` (a reference, a call, an operator, a conditional, a `for`), `template` (a string
  or heredoc with an interpolation or a directive). In `.tf.json` and `.tofu.json` every string
  is a template: one with an interpolation is stored as the JSON string, quoted, typed
  `expression` when it is a single interpolation and nothing else and `template` otherwise;
  a plain one has its `$${` and `%%{` escapes undone, and a variable's `default` is a literal
  string, as Terraform reads it.
- **Sensitive.** The `default` of a top-level `variable` with the literal `sensitive = true`
  (`"sensitive": true` in JSON) is `redacted`, whatever it holds. A `.tfvars`, `*.auto.tfvars`
  or `*.tfvars.json` file has `redacted` every value it gives a variable whose name any
  Terraform file of the repository marks sensitive: a var-file can go to a module in any
  directory, before evaluation says which, and redacting too much is the safe direction. When
  a Terraform file in the var-file's directory or above is unreadable, the variables it
  declares are unknown and every value of the var-file is redacted. The stored summary of a
  `.tfvars` file names what it was read against, and it is read again when that changes.
- **Shadowing.** A `.tofu` file hides the `.tf` file of the same base name in the same
  directory, and a `.tofu.json` the `.tf.json`; the hidden file is not read and not counted.
- **Ceiling.** Brackets (`{`, `[`, `(`) nest at most 64 deep in one file, counted even inside
  a string; past it the file is unreadable, which `meada status` counts. The value length and
  the stored budget are the ones above.

### Terraform nodes and references (#649)

- **Roots.** A directory of `.tf` and `.tofu` files is a module. A `module` block whose literal
  `source` starts with `./` or `../` and, joined to its directory and normalized, stays inside
  the repository calls the module of that directory. A directory no other directory calls is a
  root module, labelled by its path (`.` at the repository root); every node records it, and a
  node of a module two roots instantiate is one node per root.
- **Instances.** Each call instantiates the called module's blocks under the call's address and
  a dot, recursively; a `count` of a literal number, or a `for_each` over an object of literal
  keys or `toset` of literal strings, gives one node per key (`[0]`, `["eu"]`, the keys sorted
  as Terraform sorts them), and any other `count` or `for_each` one node with no key. Static
  evaluation (#650, below) widens "literal" to any value that resolves to one.
- **References.** A traversal in any expression of a block (its arguments, nested blocks,
  `depends_on`) that names a resource, a data block or a module is a reference from the block's
  node: to the instance a literal key names, else to every instance. One through `var.<name>`
  goes to what the caller's argument of that name references, one through `local.<name>` to what
  the local references, and one through `module.<name>.<output>` to what the output references
  inside the instance; each keeps that first name as its `via`, and one to a module output that
  references nothing goes to the module's node. References stay within their root and are all
  `exact`: names as written, followed without evaluating a value.
- **Queries.** `search` kind `iac` matches the address, the type (`module` for a module call)
  and the physical name, the block's `name` argument as static evaluation resolves it (#650; a
  pattern is matched by its text, `*` and all). `context` and `trace` take
  an address, `<path>::<address>` or `<repository>::<path>::<address>`, followed by
  ` (root <root>)` when several roots hold it, and answer for a node only when no symbol (and, for
  `trace`, no route or call site) has the text, so every answer about code stays as it was.
- **Storage.** The nodes and references are rewritten whole from the repository's Terraform files
  when one of them is read again or leaves the index, so a refresh that touches no Terraform file
  keeps them. #650 narrows it to the evaluation units below.

### Static evaluation of Terraform names (#650)

- **What is evaluated.** A node's physical name (its `name` argument; a module call has none) and
  the `count` and `for_each` of every block and module call, in the instance they belong to,
  with `hcl-rs`'s evaluator and the functions above. Each name an expression reads is bound to
  its value first: `var.<name>`, `local.<name>`, `module.<call>.<output>` (per instance of the
  call, as an object, a list for a `count`, an object by key for a `for_each`), `each.key`,
  `each.value` and `count.index`.
- **Variables.** In a root module, a variable is the value its var files give it, or else its
  default: `terraform.tfvars` first, then `terraform.tfvars.json`, then each `*.auto.tfvars`
  and `*.auto.tfvars.json` of the root's directory in name order, a later file winning, as
  Terraform loads them by itself (a JSON var file's strings are literals). Either is **overridable**: the
  name records the variable and where its value is (the variable's block for a default, the
  value's line for a var file), and `context` lists them. In a local module, a variable is the
  call's argument, evaluated in the caller with the call instance's `each` (it carries what the
  argument went through), or else its default, which no deployer sets. A variable with
  `sensitive = true` is never evaluated, so its value never reaches a name.
- **Never evaluated**: data sources, resource attributes (even an argument the configuration
  sets), registry and remote modules, `terraform.workspace`, `path.*`, `-var`, any `-var-file`
  other than the ones Terraform loads by itself, `TF_VAR_` and any function outside the list.
- **Unresolved parts.** An expression that does not evaluate whole keeps what it can by its form:
  a template keeps its literal text and each interpolation that resolves, every other part (an
  interpolation that does not resolve, a directive) a `*`, consecutive `*` merged into one;
  `var.<x>`, `local.<x>` or `module.<call>.<output>` alone is that name's value, a pattern
  included; anything else (a function over an unresolved value, an operator, a conditional) is
  `*` whole. The name is then a **pattern**: `search` shows it as `pattern "orders-*"` in place of
  `name`, and `context` as `name: "orders-*" (pattern)`. A pattern links nothing yet (ADR 0026
  fixes how it will).
- **Expansion.** A `count` that resolves to a whole number, or a `for_each` that resolves to an
  object or a list of strings (`toset` sorts and dedupes strings), gives one node per key, up to
  `EXPANSION_MAX`; a `count` or `for_each` that reads a module output is evaluated before the
  module has instances, so it does not resolve.
- **Ceilings.** Besides the ones above, `EVAL_VALUE_MAX` (64 KiB of one value), `EVAL_COST_MAX`
  (1 MiB that one expression could make, an upper bound computed from the sizes of the values it
  reads, products for a `for`, `formatlist`, `join` and `replace`) and `EVAL_WORK_MAX` (64 MiB of
  that cost per root module); past each, the value is unresolved. A `format` width past
  `WIDTH_MAX` (256) leaves the call unresolved.
- **Evaluation unit.** A root module with the directories of the local modules it calls, however
  deep (up to `MODULE_DEPTH_MAX`). A refresh evaluates again the roots whose unit holds a
  directory where a Terraform file or a var file was read again or left, and the roots not
  stored yet, drops the rows of the roots that are no more, and keeps every other root's rows.

### Kubernetes objects and Kustomize trees (#651)

- **Objects.** Every YAML or JSON document with an `apiVersion`, a `kind` and a string
  `metadata.name` is a node, addressed `<Kind>[.<group>]/<namespace>/<name>` (the core group
  writes no group). A cluster-scoped kind (a fixed list) has no namespace in its address and no
  Kustomize `namespace` sets one; an object with no `metadata.namespace` that no overlay gives
  one stays without it, `<Kind>[.<group>]/<name>`, and is not given the cluster's `default`. A
  Kustomize file (`kustomization.yaml`, `kustomization.yml`, or any object of the group
  `kustomize.config.k8s.io`) is no object, and neither is a document whose name holds `{{` (a
  Helm template, which is not rendered). The node's `block` is its document's key path prefix
  (`[2]`, none in a file of one document), so `context` shows the values of its document; its
  span is the lines of its first and last scalar.
- **Overlays.** A directory with a kustomization that no other kustomization names as a base
  (`resources`, `bases`, `components`) is an overlay, labelled by its path, and the root of its
  evaluation unit. Its entries that name a file or a kustomization directory of the repository
  are instantiated under it, each base first, and each kustomization on the way then applies
  its `namePrefix`, `nameSuffix` (not to `Namespace`, `CustomResourceDefinition` and
  `APIService`), `namespace` and `images` (the first rule that names the image; `newName`,
  `newTag`, `digest`) to everything below it, so the overlay's values come last. A base used by
  two overlays is one node in each, under that overlay's names. A manifest no kustomization
  lists or patches with is plain, and the plain manifests of a directory are one unit labelled
  by the directory. Patches, generators and `commonLabels` are not applied (the ceilings list
  them).
- **Paths.** Entries follow the "Paths" rule below as #649 amended it: a `..` that stays
  inside the repository is followed, since overlays name their base as `../../base`. The files
  come from the walk, which follows no symlink, so a base through a symlink is not found. An
  entry that is remote (a URL, `git@`, `github.com/`, `?ref=`), above the root, absolute, not
  there, in a cycle or nested past `OVERLAY_DEPTH_MAX` is not followed, never fetched, and is
  stored as unresolved with its reason; `meada status` counts them.
- **References.** A Service to the pod template owners (any object whose `spec.template` or
  `spec.jobTemplate.spec.template` holds containers, or a Pod) whose pod labels hold its
  selector, a pod template to the ConfigMaps and Secrets named in `env`, `envFrom` and volumes
  (including projected ones) and to its `serviceAccountName`, and an Ingress to the Services of
  its rules and default backend, each in the same namespace and the same unit, by the name as
  written before any prefix or suffix (Kustomize renames the references with the object), all
  `exact`, with the `via` naming how the source names the target.
- **Images.** The images of a workload's containers once the overlay's `images` applied are
  stored with its node and shown by `context`; the image contract of ADR 0026 reads them.
- **Storage.** The nodes and references of Kubernetes join the Terraform ones in `iac_nodes`
  and `iac_references`; `iac_images` and `iac_unresolved` hold the rest. All are rewritten
  whole when a file that holds, or may hold, an object (a kustomization, or a YAML or JSON file
  that says `apiVersion`) is read again or leaves.

### IaC node

An IaC node is a resource-like block of any format, with an address, a type, a file and a span.
Its address:

- Terraform and OpenTofu: `<type>.<name>`, `data.<type>.<name>`, `module.<name>`; a local
  module's blocks prefixed by the call's address (`module.queues.aws_sqs_queue.orders`); a
  `count` or `for_each` instance suffixed `[key]` (`aws_sqs_queue.orders["eu"]`, `[0]`).
- Kubernetes: `<Kind>[.<group>]/<namespace>/<name>` (`Deployment.apps/shop/api`), with no
  namespace for a cluster-scoped kind (`ClusterRole.rbac.authorization.k8s.io/reader`); the core
  group writes no group (`Service/shop/api`). Crossplane objects are Kubernetes objects.
- CloudFormation and SAM: the logical id within its template.
- Compose: `services.<name>`.
- Serverless: `functions.<name>`, and its resources by logical id.

Each node records the label of the root module or overlay that roots its evaluation (ADR 0026
labels a provisioner by it). Its identity is `<repo remote>::<file>::<address>`, the form of a
symbol's (ADR 0004), so links and decisions refer to it without lines.

### References

A static reference from one IaC node to another is stored like a call and walked by `trace`:

- Terraform: references to resources, data blocks, modules and their outputs;
- CloudFormation and SAM: `Ref`, `Fn::GetAtt` and the variables of `Fn::Sub`;
- Kubernetes: a Service's selector to the pod templates whose labels it matches in the same
  namespace and evaluation unit, ConfigMap, Secret and service account references, and Ingress
  backends;
- Compose: `depends_on`, `links` and shared networks;
- Crossplane: claims to composite resources to composed resources.

References per node are a named ceiling.

### Evaluation unit

An evaluation unit is the set of files evaluated together: a Terraform module directory (each
local module instantiated once per call), a Kustomize tree per overlay, a CloudFormation or SAM
template, a Compose project, a Serverless service file. A change to any file of a unit
re-evaluates the whole unit. Links across repositories resolve at link time, as today.

### Static evaluation

- **Resolved**, locally and deterministically: Terraform variable defaults, locals, literal
  `.tfvars` and `*.auto.tfvars`, local module inputs and outputs, interpolation, the pure
  functions above, and `count` and `for_each` over a collection that resolves to literals (one
  node per instance); CloudFormation and SAM parameter defaults, `Ref`, `Fn::Sub`, `Fn::GetAtt`
  and `Fn::Join` within the template; Kustomize `namePrefix`, `nameSuffix`, `namespace` and
  `images`; Crossplane managed resources and composition bases with literal names; Compose
  `${VAR:-default}`; Serverless `${self:...}` and the default of `${opt:...}`; Helm values
  (`image.repository`, `fullnameOverride`).
- **Never resolved**: data sources, remote state, registry modules (beyond what the type table
  says of their inputs), Terraform workspaces, `-var` and `TF_VAR_`, Helm template rendering,
  composition patches and functions, and anything read from a process environment.
- **Unresolved parts.** An unresolved part of a name becomes a wildcard, and the name becomes a
  pattern (ADR 0026 fixes how a pattern links).
- **Level of an evaluated name.** A name written as one literal, or assembled from literals,
  locals and functions over them, counts as written. A name that went through a value a deployer
  can override (a variable default, a tfvars value, a parameter default, a Compose or Serverless
  default, a Helm value) links `inferred`, its reason naming those values. This extends ADR
  0005's `inferred` (the sides match after Meada resolved a base URL or a prefix) to a match
  after Meada resolved a value a deployer can override.
- **Ceilings.** Value length, YAML documents per file, evaluation depth, `count` and `for_each`
  expansion, module nesting, nodes per evaluation unit and references per node are named
  ceilings. Each is listed with the known ceilings, carries the marker and is fed an input past it
  by a unit test.
- **Hostile input.** Every new reader (the event reader, HCL, Kustomize trees, CloudFormation
  intrinsics, the evaluator) adds its row to the hostile probes and reads a file of the per-file
  size ceiling built to explode in bounded time and size.
- **Paths.** A path read from IaC (a local module source, a Kustomize base, a SAM `CodeUri`, a
  Compose build context) stays inside the repository root: relative, never above the root once
  joined to its directory and normalized, and through no symlink. Any other path is not followed.
  A `..` that stays inside the root is followed (#649): `../../modules/queue` from `envs/prod` is
  how Terraform repositories lay out their modules, and the root is what keeps a read inside
  the repository.

### Type table

- A data file versioned in the repository and embedded in the binary maps an IaC type (a
  Terraform resource type, a Kubernetes kind and group, a CloudFormation type, a Serverless
  resource) to what it provisions: the contract kind with its broker or resource kind (ADR
  0026), the attribute or key path that holds the physical name, how to read a name out of an
  ARN or a URL held there, and, for a workload, where its image, its environment and its code
  path live. Every entry has a fixture case.
- Coverage, in the order the tickets set: AWS (SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, RDS,
  ElastiCache, SSM Parameter Store, Secrets Manager, Lambda, ECS), GCP (Pub/Sub, Cloud Storage,
  Cloud SQL, Secret Manager, Cloud Run), Azure (Service Bus, Event Hubs, Storage, Key Vault,
  Functions), Kubernetes core (workloads, Service, Ingress, ConfigMap, Secret), well-known
  custom resources (the Crossplane provider families, Strimzi `KafkaTopic`, the RabbitMQ
  messaging topology operator, AWS Controllers for Kubernetes, GCP Config Connector), monitoring
  (Datadog monitors, OpsGenie teams and routing rules, PagerDuty services and event
  orchestrations, Grafana alert rules, Prometheus and Loki rule groups and `PrometheusRule`), and
  popular registry modules by source and inputs (the terraform-aws-modules family).
- **`meada.toml` extension.** A table maps a private module source (or a source prefix) or an
  in-house custom resource (group and kind) to a type table entry, naming the input or key path
  that holds the physical name. It is versioned with the code, as the rest of `meada.toml` (ADR
  0005).
- A block whose type the table does not know is still an IaC node, with its references and config
  values; it holds no contract side.

### Search

- `search` gains the kind `value`: the lowercased query matches config values and key paths,
  exact first, then prefix, then substring, ordered by rank, tie-break and line, and cut by the
  token budget. A query under three characters (below the trigram) falls back to a scan.
- `search` gains the kind `iac` for IaC nodes, matched by address, type and physical name. The
  word is not `node`, which the glossary gives to a server-mode process.
- **Default search.** The default search (no kind) does not include IaC nodes. It answers as
  today when it finds a symbol or a contract; only when it finds neither does it fall back to
  config values. A provisioned resource already shows in the default search through its contract
  (ADR 0026), and `kind = "iac"` finds the node itself, so every query that finds something today
  keeps its answer byte for byte, and one that found nothing changes only by finding a config
  value.
- The MCP `search` tool's kind list gains `value` and `iac`.

## Considered options

- **Opt-in config reading** (a `meada.toml` key). Rejected: the value of search over config is in
  repositories nobody configured, and the files are the ones the walk already visits.
- **Every hidden path.** Rejected: hidden directories hold editor state, caches and tool
  output; the hidden CI list covers what publishes images and names resources.
- **Heuristic secret detection** (entropy, key names). Rejected: it misses and it false-flags,
  and it suggests a guarantee Meada cannot give. Only the format markers count.
- **Rendering Helm templates or running `terraform plan`.** Rejected: both need values,
  providers or the network that the repository does not fix (ADR 0002).
- **A full HCL evaluator with every Terraform function.** Rejected: functions that read files,
  the clock or randomness break determinism, and the others add nothing to names.
- **IaC nodes in the default search**, beside symbols or as a fallback before config values.
  Rejected: either would change answers on repositories with IaC that today find nothing or
  something else, and a provisioned resource is already found through its contract.
