# Language packs

- `lang/kotlin.rs`, `resolve.rs` (#455): only `private` constructors are enforced (`Facts::sealed`,
  primary or secondary: callable from the class's own scope, its companion and nested classes
  included). `internal` is not enforced: an `internal` class or constructor is visible
  repository-wide, since the Kotlin pack has no module unit (`Facts::unit`: Gradle module or
  source set), so `Foo()` of an `internal` class of another module can still resolve. A class
  with a `private` primary and public secondary constructors keeps the secondaries as
  candidates.
  A call `C(..)` from outside that Kotlin answers with the companion's `operator fun invoke`
  (turbine's `TurbineAssertionError`, whose primary is `private`) resolves to nothing, not to
  `Companion.invoke`: the resolver reads no `invoke` operator.
- Rust globs: a name two `use m::*;` globs give is `ambiguous` between them (#439). Meada does not
  evaluate `cfg`, so `#[cfg(a)] use x::*; #[cfg(not(a))] use y::*;` reads as two globs and the
  call stays `ambiguous` though only one compiles.
- `lang/ts.rs` (#457): a call to `send` inside `exports.send = function () {}` resolves to
  `exports.send` itself, not to a top-level `function send`: the resolver's scope walk reads the
  symbol `exports.send` as `send` in the scope `exports`, as it does `use()` inside
  `app.use = function use() {}`.
- `lang/kotlin.rs`, `lang/swift.rs` (review of #507): two `*_in_linear_time` tests do not catch
  a scan per item. `many_properties_and_locals_are_read_in_linear_time` (5,000 of each) and
  `many_names_and_many_members_are_assigned_in_linear_time` (12,000 names, 24,000 members) both
  passed with their lookup turned into a scan (1.5 s together, bounds 15 s and 2 s): their parse
  or setup outweighs the quadratic part at these sizes. Counting the lookups, as
  `dotnet/references.rs` counts its reads, would catch it; a bigger input would not stay under a
  twentieth of a usable bound.
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
- `lang/swift/values.rs`: `VALUE_DEPTH` (32): a value nested deeper (parentheses, a call chain,
  an argument inside an argument) is `Value::Unknown`, so a route written under such a chain is
  served nowhere. The pack's integration tests exceed it.
- `lang/swift.rs`: `DEFINITION_DEPTH` (128) and `PATH_DEPTH` (256) bound the definitions
  around a definition and the segments of a callee (past them a body keeps its enclosing
  symbol and a callee is `(navigation_expression)`); `QUALIFICATION_BYTES` (4 MiB) bounds the
  enclosing-name bytes a file's definitions, calls and bindings store, past which the file
  keeps no facts and `Facts::budget`, which `meada status` names; `ALIAS_DEPTH` (16) bounds the aliases of aliases of a function type `join`
  follows (`function_aliases`), past which a parameter written as one is `Closure::Unknown`.
  The pack's unit tests exceed each. Beyond the limits, what the pack leaves out
  (spec #29, #117): a target's `sources:` and `exclude:`, a closure,
  deinitializer, subscript or accessor (their calls and locals are the enclosing symbol's, so
  an accessor's local hides its name in the whole type), an operator
  function, enum cases, `Self` as a type's name, a local typed by a call result or a closure
  parameter's type (#118), a typed local of a nested block, closure or accessor (a name match
  where Java and C# keep it function-wide), a `typealias` as a symbol, one written in a type
  seen from another file (its extensions included), an outside alias of a function type
  (`Ext.Handler`: a parameter written as an outside name is `Closure::Unknown`, so a call that
  fits an overload only through its trailing closure is `inferred` or `ambiguous`, never `exact`,
  #456; a name the repository declares as a type or a standard value type, `VALUE_TYPES`,
  refuses it, and a `typealias` of a type of the repository is read as an outside name), and the
  protocols a conformed protocol inherits when the conformance is
  written on an extension. As in Java, an outside base the pack does not list among its known outside members (`NSObject`, `View`) keeps a type's own
  overloads from `exact`, and a listed one (`Codable`, a raw value type) the names it declares.
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
  rest is a hole), and a member chain in a callee is cut to the node kind. Finding the lexical
  scope of a runtime `String` binding uses the same bound; if it is not reached, the file's
  runtime conversion stays opaque. Strict-mode ancestry uses that bound too, falling back
  conservatively to the non-strict enclosing binding. The parent of every node is read once (`Parents`, which
  `lang/ts/uses.rs` and `lang/ts/types.rs` climb too), as `Node::parent` is linear in the
  tree's depth.
- `lang/ts.rs` (`url_form`, `Parents::written`, #685): `new URL(x)` of one argument is `x`, and
  `u.toString()` with no argument, `u.href` and `String(u)` pass the value of `u` through, where
  `u` is a name or a `new URL(..)`, also inside a template string (`${u}` folds through the name).
  They follow the WHATWG URL Standard (the constructor parses its argument, `toString()` is
  `href`), so a URL that `searchParams`, `search` or `hash` changes keeps its path. `new URL`
  stays a call when the file binds or reassigns `URL`, except for the named `URL` export of
  `url` or `node:url` through an import or a destructured, unshadowed `require`. Unknown packages,
  repository aliases, default and namespace imports and other exports stay opaque. The guard
  applies across the whole file, so a local binding can suppress unrelated global uses too.
  Member writes (`u.p = ..`, `u.p += ..`, `u["p"] = ..` for any `p` but `search` and
  `hash`) are tracked per name across the whole file, whichever function writes them: a `const`
  built by `new URL` whose name is written holds an unknown value, and the pass-through forms of
  that name are not read, so two functions that use the same name for different URLs lose both.
  A mutation inside a function the URL is passed to is not seen, so a wrapper called with the
  URL folds to the string it was built from. A parameter, a `catch` binding or a variable named
  `URL` also stops the fold. A shadowed or reassigned `require` stops folding its destructured
  `URL` across the whole file. `new URL(x, base)`
  of the global is a `Value::New` named `GLOBAL_URL` (#686, joined by the HTTP kind, below), so
  `.href`, `toString()` and `String(u)` on a two-argument `new URL` are not read. A `toString` or
  `href` that a repository class defines on a name is read as a pass-through of that name, not
  followed to the method.
- `lang/ts.rs` (`Facts::locals`): a local binding hides the name in its whole enclosing
  definition, whatever block holds it, the definition's own definitions, constants and
  parameters of that name included, and a function that is no definition (a method or arrow of an
  object literal that no top-level `const` binds, an IIFE) lends its
  parameters and locals to the definition around it; at the top level that is the whole file.
  A call to the hidden name elsewhere in that scope loses its exact link or its value, never
  gains a wrong one (no marker).
- `lang/ts.rs` (`sealed_field`, `member_writes`, `this_member`, #688): an instance field of a
  `.ts` or `.tsx` class that is `readonly`, `private` or `#x` (not `static` or `abstract`) holds
  a value only when it has exactly one write, its initializer or a plain `this.f = v` that is a
  statement of its class's `constructor` body; that value, written in the constructor, is what
  `this.f` (also inside a template or a `+`) reads in a method. A public or `protected` mutable
  field, a second write of a member of that name anywhere inside the class (`x.f = ..`, `op=`,
  `++`, `this['f']`, a pattern, a computed `this[k]`), a write under a condition, in a nested
  function or outside the constructor, a parameter property, a constructor parameter
  (`this.f = param`: no fold through the callers of `new`) and a JavaScript file stay unknown.
  `this` is the instance only in an instance method or field initializer, through arrow
  functions: in a `function`, a method of an object literal, a `static` member or a static block
  it reads nothing. `THIS_CLIMB` (256, `ponytail:`): a `this` nested deeper than that many nodes
  inside its member reads nothing. Not seen (`ponytail:`): a write through an alias of `this`
  (`const self = this; self.f = v`), `Object.assign(this, ..)`, a write from outside the class
  that casts the type away (`(c as any).f = v`), and a subclass that redeclares a non-private
  `readonly` field (its own initializer then overrides the value the base class's methods read):
  the resolver keeps no list of a class's subclasses.
- `lang/python.rs`: `VALUE_DEPTH` (64): a list, tuple, dict literal, parenthesized expression or
  `await` nested deeper in a value is `Value::Unknown`, so a hostile file cannot overflow the
  stack. The syntax-only terminal-exit check stops at the same depth and records possible
  fallthrough as unknown. `FOLD_DEPTH` (256): a `+` or `or` chain, a chain of string methods or `getenv`
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
- A Go struct field tagged `env:"X"` or `envconfig:"X"` reads `X` only when its struct is declared
  in the same file as the read, and a selector matches it by its field path (`cfg.API.URL` is
  `API.URL`) whatever the variable's type: an untagged field of another struct with the same path
  is read as `X` too, a path two tags read differently is no read, and a named struct type nested
  in another is not followed, nor is an embedded struct's promoted field; a package-level selector
  (`config.URL`) whose tail matches a tagged path reads as `X` too; the key prefixes of the
  libraries (`env-prefix`, `envPrefix`, envconfig's `Process("app")`) are not applied; nesting past
  64 levels is not read (#618).
- pydantic-settings reads (#619, `settings_consts` in `lang/python.rs`): a field of a `BaseSettings` subclass is read as `<env_prefix><FIELD>` uppercased or its literal `alias` / `validation_alias`, with `env_prefix` a literal of `model_config = SettingsConfigDict(...)` or an inner `class Config`. A base class defined in another file, inherited settings, an instance a function returns (`@lru_cache def get_settings(): return Settings()`), `AliasChoices`, `case_sensitive` and `env_nested_delimiter` leave the base unresolved or read the unprefixed name.
