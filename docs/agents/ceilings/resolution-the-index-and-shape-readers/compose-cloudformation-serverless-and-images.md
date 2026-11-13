# Compose, CloudFormation, Serverless and images

- `iac/actions.rs` (#823, ADR 0028): a pipeline or an action file reuses `NODES_MAX` (the file,
  then its CI jobs in the order of their ids, each with its CI steps; past it the rest of the file
  is left out), `REFERENCES_MAX` (per node: a pipeline of more than 64 CI jobs, or a CI job whose
  `needs` and CI steps pass 64 together, keeps the first ones, `needs` before steps; a CI step past
  it is still a node `search` finds, with no reference from its CI job) and `ROOT_STORED_MAX` (per
  file: addresses, names, `id` and `uses`, and references; a CI step's address repeats its CI
  job's id). A `name`, an `id` or a `uses` longer than `config_values::VALUE_MAX` is kept neither
  as a name nor as a selector. A CI step's identity is positional (`jobs.build.steps[2]`): a CI
  step inserted before it renumbers it; `id`, `name` and `uses` are selectors, never identity, and
  a selector's value is compared whole and case-sensitively. A `strategy.matrix` multiplies
  nothing: a CI job with one is one node. Only the first YAML document of a file is read. A file
  past `lang::MAX_BYTES`, one that is not UTF-8 and one the event reader refuses give no node. A
  CI job or a CI step that came by a YAML alias has no lines of its own: a step sits at its CI
  job's first line and a job at line 1, since the reader hands a location only where a mapping is
  written. A pipeline is a file directly under `.github/workflows/`, and with `action.yml` and
  `action.yaml` in one folder the action is `action.yml`, whatever each of them holds. The
  extension is `.yml` or `.yaml` in lower case. The file node spans the whole file, the documents
  after the first included. A CI job id written twice is the last block, while the config values
  keep both. A merge key (`<<`) is not expanded: what it would bring gives no CI step. The config
  values have their own budget for what aliases repeat: past it the file counts as unreadable
  there and stores no value, and its nodes stay, with `values: none` (found by #823's
  verification).
- `ci.rs` (#824, ADR 0028): a CI contract links what is written, and Meada resolves no Git ref.
  Only a ref of 40 or 64 hexadecimal digits is a commit id: an abbreviated one reads as a name
  and never gets `ref mismatch`, and a branch or a tag of that shape reads as a commit id and
  gets it. A CI job with a `strategy.matrix` is one consumer. A `uses` writes no host, so a call
  from one host to another (GitHub Connect) has no link, and a repository without `origin` names
  no peer. Owner and repository are lower-cased; the path is compared as written, so a call that
  differs from the file's name by case is a consumer with no link. The keys of `with` and
  `secrets` are kept sorted, not in file order, each up to `config_values::VALUE_MAX` and within
  the file's `ROOT_STORED_MAX`; a `secrets` that is neither a mapping nor the word `inherit` gives
  none. The indexed commit is the checkout's `HEAD` even when the checkout is dirty, read with
  one `git` call per repository that holds a reusable pipeline on each query; a provider whose
  repository records no commit (none made yet, a server-mode index without its recorded commit)
  reads `@<ref> (indexed commit unknown)`, with no flag. A `uses` that holds a
  control character names no pipeline, and a key of `with` or `secrets` that holds one is left
  out: GitHub takes neither, and each would break a line of `context`. On that line a key named
  `inherit` reads like the word, and a key that holds `, ` or `; ` like several; `meada export`
  tells them apart. The side lines close `context` on a pipeline, after its values, so a
  pipeline of many CI steps shows them only under a larger `budget`. A `uses` with two `@` has
  the ref after the first. `trace` (#825) follows a CI call only between CI nodes of the
  workspace and adds no cap for GitHub's ten levels of nesting: a chain ends at a cycle mark or
  at the budget, and the walk from an IaC node has no depth limit of its own. It reads the
  whole chain before the budget cuts the answer, so a chain of thousands of pipelines costs
  seconds. A CI job past the 64 references of its pipeline (`REFERENCES_MAX` of
  `iac/actions.rs`) has no file node above it: `trace` up ends at that CI job, and down from
  the pipeline never reaches what it calls. A contract identifier is matched as written: one
  with the owner in another case, or a trailing `/`, starts nothing.
- `ci.rs` (#826, ADR 0028): a CI step's `./<folder>` is read in the CI runner's workspace, which
  Meada knows to be the file's repository in one position only: a pipeline that is not
  reusable, after an `actions/checkout` step of the same CI job, with no `actions/checkout`
  before the CI step that names `repository` or `path` in `with` (the key in any case, as
  GitHub reads an input). It is unresolved in a reusable
  pipeline (whatever other triggers it has), where a checkout is of the caller's repository, and
  in a composite action, which runs in its caller's workspace; with no checkout before it; and
  after a checkout that names one of the two keys, also when the key names the own repository (`${{ github.repository }}`) or a
  path the folder is not under. Only `actions/checkout` at any ref counts, owner and name in
  any case: a fork of it, a wrapper action or a `git clone` in a `run` is no checkout, a
  checkout under an `if` counts whatever its condition, and a
  step that removes the files afterwards is not seen. A checkout's `ref` is not read: the link
  is to the own repository at its indexed commit. An unresolved CI step holds no candidate and
  `meada decide` offers nothing for it. A decision written by hand for it (`kind =
  "dynamic-url"`, its `target` a CI provider) is accepted for any CI provider of the workspace,
  an action or a reusable pipeline alike: it adds a `decided` link that `trace` and the
  provider's `context` follow and closes the item, while `context` on the CI step still says
  `consumes (unresolved)`.
  `$/<path>` reads on every host, though GitHub Enterprise Server does not have it. A path is
  normalised by the path rule of `iac::joined` in every form (`.`, `..`, inner and trailing
  empty segments), which GitHub may refuse in a call by owner and name; one that leaves the
  root, or starts with an empty segment (`.//x`), holds no side. A
  CI step that names a pipeline file and a CI job that names anything else hold no side, so a
  CI job's `./<file>@<ref>` holds none; a CI step's `./<folder>@<ref>` or `$/<folder>@<ref>`,
  which GitHub calls invalid, reads as a folder whose name holds the `@`. A `uses` that holds
  `${{` names nothing: GitHub evaluates no expression there. A call by owner and name is cut
  at its first `@` (`<folder>@v1@v2` reads the ref `v1@v2`), and a CI job's ref written
  `refs/heads/<branch>`, which GitHub does not allow, links as any other. An action in a folder named as a pipeline file
  (`.github/workflows/x.yml/action.yml`) provides nothing.
- `ci.rs` (#827, ADR 0028): the absent mark says what the index of the provider's repository
  holds at its indexed commit, never what the Git ref of the call holds: Meada resolves no ref,
  so a file that exists only at the ref is marked, and one removed there is not. A file Meada
  could not read (past `lang::MAX_BYTES`, not UTF-8, refused by the event reader) gives no node
  and reads as absent, and so does a path that differs from the file's by case. The second
  reason (`the file is no reusable pipeline there`) needs the pipeline's file node: a file under
  `.github/workflows/` with no `jobs` is no pipeline and gets the plain mark. A folder that
  holds a file named exactly `Dockerfile` and no action file is no provider, and a call to it
  holds no side at all, by owner and name, `$/` or `./` alike: no mark, no orphan line, no open
  item (`dockerfile`, `Dockerfile.dev` and a `Dockerfile` the walk skips do not count, and the
  call is then marked absent). The set of those folders is stored per repository (table
  `dockerfiles`), every folder of the walk, with no ceiling of its own. The mark and the orphan
  rule scan every CI side of the workspace once per answer. An orphan line needs a consumer
  with a contract in the scope: a marked consumer opens it, an external `uses` and an
  unresolved CI step do not, and a provider consumed only by a call to itself is no orphan.
  `meada status` reports and does not index: a repository never indexed prints `not indexed`
  and no orphan line. The `absent` field of `meada export` follows the links of that same
  export: a consumer row of the kind with no link.
- `iac/serverless.rs` (#654, ADR 0025): a service reuses `NODES_MAX` (functions first, in name
  order, then the resources of its `resources` section, together), `REFERENCES_MAX` (per node) and
  `ROOT_STORED_MAX` (per file, the functions' names, handlers and events counting against it before
  the resources) and adds `VARIABLE_DEPTH_MAX` (8 `self:` values or nested variables inside one
  another, counted as the height of a value, 1 plus that of the tallest `self:` value it uses: a
  value taller than that is unresolved, wherever it is met, and so is each value above it; each
  value is expanded once and memoized, so a diamond or a wide fan-out costs one expansion per path,
  and each value of a cycle is unresolved whole), `STACK_MAX` (64 `self:` values followed from one
  another before the walk stops; a value met there is not expanded, and a value within
  `VARIABLE_DEPTH_MAX` above it is memoized only for a reach at its depth or deeper, so it is
  expanded again when met nearer the top, at most 65 times, and answers the same in any order),
  `VARIABLE_BYTES_MAX` (the bytes one file's expansions may produce, `ROOT_STORED_MAX`; past it a
  variable is unresolved, so which ones depends on the order of the file) and `EVENTS_MAX` (64
  events recorded per function, the rest left out); a text past `cloudformation::PHYSICAL_NAME_MAX`
  is unresolved, and its expansion stops there. A `resources` section given as a list (`- Resources:
  ...`, often of `${file(...)}` parts) is not read: only the mapping `resources.Resources` gives
  nodes. Only `serverless.yml`, `.yaml` and `.json` are read: `serverless.ts`, `serverless.js`,
  `${file(...)}` values, `serverless.*.yml` variants, values from other files, `provider` defaults
  other than `environment` (such as `iam`), plugins and a `functions` list (the v1 form) are not, and a function given
  by `${file(...)}` is a node with no handler or events. `--stage` is modelled only in a function's
  default name (`opt:stage`, #709; a stage with an unresolved part, a pattern, lists it not):
  `${self:provider.stage}` reads the file, as Serverless v3's `self:` source does, so a name
  through it is no override; besides that only an `opt:` in the file marks a value overridable, and an `opt:` whose fallback is anything but one text is
  unresolved. Serverless's own logical ids for functions and its generated resources (roles, API
  gateway, log groups) are not nodes, so a `Ref` of `HelloLambdaFunction` references nothing. A
  string with an unresolved part that sits where the CloudFormation reader needs plain text (`Type`,
  a `Ref` target, a `DependsOn`) makes that field unread. An event's `arn` that is an `Fn::Join` or
  `Fn::ImportValue` is recorded as `*` and gives no reference, and a resource's node starts at the
  nearest line above its first scalar that starts with its logical id (the key-line fallback), since
  the event reader gives key lines two levels deep only. A reference to a resource the node ceiling
  or the byte room left out is dropped, and an `opt:` default that only a handler or an event went
  through marks that text `(overridable)` but is not listed under the node's `overridable:`, which
  holds the name's.

- `iac/cloudformation.rs` (#653, ADR 0025): a template reuses `NODES_MAX` (resources past it in
  logical id order are left out), `REFERENCES_MAX` (per resource, in address order) and
  `ROOT_STORED_MAX` (per template: the cut is by the template's own budget, with no
  repository-wide one, since each template is its own unit and a file is read once) and adds
  `INTRINSIC_DEPTH_MAX` (32 functions, or resources naming each other, inside one another; a
  deeper name is unresolved, and a name evaluated through a cycle or cut at that depth gets the
  partial result, memoized, so a resource named through one has it in logical id order: an
  evaluation per path of a diamond of references would be exponential), `PHYSICAL_NAME_MAX` (1
  KiB; a name that grows past it is unresolved, and so is a literal or a parameter default
  longer than that, before it is copied) and `iac::OVERRIDABLE_MAX` (32 parameters one name goes
  through; past it the name is unresolved). An `Fn::Sub` evaluates each variable it names once,
  however often it names it. Only `Ref`, `Fn::GetAtt`, `Fn::Sub` and
  `Fn::Join` are evaluated: `Fn::Select`, `Fn::Split`, `Fn::FindInMap`, `Fn::If`,
  `Fn::ImportValue`, conditions, mappings and `Fn::Transform` leave an unresolved part. The name
  property per type is a short table (`NAMED`), so a type outside it has no physical name until
  the type table of spec #642 lands, and a `GetAtt` is resolved only for the attribute named like
  that property; one with a computed attribute (`[R, !Ref P]`) is a reference `via Fn::GetAtt`
  that resolves no name. A `NoEcho` is read as true for `true`, `yes`, `on` and `y` in any case,
  quoted or not (YAML 1.1 types the plain ones as booleans, this reader cannot tell), and no other
  YAML 1.1 boolean is typed (a parameter `Default: yes` is the text `yes`). A parameter of an
  `AWS::SSM::Parameter::Value<...>` type or a list type gives no name. Nested stacks, SAM's
  implicit `ServerlessRestApi` and its generated roles, `Globals` outside function environment defaults, `Fn::Transform` macros and
  `AWS::Include` are not expanded, and a `Transform` that names only another macro makes no
  template. A YAML template needs `AWSTemplateFormatVersion` or the SAM transform, as a JSON one
  does. A template past `lang::MAX_BYTES` is no template (config values count it as unreadable). A
  node starts at its key's line (read by the event reader, `document::events::read_keyed`; for an
  entry that came by an alias, `services: *all`, the nearest line above that starts with the key)
  and ends at its last scalar (an aliased section's entries all end at the alias's line, backed
  over lines no deeper than the key; a block scalar's line is its first line of text), and a line
  broken by a lone CR is a break to the reader, so a Compose file written so has its services' key
  lines where the reader puts them (the text scan before #653 had them one line late). The table
  `end_of` looks block ends up in costs 4 bytes per line of the file per level, built once per
  file, only for a block whose end is more than 64 lines from its last scalar. A `Fn::GetAtt` in
  dotted string form splits at its first dot, so an attribute with dots is kept whole.

- `iac/compose.rs` (#652, ADR 0025): a Compose unit reuses `NODES_MAX`, `REFERENCES_MAX`,
  `ROOT_STORED_MAX` and `STORED_MAX` (services past `NODES_MAX` in name order, and references past
  `REFERENCES_MAX` per service, are left out; shared-network references take the first services of
  the network in name order) and adds `DEFAULT_DEPTH_MAX` (8 defaults inside each other; a deeper
  one is left unresolved). Only the four Compose names and their `.override` files are read (no
  `-f` list, `include:`, `extends:`, `COMPOSE_FILE`, profiles or `.env` file: a variable's value
  never comes from the environment), only `image`, `build` (with its `dockerfile` and `dockerfile_inline`), `environment`,
  `depends_on`, `links` and `networks` are read, a service's lines end at its last scalar, and a service with no
  `networks:` is on `default`, so a project of thousands of such services reaches
  `ROOT_STORED_MAX` through its shared-network references. A build context is judged by its text: a path inside the repository that is a symlink out of
  it is recorded (no file is touched), so the image contract must check it before reading a
  Dockerfile there (#744, `image/dockerfile.rs`). The `build` keys `dockerfile` and
  `dockerfile_inline` are read too; the inline text counts against the unit's `ROOT_STORED_MAX`
  with the rest of the node, and `additional_contexts` is not read. A service with a `network_mode` is on no `default` network. A YAML merge key
  (`<<: *anchor`) is followed at any depth and through chains of anchors. A service in an override file but no base file has the
  override's path. An override merges as Compose does: a `build` with no `context` keeps the
  base's, lists add to the base's, and `!reset` (clear) or `!override` (replace) on a read key
  drops the base value. A changed Compose file evaluates again the root of its directory, with Terraform's.


- `IMAGE_STORED_MAX` (`image/config.rs`): 4 times `lang::MAX_BYTES` per configuration file,
  bounding the image facts when a long job key is repeated for many tags. Past it further
  builder and direct-code facts are left out. Direct-code summaries charge node identity, root,
  path and reason before copying them. The reader shares the event/XML/Gradle depth and file ceilings;
  unresolved image patterns share `PATTERN_LITERAL_MIN`, `PATTERN_SEGMENTS` and `PATTERN_TEXT_MAX`.
  The orphan rows of `status` (`scope_status`) and the workspace's pattern pass
  (`patterns::prepare`) compare every image builder with every runner, with no index: 4,000
  patterns took 54.6 s for a `status` in a debug build.

- `PATHS_MAX`, `WORK_MAX` (`image/dockerfile.rs`) and `LIST_MAX` (`contract.rs`, #744): a Compose
  build runs the paths its Dockerfile takes from the context. A Dockerfile naming more than
  `PATHS_MAX` (256) sources, or matching more paths than that, is not read; the wildcard sources of
  one Dockerfile examine at most `WORK_MAX` (16,384) directory entries (a `*/*/*/*` source walks
  the tree, so past it the Dockerfile is not read); a directory past `LIST_MAX` (4,096) entries is
  not listed, so a wildcard through it leaves the Dockerfile unread. "Not read" is the fallback of
  ADR 0026: the whole context at `inferred`, the reason naming the cause. The fallback is also the
  answer for every form the reader does not parse: a source or mount with a variable, a quote or
  an escape, a mount type with a variable, a `**` source under `COPY --parents` (Docker's pattern
  matcher lets it cross directories), a `${...}` other than `${name}` on a line with `<<` (Docker
  splits it into words of its own), a source that climbs out of the context, an `# escape=`
  parser directive other than `\` (the other directives change nothing it reads), a Dockerfile
  behind a symlink or longer
  than `lang::MAX_BYTES`, a flag with a quoted value or one it does not list, an instruction it
  does not list, a heredoc with no end, and a Dockerfile outside the repository. A source that
  names nothing on disk is skipped (the build would fail), `.dockerignore` is not read (a path may
  cover files the image excludes, never fewer), `ONBUILD` instructions are ignored, and a path
  nested in another taken path is kept. The
  fallback's executions are stored in the room `IMAGE_STORED_MAX` leaves; past it a build gives
  none, as every other direct-code fact does.

- `ENVIRONMENT_BYTES_MAX` (`iac/environment.rs`, #663): one `lang::MAX_BYTES` (1 MiB) of
  environment expansion per workload, charging overwritten entries too so repeated `envFrom`
  references cannot multiply work without bound. Kustomize reference renames charge the same
  budget per workload and stage, before copying a transformed name. Past it the workload retains no environment
  entries, since a partial list could miss an override. Values beyond `VALUE_MAX` stay
  unresolved; Secret values stay absent. Compose charges repeated service names and images
  before copying them into entries; Kubernetes shares a container name while reading inputs.
  Unit/repository IaC budgets also charge these entries and multi-container images, typed
  targets, projected provisioned names and routing evidence. Lambda/SAM also charge repeated
  container/image strings before copying environment entries, including overridden entries.
  Lambda/SAM cap environment room after fixed image/code facts; ECS can discard an unread or
  oversized environment without discarding known images. Its direct JSON-encoder field scans
  spend `EVAL_WORK_MAX`, and computed encoded values keep the evaluator's `EVAL_VALUE_MAX`.
  Temporary owned map keys consume expansion room before cloning. Unknown SAM/Serverless
  map overrides keep known inherited variable names unread, rather than retaining old values.
