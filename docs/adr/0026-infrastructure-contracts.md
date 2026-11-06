# 26. Infrastructure contracts

- Status: accepted
- Date: 2026-11-04

## Context

ADR 0005 links code to code: HTTP routes, declared resources, provider documents, channels and
packages. Spec #642 (A7, config and IaC in the graph) adds the infrastructure that creates the
queues, buckets and services the code uses. ADR 0025 records how Meada reads config values and
IaC nodes and evaluates the names they build; this ADR records the contracts those nodes and
values take part in, and amends ADR 0005 where they change its model. The decisions come from
the grill of #639 on 2026-11-03 and the writing of #642.

## Decision

### Provisioner and provisioning link

- **Provisioner** is a third role of a contract, beside producer and consumer (channels, HTTP)
  and writer and reader (resources). Its side is the IaC node that creates the resource,
  labelled by its address and the root module or overlay that roots its evaluation (ADR 0025).
- Every other side of the contract links to each provisioner of it through a **provisioning
  link**: a kind of link of its own, not a producer or consumer link, with a level and a reason
  like any link. A consumer still links to its producers, and a reader to its writers, as today.
- Several provisioners of one name are normal (one per environment, overlay or region): they
  are never `ambiguous` among themselves, and each gets its provisioning links.

### Identity, brokers and resource kinds

A cloud resource joins the contract kinds that exist by its physical name, the name the type
table (ADR 0025) reads out of the node. Accounts, regions and clusters are not modelled: one
name of one kind is one contract across the workspace, as channels are today.

Queues, topics, buses and streams are channels, `channel::<broker>::<name>`. Meada reads the
brokers `amqp`, `jms`, `kafka`, `nats`, `redis`, `servicebus` and `sqs` today; `sns`,
`eventbridge`, `kinesis`, `pubsub` and `eventhubs` are new. IaC provisions these:

| Broker | Provisioned by | Name |
| --- | --- | --- |
| `sqs` | an SQS queue | the queue name |
| `sns` | an SNS topic | the topic name |
| `eventbridge` | an EventBridge event bus | the bus name |
| `kinesis` | a Kinesis data stream | the stream name |
| `pubsub` | a GCP Pub/Sub topic | the topic id |
| `pubsub` | a GCP Pub/Sub subscription | `subscriptions/<subscription id>` |
| `servicebus` | an Azure Service Bus queue or topic | the queue or topic name |
| `eventhubs` | an Azure Event Hub | the event hub name |
| `kafka` | a Strimzi `KafkaTopic` | `spec.topicName`, else `metadata.name` |
| `amqp` | a RabbitMQ messaging topology `Queue` | the queue name |

A RabbitMQ topology `Binding` object is a binding (the glossary's), read like `bindQueue`, and
an exchange object is no channel, as a publish to an exchange is no channel today.

Buckets, tables, databases, caches and secrets are resources, `resource::<kind>::<name>`. The
curated kinds:

| Kind | Provisioned by | Name |
| --- | --- | --- |
| `s3` | an S3 bucket | the bucket name |
| `gcs` | a Cloud Storage bucket | the bucket name |
| `azureblob` | an Azure Storage blob container | the container name |
| `dynamodb` | a DynamoDB table | the table name |
| `rds` | an RDS instance or cluster | the instance or cluster identifier |
| `cloudsql` | a Cloud SQL instance | the instance name |
| `cloudsql` | a Cloud SQL database | `<instance>/<database>` |
| `elasticache` | an ElastiCache cluster or replication group | its id |
| `ssm` | an SSM Parameter Store parameter | the parameter name, leading `/` kept |
| `secretsmanager` | an AWS Secrets Manager secret | the secret name |
| `gcpsecretmanager` | a GCP Secret Manager secret | the secret id |
| `keyvault` | an Azure Key Vault secret | the secret name |

A curated kind shares the namespace of `[[resource]]` kinds, so a declared `kind = "s3"` entry of
the same id joins the provisioned contract. A `[[resource]]` entry of another kind (`postgres`
for what IaC calls `rds`) stays its own contract. A broker or kind the type table adds later
amends these tables.

### Use sides

- **SDK forms.** The channel and resource kinds learn the cloud SDK forms, one module per
  language, as they own broker clients today: the AWS SDK for JavaScript and TypeScript, Python,
  Java, Go and C#, and the GCP and Azure clients. Each form names the argument that carries the
  name and the direction (`PublishCommand` produces, `GetObject` reads), and is accepted only on
  an origin the kind knows.
- **Config values as use sides.** A config value (ADR 0025) is a use side of a contract when it
  holds a structured identifier (an ARN, a queue URL, `s3://`, `gs://`,
  `projects/<project>/topics/<topic>`), at `exact`, or a plain name under a key that names the
  type (`queue`, `topic`, `bucket`, `table`), at `inferred`. A bare equal value links nothing.
  Such a side states **no direction**: it is neither producer nor consumer, writer nor reader,
  so it takes part only in provisioning links (to each provisioner of its contract) and in
  orphans, never in a producer to consumer link. Its site is the repository, located by
  `<file>#<key path>`.
- **Subscriptions.** SNS to SQS subscriptions, EventBridge rules and their targets, Pub/Sub
  subscriptions and Service Bus subscriptions are read from IaC and followed like a RabbitMQ
  binding: a consumer of the target links to the producers of the source, `exact` for an
  unfiltered subscription and `inferred` with a filter, the reason naming the subscription.

### GCP identities and workloads (#667, 2026-11-06)

Pub/Sub subscriptions use `channel::pubsub::subscriptions/<id>`. A topic id cannot contain
`/`, so a topic and subscription with the same short id cannot acquire a direct exact link
that overrides filtered delivery. The source is only the configured `topic`, resolved from a
literal or a known topic reference through the bounded evaluator. An unresolved source never
creates a topic from the subscription id. A missing or empty filter is exact; a nonempty or
unresolved filter weakens delivery to inferred.

Cloud SQL database identities are `resource::cloudsql::<instance>/<database>`; instances keep
`resource::cloudsql::<instance>`. Complete literal or evaluated instance evidence is required,
and both names contribute overridable provenance. A database never provisions its instance.

The shared GCP kind policy validates complete structured identifiers before omitting projects:
`gs://<bucket>` (object paths are opaque), `projects/<project>/topics/<id>`,
`projects/<project>/subscriptions/<id>`, and Secret Manager names with an optional
`/versions/<version>`. These config uses are exact. The final keys `pubsub_topic`,
`pubsub_subscription`, `gcs_bucket`, `cloudsql_instance`, `cloudsql_database` and
`gcpsecretmanager_secret` give inferred uses; the database key requires `<instance>/<database>`.
Generic `topic` and `bucket` retain their AWS meanings. Malformed and wrong-kind identifiers
cannot fall back to a plain exact name.

Cloud Run v1 services, v2 services and v2 jobs retain each container's image and environment,
evaluated in the node's root and instance. Image defaults preserve their overridable evidence.
Secret references retain their readable target with no resolved value; they do not expose
contents or infer a resource hidden inside those contents. These rows use the existing image,
execution, context, trace, orphan and export seams and the shared 1.4.0 minor release.

### Levels

- A link is `exact` when both names are as written and identified by their form (a plain name
  under a typed key is `inferred`, above), `inferred` when either went through a
  resolution (an overridable value, a workload's environment, a base URL through a Service, a
  pattern), and `ambiguous` when several candidates fit: listed, never picked, an open item
  `meada decide` can close (ADR 0006). A declared `[[resource]]` side keeps the level ADR 0005
  gives it.
- A name built from an overridable value (ADR 0025) links `inferred`, its reason naming the
  values it went through.

### Patterns

- A name with an unresolved part (ADR 0025) is a pattern. A pattern links at `inferred` to a
  literal name on the other side when its literal part has at least three characters and its
  wildcards fall only on segment boundaries (`-`, `_`, `.`, `/`). Several candidates make it
  `ambiguous`. A pattern never links to a pattern.
- Both limits are named ceilings: the length of the literal part (3 characters) and the segment
  boundaries. Each is listed with the known ceilings, carries the marker and is fed an input past
  it by a unit test.

### Workloads and the image kind

- A **workload** is an IaC node the type table marks as running code: Kubernetes pod template
  owners (Deployment, StatefulSet, DaemonSet, Job, CronJob, Pod), Compose services, ECS task
  definitions, Cloud Run services and jobs, Lambda functions and Azure Functions.
- A new contract kind **image**, `image::<registry>/<repository>`. The name is normalized: a
  Docker Hub short name is expanded (`nginx` is `docker.io/library/nginx`, `acme/api` is
  `docker.io/acme/api`), the tag and the digest are dropped, and the whole is lowercased.
- Its **builder** side, the provider, is the repository whose CI or build configuration
  publishes the image: docker/build-push-action `tags`, docker/metadata-action `images`, Compose
  `build` with `image`, Skaffold artifacts, ko and Jib. Its site is the repository, located by
  `<file>#<key path>`.
- Its **runner** side, the consumer, is the workload whose image field names it, including Helm
  values `image.repository` and Kustomize `images`. Levels follow the rule for evaluated names.
- **Same repository.** A workload defined next to its code (SAM `CodeUri`, a Serverless handler,
  a Lambda source directory or archive, a Compose build context) runs that code directly, at
  `exact`, with no image contract between them.

### Environment entries

- Each entry a workload sets (a literal, a reference to an IaC node's attribute, a ConfigMap or
  Secret key reference, `envFrom`) is stored with its name and its target.
- An environment read in code (the name argument of an SDK call, an HTTP base URL) resolves by
  name **only within a workload's scope**: against the entries of the workloads that run that
  code, through the image contract or the same-repository code path. The side then joins the
  contract of the resource the entry points to, at `inferred`, its reason naming the workload and
  the variable.
- An entry that points at a redacted value or at something unresolved leaves the side
  unresolved. Values come only from the repositories' IaC and config, never from a process
  environment.
- Environment entries do not enter `meada export`; the sides they resolve do.

### HTTP through IaC

- **Service names.** A consumer's base URL host that is a Kubernetes Service name in any of its
  DNS forms (`<service>`, `<service>.<namespace>`, `<service>.<namespace>.svc`,
  `<service>.<namespace>.svc.cluster.local`, with or without a port), or a Compose service name,
  resolves to the workloads the Service selects (or the Compose service) and then to the
  repositories they run. The consumer's calls link to those repositories' HTTP providers at
  `inferred`, as a resolved base URL does today, the reason naming the Service.
- **Ingress.** An Ingress host and path prefix carry an external base URL to the Service of the
  matching rule, the prefix applied like a gateway prefix. An Ingress that rewrites the path (a
  rewrite annotation) links nothing, as an unread mount yields nothing.
- **IaC-declared routes.** API Gateway routes and integrations, SAM `Api` and `HttpApi` events,
  and Serverless `http` and `httpApi` events are HTTP providers served by the function's code:
  its handler symbol when the handler resolves to one in the same repository, otherwise the
  repository as a whole. Literal paths and methods are `exact`.
- Service meshes, Ingress rewrites and API gateways other than those named are out.

### The alert kind

- A new contract kind **alert**, `alert::<key>::<value>`.
- Its **emitter** sides, providers: the labels of Prometheus and Loki rule groups (also inside
  `PrometheusRule` objects), Grafana alert rule labels and Datadog monitor tags (`key:value`).
- Its **matcher** sides, consumers: OpsGenie routing rule conditions, PagerDuty event
  orchestration conditions and Alertmanager route matchers (in an Alertmanager configuration or
  an `AlertmanagerConfig` object).
- An equality condition links `exact`; a regular expression, prefix or containment condition
  links `inferred`. A label reaches a matcher that tests tags only through a forwarding Meada
  knows: Datadog monitor tags reaching OpsGenie as tags, and the tags an Alertmanager OpsGenie
  receiver sets from labels. With no known forwarding there is no link. Alertmanager matchers
  test labels directly.
- Helm values that templates render into rules are not emitters, since templates are not
  rendered; a `[[resource]]` `via` to the key covers such shapes.

### Queries

- `context` on an IaC node shows its physical name, its references both ways, its config values
  and its contract sides; on a workload it adds the image, the repository that builds it and the
  environment entries with their targets.
- `trace` walks references, provisioning links, subscriptions and image links: down from a block
  to the code it reaches, and up from a call to the IaC that creates its resource.
- **`via`.** Besides a symbol, a `[[resource]]` `via` may name an IaC address or
  `<file>#<key path>`, with `[key=value]` selectors in list steps. A `via` that finds no node or
  key is reported stale with today's status lines.

### Orphans

- `meada status` lists, per contract kind and broker, provisioned resources with no use and uses
  with no provisioner (and, for the image and alert kinds, a side with no counterpart), but only
  when the scope (a repository, or the workspace) holds at least one provisioner and one use of
  that kind. A repository with no IaC reports no orphan.
- Otherwise `context` and `search` mark the side itself ("no provisioner in the workspace"),
  with no status line.
- `meada status` also counts the config and IaC files read and skipped, the values redacted
  (ADR 0025) and the IaC nodes.

### Export

These enter `meada export`, each as an optional field that is left out when empty, as
`bindings` is:

- `iac_nodes`: the IaC nodes that hold a contract side, each with its identity, address, type, root
  label and span. IaC nodes with no contract stay out.
- `provisioners`: the provisioner sides, each a contract and the node that provisions it.
- `uses`: the use sides read from config values, each a contract, the repository as its site
  and `at`, the `<file>#<key path>` it was read from. The config values themselves stay out.
- `provisions`: the provisioning links, each the other side, the provisioner, the contract, the
  level and the reason.
- `subscriptions`: the subscriptions read from IaC, each a source and a target contract and the
  node that declares it. The links they make go into `links`, as a binding's do.
- The image and alert kinds' builders, runners, emitters and matchers, and the HTTP providers
  of IaC-declared routes, go into `providers`, `consumers` and `links` as every kind does. A side
  read from a config file (a builder) has the repository as its site, the repository-level form,
  and an optional `at` field with its `<file>#<key path>`.

**The export format stays 1.** Under its rule (ADR 0004, amended 2026-10-30) the integer rises
only when a reader of the previous shape would misread the new one. Nothing here renames or
removes a field or changes a meaning: the new arrays are optional and a reader of format 1
ignores them (`meada_core::export` does not deny unknown fields); a new contract kind is a new
value of the open `contract` identity, its kind its prefix; a site that names an IaC node has
the `<repo remote>::<file>::<address>` form (ADR 0025), the shape a documented-only side's site
(`<repo remote>::<document>::<label>`) already has; a side read from config has the
repository-level site a declared resource side already has, and `at` is an added optional
field. Provisioning links have an array of their own precisely so that `links` keeps its
meaning, a consumer linked to a provider. The addition ships in a minor release, and the exports
of repositories with no config and no IaC stay byte-identical.

### Server mode and named workspaces

The new rows follow the existing grants, generations and partial export; nothing in them
bypasses the workspace filter.

### Amendment to ADR 0005

This ADR amends ADR 0005, which carries the dated amendment (2026-11-04): its contract kinds,
its rule on environment variables and its `inferred` level change as the sections above say.

## Considered options

- **Provisioning links in `links`, as consumer to provider.** Rejected: a provisioner neither
  serves nor calls the contract, and a format 1 reader would read the row as a consumer link;
  keeping them apart also keeps the format at 1.
- **Provisioners as `ambiguous` candidates of one another.** Rejected: one name provisioned per
  environment, overlay or region is the normal shape of IaC, not a doubt.
- **Matching environment variables by name across the workspace.** Rejected as in ADR 0005: the
  same name means different things in different repositories. A workload's scope is the context
  that makes a name mean one thing.
- **A config value equal to a name as a use side.** Rejected: equal strings are common and a
  bare match is a guess; a structured identifier or a typed key is evidence.
- **Modelling accounts, regions and clusters.** Rejected for now: one name of one kind is one
  contract, as channels are, and the provisioner's root label tells environments apart.
- **A `router` side for alerts.** Rejected: router is HTTP vocabulary; emitter and matcher name
  what each side does.

### AWS SDK forms amendment (2026-11-06, #659)

The operation matrix in the user guide specifies the low-level AWS SDK v3 and boto3 forms.
Both the client and command origins must be the corresponding SDK's; an unsent JavaScript
command has no direction. Batch and transaction entries retain each physical name. An omitted
EventBridge `EventBusName` means `default`; SNS phone and endpoint publishes name no topic.

The existing `contracts.via` stores `sdk:aws` for these directed sides and `env:<variable>`
for an unresolved environment name. Providers gain optional `via` in export format 1, absent
on existing rows; consumers already have it. Literal SDK writer-reader and provisioning links
are `exact`, with evaluated provisioner names weakening them as above. SDK resource readers
can reach SDK writers in their own repository; declared resource sides preserve their
cross-repository rule and `declared` level. The environment remains unread until the scoped
workload mechanism can resolve it; this ticket never reads the process environment.

An SSM hierarchy path is not a physical parameter name, so `GetParametersByPath` remains an
unresolved reader. A DynamoDB transaction's `ConditionCheck` is a reader of its table, even
when the same transaction writes another table. Computed batch keys follow the same literal
and environment rules as other name arguments.

### Image execution representation (#662, 2026-11-06)

The image addition keeps export format 1. Builder locations use the optional provider `at`
field described above. Workload consumers retain their IaC identity, and their nodes enter
`iac_nodes`. A direct same-repository code path enters an optional `executions` array with
`workload`, its `root`, `repository`, repository-relative `path`, `level` and `reason`. These
rows are not consumer/provider image links. Image links derive the same execution relation
at query time for context and trace. Readers that do not know the array ignore it.

Several distinct normalized image names matching a pattern are distinct candidates even when
one repository builds them. Several tags or configuration locations for one repository and
normalized image are evidence of the same candidate. The kind retains the strongest evidence.

This slice recognizes literal ko destinations in checked-in CI `KO_DOCKER_REPO` with
`ko build --bare` or `ko publish --bare`. Other ko naming modes add import-path names or hashes;
unread commands yield no guessed builder. Base-image settings never publish an image.
The merged release with #660 synchronizes both crates at 1.4.0; image contracts share that
minor release under Unreleased. ko scripts are read only as single simple commands;
multiline scripts and unsupported shell syntax yield no guessed builder. A supported command
has exactly one `--bare`, at most one literal package target and no other flags; a specified
shell must be `bash` or `sh`. Help, unknown options and other shells stay absent.

Image consumer and link rows optionally carry their evaluation `root`. Their `site` and
`consumer` still identify the workload. An open item's decision identity additionally names
the image contract and root, so two container images or overlays do not share a decision.
Replaying one decision preserves every other image side's ambiguity.

### SDK and image integration (#659 and #662, 2026-11-06)

Combining the SDK and image slices keeps the SDK slice's synchronized 1.4.0 minor release
for both crates, and export format 1. The earlier image tier's 1.3.0 release note describes
its starting tier. Optional provider `via` records SDK provenance while optional `at` records
image build configuration locations; neither replaces the other.

### Declared key locations (#641, 2026-11-06)

An explicit `[[resource]]` key `via` attaches a directed side to the key itself. Its internal
and exported site is `<repository>::<file>#<canonical key path>`, preserving distinct declared
keys in ordinary provider/consumer links. This is separate from the automatically recognized,
directionless config-use rows whose repository site and optional `at` remain as described
above. The string identity is additive within export format 1; no config scalar text is
exported. A key declaration may resolve through list selectors, but its site uses the resolved
indexes so context and trace identify the actual key. Secret/cut values and HCL expressions/templates cannot establish selector equality.
Selectors require a complete literal string, number or boolean. Only indexed values and their parent key paths
resolve; null values and empty containers have no row and remain stale (ADR 0025).
An IaC declaration names a source address, optionally file-qualified; the
` (root ...)` qualifier accepted by IaC queries is not a declaration form. A bare address
matching several source identities is stale until qualified with its file.


### Anchored provisioning choices (#674)

An ambiguous physical name is an open item at the side holding the pattern. For a provisioner,
the decision call identity is `provision::<IaC identity>::root=<evaluation root>`; its candidate
target remains `<repository>::<literal contract>`, using the existing `ambiguous-link` decision
kind. For a patterned use the call identity is `provision-use::<site>::<pattern contract>`
and the target is the literal provisioner contract in its repository. The chosen name retains
all its counterpart sites, while independent roots remain independent.
The result is a provisioning link at `decided`, with the decision's author in its reason.
A removed source, closed ambiguity, removed target or changed candidate is stale; a target in
a missing named peer waits. The export keeps its existing provisioning shape and format 1.

For a use declared at a configuration key, the item keeps the indexed path, key and line.
Its expression contains contract and location metadata, never the key's scalar value or raw
source line, preserving ADR 0025 redaction and truncation when shown to a person or decider.

An image builder pattern with several distinct literal runner names is likewise an open item
in its builder repository, identified as `image-builder::<repository>::<pattern contract>`.
Its candidates name runner repositories and literal image contracts. Several runners of one
literal image are one name, and choosing that name retains them all. This is independent of a
runner's ambiguity among several builders, whose existing decision identity is unchanged.
