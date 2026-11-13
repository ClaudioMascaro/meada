# Changelog

All notable changes to Meada are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- The values cache (`values.db`, ADR 0027) stores each entry deflated, and gives back the pages
  that replaced or dropped entries free once they are most of the file: on Meada's own repository
  it holds 4.1 MB instead of 30.2 MB. An existing cache shrinks on the first refresh, which
  replaces its entries. A stored entry that would inflate past 8 MiB,
  or that is not deflated, reads as a miss and costs a parse (#839).

### Fixed

- `trace` stops following a branch 256 levels below its start and marks the line there
  `(depth limit)`, as a process step is marked: a call chain of about 2,000 hops overflowed the
  stack and aborted `meada mcp`, with every call in flight (#732).
- A `trace` that reaches thousands of functions of one file takes time in proportion to them,
  not to their square: each line it writes was placed by counting the newlines of its file from
  the start, and now reads one block of it. `context` and the open items place their lines the
  same way (#745).
- `meada mcp` over stdio answers at most four calls at once, and the others wait their turn: each
  call held an engine and its answer in memory together with every other call in flight, so a
  batch of pipelined calls could exhaust memory (#756).
- A `terraform-aws-modules/s3-bucket/aws` call with `putin_khuylo = false`, or with a value
  Meada cannot resolve, provisions no bucket: upstream creates it only with
  `create_bucket && putin_khuylo`, and the Lambda module's call is read the same way (#787).
- A YAML or JSON file that only the config value reader takes keeps no config value once it
  holds more than 1,000, all its documents together: it is a data file (a measurement dump, a
  generated snapshot), `meada status` counts and names it apart, and a `value` search no longer
  finds its values. Terraform and OpenTofu files, and a file a reader of a kind takes (a
  template, a Kubernetes object, a document, an alert rule, a workflow, a package manifest),
  keep every value. An existing index is rebuilt on its first open by this version, which drops
  the values those files held (#817).
- A call of `meada mcp` that waited longer than the busy timeout (10 s) behind another call's
  refresh no longer fails with `database is locked` when another process writes the index as
  that refresh ends: the time spent behind a refresh of the same process no longer counts toward
  the timeout (#757).
- A call of `meada mcp` no longer answers from a refresh that found documents by a `[serve]`
  table older than the one in `meada.toml`: a refresh reads the table when it starts, not when
  its call opened the index, so a call that read the file before an edit no longer writes the
  index back by the older table (#758).
- `meada status` no longer reports orphans from a use Meada only infers from a key name (a plain
  value under `queue`, `topic`, `bucket`, `table` or a typed alias): such a use side opens no
  orphan scope and is never an orphan line, though it still links to a provisioner of its name
  and `context` still shows it with `[inferred]`. A `no provisioner` line ends with its side's
  level, and the `uses` rows of `meada export` carry `level`, an added optional field (#797).
- A structured identifier in the resource list of an IAM policy statement (`Resource` or
  `NotResource` of a `Statement` entry, `resources` or `not_resources` of an
  `aws_iam_policy_document` statement) is a policy grant, no longer a use side: it gives no
  orphan line, opens no orphan scope and no longer links to the provisioner it names, which hid a
  real `no use` line. `context` lists it with the role `policy grant`, on the resource and at its
  place, and `meada export` lists it in `policy_grants`, an added optional array. A value under
  `examples`, `default`, `enum` or `const` of a JSON Schema document gives no side and stays a
  config value (#798).
- `meada status` asks for image orphans per registry and namespace (`ghcr.io/acme`,
  `docker.io/library`), so a runner of a public image is no longer a `no builder` line because
  the workspace builds some other image. A group that lacks builders or runners prints no orphan
  line, and its sides carry `no builder in the workspace` or `no runner in the workspace` in
  `context` and `search` (#799).

## [1.3.0] - 2026-11-11

A minor release on the [1.x stability promise](https://github.com/ClaudioMascaro/meada/blob/main/docs/site/en/stability.md):
infrastructure joins the graph. Config values and IaC nodes (ADR 0025) are read from Terraform and
OpenTofu (HCL and JSON, with static evaluation of names), Compose, CloudFormation and SAM,
Serverless Framework, Kubernetes with Kustomize, and Crossplane, and answer to the new `search`
kinds `value` and `iac` (`image` and `alert` join for their contracts). Infrastructure contracts (ADR 0026) link the code that uses a queue, a
topic, a bucket, a table, a parameter or a secret to what provisions it on AWS, GCP and Azure,
through SDK use sides in TypeScript, JavaScript, Python, Java, Go and C#, with orphan reports in
`meada status`. Image contracts link builders to runners, workloads carry their environments to the
code they run, alert contracts link rule labels to Alertmanager routes and Datadog
monitor tags to OpsGenie routing, and HTTP gains route
providers declared in IaC and base URLs followed through Kubernetes Services, Ingresses and Compose.
The console graph shows the new roles, a refresh reads unchanged values from a cache beside the
index (`values.db`, ADR 0027), and `meada mcp` answers calls sent without waiting for each reply
(#704). The index schema changes, so existing indexes are rebuilt once on first use; export format
stays 1 and gains optional fields. `meada-core` 1.3.0 gives `ids::VersionEntry` a sixth element and
`ids::NON_BLOCK_KEYS` a fifth key (`added_by_time`, #454), `authorize::compose_entries` a `now`
parameter, `manifest::EntryFile` an `added_by_time` field and the export structs new fields.
Postgres migration 10 needs `meada postgres migrate` before 1.3.0 starts, and 1.2.0 binaries
refuse the migrated schema. The
entries below that name a release level were written while this release was in development. The
tree, the `meada-core` pin, the Dockerfile, the compose defaults and the Helm chart declare `1.3.0`,
and the `v1.3.0` tag goes on the `release/1.3` branch, cut from this entry's merge on main.
Publication (crates, the staged npm package, the GitHub draft release) belongs to the maintainer.
Release notes are available in
[pt-BR](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.3.0.md),
[English](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.3.0.en.md) and
[Spanish](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.3.0.es.md).

### Added

- DynamoDB transactions in Java, Go and C# (#719): `TransactWriteItems` and `TransactGetItems`
  give sides as they do in JavaScript and Python. Each `Put`, `Update` and `Delete` entry
  writes its own table, each `ConditionCheck` and `Get` entry reads its own, with the literal
  and environment rules of the other names. A list or an array of entries held in a name (in
  Go and C#, a request, an item or an entry too) gives its tables only when its function
  mentions the name twice, where it is declared and where the request reads it: any other
  mention may change it, and one that comes in a parameter (in Java, a single entry too) gives
  none. Every Java, Go and .NET AWS operation now has a test. Included in the 1.3.0 minor
  release.
- Curated operators and private IaC mappings (#671): 43 explicit Strimzi, RabbitMQ Queue,
  ACK, Config Connector and Crossplane AWS/GCP/Azure types provision existing contracts.
  RabbitMQ Exchange/Binding topology routes real publishes to queues without inventing an
  exchange channel. Six terraform-aws-modules sources read declared resource and Lambda/ECS
  workload inputs; `meada.toml` maps exact/private source prefixes and custom group/kind key
  paths to curated entries. Unknown identities and creation controls remain conservative.
  This joins the existing 1.3.0 minor development release.
- IaC HTTP route providers (#666): Terraform/OpenTofu API Gateway HTTP/REST and explicit quick
  create, SAM Api/HttpApi and Serverless http/httpApi events resolve to local handler symbols
  or the repository, with normalized parameters, cross-repository HTTP links and stored
  path/target evidence. Included in the meada/meada-core 1.3.0 minor release.
- Azure SDK sides in JavaScript/TypeScript, Python, Java, Go and C# (#670): verified
  constructors connect Service Bus and Event Hubs sends/receives, Blob container reads/writes
  and Key Vault secret reads to physical contracts. New subscription receivers retain their
  topic and subscription through exact factories and scoped workload environments. Existing
  C# Service Bus forms keep their identities; resource readers retain each operation site.
  The bounded inventory joins the shared meada/meada-core 1.3.0 minor release.

- GCP SDK use sides in JavaScript/TypeScript, Python, Java, Go and C# (#668): verified client
  constructors connect Pub/Sub publish and subscription receive, Cloud Storage reads/writes
  and Secret Manager access to their physical contracts. Subscription identities remain
  distinct from topics through scoped workload environment resolution. The documented
  operation inventory ships in the shared meada 1.3.0 minor release. Scoped request mutations
  leave stale names unresolved, typed helpers retain their resource kind through factories,
  and asynchronous factories preserve await state.
- The console graph has image builder-to-runner and alert emitter-to-matcher views, including
  links within a repository, alongside provisioner sides and provisioning links with roots,
  levels and reasons. A bilingual legend explains each role and keyboard navigation. The
  playground exposes `value`, `iac` and `alert` searches. Release level: minor, included in
  meada and meada-core 1.3.0 (#675).
- Datadog Terraform monitor tags join OpsGenie routing conditions through known tag
  forwarding. Selected Alertmanager receivers explicitly transform grouped rule labels
  into OpsGenie tags or PagerDuty v2 custom details; native PagerDuty PCL conditions keep
  their field paths. Equality is exact; regex, prefix and containment are inferred.
  Missing or unread forwarding yields no link, and matching never predicts final team
  delivery. Included in the existing meada/meada-core 1.3.0 minor release (#673).
- AWS workload execution and environments (#664): Terraform/OpenTofu Lambda images and local
  code/archive inputs, SAM CodeUri/Handler/ImageUri with Globals overrides, Serverless handlers
  with provider/function environments, and encoded ECS container images/environments. Known
  resource references retain typed identities; secret and dynamic contents remain unread.
  Context, bidirectional traces and scope-aware image orphans include these workloads; new
  local paths refuse absolute paths, every parent component and symbolic links. Included in
  the minor development release of meada and meada-core 1.3.0.

- Terraform/OpenTofu JSON (`.tf.json`, `.tofu.json`) now creates IaC nodes with source spans,
  references, evaluated names and local module instances. JSON expression and literal rules
  share the HCL evaluator, provisioners and subscriptions; declared resource `via` addresses
  resolve to JSON nodes and refresh when sources change. Sensitive defaults, tfvars and SSM
  SecureString values also stay redacted in JSON block arrays, a root array of bodies included,
  and a JSON `dynamic` property merges as the types it names (#642, #649).
- Azure Terraform/OpenTofu contracts (#669): Service Bus queues, topics and topic-qualified
  subscriptions with filter routing, Event Hubs, Storage accounts and blob containers as
  distinct kinds, and Key Vault secrets. ARM identifiers and supported blob/vault URLs give
  exact config uses; provider-specific typed keys remain inferred. Linux Functions run
  evaluated container images; Linux and Windows Functions run safe local deployment archives, with scoped
  app settings and unresolved Key Vault references. Existing C# topic processors retain
  their topic identity. This shares the 1.3.0 development release.

- HTTP base URLs follow Kubernetes Service DNS names and Compose service names through their
  workloads to the repositories running the providers. Ingress hosts and literal prefixes
  carry public URLs to their Services; rewrites remain unread mounts. Links are inferred and
  name the Service, with workload environment provenance when applicable. A Service or
  Ingress that reaches no code leaves a host to `[serve] hosts`, which still links it `exact`.
  Included in the minor development release of meada and meada-core 1.3.0 (#665).
- GCP infrastructure contracts (#667): Terraform/OpenTofu Pub/Sub topics and distinct
  subscription targets, Cloud Storage buckets, Cloud SQL instances and instance-qualified
  databases, and Secret Manager secrets. Structured GCP configuration identifiers are exact;
  provider-specific typed keys remain inferred. Cloud Run v1/v2 services and v2 jobs retain
  container images and scoped environment targets. Provisioning, subscriptions, context,
  trace, scope-aware orphans and export include these kinds and runners. Release level: minor,
  included in meada and meada-core 1.3.0.
- Declared resource `via` locations now accept IaC addresses in every indexed format and
  `<file>#<key path>` with list selectors (`[key=value]`). Sides retain their precise node or
  key for context and trace; removed locations produce the existing stale status diagnostics.
  Release level: minor, included in meada and meada-core 1.3.0 (#641).
- Resolve environment reads through the workloads running their code, including Kubernetes ConfigMap references and Compose defaults, with inferred provenance and redacted Secret targets (#663).
- Alert contracts join Prometheus, Loki, PrometheusRule and Grafana rule labels to Alertmanager
  route matchers, including AlertmanagerConfig. Equality is exact and regular expressions are
  inferred; a negated condition only filters its route, without linking the value it refuses.
  Route context includes the receiver, trace walks both ways, and export and scope-aware
  orphan reports carry the kind. Helm values are not alert emitters. Included in the minor
  development release of meada and meada-core 1.3.0 (#672).
- Anchored unresolved names across channel, resource and image contracts (#674): only
  literal counterparts with sufficient anchor evidence and whole-segment wildcards link.
  Several names remain ambiguous open items; human or optional decider choices replay through
  `meada.decisions.toml`, including provisioning links and stale-choice reporting. Release
  level: minor, included in meada and meada-core 1.3.0.

- Constructor-backed AWS SDK sides in Java 2.x, Go v2 and .NET: SQS, SNS, EventBridge, Kinesis,
  S3, DynamoDB, SSM Parameter Store and Secrets Manager. Literal names and ARNs join
  infrastructure contracts; environment reads retain their variable without a guessed name.
  Every batch name is kept. Exact factories and aliases are followed, hidden bindings and
  incomplete origins are rejected, and injected types alone create no SDK side, except SQS,
  whose declared client types keep its earlier forms. .NET named requests and its S3 metadata
  spelling are supported. SDK resource links are exact while declarations remain declared;
  hierarchy queries stay unresolved. Release level: minor, meada and meada-core 1.3.0 (#660).
- AWS SDK use sides in TypeScript, JavaScript (SDK v3) and Python (boto3): SNS publishes,
  EventBridge event entries and Kinesis record writes produce channel sides; S3 and DynamoDB
  operations write or read resource sides; SSM and Secrets Manager operations read resources. Batch
  and transaction inputs retain each named resource. Both client and operation origins must be the
  SDK's; name-matched hops and hidden client names produce no side. Complete Python factory evidence
  rejects opaque or missing returns and unawaited coroutine or generator objects, preserves exact
  arguments, class fields and awaited aliases, and shares bounded constructor traversal with
  existing message clients (#659).
- AWS SDK use sides: literal names and supported ARNs join provisioners at `exact`; environment
  reads stay unresolved and name their variable through `via: env:NAME`, without reading the process
  environment. SDK providers gain an optional `via`, absent on existing providers; format remains 1.
  Existing declared resource sides retain `declared`. Older indexes rebuild. Release level: minor,
  meada and meada-core 1.3.0 (#659).
- Config values naming AWS resources become directionless use sides (#661): recognized ARNs,
  SQS queue URLs and S3 URIs are exact; plain names under final `queue`, `topic`, `bucket` and
  `table` keys are inferred. Each key links only to provisioners and counts for scoped orphans;
  bare equal values, redacted/cut values and provisioner name definitions create no use.
  Search, context, trace, export's optional `uses` and the console retain key locations
  without adding producer/consumer or writer/reader links. Release level: minor, meada and
  meada-core 1.3.0.
- Image contracts (#662): normalized `image::<registry>/<repository>` names connect builders
  in GitHub Actions, Compose, Skaffold, ko and Jib to Kubernetes, Compose and Helm runners,
  including Kustomize image overrides. Context shows builders, trace crosses into code and
  back, and status reports image orphans only in scopes with both roles. A Compose build runs
  same-repository code directly at `exact`, without following symlinks: the paths its Dockerfile
  takes from the context (`COPY`, `ADD` and `RUN` bind mounts, in every stage), not the whole of
  it, and `inferred` when it cannot read one; editing the Dockerfile makes the index stale and an
  upward `trace` writes each workload once (#744). Export format 1 adds optional builder `at`
  locations and direct `executions`; the console graph displays both. Release level: minor for
  meada and meada-core, included in synchronized 1.3.0.

- AWS infrastructure contracts: Terraform/OpenTofu SNS, EventBridge and Kinesis channels;
  DynamoDB, RDS instances and clusters, ElastiCache clusters and replication groups, SSM and
  Secrets Manager resources, including ARN name extraction. SNS-to-SQS subscriptions and
  EventBridge rules/targets link queue consumers to source producers, exact without a filter
  and inferred with one. Context, trace, export and scoped orphan reporting cover all kinds;
  SSM SecureString values are redacted, including Terraform/OpenTofu JSON. The schema adds subscription summaries and older
  indexes rebuild. Release level: minor, meada and meada-core 1.3.0 (#658).
- Orphan infrastructure contracts (#657): `meada status` lists provisioners without uses and
  uses without provisioners only for a contract kind and broker or resource kind with both
  sides in the visible scope. Otherwise `search` and `context` mark the side with
  `no provisioner in the workspace` or `no use in the workspace`, without orphan status noise.
  Matching follows provisioning links, including patterns and root labels. Release level:
  minor for both meada and meada-core in the next release.

- Terraform provisioners (ADR 0026): `aws_sqs_queue` and `aws_s3_bucket` join the SQS channel
  and S3 resource contracts by physical name, using an embedded, documented type table. Every
  producer, consumer, writer and reader links to each root's provisioner; overridable names
  link at `inferred` with their variables in the reason. `context` lists the sides and
  provisioners, and `trace` crosses provisioning links both ways. Bounded name patterns link
  literal names at `inferred` (several names: `ambiguous`); the console graph lists provisioner
  sides and provisioning links. Export format 1 gains optional
  `iac_nodes`, `provisioners` and `provisions` arrays. The stored-name sample changes the schema,
  rebuilding older indexes. Release level: minor for both meada and meada-core (#656).
- Config values (ADR 0025): every scalar of every `*.yaml`, `*.yml` and `*.json` file the walk
  visits, and of the YAML and JSON files under the CI configuration at the root (`.github/`,
  `.gitlab-ci.yml`, `.gitlab/`, `.circleci/`, `.buildkite/`, `.drone.yml`), is indexed with its
  file, key path (jq style, the document index first in a file of several documents), value, type
  and line. `search` finds them with the new kind `value` (exact, then prefix, then substring, over
  values and key paths), and the default search answers with them only when no symbol and no
  contract matches (#639).
- Config values: a value over 1 KiB is stored cut, marked `(cut)`; a Kubernetes `Secret`'s `data`
  and `stringData` are stored as `redacted`. Lockfiles and files that do not parse are skipped, and
  `meada status` counts the config files read and skipped and the values redacted. Values stay out
  of `meada export`. The index schema changes, so existing indexes are rebuilt (#639).
- `search` with the kind `value` serves substring matches from an FTS5 trigram table beside the
  B-trees that already serve exact and prefix matches, instead of scanning every row; a query
  under three characters still scans, with the same answer. The table holds about four times the
  text of the key paths and values (21 MB for 5 MB on this repository). The index schema
  changes, so existing indexes are rebuilt (#647).
- Terraform and OpenTofu config values (ADR 0025): `*.tf`, `*.tofu`, `*.tfvars` and `*.auto.tfvars`
  files are read with `hcl-edit` and `hcl-rs`, and `*.tf.json`, `*.tofu.json` and `*.tfvars.json`
  through the JSON reader, so `search` with the kind `value` finds a team name in an OpsGenie
  routing rule or a queue name in Terraform. The key path runs over the block's type and labels and
  the attribute's name (`resource.aws_sqs_queue.orders.name`), a repeated block `[n]`; a literal
  keeps its type, and an expression or a template is stored as written under the type `expression`
  or `template`. A `.tofu` file shadows the `.tf` file of the same name (#640).
- Terraform and OpenTofu config values: the `default` of a `variable` with `sensitive = true` (or
  `"true"`) and every value a `.tfvars` file gives a variable of that name are stored as `redacted`,
  and so is a default another file of its directory marks sensitive when that directory holds an
  override file (`override.tf`, `*_override.tf`), which Terraform merges into the others; such a
  default never reaches the index, also when only the override file changes. There, a `.tf` file a
  `.tofu` one hides counts as one that does not parse, since Terraform reads it. A file that does
  not parse is skipped and counted by `meada status` (#640).
- A Go struct field that keeps a base URL (`c.base`) carries the environment read every
  composite literal of the repository that sets it gives it, directly or through a constructor's
  parameter whose callers in the repository all pass that read, so a method that reads the field,
  directly or through a request wrapper, links through `base_url_env` like the same read written
  inline. Two different reads, a literal by position or of an alias, no literal, a write through
  a selector (`c.base = x`), a constructor with no caller or that a call may reach by a name
  match or through an interface (a method another definition shares the name of), and a literal
  string leave the field
  unknown, as before (#687).
- A TypeScript class field that is `readonly`, `private` or `#x` and written once, by its
  initializer or by one plain `this.f = v` in the constructor body, is a constant of the class:
  `this.f` in a method folds to it, so `` fetch(`${this.baseUrl}/orders`) `` gets the base URL
  the constructor reads, a Backstage `config.getString` through `app-config*.yaml` included, and
  links through `base_url_env`. A public mutable field, a second or conditional write, a write
  outside the constructor, a constructor parameter and a JavaScript file stay unknown (#688).
- Backstage's `app-config*.yaml` follows `$include` and reads `$env`: a block moved to a partial (`{
  $include: ./partials/acme.yaml }`, or `./partials/acme.yaml#api` for one key path of it) and a
  value written `{ $env: API_URL }` are read as if inline, so a `config.getString` through them
  links through `base_url_env`. The path resolves against the including file's directory, only
  `.json`, `.yaml` and `.yml` files are read, and includes nest from their own directory. A partial
  is a config input of the index, so editing it alone changes the answer on the next refresh. The
  index schema golden changes (it now lists the stored roles), so existing indexes are rebuilt
  (#689).
- Backstage `$include` limits: an absolute path, one that leaves the repository, a symbolic link, a
  `${` in the path, another extension, a missing file or key path, a sibling key, a cycle, a depth
  past 8, more than 64 includes from one root, more than 1 MiB grafted into one root and more than 4
  MiB parsed for all the roots of a repository (its files named `app-config.yaml` first, then the
  others in path order; an include counting its file's bytes, or its value's size when YAML aliases
  make it larger, an include past its root's limit included) are not followed (#689).
- Backstage partials that do not parse: such a partial is parsed once per reading, charged its bytes
  or, holding several documents, what reading them built: their keys stay unread and `meada status`
  names each include with its reason. `$file` stays unread, and a config file past 1 MiB is dropped,
  not parsed by half (#689).
- Backstage proxy endpoints are read wherever Backstage's proxy backend reads them: under
  `proxy.endpoints`, and, when no `app-config*.yaml` has that key, as the keys of `proxy` that
  start with `/` (the legacy shape, accepted from before Backstage 1.17). A string value is
  `{ target }` in either shape, with the default `pathRewrite`, and `ignorePath`, `prependPath`
  and `pathRewrite` another file gives the same endpoint apply to it. An endpoint inside an
  included partial counts as if inline. With both shapes, in one file or across files, only
  `endpoints` is read. A `getBaseUrl('proxy')` call plus `/inventory/clusters` through any of
  them is `http::GET::/clusters`, linked through `base_url_env` like an object `target`. A
  minor bump (#690).
- IaC nodes (ADR 0025): every Terraform and OpenTofu `resource`, `data` block and `module` call of a
  `*.tf` or `*.tofu` file is a node with an address (`<type>.<name>`, `data.<type>.<name>`,
  `module.<name>`), a type, a file, its lines and the root module that roots its evaluation (a
  directory no other one calls as a local module). A local module's blocks are instantiated once per
  call under the call's address (`module.orders.aws_sqs_queue.this`), and a literal `count` or
  `for_each` gives one node per `[key]`. Override files (`override.tf`, `*_override.tf`) merge into
  the blocks of their directory, as Terraform applies them (#649).
- IaC nodes: the references between nodes are stored like calls, followed through `var`, `local` and
  module outputs. `search` finds the nodes with the new kind `iac` (by address, type or physical
  name; the default search leaves them out), `context` on an address shows the type, the physical
  name, the references both ways and the config values, and `trace` walks the references down and
  up. Nodes stay out of `meada export`; the index schema changes, so existing indexes are rebuilt
  (#649).
- Compose services as IaC nodes: a Compose file (`compose.yaml`, `compose.yml`,
  `docker-compose.yaml`, `docker-compose.yml`) with its override file in the same directory is one
  evaluation unit, rooted at the directory, and each service is a node `services.<name>` of type
  `service`. `depends_on`, `links` and shared networks are references (`via depends_on`, `via
  links`, `via networks.<name>`). `${VAR:-default}` resolves to its default and is marked
  overridable; `${VAR}` stays an unresolved part. A service records its image and its build
  context (a path inside the repository, from its root) for the image contract, and `context`
  shows both. `search` kind `iac`, `context` and `trace` answer for them. The index schema
  changes, so existing indexes are rebuilt; release level minor (#652).
- CloudFormation and SAM templates as IaC nodes (ADR 0025): a YAML file (the short-form tags `!Ref`,
  `!Sub`, `!GetAtt`, `!Join` and the rest are read) or a JSON file with a `Resources` mapping that
  carries `AWSTemplateFormatVersion` or the SAM transform (`AWS::Serverless-2016-10-31`) is one
  evaluation unit, rooted at the template's path, and each resource is a node addressed by its
  logical id, its type (`AWS::SQS::Queue`, `AWS::Serverless::Function`) the node's type. `Ref`,
  `Fn::GetAtt` (with the attribute in the `via`), the variables of `Fn::Sub` and `DependsOn` are
  references. `search` kind `iac`, `context` and `trace` answer for them; a changed template is
  evaluated again alone. The index schema changes, so existing indexes are rebuilt; release level
  minor (#653).
- CloudFormation and SAM physical names: the physical name of the types the contract kinds will read
  (queues, topics, buckets, tables, functions, streams, secrets and a few more) is evaluated within
  the template: a parameter's default (marked overridable), `Ref`, `Fn::Sub`, `Fn::GetAtt` and
  `Fn::Join`; a pseudo parameter, an import and any other function are unresolved parts and the name
  a pattern. A parameter with `NoEcho` has its default stored as `redacted` among the config values
  and used for no name (#653).
- Serverless Framework services as IaC nodes (ADR 0025): a `serverless.yml`, `serverless.yaml` or
  `serverless.json` with a `service` is one evaluation unit, rooted at its path. Each function is a
  node `functions.<name>` of type `Serverless::Function`, named by its `name` or
  `<service>-<stage>-<function>`, with its handler and events (`http GET /orders`, `sqs Queue.Arn`,
  `schedule rate(5 minutes)`) recorded and shown by `context`, and a reference to each resource its
  body names with `Ref`, `Fn::GetAtt` or `Fn::Sub`. The `resources` section is read as
  CloudFormation: each resource is a node at its logical id (#654).
- Serverless Framework variables: `${self:...}` resolves to the value at its path and the default of
  `${opt:...}` resolves and is marked overridable; `${env:...}`, `${ssm:...}` and the other sources
  are unresolved parts, a `*` in a name and `${...}` in a recorded text. `search` kind `iac`,
  `context` and `trace` answer for them; a changed service is evaluated again alone. The index
  schema changes, so existing indexes are rebuilt; release level minor (#654).
- Kubernetes objects and Kustomize overlays as IaC nodes (ADR 0025): every YAML or JSON document
  with an `apiVersion`, a `kind` and a `metadata.name` is a node addressed
  `<Kind>[.<group>]/<namespace>/<name>` (no namespace for a cluster-scoped kind). Each directory
  with a `kustomization.yaml` that no other names as a base is an overlay and an evaluation unit:
  its `resources`, `bases` and `components` inside the repository are instantiated under it, each
  node labelled by the overlay, with `namePrefix`, `nameSuffix`, `namespace` and `images` applied
  (#651).
- Kustomize bases and Kubernetes references: a base stays inside the repository (a `..` that stays
  inside is followed; a remote base, one above the root, through a symlink, in a cycle or past 16
  levels is not), and `meada status` counts the entries not followed. A Service references the
  workloads its selector matches, a workload the ConfigMaps and Secrets of its `env`, `envFrom` and
  volumes and its service account, an Ingress its backend Services, in one namespace and one unit.
  `search` with the kind `iac`, `context` (with the images of a workload) and `trace` answer for
  them as for Terraform's nodes. The index schema changes, so existing indexes are rebuilt (#651).
- Crossplane compositions, claims and managed resources (ADR 0025): among the Kubernetes nodes,
  `context` names what each object is to Crossplane on a new `role:` line (a composite resource
  definition, a composition, a claim or a composite resource of a kind an XRD of the repository
  defines, a managed resource with `spec.forProvider`, a composed resource), and Crossplane's own
  kinds have no namespace. Each resource a composition creates from a literal `base`
  (`spec.resources`, or a function patch-and-transform pipeline step) is a node
  `<composition>/<resource>` of the base's kind, named by its `crossplane.io/external-name`
  annotation or `metadata.name`; a managed resource is named by its external name (#655).
- Crossplane references, in one unit: a claim to the XRD of its composite kind, a composite resource
  to its XRD, a `compositionRef` to its composition, an XRD to its default and enforced composition,
  a composition to the XRD it composes and to each resource it creates, and a created or managed
  resource to the XRD or `CustomResourceDefinition` of its kind. Patches and functions are not
  evaluated: a name a patch sets, or a base with no name, is the pattern `*`, and a step running any
  other function creates no node. The index schema changes, so existing indexes are rebuilt; release
  level minor (#655).
- Static evaluation of Terraform names (ADR 0025): an IaC node's physical name (its `name` argument)
  and each `count` and `for_each` are evaluated with `hcl-rs`'s evaluator and Meada's pure functions
  (string, collection and conversion functions and `jsonencode`), from variable defaults, the root's
  `terraform.tfvars`, `*.auto.tfvars` and their JSON forms, locals, local module inputs and outputs
  per call, `each` and `count.index`, without running Terraform or reaching the network, so a
  `for_each` over a variable gives one node per instance. `context` lists under `overridable:` the
  root variables a name went through, by their default or a var file. A changed Terraform file or
  var file evaluates again only the root modules whose unit holds it. The index schema changes, so
  existing indexes are rebuilt (#650).
- Terraform names never evaluated: data sources, resource attributes, registry modules,
  `terraform.workspace`, `-var`, `-var-file`, `TF_VAR_` and sensitive variables (marked by any of
  their blocks, override files included, and every variable of a directory holding an override file
  and a Terraform file that does not read or a `.tofu` file hides): such a part of a name is a `*`
  and the name a pattern, shown as `pattern "orders-*"` by `search` and `(pattern)` by `context`,
  and linked by nothing yet. A value over 64 KiB, an expression that could make over 1 MiB and a
  root's evaluation past 64 MiB stay unresolved (#650).
- TypeScript consumers through `new URL(x)`: `new URL(x)` with one argument is `x` (unless the file
  declares a `URL` or imports one from a relative path), and `u.toString()`, `u.href`, `String(u)`
  and `` `${u}` `` pass the URL through, in a constant or built in place, so a `fetch` or a client
  wrapper called with one links to its route and a base URL read from the environment shows its
  `via`. A change through `u.searchParams`, `u.search` or `u.hash` keeps the path; a write to any
  other member of a `new URL` constant, written in parentheses or a type assertion too, leaves it
  unresolved, also through a destructuring or `for (.. of ..)` target, `++`, `Object.assign` or
  another name holding it, or with the target in parentheses or an assertion, as does such a
  write to a sealed field set where it is declared or in its constructor
  (`this.url.pathname = ..`) (#685).
- TypeScript consumers through `new URL(reference, base)`: the two arguments read as the join the
  WHATWG URL Standard defines (RFC 3986 section 5.2 for these references), so an absolute reference
  is itself, `//host/x` takes its host, `/orders` replaces the base's path and `orders` joins the
  base's directory (`https://h/api/` keeps `api`, `https://h/api` does not). Over a base read from
  the environment (or a hole) an absolute path keeps the base as its lead, so `/orders` is
  `http::GET::/orders` with `via env:API_URL`; a relative reference, one that starts with a read,
  and a query or a fragment alone stay unresolved, and a parameter on either side is resolved at the
  call sites (#686).
- `new URL(reference, base)` bases: a base is placed only when its scheme leads it, so a proxy's
  `${X}/fetch/https://api.example.com/` is an opaque base, not a link to that host, and every
  reference loses its dot segments. Swift's `URL(string:relativeTo:)` over a literal base goes
  through the same resolver and follows the same rules; over a base read from the environment it
  stays unresolved (#686).

### Changed

- Development and test builds optimize the main `meada` package at level 1 to reduce the
  resident code footprint of the expanded indexing and contract readers (#711).
- Kotlin: a name read or called in a lambda looks for a local or local function of the code
  around it in one climb, not one from every enclosing lambda: a file of 300 nested `apply`
  blocks indexes in about 12 s instead of over 100 s (debug build), with the same answers (#711).

- A refresh reads the values of the files it did not change from a values cache beside the index
  (`values.db`, ADR 0027) instead of parsing them again: on this repository, a one-line edit of a
  Rust file refreshes in 1.4 s instead of 3.4 s. The refreshed index still equals a cold one, and
  deleting the cache costs only time (#611).
- A refresh builds no resolver for a group it keeps, resolves each call of a changed group once,
  writes communities again only when the call graph moved, and the actix-web reader skips the
  lookups whose answer it does not use: on this repository a one-line edit of a Rust file
  refreshes in 1.0 s instead of 1.5 s, with the same index (#610).
- A refresh summarizes a contract configuration again only when its bytes, the IaC nodes it
  read or a path it looked at changed: 60 ConfigMaps of 290 KB refresh with no change in 0.01 s
  instead of 4.5 s (debug build), and a removed or added Lambda handler or Compose build path
  still moves its answer (#711).
- `search` with the kind `value` reads the value of only the rows its answer shows (each match
  keeps just its sort key), so a broad query such as `e` no longer formats every matching value
  before the budget cuts it (#711).

### Fixed

- A Java AWS SDK request no longer names a table, topic, bucket or queue its builder had before
  the builder changed (#719). A builder changes in place, so a name is `exact` only when every
  mention of the builder is a form the reader follows: a setter, `.build()`. Any other mention
  leaves the side unresolved: the builder handed to a call or captured, `applyMutation`,
  `copy(consumer)`, an alias at any depth, and a use the reader does not model (a cast, a
  ternary). A value built first keeps its name, as an immutable copy does, except in a loop the
  builder outlives. A request is judged whole: one such entry leaves the others unresolved.
  The check reads a file once, so its cost is linear in the calls.
- A Java AWS SDK request built by a lambda or a class that runs after its builder changed, one
  its helper assigns again as a parameter, one a caller passes on after its builder changed, a
  builder field hidden by a local, one set from an initializer or another file, one handed to
  caller code by `applyMutation` and a request type another type or an import shadows name no
  table either (#719).
- A Java AWS SDK request whose builder is set again in a loop around the one its value is built
  in, or inside the request's own expression, names no table either (#719).
- Terraform static evaluation charges the template text `hcl-rs` parses again at each
  evaluation in a `for`, not only the bytes an expression makes: a template in a `for` over
  thousands of elements is unresolved (`*`) before it runs, where one 24 KB root took seconds
  per resource and minutes in all. A read such as `var.x[0]` in a `for` is charged the copy of
  `var` it makes each time. A refused expression spends the bytes it went through (its text and
  the roots it read), at most the 1 MiB each one spent before, and a literal name that fits one
  value stays exact once the work is spent. A short template (`"${a}-x"`) in a `for` over 2,000
  elements still resolves; one of 40 interpolations resolves over 100 elements, no longer over
  200 (#714).
- `meada mcp` over stdio answers calls a client sends without waiting for each reply (#704).
  Each call refreshes the index, and a refresh failed at its first write with `database is
  locked` when another call was writing. Now the calls of one server run one refresh at a time
  and wait for it with no timer, sharing the next run; a call that meets its lock elsewhere
  (opening the index, querying it) waits for the refresh too, past the 10 s busy timeout. A
  batch after a change costs about one refresh more than an unchanged one (debug build, Meada's
  own checkout: 50 calls 21 s against 15 s). Across processes, a refresh with nothing to write
  takes no write lock and one that meets another process's writer waits for its commit and
  starts over; a wait past 10 s there still answers `database is locked`, and that server's
  calls wait behind its refresh as long as it takes.

- Terraform: a `for` binding a key over a set held in a local or a variable (`local.s` from
  `toset(...)`, a chain of locals, a variable declared `type = set(...)`) is unresolved
  instead of answering the index as an exact key, as it already was over a written `toset(...)`
  (#720). A set reached through an attribute, an index or another call stays a ceiling (#717).
- Terraform: a value that enters a variable declared `set(T)` (its default, a var file's value, a
  module argument, an override's default) is converted to elements of type `T`, then sorted and
  deduplicated as cty does, so `tolist(var.s)`, `length(var.s)` and `join` over `["b", "a", "a"]`
  see `a`, `b`, and a `set(string)` of `[10, 9]` keeps the strings `"10"` and `"9"` in string
  order. An element Meada cannot convert with certainty leaves the value unresolved (#720).
- Terraform: an override file that replaces the `type` of a variable whose own block declares a
  set type is now converted as Terraform converts it: the default is converted to the set first,
  then to the new type (`list(string)` gives the set's elements in set order, `any` and `set(string)`
  keep the set; a default an earlier override gave is converted the same way when a later
  one retypes it), and one Terraform rejects (a tuple) gets no exact name. OpenTofu 1.13 and later convert the default
  once, to the final type, and name other instances; Meada follows Terraform (#740), except for a
  variable with a block in a `.tofu` or `.tofu.json` file, which Terraform does not read: it is
  unresolved where the two conversions differ. Known gap: indexed end to end, the four cases tried
  of a `for_each` over a variable retyped to a list got one pattern name where Terraform creates an
  instance per element, and none got a wrong exact name; the cause is open (#783).
- An Alertmanager configuration with a matcher regex Go's `regexp` refuses (an unclosed
  group, a leading `*`, a backward range, a lookahead) is unreadable, as Alertmanager refuses
  to load it: it gives no alert side or link, where its valid routes kept an `exact` link,
  and `meada status` counts the file under `unreadable`. Only a syntax error established as
  a `regexp/syntax` error code counts; an expression past Meada's own budgets or syntax it
  does not read stays unmatched with the other routes linked (#718).
- AWS SDK forms in Java, Go and C# follow the ADR 0026 rule TypeScript and Python already
  followed: an EventBridge entry without `EventBusName` produces to `default`, and an SNS
  publish to a phone number or endpoint names no topic. A request Meada cannot see whole (a
  builder from `toBuilder()` or from a factory of the repository's own, a helper's result) keeps
  its unresolved side. TypeScript and
  Python now read the S3, SSM and Secrets Manager operations Java, Go and C# read
  (`DeleteObject`, `HeadObject`, `ListObjects`, `ListObjectsV2`, `PutParameter`,
  `DeleteParameter`, `DeleteParameters`, `PutSecretValue`, `UpdateSecret`, `CreateSecret`,
  `DeleteSecret`); DynamoDB transactions stay TypeScript and Python only (#660).
- `snapshot publish` no longer publishes the old graph under a new commit when only a contract
  configuration or a path its summary looked at changed (a removed Lambda `worker.zip`, a
  Compose build context): the index is fresh only while all of them are as it read them (#711).
- An image's Helm values file is any YAML file with a `values` word in its name, as for alerts,
  so `prod-values.yaml` declares image consumers, as `values.prod.yml` already did, and
  `valuesets.yaml` does not (#711).

- Python: an import inside a function names its library in that function again, so
  `from requests import get` and `import requests as r` there are HTTP consumers, and
  `import json as requests` there hides the module's `requests` (#668).
- Python framework and Go router tables retain stable identities in optimized builds, so
  Flask blueprints keep their mounted routes and standard Go mux matching stays consistent (#711).

- TypeScript and JavaScript URL consumers respect lexical bindings and writes of `String`
  (#683). A callback named `String` no longer invents a route or host, and a local binding
  leaves the global conversion in other scopes intact. Non-strict block functions follow
  Annex B's enclosing binding; modules, classes and strict prologues retain block scope.

- Image runners and links retain each evaluation root when Kustomize overlays reuse the same
  workload and normalized image. Repeated image evidence within one workload and root still
  collapses to one side (#711).
- A symbol's `context` shows the level, reason and provider of inferred HTTP call links whose
  recorded environment base selects a declared `base_url_env` peer, including through fields
  and Backstage config partials. Editing only a partial refreshes that explanation (#683).
- Resource SDK reader call sites retain their identities while traces follow writer-to-reader
  data flow and its reverse, show the actual function/location, and continue through the
  reader's body. Shared partial reads and process membership use the same direction; HTTP
  and package calls keep their existing behavior (#659, #660).


- Terraform and OpenTofu config values redact an `aws_ssm_parameter` resource's `value` when
  its literal `type` is `SecureString`, in HCL and Terraform JSON, regardless of attribute order.
  Ordinary `String` parameters keep their values (#709).
- TypeScript and JavaScript URL folding requires the runtime global or the named `URL` export
  of `url` or `node:url`. Unknown imports, repository aliases, other exports and shadowed
  CommonJS loaders no longer invent HTTP consumers or links (#709).
- CloudFormation nodes and references survive cold indexing and warm refreshes when a qualifying
  template is named `compose.yaml`: its resource types decide ownership before its filename,
  without changing real Compose ownership (#709).
- A YAML document whose anchors and aliases expand past the nesting limit (128 levels) is
  reported unreadable and skipped, where it could abort meada (#646).
- Crafted IaC and YAML files no longer exhaust memory or time while indexing: long
  CloudFormation parameter defaults joined or substituted many times, names built through more
  than 32 Terraform variables or CloudFormation parameters (now unresolved), YAML anchors nested
  around a large node, Kustomize files listed many times or reached through a diamond of bases,
  thousands of `images` rules, Ingress hosts past 253 bytes and volume names past 63 (now left
  out), an `Fn::Sub` that names its variable twice at every level, Terraform objects of
  thousands of keys (a file whose objects pass 2^24 key comparisons is now unreadable), costly
  `.tfvars` values (now given but unresolved) and long `trim` and `toset` arguments (#646, #649,
  #651, #653).
- An Alertmanager route expression of case-insensitive Unicode classes (`(?i)\p{Any}` written
  hundreds of times) no longer takes minutes to compile on every `export` and `status`, nor do
  hundreds of route configurations compile a megabyte automaton each: the expressions of one
  linking pass share 2^22 code points of case folding (counted only in ranges that hold a cased
  letter, a bracket's classes as folding leaves them), 4096 Unicode classes and 16 MiB of
  automata, in equal parts per routing scope and expression, so what links does not depend on
  the repository asking. Past its part an expression stays unmatched (`status` shows its matcher
  as an orphan, not a limit), and the same expression in route configurations of as many
  expressions is compiled once (#672).
- A long label matched by an Alertmanager route expression of many automaton positions no longer
  runs the expression over it for every route: `.*a.{1000}a.*` against a label of 300,000 bytes
  took 43 s for each route (debug). Before reading a value, each condition is charged a step per
  automaton position (a state that reads nothing, such as an assertion or an empty capture,
  included) and byte offset it may start at, or the bytes it reads for a comparison, against a
  ceiling of 2^26 steps per pass apart from the evaluation ceiling, so that label stops before
  matching and `status` reports the evaluation limit, while route visits and evaluations count as
  before (#711).
- Alerts: a regular expression Go refuses for a repetition operator stacked on another (`a**`,
  `a{2}*`) no longer links (#711).
- Alerts: an AlertmanagerConfig route links `inferred`, since prometheus-operator also matches
  the alert's namespace, which Meada does not read (its consumers say `(AlertmanagerConfig
  namespace)`); a Grafana rule group repeated with `count` or `for_each` declares no rule (it may
  be made zero times); a file with a `values` word in its name (`prod-values.yaml`,
  `values.prod.yaml`) is Helm input, not just one starting with `values`; a quoted matcher value
  reads Alertmanager's escapes (`\n`, `\"`, `\\`), so `"a\.b"` keeps its backslash instead of
  invalidating the route and its siblings; and the 4 MiB file ceiling charges the text a rule or
  route stores, so 2,000 small rules (187 KB) are no longer dropped (#672).
- A Python file of thousands of AWS SDK operations on clients a factory function returns no
  longer takes minutes to index: the operations one reader asks about in a file keep the answers
  no limit cut and share a budget of 2^18 steps, one operation taking up to all that is left
  (each still taking 16 once it is spent, so five factories of 2,000 returns resolve on their
  first operation), and a call passing thousands of clients to one function, or forwarding them
  through a wrapper to the Google Cloud and Azure readers, no longer copies them for every value
  it reads (#660, #668, #669).
- Terraform: an object written after a `-`, a number or a `/`, or after a quote or heredoc end
  inside a template directive's body, now counts toward the 2^24 key comparisons a file may
  hold, where a file of them could take minutes to parse; a file with a heredoc whose first
  line starts with its identifier is now unreadable, since the parser read it otherwise than
  Terraform and could hide such objects behind it (#640); a JSON
  var file of tens of thousands of keys reads in linear time, where it took over half a minute;
  and a var file Terraform loads that does not read (broken, too wide or past 1 MiB) leaves the
  variables it may give unresolved, where their defaults stood in (#650).
- Backstage proxy: a `proxy.endpoints`, a whole `proxy` or a root behind an `$include` Meada
  does not follow counts as the endpoints table, as Backstage reads (or refuses) whatever the
  include gives, so the legacy `proxy['/<ep>']` keys beside it or in another file no longer link
  a call; an include whose `#` key path's last key is absent does not, since Backstage leaves that
  key out (#709).
- Backstage `$include`: an `app-config*.yaml` that is one `$include` is the file it names, a root
  or a partial of more than one YAML document is unreadable, and an include of the file and key
  path being read keeps its `$include` as written rather than being a cycle, all as Backstage's
  config loader reads them (#709).
- Serverless: a function with no `name` is named after `--stage` first, so `context` lists its
  name as overridable through `opt:stage` (at `provider.stage`'s line, or the function's when the
  file has no stage) and the name links `inferred`; `${self:provider.stage}` still reads the file
  and is no override (#709).
- Authorization: an `added_by` a team operator wrote while no GitHub grant provider was
  configured no longer decides eligibility once `[oidc.github]` is enabled. Under the provider the
  console stamps each `added_by` it writes with `added_by_time` (its own clock, never the
  submitted text; part of the version id), and without it writes none; Postgres migration 10
  keeps the time the provider's first permissions set started (`<schema>.permission_state`), and `meada github-app roles` reports an addition on the
  host whose `added_by` has no time, or one no later than that set's second, `unattributed` and
  excludes it whatever its roles. Entries saved before this version carry no time, so under the
  provider their additions are excluded until the global operator writes a reviewed
  `added_by_time` or a team operator adds them again; `composeEntries` takes the time as its
  fourth argument (#454).
- A query no longer reopens each peer's index: the engine refreshes and reads the connection it
  holds, so a peer is never refreshed in one file and read from another (#443).
- GitHub App: a 403 `Resource not accessible by integration` with quota left (no `retry-after`,
  `x-ratelimit-remaining` above 0) is a permission failure, not a throttle: `github-app roles`
  passes it to the next installation unmarked, records the repository as `forbidden` (no rows, a
  warning) when every installation answers so, and the installations stay eligible for the role
  check and keep minting. A 403 or 429 with `retry-after` or an empty quota still
  throttles, as does any 403 outside the role check's collaborators listing (ADR 0013 amended,
  #452).
- prove-meada scripts: the console page checks sign in again when a route lands on the sign-in page
  and fail the route that stays there, the Helm overlays `values-identity.yaml` and
  `values-workspaces.yaml` render in their stack (as `scripts/helm-check` does) instead of failing
  alone, and `console.py start --bin` resolves a relative path against the caller's directory (#679).
- `meada export`, `meada status` and the other commands that print their result once done no
  longer panic when the reader closes its end early (`meada export | head -1`, `| true`): the rest
  of the output is dropped and the command ends as it would, where it panicked with "failed
  printing to stdout: Broken pipe (os error 32)" and exited 101. `meada decide` and `meada mcp`,
  which never panicked, still stop with a "Broken pipe" error and exit 1 (#711).
- Kustomize: with two bases of their own `namePrefix` that write the same names, a Deployment no
  longer references the other base's ServiceAccount, nor an Ingress the other base's Service. A
  Service reference, and a `serviceAccountName` of the kinds Kustomize renames (Pod, Deployment,
  StatefulSet, DaemonSet, ReplicationController, Job, CronJob), resolve as Kustomize's name
  reference does once the tree is built: among the objects of the referrer's namespace (none and
  `default` alike) a stage renamed or moved from the name as written, the only one, else those
  whose prefix and suffix stages end as the referrer's; with none left, the object called by the
  name as written, a final name included, and with several (a tree Kustomize refuses), none.
  Another kind's account, or the deprecated `serviceAccount`, which Kustomize leaves alone, names
  the account called so. An empty `namespace` in a kustomization no longer sets one (#711).
- Ingress: a path Meada cannot read as a literal prefix (`/store/(.*)`, `/api/*`) no longer drops
  out of the routing table, where a shorter rule then claimed its URLs; it is an unread mount at
  its literal head cut at its last slash. A `use-regex` annotation or a missing `pathType` also
  leaves the path unread, and so does a path left out (an ImplementationSpecific catch-all); a
  table past its ceilings (64 rules, read or not) leaves every host of the Ingress unread, not
  only the hosts read before the cut (#711).
- Kustomize `components` change what the kustomization that lists them holds, its bases'
  objects included, as Kustomize applies them; they were instantiated as one more base, so a
  component's `images` rule never retagged the overlay's workloads. Each object a component is
  handed counts against the 50,000 entries one overlay walks, so components reaching one another
  along many paths, or many objects through a long list of them, stop there (#711).
- HTTP: overlays that repeat one Ingress or Service per environment no longer make a route to a
  single provider `ambiguous`; distinct providers, or a route as long that reaches no code,
  still do. A decision recorded for such a route is now stale; `meada decide --prune` removes it (#711).
- Images: a module run from a local path (an archive or a directory) in one root no longer drops
  the image another root runs it from, with its environment and provisions; only that root runs
  code without an image (#711).
- Terraform: `split(",", "")` gives one empty element (a `count` over its length made no node), and
  an IaC name pattern needs three letters or digits besides its wildcards (`*.*.*.*` matched any
  dotted name) (#711).
- Terraform: an object walks its keys in lexical order, whether a variable or local binds it or the
  expression builds it (a literal, a `merge`): a `for` over it or a `jsonencode` of it gave an
  `exact` name in written order, and a `for` whose collection contains an object whose keys are
  known only once evaluated (a computed key, a comprehension) is now `*` (#711).
- Terraform: integer arithmetic is exact, as Terraform's: a result past `u64` or below `i64` (`0 -
  18446744073709551615`), a division by zero or with a remainder (`7 / 2`) and arithmetic on a
  fraction leave that part of the name `*`, also on a `for` variable, where the evaluator printed a
  cast, wrapped or floating-point value as an `exact` name. So do a number literal or `tonumber`
  past them, with more digits than a float holds or that a float writes with an exponent
  (`18446744073709551616` printed `1.8446744073709552e19`), a JSON number written with a fraction or
  an exponent and a negative zero (`-0`, `0 * -1` printed `0`). Such a value in a var file is given
  but unresolved, so the variable's default no longer stands in for it (#711).
- Terraform: `toset` orders its elements as Terraform does (numbers by value, `false` before
  `true`, a null last, strings with numbers or bools as strings), where it kept a number's or a
  bool's first order: `tolist(toset([10, 9]))[0]` was an `exact` `10` for Terraform's `9`. A set of
  collections, in Terraform's hash order, and a set of numbers and bools, which Terraform refuses,
  are `*` (#711).
- Terraform: a null or a collection interpolated into a name (`"q-${null}"`, `"q-${[1]}"`), which
  Terraform refuses, is `*`, where it gave the `exact` names `q-null` and `q-[\n  1\n]` (#711).
- Terraform: arithmetic on a `for` variable in a `.tfvars` file (`[for x in [1, 2] : x * 2][1]`)
  resolves, as Terraform evaluates it, where the value was given but unresolved (#711).
- Terraform: a `%{ for }` directive's collection is read as any other expression, where its keys
  kept their written order and a number its float text (`%{for k, v in {b = 1, a = 2}}` gave
  `b1a2`, Terraform `a2b1`); a negative zero there is `*`. An object of several keys inside a var
  file's template, with or without line comments, no longer drops the value for the default (#711).
- Terraform: `tolist` and `tomap` convert their elements to one type, as Terraform does, where
  they kept each element's type: `jsonencode(tolist(["a", 1]))` gave an `exact` `["a",1]` for
  Terraform's `["a","1"]`; collections unify as cty does (`[[1], ["a"]]` to `[["1"],["a"]]`). A
  number with a bool and a list's null beside a collection, which Terraform refuses, are `*` (#711).
- Terraform: a `for` over `tolist`, `toset` or `tomap` (`{for o in tolist(var.items) : o.name =>
  o}`) costs as a `for` over its argument, so forty objects from a var file expand where the
  estimate, the argument's size squared, left the resource `*` past about a kilobyte (#711).
- Terraform: a `for` or `%{ for }` binding a key over `toset(...)` (`for k, v in toset(x)`) is
  `*`, where the key was the index (`q-0`) and Terraform's is the element (#711).
- Images: a direct path that went through a value a deployer can override (`${CTX:-./app}`)
  runs code `inferred`, not `exact`; a repository's literal image and its own pattern matching
  the same runner are one link, not two `ambiguous` ones; a Helm `image.registry` prefixes
  `image.repository` (a templated one as an unresolved segment), where the runner named a Docker
  Hub image; and an unresolved image name (`image::*`, from `docker/metadata-action`) is no
  `status` orphan (#711).
- Compose: an override file's `build` with no `context` keeps the base service's context, where
  it built from the override's directory, and the `!reset` and `!override` tags clear or replace a
  base value. A `build` context Docker reads as a git repository (`git@github.com:a/b.git`,
  `github.com/a/b`) is no local directory, so it runs no code of the repository. SAM: a
  function with no `CodeUri` of its own takes `Globals.Function.CodeUri`. Kustomize: a
  `bitbucket.org/` base is remote, as `github.com/` and `gitlab.com/` ones are, where one without
  `?ref` counted as missing (#711).
- SDK names (GCP and Azure, and AWS in TypeScript): a request field written through a computed
  key (`{['name']: ..}`) or a spread of another object (a spread of a spread too), or written
  after the call inside a loop statement that also holds the call, leaves the name unresolved;
  each gave an `exact` link to the name first written (#711).
- AWS SDK: a request changed after it is made names no resource: a TypeScript command input
  (SQS's too) whose field is assigned again before the `send`; a Java builder held in a local or
  a `final` field that is set again or handed to another call, itself, through a chain off it or
  through a local holding it (`b.topicArn(..)` before `client.publish(b.build())`), also for SQS and Azure Service Bus and Event Hubs builders; and
  a named entry an inline request holds (an EventBridge entry in C# or Go, an entry builder in
  Java); and a Go request replaced whole through its pointer (`*input = sqs.SendMessageInput{..}`).
  Each gave an `exact` link and provision to the name first written. A TypeScript field
  read off a local object (`{Bucket: cfg.bucket}`) now names its bucket (#711).
- TypeScript and JavaScript: a generator (`function*` and `async function*`, declared or as an
  expression, a `*method()` of a class or an object) returns its iterator, so a `return new
  Storage()` in one no longer makes it a client factory; it gave an SDK resource (Pub/Sub, Cloud
  Storage, Azure Service Bus and Blob) and an `exact` provision to code that never holds the
  client (#711).

## [1.2.0] - 2026-11-04

A minor release on the [1.x stability promise](https://github.com/ClaudioMascaro/meada/blob/main/docs/site/en/stability.md):
package reading (PEP 440 specifiers, uv sources, Go `replace` by a held module, `fetch` URLs a
function builds), resolution in the Rust, TypeScript and Gradle packs, the console's versions
page for a named workspace, the proposal server in the Compose and Helm recipes, SCRAM channel
binding to Postgres, CLI messages in Spanish, a generated first password for the console's
default `admin` account instead of a published one (#579), a refresh that keeps the language
groups no changed file touches (#391), Backstage and HTTP contract reading (tier d1, #613 to #621), and the fixes of tiers b6 to c5. The stored
manifest summary changes, so existing indexes are rebuilt once on first use. `meada-core` 1.2.0 deprecates `msg::pt()` in
favour of `msg::lang()` (`msg::lang() == msg::Lang::Pt`), types `effective::Diagnostic::code` as
`DiagnosticCode` (the JSON is unchanged) and makes `named::message_problem("")` return `Some`;
the new `msg::Lang` and `effective::DiagnosticCode` are `#[non_exhaustive]`, so a match on
either needs a wildcard arm. A crate that declares `meada-core = "1.1"` meets the last two
changes on `cargo update`, and `=1.1.0` stays on the old API. The tree, the `meada-core` pin, the
Dockerfile, the compose defaults and the Helm chart declare `1.2.0`, and the `v1.2.0` tag goes
on the `release/1.2` branch, cut from this entry's merge on main. Publication
(crates, the staged npm package, the GitHub draft release) belongs to the maintainer.
Release notes are available in
[pt-BR](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.2.0.md),
[English](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.2.0.en.md) and
[Spanish](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.2.0.es.md).

### Added

- Backstage backends: the router a backend plugin gives its `httpRouter`
  (`coreServices.httpRouter`) is served under `/api/<pluginId>` when `createBackendPlugin` writes
  its `pluginId` as a literal or as a `const` of the file bound to one, so two plugins with the
  same relative path no longer collide, and a URL
  that starts with `discovery.getBaseUrl('<id>')` (held by a constant or written in place) starts
  at `/api/<id>`, its link's reason naming the plugin's mount (as does a hostless call of a route
  the plugin serves in its own repository). A plugin whose `pluginId` cannot be read stays at the
  root, and `meada status` names it. TypeScript and JavaScript functions now give their `return`
  values and an arrow's expression body to the value flow, so a router a factory returns
  (`httpRouter.use(await createRouter())`) is followed (#615).
- Backstage proxy: a TypeScript or JavaScript `fetch` whose URL starts with `/api/proxy/<endpoint>`
  (also `discovery.getBaseUrl('proxy')` plus `/<endpoint>`) is a call to the `target` of that
  endpoint in `proxy.endpoints` of an `app-config*.yaml`: a `${VAR}` target links through the
  peer's `base_url_env` and a literal `http(s)://` target by host, the link's reason naming the
  endpoint, and any other target leaves the call unlinked, which `meada status` says. A
  `pathRewrite` that only strips the endpoint is followed; any other one, `ignorePath`,
  `prependPath: false` and an endpoint the `app-config*.yaml` files disagree on leave the call
  unlinked too (#616).
- Go: a struct field tagged `env:"X"` (cleanenv, caarlos0/env) or `envconfig:"X"`
  (kelseyhightower/envconfig) is the environment read `X` where the field is read
  (`cfg.API.URL`), so `base_url_env` links such a call as `inferred` instead of `ambiguous`
  (#618).

- A Python pydantic-settings field is an environment read: a field `f` of a `BaseSettings`
  subclass of the same file is the variable `<env_prefix><F>` (upper case), or its `alias` /
  `validation_alias`, so the peer's `base_url_env` links `f"{settings.f}/orders"` as an
  environment read does instead of leaving it `ambiguous`; other classes are unchanged (#619).
- A TypeScript or JavaScript base URL read through Backstage's `config.getString('a.b')` or
  `getOptionalString('a.b')` is the variable `VAR` when an `app-config*.yaml` of the repository
  holds `a.b: ${VAR}`, so the peer's `base_url_env` links the call as an environment read does
  instead of leaving it `ambiguous`; a literal value or a key no file holds leaves the base
  unresolved (#617).
- A PyPI version specifier is checked against the provider's version as PEP 440 says, as npm's
  range is: `range mismatch` when the specifiers (`==`, `!=`, `<`, `<=`, `>`, `>=`, `~=`, `==X.*`
  and `===`, joined by commas) reject the local version, nothing added when they accept it, and
  `range unchecked` kept for a provider without a version and for a specifier it cannot read (a
  Poetry `^1.2`, an unknown operator). A pre-release version is rejected unless a specifier
  names one, and a mismatch removes no candidate (#534).
- A TypeScript or JavaScript `fetch` whose URL a function of the repository builds is a consumer:
  a call to a function whose body is one returned string (an arrow function's expression body, or
  a lone `return`) folds to it with the call's arguments, at the `fetch` and through a constant,
  across imports. A part that does not fold, such as a base read from `process.env`, leaves the
  consumer unresolved (#536).
- A Go `replace` by another module (`replace a => github.com/owner/repo`) keeps, among the providers
  of the required module, the one in the workspace repository that path names, `exact`, for the
  `require` and for the calls through its imports, instead of dropping the dependency; a
  replacement no repository holds stays out (#532).
- A PyPI dependency with a uv source in `[tool.uv.sources]` links to the one package it names,
  `exact`, for the entry and for the calls through its imports: `workspace = true` the member of
  the consumer's uv workspace (`[tool.uv.workspace] members` less `exclude`), `path` the package
  at that path, `git` the workspace repository of its URL; a `url` or `index` source stays out.
  An entry without a uv source keeps linking to every package of its name (#533).
- The CLI messages (`--help`, `status`, `meada.toml` and index errors, and every other message of
  the catalogs) are also in Spanish, selected when `LANG` starts with `es`; other values keep
  falling back to English, and the MCP output stays English (#103).
- The workspaces profiles run the proposal server: `compose.proposals.yaml` and
  `proposals.workspace.toml` in Compose (its own pull and volume, the read role, TLS on
  `127.0.0.1:8445`) and an opt-in `proposals` block in the Helm chart (Deployment, Service,
  bootstrap and probes with `timeoutSeconds`, absent by default); `scripts/compose-check` and
  `scripts/helm-check` assert both. The identity provider recipes say how a person or an agent
  gets a token whose `aud` is `[proposals] audience` (Entra ID, Okta, Keycloak, local accounts; Okta
  needs a second authorization server the chart cannot render), in pt-BR, English and Spanish (#504).
- Over TLS, the Postgres client of the data layer authenticates with SCRAM channel binding
  (`tls-server-end-point`, RFC 5929): it runs `SCRAM-SHA-256-PLUS` whenever the server offers
  it, so a client that requires channel binding connects (#483). Loopback without TLS keeps plain
  `SCRAM-SHA-256`.
- The console's named workspace view gains a versions page (#503): the versions newest first,
  paged with the server's cursor; one version's entries, manifest text and diff against the
  current one as the route answers; the history for operators and the administrator (a viewer
  has no history control); and applying a listed version, which is a rollback when it was
  current before, through the editor's confirmation at the base the page read. A stale base and
  a refused entry show the server's message and the page reads the versions again. In pt-BR and
  English.
- Gradle coordinate holes from `gradle.properties` and version catalogs (#467): a dependency
  written `implementation(libs.models)` or with a `${name}` or `$name` the script leaves open
  now links to the package providing its coordinate. `gradle.properties` of the repository root,
  the settings file's directory and the module (the module's wins, and a name the script assigns
  is never filled) feeds the same hole filler as the script's own declarations, and
  `gradle/libs.versions.toml`, or a `*.versions.toml` a settings `from(files("..."))` names by a
  relative path, maps `libs.<alias>` (dashes and underscores as dots) to `group:artifact` and
  its version through `[libraries]` and `[versions]`; an alias the catalog lacks declares
  nothing. Both files are bounded (`lang::MAX_BYTES`, 8192 entries) and have rows in the
  manifest probes. `buildSrc`, convention plugins, included builds and build-time BOMs stay
  unread. The stored manifest summary gains Gradle's `template` and catalog placeholders, so
  existing indexes rebuild once (ADR 0004); the `conformance-java` export gained two links.
- Rust item macros in associated-item position and by `crate::` (#505, after #398): an
  invocation of a qualifying item macro among the items of an `impl` or a trait gives its
  body's methods, associated consts and types to that block, under the same rules (first arm
  only, the attributes on the invocation, the same `cfg` handling), and `crate::m! { .. }`
  qualifies as `m!` does; `other::m!`, `self::m!` and `super::m!` stay unread. `meada decide
  --decider` now parses a file under the setting the index stored for it, so a function from an
  item macro body is function-like and its span is sent.
- TypeScript and JavaScript: a `tsconfig.json` (or `jsconfig.json`) with `extends` naming a
  relative path inside the repository takes the `paths` and `baseUrl` of the chain, the nearest
  config that declares each option winning (#465), so the alias imports under a monorepo's
  `packages/*` whose configs only extend the root's resolve. A package, a path outside the
  repository, a chain of more than 8 configs after the nearest and a cycle are not followed (the
  nearest config alone serves). Project `references` and a base not named `tsconfig.json` or
  `jsconfig.json` are still not read.

### Fixed

- Go: a method named like a resty verb (`Get`, `Post`, `Put`, `Patch`, `Delete`, `Head`,
  `Options`) called on a struct field that holds a repository, a Kubernetes client or a cache
  client (`s.repo.Get(ctx, id)`) is no longer an HTTP consumer with an unresolved URL, so
  `meada decide` lists no open item for it; a field holding a resty request or client keeps its
  consumers (#621).
- A route whose handler sits in a test file (`*_test.go`, `*.test.*`, `*.spec.*`, `test_*.py`,
  `*_test.py`, a `tests` or `__tests__` directory, `src/test`) is no provider: a test's router
  or fake upstream no longer shows in `search` and the export, and no longer makes the client
  repository a candidate for the route it consumes, so those calls link to the only real server
  instead of turning `ambiguous`. The test code stays indexed; `test_routes = true` in
  `meada.toml` keeps the routes as providers (#620).
- A comment inside a Go parameter list or argument list (`func NewRouter( //nolint:funlen`,
  `NewRouter( // engine`) is no longer read as a parameter or an argument: it took position 0, so
  a router handed to such a function reached none of its routes. The shared child walk of the
  packs now skips comments, which also fixes the same shift in the TypeScript pack (#613).

- Express routes registered on an `express-promise-router` router (the Backstage backend default)
  are provided: its default import counts as a router factory under any local name (#614).
- A refresh resolves a Swift file as a cold index does: the type parameters of a type or
  function (`Page<Item>`) are kept in the index, so an `Item()` in an extension of `Page` in
  another file is no construction of a type named `Item` after an edit to an unrelated Swift file
  (the stored format changes, so an existing index is rebuilt once) (#609).
- A Kotlin `private` constructor, primary (`class C private constructor(..)`) or secondary, is
  called only from inside its class: a call from another class no longer resolves to it, one from
  the class, its companion or a nested class stays `exact`. `internal` is still not enforced
  (#455).
- A Rust name two globs give (`use x::*; use y::*;`, which compiles only under `cfg`
  alternatives) is `ambiguous` between them, not `exact` to the latest glob; an explicit item or
  `use` beside the globs still wins (#439).
- A Swift parameter written as a name from outside the repository (`Ext.CompletionHandler`, which
  may be an alias of a function type) neither takes nor refuses a trailing closure with certainty:
  a call that fits an overload only through it is `ambiguous` among its overloads (`inferred` when
  it is the only one), no longer `exact` to a sibling overload that reads as a function type or
  absent. A parameter written as a type the repository declares or a standard value type still
  refuses it (#456).
- TypeScript and JavaScript: `exports.y = function () {}` and `module.exports.y = ..` define the
  symbol `exports.y` (exported as `y`), so a bare `y()` in the file no longer links to it and
  `m.y()` through an import still does (#457).
- A PyPI package in the consumer's own repository that no uv source names links `inferred`, not
  `exact`, when it is the one package of the name, with the reason "no uv source names the local
  package, the registry package is the likely provider": pip and uv install the registry's
  package, so a same-named stub (uv's `anyio_local`) is only a candidate. A source naming it
  (`workspace = true`, `path`) and a single provider in another repository stay `exact` (#535).
- The `golden_diff` example exits with a message naming the range's refs and the `--range A...B`
  flag when a merge-base cannot be found (a shallow or single-branch clone has no `origin/main`),
  and with a usage error for `--range` without a value, instead of panicking (#592).
- `[postgres] schema` starting with `pg_` (reserved by Postgres) is refused at manifest read with
  Meada's own message, in pt-BR and English, instead of Postgres's raw error at `meada postgres
  migrate` (#482).
- The Helm node readiness probe sets `timeoutSeconds` (`readiness.timeoutSeconds`, default 8), so a
  Postgres or Valkey that drops packets no longer exceeds the kubelet's 1 s default and marks every
  node unready (#448).
- The container build copies every top-level `tests/fixtures/*.json` in the UI stage with one
  `COPY`, so a UI test importing a new fixture no longer breaks the image (#495).
- The deploy fetch scripts (Compose `fetch-pulled.sh` and the chart `workspace.fetch`) read a
  repositories manifest whose `repo` and `path` keys are written as quoted keys (`"repo" = ...`);
  the shared awk program skipped them. Its test now takes the expected pairs of every case from the
  `toml` crate instead of hand-written lists (#568).
- Rust: a `crate::a::*` glob written inside `a` or below it sees what `use super::*` sees,
  the ancestor's private `use` included, so `Def::new()` through it is `exact` instead of
  unresolved (#438). A glob of a sibling module still takes public names only.

### Changed

- An Express app or router a TypeScript or JavaScript function returns now takes the prefixes of
  the mounts it is given to: `app.use('/v1', makeRouter())` serves `makeRouter`'s routes under
  `/v1/...`, where they were served at the root, and the links of consumers of those paths change
  with them (#615).
- A refresh keeps the rows of each group of language packs that resolve together (TypeScript and
  JavaScript, Rust, Python, ...) when no file of the group changed and the config files,
  manifests, file list and exports did not move: its contracts are not extracted again, its calls
  are not resolved again and its files are not parsed again for their values. A one-line edit of
  a Rust file of this repository refreshes in 3.3 s where it took 3.9 s (release build, median of
  five); the export stays byte-identical to a cold one. A change of Meada's version re-extracts
  every group once (#391).
- The console no longer creates the default account `admin` with a published password: without
  `admin_password_env` or `admin_password_file`, the start that creates it generates the first
  password, stores it with a change required, and prints it once on stderr (`accounts: default
  account admin created; first password: ...`, right after the listening line; `docker compose
  logs console` in the Compose recipe). No later start prints it; a lost one is recovered with
  `meada ui reset-admin`. The password policy's `default_password` rule goes with the published
  password, and ADR 0024 is amended (#579).
- The console asks `meada-core` through WebAssembly (`namedNameProblem`, `namedDescriptionProblem`,
  `namedMessageProblem`, `resourceRoles`) whether a workspace name, description and version
  message are valid and which resource roles exist, instead of keeping its own copy of the rule,
  its bounds and the role list (#501). A version message is refused when empty by the same
  function on the browser, the console API and `meada workspaces`: the empty-message refusal now
  reads "a message is 1 to 500 characters, ..." instead of the request-body shape, and the
  refusal of a too-long or control-bearing message reads "1 to 500" where it read "at most 500".

## [1.1.0] - 2026-11-02

A minor release on the [1.x stability promise](https://github.com/ClaudioMascaro/meada/blob/main/docs/site/en/stability.md):
local accounts (ADR 0024) in the console, the proposal server and the nodes, carried by the
`cloud` image, the Compose recipe and the Helm chart; the export's `format` and indexed commit;
client-side deadlines on every Postgres call; durable pulled-generation writes; resolver fixes
for C#, Kotlin, Rust and Swift (tier b3); and the fixes of tiers b1 and b2. The index schema
version changes, so existing indexes are rebuilt once on first use. `meada-core` 1.1.0 adds `format`, `commit` and `dirty` to `Export`, and its
`export::parse` refuses a document without `format`: re-export with 1.1.0 before parsing an export
written by 1.0.0. The tree, the `meada-core` pin, the Dockerfile, the compose defaults and the
Helm chart declare `1.1.0`, and the `v1.1.0` tag follows this entry's merge on main. Publication
(crates, the staged npm package, the GitHub draft release) belongs to the maintainer.
Release notes are available in
[pt-BR](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.1.0.md),
[English](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.1.0.en.md) and
[Spanish](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.1.0.es.md).

### Added

- The export format and the indexed commit (#512, ADR 0004 "Canonical export"): every export
  starts with `format` (`meada_core::export::FORMAT`, 1), which rises only when a reader of the
  previous shape would misread the new one, never for an added optional field; then
  `repository`, then `commit` and `dirty`, both optional and left out together. Locally `meada export` reads `HEAD` before and
  after its refresh and the checkout's status after it (ignored files, `.meada/` among them, do
  not count), and names the commit only when `HEAD` did not move and the index is fresh; outside
  git or before the first commit both are left out. In server mode they come from the commit
  `meada index --workspace` recorded beside the index, or a generation's `head`, never from the
  mirror's live `HEAD`, with `dirty` false. For `meada-core` crate users: `Export` gains
  `format`, `commit` and `dirty` (a minor change, the struct being `#[non_exhaustive]`, so the
  next `meada-core` release is 1.1.0), and `export::parse` refuses a document without `format`,
  naming the field, so an export written before this release no longer parses. Every `*.export.json` golden gained the three fields.

- Named workspaces and the proposal server with local accounts (#562, ADR 0024, from #551):
  `[catalog]` and `[workspaces]` (and so `[proposals]`) accept `[accounts.github]` in place of
  `[oidc.github]` and still need one of the two (`[catalog] needs [oidc.github] or
  [accounts.github]`; `[workspaces]` now names `[ui]` and `[catalog]`, the catalog naming the
  provider). Eligibility reads the account's verified login, an operator account is the platform
  administrator, and workspace operator lists, version authors, history and audit records name
  `local:` subjects. `POST /api/v1/account/tokens` takes `{"surface": "proposals"}` when the
  console's bootstrap has `[proposals]` (audience `meada:proposals`; a 422 `invalid_surface`
  without it), `config` carries `"proposals": true` and the profile page offers the surface. The
  proposal server with `[accounts]` and no `[oidc]` reads the public key at start and admits only
  those tokens (a session token or a node's agent token is a 401 with `WWW-Authenticate: Bearer`,
  no protected resource metadata), deriving the subject, login, groups, grant and role from the
  token alone. No golden changed.
- The verified GitHub login of local accounts (#561, ADR 0024, from #551): `[accounts.github]`
  (`host`, `roles`, `max_age_seconds` as `[oidc.github]`, plus `api`, `https://api.github.com` by
  default on `github.com` and required on any other host) is the GitHub grant provider of local
  accounts. A person pastes a GitHub personal access token on their profile page or sends it to
  `POST /api/v1/account/github`; the console calls `GET /user` on `api` once, keeps the lowercase
  login and drops the token, and answers a fresh session token carrying the login. A login
  another account holds is a 409 `login_taken`, GitHub's refusal a 422, GitHub out of reach a
  503. `POST /api/v1/account/github/remove` and the accounts page's `remove_github` action remove
  it. Session and agent tokens carry the verified login, so the GitHub grant joins the groups
  grant in the console (`session` shows `login`, `github_host` and `github_grant`), on nodes, in
  a team operator's additions (`added_by`), in the audit's `login` column and in the exclusions
  marks; `meada github-app roles` reads `[accounts.github]`. Recorded `account.github` and
  `account.remove_github`; the personal access token is never stored, logged or audited. No
  golden changed.

- Helm chart support for local accounts (#565, ADR 0024, from #551): `modules.accounts` renders
  `[accounts]` from values, with `values-accounts.yaml` as the proof-of-concept shape (a writable
  console with `operations = true`, Postgres and no identity provider). The Ed25519 private key
  comes from its own Secret and is mounted only in the console's `console` container, the public
  key from another Secret in the console and the nodes (a node also gets `[ui] public_url`, the
  token issuer, and `[postgres]`), the initial admin password from an optional Secret in the
  console. `modules.accounts.groups` and `modules.accounts.github` carry `[accounts.groups]` and
  `[accounts.github]` (the refresh job, with `modules.githubApp`, gets the GitHub grant provider and runs the role check, and
  `modules.workspaces` accepts it in place of `modules.oidc.github`); `modules.accounts.breakGlass`
  keeps `[accounts]` in the console alone beside `modules.oidc`. The chart refuses a missing
  Postgres or console, equal key Secrets, accounts beside OIDC without break glass, and groups or
  GitHub under break glass. `scripts/helm-check` covers the sections, the mounts and the refusals;
  the chart READMEs (pt-BR, English, Spanish) no longer say a writable console needs OIDC. The
  proposal server is not part of the chart. `[accounts.github]` renders as the spec names it and
  is parsed once #561 lands. No golden changed.

- One-command proof of concept in the Compose recipe (#564, ADR 0024, from #551):
  `cp .env.accounts.example .env && docker compose up -d` starts the generations fleet, Valkey,
  the store, Postgres with its migration, a writable console with local accounts and nodes that
  admit agent tokens. `compose.accounts.yaml` adds the Ed25519 key pair (made once by the
  `certificates` step, never printed, the private key mounted read-only in the console alone and
  the public one in the nodes), the console's and the nodes' `[accounts]` manifests, a one-shot
  `seed` of the first workspace version (only while the store has none) and a node proxy that
  forwards `Authorization` with no static bearer. The other profiles are unchanged, and
  `scripts/compose-check` covers the new one. The Compose README (pt, en, es) starts with it and
  covers `MEADA_ADMIN_PASSWORD`, `meada ui reset-admin`, key rotation and rate limiting the
  sign-in route at the proxy.

- Agent tokens for the nodes (#560, ADR 0024, from #551): a signed-in local account generates,
  on its profile page or through `POST /api/v1/account/tokens` (`{surface: "nodes", hours}`, 1 to
  168 hours, 8 by default), an agent token shown once (`{token, expires_at, surface}`), signed
  with the account's role and groups as its row stands and a random token id, recorded
  `account.token` (surface, hours, token id, never the token). A change-required account gets the
  403 `password_change_required`, a suspended one the role refusal; the console answers an agent
  token with 401. A node with `[accounts]` and no `[oidc]` (`meada mcp --listen`) now reads the
  signing public key at start, and the console's `[ui] public_url` from a `[ui]` section its
  manifest must carry, refusing to start without either; it admits only agent tokens for the
  nodes (a 401 `Bearer` before routing for any other, a session token included) and answers
  under the grant `[accounts.groups]` gives their groups (every repository without the table),
  as an identity provider's token is answered. The `accounts` feature now implies `ui`. No golden
  changed.

- Accounts page in the console (#559, ADR 0024, from #551): the global operator of a console with
  local accounts gets `/accounts`, listing every account (role, groups, state with a suspended
  account marked and a locked one saying until when, change required, last password change), a
  create form (username pattern and password length checked first, "change on first sign-in"
  checked by default) and, per account, role, groups, suspend, reactivate and reset password,
  each behind the shared confirmation dialog with the server's refusals (`last_operator` and the
  rest) shown unchanged. Any other role, and a console under break glass, gets the not-found
  page at that path. The audit page names every `account.*` action in Portuguese and English.
  The password rule `common_password` now has its own copy. Front end only, no golden changed.

- Groups of local accounts (#558, ADR 0024, from #551): the global operator gives accounts groups
  (`groups` on `POST /api/v1/accounts`, and the `groups` action of
  `POST /api/v1/accounts/<username>`, each an owner name, else a 422 `invalid_group`);
  `GET /api/v1/accounts` lists them, and each change is recorded `account.groups`.
  `[accounts.groups]` maps groups to identities as `[oidc.groups]` does: once it exists, a local
  session's grant is what its groups map to, and it bounds every view (the manifest, the
  workspace and its versions, the store, the repositories, the credentials, the playground, the
  exports and the graph views) as an identity provider's grant does; without it every local
  account still reads every repository. A local viewer whose group owns an entry of the current
  workspace version its grant reads is a team operator (ADR 0015). Session tokens carry the
  groups, so a change reaches reads at the next sign-in and writes at once (the account row is
  read before each). `GET /api/v1/admission` with `[accounts]` now answers: `accounts` (the mode,
  `accounts` or `break_glass`, and the signing public key's fingerprint, `blake3:` and the hex
  BLAKE3 hash of the raw Ed25519 key) and, without `[oidc]`, the groups table bounded by the
  caller's grant. No golden changed.

- Account administration for local accounts (#557, ADR 0024, from #551): the global operator
  lists every account (`GET /api/v1/accounts`: username, role, state, the end of a lock, change
  required, last password change, created and created by, never a hash), creates one
  (`POST /api/v1/accounts`, `{username, role, password, change_required}`, the policy applied to
  the temporary password, a change required unless the body turns it off; a malformed username a
  422 `invalid_username`, a taken one a 409 `username_taken`) and changes one
  (`POST /api/v1/accounts/<username>`, one `action`: `role`, `suspend`, `reactivate` or
  `reset_password`, which clears a lock and requires a change unless told not to; an unknown
  account a 404). Suspending or demoting the last active operator is a 409 `last_operator`,
  decided under the schema's advisory lock so two requests at once never leave none. Anyone else
  gets a 403. The routes answer whatever `[ui] operations` says; workspace and generation writes
  keep requiring it. Every console write of a local account now reads the account's row first,
  so a suspended or demoted account loses its writes at once, while its reads last until its
  token expires. Audit actions `account.create`, `account.role`, `account.suspend`,
  `account.reactivate` and `account.reset_password` name the account and the fields changed,
  never a password, written before the change (503 `audit_unavailable` otherwise); `account` is
  the route-level action of a change refused before its body is read. No route deletes or
  renames an account; under `[accounts] break_glass` (#563) none of these routes exists, each a
  404. No golden changed.

- Break-glass account beside the identity provider (#563, ADR 0024, from #551): `[accounts]
  break_glass = true` lets the section sit beside `[oidc]`; without `[oidc]` it is an error, and
  `[accounts.groups]` or `[accounts.github]` under it is an error naming the table. The console
  then signs in only the default account (any other username gets the wrong password's 401,
  audited `not_break_glass`, the table never read) as the global operator, and admits its session
  token beside the identity provider's, each bearer routed by its issuer to one verifier, so the
  identity provider's keys never check a console token and the signing key never checks an
  identity provider's; a console whose `[oidc] issuer` is its own `public_url` refuses to start.
  The password change works; no other route of local accounts exists. Nodes and the proposal
  server keep admitting only the identity provider and read no key. `config` adds `break_glass:
  true`, and the front end offers a discreet break-glass sign-in (`/break-glass`) beside the
  identity provider's, which now waits for a click under break glass. No golden changed.

- Hardening of local sign-in (#555, ADR 0024, from #551): the password policy refuses an embedded
  common-password list (SecLists, MIT, the entries of 12 to 128 characters, compared ignoring
  case; rule `common_password`). Ten consecutive failed password checks, at sign-in or as a
  password change's current password, lock an account for fifteen minutes, counted in a
  transaction holding the account row so parallel attempts never pass ten; a success resets the
  count. A locked account gets the wrong password's 401 at a hash's cost, its password unchecked
  and its lock never extended, audited `locked_now` and then `locked`. A stored hash weaker than
  the console's parameters is redone at the next successful sign-in. `meada ui reset-admin
  --workspace <manifest>` (features `ui` and `accounts`) creates or reactivates `admin` as an
  operator, clears its lock and sets the `MEADA_ADMIN_PASSWORD` value or a generated password
  printed once on stdout, with a change required, recorded `account.reset_admin` `via` `cli`
  before the write and refusing with exit 1 when the record cannot be written. No golden changed.

- Local sign-in for the console (#554, ADR 0024, from #551): the new `accounts` cargo feature
  (enabled by `cloud`) and its `[accounts]` section, which needs `[postgres]` and never sits
  beside `[oidc]`, name the Ed25519 signing key pair and the default account's initial password,
  each from a variable or a file. Migration 9 adds the `accounts` table, so `meada postgres
  migrate` reaches version 9. On its start the console checks the key pair and creates the
  default account `admin` (password `admin123` with a change required, or the policy-checked
  `MEADA_ADMIN_PASSWORD` without) once, under an advisory lock. `POST /api/v1/login` answers a
  session token the console signs with EdDSA for eight hours; a token carrying the change-required
  mark opens only `POST /api/v1/account/password`. Every route but the static files, `config` and
  `login` then needs a session token; `[ui] operations = true` is accepted with `[accounts]` and
  no `[oidc]`; `config` names the sign-in mode (`sign_in`: `accounts`, or `oidc` with an identity
  provider), `session` adds `account: true`, and
  every account reads every repository while no groups table bounds it. Passwords are argon2id at
  OWASP's parameters under a policy of 12 to 128 characters that refuses the default password and
  the username, at most four hashes run at once, and every sign-in, refusal and password change
  is audited (`account.login`, `account.password`, role `none` for an unknown username) before its
  token. The module table gains `accounts`; no golden changed.

- The console's sign-in pages for local accounts (#556, from #551): when `config` says
  `"sign_in": "accounts"` the console shows its own sign-in page (labelled username and password,
  one generic refusal, `busy` and `audit_unavailable` shown as themselves) instead of the identity
  provider's redirect. A change-required sign-in goes straight to a forced password change page
  (the current password and the new one twice, the server's rule shown on a refusal) that offers
  nothing to navigate to until it succeeds, and then opens the workspace. The session token lives
  in `sessionStorage` for the tab's life, is sent as the bearer and is cleared on sign-out and on
  any 401 (which returns to sign-in; a refused current password in the change is not one); it is never in `localStorage`, a URL
  or a log. A profile page offers the voluntary password change and sign-out. pt-BR and English
  copy; the identity provider flow is unchanged. Front end only: no golden changed.

- `meada status` names a Kotlin file the grammar misreads (#546, from #434). A declaration read
  as an infix call leaves the file out of the index; the file now sets the new `Facts::misparsed`
  (absent from every other file's stored facts) instead of `unbuilt`, and a new status line,
  `Msg::FileMisparsed`, in English and Portuguese, names it beside the byte-budget and using
  drops. The Java and Kotlin layout skips it as it skips the other drops. The schema version is
  the same: a file dropped before the upgrade gets its line when it next changes, or at once
  after deleting `.meada/index.db`. The `cli.status-budget` goldens gained the new line.

### Changed

- The grant providers take neutral inputs (#553, from #551): the new `grant` module computes a
  subject's grant from its groups and its GitHub login (`Providers::grant`), with no reference
  to a token, and holds the GitHub grant provider's permissions cache, which moved out of the
  OIDC module; the OIDC module feeds it from a token's claims. Every reader of `[oidc.github]`
  goes through `Manifest::github`, the console and the proposal server build their sessions
  from a neutral admitted subject (`grant::Admitted`), and a node's pass carries only a grant.
  Nothing a caller sees changes; no golden changed.

- The axum provider reader walks each value once per file (#431): the values a file's rebound
  `let`s reach share one `seen` set, and `Code::origins` is not asked for a call whose builder
  chain goes through a method no router hands back, nor for a name written as text, a path, a
  number, a boolean or a character. The reader's share of a one-file refresh on this repository
  drops from about 1.4 s to about 0.1 s. No golden changed.

### Fixed

- Swift: a call whose labels differ from a known outside member's resolves as if that base
  were absent (#463): `x.hash(seed: 1)` on a type that conforms to `Hashable` and declares
  `hash(seed:)` is `exact`, while `x.hash(into: &h)` stays opaque. A call with no labels, or in
  another language, is still compared by its bare name.
- Swift: an alias of an outside type reaches the repository's extensions of that type (#440). A
  receiver typed by `typealias Anim = Animation`, or by an alias of an alias, and `Anim.build()`
  match `extension Animation { }` as `Animation` does; an alias of an outside type with no
  extension stays unresolved.
- Rust: a call after `?` or `.await` is typed by the value they unwrap (#435, after #401): the
  pack keeps beside a function's written result the first type argument of a `Result` or
  `Option` (`Def::unwrapped`) and the `Output = T` of `impl Future<Output = T>` and
  `Pin<Box<dyn Future<Output = T>>>` (`Def::output`, read only through `.await`), so
  `load()?.port()` and `fut().await.port()` resolve within `T`'s family, `exact` when it
  declares one `port`, where `?` left a name match and a plain `fn` returning a future resolved
  to nothing. A type outside the repository, or a `?` before an `.await`, leaves the call as
  it was. The stored facts gain
  `Def::unwrapped`, `Def::output`, `Call::unwraps` and `Call::awaits`, so the schema version
  changes and Rust indexes are rebuilt once.
- Kotlin: a class, object or interface nested in an `expect` declaration, at any depth, is
  `expect` too (#437), so a use of it resolves to the `expect` one instead of staying
  `ambiguous` among its `actual` declarations.
- C# `using static T` imports only the static members and nested types `T` declares (#433): a bare
  `Go()` with `Go` on a base of `T` was an `exact` link to the base's method, which C# rejects
  ("inherited members aren't imported"), and is now a name match at best; `using static` of the
  base itself still gives it. Java's `import static` and Kotlin keep reading inherited members.
- Kotlin: a bare call resolves in Kotlin's order, an extension of the implicit receiver in scope
  before a top-level function of its name (#436, from #402): inside `fun CoroutineScope.x() {
  launch { } }` the call is `exact` to `CoroutineScope.launch`, not to the top-level `launch`
  (`ambiguous` with it when the extension may apply to any receiver), and an `object`'s name is
  its instance, so `Dispatchers.setMain(..)` reaches an imported `fun Dispatchers.setMain` (it
  was no link) while a class's name keeps companion and statics. "Known ceilings" drops the
  bare-call line. No golden changed.
- C#: an `internal`, `private` or `private protected` constructor or method of a public type is
  a candidate from its own project's files alone (#441); `InternalsVisibleTo` stays unread.
- The console writes one `ui: directory: unreadable` stderr line per request when the directory
  cannot be read (#451, after #335): the request's session remembers the failure at admission,
  so the flags, the admission counts and the owner picker take that answer and write no second
  line (`workspace` wrote two), and a write it refuses as 503 `audit_unavailable` prints no
  pre-authentication audit line beside it. Reads still answer with the flags unavailable, the
  counts null and the picker a 503; writes still refuse as before. No golden changed.
- Postgres calls that had no client-side deadline now have one (#447): a SCIM request (under the
  directory's one write lock), the role check's fence read and write transaction, and the
  console's start check, so a Postgres that accepts the connection and stops answering ends in a
  503, an exit 1 with no exclusions file a reader admits, and a console that starts, instead of
  holding them forever. `postgres::Config::bound(n)` now adds the version check's two statements
  itself and every call site counts only its own (the audit and directory reads and the overage
  lookup moved accordingly; the accounts' calls, which undercounted, and the permissions poll
  gained the two). No golden changed.
- A crash after the pointer swap can no longer leave `current` naming a version document that
  never reached the disk (#518): the pointer, the version document `meada workspace pull` writes,
  the pulled contexts file and the exclusions file are replaced through one helper
  (`pull::replace`) that syncs the new file before the rename and removes `<name>.new` when a
  step fails. A failed pointer rename now names `current`, not `current.new`.
- The docs say what 1.0.0 ships: nine languages (TypeScript, JavaScript, Python, Go, Rust, Java,
  Kotlin, C# and Swift). The three READMEs, the three 1.0.0 release notes and the three user
  guides no longer call Java, Kotlin, C# and Swift later 1.x work, the user guides gain a section
  for each of the four (9.5 to 9.8) with their brokers (JMS, Azure Service Bus) and package
  ecosystems (Maven and Gradle, NuGet, SwiftPM), and ADR 0003 records the change in an
  amendment. No code, default dependency or golden changed.
- A server-mode mirror's `meada.toml` and `meada.decisions.toml` are read through no symbolic
  link (#547, after #506), as a pulled generation's are: `mirror()`, behind `Manifest::read`
  (`mcp --listen`, snapshot pull, the lock and pointer operations, workspace versions, the
  heartbeat, the UI, the workspaces CLI, the GitHub App), now refuses a link unopened, one that
  stays inside the mirror included, a file that is not regular and a file replaced while read,
  with the generation's messages, and bounds each file at 1 MiB (`GENERATION_CONFIG_MAX`, now
  built without the `snapshot` feature). A local checkout and an operator's own files still
  follow links. The architecture map and the snapshots page, in its three languages, say so.
- No manifest error quotes a written value (#545), after #449 stopped serde's `invalid type` and
  `invalid value`: `unknown variant` names the key and the values it accepts (`` `workspace.source`:
  is not one of the values it accepts (`file`, `store`) ``) instead of serde's text, an error no
  key spans reads "a value is not valid here" instead of serde's message, and the rule errors
  for `serve.prefix`, `serve.strip`, `serve.documents` and a resource's `kind` name the key and
  the rule (`` `serve.strip`: must start with `/` ``) instead of the value. The console's cut of
  an error to its position stays as a second guard (it still covers an error built outside
  `describe` that names an identity). English and pt-BR texts changed; the workspace validation
  fixture changed only by the new texts.
- The deploy fetch reads a `[[repository]]` entry by its own keys (#544): any table header ends
  the entry before it, so a `[[repository.peer]]` or `[[repository.resource]]` table no longer
  replaces its `repo` or `path`, and a value is the text between its quotes, so a trailing
  comment is not part of it. Before, such a version stopped the Compose refresh and let the chart
  clone the peer's repository into the entry's path (or into a directory named after the
  comment). `fetch-pulled.sh` and the chart's `workspace.fetch` carry one awk program, and a
  test fails when the two copies drift. No golden changed.
- A repository identity on the `[oidc.github]` host compares without case wherever it meets the
  catalog (#508): the roles job gives a selected `github.com/Acme/API` and the catalog's
  `github.com/acme/api` one `both` row keyed as written, the `max_repositories` cap does not
  count it as extra, and the console's catalog check and listing match it. A repository selected
  in two spellings (a deployment's `github.com/Acme/API`, a named workspace's
  `github.com/acme/api`) keeps one `source` row under the first and gets its role rows under
  each, so grants, which match exactly, read both. Other hosts compare exactly. No golden
  changed.
- Compose mirror origins (#445): `fetch.sh` and `fetch-pulled.sh` read an existing mirror's
  `origin` before fetching and refuse one whose HTTP(S) URL carries credentials (a mirror cloned
  from a URL with a token keeps it there), naming the identity and never the URL, and the refresh
  stops before index and publish as a failed fetch does. The READMEs tell the operator to run
  `git remote set-url origin <URL without credentials>` in such a mirror.
- Java and Kotlin: a file with thousands of fields and locals is read in linear time (#442, from
  #424). The hides check indexes the fields by name instead of scanning every field per local,
  the class around a write is found from the prefixes of its scope, and `declared_before` (and
  Kotlin's `assigned`) read a node's parent from an index built once per file, since
  `Node::parent()` is found from the root and made a deep file quadratic per write. Same answers;
  no golden changed.
- Indexing: a file a pack drops for passing a limit is named by `meada status` (#434, from
  #419). The Java, Kotlin and Swift qualified-name budget (4 MiB) now sets `Facts::budget`, as the
  C# one does, instead of `unbuilt`, which prints nothing; a C# project that gives its files more
  than 256 usings (`PROJECT_USINGS`) marks each of them with the new `Facts::usings`, which a new
  status line names (`Msg::FileTooManyUsings`, with the count). Kotlin's `misparsed` file stayed
  `unbuilt` (a grammar defect, not a limit; #546 names it too). An index built before this change keeps a Java,
  Kotlin or Swift file dropped by its budget as `unbuilt` until the file changes. No limit
  changed. The `cli.status-budget` goldens gained the new cases; no stored format golden changed
  (the new field is absent from every sample), so the schema version is the same.
- Resolution: the receiver and call depth (`RECEIVER_DEPTH`) is one count for the whole lookup
  (#432, from #417), carried through a glob, an alias or an on-demand import as the link count is,
  so a chain that alternates receivers and imports costs its length, not `RECEIVER_DEPTH` per link
  up to `LINK_DEPTH`. No limit changed and no answer under the limits; no golden changed.
- Helm: the default `workspace.fetch` (#444, from #316) reads the repositories from `$1` when
  given (the pulled list under `modules.oidc.github`) else from `workspace.repositories`, now
  mounted as `repositories.toml` so a store-sourced refresh, whose bootstrap lists none, still
  fetches the operator's list, refuses a URL carrying a credential and, as the compose
  scripts do (#445), an existing mirror whose `origin` carries one, keeps each mirror strictly
  under `/data/mirrors`, and on failure prints `refresh: fetch failed for <identity>; the refresh
  stops before publication` and exits nonzero without echoing a URL or token.
  `scripts/helm-check` runs the rendered script under `sh` with a stub `git` to check these; the
  chart READMEs describe it.
- Workspace import (#450): `meada workspace import` reads its file once, bounded by the version
  size limit, and audits the id of the text it stored; a file past the bound or not a regular
  file is refused before any audit record is written.
- Manifest type errors (#449): a `meada.toml` or workspace manifest key of the wrong type, or
  with an invalid value, reports the key and the expected kind (`` `oidc.entra`: has the wrong
  type (a table was expected) ``) instead of serde's message, which echoed the written value, so a
  secret pasted under the wrong key no longer reaches stderr.
- Pulled generations (#506): a mirror's `meada.toml` or `meada.decisions.toml` that is a symbolic
  link is refused unopened, the error naming the file, so a repository can no longer make a node
  read and parse a file of the host. A link into the mirror (`meada.toml -> conf/meada.toml`) is
  refused too, as the index refuses a linked pack config. The file opened is checked to be the one
  checked. Secrets and CAs a manifest names still follow links (Kubernetes mounts them through
  one). The snapshots page says so in its three languages.
- Named workspaces on a node (#510): a node shows a named workspace's route
  (`/-/workspaces/<id>/<identity>`, pinned or not) and lists it in a 404 only to a token whose
  grant reads every entry of the workspace's version, the entries the sponsor rule excluded
  included, as the console does. Before, once A of a Y+A workspace lost its sponsor, a subject
  reading Y alone found the workspace on a node. A generation's context now records `excluded`,
  the identities the sponsor rule left out, sorted, and its digest covers them; a context without
  one is written as before, with the same generation id, and a generation published earlier reads
  as none excluded. A node on an older binary refuses a generation that records an exclusion, so
  upgrade nodes before or with the publisher. No golden changed.

## [1.0.0] - 2026-10-30

The tree declares `1.0.0`: this release raises the workspace, the `meada-core` pin, the Dockerfile,
the compose defaults and the Helm chart from `0.1.0`, and the `v1.0.0` tag follows its merge on
main. Publication is still ahead and belongs to the maintainer under
[#154](https://github.com/ClaudioMascaro/meada/issues/154): `cargo publish` of the crates (core
first), approval of the staged npm package with 2FA, and publication of the GitHub draft release,
which triggers the OCI image and Helm chart workflow.
Release notes and the scope evidence checklist are available in
[pt-BR](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.0.0.md),
[English](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.0.0.en.md) and
[Spanish](https://github.com/ClaudioMascaro/meada/blob/main/docs/releases/1.0.0.es.md).
The [1.x stability promise](https://github.com/ClaudioMascaro/meada/blob/main/docs/site/en/stability.md)
starts at publication. The [#32 tracking checklist](https://github.com/ClaudioMascaro/meada/issues/32)
still needs the post-publication evidence.

### Added

- Public fixture benchmark (issue #149): release-profile index/reindex timings, root and
  workspace index sizes, and default-answer/tool-list token estimates in every PR/main CI
  summary and retained JSON/Markdown artifact. Timings are informational.

- The English user guide and README (#150, spec #32): `docs/user-guide.en.md` translates both
  halves of the pt-BR guide section by section (sections 1 to 11) with the same neutral examples
  and links the English stability page; `README.en.md` now links it instead of the pt-BR guide.
  No code, default dependency or golden changed.
- The Spanish user guide (#151, spec #32): `docs/user-guide.es.md`, sections 1 to 11 of the pt-BR
  guide with the same structure and neutral names, linking the Spanish stability page. The Spanish
  README links it instead of the Portuguese guide, and the three READMEs link the stability page
  from the Status section. No code, default dependency or golden changed.
- The output grammar reference (#145, spec R1): `docs/reference/output-grammar.md`, in English like
  the MCP surface, specifies every line kind of `search`, `context` and `trace`, the link notation
  with its level and reason, the levels in order, the contract identifiers (`http::`, `http::ANY::`
  for a provider of every method, `channel::`, `package::`, `resource::`), the `search` kinds, the
  chain footer, the cut note and the token estimate, and tells readers to skip line kinds and
  fields they do not know. Its tables are the grammar check's input: `tests/it/grammar.rs` matches
  every line of every rendered-answer golden against the line kinds of its tool and fails naming
  the golden and the line, fails on a documented line kind no golden shows, and holds the
  document's levels, entry kinds, open item kinds and `search` kinds to the code's. The stability
  document links it. No output changed; new goldens pin the answers only inline assertions covered (`no symbol
  matches`, `no resource matches`, the server's `index behind the mirror`, and `trace` with no
  start, several symbols, several call sites or a clean working tree).
- The pt-BR user guide, second half (#147, spec #32), in `docs/user-guide.md` (sections 8 to 11):
  each contract kind (HTTP routes, declared resources and gateway prefixes, OpenAPI and AsyncAPI
  documents, channels with the limit that broker clusters are not modelled, packages), each 1.0
  language pack (TypeScript and JavaScript, Python, Go, Rust) with its frameworks, clients,
  environment reads, manifests, channel clients and known limits (including that Rust calls inside
  macros are not indexed), shape drift, and communities and processes. Java, Kotlin, C# and Swift
  are named only as planned Release 2 work. No code, default dependency or golden changed.
- The pt-BR user guide, first half (#146, spec #32), in `docs/user-guide.md`: installation through
  every channel (Cargo, prebuilt binary, shell installer, `npx @meada/meada`, container), MCP setup with
  `npx @meada/meada mcp` and the three tools with their parameters, the `meada.toml` reference (every
  table and key), decisions and deciders (protocols, environment variables, what is sent,
  privacy), every CLI command and the export. The READMEs in all three languages gain the
  installation channels, first steps, MCP setup and a 1.0 status section with the stability
  promise. No code, default dependency or golden changed.
- Surface goldens (#144, spec #32): `tests/golden/mcp.tools.json` holds the MCP tool list (names,
  descriptions and parameter schemas of `search`, `context` and `trace`), checked beside the
  1,000-token limit of the descriptions, and `tests/golden/cli.help-all.en.txt` the English help of
  every command of the default build. A test pins the default token budget of 2,000. Both
  goldens change only with `MEADA_BLESS=1`.
- The stability document (#148, spec #32), `docs/site/{pt,en,es}/stability.md`: the surface 1.x
  freezes (the MCP tools, the output grammar, CLI commands and flags, the `meada.toml`,
  `meada.decisions.toml` and `meada.workspace.toml` schemas, the export), what is breaking, what is
  minor, what is not frozen, and the review gate. The READMEs and the site home pages link it.
- `search`, `context` and `trace` read from each index its contract rows and only the symbols,
  calls and communities of their answer (#396, **Partial export**), instead of every row of every
  repository. Answers are the same bytes; on junit5 a `search` or `context` peaks at about 40 MB
  instead of 170 to 190 MB and answers in about 0.3 s instead of 1 to 1.4 s. The index gains
  seven lookup indexes (about 40% larger on junit5) and its schema version changes, so existing
  indexes are rebuilt. `meada export`, `status` and the decision report still read every row.
- A Meada skill for agents, `skills/meada/SKILL.md`: when to reach for `search`, `context` and
  `trace`, how to read each level, and that a missing caller is unproven. The READMEs and the
  site's MCP page show how to install it.
- Per-channel install and MCP setup: the READMEs open with one row per channel (npx, Cargo, shell
  installer, archive, Docker) with its install line and MCP server command, and gain a Docker MCP
  line that mounts the folder holding the repository at the same path and runs without a network.
  `scripts/container-smoke` runs that line as a stdio client.
- The site's Deploy page (`docs/site/{pt,en,es}/deploy.md`) covers Docker, Compose and Helm, and
  the chart gains `values-minimal.yaml`, which `scripts/helm-check` renders.

### Changed

- For `meada-core` crate users: the export's structs (`Export` and each of its pieces, with
  `Shape` and `Drift`) are `#[non_exhaustive]`, so a field added in 1.x is a minor change. Build
  one with `T::default()` and named field assignments, or, for a struct with a field that has no
  default (`Document`, `Import`, `Link`, `Process`, `Step`, `Stale`), with `T::new` taking every
  field in declaration order (`Shape::new` takes the source, the rest set after it). A struct
  literal or an exhaustive pattern of these types outside `meada-core` no longer compiles. No
  output, golden or exported byte changed.
- The stability promise (spec #32) lets 1.x grow the MCP surface. In
  `docs/site/{pt,en,es}/stability.md` and the three READMEs, these are minor changes: adding an MCP
  tool while the tool list stays within its 1,000-token budget, adding an accepted value to an
  existing parameter (a new value of `changes`) and adding a field at the end of an existing line
  kind. Removing or renaming a tool or parameter and changing what an existing one means stay
  breaking. ADR 0008 is amended: three tools is where the surface started, not a ceiling.
- `SECURITY` (pt-BR, English and Spanish): the latest published 1.x version receives security
  fixes, which ship as a patch or minor release.

### Fixed

- Console GitHub grant (#496): a session's GitHub grant and its state (`unread`, `current`,
  `expired`) are read from the same permissions set at the same instant, so a session never shows
  `expired` with a grant read while the set was fresh.
- Release preparation keeps the GitHub release in draft state for the maintainer to publish.
  A pinned cargo-dist regeneration helper compares the entire workflow after the explicit
  draft policy; OCI publication follows the human `release: published` event.

### Prepared

- The v1 scope: HTTP, declared resources and gateway paths, OpenAPI/AsyncAPI, channels and
  packages; levels and cross-repository chains, recorded decisions with optional deciders,
  shape drift, TypeScript/JavaScript, Python, Go, Rust, communities and processes.
- The R1 stability promise, normative MCP output grammar, surface goldens, complete user
  references and public-fixture CI benchmark. Fixture timings are informational.
- Configured Cargo, binary, shell, npm and OCI distribution. Configuration is not evidence
  of actual publication or a successful published-installer smoke check.

### Acceptance

The [public #152 receipt](https://github.com/ClaudioMascaro/meada/issues/152#issuecomment-6379316350)
retains both complete runs on AMD Ryzen 7 5825U, 16 logical CPUs, 7.16 GiB RAM,
Ubuntu 24.04.5 LTS, x86_64. The measured build was
`9b99c3a4091b6c1b2c590c43059763f25b6987ef`, `meada 0.1.0`, not every later documentation head.

| Metric | Target | First complete run | Complete rerun |
| --- | --- | --- | --- |
| Cold configured-workspace index | <= 7 s | 12.680 s, not met | 2.711 s, met |
| Root index size | <= 37 MB | 21.057536 MB, met | 21.057536 MB, met |
| Single-file configured-workspace reindex | <= 1 s | 1.346 s, not met | 1.152 s, not met |
| Compact MCP `tools/list` result | <= 1,000 tokens | 622, met | 622, met |
| Largest default answer among 11 queries | <= 2,000 tokens | 1,996, met | 1,996, met |

MB means 10^6 bytes; tokens are ceil(UTF-8 bytes / 4). Public fixture peers supplement the
private workload; the receipt does not claim every feature naturally occurs in the private root.
The [rerun of 2026-10-30](https://github.com/ClaudioMascaro/meada/issues/152#issuecomment-6531862493)
on main `1d648ad6` uses a reconstructed workspace (not directly comparable) and meets four of the
five targets: cold index (medians 5.586 to 6.625 s, <= 7 s), root index size (31.883266 MB,
<= 37 MB), `tools/list` (643 tokens, <= 1,000) and largest default answer (1,999 tokens,
<= 2,000). The single-file reindex (<= 1 s) met the target in the two quieter runs (0.99 and
0.931 s) and missed it in the two runs at a load average near 3 (1.068 and 1.123 s). The
maintainer [waived the reindex target for 1.0.0](https://github.com/ClaudioMascaro/meada/issues/350#issuecomment-6532716065),
recorded on [#350](https://github.com/ClaudioMascaro/meada/issues/350).

Known limit: the single-file reindex sits near 1 s and exceeds it under load. #350 stays open for
1.x, with the goal of 1 s with headroom under ordinary load; a speedup changes no output, so it
ships as a patch or minor release. Publication is done separately by the maintainer under
[#154](https://github.com/ClaudioMascaro/meada/issues/154).

## [0.1.0] - 2026-10-30

The first release: the trunk of the roadmap (ADR 0003) and every spec merged since, with
identity automation (ADR 0016, #319) and named workspaces (ADR 0020) landing on the release day.
The trunk's detailed entries are in [docs/releases/0.1.0.md](https://github.com/ClaudioMascaro/meada/blob/v0.1.0/docs/releases/0.1.0.md);
the two later specs are listed below in full. Prebuilt binaries cover Linux and macOS on x86_64
and arm64 and Windows on x86_64; Windows arm64 waits for a toolchain that builds it.

### Added

- TypeScript and JavaScript indexing with tree-sitter (`.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`,
  `.cjs`): definitions, imports (relative paths, `index` resolution, `tsconfig.json` `paths`)
  and calls, each with a level (`exact`, `inferred` or `ambiguous`). A call through a local
  binding never resolves `exact` to the definition it hides.
- Python language pack (spec #24): definitions, imports (relative, packages, `import *` with
  `__all__`, source roots from `pyproject.toml`) and calls, with receivers resolved through
  `self`, annotations, constructor calls and the method resolution order. The main guard and
  `[project.scripts]` targets are program entry points.
- Go language pack (spec #25), with no Go toolchain: functions, methods, structs and interfaces;
  imports through the module path of the owning `go.mod` or a local `replace`; one scope per
  package directory; build constraints for a fixed `linux/amd64` target; method calls through
  the receiver types the source writes; `func main` of each `package main` as an entry point.
- Rust language pack (spec #30), with no Rust toolchain: crates built from their roots through
  `mod` declarations; `use` declarations with renames, globs and `pub use` re-exports; path and
  method calls resolved through the module tree and written types, a type from outside the
  repository staying opaque; only the default configuration of a crate, for a fixed `x86_64`
  Linux target and its default features; `fn main` of each binary target as an entry point.
- Java language pack (issues #84, #85, #91), with no JDK or build tool: package-qualified types
  and signatures, overloads narrowed by argument count, package and static imports, calls
  through fields, parameters, locals, return types and supertypes, read per class `family`, and
  `public static void main` as an entry point.
- Kotlin language pack (issue #95), with no Kotlin compiler: `.kt` and `.kts` files with classes,
  objects, companions, extension functions and properties; single, aliased and star imports;
  calls `exact` when the argument count leaves one function; Kotlin and Java files of one
  repository call each other.
- C# language pack (issue #107), with no .NET SDK or MSBuild: `.cs` files grouped into projects
  by SDK-style `.csproj` files, namespace-qualified and `partial` types, `using` in all its forms
  (`global`, `static`, aliases, a project's `<Using>` items and implicit usings), what referenced
  projects pass on and the `Directory.Build.props` files MSBuild imports.
- Swift language pack (spec #29, #117), with no Swift toolchain or Xcode: each SwiftPM target is
  a module; symbols named with their argument labels and extension members under the extended
  type; calls narrowed by labels, `exact` on a receiver of written type; a `@main` type and a
  `main.swift` start a process.
- HTTP contracts across repositories. TypeScript `fetch` consumers (client wrappers and base URLs
  from the environment included) link to Next.js app router and Express providers; every link
  carries a level and a reason, and a call whose URL cannot be read is an `unresolved consumer`,
  never linked.
- HTTP providers in every other pack: FastAPI, Flask and Django URLconfs; `net/http`, gin, echo,
  chi and gorilla/mux; axum, actix-web and Rocket; Spring MVC and JAX-RS; Ktor; ASP.NET Core
  controllers and minimal APIs; Vapor and Hummingbird. Routers and groups are followed through
  assignments, arguments, return values and imports, so a route is served under every prefix
  around it; a prefix or mount Meada cannot read serves nothing.
- HTTP consumers in every other pack: `requests`, `httpx` and `aiohttp`; `net/http` and resty;
  reqwest and ureq; RestTemplate, WebClient, `HttpRequest`, OkHttp and the `declarative client`s
  OpenFeign and Retrofit (Java and Kotlin); the Ktor client; `HttpClient`, typed clients and
  Refit; `URLSession`, Alamofire and AsyncHTTPClient. URLs fold from literals, constants,
  concatenation and format strings, and a `client wrapper` is read at each of its call sites.
- Base URLs from the environment and configuration in every pack (`os.Getenv`, `std::env::var`,
  `System.getenv`, `Environment.GetEnvironmentVariable`, `ProcessInfo`), Spring `@Value` with
  `application.properties` and `application.yml`, and C# configuration with `appsettings.json`,
  mapped to a `peer` through `base_url_env`. Meada never reads the value. Cross-language fixtures
  link each language with TypeScript.
- Channels in Python, Go, Rust, Java, Kotlin and C#: Kafka, RabbitMQ, Amazon SQS, NATS, Redis
  streams, JMS and Azure Service Bus clients are a `producer` or a consumer of a `channel`, linked
  to the same channel in other languages, with the new broker kinds `jms` and `servicebus`. A
  call counts only when its client traces back to the library; a name Meada cannot evaluate is
  an `unresolved side`.
- Packages as contracts across seven `package ecosystem`s: npm (spec #22), PyPI, Go (spec #25),
  Cargo (spec #30), Maven and Gradle (spec #26), NuGet (spec #28) and SwiftPM (spec #29).
  A dependency on a workspace package links to it with its range flag, an import of the package
  crosses into the provider's public declarations, `search` takes the kind `package`, and
  `context` on an exported declaration lists its consumers in other repositories.
- Shape drift (spec #23, ADR 0009): each field a TypeScript or JavaScript consumer reads from a
  response that the linked route does not return. Provider shapes come from an OpenAPI
  `provider document`, a zod schema or the returned object literals, also through packages
  (issue #175); consumer shapes from reads, casts and type annotations. `trace`, `context`,
  `search` (kind `drift`), `meada status` and `meada export` show it.
- Communities and processes (spec #31): groups of symbols that call each other, found with a
  deterministic Leiden and named after their most common directory, and `entry point`s (`route`,
  `channel`, `export`, `program`) whose call tree `trace` renders from `process::<identity>`
  across repositories. `search` takes the kinds `community` and `process`, and `context` adds
  a symbol's community and the processes it belongs to.
- The query surface: an MCP server over stdio with three read-only tools, `search`, `context` and
  `trace`, answering compact English lines under a token budget; `context` names the span's
  range on its first line and cuts the span text first. `search` with kind `duplicate` lists the
  symbols whose text is the same in two or more places of the workspace.
- The `meada` command (`index`, `status`, `export` with byte-reproducible JSON, `mcp`), messages
  in Portuguese or English after `LANG`, `meada.toml` with `[[peer]]` and `[serve]`, workspace
  discovery through the `origin` remote, and a per-repository SQLite index in `.meada/`
  refreshed on every query from the files changed on disk. `meada status` points to the new
  `inference` issue form when a repository holds a shape drift or an `inferred` link.
- Server mode (spec #259, ADR 0011): `meada mcp --listen` serves the three tools over MCP
  streamable HTTP, one endpoint per repository, from the mirrors a `workspace manifest` lists;
  indexes are refreshed by command, never by the server, and a reverse proxy admits the clients.
- Cloud modules (ADR 0012), cargo features off by default and all enabled by `cloud`: the shared
  Valkey connection; the `snapshot store`, where `meada snapshot publish` uploads a `generation`
  with BLAKE3-checked bundles and `meada snapshot pull` extracts it as hostile input; serving
  generations with `--generations`, pinned at `/<identity>@<generation>`; the generation pointer
  and retained generations in Valkey; and the `refresh lock` with its `fence`, never swapping a
  `node` to a lower one.
- Operations on generations (ADR 0014): `meada snapshot pointer set`, `retain` and `drop` and
  `meada snapshot fence recover` (#299, #300), a named lock holder, and the `heartbeat` each
  `node` writes to Valkey after every pull round.
- OIDC admission (ADR 0012, feature `oidc`): the server becomes an OAuth 2.1 resource server
  that checks a bearer JWT against the issuer's JWKS and bounds every answer to the identities
  its claims, or its groups through `[oidc.groups]`, grant (#290, #291).
- GitHub App credentials (ADR 0013, feature `github-app`): `meada github-app credential` is git's
  credential helper for the indexing job, minting installation tokens cached in Valkey and
  skipping throttled installations.
- Workspace versions (spec #293, ADR 0014): with `[workspace] source = "store"`, the
  `[[repository]]` entries live as immutable documents in the snapshot store, managed with
  `meada workspace import`, `list`, `show`, `apply` (under the refresh lock, rolling back by
  applying an older one) and `pull`.
- The management `console` (spec #293, ADR 0014, feature `ui`): `meada ui --listen` serves a
  static Vite, React and TypeScript front end and a JSON API, with OIDC sign-in by authorization
  code with PKCE S256, `viewer` and `operator` roles, operational pages for fleet, store,
  repositories, GitHub App and admission, a query playground, code graph views, and operator
  writes with one audit line each, in Brazilian Portuguese and English. The workspace editor
  and version history (issue #310) validate entries and diff versions in the browser through
  the shared WebAssembly core.
- Team ownership (ADR 0015): a workspace entry may name its `owner` team, and a `team operator`
  may save and apply changes to its teams' entries alone; any other change is refused naming
  each entry and its reason. The editor marks refused changes as they are typed, and the
  console documentation gains a Teams section. The whole is additive, a minor change.
- Distribution: a cargo-dist release workflow building binaries for Linux, macOS and Windows on
  x86_64 and arm64 with a shell installer, and an npm package `@meada/meada` (spec #32) staged
  with each release for `npx @meada/meada`; multi-arch OCI images, default and `-cloud` (issue
  #279), with the console's static files in the cloud image (issue #313); a Helm chart (#281);
  Docker Compose examples (#280); and opt-in console profiles for both (issue #314).
- Documentation and community: README, CONTRIBUTING, code of conduct, security policy and issue
  and PR templates in Portuguese, English and Spanish; a CI workflow; a multilingual
  documentation site (issue #320); ADR 0020, recording the direction of spec S5.
- Every language pack and every manifest and configuration reader has a hostile-input probe
  held to a time and size budget.
- Validation and operation of the named-workspace flow (#347, spec S5): a failure test that forces
  the role check, the fetch/index and the local publication to fail in turn and shows the current
  generation stays queryable, eligibility stays fail closed and a revoked grant never revives
  (only a later complete permissions set restores it); the upgrade procedure (migrate to schema 8,
  upgrade every binary before the first refresh that records a context, then configure), the
  failure table and the acceptance map on the named-workspaces page in Brazilian Portuguese,
  English and Spanish; `[catalog]` and `[workspaces]` in the Compose bootstrap examples; and the
  Helm values `modules.workspaces` with `values-workspaces.yaml`, which put those sections (and
  the `[ui]` block the job needs) in the console's and the job's bootstrap and never a node's.
  The measurement against a real GitHub organization (#338) stays open and does not gate S5. No
  code, default dependency or golden changed.
- The proposal server (#346, ADR 0022, feature `proposals`, enabled by `cloud`): `meada workspaces
  mcp --listen <addr> --workspace <bootstrap>` with `[proposals]` (`public_url`, `audience`)
  serves CLI agents four read-only MCP tools over the console's own named-workspace functions:
  `catalog` (the repositories the caller may select), `workspaces` and `show` (the workspaces it
  may see, with their `seq`) and `propose`, which validates manifest text as a new workspace or a
  change and returns the canonical manifest, the diff against the base, diagnostics, each named
  identity's eligibility, the refusals the console would give, the effective configuration,
  `applicable` and the base, storing nothing. Identity, login, grant and role come from a bearer
  token for `[proposals] audience`, which no node or console accepts and which accepts no
  node's or console's token; no tool takes an argument naming an actor, a role or a grant, and
  repository hints from an agent's own GitHub read MCP change no result. Every Postgres session
  of the server is read-only, it calls no GitHub, node or identity provider API and writes no
  audit record, store object or Valkey key. Inputs and answers are bounded (a body of 80 KiB, a
  manifest of 64 KiB, an answer of 192 KiB, 32 calls in flight). The node's MCP server, the
  console and the default build are unchanged.
- Guided authoring of named workspaces (#345, ADR 0021 "As built (#345)", feature `workspaces`):
  the console edits the manifest of a workspace the subject operates through form controls
  (repositories from the catalog, serve settings, resources, peers and environment-variable
  names) that are one document with the manifest text, both going through `meada-core`'s
  WebAssembly. It shows each entry's eligibility, ingestion and effective configuration in text,
  the diff against the stored version, and freezes the reviewed payload until the person
  confirms; server refusals (`entries_refused`, `stale_base`, a lost operator role) are shown as
  worded and the catalog is read again. `namedDiff` and `namedChanged` join the WebAssembly
  exports. Viewers get no mutation control. No server route changes; nothing changes without
  `[workspaces]`.
- Context-specific configuration of named workspaces (#344, ADR 0021 contract 6, feature
  `workspaces`): `GET /api/v1/workspaces/<id>/effective[?version=]` shows, per entry the caller
  reads, where its configuration comes from (the repository's own `meada.toml` wins whole, else
  the workspace's `serve`, `resource` and `peer` blocks, else nothing), the blocks that are
  ignored, whether the repository is indexed, pending ingestion or excluded, and diagnostics
  (`ignored_by_repository`, `unknown_until_ingested`, `excluded_no_sponsor`, `peer_missing`,
  `documents_not_indexed`). `POST /api/v1/workspaces/effective` shows the same view for manifest
  text before it is saved and stores nothing; creation, save and apply answers carry the same
  `diagnostics`, so an ignored declaration never looks plainly applied. Two workspaces that
  share a repository keep their own peers, resources and serve hosts: the configuration is applied
  when answering and never enters a bundle, so a change of it re-indexes nothing and a pinned
  generation keeps what it recorded. The decision is `meada_core::effective`, also exported to
  WebAssembly (`namedEffective`). The console's workspace page shows the view in text and has a
  text editor for the manifest of a workspace the subject operates. Nothing changes without
  `[workspaces]`.
- The version lifecycle of named workspaces (#343, ADR 0021 contract 4, feature `workspaces`):
  `POST /api/v1/workspaces/<id>/versions` saves a version without moving anything (an operator
  submits the entries it reads and the version stored keeps the ones it cannot read; the
  platform administrator submits the whole manifest), `.../apply` applies a stored version (a
  version that was current before is a rollback), `.../rename`, `.../delete` (versions and
  history are kept) and `.../operators` (the platform administrator delegates; a workspace keeps
  at least one operator). Every write carries the `base` it read: a stale base, a revoked role, a
  refused entry or a version of another workspace is refused and changes nothing, and the
  transaction rechecks authority, base and the eligibility of what the operation adds or changes
  against the newest permissions set. Only an operator of the workspace or the platform
  administrator writes; a viewer is a 403 and a caller who cannot see the workspace gets the 404
  of an unknown id. `GET .../versions`, `.../versions/<version>?against=` (with a diff) and
  `.../history` read them under the same discovery rule. Each operation is one audit record
  (`workspaces.save`, `.apply`, `.rollback`, `.rename`, `.delete`, `.operators`) with the
  workspace and base. `meada workspaces list|show|versions|diff|create|save|apply|rename|delete|operators` are
  the command-line twins (the platform administrator, `cli` audit records). Migration 8 adds
  `workspace_versions.entries`: run `meada postgres migrate` before upgrading the binary. Without
  `[workspaces]` nothing changes.
- The first named workspace of spec S5 (#342, ADR 0021 contracts 2 to 5, cargo feature
  `workspaces`): `[workspaces]` (`mirrors`, an absolute root; `max_workspaces`, default 200, 1 to
  2000; `max_entries`, default 100, 1 to 500) needs `[ui]` with `operations = true`,
  `[oidc.github]`, `[catalog]` and `[workspace] source = "store"`. An admitted subject whose
  GitHub login holds an accepted role on every selected repository, directly or through a group,
  creates a workspace with `POST /api/v1/workspaces`; `GET /api/v1/workspaces` and
  `GET /api/v1/workspaces/<id>` list and show it to whoever's own grant reads every one of its
  repositories (anyone else gets the one 404 of an unknown id), with `state` (`applied` or
  `live`) and each entry's `ingestion` (`pending`, `ready` or `excluded`), never ready before a
  generation records it. The creator operates it; group peers only view. The
  decision is taken in one Postgres transaction against the newest permissions set (migration 7:
  `workspaces`, `workspace_operators`, `workspace_versions`, `workspace_history` and a `context`
  column on `permission_exclusions`; run `meada postgres migrate`); the version document is the
  shared store's `workspaces/<version id>.json`. `meada workspace pull` also writes the contexts
  file, `meada github-app roles` applies the sponsor rule (an entry no operator of its workspace
  holds an accepted role on is left out before the fetch, `no_sponsor`, which is not a failed
  fetch), and the repositories only named workspaces select are fetched, indexed and bundled once
  at `<mirrors>/<identity>` (`meada workspace show --pulled` lists them). A generation records them
  as `members` and the named workspaces as `contexts` (ids and digests in the generation id; a
  deployment without named workspaces keeps every id), and a node serves
  `/-/workspaces/<id>/<identity>` only to a token that reads every repository of the context (the
  first segment `-` is reserved); the console's playground gains `workspace` and
  `GET /api/v1/workspaces/<id>/repositories/<identity>/export`. The console has the workspaces
  page (list, detail, creation from the catalog) and WebAssembly exports for the shared
  rules. Save, apply, rename, delete and delegation (#343), per-context configuration (#344),
  guided authoring (#345) and proposals (#346) are not built. Without `[workspaces]` every
  process behaves as before; upgrade every binary, nodes included, before the first refresh that
  records a context.
- The repository catalog of spec S5 (#341, ADR 0021 contract 1, cargo feature `workspaces`,
  part of `cloud`): `[catalog]` (`allow` patterns on `[oidc.github] host`, `max_repositories`
  1 to 5000 with no default) makes `meada github-app roles` list, besides the workspace's own
  repositories, those the Apps' installations cover (`GET /installation/repositories`, every page,
  only for owners a pattern names) and `allow` admits, into the same permissions set with a
  `source` on each repository (Postgres migration 6; run `meada postgres migrate`). A failed or
  throttled listing, a body that is not the documented object or a candidate set above the cap
  fails the run and writes nothing. The console gains `GET /api/v1/catalog` (bounded, opaque
  pagination, `limit`, `after`, `q`) and `POST /api/v1/catalog/check`, answering only the
  repositories the caller's login holds an accepted role on, direct or group-derived alike, with
  the states `ready` and `pending` (`no_set`, `stale_set`, `missing_login`) and `unavailable`
  without a reason, plus the repository catalog page; App coverage, group names in a token and
  agent metadata alone grant nothing, and discovery creates no workspace, ACL or index. Without
  `[catalog]` every process behaves as before. The cost of expanded coverage against a real
  organization is not measured (#338 does not gate it).
- Spec S5 consolidated (#340, documentation only; nothing is implemented): ADR 0020 now holds
  the accepted domain decision (platform administrator, workspace operator and inherited viewer,
  creation eligibility from accepted repository roles, the frozen Y/A/B matrix, hidden discovery
  for partial readers, revocation), ADR 0021 the contracts (catalog, selected entries, shared
  ingestion membership, version lifecycle, query selection, effective configuration,
  deterministic ids, migration and the slice graph #341 to #347) and ADR 0022 the authenticated
  stateless proposal server. The glossary gains the terms, ADRs 0002, 0012, 0014, 0015 and 0016
  carry dated amendments, `tests/fixtures/named-workspaces.json` is the Y/A/B table with a
  `meada-core` test that checks it against the rules, and the named workspaces and roadmap pages
  are updated in three languages. No serialized role, route, id, golden or default build changes.
- Identity automation for enterprise deployments (ADR 0016, spec #319), in the entries below: the
  `postgres` and `directory` features and the `[postgres]`, `[directory]`, `[oidc] type`,
  `[oidc.entra]` and `[oidc.github]` sections; `meada postgres migrate`, `meada github-app roles`
  and `meada workspace show --pulled`; an entry's `added_by` and the `unattributed` and
  `added_by_change` reasons of `entries_refused`; the session's `login`, `github_host` and
  `github_grant`, the token check's `sources`; the audit and `GET /api/v1/audit`; the SCIM endpoint
  under `/scim/v2/`, the `deprovisioned` refusal, the owner picker, `owner_missing` and `excluded`.
  Under Spec R1's stability rule every one of them is additive: a minor change. Without the new
  sections every process behaves as before, and the default build, the CLI help, the MCP surface
  and every export and trace golden are unchanged.
- The identity automation documentation, in three languages: an enterprise identity guide (the
  data layer, the audit, the provider type, the GitHub grant, additions under the provider, the
  directory, Entra overage and Known ceilings), recipes for Entra ID, Okta and Keycloak, the
  roadmap and diagrams with S4 delivered, and the architecture map's Known ceilings gaining the
  login length and the audit's growth. S5 (#339), named workspaces and authoring through the UI or
  MCP, stays planned.
- The Compose example gains `compose.postgres.yaml` (a Postgres 15 with TLS and the three
  recommended roles, and a one-shot `meada postgres migrate` step that finishes before the job,
  the console and the nodes), a `fetch-pulled.sh` for the job's refresh under `[oidc.github]`, and
  commented `[postgres]`, `[oidc.github]`, `[directory]` and `[oidc.entra]` examples in the
  bootstraps. The Helm chart gains `modules.postgres`, `modules.githubApp`, `modules.oidc.github`,
  `modules.oidc.type` and `entra`, `modules.directory`, a migrate hook Job with pods that wait for
  the schema, an example Postgres (`postgres.enabled`) and `values-identity.yaml`; the default
  values render nothing of Postgres.
- `[oidc.github]`, the GitHub grant provider's table (ADR 0016): `host` (default `github.com`, a
  lowercase host name), `login_claim` (required), `roles` (a non-empty array of distinct role
  names of 1 to 100 characters with no control character, compared exactly with GitHub's
  `role_name`; default `["read", "triage", "write", "maintain", "admin"]`) and `max_age_seconds`
  (required, at least 60). It needs `[postgres]` and a build with `postgres`, each error naming
  them; beside it `[oidc]` needs neither `identities_claim` nor `groups_claim`.
- The GitHub grant at admission (ADR 0016): nodes and the console with `[oidc.github]` keep the
  newest permissions set in memory, read every 60 seconds without admission ever waiting on
  Postgres, and a token reads the union of its claims grant and what its `login_claim` login (1
  to 39 ASCII letters, digits, `-` and `_`, compared in lowercase) holds an accepted role on,
  everywhere the grant is read: a node's admission, routing, answers and 404 lists, and every
  bounded view of the console, the team operator role, the entry authorization and the token
  check. The GitHub part grants for at most `max_age_seconds` past the set's `finished` on
  Postgres's clock, then nothing, the claims grant unaffected; a machine token's `*` never
  depends on Postgres. A node starts and serves without Postgres, and names a schema version it
  does not know once on stderr. Every row read is checked as hostile input.
- `GET /api/v1/session` gains, with `[oidc.github]`, `login` (when the token has a valid one),
  `github_host` and `github_grant` (`current`, `expired` or `unread`); `grant` is the union.
  `POST /api/v1/admission/check` gains `sources`: `{"claims": "*" | [identities], "github":
  [identities]}`, each sorted, from the console's cache. `GET /api/v1/admission` gains `github`:
  the grant provider's settings, the grant's state and the newest set's id, `finished`, age, `max_age_seconds`,
  repositories, rows and exclusions. The admission and session pages show them, and audit
  records carry the subject's login.
- Additions under the GitHub grant provider (ADR 0015, ADR 0016): a team operator may add, for
  its team, an entry of a repository on `[oidc.github] host` that its grant does not read yet.
  The console's save sets the addition's `added_by` to the saver's login whatever the submitted
  text says (and clears it on additions off the host), and the role check fetches the entry only
  while that login holds an accepted role. Two refusal reasons join `entries_refused`, in
  precedence order `outside_grant`, `unattributed` (an addition on the host without `added_by`,
  or by a subject without a login), `ownerless`, `owner_change`, `added_by_change` (an edit
  changing, clearing or setting `added_by`) and `not_your_team`. A save that adds an identity
  the workspace already holds outside the grant is refused `outside_grant`, naming it as
  submitted. The global operator's saves and the CLI keep `added_by` as written.
  `authorize_entries` and `compose_entries` of `meada-core`, and their WebAssembly exports, take
  the subject's login and the GitHub grant provider's host (none without the GitHub grant provider, answering as before),
  and the editor marks the new reasons as it marks the others, in pt-BR and English, and shows
  each entry's `added_by`.
- `excluded`: with `[oidc.github]`, `GET /api/v1/workspace` and `GET
  /api/v1/workspace/versions/<id>` mark `excluded: true` on each entry of the version the newest
  permissions set checked whose addition it left out, for those who see the entry; the field is
  absent otherwise. The versions page and the editor mark those entries with the reason that the
  adder holds no accepted role.
- `meada github-app roles --workspace <bootstrap manifest>` (features `github-app`, `postgres`
  and `oidc`), the job's role check at every refresh: for each identity of the workspace on
  `[oidc.github] host` it lists `GET /repos/{owner}/{repo}/collaborators?affiliation=all&per_page=100`,
  every page, through the installation ADR 0013's selection picks, and replaces the permissions
  set in Postgres in one transaction (what GitHub showed of each repository, its rows, every
  collaborator listed with its `role_name`, and its exclusions with their reason), refused when a set with a higher `MEADA_REFRESH_FENCE` exists. With
  `source = "store"`, every addition whose `added_by` holds no accepted role is left out:
  written to `<index_dir>/workspace/excluded`, and `meada index`, `status`, `export` and
  `snapshot publish` treat it as absent from the workspace, refusing to run, naming the role
  check, when the file is missing or names another version. A 403 or 429 passes to the next
  installation; every one throttled, a 5xx, a timeout, an answer past the size cap or a
  `Link` header that leaves the repository's listing fails the run, which writes nothing. A
  404 or an owner no installation covers gives a warning and no rows. No App permission beyond
  Metadata (read) is needed. The job's refresh becomes `workspace pull`, `github-app roles`,
  the fetch of what `show --pulled` lists, `index` and `snapshot publish`.
- `meada workspace show --pulled --workspace <bootstrap manifest>` prints the version `meada
  workspace pull` left on disk in the `[[repository]]` shape, without the entries the role
  check excluded, for the fetch script; with a version argument it is a usage error.

- `added_by`, an optional key of a workspace version's `[[repository]]` entries beside `repo`,
  `path` and `owner` (ADR 0016): the GitHub login that added the entry, 1 to 39 characters of
  lowercase ASCII letters, digits, `-` and `_`. It is imported, stored, shown (`meada workspace
  show` prints it right after `owner`) and pulled like `owner`, joins the version id and no
  generation or bundle id, and nothing reads it to decide anything yet. An entry without it keeps
  its version id. A bad value is an error naming the file, the line and the key, in a
  bootstrap manifest's own `[[repository]]` entries (`source = "file"`) the key is an error
  naming `source = "store"`, and a repository's `meada.toml` still rejects it as unknown.
- `[oidc] type` names the identity provider that issues the deployment's tokens (ADR 0016):
  `generic` (the default), `entra`, `okta` or `keycloak`. None changes the admission, the grant or
  any answer, and a bootstrap manifest without the key works as before; the console's `GET
  /api/v1/admission` gains `"type"`, which the admission page shows beside the issuer. Any other
  value, or one that is not a string, is an error naming the file, the line, the key and the four
  values.
- `[oidc.entra] overage = "directory"` (ADR 0016): a Microsoft Entra ID token in more groups than
  it can carry (`_claim_names` mapping `groups_claim` to a source, no `groups`) gets its groups
  from the directory the identity provider pushes through the console's SCIM endpoint: the
  `group_attribute` values of the groups whose members include the active user whose
  `user_attribute` equals the token's `user_claim`. They replace the missing claim everywhere
  `groups_claim` is read: a node's grant through `[oidc.groups]`, and the console's grant,
  `[ui] operator_groups`, the team operator's teams, the session's `groups` and the token check.
  It needs `[directory]` and `groups_claim`, each error naming the file, the line and the key.
  Nodes and the console look a token up once at admission and keep the answer five minutes, per
  `user_claim` value; a failed lookup is not kept, and with Postgres unreachable the token has no
  groups, as before, with one stderr line. Meada never calls the source's endpoint. The console
  still refuses a deactivated or deleted provisioned user as `deprovisioned` before any cached
  answer is used. `overage = "ignore"` keeps today's behavior.
- `[oidc.entra]`, the settings only Entra ID needs, allowed under `type = "entra"` alone, with one
  key: `overage = "ignore"` (the default), a token in overage having no groups, as before. Any
  other value, and an unknown key, is an error naming the file, the line and the key.
- The data layer (ADR 0016): the `postgres` cargo feature, off by default and enabled by `cloud`,
  and the bootstrap manifest's `[postgres]` section, which needs no other section. It names the
  connection URL only through `url_env` (default `MEADA_POSTGRES_URL`) or `url_file`, plus
  `ca_file`, `schema` (default `meada`), `pool_size`, `connect_timeout_seconds` and
  `statement_timeout_seconds`. The URL is `postgres://` or `postgresql://` with a TCP host and no
  query parameter (`sslmode` included); an error names the key, never the URL. A host outside
  loopback always connects over TLS with its name verified, against `ca_file` alone when set,
  else the system roots; a loopback host connects in plain TCP unless `ca_file` is set. Every
  table lives in the schema `[postgres]` names. `[postgres]` in a build without the feature is
  the error naming it. Postgres 15 and later.
- `meada postgres migrate --workspace <bootstrap manifest>` creates the schema when absent and
  applies each migration up to the binary's version, each in its own transaction, under an
  advisory lock keyed by the schema, so two runs serialize; it prints the version reached and
  never lowers one. It ships migration 1, the schema's version. Every later connection refuses a
  version the binary does not know: a lower one (an absent schema included) names `meada
  postgres migrate`, a higher one names both versions.
- The directory (ADR 0016): the `directory` cargo feature, implying `postgres` and enabled by
  `cloud`, and the bootstrap manifest's `[directory]` section, which needs `[postgres]` and
  `[oidc]`: `token_env` (default `MEADA_SCIM_TOKEN`) or `token_file`, `user_claim`,
  `user_attribute` (`userName` or `externalId`) and `group_attribute` (`displayName`, the
  default, or `externalId`). Only the console reads the token, at start: shorter than 32
  characters, unset or unreadable, it refuses to start naming the key, never the value.
  `[directory]` in a build without the feature is the error naming it.
- The SCIM 2.0 endpoint: with `[ui]` and `[directory]` the console serves `/scim/v2/`, where
  Entra ID's and Okta's provisioning create, read, filter, page, replace, patch and delete users
  and groups, stored in the data layer (migration 3) as the provider sent them, never with
  `password`. It admits only its own bearer token (no console token, and no request carrying
  `Origin`), bounds a body at 1 MiB and a resource at 64 KiB, answers RFC 7644's errors with
  their `scimType` and no echoed value, a 503 with Postgres unreachable, and the
  `/ServiceProviderConfig`, `/ResourceTypes` and `/Schemas` documents. A deleted user leaves
  every group and a revocation marker until it is provisioned again, active. Each write leaves
  one stderr line, `scim: <method> <resource> <id>: <status>`.
- Deprovisioning in the console (ADR 0016): with `[directory]`, the token's `user_claim` is looked
  up in the directory by `user_attribute` (`userName` ignoring case, `externalId` exact). A user
  found inactive, or deleted and not provisioned active since, gets a 403 `deprovisioned` on every
  route but `GET /api/v1/config`, whatever its role, and the console shows a deactivated-account
  page. A user never provisioned is admitted. With the directory unreadable, reads are admitted
  and a write is a 503 `audit_unavailable` before any audit insert, with one stderr line that
  names the directory and never the claim. Migration 4 adds the directory's state row (the schema
  version is now 4); a directory that had already lost its last group before the upgrade starts
  with no group ever provisioned, so nothing is flagged until a group is provisioned again.
- The owner picker: `GET /api/v1/directory/groups?prefix=<text>`, for the global operator alone,
  lists at most 100 group names starting with the prefix (compared exactly, sorted), and the
  workspace editor offers them while the caret is inside an `owner = "..."` string.
- `owner_missing`: `GET /api/v1/workspace` and `workspace/versions/<id>` add `"owner_missing":
  true` to an entry whose owner names no directory group, once a group was ever provisioned; the
  versions page and the editor say so in text. `GET /api/v1/admission` gains the directory's
  users and groups counts and the last SCIM write, which the admission page shows.
- The audit (ADR 0016): with `[postgres]`, every console write (`workspace.save`,
  `workspace.apply`, `pointer.set`, `pointer.retain`, `pointer.drop`, `fence.recover`) and every
  CLI twin of one (`meada workspace import` and `apply`, `meada snapshot pointer set`, `retain`
  and `drop`, `meada snapshot fence recover`) leaves a record in `<schema>.audit` (migration 2):
  who (subject, role, a team operator's owners), what (action and arguments), when (Postgres's
  clock) and the outcome. The record is written as `pending` before the write touches Valkey or
  the store, so a write whose record cannot be written is refused and changes nothing: the
  console answers 503 `audit_unavailable`, the CLI exits 1 naming Postgres. Role refusals and
  refusals under the lock are recorded too. The console starts with Postgres unreachable (reads
  are served) and refuses to start on a schema version it does not know, as each CLI twin exits
  1. The global operator reads the records, newest first, through `GET
  /api/v1/audit?before=<id>&limit=<n>` and the console's new audit page. Without `[postgres]`
  every write behaves as before, and the stderr lines are unchanged in both cases.
- `meada status --workspace` with `[postgres]` prints `postgres: reachable, schema <name>
  version <n>` or `postgres: unreachable (<reason>)`, a version refusal included, without
  changing its exit code.

### Changed

- The repository is a Cargo workspace (ADR 0014, #296, #297): the library crate `meada-core`
  holds the manifest schema, the generation and version ids, the export's types, the query
  results and shape drift, builds for WebAssembly for the console, and is published to
  crates.io before `meada`. Every golden and message of the default build is unchanged.
- A refresh publishes the whole workspace or nothing (ADR 0012, #316), and a workspace of any
  number of repositories opens and answers (the limit of eleven is gone).
- `context` and `trace` show the overloads of one method as one set, and a call that decorates
  a definition (an annotation, an attribute, a decorator, a route attribute) is listed under
  `annotations:` in `context` instead of among the callees.
- A Kotlin `HttpClient` held through more locals than `LOCAL_DEPTH` is now a consumer.
- The `decisions` `decider`'s default model is now `typesafe/jev-1.14`; `MEADA_DECIDER_MODEL`
  still overrides it.
- The coding standards class each string table a language pack holds, and a test fails on an
  unlisted one.

### Fixed

- `meada mcp` answers about the client's first MCP root and falls back to its own directory
  without roots (#192); a `roots/list` that times out after 5 s or fails answers about the last
  root, or returns a tool error, instead of hanging or answering about the wrong tree (#282,
  #285).
- TypeScript and JavaScript resolution: a call with type arguments under `await`, the `paths` of
  a nested `tsconfig.json`, functions defined by assignment (`exports.y =`, prototypes, object
  literal methods) and a name two `export *` give differently.
- Python resolution: nested `pyproject.toml` package roots, star imports and names bound again
  (the last binding decides, `del` and a grown or computed `__all__` included).
- Java and Kotlin resolution: annotations written in full, method annotations through a base
  from outside the repository, calls on another call's result, secondary constructors,
  `expect` and `actual` classes, star imports, varargs, Java locals followed into their value,
  separate field and method namespaces, and Maven or Gradle coordinates named by a property.
  Ambiguous or unresolved OpenFeign and Retrofit methods are `open item`s that `meada decide`
  resolves.
- C# resolution: types of enclosing namespaces, `internal` types, a qualified name two projects
  declare, library names written in full, member reads between calls and overloads across
  projects; `obj/` and `bin/` stay out of the index.
- Swift resolution: a module per package with `@testable import`, `typealias` receivers,
  implicit members, capitalized function calls, outside member reads as `opaque outside`, Vapor
  middleware groups and `if let` environment reads.
- Rust resolution: calls on another call's result, types brought by a glob import, a `mod` any
  test build compiles, and broker clients read by path.
- A class whose outside bases are all platform types with `known outside members` resolves
  calls to its own members at `exact` (ADR 0019).
- Spring and Retrofit contracts: an AMQP publish of a repository class, `@Value` fields beside
  same-named locals, Gradle source sets outside `src/main`, Retrofit path rules, and `context`
  on a shared artifact's class listing its calls in other repositories.
- What gets indexed: `node_modules`, `Pods`, Carthage `Checkouts` and minified files stay out,
  `.meadaignore` is read like a `.gitignore`, `Package@swift-<version>.swift` is a manifest, a
  large generated C# file is read whole (or named by `meada status`), and an invalid or
  oversized config no longer stops indexing.
- Hostile input: crafted chains of re-exports, receivers, includes and static imports no longer
  overflow the stack; folded values stop at a budget of 2^16 steps per file or 256 levels; a
  manifest past 1 MiB and a provider document past 16 MiB are read capped, and a Spring YAML
  past its key budget reads as empty.
- Performance: method chains, files of thousands of names, imports or definitions, Java local
  chains, deeply nested Rust blocks and large C# project files no longer take time quadratic
  or worse in their size, and a class family near 128 classes resolves the same whatever the
  reading order.
- Console and fences: manifest groups and validation errors respect the viewer's grant, PKCE
  redirects keep the issuer's query parameters, and fence recovery shares the fleet reader's
  Valkey clock and heartbeat checks.
- Rust item macros (#398): the braced body of a `macro_rules!` of the repository whose first arm
  takes `$($x:item)*` and writes each item back behind outer attributes only (tokio's
  `cfg_rt! { pub mod runtime; }`) is read as items of the module invoking it: its functions,
  types, `impl` blocks, `use` and `mod` declarations and the calls inside them, with spans into
  the file. A `mod` in it reaches its file, and the `#[cfg(..)]` attributes the arm writes gate
  every item under the crate's default features. A name defined several times qualifies only when
  all definitions agree; any other macro stays unread. A refresh parses a file again when what it
  invokes changes elsewhere, and equals a cold index.

### Measured

The maintainer ran this release on the private dogfood set on 2026-10-14 (ADR 0003).

Links. One consumer repository calls the provider from four sites in its current code (ADR 0003
counted five); all four link to the right route at `inferred`, through `base_url_env`. The other
consumer repository's three calls do not link: its client receives `fetch` as a parameter and
takes the URL from a destructured object, two patterns this release does not read (#157, #158).

Cost, on the provider repository at the 2026-10-11 commit nearest to 633 files (634 TypeScript
and JavaScript files), median of three runs on a 16-thread Linux laptop:

| Metric | Target | Result |
| --- | --- | --- |
| Cold index time | ≤ 7 s | 1.2 s |
| Cold index size | ≤ 37 MB | 21.8 MB |
| Single-file reindex | ≤ 1 s | 0.24 s |
| Tool list | ≤ 1k tokens | 454 tokens |
| Default answer | ≤ 2k tokens | 1,834 tokens, the largest of 11 answers |

At today's 664 files: 1.3 s, 23.6 MB and 0.26 s. Tokens count 4 bytes each, as the answer budget
does. These numbers gate 1.0, not this release.

Communities and processes (spec #31) were measured on 2026-10-17 on the same repository, at the
same commit and at that day's 692 files. The runs used release builds of `main` just before and
just after the spec, each a median of three runs on a 16-thread Linux laptop with 7.2 GiB of
RAM. MB here means 10^6 bytes.

| Metric | Target | 634 files, before → after | 692 files, before → after |
| --- | --- | --- | --- |
| Cold index time | ≤ 7 s | 1.81 s → 1.80 s | 2.20 s → 2.12 s |
| Cold index size | ≤ 37 MB | 22.7 MB → 23.0 MB | 26.8 MB → 27.1 MB |
| Single-file reindex | ≤ 1 s | 0.74 s → 0.76 s | 0.89 s → 0.92 s |
| Tool list | ≤ 1k tokens | | 620 tokens |
| Default answer | ≤ 2k tokens | | 1,996 tokens, the largest of 11 answers |

The tool list counts the whole `tools/list` result as compact JSON. Among the 11 answers are
`context` on the three most-called symbols, each with its community and process membership,
and `trace` from the two longest processes. The spec adds about 0.3 MB and 0.03 s to a reindex.
The rise of the single-file reindex since 2026-10-14 predates it (specs #22 and #23), and at
692 files it is close to its target.

[Unreleased]: https://github.com/ClaudioMascaro/meada/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/ClaudioMascaro/meada/releases/tag/v1.3.0
[1.2.0]: https://github.com/ClaudioMascaro/meada/releases/tag/v1.2.0
[1.1.0]: https://github.com/ClaudioMascaro/meada/releases/tag/v1.1.0
[1.0.0]: https://github.com/ClaudioMascaro/meada/releases/tag/v1.0.0
[0.1.0]: https://github.com/ClaudioMascaro/meada/releases/tag/v0.1.0
