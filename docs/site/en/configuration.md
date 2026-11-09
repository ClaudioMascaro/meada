# Configuration

Each repository can have a `meada.toml` at its root, versioned in git, saying who it talks to.
A repository that consumes a route names its peer:

```toml
[[peer]]
repo = "example.com/acme/api"            # origin git remote, no scheme and no .git
base_url_env = ["API_URL"]               # variable the client reads its base URL from
# path = "../api"                        # optional: a hint of where the clone is
```

A repository that serves the route declares the hosts it answers on:

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"                         # optional: what the gateway adds in front of paths
strip = "/v1"                            # optional: what it removes from their start
```

With `prefix` and `strip`, a call to `/shop/orders` reaches the code route `/v1/orders`, at the
level `inferred` with the reason `gateway prefix /shop, gateway strip /v1`. A version segment
goes only when you write it in `strip`, and callers of the code path keep linking as before.

A shared resource no code names, such as a table or a bucket, goes in as `[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # lowercase identifier
id = "orders"
role = "write"                           # read, write or read-write
via = ["src/orders.ts::cancelOrder"]     # optional: the locations that do the access
```

Every writer links to every reader of the same resource in another repository, at the level
`declared`. Without `via`, or when no `via` matches a symbol, IaC node or key, the side is the whole repository.

Java 2.x, Go v2 and .NET AWS SDKs also name resources directly in code: for example, `PutObject` on `uploads` writes `resource::s3::uploads`, and `GetObject` on that bucket reads the same contract. Written SDK uses link at `exact`; an explicit declaration remains `declared`. A same-named Terraform `aws_s3_bucket` links to all those uses as their provisioner. The uses need no repeated `[[resource]]` entries. Construct the client in code or pass it through exact calls; an injected interface type alone is insufficient. Environment names remain unresolved, with the variable in the export. See the [recognized operations](capabilities.md#aws-sdks-in-java-go-and-c).

A route only a test file registers (`*_test.go`, `*.test.*`, `*.spec.*`, `test_*.py`,
`*_test.py`, a `tests` or `__tests__` directory, `src/test` in JVM) is no provider of the
repository: a test's router or fake upstream serves nothing, and counting it would make the
client repository look like a provider of the route it consumes. The test code stays indexed,
with its calls and symbols. To keep those routes as providers, set the key at the top of
`meada.toml`:

```toml
test_routes = true                       # optional: routes registered in test files serve
```

The key counts only in the repository's own `meada.toml` (a workspace manifest entry has none) and only for HTTP routes; a route is a test's by the file where its handler is defined.

Meada finds peers by their git remote, even when the clone sits at a different path on each
machine. An unknown or misspelled key gives an error naming the file, the line and the key.
Without a `meada.toml`, the repository declares no peers.

## JVM artifacts

JVM package contracts are discovered from manifests: `pom.xml`, `settings.gradle`, `settings.gradle.kts`, `build.gradle` and `build.gradle.kts`. They need no additional `meada.toml` key. Declare other repositories in `[[peer]]` as in the examples above. Use literal coordinates for `exact` links; Maven properties and Gradle expressions remain unevaluated, and those that make up the group or artifact produce `ambiguous` candidates.

In Java and Kotlin with Spring, the base URL may come from `@Value("${key}")` (in Kotlin, `@Value("\${key}")`, the `$` escaped), read from the `application.properties`, `application.yml` or `application.yaml` in each module's `src/main/resources`, with no new `meada.toml` key. A literal URL links by host in `[serve] hosts`; a `${VAR}` placeholder (with or without `:default`) uses the same `[[peer]]` `base_url_env` keys. A default whose host some peer serves links by that host.

In C#, the base URL may come from `Environment.GetEnvironmentVariable("X")` or from `configuration["Section:Key"]`, read from the project's `appsettings.json` (the base file only), with no new `meada.toml` key. A key is the variable `Section__Key` in `[[peer]]` `base_url_env`; a literal URL the file holds for it links by host in `[serve] hosts` when some peer serves that host.

## NuGet packages

NuGet package contracts are discovered from the SDK-style `.csproj` files. They need no additional `meada.toml` key: a `PackageReference` links to a provider by its id, and a `ProjectReference` by the path of its project file, which reaches a project of another repository by the roots of the workspace repositories (declare it in `[[peer]]`). Property values such as `$(Version)` stay unevaluated.

## Swift packages

Swift package contracts are discovered from `Package.swift`. They need no additional `meada.toml` key: a `.package(url:)` links to the workspace repository with the same remote identity and a `.package(path:)` to the repository at that path, which must be a `[[peer]]` of the workspace when it is another repository. Use literal URLs, paths and names for `exact` links.


## What is indexed

The walk covers every file of the repository, tracked or not, except what these exclude:

- **Always skipped**, whether or not an ignore file lists them: directories named `node_modules` or `Pods`, a `Checkouts` directory inside `Carthage`, files named `*.min.js` or `*.min.css`, and .NET build output (`obj` or `bin` beside a `.csproj`). They are dependency caches and build artifacts, never first-party code.
- **`.gitignore` and `.ignore`**, honoured with or without git, at the root and in subdirectories, and only inside the repository.
- **`.meadaignore`**, with `.gitignore` syntax, read the same way. It cannot bring back a file the defaults skip.

`vendor`, `dist`, `build` and generated code (Entity Framework migrations, for instance) are not skipped by default: any of them may be first-party code or hold a contract, and Meada cannot tell. List what you do not want indexed in `.meadaignore`:

```gitignore
vendor/
dist/
build/
Migrations/
*ModelSnapshot.cs
*.Designer.cs
```

A file a new default or a new entry excludes leaves the index on the next refresh, as a deleted file does.

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


## Private IaC mappings

Map a module source or custom group/type to an existing type table entry. `entry` uses `<format>:<type>`, for example `terraform:aws_sqs_queue`. The example reads the name from the module's declared input or the object's field:

```toml
[[iac.module]]
source_prefix = "git::https://example.com/platform/modules"
entry = "terraform:aws_sqs_queue"
input = "queue_name"

[[iac.module]]
source = "git::https://example.com/platform/modules/priority"
entry = "terraform:aws_sns_topic"
input = "settings.topic"

[[iac.custom_resource]]
group = "platform.example.com"
kind = "MessageQueue"
entry = "terraform:aws_sqs_queue"
key_path = "spec.endpoints[0].name"
```

Each `[[iac.module]]` chooses exactly one of `source` and `source_prefix`. An exact source wins; otherwise the longest prefix wins, bounded by `/` or `?`, so a neighbor such as `modules-other` does not match. `input` supports dots, indexes and JSON-quoted keys, without selectors. `[[iac.custom_resource]]` requires `group`, `kind`, `entry` and `key_path`, with the same path syntax. Module mappings can also select `terraform:aws_lambda_function` or `terraform:aws_ecs_task_definition`, with the Lambda/ECS inputs documented in capabilities; other workloads are refused. Custom resource mappings select provisioning entries. The value must supply the identity the entry requires, including qualifiers. An unknown entry causes an error naming it and `meada.toml`. There are at most 256 mappings, with text and paths of at most 1 KiB. Configuration participates in indexing: changing or removing a mapping re-evaluates affected nodes. No module is downloaded or executed.
