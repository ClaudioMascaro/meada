# Terraform and provisioning

- Private mappings (#671): `MAPPINGS_MAX` (`iac/mapping.rs`) is 256 module/custom-resource entries per
  repository configuration, above ordinary in-house abstraction inventories. Each source,
  entry and key path is bounded by `config_values::VALUE_MAX` (1 KiB), with selectors refused.
  Overflow/unknown entry is an error naming meada.toml. Registry/private input evaluation and
  source/code projection reuse existing Eval and root/repository storage ceilings; no module
  content is downloaded. Tests include 257 entries and invalid paths, plus native evaluator
  hostile rows. Workload input maps use the existing field-scan/environment budgets.
- `AZURE_IDENTIFIER_MAX` (`contract/azure.rs`, #669): 1,024 bytes, checked before splitting
  identifiers or allocating projected components. Longer ARM/URL/typed-name text stays
  unresolved. Azure projections, rule indexes and Function summaries share the existing
  root evaluation/work/storage and workload environment budgets; archives are never opened.
  The platform name ceilings `SERVICEBUS_ENTITY_MAX` (260), `SERVICEBUS_SUBSCRIPTION_MAX`
  (50), `EVENTHUB_NAME_MAX` (256) and `KEYVAULT_SECRET_MAX` (127) reject invalid names before
  they can become exact targets. `STORAGE_ACCOUNT_NAME_MIN` (3) and
  `STORAGE_ACCOUNT_NAME_MAX` (24), and `BLOB_CONTAINER_NAME_MIN` (3) and
  `BLOB_CONTAINER_NAME_MAX` (63), enforce the platform's storage name lengths. Boundary tests
  include each maximum and one character past it, and each minimum and one character below it.
  `RESOURCE_GROUP_NAME_MAX` (90 Unicode characters, not UTF-8 bytes) follows Azure's
  [resource group naming rules](https://learn.microsoft.com/en-us/azure/azure-resource-manager/management/resource-name-rules#microsoftresources):
  Unicode letter and decimal-digit categories plus `_-.()`, with no trailing period. Other
  selectors keep their own policy; invalid group selectors leave the ARM use unresolved.

- `iac.rs` (#649, ADR 0025): `EXPANSION_MAX` (1,000 instances of one `count` or `for_each`; past it,
  or over anything that does not resolve to a whole number, an object or a list of strings, the
  block is one node with no key); `MODULE_DEPTH_MAX` (16 local module calls inside each other;
  deeper, the call keeps its node and instantiates nothing, and a directory already on the stack, a
  module calling itself, is not instantiated again); `EVAL_DEPTH_MAX` (32 `var`, `local` or output
  names one reference goes through; past it, and around a cycle, it resolves to nothing, and a
  result cut that way is memoized for the instance); `OVERRIDABLE_MAX` (32 variables one name goes
  through, which CloudFormation's parameters share; past it the name is unresolved, so a name
  joined from thousands of them is not copied into every name built on it); `NODES_MAX` (10,000
  nodes per root module, the rest left out); `REFERENCES_MAX` (64 references per node and targets per name, the first in
  address order); `ROOT_STORED_MAX` (4 MiB of addresses per root) and `STORED_MAX` (16 MiB for the
  repository), past which the rest is left out. Both native (`.tf`, `.tofu`) and JSON
  (`.tf.json`, `.tofu.json`) syntax give nodes, only `resource`, `data` and `module`
  blocks are nodes (no `ephemeral`, `import` or `moved`), and only a local `source` is followed (a
  registry, Git or absolute source is the call's node alone). `count.index`, `each.key` and any
  index that is not a literal name every instance; a literal key no instance has names every
  instance too. References are followed by name, with no value evaluated: `try`, conditionals and
  `for` contribute every block they name. A changed Terraform file or var file reads every Terraform
  file of the repository again to find the roots and their units (a `ponytail:` in
  `index::refresh_keeping`), though only the units holding it are evaluated again; past
  `STORED_MAX`, a refresh that evaluates some roots again counts the kept roots' bytes first, so it
  may keep a root a cold index would cut. A root with no node is evaluated again at every Terraform
  change. A directory called only from a cycle of directories is no root, so its blocks are no
  nodes. Locals that name each other in a cycle (which Terraform refuses) resolve partly: the member
  resolved inside the other's resolution is memoized without what the cycle adds. `search` kind
  `iac` scans every node of every repository (a `ponytail:` in `iac::hits`); an indexed lookup waits
  for a workspace that makes it slow. A module call has no physical name: its `name` is one of its
  inputs.

- Terraform/OpenTofu JSON (`iac/json.rs`) shares the event reader's `MAX_NESTING` and
  per-file `MAX_BYTES`, plus the ordinary evaluator's limits. Each completed block body is
  consumed with its borrowed source location before a repeated property replaces an earlier
  one. Its retained labels and rendered paths charge `ROOT_STORED_MAX` before allocation;
  source-location metadata holds no values. `JSON_EXPR_WORK_MAX` (1,048,576 construction work
  units per file) charges expression visits, string bytes and the square of object widths or
  template separators before constructing HCL ASTs, since HCL's object insertion compares keys
  linearly. AST room is also
  charged before allocation against `ROOT_STORED_MAX`, allowing for retained copies, object key
  nodes and raw dependency traversals. From a possible template marker onward, each text byte
  reserves an expression and template element conservatively, even for escaped markers or
  literal suffixes; a long plain prefix stays proportional to its bytes. Retained local and
  argument entries charge their metadata before cloning, including unknown fallbacks. An
  expression beyond either budget stays unknown; its resource remains a node. Variable defaults
  and module sources are literal; ordinary strings and object keys are templates, with a sole
  interpolation retaining its result type. Static `depends_on` strings are traversals. Array
  block forms retain source indexes in their config key paths. Provider-specific computed block
  schemas are not executed or fetched; unsupported expressions stay unresolved.

- AWS and Pub/Sub subscriptions (#658, #667) share `EVAL_DEPTH_MAX`, `EVAL_WORK_MAX`, `ROOT_STORED_MAX` and
  `STORED_MAX`: alias/module traversal is bounded by evaluation depth/work and every stored
  summary is charged to the existing per-root and repository byte budgets. The hostile probe
  table includes repeated subscription endpoints and filters; its output counts the summaries.

- `iac/types.rs` (#656): `FORM_TEXT_MAX` (1,024 bytes); longer identifiers keep their whole
  text as the name, bounding the form matcher. The table covers SQS queues and S3 buckets in
  `.tf`, `.tf.json`, `.tofu` and `.tofu.json` resource blocks. Data blocks and unlisted types
  make no provisioner.
  `iac/pattern.rs`: `PATTERN_LITERAL_MIN` (3 literal characters), `PATTERN_SEGMENTS` (`-_./`)
  and `PATTERN_TEXT_MAX` (1,024 bytes for either name): a pattern outside these bounds matches
  nothing. Literal characters include written separators. Tests reject anchors below three
  characters, wildcard boundaries outside those four separators, and text above the byte
  ceiling. These bounds keep the evidence meaningful and the matching work bounded.
  `crate::iac::provision::links` scans contract sides for each provisioner;
  IaC traces hydrate all provisioned sides before following the selected node's references.

- `iac.rs` static evaluation (#650, ADR 0025): `EVAL_VALUE_MAX` (64 KiB of one evaluated value,
  as `iac::size` counts it; past it, the value is unresolved, and a template past it is `*`
  whole), `EVAL_COST_MAX` (1 MiB that one expression could make, `iac::cost`'s upper bound; past
  it `hcl-rs` does not evaluate the expression, only its template parts resolve) and
  `EVAL_WORK_MAX` (64 MiB of cost per root module; past it every value left is unresolved;
  converting an object literal for `hcl-rs` also spends the square of its keys, so a wide local
  evaluated in many module instances stops there). A var file's values are bounded the same way,
  each file with an `EVAL_WORK_MAX` of its own: a value past `EVAL_COST_MAX`, that work or
  `EVAL_VALUE_MAX` is given but unresolved (the default does not stand in for it), and one that
  is no constant is left out, as Terraform refuses it;
  `WIDTH_MAX` (`iac/functions.rs`: a `format` width of 256; past it the call is unresolved). `cost`
  bounds a `for` by the size of what it iterates, so a `for` over a value it cannot size (a
  function's result) is costed as its square and may stay unresolved below the real ceiling.
  The type table selects `bucket` for S3 and `name` for SQS; unlisted types still read
  `name` (no `function_name` or `name_prefix`). A resource's attribute is never evaluated, even an
  argument its configuration sets, so an output of `aws_sqs_queue.this.name` is `*`. A `count`
  or `for_each` that reads a module output does not resolve (module instances are made before
  outputs are read). Only the var files Terraform loads by itself give values
  (`terraform.tfvars`, `terraform.tfvars.json`, `*.auto.tfvars`, `*.auto.tfvars.json`): a
  `-var-file` is not read, and a JSON var file's value line is the first occurrence of its key.
  A value cut at `EVAL_DEPTH_MAX` or around a cycle is memoized for its instance, so a shallower
  reader of the same local gets the cut value. A `for_each` over a list of strings expands
  (`toset` gives a list too, so a list Terraform refuses cannot be told apart), and `toset` over
  mixed types keeps their first order. A function over an unresolved argument is `*` whole, even
  `format` or `join`, which keep no literal part. A literal
  `*` in a name reads as a wildcard. A child module's variable default its call leaves is not
  overridable. Names through a cycle of locals are partly resolved, the member met again a `*`.

- GCP name projections and Cloud Run container summaries (#667) reuse the evaluator work,
  root and repository byte budgets, plus `ENVIRONMENT_BYTES_MAX` for each workload. Projection
  evidence is charged before copying, including added instance provenance. Subscription
  pattern evidence is checked before adding its canonical prefix; the prefix cannot supply
  the physical name’s minimum literal evidence.
  Structural scans charge retained block metadata to `EVAL_WORK_MAX` before walking or
  collecting children, including empty bodies. Exhausting a workload's environment byte
  budget clears its entries while preserving charged images from its containers.


- IaC HTTP route summaries (#666, `http/iac.rs`): `IAC_HTTP_STORED_MAX` is `4 * lang::MAX_BYTES`
  (4 MiB) per config file, counting repeated contracts/handler names before copying and the
  complete serialized result. Overflow drops the whole summary. Route facts share the IaC
  root/file budgets; attribute aliases and REST parent/method chains share `EVAL_DEPTH_MAX`
  and the evaluation work budget. Only finite API Gateway v2 HTTP routes/explicit quick
  create and v1 API/resource/public-method joins to a proven Lambda are read. WebSockets,
  non-Lambda services, `$default`, unknown selections and incompatible APIs provide none.
  SAM requires explicit Api/HttpApi method/path fields and any explicit API reference to be
  known. Serverless reads its existing bounded event list and keeps method/path distinct;
  only a method field's `*` means `ANY`. Handler validation reuses `image/code.rs`, with a
  unique supported local module or explicit source file and an existing stored symbol;
  archives and unsafe/ambiguous paths use the repository. Provider resolution never validates
  paths during queries; existing code rendering may read source for line displays.
