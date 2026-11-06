# Terraform and provisioning

- `iac.rs` (#649, ADR 0025): `EXPANSION_MAX` (1,000 instances of one `count` or `for_each`; past it,
  or over anything that does not resolve to a whole number, an object or a list of strings, the
  block is one node with no key); `MODULE_DEPTH_MAX` (16 local module calls inside each other;
  deeper, the call keeps its node and instantiates nothing, and a directory already on the stack, a
  module calling itself, is not instantiated again); `EVAL_DEPTH_MAX` (32 `var`, `local` or output
  names one reference goes through; past it, and around a cycle, it resolves to nothing, and a
  result cut that way is memoized for the instance); `NODES_MAX` (10,000 nodes per root module, the
  rest left out); `REFERENCES_MAX` (64 references per node and targets per name, the first in
  address order); `ROOT_STORED_MAX` (4 MiB of addresses per root) and `STORED_MAX` (16 MiB for the
  repository), past which the rest is left out. Only `.tf` and `.tofu` files give nodes (a
  `.tf.json` or `.tofu.json` file gives config values only), only `resource`, `data` and `module`
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

- AWS subscriptions (#658) share `EVAL_DEPTH_MAX`, `EVAL_WORK_MAX`, `ROOT_STORED_MAX` and
  `STORED_MAX`: alias/module traversal is bounded by evaluation depth/work and every stored
  summary is charged to the existing per-root and repository byte budgets. The hostile probe
  table includes repeated subscription endpoints and filters; its output counts the summaries.

- `iac/types.rs` (#656): `FORM_TEXT_MAX` (1,024 bytes); longer identifiers keep their whole
  text as the name, bounding the form matcher. The table covers SQS queues and S3 buckets in
  `.tf` and `.tofu` resource blocks only. Data blocks and unlisted types make no provisioner.
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
  `EVAL_WORK_MAX` (64 MiB of cost per root module; past it every value left is unresolved);
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
