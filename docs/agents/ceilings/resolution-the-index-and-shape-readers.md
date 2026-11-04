# Resolution, the index and shape readers

- `engine.rs` (`Engine::refresh`, #443): the peers' connections are opened once, with the engine, and
  every query refreshes and reads through them. A peer whose index schema changes while the
  process runs (another Meada version rebuilding it) is not rebuilt mid-session: the engine keeps
  the file it opened, and the rebuilt one is read from the next process start. Reopening a peer per
  query is what #443 removed (it split the refresh from the read); a mid-session reopen would
  have to swap the handle `exports` reads, and the ticket left it out.
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
- `resolve.rs` (`Resolver::may_declare`, ADR 0019): a base among a pack's known outside members is
  matched by its spelling, so a same-named type of a wildcard-imported outside package that
  declares an overload of the called name may give a wrong `exact`; a bare call inside the type,
  a call with a trailing closure or one without labels compares a Swift entry by its base name; an extension
  the tables do not list, in scope of a call no own overload fits by type, is missed. A base the
  tables do not list keeps every name opaque.
- `jvm/spring.rs` (`read_yaml`): the dotted keys a YAML file flattens to are charged to
  `YAML_KEY_BUDGET` (1 MiB); a file past it (a long key over thousands of children) reads as
  empty, so its `${key}` placeholders name environment variables.
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
- `engine.rs`: `search` scans every symbol. Kind `duplicate` folds no nested groups (a duplicated
  class and each of its methods are groups of their own) and has no minimum span size (#254).
- `index.rs`: a refresh keeps the rows of a group of packs that resolve together only while no
  file of it changed and nothing every group reads moved (config files and manifests, Meada's
  version, the file list, the exports of every group): a file added or deleted anywhere, or an
  edit that changes what a file exports, extracts and resolves every group again. A group holding
  a changed file is extracted and resolved whole, over the values of every file of it the last
  extraction read (`valued`), since the actix-web and Rocket reader reads every call's values
  (`Web::read`, under one `ITEM_WORK` budget spent in walk order) and every file's mentions
  (`Web::mentioned`). The values cache (ADR 0027) gives those back for every unchanged file: on
  this repository a one-line edit of a Rust file refreshes in 1.4 s (3.4 s without the cache),
  0.1 s of it reading 221 entries back and 23 ms parsing the edited file; the rest is the
  extraction and resolution of the group. Narrowing a group per file needs each kind to say what
  its contracts read (#391, #610).
- `index/values.rs`: the values cache holds one entry per file of `valued` (pruned on every
  extraction), each the file's parse as JSON, values included: on this repository 18.7 MB beside a
  60.8 MB index, 616 entries, the largest 1.6 MB. An entry is keyed by the file's bytes, setting
  and pack and by Meada's build (version, stored format, the executable's path, size and mtime),
  so a rebuilt or moved binary misses every entry once. An entry is at most `ENTRY_BYTES`
  (8 MiB) of JSON, so the cache is at most that per `valued` file: a file whose parse serializes
  past it (a C# read's shared scope is written once per read) is parsed on every refresh. An entry nested past `serde_json`'s depth
  (128) does not deserialize and is parsed again on every refresh, as is anything else wrong with
  the cache (unreadable, locked past the busy timeout, a write that failed): a miss costs a parse,
  never a different index. A changed file is parsed twice per refresh, once for its stored facts
  and once for its values, since the first parse is under the setting `settle` may still change.
  A refresh that changes nothing or keeps every group leaves the cache as it is. A dev build that changes a kind
  without changing the version keeps the rows an older build gave the groups it does not touch.
  An entry point that asks the name of an inline definition of a kept file makes the refresh
  roll back and run again keeping no group, a second full pass.
  An unreadable `contracts.fragments` value is read as
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
  a `replace` to an absolute directory leaves the module outside. A `replace` by another module
  (`via module <path>`) keeps the one provider whose repository identity starts that path (the
  lower-cased path, a `/vN` suffix dropped, the rest naming the `go.mod`'s directory) and no
  other: the replacement's own `go.mod` is not consulted, so a fork that declares another module
  path than the required one is no provider of the required module and the dependency links to
  nothing; the identity must be the start of the path, so a mirror under another host stays out too, and
  two repositories whose identities nest (`host/g` and `host/g/sub`) may both fit one target.
  An index built before #532 keeps `via outside` for such a `go.mod` until the file changes or the
  schema version does (`tests/golden/schema.txt` has no `go.mod` sample), so the new links appear
  after the first edit of that `go.mod` or the next schema change.
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
- `resolve.rs`, `lang/ts.rs` (#536): a call folds to the string its function returns
  (`Code::built`, `Def::lone_string`) only for TypeScript and JavaScript (the other packs leave
  `lone_string` empty), for a `function` declaration or an arrow or function expression with an
  expression body or a body of one lone `return`, not `async` or a generator, whose value is a
  string (a template, a literal or `+`), and for a call that resolves `exact` (a name match may
  reach another function); the parameters bind to the call's arguments, so the fold costs the
  `EVAL_DEPTH` and `EVAL_BUDGET` of the file it reads. Misses, the URL staying an unresolved
  consumer: a `return` after other statements (a local, a guard, a `const`), a returned
  value that is not a string (a call, a conditional, a member), a class method (`lone_string` reads
  functions and arrow or function expressions, never a `method_definition`), and a string with a part that does not
  fold, an environment read (`process.env.API_URL`) or a build-time global included: the builder
  gives up as a whole where a template written in the `fetch` call keeps the environment read and
  links by `base_url_env`, so a base that is a conditional over a build-time global, as
  spring-petclinic-reactjs writes it, stays a separate gap.
- `resolve.rs` (`Code::origins` with `Follow::Constants`): only `const` bindings with a plain name are followed; a client
  held by `let`, a class field, `this.x`, a destructuring or a parameter, or chained without a
  binding (`new Kafka().producer().send`), is not recognized (no marker).
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
  `[project.optional-dependencies]` are not read; a range is checked as PEP 440 says by a
  hand-written subset (`package/pep440.rs`: `==`, `!=`, `<`, `<=`, `>`, `>=`, `~=`, `==X.*` and
  `===`), and a range or a local version it cannot read (a Poetry `^`, `~` or `*`, a `||`, a
  space between clauses, a wildcard past the release (`==1.0a1.*`), a local label after an
  operator that allows none, `~=1`, a version past `u64`) or a range past `MAX_RANGE` (256
  bytes, npm's) is `range unchecked`, never a mismatch; a mismatch removes no candidate (a pin
  can still `exact`-link a versionless copy); environment markers are dropped (no marker). A
  `[tool.uv.sources]` entry is read only for `[project] dependencies`, and only as a table naming
  one of `workspace = true`, `path`, `git`, `url` and `index`: a list of sources (one per
  marker) keeps the name rule; a `workspace = true` source whose root (the nearest
  `pyproject.toml` at or above the consumer with `[tool.uv.workspace]` whose members hold it)
  is not found, or whose workspace holds no single `pyproject.toml` of the name, keeps the name
  rule too; the `members` and `exclude` globs read `**` and `*` only (`?` and `[...]` are
  literal), at most `MAX_GLOBS` (256) of each list and `MAX_GLOB_PARTS` (64) parts of each glob
  (a longer glob fits nothing); an outer root whose globs hold a project an inner root excludes
  takes it as a member (uv may treat it as standalone); a `path` to a file (a wheel or an sdist) names no provider; an index built before
  the uv sources were read keeps its stored entries until the `pyproject.toml` changes or the
  schema version does; a PyPI entry with no uv source and one package of the name in its own
  repository is `inferred` (#535), whatever the source: a Poetry `path` dependency is not read,
  so it is `inferred` too (a package of another repository stays `exact`). A
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
- `index.rs` (`refresh_stamped`): a `package.json` or document that cannot be statted or read
  (it vanished after the walk, or is unreadable) has no row instead of failing the refresh; a
  `[serve] documents` entry in that state is then reported as missing.
- `document/`: a document never fails indexing, so its readers have fixed limits; deeper or
  larger parts are unreadable or open. `mod.rs`: `DOCUMENT_BYTES_MAX` (16 MiB, above the largest public
  single-file descriptions, GitHub's REST API and Stripe's SDK spec at 10 to 15 MB): a larger
  document is unreadable at its first line. `events.rs`, the YAML and JSON event reader:
  `MAX_NESTING` (128 nested collections, as `serde_json` has for JSON, an alias's node counted at
  the depth it lands) and the alias budget (`EXPANSION` times the source plus `SLACK`, no
  marker), which also charges every document of a stream; the count of documents per file is
  config values' (`config_values::DOCUMENTS_MAX`). `schema.rs`: `MAX_DEPTH` (128 levels per shape), `WORK` (64 MiB per document) and
  `BUDGET` (10,000 schemas per shape, no marker). `openapi.rs`: a path item reference behind
  another one that is external is dropped without a status line (no marker).
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
  part. The YAML file goes through the event reader (`document/events.rs`), with its
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
- `config_values.rs` (#639, ADR 0025): `VALUE_MAX` (1 KiB: a value is stored cut at the last
  character boundary within it, marked `(cut)` in the answers, and matched as stored);
  `DOCUMENTS_MAX` (1,000 documents per file: a stream of more is unreadable, since a manifest
  bundle holds tens and past a thousand it is generated output); `STORED_EXPANSION` (the bytes of the
  key paths and values one file stores, 4 times its size plus `STORED_SLACK`, 4 KiB, the hostile
  probes' bound:
  an indented file stores about twice its size, but a minified file of tiny scalars, such as a
  long array of numbers, and long keys nested deep are unreadable). An unreadable file and a
  lockfile are counted in `meada status`, not named. Search kind `value` finds exact and prefix
  matches through the B-trees on the lowercased key path and value, and substrings by scanning
  every row of every repository (no marker on a `const`, a `ponytail:` comment in `hits`): the
  FTS5 trigram table ADR 0025 describes waits for a workspace the scan makes slow. Redaction covers a Kubernetes `Secret` at a document's root or as an item of a
  list's `items`; a `Secret` with no `apiVersion`, or nested anywhere else (a Helm value, a
  Crossplane composition's base), keeps its values (the other secret markers of ADR 0025 belong
  to the IaC formats, #640). The hidden CI paths are read at the repository root only, and only
  their YAML and JSON files.
  Terraform and OpenTofu (#640, `config_values/hcl.rs`): `NESTING_MAX` (64 brackets open inside
  each other in one file, counted by a scan that includes the ones inside strings, so a file
  with many unbalanced brackets in strings can be unreadable; past it the file is unreadable and
  counted). The parser's own limits (`hcl-edit` 0.10's `MAX_OPERATOR_DEPTH`, 128 chained
  operators, and `MAX_NESTING_DEPTH`, 64 levels of nested expressions, templates and blocks) make
  a file past them unreadable, counted by `meada status`; `PARSER_OPERATOR_DEPTH` and `PARSER_NESTING_DEPTH`
  in `hcl.rs`' tests mirror them, and a test feeds a file at and past each. An `expression` or
  `template` is shown on one line, its line breaks written `\n` (`config_values::one_line`).
  The value length and stored budget are the YAML ones. A `.tfvars` value is
  redacted by variable name across the repository (a sensitive name redacts it in every
  var-file, whichever module it goes to), and wholly when a Terraform file at or above its
  directory is unreadable (`config_values::ANY_VARIABLE`); a variable marked sensitive by an
  expression keeps its values, and `sensitive` is read as the literal `true` of a top-level
  `variable` (JSON: the object form of `variable`, not the array form). A `//` comment
  property of a Terraform JSON file is a value, and a block written as an array of one is
  indexed `[0]` there. A number is stored as
  written. A block repeated in one body is indexed from its second occurrence on, so adding a
  second block renames the key path of the first. A `.tf` file a `.tofu` one shadows is neither
  read nor counted. The old value of a variable made sensitive later stays in SQLite's free
  pages until they are reused (`secure_delete` is off).
