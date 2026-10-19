# Changelog

All notable changes to Meada are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

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

- The default model of the `decisions` decider is now `typesafe/jev-1.14` (was `typesafe/jev-1.13`);
  `MEADA_DECIDER_MODEL` still overrides it. The request and answer shapes are unchanged.

### Fixed

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
