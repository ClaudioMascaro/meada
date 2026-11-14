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
  A YAML or JSON file that only this reader takes is data past a named ceiling of values
  (`VALUES_MAX`, 1,000, all its documents together, redacted values counted; #817,
  2026-11-12): it keeps none, and `meada status` counts it apart and names it. Terraform and
  OpenTofu files in every form, and a file a reader of a kind takes as its own (a document, a
  config file a language pack reads, Compose, a GitHub workflow or Skaffold file, an action
  file (ADR 0028), a package manifest, a Kubernetes object, a CloudFormation or SAM template, a
  Serverless service, an alert rule, route or receiver), keep every value, since the config use
  rule reads their rows.
  A Helm chart's values file is a generic file here: past the ceiling it keeps none.
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
- a sensitive Terraform variable, and every value a `.tfvars` file gives it;
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

(Amended 2026-11-12; ADR 0028.) **GitHub Actions** joins these formats: a file under
`.github/workflows/` that holds `jobs` (a pipeline) and an `action.yml` or `action.yaml` that
holds `runs` (an action). The files of other CI systems stay config values only.

### HCL reader

- HCL is read with `hcl-edit` (a span for every node) and `hcl-rs` (its expression evaluator),
  both MIT OR Apache-2.0. `.tf.json` and `.tofu.json` go through the JSON reader with
  Terraform's JSON syntax rules: an array of bodies may stand wherever a body does, the root of
  the file included, and each key of a `dynamic` property is the type of the blocks it
  generates. A file that does not parse is skipped and counted.
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
- **Sensitive.** The `default` of a top-level `variable` is `redacted`, whatever it holds,
  unless its `sensitive` attribute is absent, null, `false` or `"false"`: Terraform converts the
  string `"true"` to the boolean and refuses other values, so reading those as sensitive can
  only redact too much. Terraform merges the blocks of an override file (`override.tf`,
  `*_override.tf` and their OpenTofu and JSON forms) into those of the other files of its
  directory, so in a directory holding one, a `default` is also `redacted` when another
  Terraform file of the directory marks its variable sensitive, or does not parse. Such a file
  is first read with every default redacted, then again once all the directory's files are
  stored, so a marked default never reaches the index. A `.tfvars`, `*.auto.tfvars` or `*.tfvars.json` file has `redacted`
  every value it gives a variable whose name any Terraform file of the repository marks
  sensitive: a var-file can go to a module in any directory, before evaluation says which, and
  redacting too much is the safe direction. When a Terraform file in the var-file's directory or
  above is unreadable, the variables it declares are unknown and every value of the var-file is
  redacted. The stored summary of a `.tfvars` file, and of a Terraform file of a directory
  holding an override file, names what it was read against, and it is read again when that
  changes.
- **Shadowing.** A `.tofu` file hides the `.tf` file of the same base name in the same
  directory, and a `.tofu.json` the `.tf.json`; the hidden file is not read and not counted.
  Terraform, which ignores `.tofu` files, reads it, so beside an override file it counts as a
  Terraform file that does not parse: it may mark any variable of its directory sensitive
  (Sensitive, above; Override files, below). Elsewhere it changes nothing, as no other file
  takes its blocks.
- **Ceiling.** Brackets (`{`, `[`, `(`) nest at most 64 deep in one file, counted even inside
  a string; past it the file is unreadable, which `meada status` counts. The value length and
  the stored budget are the ones above.

### Terraform nodes and references (#649)

- **Roots.** A directory of `.tf` and `.tofu` files is a module. A `module` block whose literal
  `source` starts with `./` or `../` and, joined to its directory and normalized, stays inside
  the repository calls the module of that directory. A directory no other directory calls is a
  root module, labelled by its path (`.` at the repository root); every node records it, and a
  node of a module two roots instantiate is one node per root.
- **Override files.** Terraform reads a directory's override files (`override.tf`,
  `*_override.tf` and their OpenTofu and JSON forms) after its other files, in name order, and
  merges each of their blocks into the block of the same address: an argument it sets replaces
  the base block's, with its references, and its nested blocks of a type (a `dynamic` block
  counts as its label's type) replace all the base block's of that type. A `locals` entry or an
  `output` replaces the one of its name, and a `variable`'s default the declared one. A block
  with nothing to merge into, which Terraform refuses, is left out. A variable either block
  marks sensitive stays sensitive: an override's `sensitive = false` does not lift it, which
  can only leave a name unresolved. Beside an override file, a Terraform file that does not
  read (it does not parse, passes a ceiling, or a `.tofu` file hides it) may mark any variable
  sensitive once merged, so
  no variable of the directory is evaluated, as config values store none of their defaults.
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

- **What is evaluated.** A node's physical name (its `name` argument; a module call has none
  unless the type table names the input that holds it, as for #671's registry sources) and
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
  argument went through), or else its default, which no deployer sets. A sensitive variable
  is never evaluated, so its value never reaches a name.
- **Never evaluated**: data sources, resource attributes (even an argument the configuration
  sets), registry and remote modules, `terraform.workspace`, `path.*`, `-var`, any `-var-file`
  other than the ones Terraform loads by itself, `TF_VAR_` and any function outside the list.
- **Unresolved parts.** An expression that does not evaluate whole keeps what it can by its form:
  a template keeps its literal text and each interpolation that resolves, every other part (an
  interpolation that does not resolve, a directive) a `*`, consecutive `*` merged into one;
  `var.<x>`, `local.<x>` or `module.<call>.<output>` alone is that name's value, a pattern
  included; anything else (a function over an unresolved value, an operator, a conditional) is
  `*` whole. The name is then a **pattern**: `search` shows it as `pattern "orders-*"` in place of
  `name`, and `context` as `name: "orders-*" (pattern)`. Terraform provisioners match patterns
  against literal contract names under ADR 0026's rules (#656).
- **Expansion.** A `count` that resolves to a whole number, or a `for_each` that resolves to an
  object or a list of strings (`toset` sorts and dedupes strings), gives one node per key, up to
  `EXPANSION_MAX`; a `count` or `for_each` that reads a module output is evaluated before the
  module has instances, so it does not resolve.
- **Ceilings.** Besides the ones above, `EVAL_VALUE_MAX` (64 KiB of one value), `EVAL_COST_MAX`
  (1 MiB of steps one expression could take, an upper bound computed from the sizes of the values
  it reads, products for a `for`, `formatlist`, `join` and `replace`, and, in a `for`, the
  template text `hcl-rs` parses again and the root objects it copies at each evaluation) and
  `EVAL_WORK_MAX` (64 MiB of that cost per root module, a refused expression spending the bytes
  it went through to decide, its text and the roots it read, at most `EVAL_COST_MAX`); past
  each, the value is unresolved, but for a literal name that fits `EVAL_VALUE_MAX` once the
  work is spent: it has nothing to evaluate. A `format` width past
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
  are instantiated under it, each base first and each component over what the kustomization
  that lists it holds so far (its resources' objects, which it changes and adds to), and each
  kustomization on the way then applies
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
  its rules and default backend, each in the same namespace and the same unit, all `exact`, with
  the `via` naming how the source names the target. A ConfigMap or a Secret is renamed with the
  references to it at each stage. A Service, and a `serviceAccountName` of the kinds Kustomize
  renames (Pod, Deployment, StatefulSet, DaemonSet, ReplicationController, Job, CronJob), resolve
  as Kustomize's name reference does once the tree is built: among the objects of the
  referrer's namespace (none and `default` alike) a stage renamed or moved from the name as
  written, the only one, else those whose prefix and suffix stages end as the referrer's do (an
  empty list matching any, then strictly); with none left the reference names the object called
  so, and with several it names none (Kustomize refuses that tree). Another kind's account, or
  the deprecated `serviceAccount`, which Kustomize leaves alone, names the account called so.
- **Images.** The images of a workload's containers once the overlay's `images` applied are
  stored with its node and shown by `context`; the image contract of ADR 0026 reads them.
- **Storage.** The nodes and references of Kubernetes join the Terraform ones in `iac_nodes`
  and `iac_references`; `iac_images` and `iac_unresolved` hold the rest. All are rewritten
  whole when a file that holds, or may hold, an object (a kustomization, or a YAML or JSON file
  that says `apiVersion`) is read again or leaves.

### Compose nodes (#652)

- **Unit.** A directory's Compose file (the first of `compose.yaml`, `compose.yml`,
  `docker-compose.yaml`, `docker-compose.yml`) and the override file of the same family
  (`compose.override.*`, `docker-compose.override.*`) are one evaluation unit, rooted at the
  directory (`.` at the root). An override with no base of its family is no project. The merge
  follows Compose for what is read: a scalar the override sets replaces the base's, the lists add.
- **Nodes.** Each service is a node `services.<name>`, type `service`, no physical name, at the
  base file's lines (the override's when it adds the service).
- **References.** `depends_on` (a list or a mapping), `links` (without the alias) and every other
  service on a network both list, each `exact`, with `via` `depends_on`, `links` or
  `networks.<name>`. A service with no `networks:` is on the project's `default` network, as in
  Compose, and shares it with the others. A name no service has is dropped.
- **Variables.** `${VAR:-d}` and `${VAR-d}` resolve to `d` (itself expanded) and mark the text
  overridable; `${VAR}`, `$VAR`, `${VAR:?e}` and `${VAR:+a}` stay in the text as `${VAR}`, the
  unresolved part the image contract turns into a wildcard; `$$` is `$`.
- **Image and build.** The node records the expanded `image` and the build context, each with its
  overridable mark. A context is kept only as a path inside the repository from its root (`.` for
  the root itself); an absolute path, a URL, a path that climbs out of the root or one with an
  unresolved part is not recorded. No file is touched, so no symlink is followed.

### Crossplane objects (#655)

- **Recognition.** Crossplane objects are Kubernetes objects, read and addressed by #651's
  rules, and each node records what it is to Crossplane, its role: a `CompositeResourceDefinition`
  (XRD) or a `Composition` of `apiextensions.crossplane.io`; a claim or a composite resource, an
  object whose group and kind are the claim or composite kind an XRD anywhere in the repository
  defines; a managed resource, any other object holding a `spec.forProvider` mapping (every
  provider's managed resources have one, whatever their group); and a composed resource (below).
  Every kind of `apiextensions.crossplane.io`, `ops.crossplane.io` and `pkg.crossplane.io` is
  cluster-scoped: no namespace in its address, and no Kustomize `namespace` sets one.
- **Composed resources.** Each resource a composition creates from a literal `base`, in
  `spec.resources` or in the `input.resources` of a pipeline step whose input is function
  patch-and-transform's (`pt.fn.crossplane.io`), is a node under the composition,
  `<composition address>/<resource name>` (its index when it has no name), typed by the base's
  kind, its span and block those of the resource. Its physical name is the base's
  `crossplane.io/external-name` annotation, else its `metadata.name`, as written; a managed
  resource's is its external name too, since that is its name in the provider's API.
- **Never evaluated.** Patches, patch sets, transforms and composition functions. A resource
  whose name or external name a patch writes (directly or through a patch set, a patch from a
  field path with no `toFieldPath` writing the path it reads; a patch that writes the composite
  or the environment does not count), or that has neither, has the name
  `*`, a pattern (Crossplane generates or patches it at run time). A pipeline step that runs any
  other function creates no node: its input is a template or a program.
- **References.** Within one evaluation unit, as every Kubernetes reference, all `exact`: a
  claim to the XRD of its composite kind (`composite <Kind>.<group>`), a composite resource to
  its XRD (`kind ...`), a claim or composite resource to the composition its `compositionRef`
  names, an XRD to its `defaultCompositionRef` and `enforcedCompositionRef`, a composition to the
  XRD of its `compositeTypeRef` (`compositeTypeRef ...`) and to each resource it creates
  (`resources`, `pipeline <step>`), and a composed or managed resource to the definition of its
  kind in the unit, an XRD (a nested composite) or a `CustomResourceDefinition` (`kind ...`). A
  kind is a node only through its definition, so a managed resource kind whose provider CRD the
  repository does not hold is reached by nothing; the composed resource still carries it as its
  type. The chain from a claim to its composite and to what its composition creates is the walk
  claim, XRD, default composition (or the claim's `compositionRef`), composed resources.
- **Storage.** The role is a column of `iac_nodes` (NULL for any other node), shown by `context`
  as `role:`; the composed resources are rows of `iac_nodes` and `iac_references` like any
  object's.

### CloudFormation and SAM nodes (#653)

- **Unit.** A YAML or JSON file that says `AWSTemplateFormatVersion` or the SAM transform
  (`AWS::Serverless-2016-10-31`, alone or in a list) and holds a `Resources` mapping is one
  evaluation unit, its root labelled by its path. Nothing joins two templates (a nested stack, an
  `Fn::ImportValue` and a `Fn::Transform` are not followed), so a changed template is evaluated
  again alone and no other root is touched.
- **YAML tags.** The event reader has a mode that reads a node tagged `!Name` as the mapping
  `{"Fn::Name": node}` (`!Ref` and `!Condition` as `Ref` and `Condition`), the long form JSON
  templates use, so both syntaxes reach one evaluator. The mode is opt-in: the config values and
  every other reader still ignore tags.
- **Nodes.** Each resource with a `Type` of the form `Provider::Service::Resource` or
  `Custom::Name` is a node at its logical id, with that type, at the lines of its block. A
  resource with no such type is no node. The `::` also tells these rows from Kubernetes kinds and
  Terraform types, which never hold one, so each refresh rewrites its own rows only.
- **References.** `Ref`, `Fn::GetAtt` (`[Resource, Attribute]` or `Resource.Attribute`, the
  attribute in the `via`: `Fn::GetAtt.Arn`; a computed attribute, `[Resource, !Ref Param]`, gives
  the `via` `Fn::GetAtt`), the `${Resource}` and `${Resource.Attribute}`
  variables of `Fn::Sub` that its own variable map does not define (`${!Text}` is a literal) and
  `DependsOn` (a string or a list), wherever they sit in the resource, each `exact`. A name that
  is no resource of the template (a parameter, a pseudo parameter) is no reference.
- **Evaluation.** The property that holds a type's physical name (`QueueName`, `TopicName`,
  `BucketName`, `TableName`, `FunctionName`, ...) is evaluated with `Ref`, `Fn::Sub`,
  `Fn::GetAtt` and `Fn::Join` over the template's own values: a parameter's default (the
  overridable value of the rule for evaluated names, recorded with its line), another resource's
  name where the type says `Ref` returns it (a bucket, a table, a function) and `Fn::GetAtt` of
  the attribute that is that property, and literals. A pseudo parameter (`AWS::Region`), an
  `Fn::ImportValue`, any other function, a parameter with no default, of a list type, of an SSM
  parameter value type or with `NoEcho`, and an attribute the type does not say are unresolved
  parts: a `*`, and the name a pattern. A resource that sets no name (CloudFormation generates it) has none.
- **Names by type.** A short table maps the types the contract kinds will read to their name
  property; the type table of the spec replaces it. A type outside it is still a node, with its
  references, and has no physical name.
- **Secrets.** A parameter with `NoEcho` (true, or a string that reads as true: `true`, `yes`,
  `on` or `y` in any case, since CloudFormation's YAML 1.1 types the plain ones as booleans and
  redacting too much is the safe direction) in a template has its `Default` stored as `redacted`
  among the config values, and no name is resolved through it.
- **Limits.** `INTRINSIC_DEPTH_MAX` (nesting of functions and of resources naming each other),
  `PHYSICAL_NAME_MAX` (a name longer is unresolved), and the shared `NODES_MAX`,
  `REFERENCES_MAX` and `ROOT_STORED_MAX`, each fed an input past it by a unit test. The hostile
  probes hold the reader to a file of the size ceiling.

### Serverless Framework nodes (#654)

- **Unit.** A file named `serverless.yml`, `serverless.yaml` or `serverless.json` that holds a
  `service` and a `provider`, `functions` or `resources` is one evaluation unit, its root labelled
  by its path, rewritten with the CloudFormation rows it owns (a function's type holds a `::`, so
  `iac::Owner::CloudFormation` covers it). `serverless.ts`, `.js` and the files `${file(...)}`
  names are not read.
- **Nodes.** Each entry of `functions` is a node `functions.<name>` of type
  `Serverless::Function`, at the lines of its block, with its physical name (its `name`, else
  `<service>-<stage>-<name>`, the stage `provider.stage` or `dev`, overridable as `opt:stage` at
  `provider.stage`'s line or the function's, since Serverless names such a function after
  `--stage` first, #709), its `handler` and its `events` recorded (`iac::Node::handler`,
  `events`; `iac_nodes` gains the columns). An event is its type, then what identifies it:
  `http GET /users` (method upper-cased, path with its leading slash),
  `httpApi ANY /v2`, `sqs Queue.Arn` (a `Fn::GetAtt` as `Resource.Attribute`, a `Ref` as the
  name), `sns <topic>`, `schedule rate(5 minutes)`, `s3 <bucket>`, `websocket <route>`; an
  intrinsic it cannot read is a `*`. The `resources.Resources` mapping is read as the
  CloudFormation reader reads `Resources`, nodes at their logical ids, with the types and name
  table of ADR 0025's CloudFormation section.
- **Variables.** Over the file alone: `${self:a.b}` is the value at that path of the file, itself
  expanded (a number or boolean as its text; a mapping or list is unresolved); `${opt:name,
  default}` is the default and the name or text is overridable, its `Overridable` named by the
  expression and placed at the line that writes it, since a deployer passes `--name`. The
  alternatives of `${a, b, c}` are tried in order: a quoted or bare literal ends them, a `self:`
  path the file lacks falls to the next, an `opt:` is passed over and marks the result
  overridable. Any other source (`env`, `ssm`, `sls`, `aws`, `cf`, `file(...)`, `s3`) is an
  unresolved part and ends the chain, whatever follows it: a `*` in a name, `${expr}` in a
  recorded text. A variable inside a variable is expanded first, within its alternative, and must
  give a text, read as one source (a comma in it is no alternative); `provider.stage` left out is
  `dev` to `${self:provider.stage}` as to a name, and is no override there, since `self:` reads
  the file and never `--stage`. A value that names itself, directly or around a cycle, is
  unresolved whole, whichever value of the cycle is met first. `${...}` with no source, or with
  `::` (`${AWS::Region}`), is CloudFormation's and stays.
- **Resources.** The section's strings are expanded before the CloudFormation reader sees them: a
  string that resolves is that text; one with an `opt:` default becomes a `Ref` to a synthetic
  parameter holding it (shown as the `opt:` expression), one with an unresolved part an
  `Fn::ImportValue`, joined with its literal text (inside an `Fn::Sub`, the same two as
  variables), so the one evaluator gives names, patterns and overridable values.
- **References.** A function references the resources of the section its body names with `Ref`,
  `Fn::GetAtt` and `Fn::Sub`, each `exact` with its `via`, as a resource does. Serverless's own
  logical ids for functions (`HelloLambdaFunction`) are not followed.
- **Limits.** `VARIABLE_DEPTH_MAX` (the height of a chain of `self:` values), `STACK_MAX` (the
  walk's depth), `VARIABLE_BYTES_MAX` (the bytes one file's expansions produce), `EVENTS_MAX` (per
  function), the CloudFormation reader's `PHYSICAL_NAME_MAX` (a longer text is unresolved), and the
  shared `NODES_MAX` (functions and resources together), `REFERENCES_MAX` and `ROOT_STORED_MAX`,
  each fed an input past it by a unit test.

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
- (Amended 2026-11-12; ADR 0028.) GitHub Actions: the key path of the block in the grammar of
  "Key path" above. A CI job is `jobs.build`, and `jobs["publish-prd"]` when its id is not a
  plain identifier; a CI step is `jobs.build.steps[2]` or `runs.steps[0]`; the pipeline or the
  action itself is `.`. A CI step's address is positional: its `id`, `name` and `uses` are
  selectors, never identity. Only the first YAML document of such a file is read.

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
- Crossplane: claims to composite resources to composed resources;
- (Amended 2026-11-12; ADR 0028.) GitHub Actions: a pipeline to its CI jobs, a CI job
  or a composite action to its CI steps, and a CI job to the CI jobs its `needs` names.

References per node are a named ceiling.

### Evaluation unit

An evaluation unit is the set of files evaluated together: a Terraform module directory (each
local module instantiated once per call), a Kustomize tree per overlay, a CloudFormation or SAM
template, a Compose project, a Serverless service file. A change to any file of a unit
re-evaluates the whole unit. Links across repositories resolve at link time, as today.
(Amended 2026-11-12; ADR 0028.) A pipeline file and an action's manifest are each one
evaluation unit.

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
- (Amended 2026-11-12; ADR 0028.) **GitHub Actions.** A `${{ }}` expression resolves
  only where a literal `env` of the CI step, the CI job or the pipeline gives its value. Anything else
  is an unresolved part, and `secrets.X` and `vars.X` have no value in a repository.
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
- #671 implements a bounded inventory of 43 custom resource types plus RabbitMQ topology
  roles, documented in capabilities, and six named terraform-aws-modules sources. Crossplane
  provider identities require explicit external-name annotations; literal composition bases
  qualify, while patches that could alter identity or routing suppress the projection.
  `[[iac.module]]` selects `source` or a component-bounded `source_prefix`, `entry` and `input`;
  `[[iac.custom_resource]]` selects `group`, `kind`, `entry` and `key_path`. Exact sources win,
  then the longest prefix. Mappings reuse the existing readers and evaluator; they never
  download modules. Their digest and the embedded table digest invalidate affected indexes.
  Physical node names remain separate from canonical contracts and explicit absent projections.

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
- (Amended 2026-11-12; ADR 0028.) `search` gains the kind `ci` for the nodes of the
  GitHub Actions format, matched by address, type and a CI step's `id`, `name` and `uses`. The
  kind `iac` keeps its answers, the default search still includes no IaC node, and the MCP tool's
  kind list gains `ci`.

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
