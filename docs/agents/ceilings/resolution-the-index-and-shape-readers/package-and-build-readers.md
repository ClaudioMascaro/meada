# Package and build readers

- `jvm/spring.rs` (`read_yaml`): the dotted keys a YAML file flattens to are charged to
  `YAML_KEY_BUDGET` (1 MiB); a file past it (a long key over thousands of children) reads as
  empty, so its `${key}` placeholders name environment variables.

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

- `package/pypi.rs` (#831, ADR 0028): the install row of the command table reads six commands
  by their leading words (`pip install`, `pip3 install`, `python -m pip install`,
  `uv pip install`, `pipx install`, `uv tool install`); another spelling (`python3 -m pip`, an
  option before `install`, a prefix) is outside the table. An option of `FLAGS` takes no value
  and any other takes the next word, so a flag missing from the list costs the name after it
  and never gives a wrong one. An argument is a requirement by name: a path, a URL, a direct
  reference, `-e` and the file of `-r` (not followed) give no side, and a wheel or a folder
  written with no slash reads as a name. A name the workspace provides wins over a public
  package of the same name: one provider in another repository is `exact`, whatever index
  serves the install, and one in the CI step's own repository is `inferred`, as for a manifest
  entry with no source. A name with an open part is a pattern within `iac/pattern.rs`'s limits
  (`inferred` to the one package it fits, `ambiguous` to several, no side when it fits none: an
  open part glued to a word, `shared-lib$X`, replaces no whole segment and fits nothing); a
  literal name with an open range or extra, or an open part right after the extras
  (`shared-lib[fast]$X`), is the package, its range as written and `range unchecked`; a
  name left open whole is one unresolved consumer per CI step, with what is written as its
  `via`, a cause and no candidate, and only in a workspace that provides a PyPI package (no
  side elsewhere). The first install of a name by a CI step gives its range, and a range pip
  would refuse (`shared-lib==`) links as written. `context` on a package lists the dependents
  that name it: a pattern that fits it is counted by `search` and shown by `context` on its
  own CI step only. `trace` walks no package link from a CI step: a manifest is no node of
  it.
- `package.rs`, `package/pypi.rs` (#832, ADR 0028): a CI step that runs a console script links
  to the script's function, never to the handler of a subcommand, and only for PyPI
  (`[project.scripts]`, else `[tool.poetry.scripts]`): an npm `bin`, a Go or a Cargo binary is
  not read, nor `gui-scripts` or another entry point group. The command's first word is
  compared byte for byte with the script's name as written, since pip writes the command under
  that name unchanged (a probe with pip 26 and setuptools kept `Acme_Tool`, `acme.dotted` and
  `acme_under`; the specification page was not reachable to cite): a path to the script,
  `python -m <module>`, `uv run`, `poetry run`, a prefix (`sudo`, `VAR=x`, `time`) and a name
  with an open part match nothing. `uvx <package>` and `pipx run <package>` are read only with
  the package as the next word: `--from`, `--spec`, another option first, `<package>@<version>`
  and `uv tool run` give no side, and a range after the package (`uvx 'core-sdk>=1'`) gives
  the package and no program. A script whose function is no program entry point of its
  repository (a module outside the declared roots, a class, generated code) gives no side.
  The evidence for `exact` is an install by a literal name in an earlier command of the same CI
  job, in file order of its CI steps: an install in a later CI step, in another CI job, in a
  called script or a composite action, by a pattern, by `-e .`, by `-r` or by a path is not
  seen, and neither is an `if:` that skips the installing CI step. With no install the name
  alone links `inferred`, so a script of the workspace wins over a shell builtin or a public
  tool of the same name. `MAX_DECLARING` (8): a name more packages of the workspace declare
  gives no side at all. The link is at the weaker of the evidence's level and the package
  link's, and its range is the install's. `context` on the program's symbol lists no CI step
  (`callers` are calls in code): `trace` up from it does.
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

- `lang/mod.rs`: `MAX_BYTES` (1 MiB, beside the packs that read their configs by it) bounds every
  manifest and pack config the package readers and the packs take; a larger file reads as empty,
  and the index reads it up to one byte past the limit (`read_up_to`).

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
