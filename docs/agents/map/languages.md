# Language packs

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

The steps that add a language pack, and the forms of the builder languages and Go
environment reads; the architecture map keeps the lead of the first:

- A **builder** language (spec #30 #129: a call whose method returns its receiver, such as
  `Router::new().route(..)`): the pack sets `LanguagePack::fluent` and gives each call its
  `Call::receiver` (a value emitted from the syntax, not stored in the index, like `args`).
  `Code::origins` then walks a chain down its receivers, by `call_at` (a call found by its byte
  range; `Code::call_index` for a kind) and `Code::receiver` (the value a call is made on).
  `Code::chain` is that walk, memoized by `chain_tail`: the calls from one down to where the
  flow stops (a call a function of the repository answers, or one made on no call). A kind
  that reads a chain step by step (an axum method router, an actix-web `ServiceConfig`, a
  Kafka record, an SQS request, redis read options) goes through it, never its own walk.
  A step no function of the repository answers passes the receiver on; one only a name match
  found does when the chain starts at a constructor of an outside crate (whose method it then
  is), else the flow ends. Which methods pass the object on is listed per object, with its
  constructors: `FRAMEWORKS` in `http/rust/frameworks.rs` for the web frameworks and HTTP
  clients (`passes_on`, and `frameworks::made`), `LIBRARIES` in `channel/rust.rs` for the
  brokers. The tables live under the kinds, never in the pack: the kind hands the question its
  predicate (`Follow::Values { passes }`, `passes_nothing` for a reader with no table) and the
  filter reads it. A kind reads a route off each call of the chain (`http/rust/axum.rs`, and `http/rust.rs`: actix-web
  `App`, `web::scope`, `web::resource`, `.configure(f)` and Rocket's `build().mount(..)`, whose
  `routes![..]` the Rust pack reads as a list of the handlers' paths).
  `Code::returned` and `Code::constant` give a kind the values a call may return and a
  constant holds, for what it must not leave unmounted. `Code::origins_within` is the value
  flow of what is written inside the functions some calls reach (an origin's
  `Route::through`), each call binding the parameters of its function as when the walk climbs
  its return values: a kind that climbed to a value reads what is written there with that
  call's arguments alone, so two objects one factory makes from two arguments stay apart,
  through a local holding the parameter too (`http/csharp/minimal.rs`). `Code::reaches` says
  which function of the repository the value flow reaches through a call (exact, or a C#
  extension method), for a kind whose library name a method of the repository hides only
  where it answers the call.
  `Expand::Chain` is `Code::expand` for a request built by a chain (`http/java/clients.rs`; in
  `channel/java.rs` an SQS request, the stream maps of a Redis group read and a
  `BindingBuilder` chain):
  the reader gets the call as one `Value::Call`, its receivers and every argument along the
  chain folded with the wrapper's callers bound, and a name an import from outside binds as a
  `Value::External`, spelled (`Outside`, candidates included) in the file that writes it. An
  enum member passed through a wrapper so keeps the caller's import evidence, and a type of
  the caller's repository by that name is no member. Only this reading folds a call's
  arguments or makes a `Value::External`: `Expand::Call` and `Code::value` leave them as
  written, so no other reader sees a change.
  `Code::expand_with` is `Code::expand` with an injector (`resolve::Inject`): what a kind reads
  off an annotation written on a field or a parameter (`Injected::Annotated`, from
  `Facts::annotated`, transient, read from the full facts so a file the index kept is parsed
  again through `Code::misses`), and, for a parameter no annotation on it injects into, what it
  gives the parameter itself (`Injected::Param(function, n)`: a dependency injection container
  passes a constructor's parameter by its type, with no annotation, as C#'s typed client takes
  its `HttpClient`). The shared side asks it for a field no file assigns (`Code::field_written_in`, the `.f` and
  `.f@class` marker constants the Java pack writes for a field write: `f = v` of a name no local or parameter declared
  before it, `f++`, `this.f = v` and `x.f = v`; an assignment to a local or a parameter is not one, and a write carries
  its class when the pack knows it, which a classless write, as `x.f = v`, does not) and for a parameter no frame
  binds of a function no call of the repository may reach (`Code::may_be_called`: an exact
  caller, or any callee ending in its name, so a name match, a call on an opaque receiver, a
  `new` inside an anonymous class and `this(..)` or `super(..)`, which the Java pack emits as
  constructor calls, all count); a function some exact call reaches climbs to its callers as
  before, so the injected value never stands in for what a caller passes. A
  constructor's parameter a `final` field holds (`Const::scope`, the constructor the value is
  written in) is no wrapper's parameter: it is injected or unknown, never climbed. The values
  an injector gives are transient like the arguments they stand for, so the index stays free of
  them. `jvm/spring.rs` is the JVM packs' injector (Spring's `@Value`, `spring::inject`, which is
  given the annotation with what it marks, and nothing for `Injected::Param`), and
  `http/csharp/clients.rs` C#'s (an `HttpClient` constructor parameter, `Injected::Param` only); `spring::resolved` is the one
  rule for an annotation's text Spring resolves (`@Value`, a Feign `url` or `path`, a listener's
  topic or queue): literal text with its placeholders resolved, an environment read kept, any
  other part unknown. The list of Spring's config files is the reader's too
  (`spring::CONFIG_FILES`), which the Java pack returns as its `config_files`. `names.rs`
  holds the name questions the JVM and C# kinds share: `one` (exactly one spelling in the kind's
  table), `known` (a name an import binds), `annotation` (`known` for an annotation's callee, which no
  field of an outside base hides, #354), `spellings`, `typed` and `declared` (the outside type
  the source writes for a name; a type of the repository is an answer with no spelling, `Code::declared`
  gives it an origin with no outside name), `in_full` and `is_type` (a type written in full is the
  library's unless a definition of the repository binds that very name, as a compiler prefers
  the source's type; else the one spelling the usings give it, one rule for every C# kind) and
  `is_method` (an annotation on a method of the file);
  `jvm/names.rs` keeps the Java readers' filters `java_files` and `jvm_files` (Java and Kotlin
  files), `kotlin` and `array_of`.

- **Go environment reads** (spec #25, #80): `os.Getenv("X")` is `Str([Env(X)])` and
  `os.LookupEnv("X")` is so for the first name of a `:=` or `var` (`values::lookup_env`), through
  `strings.TrimSuffix`/`TrimRight`/`TrimSpace` (`values::env_value`) of the read or of a name
  `Std::note` saw bound to one earlier in the file (source order; a package-level `var` counts).
  `v = strings.TrimX(v, ..)` leaves `v` a constant when it holds a read, and so does
  `if v == "" { v = .. }` when the default is plain (`values::keeps_env`, `values::plain`,
  decided in `Walk::finish_values`); the test may also be `len(v) == 0` (no binding of the file
  hides `len`; a package-level `len` in another file is not seen), `!ok` or `!ok || v == ""`
  (`values::Guard`: `ok` is the second name of the `v, ok := os.LookupEnv(..)` that bound `v`,
  bound once and never assigned): text, or names that are constants of the same file holding
  plain values; a call, a name bound to a read, a loop variable, a package `var` or a name from
  another file leaves `v` no constant, so the base is absent. `http/go/clients.rs` `one_read`
  makes every read of a URL a hole when it holds two. `http.rs` `locate` and `link` map the
  head `Env` as for every language. The `xlang-go-*` fixtures are the cross-language chain: a
  repository of another language joins by one more `common::xlang_go` entry.

- New **language pack**: add it to `PACKS` in `lang/mod.rs`, a fixture
  `tests/fixtures/conformance-<language>/` and one `conform(...)` test in `tests/it/conformance.rs`.
  A pack is done when that suite passes. A pack ships syntax and value evaluation only: no
  table of libraries and no form of one. The forms of its web frameworks, HTTP clients and message
  libraries are separate tickets, each a module of the pack's language in the kind that reads
  them (`http/<language>.rs`, `channel/<language>.rs`). A pack whose facts depend on its config files
  (Rust's default features) also gives `settings` (a string per file path, stored as
  `files.setting` and `Facts::setting`) and takes it in `parse_with`; a file whose setting
  changed is parsed again, whatever its content. A pack whose facts depend on other files of the
  pack (Rust's item macros, #398) also gives `extend`, what the facts of every file add to each
  one's setting, on the lines after it (`lang::setting`, `lang::base`, `lang::extra`): it reads
  only facts no setting changes, so `index::settle` parses each file whose setting moved once
  more, after the changed files are parsed and the kept ones read, and a cold index and a refresh
  agree. `refresh` and `fresh` compare only the first line with `settings`: the rest moves only
  when some file's facts do. Besides `parse` and `modules`, it says what the
  shared resolution cannot guess: `members_in_scope` (bare names may name class members),
  `assigned_members` (a definition named by a dotted path, `app.use` of `app.use = function`, is what
  that path written in a call reaches, though its head is a local or constant: JavaScript's),
  `guesses_constructs` (`Call::constructs` is a guess from the callee's case: a guess that is no
  `exact` type is read again as a plain call, and a function it binds wins),
  `unordered_globs` (combine wildcard candidates, with an inside glob taking priority),
  `qualified_types` (an unbound path head may name a repository type through its layout),
  `Facts::overloads` and `Facts::constructors` (separate method and constructor families with
  written signatures), `Call::arity` and `Call::constructs`
  (the argument count and constructor call form, kept in cached facts), `Def::suspends` and
  `Call::awaited` (Swift: a call not under `await` does not fit an `async` function), `Facts::statics` (members a class wildcard
  import takes), `Imported::Qualified` (a fully qualified definition import),
  with Java parameter types spelled from syntax tokens, excluding annotations and comments,
  and `#method` after a class-named method's signature to distinguish it from a constructor,
  `receiver` and `base_receiver` (keywords for the enclosing
  class and its base), `globals` (names bound everywhere), `Def::bases` (a class's bases, for
  its family), `Facts::receivers` (the class each binding gives a name), `Facts::star` (what a
  star import takes), `stars_bind` (whether a star import rebinds names in statement order, as
  Python's does, or only re-exports, as an ES `export *` does) and `Param::binds` (which
  argument binds each parameter; `Binds::Optional`, a Kotlin parameter with a default value, and
  `Binds::Rest` (a Kotlin `vararg`, a Java spread parameter) widen the argument counts an overload takes, `Arity` in `Resolver::narrow`). A pack
  whose files resolve with another pack's says so through `LanguagePack::resolves_with` (Kotlin
  with Java): `refresh` keys its resolution groups by `lang::resolver_of`, so one `Resolver` and
  one `Code` built from that pack's `layout` and flags take both packs' files, and calls between
  the two languages resolve by fully qualified name. The joining pack's own flags and `layout` are
  not read, and its files reach the contract kinds through that pack's `Code` (the Java kinds read
  `.java` files alone, but the Spring MVC providers, which read Kotlin too). This keeps one
  resolver per repository and pack group, with no new invariant (ADR 0002). Java's `layout` therefore also maps a Kotlin
  file's top-level overload families and constants by fully qualified name, and a Kotlin class
  joins its own name's overload family with its primary constructor's parameters, so calling the
  class constructs it. A companion object is written as its class's last base: `Resolver::lineage`
  keeps a class's own nested class out of its family, so the companion's bases lend nothing to
  instances, and `Resolver::companion` reads it after the family, for a bare call in the class
  (`inherited`) and a member of an instance (`member`). A call or a read on the class's name
  (`Type.create()`, `Type.X`) reads the companion first (`Resolver::walk` through
  `Resolver::companions`, each class's companion read once after the families), then a nested
  class or a constant of the class, never an instance member, so `Type.m()` is the companion's
  when the class declares an `m` too; `this.m()` is a call on an instance. `Resolver::member` finds
  a class nested in a class while that class's family is still being linearized, which resolving
  such a base needs. A bare Kotlin call is `Call::by_path`, as Java's, so it reads the class family
  first, unless a local, parameter, property or constant of its name in a scope around it holds
  what it invokes (`bare_calls`). Java's `layout` gives a package's files before a Kotlin
  top-level declaration of the same fully qualified name, and leaves `.kts` scripts out of every
  package, since a script's declarations are its own (the Kotlin pack exports none of them). A
  Kotlin `object` has an `INSTANCE` property of its own type, which Java calls go through.
  Kotlin writes its receivers as Java does: a property, parameter or local typed by its written
  type (`Class::Type`) or initialized with a constructor call `T(..)` (`Class::Named`), where
  the one binding of its name says so; `Facts::externs` lists the type heads written in the
  file, so a type nothing binds is outside the repository (Opaque outside). A call on `x!!` or
  `x?.` is a call on `x` (`kotlin::callee`), of the type `x` is declared with. An extension
  function or property is a `Facts::extensions` block of its receiver's type, named as the
  function is (`pkg.Receiver.f()`): it tells what `this` is in the extension's body, and its
  functions are no members of the type, so Java calls never reach them, not even by name
  (`Resolver::candidates` drops them for a file whose pack has no extension functions). Written in
  its class's file, an extension is named under the class as a member is, so the member lookup
  (`Resolver::member`, `overloaded`) skips it by `Resolver::extends`: a member of the class or of
  a base (a Java one too) comes first, and the extension is reached through `extended` alone. A Kotlin call no member
  answers (`LanguagePack::extension_functions`) reads `Resolver::extended`: the extensions of its
  name in scope of the file, first to last a scope around the call, an explicit import, the
  file's package, a star import (`visible`), narrowed by argument count. On a receiver of a class
  of the repository whose family is all known, the first scope that holds one of a class of the
  family gives the one of the nearest class, `exact`. An extension of a type from outside the
  repository or of a type parameter may apply to any receiver (`Any`, `T`, `CharSequence`), and
  an outside receiver's own members are unknown, so those are name matches over the extensions in
  scope that may apply; a call on a type's name (`Outside.create()`) reaches no extension. A
  lambda passed to `apply`, `run` or `with` is a `Kind::Inline` definition of its own
  (`@<offset>`, folded into the one around it) and an `Impl` block of the receiver's type that
  declares no member (`impls` leaves it out, so a class's lookups never read one per lambda):
  `this` and a bare call there read a local function around the block first, then that type's
  family (`lexical_as` through `blocks`), then the names around it, or stay a name match when
  the block's type is no class of the repository (`unread`). The lambda of `let` or `also` is
  such a scope too, with its parameter (`it`) typed and no block; a lambda in it that declares no
  parameter is a scope of its own, whose `it` hides the typed one only inside it. Any other
  lambda written as an argument (trailing or in parentheses) is a `Kind::Inline` scope with no
  block, as the TypeScript and Rust packs write their closures: the call's argument is its
  `Value::Function`, so a kind can name it (a Ktor route handler), and it folds otherwise. A
  function declared where it is no definition (in a lambda, an object literal or an initializer)
  is such a scope too, so its body is not the code around it.
  `LanguagePack::is_code` drops from the walk the files of a pack's
  extensions that are no code (Kotlin's `build.gradle.kts` and `settings.gradle.kts`, which the
  package ecosystems read as manifests). A pack that
  writes none of them (TypeScript writes no bases or receivers) gets name matches where those
  would have resolved. A language whose files are not all built, or whose imports name files by
  a tree spread over several files (Rust's crates and `mod` declarations), says so through
  three more: `config_files` names with a `**/` prefix (every `Cargo.toml` the walk finds, in
  path order, or with `**/*.ext` every file ending so, as C#'s `**/*.csproj`; `lang::names_config`
  in the index and in `meada decide`, each read as a root config is (`read_config`) and folded into the configs digest on
  every refresh) and `layout`, which `refresh` calls once per pack and refresh with the configs
  and every file the pack parsed, and which answers five things from one build (Rust's crate
  `Tree`, so the manifests are parsed and the module trees walked once): `Layout::members` (the
  files it builds; the rest keep their facts in `files` with an empty `resolved` and no rows,
  and leave the export), `Layout::modules` (by default the pack's `modules`; it answers with a shared
  list, `Rc<[String]>`, so a lookup costs the same whatever the namespace holds), `Layout::published`
  and `Layout::programs` (both below), and `Layout::reaches` (default: none, every file), the
  files a name match may reach from a caller (`Resolver::reaching`, called by `refresh`):
  Rust's are its crate's and those of the repository's libraries it depends on. A language whose
  files share one scope (a Go package: the files of one directory that declare one package; Java and Kotlin packages, which share the Java layout, as C# namespaces do) says which through `Layout::shared`, groups of files that
  `Resolver::with_shared` takes: a name a file and its imports leave unbound is looked up
  among the top-level definitions and constants of the others in its group, private ones
  included (`Resolver::sibling`, at the end of `lexical_as`), and an import that names any
  file of a group (`Modules` gives the first candidate, which stands for them all) exports what
  any file of the group that names it in its `Facts::exports` gives (`Resolver::exports`, over
  `file_exports`); a method declared in another file of the group is found for its type too
  (`Resolver::sibling_method`, in `member`); files that define one name apart are `Binding::Several`, an `ambiguous`
  call. The group is built from `Facts::package`, the package each file declares. A member
  lookup (`Resolver::member`) reads the class's own file, then the group's others for a method
  (`sibling_method`). A name a class family declares as an overload family (`Facts::overloads`)
  is read per signature instead (`Resolver::overloaded`): in C3 order, each written signature
  from the first class that declares it, so an override replaces its base's signature and an
  inherited overload of another arity stays; a class past an opaque base that declares the name
  leaves it unknown. A pack with `members_in_scope` reads a bare called name with
  `Reading::Method`, which at each class body takes that family's overloads
  (`Resolver::inherited`) and stops, unknown, at a class whose family is absent or has an
  opaque base and declares none, or that does not declare a name every class has
  (`LanguagePack::implicit_members`); a bare field name reads the family's fields first
  (`Resolver::inherited_field`). Overloads merge across the family, so an opaque base anywhere
  in it keeps them from `exact` (Opaque outside), unless the pack knows its members
  (`LanguagePack::known_outside`, Known outside members, ADR 0019): a base its table lists is
  opaque only for the names it declares (`Resolver::may_declare`, asked by `Resolver::opaque`,
  `overloaded`, `member`, `inherited` and `inherited_field`), and for any other name the family
  resolves as one without bases, so a class `implements Serializable, Comparable<T>` has its own
  overloads `exact` and its `compareTo` a name match. The base is spelled as
  `Resolver::imported_name` spells it (qualified under an explicit import, bare under a wildcard
  or `java.lang`) and matches an entry by its qualified name, or a bare name by the entry's last
  segment. A pack's `Resolver` reads the tables of the packs that resolve with it too
  (`lang::known_outside`).
  `LanguagePack::unordered_bases` drops the written base list from the C3 merge (Java's
  `implements I, J` with `J extends I`), and a `base_receiver` call reads the overloads of the
  family past the class (`Resolver::base_member`). A call made on a call's result names it
  (`Call::on`), as a local may (`Class::Result`): the receiver is the class the declared result
  of that call's exact target names (`Resolver::result_of`, bounded by `RECEIVER_DEPTH`, across
  imports too, see [Known ceilings](../ceilings.md)). A name match of a call narrowed by `Call::arity`
  (`Resolver::matched`) reads the family that owns it: a typed receiver, `this` or a bare call
  match within that family alone, a bare call past an
  opaque base, or past a class around it with one for the name (`Resolver::outer_opaque`), falls
  back to what its scope binds, and any other receiver takes, from several,
  the one declared in the family of the class around the call other than the caller; a match
  narrowed from several is `Resolution::Narrowed`, stored as `inferred` with `calls.narrowed`. A language whose imports name a path rather than a file (Rust's
  `use a::b as c`) writes `Imported::Path` (what the dotted callee-like name gives, resolved
  from the scope of the import's own `local`, which a pack qualifies by the scope that holds
  it: `inner.c`, `f.c`) and `Imported::Glob` (every name the module the path gives takes:
  `inside` it, the module's private items too; `public`, a `pub use` also re-exports them),
  both found by `Resolver::lexical` at each enclosing scope (`imported_at`) and, for the file
  itself, by `imported` and `export`; explicit imports beat globs, and an alias or a glob
  that reaches itself gives nothing (`Seen`: the names being resolved, the globs by file, import
  and name, and the depth of links followed, bounded by `LINK_DEPTH`). A call name is never read
  as a `Kind::Module` (`Reading::Value`); a use path is (`Reading::Alias`). `Facts::externs` names the first segments of
  paths that nothing in the file binds (the crates outside it), resolved last in `imported`:
  a glob beats them, another crate of the repository answers through `Modules` and the rest
  is `Binding::External`. `Facts::private` lists the `exports` names `Resolver::exported`
  leaves out of the exports table (Rust's non-`pub` items: they still resolve inside the
  crate), and `Layout::published` (default: none) the files whose exports a pack counts
  at all (`Resolver::publishing`, called by `refresh`): Rust's are the files reached from a
  library root through `pub mod` declarations, so a binary crate exports nothing.
  `Facts::hidden` lists the methods (`Class.method`) that are no export door although their
  class is one (Go's lower-case methods and interface methods of an exported type):
  `entry::exports` skips them, read as a set per file. A block that adds members to a type written elsewhere (Rust's
  `impl`) is a `Facts::impls` entry (`Impl {scope, ty, of, within}`), which `Resolver::member`
  (`block_member`, before the class's own names: the inherent blocks' definitions, else the
  trait blocks' or their traits' default bodies; several at that step are `Binding::Several`,
  an `ambiguous` call with exactly those candidates) and `Resolver::class` (`Self` inside a
  block) read. A module is `Kind::Module`: its functions are name-match candidates like
  top-level ones, a `Kind::Module` that an import of its own name binds to a file (Rust's `mod
  store;`) has that file's exports as members, and `Resolver::lexical` does not climb past
  one (the names around a module are not its own; the file's imports still are). A call
  written as a path sets `Call::by_path`: its head is read among types and modules alone
  (`lexical_as`, no local or parameter hides it), and a failed resolution matches functions as
  well as methods by name. A pack whose types are closed (`LanguagePack::strict`, Rust) has a
  member its blocks lack on a type the receiver or the path head writes resolve to nothing
  (`Resolver::lacks`: a derive, a blanket or outside trait), not by name, and its attribute
  calls (`Call::decorates`) resolve to nothing, read only through `Code::outside` with the path flag. A pack that has program entry points fills
  `Facts::programs` in `parse` (it sees the path and the source; default: none) with the
  `Def::name` of each definition that starts a program; each becomes an entry point of kind
  `program`, labelled by its qualified name, and the conformance test lists them. Where the
  entry point is top-level code, `parse` yields a definition for that code (the calls it makes
  belong to it) and names it. A name with no symbol is dropped silently: the pack's conformance
  golden catches a wrong one. A form declared outside the code file (Python `[project.scripts]`,
  which names a function of another file) needs a repository-level step feeding
  `entry::Local::programs` with `entry::Program`s: for Python, the seam's `package::programs` has the
  PyPI adapter map the `Summary::scripts` of each `pyproject.toml` (stored with the manifest
  rows, whose digest makes an edit re-extract) to files through the package's source roots. A form only the whole repository tells
  (Rust `fn main`, a program only in a binary target's root, which one file's `parse` cannot
  see) is `Layout::programs`, which `refresh` takes from each pack's `layout` beside
  the seam's `programs`; Rust asks the crate `Tree` for its binary roots (`Tree::bins`). The forms per language are spec #31 "Program entry points per language
  pack": Python `if __name__ == "__main__":` as symbol `__main__` plus each `[project.scripts]`
  target; Go `func main` in package `main`; Java `public static void main(String[])`; Kotlin a
  top-level `fun main`; C# a `static Main` (any signature) and, for top-level statements, a
  symbol `Main` (none when the file declares a global type `Main`, which would share it); Swift
  an `@main` type (its `main()` method when the type declares one, the type otherwise) and
  `main.swift` as symbol `main` (spec #29, #123: the statements of the file, and the
  initializers of its top-level `let`/`var`, are scoped `main`, so their calls belong to it;
  the bindings and constants themselves stay globals of the module); Rust `fn main` in `src/main.rs` and
  `src/bin/`. TS/JS declares none.
  A pack whose files bind names another file writes (C#'s `global using` directives, and the
  `<Using>` items and implicit usings of a project file, which apply to every file of the
  project) gives `LanguagePack::join`: on every refresh, before `layout`, the index hands it the
  pack's config files and the facts of all its files as parsed, and it gives back a `Join`, what
  to do to one file's facts, which the index applies to each file and to each file the
  contracts parse again for their values (the C# pack appends each project's usings to the
  `imports` of each of its files, from `Facts::project` and `dotnet::msbuild`, reads an alias as
  a type's when a class of the repository has its target's name, and sets `Facts::unit` to the
  project). The index keeps the facts as parsed and joins them again each time, so a file whose
  own text did not change resolves as a cold index resolves it; `resolved` then rewrites its
  rows. A type a pack declares in parts (`Facts::partial`, C#'s `partial`) in several files of
  one `Facts::unit` is one type (`Resolver::parts`): a member lookup (`member`, `overloaded`,
  `inherited_field`) reads every part after each class of a family (`with_parts`), the family
  linearizes the bases every part writes (`bases`), a name the parts' files export binds the
  asking file's own part, else the first by path (`exports`), several parts a name binds
  construct the first (`one_type`), and a constructor call takes the constructors of every part.
  Classes of one name that are not parts of one type stay several, `ambiguous`, but for a Kotlin
  `expect` class and `actual` ones of its name (`Facts::expect`, `Facts::actual`), which
  `one_type` makes the `expect` one wherever the name binds them all (`exports`, a constructor
  call): a base or a receiver so named reads the `expect` class's family, and a member only an
  `actual` declares is a name match (`Resolver::matched` reads the `actual` classes of each
  `expect` one of the family, `Resolver::actuals`, and `lacks` holds no `expect` class closed).
  Where globs combine (`unordered_globs`), an `inside` glob
  that gives the name comes before the other globs of its scope, and the `inside` ones combine
  with each other (Kotlin's written star imports, which see `internal` members), but for the
  file's own package's (`Resolver::own_glob`), which stops the reading: a name two Kotlin star
  imports give is `ambiguous`. A glob of a
  namespace (`wildcard`'s `Imported::Namespace` source, `Resolver::glob_namespace`) takes the
  classes declared in that namespace alone (`Binding::of_namespace`), since a C# file's group
  is its first namespace's and it may declare types in another; the C# layout names such a file
  for each other namespace it declares types in, when no file has that namespace as package.
  A pack whose type names carry no namespace turns that check off (`LanguagePack::namespaced`,
  false for Swift, a module being no part of its types' names, and for Rust, whose type names
  are paths in their file, so a `use super::*` takes the parent's types as it takes its
  functions).
  The Swift pack (spec #29, #117; `lang/swift.rs`) reads `Package.swift` with the same grammar
  (`lang/swift/manifest.rs`: every `.target`, `.executableTarget`, `.testTarget`, `.macro` and
  `.plugin` call with a literal `name:` outside a `dependencies:` argument, under its literal
  `path:` inside the package or the default directory) as a config file (`**/Package.swift`,
  no code: `is_code`). Each target is a module of its package (`Targets`: the package's directory
  and the target's name, which is the file's `Facts::unit`, so two packages that name a target
  alike give two modules) of the files under its directory, the deepest
  one when several hold a file (looked up by the file's directories, so the cost is the path's
  depth), and the files no target holds (all of them without a manifest) are one module no
  import names. The layout shares a module's scope among its files (`Layout::shared`), whose
  top-level overload families are one family across its files (`Group::families`,
  `Resolver::family_at`), and names it by its first file; `join` sets each file's
  `Facts::unit` to its module, so `internal` declarations (`Facts::private`: C#'s non-public types, and constructors and
  methods hidden outside their project) stay in it
  (`Resolver::hidden`), and appends the module's own inside glob after the imports, so its
  names come before an import's. `import M` is a namespace import of `M` and an outside glob
  of it, which takes `M`'s public names (`Facts::star`); `M` is the target of that name in the
  importer's own package (the deepest around it), else in the nearest package by steps through
  the directory tree, every one when several are as near (`Targets::named_from`). An import written
  `@testable` is also listed in `Facts::testable`; `join` sets `Facts::internals` to the modules
  it names, whose `internal` declarations the file sees (`Resolver::hidden`, and the glob reads
  the module's exports, `Resolver::sees_internals`), but for the `fileprivate` and `private`
  ones (`Facts::confined`). A function's name carries its
  argument labels (`place(order:quantity:)`, `init(baseURL:)`, `_` for none); declarations of
  one scope in a file (a type's body and its extensions alike) with the same labels and other
  types carry each label's written type too (`audit(_:Order)`): `parse` walks the file twice,
  the first time to find them. Each `Param::label` and each `Call::labels` (`_` for an
  unlabelled argument, empty for a trailing closure) is kept with the facts, and `Arity`
  narrows an overload family by them (`labelled`: in order, past parameters with a default
  value, a variadic one taking the unlabelled arguments after its first, a trailing closure any
  label of a parameter that takes one), so a tie is `ambiguous` with each overload. Each
  `Param::closure` is read from the written type: a function type (through `?`, parentheses and
  attributes such as `@escaping`), a type parameter or `Any` takes a trailing closure, a name is
  `Closure::Named` until `join` looks it up among the repository's aliases of a function type
  (`Facts::function_aliases`, and aliases of those to `ALIAS_DEPTH`, by last segment in any scope), and
  any other type refuses it. A `typealias` of a named type is an `Imported::Path` import of its
  target under the alias's qualified name, exported by its access as a declaration is: a type
  written as the alias resolves through `Resolver::alias` (a cycle or `LINK_DEPTH` ends with
  nothing). An extension is no definition but a
  `Facts::impls` block of the extended type, one more per protocol it writes (`Impl::of`).
  `Resolver::overloaded` reads one level per class of the family, the class with its blocks in
  any file (two declarations of one signature there are two overloads, but for a protocol a
  block's member of a requirement's signature is its default body), then one level per
  protocol a block conforms to, with that protocol's inherent blocks; a signature an earlier
  level declares overrides a later one. A protocol from outside the repository a block conforms
  to makes the family opaque (`Resolver::opaque`, which `inherited` and `matched` read as an
  opaque base) for the names it may declare (`Block::outside`, `Resolver::may_declare`): every name,
  unless the Swift table of known outside members lists it (`Hashable`, `Codable`,
  `CustomStringConvertible`, the raw value types, Vapor's `RouteCollection`), whose entries are
  compared without their labels. The same table weighs a written base (`: NSObject` stays
  opaque, `: String, CaseIterable` does not). A protocol extension's member of the requirement's signature in the
  requirement's file is that one definition. A capitalized callee (`Order(id:)`,
  `Order.Line()`) constructs (`Call::constructs`): the type's initializers in its body and its
  blocks (`Facts::constructors`, also the family `Type.init` for `self.init(..)`), without
  those its module keeps to itself when the call is in another (`Resolver::accessible`), or the
  type itself when it declares none (a memberwise initializer). The case is a guess
  (`LanguagePack::guesses_constructs`): a capitalized callee that binds a function instead of a
  type is a plain call of it (`Check(1)`), and `await` decides an `async` target (`Call::awaited`
  from the `await_expression` or `async let` around the call, up to its function or closure;
  `Def::suspends` from the `async` of the declaration). Bare calls are Kotlin's
  (`bare_calls`).
  The values the contract kinds read (spec #29, #119; `lang/swift/values.rs`) are written for
  each call: its receiver (`a.b` of `a.b.f()`), its arguments (a label makes a keyword
  argument, `get("users", use: list)`, a trailing closure the last positional one), a string as
  its parts (an interpolated name a `Part::Ref`, any other expression or an escape a hole), a
  name or a member chain as `Value::Ref`, an implicit member as `Value::Member` with no type
  (`.post`), a postfix `!` as its target, `a + b` as a string of both, an array as a list and a
  call as `Value::Call` (its labelled arguments in `Invocation::kwargs`, spec #29, #121), read to
  `VALUE_DEPTH` (32).
  `ProcessInfo.processInfo.environment["X"]` (spec #29, #122) is `Str([Env(X)])` (`subscript_read`),
  through a postfix `!`, parentheses, `trimmingCharacters`, `replacingOccurrences`, `lowercased`
  and `uppercased` (an argument that may read again, a read, a hole or a name of the code but not
  a type's member such as `CharacterSet.whitespaces`, makes a hole), and a `??` whose fallback is a
  text-only literal (any other fallback is a `Hole`: which value is the base is unknown); it
  is also the one value an interpolation may hold besides a name. The pack decides by the
  spelling alone, as Go's and Rust's do; a repository type named `ProcessInfo` is not looked for. Vapor's
  `Environment.get` is no read; an `if let` or `guard let` of one read is (see
  `Facts::bound`). `http` takes the leading `Env` as the
  `base_url_env` variable, as for every language. A closure written in code (not in a
  type's property initializer) is a `Kind::Inline` definition named `<scope>.@<offset>`, and the
  calls and names inside it belong to that scope. `Facts::bound` holds a `let` or `var` of one name given
  a call, a name or a string (#121), and an `if let` or `guard let` of one name given one
  environment read (#388), and drops the name when another binding of the
  function (a loop variable, an `if let`, a `guard let`, a `catch`, a `case let`, a `var` or
  another `let` in a block) also declares it, when its function (or a closure in it) assigns it
  (`x = ..`, `&x`) or calls a method on a `var` of the name that holds a string, a name or a URL
  (`url.appendPathComponent(..)` changes it in place); a global `var` is not bound, since
  another file of the module may assign it; a top-level or type-level `let` is a `Const` (qualified by its type) holding its
  initializer, and a binding in a block of the top level is a local, not a global. A member
  assigned through a local that holds a call (`request.httpMethod = "POST"`) is a `<member>=`
  keyword argument of that call, when it is a plain `=` in the function's own statements or in a
  `do` block that is one (not in an `if`, a loop, a `catch` or a closure); written twice, or
  where it may not run, it is `Value::Unknown`.
  `Def::return_values` holds each `return` and the one expression of a function that writes a
  result type. `Code::arg` matches the labels of a call to the parameters' with the matcher
  `labelled` decides the fit with (`labelled_args`, one table of which parameters take which
  arguments): an unlabelled argument takes a parameter written `_`, a labelled one a parameter
  of its label, a parameter that may be left out gives its label to a later one when the call
  fits no other way, and the first trailing closure takes the first parameter that takes one
  (`Param::closure`) and cannot be left out; a trailing closure after the first (`f { } other: { }`) is not bound.
  Method calls on a receiver (spec #29, #118) use the seams Java's and C#'s do, with no new
  one. The pack writes `Facts::receivers` from the written type of a stored property, a
  parameter (scope: the function's labelled name) and a `let` or `var` of code or the top level
  (`Class::Type`; `T?`, `any P` and `some P` are `T` and `P`; an array, tuple, function type,
  variadic or type parameter is no class). A function's own type parameters are read while
  parsing; those of a type and the associated types of a protocol are `Def::type_params`, and
  `join` unsets the class of a receiver scoped in the type or in an extension of it, in any
  file, whose type is named alike; a call `T()` of such a name constructs nothing
  (`Call::constructs` unset, here and in `parse`). A `let` or `var` of code is a receiver only directly in the
  body of a function, method or initializer (or at the top level of a file): in a nested
  block, a closure, an accessor, a deinitializer or a subscript it only hides its name
  (`Facts::locals`), since its scope is the function's, the type's or the file's and the name
  would reach code outside it. Without a written type, the class is the one an initializer
  constructs (`Type(..)`, through `try` and `await`: `Class::Named`). A name bound twice in a scope with two classes, or also
  by a binding with no written class (`Facts::locals`: an `if let` or `guard let`, a `for` or
  `case` pattern, or a closure parameter, whose scope is the closure's own), is unknown for the
  whole scope. `member` walks the
  family and its blocks upward only, so a protocol-typed receiver stops at the requirement, and
  the labels narrow as they do for a bare call. A receiver of unread type (a call result, a
  closure parameter, an element) is the name match `matched` makes by base name and labels. A
  type from outside the repository (Opaque outside, `LanguagePack::outside_blocks`): a receiver
  whose written or constructed type nothing in the repository binds (`session: URLSession`, a
  standard-library type too), or a capitalized name nothing binds (`AF.request`), matches the
  members of that type's extensions in the repository alone (`Resolver::outside_type`), and a
  call past a member read on such a name (`URLSession.shared.data(..)`) resolves to nothing. A
  local or property whose value is such a member read holds the read as written
  (`Class::Member`, through `try` and `await`; not a force unwrap, an optional chain or a
  subscript), and `outside_type` reads its head: outside the repository, the receiver is opaque
  (`let session = URLSession.shared`); a head the repository declares leaves the call to the
  name match. Ceiling: a `var` assigned again (`var client = URLSession.shared; client = other`)
  holds no one value and keeps the name match.
  C# method calls (spec #28, #108) use the seams Java's do and add none but one flag. The pack
  writes `Facts::receivers` from the declared type of a field, a property, a parameter
  (primary constructors' too), a local with an explicit type, a `foreach`, `out` or pattern
  variable, and a `var` initialized with `new T(..)` (`Class::Type`), or with a call
  (`Class::Result`); a target-typed `new()` takes the declared type. `Def::result` is a
  method's or local function's written return type, so `Make().Save()`, `Make()?.Save()` and
  `var x = Make()` follow it (`Call::on`). A type is read as written, without its type
  arguments or a nullable mark; a predefined type, an array, a tuple, `var` and a type
  parameter are no class. A name bound twice in a scope with two classes, or by a local named
  like a field of the file, is unknown. A class's written bases (`Def::bases`) serve
  unqualified, `this.` and receiver calls, an interface receiver reaches the interface's
  member alone, and overloads are narrowed by `Call::arity`, a tie `ambiguous`. The `this`
  parameter of an extension method is `Binds::Instance`: a call made on a receiver counts one
  argument fewer than `Ext.F(x)` does (`Arity::receiver`, `Resolver::on_receiver`), so
  `AddOrders(this S)` and `AddOrders(this S, Action<O>)` tell `s.AddOrders(f)` from
  `Ext.AddOrders(s)`, and `Code::arg` binds the receiver to that parameter.
  `LanguagePack::extension_methods` (C# only) lets an extension method answer a call no member
  does: on a receiver of a class of the repository whose family lacks the method and has no
  opaque base (a base among the C# known outside members, `IDisposable` or `IComparable`, is
  opaque only for the names it declares, as Java's), or whose written type is outside the
  repository (`List<T>`), the name match
  keeps the methods with a `Binds::Instance` first parameter alone (`inferred`, or `ambiguous`
  with every one that fits). With an opaque base, or on a type's name (`Teller.Missing()`),
  or a bare call, the call resolves to nothing, and so does a method of the repository that
  is no extension. A receiver without a written type (`var x = Make()` of unknown result, an
  implicitly typed lambda parameter, a type parameter) keeps the broad name match; so does a
  lambda parameter written with `in` or `out` (#379), which tree-sitter-c-sharp 0.23.5 does not
  parse (an `ERROR` node). The extensions are not
  looked up: an extension method called as `x.M()` is never `exact`, one called as `Ext.M(x)`
  is. A local a call from outside initialized (`var app = builder.Build()`, `var b =
  WebApplication.CreateBuilder(args)`) is from outside too (`Resolver::outside_result`): the
  call resolves to nothing of the repository and is made on a name that binds nothing of the
  repository, on a receiver of a written outside type or on such a local, so a call on it
  takes extension methods alone (`app.Run()` reaches no repository `Run()`). The callee keeps
  no type argument, so a local of `GetRequiredService<OrderService>()` or
  `Deserialize<Order>(..)` is from outside too: its calls lose their name match. Ceilings: no generic type argument is read (`Result` and `Result<T>` are one type), a
  `var` takes a type from `new` and a call alone (no cast, `await` or conditional), a
  target-typed `new()` records no constructor call, and a parameter type written `global::A`
  or `alias::A` is spelled `A` and `alias.A` in a signature.
  The value flow (`Code::origins` with `Follow::Values`, spec #28, #110) follows what C#
  code hands a route builder through. A call made on a receiver that the name match gives one
  extension method (`inferred`) is taken as the language binds it when the static class
  declaring the method is in scope of the call by its name (its namespace is the call's,
  one around it, or a `using`'s): `Resolver::extension_callers` holds those calls beside the
  exact `callers`, and the flow climbs them to the receiver (the `this` parameter) and into
  what the method returns (`Resolver::flows_to`); wrapper expansion and `Code::param_args`
  still climb exact calls alone. A local the pack bound to one value (`Facts::bound`: every
  local declared in a definition, dropped when the file assigns its name anywhere with `=`,
  a compound operator, `++`, `--`, `out` or `ref`) is followed into that value, for every pack
  that fills `Facts::bound` (Kotlin's `val` and a Java local declared with an initializer, too:
  the Java pack drops it when the file assigns its name anywhere, when another binding of the
  name shares its definition or when a field of an enclosing class has the name). The pack
  fills `Def::return_values` from
  `return x;` and a method's or local function's `=> x`, and makes every lambda written in a
  definition (not in a type's initializer) a `Kind::Inline` scope `@<offset>` with its
  parameters, which a call's argument names (`Value::Function`) and which folds into the
  symbol around it unless a kind claims it; `new[] { .. }`, `new T[] { .. }` and `[..]` are
  lists.
