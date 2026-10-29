# Changelog

All notable changes to Meada are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-29

The first release: the trunk of the roadmap (ADR 0003) and every spec merged since.
The full list of changes is in [docs/releases/0.1.0.md](https://github.com/ClaudioMascaro/meada/blob/v0.1.0/docs/releases/0.1.0.md).
Identity automation (ADR 0016, #319) and named workspaces (ADR 0020) are decided here and ship
in a later release. Prebuilt binaries cover Linux and macOS on x86_64 and arm64 and Windows on
x86_64; Windows arm64 waits for a toolchain that builds it.

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
  x86_64 and arm64 with a shell installer, and an npm package `meada` (spec #32) staged with
  each release for `npx meada`; multi-arch OCI images, default and `-cloud` (issue #279), with
  the console's static files in the cloud image (issue #313); a Helm chart (#281); Docker
  Compose examples (#280); and opt-in console profiles for both (issue #314).
- Documentation and community: README, CONTRIBUTING, code of conduct, security policy and issue
  and PR templates in Portuguese, English and Spanish; a CI workflow; a multilingual
  documentation site (issue #320); ADR 0020, recording the direction of spec S5.
- Every language pack and every manifest and configuration reader has a hostile-input probe
  held to a time and size budget.

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
