# User guide

> This is a translation. The Brazilian Portuguese [guide](user-guide.md) is the source; if they
> disagree, it wins, and a fix to this page is welcome.

This guide takes an English-speaking person from zero to a working Meada: install it, connect an
agent, configure a workspace, decide what the code leaves unclear, use the command line and read
the export. The examples use the repositories `web`, `api` and `worker` and the host
`api.example.com`.

The second half (sections 8 to 11) explains what Meada links and where it stops: each contract
kind, each language, shape drift, communities and processes. Use it to tell whether a missing link
is a known limit or a bug.

Contents:

1. [Installation](#1-installation)
2. [Connect an agent (MCP)](#2-connect-an-agent-mcp)
3. [The workspace and first use](#3-the-workspace-and-first-use)
4. [`meada.toml`: reference](#4-meadatoml-reference)
5. [Decisions and deciders](#5-decisions-and-deciders)
6. [CLI commands](#6-cli-commands)
7. [The export](#7-the-export)
8. [Contract kinds](#8-contract-kinds)
9. [Languages](#9-languages)
10. [Shape drift](#10-shape-drift)
11. [Communities and processes](#11-communities-and-processes)

## 1. Installation

There are five channels, and all of them deliver the same `meada` binary. Pick the one that fits
your environment.

### Cargo

```sh
cargo install meada
```

It builds from crates.io and needs the Rust toolchain.

### Prebuilt binary

Download the archive for your system from the
[GitHub releases](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS and Windows, on
x86_64 and arm64), extract it and put `meada` on your `PATH`.

### Shell installer

For Linux and macOS:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh
```

### npm (`npx @meada/meada`)

```sh
npx @meada/meada --version
npx @meada/meada index
```

The `meada` npm package holds only the installer: it downloads the binary for your platform from
the GitHub release and runs it with the arguments you passed. The package's version is always the
release's. The install downloads the binary on the spot, so it needs a network (there is no
offline install through npm). It is the channel that matters most to whoever configures an agent,
because `npx -y @meada/meada mcp` is the launch line of the MCP server (section 2).

### Container

```sh
docker run --rm ghcr.io/claudiomascaro/meada:latest --version
```

`ghcr.io/claudiomascaro/meada:<version>` is the default build and `<version>-cloud` carries every
cloud module, both for Linux amd64 and arm64. The image runs without root and defines no command:
pass the arguments explicitly. To serve an organisation, see
[Server mode](https://claudiomascaro.github.io/meada/en/server/).

### What Meada needs

Only the binary and `git`. Indexing reads the repository's files without running the code or the
language's toolchain, uses no LLM and opens no network. Check the install with `meada --version`.

The management console (an optional module, `--features ui`) and the other cloud modules have
their own installation, described in
[Installation](https://claudiomascaro.github.io/meada/en/installation/).

## 2. Connect an agent (MCP)

`meada mcp` speaks MCP over stdio. The client (the agent) runs the command and talks to it. The
documented launch line is `npx -y @meada/meada mcp`, which needs no separate install.

### Client configuration

In Claude Code's `.mcp.json`, or your client's equivalent file:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["-y", "@meada/meada", "mcp"] }
  }
}
```

In Claude Code, the same in one command: `claude mcp add meada -- npx -y @meada/meada mcp`.

With `meada` already on your `PATH` (through Cargo, the binary or the installer), swap the command
for `"command": "meada", "args": ["mcp"]`, which avoids the npm lookup on every start.

### Which repository the server answers about

The server answers about the first root (`roots`) the client sends. Claude Code sends the
session's folder, so opening a session in `web` makes Meada answer about `web`, even if the server
was started somewhere else. Without roots, it answers about the folder it was started in, which
must be inside the repository. If the client does not answer `roots/list` within 5 seconds, the
call uses the last root sent on that connection or returns an error that says so; it never falls
back to the server's folder for that reason.

Every query first refreshes the index with what changed in the files, including what is not yet
committed. There is no reindex step for the agent.

### The three tools

There are three, with names and descriptions in English, and none of them writes. Answers are
compact lines inside a token budget (`budget`, 2,000 by default, adjustable on each call; a token
is four bytes of output). When an answer goes over the budget, it ends with
`… cut after N of M lines (budget B tokens); raise budget for more`.

#### `search`: find the starting point

| Parameter | Type | Default | Role |
| --- | --- | --- | --- |
| `query` | text, required | | The name to look for, case-insensitive |
| `kind` | text | all kinds | Restricts the kind of result |
| `budget` | integer | 2000 | Token budget of the answer |

The values of `kind`:

| `kind` | Looks for |
| --- | --- |
| `symbol` | a symbol by name |
| `contract` | a contract by identifier (`http::GET::/orders`) |
| `route` | an HTTP contract |
| `resource` | a declared resource, by kind and id |
| `channel` | a queue or a topic, by broker and name |
| `document` | an OpenAPI or AsyncAPI file, by path or title |
| `package` | a package by name, with its consumers |
| `drift` | fields that consumers read and a route does not return, by route or field path |
| `community` | a community by name |
| `process` | a process by the label of its entry point, with the `process::` identity `trace` starts from |
| `duplicate` | symbols with identical text in two or more places, by name or path |
| `value` | a config value of a YAML or JSON file, by value or key path |
| `iac` | an IaC node (a Terraform resource, data block or module call, or a Kubernetes object), by address, type or physical name |
| `ci` | a GitHub Actions pipeline, action, CI job or CI step, by address, type, id, `name` or `uses` |
| `ambiguous`, `unresolved` | the open items, with the `meada decide` command for each |

Each result is one line: repository, `path:line` and name.

With no `kind`, search answers with symbols and contracts; only when neither matches does it
answer with config values. With `kind = "value"`, each line is `<repository> <path>:<line> <key
path> = <value>`, a string quoted as in JSON:

```text
example.com/acme/app chart/values-prd.yaml:6 alerts.loki[0].labels.support = "team-b"
example.com/acme/app deploy/app.yaml:26 [1].data.password = redacted
```

The key path is jq style: keys joined by dots, a list item `[n]` from 0, a key that is not a
plain identifier in `["..."]` (`metadata.labels["app.kubernetes.io/name"]`) and, in a file of
several documents, the document's index first (`[2].spec.replicas`). The query, lowercased,
matches the value or the key path: equal ones first, then those that start with it, then those
that hold it. A value over 1 KiB is stored cut and shows followed by `(cut)`.

Equal values and those that start with the query come from B-trees on the lowercased key path and
value; those that hold it come from an FTS5 table with the trigram tokenizer, when the query has
three characters or more (a shorter query scans the rows, with the same answer). That index costs
space: it holds about four times the text of the key paths and values (in a repository of 92
thousand values and 5 MB of text, 21 MB of `.meada/index.db`), on top of what the index already
held.

With `kind = "iac"`, each line is an IaC node: `<repository> <path>:<line> <address> (<type>,
name "<physical name>", root <root module>)`, without the `name` when the block has no `name`
attribute, and with `pattern` in its place when a part of the name is unresolved (`pattern
"orders-*"`). Search with no `kind` does not answer with IaC nodes.

```text
example.com/acme/infra envs/prod/main.tf:35 aws_sqs_queue.dlq[0] (aws_sqs_queue, name "dead-letters", root envs/prod)
example.com/acme/infra modules/queue/main.tf:9 module.orders.aws_sqs_queue.this (aws_sqs_queue, name "orders", root envs/prod)
```

#### `context`: understand a symbol

| Parameter | Type | Default | Role |
| --- | --- | --- | --- |
| `symbol` | text, required | | A name, `<path>::<name>` or the full identity; a resource (`resource::postgres::orders`), a channel (`channel::kafka::orders.created`) or an IaC node's address (`aws_sqs_queue.orders`, `<path>::<address>`) |
| `budget` | integer | 2000 | Token budget of the answer |

It returns the symbol's community and processes, the code excerpt read from disk, who calls it
(`callers:`), who it calls (`callees:`) and the contracts it touches (`contracts:`). For a
resource, it lists who writes and who reads; for a channel, the producers and consumers. For an
IaC node, its type, its physical name (followed by `(pattern)` when a part is unresolved), its
root module, the root variables its name went through (`overridable:`, each with `(default)` or
`(tfvars)`), the nodes it references (`references:`)
and those that reference it (`referenced by:`), each with the name it went through (`via
var.queue_arn`), and the config values of its block (`values:`). An address several root modules
hold lists the nodes, each with what to pass: `<address> (root <root module>)`. A symbol of the
code with the same name comes before the node; `<path>::<address>` reaches the node.

```text
example.com/acme/web src/status.ts:2-4 health
community: src#3
processes: 1 (1 export; 1 repository): dashboard (example.com/acme/web)
function health() {
  return fetch("/api/health");
}
callers:
example.com/acme/web src/dashboard.ts:5 dashboard
callees:
example.com/acme/web src/status.ts:3 (unresolved) fetch
contracts:
example.com/acme/web src/status.ts:3 consumes http::GET::/api/health →[exact: host api.example.com is served by example.com/acme/api] example.com/acme/api::src/server.ts::GET /api/health
```

#### `trace`: follow a chain

| Parameter | Type | Default | Role |
| --- | --- | --- | --- |
| `from` | text | | The start: a symbol, a route (`GET /users/{}` or the contract), a call site, an IaC node's address or a process (`process::<identity>`, downstream only) |
| `changes` | text | | `uncommitted`: starts from the symbols the working-tree diff touches |
| `direction` | `down` or `up` | `down` | `down` is what the start reaches, `up` is what reaches it |
| `budget` | integer | 2000 | Token budget of the answer |

Pass `from` or `changes: uncommitted`, never both. The second serves to measure the impact of
what was just edited, and works only in local Meada (server mode has no working tree). From an
IaC node, `down` follows its references and `up` those that reach it, within its root module,
into the local modules through their inputs and outputs.

```text
example.com/acme/web src/orders.ts:5 listOrders
  example.com/acme/web src/api/client.ts:4 request
  →[inferred: base URL from API_URL, a base_url_env of peer example.com/acme/api] example.com/acme/api src/orders.ts:5 listOrders
chain: inferred · 1 hops · 2 repos · 93 tokens
```

(Illustrative example: the footer writes `hops` even for one hop, and the token count varies with
the answer.)

#### How to read the levels

Every link between repositories carries `→[level: reason]`. The level says how firm the link is:

| Level | Meaning |
| --- | --- |
| `exact` | certain: the code or the configuration proves the link |
| `declared` | comes from a declaration of yours in `meada.toml`, such as a shared resource |
| `inferred` | a match by name or by convention |
| `decided` | comes from `meada.decisions.toml`; the reason says who decided |
| `ambiguous` | several candidates fit, and Meada lists all of them without choosing |

`trace` ends with a footer: the weakest level of the chain, the number of hops, the repositories
involved and the tokens used. A chain is worth as much as its weakest link. A missing caller is
"unproven", not "nonexistent": calls inside macros (such as `assert!` and `format!` in Rust) are
not indexed.

## 3. The workspace and first use

The **workspace** is the set of repositories a query sees. It gathers the repository where Meada
runs, the peers its `meada.toml` names (and their peers, transitively) and the sibling git
clones (in the same folder as a repository of the set) whose own `meada.toml` names a repository
already in it. Discovery reads only local files. A repository is identified by the git remote of
`origin`, without scheme or `.git` (`example.com/acme/api`), so the clone can sit at a different
path on each machine.

In the root of `web`:

```sh
meada index
meada status
```

- `meada index` builds the index of each repository in the workspace, in `.meada/` inside it. It
  is optional before querying, since every query refreshes the index, but it makes the first
  answer fast.
- `meada status` shows each repository, the state of its index and the peers that were not found
  (`missing peer: example.com/acme/api, named by example.com/acme/web`).

For `web` to see `api`, the `meada.toml` of `web` declares the peer (section 4). Indexing writes
only inside `.meada/`, which Meada keeps out of git; the only other file it writes is
`meada.decisions.toml`, and only when you run `meada decide`.

### What is indexed

The scan covers every file in the repository, tracked or not, except:

- **Hidden**: files and directories whose name starts with `.` are not read, except the CI
  configuration at the root (`.github/`, `.gitlab-ci.yml`, `.gitlab/`, `.circleci/`,
  `.buildkite/` and `.drone.yml`), whose YAML and JSON files become config values.
- **Always ignored**: `node_modules` and `Pods` directories, a `Checkouts` inside `Carthage`,
  `*.min.js` and `*.min.css` files and the .NET build output (`obj` or `bin` next to a `.csproj`).
- **`.gitignore` and `.ignore`**, respected with or without git, at the root and in
  subdirectories.
- **`.meadaignore`**, with the `.gitignore` syntax. It does not bring back what the always-ignored
  list above excludes; a `!` pattern in it, however, re-includes what `.gitignore` ignores.

`vendor`, `dist`, `build` and generated code are not ignored by default, because any of them may
be your own code. List in `.meadaignore` what you do not want indexed:

```gitignore
vendor/
dist/
Migrations/
*.Designer.cs
```

A file that a new rule excludes leaves the index on the next refresh, like a deleted file.

### Config values

Each scalar (a string, a number or a boolean; a null is left out) of each `*.yaml`, `*.yml` and
`*.json` file the scan reads, and of the YAML and JSON files of the CI configuration above, is a
**config value**: file, key path, value, type and line. There is no key to turn it on: search
with `kind = "value"` finds them, and search with no `kind` answers with them when nothing else
matches. They stay out of `meada export`.

- **Lockfiles** (`package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml` and
  `packages.lock.json`) and files that are not read (a Helm template, which is no YAML until
  rendered; a file over 1 MiB; a file of more than 1,000 documents) are left out, and `meada
  status` counts them:
  `config files: 5 read, 2 skipped (lockfiles: 1, unreadable: 1); values: 46, redacted: 3`.
- **Terraform and OpenTofu**: `*.tf`, `*.tofu`, `*.tfvars`, `*.auto.tfvars`, `*.tf.json`,
  `*.tofu.json` and `*.tfvars.json` files are config values too. The key path runs over the
  block's type and labels and the attribute's name (`resource.aws_sqs_queue.orders.name`), and a
  block repeated under the same type and labels is `[n]` (`ingress[0].from_port`). A literal
  keeps its type; an expression (`var.environment`) or a template (`"${var.environment}-orders"`,
  a heredoc with an interpolation) is stored as written, with the type `expression` or
  `template`. A `.tofu` file shadows the `.tf` file of the same name in its directory (and
  `.tofu.json` the `.tf.json`), and the shadowed file is not read. A file that does not parse,
  or nests brackets past 64 levels, is skipped and counted like the others; `.terraform/` is
  hidden and stays out.
- **Secrets**: the values of `data` and `stringData` of a Kubernetes `Secret` (a document with
  `apiVersion` and `kind: Secret`, or such an item of a list's `items`), the `default` of a
  Terraform `variable` with `sensitive = true` and every value a `.tfvars` file gives a variable
  of that name (every value, when a Terraform file in its directory or above does not parse) are
  stored as `redacted`. SSM parameter values with SecureString or unresolved types in HCL and
  Terraform/OpenTofu JSON are redacted too. Each row keeps its key path, type and line. Nothing else is filtered: the index holds what the checkout
  already does. List in `.meadaignore` the files that must stay out of the index (`secrets/`,
  `*.env.json`).

An AWS ARN recognized by the type table, an SQS URL or `s3://archive/path` in config becomes an `exact` use side, located by `<file>#<key path>`. A plain name under the final key `queue`, `topic`, `bucket` or `table` becomes an `inferred` SQS, SNS, S3 or DynamoDB use, respectively. For example, `queue: orders` in Helm values uses `channel::sqs::orders`; `other: orders` links nothing. A use has no direction: it links only to provisioners and counts for orphans, without becoming a producer, consumer, writer or reader. `context` and contract search show these uses, and `trace` follows provisioning links to the key and back. Redacted, cut or unresolved values stay out; the attribute defining a provisioner's name does not use its own resource. Export includes the sides in `uses`, without value text, and the console shows them separately from directed sides.

### IaC nodes

Each `resource`, `data` block and `module` call of the `*.tf`, `*.tf.json`, `*.tofu` and `*.tofu.json` files is an **IaC
node**, with an address, a type, a file, its lines and the root module its evaluation starts
from. A directory of Terraform files is a module; a directory no other one calls as a local
module (a `./` or `../` `source` that stays inside the repository) is a root module, labelled
by its path (`.` at the root).

Terraform JSON uses the same evaluation and reference rules. Ordinary strings are templates; a sole interpolation keeps its result type, so `${var.items}` can supply a collection. Variable defaults and module sources are literal. Static `depends_on` strings name references. Block arrays retain their indexes in config key paths. Both JSON extensions support node addresses in declared resource `via` locations.

- **Addresses**: `<type>.<name>`, `data.<type>.<name>` and `module.<name>`. Each call of a local
  module instantiates its blocks with the call's address in front
  (`module.orders.aws_sqs_queue.this`); a `count` or a `for_each` over values that resolve to
  literals gives one node per instance, with `[key]` at the end (`aws_sqs_queue.dlq[0]`,
  `aws_cloudwatch_metric_alarm.depth["orders"]`). A `count` or `for_each` that does not resolve
  (below) gives one node, with no key.
- **Static evaluation**: the physical name (the `name` argument) and each `count` and `for_each` are
  evaluated as Terraform would, without running it or reaching the network: variable defaults, the
  values of the root's `terraform.tfvars`, `*.auto.tfvars` and their JSON forms (in that order, a
  later file winning), locals, a local module's inputs and outputs per call, `each`, `count.index`,
  templates and the pure functions `format`, `formatlist`, `join`, `split`, `lower`, `upper`,
  `title`, `trim`, `trimprefix`, `trimsuffix`, `trimspace`, `chomp`, `replace` (a literal
  substring), `substr`, `concat`, `coalesce`, `element`, `keys`, `values`, `lookup`, `merge`,
  `length`, `toset`, `tolist`, `tomap`, `tostring`, `tonumber` and `jsonencode`. Data sources,
  resource attributes, registry modules, `terraform.workspace`, `-var`, `-var-file`, `TF_VAR_`,
  sensitive variables and any other function are never evaluated: such a part of a name is a `*` and
  the name a **pattern** (`"orders-${terraform.workspace}"` is `orders-*`), matching literal names under the pattern rules below. A
  name that went through a root module's variable lists it under `overridable:`, since a deployer
  can set another value. A changed Terraform file or var file evaluates again only the root modules
  whose unit (the root and the local modules it calls) holds it.
- **References**: an expression that names a resource, a data block or a module is a reference
  of its node, stored like a call. One that goes through `var.<name>`, `local.<name>` or a
  module's output (`module.<name>.<output>`) is followed to what the call's argument, the local
  or the output names, and shows what it went through (`via var.queue_arn`). A registry module
  stays as the call's node, with nothing inside.
- **Queries**: `search` with `kind = "iac"` finds the nodes, `context` explains one and `trace`
  follows its references both ways. Nodes stay out of search with no `kind`; only nodes
  holding a contract side enter `meada export`.

### Kubernetes objects and Kustomize overlays

Every YAML or JSON document with an `apiVersion`, a `kind` and a `metadata.name` is an IaC node
too, addressed `<Kind>[.<group>]/<namespace>/<name>` (`Deployment.apps/shop/api`,
`Service/shop/api`), with no namespace for a cluster-scoped kind
(`ClusterRole.rbac.authorization.k8s.io/reader`) or for an object that sets none.

- **Overlays**: a directory with a `kustomization.yaml` that no other kustomization names as a
  base is an overlay and an evaluation unit. Its `resources`, `bases` and `components` inside
  the repository are instantiated under it, each node labelled by the overlay (`root
  overlays/dev`), with the `namePrefix`, `nameSuffix`, `namespace` and `images` of each
  kustomization on the way applied. A `..` that stays inside the repository is followed
  (`../../base`); a remote base, one above the root, absolute, missing, through a symlink, in a
  cycle or nested past 16 levels is not followed, and `meada status` counts it (`kustomize
  entries not followed: N`). Manifests no kustomization lists are plain: a directory's
  manifests are one unit.
- **References**: a Service to the workloads whose pod labels hold its selector (`via selector
  app=api`), a workload to the ConfigMaps and Secrets of its `env`, `envFrom` and volumes (`via
  env DATABASE_URL`) and to its `serviceAccountName`, an Ingress to the Services of its rules
  (`via backend shop.example.com/api`), all in one namespace and one unit, by the name as
  written. `context` on a workload adds `images:` with what each container runs once the
  overlay applied its `images`.
- **Not read**: patches, generators, `commonLabels`, Helm templates and `kind: List`.

### Crossplane

Crossplane objects are Kubernetes objects, read as above, and each is recognized by what it is
to Crossplane, which `context` shows as `role:`.

- **Roles**: a `CompositeResourceDefinition` (XRD) and a `Composition` (both cluster-scoped, so
  with no namespace), a claim or a composite resource (an object of the claim or composite kind
  an XRD of the repository defines) and a managed resource (an object with a `spec.forProvider`
  mapping).
- **Created resources**: each resource a composition creates from a literal `base` (in
  `spec.resources`, or in the `input.resources` of a pipeline step that runs function
  patch-and-transform) is a node under the composition, `<composition>/<resource name>`
  (`Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws/instance`), of the base's
  kind. Its name is the base's `crossplane.io/external-name` annotation or its `metadata.name`
  as written; a managed resource's name is its external name too.
- **References**: a claim to the XRD of its composite kind (`via composite
  XPostgreSQLInstance.database.acme.io`), a composite resource to its XRD, a claim or composite
  resource to the composition its `compositionRef` names, an XRD to its
  `defaultCompositionRef` and `enforcedCompositionRef`, a composition to the XRD of its
  `compositeTypeRef` and to each resource it creates (`via resources`, `via pipeline <step>`),
  and a created or managed resource to the definition of its kind (an XRD or a
  `CustomResourceDefinition`) when there is one. All in one unit, as every Kubernetes reference:
  a claim in another directory than its XRD keeps its role and references nothing. So `trace`
  from a claim walks down to its XRD, the default composition and the resources it creates.
- **Not evaluated**: patches and functions. A resource whose name or external name a patch (or
  a patch set) sets, or that has neither, has the name `*`, a pattern; a pipeline step that runs
  any other function (Go templating, KCL) creates no node.

### Compose

A Compose file (`compose.yaml`, `compose.yml`, `docker-compose.yaml` or `docker-compose.yml`)
with its override file (`compose.override.*`, `docker-compose.override.*`) in the same directory
is read as one project, rooted at the directory. Each service is an IaC node `services.<name>`
of type `service`.

- **References**: `depends_on`, `links` and the services that share a network (`networks:`; a service with none is on `default`),
  shown with `via depends_on`, `via links` or `via networks.<name>`. A name no service has is
  dropped.
- **Variables**: `${VAR:-default}` resolves to its default and `context` marks it
  `(overridable)`; `${VAR}` stays in the text as an unresolved part, marked `(unresolved)`.
- **Image and build**: `context` shows the service's `image:` and `build:`, kept for the image
  contract, with the Dockerfile the build names (`dockerfile` or `dockerfile_inline`; an override
  merges them like the other `build` keys). A build context is kept as a path inside the repository from its root; an absolute
  path, a URL, one that climbs out of the root or one with an unresolved part is not.
- **Queries**: as for Terraform nodes, `search` with `kind = "iac"`, `context` and `trace`.

### Images and workload code

The `image::<registry>/<repository>` contract connects a workload to the repository that builds
its image. Docker Hub short names expand, names become lowercase, and tags and digests are
removed. Builders come from docker/build-push-action tags, docker/metadata-action images,
Compose with `build` and `image`, Skaffold artifacts, Jib destinations and simple ko commands
with checked-in `KO_DOCKER_REPO` and `--bare`. Unread shell syntax in ko scripts declares no
builder. The command accepts one `--bare`, at most one literal package and no other flags; an
explicit shell must be `bash` or `sh`. Runners come from Kubernetes containers and init containers, Compose services,
Helm values `image.repository` and images after Kustomize overrides.

`search` with `kind = "image"` finds these contracts. Workload `context` shows its image and
builder repository; `trace` goes down from the workload to code and up from code to workloads.
Literal names link at `exact`; patterns and overridable values link at `inferred`, with a reason.
Several candidates stay `ambiguous`. A safe local Compose build connects the service directly to
the code its Dockerfile takes from the build context (`COPY`, `ADD` and `RUN --mount=type=bind`,
in every stage) at `exact`, even without an image, or at `inferred` when the context or the
Dockerfile path went through an overridable value. A Dockerfile Meada cannot read (missing, behind
a symbolic link, a source with a variable, a form it does not parse) leaves the whole context at
`inferred`, and the reason says it was not read. `.dockerignore` is not read. Paths never cross
symbolic links.

`meada status` lists image orphans only when the scope contains both builder and runner. With
one role, `search` and `context` show `no builder in the workspace` or `no runner in the workspace`.
Export keeps `format: 1`, retains builder locations and workload roots, and includes `executions`
for direct paths; the console graph shows the image contracts and workload code.

### CloudFormation and SAM

A qualifying template named `compose.yaml` keeps its nodes and references through cold indexing and warm refreshes; its filename does not make it Compose.

A YAML template (the short-form tags `!Ref`, `!Sub`, `!GetAtt`, `!Join` and the rest are read) or a
JSON one with a `Resources` mapping that carries `AWSTemplateFormatVersion` or the SAM transform
(`AWS::Serverless-2016-10-31`) is read as one evaluation unit, rooted at its path. Each resource
is an IaC node addressed by its logical id, its type (`AWS::SQS::Queue`,
`AWS::Serverless::Function`) the node's type.

- **References**: `Ref`, `Fn::GetAtt` (with the attribute in the `via`, `via Fn::GetAtt.Arn`), the
  variables of `Fn::Sub` and `DependsOn`.
- **Names**: the physical name of a queue, topic, bucket, table, function, stream, secret and a
  few more types is evaluated within the template from a parameter's default (`context` lists it
  under `overridable:`), `Ref`, `Fn::Sub`, `Fn::GetAtt` and `Fn::Join`. A pseudo parameter
  (`AWS::Region`), an import and any other function are unresolved parts: the name is a pattern,
  shown `pattern "assets-prod-*"`.
- **Secrets**: a parameter with `NoEcho` has its default stored as `redacted` and no name goes
  through it.
- **Queries**: as for Terraform nodes, `search` with `kind = "iac"`, `context` and `trace`.
- **Not followed**: nested stacks, macros, conditions and mappings.

### Serverless Framework

A `serverless.yml`, `serverless.yaml` or `serverless.json` with a `service` is read as an
evaluation unit, rooted at its path. Each function is an IaC node `functions.<name>` of type
`Serverless::Function`.

- **Function**: the physical name is the function's `name` or `<service>-<stage>-<function>` (the
  stage is `provider.stage` or `dev`, listed under `overridable:` as `opt:stage`, since Serverless
  names the function after `--stage` first). `context` shows `handler:` and `event:` (`"http GET
  /orders"`, `"sqs Queue.Arn"`, `"schedule rate(5 minutes)"`) and the references to the resources
  its body names with `Ref`, `Fn::GetAtt` or `Fn::Sub`.
- **Resources**: the `resources` section is read as CloudFormation (nodes at the logical id, as
  above).
- **Variables**: `${self:custom.stage}` resolves to the value at that path; the default of
  `${opt:stage, 'dev'}` resolves, and `context` lists it under `overridable:` and marks it
  `(overridable)`. `${env:...}`, `${ssm:...}`, `${sls:...}`, `${file(...)}` and the other sources
  are unresolved parts: the name becomes a pattern (`pattern "*-dead"`).
- **Queries**: as for Terraform nodes, `search` with `kind = "iac"`, `context` and `trace`.
- **Not read**: `serverless.ts`, plugins and values from other files.

### Orphan contracts

Default `search`, and `search` with `kind = "contract"`, `channel` or `resource`, also find provisioner sides by contract.

`meada status` lists orphan contracts per kind and broker (or resource kind): provisioners
with no use and uses with no provisioner. Orphan lines appear only when the queried repository
or workspace contains at least one provisioner and one use of that group. A code-only or
IaC-only repository produces no such lines; `search` and `context` mark the side itself with
`no provisioner in the workspace` or `no use in the workspace`. Provisioning links, including
patterns and independent root modules, determine whether a counterpart exists. Existing
warnings about declared resources without writers and readers remain separate.

### Terraform provisioners

An `aws_sqs_queue` resource provisions `channel::sqs::<name>` through its `name` argument;
an `aws_s3_bucket` provisions `resource::s3::<name>` through `bucket`. The type table in
`src/iac/types.toml`, embedded in the binary, also reads names from ARNs and URLs. A declared
`[[resource]]` with `kind = "s3"` and the same `id` joins that contract, keeping `declared`.

CloudFormation and SAM use the same contracts for SQS, SNS, EventBridge, Kinesis, S3,
DynamoDB (Table, GlobalTable and SimpleTable), RDS (instance or cluster), ElastiCache
(cluster or group), SSM and Secrets Manager when the physical name is known. The property
defining that name does not use its own resource; secrets contribute only their identity.

Every producer, consumer, writer and reader links to each provisioner, identified by address
and root module. Roots such as `envs/dev` and `envs/prod` are never ambiguous among themselves.
Written names give `exact`; names passing through overridable values give `inferred`, with the
variables and their sources in the reason. `context` on the block lists its contract sides;
on a producer it lists provisioners. `trace` follows the block to code with `down` and code to
infrastructure with `up`. Export keeps `format: 1` and adds `iac_nodes`, `provisioners` and
`provisions`, omitted when empty. Unlisted types and data blocks do not provision contracts. A
pattern
with at least three literal letters or digits (separators do not count) and wildcards on segment
boundaries (`-`, `_`, `.`, `/`) links to a literal name at `inferred`, or `ambiguous` when several
names match. Patterns never link to patterns. The console graph lists provisioner sides and their
separate links.

### GCP and Cloud Run

Terraform/OpenTofu recognizes `google_pubsub_topic` and `google_pubsub_subscription` as
`pubsub` channels, `google_storage_bucket` as `gcs`, `google_sql_database_instance` and
`google_sql_database` as `cloudsql`, and `google_secret_manager_secret` as
`gcpsecretmanager`. The name comes from `name`, except secrets use `secret_id`.
Cloud SQL instances use `<instance>`; databases use `<instance>/<database>`, with a literal
instance or a resolved reference to a known instance. A database does not provision its
instance. Projects stay outside the identity.

A topic uses `channel::pubsub::<id>`; a subscription uses
`channel::pubsub::subscriptions/<id>`, including when both share the same id. A subscription
links its target's consumers to producers of the topic named by `topic`: `exact` without a
filter, `inferred` with a filter or overridable names. An unresolved source adds no route
and never invents a topic from the subscription's name.

`gs://<bucket>/object`, `projects/<project>/topics/<id>`,
`projects/<project>/subscriptions/<id>` and `projects/<project>/secrets/<id>` (also with
`/versions/<version>`) are `exact` config uses. Plain names under final keys
`pubsub_topic`, `pubsub_subscription`, `gcs_bucket`, `cloudsql_instance`, `cloudsql_database`
and `gcpsecretmanager_secret` are `inferred`; `cloudsql_database` requires
`<instance>/<database>`. Generic `topic` and `bucket` retain their SNS and S3 meanings.
Malformed identifiers, hidden or cut values and bare text equality create no use.

`google_cloud_run_service`, `google_cloud_run_v2_service` and `google_cloud_run_v2_job` are
workloads: each container keeps its image and environment. Images join the repository that
publishes them; overridable evidence weakens the link. Environment reads receive values only
from the container running their code and remain `inferred`. Secret Manager references stay
unresolved targets, without reading secret contents. Search, `context`, `trace` in both
directions, scoped orphans and export include the new contracts and runners; environment
entries and configuration values stay outside the export.

### GCP SDK use in five language groups

Constructor-backed GCP clients create Pub/Sub producers and subscription consumers, Cloud Storage writers/readers and Secret Manager readers. JavaScript and TypeScript share one group, with both source languages checked independently. The operation inventory is explicit; other overloads are outside this list.

| Group | Pub/Sub producer / consumer | Cloud Storage writer / reader | Secret Manager reader |
| --- | --- | --- | --- |
| JavaScript + TypeScript | `new PubSub().topic(name).publishMessage(...)` / `.subscription(name).on("message", handler)` | `new Storage().bucket(name).file(object).save(...)` / `.download(...)` | `new SecretManagerServiceClient().accessSecretVersion({name})` |
| Python | `PublisherClient().publish(topic, ...)` / `SubscriberClient().subscribe(subscription, callback=...)` | `storage.Client().bucket(name).blob(object).upload_from_string(...)` / `.download_as_bytes()` | `SecretManagerServiceClient().access_secret_version(request={"name": name})` |
| Java | `Publisher.newBuilder(topic).build().publish(...)` / `Subscriber.newBuilder(subscription, handler).build().startAsync()` | `StorageOptions.newBuilder().build().getService().create(BlobInfo.newBuilder(BlobId.of(bucket, object)).build(), ...)` / `.readAllBytes(bucket, object)` | `SecretManagerServiceClient.create().accessSecretVersion(name)` |
| Go | Pub/Sub v2 `NewClient`: `Publisher(topic).Publish(...)` / `Subscriber(subscription).Receive(ctx, handler)`; v1 `Topic` / `Subscription` | `storage.NewClient`: `Bucket(bucket).Object(object).NewWriter(ctx)` / `.NewReader(ctx)` | `secretmanager.NewClient`: `AccessSecretVersion(ctx, &secretmanagerpb.AccessSecretVersionRequest{Name: name})` |
| C# | `client = await PublisherClient.CreateAsync(topic); client.PublishAsync(...)` / `client = await SubscriberClient.CreateAsync(subscription); client.StartAsync(handler)` | `StorageClient.Create()`: `UploadObject`, `UploadObjectAsync` / `DownloadObject`, `DownloadObjectAsync`, bucket as first argument | `SecretManagerServiceClient.Create()`: `AccessSecretVersion`, `AccessSecretVersionAsync` |

Literal topic, subscription, bucket and secret identities use the same normalization as Terraform and config. A subscription is `channel::pubsub::subscriptions/<id>` and never a guessed topic. Names read from the environment remain unresolved without a workload that actually runs the code; scoped values resolve at `inferred`, preserving the operation subtype. Hidden constructors, injected types without construction, name-matched factories, reassigned clients and opaque return alternatives supply no SDK side.

Request fields changed before use leave the name unresolved; reads before the change and independent objects preserve their identity. Typed helpers must match the resource being used. In C#, await `CreateAsync` first, then invoke the operation on the returned client.

### AWS channels, data, secrets and subscriptions

The Terraform/OpenTofu table also recognizes the types below. ARNs reduce to physical names,
including parameter and secret paths; the six-character Secrets Manager ARN suffix is removed.
The same provisioning, `context`, `trace` and export rules apply to every contract.

| Terraform type | Physical name | Contract |
| --- | --- | --- |
| `aws_sns_topic` | `name` | `channel::sns` |
| `aws_cloudwatch_event_bus` | `name` | `channel::eventbridge` |
| `aws_kinesis_stream` | `name` | `channel::kinesis` |
| `aws_dynamodb_table` | `name` | `resource::dynamodb` |
| `aws_db_instance` | `identifier` | `resource::rds` |
| `aws_rds_cluster` | `cluster_identifier` | `resource::rds` |
| `aws_elasticache_cluster` | `cluster_id` | `resource::elasticache` |
| `aws_elasticache_replication_group` | `replication_group_id` | `resource::elasticache` |
| `aws_ssm_parameter` | `name`, leading `/` kept | `resource::ssm` |
| `aws_secretsmanager_secret` | `name` | `resource::secretsmanager` |

An SSM `SecureString` parameter stores `value` as `redacted` in HCL and Terraform/OpenTofu JSON. When `type` is an expression the
value reader cannot resolve, its value is also hidden conservatively.

An `aws_sns_topic_subscription` with `protocol = "sqs"` links queue consumers to topic
producers. `aws_cloudwatch_event_rule` with `aws_cloudwatch_event_target` does the same from a
bus to a queue; absent `event_bus_name` means `default`. Without a filter the level is `exact`;
with `filter_policy` or `event_pattern` it is `inferred`. Overridable names also give `inferred`,
with their variables in the reason. The reason identifies the subscription or target and rule.
Unresolved endpoints, other protocols and targets without a unique rule in the same bus add no
links. Timer-only rules do not route events published to the bus.

`context` lists producers and consumers through the subscription. `trace` goes up from the
consumer to the producer and down from the provisioner to consumers; an `sns`, `eventbridge`
or `kinesis` source channel can also start a trace. Subscription summaries enter the optional `subscriptions` array with source, target, node and
root. Their links enter `links`, keeping
provisioning links separate in `provisions`. `meada status` lists provisioned contracts without
use and uses without a provisioner, per kind and broker, only when the workspace contains at
least one provisioner and one use of that group. Other IaC formats remain nodes and references;
this contract table covers `.tf`, `.tf.json`, `.tofu` and `.tofu.json` files.

### AWS SDK use in Java, Go and C#

The Java 2.x, Go v2 and .NET SDK forms recognize SQS, SNS, EventBridge and Kinesis as channels,
and S3, DynamoDB, SSM Parameter Store and Secrets Manager as resources. The client must originate
in an SDK constructor: Java `create()` or `builder().build()`, Go `New` or `NewFromConfig`,
and C# `new Amazon...Client(...)`. Aliases and factories follow only `exact` hops; an injected
type alone, a hidden name or a name-matched hop does not establish the client. SQS keeps its
earlier forms: a field, parameter or typed local declared with the SDK's SQS client type
(`SqsClient`, `SqsAsyncClient`, `*sqs.Client`, `IAmazonSQS`, `AmazonSQSClient`) is a client,
injected ones included, while a repository type of that name is not.

Literal names and ARNs identify the contract; an environment read keeps an unresolved side
and records its variable, including for producers. Batches and DynamoDB transactions
(`TransactWriteItems`, `TransactGetItems`) retain every name, and a `ConditionCheck` reads its
table. Readers belong to their enclosing function, writers to the operation call, and SDK
links are `exact`;
resource declarations remain `declared`. Unresolved sides never link, and
`GetParametersByPath` does not identify a physical parameter. The
[capabilities table](site/en/capabilities.md) lists operations and differences between SDKs.


## 4. `meada.toml`: reference

Each repository may have a `meada.toml` at its root, versioned in git, that says who it talks to.
The file is optional: without it, the repository declares no peer. There are three kinds of
table, `[[peer]]`, `[serve]` and `[[resource]]`, and no other. An unknown or misspelled key is an
error that names the file, the line and the key.

```toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]

[serve]
hosts = ["web.example.com"]

[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

### `[[peer]]`: what this repository consumes from

Repeatable, one table per peer. Whoever consumes a route names the peer.

| Key | Type | Required | Role |
| --- | --- | --- | --- |
| `repo` | text | yes | The peer's identity: the git remote of `origin`, without scheme or `.git` |
| `path` | text | no | A hint of where the clone usually sits, relative to this repository's root |
| `base_url_env` | list of text | no (empty) | Variables this repository reads the peer's base URL from |

```toml
[[peer]]
repo = "example.com/acme/api"
path = "../api"
base_url_env = ["API_URL"]
```

Meada never reads the variables' values, only their names. When the code builds a URL from
`API_URL`, the peer that declares `API_URL` in `base_url_env` is the provider, with level
`inferred` and the reason `base URL from API_URL, a base_url_env of peer example.com/acme/api`.

### `[serve]`: what this repository serves

A single table. Whoever serves routes declares the hosts it answers on and how a gateway rewrites
the paths.

| Key | Type | Required | Role |
| --- | --- | --- | --- |
| `hosts` | list of text | no (empty) | The hosts this repository answers on |
| `prefix` | text | no | What a gateway puts in front of every served path |
| `strip` | text | no | What a gateway removes from the start of every served path |
| `documents` | list of text | no (empty) | OpenAPI and AsyncAPI documents besides those found by file name |

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"
strip = "/v1"
documents = ["docs/openapi.yaml"]
```

- **`hosts`**: a call to `https://api.example.com/...` links to the repository that lists that
  host, at level `exact`.
- **`prefix` and `strip`**: must start with `/`. Meada normalizes the value and drops the trailing
  slash; a lone `/` rewrites nothing. With the two above, a call to `/shop/orders` reaches the
  code's `/v1/orders` route, at level `inferred`, with the reason
  `gateway prefix /shop, gateway strip /v1`. `strip` matches whole segments (`/v1` does not strip
  `/v10`), and a version segment only comes off when you write it. A caller that uses the code's
  own path keeps linking as before.
- **`documents`**: paths relative to the repository root, with no `..` and no absolute path.
  Meada already finds OpenAPI and AsyncAPI documents by file name; use the key for those with
  another name. A listed document is read even if `.gitignore` ignores it.
- The mere presence of the `[serve]` table, even empty, makes the repository's documents count as
  a source of providers.

### `[[resource]]`: shared resources

Repeatable. A resource that no code names, such as a table or a bucket, goes here.

| Key | Type | Required | Role |
| --- | --- | --- | --- |
| `kind` | text | yes | The kind, a lowercase identifier (letters, digits and hyphens, starting with a letter) with no meaning to Meada: `postgres`, `s3-bucket` |
| `id` | text | yes | The resource's id, not empty |
| `role` | `read`, `write` or `read-write` | yes | What this repository does with it |
| `via` | list of text | no (empty) | Symbols, IaC addresses or `<file>#<key path>` with `[key=value]` selectors |

```toml
[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]
```

Each writer links to each reader of the same resource in another repository, at level `declared`.
Without `via`, or if no `via` matches a symbol, IaC node or key, the side is the whole repository. `meada status`
shows, under each repository, the unlinked resources and the `via` entries that match no symbol, IaC node or key.
The resource's identity in `search` and `context` is `resource::postgres::orders`.

### A complete example: `web`, `api` and `worker`

`web` consumes the API:

```toml
# web/meada.toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]
```

`api` serves routes behind a gateway and writes to a table:

```toml
# api/meada.toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"
strip = "/v1"

[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]
```

`worker` consumes `api` and reads the same table:

```toml
# worker/meada.toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]

[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

Running `meada status` in `web`, Meada finds `api` through the git remote (or through `path`, if
there is one), and `worker` joins as a sibling clone of `web` or of `api`, because its
`meada.toml` names `api`, which is already in the workspace. `worker` reads the `orders` table
that `api` writes, and Meada links the two through the resource, at level `declared`.

### In server mode

In server mode, a mirror without a `meada.toml` receives the same `serve`, `resource` and `peer`
blocks (the last without `path`) through the `meada.workspace.toml` manifest. The repository's own
`meada.toml`, when it exists, applies in full and the manifest's blocks are ignored. See
[Server mode](https://claudiomascaro.github.io/meada/en/server/).

## 5. Decisions and deciders

An **ambiguous link** (several providers serve the route) or a **dynamic URL** (the call builds
the URL at run time) stays an open item. Meada does not guess: it lists the candidates and waits
for a decision. The one who decides is you, or a model, if you ask for it.

To see the open items:

- over MCP, `search` with `kind: ambiguous` or `kind: unresolved` lists each item with its number
  of candidates;
- on the CLI, `meada decide --list` lists the items with numbered candidates.

### Deciding as a person

`meada decide`, in the repository's root, shows each open item with numbered candidates. Pick a
number, or `s` to skip. At the end, the command writes the choices to `meada.decisions.toml`; if
you chose nothing, it writes nothing.

```text
example.com/acme/web::src/status.ts::health::fetch#0
src/status.ts:3  fetch("/api/health")
  1) example.com/acme/api GET /api/health
  2) example.com/acme/worker GET /api/health
```

### The `meada.decisions.toml` file

It sits at the root and is versioned in git like a lockfile, and its diff is what gets reviewed.
Indexing only rereads it: no decision goes through a model or the network at that point.

```toml
[[decision]]
kind = "ambiguous-link"
call = "example.com/acme/web::src/status.ts::health::fetch#0"
target = "example.com/acme/api::http::GET::/api/health"
by = "human"

[[decision]]
kind = "dynamic-url"
call = "example.com/acme/web::src/status.ts::track::fetch#0"
target = "example.com/acme/worker::http::POST::/api/events"
by = "example/model"
p = 0.62
```

| Key | Type | Required | Role |
| --- | --- | --- | --- |
| `kind` | `ambiguous-link` or `dynamic-url` | yes | The item's kind: several providers, or a URL Meada does not evaluate |
| `call` | text | yes | The call site, as `meada export` and `meada decide --list` name it |
| `target` | text | yes | `<provider repository identity>::<contract identifier>` |
| `by` | text | yes | `human`, or the model's id |
| `p` | number from 0 to 1 | only for a model | The probability the model declared; forbidden with `by = "human"` |

The file has one entry per `call`: repeating a call is an error that names the line. The decided
link gets level `decided`, and the reason shows who decided.

When the code changes and a decision no longer fits (the call is gone, the item is no longer open,
the provider is gone or is no longer a candidate), `meada status` shows it as **stale** under the
repository, and `meada decide --prune` removes it. A decision waiting on a peer not yet found
appears separately (`decision waiting on missing peer ...`) and is kept.

### Model decider

`meada decide --decider` swaps the person for a model. It is Meada's only path to a language
model, and it runs only when you ask for it with that flag. No other part of Meada, indexing and
querying included, uses an LLM or the network.

#### Environment variables

The decider is configured only through environment variables (nothing comes from a file):

| Variable | Role | Default |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` or `chat` | `decisions` |
| `MEADA_DECIDER_URL` | base URL | `https://openrouter.ai` (`decisions`); required with `chat` |
| `MEADA_DECIDER_KEY` | API key, sent only in the `Authorization` header | required with `decisions`; optional with `chat` |
| `MEADA_DECIDER_MODEL` | model | `typesafe/jev-1.14` (`decisions`); required with `chat` |

#### Protocols

- **`decisions`** (the default) uses OpenRouter's decisions API with the Jev model. A single
  request carries all the items, and the response brings the probability of each candidate. The
  request asks for zero data retention (`zdr: true`, `data_collection: deny`); if OpenRouter
  refuses that preference, the request fails and nothing is written.

  ```sh
  export MEADA_DECIDER_KEY=...
  meada decide --decider
  ```

- **`chat`** talks to any endpoint compatible with the OpenAI chat API
  (`<URL>/chat/completions`), a local model included, for example through Ollama. One request per
  item. The probability is the one the model itself states, not a calibrated measure.

  ```sh
  MEADA_DECIDER_PROTOCOL=chat \
  MEADA_DECIDER_URL=http://localhost:11434/v1 \
  MEADA_DECIDER_MODEL=<model> \
  meada decide --decider
  ```

Any gateway that speaks one of the two protocols works: just change the URL. HTTPS is required
everywhere; HTTP only for `localhost` or a loopback address.

#### What is sent

Per item, and nothing besides:

- the call (the code excerpt at the call site);
- the function that contains it, read from disk (without it, when the call is at the top level of
  the file or the enclosing symbol is not a function);
- the list of candidates (repository, method and path);
- the identity of the calling repository, the model id and a fixed sentence that describes the
  task.

No provider code and no other file leaves the machine. To see exactly what would be sent, run
`meada decide --decider --show-request`: it prints each request, with the key replaced by
`<hidden>`, and sends none.

#### Privacy

- Without `--decider`, no code leaves the machine. Indexing, querying over MCP and deciding as a
  person use no network. Two things escape that and are not decisions: `npx @meada/meada` downloads the
  binary, and `mcp --listen` opens an HTTP server (see server mode).
- With `--decider`, the code above goes to the endpoint you configured. To keep everything on the
  machine, use the `chat` protocol with a local model (`http://localhost:...`).
- The key travels only in the `Authorization` header and is never printed.
- Each response is validated per item. An invalid response or a failed request is reported
  without writing that item, and the command exits with an error.
- What the model proposes reaches only `meada.decisions.toml`, which is a versioned file: the
  decision goes through diff review like any other change.

#### Fixed limits

- `--min-p` (from 0 to 1) is 0.5 by default: below it, the proposal is printed as a suggestion
  and not written.
- At most 50 candidates per item: with more, the item is skipped in decider mode (human mode
  still offers it).
- 120 seconds per request, from connection to the last byte, with no retry.
- With the `decisions` protocol, the probabilities of a response must sum to 1, with a tolerance
  of 0.01.

In server mode, decisions are made in a pipeline, not by the server; see
[Decide in a pipeline](https://claudiomascaro.github.io/meada/en/server/#decide-in-a-pipeline).

## 6. CLI commands

```text
meada <COMMAND>
```

Messages follow `LANG` (Portuguese, English or Spanish); MCP's are always in English. Every invocation
accepts `--help` and `--version`.

### The five commands of the default build

| Command | What it does |
| --- | --- |
| `meada index` | Builds the indexes of the workspace of the repository in the current directory |
| `meada status` | Shows the workspace: each repository, the state of its index and the peers not found |
| `meada export` | Prints the index as sorted JSON, with the links that touch it |
| `meada mcp` | Serves the repository in the current directory to agents over MCP (stdio) |
| `meada decide` | Resolves this repository's open items and records the choices in `meada.decisions.toml` |

#### `meada index`

```sh
meada index [--workspace <manifest>]
```

Refreshes the index of the current repository and its peers. With `--workspace`, it refreshes
that of every repository the `meada.workspace.toml` manifest lists (the operator's refresh, in
server mode).

#### `meada status`

```sh
meada status [--workspace <manifest>]
```

Shows the workspace's repositories and the state of each index. Under each repository, it shows
the unlinked resources, the `via` entries that match no symbol, IaC node or key, the stale decisions or those
waiting on a peer, and each source file a language pack dropped for going over its budget. Peers
that were not found appear as `missing peer`.

#### `meada export`

```sh
meada export [--workspace <manifest> --repo <identity>]
```

Prints the current repository's index as sorted JSON (format in section 7). With `--workspace`,
the `--repo` flag is required and picks the manifest's repository; each requires the other.

#### `meada mcp`

```sh
meada mcp
meada mcp --listen <address> --workspace <manifest>
```

Without flags, it serves over MCP on stdio (section 2). With `--listen`, it serves over HTTP in
server mode, at `host:port`, `:port` or `port` (loopback without a host); `--listen` and
`--workspace` go together.

#### `meada decide`

```sh
meada decide [--item <id>] [--dry-run] [--list] [--prune]
             [--decider [--show-request] [--min-p <p>]]
```

| Flag | Role |
| --- | --- |
| `--item <id>` | Handles only that item (the id `--list` or `search` prints), even if already decided, to redo it |
| `--dry-run` | Prints the file that would be written and writes nothing |
| `--list` | Prints the open items, the stale decisions and those waiting on a missing peer; neither asks nor writes |
| `--prune` | Removes the stale decisions; keeps those waiting on a missing peer. Does not combine with `--item`, `--dry-run` or `--list` |
| `--decider` | Asks the model configured by the `MEADA_DECIDER_*` variables instead of a person |
| `--show-request` | Only with `--decider`: prints each request that would be sent and sends none |
| `--min-p <p>` | Only with `--decider`: the minimum probability, from 0 to 1, to write (default 0.5) |

### Cloud module commands

Builds with optional modules (the `-cloud` image carries them all) add commands to run Meada as an
organisation's service. They are not part of local use and are described in the operations pages.

| Command | Module | Role |
| --- | --- | --- |
| `meada snapshot` | `snapshot` | The snapshot store: `publish`, `pull`, `lock`, `pointer` and `fence` |
| `meada workspace` | `generation-pointer` | The workspace versions in the snapshot store |
| `meada workspaces` | `workspaces` | The named workspaces, with `mcp` in the `proposals` module |
| `meada ui` | `ui` | The management console |
| `meada github-app` | `github-app` | Git credentials from GitHub App tokens, for the indexing job |
| `meada postgres` | `postgres` | The data layer: `migrate` creates the schema or takes it to the binary's version |

The `--generations` flags of `status`, `export` and `mcp` exist only in the `snapshot` module. See
[Server mode](https://claudiomascaro.github.io/meada/en/server/) and
[Cloud modules](https://claudiomascaro.github.io/meada/en/cloud/).

## 7. The export

`meada export` prints a repository's index as JSON, ready for `diff` between two runs and for
feeding a knowledge base.

```sh
meada export > web.export.json
```

- **Deterministic**: the same file tree produces the same bytes. Every list comes sorted and the
  text ends with a newline.
- **No source text**: whoever needs the excerpt reads the `span` in the file.
- **One repository per export**: an organisation runs `meada export` on each repository and joins
  the results by symbol identity.
- **With the export format and the indexed commit**: `format` (1 today) rises only when a reader of the
  previous format would misread the new one, so readers must ignore the fields they do not know:
  a new optional field is a minor change. `commit` and `dirty` say which state of the repository
  the index was read from (see [Usage](https://claudiomascaro.github.io/meada/en/cli/)).

`jq` recipes (one record per symbol, duplicates, fan-in across repositories) are in the
[README](../README.en.md#feed-a-knowledge-base).

### Top-level fields

Empty lists are omitted, except `files`, `symbols`, `calls`, `providers`, `consumers` and `links`,
which always appear (like `repository`, which is text).

| Field | Content |
| --- | --- |
| `format` | The export format, 1 today |
| `repository` | The repository's identity (`example.com/acme/web`) |
| `commit`, `dirty` | The indexed commit and whether the checkout differed from it; left out together outside git, before the first commit and when nothing trustworthy is known |
| `files` | The files read: `path` and `hash` |
| `symbols` | The symbols: `id`, `span` (`path`, `start`, `end`, byte offsets) and `hash` (hexadecimal BLAKE3 of the span's bytes) |
| `calls` | The call sites: `id`, `caller`, `target` and `level` (empty until resolved), `candidates` (if ambiguous), `span`, `narrowed` and `decorates` |
| `providers` | The provider side of each contract: `contract`, `site`, and if present `gateway` and `shape` |
| `consumers` | The consumer side: `contract` (null if the URL was not evaluated), `site`, `via` (how it names the provider's repository, such as `host:api.example.com` or `env:API_URL`) and `shape`. On a consumer of a CI contract, `via` is the `uses` as written, with the optional fields `ref` (the Git ref after the `@`), `with` (the keys passed, sorted) and `secrets` (the keys, or the word `inherit`) |
| `links` | The links: `consumer`, `provider`, `contract`, `level`, `reason`, and if present `by`, `p` and `drift` |
| `bindings`, `publishes` | The bindings (`contract`, `exchange`, `pattern`, `site`) and the publications (`exchange`, `key`, `site`) of RabbitMQ that the repository declares |
| `documents` | The OpenAPI and AsyncAPI documents read: `path`, `format`, `version` and `contracts` |
| `packages` | The packages the repository provides: `name`, `version`, `repository`, `directory` and `ecosystem` |
| `dependencies` | The dependency entries that name a workspace package: `manifest`, `field`, `name`, and if present `alias` and `range` |
| `imports` | The imports resolved to a workspace package: `site`, `specifier`, `name`, `level`, `how` and `target` |
| `communities` | The communities: `name` and `members` (sorted) |
| `processes` | One process per entry point: `entry_point`, `kind` (`route`, `export`, `channel` or `program`), `label` and `steps` |
| `stale_decisions` | The decisions that no longer apply: `call`, `target` and `cause` (`call-gone`, `item-not-open`, `target-gone` or `not-candidate`) |

### Recurring values

- **`level`**: `exact`, `declared`, `inferred`, `decided` or `ambiguous`, from firmest to weakest.
- **Contract identifiers**: always start with the kind, `http::GET::/api/health`,
  `channel::kafka::orders.created`, `package::...` or `resource::postgres::orders`.
- **Symbol identity**: `<repository>::<path>::<name>`, for example
  `example.com/acme/web::src/status.ts::health`. A call site adds the target and the index within
  the symbol: `...::health::fetch#0`.
- **`shape`** (in `providers` and `consumers`): the field paths that a response or a message
  carries, with `source` and `fields`, among other keys. Drift, on the `link`, lists in `missing`
  the fields the consumer reads and the provider does not return.
- **`steps`** of a process: `depth`, `repository`, `level`, `via` (`entry`, `call` or `link`),
  `node`, and if present `reason`, `mark`, `candidates` and `contract`.

### Example

```json
{
  "repository": "example.com/acme/web",
  "files": [{ "path": "src/status.ts", "hash": "d61eae8a..." }],
  "symbols": [
    {
      "id": "example.com/acme/web::src/status.ts::health",
      "span": { "path": "src/status.ts", "start": 31, "end": 99 },
      "hash": "5cc3a63c..."
    }
  ],
  "calls": [],
  "providers": [],
  "consumers": [],
  "links": []
}
```

(Illustrative excerpt: a real export carries all the repository's files, symbols and links.)

## 8. Contract kinds

A **contract** is what one repository exposes and another uses. Meada reads six kinds, and each
has its own linking rule. This section says what each kind links and where it stops, so you can
tell a link that is missing because of a known limit from one that is missing because of a bug.

Every contract identity starts with its kind, and that identity is what you pass to `context` and
`trace`:

| Kind | Identity | Example |
| --- | --- | --- |
| HTTP route | `http::<METHOD>::<path>` | `http::GET::/api/orders/{}` |
| Channel | `channel::<broker>::<name>` | `channel::kafka::orders.created` |
| Package | `package::<ecosystem>::<name>` | `package::npm::@example/shared` |
| Resource | `resource::<kind>::<id>` | `resource::postgres::orders` |
| CI | `ci::<repository identity>/<path>` | `ci::github.com/acme/workflows/.github/workflows/publish.yml` |

OpenAPI and AsyncAPI documents are not a separate kind: they give routes and channels to the kinds
above (section 8.3).

How to tell whether a missing link is a bug:

- If the code uses a library, a framework or a form that sections 8 and 9 do not list, the link is
  missing because of a known limit, not a bug.
- If the library is on the list and the code uses the described form, the link should exist. Run
  `meada status` and `search` with `kind=unresolved` and `kind=ambiguous` to see whether Meada
  read the side and did not link it, and then open an issue with the query, the answer and the
  expected one.
- A call inside a Rust macro is not indexed: a caller that is missing there is "unproven".
- A name that Meada cannot compute yields no link by itself. A URL built at run time is an
  unresolved consumer, which you decide with `meada decide` (section 5). A channel name read from
  the environment is an unresolved side, a known limit that no decision resolves (section 8.4).

### 8.1 HTTP routes

A route links whoever **serves** it (the provider, the code that handles it) to whoever **calls**
it (the consumer, an HTTP client call). In every language, Meada normalizes the path:
`/users/:id`, `/users/{id}` and `/users/<int:id>` become the same `/users/{}`, and the method is
uppercased. Path case and API versions are kept.

Infrastructure can declare routes: API Gateway HTTP and REST in Terraform/OpenTofu, SAM
`Api` and `HttpApi` events, and Serverless `http` and `httpApi` events. For example, a SAM
event with `Method: GET` and `Path: /orders/{id}` provides `http::GET::/orders/{}`. Declare the
public host in `[serve] hosts`, as for a code route. The target is the handler symbol in the
same repository when it resolves with certainty, including an aliased export; otherwise it
is the whole repository. Missing or ambiguous files, ZIP archives, remote or absolute paths,
parent steps and symlinks never fabricate a handler.

A REST API's public method comes from `http_method`, not the POST used to invoke Lambda.
The integration must identify one Lambda in the same evaluation unit and instance;
incidental references, mismatched APIs and unknown targets provide no route. Quick create
requires an explicit method/path `route_key`. `ANY` (or Serverless `method: '*'`) serves any
method at that path; `$default` or a catch-all without a finite path provides none. Overridable
values retain `inferred` evidence. Evaluation and path validation happen during indexing,
without network access; queries use stored facts.

How the link is chosen, depending on what the URL shows:

1. **Host.** The URL has a host (`https://api.example.com/orders`) that some repository lists in
   `[serve] hosts`. The link is `exact`. If no repository serves that host, there is no link.
2. **Environment variable.** The URL starts with an environment read (`API_URL`) that a peer
   declares in `base_url_env`. The link is `inferred`; the process environment value is never read.
3. **Unknown host.** The URL does not say which host it targets (a relative path, for example)
   and only one repository in the workspace serves that method and that path. The link is
   `inferred`, with the reason `the only repository serving it is`.
4. **Several providers.** The link is `ambiguous`, and Meada lists all the candidates, one link per
   candidate. It is an open item that `meada decide` resolves.

A provider that serves every method has the method `ANY` (`http::ANY::/health`) and links to
consumers of any method, with `any method` in the reason. A consumer whose URL Meada cannot
compute, even after folding constants and the arguments of functions that wrap the client, is an
**unresolved consumer**: it appears in `search`, in `context` and in the export, never links, and
is an open item. A function that builds the URL from its parameters becomes a consumer at each
call of it, with the arguments substituted, at any depth.

Why a link may be missing:

- The path that a **provider** declares has a part Meada does not read (an interpolation, a
  parameter, an environment read in the middle of the path): the route does not exist for Meada.
  An unreadable prefix does not become the root, it leaves the route with no prefix at all. (On the
  consumer side, a hole in the middle of the URL becomes a `{}` parameter and still links.)
- The framework or the client is not on the language's list (section 9).
- The repository that serves the route is not among the peers, and the workspace does not have it.

A literal base URL from configuration or a workload entry can also name a Kubernetes Service:
`http://orders:8080`, `orders.shop`, `orders.shop.svc` or `orders.shop.svc.cluster.local`,
with or without a port in every form. Meada follows the selector, the selected workload and its
image to the repository serving the route. Compose names follow the service and its image or
local build context. The link is `inferred`, naming the Service; an environment-based link
also names its workload and variable. Only indexed entries of the workload running that code
are read, preserving container, namespace, evaluation root and local code path scope.
Distinct candidates remain `ambiguous`.

An Ingress with host `shop.example.com`, prefix `/store` and backend `orders` lets
`https://shop.example.com/store/orders` reach that Service's `/orders` route. The most specific
prefix wins, like a gateway prefix. Rewrite annotations, unresolved backends and a `pathType`
other than `Prefix` create no links; older forms without `pathType` are accepted. Wildcard
hosts and controller-specific paths are not interpreted. Resolution uses indexed facts and
never queries a cluster.

### 8.2 Declared resources and gateway prefixes

**Shared resources.** What no code names, such as a table or a bucket, you declare in
`[[resource]]` (section 4). Meada does not read code for this kind: it links each writer to each
reader of the same `kind` and `id` in another repository, at level `declared`, with the contract
as the reason.

```toml
# api/meada.toml
[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]

# worker/meada.toml
[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

With `via`, the side is the symbol, IaC node or key; without `via`, or if no location matches, the side is the whole
repository and appears as `(repository-level)`: a `trace` shows the link but does not enter that
side. `meada status` lists, under each repository, the unlinked resources and the `via` entries
with no matching location. Since Meada does not interpret `kind`, nothing guarantees that `postgres` and
`postgresql` are the same resource: write it the same way on both sides.

**Gateway prefixes.** When a gateway rewrites paths, the consumer calls a path the provider's code
does not have. You declare the rewrite in `[serve]` with `prefix` and `strip` (section 4), and
Meada also accepts the path of each `servers` URL of an OpenAPI document as that document's
prefix. A route links through its gateway path as well, and that link is `inferred`, with the
reason `gateway prefix /shop`, `gateway strip /v1` or both. Within one repository, the code's path
wins over the gateway's.

### 8.3 OpenAPI and AsyncAPI documents

A repository with the `[serve]` table (even empty) has its documents read as a source of
providers. Meada reads **OpenAPI 3.0 and 3.1** and **AsyncAPI 2.x and 3.0**, in JSON or YAML,
found by file name or listed in `[serve] documents`. Without `[serve]`, the documents are ignored.

- **Routes.** Each route in the document is a provider. If the code already has a route with the
  same contract, the two become a single provider, with the code's point as the site. A route that
  only the document lists appears as `<document> (documented only)` and no `trace` enters it. A
  provider coming from a document is a written contract: it does not lower the level (a matching
  host stays `exact`).
- **Channels.** Each AsyncAPI channel becomes a producer or consumer side, merged with the code's
  side when the contract is the same. In a document, the name of an `amqp` channel is a queue's,
  never an exchange's.
- **Data format.** What the document says about the body of a response or a message is the shape
  (section 10) of that side. Only a route's shape enters the drift comparison; a message's is
  recorded, but no drift is computed for channels.
- **Hosts.** The host of each `servers` URL counts as a `[serve] hosts`.

`meada status` shows, per repository, what was read and what was ignored. A part of the shape that
Meada cannot list (an object that accepts extra properties, a schema behind an external
reference) stays **open**: what lies below it may exist. When the document and the code disagree
about a route's shape, the document's shape wins.

### 8.4 Messaging channels

A **channel** is a broker's queue or topic. Each producer links to each consumer of the same
channel, also within the same repository. The message goes from the producer to the handler, so
`trace` descends in that direction.

The broker is part of the identity (`channel::<broker>::<name>`), and the same name on two
brokers is two channels:

| Broker | Value in `<broker>` | Note |
| --- | --- | --- |
| Kafka | `kafka` | |
| RabbitMQ | `amqp` | exchanges, queues and bindings |
| Amazon SQS | `sqs` | |
| NATS | `nats` | wildcards `*` and `>` |
| Redis streams | `redis` | |
| BullMQ | `bullmq` | TypeScript and JavaScript only |
| JMS | `jms` | Java and Kotlin only (Spring JMS) |
| Azure Service Bus | `servicebus` | JS/TS, Python, Java, Go and C# |

The clients of each language are in section 9. Swift has no channel reading in 1.0.

Rules that apply to all of them:

- **Name computed like a URL.** Constants, arguments of functions that wrap the client and simple
  interpolations are folded. A name that cannot be computed (read from the environment, built at
  run time) is an **unresolved side**: contract `channel::kafka`, shown as `(unresolved)`,
  present in the export, never linked and never an open item. It is a known limit, and
  `meada decide` does not see it.
- **RabbitMQ.** A binding of a queue to an exchange (`bindQueue`, `QueueBind`, `queue_bind`) is a
  routing rule, not a side. A publish to a named exchange reaches the queues whose binding matches
  the routing key, coming from any repository in the workspace, on any exchange type: `exact` when
  the pattern is the key, `inferred` with a wildcard. A publish to the default (empty) exchange is
  a producer of the queue the key names.
- **NATS.** A consumer with `*` (one token) or `>` (the rest) links to the producers whose subject
  matches, at level `inferred`.
- **Clusters are not modelled.** A name on a broker kind is one channel across the whole
  workspace: topics of the same name in different clusters link as if they were in one cluster. If
  this produces a false link, that is the limitation, not a bug.
- **The handler** of a consumer is the function the subscription names.

### 8.5 Packages

A **package** links whoever declares it in a manifest (the provider) to whoever lists it as a
dependency (the consumer), and goes further: an `import` of a function of the package reaches the
provider's code, so the `context` of an exported function lists the callers from the whole
workspace and `trace` crosses the hop. Only packages that some repository in the workspace
provides enter; dependencies from outside the workspace appear nowhere, Meada queries no registry
and does not read lockfiles (`go.sum`, `Cargo.lock` and the like).

The reason of a package link carries the range as written and, where it applies, ends with
`range unchecked` (the range was not checked) or, on npm, `range mismatch` (the range rejects the
provider's local version). On npm, a range that accepts the local version carries no suffix. An
import that reaches the code adds `; import <what was used>` to the reason.

| Ecosystem | Manifest | Identity | Notes |
| --- | --- | --- | --- |
| npm | `package.json` with `name` | `package::npm::<name>` | Reads `dependencies`, `devDependencies`, `peerDependencies` and `optionalDependencies`. The range is checked (npm semver) |
| PyPI | `pyproject.toml` with a name in `[project]` or `[tool.poetry]` | `package::pypi::<name>` | Normalized by PEP 503. Also reads `requirements*.txt`. Ranges are not checked |
| Cargo | `Cargo.toml` with `[package] name` | `package::cargo::<name>` | Each member of the Cargo workspace is a provider. Ranges are not checked |
| Go | `go.mod` with `module` | `package::go::<module path>` | Reads `require` and `replace` to a local directory |
| Maven | `pom.xml` with literal coordinates, or a Gradle project with a literal `group` and name | `package::maven::<group>:<artifact>` | Each module is an artifact. Also reads `settings.gradle` and `settings.gradle.kts`. Ranges are not checked |
| NuGet | SDK-style `.csproj` | `package::nuget::<id>` | The id is the `PackageId`, else the `AssemblyName`, else the file name. Ranges are not checked |
| SwiftPM | `Package.swift` | `package::swiftpm::<name>` | The name is the literal of `Package(name:)`. Ranges are not checked |

Details per ecosystem:

- **npm.** An `import` enters the provider's code through `exports`, `types`, `module`, `main` and
  re-exports. If the entry points at a build's output, the `outDir` of `tsconfig.json` leads back
  to the `rootDir`. A package with the same name on npm and on PyPI does not link. A `workspace:`
  range is only valid inside the repository itself. `search` with `kind=package` reports name,
  repository, directory, version and consumers.
- **PyPI.** The `import` reaches the code as `exact` when the module matches the distribution's
  name, `inferred` when it is one of its top-level modules and `ambiguous` when several
  distributions expose it.
- **Cargo.** A dependency by `path` resolves the crate in that directory, in the same repository or
  in another one of the workspace (the path is resolved textually, without leaving the repository
  roots or following symbolic links); by `git`, the repository with the normalized URL; by
  version, the provider of the same name. `package =` gives the real name of a renamed dependency,
  and `workspace = true` reads `[workspace.dependencies]`. A `use` follows `[lib] name` or the
  renamed name and the `pub use` chains down to the public items.
- **Go.** A `replace` that leaves the repository is resolved textually, like Cargo's `path`, and a
  replacement by another module is left out. An `import` of `<module>/<directory>` reaches the
  exported functions, types and methods of the directory.
- **Maven and Gradle.** A dependency, also in Gradle's map notation (`group: 'g', name: 'a'`),
  links to the workspace artifact as `exact`, with `range unchecked`. A group or artifact written
  with a property or a variable links when the build declares the value as a literal, or when a
  `gradle.properties` (at the root, in the settings directory or in the module) defines it; a
  version catalog accessor (`libs.x`) is read from `gradle/libs.versions.toml` or the catalog the
  settings file names. Otherwise the link is `ambiguous`. An `import` of a class of the artifact reaches the provider's code
  through the files' `package` declaration, not through a path (in Kotlin, also that of a
  top-level or extension function): `exact` when a single provider declares the package and the
  type, `ambiguous` when several declare it or overloads with the same number of arguments
  remain, `inferred` when an ancestor of the class comes from outside the provider's repository.
  The reason ends with `; import package`.
- **NuGet.** A `PackageReference` that names a workspace package links as `exact`, with `Version`
  as the range, and a `ProjectReference` to a project of another repository of the workspace
  links to that project's package. A `using` of a namespace of a project the file depends on
  reaches its types: `exact` when a provider declares the namespace and the type, `ambiguous`
  when several declare it, `inferred` when an ancestor comes from outside the provider's
  repository; the reason ends with `; import namespace`. Meada queries no feed and does not read
  `Directory.Packages.props`.
- **SwiftPM.** A `.package(url:)` links to the workspace repository with the same remote
  identity, and a `.package(path:)` to the repository at that path. Only literal values count: a
  URL, a path or a computed name leaves the link `ambiguous`. An `import Module` of a module of a
  package the `Package.swift` depends on reaches the `public` and `open` declarations, chosen by
  name and by the argument labels; the reason ends with `; import module`. Platform modules
  (`Foundation`, `SwiftUI`) never become a link, and `Package.resolved` is not read.

When two providers serve the same name, the link is `ambiguous`, one per candidate, with the
reason starting with `<n> providers fit`.

### 8.6 CI: GitHub Actions

A **CI contract** links a CI file that others reuse to whoever calls it. The provider is a
reusable pipeline or an action, and the consumer is each CI job or CI step of the workspace whose
`uses` names it. The identity is `ci::<repository identity>/<path>`, without the Git ref: one
contract gathers every call to a file, whatever the refs.

**What is read.** A `.yml` or `.yaml` file directly under `.github/workflows/` that holds `jobs`
is a pipeline, and an `action.yml` or `action.yaml` that holds `runs` is an action, in any
folder. The file, each CI job and each CI step are nodes that `search` with `kind=ci` finds and
`context` explains. A pipeline is a provider when its `on` holds `workflow_call`, and an action
always is. Nothing is run and no ref is fetched: Meada reads the files of the checkout.

**The forms of `uses`.**

| `uses` | On | Links to |
| --- | --- | --- |
| `<owner>/<repo>/.github/workflows/<file>@<ref>` | CI job | the reusable pipeline of that repository of the workspace |
| `<owner>/<repo>[/<folder>]@<ref>` | CI step | the action of that folder, or of the root, of that repository of the workspace |
| `./.github/workflows/<file>` | CI job | the pipeline of its own repository, with the reason `own repository` |
| `./<folder>` | CI step | the action of its own repository, only after a default `actions/checkout` in the same CI job |
| `docker://<image>` | CI step | nothing: it holds no CI contract |

A `uses` writes no host, so the host is the one of the calling repository. A CI step's
`./<folder>` is read in the CI runner's workspace, which is the step's own repository only after
a checkout with no `repository` and no `path`. In a reusable pipeline, in a composite action or
without that checkout, the step stays unresolved: an open item with no candidate, which `search`
with `kind=unresolved` shows with its cause. The other forms and the limits are in
[Capabilities and limits](site/en/capabilities.md#github-actions).

**External files.** A `uses` whose repository is not in the workspace (`actions/checkout`, a
pipeline of another organization) holds no side: it is no orphan, no open item and no contract.

**The Git ref.** The link is `exact`, and its reason shows the ref as written and the commit
Meada read the provider at. `context` on the pipeline's path lists its callers:

```text
provides ci::github.com/acme/workflows/.github/workflows/publish.yml
  consumer: github.com/acme/app-a .github/workflows/ci.yml:12 jobs.publish [exact: @main (indexed 1a2b3c4)] with: package, registry; secrets: inherit
  consumer: github.com/acme/app-b .github/workflows/ci.yml:11 jobs["publish-prd"] [exact: @v1 (indexed 1a2b3c4)] with: package; secrets: token
```

Meada never resolves a ref: `exact` says the call names that file, not that it runs the indexed
version. When the ref is a full commit id other than the indexed commit, the reason ends with
`, ref mismatch`. `trace` with `direction=up` follows the chain through intermediate reusable
pipelines and composite actions, up to every calling CI job; the file's path, the file's node
(`<repository>::<path>::.`) and the contract start the same chain.

**Orphans and the absent mark.** `meada status` lists a reusable pipeline or an action that
nothing calls as an orphan, only when the visible scope holds at least one provider and one
consumer of the CI contract: a repository of actions indexed alone prints no orphan. A call whose
repository is in the workspace and whose path names no provider there carries the mark
` (absent at the indexed commit)` on the consumer's side, and `"absent": true` in `meada export`.
Absent is not broken: the file may exist at the ref the call names.

**Other CI systems.** Meada reads GitHub Actions. For a system it does not read (GitLab CI,
Jenkins, Azure Pipelines), declare the link by hand with `[[resource]]` and `via` on the key
that makes the call (section 4):

```toml
# in the repository of the shared pipeline
[[resource]]
kind = "pipeline"
id = "acme/ci-templates/publish"
role = "write"
via = ["templates/publish.yml#publish.script"]
```

```toml
# in each repository that calls it
[[resource]]
kind = "pipeline"
id = "acme/ci-templates/publish"
role = "read"
via = [".gitlab-ci.yml#include[0].file"]
```

The link is `declared`, with one side per key, no Git ref and no chain: a `trace` from the
pipeline stops at its direct callers, and a caller nobody declared stays invisible. The same
declaration on a key of a GitHub Actions pipeline still holds, beside the CI contract.

## 9. Languages

Meada 1.0 reads **TypeScript and JavaScript, Python, Go, Rust, Java, Kotlin, C# and Swift**,
nine languages. Each section below lists the files read, the frameworks and HTTP clients, the
environment reads, the manifests, the channel clients and the known limits. A library outside
these lists produces no link.

Rules that apply to all languages:

- Meada reads the text with tree-sitter. It runs no compiler, interpreter or package manager and
  opens no network, and it never reads the value of an environment variable.
- A call is `exact` when the code shows the receiver's type or the written path, and `inferred` or
  `ambiguous` when only the name decides (section 2, "How to read the levels").
- A library counts when the file imports it (JavaScript's global `fetch` is the exception).
- A decorator, an attribute or an annotation written on a definition is a call from the scope that
  encloses it. `context` lists them apart, under `annotations:`, and not among the callees.
- A URL or a channel name is folded: literals, concatenations, simple interpolations, constants
  (also from other files of the repository) and the arguments of functions that wrap the client,
  at any depth. What does not fold is an unresolved consumer (HTTP) or an unresolved side
  (channels).

The 1.0 stability promise covers all nine languages: it freezes the surface (tools, CLI,
configuration and export), and resolving better what a language finds is a fix, not a break.

### 9.1 TypeScript and JavaScript

**Files.** `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs` and `.cjs`. An `import` uses the `paths` and
`baseUrl` of the nearest `tsconfig.json` (or `jsconfig.json`), in the file's directory or above,
relative to that configuration file's directory. When it has an `extends` to another `tsconfig.json`
or `jsconfig.json` of the repository (a relative path), `paths` and `baseUrl` come from the chain, the
nearest config that declares each option winning; an `extends` to a package or outside the repository
is not read, and a chain that is too long or circular counts as the nearest config alone.
`node_modules` is ignored.

**What becomes a definition.** Functions, classes and methods, the `const`, `let` and `var` of a
function or class and `export default`. At the top of a file, assigned functions count too:
`obj.y = function () {}` defines `obj.y`, `X.prototype.y = function () {}` defines the method
`X.y`, `exports.y = ...` and `module.exports.y = ...` define the export `y`, and
`module.exports = function () {}` defines the default export. A `let` or `var` is only a
definition if the file never assigns the name again. The functions of the object literal of a
top-level `const` define `o.m`.

**HTTP.**

URLs built with `new URL(x)` or `new URL(ref, base)` are read only for the unassigned global
`URL` or the named `URL` export of `url` or `node:url` through an import or unshadowed
`require`. Unknown packages, repository aliases and other exports stay opaque.

`String(u)` respects local bindings of `String` and passes the URL through only when the call uses the global conversion. Assignments and increments that can replace that conversion leave the URL unresolved. A local binding preserves the global conversion outside its scope. Block functions in non-strict JavaScript also hide the conversion in the enclosing function; modules, classes and `use strict` preserve block scope.

| Role | Library |
| --- | --- |
| Consumer | global `fetch`, and a call to a parameter whose default is `fetch` |
| Provider | Next.js (app router: route files) and Express |

In Express, a route holds under each prefix where the router is mounted (`use`, `Router()`). The
`Router()` of `express-promise-router` (the default in Backstage backend plugins) counts like
`express.Router()`. A prefix or a path with an unreadable part (a template-literal hole, a parameter, an environment
read) generates no route, and a router mounted only under such a prefix is left with no prefix at
all. A literal `:id` is still a parameter. No client besides `fetch` is read: `axios`, `got` and
`XMLHttpRequest` generate no consumer.

In Backstage, the router a backend plugin gives its `httpRouter` (`httpRouter.use(r)`, with
`httpRouter` taken from `coreServices.httpRouter` in the `deps` of `registerInit`) holds under
`/api/<pluginId>`, the path where the framework mounts it, when `createBackendPlugin`, imported
from a `@backstage/` package, writes its `pluginId` as a literal or as a `const` of the same file
bound to one. A plugin whose `pluginId` cannot be read that way (an environment read, an import)
leaves its router at the root, as before, and `meada status` names the plugin. On the
consumer side, a URL that starts with `discovery.getBaseUrl('<id>')` (held by a constant or
written in the URL itself) starts at `/api/<id>`, and the link's reason names the plugin and that
mount; a hostless URL of a route that the mount of a plugin of the same repository serves names it
too.

Backstage's `proxy` plugin holds through the `proxy.endpoints` table of an `app-config*.yaml` of
the repository: a URL that starts with `/api/proxy/<endpoint>` (also the one written by
`discovery.getBaseUrl('proxy')` plus `/<endpoint>`) is a call to that endpoint's `target`, and the
rest of the URL follows the target. A `target` that is exactly `${VAR}` links through the peer's
`base_url_env`, and the reason names the variable and the endpoint; a `target` that is a literal
`http://` or `https://` URL counts as the host (with its path). Any other `target` (text around
the variable, a default value) leaves the call unlinked, and `meada status` names the endpoint and
how many calls go through it. A `pathRewrite` that only strips the endpoint (one rule,
`'^/api/proxy/<endpoint>': ''`, or `'^/api/proxy/<endpoint>/': '/'` for a URL with a path) is what
the proxy does anyway, so the rest of the URL goes to the target; any other `pathRewrite` (another
replacement, several rules) is read like a target that cannot be read: no link, and `meada status`
says so. `ignorePath`, `prependPath: false` and an endpoint the `app-config*.yaml` files give
different values (which one a deployment loads is not known) are read the same way. As Backstage's
proxy reads the configuration, the endpoints are those of `proxy.endpoints` and, when no file has
that key, the keys of `proxy` that start with `/` (the legacy shape, accepted from before Backstage
1.17); with both shapes, in one file or across files, only `endpoints` counts, and so does a
`proxy.endpoints`, a whole `proxy` or a whole `app-config` behind an `$include` Meada does not
follow (Backstage reads whatever it gives), so the legacy keys are not read then; an `$include`
whose `#` key path ends at an absent key does not count, as Backstage leaves that key out. An
endpoint written
as a string (`'/<endpoint>': https://...`) is `{ target }` with the default `pathRewrite`, in both
shapes, and the rules above apply to it. An endpoint inside a partial included through `$include`
counts as if it were in the file.

**Environment.** `process.env.X`, `process.env["X"]` and `import.meta.env.X`, alone or passing
through text methods such as `trim()`, `replace(...)` and `toLowerCase()`. A URL that starts with
such a read links through the peer's `base_url_env`. `.env` files are not read. So does
Backstage's configuration read, `config.getString('a.b')` or `config.getOptionalString('a.b')`,
when an `app-config*.yaml` of the repository holds `a.b: ${VAR}` (`app-config.yaml` first, the
first file with the key wins): the read is the variable `VAR`. A literal value or a key no file
holds leaves the base unresolved. A block moved to a partial file with
`{ $include: ./partials/a.yaml }` (or `./partials/a.yaml#a.b`, for its key `a.b` alone) and a
value `{ $env: VAR }` are read as if written in the file itself, as Backstage applies them: the
path starts from the including file's directory, only `.json`, `.yaml` and `.yml` files are read,
a partial may include another, from its own directory, and an `app-config` that is one
`$include` is the file it names. A partial is an input of the index:
editing it alone changes the answer on the next query. Not followed: an absolute path, one that
leaves the repository, a symbolic link, a `${` in the path, another extension, a file or key path
that does not exist, an object with another key beside `$include`, a cycle, more than 8 includes
deep, more than 64 includes from one `app-config` and more than 1 MiB of partials grafted into
it: the keys under that `$include` stay unread, and `meada status` names each with its reason.
`$file` is not read, and an `app-config` or partial over 1 MiB, or of more than one YAML
document (Backstage refuses it), is ignored.

**Manifests.** `package.json` (`name`, `version`, the four dependency fields, `exports`, `types`,
`module` and `main`) and `tsconfig.json` (`paths`, `baseUrl`, `outDir`, `rootDir`). See npm in
section 8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | `kafkajs` |
| RabbitMQ | `amqplib` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` |
| NATS | `nats` |
| Redis streams | `ioredis`, `node-redis` |
| BullMQ | `bullmq` |

**Data format.** It is the only language where Meada reads the format of a response from the code
itself, on both sides: what a `fetch` consumer reads from `.json()` and what a Next.js or Express
provider returns, through zod schemas or object literals. See section 10.

**Known limits.**

- `require` and `module.exports` are not imports between files, and `new F()` of a constructor
  function does not reach the methods of `F.prototype`.
- A `let` or `var` reassigned anywhere in the file does not count as a single definition.
- A module that `tsconfig.json` does not map and that lies outside the repository is external.
- Routes mounted by a value Meada does not read (a router coming from an outside call) are not
  served anywhere.

### 9.2 Python

**Files.** `.py`. Each `pyproject.toml` gives code roots (its directory, the roots it declares or
`src`) to the files below it, and it is through them that an `import` finds the module.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | `requests`, `httpx`, `aiohttp` |
| Provider | FastAPI, Flask, Django |

- **Consumers.** A module function (`requests.get`, `httpx.post`, also under an alias) or a method
  of a client that the code shows came from the library's constructor (`Session()`, `Client()`,
  `AsyncClient()`, `ClientSession()`), kept in a variable, in an attribute of `self`, opened by
  `with` or received as a parameter. The constructor's `base_url=` is the base of relative URLs. A
  `get` on any other object is nothing.
- **FastAPI and Flask.** Method decorators hold on an object that traces back to the framework's
  constructor (`FastAPI()`, `APIRouter()`, `Flask()`, `Blueprint()`). In FastAPI: `@app.get`,
  `@router.post` and the other methods, and `api_route`. In Flask: `@app.route(..., methods=[...])`
  (the `@app.get` shortcut generates no route). The prefix of `APIRouter(prefix=)`, of
  `include_router` and of `mount` is summed; in Flask, the `url_prefix` given to
  `register_blueprint` replaces that of the `Blueprint` itself. What nothing mounts is served at
  the root, except an `APIRouter` or `Blueprint`, which only serves where a mount that Meada reads
  puts it.
- **Django.** Meada starts from the root URLconf (a file with `urlpatterns` that no `include()`
  names) and descends through `path()`, `re_path()` and `include()`. A `re_path()` only holds with
  literal text and named groups (`(?P<name>...)`); any other pattern generates no route. A
  function view serves the methods that `require_http_methods`, `require_GET`, `require_POST` and
  `require_safe` allow, or all of them; `C.as_view()` serves the methods (`get`, `post`...) that
  `C` defines.

**Environment.** `os.environ["X"]`, `os.environ.get("X")` and `os.getenv("X")` (also bare
`environ` and `getenv`, matched as written and not through imports), alone or in an f-string hole,
with an `or` fallback, with a default argument and with text methods such as `strip` and
`replace`. A field `f` of a `BaseSettings` subclass (pydantic-settings) in the same file is the
variable `<env_prefix><F>` in upper case, or its `alias` / `validation_alias` when given; a class
that is not a `BaseSettings` is not read.

**Manifests.** `pyproject.toml` (`[project]` and `[tool.poetry]`, including `[project.scripts]`)
and `requirements*.txt`. See PyPI in section 8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | `confluent-kafka`, `kafka-python` |
| RabbitMQ | `pika` |
| Amazon SQS | `boto3` |
| NATS | `nats-py` |
| Redis streams | `redis-py` |

**Known limits.**

- A route decorator on an object that does not trace back to the framework's constructor generates
  no route: Meada does not guess by name.
- A prefix or a path with an unreadable part (an f-string hole, a parameter) generates no route.
  `str.format` and `%` do not fold a URL; f-strings and `+` do.
- A router mounted through an import that does not resolve is not presumed to sit at the root.
- In Django, without a determinable root URLconf (an `include()` that Meada does not read), no
  route is served.
- Python configuration files (`settings.py`, `.env`) are not read beyond the environment reads in
  the code.

### 9.3 Go

**Files.** `.go` included by the package's build rules. Each `go.mod`'s `module` resolves imports
to package directories, a `replace` to a directory of the repository included. A nested module
owns its subtree, and the files of a directory with the same `package` share scope (`_test.go`
too). Meada does not run Go's toolchain.

The analysis targets linux/amd64: `_windows.go` files, files with a false `//go:build` or that use
cgo (`import "C"`) are left out.

**What becomes a definition.** Functions, types, interfaces and methods (`Type.Method`). Calls
resolve within the package and through imports (`pkg.F()`, aliased or dot). The standard library
and external modules stay external.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | `net/http` (`Get`, `Head`, `Post`, `PostForm`, `NewRequest`, `NewRequestWithContext`), resty |
| Provider | `net/http`, gin, echo, chi, gorilla/mux |

A router is the call that creates it (`gin.Default()`, `echo.New()`, `chi.NewRouter()`,
`http.NewServeMux()`, `mux.NewRouter()`). Prefix groups (`Group(prefix)` of gin and echo,
`Route(prefix, fn)` and `Mount(prefix, router)` of chi, `PathPrefix(...).Subrouter()` of
gorilla/mux) sum the prefix, and gorilla/mux restricts methods with `.Methods(..)`. Each
framework's path parameters (`{name}`, `:name`, `*rest`) become `{}`. A resty client's base is the
argument of `SetBaseURL`.

**Environment.** `os.Getenv` and `os.LookupEnv`, including the pattern written with `if v == ""`,
`len(v) == 0`, `!ok` or `!ok || v == ""` when the `ok` comes from the same `os.LookupEnv`, and
then `strings.TrimSuffix`, `TrimRight` and `TrimSpace`. A field of a struct tagged `env:"X"`
(cleanenv, caarlos0/env) or `envconfig:"X"` (kelseyhightower/envconfig) is the read of `X`
where the field is read (`cfg.API.URL`), nested structs included, when the struct is declared in
the same file.

**Manifests.** `go.mod` (`module`, `require`, `replace`). `go.sum` is not read. See Go in section
8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` |
| RabbitMQ | `amqp091-go` |
| Amazon SQS | `aws-sdk-go-v2` |
| NATS | `nats.go` |
| Redis streams | `go-redis` |

**Known limits.**

- A `recv.M()` call is `exact` when the code gives the receiver's type (a parameter, field, `var`,
  composite literal or function result). Without that, name resolution is `inferred` or
  `ambiguous`.
- A call through an interface stops at the interface's method: Meada does not choose the
  implementation.
- A handler wrapped in a way Meada does not read (`http.StripPrefix`, `echo.WrapHandler`, a
  middleware) mounts the router where it does not reach, and its routes do not appear.
- echo's `Add(method, path, handler)` is not read.
- Code with cgo, `_windows.go` and a false `//go:build` is not indexed.

### 9.4 Rust

**Files.** `.rs` reachable through each crate's module tree. Every `Cargo.toml` with `[package]`
defines a crate, in any directory, and Meada follows the `mod` declarations without running
`cargo` or `rustc`. A file that no `mod` reaches is left out.

**What becomes a definition.** Functions, types, traits, modules and `impl` methods. Calls such as
`crate::a::f()`, `Type::new()` and `Self::f()` follow the written path; resolution also follows
`use`, groups, renames, globs, `pub use` and other workspace crates by name. In `x.m()`, the
receiver's type comes from a `let`, parameter, field, `self` or a written function return; a call
on the result of another (`Router::new().route(..)`, also through `.await` or parentheses)
resolves by the written return type of the inner call; a `dyn Trait` or a trait-bounded generic
reaches the trait's method. Without a written type, resolution is by name.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | `reqwest` (including `blocking`), `ureq` |
| Provider | axum, actix-web, Rocket |

In axum, `route`, `nest` and `merge` sum the prefixes along the `Router::new()` chain. In actix-web
and Rocket, the function with the route attribute (`#[get]`, `#[route(...)]`) or named by
`.route(path, web::get().to(h))` is the handler, and `web::scope`, `web::resource`,
`.configure(f)` and the Rocket instance mount prefixes. The body of `format!` and `concat!` folds a
URL when the arguments are literals or, in `format!`, simple names; any other argument becomes a
parameter (`concat!` only accepts literals). A `let mut` String that is only read and appended to
by `push_str`, `push` and `+=` also folds.

**Environment.** `std::env::var("X")`, `dotenvy::var("X")` and `env!("X")` (also as an argument of
`concat!` or `format!`), with `unwrap`, `expect`, `?`, `trim`, `to_string` and the like.
`option_env!` is not a read.

**Manifests.** `Cargo.toml` (`[package]`, `[dependencies]`, `[dev-dependencies]`,
`[build-dependencies]`, `[workspace.dependencies]`, `[lib] name`). See Cargo in section 8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | `rdkafka` |
| RabbitMQ | `lapin` |
| Amazon SQS | `aws-sdk-sqs` |
| NATS | `async-nats` |
| Redis streams | `redis` crate |

**Known limits.**

- **Calls inside macros are not indexed** (`assert!`, `format!`, `vec!`, `println!`). A caller that
  only calls from inside a macro does not appear, and `context` treats it as "unproven". The only
  exception is the braced body of an item macro of the repository itself (a `macro_rules!` whose
  first rule takes `$($x:item)*` and rewrites each item with outer attributes only, like tokio's
  `cfg_rt!`), invoked as `m!` or `crate::m!`: it is read as the items of the module, or of the
  `impl` or trait block the invocation sits in. Macros from other crates remain unread.
- Methods that are not in the type's `impl` blocks, including those generated by `derive` or
  supplied by external traits, produce no link.
- The analysis uses each crate's default configuration for x86_64 Linux: items under a false
  `#[cfg(..)]` and features outside `default` are left out; modules compiled in a test build
  enter, not just the `#[cfg(test)]` ones.
- An axum router handed to a call that Meada does not read (`route_service`,
  `fallback_service`, an outside wrapper) is not mounted anywhere.
- `option_env!` and an `env!` that the file itself defines are not reads.

### 9.5 Java

**Files.** `.java`, resolved by the `package` declarations. Meada needs no JDK, Maven or Gradle
and runs no code from the repository.

**What becomes a definition.** Classes, interfaces, enums, records, methods and constructors,
nested ones included. Names carry the package and, for methods, the written parameter types
(`example.Orders.place(String,int)`). The calls of a lambda or an anonymous class belong to the
symbol that contains them. Public types and their public or protected members form the exported
surface. Single, package, static and `*` imports resolve through the `package` declarations. A
call with no receiver, with `this` or static is `exact` when one method remains by argument count
(`varargs` included), and overloads with the same count are `ambiguous`. On a field, parameter or
local variable with a written type, the call resolves in that type and in the supertypes of
`extends` and `implements`. A supertype from outside the repository has unknown members: the call
resolves by name, unless it is a platform type with known members (`Serializable`, `Comparable`
and the like).

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | RestTemplate, WebClient, `java.net.http`, OkHttp, OpenFeign, Retrofit |
| Provider | Spring MVC, JAX-RS |

- **Providers.** A method of a `@RestController` or `@Controller` class with `@GetMapping`,
  `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` or `@RequestMapping(method =
  ...)` is a provider, under the class's `@RequestMapping` prefix; a `@RequestMapping` with no
  `method` serves every method (`http::ANY::`). In JAX-RS, a method with `@GET`, `@POST` and the
  others in a class with `@Path` is a provider. `static final` constants fold into the path.
  `server.servlet.context-path` is not applied.
- **Consumers.** The RestTemplate methods, the `.uri(...)` of a WebClient request, and the
  `build()` of an `HttpRequest.newBuilder()` or an OkHttp `Request.Builder()` (with no method in
  the chain, `GET`). The client counts by the type the code writes for it or by the call that
  creates it, and a WebClient's base comes from the `WebClient.create(base)` or `baseUrl(base)`
  it comes from. A method that builds the URL from its parameters is a wrapper.
- **Declarative clients.** In a `@FeignClient` or Retrofit interface (`@GET("x")` and the
  others), each method is a consumer, at the interface method itself. The base comes from `url`
  and `path` in Feign (with `${key}` placeholders from the Spring configuration) and from the
  `Retrofit.Builder().baseUrl(base)` that creates the interface.

**Environment.** `System.getenv("X")` and Spring's `@Value("${key}")` on a field or constructor
parameter, with the key read from `application.properties`, `application.yml` or
`application.yaml` in the module's `src/main/resources`. A `${VAR}` placeholder that no key fills
is the variable `VAR`, which the peer's `base_url_env` maps. Profiles (`application-<profile>`)
and SpEL are not read.

**Manifests.** `pom.xml` and the Gradle build files (`build.gradle`, `build.gradle.kts`,
`settings.gradle`, `settings.gradle.kts`, `gradle.properties` and the `*.versions.toml` version
catalogs). See Maven and Gradle in section 8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | Spring Kafka |
| RabbitMQ | Spring AMQP |
| Amazon SQS | AWS SDK v2 |
| NATS | jnats |
| Redis streams | Jedis, Lettuce |
| JMS | Spring JMS |

Outside the AWS SDKs, the client counts by the type the code writes for it, and a listener by the Spring annotation
(`@KafkaListener`, `@RabbitListener`, `@JmsListener`). The channel name folds like a URL, with the
`${key}` of the annotations read from the Spring configuration.

**Known limits.**

- A repository type with the name of an annotation or a client of the library hides it.
- Feign's `@RequestLine`, Retrofit's `@HTTP` and an annotation inherited from an interface are not
  read.
- A channel name that cannot be computed (an environment variable, SpEL) is an unresolved side.
- A file whose repeated enclosing names exceed 4 MiB in total is left with no symbols or calls,
  and `meada status` names it.

### 9.6 Kotlin

**Files.** `.kt` and `.kts` (except `build.gradle.kts` and `settings.gradle.kts`), resolved by the
`package` declarations. Meada needs no Kotlin compiler, JDK or Gradle.

**What becomes a definition.** Classes (also `data`, `sealed`, `enum` and `annotation`),
interfaces, objects, companions, top-level functions, extension functions, properties with a
getter and methods, nested ones included. Names carry the package and, for functions, the written
parameter types (`example.helper(Int)`); an extension function carries the receiver type
(`example.Point.norm()`). The calls of a lambda belong to the symbol that contains it.
Declarations that are neither `private` nor `internal` form the exported surface. Single, `as`
and `*` imports resolve through the `package` declarations, and the file's own package is visible
without an import. Calling a class name constructs it. Kotlin and Java in the same repository call
each other by fully qualified name. Inside a lambda, a call with no receiver may belong to an
implicit receiver, and resolves by name only.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | RestTemplate, WebClient, OkHttp, Retrofit, Ktor client |
| Provider | Spring MVC, Ktor |

- **Spring, OkHttp and Retrofit.** They follow Java's rules (section 9.5), written the Kotlin
  way: `RestTemplate()` without `new`, named arguments and `Api::class.java` as the Retrofit
  service.
- **Ktor, provider.** A route inside `routing { }` (`get`, `post`, `put`, `patch`, `delete`,
  `head`, `options` and `route(path, HttpMethod.X)`) is a provider under the prefix of each
  enclosing `route(prefix) { }` block, also in an extension function of `Route` or `Routing`
  called inside the block. The route's lambda is the handler. Ktor names count only with the Ktor
  import, and typed routing (resource classes) is not read.
- **Ktor, consumer.** A call (`get`, `post` and the others, or `request(url) { method =
  HttpMethod.X }`; with no method, `GET`) on an `HttpClient`. A `defaultRequest { url(base) }` is
  the base of relative URLs. `url { path(..) }` is not read.

**Environment.** `System.getenv("X")` and `System.getenv()["X"]`, also through `?:`, `!!`,
parentheses and string methods such as `trim` and `removeSuffix`, and Spring's `@Value("\${key}")`
(the `$` escaped), with Java's configuration. The read may be in a property, in a local `val` or in
the URL itself; a `var` is not followed.

**Manifests.** Java's: see Maven and Gradle in section 8.5.

**Channels.** The same brokers and clients as Java (section 9.5), written the Kotlin way: named
arguments and arrays (`@KafkaListener(topics = ["a", "b"])`), a top-level `const val` or one in a
`companion object`, and a client received through the constructor.

**Known limits.**

- A call on an implicit receiver (a client extension function, `apply` and `with` blocks) is not
  read in channels.
- In a `request` block, a method assignment that may not run leaves the method unknown, never
  `GET`.
- A channel name with a part that cannot be evaluated (`"orders-$env"`) is an unresolved side.

### 9.7 C#

**Files.** `.cs`, grouped into projects by SDK-style `.csproj` files (with `Sdk`; an old
`.csproj` is not a project, and `.sln` files change nothing) and resolved by the `namespace`
declarations. The `obj/` and `bin/` folders next to a `.csproj` stay out of the index. Meada needs
no .NET SDK or MSBuild.

**What becomes a definition.** Classes, records, structs and interfaces, nested and `partial`
ones included, methods and constructors with the written parameter types
(`Acme.Orders.OrderService.Place(string,int)`) and local functions. Public types and their public
or protected members form the exported surface, and an `internal` type exists only for the files
of its project. `using`, `using static`, aliases and `global using` resolve through the namespaces
of all projects; `global using`, the `<Using>` items of the `.csproj` and implicit usings apply
to the files of the project. A call with no receiver, with `this`, `base`, static or through
`using static` is `exact` when one method remains by argument count (`params` and default values
included). A base from outside the repository leaves calls to the class's members without
`exact`, except for a type with known members (`IDisposable`, `IEquatable` and the like). An
attribute is a call on what it decorates.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | `HttpClient`, typed clients, Refit |
| Provider | ASP.NET Core (controllers and minimal APIs) |

- **Controllers.** A method of a controller class (with `[ApiController]`, or deriving from
  `ControllerBase` or `Controller`) with `[HttpGet]`, `[HttpPost]` and the others is a provider,
  under the class's `[Route]`; `[controller]`, `[action]` and `{id:int}` are understood.
  Conventional routing (`MapControllerRoute`) is not read.
- **Minimal APIs.** `MapGet`, `MapPost`, `MapPut`, `MapPatch`, `MapDelete` and `MapMethods` on the
  application or on a `MapGroup(prefix)`, with nested groups; the prefix follows the group through
  variables, arguments, returns and extension methods. A group Meada does not read (a lambda
  parameter, a field, a reassigned variable) yields no route.
- **Consumers.** `GetAsync`, `PostAsync`, `GetFromJsonAsync`, `SendAsync(new
  HttpRequestMessage(HttpMethod.X, url))` and the like on an `HttpClient`, by the declared type
  or by the construction, never by the name alone. The URL folds literals, interpolation, `+` and
  `const` and `static readonly` fields. A `BaseAddress = new Uri(base)` is the base of relative
  URLs, and a typed client registered with `AddHttpClient<T>(c => c.BaseAddress = ...)` gives the
  base to the `HttpClient` of `T`. Each method of a Refit interface (`[Get("/x")]` and the others)
  is a consumer, with the base of the `RestService.For<T>(url)` that creates it.

**Environment.** `Environment.GetEnvironmentVariable("X")`, through `??`, `!` and `Trim`, and
`configuration["Section:Key"]` on an `IConfiguration` or on `builder.Configuration`, which is the
variable `Section__Key`. A literal URL in the project's `appsettings.json` is the default of that
variable. `appsettings.<Environment>.json`, `GetValue`, `GetSection`, options binding and user
secrets are not read.

**Manifests.** SDK-style `.csproj` and `Directory.Build.props`. See NuGet in section 8.5.

**Channels.**

| Broker | Library |
| --- | --- |
| Kafka | `Confluent.Kafka` |
| RabbitMQ | `RabbitMQ.Client` |
| Amazon SQS | `AWSSDK.SQS` |
| NATS | `NATS.Client` |
| Redis streams | `StackExchange.Redis` |
| Azure Service Bus | `Azure.Messaging.ServiceBus` |

AWS clients require constructor origin, except SQS's declared `IAmazonSQS` and
`AmazonSQSClient`, as described in the AWS SDK section. Other clients count by their written
type or the construction that created them, resolved through the `using`s, never by the name
alone.

**Known limits.**

- A controller that inherits from another class of the repository, a class that is the base of
  another and a `partial` class spread over several files yield no route.
- `IHttpClientFactory.CreateClient(name)` is not read, and a Refit interface registered with
  `AddRefitClient` has an unknown base.
- MassTransit, `BasicGet`, `SubscribeSync` and NATS.Net v2 are not read in channels.
- A channel name read from the environment or from `IConfiguration` is an unresolved side.

### 9.8 Swift

**Files.** `.swift` (except `Package.swift` and `Package@swift-<version>.swift`), grouped into
modules by the targets of `Package.swift`; the files outside any target form a single module.
Meada needs no Swift toolchain or Xcode.

**What becomes a definition.** Classes, structs, enums, actors and protocols, nested ones
included, and functions, methods and initializers with the argument labels in the name
(`OrderService.place(order:quantity:)`). The members of an extension belong to the extended type.
The calls of a closure belong to the symbol that contains it. Names resolve first in the file's
module and then in the modules it imports, through the `public` and `open` declarations (and
`internal` with `@testable`). A call is `exact` when one declaration remains by the labels, and
overloads with the same labels are `ambiguous`. A receiver whose type comes from outside the
repository (`session: URLSession`) reaches only the extensions of that type that the repository
writes, and a call through a protocol-typed receiver stops at the requirement.

**HTTP.**

| Role | Library |
| --- | --- |
| Consumer | `URLSession`, Alamofire, AsyncHTTPClient |
| Provider | Vapor, Hummingbird |

- **Vapor.** A route (`get`, `post`, `put`, `patch`, `delete` and `on(.METHOD, path)`) called on
  the application or on a `grouped(prefix)` of it is a provider under the prefix of each
  enclosing `grouped`. A `RouteCollection` registered with `register(collection:)` serves the
  routes of its `boot(routes:)`. Names count only with the `Vapor` import.
- **Hummingbird.** A route (`get`, `post`, `put`, `patch`, `delete` and `on(path, method: .get)`)
  called on a `Router()` or on a `group(prefix)` of it is a provider under the prefix of each
  enclosing `group`. Names count only with the `Hummingbird` import.
- **Consumers.** `data(from:)` and `data(for:)` on a `URLSession`, `AF.request(url, method:)` and
  `execute` of an `HTTPClientRequest(url:)`. The method is GET when not written, and the
  `httpMethod` assigned to a `URLRequest` counts. The URL folds `URL(string:)`,
  `appendingPathComponent`, literals, interpolation, `+` and `let` constants.

**Environment.** `ProcessInfo.processInfo.environment["X"]`, through `!`, parentheses, `??` with a
plain string literal and string methods such as `trimmingCharacters` and `lowercased`. The read
may be stored in a `let`. A reassigned `var` is not followed, and Vapor's `Environment.get("X")`
is not a read.

**Manifests.** `Package.swift`. See SwiftPM in section 8.5.

**Channels.** Swift has no channel reading in 1.0.

**Known limits.**

- `dataTask(with:)` and a Hummingbird controller's `addRoutes` are not read.
- A middleware built in `grouped(..)` adds no path, and a path Meada does not read yields no
  route.
- A module that is not a target of the repository is external.
- All the dependencies of the nearest `Package.swift` apply to the files below it, without
  separating what each target declares.

## 10. Shape drift

A contract's **shape** is what it carries, recorded as a set of field paths (`items[].id`;
`[].id` when the body is a list). **Drift** is a field that a consumer reads and that the shape of
the provider linked to it does not contain: a field the route stopped returning. A link may stand
and still break, and drift is the warning for that.

Where each shape comes from:

| Side | Source | Field level |
| --- | --- | --- |
| Provider | OpenAPI document (any language) | `exact` |
| Provider | zod schema that an Express or Next.js handler's response goes through | `exact` |
| Provider | Object literals that an Express or Next.js handler returns | `inferred` |
| Consumer | Declared type or schema (`z.infer`) of the body read | `exact` |
| Consumer | Fields read from the `.json()` of a `fetch` | `inferred` |

Each missing path carries its level: `exact` when the provider's shape and the consumer's path are
both declared (document, schema, type), and `inferred` when one of the two comes from what the
code does (field reads, returned literals). The link's level does not change because of drift.
Today only TypeScript and JavaScript code generates a consumer's shape, and only `fetch`
consumers and Next.js and Express providers generate a provider's from code; providers in other
languages enter through the document.

How Meada shows drift:

| Query | Result |
| --- | --- |
| `trace` | Adds `drift: missing <path>` to the link's line |
| `context` on a route | Shows the shape's origin, the consumers, the drift, the level and the unchecked reads |
| `search` with `kind=drift` | Lists the drifts, one per missing path |
| `meada status` | Counts the missing reads, under the consumer's repository; the provider gets no count |

Example of a `trace` line:

```text
example.com/acme/api src/orders.ts:12 GET /api/orders
  →[inferred: base URL from API_URL, a base_url_env of peer example.com/acme/api] example.com/acme/web src/orders.ts:8 loadOrders drift: missing discount
```

(Illustrative example in `trace` format: `web` reads `discount` from the response of `GET
/api/orders`, and `api` does not return it.)

The limits of this analysis, so you do not mistake silence for absence of drift:

- **Presence only, never types.** Meada compares the presence of paths. It does not compare types,
  formats, `required` or `nullable`.
- **Open parts are not checked.** An object that accepts extra properties
  (`additionalProperties`, `.passthrough()`, `z.record`, `z.any`), a schema behind an external
  reference and a body that Meada cannot list leave the part **open**: everything below it may
  exist, and the read appears under "unchecked" instead of as drift.
- **Nothing below a scalar.** `name.length` is not a field, and `list.length` below an array is not
  drift.
- **Only the shallowest path.** Of each branch, only the first missing path is listed.
- **Ambiguous links have no drift.** Without a single provider, there is no shape to compare.
- **The handler must respond in a way Meada reads.** A body that comes from a value Meada does not
  follow (an outside call, a spread of something unknown) opens that path. Only 2xx responses
  count; a response with a literal status outside 2xx is ignored.
- **Without a document or schema, the shape is only what the code shows.** Literals and reads are
  a guess at what the code returns, hence `inferred`.

## 11. Communities and processes

Two analyses give a map of the code beyond point-to-point links. Neither uses a language model,
and both come out the same on any machine.

### Communities

A **community** is a group of symbols in a repository that call each other more than they call the
rest. Meada finds them from the calls of level `exact` or `inferred`, with the Leiden algorithm,
with no randomness and with integer arithmetic. The name comes from the directory with the most
members (`src`, and `src#2` when another community in the repository already has that name). Every
symbol that is in at least one of those calls, as caller or as callee, is a member of some
community; a symbol whose only call is to itself, or that is only called from the top of a file,
forms a community of its own; a symbol outside every call belongs to none.

- `search` with `kind=community` finds communities by name.
- `context` of a symbol states its community, or `community: none`.
- `meada export` includes the communities.

### Processes

An **entry point** is a door into a repository. Each one has its own process; no score or
threshold chooses which ones count.

| Kind | What it is |
| --- | --- |
| `route` | Each HTTP route the repository serves; the handler is the function |
| `channel` | Each channel that a handler consumes |
| `program` | A program start that the language or the manifest declares |
| `export` | Each exported function or method that nothing calls in the repository itself |

Program starts, per language:

- **Python:** `if __name__ == "__main__":` (symbol `__main__`) and the functions of
  `[project.scripts]` in the `module:function` form. The exported names are the public ones,
  without a leading `_`, or those of a literal `__all__`.
- **Rust:** the `fn main` of each binary, in `src/main.rs`, `src/bin/` or `[[bin]] path`.
- **Go:** `func main` in a `package main`.
- **Java:** `public static void main` with a `String[]` or `String...` parameter.
- **Kotlin:** a top-level `fun main`.
- **C#:** a `static Main` (any signature) and, in a file with top-level statements, the symbol
  `Main`.
- **Swift:** a type marked `@main` (through its `main()` when it declares one) and the top-level
  statements of a `main.swift`, in the symbol `main`.
- **TypeScript and JavaScript:** there is no `program` kind; the doors are the routes, the channel
  handlers and the exported functions that nobody calls.

A **process** is the call tree that an entry point sets in motion: the calls within the repository
at all levels and the links to other repositories, down to the leaves. It ends at an ambiguous
call or link (a step that lists the candidates), at an unresolved consumer, at a cycle or at a
missing peer. The process is not stored: it is walked at query time, from the calls and links
already stored.

- `trace` with `process::<entry point identity>` reads the tree. The identity is the handler's,
  plus the contract for a route or a channel, for example
  `process::example.com/acme/worker::src/consumer.ts::onOrderCreated::channel::kafka::orders.created`.
- `search` with `kind=process` finds processes by the entry point's label (`GET /users/{}` or the
  qualified name).
- `context` of a symbol lists the processes it is part of, walking the calls and links toward the
  callers.
- `meada export` includes each process.

### Duplicates

`search` with `kind=duplicate` lists the symbols whose text is identical in two or more places of
the workspace (in different repositories or in the same one), in groups ordered by size.
The export carries each symbol's BLAKE3 `hash`. Near copies (a whitespace difference) are not
duplicates.

## Where to go next

- [Capabilities and limits](https://claudiomascaro.github.io/meada/en/capabilities/): the detailed
  state of each language and contract.
- [Stability](https://claudiomascaro.github.io/meada/en/stability/): what 1.x freezes.
- [Server mode](https://claudiomascaro.github.io/meada/en/server/): running Meada for a team.
- A wrong result on Meada's own code is a bug: open an issue with the query, the answer and the
  expected one.

## AWS SDK calls in code

For resource readers, tracing up reaches their writers; tracing down from a writer reaches the reader and continues through its function body. A call side keeps its identity while the answer shows its location and owning function. This also covers calls with no writer and process membership.

TypeScript and JavaScript recognize v3 commands sent with `client.send(new Command(...))`,
with both client and command imported from the corresponding AWS package. Python recognizes
methods on clients constructed by `boto3.client(...)` or a `Session`. Names can come from
constants and arguments of exactly resolved wrappers. A client reached only by a name match,
or hidden by a parameter, `catch`, destructuring or another binding, contributes no side.

| Service | Produces or writes | Reads | Name argument |
| --- | --- | --- | --- |
| SNS | `Publish`, `PublishBatch` | | `TopicArn` |
| EventBridge | `PutEvents` | | `Entries[].EventBusName`, `default` when omitted |
| Kinesis | `PutRecord`, `PutRecords` | | `StreamARN` or `StreamName` |
| S3 | `PutObject`, `DeleteObject` | `GetObject`, `HeadObject`, `ListObjects`, `ListObjectsV2` | `Bucket` |
| DynamoDB | `PutItem`, `UpdateItem`, `DeleteItem`, `BatchWriteItem`, `TransactWriteItems` | `GetItem`, `Query`, `Scan`, `BatchGetItem`, `TransactGetItems` | `TableName`, `RequestItems` keys, or `TransactItems` tables |
| SSM | `PutParameter`, `DeleteParameter`, `DeleteParameters` | `GetParameter`, `GetParameters`, `GetParametersByPath` | `Name`, `Names`, `Path` |
| Secrets Manager | `PutSecretValue`, `UpdateSecret`, `CreateSecret`, `DeleteSecret` | `GetSecretValue`, `BatchGetSecretValue` | `SecretId`, `SecretIdList`, `Name` (`CreateSecret`) |

In JavaScript each operation above has the `Command` suffix; boto3 uses snake case
(`put_object`, `get_secret_value`). SNS, EventBridge and Kinesis are producers; S3, DynamoDB,
SSM and Secrets Manager use resource writer and reader sides. Literal names and supported ARNs join infrastructure by
physical name, without account or region. Literal SDK sides link at `exact`, including other
SDK sides in the same repository; a side declared in `meada.toml` keeps `declared`.
Overridable infrastructure values still weaken a link to `inferred`.

Without a matching workload environment, a read such as `process.env.BUCKET` or `os.getenv("BUCKET")` stays unresolved, with
`via: "env:BUCKET"` in the export and the variable named in its function's context. An SDK
producer or writer also has `via`, optional in format 1; a literal uses `sdk:aws`.
Meada never reads the process environment. Constructing an unused command, SNS publishes to
phone numbers or endpoints, high-level `boto3.resource` and `DynamoDBDocumentClient` clients,
and operations outside this table are not recognized by these forms.

`GetParametersByPath` queries a hierarchy, so its side stays unresolved; the path is not treated as a parameter name. A `ConditionCheck` in `TransactWriteItems` is a reader, even when the transaction also writes other tables. Computed `RequestItems` keys retain a literal or the environment variable name.

### Partially resolved names

A channel, resource or image with unknown parts retains a pattern such as `*-orders`.
`${terraform.workspace}-orders` matches `prod-orders` at `inferred`: at least three literal
letters or digits are required (separators do not count), and each `*` must occupy whole segments
bounded by `-`, `_`, `.` or `/`. A pattern never matches another pattern. Several matching literal
names stay `ambiguous`, all listed, and become open items. `meada decide` records the choice in
`meada.decisions.toml`; the chosen link becomes `decided`. Provisioning decisions belong to the side
containing the pattern and retain its evaluation root.

Configuration-key items use file, line, key and contract metadata, including when the value is redacted or cut.

An image builder pattern with several literal runner names also keeps its own open item in the repository that builds the image.

## Locations of declared resources

A `[[resource]]` `via` accepts symbols (`src/orders.ts::cancelOrder`), IaC addresses and
`<file>#<key path>`. The side belongs to the block or key, reachable through `context` and
`trace`, and its links stay `declared`. Addresses include Terraform/OpenTofu
(`aws_sqs_queue.orders`), Kubernetes/Crossplane (`Deployment.apps/shop/api`), Compose
(`services.api`), CloudFormation/SAM (`template.yaml::Orders`) and Serverless
(`serverless.yml::functions.api`). Use `<file>::<address>` to distinguish files. The location identifies the source block;
the ` (root ...)` qualifier of IaC queries is not a `via` form.
An address matching several source blocks needs the file qualifier; without it the `via`
is stale.

Key paths use dots, indexes (`groups[0].name`) and JSON-quoted keys
(`metadata.labels["app.kubernetes.io/name"]`). In lists, `[key=value]` selects items by exact,
complete value equality, for example
`values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support`.
Locations come from indexed values and their parent key paths.
Null values and empty containers have no indexed value; their `via` is stale.
A selector can match several items; each matching key gets a side. Redacted or cut values
cannot select an item. HCL expressions and templates cannot satisfy selector equality. Files follow the index's rules, including `.meadaignore`; a `via`
never opens a file on its own.

An unmatched `via` is stale and appears in the existing `meada status` lines. With no valid
`via`, the side falls back to the whole repository. This connects labels in Helm values to
routing rules through declarations without rendering templates.

```toml
[[resource]]
kind = "routing"
id = "team-b"
role = "write"
via = ["values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support"]
```

## AWS functions and tasks

Terraform/OpenTofu Lambda runs `image_uri`; SAM runs `ImageUri`, and ECS tasks run each
container image in `container_definitions`, including `jsonencode` expressions. These runners
participate in image contracts, context, traces and orphans. Environment entries stay with
their container and evaluation root, without reaching code belonging to another image.

A local Lambda `filename` or SAM `CodeUri` directly connects code in the same repository.
An exact `data.archive_file.<name>.output_path` reference may follow only its explicit local
`source_dir` or `source_file` input. ZIP files remain opaque locations; their contents are
never extracted. A Serverless handler such as `worker/send.dispatch` selects one existing
local source file (`.ts`, `.js`, `.mjs`, `.cjs` or `.py`); ambiguous extensions remain unread.
New paths must be relative, contain no `..` component and cross no symbolic link. S3, URLs
and remote code do not become local paths.

SAM merges `Globals.Function.Environment.Variables` with the function environment; Serverless
merges `provider.environment` with `functions.<name>.environment`. The function wins, even
when its override is unreadable. Terraform Lambda and ECS also supply their environments.
Known resource references may carry the contract identity without inventing a deployment ARN
or URL. Secrets, dynamic references and external values retain no contents. The code read
resolves at `inferred`, with workload and variable in its reason; `context` and `trace` show
both image execution and direct code paths.

References such as `aws_sqs_queue.orders.url` and `aws_s3_bucket.assets.bucket` identify
the contract without supplying a deployment URL; content attributes such as `value` stay unread.

## Workload environments

An environment read naming a queue, topic or resource, such as `process.env.QUEUE_URL`,
resolves only against entries of the workload running that code. Execution evidence comes
from the image the repository publishes or a checked local workload code path. Containers keep
their own environments. Resolution is `inferred`, with the workload and variable in its reason.

Kubernetes supplies literals, ConfigMap or Secret key references and `envFrom`; Compose supplies
map or list entries and defaults such as `${QUEUE_URL:-orders}`. Workload `context` shows
names, targets and available values. A Secret stays `redacted`, and a missing reference or
unresolved value creates no link. Names match only within the workload's scope: values come
from the repositories' IaC and configuration, never from a process environment.

## Alerts and Alertmanager routes

The `alert::<key>::<value>` contract joins labels on Prometheus and Loki rules
(including `PrometheusRule`) and Grafana rules to Alertmanager route matchers, in
configuration files and `AlertmanagerConfig` objects. Equality creates an `exact` link;
a regular expression creates an `inferred` link. A route's conditions are checked together.

`search` with kind `alert` finds the contract; `context` on a route shows its receiver,
and `trace` walks from a rule to its route and back. `meada export` includes emitters,
matchers and links. `meada status` lists orphans only when the visible scope has both
sides; otherwise `search` and `context` mark the missing side. Helm values awaiting
template rendering are not emitters: Meada does not render templates.

```json
{"tool":"search","arguments":{"query":"alert::support::team-b","kind":"alert"}}
{"tool":"context","arguments":{"symbol":"alertmanager.yaml::route.routes[0]"}}
{"tool":"trace","arguments":{"from":"rules.yaml::groups[0].rules[0]","direction":"down"}}
```

Grafana rules also come from JSON exports and `rule` blocks of Terraform/OpenTofu `grafana_rule_group` resources with literal labels.

A root route without matchers creates no alert side: there is no key and value for a contract. The graph describes written matchers, without predicting final notifications, inhibition or schedules.

Prometheus and Loki also emit the literal `alertname` from the rule name, overriding a label of that name. Labels that may come from the expression remain unknown, not empty. Files under `templates/` are not rendered or treated as alert emitters.

### OpsGenie and PagerDuty in Terraform

Literal `key:value` tags on `datadog_monitor` are emitters. Tag conditions in
`opsgenie_team_routing_rule` and PagerDuty
orchestration PCL conditions, are matchers. Equality is `exact`; regex, prefix and
containment are `inferred`. Conditions and rule order are evaluated together; unknown
values or unread operations do not choose a destination by guess.

Datadog tags reach OpsGenie. Rule labels reach tags only through a selected Alertmanager
OpsGenie receiver that sets `tags` explicitly. Simple `.CommonLabels.<key>` or
`.GroupLabels.<key>` templates use labels fixed by the effective `group_by`. A PagerDuty
v2 receiver can forward these labels through `details`, which becomes
`event.custom_details.<key>`; there is no implicit tag forwarding to PagerDuty. Unread
templates and missing forwardings create no links.

`context` shows the rule's written scope, and `trace` retains the path to its routing
block. A compatible matcher does not establish delivery or a final team notification.
In public example #638 the rule tests `service:app`: the same monitor also emits
`support:team-b`, but there is no `support` matcher there. The original nonstandard
monitor YAML and Helm values remain configuration only.

## Azure infrastructure

Terraform/OpenTofu provision Service Bus queues, topics and subscriptions, Event Hubs,
Storage accounts (`azurestorage`), Blob containers (`azureblob`) and Key Vault secrets
(`keyvault`). A subscription is `channel::servicebus::subscription:orders/audit`, distinct
from the legal queue `orders/subscriptions/audit`. Declared subscription rules weaken
delivery to `inferred`; no rule gives `exact`. Existing C# processors keep topic identity.

A complete supported ARM ID or blob/secret URL is an `exact` use. Final keys
`servicebus_queue`, `servicebus_topic`, `servicebus_subscription`, `eventhubs_hub`,
`azurestorage_account`, `azureblob_container` and `keyvault_secret` give `inferred` uses;
a subscription value is `topic/subscription`. Generic `queue`, `topic`, `bucket` and
`table` retain their AWS meanings. Account, namespace, vault, version and object selectors
are discarded only after validating the complete identifier.

Linux Functions read `site_config.application_stack.docker` fields `registry_url`,
`image_name` and `image_tag`. Both Linux and Windows Functions read `zip_deploy_file` and
`app_settings`; only a safe existing local archive gives direct execution, without opening
the ZIP. Missing, remote, parent-step and symlink paths give no code. App settings resolve
reads only in code run by the corresponding image or path, at `inferred`. Secret references
and unknown values stay unresolved. See [capabilities](site/en/capabilities.md#azure-contracts-and-functions)
for the types and limits; `context`, `trace`, export and orphans follow the existing
provisioning rules.

### Azure SDK use in five language groups

Known Azure SDK constructors create the operation sides below. JavaScript and TypeScript share one group and are checked separately. Java uses the listed `com.azure` builders; JavaScript uses the `@azure/service-bus`, `@azure/event-hubs`, `@azure/storage-blob` and `@azure/keyvault-secrets` constructors. Python uses `azure.servicebus`, `azure.eventhub`, `azure.storage.blob` and `azure.keyvault.secrets`, including their `aio` counterparts and the first three services' `from_connection_string` factories. Go uses the corresponding `github.com/Azure/azure-sdk-for-go/sdk` packages: Service Bus and Event Hubs also accept their `FromConnectionString` factories; Blob accepts `NewClient`, `NewClientWithNoCredential`, `NewClientWithSharedKeyCredential` and `NewClientFromConnectionString`. .NET uses `Azure.Messaging.ServiceBus`, `Azure.Messaging.EventHubs.Producer` / `.Consumer`, `Azure.Storage.Blobs` and `Azure.Security.KeyVault.Secrets`. JS Blob also accepts `BlobServiceClient.fromConnectionString`.

| Group | Service Bus producer / consumer | Event Hubs producer / consumer | Blob writer / reader | Key Vault reader |
| --- | --- | --- | --- | --- |
| JavaScript + TypeScript | `createSender(name).sendMessages` / `createReceiver(queue)` or `(topic, subscription)`, then `receiveMessages` or `subscribe` | `EventHubProducerClient(..., hub).sendBatch` / `EventHubConsumerClient(group, ..., hub).subscribe` | service → `getContainerClient(container)` → `getBlockBlobClient(object)`: `upload`, `uploadData`, `uploadFile`, `uploadStream`; blob `download`, `downloadToBuffer`, `downloadToFile` | `SecretClient.getSecret(name)` |
| Python | `get_queue_sender` / `get_topic_sender` → `send_messages`; `get_queue_receiver` / `get_subscription_receiver` → `receive_messages` | explicit `eventhub_name`: producer `send_batch` / consumer `receive` | service `get_blob_client(container, blob)` or `get_container_client(container).get_blob_client(blob)`: `upload_blob` / `download_blob` | `SecretClient.get_secret(name)` |
| Java | `ServiceBusClientBuilder.sender().queueName` or `topicName` → `buildClient`: `sendMessage`, `sendMessages`; receiver `queueName` or `topicName` + `subscriptionName` → `buildClient().receiveMessages` | builder `connectionString(cs, hub)` or `credential(namespace, hub, credential)`: `buildProducerClient().send` / `consumerGroup(...).buildConsumerClient().receiveFromPartition` | `BlobServiceClientBuilder...buildClient().getBlobContainerClient(container).getBlobClient(object)`: `upload`, `uploadFromFile` / `downloadContent`, `downloadStream`, `downloadToFile` | `SecretClientBuilder...buildClient().getSecret(name)` |
| Go | `NewSender` → `SendMessage`, `SendMessageBatch`; `NewReceiverForQueue` / `NewReceiverForSubscription` → `ReceiveMessages` | `NewProducerClient` → `SendEventDataBatch`; `NewConsumerClient` → `NewPartitionClient` → `ReceiveEvents` | `UploadBuffer`, `UploadFile`, `UploadStream` / `DownloadBuffer`, `DownloadFile`, `DownloadStream`, container after `ctx` | `GetSecret(ctx, name, version, ...)` |
| C# | existing sends, schedules and processors retained; new `CreateReceiver(queue)` / `(topic, subscription)` → `ReceiveMessageAsync`, `ReceiveMessagesAsync` | explicit hub: producer `SendAsync` / consumer `ReadEventsAsync` | service → `GetBlobContainerClient(container)` → `GetBlobClient(object)`: `Upload`, `UploadAsync` / `DownloadTo`, `DownloadToAsync`; container `UploadBlob`, `UploadBlobAsync` | `SecretClient.GetSecret`, `GetSecretAsync` |

Names identify queues/topics, hubs, blob containers and secrets. Group, partition, object and secret-version arguments do not change the contract. Supported ARM IDs and Blob/Key Vault URLs use the same physical-name rules as infrastructure and config. A new topic subscription is `channel::servicebus::subscription:<topic>/<subscription>`; a queue containing `/subscriptions/` stays a distinct queue. Existing C# topic processors retain their topic identity.

An environment name stays unresolved with `env:NAME` unless a stored workload actually runs that code. Topic and subscription companions resolve together only within the same workload, root and container; unknown or incompatible typed targets stay unresolved. Resolved workload links are `inferred`. Exact factory calls retain their own receiver names. Hidden constructors, type-only injected clients, name-matched hops, reassigned receivers and opaque return alternatives supply no new SDK side.

This inventory is bounded. Connection strings and credentials are not parsed for resource names; Event Hubs needs an explicit hub argument. Direct Blob/Container URL constructors, session processors, other polling/streaming families, list/delete/metadata operations and other SDKs are outside this list. Merely creating a client or handle supplies no direction.

Queue forms `createReceiver(queue, options)` in JS/TS and `CreateReceiver(queue, options)` in C# retain queue identity when options are proven: a JS/TS object or a known `ServiceBusReceiverOptions` construction. JS/TS also accepts explicit, unshadowed `undefined`; C# accepts `options: null`. Reassigned or opaque second arguments stay unresolved. A string second argument still names a subscription, with optional options in the third argument.

### Custom resources and infrastructure modules

Infrastructure defined by Strimzi, Config Connector, ACK or the listed Crossplane families can provision the same contract code uses. For example, a `KafkaTopic` with `metadata.name: topic-object` and `spec.topicName: shipments` appears in `search` with `kind = "iac"` by its object and by `shipments`; `context` shows `channel::kafka::shipments`. `trace` goes down from the node to its uses and up from a use to infrastructure. For a private module, add `[[iac.module]]` to `meada.toml`; for your own CRD, use `[[iac.custom_resource]]`. See the exact inventory and limits in [capabilities](site/en/capabilities.md#custom-resources-and-infrastructure-modules) and [configuration](site/en/configuration.md#private-iac-mappings). Incomplete names remain searchable without an invented provisioner.
