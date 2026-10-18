# Changelog

All notable changes to Meada are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

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
  exports it (`def x(): ...`, then `from a import *`) is linked to that module's, as Python
  rebinds it, and no longer to the earlier definition.
- A name two `export *` of one module give differently is no longer linked `exact` to the first
  one's: as in ES modules, the module exports neither. Two that lead to the same definition
  still export it.
- A class whose base is named through a member of a class with a family close to 128 classes
  long (`class Z(K.D)`) no longer resolves its members differently depending on which class was
  read first.

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
