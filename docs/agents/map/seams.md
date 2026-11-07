# Seams: ecosystems, shared readers and libraries

Detail behind the [architecture map](../architecture.md), which routes flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

The extension steps of the package ecosystems, the readers the JVM and .NET share, and the
entries per library of the HTTP and channel kinds. The other extension steps are in
[Seams and what extends them](overview/seams-and-what-extends-them.md).

- `dotnet/` holds what the .NET pack and ecosystems share (spec #28), as `jvm/` does for the
  JVM: `dotnet/msbuild.rs` reads an SDK-style project file (`msbuild::read`: whether it is a
  project and, if so, its `Using`s (the SDK's implicit usings among them), its `PackageId`,
  `AssemblyName` and version, and its `PackageReference` and `ProjectReference` items, each
  with whether it is private: a `PrivateAssets` naming `all` or `compile`, or computed, keeps it
  from passing on), which the C# pack's `join` and the NuGet ecosystem (#116) both read; a second
  reader of project files is a finding. `msbuild::read` takes the `Directory.Build.props` files
  MSBuild imports into the project (#381), each read once by `msbuild::props` (a file of more than
  `PROPS_ITEMS` (256) usings and references reads as unreadable, so what it copies into each
  project stays bounded), evaluated before it: `msbuild::chain` finds them
  (the nearest at or above the project's directory, then each it imports with
  `GetPathOfFileAbove('Directory.Build.props', ...)` or a relative path, outer first, up to
  `PROPS_DEPTH` (8) and not one twice), items add up, the project's property wins, a `<Using
  Remove>` drops what the files before it added, and an unreadable file adds nothing but still hides
  those above it. The pack's `join` reads them as configuration files (`**/Directory.Build.props`)
  and the NuGet ecosystem keeps one as a manifest that is no project: `Nuget::inherit` gives its
  `PackageReference` and `ProjectReference` entries (a `ProjectReference` relative to the project,
  as MSBuild evaluates it; one on `$(MSBuildThisFileDirectory)` is computed) to the projects below
  it and empties the file's own, so they declare them and it declares none, and names each
  project with the package properties the chain writes under the project's own
  (`msbuild::package`, from `Summary::properties`): a computed id from either makes no provider.
  `dotnet/references.rs` (#369) walks a repository's
  `ProjectReference`s: `located` turns an `Include` into the project file's path in the
  repository, and `referenced` gives the projects a project references directly or through the
  projects those reference, nearest first (breadth first), each once, following a private
  reference from the start project alone and stopping at a project already found and past
  `REFERENCE_DEPTH` (32), so each project's references are read once. It takes a project's
  references through a function over what its read yields, so NuGet's `entries` (from the
  manifests' entries) and the C# resolver (from `msbuild::read`) walk the same graph. `xml.rs` holds the bounds every XML build file reader shares
  (`within_limits`, `XML_NODES`), owned by neither ecosystem: `jvm/maven.rs` and
  `dotnet/msbuild.rs` both check them before roxmltree parses.
- New **Go router** (spec #25; net/http, gin, echo, chi and gorilla): a row of `ROUTERS` in
  `http/go.rs` (the package, the functions that make a router, the methods that make a group
  (a `Form`: `Group(prefix)`, chi's `Group(fn)` and `Route(prefix, fn)`, gorilla's
  `PathPrefix(prefix).Subrouter()`) or mount a router (chi's `Mount`), the route methods with
  the methods each serves, where the handler sits, which path spellings are parameters);
  `Go::provide` reads the routers through `Code::origins` and `prefixes`, as the Rust kinds do.
  The function a `Route` or `Group` call is given (`Go::callbacks`) binds its first parameter to
  that group, which `Code::origins` does not see; `Go::reach` follows it from there through the
  exact callers that pass the parameter on and the names bound once to it (`Code::param_of`,
  `Code::param_args`, `Code::constant`). The pack gives it `Call::args`,
  `Call::receiver`, the constants a function binds once (`Facts::consts`) and what it returns
  (`Def::return_values`), read off the syntax only (`lang/go/values.rs`).
- New **Go HTTP client** (spec #25, #79): `http/go/clients.rs` reads `net/http` calls (on the
  package, a `Client`, `DefaultClient`), `NewRequest(WithContext)` and resty requests as
  consumers; a call is a function of a library when `Code::outside` says so (`direct`: its head is the
  import itself, the one check the Go routers, clients and channels share), and a receiver is a
  client of a library when `Code::origins` with `Follow::Declarations` answers the form `Type`
  (the type a typed parameter, field or `:=` names, qualified by its import path). The URL is folded in
  `lang/go/values.rs` at parse time (`+`, `fmt.Sprintf` verbs as holes, `net/http` method
  constants, package-level `const` and a `var` nothing assigns, in the package or in another one
  by import), and a resty base is the value of the `SetBaseURL` argument on the chain (or the
  statement) that built the client. A base belongs to one client, not to the call that makes
  clients: a client is the `resty.New()` call with the calls of the repository's functions it
  came through (`Code::origins` with `Follow::Values`, each route's climbed calls), so two clients one factory makes keep their own
  bases, and a `SetBaseURL` inside the factory reaches every client it makes. A client held in a
  struct field (`c.rc`, `Code::field`) holds the clients of the values every composite literal
  of the repository gives that field of that type (`Code::field_values`, read from the marker
  constants `{field}` the Go pack writes for each field a literal sets, `{*}` by position, the
  type `?` when it is not read): a literal by position, of a type not read or not known, or of
  a type that declares no such field, a value that is not traced, a field written through a
  selector anywhere (`Code::field_written`), or one met again while it is followed, makes the
  base unknown, and so does a `SetBaseURL` called through a field. A field read as a request
  (`s.req.Get(..)`, no `R()` before it) holds a request only through a traced one or a value
  written as `resty.Request`: untraced there, or written through a selector, it is no request. Wrapper climbing is
  `Code::expand`, as for every kind. A package `var` another file of the package assigns, or any
  file of the repository writes through a selector (`pkg.X = ..`, `&pkg.X`, `pkg.X++`: any write
  to a field called `X`, whatever it belongs to), is `Unknown` (`Resolver::redeclared`, read from
  the markers `Name.var` and `.Name` the Go pack stores as constants of no value); a `const` is
  never one.

- **Go channels** (spec #25, #82): `channel/go.rs` lists each library (`LIBRARIES`: the
  import path without its module's major version, `modfile::unversioned`, which the Go routers
  and clients use too, and the constructors that make its client)
  and reads a call when `Code::origins` with `Follow::Declarations` traces its receiver to one of them (the
  same question as the written-type read of the clients, with the constructor's steps: it also follows `x, err := f()`, a repository function that returns a
  library type, and a method on a value the library made, such as `conn.Channel()`). The
  channel name goes through `Code::expand`, struct fields of a composite literal included:
  `values::composite` gives `Value::New(Type, [Object(fields)])` for a struct written with its
  type and `Value::List` for a slice (`Code::eval` descends into `New`); `aws.String(x)` is `x`.
  A callback is the handler (`Handler::Inline`, or `Def` for a function or a method value
  `a.onAudit`, through `Code::definition`); a polling client's handler is the function containing
  the call that names the topic, queue or stream (the `consume` default); a sarama group's is
  the `ConsumeClaim` of the type its `Consume` is given (`Code::method`, which serves nothing
  else). A writer that names a topic (`kafka.Writer{Topic:}`, or the argument of kafka-go's own
  `NewWriter`, never a repository function of that name) stands for its messages, read from
  `Code::constant` of the writer's name, and a `Topic` field written anywhere in the repository
  (`Code::field_written`, the markers of #79) leaves it unresolved; a writer a function made, or one with no topic, is read from each message. The
  `xlang-go-*` fixtures carry a TypeScript producer to the Go consumer group.
- **Java channels** (spec #26, #92): `channel/java.rs` lists each client by the outside types
  the source may write for its receiver (`CLIENTS`: `KafkaTemplate`, `RabbitTemplate`,
  `JmsTemplate`, the SQS clients, jnats `Connection` and `Dispatcher`, the Jedis clients and
  Lettuce's `RedisCommands`), read through `names::typed` and `one`, and the listener
  annotations (`@KafkaListener`, `@RabbitListener`, `@JmsListener`) through `names::known`
  on a method's `Call::decorates`. Names fold through `Code::expand_with` with
  `jvm::spring::inject` (`Expand::Chain` for an SQS request, a Redis group read and a
  `BindingBuilder` chain); an annotation's values fold through `Code::value` and its placeholders
  resolve through `jvm::spring::resolved`. An annotated listener's handler is the method
  (`Handler::Def`); every other client takes `consume`'s default, the method holding the call
  that names the topic or queue (the Java pack keeps a lambda's calls in the method that writes
  it, so a jnats callback is that method too). The overload of `RabbitTemplate.convertAndSend`
  and `JmsTemplate.convertAndSend` is told by the type of the argument a `MessagePostProcessor`
  would take (`of`, through `names::declared`): a name the source types as one, or a `new`
  of one, picks the routing-key form, and so does a `CorrelationData` there
  (`(routingKey, message, correlation)`) unless a `MessagePostProcessor` before it makes it
  `(message, postProcessor, correlation)`; an argument whose type the source does not tell (a
  lambda, a call, a name with no written type, anything the pack cannot read) leaves the
  producer unresolved, and a literal, a name of another written type or one written as a type of the repository (a
  DTO) is the message, so a publish of a repository type resolves. The two-argument AMQP form publishes to the
  template's exchange: `spring.rabbitmq.template.exchange` of the module's configuration, else
  the default exchange, a producer of the queue the key names. An `@Exchange` is `direct`
  unless its `type` (a string, or a member of `ExchangeTypes` bound by an import) says
  otherwise: a direct or fanout exchange keeps only the keys with no wildcard word, a headers or
  custom one none, a topic one, or one whose type is not read, every key; a `DirectExchange` in
  `BindingBuilder` keeps only a key with no wildcard. The listener annotation is asked first
  (`names::known`), and only then whether it is on a method (a set of the file's methods,
  built once), so a file of many `@Override` methods is read in linear time. The Java pack reads an
  annotation written as an element value as `Value::New(name, [Object(elements)])`, its unnamed
  element as `value` (the shape of a Go composite literal; `Code::eval` folds it), only inside
  an annotation's arguments, which the index never keeps; `channel/java.rs` reads a nested
  `@QueueBinding`, `@Queue`, `@Exchange` or `@TopicPartition` by its element's key, since Java
  types an element by the annotation that declares it. `jms` is a broker kind of its own (also
  AsyncAPI's `jms` protocol), which the Kotlin pack reuses.
- **Kotlin channels** (spec #27, issue #104): `channel/java.rs` reads `jvm_files` (`.java`,
  `.kt`, `.kts`) with no pack gate, since both packs share one `Resolver` and one `Code`. The
  Kotlin pack gives it the same shapes: `kotlin::element` reads an annotation element value
  (a nested `QueueBinding(..)` as `Value::New`, `[..]` and a nested `arrayOf(..)` as
  `Value::List`; a top-level `arrayOf(..)` stays a `Value::Call` that `jvm::names::array_of`
  accepts only when `known` binds `kotlin.arrayOf`, the question the Spring MVC reader asks
  too). An entry `k to v` of a `mapOf(..)` is its key `k` (`Args::Pairs`): every entry of a
  `mapOf` is a pair and only the key is read, and a pair built as a call costs two nodes of
  `EVAL_BUDGET` per entry in a file full of `mapOf` calls. The Kotlin `Call::on` is always empty,
  so a `BindingBuilder` chain is found by the callee shape `(call_expression).with` and its
  start call by range in a map built once per file. A stream map written `mapOf(key to id)`
  counts when `known` binds `kotlin.collections.mapOf`; a constructor with no receiver
  (`Queue("q")`) counts as `new`, and so does a message `Order("1")` of `of()` when the callee
  is a class (the repository's, or an outside one an import spells): `buildMpp()` is not told.
  A lambda argument (`convertAndSend("x") { m -> m }`) is an untold overload, as Java's, so its
  producer is unresolved.

  A member of an implicit receiver comes before the names around it, as Kotlin resolves them:
  in the block of a `with`, `run` or `apply`, or in an extension function, `kafka` in
  `kafka.send(..)` is the receiver's property when the receiver is a class of the repository
  that has one (`Resolver::lexical_as`, through `Facts::impls` and `Facts::extensions`), for
  every name read: the head of a call, a constant's value, the type a name is declared with
  (`Code::declared`) and the object the flow of `Follow::Values` follows. A parameter or a local
  of the code around the block comes first (`Resolver::local_around`), and the property of the
  class around only when the receiver is a class of the repository without that member. A
  receiver whose type is not the repository's leaves the name unknown only where the object it
  holds is asked (`Reading::Member`: `Code::declared` and the flow of `Follow::Values`, which find
  clients); a constant's value and a call's receiver are read outside the block (a Known
  ceiling). A call on the receiver itself
  (`kafka.apply { send(..) }`, `with(kafka) { send(..) }`, an extension function of the
  template) is not read: its callee has no receiver name. The `conformance-kotlin` fixture holds
  each channel form and the hidden-name cases (`channels/Receivers.kt`, `jvm/Hidden.kt`), and the
  `xlang-kotlin` one a TypeScript producer reaching a Kotlin `@KafkaListener`.
- New **package ecosystem** (spec #22 user story 26, spec #225; track B packs): implement the
  `Ecosystem` trait of `package.rs` in a module of its own under `package/` (`npm`, `pypi`,
  `cargo`, `go`, `maven`, `nuget`, `swiftpm` today) and add it to `ECOSYSTEMS`; the package kind, the index writer
  and the packs learn no name. An arm for one ecosystem added to the shared code is a finding. It plugs
  in another manifest format (`Cargo.toml`, `go.mod`) without a new kind of contract: the
  contract `package::<name>::<package>` (`Ecosystem::prefix`, derived from `name`) still routes
  to the same `Packages` steps (`declare`, `consume`, `import`, `shapes`, `link`, `place`,
  `reached`), `Providers` is keyed by ecosystem name and package name, so two ecosystems never
  link, and the reason grammar (`reason`) stays one function that asks the ecosystem.
  The questions, each with its default (an adapter overrides only what its ecosystem answers
  differently):
  - Required: `name` (the contract prefix and the export's `ecosystem` field spell it);
    `is_manifest(file name)`; `read(path, bytes)` (the same `Summary` for every ecosystem: name,
    version, dependency entries, entry fields, source roots, console scripts; it never fails,
    hostile bytes read as empty); `sources()` (the extensions of the source files whose imports
    name its packages); `names_package(specifier)`; `entries(..)` (the dependency entries an
    import of a file may name, each with the subpath inside the package). `imports(..)` (the
    `(specifier, chain)` pairs a call goes through) has a default, `Code::imported`; Maven
    overrides it (the class and `name(arity)`, from the head's import or the receiver's type,
    one pair per on-demand candidate, `Outside::spellings`; a Kotlin file adds the package and
    the name of a top-level function or class a bare name imports, and for a call on a receiver
    that is no class of the repository the package and `.name(arity)` of the extension function
    its name imports; `kotlin.` and `java.` packages are none). `declares(..)` (whether a
    provider declares what the `(specifier, chain)` names, whatever its signature) defaults to
    `true`: `import` lets none of one site's pairs enter when two are declared, so only an
    ecosystem with several pairs per call answers it. `settle(..)` (default: nothing) runs at the
    start of the workspace step, over every repository's manifests, for an ecosystem whose
    entries name a provider by location: NuGet gives a `ProjectReference` entry the id of the
    project at its path (a project file's name need not be its id), and a `PackageReference` the
    spelling of the one provider whose id differs by case only, before `consume` reads them,
    each by one folded-location map (linear in the manifests).
  - `fits`: literal package-name equality; Maven matches the literal fragments of a computed
    coordinate (`pattern_fits`), a coordinate being computed only for a `${..}` or `$name` hole the
    build files leave without a literal (a pom property, `project.groupId` or `project.artifactId`,
    a parent pom's property through `inherit`; a Gradle name assigned once to a plain string).
    `uncertainty`: none; Maven's `computed` source keeps even one match ambiguous.
  - `unsourced`: none; PyPI says an entry with no uv source names no local package, so a lone
    fit in the consumer's own repository is `inferred` (#535).
  - `flag`: the range is unchecked (`range unchecked`); npm checks semver and
    PyPI PEP 440 (`package/pep440.rs`).
  - `reaches`: every provider; npm's `workspace:` keeps the dependent's own repository, Cargo
    and Go share `sourced`, in the seam (a `path` by the folded location, a `git` by the
    remote's identity); NuGet's `ProjectReference` (a `path <project file>` source) uses it too
    and only for a project of another repository, so a reference inside one repository adds no
    package link (the pack resolves it), while a `PackageReference` reaches every provider of
    the id.
  - `submodule`: none; PyPI's package `__init__.py` names its submodules (`name/__init__.py`, then
    `name.py`), as `import pkg` then `pkg.name.f()` reaches them.
  - `owns`: every manifest of the ecosystem bounds the files under it; PyPI only a
    `pyproject.toml`, Cargo only a manifest with a `[package]`.
  - `ways` and `resolve`: Node's rule (`exports`, else `types`, `module` and `main`, else the
    subpath as a path, or `index.<ext>` for the bare name when none of the three exists, `exact`;
    then the candidates among the files the manifest owns); Cargo follows the crate's library root alone
    (`How::CrateName`, no candidates, and the library-head check of an entry that does not
    rename its crate); Go looks in the one directory the subpath names (`How::ModulePath`); Maven
    looks the class up by package declaration in the files the artifact owns that are no test
    sources and picks the method by name and argument count among the overloads of the class's
    family (`How::Package`, `Maven::resolutions`; the provider's `families` rows, an opaque
    ancestor keeping the target from `exact`). Where no class is the specifier, Maven reads it
    as a Kotlin package (`top_level`: the symbols of `Classes::named` that the file exports both
    as written and with its package, which is what makes a declaration top-level and public): a
    function by name and argument count (`ambiguous` when several fit), a class by its capital,
    as the class itself (its constructor), an extension function (`.name(arity)`), which
    crosses only through the import of its name: the chain carries the receiver's type
    (`.name(arity)@Class`) when the source writes one, and a member of that class the provider
    declares hides the extension. The one extension of that class itself (`package.Class.name`,
    so the class is of the extension's package) is `exact` when a provider declares the class
    and its family is all known; any other is a name match, `inferred` (or `ambiguous`). An
    extension file that imports another class of the name its package declares would make that
    `exact` wrong, which the provider rows cannot tell (a Known ceiling). A declaration `Classes::published` shows as an
    extension of its class (`Class.method`) is never one of its members (`methods`). A chain
    `Companion.name(arity)` is a call on the class's name: the companion's member first, the
    class's own after; a plain chain is the instance's, in the reverse order. A function with a
    default parameter or a vararg has a `flexible` row (`resolve::Flexible`, the least and most
    argument counts), which `Classes::fits` reads before counting the parameters in the id.
    NuGet shares Maven's `imports`, `resolutions` and `declares` (free functions in
    `package/maven.rs`, taking the ecosystem and the `How` to report: `How::Namespace`,
    `; import namespace`): C# exports its types as Java does, under the qualified name, so the
    class lookup, the family walk and the argument-count pick are the same code; `declaring`
    counts a file only for the ecosystem whose `sources()` has its extension; `entries` is the
    references of the nearest SDK-style `.csproj` above the file (`owner`, `Summary::project`,
    as the pack's `join`), then those that pass on (`Entry::private` false) of each project of
    its repository it references (`references::referenced`), nearest first, the nearer entry
    winning a name; Maven's is the dependencies of the poms holding the file, then those that
    pass on (a `compile` scope, the default, not `optional`) of each module of the repository a
    `pom.xml` names by its coordinate, through the dependencies that pass on, nearest first and
    at most `MODULE_DEPTH` (32) modules deep (a module is its own `pom.xml`, without its
    parent's dependencies, and a Gradle dependency is not followed); the three provider loops keep the providers an entry's `reaches`
    lets through, as the default `resolutions` does, so a `ProjectReference` pins its project;
    the shared code names no ecosystem: each passes its platform prefixes (`java.`, `kotlin.`
    for Maven, `System.` for NuGet) to `imports`, and the class tables take every ecosystem's
    source extensions.
    Ceilings: a `ProjectReference` path written in another case than the file system's does not
    match (ids match case-insensitively, paths do not), extension methods and a name written
    fully qualified with no `using` are not followed, a receiver typed by a `var` whose value is
    a call through a package (`var o = Order.Of(1); o.Total()`) is not followed (the return
    type lives in the provider's index; Java has the same gap), and two projects in one
    directory share it (the first by path owns the files).
    SwiftPM (spec #29, #124; `package/swiftpm.rs`) reads `Package.swift` with
    `package/swiftpm/manifest.rs` (`package`: the `Package(name:)` literal, `.library` products
    with their literal target names, and `.package(url:|path:, ..)` dependencies with their
    requirement, walked in source order on an explicit stack), the PackageDescription forms, on
    the grammar readers of the pack's `lang/swift/manifest.rs` (`call`, `pattern`, `literal`,
    beside the `targets` the pack's layout reads), so there is one manifest reader.
    `Summary::products` and `Summary::targets` (module, directory) record what an import can
    name. An entry's `via` is `git <identity>` (`normalize_remote`, from the root manifest only),
    `path <dir>/Package.swift` (`sourced`) or, for a URL or path with an interpolation, `computed
    git|path <pattern>` (holes written `*`, matched with `package::pattern_fits`, so the dependency is
    `ambiguous` and lists every provider whose literal parts fit). An uncertain entry's name always
    carries a `*` (`marked`), even when folding took the computed part out of the location (a
    userinfo, a port, a `..`), since `fits` tells an uncertain entry by it; a computed path
    that folds to a first part of its own is held to its repository (`./*`), one that starts
    computed is not placed. A package whose `name:` is computed is a provider named by its
    pattern (`*`, `Kit*`); `settle` gives each literal entry the provider's name from its
    location, and turns the entry into the `computed` form of its source when that name has a
    `*`, so it links `ambiguous`. A `path:` to the repository's own root is an entry named `.`.
    `imports` turns a call into `(module, Type.member(labels))`: the receiver's written type
    (`Code::origins`, `Follow::Declarations`) or the construction a local holds
    (`Follow::Values`, one origin, no steps), else `Code::outside`'s candidates; the lookup
    reads the provider's `exported` rows (a public name only, an overload family has no target)
    and narrows by labels from the symbol id plus its `Flexible` argument counts, with the
    resolver's matcher (`resolve::labelled`), each parameter one the call may leave out (and,
    for a function with a variadic one, each variadic), since which have defaults is not
    stored. `resolve` is `exact` for one exposing target and `ambiguous` for several; the level
    of every link of an import is `ambiguous` when several packages the file depends on expose
    the module, whether or not each declares the name (`Swiftpm::exposing`, on the entries and the
    providers they reach that `resolutions_by_ways` walks, `reachable`). `How::Module`,
    `; import module`; the platform modules (`PLATFORM`) are never
    followed. Ceilings: every dependency of the nearest `Package.swift` counts for the files
    below it (not only a target's `.product(name:package:)`), an `internal` member of a public
    type, an inherited member and an extension of another module are not told apart, and the
    `Flexible` counts do not say which parameter is optional, so a call that skips a required
    parameter and keeps an optional one still fits.
    `member_of`: none; Maven's and NuGet's
    constructors and methods of a class (a companion's too), so `context` on a public class
    lists, once per call, the calls that reach its members (`reached`, at the best level of the
    call's links into the class). `JVM` source roots (`jvm::ROOTS`, `jvm::TEST_ROOTS`) are
    `src/{main,test}/{java,kotlin}`, and the Gradle scanner reads the `java` and the `kotlin`
    directory sets, `sourceSets["main"]`, `named("main")` and `getByName("test")` heads
    included. A Java set other than `main` is a test set; a Kotlin one is told by its name
    (`main` and `*Main` production, `test` and `*Test` test, any other not read).
  - `resolutions`: the single-entry loop (the entries, each provider the entry `reaches`,
    `resolve` in it); PyPI replaces it whole, since its imports name a module and several
    distributions may expose it.
  - `shapes`: false; npm's packages carry them. `programs`: none; PyPI's console scripts.
    `inherit`: nothing; Cargo's workspace entries and version.
  Callers use the seam's free functions, never the adapters: `of` (the adapter whose manifest a
  file is), `of_source` (the adapter whose sources a file is, none for a file no ecosystem
  claims, so a pack whose ecosystem has not landed imports nothing), `of_contract` (by prefix),
  `is_manifest` and `read_manifest`, `programs` (every adapter's, sorted, without repeats) and
  `inherit` (every adapter's, in list order). `package::is_manifest` makes `manifests_found`
  (`index.rs`) stamp the manifest files into `manifests`. The list order fixes the order `of`
  and `of_contract` search in and the order `inherit` runs in; a new ecosystem appends. The
  deliberate extras: `is_npm`, the export's compatibility rule (it names the ecosystem only when
  it is not npm, so the goldens that predate the field stay); a reader in a language pack when the manifest is also a pack config (as
  `go.mod`, read by `lang::go::modfile`, and `Cargo.toml`, read by `lang::rust::crates`, are;
  the adapter calls the pack's reader, never the other way); and, for a user-visible ecosystem,
  the README state line and the CHANGELOG.
  Which ecosystem a call through an import belongs to is its file's (`of_source`; Cargo's for a
  `.rs` file, Go's for a `.go` one), and resolving the import into the provider is the
  ecosystem's own rule behind `resolutions` (npm: `split`, then `ways` and `resolve` by Node's
  rule; PyPI: the module found under the provider's roots; Cargo: `cargo::names_crate`, the
  `use` head matched to the key of a renamed dependency or to the provider's library name
  (`cargo::lib_head`), then the crate's library root; Go: any import path that is a valid
  contract name (`valid_name`), matched to the longest module path of the nearest `go.mod`'s
  entries (`modfile::inside`), then the one directory of the provider the rest names; `Go::read`
  takes `require` and `replace` from `lang::go::modfile`, the Go pack's one `go.mod` reader, and
  gives a local `replace` the Cargo `path` form in `Entry::via`). A step outside `ContractKind`
  fills what a manifest inherits: the seam's `inherit`, run in `index::read_manifests` over the
  whole manifest list after the reads, has Cargo fill the `workspace = true` entries and
  `version.workspace = true` of each `Cargo.toml` from the nearest one above it, so a refresh
  equals a cold export. `Ecosystem::reaches` takes the repositories' roots (`Export::root`, set
  by `Engine::exports`) for a Cargo `path` that leaves its repository: it folds both locations
  as paths, never opening one, so a symlink changes nothing. A change to what is stored
  re-blesses `tests/golden/schema.txt`.

- **C# channels** (spec #28, ticket #115): `channel/csharp.rs` answers for the `cs` pack only.
  A form is a method called on a client, and a call counts only when its receiver is one of
  `TYPES` (Confluent.Kafka's `IProducer`, `IConsumer` and builders, RabbitMQ.Client's
  `ConnectionFactory`, `IConnection`, `IModel`, `IChannel` and the eventing consumers,
  AWSSDK.SQS's `IAmazonSQS` and `AmazonSQSClient`, NATS.Client's `ConnectionFactory` and
  `IConnection`, StackExchange.Redis's multiplexer and `IDatabase`, Azure.Messaging.ServiceBus's
  `ServiceBusClient` and `ServiceBusSender`) by the type the source writes for it (`Code::written`:
  a field, a property, a parameter, a typed local) or by the construction it was made by: a
  `Value::New`, a local the pack bound to one, or a call on a client that makes another
  (`STEPS`: `Build`, `CreateConnection`, `CreateModel`, `GetDatabase`, `CreateSender`, the
  static `ConnectionMultiplexer.Connect` called on the type's own name; a builder's `Set*`
  hands the builder back). A type is accepted written in full (`names::in_full`) or as the one
  spelling in the table the usings give it (`names::spellings`, `names::one`), so a type of the
  repository with the same name hides
  the library's, and `ConnectionFactory` or `IConnection` with RabbitMQ.Client and NATS.Client
  both imported is none. The receiver is followed at most `CHANNEL_CLIENT_DEPTH` links. Names fold
  like a URL through `Code::expand` (a wrapper's parameter makes each caller a producer or a
  consumer): `string.Empty`, and `String.Empty` or `System.String.Empty` that bind
  `System.String` (`names::is_type`), written in the exchange of a `BasicPublish`, is `""`
  (`empty`), the pack folding none of them; an environment or configuration read is
  `Part::Read`, so an unresolved name; the first argument names a Kafka topic (a list for
  `Subscribe`), an SQS queue (the last segment of the URL, or the `QueueUrl` of a request
  built in place or in a local), a NATS subject, a Redis stream key and a Service Bus queue or
  topic. A named argument is read by its parameter name. RabbitMQ: `BasicPublish(Async)` is a
  publish to an exchange (the empty one is a producer of the queue its key names), `QueueBind(Async)`
  a binding (spec #20's routing rules), `BasicConsume(Async)` takes its queue from the first
  argument (the second when the first is a consumer by its type, a field and a parameter
  included). `.ConfigureAwait(..)` hands the client back, as a builder's `Set*` does. Service
  Bus: a send method on a sender is a producer named by the first argument of the `CreateSender`
  call that made it (in place, or held by a local, a field or a property written once: every
  write of a name is a `name=` bound of its own, an initializer that is written again
  included, and `sender` accepts exactly one); when that argument is a parameter, the producers
  are the callers of the method around the `CreateSender` call (`Code::expand` from it, once
  for all the sends on the sender), at their call sites. `CreateProcessor(queue)` and
  `CreateProcessor(topic, subscription)` are consumers of the first argument alone, so a
  processor on a topic consumes every message sent to it whatever its subscription.
  Handlers: the pack records each `x.Event += handler` as a bound named `x.Event+=` (a lambda as
  its `Kind::Inline` definition, a method group as the name it writes), one per attachment, and
  the reader makes a consumer of each handler on the event (`Received`, `ReceivedAsync`,
  `MessageHandler`, `ProcessMessageAsync`) of the consumer, subscription or processor the
  call's result is held by, in the order attached
  (`Code::bindings`, read once per file into a map by the call's range); a NATS
  `SubscribeAsync` takes the callback it is given, a lambda or a method of the repository; every
  other client, and a callback not found, takes `consume`'s default, the method holding the call.
  The pack also reads an `await` as its operand, in a value and in what a field or a local holds
  (`held`). The `conformance-csharp` fixture holds a client of each library
  (`Channels.cs`, `Channels.Nats.cs`) and `xlang-csharp-{web,api}` a topic produced in
  TypeScript and consumed in C#.
- **Shapes through package imports** (issue #175): a source reading a name that `Code::imported`
  says comes from outside the repository records a `shape::Import` on the shape (its path, the
  importing file, the specifier and name, and the `Want` view: `zod`, `infer`, `literals`,
  `type`, or `opens` for a provider typed with a package type, which takes that type's index
  signatures alone) instead of opening it or dropping it. A repository keeps, beside its
  contracts, the shapes of the module-level constants and types the files of its packages'
  public surface export (`surface`: the entries of each named manifest, as an import
  resolves them, and what their `exports` rows lead to; `exported_shapes`, a step the package
  kind's `extract` runs after its imports, from `Indexed`, as
  `Role::Shape` rows; each source reads its own: `Types::exported`, `Zod::exported`,
  `Literals::exported`). The workspace step `package::shapes`, after `import` in the package
  kind's `workspace_step` (which `Engine::exports` runs), resolves each import as a call through it resolves (the ecosystem's
  `resolutions`, over the `exports` rows, which name constants and types too) and fills it with
  `shape::fill`, the one recursion and the one place its limits live, carrying the
  resolution's level: through one that is not `exact` (an `outDir` mapping) a provider's part is
  `guessed` and a consumer's paths are not declared, so no drift on them is `exact`. A new source that reads
  an exported name adds a `Want`, a reader's `exported` and an arm in `exported_shapes`.
