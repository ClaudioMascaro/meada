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
| `azurestorage` | an Azure Storage account | the account name, distinct from any container |
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
  a Lambda source directory or archive, a Compose build) runs that code directly, at `exact`,
  with no image contract between them. A Compose build runs the paths its Dockerfile takes from
  the context, not the whole context (see "Compose build contexts read through the Dockerfile").

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
- **No code reached.** A Service that selects no indexed workload, or an Ingress rule left
  unread, links nothing itself and leaves the host to `[serve] hosts`: a host a repository
  declares there still links `exact`, as it did before Meada read infrastructure.
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
  links `inferred`. A negated condition only filters its route: it is no matcher side, since
  the value it names is one the route refuses. A label reaches a matcher that tests tags only
  through a forwarding Meada knows: Datadog monitor tags reaching OpsGenie as tags, and the
  tags an Alertmanager OpsGenie receiver sets from labels. With no known forwarding there is
  no link. Alertmanager matchers test labels directly.
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

### Curated abstraction projection (#671)

The embedded type table maps a bounded documented custom-resource inventory and registry
module source/input rows onto existing kinds. Private mappings select these same entries.
Physical names remain on IaC nodes; canonical contract targets and explicit unknown targets
are separate stored projections. Creation controls that are false or unknown establish no side.
No operator, module, cloud API, packager or archive is executed while indexing.

RabbitMQ Queue provisions AMQP; Exchange supplies routing metadata and is no channel; Binding
routes only through real, uniquely resolved endpoints of a supported type. Routed publish uses
retain the actual code site, exact/inferred route level and binding reason. A neutral batch hook
on ContractKind exposes these additional uses to provisioning, with an empty default for other
kinds. The engine does not learn broker policies and raw producers stay unchanged.

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

The Java 2.x, Go v2 and .NET SQS forms existed before constructor evidence, so a client the
source declares with the SDK's SQS client type keeps them: a field, parameter or typed local,
injected ones included. A repository type of that name hides the SDK's. Other services need
construction.

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


### AWS workload execution (#664)

Lambda image_uri, SAM ImageUri and each ECS container image are image runners, independent
of whether the container declares environment entries. The format readers keep evaluated
container images and provenance in the same stored node facts; image contracts project those
facts alongside Compose and Kubernetes through one runner inventory.

Local Lambda filename inputs, SAM CodeUri and an unambiguous Serverless handler source can
create direct same-repository executions. An exact archive_file output_path traversal may
follow only an explicit source_dir/source_file input, with no general data-source evaluation
and no extraction. New paths are relative, contain no raw parent component and traverse no
symlink; indexing alone checks existence, with path-sensitive configuration invalidation.

SAM Globals and Serverless provider environment entries are overridden by the function, even
when its value is unreadable. Environments retain their workload, container image and root.
A known IaC resource reference can carry an optional typed contract identity without a fake
URL/ARN; it resolves only a matching unresolved contract kind through the same scope index
as literals. Secret contents, dynamic references and external values remain unavailable.
Environment-derived names stay inferred with workload and variable provenance.

### Azure identifiers, subscriptions and Functions (#669)

The AzureRM Terraform/OpenTofu entries are Service Bus queues, topics and subscriptions,
Event Hubs, Storage accounts and containers, Key Vault secrets, and Linux/Windows Function
apps. Accounts use `resource::azurestorage::<account>`; containers retain
`resource::azureblob::<container>`. A subscription target is
`channel::servicebus::subscription:<topic>/<subscription>`. The colon marker is impossible
in an Azure queue/topic name, whereas slash is legal. Thus neither a same-name topic nor
the queue `orders/subscriptions/audit` impersonates a subscription. Physical node names
remain available beside their separately stored projected target and evaluation evidence;
an explicitly unresolved projection forbids falling back to an incomplete physical name.

Complete ARM IDs validate subscription/resource-group/provider/type segments before
discarding deployment selectors. Provider and type labels are case-insensitive; physical
names retain their case. Blob URLs identify their container, ignoring the object suffix;
versioned vault URLs identify their secret. Malformed/wrong-service identifiers and
namespace-only Service Bus URLs identify nothing. Provider-specific final typed keys are
the bounded inventory `servicebus_queue`, `servicebus_topic`, `servicebus_subscription`
(topic-qualified value), `eventhubs_hub`, `azurestorage_account`, `azureblob_container`
and `keyvault_secret`, always inferred. The generic AWS key inventory remains unchanged.
The kind owns this normalization in `contract/azure.rs`, shared with future SDK readers.

A Service Bus subscription joins its topic like a binding. An observed subscription rule,
including an unread filter, weakens that route to inferred and names the rule. An absence
of declared rules gives exact routing, subject to name evaluation evidence. This models
repository configuration; it makes no claim that a deployment removed the broker's default
rule. Existing C# `CreateProcessor` forms retain their established topic-based identity and
origins, independently of these subscription-aware structured identifiers. AsyncAPI's
`servicebus` and `eventhubs` protocol labels provide the same channel identities; subscription
addresses use their full ARM ID.

Linux Functions read the single Docker application-stack block's registry/name/tag and
app settings. Linux and Windows Functions also support local `zip_deploy_file`, validated
through the image kind's indexing hook with the same path/symlink rules as other workloads.
Archives are execution locations, never opened and never widened to their parent directory.
Remote package settings identify no local code. Literal/evaluated app settings and known
resource attribute targets use the common workload-scoped environment mechanism. Key Vault
references and secret-content attributes retain no resolved value. This addition shares
the synchronized 1.4.0 minor release and export format 1.

### GCP SDK forms amendment (2026-11-06, #668)

The adopted operation inventory counts JavaScript and TypeScript as one of five language
groups, with independent controls for both source languages. A known constructor must establish
the client through complete exact calls. A declared type, a name-matched factory, a hidden
constructor, a reassigned client or an opaque return alternative supplies no client evidence.
Making a topic, subscription, bucket or object handle alone supplies no direction.

| Group | Pub/Sub producer / consumer | Cloud Storage writer / reader | Secret Manager reader |
| --- | --- | --- | --- |
| JavaScript + TypeScript | `@google-cloud/pubsub`: `new PubSub().topic(name).publishMessage(...)` / `.subscription(name).on("message", handler)` | `@google-cloud/storage`: `new Storage().bucket(name).file(object).save(...)` / `.download(...)` | `@google-cloud/secret-manager`: `new SecretManagerServiceClient().accessSecretVersion({name})` |
| Python | `google.cloud.pubsub_v1.PublisherClient().publish(topic, ...)` / `SubscriberClient().subscribe(subscription, callback=...)` | `google.cloud.storage.Client().bucket(name).blob(object).upload_from_string(...)` / `.download_as_bytes()` | `google.cloud.secretmanager.SecretManagerServiceClient().access_secret_version(request={"name": name})` |
| Java | `com.google.cloud.pubsub.v1.Publisher.newBuilder(topic).build().publish(...)` / `Subscriber.newBuilder(subscription, handler).build().startAsync()` | `com.google.cloud.storage.StorageOptions.newBuilder().build().getService().create(BlobInfo.newBuilder(BlobId.of(bucket, object)).build(), ...)` / `.readAllBytes(bucket, object)` | `com.google.cloud.secretmanager.v1.SecretManagerServiceClient.create().accessSecretVersion(name)` |
| Go | `cloud.google.com/go/pubsub/v2.NewClient`: `Publisher(topic).Publish(...)` / `Subscriber(subscription).Receive(ctx, handler)`; v1 `cloud.google.com/go/pubsub` uses `Topic` / `Subscription` | `cloud.google.com/go/storage.NewClient`: `Bucket(bucket).Object(object).NewWriter(ctx)` / `.NewReader(ctx)` | `cloud.google.com/go/secretmanager/apiv1.NewClient`: `AccessSecretVersion(ctx, &secretmanagerpb.AccessSecretVersionRequest{Name: name})` |
| C# | `client = await Google.Cloud.PubSub.V1.PublisherClient.CreateAsync(topic); client.PublishAsync(...)` / `client = await SubscriberClient.CreateAsync(subscription); client.StartAsync(handler)` | `Google.Cloud.Storage.V1.StorageClient.Create()`: `UploadObject(bucket, ...)`, `UploadObjectAsync` / `DownloadObject(bucket, ...)`, `DownloadObjectAsync` | `Google.Cloud.SecretManager.V1.SecretManagerServiceClient.Create()`: `AccessSecretVersion(name)`, `AccessSecretVersionAsync(name)` |

Names retained in receivers are evaluated in their constructor/factory call's scope and exact
return frames. Two calls of a factory retain their own names. A known bucket is not erased by
an unknown object. Verified Java `TopicName.of`, `SubscriptionName.of`, `SecretVersionName.of`,
C# typed-name constructors and `FromProjectTopic`/`FromProjectSubscription`, and Python
`topic_path`/`subscription_path` select the identity component; an identically named local helper
does not. Helper subtype survives factory memoization: a subscription helper cannot name a
topic, bucket or secret. Complete structured identities use the shared GCP physical-name policy above.
Selected request fields and factory identity arguments reject prior writes through exact aliases,
including mapping update/deletion and exact helper parameters. Later writes, sibling objects
and unrelated fields preserve valid names. Unknown or exhausted mutation evidence cannot
prove an unchanged initializer. Async factory results retain await state; a bare C# Task and
a twice-awaited C# client supply no receiver evidence. Resource reads retain each operation's
call site, including identical calls within one function.

Pub/Sub receive targets remain `subscriptions/<id>`, including a plain id supplied by a scoped
workload environment. Receives never derive a topic from a subscription id. Literal SDK
identities are exact; provisioning and subscription delivery retain their existing scope and
filter evidence. Workload resolution remains `inferred` with the workload/variable reason.
Without a matching workload the side remains unresolved with its established `env:NAME` metadata.
The stored operation retains subtype evidence for this normalization; private intermediary
provenance is never exported. The operation list is deliberately bounded: other overloads,
streaming/polling families, delete/list/metadata operations and other SDKs are not promised.

### Azure SDK forms amendment (2026-11-06, #670)

The adopted five-group inventory below closes the Azure operation list left implicit in the
original ADR. JavaScript and TypeScript share a reader and have independent source controls.
The new forms require verified SDK construction through complete exact calls. Injected types,
hidden constructors, reassigned clients, name-matched hops and opaque return alternatives
supply no new client evidence. Existing C# Service Bus sends, schedules and processors keep
both their accepted origins and their topic identity. A handle alone supplies no direction.

| Group | Service Bus producer / consumer | Event Hubs producer / consumer | Blob writer / reader | Key Vault reader |
| --- | --- | --- | --- | --- |
| JavaScript + TypeScript | `createSender(name).sendMessages` / `createReceiver(queue)` or `(topic, subscription)`, then `receiveMessages` or `subscribe` | `EventHubProducerClient(..., hub).sendBatch` / `EventHubConsumerClient(group, ..., hub).subscribe` | service → `getContainerClient(container)` → `getBlockBlobClient(object)`: `upload`, `uploadData`, `uploadFile`, `uploadStream`; blob `download`, `downloadToBuffer`, `downloadToFile` | `SecretClient.getSecret(name)` |
| Python | `get_queue_sender` / `get_topic_sender` → `send_messages`; `get_queue_receiver` / `get_subscription_receiver` → `receive_messages` | explicit `eventhub_name`: producer `send_batch` / consumer `receive` | service `get_blob_client(container, blob)` or `get_container_client(container).get_blob_client(blob)`: `upload_blob` / `download_blob` | `SecretClient.get_secret(name)` |
| Java | `ServiceBusClientBuilder.sender().queueName` or `topicName` → `buildClient`: `sendMessage`, `sendMessages`; receiver `queueName` or `topicName` + `subscriptionName` → `buildClient().receiveMessages` | builder `connectionString(cs, hub)` or `credential(namespace, hub, credential)`: `buildProducerClient().send` / `consumerGroup(...).buildConsumerClient().receiveFromPartition` | `BlobServiceClientBuilder...buildClient().getBlobContainerClient(container).getBlobClient(object)`: `upload`, `uploadFromFile` / `downloadContent`, `downloadStream`, `downloadToFile` | `SecretClientBuilder...buildClient().getSecret(name)` |
| Go | `NewSender` → `SendMessage`, `SendMessageBatch`; `NewReceiverForQueue` / `NewReceiverForSubscription` → `ReceiveMessages` | `NewProducerClient` → `SendEventDataBatch`; `NewConsumerClient` → `NewPartitionClient` → `ReceiveEvents` | `UploadBuffer`, `UploadFile`, `UploadStream` / `DownloadBuffer`, `DownloadFile`, `DownloadStream`, container after `ctx` | `GetSecret(ctx, name, version, ...)` |
| C# | existing sends, schedules and processors retained; new `CreateReceiver(queue)` / `(topic, subscription)` → `ReceiveMessageAsync`, `ReceiveMessagesAsync` | explicit hub: producer `SendAsync` / consumer `ReadEventsAsync` | service → `GetBlobContainerClient(container)` → `GetBlobClient(object)`: `Upload`, `UploadAsync` / `DownloadTo`, `DownloadToAsync`; container `UploadBlob`, `UploadBlobAsync` | `SecretClient.GetSecret`, `GetSecretAsync` |

The constructors are JS/TS's imported `@azure/service-bus`, `@azure/event-hubs`,
`@azure/storage-blob` and `@azure/keyvault-secrets` client classes; Python's corresponding
`azure.servicebus`, `azure.eventhub`, `azure.storage.blob`, `azure.keyvault.secrets` classes,
including verified `aio` equivalents; Java's `com.azure` builders; Go's `sdk/messaging/azservicebus`,
`sdk/messaging/azeventhubs`, `sdk/storage/azblob`, `sdk/security/keyvault/azsecrets` constructors;
and .NET's `Azure.Messaging.ServiceBus`, `Azure.Messaging.EventHubs.Producer` / `.Consumer`,
`Azure.Storage.Blobs`, `Azure.Security.KeyVault.Secrets`. JS Blob's `fromConnectionString`,
Python's first three services' `from_connection_string`, Go messaging's `FromConnectionString`
and Blob's `NewClientWithNoCredential`, `NewClientWithSharedKeyCredential`,
`NewClientFromConnectionString` are included. Known Java builder setters are enumerated;
arbitrary fluent calls do not preserve a client.

Primary inventories: [Service Bus JS](https://learn.microsoft.com/en-us/javascript/api/overview/azure/service-bus-readme?view=azure-node-latest),
[Event Hubs Python](https://learn.microsoft.com/en-us/python/api/overview/azure/eventhub-readme?view=azure-python),
[Blob Java](https://learn.microsoft.com/en-us/java/api/overview/azure/storage-blob-readme?view=azure-java-stable),
[Go messaging](https://pkg.go.dev/github.com/Azure/azure-sdk-for-go/sdk/messaging/azeventhubs),
[Go storage](https://pkg.go.dev/github.com/Azure/azure-sdk-for-go/sdk/storage/azblob) and
[Key Vault .NET](https://learn.microsoft.com/en-us/dotnet/api/overview/azure/security.keyvault.secrets-readme?view=azure-dotnet).

Only explicit entity arguments identify resources: queue/topic, hub, blob container and secret.
Group, partition, object and version arguments are irrelevant to identity. Pure Azure helpers
normalize supported complete ARM identifiers and blob/vault URLs, rejecting malformed and
wrong-service shapes. Connection strings and credentials are not parsed for names. An Event
Hubs constructor without a separately written hub remains unresolved. Direct Blob/Container URL
constructors, session processors, other polling/streaming families, list/delete/metadata methods
and other async families are outside this inventory.

The shared SDK walker carries exact return frames and complete receiver evidence; kind policy
retains folded name arguments. A new subscription receiver uses
`subscription:<topic>/<subscription>`, avoiding legal slash queue names and same-name topics.
Literal names yield exact SDK links; provisioning and filtered delivery retain their existing
levels and reasons. Workload resolution is inferred, with stored workload and variable evidence.
Both topic and subscription must come from the same workload, root and selected container;
unknown, unread, incompatible typed or cross-frame companions cannot be combined.

`Found.method` and `via` remain actual operation/provenance. These fields cannot retain both
arguments and subtype for `createReceiver` / `CreateReceiver`, which name queues as well as
subscriptions. A neutral optional `contracts.identity` JSON therefore stores only a kind-owned
subtype plus normalized literal/environment/unresolved name parts. It contains no constructor,
request, namespace, connection string or credential object and never enters public export/MCP.
Absence preserves legacy behavior. Internal Provider/Consumer summaries carry the optional
value; the contract row key, internal sorting and declaration deduplication include it so unresolved sibling pairs cannot collapse. The
existing schema-golden hash deterministically rebuilds source indexes from an older layout;
read-only servers consume the rebuilt stored summaries without rereading source or environment.
No cloud parsing enters the generic engine, index, environment collector or language packs.

Queue receiver overloads distinguish options from subscription names by proven argument form,
not positional count: JS/TS object literals or exact folded objects, and C# verified
`ServiceBusReceiverOptions` constructions. Explicit unshadowed JS/TS `undefined` and named C#
`options: null` select the queue form. Bare C# positional null remains ambiguous. An opaque or
reassigned argument supplies no exact entity; strings retain the topic/subscription form.
The C# options constructor category is retained in private memo keys, preventing a valid first
factory call from making a later opaque actual look like options. No new walker is involved.

### Compose build contexts read through the Dockerfile (#744, 2026-11-10)

A build context says what the builder may read, not what the image holds. A Compose service
built from the repository root ran every symbol of the repository at `exact`, including a service
whose Dockerfile copies only `frontend/`, and its environment entries scoped every environment
read in the repository. A Compose build now runs the paths its Dockerfile takes from the context:

- **The Dockerfile** is the one Compose builds with: `build.dockerfile`, relative to the context,
  or `build.dockerfile_inline`, else `Dockerfile` in the context. An override merges `dockerfile`
  and `dockerfile_inline` like the other `build` keys, and a Compose node keeps the reference next
  to its build context.
- **Paths.** Every `COPY` and `ADD` source read from the context, in every stage (a build stage
  compiles the code a later stage runs), and every `RUN --mount=type=bind` source with no `from`
  (with no `source`, the whole context). `COPY --from`, `ADD` of a URL or git repository and a
  heredoc read nothing from the context. A wildcard source is matched against the repository's
  files with Docker's rules (`*` does not cross `/`), one path per match, so one workload and
  root may have several executions.
- **Levels.** Each path runs at `exact`, or `inferred` when the context or the Dockerfile path
  went through an overridable value. `COPY . .` keeps the whole context at `exact`.
- **Fallback.** A Dockerfile Meada cannot read (missing, outside the repository or behind a
  symlink, a source with a variable, a form the reader does not know) leaves the whole context
  at `inferred`, and the reason says the Dockerfile was not read. A Dockerfile that reads nothing
  from the context runs no code directly, and the service's image side links as any runner's does.
- **`.dockerignore`** is not read: a path may cover files the image excludes, never fewer.
- **Freshness.** The Dockerfile's bytes, and the listing of each directory a wildcard source
  opens, are inputs of the Compose file's stored summary, as its build paths are (#711): editing,
  adding or removing the Dockerfile makes the index stale.
- **Trace.** Upward, each workload and root that runs code is written once per answer, under the
  first node it runs.

Lowering a root context to `inferred`, or dropping it, was rejected: the most common Compose
layout, one service with `build: .` and `COPY . .`, does hold the whole repository, and a
whole-repository `inferred` execution still widens environment-entry scopes. Reading the
Dockerfile of a CI image build is a follow-up; the image contract's builder side stays a
repository.
