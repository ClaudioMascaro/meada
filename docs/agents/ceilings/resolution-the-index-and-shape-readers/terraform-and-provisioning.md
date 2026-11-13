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
  `iac/pattern.rs`: `PATTERN_LITERAL_MIN` (3 letters or digits; separators are no evidence), `PATTERN_SEGMENTS` (`-_./`)
  and `PATTERN_TEXT_MAX` (1,024 bytes for either name): a pattern outside these bounds matches
  nothing. Tests reject anchors below three letters or digits, wildcard boundaries outside those four separators, and text above the byte
  ceiling. These bounds keep the evidence meaningful and the matching work bounded.
  `crate::iac::provision::links` scans contract sides for each provisioner;
  IaC traces hydrate all provisioned sides before following the selected node's references.

- `iac.rs` static evaluation (#650, ADR 0025): `EVAL_VALUE_MAX` (64 KiB of one evaluated value,
  as `iac::size` counts it; past it, the value is unresolved, and a template past it is `*`
  whole), `EVAL_COST_MAX` (1 MiB of evaluation steps one expression could take, `iac::cost`'s
  upper bound, #714: a step is a byte made or, each time it runs in a `for`, one of the steps
  that make none: `TEMPLATE_PARSE_COST` (32) per byte of a template's text, whatever the text
  holds (`hcl-rs` parses a template again at each evaluation, so a long literal template
  repeated a few times is refused), and a byte of the root object a traversal reads (`hcl-rs`
  clones `var`, `local`, `each` or `module` whole to read `var.x[0]`); a template outside every
  `for` is not charged its parse; a `for` collection counts its own elements through any root,
  a conditional its larger branch; past it `hcl-rs` does not evaluate the expression, only its
  template parts resolve; the implementer measured about 0.2 s for an expression at the ceiling
  (15abf1c0, debug)) and `EVAL_WORK_MAX` (64 MiB of cost per root module; past it every value left is
  unresolved, but for a literal name that fits `EVAL_VALUE_MAX`. A refusal spends an upper
  bound of the bytes it went through to decide, never more than `EVAL_COST_MAX`: its own text,
  and the size of all the roots times two plus twice its reads of a root (`var`, `local`,
  `module`, `each`, `count`; a resource it names is none, and a read of `count.index` counts
  all the roots as a read of `var` does), so 9,999 refusals over a 21,000-element variable stop
  evaluating after 89 and take 2.3 s in process (1bdcc75d; a cold index of that root took
  4.30 s at b045c759 against 4.17 s on main 05585ce4, then 4.46 s at ae8139de against 4.40 s
  at b045c759, all debug), and a long expression refused in each instance of a `count` stops
  once its text has spent the work. The steps it charges bound a root of expressions near the
  ceiling: 80 resources took 16.6 s (the implementer's, 1bdcc75d, debug). It does not charge
  what no step counts: a template outside every `for` is parsed again once per `count` or
  `for_each` instance (10,000 interpolations: 0.72 s at `count = 1`, 4.41 s at 10, 48.4 s at
  100, the implementer's, 1bdcc75d, debug), one 1 MiB template of 130,000 interpolations took
  8.8 s, the work is per root, not per repository (20 roots of 80 resources near the ceiling
  took 234.75 s; both measured by a black-box verifier on a debug build), and the estimate
  itself is charged nothing: by a reading of `cost`, it sizes a bound value at each read of it,
  so an expression it then accepts pays that again per instance (ae8139de, debug, one run each:
  20,000 reads of a 21,000-element variable in a `for` over an empty collection took 2.6 s at
  `count = 1` and 17.3 s at 10, and 1.7 s and 4.8 s when each read is `var.big[0]`);
  converting an object literal for `hcl-rs` also spends the square of its keys, so a wide local
  evaluated in many module instances stops there; their nodes stay, named `*`). A var file's
  values are bounded the same way, each file with an `EVAL_WORK_MAX` of its own: a value past
  `EVAL_COST_MAX`, that work or `EVAL_VALUE_MAX` is given but unresolved (the default does not
  stand in for it), and one that is no constant is left out, as Terraform refuses it. A var file
  that does not read (past `MAX_BYTES`, with HCL objects past `OBJECT_WORK_MAX`, or broken) may
  give any variable: each is unresolved unless a file Terraform loads after it gives the value;
  `WIDTH_MAX` (`iac/functions.rs`: a `format` width of 256; past it the call is unresolved).
  `iac::Exact` puts integer arithmetic's exact value in place first; a result outside
  `i64::MIN..=u64::MAX`, a division with a remainder (`7 / 2`, 3.5 in Terraform) and arithmetic on a
  fraction are unresolved (in a var file, given but unresolved, so the default does not stand
  in); an operation on a `for` variable becomes a call of `meada_exact`, the same arithmetic at
  evaluation time (declared alone in a var file, which calls no function). A number
  `hcl-rs` would print otherwise than Terraform (past `i64::MIN..=u64::MAX`, more digits than a
  64-bit float holds, or a float it writes with an exponent, past 1e16 or below 1e-5) is
  unresolved too, as a literal or a `tonumber`, and so is a JSON number written with a fraction or
  an exponent or past that range, even `1.0`, in a `.tf.json` or a var file (`serde_json` keeps no
  digits). A negative zero (`-0`, `-(0)`, `0 * -1`), `-0` in Terraform, is unresolved even where
  only its value counts, since `hcl-rs` numbers hold no negative zero: an index, a comparison, and
  a `count`, which gives one node as any unresolved `count` does, where Terraform makes none. `cost`
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
  reader of the same local gets the cut value. A `for_each` over a list of strings expands unless
  Meada knows it is a tuple, which Terraform and OpenTofu refuse (#769): one written out (`[...]`,
  `[for ...]`), or a variable read whole that holds one (a var file's value or a default under
  `any` or no type, a declared tuple type, a module argument that is a tuple in its caller). Any
  other list of strings expands (`toset` gives a list too), so a list or a tuple both tools
  refuse, from a declared `list(...)`, a local or a function (`concat`, `keys`, `tolist`), names
  its elements at `exact`. `tolist`, `toset`
  and `tomap` convert their elements to one type as cty unifies them (scalars with a string
  become strings, objects of one set of attributes attribute by attribute and others to a map,
  tuples of one length element by element and others to a list, a number with a bool refused). A
  list value is read as a tuple, a set as a tuple and a map as an object, so a `tolist`, `toset`
  or `tomap` result beside a literal inside another conversion may be exact where Terraform
  converts more to strings, keeps the set's element type or refuses the call. A `for` binding a
  key over a `toset(...)` call, bare, in parentheses or a conditional's branch, is unresolved
  (Terraform's key is the element, or the index when the other branch is a list: a name whose
  conditional has a set in one branch is unresolved the same way, though Terraform unifies both
  branches to a list when the other is one), and so is one
  over a local or a variable that is such a call, another such name or a variable declared
  `type = set(...)`, but not one declared with another type (`list(...)`, `object(...)`, a
  primitive), which converts the value; over a set reached through an attribute, an index, a module output or
  another call, its key is the index (#717). A
  null beside a collection is unresolved, but for a `tomap` whose other values share one type.
  `toset` orders as cty
  does (strings bytewise, numbers by value, `false` before `true`, a null last); a set of
  collections, in cty's hash order, is unresolved. An object walks its keys in lexical order, as Terraform
  does: a value a name binds, a literal whose keys are written as names or strings, a `merge`,
  anything `jsonencode` writes. A `for` whose collection expression contains, anywhere, an object
  whose keys are known only once evaluated (a computed key, a comprehension) is unresolved, even
  where the order does not show (a `length` of the `for`, or a `values`, `lookup` or `jsonencode`
  of that object inside the collection), and a template `%{ for }` directive is not evaluated (a
  `*`). A file's own call of `meada_exact` is no function.
  A function over an unresolved argument is `*` whole, even
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
- Meada reads a directory as Terraform does, and OpenTofu 1.13 and later differ (#740; the
  maintainer chose Terraform). Terraform converts a variable's default to the type its own block
  declares and again to the type an override file gives; OpenTofu 1.13+ converts it once, to the
  final type. Meada follows Terraform in each row below and OpenTofu 1.13+ differs in each,
  under `type = set(string)` with a default `["b", "a", "a"]` (in the pair of override files,
  over a declared default `["x"]`): an override `type = list(string)` (Terraform `0-a`, `1-b`;
  OpenTofu `0-b`, `1-a`, `2-a`), `type = any` (the set, `a-a`, `b-b`; OpenTofu index keys),
  `type = tuple([string, string])` over `["b", "a"]` (Terraform rejects it, so Meada gives no
  exact name; OpenTofu `0-b`, `1-a`), an `a_override.tf` giving the default and a
  `b_override.tf` retyping it, a child module called without the argument with the override in
  its directory, and a `null` default retyped `tuple([string, string, string])` with a var file's
  `["b", "a", "a"]` (Terraform rejects the default, so Meada gives no exact name; OpenTofu `0-b`,
  `1-a`, `2-a`). The same default with no override, a restated `set(string)`, an override
  giving `type` and `default` together and a var file's value name the same in both. OpenTofu
  1.13.9 was run; that 1.12 and earlier behave as Terraform is read from the source, not run.
  The exception covers a variable all of whose blocks Terraform reads (`.tf`, `.tf.json`): it
  follows Terraform, whatever the override. A variable with a block in a file only OpenTofu
  reads (`.tofu`, `.tofu.json`), its declaring block or an override file's, is one Terraform
  does not read whole, and OpenTofu 1.8 to 1.12 (read from the source, not run) convert its
  default as Terraform does where 1.13+ converts once, so no value is certain for it: when an
  override retypes it (in any of `override.tf`, `*_override.tf`, `override.tf.json`,
  `override.tofu`, `*_override.tofu`, `override.tofu.json`) the default is unresolved where the
  two conversions hold other elements (a set's compared once converted, so `[10, 9]` and
  `["10", "9"]` are the same under `set(string)`; a list's as a name spells them, so
  `["010", "9"]` declared `list(number)` and retyped `list(string)` or `any`, `10` converted
  twice and `010` once, is unresolved) or another kind of collection, and exact where they
  agree, with OpenTofu 1.13's value (no override, an override restating the set, an override
  with its own default of the final type, a var file's value, and a default its types convert
  alike once and twice). `["10", "9"]` declared `list(number)` in a `.tofu` file and retyped
  `any` holds OpenTofu 1.13's strings, so a read that tells `"10"` from `10` names OpenTofu's
  instances, not the ones Terraform names with that override in a `.tf` file. A declaration
  Terraform reads, followed by a block in a file only OpenTofu reads, has a second reading,
  Terraform's without that file (#767): the variable is unresolved where the two give it another
  value or another type, even where both tools name alike (`main.tf` with the set above and an
  `override.tofu` giving `default = ["c"]`: Terraform `0-a` and `1-b`, OpenTofu `0-c`;
  `["10", "9", "10"]` declared `list(string)` and retyped `list(number)`: both `0-10`, `1-9`,
  `2-10`), and exact where they give one (an override restating the default or the type, a
  `.tofu` override's type and then a `.tf` one's, a var file's value or a module argument under
  one type). Types are compared as written, whitespace aside, and no `type` is `any` (`"010"`
  declared `string` and retyped `number`: Terraform `f-010`, OpenTofu `f-10`). Where OpenTofu
  refuses the directory, Terraform's reading is the variable's, and Meada tells one refusal: a
  string no number spells in a default a `.tofu` block retypes `list(number)` or `set(number)`
  (`["z"]` declared `list(string)` names `0-z`, and Terraform's instances of a var file's value). A
  default Terraform refuses under a type only OpenTofu reads names nothing exactly. A local
  overridden in a `.tofu` file keeps OpenTofu's value (`"b"` in `override.tofu` over `"a"` names
  `f-b`, Terraform `f-a`): only variables are compared. A module is judged by its own files,
  whatever calls it: a module of `.tf` files called only from a root written in `.tofu` follows
  Terraform though Terraform cannot read that root, and OpenTofu's answer differs.
- A variable's declared type converts what it receives (a default, a var file's value, a module
  argument) where Meada can make the conversion with certainty: a `set(T)` converts its elements,
  sorts and deduplicates them, and a `list(T)` of `string`, `number` or `bool` converts its
  elements in their order (`["010", "1.50"]` under `list(number)` names `10` and `1.5`, #770).
  An element Meada cannot convert with certainty leaves the whole value unresolved, `-0` under
  `number` among them: Terraform keeps a zero's sign and Meada's numbers do not. Meada converts
  no other type: a primitive, a `list(any)`, a list of collections or objects, a tuple, a map
  and an object keep the value as written, and a name read from one is exact even where
  Terraform converts an element and spells it otherwise (`"010"` given to `type = number`
  names `010`, where Terraform names `10`). A default its types refuse leaves the value of every
  source unresolved, as Terraform and OpenTofu refuse the configuration whatever gives the value
  (#769), and so does a default Meada cannot convert with certainty, though both tools take it:
  `["-0"]` under `list(number)`, given `["1", "2"]` by a var file or a module argument, names
  nothing exactly where both name `0-1` and `1-2`. A variable with a block in a file only
  OpenTofu reads is not checked this way: declared in a file Terraform reads, it is compared
  with Terraform's reading (#767, above).
