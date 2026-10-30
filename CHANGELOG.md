# Changelog

All notable changes to Meada are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

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
  every channel (Cargo, prebuilt binary, shell installer, `npx meada`, container), MCP setup with
  `npx meada mcp` and the three tools with their parameters, the `meada.toml` reference (every
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

### Fixed

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
- Console GitHub grant (#496): a session's GitHub grant and its state (`unread`, `current`,
  `expired`) are read from the same permissions set at the same instant, so a session never shows
  `expired` with a grant read while the set was fresh.

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

[Unreleased]: https://github.com/ClaudioMascaro/meada/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ClaudioMascaro/meada/releases/tag/v0.1.0
