# Provider documents and shapes

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

- `document/` (spec #21): provider documents. `mod.rs`: `is_document(path)` (discovery by file
  name: `{openapi,asyncapi}.{yaml,yml,json}` and `*.{openapi,asyncapi}.{yaml,yml,json}`),
  `read(path, bytes) ->
  Summary` (never fails: JSON by extension, else YAML, both through `events::first`; what cannot be read
  becomes a `Problem`, with the line clamped to the file's last line; the content decides,
  since a listed document can have any name: a top-level `openapi` field goes to `openapi.rs`,
  an `asyncapi` one to `asyncapi.rs`, and with neither the name only picks which format it is
  reported as not being, `asyncapi.rs` for an `asyncapi` name, `openapi.rs` for any other),
  `Summary {format: Option<Format>,
  version, title, routes: Vec<Route {path, method, shape}>, servers: Vec<Server {host, path}>,
  channels: Vec<Side {broker, name, role, shape}>, problems: Vec<Problem>}` (stored as JSON; `Route::contract`, `Route::label` = `GET
  /users/{}`; `servers` are the root `servers[].url` entries with variables replaced by their
  defaults in one pass over the URL (a default naming a variable stays as written), a URL
  still holding one dropped, a relative URL giving only its path, host lower
  case, path normalized with no trailing `/`; `Side::contract`, `Side::label` = `kafka send
  orders.created`, `Role::{Producer, Consumer}`; sorted by broker kind, name, role),
  `Problem::Unreadable {line, error}`,
  `Problem::Format {found}`, `Problem::AsyncFormat {found}`, `Problem::Protocol {protocol}`,
  `Problem::Parameters {channel}`, `Problem::NoServers`, `Problem::NoProtocol {server}`,
  `Problem::UnknownServer {channel, server}`, `Problem::ExternalRef {reference}` and
  `Problem::ExternalPathItem {reference}` (structured, rendered through `Msg`), `Format::{Openapi,
  Asyncapi}` (stored as `openapi` and `asyncapi`; `Msg::DocumentRead` takes it with the count),
  `Summary::shown()` (what `search` prints of a document read), `Summary::count(format)`
  (routes for OpenAPI, distinct channels of the sides for AsyncAPI) and
  `report(repo, export)`: the status lines, per document in path order (`Msg::DocumentRead`
  for one read, each problem of it, then each route a later document by path lists again:
  `Msg::DuplicateRoute`), then `Msg::DocumentMissing` for each `[serve] documents` path with
  no row. `openapi.rs`: `read(path,
  value)`: a top-level `openapi` of `3.0` or `3.1` (any patch; a YAML number counts), else
  `Problem::Format` naming the `openapi` or `swagger` field it found; every `paths` key starting
  with `/` and every method of `get put post delete options head patch trace` is a route (a
  path item that is a local `$ref`, such as `#/components/pathItems/...`, is read where it
  points; an external one is a `Problem::ExternalPathItem` and its routes are not read), its
  templates (`{id}`) written as `{}` before `http::normalize_path`, sorted and deduplicated (of
  two templates of one route, the first key gives the shape). A route's `shape` is the
  `application/json` schema of its lowest explicit 2xx response that has one, else of `2XX`
  (a response may be a local `$ref`; `application/*+json` is not read); every external `$ref`
  met, once, sorted, is a `Problem::ExternalRef`. `schema.rs`: `read(doc, schema, source) ->
  (Shape, external refs)`, the schema-to-shape reader: `properties` give fields, `items` the
  `[]` elements (an array with no `items` has open elements), `allOf`, `oneOf` and `anyOf` add
  their members' fields (an `allOf` member with no structure keyword adds nothing; anywhere
  else such a schema, or `true`, is open); `additionalProperties` `true` or a schema opens the
  object, and an object type that lists no fields, no members and no `$ref` is open unless
  `additionalProperties` is `false`. Local `$ref`s (JSON pointers into the document) resolve; a
  reference already being read (a cycle), a missing target and an external reference leave
  that part open. Reading one shape visits at most 10,000 schemas (`BUDGET`) and goes at most
  `MAX_DEPTH` (128) levels deep, a property, element, member or reference being one level; all
  the shapes of one document share one `schema::Work` (`WORK`, 64 MiB: 32 per schema visited
  plus the bytes of every field path listed), which the reader of each document creates and
  passes to every `read`. Past any of them, every part not read is open, so a document's shapes
  have bounded memory and the recursion bounded depth. To read several schemas as one shape
  (AsyncAPI messages), read `{"oneOf": [...]}` of them. `asyncapi.rs`: `read(path, value)`: a top-level `asyncapi` of `2.x`
  or `3.0` (any patch), else `Problem::AsyncFormat` naming the `asyncapi`, `openapi` or `swagger`
  field it found. 2.x: the key of a channel is its name, `subscribe` is a producer and `publish`
  a consumer (the application's side, as the specification reads them), the payloads those of
  `message` (or each member of its `oneOf`), a message a local `$ref`; the channel's `servers`
  are server names. 3.0: the name is the channel's `address` (a channel with none is skipped),
  `action: send` is a producer and `receive` a consumer, the payloads those of the operation's
  `messages` when it lists them, else every message of its channel; the channel's `servers` are
  `$ref`s to `#/servers/<name>`. A channel uses the servers it lists, else every server of the
  document; the lower-cased `protocol` gives the broker kind (`kafka`, `kafka-secure` ->
  `kafka`; `amqp`, `amqps` -> `amqp`; `jms`; `nats`; `redis`; `sqs`), any other is a `Problem::Protocol`
  once and its channels are ignored. A document with channels and no root `servers` is a
  `Problem::NoServers` (every channel ignored), a server a channel uses without `protocol` a
  `Problem::NoProtocol`, and a server name a channel lists that the document lacks a
  `Problem::UnknownServer` (the channel is ignored on it). A name holding `{` or `}` is a `Problem::Parameters` and
  ignored (AMQP bindings are not read). Sides with the same broker kind, name and role (several
  operations, several servers of one kind) are one, their payloads read together as one
  `oneOf` shape. `events.rs` (#646): the event reader every YAML and JSON reader goes through
  (provider documents, Spring's `read_yaml`, and Backstage's `app-config*.yaml` with the partials
  they include, through `spring::flatten`).
  `first(syntax, bytes) -> Result<Value, (line, reason)>` reads the first document and nothing
  past it; `read(syntax, bytes, each)` reads every document of a stream into a `Vec<Value>`
  (`read_keyed`, #653, the same with each second-level key's line and, with `tags`, a node tagged
  `!Name` read as `{"Fn::Name": node}`, for CloudFormation) and hands `each` every scalar as a
  `Scalar {document, path: &[Step], value, line}` (`Step::{Key,
  Index}`, 0-based document and index, 1-based line; a block scalar's line is its text's first,
  a scalar an alias repeats has the alias's line, an aliased mapping's scalars come in key
  order); `Syntax::of(path)` is JSON by the `.json` extension, YAML otherwise. YAML goes through
  `yaml-rust2`'s event parser, taken one event at a time with `Parser::next_token` (never
  `YamlLoader`, nor `Parser::load`, which recurses per level of nesting): keys as written, plain
  scalars typed by the YAML 1.2 core schema, quoted and block scalars strings, a non-scalar key
  an error, anchors per document. JSON is read by `serde_json` first (its line and reason are
  the error, as before), then a lexer over the accepted text decodes each string and literal
  with `serde_json`, so the tree is `serde_json`'s. Both build on one explicit stack, at most
  `MAX_NESTING` (128, as `serde_json`) collections deep, an alias's node counted at the depth
  it lands (deeper, the stream is unreadable at that line), and every node built and every
  document, aliases included, is charged to a budget of 4 times the source length plus 64 KiB
  (over it, the stream is unreadable). `read_located` also lends each completed collection and
  its key/end lines; `json_spanned` uses that seam for JSON alone. A repeated JSON property is
  handed out before it replaces the earlier value. Source-location metadata holds no values.
  Format readers interpret `serde_json::Value`, never lexer tokens.
- `config_values.rs` (#639, ADR 0025): config values. `is_config(path)` (`*.yaml`, `*.yml`,
  `*.json`) picks them from the walk and from `index::hidden_ci`, the second walk of the hidden
  root entries `HIDDEN_CI` (`.github`, `.gitlab-ci.yml`, `.gitlab`, `.circleci`, `.buildkite`,
  `.drone.yml`, read whole, under the same ignore files; no other hidden path is walked, and the
  packs never see these files); `index::configs_found` joins both. `read(path, bytes) ->
  (Outcome, Vec<Row>)` never fails: a lockfile by name (`LOCKFILES`: `package-lock.json`,
  `npm-shrinkwrap.json`, `pnpm-lock.yaml`, `packages.lock.json`) is `Outcome::Lockfile`, a file
  past `lang::MAX_BYTES`, one `events::read` cannot read, one of more than `DOCUMENTS_MAX`
  documents and one whose rows pass the stored budget are `Outcome::Unreadable {line, reason}`
  with no row (an Alertmanager configuration with a regex Go refuses, `alert::refused` #718, is
  `Unreadable` too, keeping its rows); otherwise every scalar but a null is a `Row {key, value, kind, line, cut}`
  (`kind` `string`, `number` or `boolean`, the value cut at `VALUE_MAX` on a character
  boundary), and `Outcome::Read {values, redacted}`. `key_path(steps)` renders ADR
  0025's grammar (dots, `[n]`, `["..."]` by `serde_json` escaping for a key that is not
  `[A-Za-z_][A-Za-z0-9_]*`, `.` for a document that is the scalar); a stream of several
  documents prefixes `[d]` once the read ends (`prefixed`). A value under `data` or `stringData` of a
  document that is a Kubernetes `Secret` (an `apiVersion` and `kind: Secret`), or of such an item
  of a list's `items`, keeps its key path, type and line with no value. `store` and `forget`
  write the rows; `index::refresh` runs `refresh_stamped` on `config_files` beside the documents
  and manifests, before the code, so an edited config file rewrites its rows and nothing else.
  Terraform and OpenTofu (#640): `is_config` also picks `*.tf`, `*.tofu` and `*.tfvars`
  (`*.auto.tfvars` included); `unshadowed` drops a `.tf` file beside a `.tofu` one of its name
  (and `.tf.json` beside `.tofu.json`) from `configs_found` and returns the paths it drops;
  `configs_found` also sorts the `.tfvars` and
  `.tfvars.json` files last. `config_values/hcl.rs` reads `.tf`, `.tofu` and `.tfvars` with
  `hcl-edit` (`parse_body`, spans for the line and the text as written; `hcl-rs` evaluates the
  string a heredoc of literals holds): attributes and blocks walk to rows whose key path is the
  block type, labels and attribute (a block repeated under the same names in one body `[n]`),
  a list or an object of literal keys element by element, anything else one row typed
  `expression` or `template` with its source text (shown on one line by `one_line`); a file nested past `NESTING_MAX` brackets
  (counted by `nested_past` before the parser runs) is unreadable. `.tf.json`, `.tofu.json`
  and `.tfvars.json` go through the event reader (their key paths are the same;
  `iac::json::from_section` drops the index of a root array of bodies before a block path is
  matched), their strings
  with an interpolation typed by `json_kind` and stored as the quoted JSON string.
  SSM parameter `value` scalars in HCL and Terraform/OpenTofu JSON share the type policy:
  SecureString and unresolved types are redacted; literal String and StringList remain public.
  Sensitive:
  a top-level `variable` with `sensitive = true` hides its `default` (`SecretPlace::Variable`
  in JSON, `Hide::Attribute` in HCL); an `aws_ssm_parameter` resource with literal
  `type = "SecureString"` hides its `value` (`SecretPlace::SsmParameter` in JSON,
  `Hide::Attribute` in HCL), regardless of attribute order. `Sensitive` collects, once per refresh from the rows
  already stored (`variable.<name>.sensitive` = true, of the files found), the sensitive
  variable names of the repository and the directories holding an unreadable Terraform file,
  and `read_with` redacts the values a `.tfvars` file gives those names (`Sensitive::of`; all
  of them, `ANY_VARIABLE`, at or below such a directory). `Outcome::Read::sensitive`
  keeps that set for a `.tfvars` file and `Sensitive::stale` makes `refresh_stamped` read the
  file again, though unchanged, when it differs (its `forced` argument). In a directory holding
  an override file (`is_override`, `Sensitive::overridden`), `of` gives a Terraform file
  `ANY_VARIABLE`, so the main pass stores every default of it redacted; after the walk `index.rs`
  reads again each Terraform file whose `Sensitive::merged` set (the names the directory's other
  files mark, `ANY_VARIABLE` for an unreadable one or one a `.tofu` file shadows,
  `Sensitive::shadowed`) differs from its stored `Outcome::Read::sensitive`, so a default another
  file marks never reaches the database, and evaluates its directory again: a shadowed file has
  no stamp. `index::fresh` asks `merged` and `stale` the same.
  `store` keeps each key path and value lowercased beside it (`folded_key`, `folded_value`, each
  with a B-tree); `matches(identity, db, needle)` takes exact matches from the B-trees (rank 0),
  then prefixes from them (rank 1, the range from `needle` up to `successor(needle)`, the least
  text above every text that starts with it), then
  substrings (rank 2) from the trigram table `config_search` (`MATCH` on the needle as a quoted
  phrase, for a needle of three characters or more without a NUL, narrowed again by `instr`;
  else a scan of `instr`; `matches_by` takes the choice so a test compares both), a row keeping
  its first rank and only its sort key and rowid; `line` renders the row an answer shows
  (`search::answer_by` stops making lines at the first tie group past the budget) as `<repository> <path>:<line> <key path> = <value>` (a string as JSON,
  `(cut)` after a cut one, `redacted` for a secret), tie-broken by repository, path, zero-padded
  line and key path; `Engine::search` answers `kind = "value"` with them, and the default search
  with them only when it found no symbol and no contract. `status(db)` is the `meada status`
  line (`Msg::ConfigFiles`), absent with no config file. `under(identity, db, path, block)`
  gives the rows of one file under a key path (`block`, `block.` or `block[`), as `hits` shows
  them: the `values:` of `context` on an IaC node.
- `iac/config_uses.rs` (#661): derives directionless use sides from uncut, non-redacted stored
  strings. AWS structured forms come from `iac/types.rs`; final typed keys map queue/topic/
  bucket/table to SQS/SNS/S3/DynamoDB at inferred. A provisioner's defining name is excluded.
  Internal key identities are `<repository>::<file>#<key path>`. The optional `uses` export
  array keeps the repository-level site and its `at` locator; no scalar text is exported.
  `provision::uses` shares
  these sides with provisioning and orphan scope; directed kind matching never sees them.
- `iac/provision.rs`, `iac/pattern.rs` (#656): `crate::iac::provision::read` derives provisioner
  sides from stored
  IaC names using the embedded `iac/types.toml` table; `crate::iac::provision::links` joins them
  to every matching channel/resource side without entering consumer-link ambiguity or decisions.
  The export keeps root labels alongside node identities; query-time links are never stored.
- `iac.rs` (#649, ADR 0025): IaC nodes. `Repository::evaluate(pick, left) -> (Vec<Node>,
  Vec<Reference>)` is pure over the `.tf`, `.tf.json`, `.tofu`, `.tofu.json` and var files
  (`is_input`) of one repository that `Repository::read` read: `read` dispatches JSON to
  `iac/json.rs`, which maps JSON templates,
  literal variable defaults, static dependencies and block arrays into the same model, retaining
  actual JSON key paths. Its `body_path` also supplies array-aware secret/default recognition
  to config reading; `config_values/sites` shares its key-path parser with sensitive-name
  discovery. `events::json_spanned`/`read_located` supply collection key/end lines and borrowed
  completed bodies before repeated section properties replace them, without allocating source
  paths; native syntax parses with `hcl-edit` (skipping one
  `config_values::nested_past` or the parser refuses) into its directory's `Module` (the `resource`,
  `data` and `module` blocks with their span, literal `name` expression, `count` and `for_each`, the
  `Ref`s their expressions make, found by the `Refs` visitor, a module's literal `source` and each
  argument as a `Named`; the `output` and `locals` values, the `variable` blocks, and the values a
  root's `terraform.tfvars` and `*.auto.tfvars` give, read by `read_var_file`).
  `Repository::read_with` reads override files (`config_values::is_override`) after all the
  others, and `Module::add`, `Module::declare` and `assign` merge each of their blocks into the
  one of its address (`Block::merge`: what it sets replaces the base's, `Block::refs` keyed by
  argument or nested block type, `nested_type`, a JSON `dynamic` property by each of its keys),
  dropping one with no base. A directory holding an override file and a Terraform file that does
  not read or is shadowed (`Module::overridden`, `unreadable`; the refresh hands over files past
  `MAX_BYTES` and `read_with`'s `shadowed` paths for it) has every variable sensitive. A
  directory no other one calls through a local `source` (`local_source`: `./` or `../`,
  normalized, never above the
  root; the files come from the walk, so a symlinked directory holds none) is a root; `Eval::root`
  instantiates it and, per module call, the called directory under `<call address>.`
  (`Eval::instantiate`, a stack against cycles), then resolves each block's `Ref`s in its instance
  (`Eval::resolve`, memoized per instance and `Ref`: a block by kind, type and name, a literal key
  picking its instance; a module output into the child instance; `var.<name>` into the caller's
  argument; `local.<name>` into the locals), each reference keeping `Ref::via`. `refresh` (below)
  rewrites `iac_nodes` and `iac_references` root by root. `hits` renders `search` kind `iac`
  (`<repository> <path>:<line> <address> (<type>, name "<name>", root <root>)`, rank over address,
  type and name); `named` reads an address, `<path>::<address>` or
  `<repository>::<path>::<address>`, each with an optional ` (root <root>)`; `context` and `trace`
  answer for `Engine::context` and `Engine::trace` once no symbol (`context`) or no symbol, route or
  call site (`trace`, its `trace::no_start` answer) has the text, and return `None` when it names no
  node (`trace` walks depth first on its own stack, rows through `meada_core::trace::Chain`, every
  reference `exact`). Static evaluation (#650): `Eval::value` binds the names an expression reads
  (`Eval::name`, memoized per instance: `Eval::var` for a call's argument, a var file's value or a
  default, recording an `Overridable` for a root's variable; a local; a module output through
  `Eval::module`) and `each`/`count`, then evaluates it with `hcl-rs` and `functions::declare`'s
  pure functions when `cost` (an upper bound of its steps: bytes made, and in a `for` template
  text parsed and root objects copied) fits `EVAL_COST_MAX` and the root's `EVAL_WORK_MAX`; else
  it spends the bytes it went through to decide and `Eval::partial` keeps a template's resolved
  parts and puts `*` for the rest.
  `Eval::keys` evaluates `count` and `for_each` while instances are made (no module output read
  yet), and `Eval::root` evaluates the names once every instance exists. `Repository::read` reads
  the files, `Repository::units` gives each root with the directories of its unit, and `refresh`
  (called by `index::refresh_keeping` with the directories a Terraform file or var file changed in)
  evaluates again the roots whose unit holds one, and the roots not stored yet.
- `kube.rs` (#651, ADR 0025): Kubernetes objects and Kustomize trees, into the same nodes
  (`iac::Node`, `iac::Reference`). `evaluate(files) -> Output {nodes, references, images,
  unresolved}` is pure over the YAML and JSON files `index::refresh` hands it (`wanted`: a
  kustomization or a file that says `apiVersion`; any other gives empty bytes, a file that
  exists and holds nothing). `read` parses a file with `events::read`, one `Obj` per document
  with a `metadata.name` (`object`: address `<Kind>[.<group>]/<namespace>/<name>`, no namespace
  for `CLUSTER_SCOPED`, `block` `[n]` in a file of several documents, its span the lines of
  its scalars) and its `Facts` (pod labels, a Service's selector, ConfigMap and Secret mounts,
  the service account, an Ingress's backends, the images), and a kustomization file into a
  `Kustomization`. A kustomization directory no other names (`Eval::entry` resolves each entry
  of `resources`, `bases`, `components` to a file, a directory or `Unresolved`: `remote`,
  `outside`, `missing`, `cycle`, `depth`) is an overlay: `Eval::objects` instantiates it base
  first, hands a component what its lister holds so far, and applies `namePrefix`,
  `nameSuffix`, `namespace` and `images` (`retag`) on the way out; a manifest no kustomization
  lists or patches with is a plain unit of its directory. `Eval::unit` sorts, drops repeated
  addresses and resolves the references: a ConfigMap or Secret by the name the stages gave it,
  a ServiceAccount or Service with `Obj::resolve` (Kustomize's `selectReferral`). `store` rewrites `iac_images` and `iac_unresolved`, `images` feeds `iac::context`,
  `status` is the `Msg::KustomizeUnresolved` line, and `changed` tells `index::refresh` that a
  file read again or left holds, or held, objects.
- `kube/crossplane.rs` (#655, ADR 0025): Crossplane among the Kubernetes nodes. `facts` reads
  each object's `crossplane::Facts` into `Obj::facts.xp` (the kind an XRD or a
  `CustomResourceDefinition` defines, an XRD's claim kind and default and enforced composition, a
  composition's `compositeTypeRef` and `bases`, a claim's or composite's `compositionRef`,
  `spec.forProvider`, the external name); `bases` reads `spec.resources` and the
  `input.resources` of a `pt.fn.crossplane.io` pipeline step into `Base`s, a name a patch or a
  patch set writes (`renamed`) left `None`, with the span `kube::read` gathers per resource
  (`kube::resource`). `Kinds::of` gathers the claim and composite kinds of the repository's XRDs
  (`kube::Eval::kinds`) and `Kinds::role` gives an object's `Role` (`Role::text` is the
  `role:` line); `kube::Eval::unit` adds a node
  `<composition>/<resource>` per base (`Role::Composed`, name `*` and `pattern` when it has no
  literal one) and asks `Targets::of` and `Targets::of_base` for the references. `cluster_scoped`
  keeps Crossplane's own groups out of any namespace (`kube::cluster_scoped`).
- `iac/compose.rs` (#652, ADR 0025): Compose services as IaC nodes. `is_file` names the Compose and
  override files (`iac::is_source` includes them, so a changed one is an input of `refresh`);
  `Repository::read` keeps them apart and `Repository::evaluate` hands a root's directory to
  `compose::evaluate(files, left)`, one unit per directory, joining the Terraform root of the
  same directory (the
  first base name that exists plus the override of its family: `read` each with the event reader,
  `merge` the override in), a node `services.<name>` of type `service` per service with its
  `image` and `build` (`iac::Resolved`: text and overridable flag, from `expand` and `context`,
  which keeps a build path only inside the repository) and its references from `depends_on`,
  `links` and shared `networks`. `iac::context` prints `image:` and `build:` for them.
- `iac/cloudformation.rs` (#653, ADR 0025): CloudFormation and SAM templates as IaC nodes, one
  unit per file (root: its path). `wanted` (a YAML or JSON file that says
  `AWSTemplateFormatVersion` or the SAM transform) and `is_template` (the parsed document) decide;
  `evaluate(path, bytes)` reads it with `document::events::read_keyed` (a `!Name` tag is the map
  `{"Fn::Name": node}`), takes each resource of a `Provider::Service::Resource` type as a node at
  its logical id (`read_keyed` gives its key line, `iac::compose::KeyLines::end_of` its last),
  `referenced` collects `Ref`, `Fn::GetAtt`, the variables of `Fn::Sub` and `DependsOn`, and
  `Eval` resolves the name property `NAMED` gives per type with `Ref`, `Fn::Sub`, `Fn::GetAtt` and
  `Fn::Join` (a parameter's default is an `iac::Overridable`; `no_echo` is also what
  `config_values` redacts a default by). The index rewrites a template's own rows
  (`iac::template_changed`, `iac::refresh_template`) as the closure reading its config file runs,
  and `iac::Owner` keeps each refresh to its own rows: CloudFormation by a type with `::`,
  regardless of filename; among the other types, Terraform and Compose by file name,
  Kubernetes by any other file name.
- `iac/serverless.rs` (#654, ADR 0025): Serverless Framework services as IaC nodes, one unit per
  `serverless.{yml,yaml,json}` (root: its path). `wanted`, `is_file` and `is_service` decide,
  `evaluate(path, bytes)` reads it with `document::events::read_keyed`, expands the file's
  variables (`Vars`: `self:` by path, an `opt:` default as `Seg::Opt`, any other source
  `Seg::Unknown`), records each function as an `iac::Node` with `handler` and `events`
  (`event_text`; a default name's stage overridable as `opt:stage`, `stage_of_default_name`, #709), takes the
  references of its body with `cloudformation::referenced`, and hands `resources.Resources`,
  expanded by `substitute` into a template with synthetic `opt:N`
  parameters, to `cloudformation::nodes` (`Unit`: the path, the document, the spans, the key
  lines, the block prefix, the node and byte room left). `iac::template_wanted` and
  `iac::template_evaluate` pick between a template and a service by file name, so
  `iac::template_changed` and `iac::refresh_template` serve both. `iac::context` prints
  `handler:` and `event:`.
- `shape.rs` (in `meada-core`, reached as `crate::shape`; spec #21, ADR 0009): `Shape {source, fields, open, scalars, declared, optional,
  imports, guessed}`, the one form of what a
  contract carries, whatever it is read from, so shape drift (spec #23) compares shapes without
  caring about their source. `fields` and `open` are sorted sets of field paths: dotted, `[]`
  for array elements (`items[].id`), `[]` first for a payload that is an array (`[].id`); every
  node below the root is a field (`items`, `items[]`, `items[].id`); the empty path is the root
  (`shape::shown` prints it `(root)`). An open path is a part whose fields Meada cannot list:
  anything below it may be present. What is recorded is presence, the scalar leaves and the
  optional fields, never types compared: no types, formats, `required` or `nullable`; `scalars`
  are the paths known to hold a string, number or boolean (nothing below them); on a consumer,
  `declared` are the paths a declared type gives rather than a field read and `optional` those
  it marks optional (`Shape::list` is the one place either grows, keeping them within
  `fields`); `imports` are the parts a package import leaves for the query to fill (stored,
  never exported) and `guessed`, on a provider, the parts one filled through a resolution that
  was not `exact` (issue #175). `Shape::merge` adds another shape's paths (one side reached by
  several Founds). `shape::Budget` is the per-file budget the code readers (`zod.rs`,
  `declared.rs`, `literals.rs`) charge, one per extraction. `schema::read` marks a part scalar when its `type` is only
  `string`, `number`, `integer` or `boolean` (with `null` or not) and no other structure keyword
  is there, and drops the mark where another member gives the part fields or leaves it open. `Source::Document(path)` (exported `{"document": "openapi.yaml"}`) and
  `Source::Literals(path)` (`{"literals": "src/orders.ts"}`, the object literals a route
  handler returns, in the file holding the handler; spec #23) and
  `Source::Reads(path)` (`{"reads": "src/a.ts"}`, the field reads of an HTTP consumer, in the
  file of its call site; spec #23) and `Source::Zod(path)` (`{"zod": "src/orders.ts"}`, the
  zod schema a route handler's response passes through, in the handler's file; declared, like
  a document) are the sources yet; the declared types of a consumer
  (`declared.rs`, spec #23) fill `Shape::declared` and `Shape::optional` of a `Reads` shape. `zod.rs` (spec #23) reads the schema into a `Shape`:
  `Zod::returned(code, responses, handler)` takes the 2xx bodies `Literals::responses` lists
  and reads each as a schema, from `schema.parse(x)`, `schema.safeParse(x).data`, or a value
  or handler return type annotated `z.infer<typeof schema>` (`lang/ts/types.rs` names the
  schema; `Def::returns` and `Value::Schema` carry it), following local and same-repository
  imported constants through `Code::constant`; when any body is no schema, it gives `None`
  and the literals are read (all or nothing). A schema becomes a `Node` tree: `z.object` and
  `.extend/.merge/.pick/.omit` give fields, `z.array` and `.array()` an element `[]`, unions
  (`z.union`, `discriminatedUnion`, `.or`) the union of the members' fields, the primitives
  scalar leaves, `.optional/.nullable/.nullish` and the like keep the part; `.passthrough`,
  `.catchall`, `z.record`, `z.any`, `z.unknown`, `.transform`, `z.lazy` and anything unreadable
  are open. `literals.rs` (spec #23) reads a handler's returned literals into a `Shape`:
  `Literals::responses(code, path, handler, Handler::Express | Next)` takes the calls in the
  handler's byte range (and its inline callbacks), keeps the ones that send a response (Express:
  `<res>.json|send`, `res` being the handler's second parameter, also after `<res>.status(n)`
  in a chain; Next: `NextResponse.json` imported from `next/server` and the global
  `Response.json`, status from the options' `status`), drops any response with a literal
  status outside 2xx whatever its form (also `res.sendStatus(n)`, a status chained before
  `end` or `render`, `new Response(body, { status })`) and Express's `next(err)`, and
  unions the bodies. It follows constants one at a time through `Code::constant` (imports
  included), not through `Code::value`, so `LITERALS_DEPTH` bounds nesting and constant hops
  alike and `LITERALS_BUDGET` the work of a file; a body it cannot read is open at that path
  (an empty array and a spread of anything but a known object included), a string with more
  than one part, a number and a boolean are scalar leaves. `Literals::responses` lists the 2xx
  bodies (with the type each is cast to and the handler's return type, whose index signatures
  open the part they type, as do those of a typed constant it follows; and whether the handler answers another way, which opens the root; see the Known
  ceilings), `Literals::shape` reads them; only module-level constants are followed. `Source::at`
  names it in `context`; `Source::declared` tells a declared source (a document) from one read
  from code. `drift(provider, consumer) -> Option<Drift {missing, unchecked}>` is the
  one drift comparison: each consumer path is checked from the root one step at a time
  (`prefixes`), the first step below an open path of the provider is not checked
  (`unchecked`, mapped to that open path), the first step the provider's fields and open paths
  lack is `missing` (so only the shallowest missing path of a branch is listed), nothing past
  a provider scalar is checked (`name.length`; a scalar root checks nothing), and
  `<path>.length` below a provider array (`<path>[]` present) is no field. Each missing path
  maps to its level: `exact` when the provider's source is declared and the path is in the
  consumer's `declared`, else `inferred`; `None` when nothing is missing or unchecked.
  `Shape::imports` (issue #175, stored, empty once filled) holds the parts a name imported from
  outside the repository gives: `Import {at, from, specifier, name, want}`, `Want` being the view
  the reader needs (`zod` and `literals` on a provider, `infer` and `type` on a consumer, and
  `opens` for a provider value typed with a package type, which reads the kept `type` shape,
  `Want::kept`, and takes its open parts alone, `literals.rs`'s `typed`; the types inside it
  give theirs too). `Shape::guessed` lists the provider parts filled through a resolution that
  was not `exact`: `drift` makes a missing path at or below one `inferred`.
  `literals.rs` records one for a body or spread naming an import (`Reader::imported`), `zod.rs`
  for a schema name (`Reader::named`; `Node::imports`, rendered at the node's path, turned
  `infer` by `Node::declared`), `declared.rs` for a `Ty::Named` and a `z.infer` it cannot find
  (`Declared::imports`), which `response_reads` (`http/ts/clients.rs`) puts below the read path. `fill(shape,
  at, find)` is the one recursion: `find` gives the kept shape, its repository and the level the
  import resolved at (a consumer's paths through one that is not `exact` are not declared), the kept
  shape is filled first, then grafted at the import's path in its view (`Shape::graft`: a
  provider takes fields, open parts and scalars, a consumer the declared paths, optional only
  when every type says so and never when read); unresolved, a provider's part is open and a
  consumer's adds nothing. `under(prefix, path)` joins paths (the root below `a` is `a`).


`kube/environment.rs` (#663) reads container literals, key references and `envFrom`; its lookup
follows ConfigMap/Secret references at each overlay transformation, then resolves final names
within the evaluated namespace and unit. Duplicate targets stay unresolved. Stored targets and
container images reflect Kustomize transforms. An unread later `envFrom` invalidates earlier
imports only in its container and prefix; explicit entries can restore them. Secrets contribute
names only. `iac/compose.rs` merges map/list environment entries by variable,
including explicit unresolved overrides, and expands only checked-in defaults. Both store
`iac::environment::Entry` on the workload node for the image kind's scoped resolution.

## Declaration key paths (#641)

`config_values/sites.rs` parses jq-style paths and `[key=value]` list selectors iteratively.
It matches only indexed keys in the specified file and compares complete, nonredacted scalar
values of type string/number/boolean (never HCL expressions/templates). Quoted keys and
selector values use JSON escaping; nested list selectors retain their
own item's prefix. A selector matching several keys yields each canonical location once.
Missing, ignored and unreadable paths have no indexed row, including absolute/escaping paths.
Null values and empty containers remain absent under ADR 0025, so their declarations are stale.
The existing value and reader ceilings bound inputs; selectors introduce no recursive file
walk or expansion. `Site` holds only the query spelling, identity, path, key and line and lives
for one query, never as another stored copy of config text.
