# Package contracts and source imports

- `package.rs` (spec #22): packages as contracts. `Npm::read` (`package/npm.rs`) turns a `package.json` into a
  `Summary {name, version, dependencies: [Entry {field, name, range}], exports, types, module,
  main}` (the four dependency fields, sorted by field then name; `exports` as written, `null`
  counting as absent) and never fails: a file over `lang::MAX_BYTES` (1 MiB), not a JSON
  object or nested past `serde_json`'s depth limit reads as empty, a field of another type or an
  empty string counts as absent, and a name that is empty, over `MAX_NAME` (214), or holds a
  control character, whitespace or `:` is no name (a dependency entry with such a name is dropped).
  The kind `Packages` (`package::npm::<name>`, sides `package` and `dependent`) `extract`s an
  `Import` row (`Role::Import`, never exported) for each call whose callee's head names an import
  of a bare specifier from outside the repository (`Code::imported`: specifier in `via`, the
  chain spelled from the imported name in `method`); `declare` makes a provider at `<repository>::<manifest path>` (`Provider::version` for the
  reason) and a `packages` row `{name, version, repository, directory}` (`.` at the root) for each
  named manifest, private ones too. `Packages::workspace_step`, which `Engine::exports` runs after
  every repository's `declare` (one repository cannot see another's providers), builds the
  `Providers` index of package names to their manifests once and runs `consume`, `import`,
  the dropped export entry points and `shapes` over it (npm's `reaches` is the one place the
  `workspace:` rule lives). `consume(exports, providers)` is the first: for
  each dependency entry whose package some repository of the workspace provides (a `workspace:`
  range needs one in its own repository), it adds a consumer at `<repository>::<manifest
  path>::<field>::<name>` (the range travels in `Consumer::method`, which is `serde(skip)`) and a
  `dependencies` row `{manifest, field, name, range}`. An entry on a package outside the workspace
  appears nowhere. `link` gives one candidate `exact` and several `ambiguous` (one link per
  candidate, `workspace:` keeping the consumer's repository), with the reason `<identifier>
  <range|none> (local <version|none>)`, prefixed `<n> providers fit; ` when ambiguous.
  `reason()` is the one place it is built; it ends with `, range mismatch` or `, range
  unchecked` when the ecosystem's `flag(range, version) -> Option<Flag>` (the trait's; the `consumers:` section
  asks it too, and nobody parses the reason back) says so. npm's `flag` checks with `nodejs-semver` (npm syntax: `^ ~
  x - ||`, prereleases as npm): `Mismatch` when the range does not accept the local version,
  `Unchecked` for a missing range or version, a `workspace:`/`file:`/`link:`/`npm:` range, a git
  or URL source, a tag such as `latest`, a range past `MAX_RANGE`, and any range with a token
  that is not a comparator (the crate parses `foo 1.2.3` as `1.2.3`). The flag is computed at
  query time and stores nothing. `import(exports, providers)` is the second workspace step, after
  `consume`: for each import row whose package the importing file's nearest manifest
  (`nearest`: the deepest directory holding it, named or not) depends on in any field, it
  resolves the call in every provider of that package (a `workspace:` range: its own repository
  only) through the ecosystem's `resolve` (npm's is the trait's default, Node's rule): `ways` gives the ways in, each with the files it names and
  its `How` (`exports` through `exported`, the table-tested lookup of spec #22: exact key, then
  the `*` pattern with the longest prefix, the first of `types`, `import`, `default`, `require`
  at each level, `null` blocking, a key or a target with two `*` not read, the only way in when
  present; without `exports`, for the bare name one way for each of `types`, `module`, `main`
  present, tried in that order (an empty or non-string `types`, `module` or `main` counts as
  absent), or, with none of the three, `index.<ext>` for each pack extension, as Node's default
  does (`path`, never `index` alone or `index/index.<ext>`); for any other subpath, the subpath
  itself, as a path with the trunk's extension and `index` rules, `path`). A file must stay
  inside the package directory (`inside`), be a source file the provider indexes, not a
  `.d.ts`, and have this package's manifest as its nearest (`owned_source`: a file of a nested
  package is not the outer package's); the first way that leads to one, directly or through the
  mapping, wins, so a field that leads nowhere does not stop the search. One that is build output
  maps back to source: `mapped` takes the `tsconfig.json` in the package directory; only when that
  file is absent or invalid (stored as `null`, no `Config`) the repository root's, and a valid
  one without a usable `outDir` maps nothing, with no fallback. `roots` turns its `outDir` and `rootDir` (relative to that config;
  `rootDir` defaults to the package directory) into repository directories, and `sources`
  (the pure, table-tested mapping) swaps the entry's `.d.ts`, `.js`, `.mjs`, `.cjs` or `.jsx` for
  each pack extension in `EXTENSIONS` order under `rootDir`; the first the provider indexes wins,
  `inferred` with `how` `outDir`. When no way finds a file, directly or through the mapping, the
  import is `ambiguous` and `how` names the declared field or rule whose entry could not be
  followed to a source file (the first way tried: `exports`, the first of `types`, `module`,
  `main` present, or `path`): `candidates` gives the
  symbol the chain names in every source file whose nearest manifest is this package's (not a
  nested package's, not one under a `node_modules` segment, not a `.d.ts`) and that exports its
  head (sorted, one hop each; none, nothing). The files by owning manifest are grouped once per
  provider (`Provided::owned`), not once per call. `lookup` then reads the chain's
  head from the `exports` rows (the file's own export of the name decides; else its `export *`
  files, never for `default`, two that give it differently making it ambiguous and so not
  exported, as in ES modules; `seen` cuts cycles) and each further name as a member of that
  definition or a name of a whole file. A `tsconfig.json` has a row in `manifests` too, read
  by `read_tsconfig` (`out_dir`, `root_dir`; `Export::tsconfigs`, `serde(skip)`; `extends` is not
  followed here, though the trunk follows it for `paths`), so an edit relinks like a `package.json` one. `import` skips a specifier no repository provides before
  reading anything else, and builds a provider's tables (`Provided`) the first time an import
  reaches it; `index::export` reads the `import` rows only for a repository with a manifest and
  the `exports` rows only for one with a named manifest. A call resolved in one provider at least
  becomes a
  consumer of the package at its call site (range in `Consumer::method`, where it resolves in
  `Consumer::hops`, both `serde(skip)`), and each resolution an `imports` row `{site, specifier,
  name, level, how, target}`. `link` then gives a call with hops, instead of the manifest, the
  symbol in each provider whose package link it would have, at the weaker of the two levels,
  with the reason `<package reason>; import <how>` (`How::name`: `exports`, `types`, `module`,
  `main`, `path` or `outDir`; on an `ambiguous` link, the field or rule whose entry could not be
  followed to a source file). The hop is derived at query time from stored facts and never stored; an import of a
  package outside the workspace, or that resolves nowhere, appears in no export. A re-export of
  another package inside the provider ends the lookup (no second hop). No lockfile is read and
  no registry queried. Shapes cross the same way (issue #175): `surface(manifests, configs,
  exported, files)` is the public surface, the entry files of each named manifest's bare name
  and `exports` subpaths without a `*` (`Provided::entry`, the part of `resolve` that follows
  the ways in) and every file their `exports` rows name, in turn; `exported_shapes(code,
  surface)` (run by the package kind's `extract`) gives, for the files of the surface, a `Role::Shape` row (`site`
  the file, `method` the name, `via` the `shape::Want`) for each module-level constant or type
  a file exports: `Types::exported` for an interface or alias (`type`), `Zod::exported` for a
  schema (`zod` and `infer`), else `Literals::exported` for an object or array literal
  (`literals`); `index::export` loads them as `Export::shapes` (`ExportedShape`). `shapes(exports,
  providers)`, the third workspace step, fills every provider and consumer shape holding a
  `shape::Import` through `shape::fill`, whose lookup `kept` resolves the import like a call
  (`nearest` manifest of the importing file, its dependency entry, `reaches`, then
  `resolve` with the chain the import names, `known` being the kept shapes of the
  import's view instead of the symbols) and gives the resolution's level (the weaker of two
  providers keeping the same shape); none, `ambiguous`, several names or two providers keeping
  different shapes is unresolved. A consumer left with no field has no shape.
  `Resolver::exported` names constants and types as targets too, which no call reaches since
  they are no symbols.
  Go (spec #25): `package/go.rs` reads a `go.mod` (module path, no version; each `require` an
  entry under `require` with the version as range; a `replace` to a local directory gives its
  entry, or one under `replace` when nothing here requires the module, the Cargo `path` form in
  `via`; a `replace` by anything else marks the entry `outside`, which reaches no provider). A call
  from a `.go` file through an import of a module the nearest `go.mod` requires (a path with no dot included) resolves in the provider
  directory the import path names (Go's `resolve`): the exported name of the one non-test
  source file of that directory (nested modules left out) that defines it, `exact`, `How::ModulePath`.
  Ecosystems (specs #24, #25, #30 and #225): everything above is npm's, one adapter among four.
  Each ecosystem is a module under `package/` behind the `Ecosystem` trait (`Npm`, `Pypi`,
  `Cargo`, `Go`), registered in the list `ECOSYSTEMS`; the shared steps ask the trait and match
  on no ecosystem, and a new one appends to the list (its order is the search order of `of` and
  `of_contract` and the run order of `inherit`). An ecosystem must say which files are its
  manifests (`is_manifest`, by file name; `of` finds the adapter), its `name` (the contract
  prefix `package::<name>::`, `package::npm::` and `package::pypi::` among them, and the export's
  `ecosystem` field, left out for npm), how a manifest reads (`read`, never failing), which
  source files import its packages (`sources`; `of_source` is none for a file no ecosystem
  claims), whether a specifier may name a package (`names_package`) and which dependency entries
  an import may name (`entries`). The rest has a default body: `flag` (unchecked), `reaches`
  (every provider), `owns` (every manifest of the ecosystem), `ways` and `resolve` (Node's
  rule), `resolutions` (the single-entry loop), `shapes` (false), `programs` (none) and
  `inherit` (nothing). The overrides: npm answers `flag` (semver), `reaches` (a `workspace:`
  range keeps the dependent's own repository) and `shapes`; PyPI `flag` (PEP 440), `owns`, `resolutions`,
  `programs`, `reaches` and `inherit`; Cargo `reaches`, `owns`, `ways`, `resolve` and `inherit`; Go `reaches` and
  `resolve`. `Providers` is keyed by ecosystem name and package name, so an npm and a PyPI
  package of one name never link, and `nearest` takes the ecosystem (a file's nearest
  `package.json` and nearest `pyproject.toml` are apart). A file belongs to the package of its
  `owner`: the nearest manifest its ecosystem's `owns` accepts (for PyPI the nearest
  `pyproject.toml`, since a `requirements*.txt` names no package and bounds none; for Cargo the
  nearest `Cargo.toml` with a `[package]`, since a virtual workspace root bounds none either). `package/pypi.rs` reads a
  `pyproject.toml` (through `lang::python::pyproject::Pyproject`, the one parse the pack's
  source roots share: `[project] name`, else `[tool.poetry] name`, `version`, `[project]
  dependencies` as PEP 508 strings, `[tool.poetry.dependencies]` but `python`, the declared
  `roots` and the console `scripts`, `[project.scripts]` else `[tool.poetry.scripts]`, as
  `(name, module, function)`) and a `requirements*.txt` (line by line, comments, option lines, URLs and paths
  skipped), each name normalized as PEP 503 does (`normalize`) and each specifier kept as
  written, extras and markers dropped (`requirement`). A CI step that installs by name is a
  consumer too (#831, ADR 0028): `consume` asks `Ecosystem::installs` for each command of
  `Export::scripts`, and PyPI's row (`INSTALLS`, `FLAGS`, `installed`) gives a name, a pattern
  (`via` `pattern`, fitted by `iac::pattern` and linked `inferred` through
  `Ecosystem::uncertainty`, which now carries the level) or a name left open, an unresolved
  consumer whose cause `Packages::unresolved_causes` gives; `Packages::context_at` shows them
  on the CI step. A CI step that runs a console script starts its program (#832): `consume`
  asks `Ecosystem::runs` for the name a command runs (and the package, for `uvx <package>` and
  `pipx run <package>`), looks it up in `console_scripts` (the `Program`s of
  `Ecosystem::programs` that are program entry points, by ecosystem and name) and adds a
  consumer of the package whose `via` is `run <command as written>` and whose `hops` name the
  program's symbol with the level of the evidence (`installed`, the literal installs of the CI
  job so far, in the order `step_of` gives); `Packages::link` turns each hop into a link at
  the weaker level, as it does for an import. A PyPI range is checked by
  `package/pep440.rs` (`Pypi::flag`, a hand-written subset: a comma-separated set of `==`, `!=`,
  `<`, `<=`, `>`, `>=`, `~=`, `==X.*` and `===`, over PEP 440 versions with epoch, pre-, post-,
  dev-releases and local labels, no crate: the subset is small, and the evaluator stays inside the clean-room rules and the
  dependency policy): `, range mismatch` when the set rejects the local
  version, nothing when it accepts it, `, range unchecked` for a provider without a version, a
  specifier or version it cannot read (a Poetry `^1.2`, `~1.2` or `*`, an unknown operator, a
  local label after `<`, `>=`..., `~=1`, `==1.0a1.*`) and a range past npm's `MAX_RANGE`. As
  `packaging` does, a pre-release or dev-release local version is rejected unless a clause of
  `==`, `>=`, `<=`, `~=` or `===` names one, `==1.0` ignores a local label of the version, and
  `>1.0` takes neither a post-release nor a local version of 1.0, and `===` compares the
  normalized version (`1.0-RC1` is `1.0rc1`). A mismatch removes no
  candidate. A Poetry `git` or `url` source without a `path` is a direct
  reference, no entry, as `name @ url` is, and a requirements line ending in `\` continues on
  the next. A `[project] dependencies` entry takes its `[tool.uv.sources]` source as `via`, as
  Cargo's do (`uv_source`): `path <manifest>` (folded from the manifest's directory), `git
  <identity>`, `outside` for `url`, `index` or an absolute path, and `inherits` for `workspace =
  true`, which `Pypi::inherit` turns into `path <member>`: the root is the nearest
  `pyproject.toml` at or above the consumer whose `Summary::uv_workspace` globs hold it
  (`member`, `glob`), the member its one `pyproject.toml` of the name; with no root or no single
  member the entry keeps no source. `Pypi::reaches` is `sourced`, and `resolutions` skips the
  providers it refuses. `Pypi::unsourced` (#535) lowers the link of an entry with no source to
  `inferred` when its one fit shares the consumer's repository (`Packages::link`; the calls through
  its imports take the weaker level). A call from a `.py` file (`of_source`) resolves through
  PyPI's `resolutions`: the import names a module, so each distribution a PyPI manifest of a
  directory holding the file depends on (`holding`: every `pyproject.toml` and
  `requirements*.txt` above it, the nearest first, whose entry gives the range) is a candidate when a provider exposes the import's top-level module under its roots
  (`roots`: the manifest's directory, then its declared roots or `src`;
  `module` finds `a/b/__init__.py` or `a/b.py`), `exact` with `How::ModuleName`
  (reason `; import module name`) when that module is the normalized name with `-` turned into
  `_`, `inferred` with `How::TopLevel` (`; import top-level module`) otherwise, and `ambiguous`
  for each when several distributions expose it (one consumer per distribution at the call
  site). The chain is followed from the module the specifier names (`import shared_lib` then
  `shared_lib.api.greet()`: `shared_lib`, then `api.greet`; `from shared_lib.api import greet`:
  `shared_lib.api`, then `greet`), and `Provided::member` takes a package's submodule when its
  `__init__.py` exports no such name. `surface` reads npm manifests alone: no shape crosses a
  PyPI package.
  Cargo (spec #30): the `Cargo` adapter names `Cargo.toml`. `package/cargo.rs` reads one through
  the Rust pack's walker (`lang::rust::crates::entries`, also what the module trees use): `name`
  and `version` of `[package]`, `main` the library root as a path of the repository (`crates::lib_root`:
  `[lib] path`, else `src/lib.rs`, folded from the manifest's directory), `lib_name` the `[lib]
  name` when a `use` writes something other than the package name (`crates::lib_name`, also what
  the module trees use), and the entries of `[dependencies]`, `[dev-dependencies]` and
  `[build-dependencies]`, also under `[target.*]`, as `Entry` with `name` the real crate
  (`package =`), `alias` the key when renamed, `range` the version requirement and `via` where it
  comes from: `path <manifest>` (`path` folded lexically from the manifest's directory: a path of
  the repository, or one that starts with `../` when it leaves it), `git <identity>`
  (`repo::normalize_remote`) or none. An entry whose declared source cannot be read (an empty or
  mistyped `path`, `git` or `package`, an absolute `path`, both `path` and `git`) is dropped, not
  read as a version dependency. A `workspace = true` entry is kept as `inherits`,
  `[workspace.dependencies]` as `Summary::workspace`, `version.workspace = true` as
  `inherits_version` and `[workspace.package] version` as `workspace_version`; the seam's `inherit`,
  run by `index::read_manifests` over the whole list, has Cargo fill each from the nearest
  manifest above it, so a refresh equals a cold export. The consumer site names the key
  (`<repository>::<path>::<field>::<key>`) and `Consumer::via` carries `Entry::via`, which
  Cargo's `reaches` checks against the provider: a `path` of the repository only the manifest
  at that path of the same repository, a `path` that starts with `../` only the manifest whose
  location, `Export::root` (set by `Engine::exports`, or `Repository::root` in `link`) joined with
  its path, folds to the same absolute location (lexical, nothing is read outside a repository,
  no symlink is followed; a path landing in no workspace repository links to nothing), `git` only
  the repository of that identity, a version every repository. The range is never evaluated
  (`range unchecked`). A `use` (`Packages::extract` stores the call when `Code::imported` names a
  crate head, `cargo::names_crate`: not `std`, `core` or `alloc`) resolves in `resolutions`
  through the file's nearest named `Cargo.toml`: the head, hyphens as underscores, must equal the
  key of a renamed dependency or, for one that is not renamed, the provider's library name
  (`cargo::lib_head`: `[lib] name`, else the package name), and Cargo's `resolve` then follows
  the chain from the provider's library root through the `exported` rows of the Rust pack (`pub
  mod`, `pub use`; an item behind a private module is not reached), `exact` with `How::CrateName`
  (`; import crate name`) and no `ambiguous` fallback. A crate of the repository itself is
  reached through the module tree instead, so a path or version dependency inside the repository
  links at the manifest only.
