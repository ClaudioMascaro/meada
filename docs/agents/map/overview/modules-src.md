# Modules (`src/`)

The original map was verified against `src/` and `tests/` of the v0.1 trunk. The code wins
over this catalogue, and a change to a seam updates its detail.

Each line names what a module owns and links the file in `map/` with its detail. Read the one
for the area a change touches.

- `engine.rs`: `Engine`, the one query seam (`open`, `refresh`, `export`, `search`, `context`, `trace`, open items, decisions, `status`); it builds the results `meada-core` renders. [queries](../queries.md)
- `items.rs`: consumer open items; `iac/provision.rs` supplies patterned provisioning items through the same decision transport. [queries](../queries.md)
- `trace.rs`: the `Graph`, the walk and the process walk (`Graph::process`). [queries](../queries.md)
- `partial.rs`: the partial export a query reads (#396): the contract rows whole, the code rows the answer needs. [queries](../queries.md)
- `drift_view.rs`: places the links `context`, `search` and `status` show with their drift. [queries](../queries.md)
- `render.rs`: the source files an answer reads, and the budget and line helpers `meada-core` holds. [queries](../queries.md)
- `mcp.rs`: three read-only MCP tools over stdio and streamable HTTP, the `Gate` before routing, `serve_http`. [queries](../queries.md)
- `main.rs`: clap, the CLI's commands. [queries](../queries.md)
- `msg.rs`: the CLI message catalog, pt-BR, English and Spanish. [queries](../queries.md)
- `lib.rs`: the crate root, re-exporting the modules the CLI, the MCP server and the tests share. [queries](../queries.md)
- `workspace.rs`: the workspace (`Workspace::discover`: repositories, peers, sibling clones) and server mode's workspace manifest (`Manifest`). [indexing](../indexing.md)
- `config.rs`: `meada.toml` (`[[peer]]`, `[serve]`, `[[resource]]`) and the shared TOML reader. [indexing](../indexing.md)
- `repo.rs`: repository root and identity, read through `git`. [indexing](../indexing.md)
- `index.rs`: the SQLite index: schema, the freshness pipeline (`refresh`) and `export`; `index/values.rs`, the values cache beside it (ADR 0027). [indexing](../indexing.md)
- `lang/`: the language adapter and the TS/JS, Python, Rust, Go, Java, Kotlin, C# and Swift packs. [indexing](../indexing.md)
- `resolve.rs`: the shared resolution: calls to definitions (`Resolver`), values and receivers (`Code`). [indexing](../indexing.md)
- `community.rs`: the deterministic Leiden behind communities (spec #31). [indexing](../indexing.md)
- `entry.rs`: entry points (spec #31), found when an index is written. [indexing](../indexing.md)
- `contract.rs`, `contract/`: the `ContractKind` trait, `KINDS`, the free functions the engine asks, and shared SDK constructor/request mechanics in `contract/sdk/`. [contracts](../contracts.md)
- `http.rs`, `http/`: the HTTP kind: the contract and path rules, mounts, and one submodule per language for providers and consumers. [contracts](../contracts.md)
- `resource.rs`, `resource/`: the resource kind, declared in `meada.toml` at symbols, IaC addresses or indexed config keys (`resource/via.rs`, #641), or read from SDK calls; `resource/aws.rs` owns its AWS operation table. [contracts](../contracts.md)
- `alert.rs`, `alert/`: alert labels as emitters, monitoring readers, bounded Go/RE2 matching, receiver context and two-role orphans. [contracts](../contracts.md)
- `image.rs`, `image/config.rs`, `image/patterns.rs`: image builders/runners, normalized names, checked-in build configuration, builder-pattern choices, workload execution, scoped environment resolution (`image/environment.rs`, #663), and two-role orphans. [contracts](../contracts.md)
- `channel.rs`, `channel/`: the channel kind (queues and topics), one module per language for message libraries. [contracts](../contracts.md)
- `ci.rs`: the CI kind (ADR 0028, #824): reusable pipelines of GitHub Actions and the CI jobs that call them, both sides read from the CI nodes at query time. [contracts](../contracts.md)
- `package.rs`, `package/`: packages as contracts, one module per ecosystem behind the `Ecosystem` trait. [contracts](../contracts.md)
- `names.rs`, `names/`: external constructor origins shared by kind-owned language forms (`names/python/arguments.rs` checks complete factory evidence), and the questions the contract kinds ask of a name: what a library outside the repository is. [languages](../languages.md)
- `jvm/`: what the JVM ecosystems and kinds share: the Maven and Gradle readers, Spring configuration, file readers. [languages](../languages.md), [seams](../seams.md)
- `dotnet/`: what the .NET pack and ecosystems share (spec #28): the MSBuild project reader and what builds on it. [seams](../seams.md)
- `xml.rs`: the bounds the XML build readers of `jvm/` and `dotnet/` share. [seams](../seams.md)
- `document/`: OpenAPI and AsyncAPI provider documents, read into a `Summary` (spec #21), and `events.rs`, the one YAML and JSON event reader every YAML or JSON reader goes through (#646). [documents](../documents.md)
- `config_values.rs`, `config_values/hcl.rs`, `config_values/sites.rs`: config values and indexed key-path locations for declarations (#641); config values (#639, #640, ADR 0025): every scalar of the YAML, JSON and Terraform files the walk visits and of the hidden CI paths, with its key path, `search` kind `value` and the `meada status` counts. [documents](../documents.md)
- `iac.rs`, `iac/`: the embedded provider type table, provisioning sides/links and subscription
  summaries (`iac/subscription.rs`, #656, #658, #667), GCP identities and Cloud Run workloads
  (`contract/gcp.rs`, `iac/gcp.rs`), directionless config uses
  (`iac/config_uses.rs`, #661), and `iac/orphans.rs`, scope-aware orphan
  reports and side markers (#657); IaC nodes (#649,
  ADR 0025): the Terraform resources, data blocks and module calls with their address and root
  module, the references between them, the static evaluation of their names (#650,
  `iac/functions.rs` the pure functions, `iac/json.rs` the Terraform JSON adapter), `search` kind `iac`, and `context` and `trace` on an
  address; Compose, CloudFormation and Serverless services (#652 to #654) are nodes of the same
  shape. [documents](../documents.md)
- `kube.rs`: Kubernetes objects and Kustomize overlays as IaC nodes (#651, ADR 0025): address `<Kind>[.<group>]/<namespace>/<name>`, an overlay as the root of its evaluation with prefix, suffix, namespace and images applied, the references between objects, and the `meada status` count of the entries not followed; `kube/crossplane.rs`, Crossplane's roles, composed resources and references (#655). [documents](../documents.md)
- `literals.rs`, `zod.rs`, `declared.rs`: the shape of a route read from its handler's returned literals, zod schemas and declared types (spec #23); `lang/ts/types.rs` finds the schema a `z.infer` annotation names. [documents](../documents.md)
- `decisions.rs`: `meada.decisions.toml` and its replay at query time. [decisions](../decisions.md)
- `decide.rs`, `decider/`: `meada decide` and decider mode, the only network path. [decisions](../decisions.md)
- `modules.rs`: the section table of `meada.workspace.toml` (ADR 0012): each cloud module's section, its cargo feature, and whether this build has it. [cloud](../cloud.md)
- `valkey.rs`: the one Valkey client the cloud modules share. [cloud](../cloud.md)
- `postgres.rs`: the data layer (feature `postgres`, ADR 0016): the durable mutable state of server mode. [cloud](../cloud.md)
- `audit.rs`: the audit (ADR 0016, #328): a record in `<schema>.audit` per console write and per CLI twin of one. [cloud](../cloud.md)
- `directory.rs`, `directory/`: the directory (feature `directory`): users and groups the identity provider pushes in over SCIM 2.0. [cloud](../cloud.md)
- `oidc.rs`, `oidc/`: server mode as an OAuth 2.1 resource server (feature `oidc`, ADR 0016). [cloud](../cloud.md)
- `grant.rs`, `grant/`: the grant providers (ADR 0016): what an admitted subject may read. [cloud](../cloud.md)
- `accounts.rs`, `accounts/`: local accounts (feature `accounts`, ADR 0024): passwords, session tokens and agent tokens. [cloud](../cloud.md)
- `github_app.rs`, `github_app/`: git's credential helper for the indexing job, from GitHub App installation tokens (ADR 0013). [cloud](../cloud.md)
- `snapshot.rs`, `snapshot/`: the snapshot store (feature `snapshot`, ADR 0012): bundles, generations, publish, pull, workspace versions. [snapshot](../snapshot.md)
- `ui.rs`, `ui/`: the management console's process (feature `ui`, ADR 0014): its API, its sessions and the front end's files. [console](../console.md)
- `workspaces.rs`, `workspaces/`: named workspaces (spec S5, ADR 0020, 0021): the `[workspaces]` section, contexts, lifecycle and store. [workspaces](../workspaces.md)
- `catalog.rs`: the `[catalog]` section (#341): the repositories the role check may list. [workspaces](../workspaces.md)
- `proposals.rs`: the `[proposals]` section (ADR 0022): where agents reach the proposal server. [workspaces](../workspaces.md)
