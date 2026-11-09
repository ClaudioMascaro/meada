# Detail: Go, resolution and JVM builds

- `lang/go.rs` (spec #25) yields functions (`Kind::Function`), methods named by their
  receiver's type (`Type.Method`, `*T` and `T[K]` alike, `Kind::Method`), every named type
  and alias (`Kind::Class`) and interface methods (`Interface.Method`). A function literal is
  a `Kind::Inline` definition (`@<offset>`, which the index names or folds into the one around
  it, so its calls belong to the enclosing symbol), its parameters bound in it; so is a blank
  `func _()`, which folds into the file. A second `init` of a file is `init#2` (and so on).
  Top-level `const` and `var` names are `Facts::consts` (no value yet); names bound inside a
  function (`:=`, `var`, `const` and `type` in a block, `range`, `select` receives, type switch
  bindings, named results, type parameters, a method's receiver and the type parameters its
  type arguments name) are its locals, unless every binding of the name in the function writes
  one class (`Walk::finish`): then it is a `Receiver` (`Class::Type` for a parameter, the method
  receiver, a named result, a `var` with a type, a composite literal; `Class::Called` for a
  call, whose function's `Def::result` is its first result type). A struct's named fields are
  receivers in the struct's scope. Exports are the top-level names that start with an
  upper-case letter. Imports: plain (`assumed_name`) and aliased ones are
  `Imported::Namespace`, a dot import `Imported::All`, a blank one nothing; a name two imports
  give is imported from the empty specifier, which names nothing (`unbind_clashes`). `parse`
  evaluates the build constraints (`lang/go/build.rs`: file name suffixes and the
  `//go:build` line of the header, read as the Go tools read it: before the first line of code,
  outside block comments) for `linux/amd64` and marks a file they exclude `Facts::unbuilt`,
  which `layout` leaves out of `members`. `layout` reads the `go.mod` files once
  (`lang/go/modfile.rs`: `module` and `replace` directives, a line format read with no parser
  crate) and answers `modules` with the first non-test file of the directory an import names,
  which stands for the package (`Modules::dir`: a `replace` of the importing file's module,
  none when it is by a module from outside, else the longest module path of the repository, a
  directory its module owns), and `shared` with the files of each directory and
  `Facts::package`. A stable first binding of a multiple-result call is not a scalar
  constant. `Facts::first_results` retains its scope/name and exact call range only in full
  facts/value caches, after the same single-binding and assignment guards. The shared SDK
  walk asks `Code::first_result`, which checks the nearest lexical binding before following
  that exact call; ordinary value folding does not use this evidence.
- `resolve.rs`, `resolve/mutations.rs`: shared resolution and scoped selector effects behind the adapter. `Resolver` connects calls to definitions
  (scope, imports, re-exports, `this.method`, member chains) and gives each a `Resolution`. A name may be
  bound by several imports (the latest in source order decides, whatever it resolves to), a
  callee may start with a dotted import, and an `Imported::All` import counts as one binding
  of each name its module's `Facts::star` takes. `lexical` skips a class's scope unless the
  lookup starts in it (a class body's names are not in scope for its methods). A member of a
  class, or of a parameter or local whose `Class` resolves to a class of the repository
  (`Binding::Instance`), is looked up through the class's family: the class and the ancestors
  its written bases give, in C3 order (Python's method resolution order), linearized once per
  class in `Resolver::new`. A base that is not a class of the repository is an opaque
  ancestor, named by what the file imports for it (`abc.ABC`) or as written (`object`), so
  classes that share it give C3 one entry; a base that is no name (`mixin()`) is its own
  entry: reaching it before a class that has the member leaves the member unknown, and so
  does a hierarchy C3 rejects, bases in a cycle or a family longer than `FAMILY_DEPTH` (cut
  without being kept on the way up when the walk down the bases reaches that depth, `Cut`). The first class that defines the member, or
  binds it as an attribute (a `Receiver` of the class), wins, `exact`. A receiver whose class (or whose value's
  callee) is outside the repository is unresolved; any other member call falls back to the
  name match. `Busy` guards locals and families that resolve through themselves. A single
  module-level binding is read from functions as it is; a function that rebinds it through
  `global` makes it unknown. A pack that writes `Def::bases` and `Facts::receivers` gets this
  without resolver changes (TypeScript writes none yet; its `this` is the pack's `receiver`
  keyword). A class's blocks (`Facts::impls`: Rust `impl`s, in any file, their type and trait
  resolved from `Impl::within` once in `Resolver::new` into `impls` and `blocks`) add members:
  `member` tries `block_member` first (the inherent blocks' `scope.m`, else the trait blocks'
  `scope.m` or their traits' default `Trait.m`; several at that step are `Binding::Several`,
  which `call` answers as `Ambiguous` with exactly them), then the class's own `Class.m`, and
  `class` (the `receiver` keyword, `Self`) answers the block's type inside a block. `instance`
  reads `Class::Returned` through `returned`: the callee resolved as a call (a path with
  `path_binding`), its definition's `Def::result` resolved from the function's own scope;
  `Class::Called` (Go) reads the callee as a value expression instead, so `h.Counter()` follows
  the receiver `h` to its class's method (`returned(.., value)`). A
  `Kind::Module` definition is a member chain like a function (`inner.helper`); one an import
  of its own name binds to a file (`mod store;`) has that file's exports as members; `lexical`
  stops climbing at one, so a module written in place does not see the names around it (only
  the file's imports). `call` reads a `Call::by_path` head with `lexical_as(types)`: no local,
  parameter or receiver hides it. Name inference keys a function or class of modules written
  in place by its last name, like a top-level one, and a call with `Call::by_path` matches
  functions as well as methods by that name; `infer` keeps the candidates the calling file
  reaches (`Resolver::reaching`, from `Layout::reaches`), so a Rust call never matches a crate
  it does not depend on.
  `Code`
  folds constants and client wrapper arguments into values for contract kinds, a destructured
  object parameter reading its names (`Param::fields`) off the literal object the caller passes.
  A call to a function that only returns a string (`Def::lone_string`, filled by the TS/JS pack for an
  expression body or one lone `return`) folds to it with the call's arguments (`Code::built`, #536):
  `exact` calls alone, every part known or the call stays as written. The TS/JS pack lists each
  `return` of a function (and an arrow's expression body) in `Def::return_values` (#615), so the
  value flow follows a router or client a factory returns; a `return` of a function that is no
  definition (one returned in place) is nobody's.
  `Code::expand` returns, for each site, the wrapper calls it
  climbed (`Expanded::frames`, `Frame {def, path, index}`, innermost first), and `Code::uses(path, index)` reads
  `Call::uses` like `args` (a file without full facts is a miss). An argument binds a parameter as its `Param::binds` says: by position or by
  the parameter's name among the keyword arguments, only by name, or not at all (`Rest`); a
  method called on an instance (`x.m(a)`) passes nothing for an `Instance` receiver, one called
  on its class (`C.m(obj, a)`) passes it first, and a `Class` receiver is never passed.
  `Code::expand(Expand::Call(path, index), read)` hands
  `read` the call's positional and keyword arguments, folded, and walks the wrappers' callers
  with them; a call found only by a name match (`data.get(...)` matched to a wrapper method,
  or a bare name no import binds; `Resolver::callers` keeps `exact` calls alone) is not climbed, since it may be on any
  object or reach another function, so a wrapper with no other caller is read where it stands. `Code::origins(path, scope, of, follow)` is
  the one origin question: what made the value of a name or a value as written (the call of a
  library's constructor or factory, or what the source declares a name is), with the methods
  called on the way, and every route the walk took to it. The evidence policy picks the walk, and
  each reader asks the one it needs. `Follow::Values` (Python, Rust, Go's routers and request
  clients, Express) is the shared value flow: the call sites whose result a name may hold,
  followed through constants, imports and module attributes, the arguments every caller passes to
  a parameter (a caller found only by a name match is not climbed, as in `Code::expand`), the
  values a function of the repository returns (its parameters then bound by that call only) and
  the items of a list or tuple; an attribute assigned a call once in its class
  (`self.client = Client()`) counts as a constant; a call that reaches no function of the
  repository is the origin. A constant, parameter or function met again on one path (a cycle) adds
  nothing. `Follow::Constants` (the TypeScript forms) follows `const` bindings alone (same file
  and imported) down to an import whose module is outside the repository, so a parameter or a
  return value has no origin; the answer carries the form of the call that made the value (`new`, a call or the bare
  name), what it names outside, the methods called on the way and, as `held`, the constant the
  name refers to, which pairs the calls made on the same client. `Follow::Declarations` (Go's
  typed clients and forms) reads what the source declares the name is: a written type, or the
  call it was declared from through the methods called on the way. `Code::outside(path, scope,
  name, by_path)` names what an import from outside the repository binds a name to
  (`fastapi.APIRouter`, `express.default`): the package and name there, the name as the file's
  imports spell it and whether its head is the import itself. A pack with `unordered_globs()`
  (Java) adds `Outside::candidates` for a name nothing else binds: one spelling per top-level
  on-demand import from outside (`import a.b.*;`, `import static a.B.*;`, the implicit
  `java.lang`), sorted (`Resolver::on_demand`, which asks `through_glob`, the per-glob step of
  `globs`, whether any repository glob may give the name). Compiling code has at most one real
  candidate, so a kind accepts the name when exactly one candidate is in its table; a repository
  type, an ambiguity among globs or a cut link leaves none. Contract kinds recognize framework
  constructors with it; with `by_path` the name is a path (Rust's `a::b::f()`) whose head is read
  among types and modules. `Code::expand(Expand::Constructed { held, at }, read)` is the
  expansion over the arguments of the `new` a `held` constant holds, on behalf of the call `at`
  made on it. `Code::pack()` is the pack of the files.
  `Code::definition` resolves a name or a member chain (`views.index`) to a definition,
  `Code::module` a name to the file it binds as a whole module, and `Code::import_file` an
  import specifier (`shop.orders.urls`) to its file.
  `Code::imported(path, scope, chain)` reads the head alone, without `const`s: the specifier and
  spelled chain when the chain's head is itself an import from outside the repository (what `Packages::extract` stores). `Resolver::exported()` is
  what each file exports, as `export` reads it, for the `exports` table.

- JVM build files (spec #26, ticket #93) join the existing `manifests` pass through the package
  ecosystem seam: Maven POMs and Gradle settings/build files, including nested modules. Their
  stored summaries contain coordinate fragments and the literal metadata needed to join a
  Gradle build to its settings, never source text. `package::inherit` runs that join after all
  summaries are loaded; changing any build/settings file refreshes the same workspace links.
  The summary also keeps the source roots (`roots`, with the test ones in `test_roots`), so an
  edit to a Gradle `sourceSets` directory changes which provider classes an import reaches.


`Code::constant_value` also recognizes a once-assigned class attribute, as the value-origin
walk does. `Code::param_values` supplies complete exact-caller arguments or a selected call
frame, including a declared default for an omitted argument; a missing argument stays unknown.
Strict constructor readers use this seam without changing the legacy `param_args` policy.

`Code::coroutine` exposes a definition's coroutine fact to strict readers. Python factory
frames retain complete constructor evidence for ordinary and awaited parameter values,
keyed by the function and those values, so shared parameter-return lattices are not expanded
once per route. Each receiver question has its own memo and existing evaluation bounds.
