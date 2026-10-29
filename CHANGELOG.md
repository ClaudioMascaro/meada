# Changelog

All notable changes to Meada are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `[oidc.github]`, the GitHub grant provider's table (ADR 0016): `host` (default `github.com`, a
  lowercase host name), `login_claim` (required), `roles` (a non-empty array of distinct role
  names of 1 to 100 characters with no control character, compared exactly with GitHub's
  `role_name`; default `["read", "triage", "write", "maintain", "admin"]`) and `max_age_seconds`
  (required, at least 60). It needs `[postgres]` and a build with `postgres`, each error naming
  them; beside it `[oidc]` needs neither `identities_claim` nor `groups_claim`. Until the grant
  at admission lands, such a deployment grants nothing through GitHub, failing closed.
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
- An entry of a workspace version may name the team that answers for its repository with
  `owner = "<OIDC group>"` (ADR 0015): one string of 1 to 256 characters, no control character, no
  whitespace at either end, compared exactly. It is imported, stored, shown (right after `path`)
  and pulled like the entry's other keys, and changes the version id; an entry without it keeps
  its id, and generation and bundle ids never read it. `meada index` ignores it. In a bootstrap
  manifest's own `[[repository]]` entries it is an error naming `source = "store"`.
- `meada-core` decides what a subject may change in a workspace version (ADR 0015):
  `authorize_entries` lists each entry a team operator may not add, edit or remove with its
  reason (`outside_grant`, `ownerless`, `owner_change`, `not_your_team`), and `compose_entries`
  builds the version a team operator's save stores, keeping every entry outside its grant in
  place. The browser gets both through WebAssembly. The apply path checks a team operator under
  the refresh lock against the version then current; `meada workspace apply` acts as the global
  operator, so its output and exit codes do not change.
- The console has a team operator (ADR 0015): with `[oidc]` carrying `groups_claim`, `[ui]
  operations = true` and `[workspace] source = "store"`, a subject one of whose groups owns an
  entry of the current workspace version that its grant reads. It sees what a viewer with its
  grant sees and may save and apply changes to its teams' entries; `pointer` and `fence/recover`
  stay the global operator's. Any other change is a 403 `entries_refused` naming each entry
  with its change and reason (an apply counts the entries outside the grant in `hidden`) and
  writing nothing. `GET /api/v1/session` gains `groups` and `grant` with `[oidc]` and `teams`
  for a team operator, and its `role` may be `team_operator`. A team operator's write logs
  `ui: <subject> as <owners> <action> <arguments>: <outcome>`.
- The console's workspace editor serves a team operator (ADR 0015): it holds only the entries its
  grant reads, each with its owner and whether it may change it and why not, marks every change
  the API would refuse in the diff as it types (computed by `authorize_entries` and
  `compose_entries` in WebAssembly, in pt-BR and English) and disables the save while one stands.
  Its save and apply confirmations name the teams that authorize them instead of a CLI command,
  a 403 `entries_refused` shows the server's entries and the hidden count, a lost role turns the
  editor read-only, and apply is offered only when the entries it reads authorize it. Every role
  sees each entry's owner on the versions page, where an owner change lists as the changed key
  `repository.owner` like any other. `meada-core` exports `entriesJson` to WebAssembly.
- The console documentation gains a Teams section in three languages (shared trust, the `owner`
  key, roles, delegation, the `[oidc.groups]` mapping, static manifests, the CLI twins, owner-only
  applies, position and the whole-publication abort), and the Compose and Helm console profiles
  map an example team group. Team ownership as a whole (the `owner` key, the `team_operator` role,
  the session's `groups`, `grant` and `teams`, the `entries_refused` refusal) is additive under
  Spec R1's stability rule: a minor change.
- ADR 0020 and the glossary record the direction of spec S5: a named query context over shared
  indexing and publication.
- `meada status` ends a repository's lines with a pointer to the new `inference` issue form when the
  repository holds a shape drift or an `inferred` link, so a wrong conclusion can be reported.
  The MCP tools never print it.
- `search` with kind `duplicate` lists the symbols whose text is the same in two or more places
  of the workspace, across repositories or within one: groups of exact copies (no
  normalisation), longest first, found by a member's name or path. Each symbol stores the BLAKE3
  of its span's bytes and the export's `symbols` carry it as `hash`. The index schema version
  changes, so existing indexes are rebuilt.

### Changed

- A refresh publishes the whole workspace or nothing (ADR 0012, #316): a failed fetch or index
  stops it before publication and the current generation stays served. `meada index --workspace`
  ends its failures with `index failed for <n> repositories`, and the Compose `fetch.sh` names
  the repository and stage of a failed fetch and refuses a `REPOSITORIES` URL carrying
  credentials, without echoing it.
- `context` and `trace` show the overloads of one method (an ambiguous call whose candidates share a
  file, a class and a name) as one set: `Class.m (one of N overloads)` in `context`, a leaf
  `Class.m (N overloads)` in `trace`. The level, target and candidates in the export do not change.
- A workspace of any number of repositories opens and answers: each index is read through its
  own connection, so the limit of eleven repositories (SQLite attaches ten databases to one
  connection) and its error are gone. Answers and exports of a workspace of eleven or fewer
  are unchanged.
- A call that decorates a definition (a Java or Kotlin annotation, a C# attribute, a Python
  decorator, a Rust route attribute) carries the definition in the index and in the export's call
  site (`decorates`), and `context` lists it in an `annotations:` section instead of among the
  callees of its enclosing class or package. Targets, levels and callers do not change. The index
  schema version changes, so existing indexes are rebuilt.

### Fixed

- Imports of a project under a nested `pyproject.toml` resolve: the nearest manifest above an
  importer gives its package roots (`src`, Poetry `from`, setuptools `where`), so `from app.crud
  import f` in a template's `backend/` reaches `backend/app/crud.py`, and two sibling projects
  that each hold an `app` package keep their own. An unreadable nested manifest declares nothing.

- TypeScript and JavaScript: the `paths` and `baseUrl` of a nested `tsconfig.json` (or
  `jsconfig.json`) resolve the imports of the files below it; the nearest config above an importer
  wins, and `extends` is still not followed. A config under `node_modules` declares nothing.

- A Spring AMQP publish whose message is a class of the repository (`convertAndSend("q", order)`)
  reads the queue, or the exchange and the routing key, as it does for a string message: the
  overload no longer needs the message's type from outside the repository. Java and Kotlin.

- A `@Value` field keeps its base URL when the only writers of its name in the repository are a
  local or parameter of that name, or another class's field of it: an assignment is a write of a
  field only where the name can be one, and it names its class when the innermost class declares
  the field.

- A typed local (`WebClient client = ..`) stays typed when another class in its file has a field
  of the same name: only a field of its own class or of an enclosing class hides it. A Kotlin
  top-level property still hides the locals of its file.

- A Java or Kotlin annotation written in full without an import (`@retrofit2.http.GET`,
  `@org.springframework.web.bind.annotation.GetMapping`) is read as the library's, unless the
  repository defines that name or an import binds its head.

- Swift: a Vapor group of static middleware calls (`grouped(AuthMiddleware())`) adds no path
  segment, so the routes registered on it are served at the parent's path. A route group argument
  Meada cannot tell from a middleware (`grouped(Paths.api())`) is read as one, an accepted false
  positive, documented in the repo map.

- Swift: an `if let` or `guard let` of an environment read
  (`guard let base = ProcessInfo.processInfo.environment["X"]`) is a base URL like a `let`
  holding it, so `base_url_env` maps it.

- TypeScript and JavaScript: a function defined by assignment is a symbol, and the calls of its
  file reach it: `obj.y = function`, `X.prototype.y = function` (the method `X.y`), `exports.y =`,
  `module.exports.y =` and `module.exports = function` (the export), a `let` or `var` function
  the file assigns nowhere else, and the methods and function properties of a top-level `const`
  object literal (`o.m`).

- A Swift local or property bound to a member read on a name from outside the repository
  (`let session = URLSession.shared`) is opaque, as the direct chain `URLSession.shared.data(..)`
  is: a call on it no longer matches a repository method of the same name and labels
  (`Cache.data(from:)`). A `var` assigned again keeps the name match. The stored facts of a Swift
  file gain `Class::Member`, so an existing index is rebuilt.

- A Swift module is a target of one package: two packages of a repository that each declare a
  target `App` are two modules, so a call binds its own package's declaration instead of staying
  `ambiguous` among the others; `import App` binds the importer's own package's target, else the
  nearest one's. `@testable import App` shows the `internal` declarations of that module
  (`fileprivate` and `private` stay hidden), so a test's call to them resolves. The stored facts of
  a Swift file gain `testable` and `confined`, so an existing index is rebuilt.

- A Swift receiver whose written type is a `typealias` (`store: StoreOf<Feature>`, optional or
  not, on a parameter, a property or a local) is typed as the alias's target, through an alias of
  an alias; a cycle of aliases types nothing and an alias of an outside type leaves the receiver
  outside; a constructor call written through the alias (`StoreOf<X>(..)`) reaches the target's
  initializers. An initializer another module keeps to itself (`internal`) is no longer a candidate
  of a constructor call there. A trailing closure fills only a parameter whose written type is a
  function type (also optional or `@escaping`), a generic parameter or a repository `typealias`
  of a function type, so `store.send(.action) { }` no longer weighs the `animation:` overload.
  The stored facts of a Swift file gain each parameter's `closure` and the file's `function_aliases`, so
  an existing index is rebuilt.

- A Swift call on an implicit member (`.make()`, `wait(.seconds(1))`) keeps its member name under
  the head `(implicit)`, instead of the placeholder `(prefix_expression)`, and resolves by name
  and labels among methods (`inferred` or `ambiguous`), never to a free function or a type of
  that name; `.init(..)` stays unresolved. The call ids of such calls change.

- A Swift call to a function with a capitalized name (`Check(1)`) resolves as a plain call, by
  scope, labels and arity, instead of staying unresolved as constructor syntax; a name that binds
  a type still constructs it and a name the repository does not declare stays unresolved. A
  call not written under `await` no longer reaches a function declared `async`. The stored facts
  of a Swift file gain `awaited` and `suspends`, so an existing index is rebuilt.

- A Maven or Gradle dependency whose group or artifact names a property or variable the build
  files declare as a literal reads as that literal, and links like a written coordinate: a pom
  `<properties>` entry, `project.groupId` or `project.artifactId`, or a property of a parent pom
  in the repository; a Gradle `val`, `def` or `ext` name assigned once to a plain string, the
  project's `group` included. A name the build files do not declare, declare more than once or
  compute stays `ambiguous`, as before. Versions are untouched. The stored summary of a `pom.xml`
  gains `properties`, `parent` and a computed entry's `template`, so an existing index is rebuilt.

- A Java local declared with an initializer and never assigned again in its file is followed into
  its value, as a Kotlin `val` is: `String base = System.getenv("WEB_URL");` then
  `rest.getForObject(base + "/api", ..)` carries `env:WEB_URL`, so `base_url_env` maps it to its
  peer, and a local `WebClient` made by `WebClient.create("https://x")` has that host. A local the
  file assigns, one sharing its name with another binding or a field of its class, one in an
  initializer block and one in an anonymous class's method stay unread.
- A Java call with two or more arguments to a varargs method (`any(String... parts)`) reaches it:
  a spread parameter takes any number of arguments, as Kotlin's `vararg` does, and the method is
  among the functions of the `flexible` table.
- A method annotation of a Java or Kotlin class whose family has a base from outside the
  repository is read through the file's imports, so a Spring controller, a JAX-RS resource, a
  declarative client (Feign, Retrofit) and a channel listener (`@KafkaListener`, `@RabbitListener`,
  `@JmsListener`) that extends an internal library's base gives its providers and consumers.
  Single-type and on-demand imports agree. A member type of the outside base named like the
  annotation, which Java would read before the import, is the one case read wrong. Before, the
  opaque base hid the single-type import and the class gave nothing.
- A Kotlin call made on another call's result (`make().open()`, `Box().open()`,
  `find()?.open()`) resolves within the class of the first call's written return type, or the
  class its constructor builds, `exact` when one member fits. A type with arguments or a `?`
  names its class; an expression body, a type parameter, a function type and a type from outside
  the repository give no class. Before, the Kotlin pack recorded no result and every such call
  was a name match (okhttp's `Request.Builder().build()`).
- A Kotlin class with a secondary constructor is constructible: each constructor, the primary
  one and each secondary one, is a callable of the class's name with its own parameter list, and
  a call narrows among them by argument count (`exact` when one fits, `ambiguous` when two take
  that count). Before, the class left the callables of its name and every construction stayed
  unresolved (okhttp's `Request.Builder()`).
- A Rust method called on another call's result (`Router::new().route(..)`, also through
  `.await` and parentheses) resolves within the family of the inner target's written result,
  `exact` when one member fits. Before, it was a name match among every method of its name
  (axum's `.route` after `Router::new()`). After `?` the call stays a name match: the written
  result is the `Result` or `Option` around the value.
- A Rust type a glob import brings (`use super::*;`, `use crate::x::*;`) heads a path called,
  as a function it brings is called: `Def::new()` under `use super::*;` resolves `exact`, also
  when the parent imports `Def` by name. Before, a glob gave its functions alone and the call
  stayed unresolved (axum's `Router::new()` and actix-web's `ResourceDef::new()` in their test
  modules).
- A Kotlin `expect` class and its `actual` classes of one name are one class, the `expect` one:
  as a base, a constructor or a written type the name binds it, so the members it declares
  resolve `exact` on its subclasses. Before, the name bound every declaration, ambiguously, and
  a subclass's bare calls of the base's members stayed unresolved. A member only an `actual`
  declares stays a name match, as does a call an `actual`'s own overload of the name may
  answer. A name two Kotlin star imports both give is `ambiguous`; before, the last import gave
  it.
- A Kotlin class a star import brings (`import t.*`) binds as a base, a constructor and a
  written type, and its members resolve on a subclass. Before, the file's own package was
  asked first and its reading also took the file's star imports, whose answer it then dropped,
  so the name stayed unresolved.
- A C# type of a namespace around the file's (`Acme.Query` from `Acme.Query.Internal`) binds
  without a `using`, as a static call's receiver, a constructor and a base: the innermost
  namespace first, and before a type a `using` brings. Before, the name stayed unresolved, or
  bound to a farther namespace's type of the same name. A namespace declared beside the code's
  in the same file does not enclose it.
- A Java, Kotlin, C# or Swift class whose outside bases are all platform types with known
  members (`Serializable`, `Comparable`, `IDisposable`, `Hashable`, `Codable`, an enum's raw
  value type, Vapor's `RouteCollection`) resolves calls to its own members at `exact`; only the
  names those types declare (`compareTo`, `Dispose`, `hash(into:)`) stay name matches. Any other
  outside base keeps the strict rule (ADR 0019). A helper a `RouteCollection` calls from `boot`
  now serves its routes under the collection's prefix.
- The overloads of a member stay candidates across projects and modules: a C# call that fits a
  base's overload in another project no longer links `exact` to the derived class's own, and a
  Swift call through a type extended in a test module still reaches the type's overloads in
  its library.

- A C# file of a few hundred KB (an Entity Framework model snapshot) is no longer dropped
  silently: the qualification budget is charged for enclosing names past 256 bytes, not for every
  node, so a generated file reads whole. A file the budget still drops (a long namespace repeated
  across a broad file) is named by a `meada status` line.
- `node_modules` and `Pods` directories, the `Checkouts` of `Carthage` and `*.min.js` and
  `*.min.css` files stay out of the index, whether or not an ignore file lists them. A
  `.meadaignore` file, at the root or nested, is read like a `.gitignore`.
- A Rust `mod` a test build compiles (`#[cfg(any(test, feature = "p"))]`, `#[cfg(all(test, unix))]`)
  is indexed, not only a `#[cfg(test)]` one; any other item is still judged with `test` false.
- A Swift `Package@swift-<version>.swift` is a manifest, not code: it yields no symbols or calls.
- A static import of a class whose own base is looked up through it (a C# `using static N.A;`
  beside `class A : Inner`, as in efcore; Java's `import static p.A.*;` the same way) no longer
  overflows the stack of the indexer: the base is opaque unless another import names it, and
  the depth limits bound the whole walk past every import.
- A C# `internal` type (one with no access modifier too) links from its own project's files
  alone: a file of another project of the repository gets no link to it, and the name binds to
  the type that file sees (a public one of the same name, or one a `using` brings even from a
  file of the internal type's namespace). `InternalsVisibleTo` is not read.
- A C# member read between a call and the member called (`Repo.Make().x.Save()`) is the
  receiver: the field's or property's written type owns `Save`, and an unread one leaves a name
  match, never the call's class.
- The `obj/` and `bin/` directories beside a `.csproj` are .NET build output and stay out of the
  index, whether or not an ignore file lists them: their generated sources no longer add
  symbols or repeat implicit usings.
- A C# project file that removes thousands of `<Using>` items, a `Directory.Build.props` read
  under hundreds of projects, a project referencing many others and many imports of one
  directory index in time linear in their size; a props file of more than 256 usings and
  references adds nothing, as one that cannot be read.

- A Spring `application.yml` whose joined keys pass 1 MiB (`YAML_KEY_BUDGET`) reads as empty
  instead of allocating every key joined to each of its children: a crafted file no longer
  costs gigabytes to index.
- A Java chain of locals each made by a call on the one before (`var a2 = a1.m();`) costs its
  length to resolve: a call's result was read twice per link, through exact targets and again
  by name, which doubled the cost of every link.
- A C# type two projects of one repository declare under one qualified name binds, from a file
  of one of them, to that project's own type, as the compiler prefers it; before, every link
  to the name was dropped, same-project calls included. A caller in neither project still finds
  the name ambiguous.
- A C# library type or attribute written in full (`System.Net.Http.HttpClient`,
  `[Microsoft.AspNetCore.Mvc.HttpGet]`) is the library's unless the repository declares that
  very name: a client of the repository's own `System.Net.Http.HttpClient` is no consumer, and a
  repository namespace under the library's root (`Microsoft.Acme`) no longer hides every name
  written in full.
- C# top-level statements in a file that also declares a global type `Main` make no synthetic
  `Main` program, which would share the type's name.
- A Java OpenFeign or Retrofit method whose link is ambiguous (a Feign client without `url`
  whose path two repositories serve) or whose URL is unresolved (Retrofit's `@Url`) is an open
  item at the method: `search` with `kind=ambiguous` or `kind=unresolved` lists it, and
  `meada decide` resolves it, sending the method as the excerpt.
- `context` on a public Java class of a shared Maven or Gradle artifact lists the calls in other
  repositories that reach its constructors and methods, one line per call.
- A Spring `@Value` in a Gradle source set whose `srcDirs` lies outside `src/main` reads its
  module's `src/main/resources` configuration.
- A Retrofit annotation's path is read from `value` alone, and a Retrofit path with a `{` that
  does not close within its segment gives no consumer, as for Spring, JAX-RS and Feign.
- A Java field and a method of the same name keep separate namespaces: `repo.save()` and
  `this.repo.save()` read the field `repo`, `repo()` calls the method.
- Console manifest groups and validation errors respect the viewer's grant; PKCE redirects
  retain issuer query parameters while replacing generated OAuth keys.
- Fence recovery shares the fleet reader's Valkey clock and complete heartbeat validation,
  and refuses counters that would prevent the next native INCR acquisition. Exact u64
  transport remains supported, with bilingual operational refusals at the signed ceiling.

- Rust message brokers: the client a producer or consumer traces back to is read from its
  constructor by path, as the Rust HTTP readers do, and only when the head of the name is the
  import itself: a function spelled like the crate (`fn lapin() {}` beside `lapin::Connection::
  connect(..)`) no longer hides the client, and a value spelled like the crate gives none.

### Added

- Swift language pack (spec #29, #117): `.swift` files without a Swift toolchain or Xcode. Each
  SwiftPM target of `Package.swift` is a module (one module without it); types, protocols,
  functions, methods and initializers are symbols named with their argument labels
  (`OrderService.place(order:quantity:)`), with an extension's members under the extended type.
  Unqualified, `self`, static and initializer calls resolve within the module and the modules
  it imports, narrowed by argument labels; a tie is `ambiguous`. A call on a property,
  parameter or local whose type is written (`let x: T`, `T(...)`) resolves within that type and
  the protocols it conforms to, `exact`; through a protocol-typed receiver it stops at the
  requirement. A receiver of unread type is a name match by base name and labels; one whose
  type is outside the repository (`URLSession`, `String`, `AF`) reaches only the members of
  that type's extensions in the repository. The stored
  Swift facts change, so existing indexes are rebuilt.
- Vapor routes are HTTP providers (spec #29, #119): `get`, `post`, `put`, `patch`, `delete` and
  `on(.METHOD, ..)` with their `grouped(..)` prefixes, a `RouteCollection` under the builder it
  is registered on, and builders followed through assignments, arguments and return values; the
  handler is the trailing closure or the function given as `use:`.
- Hummingbird routes are HTTP providers (spec #29, #120): `get`, `post`, `put`, `patch`,
  `delete` and `on(path, method:)` called on a `Router` or on a `group(prefix)` of it, at any
  depth, with Vapor's value flow and handler rules; a controller's `addRoutes` is not read.
- Swift packages (spec #29, #124): each `Package.swift` provides `package::swiftpm::<name>`,
  with its library products and their targets recorded. A `.package(url:)` links to the
  workspace repository of the same remote identity and a `.package(path:)` to the one at that
  path, `exact`, with the written requirement as range and `range unchecked`; an interpolated
  URL, path or name is `ambiguous`, listing every provider that fits, and so is a dependency
  on a package whose own name is computed; a `path:` to the repository's own root links. An `import Module` of a
  module of a depended-on package reaches its public declarations (`; import module`), narrowed
  by argument labels, `ambiguous` when several packages expose the module; `context` and `trace`
  cross the hop. A dependency outside the workspace is ignored and `Package.resolved` is not
  read. The stored manifest summary changes, so existing indexes are rebuilt.
- Swift HTTP consumers (spec #29, #121): `URLSession` `data(from:)` and `data(for:)` (with a
  `URLRequest(url:)` whose `httpMethod` is assigned), Alamofire `AF.request(url, method:)` and
  the same on a `Session`, and AsyncHTTPClient `execute` of an `HTTPClientRequest(url:)` with
  `method` set; GET when the method is absent. The URL folds `URL(string:)`,
  `URL(string:relativeTo:)`, `appendingPathComponent`, literals, interpolation, `+` and `let`
  constants (`static let` too) from other files and modules; a wrapper at any depth is a
  consumer, and a URL that cannot be read is an unresolved consumer, shown in the export and in
  `context`. The stored Swift facts change, so existing indexes are rebuilt.
- Swift base URLs from the environment (spec #29, #122): `ProcessInfo.processInfo.environment["X"]`
  at the start of a URL is the base URL variable `X` and maps through `base_url_env`, also
  through `!`, parentheses, `??` fallbacks that are plain literals and string methods such as
  `trimmingCharacters`; a second read, any other fallback, a key that is no literal and Vapor's
  `Environment.get` leave the base unknown. A Swift repository joins a TypeScript workspace with
  the same `meada.toml` keys, and `trace` follows the chain across both boundaries. The stored
  Swift facts change, so existing indexes are rebuilt.
- Swift program entry points (spec #29, #123): a type marked `@main` (through its `main()`
  method when it declares one) and the top-level code of a `main.swift` file, a symbol named
  `main` that owns the calls written there, each start a process. The stored Swift facts
  change, so existing indexes are rebuilt.
- `Directory.Build.props` files apply to the C# projects below them as MSBuild imports them
  (the nearest one, then those it imports with `GetPathOfFileAbove` or a relative path): their
  `<Using>` items, `PackageReference`s and `ProjectReference`s, and the `PackageId`,
  `AssemblyName` and version that name a project's NuGet package under the project's own. A
  computed id there provides no package. The stored manifest summary changes, so existing
  indexes are rebuilt.
- A C# file sees what the projects its project references in the repository pass on, directly
  or through others, and a Java file what the Maven modules its module depends on pass on (a
  `compile` dependency that is not `optional`); a reference whose `PrivateAssets` includes `all`
  or `compile`, or is computed, does not pass on.
- A C# name that two other projects of the repository declare binds, from a file of a third,
  to the one in the project its project references, instead of `ambiguous`.
- C# `nameof(x)` is the text of the last identifier of `x`, so a channel name or a URL part
  written with it is known.
- A Refit `RestService.For<T>(base)` gives its base to the methods `T` inherits from the
  repository's interfaces, not only to those `T` declares.
- A C# receiver typed by a declaration (a parameter, field, property or local) whose type comes
  through a package `using` links through that package, as a call on the type's name does.
- An explicitly typed C# lambda parameter (`(Order o) => o.Total()`) types its receiver as a
  method parameter does; one written with `in` or `out` keeps the name match.
- Every language pack and every manifest and configuration reader has a hostile-input probe
  (a chain of locals on call results, files built to explode), held to a time and size budget.

- NuGet packages (spec #28): each SDK-style `.csproj` provides `package::nuget::<id>` (its
  `PackageId`, else `AssemblyName`, else file name). A `PackageReference` naming a workspace
  package and a `ProjectReference` to a project of another repository link to it at `exact`
  (`range unchecked`); a `using` of a namespace of a project the file depends on reaches its
  types, so `context` on a public type lists the calls in other repositories and `trace` crosses
  the hop, with `; import namespace` in the reason. The exported import's `how` can now be
  `namespace`.
  A `ProjectReference` pins its project (a twin package id elsewhere never makes the link
  `ambiguous`), only an SDK-style project owns files (the stored manifest summary gains a
  `project` marker, so existing indexes are rebuilt), and package ids match without regard to case.
- C# source indexing (issue #107): `.cs` files, grouped into projects by SDK-style `.csproj`
  files (an old-style one, with no `Sdk`, is none; `.sln` files change nothing), yield
  namespace-qualified classes, records, structs and interfaces, nested and `partial` ones
  included (each part in its own file of one project, a member lookup reading every
  part and its bases), methods and
  constructors with written parameter types, and local functions (`Method.Local`). `using`,
  `using static`, aliases and `global using` resolve through every project's namespaces; a
  `global using`, a project file's `<Using>` items and its enabled implicit usings apply to every
  file of that project. Bare, `this`, `base`, static and `using static` calls are `exact` when
  the argument count, counting `params` and default values, leaves one method. Attributes are
  calls on what they decorate. No .NET SDK or MSBuild is involved.
- C# ASP.NET Core controllers as HTTP providers (issue #109): a public method of a class with
  `[ApiController]`, or deriving from `ControllerBase` or `Controller`, carrying `[HttpGet]`,
  `[HttpPost]`, `[HttpPut]`, `[HttpPatch]`, `[HttpDelete]`, `[HttpHead]` or `[HttpOptions]`
  serves the class `[Route]` prefix joined to its template, with `[controller]`, `[action]`
  (without `Async`, or a literal `[ActionName]`) and `{id}`, `{id:int}`, `{id?}` read as ASP.NET
  Core does; a template starting with `/` or `~/` replaces the prefix, and a method `[Route]`
  without a method attribute serves every method (`http::ANY::<path>`, linked to consumers of
  any method). Conventional routing, a controller with a base class of the repository, a class
  in `partial` parts, `[NonAction]` methods and attributes that are not MVC's are not read.
- C# ASP.NET Core minimal APIs as HTTP providers (issue #110): `MapGet`, `MapPost`, `MapPut`,
  `MapPatch`, `MapDelete` and `MapMethods(pattern, methods, handler)` on the application or a
  `MapGroup(prefix)` group serve under every prefix around them, groups nested at any depth,
  followed through locals, method arguments, return values and extension methods called on a
  group (`group.MapOrders()`), so one called on two groups serves under both. A lambda handler
  is an inline handler named for its route, a method group the method it names. A builder
  Meada cannot read (a lambda's parameter, a field, a reassigned local) serves nothing.
  The shared value flow now follows a local a pack binds to one value (C# and Kotlin) and a C#
  extension method called on a receiver in scope; every C# lambda in a definition is a scope of
  its own, so its parameters hide outer names only inside it. A C# local an outside call made
  (`var app = builder.Build()`) is of an outside type, so `app.Run()` reaches no repository
  `Run()`.
- C# HttpClient calls as HTTP consumers (issue #111): `GetAsync`, `PostAsync`, `PutAsync`,
  `PatchAsync`, `DeleteAsync`, `GetStringAsync`, `GetStreamAsync`, `GetByteArrayAsync`,
  `GetFromJsonAsync`, `PostAsJsonAsync`, `PutAsJsonAsync` and `SendAsync` with a
  `new HttpRequestMessage(HttpMethod.X, url)` on an `HttpClient` (by its declared type or
  construction, resolved through usings) are consumers with the method and the evaluated path.
  URLs fold string literals, interpolation, verbatim and raw strings, `+`, and `const` and
  `static readonly` fields, also of other types, and locals bound once. A name assigned again
  anywhere in its file is unknown. `BaseAddress = new Uri(base)` in the client's initializer
  (field, local or property) or one assignment is its host, joined to a relative URL as `Uri`
  does, and a wrapper is read at each of its callers. `request.Method = HttpMethod.X` and
  `a?.M()` calls are read.
  A call whose URL cannot be evaluated is an unresolved consumer; clients from
  `IHttpClientFactory.CreateClient(name)` are not read.
- C# base URLs from the environment and configuration (issue #112): `Environment.GetEnvironmentVariable("X")`,
  through `??`, `!` and `Trim`/`TrimStart`/`TrimEnd`, is the base URL variable `X`; `configuration["A:B"]` on
  an `IConfiguration` or on `builder.Configuration` is the variable `A__B`, with the literal the project's
  base `appsettings.json` holds for it (comments, trailing commas, case-insensitive keys, nested
  objects) as its default, so a literal host some peer serves links by host and any other goes
  through `base_url_env`. `appsettings.<Environment>.json`, `GetValue`, `GetSection` and options
  binding are not read. New neutral fixtures `xlang-csharp-{web,api}` cover the TypeScript to C#
  and C# to TypeScript chain.
- C# typed clients and Refit interfaces as HTTP consumers (issue #113): `AddHttpClient<T>(c =>
  c.BaseAddress = ...)` gives its base, folded and read from the environment or configuration, to
  the `HttpClient` the container passes to `T`'s constructor and to every call on it in `T`,
  through a `readonly` field the constructor assigns; two registrations with different bases, a
  registration Meada cannot read, or a constructor the code calls (a typed `T t = new(..)`
  included) leave it unknown. A `BaseAddress` assigned conditionally, or one that differs from
  the client's initializer, is unknown. Each method of a Refit interface (`[Get]`, `[Post]`, `[Put]`, `[Patch]`,
  `[Delete]`, `[Head]`) is a consumer with the base of each `RestService.For<T>(url)`, and its
  callers reach it, so `trace` crosses it; `AddRefitClient` is not read. The shared injector
  (`resolve::Inject`) is now also asked about a parameter no annotation marks
  (`Injected::Param`).
- C# messaging channels (issue #115): Confluent.Kafka (`Produce`, `ProduceAsync`, `Subscribe`),
  RabbitMQ.Client (`BasicPublish`, `BasicConsume`, `QueueBind`, with spec #20's routing rules),
  AWSSDK.SQS (`SendMessageAsync`, `ReceiveMessageAsync`), NATS.Client (`Publish`,
  `SubscribeAsync`, wildcards `inferred`), StackExchange.Redis streams (`StreamAdd`,
  `StreamReadGroup`) and Azure Service Bus (`CreateSender`, `CreateProcessor`) are producers and
  consumers of channel contracts, with the new broker kind `servicebus` (a processor on a topic
  consumes every message sent to it, whatever the subscription). A client counts by its written
  type or its construction, resolved through the usings. Handlers are the callback (a lambda or a
  method group) or each one attached with `+=` (a consumer per handler) or, for a polling
  client, the method holding the call. Names fold like URLs, a Service Bus sender made for a
  parameter is named by the callers of its method, and `String.Empty` is the default exchange;
  an environment or configuration read is an unresolved side, and so is a sender written more
  than once. MassTransit is not read.
- Kotlin source indexing (issue #95): `.kt` and `.kts` files (Gradle's `build.gradle.kts` and
  `settings.gradle.kts` stay manifests) yield package-qualified classes, interfaces, objects,
  companions (`Class.Companion` when unnamed), top-level and extension functions, properties
  with getters and methods, with written parameter types. Single, aliased and star imports
  resolve through package declarations; bare, `this`, object and companion calls are `exact`
  when the argument count, counting default values and `vararg`, leaves one function. An
  extension function is never a member of the class it extends, even written in that class's
  file: a member of the class or of a base wins, and a Java call never reaches it. A call on a
  class's name reaches its companion's member before an instance member of the same name. A call on
  `x!!` or `x?.` is a call on `x`, of the type `x` is declared with. Kotlin and
  Java files of one repository call each other by fully qualified name. No Kotlin compiler, JDK
  or Gradle is involved.

- Ktor server providers (issue #98): routes inside `routing { }` (`get`, `post`, `put`, `patch`,
  `delete`, `head`, `options`, with a path or none, and `route(path, HttpMethod.X)`) serve the
  prefix of every enclosing `route(prefix) { }` block, through extension functions on `Route` or
  `Routing` called inside a block, in any file and once per prefix they are called under. Each
  route's lambda is its handler, named for the route. Ktor names count only with Ktor's import;
  type-safe routing is not read. A Kotlin lambda passed as an argument, and a function declared
  inside one, is now a scope of its own.
- Ktor client consumers (issue #99): `get`, `post`, `put`, `patch`, `delete`, `head`, `options`
  and `request(url) { method = HttpMethod.X }` on an `HttpClient` are HTTP consumers, with a
  `defaultRequest { url(base) }` of the client as the base of its relative URLs. String
  templates (`"$base/users/${id}"`), `+`, `const val` and properties fold, also from other
  files, and a hole that does not fold is a parameter; a function that wraps the client is read
  at each of its call sites, the URL and the method (`method = m`, `this.method = m`) it takes
  from its parameters substituted there, and a call whose URL or method cannot be evaluated is an
  unresolved consumer, never a `GET` guessed. The client may be imported by `*` or held by an
  untyped local `val` (`HttpClient(CIO).use { it.get(..) }` too). `url { path(..) }` builders are
  not read.
- Kotlin imports into provider sources (issue #106): an import of a class, a top-level function
  or an extension function from a workspace artifact the repository depends on reaches the
  declaration in the provider by its `package` declaration, so `context` on a public declaration
  lists its consumers in other repositories and `trace` follows the hop. `exact` when one
  provider declares the package and declaration, `ambiguous` when several do or overloads of the
  argument count remain; the reason ends `; import package`. A class name called constructs the
  class, a call on a class's name reaches its companion before its members (an instance's call,
  the member first), and an extension function crosses only through the import of its name,
  never as a member of the class it extends: a member of the receiver's type with the name wins,
  and the link is `exact` when the type the source writes for the receiver is a class of the
  provider's package whose members are all known and one extension of that class fits,
  `inferred` on any other receiver. A call that may leave a default argument out or fills a
  `vararg` reaches every overload it could, so the index stores the functions with a variable
  argument count (the `flexible` table, a new stored format). The source roots gain
  `src/main/kotlin` and `src/test/kotlin`, and the Gradle scanner reads the `kotlin` directory
  set, `sourceSets["main"]` and `getByName("test")` alongside the `java` forms: a Kotlin set is
  main when named `main` or `*Main`, test when named `test` or `*Test`, and any other is not
  read. A named companion and `setSrcDirs` are not followed.
- Kotlin base URLs from Spring configuration and the environment (issue #100): the Ktor client
  reads a base a property or a constructor property gets from Spring's `@Value("\${key}")` (the
  `$` escaped in a Kotlin string) in the `application.properties` or `application.yml` of its
  module, also in a repository with no Java file, with the Java rules: a literal URL links by
  host, a `${VAR}` placeholder maps through `base_url_env`, and a default whose host a peer
  serves links by host. `System.getenv("X")` and `System.getenv()["X"]`, through `?:`
  fallbacks, `!!` and string methods such as `trimEnd` and `removeSuffix`, are the base URL
  variable `X`, also through a local `val` of the function (`val base = System.getenv("X") ?:
  ".."`), which folds where it is written as TypeScript's `const` does. A cross-language fixture links TypeScript and Ktor in both directions.
- Kotlin HTTP consumers (issue #101): `RestTemplate`, `WebClient`, OkHttp and Retrofit are read
  in Kotlin files with the Java rules, written the Kotlin way: no `new`, a `RestTemplate()` or
  `Request.Builder()` constructor call, `HttpMethod` and `HttpUrl` imports by `*`, and
  `Api::class.java` as the Retrofit service. A Retrofit interface with annotated `fun`s links
  each function to its provider, and a Kotlin base comes from a literal, `@Value` or
  `System.getenv` with the rules of issue #100. A client held by a property, a constructor
  property or a local `val` is followed (`val x = RestTemplate()`, `val web =
  WebClient.create(..)`, `val retrofit = Retrofit.Builder()...build()`), and the base comes from
  the call that made it. The extension `retrofit.create<Api>()` (with `import retrofit2.create`)
  creates the interface as `create(Api::class.java)` does. A named argument on a nested builder
  step and an OkHttp builder held by a local are not read. A cross-language fixture calls the
  TypeScript provider through a Retrofit interface.
- Kotlin channels (issue #104): Spring Kafka, Spring AMQP, JMS, AWS SDK v2 SQS, jnats, Jedis
  and Lettuce clients are read in Kotlin files with the Java rules, written the Kotlin way:
  named arguments (`topics = [..]`, `queues = arrayOf(..)`), a nested annotation written without
  `@` (`QueueBinding(..)`, `TopicPartition(..)`), `Queue("q")` without `new`, `const val` and
  companion constants, a constructor-injected template, `@Value` and `${key}` placeholders
  through Spring configuration, and `mapOf("id" to id)` as a Redis stream map. The default
  exchange reaches the queue named by its key (`exact`) and a wildcard NATS subject is
  `inferred`. A name that cannot be evaluated is an unresolved producer or consumer. A
  trailing-lambda post-processor (`convertAndSend("x") { m -> m }`) is an untold overload, as
  Java's lambda: its producer is unresolved. A message built in the call (`Order("1")`) picks
  the overload as Java's `new` does. In `with`, `run`, `apply` and extension function blocks, a
  member of the implicit receiver hides the property of the class around and a top-level
  property, after the parameters and locals of the code around, for channel and HTTP clients,
  call resolution and folded values alike; a client read in the block of a receiver whose type is
  outside the repository is unknown. A call on the receiver itself (an extension function on the client, `apply`,
  `with`) is not read. A cross-language fixture produces a topic in TypeScript
  and consumes it in a Kotlin `@KafkaListener`, and `trace` down from the producer reaches the
  handler.
- Java OpenFeign and Retrofit clients (issue #90): each method of an `@FeignClient(name, url)`
  interface with Spring mapping annotations, or of a Retrofit interface with `retrofit2.http`
  verbs, is an HTTP consumer sited at the interface method, so a caller of the method reaches
  the provider in `trace` and `context`. A Feign `url` and `path` resolve through the Spring
  configuration; without `url` the host is unresolved, the consumer's `via` is
  `client:Feign client <name>` and the reason names the client
  (`host unresolved (Feign client <name>)`). A Retrofit base comes from the
  `Retrofit.Builder().baseUrl(base)` that creates the interface, one consumer per creation. The
  Java pack reads `Api.class` as a class literal. `@RequestLine` and other Feign contracts are
  not read.
- Java program entry points (issue #91): a `public static void main(String[])` or
  `String...` method is a program with its own process.
- Java source indexing (issue #84): package-qualified types and method signatures, overloads
  narrowed by argument count, package and static imports, and conformance, determinism and
  incremental-refresh coverage, without a JDK or build tool.
- Java method calls (issue #85) through the types the source writes: fields, parameters,
  locals, `var` with `new T(...)` or a call, chains `f().g()` through the declared return type,
  `super.f()`, and the supertypes written in `extends` and `implements`. Overload families are
  read per signature across a class family, an interface receiver stops at the interface, an
  outside base anywhere in a family keeps its methods from `exact`, and a call on an unknown
  receiver is a name match that prefers the one candidate in the owning family; a narrowed match
  says "one of several" in its reason.
- Maven and Gradle artifact contracts (spec #26, #93), using shared JVM manifest readers:
  literal `group:artifact` providers, Maven modules and parent groups, Gradle project names,
  dependencies with unchecked ranges, and ambiguous candidates for computed coordinates.
  Indexing invokes no JVM toolchain and looks up no registry.
- Java imports into provider sources (spec #26, #94): a call through an import of a class from a
  workspace Maven or Gradle artifact the repository depends on (`Type.method()`, `new Type(..)`,
  a call on a field, parameter or local of the type, on-demand and static imports) reaches the
  method or constructor in the provider by package declaration, picked by name and argument
  count, through the package path npm, PyPI, Cargo and Go use. `exact` when one provider
  declares the package and type, `ambiguous` when several do or overloads of the count remain;
  the reason ends `; import package` and the export's `how` is `package`. `context` on a public
  class method lists its consumers in other repositories and `trace` follows the hop. Test
  source roots (`src/test/java`, and a Gradle `sourceSets` directory of a set other than `main`)
  are never imported; the Gradle scanner reads literal `srcDirs`, `srcDir` and `srcDirs +=`
  (Groovy and Kotlin DSL), nothing computed or conditional. A method is picked among the
  overloads of the class's family in the provider, as Java merges them (an inherited `find(long)`
  stays beside a declared `find(String)`); an ancestor outside the provider's repository keeps
  the link from `exact` (`inferred`). Two on-demand imports that both bring a class some
  provider declares give no link. The stored manifest summary gains `test_roots` and the index
  a `families` table, so existing indexes rebuild.
- Spring MVC HTTP providers (spec #26, #86): methods of `@RestController` and `@Controller`
  classes with `@GetMapping`, `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping`
  or `@RequestMapping(method = ..)`, under the class `@RequestMapping` prefix, with `static
  final` constants and `+` folded across classes and one provider per path of an array. A
  `@RequestMapping` without `method` is an `http::ANY::<path>` provider. Annotations and
  `RequestMethod` count only through a Spring import (single-type, static or on-demand, the
  latter through the new `Outside::candidates`, which every Java kind can use); a shadowing
  repository type, a competing import and `server.servlet.context-path` give no route.
- JAX-RS HTTP providers (spec #26, #87): methods with `@GET`, `@POST`, `@PUT`, `@PATCH`,
  `@DELETE`, `@HEAD` or `@OPTIONS` in a class with `@Path`, under `javax.ws.rs` or
  `jakarta.ws.rs`, at the class path joined to the method `@Path` with constants folded and
  `{id:regex}` as a parameter. Names count only through an import, as for Spring; a method with
  only `@Path` and `@ApplicationPath` give no route.
- Java HTTP consumers (spec #26, #88): RestTemplate methods, WebClient requests (base from
  `WebClient.create` or `builder().baseUrl`, through constants, `final` fields and each caller
  of a wrapper), `HttpRequest.newBuilder()` and OkHttp `Request.Builder` chains, `GET` when a
  whole chain names no method. Clients count through single-type imports and the one candidate
  of on-demand ones. URLs fold `+`, constants, `URI.create`, `String.format` (each `%s` or `%d`
  its argument) and `{id}` templates; wrappers are read at each call site
  through any number of levels, and an unreadable URL is an unresolved consumer. A new
  `Expand::Chain` reads a call whole, its builder chain and arguments folded at the callers,
  and carries an outside name such as `HttpMethod.GET` as a `Value::External` spelled by the
  writing file's imports, so a shadowing type is no method. The written type of a field typed
  by a single-type import is now spelled as that import names it, and the Java pack keeps a
  `final` field's one value (its initializer or a single constructor assignment) as a constant
  of its class.
- Java base URLs from Spring configuration (spec #26, #89): a field or a constructor parameter
  with `@Value("${key}")` holds `key` from the `application.properties`, `application.yml` or
  `application.yaml` of its module's `src/main/resources` (a shared `jvm::spring` reader the
  Kotlin pack can call). A literal URL links by host; a `${VAR}` placeholder, and `System.getenv("VAR")`, map
  through `base_url_env`; with `${VAR:default}`, the default links by host when a peer serves
  it, otherwise `VAR` maps as before. Meada reads no environment value, and profiles and
  relaxed binding are not read. A constructor parameter takes Spring's value only when no call
  in the source creates the class (`this(..)` and `super(..)`, now constructor calls,
  included). A cross-language chain fixture (TypeScript and Java) is
  golden for its exports and `trace` both ways.
- Java channels (spec #26, #92): Spring Kafka (`KafkaTemplate.send`, `@KafkaListener`), Spring
  AMQP (`RabbitTemplate.convertAndSend` by its overload, the default exchange reaching the queue
  its key names, `@RabbitListener` with `@QueueBinding` bindings and
  `BindingBuilder.bind(..).to(..).with(..)`), JMS (`JmsTemplate.send` and `convertAndSend`,
  `@JmsListener`) under the new broker kind `jms` (also an AsyncAPI protocol), the SQS client of
  the AWS SDK v2 (the queue the last segment of `queueUrl`), jnats `publish` and `subscribe`,
  and the Redis streams of Jedis and Lettuce (`xadd`, `xreadGroup`, `xreadgroup`). A client
  counts by the type the source writes for it; names fold like URLs, the `${key}` placeholders
  of annotations and `@Value` fields read from the module's Spring configuration. An annotated
  listener's handler is the annotated method, a polling client's the method that names the
  topic or queue. The Java pack reads an annotation written as an element value
  (`@Queue("orders")`) as its name and elements, kept only while contracts are extracted. The
  cross-language chain reaches a `@KafkaListener` from a kafkajs producer. The overload of
  `convertAndSend` follows the written type of its third argument (`MessagePostProcessor`,
  `CorrelationData`), and an argument of untold type leaves the producer unresolved; an
  `@Exchange` is `direct` unless its `type` says otherwise, so a wildcard key binds only on a
  topic exchange. The Java pack indexes an annotation type (`@interface`) as an interface, so a
  repository's own annotation hides Spring's of the same name for routes, channels and
  `@Value`.

- A multilingual documentation site (issue #320), with the README reference migrated to
  Portuguese, English and Spanish guides, React Flow maps and roadmap, accessible static and
  textual fallbacks, and a strict GitHub Pages build. S3, S4 and S5 proposals are clearly separated from delivered S1/S2.

- Workspace editor and version history (issue #310): viewers compare any two immutable
  versions; operators validate entries and review a local WASM diff before confirming save
  or apply. Rollbacks name the next refresh, existing saves show their first author/message,
  and CLI import and WASM share one bilingual validation fixture table and the same validator.

- Opt-in console profiles for Compose and Helm (issue #314), with their own bootstrap,
  snapshot storage, pull companion and proxy or Ingress. Managed refresh pulls the workspace,
  fetches operator-provisioned mirrors, indexes and publishes under one lock; the initial seed
  and apply remain explicit. The console stays disabled by default. Brazilian Portuguese,
  English and Spanish guides cover admission, PKCE, operations and fenced recovery.

- Console operator writes (spec #293): verified groups or a boolean claim grant the operator
  role and the whole fleet view. Fenced pointer changes, fence recovery, workspace save and
  apply share the CLI operations, with authorization before uploads and one escaped audit
  line per accepted or refused write. Entry validation is shared with CLI import, concurrent
  imports preserve the first version's metadata, and serialized versions respect the reader's
  existing size cap. Recovery ignores malformed heartbeats and includes workspace fences.
  Managed status uses one workspace version for its report and readiness exit code.

- Fleet and store operators can set, retain and drop generations and recover a stuck fence
  after confirming the exact CLI command. Viewer sessions and disabled operations expose no
  actions; keyboard cancellation sends no write, and CLI refusals preserve the lock holder.

- Console frontend distribution (issue #313): the cloud image includes the static UI at
  `/usr/share/meada/ui`, built with pinned Node and wasm-pack from the npm lockfile. The
  container release workflow also attaches `meada-ui-<version>.tar.gz` with the same files
  under one versioned directory, for Cargo installations with `ui`. Default images, Cargo
  builds and cargo-dist's binary artifacts remain independent of the frontend.

- Multi-arch OCI images (issue #279): `ghcr.io/claudiomascaro/meada:<version>` and `latest`
  for the default build, `<version>-cloud` and `latest-cloud` for every cloud module, on
  Linux amd64 and arm64. Prereleases get version tags only. The Debian bookworm-slim runtime
  includes git and CA roots, runs without root and has no default command or embedded data.
  A separate release workflow builds both variants from source with the lockfile; its pull
  request check builds the default amd64 image and searches it over HTTP. The base is refreshed
  only with a Meada release.

- Console query playground and code graph views (spec #293): run the read-only MCP tools
  against a repository and held generation, preserving their answer text, and inspect
  communities, contracts, cross-repository links, shape drift and decisions through accessible
  tables. A repository export is checked by the shared WebAssembly core and reused across
  graph views in the browser.

- Docker Compose examples for two default-image nodes with independent indexes and cloud-image
  nodes sharing retained generations. Caddy admits bearer tokens over TLS; internal Valkey and
  RustFS use a private CA. Index, publish and pull jobs have separate writable volumes, while
  servers read their manifests and data through read-only mounts. Both profiles are checked
  without starting services in CI. Optional `snapshot.ca_file` augments the system TLS roots
  with a bounded, validated PEM file for private-CA stores (#280).

- Operational console pages for fleet diagnostics and recoveries, store bundles and retention,
  repository provenance, GitHub App cache expiry and throttling, and OIDC admission checks.
  Missing sections are named explicitly. The token check clears its field after each request.
  Large fences retain their exact decimal values, and tables scroll within mobile viewports.

- Static management console frontend (spec #293, ADR 0014): Vite, React and strict TypeScript
  with the shared WebAssembly core, Brazilian Portuguese and English navigation, configuration,
  session and bootstrap manifest pages. OIDC sign-in uses authorization code with PKCE S256;
  bearer tokens stay in memory. The standalone frontend gate pins Node and wasm-pack, installs
  from a lockfile and writes the matching Meada version into `meada-ui.json`. Cargo builds
  remain independent of Node.

- Console operational reads (spec #293, ADR 0014): fleet heartbeats, pointer, retention and
  kept marks, lock holder/TTL, fence diagnostics and recoveries, store documents and bundle
  metadata, managed workspace versions, repository commits/schema versions and GitHub App
  cache expiry/throttle state. Service reads share a two-second deadline, malformed records
  are checked before use, repository and nested peer identities follow the viewer's grant,
  and credentials never leave Valkey. The token check now reads identities from the current
  managed workspace. Server-mode indexing records optional commit provenance beside the index,
  validated against its contents without changing the SQLite schema.

- Helm chart for module-free nodes and generations, with local readiness, fenced refresh jobs,
  external TLS services, OIDC values and OCI release publishing (#281).

- Node heartbeat (spec #293, ADR 0014, feature `generation-pointer`): with `[generations]`,
  every round of `meada snapshot pull --every <seconds>` ends by writing the node's state into
  Valkey, whether the round swapped, changed nothing or failed: `{<key_prefix>}:node:<name>`, a
  JSON document (`name`, `generation`, `fence`, `held`, `ready`, `version`, `time`) living three
  periods, and the name in the sorted set `{<key_prefix>}:nodes`, scored by the expiry in Unix
  seconds, from which the same script removes every expired name. A single `pull` and a node
  without `[generations]` write none. A write that fails is one warning for the round and
  changes nothing else. The new optional `[snapshot] node_name` names the node (the host name
  in lowercase by default), checked against the node name pattern; names must be unique.

- Workspace versions (spec #293, ADR 0014, feature `generation-pointer`): the `[[repository]]`
  entries of a workspace stored as immutable documents in the snapshot store,
  `workspaces/<version id>.json` (id, author, time, message, entries in manifest order), the id
  derived from the entries alone. `meada workspace import <file> --message <text> [--author
  <name>] --workspace <bootstrap manifest>` validates the file's entries before any connection
  and stores a version, or answers the one already stored; `meada workspace list` prints the
  versions newest first, the current one (`{<key_prefix>}:workspace`) marked; `meada workspace
  show [<version>]` prints a version as `[[repository]]` entries. Everything read back is
  checked as hostile input. The new `[workspace]` section, read in every build, takes `source =
  "file"` (the default) or `"store"`, which needs `generation-pointer`, `[snapshot]` and
  `[generations]` and no `[[repository]]`.

- Applying and pulling workspace versions (spec #293, ADR 0014): `meada workspace apply
  <version>` (feature `refresh-lock`) moves `{<key_prefix>}:workspace` to a stored version
  under the refresh lock with a new fence and prints `<id> <fence>`; rolling back is applying
  an older version. `meada workspace pull` brings the current version under
  `<index_dir>/workspace/` (the document and a `current` file swapped by rename, never to a
  lower fence), checking everything it reads as hostile input. With `source = "store"`, `meada
  index`, `status`, `export` without `--generations` and `snapshot publish` read the pulled
  version from disk and refuse without one, naming `meada workspace pull`; `meada index` still
  opens no connection. `meada mcp --listen` without `--generations` refuses such a manifest,
  naming `--generations`. The generation document gains an optional `workspace` field, the
  version that produced it (absent with `source = "file"`, part of no id, ignored by older
  binaries), which a pull checks. `meada snapshot fence recover` also counts the workspace
  pointer's fence. The refresh job of a managed workspace runs `meada workspace pull`, the fetch
  of the mirrors, `meada index --workspace` and `meada snapshot publish` under `meada snapshot
  lock`.

- Management console (spec #293, ADR 0014, feature `ui`, in `cloud`, no crate of its own):
  `meada ui --listen <addr> --workspace <bootstrap manifest>` serves, behind the operator's
  reverse proxy, the front end's files from the new `[ui]` section's `dir` and a JSON
  API under `/api/v1/` with `config`, `session` and `manifest` (the manifest as loaded, never a
  secret's value, the module sections this build has, and the errors of the file as it now
  stands on disk). Each request passes a `Host` check and an `Origin` check (a write needs the
  console's own `Origin`) before routing; no answer sets a cookie or a CORS header. Static files
  are served from a fixed extension table, with `nosniff`, `no-referrer` and a fixed content
  security policy, and no path leaves `[ui] dir`. The start is refused, every problem listed,
  for `[ui]` errors, a missing `[ui] dir` and front end files built for another version
  (`meada-ui.json`). Serving nodes do not change.

- Console admission with `[oidc]` (spec #293, ADR 0014, features `ui` and `oidc`): the console
  is a resource server like the nodes, admitting a bearer token for `[ui] audience` with the
  nodes' own OIDC module (401 with `WWW-Authenticate`, 403 for a missing scope); the static
  files and `config` need no token, and `config` adds the issuer, the new required `[ui]
  client_id`, the scopes and the audience for the browser's authorization code flow with PKCE.
  Admitted subjects are viewers unless a configured verified group or boolean claim grants
  the operator role (`session`). `GET /api/v1/admission` shows the issuer's
  metadata, the JWKS cache (key ids, last read, a failed refresh with its reason) and the groups
  table; `POST /api/v1/admission/check` answers a pasted token's outcome per check and the
  identities its grant admits, from the cached keys only, holding the token for that request
  alone (body capped at 16 KiB). The token's grant bounds every view as it bounds a node's
  answers. The start is refused when the issuer's metadata or JWKS cannot be read. A JWKS that
  is not a key set is now reported by line and column, never by its content, on nodes too.

- Console playground and exports (spec #293, ADR 0014, feature `ui`): `POST /api/v1/query`
  takes `{"repository", "generation", "tool", "arguments"}` (`search`, `context` or `trace`,
  the arguments as the MCP tool takes them; body capped at 16 KiB) and answers `{"text",
  "generation"}`, the text the node's tool answers byte for byte; `GET
  /api/v1/repositories/<identity>/export?generation=<id>` answers the bytes `meada export`
  prints. The console answers both in process and never calls a node: with `[snapshot]`, from
  the generations its own `meada snapshot pull` keeps in its `local_dir` (the current one by
  default, a retained one when named, `generation` its id); without it, from the indexes under
  `index_dir` of the repositories its manifest lists (`generation` null). It starts without a
  servable generation or index: the two routes then answer a 503 naming `meada snapshot pull`
  or `meada index --workspace`, and the next request after it answers with no restart. An
  unknown repository, one outside the viewer's grant and a generation not held are 404s, the
  last naming the held ones; peers outside the grant are left out of the answer, as on a node;
  `trace` with `changes: uncommitted` is refused with the node's text.

- Server mode (spec #259, ADR 0011): `meada mcp --listen <addr> --workspace <manifest>` serves
  the three tools over MCP streamable HTTP, one endpoint per repository at `/<identity>`, from
  mirrors that a workspace manifest (`meada.workspace.toml`) lists. Each index lives under the
  manifest's `index_dir` and is refreshed by command (`meada index --workspace`, run by the
  operator's fetch job or webhook), never by the server, which only reads; `meada status
  --workspace` and `meada export --workspace --repo` read the same manifest. SIGINT and SIGTERM
  stop the server after the answers in flight, within 25 seconds. Meada authenticates nobody: a reverse proxy admits
  the clients. A request carrying `Origin` gets 403 on every path, before any listing.
- Server mode, cloud modules (ADR 0012): the manifest's module sections are tied to cargo
  features that are off by default, so a section in a build without its feature is an error
  naming the file, the line, the section and the feature. The first module is the shared Valkey
  connection (internal feature `valkey`, section `[valkey]`: TLS outside loopback, an AUTH/RBAC
  user, the password only from an environment variable or a file, standalone or cluster, every
  key under `key_prefix`); `meada status --workspace` says whether Valkey is reachable and its
  exit code does not depend on it. The release binaries stay the default build. The `cloud`
  feature enables every module at once (Unix only, as `refresh-lock` is).
- Server mode, snapshot store (ADR 0012, feature `snapshot`, section `[snapshot]`): `meada
  snapshot publish --workspace <manifest>` checks, reading only, that every index equals its
  mirror and every mirror is clean, then uploads the workspace to an S3-compatible bucket as a
  generation (one tar bundle per repository, its mirror and `index.db`, a generation document
  with each bundle's size and BLAKE3 hash, then the `current` pointer) and prints its id. Ids
  derive from the commits, the schema and binary versions and the entries' blocks, so a
  republish or a second machine at the same commits uploads nothing. Credentials come from one
  source (`static` keys from variables or files, `web-identity`, `pod-identity` or `instance`),
  TLS is required outside loopback, and no request follows a redirect. `meada index` stays
  offline.
- Server mode, snapshot store pull (ADR 0012, feature `snapshot`): `meada snapshot pull
  --workspace <manifest>` fetches the store's current generation into the section's
  `local_dir`, checks every bundle's size and BLAKE3 hash against the generation document,
  extracts it as hostile input (ids and identities checked before they become paths; no
  absolute path, `..`, escaping link, write through a symbolic link or special file; byte and
  entry ceilings), replaces each mirror's `.git/config` with a minimal one and removes its
  `.git/hooks`, and swaps the node's pointer file (`<local_dir>/current`, `<generation id>
  <fence>`) by rename, keeping the current and the previous generation. Meada's `git` calls on
  pulled mirrors read no system or global configuration. One pull runs at a time per
  `local_dir`; `--every <seconds>` repeats it. A failed pull changes nothing. A pull refuses an
  `index.db` that is not in rollback journal mode or holds any schema object (a view, a
  trigger, a table or an index) other than the ones this build creates, naming the repository
  and the object.
- Server mode, serving generations (ADR 0012, feature `snapshot`): `meada mcp --listen <addr>
  --workspace <manifest> --generations` serves the generation `<local_dir>/current` names; each
  request reads the pointer file, reads a generation's document and mirror configuration once
  (each `meada.toml` and `meada.decisions.toml` at most 1 MiB), and the call opens the engine
  over the generation its request was routed by (read-only, defensive, nothing written per
  call), so a call open before a swap finishes on its generation and the next one sees the
  new. A source path from an index renders only inside its mirror, through no symbolic link,
  from a regular file of at most 16 MiB; any other shows as unavailable. A node with no
  servable generation answers 503 with the body `no servable generation`, the reason on
  stderr. The node's manifest contributes only its module
  sections, nodes never run `meada index`, and the server refuses to start without a servable
  current generation (naming `meada snapshot pull`, or the repository and generation of an
  index at another schema version or with a schema object this build does not create). A
  missing or interrupted index names the generation's directory to remove before pulling
  again. `meada status --workspace <manifest> --generations` prints
  the current id, the held ids and `store: reachable` or `store: unreachable (<reason>)`, and
  exits 1 only without a servable current generation, whatever the store; `meada export
  --workspace <manifest> --repo <identity> --generations` exports from it. The flag is absent
  from the default build.
- Server mode, generation pointer and pins (ADR 0012, feature `generation-pointer`, section
  `[generations]` with `keep`, at least 2; it needs `[snapshot]` and `[valkey]`): `meada
  snapshot publish` ends by writing the current generation and the retained ones to Valkey in
  one script (`{<key_prefix>}:current`, a hash `{id, fence}`, and `{<key_prefix>}:retained`, a
  list of `<id>:<fence>` newest first, each id once, trimmed to `keep`; fence `0` without the
  refresh lock) instead of the store's `current`, and `meada snapshot pull` reads them there:
  every id and fence is checked before the node changes (a malformed entry, a missing pointer or
  an empty list is an error naming the key), the current generation is downloaded and swapped in
  first, then every retained one the node lacks (one that fails is a warning naming it, which
  blocks nothing and the next pull retries), the others are deleted but for the current one and
  the one just replaced, and the node's pointer file carries the fence read with the id. `meada
  status --generations` reads the current id from Valkey and checks the store holds its
  document. `meada mcp --listen --generations`
  then serves `/<identity>@<generation>` from a held generation, a 404 naming the requested,
  the current and the held generations otherwise, and `/<identity>` keeps serving the current
  one. Every answer names its generation in a `Meada-Generation` header and every tool result
  in `_meta` (`meada/generation`); tools, schemas and text output are unchanged.
- Server mode, refresh lock (ADR 0012, feature `refresh-lock`, section `[refresh_lock]` with
  `ttl_seconds`, default 300, at least 10; it needs `[generations]`): `meada snapshot lock
  --workspace <manifest> -- <command>` takes `{<key_prefix>}:lock` in Valkey, gets the next
  fence from `{<key_prefix>}:fence`, runs the refresh as its child with `MEADA_REFRESH_LOCK`
  and `MEADA_REFRESH_FENCE`, renews the lock every third of the TTL and releases it, exiting
  with the child's code. A lock already held exits 1 running nothing; a lock lost (another
  token, or no renewal for a whole TTL) terminates the child's process group and exits 1.
  `meada snapshot publish` then needs both variables, stops before any upload once the lock no
  longer holds its token, and the pointer script writes only while the lock holds it and the
  fence is above the current one. A node never swaps to a lower fence than its pointer file's;
  a lost fence key is named, with the recovery, by the refused publish, the refused pull and a
  `fence: stuck` line of `meada status --generations`. `meada index` stays offline.
- Server mode, OIDC (ADR 0012, feature `oidc`, section `[oidc]` with `issuer`, `public_url`,
  `audience`, `identities_claim`, `scopes` and `jwks_refresh_seconds`, default 3600, at least
  60): the server becomes an OAuth 2.1 resource server as the MCP authorization specification
  (2026-07-28) describes. At start it reads the issuer's metadata (its `issuer` checked) and
  JWKS, with no redirect, no proxy, a 10-second timeout and a size cap, or refuses to start.
  `/.well-known/oauth-protected-resource/<identity>` (and its pinned form) serves the protected
  resource metadata without a token. Every other request needs `Authorization: Bearer <JWT>`:
  an asymmetric algorithm that fits its key, `iss`, `aud` (the configured audience, or the
  endpoint's URL, which its pins accept too), `exp` and `nbf`, and every configured scope; a
  missing or invalid token gets 401 with `WWW-Authenticate`, and a valid one whose
  `identities_claim` does not list the endpoint's identity gets 403; behind an endpoint it may
  read, every answer covers only the repositories the claim lists (#290). A token may also be
  granted through its groups: `groups_claim` with the `[oidc.groups]` table mapping a group to
  identities, the grant being the union of both claims (#291). Authentication runs
  before routing, so a 404 needs a valid token and lists only what it may read. The JWKS is read again
  every `jwks_refresh_seconds` and on an unknown `kid`, at most once a minute, keeping the cached
  keys when that fails. No token or part of one reaches stderr or an error; the proxy stays the
  documented default admission.
- Server mode, GitHub App credentials (ADR 0013, feature `github-app`, blocks `[[github_app]]`
  with `host`, `api`, `app_id` and one of `private_key_file` and `private_key_env`; needs
  `[valkey]`): `meada github-app credential --workspace <manifest> <get|store|erase>` is git's
  credential helper for the indexing job. `get` answers `x-access-token` and a GitHub App
  installation token for a repository some installation covers, minted once and cached in
  Valkey until 10 minutes before it expires; installations are listed from the GitHub API at
  most every 10 minutes and taken round robin per owner, skipping the throttled ones. A
  repository no installation covers gets no answer and exit 0, with one stderr line naming the
  host and the owner. `erase` drops the token git was refused with. `meada github-app throttled
  --workspace <manifest> <repository URL> [--retry-after <seconds>]`, run after a fetch that
  failed with 403 or 429, marks the installation that served it; a 403 or 429 to the module's
  own calls marks its installation or App the same way. No private key, JWT or token reaches
  stderr or an error. `meada mcp --listen` never calls GitHub.
- Server mode, pointer and fence operations (ADR 0014, #299, feature `refresh-lock`, needing
  `[refresh_lock]`): `meada snapshot pointer set <generation> --workspace <manifest>` moves the
  Valkey pointer to a retained generation (a rollback, or forward again) with a fence above
  every node's, so nodes swap on their next pull; the rollback holds until the next publish.
  `meada snapshot fence recover [--at-least <fence>] --workspace <manifest>` raises
  `{<key_prefix>}:fence` to the highest fence recorded in the pointer, the retained list and
  every live heartbeat, or to the floor given, never lowering it; what it cannot read is named
  on stderr and not counted. Each takes the refresh lock itself and releases it. The lock now
  has a holder, `{<key_prefix>}:holder` (host, process, start time and what holds it), written,
  renewed and deleted with it, and every refusal of a held lock names it. The messages for a
  refused, lowered or stuck fence name `meada snapshot fence recover` instead of setting the
  fence key by hand.
- Retained generations kept by hand (ADR 0014, #300, feature `refresh-lock`): `meada snapshot
  pointer retain <generation> --workspace <manifest>` keeps a generation whose document the
  store holds on every node, outside the `keep` trim, in the new set `{<key_prefix>}:kept`
  (appended at the retained list's oldest end with the operation's fence when it was not
  retained); `meada snapshot pointer drop <generation> --workspace <manifest>` frees a
  generation that is not current, kept or not, and the next pull deletes it. Each prints the id
  and takes the refresh lock itself. The kept set holds at most `KEPT_MAX` (8) generations. The
  pointer script now trims only entries that are not kept, down to `keep` of them, and every
  pull holds the whole retained list (at most `keep` plus `KEPT_MAX` entries, refused past it),
  so a pin on a kept generation answers until it is dropped; `pointer set` accepts any retained
  generation.
- npm installer (spec #32): cargo-dist builds an npm package named `meada` with each release, at
  the crate's version, so `npx meada mcp` launches the MCP server and `npx meada <command>` runs
  any command; the release configuration is planned as a dry run on pull requests. The release
  workflow only stages the package (`npm stage publish`); it reaches npm when a maintainer
  approves the staged version with 2FA.
- Go language pack (spec #25): `.go` files with no Go toolchain. Functions, methods named
  `Type.Method` (pointer and value receivers alike), structs, interfaces and other named types,
  and interface methods (`Interface.Method`); a function literal is no definition, its calls
  belong to the function around it. An import names a package directory through the module path
  of the `go.mod` that owns it (a nested module owns its subtree, two modules of one path name
  none), or through a `replace` to a local directory of the repository (a `replace` by an
  outside module makes the module external); plain, aliased, dot and blank imports, a name two
  imports of a file give linking to nothing. The files of one directory that declare one package share their top-level
  scope, `_test.go` files included, and an import names them all; the standard library and
  modules outside the repository are external. Build constraints hold for a fixed `linux/amd64`
  target (gc, no cgo): a file whose name ends in another system or architecture, or whose
  `//go:build` line is false, or that imports `"C"`, is left out. Exports are the names that start with an upper-case
  letter. The language-neutral seam `Layout::shared` gives a group of files one scope. Method
  calls: a call on a receiver whose type the source writes (a parameter, the method receiver, a
  struct field, a `var`, a composite literal `T{}` or `&T{}`, or the first result of a function
  whose result type is written) reaches that type's method at `exact`, also when another file
  of the package declares it; a receiver whose type is not written is a name match
  (`inferred`, or `ambiguous` with every candidate), and a call through an interface stops at
  the interface method. A promoted method of an embedded struct is a name match. The
  `func main` of each `package main` is a program entry point with its process; a `main` in
  another package, a method named `main` or a file left out of the build is not. The index
  is rebuilt once (new schema version).
- Go packages (spec #25): each `go.mod` with a `module` provides `package::go::<module path>`, and
  each `require`, and each `replace` to a local directory (in the repository or, when it leaves it,
  in another repository of the workspace, resolved from the text and the repositories' roots),
  links to the workspace provider of that module; a `replace` by another module leaves the module
  out. The link is `exact`, the reason `package::go::<module path> <version> (local none), range
  unchecked`. An import of `<module path>/<directory>` reaches the exported names of that
  directory of the provider, `exact` when the directory exists, with `; import module path` ending
  the reason. `go.sum` and the module proxy are never read.

- Go HTTP providers (spec #25): `net/http` (`HandleFunc` and `Handle` on the package or a
  `ServeMux`, Go 1.22 `METHOD host/path` patterns with the host dropped, a mux given as the
  handler of another under that pattern),
  gin (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`, `Handle`, `Any` on the engine
  or a `Group`) and echo (the same, minus `Handle`). A path's `{id}` and `{id:[0-9]+}` are one
  parameter segment, and for gin and echo `:id` and `*path` too (`net/http` reads `{name}`,
  `{name...}` and `{$}` alone); a route of every method is `http::ANY::<path>`. A router is
  followed through assignments, arguments, return values and imports, so a function registering
  on the group it receives serves its routes under every prefix its callers give it; a prefix,
  path or mount Meada cannot read serves nothing: a `ServeMux` reached only as an argument of a
  call (`http.StripPrefix`, `echo.WrapHandler`, `gin.WrapH`, a middleware) is an unread mount.
  A mux given as a handler sees the whole path, so its routes are served where they fall under
  the outer pattern and for its method. A function literal is the route's inline
  handler. The Go pack now reads what a call is given, what a function returns and what a `:=`
  holds. The index is rebuilt once (new schema version).

- Go HTTP consumers (spec #25): `net/http` calls on the package, a `Client` or `DefaultClient`
  (`Get`, `Head`, `Post`, `PostForm`), `NewRequest` and `NewRequestWithContext` (the construction
  is the consumer, its method read from a literal or `http.MethodX`), and resty requests (the
  verbs and `Execute`), with the host of the `SetBaseURL` of the client that issued them (that
  one client: two clients a factory makes keep their own bases), also when the client is a
  function result, a wrapper's parameter whose callers all pass a traced client, or a struct
  field, whose base is the one of the clients every composite literal of the repository sets it
  to (two bases, a literal whose client is not traced or that sets it by position, a base set
  through the field, and a field written through a selector anywhere give no host); a client
  written only as a type, such as a field no literal sets, has no host, and so has a wrapper
  parameter some caller passes an untraced client to. A URL is folded from literals, `+`, `fmt.Sprintf` (a bare
  `%s` or `%v` is the argument, `%d` and `%v` an integer literal; any other verb and argument,
  and any flag, width or precision, a parameter segment), constants and package-level names of
  the package or of another one (a `var` nothing assigns, in the package or through a selector
  such as `config.Base = ..`, folds like a constant). A wrapper over
  a client is read at its call sites, to any depth, with cycle detection. A path that cannot be
  folded is an unresolved consumer.

- Go base URLs from the environment (spec #25): `os.Getenv("X")` and `os.LookupEnv("X")` (its
  first result) lead a URL as the base URL variable `X`, which maps through `base_url_env` like
  the other languages' (Meada never reads the value). The read survives a fallback assignment
  when the value is empty (`if v == "" { v = ".." }`, also tested as `len(v) == 0` with `len`
  unshadowed in the file, or as `!ok` or `!ok || v == ""` where `ok` is the second result of
  the same `os.LookupEnv`, bound once and never assigned; the default never read, provided it
  folds to text from literals and constants of the same file) and
  `strings.TrimSuffix`, `strings.TrimRight` and `strings.TrimSpace`, of the read or of a name
  bound to it. Two reads in one URL, a fallback to another read, a name assigned any other way
  and a read the code spells with a shadowed `os` give no base. Cross-language fixtures
  `xlang-go-web` (TypeScript) and `xlang-go-api` (Go) prove both directions through a variable
  and through a declared host.

- Go HTTP providers for chi and gorilla/mux (spec #25). chi: `Get`, `Post` and the other
  methods, `Method`/`MethodFunc`, `Handle`/`HandleFunc` (every method), `Route(prefix, fn)` and
  `Group(fn)` (the router `fn` receives, a literal or a named function, is the group, also passed on or held by a name) and
  `Mount(prefix, router)`; `{id}`, `{id:regex}` and a trailing `*` are one parameter segment.
  gorilla/mux: `HandleFunc`/`Handle` with `.Methods(..)` (one provider per method; every method
  without it; an unreadable or doubled `Methods` serves nothing) and
  `PathPrefix(prefix).Subrouter()`. Prefixes follow the router at any depth, with cycles cut;
  a router mounted inside a call (a middleware) or given to a router Meada does not read is an
  unread mount, and `http.StripPrefix` is not modelled.

- Go channels (spec #25): producers and consumers of the common broker clients, linked to the
  other languages' sides of the same channel. Kafka: segmentio/kafka-go (`Writer{Topic:}` and
  `WriteMessages`, `Message{Topic:}`, `NewReader(ReaderConfig{Topic:, GroupTopics:})`),
  confluent-kafka-go (`Produce` with `TopicPartition.Topic`, `Subscribe`, `SubscribeTopics`) and
  sarama (`SendMessage`, `ConsumePartition`, `ConsumerGroup.Consume`, whose handler is the
  `ConsumeClaim` of the handler type). RabbitMQ: amqp091-go (`Publish`, `PublishWithContext`,
  `Consume`, `QueueBind`; the default exchange reaches the queue its key names). SQS:
  aws-sdk-go-v2 (`SendMessage`, `SendMessageBatch`, `ReceiveMessage`, the queue being the last
  segment of `QueueUrl`, through `aws.String`). NATS: nats.go (`Publish`, `Subscribe`,
  `QueueSubscribe`, with the `*` and `>` wildcards inferred). Redis streams: go-redis (`XAdd`,
  `XReadGroup`; a read without a group is no consumer). A name is folded like a URL, struct
  fields of a composite literal included; a name that cannot be folded, a pattern topic and a
  producer whose receiver does not come from the library are unresolved or no side. A callback
  client's handler is the callback, a polling client's the function that names the topic or
  queue. `&topic` as the `Topic:` of a `TopicPartition` keeps the name a constant unless the function writes through a pointer; a `Topic` field written anywhere leaves a writer's topic unresolved. The cross-language
  fixtures carry a TypeScript producer to a Go consumer group, with a golden `trace`. The index
  is not rebuilt (no schema change).

- Rust channels (spec #30): producers and consumers of the common broker clients, on a client
  that traces back to the library's constructor. rdkafka (`send` of a `FutureRecord::to(topic)`
  or `BaseRecord::to(topic)`, and `subscribe(&[topics])`, on a client from `ClientConfig::new()`),
  lapin (`basic_publish`, `basic_consume` and `queue_bind` on a channel of
  `Connection::connect`, with the routing rules of the other languages), aws-sdk-sqs (the
  `queue_url` of a `send_message`, `send_message_batch` or `receive_message` builder; the queue
  is the last URL segment), async-nats (`publish`, `subscribe`, `queue_subscribe`; wildcards link
  as `inferred`) and the redis crate (`xadd`, and the keys of an `xread_options` whose options
  set a `group`). Names are evaluated like URLs, `format!` included; one that cannot be is an
  unresolved side. A polling or stream client's handler is the function that holds the call. A
  `let mut` that nothing else binds or assigns now keeps the object its call made, for the
  client's flow only (a redis connection is held by one). The cross-language fixture gains a
  topic a TypeScript kafkajs producer sends and a Rust consumer reads, with a golden `trace`
  down. A queue URL built from two literals the evaluation folds (`.../{queue}` given a literal)
  now names its last segment in every language. A Rust method called on a call chain whose root is
  an outside type (`client.get(u).send()` on a `reqwest::Client`) resolves to nothing, never to a
  repository method of the same name. The index is rebuilt once (schema version 36).

- Rust base URLs read from the environment (spec #30): `std::env::var("X")`, `env::var("X")`
  after `use std::env` (or `use std::env::var`), `dotenvy::var("X")` and `env!("X")`, also inside
  `concat!`, through `unwrap`, `unwrap_or`, `unwrap_or_else`, `unwrap_or_default`, `expect`, `?`
  and string methods such as `trim_end_matches`. A leading URL term that comes from exactly one
  such read is the base URL variable `X`, linked through `base_url_env` like the TypeScript and
  Python ones; Meada never reads the value. `option_env!` is not read, an `env` the file defines
  or one with no `use std::env` is no read, and a second read in a fallback leaves a hole. A
  cross-language fixture (a TypeScript `fetch` reaches an axum handler, a reqwest call reaches a
  Next.js route, through a variable and through a declared host) covers `trace` down, up and
  from uncommitted changes. The index is rebuilt once (schema version 33).
- Cargo packages (spec #30): each `Cargo.toml` with a `[package] name` provides
  `package::cargo::<name>` (every workspace member is its own provider), and each entry of
  `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]` (also under `[target.*]`,
  `workspace = true` read from `[workspace.dependencies]`, `package =` naming a renamed crate)
  links to the provider: by `path` to the crate at that path, in its own repository or, when the
  path leaves it (`../other`), in another repository of the workspace (resolved from the text
  and the repositories' roots, reading nothing outside them), by `git` to the repository whose
  identity is the normalized URL, by version to the workspace provider of the name. The link is
  `exact`, the reason `package::cargo::<name> <range> (local <version>), range unchecked`;
  `version.workspace = true` reads `[workspace.package]`. A `use` of the crate (hyphens as
  underscores; the `[lib] name`, or the renamed key) reaches the provider's public items through
  its `pub use` re-exports, `exact`, the reason ending `; import crate name`. A dependency
  outside the workspace adds nothing, and so does one whose `path`, `git` or `package` is empty
  or mistyped; `Cargo.lock` and crates.io are never read. The index is rebuilt once (schema
  version 26).
- Rust conditional compilation (spec #30): only the default configuration of a crate is
  indexed, for a fixed `x86_64` Linux target (`unix`, `target_os = "linux"`,
  `target_family = "unix"`, `target_arch = "x86_64"`, `target_pointer_width = "64"`,
  `target_endian = "little"`, `target_env = "gnu"`, `debug_assertions`, `test`; any other
  predicate is false), so the result never depends on the machine. An item under a
  `#[cfg(..)]` that is false is left out, `all`, `any` and `not` included; `feature = "x"` holds
  for the features in the crate's `[features] default`, followed through features that enable
  others (`x/feat` also enables an optional dependency `x`). A `mod x;` that is cfg-false does
  not read its file, and neither does one whose file opens with a false `#![cfg(..)]`.
  `test` is false (`cfg(not(test))` items are indexed), but a `mod` under exactly `#[cfg(test)]`
  is indexed too; `#[cfg_attr(..)]` leaves the item in. Toggling a default
  feature in `Cargo.toml` parses the crate's files again. The index is rebuilt once (schema
  version 29).
- A Rust language pack (`.rs`, spec #30, first ticket): the repository's crates (each
  `Cargo.toml` with a `[package]`, at any depth, a workspace's members included), each built
  from its roots (`src/lib.rs` or `[lib] path`, `src/main.rs`, `src/bin/`, `[[bin]] path`)
  through its `mod` declarations (`x.rs`, `x/mod.rs`, `#[path]`, inline modules); a file no
  `mod` reaches is not indexed. Functions, structs, enums, traits, modules and `impl` methods
  (`Type.method`, `Type.Trait.method`, `Trait.method`) are symbols, and calls written as paths
  (`crate::`, `super::`, `self::`, `Type::new()`, `Self::helper()`) resolve `exact` through the
  module tree, an `impl` in another file than its type included; a path that does not resolve
  matches functions and methods by name, and one that fits several `impl` blocks is
  `ambiguous`. A name match only reaches the caller's crate and the repository's libraries it
  depends on, through their own `[dependencies]` too: never a crate it does not depend on. The
  `fn main` of each binary target (`src/main.rs`, `src/bin/`, `[[bin]] path`) is a program
  entry point with its process; a `main` elsewhere is not. Calls inside macro invocations are
  not indexed. No Rust toolchain is run. A config file of any pack (`tsconfig.json`,
  `pyproject.toml`, `Cargo.toml`) larger than 1 MiB is not read.
- Rust `use` declarations (spec #30): paths from `crate::`, `super::`, `self::`, a module in
  scope or a crate name, groups, renames, globs (`use super::*` takes the private items too,
  any other glob the public ones) and `pub use` re-exports at any depth, with cycles cut; a
  `use` in a module written in place or in a function binds and hides there alone, and a call
  through a `use`d name, a renamed one or a glob is `exact` when one definition is found. A
  crate outside the repository (`std`, `serde`) is external, never name-matched, and another
  crate of the repository is reached by its name with hyphens as underscores, from a crate whose
  `Cargo.toml` lists it as a dependency (by key or `package =`). A library's exports are its
  `pub` items reachable from `src/lib.rs` (or `[lib] path`) through `pub` modules and `pub use`;
  `pub(crate)` and narrower items stay inside the crate, and a binary crate exports nothing.
  The index is rebuilt once (schema version 24).
- Rust method calls (spec #30): `x.m()` reaches the method in the `impl` blocks of the
  receiver's type, `exact` when one fits, when the type is written in the source: a `let` with
  a type or holding a call of a function whose return type is written (`Self` included), a
  parameter (`&T`, `&mut T`), a field through `self.field`, or `self`. A method of `impl Trait
  for Type` is found through the type; a `dyn Trait`, an `impl Trait` or a generic bounded by
  one trait stops at the trait's method (required methods of a trait are symbols now) and never
  searches the implementations. A receiver whose type is not written goes by name, `inferred`
  with one candidate and `ambiguous` with every one, among the crates a name match reaches; a
  type from outside the repository (`std`, another crate) is opaque, and its methods are not
  matched by name. A type of the repository whose `impl` blocks lack the method (a `derive`, a
  blanket or outside trait), written on the receiver or as a path (`Store::default()`),
  resolves to nothing rather than by name. The index is rebuilt once (schema version 25).
- Rust actix-web and Rocket providers (spec #30): a function with a route attribute
  (`#[get("/x")]`, actix-web's `#[route("/x", method = "GET")]`) or one named by
  `.route(path, web::get().to(h))` provides `http::<METHOD>::<path>`, served under the prefixes of
  what registers it: `App::new().service(h)`, nested `web::scope(prefix)`, `web::resource(path)`,
  a `.configure(f)` function, and Rocket's `build().mount(prefix, routes![a, b])`. They go
  through the same mounts and prefixes as axum. A handler no code of its crate refers to (of any
  crate, when its file is public in a library) is served at its own path: a name only bound (a
  `let`, a parameter, a field) or a reference a local of the name hides does not count. One
  that code Meada does not read refers to (`services![..]`, `vec![..]`, `concat()`, `cfg_if!`,
  an alias, a use before a `let` of the name) may be registered there, and is served nowhere,
  as it is under a scope or mount prefix Meada cannot read; a file too large to tell what it
  refers to does so for the handlers of its own crate alone. `.route(path, web::get().to(h))`
  with a handler Meada cannot resolve is still a provider, at the file site: the path and the
  method are certain, the handler is unknown. A `ServiceConfig` passed on to another function
  is followed; `#[macro_use] extern crate rocket;` in the crate's root (wherever `[lib] path`
  puts it) brings Rocket's attributes into every module of the crate. A route attribute is no
  call: it never links to a function of the repository, by name or by path. `{id}`, `{tail:.*}`, `<id>`, `<path..>` and `?<q>` are params. The Rust pack
  now emits route attributes as calls that decorate their function, and reads `routes![..]`. A
  crate path (`rocket::build()`) is no longer hidden by a function of the file with the same
  name. The index is rebuilt once (schema version 35).
- Rust axum providers (spec #30): a route of a `Router::new()` chain (`route`, `nest`, `merge`)
  provides `http::<METHOD>::<path>`; `get(a).post(b)` gives one provider per method, `any` gives
  `ANY`, and a closure handler is an inline route. A router is served under every prefix it is
  nested under (one router under two prefixes is two providers), through functions in other
  modules, constants and `let`s, as the Express ones are; `:id`, `{id}`, `*rest` and `{*rest}`
  are params. A router nested under a prefix Meada cannot read, or mounted on a field, is served
  nowhere; a router nothing nests is served at its root. The Rust pack now emits call
  arguments, constants, return values and closures as values, and a call in a builder chain
  keeps its receiver (`Call::receiver`), so the value flow follows `Router::new().route(..)`.
  A router handed to a call Meada does not read (`route_service`, `fallback_service`, an
  outside function wrapping it, a method found only by name) is served nowhere, a router held by a `let mut`
  or a name bound or assigned again is served nowhere too, `{{` and `}}` stay literal braces, and the method
  of `get as fetch` is `GET`. `prefixes` (also Express, FastAPI and Flask) reads a router
  nested past 64 mounts, or through more than 4096, as served nowhere. The index is rebuilt
  once (schema version 30).
- Rust HTTP consumers (spec #30): `reqwest::get`, `reqwest::blocking::get`, a `Client` (from
  `Client::new()`, `Client::builder()...build()?`, the `blocking` ones, a parameter the callers
  pass one to) with `get`, `post`, `put`, `patch`, `delete`, `head` or `request(Method::X, url)`,
  and `ureq` (`get` and the rest, `request("X", url)`, an `Agent`) are `http` consumers, with
  the client wrappers followed through their callers as the Python ones are. The URL folds
  from literals, `const` and `static` items (also imported through `use`), `+`, `String::from`,
  `.to_string()`, `format!` and `concat!` (`{}`, `{0}`, `{name}`, inline captures; a format
  spec is a hole) and from a `let mut` String the function only grows with `push_str`, `push`
  and `+=`. A URL that does not fold is an unresolved consumer. A value a client hands back
  (a response, a JSON value, a method of a trait the crate defines) is no client: only the
  library's own builder methods pass the object on. The index is rebuilt once (schema
  version 32).
- A Python language pack (`.py`, spec #24): definitions, imports (relative, packages, `import
  *` with `__all__`, source roots from `pyproject.toml`) and calls, with receivers resolved
  through `self`, annotations, constructor calls and Python's method resolution order.
- Python HTTP providers (FastAPI, Flask and Django URLconfs, with their prefixes, mounts and
  `include()`s, and the methods each view serves) and consumers (`requests`, `httpx` and
  `aiohttp`, with a client's `base_url` and client wrappers read at each call site), linked
  to TypeScript repositories both ways. A base URL read from the environment links through
  `base_url_env`. A router or URLconf Meada cannot place, and a route with a part it cannot
  read, stay out of the map rather than being guessed.
- Python message channels, linked to the same channels in TypeScript both ways: Kafka
  (`confluent-kafka`, `kafka-python`), RabbitMQ (`pika`, with exchanges and bindings), Amazon SQS
  (`boto3`), NATS (`nats-py`, wildcards `inferred`) and Redis streams (`redis-py`: `xadd` and the
  streams of `xreadgroup`). A call counts only when its client traces back to the library's
  constructor; a channel name Meada cannot evaluate is an unresolved side, never linked.
- PyPI packages: a named `pyproject.toml` (`[project]` or `[tool.poetry]`) provides
  `package::pypi::<name>` (PEP 503 normalized), and dependencies in `[project] dependencies`,
  `[tool.poetry.dependencies]` and `requirements*.txt` that name a workspace package link to it,
  with the range as written and `range unchecked`. A Python import of one of its modules crosses
  into the provider's symbols: `exact` through the distribution's own name, `inferred` through
  another top-level module, `ambiguous` when several distributions expose it. An npm and a PyPI
  package of one name never link. The index is rebuilt once (schema version 23).
- Python program entry points, each with its process: the `if __name__ == "__main__":` block of a
  file (symbol `__main__`, its range the `if` block) and each function a `[project.scripts]`
  (or Poetry `[tool.poetry.scripts]`) target names, mapped to its file through the package's
  source roots. Only public names (no leading `_`, or those of a literal `__all__`) are export
  entry points, though an import still reaches the others.

### Changed

- Coding standards class each string table a language pack holds (syntax, the language's
  runtime, or a decided exception), and a test fails on a new table until it is listed with
  its class.
- A Kotlin `HttpClient` held through more locals than `LOCAL_DEPTH` (8) is now a consumer:
  the shared value flow follows the `val`s the pack binds (`Facts::bound`, issue #110), up to
  its own depth (`EVAL_DEPTH`, 256 more). Before, such a chain of locals gave no consumer.
- The repository is a Cargo workspace (ADR 0014, #296): beside the `meada` binary, the library
  crate `meada-core` holds the schema of a workspace manifest's `[[repository]]` entries with
  every check their text decides, the bundle and generation ids of ADR 0012 and the new
  workspace version id. It builds for WebAssembly (feature `wasm-bindgen`, which the binary never
  enables) for the management console's front end. Both crates share one version, and
  `meada-core` is published to crates.io before `meada`, so `cargo install meada` keeps
  working. Nothing a user sees changes: every message, golden and dependency of the default
  build is what it was.
- `meada-core` also holds the export's types, the results of `search`, `context` and `trace`
  with their text answers and the token budget, and shape drift (ADR 0009) as `context`,
  `search` and `status` show it (#297): the binary writes every export and renders every answer
  through them. Its `wasm-bindgen` feature exports `parseExport`, which reads an export into
  those types and writes it again, or names the field that does not fit. Every export, `search`,
  `context` and `trace` golden is what it was, and an export read and written again is the same
  bytes.
- The default model of the `decisions` decider is now `typesafe/jev-1.14` (was `typesafe/jev-1.13`);
  `MEADA_DECIDER_MODEL` still overrides it. The request and answer shapes are unchanged.

### Fixed

- `meada mcp` answers about the client's first MCP root (in Claude Code the session's directory,
  also in an app-made worktree) and falls back to its own directory without roots (#192).
- `meada mcp` no longer waits forever on a client that stops answering `roots/list`: after 5 s a
  call answers about the last root that client sent, with one line on stderr, or returns a tool
  error naming `roots/list` when it sent none (#282).
- `meada mcp` treats a `roots/list` that fails (an error answer, a closed transport, an
  unexpected response) as one that times out: the last root with one stderr line naming the
  error, or a tool error; it no longer answers silently about its own directory (#285).
- A root `tsconfig.json` (or `jsconfig.json`) that is not valid JSON, or is larger than 1 MiB,
  no longer stops indexing: it declares no `paths`, as a `pyproject.toml` that cannot be read
  declares no source roots. A root config that is a symbolic link is not read, as the walk
  follows none.
- A manifest or config larger than 1 MiB (`package.json`, `pyproject.toml`,
  `requirements*.txt`, `tsconfig.json`) is no longer read whole on every refresh: only its first
  1 MiB and one byte are read, and it still counts as empty.
- A provider document (OpenAPI, AsyncAPI) larger than 16 MiB is no longer read and parsed
  whole: only its first 16 MiB and one byte are read, and `meada status` reports it unreadable.
- A name re-exported through a crafted chain of thousands of files no longer overflows the
  stack while indexing: past 64 files it is unknown, and no other import or `export *` binds
  it instead.
- A receiver whose class is named through a crafted chain of thousands of receivers
  (`x: y.C`, `x = y.C()`) no longer overflows the stack while indexing: past 64 receivers its
  class is unknown.
- A name that a later star import (`from a import *`, `export * from`) binds to a constant or to
  something from outside the repository is no longer linked `exact` to an earlier star import's
  function of that name. A Python module that star-imports one name from several modules
  re-exports the last one's, as Python binds it.
- A Python name defined or assigned before a star import of a module of the repository that
  exports it (`def x(): ...`, then `from a import *`) is linked to that module's from a
  function, as Python rebinds it, and no longer to the earlier definition. Code that runs while
  the module loads (its top level, a class body, its main guard) may run before the star import,
  and links neither. A name bound again after the star import (`with ... as x`, `for x in`,
  `except ... as x`, `:=`, a `case` capture, unpacking, `+=`, a function's `global x`) keeps
  that binding, and a star import of a module whose `__all__` is computed rebinds a name to
  nothing known.
- A Python top-level name bound again after its definition (`def x(): ...`, then `x = 1`, an
  import of `x` or any other binding, a function's `global x` included, a `def x()` under it
  too) is no longer linked `exact` to that definition: the last binding decides, and a call
  reaches the definition by name match at most. So is a function's nested `def` or `class`
  bound again in that function, or by a nested function's `nonlocal` (a `def` under it too). A definition under `if`, `try`, a loop, `with` or
  `match` may not run, so after another binding of its name (`try: from collections import
  OrderedDict`, `except ImportError: class OrderedDict(dict): ...`) it leaves the name to no
  `exact` link either. After `del x` the name reaches nothing it was bound to before, at the top
  level or in a function. A module's `__all__` grown by literal names (`+=`, `.extend`,
  `.append` in a statement of its top level) gives a star import the names it adds, and one
  changed any other way (under `if`, `try` or a loop, in a lambda or a function, an item set
  or deleted) is read as computed, so a star import of it no longer takes a stale set of names.
  The index is rebuilt once (schema version 37).
- A name two `export *` of one module give differently is no longer linked `exact` to the first
  one's: as in ES modules, the module exports neither. Two that lead to the same definition
  still export it.
- A class whose base is named through a member of a class with a family close to 128 classes
  long (`class Z(K.D)`) no longer resolves its members differently depending on which class was
  read first.
- Values folded or traced through a crafted file no longer hang indexing or exhaust memory, in
  every language: a string doubled through forty constants (`const b = a + a`), a chain of
  thousands of constants, parameters or return values, and a lattice of functions each calling
  or returning the next twice now stop at a budget of 2^16 steps per file or 256 levels deep,
  past which the value is unknown and no origin is found through it. A client wrapped more than
  64 functions deep is read where the climb stops, its parameters unknown.
- A TypeScript method chain of a thousand calls (`a.m().m()...`) no longer costs time cubic in
  its length: the steps each call copies are charged to the file's budget for reading call
  results, and the calls past it read nothing.
- A Django URLconf chain of thousands of `include()` calls no longer overflows the stack while
  indexing: past 64 includes, the routes of the modules included deeper yield no provider.
- A file of thousands of top-level names no longer takes time quadratic in their count to
  index: each name it exports is looked up among the file's exports, and the functions and
  methods of each export entry point among its definitions, by name rather than by reading
  them all.
- A file of thousands of imports, of names in a literal `__all__`, or of Python definitions and
  additions to a list (`xs += [...]`, `xs = xs + [...]`) no longer takes time quadratic in
  their count to index: each name is looked up among the file's imports, externs and
  `__all__`, each Python definition among the names bound before it, and each addition among
  the values assigned before it, by name rather than by reading them all.
- A value followed through a file of thousands of functions or constants, or a name read in a
  function of thousands of parameters, no longer takes time quadratic in their count: each
  definition, constant and parameter is looked up by name rather than by reading them all.
- A Rust `let mut` String in a block nested deep in other blocks no longer costs, for each
  statement after it, time in proportion to the depth: its statements are read once from the
  block instead of being looked up from the root of the tree, so a thousand `let mut`s a
  thousand blocks deep no longer take most of a minute to index.
- A Rust file nested deep in blocks no longer costs, for each name it mentions, each type it
  writes and each function with route attributes, time in proportion to the depth: what the
  walks need is kept on the way down instead of being looked up from the root of the tree, so
  a few hundred `let x: T` a thousand blocks deep no longer take seconds to index.

## [0.1.0] - Unreleased

The first release: the trunk of the roadmap (ADR 0003). The maintainer sets the date when
tagging.

### Added

- Indexing of TypeScript and JavaScript repositories (`.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`,
  `.cjs`) with tree-sitter: definitions, imports (relative paths, `index` resolution and
  `tsconfig.json` `paths` aliases) and calls, each with a level (`exact`, `inferred` or
  `ambiguous`).
- A call through a local binding (`let`, `var`, `catch`, destructuring, `for...of`, nested
  parameter patterns) never resolves `exact` to the definition, constant or parameter of the
  same name that it hides.
- HTTP contracts linked across repositories: `fetch` consumers (including client wrappers and
  base URLs read from environment variables) matched to Next.js app router and Express
  providers. Every link carries a level and a reason; calls whose URL cannot be resolved are
  reported as unresolved, never linked.
- `meada.toml` with `[[peer]]` (`repo`, `path`, `base_url_env`) and `[serve]` (`hosts`). Errors
  name the file, the line and the key.
- Workspace discovery through the normalized `origin` git remote, with peers and sibling clones
  found locally. Peers that are not cloned are reported and the rest still answers.
- A per-repository SQLite index in `.meada/`, which ignores itself in git, refreshed on every
  query from files changed on disk, including uncommitted and untracked ones.
- The `meada` command with `index`, `status`, `export` (sorted, byte-reproducible JSON) and
  `mcp`. Messages follow `LANG`: Portuguese or English.
- An MCP server over stdio with three read-only tools, `search`, `context` and `trace`, whose
  answers are compact English lines under a token budget, with a chain footer on `trace`.
- Packages as contracts (spec #22): each named `package.json` provides `package::npm::<name>`,
  and each dependency entry that names a package of the workspace (any repository, its own
  included; a `workspace:` range keeps its own) links to it, flagged
  `range mismatch` or `range unchecked` when the range does not plainly accept the provider's
  version. A call to a function imported from such a package links to the function in the
  provider, across repositories, through `exports`, `types`, `module`, `main` and re-exports,
  and through the `tsconfig.json` `outDir` back to the source. `search` takes the kind
  `package` (repository, directory, version and consumers), and `context` on an exported
  function lists its consumers, from any repository of the workspace, with the range flag of
  each.
- Shape drift (spec #23, ADR 0009): Meada reports each field a TypeScript or JavaScript
  consumer reads from an HTTP response that the linked route does not return, on every link
  but an ambiguous one. Each missing path has a level: `exact` when the provider's shape comes
  from a document or a zod schema and the path from a declared type, `inferred` otherwise; the
  link keeps its own level.
  - Provider shapes, the first source that applies: the response schema of an OpenAPI
    document; the zod schema the handler's response passes through (`schema.parse(..)`, the
    `data` of `schema.safeParse(..)`, a value or return type annotated `z.infer<typeof
    schema>`); the object literals the handler returns through `NextResponse.json`,
    `Response.json`, `res.json` or `res.send`. `z.object` and object literals give fields,
    `z.array`, `.array()` and array literals `[]` elements, the zod primitives and literal
    strings, numbers and booleans scalar leaves; unions give the union of their fields,
    `.optional/.nullable/.nullish` keep a field and `.extend/.merge/.pick/.omit/.partial` change
    the set as written. A spread of an unknown value, an index signature in the type of the
    returned value, `.passthrough`, `.catchall`, `z.record`, `z.any`, `z.unknown`, `.transform`,
    `z.lazy` and anything Meada cannot read open that part: a read under it is reported as not
    checked, never as drift. Several returns give the union of their fields; a response with a
    literal status outside 2xx, whatever its form (`res.status(404).json(..)`,
    `res.sendStatus(404)`, `new Response(null, { status: 404 })`), and Express's `next(err)` are
    left out, and a response sent another way (`return ok(user)`, `res.redirect(..)`, `res`
    handed to a function, `next()`) opens the root. Constants and schemas are followed through
    module-level constants and imports.
  - Consumer shapes: the fields read from the parsed response (`await res.json()`), also
    through a client wrapper that returns the parsed body, by member access, optional chaining,
    a literal index (an element), `obj["key"]`, destructuring (nested, with defaults and
    renames), `for...of` and the element parameter of array methods (`map`, `forEach`,
    `filter`, `find` and the like). A read stops at a method call, and one below a scalar leaf
    (`name.length`) is checked up to it. A cast (`as T`), a type argument given to a client
    wrapper or to `json<T>()`, and an annotation on the receiving variable make every field of
    the type a read: interfaces (with `extends`, also from another file), aliases to object
    types, intersections, arrays and `z.infer<typeof schema>`; other forms add none, and an
    optional field (`?`, or `.optional()` in a schema) adds no drift. A name rebound in the
    function and a value passed to another function are not followed.
  - Through packages (issue #175): zod schemas, constants and types imported from a package of
    the workspace (spec #22) count like local ones, in the same repository or another. The
    package's index keeps the shape of each constant, schema and type its public files export
    (never their source) and the importer fills it at query time, so an edit to the package
    changes its importers' drift. An import that does not resolve, resolves `ambiguous` or names
    something Meada cannot read stays open on a provider and adds nothing on a consumer; shapes
    are followed through at most 8 packages, and one reached through a build output mapped back
    to source (`outDir`) gives drift at most `inferred`. Since exported constants and types now
    count as exports of a package, two `export *` that give one name differently export
    neither, as in ES modules, so a call to that name through the package no longer links.
  - Where it shows: `trace` ends a link line with `drift: missing <path>`; `context` on a route
    names its shape source (`(document)`, `(zod)` or `(literals)` after the file) and lists
    every consumer that reads fields of the response, with its missing paths and their levels
    and the reads not checked; `search` takes the kind `drift`, one line per missing path
    (consumer location, route, path and level) matched by route path or field path; `meada
    status` prints each repository's number of missing reads when there is any, in Portuguese
    and English; `meada export` carries every shape and each link's drift. The MCP server still
    has three tools.
- Community files in Portuguese, English and Spanish: README, CONTRIBUTING, code of conduct,
  security policy and issue and PR templates; a CI workflow running `cargo fmt --check`,
  `cargo clippy` and `cargo test`.
- A release workflow built with cargo-dist: from a version tag it builds binaries for Linux,
  macOS and Windows on x86_64 and arm64 and a shell installer, published to GitHub Releases.
  The crate is ready for `cargo install meada`.
- Communities (spec #31): each repository's index holds groups of symbols that call each other
  more than they call the rest, over its calls at `exact` or `inferred`, found with Leiden made
  deterministic (fixed visit order and tie breaking, 128-bit integer arithmetic, no iteration
  cap) and named after the most common directory of their members (`src`, `src#2`). `meada
  export` carries them per repository, each with its sorted members. A symbol whose only such
  call is to itself or comes from top-level code is a community of its own. No model names them.
- Entry points and processes: each route a handler serves, each channel a consumer handler
  consumes (kind `channel`, labelled `channel::kafka::orders.created`; an unresolved side too,
  labelled `channel::kafka (unresolved)`) and each exported function or method that nothing in
  its repository calls is an entry point (kind `route`, `channel` or `export`), stored in the
  index. `trace` from `process::<entry point identity>` renders the call tree that entry point
  sets in motion, across repositories, under the token budget; an ambiguous call is one
  step listing its candidates, and a symbol is expanded once. `meada export` carries each
  repository's `processes`. A language pack can declare program entry points (kind `program`);
  the TypeScript and JavaScript pack declares none.
- Communities and processes in `search` and `context`: `search` takes the kinds `community`
  (name, repository and member count) and `process` (matched by the entry point's label: name,
  `process::` identity, entry point kind and repository). `context` on a symbol adds its
  community, or `community: none`, and the processes it belongs to, found by walking upstream
  over the same calls and links a process expands (decided links count, ambiguous ones do not).
  The processes line opens with a summary (count, count per entry point kind, repositories),
  then the entries that fit; under the budget the span text is cut first, ahead of the callers,
  callees, consumers and contracts, so the line keeps its summary and as many entries as fit;
  a span with no room for its cut note is left out, to be read from the file the header names.
  The first line names the span's range, `<path>:<start>-<end>`, so a cut or omitted span can
  be read exactly from the file; it names the start alone when the file changed since indexing.

### Fixed

- TypeScript and JavaScript: a call with type arguments under `await`, such as `await
  api.get<Row>("/users")`, is read as a call of `api.get` again, so it links to its target and
  its HTTP consumer is found.

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
