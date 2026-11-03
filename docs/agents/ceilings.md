# Ceilings of the code

Every limit of the code that `CODING_STANDARDS.md` asks for (hostile input) and every other ceiling
a ticket accepted, each with its value and its reason. Detail behind the
[architecture map](architecture.md); the code wins over this file. A limit marked `ponytail:` in
`src/` is named here, and `tests/it/docs.rs` checks that it is.

## Known ceilings

- `names.rs`, `http/csharp.rs`, `channel/java.rs` (#466): two cases are not pinned.
  (1) #358: a Rabbit `convertAndSend("orders", o)` whose `o` is a parameter of a type the
  repository does not declare (`void f(Order o)`) reads as the named producer `amqp orders
  producer`, because Java's implicit `java.lang` on-demand candidates make the type an outside
  one; #358 asks for a producer with no name. `an_argument_of_unknown_type_does_not_tell_the_overload`
  fixes the observed answer, and which is right waits on the maintainer.
  (2) #354: the C# HTTP reader reads its attributes through `names::known` (the value reading),
  not `names::annotation` (the decorator reading, which a class with an outside base does not
  hide an import from); the test in `http/csharp.rs` pins `annotation` over C# facts, not that
  reader.

- `lang/kotlin.rs`, `lang/swift.rs` (review of #507): two `*_in_linear_time` tests do not catch
  a scan per item. `many_properties_and_locals_are_read_in_linear_time` (5,000 of each) and
  `many_names_and_many_members_are_assigned_in_linear_time` (12,000 names, 24,000 members) both
  passed with their lookup turned into a scan (1.5 s together, bounds 15 s and 2 s): their parse
  or setup outweighs the quadratic part at these sizes. Counting the lookups, as
  `dotnet/references.rs` counts its reads, would catch it; a bigger input would not stay under a
  twentieth of a usable bound.

- `tests/it/docs.rs` (#491): the check that a limit is marked reads only a `const` named `MAX_*`,
  `*_MAX`, `*_LIMIT` or `*_CAP` (a minimum or a test's allowance goes in `NOT_CEILINGS` with its
  reason), one line each; a limit under another name (`*_DEPTH`, `*_BUDGET`, `*_TTL`) is marked
  by hand, and only a limit-named marked `const` must be unique (each language pack keeps its own
  `VALUE_DEPTH`).

- `deploy/` (#568): the fetch scripts' awk program (Compose `fetch-pulled.sh`, the chart's
  `workspace.fetch`) reads bare and double-quoted `repo` and `path` keys, not single-quoted literal
  keys (`'repo' = ...`): a `'` would end the shell's single-quoted program.
- `deploy/` (#504): the proposal server's Helm block renders no Ingress (the operator exposes its
  Service behind their own TLS at `proposals.publicUrl`), and Compose's `proposals.workspace.toml`
  keeps `[oidc]`, `[catalog]` and `[workspaces]` commented, so the service does not start until the
  operator copies the console's sections (the local accounts profile has no proposal server). An
  Okta authorization server stamps one `aud`, so the proposal server needs a second authorization
  server as its `[oidc] issuer` (untested); the chart renders one `[oidc]` for every pod and cannot
  give it one.

- `accounts.rs` (#554, ADR 0024): a password holds `PASSWORD_MIN` (12) to `PASSWORD_MAX` (128)
  characters and a username at most `USERNAME_MAX` (64); an agent token lives `AGENT_HOURS` (8) by
  default and `AGENT_HOURS_MAX` (168) at most. `KEY_MAX` (16 KiB) bounds a key file of `[accounts]` and
  `PASSWORD_FILE_MAX` (1 KiB) `admin_password_file`, each refused past it naming the file;
  `HASH_MEMORY_MAX_KIB` (64 MiB), `HASH_ITERATIONS_MAX` (8) and `HASH_LANES_MAX` (4) bound the
  argon2id parameters a stored hash may carry (a row past one is hostile and admits nothing);
  `ui/accounts.rs`: `HASHES_MAX` (4) bounds the argon2id hashes the console computes at once,
  19 MiB each (a further sign-in or password change is a 503 `busy`, nothing hashed), and
  `LOGIN_MAX` and `PASSWORD_BODY_MAX` (4 KiB each) the bodies of `login` and `account/password`
  (413 past them); `ui/accounts/administration.rs` (#557): `ADMIN_BODY_MAX` (4 KiB) the body of
  an account's creation or change (413 past it); `TOKENS_BODY_MAX` (1 KiB, #560) the body of
  `account/tokens` (413 past it); `ui/accounts/github.rs` (#561): `GITHUB_BODY_MAX` (1 KiB) the
  body of `account/github` (413 past it), `GITHUB_TIMEOUT` (10 s) the one call to GitHub and
  `GITHUB_ANSWER_MAX` (64 KiB) its answer (a 503 `github_unavailable` past either). The lockout
  (#555, `accounts.rs`): `LOCKOUT_FAILURES` (10) consecutive failed password checks, counted under
  the account row's lock, lock an account for `LOCKOUT_MINUTES` (15), never extended by an attempt
  while it holds.

- `ui/workspaces/proposals.rs` (#346, ADR 0022): `BODY_MAX` (80 KiB) bounds a request's body
  (413), `OUTPUT_MAX` (192 KiB) a tool's answer (`too_large` with its counts, never cut),
  `CALLS_MAX` (32) the calls in flight (503 `busy`; `MEADA_PROPOSALS_CALLS` lowers it in a debug
  build for a test), `HINTS_MAX` (50) and `NOTE_MAX` (256 characters) a proposal's hints; the
  catalog tool takes the console's `PREFIX_MAX`, `CURSOR_MAX` and `LIMIT_MAX` (`ui/catalog.rs`),
  and a manifest is `named::TEXT_MAX`.
- `ui/workspaces.rs`, `workspaces.rs`, `snapshot/versions.rs`, `meada-core/src/named.rs` (#342):
  `CREATE_MAX` (80 KiB) bounds a creation's body (413), `TEXT_MAX` (64 KiB) a named manifest's
  text, `NAME_MAX` (80) and `DESCRIPTION_MAX` (500) their characters, `LIST_LIMIT_MAX` (100) and
  `AFTER_MAX` (256) a list page and its cursor (`LIST_LIMIT`, 25, a page without `limit`), `WORKSPACES_MAX` (2000) and `ENTRIES_MAX` (500)
  (`WORKSPACES_DEFAULT`, 200, and `ENTRIES_DEFAULT`, 100, are their defaults)
  what `max_workspaces` and `max_entries` may allow and what a reader of the contexts file or a
  version document accepts whatever they say now (a version document is also bounded by the
  deployment version's `VERSION_MAX`), `OPERATORS_MAX` (20) a workspace's operators (the
  console's lifecycle bodies are capped by `APPLY_BODY_MAX` (1 KiB), `RENAME_BODY_MAX` (4 KiB),
  `DELETE_BODY_MAX` (1 KiB) and `OPERATORS_BODY_MAX` (16 KiB), a delegated subject holds at most `SUBJECT_MAX` (256) bytes and `PAGE_MAX` (50)
  bounds a page of versions or history, #343),
  `CONTEXTS_MAX` (256 MiB) the contexts file `meada workspace pull` writes and `EXCLUDED_MAX`
  (128 MiB) the exclusions file; past one the file counts as hostile. Their product is what a
  refresh pulls and publishes: the single failure domain of one fleet (ADR 0021).
- `directory/read.rs`: the owner picker lists at most `PICKER_MAX` (100) group names and says
  nothing when it cut the list; the global operator narrows it by typing more of the prefix
  (#335).
- `meada-core/src/manifest.rs`: an entry's `owner` holds at most `OWNER_MAX` (256) characters
  (ADR 0015); a longer name is an error naming its line and key, wherever the entry is read
  (import, a stored version, a pull, the browser). A named entry's peer names environment
  variables of at most `ENV_NAME_MAX` (128) bytes in `base_url_env` (#342).
- `decider/mod.rs`: an item with more than `MAX_CANDIDATES` (50) candidates is skipped in decider
  mode and reported as such; human mode still offers it.
- `ui/catalog.rs` and `catalog.rs` (#341): `GET catalog` answers at most `LIMIT_MAX` (100)
  records a page (`LIMIT`, 50, without `limit`), takes a `q` of at most `PREFIX_MAX` (128) characters and an `after` of at most
  `CURSOR_MAX` (256); `POST catalog/check` takes at most `CHECK_IDENTITIES` (50) identities in a body of at most
  `CHECK_BODY_MAX` (16 KiB);
  `[catalog] max_repositories` may allow at most `MAX_REPOSITORIES` (5000).
- `meada-core/src/authorize.rs`: a refusal's `refused` list (ADR 0015) holds one `Refusal` per
  changed entry, so it is bounded by the version's entries, which `VERSION_MAX` in `src/snapshot/versions.rs` (4 MiB, S2's
  document size cap) bounds; it is not truncated.
- `resolve.rs` (`Resolver::may_declare`, ADR 0019): a base among a pack's known outside members is
  matched by its spelling, so a same-named type of a wildcard-imported outside package that
  declares an overload of the called name may give a wrong `exact`; a bare call inside the type,
  a call with a trailing closure or one without labels compares a Swift entry by its base name; an extension
  the tables do not list, in scope of a call no own overload fits by type, is missed. A base the
  tables do not list keeps every name opaque.
- `jvm/spring.rs` (`read_yaml`): the dotted keys a YAML file flattens to are charged to
  `YAML_KEY_BUDGET` (1 MiB); a file past it (a long key over thousands of children) reads as
  empty, so its `${key}` placeholders name environment variables.
- `lang/java.rs`: `DEFINITION_DEPTH` (128) bounds nested named types; deeper definitions and
  their bindings stay with the enclosing symbol. `VALUE_DEPTH` (64) bounds nested values and
  `PATH_DEPTH` (256) bounds callee segments; deeper values or callees are unknown.
  `QUALIFICATION_BYTES` (4 MiB) bounds enclosing-name bytes across syntax visits, before each
  visit's bounded metadata fanout. Pending visits share their scope strings. A file that
  exceeds this budget contributes no facts or package layout, so an incomplete overload
  family cannot produce an exact link; other files still index, and `Facts::budget` (not
  `unbuilt`) makes `meada status` name the file, as for C#. The Java hostile-depth and
  long-prefix engine fixtures exceed these limits.

- `jvm/names.rs`: `LOCAL_DEPTH` (8) bounds how many locals a JVM client reader (Ktor,
  `RestTemplate`, `WebClient`, Retrofit) follows a client through (`val b = a`, `val a =
  HttpClient(..)`) where the value flow does not reach it; past it the client is not followed
  and its calls are no consumers. The value flow follows a Kotlin `val` or a Java local the file
  never reassigns itself (`Facts::bound`), as deep as `EVAL_DEPTH`.

- `lang/kotlin.rs`: `DEFINITION_DEPTH` (128), `VALUE_DEPTH` (64), `PATH_DEPTH` (256) and
  `QUALIFICATION_BYTES` (4 MiB) bound the Kotlin pack as they bound the Java pack's, with the same
  outcome past each; the Kotlin hostile-depth and long-prefix engine fixtures exceed them. The
  budget charges each syntax node the qualified name up to its last named definition, and the
  `.@<offset>` segments of the lambdas and local functions after it only at a node that stores
  the name (a call, a definition, a binding), so nested lambdas cost what the facts keep: a file
  of deeply nested lambdas (a Compose UI, a Ktor routing file) reaches the budget at about 92%
  of the size it did before lambdas were scopes. Beyond
  the limits, what the pack leaves out (spec #27, #95, #96): a bare or `this` call inside a lambda
  other than the five scope functions', an object literal or an enum entry's body may reach an
  implicit receiver no syntax names, so it is read as a name match (`(anonymous).f`), never
  `exact`; so is one in a lambda nested in an `apply`, `run` or `with` block. Those five are
  told by name (a function of the repository called `apply` is not told apart), and a receiver
  is known when it is a name with a written type, a constructor call `T(..)` or an `apply` or
  `also` chain of them; any other (a call's result, a property chain) leaves the lambda as any
  other. A function's written return type is its `Def::result` (`Box?` and `Box<T>` name `Box`; none
  for an expression body, a type parameter or a function type), and a class and each secondary
  constructor give their class, so a call made on a call (`make().m()`, `Box().m()`, also
  through `?.`, `!!` and a property) has `Call::on` and resolves within that class's family. A
  local initialized with `make()` is a name match. A typealias is no class of the repository:
  a receiver typed by one is outside it, and resolves to nothing. A name bound twice in one
  scope with different types, or named like a property of its class or of an enclosing class (or
  of the file), holds an unknown type. A call `X.f(..)`
  is read as a constructor only when `f` is capitalized, as Kotlin names classes. An import alias
  of an extension (`import a.slug as s`) brings nothing in scope for it. A star import of a class (`import a.C.*`) reads every export of
  the class's file. An imported extension of the provider's class `p.C` is `exact` on a
  receiver typed `p.C`, though the extension's file may import another `C` over its package's. In the block of a receiver whose type is outside the repository (a Ktor
  `Application` or `Route` extension function), a constant's value and a call's receiver are
  the names outside the block, though that type may declare a member of the name; the object a
  client walk or a declared type asks is unknown there. A fully qualified function call (`example.helper()`) is a name match. A class's
  constructors, the primary one (the class's own definition) and each secondary one (a method
  `C.C(types)`, `internal` and `private` ones hidden as a function's), are the callables of its
  name, so a call narrows among them by argument count; a class with secondary constructors and
  no primary one is only those. A single import of a top-level declaration named like a package of the repository
  (`import app.config` beside package `app.config`) binds nothing, since the package keeps the
  name. A property of function type that a base class declares, invoked by bare name, is read as
  a method call. A companion's outside base leaves the companion's members unknown, so
  `Type.create()` then resolves to nothing (Opaque outside, as Java's: a base among the known
  outside members, the JDK's of Java's table or `kotlin.Comparable`, hides only the names it
  declares, which for Kotlin include the standard library's extensions on it: `use`, `coerceIn`). Kotlin files
  resolve with Java's flags, so `java.lang.Object`'s members are the implicit members of a Kotlin
  class too: a bare `notify()` in a class is unknown. A star import of a Java package from Kotlin sees its
  package-private classes. The Kotlin pack lists the same Spring configuration files as the Java
  pack (`jvm::spring::CONFIG_FILES`, spec #27, #100), so a Kotlin-only repository gets them
  through `Indexed::configs` (a mixed one lists each twice, which `Configs::read` takes as once).
  A declaration the grammar reads as an
infix call (a hard keyword as an operand, `misparsed`) leaves the whole file out (`Facts::misparsed`, which
`meada status` names), as the qualification budget does (ADR 0017).

- `resolve.rs`: `TYPE_PARTS` (64) bounds the files of one unit one partial type is read across
  (`Resolver::parts`); classes of one name in more files are each a class of its own, so a
  member lookup reads its own file and a name binding them all is `ambiguous`. A C#
  conformance test writes one type in 64 files and in 65. Definitions of one name in files of
  several units (C# projects) bind, from a file of one of those units, to its own unit's
  (`Resolver::in_unit`, at the end of `resolve_as`), as the compiler prefers the project's type
  to a reference's; from a file of a unit that declares none of them, the name binds to the
  one defining unit among those its project references, directly or through the references
  that pass on (`Facts::unit_references`, from the walk of `dotnet::references`;
  `Resolver::in_references`), and stays `ambiguous` with several or none, since a project
  not referenced may still be visible through references not read. The overloads of one
  family's member are kept whole, since they merge across units (a base in another project, a
  Swift extension in a test module): methods narrow only when two of their types share a name.

- `resolve.rs` (`labelled_args`): `LABELLED_CELLS` (65,536) bounds the pairs of a parameter and
  an argument the label matcher weighs for one call; a wider call fits no function that labels
  its parameters, so it resolves by name or to nothing.
- `lang/swift/values.rs`: `VALUE_DEPTH` (32): a value nested deeper (parentheses, a call chain,
  an argument inside an argument) is `Value::Unknown`, so a route written under such a chain is
  served nowhere. The pack's integration tests exceed it.
- `lang/swift.rs`: `DEFINITION_DEPTH` (128) and `PATH_DEPTH` (256) bound the definitions
  around a definition and the segments of a callee (past them a body keeps its enclosing
  symbol and a callee is `(navigation_expression)`); `QUALIFICATION_BYTES` (4 MiB) bounds the
  enclosing-name bytes a file's definitions, calls and bindings store, past which the file
  keeps no facts and `Facts::budget`, which `meada status` names; `ALIAS_DEPTH` (16) bounds the aliases of aliases of a function type `join`
  follows (`function_aliases`), past which a parameter written as one takes no trailing closure.
  The pack's unit tests exceed each. Beyond the limits, what the pack leaves out
  (spec #29, #117): a target's `sources:` and `exclude:`, a closure,
  deinitializer, subscript or accessor (their calls and locals are the enclosing symbol's, so
  an accessor's local hides its name in the whole type), an operator
  function, enum cases, `Self` as a type's name, a local typed by a call result or a closure
  parameter's type (#118), a typed local of a nested block, closure or accessor (a name match
  where Java and C# keep it function-wide), a `typealias` as a symbol, one written in a type
  seen from another file (its extensions included), an outside alias of a function type
  (`Ext.Handler`, which takes no trailing closure, as the brief of #411 decides for every
  outside name), and the protocols a conformed protocol inherits when the conformance is
  written on an extension. As in Java, an outside base the pack does not list among its known outside members (`NSObject`, `View`) keeps a type's own
  overloads from `exact`, and a listed one (`Codable`, a raw value type) the names it declares.
- `http/swift/client.rs`: `CLIENT_DEPTH` (8) bounds the locals, properties and constants followed
  from a call's receiver to a client; past it the receiver is no client and the call is no
  consumer. The pack's unit tests and the hostile-URL integration test exceed the URL limits
  (a `+` chain, `appendingPathComponent` chain or nested `URL(string:` past `VALUE_DEPTH` is an
  unresolved consumer). Beyond the limit, what it leaves out (spec #29, #121): see its entry.
- `lang/csharp.rs`: `DEFINITION_DEPTH` (128), `VALUE_DEPTH` (64), `PATH_DEPTH` (256) and
  `QUALIFICATION_BYTES` (4 MiB) bound the C# pack as they bound the Java pack's, with the same
  outcome past each (a call inside a definition nested too deep is `(opaque).`, resolving to
  nothing); the budget is charged each visit only for the enclosing name past
  `QUALIFICATION_FREE` (256 bytes), so a generated file of short names reads whole at any size
  (`MAX_BYTES`), and a file past it keeps no facts and `Facts::budget` (not `unbuilt`, which a
  pack sets for a file it leaves out on purpose) makes `meada status` name it (`index::export`
  reads it from the stored facts of the files with no symbols, as `Export::dropped`); `PROJECT_USINGS` (256) bounds the distinct usings `join` adds to every file of a
  project, past which the project's files keep no facts, since a partial set could make a
  name exact that a missing using leaves ambiguous; `join` marks each such file with
  `Facts::usings`, the index writer stamps the mark into its stored facts (and clears it
  once the project is back within the count), and `meada status` names the file and the
  count (`Msg::FileTooManyUsings`). The C# hostile-depth, long-prefix and
  pack unit tests exceed them. Beyond the limits, what the pack leaves out (spec #28, #107):
  a `using` inside a namespace block binds in the whole file, and a file joins its first
  namespace's group (`Facts::package`): a type it declares in another namespace binds there
  through the file (`Binding::of_namespace`) only when no file has that namespace as package,
  and by full name otherwise; the global namespace's types are seen only from the global
  namespace. A project file is SDK-style when its `Project` has an `Sdk` attribute, an `Sdk`
  element or an `Import` with an `Sdk` (any case); its properties are read as written, with no `Condition` and no import
  but a parent `Directory.Build.props` (`Directory.Build.targets` and `Directory.Packages.props`
  stay unread; the props chain is followed `PROPS_DEPTH` (8) files up, and a props file of
  more than `PROPS_ITEMS` (256) usings and references adds nothing, as an unreadable one:
  `dotnet/msbuild.rs`), and the nearest project directory owns a file (MSBuild items that
  include or remove files are not read; two project files in one directory take the first).
  Implicit usings add the SDK's own list for `Microsoft.NET.Sdk`, plus `.Web`'s and
  `.Worker`'s; other SDKs add the base list. `.sln` files are not read: they only group
  projects for display, which nothing shows. A field, property, event, primary constructor
  parameter or local hides an outer name with no type read (calls on typed receivers are
  #108's), so `Name.M()` on a property named like its type is a name match. A bare call no
  class of its family declares, and an unbound capitalized receiver (as Java's), resolve to
  nothing; an extension method's `this` parameter counts toward its argument count. A
  parameter's `ref`, `out` and `in` are not part of a written signature, so `M(int)` and
  `M(ref int)` share one id; a static constructor, a primary constructor, an operator, an
  indexer, a property accessor and an explicit interface implementation define no symbol
  (their calls are the type's). The parts of a partial type come from one project. A raw string
  literal over several lines (indented by its closing quotes) is an unknown value; plain,
  verbatim, one-line raw and interpolated literals and `+` fold.
  A configuration read is decided with the project of the file that holds the call, the
  constructor or the registration it is read for (`http/csharp/reads.rs`), so a client in a
  library project reads the library's `appsettings.json`, never the consuming app's.
  An `internal` type (one with no access modifier too) links from its own project's files
  alone (`Resolver::hidden`); `InternalsVisibleTo` is not read, so a friend project's calls to
  it get no link. A constructor or method of a public type (`internal`, `private`,
  `private protected`; a plain `protected` one stays visible) is in `Facts::private` the same way, so another project's call narrows among the
  accessible members. From a file of another project an `internal` type is not there at all, so a type of its
  name a using brings binds even from a file of its namespace. A repository extension method wins over an outside
  type's instance member of the same name (`sb.Append("x")` beside a
  `static StringBuilder Append(this StringBuilder b, string s)`), where C# picks the member
  (ADR 0018). A file sees what the projects of its repository its project references pass on up
  to `REFERENCE_DEPTH` (32) projects deep (`dotnet/references.rs`), and nothing that a project
  of another repository references. A lambda parameter written with `in` or `out` (`(in Order
  o) => o.Total()`) types nothing (#379): tree-sitter-c-sharp 0.23.5 reads the list as an
  `ERROR` node, so its receiver keeps the broad name match.

- `snapshot/pull.rs`: a generation's bundles hold at most `GENERATION_BYTES_MAX` (256 GiB)
  together, as the document records their sizes (checked before any download) and as the
  regular files extracted from them add up; at most `GENERATION_ENTRIES_MAX` (4,000,000) archive
  entries together; and each GNU long name or link, or pax header, at most
  `HEADER_EXTENSION_MAX` (64 KiB), which a scan of the headers checks before the tar crate,
  which reads them into memory whole, sees the archive. Past any of them the pull fails and
  changes nothing. A symbolic link whose target goes through another symbolic link is refused
  even when both stay inside the mirror. A sparse file is refused. The node pointer file holds at
  most `NODE_POINTER_MAX` (64 bytes); past it the pointer file is malformed.
- `snapshot/generations.rs`: `{<key_prefix>}:kept` holds at most `KEPT_MAX` (8) generations,
  since every node holds every retained generation on disk: `pointer retain` refuses past it,
  and a pull refuses a retained list longer than `keep` plus `KEPT_MAX`, naming the key and
  changing nothing.
- `snapshot.rs`: a generation document holds at most `DOCUMENT_MAX` (16 MiB), in the store and
  as a node holds it, and the store's `current` at most `CURRENT_MAX` (64 bytes); past either
  the object is refused, naming its key or file, and a pull changes nothing.
- `snapshot/store.rs`: a credential file (key id, secret, token) holds at most `SECRET_MAX`
  (16 KiB), and `ca_file` at most `CA_MAX` (1 MiB); past either the file is refused, naming it.
  An empty CA file or invalid certificate is also refused before connecting.
- `valkey.rs`: `password_file` holds at most `VALKEY_PASSWORD_MAX` (4096 bytes) and `ca_file` at most
  `VALKEY_CA_MAX` (1 MiB); past either the connection fails, naming the file.
- `postgres.rs`: `url_file` holds at most `URL_MAX` (4096 bytes) and `ca_file` at most
  `POSTGRES_CA_MAX` (1 MiB); past either the connection fails, naming the file. `schema` holds at
  most `SCHEMA_MAX` (63 bytes, Postgres's own identifier length) and `statement_timeout` at most
  `STATEMENT_MAX` (about 2.1 million seconds, `INT_MAX` milliseconds), which Postgres would refuse
  anyway.
- `postgres.rs`: SCRAM's channel binding (`end_point`) knows the RSA PKCS#1, ECDSA and RSASSA-PSS
  signatures with MD5, SHA-1 or SHA-2 (SHA-224 apart, which ring does not hash); a server certificate signed otherwise
  (Ed25519, for which RFC 5929 names no hash and Postgres binds nothing either) gets no binding,
  and SCRAM runs as `SCRAM-SHA-256` without `-PLUS`. An RSASSA-PSS certificate on a Postgres that
  cannot hash one (an old build or OpenSSL) fails `-PLUS` authentication, as libpq does.
  `tls-unique` and client certificates are not supported.
- `grant/permissions.rs`: a process holds every accepted-role row of the newest permissions set in
  memory (logins times repositories under an organization base role), loaded whole when the set
  changes; a revocation on GitHub reaches it at the next successful refresh plus at most
  `PERMISSIONS_POLL` (60 seconds), and without one at most `max_age_seconds` after the last.
- `oidc/overage.rs`: an overage answer is kept `OVERAGE_TTL` (5 minutes), so a membership change
  reaches an overage subject that late; the cache has no cap on live entries, since only a token
  that passed every check adds one, and holds at most the subjects seen in the last two TTLs
  (an expired entry is dropped when found and swept once per TTL); a failed lookup is not cached,
  so with Postgres blackholed every uncached overage request waits up to the connect and
  statement timeouts and writes one stderr line; a console whose directory cannot be read admits
  reads (the deprovisioning rule of #335), so within the TTL a user deactivated meanwhile keeps
  its cached overage groups for reads, while writes are refused (503 `audit_unavailable`).
- `oidc/overage.rs` (feature `directory`, #336): Entra overage through the directory. A token is
  in overage (`in_overage`) when it has no claim named `groups_claim` and its `_claim_names` is an
  object whose value for `groups_claim` is a string; anything else, `groups` present with any
  value included, is a token without groups and is never looked up. The groups of such a token
  are the `group_attribute` values (`displayName` or `externalId`) of the groups whose members
  include the active `directory_users` row whose `user_attribute` equals the token's
  `user_claim` value (`externalId` exactly, `userName` through `lower()`), one statement after
  the version check (`postgres::versioned`), bounded by `Config::bound`; an inactive or absent
  user has none, and a value failing `owner_problem` is skipped with one `Msg::PermissionsBadRow`
  line naming `directory_groups`. `Overage::groups` looks up at admission, on nodes (`Oidc::admit`,
  inside `block_in_place`) and in the console (`admit_console`, the token check's `check`), and
  keeps the answer in memory by `user_claim` value for `OVERAGE_TTL` (5 minutes; in debug builds
  `MEADA_OVERAGE_TTL_MS` shortens it, a test knob as `MEADA_DRAIN_MS` is): an answer of no groups
  is kept like any other, a failed lookup is not (one `Msg::OverageUnreadable` line, the token
  having no groups, whether Postgres is unreachable or its schema version unknown), and an entry
  is dropped when found expired and by a sweep of all the expired ones that a store triggers once
  per TTL (`Cache::put`). Only a token that passed every check reaches the lookup. It is the
  only lookup on a node's request path. The console's `directory_refusal` runs after
  `admit_console` and before any role is computed from the groups, so an inactive or deleted
  provisioned user is refused (`deprovisioned`) before cached overage authority is used; nodes do
  not check deprovisioning.
- `oidc.rs`: each metadata and JWKS request takes at most `FETCH_TIMEOUT` (10 seconds), a
  metadata document at most `METADATA_MAX` (64 KiB) and a JWKS at most `JWKS_MAX` (256 KiB);
  past any of them the request fails (at start, the start is refused; on a refresh, the cached
  keys stay). An identity whose path starts with `.well-known/oauth-protected-resource` is
  unreachable with `[oidc]`, since that path serves the metadata. A role name of
  `[oidc.github] roles`, and a `role_name` the role check stores, holds at most `ROLE_MAX` (100,
  in `grant.rs`) characters; a longer `role_name` from GitHub is a skipped row. The `scope`
  claim is read as a
  space-separated string; `scp` and arrays are not read. With `audience` unset, the root's 404
  takes a token whose `aud` is `public_url` itself. The caps are inclusive: a body of exactly
  the cap is read. The `oidc` build carries `rsa`, whose RUSTSEC-2023-0071 (Marvin timing) hits
  private-key operations; Meada only verifies with public keys, so it does not apply.
- `meada-core/src/manifest.rs`: a GitHub login (an entry's `added_by`, and a token's `login_claim`
  value, which `grant/permissions.rs` reads through `added_by_problem`) holds at most `ADDED_BY_MAX`
  (39) characters; a longer one is an error in an entry and no GitHub grant in a token.
- `audit.rs`: the audit grows without bound, one `<schema>.audit` row per write and refusal, until
  the operator prunes it in SQL; Meada never deletes a record, and a page of `GET /api/v1/audit`
  answers at most 100.
- `github_app.rs`: each REST request takes at most `REQUEST_TIMEOUT` (10 seconds) and reads at
  most `ANSWER_MAX` (4 MiB); an App's installations are listed over at most `PAGES_MAX` pages of
  `PER_PAGE` (100 each; 10,000 installations), the rest unseen; the role check lists one repository's
  collaborators over at most `COLLABORATOR_PAGES_MAX` pages of 100 (100,000 collaborators), past
  which the run fails, writing nothing; with `[catalog]` (#341) it lists one installation's
  repositories over at most `PAGES_MAX` pages of 100 (10,000 repositories), past which it fails
  too (a truncated catalog would revoke), and refuses a candidate set above `max_repositories`
  (at most 5000, no default), costing `ceil(covered / 100)` more requests per installation
  (every repository it covers, allowed or not) and the collaborator pages of each candidate;
  the permissions cache keeps a catalog identity of at most `CATALOG_IDENTITY_MAX` (192) bytes,
  so that the pagination cursor (at most 256 characters of base64url) can name it, and counts
  a longer row as hostile. It costs one listing per repository on the host,
  `ceil(collaborators / 100)` requests each, against the installation's REST budget (#319's
  Known ceilings), and it holds every row of the set in memory once, as Postgres receives it; a `private_key_file` holds at most `APP_KEY_MAX` (16
  KiB) and git's request at most `INPUT_MAX` (64 KiB). The time margins are fixed, not
  configured: `TOKEN_MARGIN` and `REFRESH_EVERY` (10 minutes each), the JWT's `JWT_LIFE` (9
  minutes) and `JWT_BACKDATE` (60 s), `INSTALLATIONS_TTL` (the listed installations stay usable
  an hour while listing fails), `LAST_TTL` (`throttled` finds the installation of a `get` within
  an hour), `LOCK_TTL` (30 s, a listing or mint lock and the wait for another run's) and
  `THROTTLE_MAX` (a mark lasts at most an hour). A lock that expires under its holder lets a
  second run list or mint too. The `github-app` build signs with RustCrypto's `rsa`, whose
  RUSTSEC-2023-0071 (Marvin timing) applies to private-key operations; the JWT is signed in the
  job's process, at most once per App per run, where no remote party times it.
- `mcp.rs`: the stop signal drains for at most `DRAIN` (25 seconds, no flag, under Kubernetes'
  30-second default grace period); past it the connections left are dropped, an answer still
  waiting on an index lock included, and the exit is 0.
- `ui.rs`: `meada-ui.json` is read up to `UI_VERSION_MAX` (4 KiB); a larger file counts as front
  end files built for another version, and the console refuses to start. A token check's body
  is read up to `CHECK_MAX` (16 KiB), and a playground query's up to `QUERY_MAX` (16 KiB); past
  either the answer is a 413. `GET audit` answers at most `AUDIT_LIMIT_MAX` (100) records, `AUDIT_LIMIT`
  (50) without `limit`. Without `[snapshot]` the playground and the exports serve the
  `[[repository]]` entries as read at start (a mirror missing then stays unservable until a
  restart, as on a node). Operational service reads and managed-workspace token checks share
  a two-second request bound, including the wait for one of `READERS_MAX` (4) workers;
  timed-out workers retain their slots until I/O completes, so overload starts no additional
  work. `index.rs` reads optional `index.commit.json` up to
  `PROVENANCE_MAX` (4 KiB); anything larger has no trusted indexed commit.
- `directory/scim.rs`: a request's body is read up to `SCIM_BODY_MAX` (1 MiB), and a stored
  resource (the JSON the provider sent, a group without its members) holds at most
  `SCIM_RESOURCE_MAX` (64 KiB) after every change; past either the answer is a 413. A list
  answers at most `SCIM_PAGE_MAX` (100) resources, `count` above it is lowered to it. Each request opens
  its own Postgres connection (`pool_size` is still unread), and every write holds one advisory
  lock of the schema, so the directory's writes run one at a time: per-resource locks when a
  provider's throughput needs them. A group's answer lists every member.
- `directory.rs`: `token_file` holds at most `TOKEN_MAX` (4096 bytes); past it the console
  refuses to start, naming the file.
- `ui/write.rs`: `POINTER_MAX`, `FENCE_MAX` and `APPLY_MAX` bound their respective POST bodies
  to 1 KiB; `SAVE_MAX` bounds a version save's JSON body to 4 MiB. Over-cap authorized uploads
  get 413 before any operation; unauthorized uploads are never consumed.
- `snapshot/versions.rs`: a workspace version document holds at most `VERSION_MAX` (4 MiB),
  checked both before immutable creation and while reading;
  past it the document is refused (on import, apply and pull, and read from
  `<index_dir>/workspace/`) or listed as malformed, naming its key or file.
- `render.rs`: a source file an answer reads (`render::source`, for `path:line` and a symbol's
  source) is read only through plain relative parts of its path, through no symbolic link, as a
  regular file of at most `SOURCE_MAX` (16 MiB); anything else renders as unavailable, the path
  without its line, as a missing file does. An answer keeps each file it reads, so several
  large files add up.
- `partial.rs` (#396): a query reads every symbol of a repository with a named manifest, since
  an import resolves into it through any of them (`package::Provided`, `ContractKind::
  symbols_read`), and every contract row of every repository, links deriving from them at each
  query; `trace` reads every node its walk may reach, as the walk visits them all before the
  budget cuts the chain, and `context` every node upstream of the symbol for its processes. On
  junit5 (96k calls), a `trace` up that reaches a fifth of the calls reads them through
  indexed lookups and takes longer than a whole read did (no marker).
- `engine.rs`: `search` scans every symbol. Kind `duplicate` folds no nested groups (a duplicated
  class and each of its methods are groups of their own) and has no minimum span size (#254).
- `index.rs`: every contract is extracted again and every call resolved again on any change
  (rows rewritten only when they differ), and every file whose values the last extraction read
  (`valued`) is parsed again for it: on this repository, a one-line edit of a Rust file parses
  about 530 files again (#391). An unreadable `contracts.fragments` value is read as
  no fragments, since only `refresh` writes it. A manifest or pack config file (`package.json`,
  `pyproject.toml`, `requirements*.txt`, `Cargo.toml`, `go.mod`, `tsconfig.json`, `jsconfig.json`) is read up to one
  byte past `lang::MAX_BYTES`, and a provider document up to one byte past
  `document::DOCUMENT_BYTES_MAX` (`read_up_to`), which their readers take as invalid whatever the rest
  holds, so the `manifests` and `documents` hashes and the config digest are of what was read. A
  pack config, at the root or at any depth (`**/`), is read through no symbolic link
  (`read_config`), as the walk follows none.
- `package/go.rs`: a `replace` applies to every version of the module it names (its old version is
  read and dropped, as `lang/go/modfile.rs` does for imports); a `go.work` is not read; a `vendor/`
  directory and an `internal/` package are no different from any other directory of the provider;
  a `replace` to an absolute directory leaves the module outside.
- `workspace.rs`: a sibling with an invalid `meada.toml` or `meada.decisions.toml` is skipped
  silently. Every read of a mirror's `meada.toml` and `meada.decisions.toml`, in a pulled
  generation and in a server-mode manifest's entry (`mirror()`, behind `Manifest::read`) alike,
  reads only regular files of at most `GENERATION_CONFIG_MAX` (1 MiB, `config.rs`), through no
  symbolic link (`config::read_unlinked`, refused unopened, then the opened file checked against
  the one checked), one that stays inside the mirror included; past the bound, or through a
  link, the generation or the entry is not servable, the error naming the file. A local checkout
  (`Repository::local`, the sibling scan of `Workspace::discover`) is the developer's own tree
  and follows links, and so does an operator's file (a secret, a CA, `config::read_bounded`),
  as Kubernetes mounts secrets through one. Both reads check a file and then open it
  (`read_unlinked` then compares the opened file with the checked one), so a regular file
  replaced by a FIFO between the check and the open blocks the read instead of being refused.
  Reaching the window takes write access to the directory, which already allows worse;
  repository content cannot (git stores no FIFO, and the generation extraction refuses one); a
  FIFO given as the path is refused by the check. Closing it would take
  `O_NONBLOCK | O_NOFOLLOW` and a direct `libc` dependency, declined in #548.
- `decisions.rs`: a repository serving a decision's contract from several sites links to the
  first by site; the target names no site.
- `decider/decisions.rs`: the upstream documentation gives no range for `confidence`; [0,1] is
  assumed.
- `resolve.rs`: wrapper expansion (HTTP clients, message producers and consumers) walks every
  simple path up the callers, and does not climb a call found only by a name match, so a wrapper
  method called on a receiver whose class Meada cannot tell (in TypeScript any receiver but
  `this`, a `const api = new Api()` included), or a function called by a bare name no import
  binds, is read where it stands; value flow (`Code::origins`, for a parameter's arguments) does
  not climb such a call either; a parameter captured by a closure is unknown (`Code::origins`
  does not follow it either; nor a name its scope binds more than once, or an attribute not
  assigned a call once in its class); a nested or rest
  pattern in a destructured parameter binds nothing, and a spread in the object a caller passes
  makes the names it may override unknown. `WRAPPER_DEPTH` (64): a wrapper more calls up than
  that is read where the climb stops, its parameters unknown. `EVAL_BUDGET` (2^16 per file):
  folding values (`Code::expand`, `Code::value`) and tracing them to their origins
  (`Code::origins`) charge each step to the file whose code it reads (a value folded, a part
  copied, a call a wrapper climb reaches, a constant, parameter or return value followed, a
  composite literal `Code::field_values` reads, past which the field's value is open);
  past it, that file's values are unknown, the climb stops at its calls and no origin is found
  through it, so a string doubled through forty constants (`const b = a + a`), or a lattice of
  functions each calling or returning the next twice, ends. `EVAL_DEPTH` (256): a value
  folded deeper than that, through the items of lists and objects, the constants it names and
  the arguments callers pass, is unknown, and a value traced through more constants,
  parameters and return values than that, each into the next, has no origin.
  `FAMILY_DEPTH` (128): a class family (the class and its ancestors in C3 order) longer than
  that is absent, so every member of the class is unknown (a name match at most), and the
  linearization recurses no deeper down the bases. The bases count from where the walk
  started, a family looked up for a member a base names (`class Z(K.D)` linearizes `K`'s)
  included. The families being linearized are the resolver's (`Resolver::linearizing`), so a
  lookup that begins its own walk (a glob, an alias, an on-demand import, a base, a receiver)
  sees and counts them too: a `using static N.A;` read to name `A`'s base (`class A : Inner`)
  does not linearize `A` inside itself, the member it asks of `A` is answered as in a cycle, and
  the base is opaque unless another import names it.
  `LINK_DEPTH` (64): a name followed through more links than that, each into the next (a file
  re-exporting it from another: `export { x } from`, `export * from`, a Python `from m import x`
  or `import *`, a Rust `pub use`; a Rust alias, `use a as b`, or glob, `use m::*`), is unknown
  (a name match at most), and no other import, `export *`, glob or submodule binds it in its
  place. The links count across a walk begun past a glob, an alias or an on-demand import: it
  goes on from the links already followed (`Resolver::links`). A star import from outside the
  repository (`from numpy import *`) is taken to bind nothing (`ponytail:` in
  `Resolver::imported`): a name it rebinds stays linked `exact` to the import or definition
  before it, as with the standard library's accelerator modules (`from _heapq import *` after
  `def heappush`). Where a Python star import sits is not compared with
  the code that reads a name it rebinds (`ponytail:` in `Resolver::rebound`): code run while
  the module loads (its top level, a class body there, its main guard) gets the name unknown, as
  it may run before the star import, and a function's code gets the star import's, as for every
  binding a function reads. `RECEIVER_DEPTH` (64): a receiver whose class is named through
  more receivers than that, each through the next (`x: y.C`, `x = y.C()`, or a Rust `let` that
  takes the class of a call's result on the next, `let x = y.f()`, `Class::Returned`), holds an
  unknown class.
  The receivers and the calls whose result is being read count from where the walk started, and
  across a walk begun past a glob, an alias or an on-demand import: the count is one for the
  whole lookup (`Resolver::chain`, as `Resolver::links` is for the links), so a chain that
  alternates receivers and imports costs its length, not `RECEIVER_DEPTH` per link up to
  `LINK_DEPTH`. A receiver or call met again is found in its own `Work` alone, and so are the
  families linearized so far: an import's lookup begins with none. When `FAMILY_DEPTH` or
  `RECEIVER_DEPTH` cuts a walk, or `LINK_DEPTH` cuts one begun past an import, the families
  being linearized give up and none is kept: each is linearized again when asked for, and does
  not depend on the walk that first reached it.
- `lang/go/values.rs`: `VALUE_DEPTH` (64): a value (an argument, a returned or held expression)
  nested deeper (`f(g(h(..)))`) is unknown. A name is a constant only when it is bound once in
  the function (`:=` or `var`, one name, never assigned, in a statement of the function body, not
  in a block, an `if` or a loop), so a name bound again, assigned or shadowed holds no value
  (the `Hidden` binding wins); a function returns a value only for a `return` of one expression.
  A name whose address is taken (`&p`) is no constant, except `&p` as the `Topic:` of a
  `TopicPartition` literal (confluent-kafka-go's `*string`; `ponytail:` in `Walk::body`,
  `Keep::Pointer`), unless the function, or one around or inside it, writes through a
  dereference (`*x = ..`, `*x.Topic = ..`): no alias analysis, so `*q = ..` of another pointer
  counts too. A struct literal nests as deep as `VALUE_DEPTH`; one written by position, a map and
  a literal of any other type are `Unknown`. A name bound in a block or an `if` hides
  the name in the whole function for constants (absence only: a router or path written outside
  the block after such a binding yields no provider; the receivers of the pack hold a name for a
  region, constants do not).
- `channel/go.rs` (spec #25, #82): a receiver that does not trace back to a library's
  constructor or type makes no side (a type of the repository with a `Publish` or
  `WriteMessages` method is never one). A confluent `Produce` takes the topic from
  `TopicPartition.Topic` of the message literal written in the call or held by one name; a
  message built elsewhere is unresolved. A topic starting with `^` is a pattern and names no
  topic. A Redis read without a consumer group (`XRead`) is no consumer, and `XReadGroup` gives
  the first half of `Streams` as the names (the ids are the second). A sarama `Consume` handler
  is found when its third argument is a literal or a name that holds one; any other leaves the
  consumer on the function that calls `Consume`. A queue name is the last segment of the SQS
  queue URL.
- `channel/java.rs` (spec #26, #92): a client counts only by the type the source writes for its
  receiver: one a call returns in place (`connection.sync().xadd(..)`), an interface the table
  does not list (`KafkaOperations`, `AmqpTemplate`, `RedisStreamCommands`) and a `var` with no
  readable type make no side. A listener annotation on a class (`@KafkaHandler`,
  `@RabbitHandler` methods), a composed annotation, `@KafkaListeners`, `topicPattern` and an
  `id`-only listener read no name (one unresolved consumer when the annotation is on a method).
  A placeholder that resolves to a comma-separated list stays one name. A `RabbitTemplate`
  whose exchange or routing key the code sets (`setExchange`, `setRoutingKey`) is not read:
  only the module's `spring.rabbitmq.template.exchange` replaces the default exchange; the
  one-argument forms and a JMS default destination are unresolved. `BindingBuilder` is read
  only as `bind(q).to(e).with(key)` with the queue and the exchange written as a `new Queue`,
  `new TopicExchange` or `new DirectExchange` (through constants and `final` fields); a `@Bean`
  method's parameter, `QueueBuilder`, `ExchangeBuilder` and a fanout `to(..)` bind nothing, and
  neither does a `@QueueBinding` without a key. An SQS request built in a lambda
  (`sendMessage(b -> b.queueUrl(..))`) or held in a local, and Redis streams held in a map
  variable, are unresolved. The same file reads Kotlin (see Kotlin channels in [map/seams.md](map/seams.md)), with the
  same limits.
- `channel/csharp.rs` (spec #28, #115): `CHANNEL_CLIENT_DEPTH` (32) bounds how many links a
  receiver is followed through (a local bound to a call on a local bound to another); past it
  the call is no form, and the unit test holds a client behind thousands of locals. A client
  counts by its written type or its construction only: a value a repository method returns, a
  client built by a factory the table does not list, one injected as `object` or an interface of
  the repository, and a local assigned twice are no client. The pack's `name=` bounds (one per
  write, looked up by a map, never searched) are as many as the file's writes. Left out:
  MassTransit; `SubscribeSync`, `QueueSubscribe` and `Request` of NATS.Client, and NATS.Net v2 as
  a whole; `BasicGet` and a consumer class that overrides `HandleBasicDeliver` (the handler is
  the method holding `BasicConsume`); `CreateReceiver` and `CreateSessionProcessor`; a Service
  Bus sender made in another file than the one that sends; a stream read by an array of
  `StreamPosition` (unresolved); a `TopicPartition` as the Kafka target; a topic list built as
  `new List<string> { .. }` (unresolved); `request.QueueUrl = ..` set after the construction; a
  name read from `IConfiguration` or the environment (unresolved on both sides, so appsettings
  are not read); `String.Empty` passed by a wrapper's caller to the exchange of a `BasicPublish`
  (unresolved: only the exchange the form's own call writes is read). A `+=` handler is recorded
  only for a member target (`x.Event += h`), not for `Event += h` on `this`, and for a lambda, an
  anonymous method, a name or a member access.
- `http/java.rs` (spec #26, #86): a Spring MVC route is read from `Call::decorates` on a method
  of a `@RestController` or `@Controller` class, under the class's `@RequestMapping` prefix
  (nested classes do not inherit the outer prefix). An annotation or `RequestMethod.X` counts
  only when `Code::outside` says an import from Spring binds its name: single-type, static, or
  the one on-demand candidate in the kind's table (`Outside::candidates`: `...bind.annotation.*`
  alone or beside `java.util.*`). A type of the repository with the same name (a class, an
  interface or an annotation type, which the Java pack indexes as an interface; a nested,
  same-package or repository-wildcard `GetMapping` or `RequestMethod`), a single-type import of
  another package and two candidates in the table yield no route; the kind reads the enum
  member off the written `Value::Ref`, with no table in the pack. A path folds constants and
  `+` through `Code::value`; one that does not fold, a `value` with a `path`, a class-level
  `method`, a `method` with no readable element, or a pattern with a `/` inside braces
  (`{id:a/b}`) serves nothing. An unreadable element beside readable ones of a `method` or a
  path array is dropped: the others still serve. Interface-declared mappings, composed annotations
  and `server.servlet.context-path` are not read.
  Kotlin (spec #27, #97): the Spring reader reads Kotlin files too, through `jvm_files` (`.java`,
  `.kt`, `.kts`, in the shared `Code`), with the same annotations and rules: the Kotlin pack
  writes `["/a", "/b"]` as a `Value::List` and `method = [RequestMethod.X]` as a keyword list, and
  a bare constant of a class's companion folds (`Resolver::inherited_field` reads the companion
  after the family, unless its own bases are unknown: a ceiling, since a same-named name outside
  then wins over the companion's inherited member). Several unnamed paths (`@GetMapping("/a",
  "/b")`) fill the `value` array, and `arrayOf(..)` counts as `[..]` in a path or a `method`
  when `known` says the name is `kotlin.arrayOf`. The pack writes a star import as an `inside`
  glob (it sees `internal` members), so `Resolver::on_demand` takes the outside source of an
  `inside` glob too (never the repository's names), and an outside star import
  (`...bind.annotation.*`) gives `known` its candidate, as Java's does.
  JAX-RS and Feign keep `java_files`; the `RestTemplate`, `WebClient`, OkHttp and Retrofit readers
  take `jvm_files` (spec #27, #101): Kotlin writes the same clients without `new`
  (`RestTemplate()`, `Request.Builder()`, a `Call::receiver` of `Retrofit` and no `Call::on`),
  so the `uri` and `build` steps of `clients.rs` match a chain step spelled
  `(call_expression).name`, `constructs` takes a `RestTemplate(..)` call for the written type, and
  `made` falls back, after typed and origin lookups, to `Code::local` (Kotlin's
  `Facts::bound` (Kotlin's `val`, a Java local the file never reassigns), shared with the Ktor
  reader and indexed by scope and name in `ByName`, so a
  lookup costs its scope depth) up to `jvm::names::LOCAL_DEPTH`. The value evaluation reads the
  same local (`Code::eval`, a `Binding::Hidden` or `Binding::Local` with a bound): a URL or a
  topic written through a local `val` of a function folds where the `val` is written, an
  environment read through `?:` included, as TypeScript folds a function's `const`. The pack binds every local
  `val` with an initializer, whatever its class (a factory call `WebClient.create(..)` or a
  `.build()` has none), and drops the bound of one whose name a loop variable or a `catch`
  parameter also declares in the scope; `local` then does not take the `val`'s own entry in
  `Facts::locals` for another binding (a typed one still has its receiver). A client held by such a
  local gets its base from `folded_local` (`http/java.rs`): the call it is bound to, folded by
  `Expand::Chain` where it is made, replaces the evaluated `Ref` that `sends` (`Client::Web`) and
  `declarative::base` would read as an unknown base. The Kotlin pack reads `X::class.java` as
  `Value::Member(X, "class")`, the Java class literal, and gives a call's one type argument
  (`create<Api>()`) as the same member under the keyword `lang::kotlin::TYPE_ARGUMENT`, which no
  name matches; `retrofit_clients` takes it for `create` only when `retrofit2.create` is what
  `known` says the name is. In `declarative.rs`, `builds`/`base` take the Kotlin constructor
  `Retrofit.Builder` as the chain start, and the bases of a `.kt` file expand with Spring's
  `@Value` inject (Java Retrofit bases stay as they were).
  JAX-RS (#87, `jax_rs` in the same file): a method with a verb annotation (`GET` ... `OPTIONS`)
  of a class with `@Path`, from `javax.ws.rs` or `jakarta.ws.rs`, joined to the method's own
  `@Path`; both packages through the same `known`, so `import javax.ws.rs.*` beside
  `jakarta.ws.rs.*` is two candidates and serves nothing. A `@Path`-only method and
  `@ApplicationPath` are not routes, and neither is a method of an interface (`Facts::interfaces`):
  a `@Path` interface is a MicroProfile or Quarkus client. A controller or resource class whose
  family has an outside base (`extends com.corp.Base`, top-level or nested) still gives its
  providers (#354): the callee of a decorator call (`Call::decorates`) is read as
  `Reading::Decorator` (`Code::outside_decorator`, `names::annotation`), which skips the inherited
  field pass, so an opaque ancestor hides no import from an annotation, and single-type and
  on-demand imports agree. The one miss, accepted and not detected, is a member type of the
  outside base named like the annotation (JLS 6.4.1 would put it before the import); the same
  reading serves the declarative clients and the channel listeners, in Java and Kotlin. A fully
  qualified annotation without an import (`@javax.ws.rs.GET`) is read too, as `names::known` and
  `annotation` take a dotted name whose head nothing binds as its own spelling (`written_in_full`,
  `in_full`), the repository winning: an import, or a definition of the repository named by the head
  or by the full name, keeps its reading. Only the annotations an implementation inherits from an
  interface are not read.
- `http/csharp.rs` (spec #28, #109): an ASP.NET Core controller route is read from `Call::decorates`
  on a method of a class that is a controller: `[ApiController]`, or a base that `Code::outside`
  says is `Microsoft.AspNetCore.Mvc.ControllerBase` or `Controller`. The pack ships syntax only,
  so the attribute names live here: `Route`, `ApiController`, `ActionName`, `NonAction` and `Http<Method>`, with or without
  the `Attribute` suffix, count when a `using` of `Microsoft.AspNetCore.Mvc` binds them (the one
  on-demand candidate in the kind's table, through `names::known`)
  or when written in full (`names::in_full`), and a class of the repository that the
  attribute's scope reaches (`Code::definition`, named `<Name>` or `<Name>Attribute`), or that
  has the full name written with `Attribute` (`Hiding::full`), hides the framework's. A decorator
  named `Route` that is not confirmed as MVC's leaves its class or method unread (never a route
  without the prefix it would add). The class `[Route]` templates are the prefixes; each method
  attribute with a template is a route of its method, a method `[Route]` is one of every method
  the action's method attributes name, or of `ANY` (`http::ANY::<path>`, whose link reason says
  "any method" in `http.rs`) without any, and a method attribute without template gives the
  prefix alone when the class has a `[Route]` and no attribute has a template: with no class
  `[Route]` it is conventional routing, which is not read (`[Route("")]` is the root). A
  template starting with `/` or `~/` replaces the prefix; `[controller]` and `[action]` are
  replaced on the joined path (`[action]` is the method name without a trailing `Async`, as
  ASP.NET Core does by default, or the literal of `[ActionName]`; a non-literal one serves
  nothing), `[[` and `]]` escape the brackets, any other token or a pattern with a `/` inside
  braces (`balanced`, shared with `http/java.rs`) serves nothing, and `?` of `{id?}` is dropped
  before the trunk's normalization, which reads it as a query. Case is kept, so `[controller]`
  on `OrdersController` is `/api/Orders` (ASP.NET Core matches case-insensitively; the trunk
  does not). A method serves nothing when `[NonAction]` marks it or `Facts::hidden` says it is
  not public. Ceilings: a template that does not fold to a literal serves nothing; a class with a base class of the repository (an interface is fine), a
  class that is the base of another one of the repository, and a class declared in parts
  (`Facts::partial` in several files) serve nothing, since the attributes they have or pass
  down are not followed or joined; `[Area]` and `Name=` route names are not applied;
  `SuppressAsyncSuffixInActionNames` turned off is not read; `[HttpGet("a")] [Route("b")]`
  serves `GET /b` (ASP.NET Core may serve it for every method), and a method attribute without
  template beside a templated one gives no route.
- `http/csharp/minimal.rs` (spec #28, #110): minimal API routes, through the same `Mounts` and
  `prefixes` as the other routers, whose nodes are the origins of a builder (`Code::origins`
  with `Follow::Values`, each with the calls the walk climbed through return values). A
  `MapGroup(prefix)` call is mounted on the origins of its receiver under its prefix, a
  convention of `CONVENTIONS` (`RequireAuthorization()`, `WithTags(..)`) with none, and the
  application is the root: `WebApplication.Create(..)`, or `Build()` on what
  `WebApplication.CreateBuilder`, `CreateSlimBuilder` or `CreateEmptyBuilder` made, as the
  file's usings (the Web SDK's implicit ones included) spell them. `MapGet`, `MapPost`,
  `MapPut`, `MapPatch`, `MapDelete` and `MapMethods(pattern, methods, handler)` (a list of
  literals or `HttpMethods.X`) register a route; the pattern, the methods and the handler are
  read positionally or by name (`pattern:`, `httpMethods:`, `handler:`, `prefix:`), and `?` of
  `{id?}` is dropped as for controllers. A lambda handler is an inline handler named for the
  first route it serves, a method group (`Handlers.List`) the method it names (one overload),
  anything else the file. A method of the repository hides the framework's `MapGet`,
  `MapGroup` or convention only where it answers the call (`Code::reaches`); `Build()` is the
  framework's when what it is called on comes from its builder. A route above a node whose
  origins climb one call twice (a factory that calls itself to make the group it returns) is
  served nowhere, and so is every route past `MINIMAL_NODES` (4096) nodes in a repository.
  Ceilings: a builder Meada does not read serves nothing (a lambda's parameter, as in
  `UseEndpoints(e => e.MapGet(..))`, a parameter no call reaches, a field, a builder
  reassigned), and so does a group configured through a call outside `CONVENTIONS`; `Map(pattern, handler)`,
  `MapFallback`, `MapHub`, `MapControllers` and `MapRazorPages` are not read, and neither is a
  pattern that does not fold to a literal.
- `http/swift/vapor.rs` (spec #29, #119): Vapor routes, through the same `Mounts` and `prefixes`
  and the origins of a builder as `http/csharp/minimal.rs`. A `grouped(..)` call is mounted on
  the origins of its receiver under its path components (string arguments joined with `/`; one
  that constructs a value is a middleware and adds none, but `String`, `Substring` and
  `PathComponent` construct a path, so those and any other unreadable argument serve nothing),
  and the application is the root: a call that `Vapor.Application` or
  `Vapor.Application.make` names as the file's imports spell it. `get`, `post`, `put`, `patch`,
  `delete` and `on(.METHOD, ..)` register a route whose path components are the string
  arguments (`*` and `**` are one parameter each, `http/swift.rs`), whose handler is the
  trailing closure (an inline handler) or `use:` (a function; `self.f` is `f` of the type
  around the route). Vapor calls `boot(routes:)`, so no call binds its parameter:
  `register(collection: C())` (the type constructed in the argument or in the local it names)
  mounts `Node::Boot`, the `routes` parameter of `C.boot(routes:)`, on the origins of the
  builder it is called on, with no segment; a collection registered on two builders serves each
  route under both. `routes` is followed through a `let` alias and, as any parameter no call
  binds (`Code::origins_unbound`), through the arguments of exact calls. Ceilings as the
  minimal APIs': a builder Meada does not read serves nothing (a parameter no call reaches, a
  `var`, a property), the closure form `group("api") { }` is not read, and a repository past
  `VAPOR_NODES` (4096) nodes serves nothing. Also: a helper `boot` calls
  (`mountReports(routes)`, bare or through `self`) is followed when the call is exact: a member
  `RouteCollection` does not declare is the collection's own (Known outside members), so a
  helper method of the collection is, and the routes it registers serve under the builder it
  was given (a free function helper is exact too, for the same reason);
  a `grouped` whose every argument is a call on a capitalized head with no path producer
  (`String`, `Substring`, `PathComponent`) in its callee (`AuthMiddleware()`,
  `User.authenticator()`) is the middleware overload (`Dialect::middleware`, Vapor only) and
  adds no segment, by shape: a member read argument (`app.sessions.middleware`) leaves the
  group unread, and a static call that returns a path component (`Paths.api()`) is read as
  middleware, so its routes are served under the parent prefix (accepted false positive); an inline handler
  served under two prefixes has one symbol, named by one of them (as in C#); a factory that calls itself serves nothing through
  it, and a builder it reaches by one origin among others serves its routes under the others.
- `http/swift/hummingbird.rs` (spec #29, #120): Hummingbird routes, a `Dialect` that
  `vapor.rs::providers` runs with Vapor's value flow, mounts and handler rules. The root is a
  call that `Hummingbird.Router` names, the child builder is `group(prefix)` (one optional
  string), and `on(path, method:)` takes the path first. Ceiling: there is no
  `RouteCollection`, and a controller's `addRoutes(to:)` is not read, so its routes are not
  served.
- `http/swift/client.rs` (spec #29, #121): a call is a consumer when it is `data(from:)` or
  `data(for:)` on a `URLSession`, `request(_:)` on Alamofire's `AF`, a `Session` or
  `Session.default`, or `execute(_:..)` on an `HTTPClient`. The receiver counts by its spelling
  (`URLSession.shared`, `AF`, `HTTPClient.shared`, as the imports spell them), by a written type
  (`names::typed`) or by a local, property or `let` constant that holds one, followed to
  `CLIENT_DEPTH` (8) names; a repository type of the same name hides the library's. The call is
  read whole by `Expand::Chain` as in `http/kotlin/client.rs`, so a wrapper at any depth (the
  URL, the method or the request a parameter) is a consumer, with cycle detection. The URL is
  `URL(string:)`, `URL(string:relativeTo:)` (RFC 3986 resolution: the scheme before any `/?#`,
  network paths, dot segments, the base's query dropped), `appendingPathComponent` (before the
  query), a literal, an interpolation, a `+` or a `let` constant (`static let` too, from other files and
  imported modules); a hole the evaluation cannot fold stays a parameter marker, a reference or a
  base that is a wrapper's parameter is resolved at its call sites, and a URL left unknown is an
  unresolved consumer. UIKit, AppKit, SwiftUI and Cocoa spell Foundation's names (they re-export it). `URLRequest(url:)` takes its method from an `httpMethod`
  assignment, `HTTPClientRequest(url:)` from `method`, Alamofire from `method:`; GET when absent.
  Not read: `dataTask(with:)`, `AF.download` and `AF.upload`, a request mutated through an
  `inout` or an alias, a request with `url` assigned afterwards (unknown URL), a method that is
  not one of the HTTP verbs.
- `http/csharp/clients.rs` (spec #28, #111): an `HttpClient` call is a consumer when its method
  is in `CLIENT_METHODS` (`GetAsync` to `PutAsJsonAsync`) or is `SendAsync`, and its receiver is
  an `HttpClient`: `Code::written` gives the type text written for a field, a parameter or a
  typed local, `names::is_type` confirms it in full or through the usings (an alias counts, a
  class of the repository named `HttpClient` hides the framework's), and a construction in place or a
  local made by `new HttpClient()` counts without a written type. The pack ships syntax only and
  adds no externs: C# types are not in `Facts::externs`, since `Binding::External` would turn
  the calls on them from inferred to unresolved. The call is read whole by `Expand::Chain`, so a
  wrapper (the URL, the method, the `HttpRequestMessage` or the client a parameter) is a consumer
  at each caller with the trunk's depth bound and cycle check; `SendAsync(new
  HttpRequestMessage(HttpMethod.X, url))` takes its method and URL from the request, and
  `new Uri(x)` is `x`. The pack folds string literals, verbatim, raw (one line) and interpolated
  strings (`{{` and `}}` unescaped, a hole with an alignment or format clause is a parameter),
  `+`, and `const` and `static readonly` fields, of other types too (`Facts::consts` and
  `Facts::statics`, as Java's `static final`). The pack binds every local to its initializer
  (`Facts::bound`, an `Unknown` bound for a loop variable, a pattern or a lambda parameter, so
  that a second binding of the name leaves none), and a field or property that holds a
  construction or a call. A name assigned anywhere in its file (`=`, `+=`, `++`, `out`,
  `ref`, `this.x = v`) loses its bound: it holds no one value (the rule of the minimal API
  reader, spec #28, #110), so a reassigned local, request or client is unknown, and
  `Code::local` follows a local of unknown class when the pack bound it once. A base is the
  `BaseAddress = new Uri(x)` of the client's initializer (the pack gives an initializer as the
  last argument of the creation, a `Value::Object`) or an assignment on the name the call is
  made on (a `Bound` named `<name>.BaseAddress`, in the scope that declares `<name>`, which
  `clients.rs` checks against `Code::written` so that a parameter that hides a field, or a
  nested type's own field, keeps its own); two assignments leave an `Unknown` bound, which is
  an unknown base. The assignment wins over the initializer. A relative URL joins the base as
  RFC 3986 does (`https://c/v1` and `orders` give `/orders`, `https://c/v1/` gives
  `/v1/orders`) and one starting with `/` keeps the origin; a base with no literal scheme is
  cut at the first text after its non-literal parts. `request.Method = HttpMethod.X`, set
  after the construction, is read the same way (a `Bound` named `<name>.Method`). A client with
  no readable base is a path-only contract. Named clients (`CreateClient(name)`) held by a
  local or by a field assigned from it (a `Bound` named `<field>=`, read with `Code::bounds`)
  are not consumers. `a?.M()` is a call of `M` on `a` (the grammar parses `await a?.M()` with
  the `await` around `a`; `condition` reads through it). Ceilings: a field is followed only
  when it holds a construction or a call (an instance `readonly` string is not folded, only
  `const` and `static readonly`); a name assigned anywhere in the file is unknown in all of
  it, even in another method; `new Uri(base, relative)` with two arguments, a
  `HttpRequestMessage` built with an object initializer and a raw string of several lines are
  not read.
  A member assigned by `??=` or a compound operator, or by an assignment that may not run when
  its definition does (the pack's `Visit::conditional`: inside a branch, a loop, a `catch`,
  `?.` or the right of `&&`, `||`, `??`, reset at each method, constructor and lambda), is an
  `Unknown` bound, and an assignment whose address differs from the initializer's leaves the
  base unknown (the call may come before it).
  Typed clients (spec #28, #113): an instance `readonly` field with no initializer that one
  write of its class sets, a plain `=` in a constructor (`this.f = v`, or `f = v` with no
  parameter or local `f` there; `out` and `ref`, `this.f` too, count as writes), is a constant
  of the class whose value is written in that constructor (`Const::scope`, Java's `final`
  model), so `_http = http` makes `_http` the constructor's parameter (`x ?? throw ..` is `x`).
  The pass groups the writes by class and name once. The pack gives a call's type arguments
  as written (`Call::uses`, one chain `[Step::Argument(tys)]`, the meaning shape facts give
  it), and a target-typed `new(..)` whose declared type it reads (a local, a field or a
  property declaration) is a construction call of that type, so `may_be_called` sees it. The
  kind expands with an injector for `Injected::Param` only: a parameter of a constructor no
  call may reach, declared `HttpClient`, is a client whose base is the one
  `<param>.BaseAddress` the constructor assigns when every registration of its class sets the
  same address or none does, else the one address every framework `AddHttpClient<T>(..)` of
  its class sets (`AddHttpClient<I, T>`: the last type argument; the call must reach no
  function of the repository, `Code::reaches`). A registration's address is the one
  `<param>.BaseAddress` bound of its lambda's parameters (its bindings of no class and no
  value, so a local of the lambda is none), folded and its reads decided where the lambda is
  written; one that only names the client adds none, and a method group, a lambda that passes
  a parameter to a call, or a `ConfigureHttpClient` chained on it (`Call::on`) add an unknown
  one. Two registrations with different addresses, one that does not fold, a constructor some
  call reaches, or a parameter of a method leave the base unknown. The injected value is a
  construction (`new HttpClient { BaseAddress = new Uri(parts) }`), so the base is read as an
  initializer's, and an assignment on the field still wins. Ceilings: a primary constructor's
  parameter (`class T(HttpClient http)`) is a binding of the class, no constructor parameter,
  so it is not injected; a target-typed `new(..)` passed, returned or assigned is no call.
- `http/csharp/refit.rs` (spec #28, #113): a method of an interface (`Facts::interfaces`) with
  Refit's `[Get]`, `[Post]`, `[Put]`, `[Patch]`, `[Delete]` or `[Head]` (through the usings, or
  written as `Refit.Get`, read in the scope around the interface so that a method named like the
  attribute is no type; `Hiding`, shared with the controllers, says a class of the repository
  named `GetAttribute` hides it) and a path from the root is a consumer at its definition
  (`Site::Def`), as Retrofit's are. Its bases are the first arguments of each framework
  `RestService.For<Api>(base)` whose first type argument names the interface or one that
  extends it (#367: the interfaces `Def::bases` reach, up to `BASE_DEPTH` (32) levels, each
  once), read with
  `Expand::Call` (a wrapper's callers pass it) and the reads decided in the file of `For`; one
  consumer per base, and an interface no `For` makes has an unknown base. The route follows the
  base's own path (Refit appends it). `AddRefitClient` and `For` given an `HttpClient` are not
  read. A caller reaches the method through a receiver whose type is declared (a field, a
  parameter, a typed local); `var api = RestService.For<Api>(..)` types nothing, since the
  resolver reads no library's result (the kinds run after it), so its calls stay name matches.
- `http/csharp/reads.rs` and `dotnet/appsettings.rs` (spec #28, #112): environment and
  configuration reads in a consumer's URL. The pack records what looks like a read without
  deciding what it reads: a subscript with one string literal, and a call of
  `GetEnvironmentVariable` (`ENVIRONMENT_READ`, the pack's one library form, on the precedent of
  Java's and Kotlin's `getenv`) with one, is `Value::Str([Part::Read { .. }])` (a
  neutral carrier, since only `Env` and `EnvOr` survive `unbind`; a call of any other member, as
  `MapGroup("/api")`, stays a call). Being a string, a local or a field bound to a read folds
  into the URL like any other. The reads of one node share one `Arc` of their scope. The pack
  folds `??` chains (the first operand a read and no other operand a read, so the reads count
  across the whole chain; past `VALUE_DEPTH` operands it is unknown), `!` around it, and keeps
  the members called on it in `then`, except the trims of `System.String` (`Trim`, `TrimStart`,
  `TrimEnd`: `TRIMS`, the language's own runtime), which leave it the read. `Reads::value` in the kind turns the part into `Env`,
  `EnvOr` or `Hole` with the resolver: a read whose `then` holds a member is a hole, `Environment.GetEnvironmentVariable` is the
  framework's `System.Environment` (a repository type of that name hides it, through
  `names::spellings`), and `x[".."]` is a configuration when `x` is declared `IConfiguration`
  (`Code::written`, confirmed through the usings) or is `builder.Configuration` of a
  `WebApplicationBuilder` (declared, or made by `WebApplication.Create*Builder`). A configuration
  key is the variable `Section__Key`, as the code spells it; the literal that `Settings` finds
  for it is the variable's default (`Part::EnvOr`), so `http::fetches` keeps the twin readings
  Spring's `${X:default}` has: by host when a workspace provider serves the default's host, else
  through the variable and `base_url_env`. `based_on` rebases such a default on a relative URL
  as `Uri` joins. `Settings` reads, from `Indexed::configs` (`**/appsettings.json` beside
  `**/*.csproj`, which the pack's `join` reads), the base file of the project of a source (its
  `Facts::unit`; a file in no project reads the nearest file above it) with serde_json after
  dropping comments, trailing commas and a byte order mark. The parsed document is kept as it is
  and a key `A:B` is looked up by descending its segments without case (cost: segments times
  children), never flattened to full keys; serde_json's recursion limit (`JSON_DEPTH`, 128)
  bounds the nesting, and a file past it is not read. Only strings are values. A value lives
  only during the extraction (`Facts` is not serialized), and a config edit re-extracts the
  consumers through the config digest. Ceilings: a read passed in through another file's
  wrapper call, `using static System.Environment` and a read behind a `??` fallback of a local
  are an unknown part (`ponytail:` marker in `reads.rs`); the variable keeps the key's spelling
  and `base_url_env` compares it as written, though .NET compares keys without case; a JSON key
  that holds a `:` is not matched; `appsettings.<Environment>.json`, `GetValue`, `GetSection`,
  options binding, arrays and user secrets are not read; a chain past `VALUE_DEPTH` is a hole.
- `http/kotlin.rs` (spec #27, #98): Ktor routes, through the same `Mounts` and `prefixes` as
  the other routers, whose nodes are definitions (file, qualified name): the lambda of each
  `routing { }` call (a root), the lambda of each `route(prefix) { }` call (mounted under its
  prefix on the routing scope around the call) and each extension function on Ktor's `Route` or
  `Routing` (mounted with no segment on each routing scope that calls it through the implicit
  receiver, so one called under two prefixes serves under both; one nothing calls is served
  nowhere). The routing scope of a call is the nearest such node around it through lambdas
  passed as arguments (`Kind::Inline` scopes inside a call of the scope around them, by the
  calls' and definitions' ranges), stopping at a route's handler and at a local function's
  body. `get`, `post`, `put`, `patch`, `delete`,
  `head` and `options` with a path or none (the block's path), and `route(path, HttpMethod.X)`,
  register a route whose handler is the trailing lambda (`handler_site`); the path and the method
  are read positionally or by name (`path =`, `method =`). The extension called
  is the nearest one in scope (`Code::extensions`, Kotlin's lookup order: around the call, an
  explicit import, the package, a star import, after the locals and parameters that hide the
  name) whose receiver is Ktor's `Route`, or `Routing` in a scope whose receiver is one:
  the routing block tells the implicit receiver, which the resolver's own answer for the call
  (a name match inside a lambda) does not know. A Ktor name counts only when `Code::outside`
  says an import from `io.ktor.server.routing` (or Ktor 1's `io.ktor.routing`) binds it, and
  `HttpMethod` from `io.ktor.http`; a name a star import of `io.ktor.server.resources` may bind
  too is no route, so type-safe routing is not read. Ktor parameters (`{id}`, `{id?}`, `{...}`)
  fold as actix-web's (`actix_path`).
- `http/kotlin/client.rs` (spec #27, #99): Ktor client calls (`get`, `post`, `put`, `patch`,
  `delete`, `head`, `options` and `request` of `io.ktor.client.request`, counted by the import
  `Code::outside` says binds the verb) on a receiver that is an `io.ktor.client.HttpClient`: one
  `Code::origins` (`Follow::Values`) traces to a call to it (counted by the import of its
  callee as `names::spellings` reads it, so `import io.ktor.client.*` counts), a local the
  pack recorded (`Facts::bound`: a `val x = Callee(..)` with no written type, and the parameter
  of a `let`, `also` or `use` lambda, whose value is the receiver; `Code::local` takes the
  nearest binding of the name and none when another binding of it shares its scope, so a
  parameter, a loop variable, a `var`, a local named like a property of its class, of an enclosing class or of the file, or one declared
  twice hides it) followed through `origins` again, or one whose written type is it
  (`names::typed`); a map, another library's client and a name a local hides are none. The
  call is read whole (`Expand::Chain`, which also gives the reader the call's keyword arguments),
  so an enum member a wrapper passes keeps the evidence of the caller's imports. The URL (first
  argument, or `urlString`) goes through `Code::expand`, as the Go clients do, so a wrapper is
  read at each call site of the function that determines the URL or the method, through any
  number of levels, with cycle detection; a call with no URL argument (`get { url { .. } }`) is
  an unresolved consumer. The base of a client made by a call is the `url(..)` of the
  `defaultRequest { }` in the configuration lambda of that call (Ktor 2 and 3
  `io.ktor.client.plugins.defaultRequest`, Ktor 1 `io.ktor.client.features.defaultRequest`),
  folded in its scope: a hole when it is set more than once or not by a string, none for a client
  known by its type alone. `request` sends the method its block assigns: the pack records any
  property assignment `p = v` or `this.p = v` written as a statement of a lambda argument as the
  `p=` key of that call's `Call::kwargs` (`kotlin::assigned`, a key no Kotlin name matches and the
  pack names no property; `given` in `http/kotlin.rs` and in `http/kotlin/client.rs` ignores keys
  ending in `=`), `Unknown` where the statement may not run (inside an `if`, `when`, `try` or
  loop) and, for the call that owns each lambda around a nested assignment (`headers { method = X
  }`), because the inner lambda may not run, and when the block assigns it twice. The Ktor
  client reads the `method=` key (`client.rs`). `GET` when it assigns none and the call is given no other argument, a `Ref`
  while it depends on a parameter of a wrapper, unknown for anything else (a block passed as a
  value, `HttpMethod.parse(..)`, a method that is no `HttpMethod` member of Ktor).
  The Kotlin pack folds a string literal's templates into parts (`$name`, `${name}` as names,
  any other `${expr}` as a hole); the grammar reads `$name` as text, so the pack finds the name
  in the text. A literal with an escape other than `\$` is unknown; `\$` is a `$` that starts no
  template, so Spring's `@Value("\${key}")` is the text `${key}` (#100).
  Base URLs (spec #27, #100): the client reader expands with `Code::expand_with` and
  `jvm::spring::inject` (as `http/java/clients.rs`, `Configs` read the first time an annotation
  is injected). The Kotlin pack records `Facts::annotated` for a property (a field of its class,
  `static` at the top level or in an object or a companion, where Spring injects nothing), a
  constructor parameter (`Marked::Param` of the class, and the `Marked::Field` of the property
  a `val` or `var` declares, whose `Const` is dropped when annotated, as a `lateinit var` has
  none, so the annotation is what it holds) and a function's parameter, by use-site target: none,
  `field`, `set` and `setparam` for a property, none and `param` for a parameter; `get`,
  `property` and the others inject nothing. Every assignment to a name that can be a
  property (`base = ..` of a name no local or parameter declared before it, `this.base += ..`, `base++`, `o.base = ..`)
  is a `.name` constant, or `.name@class` when the pack knows the class, as Java's, which
  `Code::field_written_in` reads; an assignment to a local or a parameter is not one, and a classless write
  matches the name in every class: a field some code of its class assigns holds no injected value. `value()` reads
  `System.getenv("X")`, `java.lang.System.getenv("X")` and `System.getenv()["X"]` as `Part::Env`,
  through `!!`, parentheses, `?:` fallbacks and the string methods of `STRING_METHODS`
  (`trimEnd`, `removeSuffix`, ...), unless the file binds `System` to another type
  (`SYSTEM_BOUND`, as Java's); each node is read once, a string method passing on the
  `Part::Env` its receiver already evaluated to. A fallback that reads the environment too (a
  `System.getenv` call, or a property of the file whose declaration holds one, `ENV_NAMES`) makes
  the term a hole: exactly one read. Adjacent literal parts of a `+` join, so `"\$" + "{key}"`
  is a placeholder. The variable maps through `base_url_env` as in the trunk. A function-local
  `val` is no constant in the Kotlin pack, so the read must stand in a property or the URL
  expression.
- `http/java/clients.rs` (spec #26, #88): a RestTemplate or a WebClient counts by the type the
  source writes for the receiver (`Follow::Declarations`), or, for a WebClient, by the
  `WebClient.create` or `WebClient.builder()` chain it traces to (`Follow::Values`); a name
  imported on demand counts through its one candidate in the kind's table (`Outside::candidates`,
  now also for a head only the file's outside names bind). A WebClient's base is read off the
  folded chain (`Expand::Chain`), through constants, `final` fields and a wrapper parameter's
  arguments; a client held in a local has an unknown base, so the consumer links by path. The
  Java pack keeps a `final` instance field's one value (its initializer, or the single `this.f =
  v` or `f = v` of all its constructors; two give none) as a `Const` of its class, outside
  `statics`, a constructor's value read in that constructor (`Const::scope`): its parameters
  are what Spring injects (#89), else unknown. A builder
  (`HttpRequest.newBuilder()`, `new Request.Builder()`) is read only as one chain ending in
  `build()`; one held in a variable between steps is no consumer, and a chain folded short of
  its start names no method. `String.format` puts each `%s` or `%d` argument in place (a
  parameter keeps the wrapper climbing, anything else is a hole) and reads `%%`; any other
  conversion leaves the URL unread. A `HttpMethod.X` names a method only when one spelling
  (`Outside`) is Spring's.
  Bases from configuration (#89): the reader expands with `jvm::spring::inject`, so a field or a
  constructor parameter with Spring's `@Value("${key}")` (bound by an import, as for the
  annotations of #86) holds `key` from the `application.properties`, `application.yml` or
  `application.yaml` of its module's `src/main/resources`, in that order of precedence (a file
  past `lang::MAX_BYTES` is not read, nor a `.properties` past its first `#---`). A source's
  module is that of the deepest main source root a Maven or Gradle build declares for it
  (`Indexed::manifests`, a Gradle source set's `srcDirs` included), else the one whose
  `<module>/src/main/` it is under. The pack records every annotated field with whether it is
  `static`, and `spring::inject` gives a `static` one nothing; a field some file assigns, and
  one a parameter, a local or a same-name binding hides, hold nothing too. A placeholder no key fills is the variable it
  names (`Part::Env`), and `${X:default}` keeps both (`Part::EnvOr`): `http::fetches` reads
  such a URL through `X` and, when the default names a host, through that host too, the twins
  marked in their `via` (`env:X|host:<host>` and `host:<host>|default`). `Http::workspace_step`
  (`keep_applicable`), after every `declare` filled `Provider::hosts`, keeps the one that
  applies, its mark dropped: the default's when a provider of the workspace serves its host,
  else the variable's. Links, `context`, `trace` and the open items see one consumer per call.
  The Java pack reads `System.getenv("X")` (or `java.lang.System.getenv`) as `Part::Env`,
  unless the file imports or may declare another `System` (`binds_system`, a word after
  `class` in a comment counts; `import java.lang.System;` does not). Profiles, relaxed binding,
  YAML lists and a SpEL `#{...}` value are not read. The configurations are read once per
  extraction, the first time an annotation is injected.
- `http/java/declarative.rs` (spec #26, #90): a Feign or Retrofit interface method is a consumer
  sited at the method (`Site::Def`, built by `http::consumer_at`), not at a call: its callers
  reach it as any call to a definition. Feign: `@FeignClient` from
  `org.springframework.cloud.openfeign` (bound as the Spring annotations of #86, through `known`)
  on an interface; `url` (empty is none; without a scheme it gets `http://`, as Spring Cloud does) and `path` resolve with `jvm::spring::resolved`
  over the module's configuration, the methods' mappings are `request_methods` and `paths` of
  `http/java.rs`, a `@RequestMapping` with no `method` meaning GET, as Feign sends it. Without
  `url` the consumer's `via` is `client:Feign client <name>`: `http::CLIENT_VIA` is the prefix of
  a client that names itself, the rest the words the reader writes, and `Http::link` words the
  unresolved host as `host unresolved (<words>)`, so the shared code names no library; a name with
  `|` is refused, it would collide with the twin marks. A `@RequestMapping` on the interface,
  `@RequestLine` and other contracts give nothing. Retrofit: the verbs of `retrofit2.http`
  (their names collide with JAX-RS, so `known` accepts both spellings and the result must be
  Retrofit's); the base is the folded `Retrofit.Builder().baseUrl(..)` chain of each
  `retrofit.create(Api.class)` (`Expand::Chain`, climbing wrapper callers while the base holds a
  `Part::Ref`), the receiver proved Retrofit by its type or by its `new Retrofit.Builder()`
  chain; one consumer per creation, none gives an unknown base. Its annotations name the path by
  `value` alone, and a route with a brace open past its segment is not read (`balanced`, as for
  Spring, JAX-RS and Feign). A route from `/` replaces the
  base's path (read over the base's leading text parts, so `HOST + "/v1/"` counts), and an empty
  route is Retrofit's dynamic `@Url`: a consumer with no contract. A consumer at a method is an
  open item like a call's (`items::list`): its location is the method, its expression the method
  on one line, and its enclosing symbol the method itself, so `meada decide` sends the method's
  lines as the minimal excerpt (ADR 0006). The Java pack reads `Api.class` as `Value::Member("Api", "class")`.
  `ContractKind::calls_from_definition` (true for HTTP) tells `trace` that a consumer at a
  symbol is a call to follow, unlike a channel handler, where the link ends.
- `http/go.rs` (spec #25): a route is read only with a literal path and a literal prefix; a
  group prefix or a pattern Meada cannot read serves nothing. `net/http` reads `{name}`,
  `{name...}` and `{$}` alone: a `:name` segment is a literal no contract can spell, so its route
  has no provider, and `*` stays literal; gin and echo read `:name` and `*rest` as parameters.
  A `ServeMux` is mounted only as the handler of a `Handle(pattern, mux)` (or a function
  returning it) on a router Meada reads; it keeps its own paths, served for the pattern's
  subtree (a trailing `/` or `{name...}`, else the exact path) and method alone, so a route
  outside the pattern, or of another method, has no provider. A mux held by any other argument of
  a route call (`http.StripPrefix`, `echo.WrapHandler`, `gin.WrapH`, any middleware), or given
  to a router held in a field, a map or a slice, is an unread mount: served nowhere. A mux
  nested deeper than `PREFIX_DEPTH` or through more mounts than `PREFIX_WORK` is served nowhere
  too. `http.StripPrefix` and other middleware that rewrites paths are not modelled (spec #25).
  Middleware (`Use`, extra handlers) has no effect on a gin or echo path. A router held in a
  struct field, a map or a slice is not followed (no marker).
  chi: `*` is a parameter segment, `:name` a literal no contract can spell (no provider);
  `Mount(prefix, router)` follows the router like a group, any router inside a call of the
  handler argument (a middleware), and a `ServeMux` given to `Mount`, are unread mounts (chi's
  `Mount` leaves the request path alone, so the mux would see its full path under the mount's
  prefix: combining those chains with the router's prefixes is not modelled); `With(..)` is not
  a group. A `Route` or `Group` function binds its first parameter to the group of that call,
  and the group is followed from there through exact calls (`users(r)`) and names bound once
  (`sub := r`), at most `PREFIX_DEPTH` hops and `PREFIX_WORK` parameters (past it the group is
  served nowhere); a call found only by a name passes nothing on, and a parameter
  rebound anywhere in the function hides it in the whole function. gorilla: `.Methods(..)`
  restricts the call it is chained on through `Name`, `Schemes`, `Headers`, `Queries` and
  `Host`; arguments Meada cannot read, no argument and a second `Methods` serve nothing;
  `Path(..).HandlerFunc`, `NewRoute`, `Host` subrouters and `StrictSlash(..)` chains are not
  read (a router made by a call that is not `NewRouter` has no provider).
- `http/go/clients.rs` (spec #25, #79): `CLIENT_DEPTH` (64): a client or a request is followed
  back through that many calls, names and struct fields (`New().SetX().R().SetY()`, `b := a`,
  `c.rc`); past it the base is unknown and the request is no consumer (the chain is no longer
  known to be resty's). A request on a client only written as a type (a parameter, or a field
  no composite literal sets) is a consumer with no host, and so is one on a parameter some
  caller passes an untraced client to (the base is unknown). A field's literals are those of
  its type: a literal of an alias or a defined type (`type B = T`, `type B T`) written by
  position, and a conversion from another struct type (`T(other)`), are not seen as setting
  the fields of `T` (`Code::field_values`), and a literal nested deeper than `VALUE_DEPTH`
  inside literals written without their type sets the fields of a type not read. Only the
  libraries of the table (`net/http`, resty v2 and v3, `resty.dev`) are
  clients; a client assigned more than once, one held in a map, a slice or a channel, or an
  interface, is untraced and its requests are no consumer. A `fmt.Sprintf` format with a verb
  other than `%s`, `%d` and `%v`, a count of verbs and values that differ, a `var` assigned
  anywhere but its declaration (or written through a selector), fold to nothing: the consumer is
  unresolved (no marker). A verb with flags, width or precision is a hole, and so is `%d` of
  anything but an integer literal, and `%s` of an integer literal. `values::Std` folds
  `fmt`, `net/http`, `os`, `strings` and `aws` only where nothing else in the file spells the
  name they are imported by (`hidden`: a parameter, a local or a function of that name in any
  function of the file leaves every call of the package in the file unfolded, whatever its
  scope).
- `lang/go.rs`: `PATH_DEPTH` (256): a callee keeps that many selectors of a chain (a longer
  one is its node kind) and a receiver's type is read through fewer `*`, type arguments and
  parentheses (past it the method is no definition, and a parameter, field, `var` or result
  type read through more names no class). Receivers (`Walk::bound`, `finish`): a name is a
  `Facts::receivers` row only when every binding of it in its function (parameters, the method
  receiver, named results, `var`, `:=`) writes the same class, one scope per function and per
  literal, so a name bound again to another type (in any block) or by `range`, a type switch or
  a second result is a local that hides, and so is a name whose type a local of the function
  hides (a local `type`, a type parameter; a type declaration's own parameters hide only in
  the declaration, not in its methods). A `:=` or `var` holds from the end of its statement to
  the end of its block, clause or statement, and the name is a receiver only if every call that
  starts at the name in its function (and in the literals in it) lies inside the regions of its
  bindings; a call before it, or after the block, may mean an outer name, so the name stays a
  hiding local. A struct's named fields are receivers in the type's
  scope; an embedded field, and so a promoted method, is not followed (a name match).
  `Class::Called` is a call read as a value (`h.Counter()`, `shapes.NewCircle()`), the first
  result only. `DEFINITION_DEPTH` (128): a function
  literal nested deeper is no definition (its parameters and calls are the enclosing one's).
  `CONSTRAINT_DEPTH` (64, `lang/go/build.rs`): a `//go:build` expression nested deeper (`!`,
  parentheses) is not read, and the file is left out, as one that cannot be read (two
  `//go:build` lines, not UTF-8) is. Build constraints hold for one target (`linux`, `amd64`,
  `unix`, `gc`, every `go1.N`; `cgo` and any other tag are false, and a file that imports `"C"`
  is left out); a legacy `// +build` line
  alone is not read. A plain import binds the name the Go tools assume for its path
  (`assumed_name`: the last element, the one before a `vN`, without `go-`, up to the first
  character no identifier holds), not the package clause of the directory it names, so a
  package whose clause differs from its directory is reached by its alias alone, and a name two
  imports of a file give is unknown (`unbind_clashes`: an import of it from the empty
  specifier, which names nothing), so a call through it links to nothing. An import of two
  modules of the repository that declare one path names nothing unless a `replace` picks one; a
  `replace` by a module or a directory outside the repository makes the replaced module an
  outside one; a `vendor/` directory is not followed. A `go.mod` past `lang::MAX_BYTES`
  (read up to one byte past it, as every pack config is, so its last line may be cut) declares no
  module for the imports either (`modfile::Modules::new`).
- `lang/rust.rs` (types): `written` and `parameter` read a type and climb to the items that
  declare a type parameter within `PATH_DEPTH` (256) steps (references, `A + B + 'c` bounds and
  ancestors); past it a type names nothing and a parameter is taken as declared with no bound,
  so no repository type is guessed. The walk in `parse` carries each node's depth and the items
  around it, so `parameter` asks for no parent (each lookup walks down from the root of the
  tree).
- `lang/rust.rs` (`inner_call`): a method called on another call (`Router::new().route(..)`)
  names it (`Call::on`) through `.await`, parentheses and one `?`, so its receiver is the inner
  target's written result, or, through `?`, the first type argument of a `Result` or `Option`
  (`Def::unwrapped`). A plain `fn` that returns a future (`impl Future<Output = T>`,
  `Pin<Box<dyn Future<Output = T>>>`) keeps `T` as `Def::output`, which only `.await` reads: a
  call on the future itself keeps the written future as its receiver. The `Result` or `Option`
  inside a future's output is not kept, so `.await?` after it is a name match. Two `?` in a row
  name no type, nor does a `?` before an `.await` (`f()?.await`), and a `?` whose unwrapped type is outside the repository, a type parameter or not written
  leaves the call a name match. A `Result` or `Option` is told by its last path segment, so a
  user type of that name with another first argument is read the same way.
- `lang/ts.rs`: tsconfig `references` are not followed, and `extends` only for `paths` and
  `baseUrl` (`Aliases::of_chain`; `package::read_tsconfig`'s `outDir` and `rootDir` still ignore it).
  An `extends` that is a relative path inside the repository (a package such as `@tsconfig/node18`,
  an absolute path, one that leaves the repository, and an array are not read, and the chain stops
  there) is followed to a config named `tsconfig.json` or `jsconfig.json`, since only those are
  read (`tsconfig.base.json` has no bytes here: a follow-up). Each option is the nearest
  declaration's, as a whole (a nested `paths` replaces the base's), `paths` relative to the config
  that declares them. `EXTENDS_DEPTH` (8, `ponytail:`): a chain with more than that many configs
  after the nearest, and a cycle, read as the nearest config alone, so the walk, the budget of each
  config, costs at most ten lookups. Every `tsconfig.json` and
  `jsconfig.json` but those under `node_modules` is read; an importer uses the nearest one in its
  directory or above (`tsconfig.json` over `jsconfig.json`), `paths` and `baseUrl` relative to it.
  Each is read as `package::read_tsconfig` reads one
  (`package::tsconfig_root`, `MAX_BYTES`), so one past the size limit, not UTF-8, not JSON or
  not an object declares no `paths` or `baseUrl` and the refresh carries on, with no status
  line. `require` and
  `module.exports` are not imports (no `ponytail:` marker). `new X()` is a call fact (callee `X`)
  and a `Value::New(callee, args)`. `DEFINITION_DEPTH` (128): a function or class nested deeper
  is not a definition (its body belongs to the one around it, and its name is a local that hides outer
  ones), since qualified names and `Resolver::lexical` cost time cubic in the nesting; a function
  written in place there is `Value::Unknown`. `VALUE_DEPTH` (64): a value nested deeper in
  arguments, objects, arrays and the receivers of method chains is `Unknown` from there.
  `FOLD_DEPTH` (256): a `+` chain or a chain of string method calls folds only that deep (the
  rest is a hole), and a member chain in a callee is cut to the node kind. The parent of every node is read once (`Parents`, which
  `lang/ts/uses.rs` and `lang/ts/types.rs` climb too), as `Node::parent` is linear in the
  tree's depth.
- `resolve.rs` (`Code::origins` with `Follow::Constants`): only `const` bindings with a plain name are followed; a client
  held by `let`, a class field, `this.x`, a destructuring or a parameter, or chained without a
  binding (`new Kafka().producer().send`), is not recognized (no marker).
- `channel/ts.rs`: a kafkajs handler passed through a wrapper's parameter is not followed
  (the function that subscribes handles); only the first `run` on a consumer binding counts. An
  amqplib `bindQueue` whose queue, exchange or pattern is not a literal is dropped.
  A BullMQ `Queue` is named by its own `new Queue(name)` binding: `new Queue(name).add(...)` with
  no binding, a `Queue` that is a class field or a parameter, and `addBulk` are not recognized. A
  node-redis client made as `await createClient().connect()` is not traced (the `connect` chain
  has no binding); a BullMQ processor given as a file path is not followed (the function that
  makes the `Worker` handles).
- `channel/python.rs`: a client is traced to its constructor by `Code::origins` (a name its
  scope binds once, an attribute assigned a call once, a parameter every exact caller passes, a
  value a function returns, or a call written in place: `boto3.client("sqs").send_message(...)`,
  `pika.BlockingConnection(p).channel().basic_publish(...)`), with one level of `channel()` or
  `client()` above it (a pika channel of a connection, a boto3 client of a session); deeper
  factories, a client held in a container and a module imported under another path
  (`kafka.producer.KafkaProducer`) are not recognized. A boto3 client counts for SQS when its
  service is the literal `"sqs"` (a name or constant is not followed). A topic that starts with
  `^` is a regular expression, so it is an unresolved name. A pika `queue_bind` whose queue, exchange or routing key is left out
  or not a literal is dropped. A callback is a handler when it is a definition of the repository
  (`on_message_callback=handler`, `cb=self.on_job`); a lambda or anything else leaves the
  function that subscribes. The streams of an `xreadgroup` are read from a dict literal (or a
  constant folded to one); a dict built otherwise is an unresolved stream.
- `channel/rust.rs`: a client is recognized when its receiver traces back to the library's
  constructor (`Code::origins` with `Follow::Values`, then `Code::outside`, one constructor per `LIBRARIES`
  entry); a client held in a struct field, passed as `&mut con` to a parameter, or rebound by
  assignment is not followed. `lapin` `set_delegate` closures are not followed (the function
  that holds `basic_consume` handles). A Kafka topic starting with `^` is unresolved; an
  `queue_bind` that cannot be fully evaluated is dropped; an SQS `queue_url` on a builder held
  in a `let` is not recognized. `Code::mutable` takes the value of a `let mut` bound once and
  never assigned (`Facts::mutable`, keyed by qualified name); a closure parameter, a `let` in a
  closure given to a call, a nested function's parameter or an assignment in such a closure hides it.
  `CHAIN` (64): a builder chain (`FutureRecord::to(t).payload(p)`, an SQS `send_message()...`,
  redis `StreamReadOptions`) is read to that many calls below the form (`Code::chain`, itself
  bounded by `FLUENT_CHAIN`); a longer one is not a producer or a consumer.
- `resolve.rs` (`Resolver::is_self_or_ancestor`, #438): a Rust glob reads the private names of
  the importing file or of an ancestor file on its `super` chain; an ancestor written as an
  in-file `mod` (`use crate::a::outer::*` from `a::outer::inner`) is read as a foreign module, so
  only its public names come through the glob.
- `resolve.rs` (`Resolver::owned_outside`): a Rust method called on a call chain
  (`client.get(u).send()`, `Call::root` = `client`) resolves to nothing when `client.send` would
  be external, and so does one on a chain that starts at a call (`rocket::build().mount(..)`,
  `Client::new().get(u)`, no `Call::root`) when that first call, the innermost one starting
  where the chain does, is external (`Resolver::rooted_outside`, Rust only); a root that is a
  repository type, a parameter, or a local with no written class (a client a repository
  function returns) is name-matched as before. `self` and `Self` in an `impl` of a type from
  outside the repository (`impl ApiExt for reqwest::Client`) are opaque too
  (`Resolver::outside`): `self.clone()` there resolves to nothing.
- `channel.rs`: broker clusters are not modelled (one name of one broker kind is one channel
  across the workspace), and an exchange's type is not read: every binding pattern follows
  topic-exchange rules.
- `package.rs`: a manifest never fails indexing, so its reader has fixed limits. `MAX_NAME`
  (214 bytes; a longer name is no package) and `MAX_BYTES` (under `lang/mod.rs`). A range is
  stored as written, with no length limit beyond `MAX_BYTES`; npm's `flag` (`package/npm.rs`) checks only one of at most
  `MAX_RANGE` (256 bytes) and calls a longer one `range unchecked`. PyPI (`package/pypi.rs`, spec #24): `MAX_NAME` and
  `MAX_BYTES` hold for `pyproject.toml` and `requirements*.txt` too (a file past it, not UTF-8
  or not TOML reads as empty; the toml crate's own depth limit stops nested input); a
  requirements file is read line by line, its option lines (`-r`, `-e`, `-c`, `--index-url`),
  URLs and paths skipped (a Poetry `git` or `url` source too), and what `-r` names is not
  followed; environment markers and extras
  are dropped and a direct reference (`name @ url`) is no entry; Poetry groups and
  `[project.optional-dependencies]` are not read; a range is never evaluated (no marker). A
  Python import is looked up in the distributions every PyPI manifest of a directory holding
  the file depends on (the nearest naming one gives the range), among the top-level modules under each provider's roots (its directory, then the roots its
  `pyproject.toml` declares, or `src`); a namespace package (a directory without
  `__init__.py`) exposes nothing, and an import several distributions expose is `ambiguous` for
  each, its reason naming none of the others (no marker). Cargo (`package/cargo.rs`, spec #30):
  `MAX_NAME` and `MAX_BYTES` hold for `Cargo.toml` too (a file past it, not UTF-8 or not TOML
  reads as empty); a name outside Cargo's alphabet makes no provider and no entry; an entry that
  is neither a version nor a table, or whose `path`, `git` or `package` is empty or not a string,
  or that gives both `path` and `git`, or whose `path` is absolute, is dropped, never read as a
  version dependency; of two entries of one table and key (a plain one and a `[target.*]` one) the
  first stays (a set keyed by table and key, no quadratic scan). A range is never evaluated,
  `registry`, `optional` and features are not read, and neither `Cargo.lock` nor a registry is
  opened (no marker). A `path` is folded lexically from the manifest's directory: one that
  leaves its repository (`../`) links only to the workspace repository whose root, joined with
  the manifest of a provider, folds to the same location, and one that lands in no workspace
  repository adds nothing; nothing outside a repository is read and no symlink is followed, so
  a symlinked checkout is matched by the paths as they are written (no marker). `[lib] name` is
  followed (a `use` writes it, unless the key renames the crate); `version.workspace = true`
  reads the nearest `[workspace.package] version`, and no other inherited field is read. A `use`
  follows `pub mod` and `pub use` rows alone, so an item behind a private module is not
  reached (no marker). Import resolution: `exported` (`package/npm.rs`) follows at most
  `MAX_CONDITIONS` (32) nested conditions and `lookup` (`package.rs`) at most `MAX_REEXPORTS` (64) `export *`
  levels; deeper is no target. An array of fallbacks in `exports` and a key with two `*` are not
  read (one `*` in a target keeps every expansion the size of the subpath). `read_tsconfig` reads
  like `read`: past `MAX_BYTES` or `serde_json`'s depth limit, not UTF-8 or not an object it is
  invalid (`None`, no config; an option of another type is absent in a valid one), and `roots` voids the mapping for an absolute `outDir` or `rootDir`, one with an empty or
  `..` segment, or an `outDir` that is the config's own directory (a path read from a config stays
  inside the repository; nothing is opened on disk, the mapped file must be one the walk indexed,
  and the walk follows no symlink). `extends` is not followed (`ponytail:` in `Tsconfig`; the
  trunk follows none for `paths` either): a base config's `outDir` counts as absent, so such an
  entry is `ambiguous`; `declarationDir` is not read, and a package with no `rootDir` maps
  `dist/index.js` to `index.ts` at its own directory. An entry with no output extension never
  maps (`main: "./dist"`, `"dist/index"`, `types: "dist/index.d.mts"` or `.d.cts`): only `.d.ts`,
  `.js`, `.mjs`, `.cjs` and `.jsx` are stripped (no marker). The ambiguous candidates of an
  import have no cap: every file of the package exporting the name is listed, so a name exported
  by many files gives as many hops (the spec lists them all; no marker). Only a call whose callee's head is the import crosses: a `const` alias or a
  client bound from it (`const c = new Client(); c.run()`) does not (no marker).
- `lang/ts/uses.rs`: following call results (`Call::uses`) charges every node it visits and
  every step it copies into a `const`'s uses or past a method call to `USES_BUDGET` (2^20 per
  file), so a long method chain spends it; past it, the
  file's remaining calls have no uses, so their consumers read nothing and give no drift. A
  value is followed only in the function making the call, through the names a `const`
  declaration, a `for...of` variable or an array method's callback parameter binds (patterns
  included), when nothing else in that function binds or assigns the name; a `let`
  declaration, `.then(...)`, a callback given by name, `reduce` and a value passed to a
  function end it (no marker). A destructuring pattern nested deeper than `PATTERN_DEPTH` (32)
  reads nothing below that depth (`ponytail:`; a unit test feeds it a deeper pattern). A wrapper's caller is read only when the
  wrapper returns one thing (the response, the body or one path below it) from the function
  holding its call; whether the body was awaited is not checked (no marker).
- Shape readers (`zod.rs`, `declared.rs`, `literals.rs`): each per-file budget below is one
  `shape::Budget` per extraction (`Http::extract`, `exported_shapes`), shared by every
  reader of that extraction, so a file spends each at most once per extraction.
- `zod.rs`: reading the schemas of a file's handlers charges every schema expression examined
  to `ZOD_BUDGET` (2^14, over all the handlers of the file, and the `z.infer` schemas its
  types name); past it, the remaining handlers
  give no shape from a schema. Nesting, constants followed and methods chained count together
  against `ZOD_DEPTH` (32): a part deeper is open. A schema is read only when every 2xx body a
  handler sends is typed with one; otherwise the literals are read (no marker). A name imported
  from a package is a schema the query fills (issue #175) where a schema stands (the receiver
  of `parse`, a field, an element, a member, a `z.infer`), or under the methods `KEEPS` and
  `SCHEMA_METHODS` list; `.pick`, `.omit`, `.partial` and `.required` on it open the part,
  since its fields are not known here (no marker). `.passthrough`, `.catchall`, `z.record`, `z.any`, `z.unknown`, `.transform`, `z.lazy`
  and any call Meada does not read open the part; so does a `pick` / `omit` with a computed
  key and a `z.union([])`. A schema or a parse result held in a constant of the handler (a
  function-local `const out = Schema.parse(..)`) is followed, unlike in `literals.rs`, and
  nothing records a later write to it (`Object.assign(out, { extra: 1 })`), so it is still
  read as the schema's shape (no marker).
- `lang/mod.rs`: `MAX_BYTES` (1 MiB, beside the packs that read their configs by it) bounds every
  manifest and pack config the package readers and the packs take; a larger file reads as empty,
  and the index reads it up to one byte past the limit (`read_up_to`).
- Declared types (spec #23, `declared.rs`, `lang/ts/types.rs`): a type is read to at most
  `TYPE_DEPTH` (32) levels, both when it is written (`lang/mod.rs`, the nesting of an object or
  array type) and when a consumer's cast, type argument or annotation is followed through
  `extends`, aliases and intersections; a level deeper adds no fields. Reading the types of one
  file (its consumers' and its handlers') charges every type examined to `TYPES_BUDGET` (2^14); past it, the type being
  read adds no fields on a consumer, and opens the value on a provider. A declaration that leads
  back to itself (`interface A extends B`, `interface B extends A`) adds what it did the first
  time. A type parameter hides a declared type of its name only when it sits within
  `HIDING_CLIMB` (64) levels of the use; one declared farther up leaves the name unknown, so
  it adds no fields. Only interfaces (with `extends`), aliases to object types, intersections,
  arrays and `z.infer<typeof S>` give fields; a union, a mapped or conditional type
  add none (no marker); a type from a package is filled at query time (issue #175). A generic of the repository
  (`Paged<Row>`) adds its own fields, its type arguments not substituted (its `T` adds none).
  An optional field (`?`) adds no drift, nor does what lies below it; the fields of a
  `z.infer` schema that `.optional()`, `.nullish()` or `.partial()` make may be absent are
  optional too (`.required()` undoes it), and a schema union adds no fields. A type argument
  given to a wrapper types the body only when the wrapper's return type is its own type
  parameter (`Promise<T>`) or it casts the body to it (`as T`, `json<T>()`); `json<T>()` and
  the like type their result when they have exactly one type argument. A cast or an annotation is read at the top of a value (not
  inside an object literal), and a value that goes through a function, a `let` or a rebinding
  is not followed (no marker). On a provider, an index signature in the type of a returned
  constant, a cast body or the handler's return annotation opens that part of the shape.
- `literals.rs`: reading a handler's returned literals charges every call examined and every
  value read or constant followed, over all the handlers of a file, to `LITERALS_BUDGET` (2^16);
  past it, the file's remaining handlers give their routes no shape. Nested literals and
  constant hops count together against `LITERALS_DEPTH` (32): a part deeper is present and open,
  and a constant chain longer is unknown. Only a body Meada reads (an object or array literal,
  a constant naming one) gives fields; a call, a `map` over rows, a parameter or a spread of
  anything but a known object is open (no marker). The handler is the function `res.json` and
  its kin sit in or in a callback inside it. A response with a literal status outside 2xx is
  left out whatever its form (`res.status(404).json(..)`, `res.status(404).end()`,
  `res.sendStatus(404)`, `new Response(null, { status: 404 })`), and so is Express's error path,
  `next(err)` (`next` called with an argument other than `'route'` or `'router'`). A response
  sent another way opens the root of the shape (no marker): in Next.js, a call whose result the
  handler itself returns and that is no `NextResponse.json` / `Response.json` (`return
  ok(user)`, `new Response(..)` with no such status, `redirect`); in Express, `res` passed to
  another call, a `res` method other than `json`, `send` and the ones that only set a header or
  the status (`end`, `render`, `redirect`, `sendFile`, `sendStatus`, ...) with no such status,
  or `next()`, `next('route')` and `next('router')`. A `res` stored in another name or a status set in a separate
  statement (`res.status(404);` then `res.json(...)`) is not followed, so such a send counts as
  2xx (no marker). Only a module-level constant is followed: nothing records a later write to
  a function-local one (`body.next = cursor`, `Object.assign(body, x)`), so it is unknown and
  open; a module-level object mutated after its declaration is still read as written. A constant
  held by another repository (a package import the repository's `Resolver` does not map to one
  of its files) is filled at query time (issue #175), and so are the index signatures of a
  package type a returned value, a cast or the return annotation names.
- Shapes through package imports (issue #175, `shape.rs`, `package.rs`): `shape::fill` follows
  an import into the shape the package keeps, and the imports of that shape in turn, at most
  `PACKAGE_HOPS` (8) packages deep, and charges every import looked up and every path grafted
  to `GRAFT_BUDGET` (2^16) per side; past either, the import is unresolved: open on a provider,
  nothing on a consumer. So is an import that resolves nowhere, `ambiguous`, to several names,
  or in two providers to different shapes, and a name no reader keeps a shape for (a call, a
  union type). Only a module-level constant or type a file of a package's public surface
  (`surface`) exports itself keeps a shape (a re-export reaches it through the
  `exports` rows): the files the entries of each named manifest (the bare name and each
  `exports` subpath without a `*`, as an import resolves them) lead to through the `exports`
  rows. Those are re-read, their files parsed again, on every refresh that extracts, so the
  cost is bounded by the packages' surface, not the repository. A subpath pattern (`./x/*`) and
  a deep import into a package without `exports` (`pkg/src/x`) reach files off the surface,
  which keep no shape, so such an import is unresolved (no marker). An edited `package.json` or
  `tsconfig.json` extracts again. A type through a namespace import (`C.Receipt`) is not read
  (no marker); a schema through one (`C.Quote`) is. Since constants and types are `exports`
  targets, two `export *` that give a name differently (a function and a constant) export
  neither, as in ES modules.
- `lang/ts.rs` (`Facts::locals`): a local binding hides the name in its whole enclosing
  definition, whatever block holds it, the definition's own definitions, constants and
  parameters of that name included, and a function that is no definition (a method or arrow of an
  object literal that no top-level `const` binds, an IIFE) lends its
  parameters and locals to the definition around it; at the top level that is the whole file.
  A call to the hidden name elsewhere in that scope loses its exact link or its value, never
  gains a wrong one (no marker).
- `index.rs` (`refresh_stamped`): a `package.json` or document that cannot be statted or read
  (it vanished after the walk, or is unreadable) has no row instead of failing the refresh; a
  `[serve] documents` entry in that state is then reported as missing.
- `document/`: a document never fails indexing, so its readers have fixed limits; deeper or
  larger parts are unreadable or open. `mod.rs`: `DOCUMENT_BYTES_MAX` (16 MiB, above the largest public
  single-file descriptions, GitHub's REST API and Stripe's SDK spec at 10 to 15 MB): a larger
  document is unreadable at its first line. `yaml.rs`: `MAX_NESTING` (128 nested collections, as
  `serde_json` has for JSON) and the alias budget (`EXPANSION` times the source plus `SLACK`, no
  marker). `schema.rs`: `MAX_DEPTH` (128 levels per shape), `WORK` (64 MiB per document) and
  `BUDGET` (10,000 schemas per shape, no marker). `openapi.rs`: a path item reference behind
  another one that is external is dropped without a status line (no marker).
- `drift_view.rs`: `context`, `search` and `status` find each shown link's consumer call by
  scanning the calls of every repository, once per link (no marker); a query has read only those
  calls (`drift_view::sites`), `status` every call.
- `lang/python.rs`: `VALUE_DEPTH` (64): a list, tuple, dict literal, parenthesized expression or
  `await` nested deeper in a value is `Value::Unknown`, so a hostile file cannot overflow the
  stack. `FOLD_DEPTH` (256): a `+` or `or` chain, a chain of string methods or `getenv`
  defaults and nested f-strings fold only that deep (the rest is a hole), and an attribute
  chain in a callee is cut to the node kind; each call's arguments are folded again, so a file
  of nested calls costs up to `FOLD_DEPTH` steps per call. `DEFINITION_DEPTH` (128): a
  function or class nested deeper is not a definition (its body belongs to the one around it,
  its name and parameters are locals that hide outer ones there, and its `return`s are no
  one's), since qualified names and `Resolver::lexical` cost time cubic in the nesting.
  `TARGET_DEPTH` (64 nodes): a name deeper in an unpacking target or a `case` pattern is not
  bound, so it hides nothing. An annotation reads `Optional[...]` and `... | None` to
  `TYPE_DEPTH` levels; deeper is no class. For imports inside the repository every `pyproject.toml` is read
  (`lang/python/pyproject.rs`, which the PyPI reader shares), the setuptools and Poetry root
  declarations only; an importer tries the roots of the nearest one in its directory or above, then
  of each further ancestor, nearest first, and a sibling project's never; each is read as a PyPI manifest is (`package::read_manifest`, `MAX_BYTES`), so one past the size limit, not UTF-8 or not TOML declares no roots and the refresh carries on, with no status line. `src` is a source root when nothing is declared, whether or not it holds
  packages (a pack sees no file list). An import binds its names for the whole file, wherever
  it sits. A comprehension's targets are bound in the enclosing scope, so they hide a name
  there for the whole scope (the safe side: a name match instead of `exact`); at the top level
  they, and a lambda's parameters, hide nothing, so `[h() for h in hs]` beside `def h()` stays
  `exact` to the definition (hiding `h` there alone needs a scope of the comprehension's own). A write to an
  attribute from outside the class (`obj.x = ...` in another function) is not seen, so a
  class-body `x = C()` still types `self.x`. `__all__` is read only as one literal list or
  tuple, grown by literal names in a statement of the module itself (`+=`, `.extend`,
  `.append`); any other change to it, one under `if`, `try` or a loop, in a lambda or a
  function, and an item set or deleted (`__all__[:] = ...`, `del __all__[0]`) make it computed. A name `del` unbinds (at the top level,
  unless bound again), and a definition another binding of its scope follows (at the top level
  or in a function, where a nested function's `nonlocal` binding counts as following), and one
  under `if`, `try`, a loop, `with` or `match` after another binding of its name, which it may
  leave in place, are hidden in their whole scope, wherever the call sits (the safe side: a name match instead of
  `exact`). In a class body a binding after a method hides nothing: `self.m()` reads the method
  through its class, right for `m = staticmethod(m)` and as `@property` is read for `m =
  property(m)`, but a class-body `m = None` leaves `self.m()` linked to `def m`. A `**k` splat in a call is not read; a `*a` splat is read as one positional argument.
  Console scripts (`Pyproject::scripts`) are read as `"module:function"` strings only: the Poetry
  table form (`{ callable = "...", extras = [...] }`) is left out, and so are
  `[project.gui-scripts]` and `entry-points`. The block of an `if __name__ == "__main__":` is the
  scope `__main__`, so a name it assigns is a local of that symbol and no longer rebinds the
  module's global of that name; only the first `__main__` of a file (a guard or a
  `def __main__()`) counts.
- `lang/rust/cfg.rs` (spec #30): `PREDICATE_DEPTH` (64): a `cfg` predicate nesting `all`,
  `any` and `not` deeper is false; `FEATURES` (4096): the default features of a crate followed
  through features that enable others (a cycle ends sooner, by the visited set), the rest is
  not followed.
- `lang/rust/macros.rs` (#398): `ITEM_MACRO_DEPTH` (8): item macros nested deeper in one
  another's bodies are skipped, as any other macro; each level parses its body again;
  `ATTRIBUTE_BYTES` (1024): an arm whose attributes take more, as tokens, is no item macro, since
  each file that invokes it stores them in its setting. An item
  macro is found by name alone (no `#[macro_use]`, `#[macro_export]` or textual scope is
  followed), across every Rust file of the repository whatever its `cfg`, so a definition of the
  name anywhere that disagrees leaves every invocation unread (no marker); only `m!` or
  `crate::m!` invoked with braces among the items of a module, an `impl` or a trait qualifies
  (`other::m! {}`, `self::m! {}`, `m!(..)` and invocations in a function body are skipped, no
  marker).
- `lang/rust.rs` (spec #30): `DEFINITION_DEPTH` (128) as in the other packs; `BINDING_DEPTH`
  (64): a pattern or a `use` tree deeper binds nothing; `PATH_DEPTH` (256): a longer path or
  member chain in a callee is its node kind, and an `impl` type behind more references names
  no type; `USE_BINDINGS` (4096): the names the `use` declarations of a file bind, the rest
  bind nothing; `VALUE_DEPTH` (64): a value nested deeper in wrappers (`(x)`, `&x`, `x?`) and
  arrays is `Unknown` from there. A call inside a value is a stub (its callee and span), so a
  builder chain costs one value per call. A closure written as an argument of a call is a
  `Kind::Inline` definition; any other closure is not, and neither is a `let mut`, a name bound
  twice in its scope or assigned again, or a destructuring a constant. A string literal with an escape sequence is
  `Unknown`. A `Cargo.toml` past `lang::MAX_BYTES` (read up to one byte past it, as every
  pack config is), not UTF-8 or not TOML declares no crate, with no status line. Every `Cargo.toml` with
  a `[package]`, at any depth, is a crate (a workspace's `members` and `exclude` add or remove
  none, and a vendored or fixture crate the walk finds is indexed too); `tests/`, `examples/`,
  `benches/` and `build.rs` are no crate roots (no marker). The default features a file is
  parsed with (`settings`) are those of the nearest `Cargo.toml` with a `[package]` above it,
  read before any module tree exists, so a file another package's `#[path]` reaches is parsed
  with its nearest package's features, not those of the crate that builds it (no marker). A file two crates reach (`mod
  util;` in both `lib.rs` and `main.rs`) belongs to the first by root order (the library
  first), so its `crate::` names that root (no marker). `#[path]` on a module written in place
  is not read, and `super::` out of a file declared inside a module written in place of its
  parent reaches nothing (no marker). The `mod` imports and the synthetic ones (`crate`,
  `super`, `mod`) bind for the whole file, so they are seen inside every inline module too;
  a `use` binds in the scope that holds it, and the `mod` import of a declared module is not
  seen inside a module written in place. A `use` path whose first segment nothing in the
  file binds is a crate outside it (`Facts::externs`, no marker, never name-matched), also when
  the file holds a glob that brings no such name. A glob of a module written in place from
  outside it, and of an enum or a type, bring nothing (the visibility of the items of a module
  written in place is not kept; no marker), and a glob takes `pub` items alone, so a
  `pub(crate)` item reached through one falls to a name match. `pub use` names are `pub`;
  a crate in the workspace is reached only from a crate whose `Cargo.toml` lists it in
  `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]` (by key, or by `package =`;
  `workspace = true` through `[workspace.dependencies]`), a binary naming the library of its own
  package; the same tables under `[target.*]` count, and `optional` and features are not read
  (a listed dependency counts as present; no marker). Any other crate name is external. A module declared inside a module
  written in place is not followed for exports (no marker). A chain of `use` aliases, globs and
  `pub use` longer than `LINK_DEPTH` (see `resolve.rs` above) is unknown, and no other glob
  binds the name in its place.
  `self::` and `super::` read inside a module that sits in a function are
  `(scoped_identifier)`, unresolved, and a `use` there binds nothing. A file's exports table
  rows are its `pub` items, `pub use` names, `pub` modules and, inside `pub` modules written
  in place, the dotted names of their `pub` items (`entries.add`), and a `pub use` glob of a
  file gives that file's public names; whether the file itself is reachable from the crate's
  root is for the reader to follow through the `pub mod` rows. Calls inside macro invocations
  are not indexed.
- `lang/rust.rs` (spec #30, actix-web and Rocket): `ROUTES_PER_FUNCTION` (16): a function with
  more route attributes (`#[get("/x")]`, `#[route(..)]`) than that has the last ones (nearest the
  function) as calls that decorate it (`Call::decorates`), the rest nothing. `ROUTE_ITEMS` (1024, `lang/rust/values.rs`):
  a `routes![..]` listing more handlers is `Unknown`; `ROUTE_METHODS` (16): the `method = ".."`
  of a `#[route]` past that are `Unknown`, so the attribute gives no provider. `MENTIONS`
  (16384, `lang/rust/mentions.rs`): a file referring to more distinct names has no
  `Facts::mentions`, so none of the handlers of its crate is taken as unmentioned.
  `Facts::mentions` is not kept in the index (see `Call::args`); the providers read it from the
  files parsed again, all of the pack's when a handler is registered by no call they read. A
  mention is a name in a reference position (an expression, a receiver, a path, a `use`, a
  macro's tokens); a pattern, a parameter or a field declaring it is none, and a local hides a
  reference from the end of its `let` to the end of the block, a parameter in its function's
  or closure's body, a `match`, `if let`, `while let` or `for` binding in its arm or block (an
  `else` sees none). An item declared in a block (`const`, `static`, `fn` inside a function)
  hides nothing, and a pattern deeper than `BINDING_DEPTH` binds nothing, so the reference
  still counts (no marker). The walk over mentions carries each node's parent, the token
  before it and where an `if let` binds, and `built` reads the attributes before a function
  with the one read of its parent's children, so neither asks for a parent or sibling.
- `http/rust.rs` (spec #30): `ITEM_WORK` (16384): the values the expansion of constants and
  returned lists adds, over the whole repository, to the handlers a call names; a list past it
  is read no further, and a handler it left out is taken as mentioned. `macro_use`: the crate
  of a bare route attribute is read from the `#[macro_use] extern crate x;` of its crate's root
  (the `Tree`'s `crate`, wherever `[lib] path` puts it), and it needs exactly one such crate
  (actix-web or Rocket). A handler is served at its own path only when no file of its crate
  mentions its name (`mentions.rs` above), or of any crate when its file is one another crate
  may import (`Code::published`, a library's `pub mod` files); any other mention Meada does not
  read as a registration (`services![..]`, `vec![..]`, `concat()`, `cfg_if!`, an alias, a use
  before a `let` of the name) means it may be registered where Meada cannot tell, and then it is
  served nowhere. A file two crates reach belongs to the first (see above), so a mention in the
  second, of a handler not published, is not counted (no marker). A service configuration is followed into the functions of the
  repository it is passed to (`f(cfg)`, answered exactly), not through methods or closures.
- `http/rust/axum.rs`: `METHOD_CHAIN` (64): an axum method router chain (`get(a).post(b)`) is read
  to that many calls, a longer one gives its route no provider. `FLUENT_CHAIN` (1024,
  `resolve.rs`): a builder chain (`Router::new().route(..)`, an actix-web `ServiceConfig`'s
  `cfg.service(a).route(..)`) is read to that many calls down to what starts it (`Code::chain`),
  and a call past it has no origin, so its route is no provider (the walk is memoized per call,
  linear in the chain). `PREFIX_DEPTH` (64) and `PREFIX_WORK` (4096):
  `prefixes`, for every kind that mounts (Express, FastAPI, Flask, axum, actix-web, Rocket, Ktor,
  ASP.NET Core minimal APIs), reads a router
  nested deeper than the first, or through more mounts than the second, as served nowhere
  rather than under part of its prefixes. A name bound again in its scope (`let mut`,
  shadowing) or assigned again holds no known value, and the routers its `let` holds
  (`Facts::rebound`) are served nowhere; `on(MethodFilter, h)` and a method router held in a
  variable are not followed. A router nothing mounts is served at its root. A router
  given to a call Meada does not read (`route_service`, `fallback_service`, an outside
  function wrapping it, a method only a name match found) is mounted nowhere, so it is not
  served at the root; so is one whose `nest` prefix cannot be read, or that is mounted on a
  router Meada cannot read. A router handed to a call that is neither a mount nor a wrapper
  of one in its arguments (a router put in a struct, a closure capture) is not told from one
  nothing mounts. The axum reading of those unread values (`axum_unread`) walks each value
  once per file (its `Unread` state keeps the values walked across the file's starts), and asks
  `Code::origins` only of a call whose builder chain (`Code::chain`) goes through methods a
  router hands back, and of a name not written (`Code::written`) as text, a path, a number, a
  boolean or a character (`is_plain`, by the last path segment) (#431).
- `http/kotlin.rs` (spec #27, #98): a scope holding a `route` call whose lambda is not read (one
  nested past the Kotlin pack's `DEFINITION_DEPTH`, or a block passed as a function reference,
  `route(path, Route::orders)`) registers nothing: its routes and routing extension calls, the
  block's siblings included, are served nowhere, never under part of a prefix. The implicit
  receiver of a routing block is taken as Ktor's `Route` (`Routing` in `routing { }`), kept
  through any lambda argument inside the block (`authenticate { }`) but a handler's or a local
  function's: a member of `Route` named like an extension of the repository, and an extension of
  a supertype (`Any`, a type parameter) nearer in scope, are not weighed, and an extension on
  `Routing` called in a `route` block (which Kotlin runs on the `Routing` around it) mounts
  nothing. A local or a parameter of the name hides an extension (`Code::extensions`), and a
  local function of the name mounts nothing. Routes nested in `route(path, HttpMethod.X) { }`
  (its lambda is the handler), `method(HttpMethod.X) { }`, `install(Routing)`, a lambda passed
  by name (`body = { }`), any named argument other than `path` and `method`, and a path with a
  string template that does not fold are not read. A Ktor verb inside a lambda (`get`, `put`) is a call the
  resolver reads as a name match, as any call there is, so it may match a repository method of
  the same name: that method then has a caller and loses its export door.
- `http/kotlin/client.rs` (spec #27, #99): `url { path(..) }`, `url(..)` and `takeFrom` inside a
  request block, `HttpRequestBuilder` values, `submitForm`, the `prepare*` forms, an aliased
  import (`import .. as k`) and a client the call reaches through `with(client) { get(..) }`
  are not read. A `defaultRequest` base given by a parameter of a client factory is a hole. A
  local client is followed only when its `val` is written with the constructor call (or a
  `let`, `also` or `use` receiver): `val b = a` and a `var` are not, and a local named like a
  property of its class, of an enclosing class or of its file is typed unknown by the pack, so it is not either. A nested lambda's
  `method = X` makes the outer request's method unknown even when the inner lambda always runs
  (`headers { }`).
- Kotlin base URLs (spec #27, #100): `STRING_METHODS` also lets `replace`, `lowercase` and the
  others of the trunk packs' lists pass an environment read through, beyond `trimEnd` and
  `removeSuffix`. A fallback that reads the environment through a name of another file, a
  function or a local is not seen (`ENV_NAMES` is the file's properties). An environment read
  kept in a local `val` (`val raw = System.getenv(..) ?: ..; raw.trimEnd('/')`) or in a
  function stays open: the consumer is open, never a wrong link. Parity with Java, not
  regressions: a constructor parameter without `val` used in an initializer stays open; a
  call that constructs the class in the source (`ExplicitClient("http://localhost:1")`) does not
  stop injection into the constructor's property; `@Value` on a plain function's parameter is
  injected as if the function were a bean's. A secondary constructor's `@Value` parameters are
  not read, and a `@set:Value` property is taken as injected into its field.
- Kotlin HTTP consumers (spec #27, #101): a named argument on a nested builder step
  (`.url(url = ..)`) is dropped by `Invocation`, so the consumer is open, never a wrong link; an
  OkHttp `Request.Builder` held by a local `val` is not followed (the chain starts at the local's
  name, as in Java); a local that a wrapper's callers fold to several bases has an unknown one;
  Java Retrofit bases from `@Value` stay unread; Feign remains Java-only.
- Rust values (`lang/rust/values.rs`, `lang/rust/builders.rs`, spec #30 #131): `BUILDER_BUDGET`
  (2^20 syntax nodes per file, over every `let mut`, block by block in the order the blocks
  open), `BUILDER_DEPTH` (64 builders read within
  one another), `BUILDER_SIZE` (4096, bytes of text plus parts) and `BUILDER_WORK` (2^22 size
  units per file, over every prefix folded and every read): a `let mut` String past the budget
  is unknown, and one read past the depth, the size or the work is a hole (`push_str(&s)` of
  itself doubles). `FORMAT_ARGS` (64), `FORMAT_PLACEHOLDERS` (256) and the size above bound
  one `format!` or `concat!`; past any the value is unknown. A `let mut` String is a builder
  only when its block reads it, `push_str`s, `push`es and `+=`s it in statements of that block
  (a statement `#[cfg]` leaves out, decided where the block is read, is not there); a push in
  a branch or a loop, `&mut s`, `write!(s, ..)`, an index or deref use (`s[..]`, `*s`), another
  binding of the name, a method that changes it (`insert`, `clear`) or a second builder of
  that name in the scope leave it unknown. A builder takes the statements after its `let` from
  the one read of the block in `parse`, and its walk asks for no parent or sibling (each lookup
  walks down from the root of the tree), so every node the budget counts costs the same at any
  depth. `format!` and `concat!` read
  a literal format string and arguments that are a literal or a plain name; a spec such as
  `{:.*}` or `{:1$}`, a count that does not match or a `{`/`}` left alone are unknown; a macro
  of another name or one the file defines (`macro_rules!`, in this file only) is not read.
  `Method::GET` is text only through a `use` of one name from `reqwest`, `http`, `hyper`,
  `axum` or `actix_web`, or written in full from one of them; a glob import or an alias is not
  followed. `a + b` is a string only when one side is a string literal or a conversion: two
  names may add numbers. Rust clients: a client held in a field of a struct (`self.client`), in
  a `static` or a `OnceLock`, and a client from a crate other than `reqwest` and `ureq`
  (`hyper`, `surf`, `isahc`, `attohttpc`) are not read; a client parameter is followed through
  the callers that pass a constructor, as in Python; `reqwest::Request::new` and
  `RequestBuilder::build` are not followed; a `Url::parse(..)?` or `join` URL is unknown.
  A value derived from a client (a response, a header map, a JSON value, the result of an
  extension trait's method) is no client: a method passes the object on only when the
  object's entry lists it (`FRAMEWORKS` in `http/rust/frameworks.rs`, the constructors named
  exactly: what a name of the crate that is no listed constructor makes passes nothing), so a
  setter missing from the list ends the chain and its client (or its router, app, scope,
  resource or Rocket instance) is not read.
- `http/ts.rs`: `router.route(path)` chains and `require` are not followed. A route path or
  prefix that depends on a parameter is not read through callers (it yields no route;
  `http.rs` `path_literal`, for every framework). `http/python.rs`: a FastAPI
  mount whose child cannot be read hides every other app of its framework that nothing else
  mounts. Django: one `include()` whose argument cannot be read leaves no root URLconf; the
  walk reads at most `URLCONF_BUDGET` (2^15) entries over every root and `include()` (a
  module included under several prefixes is read once for each), so the entries past it yield
  no provider, and follows at most `INCLUDE_DEPTH` (64) `include()` calls, each of the next
  module, so the routes of a module included deeper yield none; what
  an extension of `urlpatterns` adds other than a list literal is left out (marked in
  `lang/python.rs`), and a single `urlpatterns = [...] + other` (no list literal as a whole)
  yields no entry; `django.conf.urls.url`, a view wrapped in a call in the URLconf
  (`require_POST(view)`), a handler method a class-based view inherits and `as_view()` on a
  class outside the repository yield nothing; a `re_path()` pattern is read as written, so a
  non-raw string with doubled backslashes yields nothing. Python clients: a receiver that is the
  result of a call (`Session().get(...)`, `make().get(...)`) or an attribute set from anything
  but a call (`self.http = http`, marked in `lang/python.rs`) is not read; a parameter given a
  client takes the base of every caller that passes one, and several different bases make it a
  hole.
- `entry.rs`: an export entry point is a function, or a method of an exported class (a
  constructor excepted), whose exported name the exporting file's `Facts::star` lets through when
  its pack names a public set (Python: no leading `_`, or a literal `__all__`; TypeScript names
  none, so all pass), that the `exports` rows name and no call reaches in its repository,
  an ambiguous candidate counting as a caller and a self-call not; a call through a package
  import of its own repository counts too, but only at query time (`Engine::exports`), so the
  stored rows over-list in a monorepo. A method of a class that is not exported, a
  default-exported anonymous function and an export the resolver cannot place are not found (no
  marker). A route handler Meada could not find (an Express handler passed as a parameter) has
  no entry point.
- `trace.rs` (process walk): an ambiguous call or link is one unexpanded step listing its
  candidates; `inferred` and `decided` links expand. A process is walked down only; `trace`
  up from `process::...` answers with a note. `PROCESS_DEPTH` (256) caps the recursion of one
  branch: the step at the limit is marked `depth-limit` (`(depth limit)` in `trace`) and not
  expanded, so a longer call chain is cut. `Graph::processes_of` (the processes `context` lists)
  walks upstream with no depth limit, so on chains deeper than `PROCESS_DEPTH` membership and
  trees can disagree: a node past depth 256 is no step of the tree, and the limit can hide a
  node shallower than that, through `Seen`. E reaches X at depth 255, so X expands and each of
  its callees, Y among them, is a `depth-limit` step at 256 whose own callee Z is no step; a
  later direct E to X shows X as seen, so Y is not expanded there either, yet `processes_of(Z)`
  lists E.
- `community.rs`: Leiden has no iteration cap (spec #31 asks for none). Each pass that changes
  anything either moves a node to a community with a strictly positive modularity gain, which
  raises the modularity, an integer here (`i128`, no floating point), so no partition repeats,
  or merges subcommunities and shrinks the graph, which cannot happen forever; so the loop ends.
  The work grows with the repository's calls and passes (no marker).
- `engine.rs`: `context` builds `trace::Graph` and calls `Graph::processes_of` for every symbol
  it answers about: a reverse map of every edge of the workspace (every call and link) is built
  per call, with no cache, so the cost follows the workspace's size, not the symbol's (no
  marker).
- `repo.rs`: a path git still quotes (tab, newline or quote in the name) is skipped by
  `uncommitted()`.
- No `busy_timeout`: two concurrent refreshes of one repository may hit `SQLITE_BUSY` (no marker).

- `jvm/spring.rs` (spec #26, #89): `PLACEHOLDER_DEPTH` (32) bounds how many keys deep a
  placeholder is resolved, a cycle included, and `PLACEHOLDER_BUDGET` (64 KiB) what one value
  may cost (each placeholder visited, each byte copied); past either the rest is an unknown
  part. The YAML file goes through the provider documents' reader (`document/yaml.rs`), with its
  budget and nesting limit; a `.properties` file is bounded by `lang::MAX_BYTES` like every pack
  config. A same-package `System` type in another file is not seen by the Java pack's
  `getenv`.
- `xml.rs`, shared by `jvm/maven.rs` (spec #26) and `dotnet/msbuild.rs` (spec #28): `XML_DEPTH` (64 nested elements), `XML_NAMESPACES` (16 `xmlns`
  occurrences in the file) and `XML_ATTRIBUTES` (64 per tag) are checked by one linear pass
  before parsing, since roxmltree copies the enclosing namespaces into each element that
  declares one and checks a tag's attributes pairwise; `XML_NODES` (65,536) bounds the parsed
  tree, in addition to `lang::MAX_BYTES`. `roxmltree` rejects DTDs and never loads external
  entities. An over-limit, unterminated or malformed manifest reads as an empty summary (a
  project file as no project); indexing continues. `jvm/maven.rs`: `PROPERTY_DEPTH` (16) bounds
  how many properties a `${..}` follows, a cycle included; a property longer than a package name
  is not kept; `FILL_BUDGET` (100 000) bounds the properties one pom reads, past which a hole
  stays as written. `jvm/gradle.rs`: `GRADLE_DEPTH` (64) bounds nested blocks; a deeper build
  or settings file reads as an empty summary. The Gradle scanner does not read Groovy slashy
  strings, and reads a triple-quoted string as quoted pairs. It keeps at most `MAX_ROOTS` (64)
  source directories. The Groovy and the Kotlin DSL files (`*.gradle`, `*.gradle.kts`) go
  through this one scanner (spec #27: `tests/it/kotlinpkg.rs`); a `project(":x")` dependency
  declares no coordinate. A block comment nests, as Kotlin's does (Groovy's does not, so a
  nested opener there turns a coordinate into a miss, never a wrong `exact`). `gradle.properties` and a version
  catalog (`*.versions.toml`) are manifests of the Maven ecosystem that bound no package
  (`Maven::owns`), read by `jvm::gradle::properties` and `catalog`: each reads as empty past
  `lang::MAX_BYTES`, keeps at most `MAX_ENTRIES` (8192) properties or `[libraries]` aliases and
  skips a value longer than a package name; a settings file keeps at most `MAX_CATALOGS` (16)
  `from(files(..))` catalogs, each a relative path that stays inside its directory and ends in
  `.versions.toml` to be read as a manifest. `Maven::inherit` fills a build's holes from them
  in one pass per build, sharing `FILL_BUDGET`. Ceilings, all
  misses: `buildSrc`, convention plugins, included builds and plugin-provided BOMs (code or
  build-time resolution, not declarations), a catalog accessor that is not a `[libraries]` alias
  (`libs.bundles.x`, `libs.versions.x`, a call on it such as `.get()`), a catalog whose file name
  does not end in `.versions.toml`, an alias written as a dotted TOML key (it parses as a nested
  table), a `gradle.properties` value with a backslash escape or a
  continued line, a `gradle.properties` of the user's home, and one in a directory between the
  repository root, the settings file and the module (the repository root's is read even when the
  settings file sits deeper), a `dependencies` block nested deeper than one block
  (`subprojects`, `constraints`, `sourceSets { commonMain { dependencies } }`), a configuration
  called as a string (`"implementation"("g:a:1")`), a positional `implementation("g", "a",
  "1")`, a `group` set only in `subprojects`/`allprojects`, and a top-level `val group`. A
  `group` reset in `afterEvaluate` is not seen, and `publishing { artifactId = .. }` is
  ignored, so the announced package may differ from the published one. JVM coordinate names share
  `package::MAX_NAME` (214 bytes). These readers never run Maven, Gradle or a JDK. A file sees
  what the modules of its repository it depends on pass on up to `MODULE_DEPTH` (32) modules
  deep (`package/maven.rs`); a module's parent pom adds nothing to what it passes on, and a
  Gradle dependency passes nothing on.
