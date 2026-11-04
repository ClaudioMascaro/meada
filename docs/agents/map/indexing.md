# Discovery, index and resolution

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

## Modules in brief

- `workspace.rs`, `config.rs`, `repo.rs`: repositories, peers, `meada.toml` and identities.
  `workspace.rs` also reads the workspace manifest (`Manifest`) and refuses the entries server
  mode cannot serve; an entry's `[repository.serve]`, `[[repository.resource]]` and
  `[[repository.peer]]` reuse `config.rs`'s shapes (the peer without `path`) and are the
  configuration of a mirror with no `meada.toml`. The entry's schema, those shapes and every
  check the text decides (identities included, `entry_problems`) are `meada_core::manifest`'s;
  `mirror` keeps the checks that need the mirror, after them. A mirror's own `meada.toml` wins whole: the
  entry's blocks are ignored, not merged, and `Listed::ignored` names them for `status`.
  The file `--workspace` names is the bootstrap manifest: its `[workspace]` section, read in
  every build, has `source`, `"file"` (the default, and what a file without the section means:
  its own `[[repository]]` entries) or `"store"` (the current workspace version,
  `snapshot/versions.rs`). `source = "store"` without `generation-pointer` is an error naming
  the feature (`Msg::WorkspaceStoreNeedsFeature`), without `[snapshot]` or `[generations]` one
  naming them (`modules::needs`), and beside a `[[repository]]` one naming both
  (`Msg::WorkspaceStoreBeside`); `Manifest::store` records it, with no entries from `read`. An
  `owner` in the bootstrap manifest's own `[[repository]]` entries (`source = "file"`) is an
  error naming the file, the line, `repository.owner` and `source = "store"`
  (`Msg::WorkspaceOwnerStatic`), and an `added_by` likewise (`Msg::WorkspaceAddedByStatic`);
  `meada workspace import` reads the same entries with them allowed.
  `Manifest::read_listed`, which `index`, `status`, `export` without `--generations` and
  `snapshot publish` call, reads the entries of the version `meada workspace pull` left under
  `<index_dir>/workspace/` (`versions::pulled`, from disk, once per read, `Manifest::version`
  its id), its paths relative to the bootstrap manifest's directory, and goes on as for a
  file's entries (the checks that need a mirror run in `mirror`); without one it refuses naming
  `meada workspace pull` (`Msg::WorkspaceNotPulled`). Under `[oidc.github]` (#331) it reads
  `versions::eligible` instead: the pulled version without the identities the exclusions file
  `<index_dir>/workspace/excluded` names (`versions::excluded`: hostile input, at most
  `VERSION_MAX`, its first line a version id equal to the pulled one, then one identity per
  line, no space or control character and no `identity_problem`), refused naming `meada
  github-app roles` when the file is missing (`Msg::ExcludedMissing`), names another version
  (`Msg::ExcludedOther`) or holds a malformed line (`Msg::ExcludedBad`, the file and the line);
  an excluded identity is absent as an entry the workspace does not list (a peer naming it misses
  it). `Manifest::excludes` says whether a manifest's readers do so (`[oidc.github]` with
  `source = "store"`). `meada workspace show --pulled` (`versions::show_pulled`) prints what
  they read, in the `[[repository]]` shape, for the fetch script; without `[oidc.github]` the
  pulled version whole, and with a version argument it is a usage error (clap's
  `conflicts_with`). `[oidc.github]` without `[postgres]` is refused here, at the table's line,
  naming both. `mcp --listen` without `--generations`
  refuses such a manifest naming `--generations` (`Msg::WorkspaceStoreRefused`): a managed
  workspace's nodes serve generations, which take their repositories from the generation.
  `Manifest::index_dir` is the directory every index lives under.
  `index.rs`: the SQLite schema, the freshness pipeline in `refresh` (with `fresh`, its
  read-only twin for `meada snapshot publish`: the same walk, `packs_of` and stamps, and a file
  whose stat moved but whose hash did not is no difference), and
  `export`; `open` and `state` take the index's path, `<root>/.meada/index.db` locally (`path`,
  with a `.gitignore` of `*`) or `<index_dir>/<identity>/index.db` in server mode (no
  `.gitignore`). A rebuild (a missing index, or one of another schema) is a rename under the
  rebuild lock: `open` builds the whole new index in `index.db.new.<pid>` while it holds
  `index.db.lock` and renames it over `index.db`, so a reader sees the old index or the new
  one, never a missing or empty file; `refresh` stays one transaction in place. An operator
  renames an index in (built elsewhere) only while no `meada index` writes that index, one
  writer per index: a refresh's live `index.db-journal` would be taken for the new file's. `lang/`: the
  language adapter and the TS/JS, Python, Rust, Go, Java, Kotlin, C# and Swift packs. `resolve.rs`: calls to definitions (`Resolver`), values
  and receivers (`Code`: `Code::origins`, the one origin question, with three evidence policies
  (`Values`, `Constants` for the TypeScript forms, `Declarations` for Go's typed clients and
  forms), `Code::outside` for what an import from outside binds a name to, `Code::extensions`
  for the extension functions a Kotlin call has in scope, and `Code::expand`
  for wrappers, `Expand::Chain` reading a call whole). `community.rs`: the deterministic Leiden behind communities (spec
  #31).

## Stored values

The invariant of the architecture map, "The index holds no source text and no values, apart
from ...", with the whole list. A value the index now stores joins it.

- The index holds no source text and no values, apart from the BLAKE3 of each symbol's span
  (`symbols.hash`, `Def::hash`), the argument count, the argument
  labels and constructor form of an overloaded call (`Call::arity`, `Call::labels`,
  `Call::constructs`), the argument label of a parameter and whether its written type takes a
  trailing closure, or the name it is written as (`Param::label`, `Param::closure`), the names
  of a Swift file's aliases of a function type (`Facts::function_aliases`), the type
  parameters of a function or type, which Swift's join reads from every file of its pack
  (`Def::type_params`), the place of the call a
  chained call is made on and whether it is reached through `?` (`Call::on`, `Call::unwraps`), the class `?` unwraps a Rust function's written result to and the one `.await` yields from a future signature (`Def::unwrapped`, `Def::output`, `Call::awaits`), whether a Swift call is written under `await` and
  whether a function needs one (`Call::awaited`, `Def::suspends`), the modules a Swift file imports
  `@testable` and the names only their file sees (`Facts::testable`, `Facts::confined`), the Kotlin constructors only their own class calls (`Facts::sealed`), whether an `inferred` call was narrowed from several
  definitions of its name (`calls.narrowed`), the definition a decorating call decorates (`calls.decorates`, the
  call site's `decorates`), callable-to-signature and
  static-member names (`Facts::overloads`, `Facts::constructors` and
  `Facts::statics`), the namespaces and types a C# file's `global using` directives name
  (`Facts::project`, names as `Facts::imports` holds them), the qualified names of the types a C#
  file declares in `partial` parts (`Facts::partial`), the qualified names of the classes a
  Kotlin file declares `expect` or `actual` (`Facts::expect`, `Facts::actual`), the size of the budget a file exhausted
  (`Facts::budget`, which `meada status` names), the count of usings the project of a file
  passed (`Facts::usings`, set by a join and stamped into the stored facts after it, which
  `meada status` names), whether the grammar misread a Kotlin file (`Facts::misparsed`, which
  `meada status` names), the literal URL path fragments and the
  method of an unresolved consumer (`contracts.fragments`, `contracts.method`), the field paths
  of the shape the code gives a side and its source file (`contracts.shape`: the fields an HTTP
  consumer reads from the parsed response, and those of the zod schema or object literals a
  route handler returns, spec #23, with the parts a package import gives as the specifier, the
  name imported, the importing file and the view, issue #175), the shapes a repository with a
  named manifest keeps for the constants and types its files export (`contracts` rows of role
  `shape`: the exported name in `method`, the view (`shape::Want`) in `via`, and field paths, open
  parts, scalar leaves, declared and optional paths and the imports they leave to fill, issue
  #175; what crosses repositories is this shape of an exported name,
  never its source text), the Backstage plugins whose `pluginId` cannot be read (`contracts`
  rows of role `unmounted`, no contract, at the `createBackendPlugin` call, issue #615), the names of the Cargo features a Rust crate enables by default
  (`files.setting`, a comma-separated list per file, from `[features] default`), the Rust item
  macros a file invokes (#398: the lines after it in `files.setting`, each a macro's name and the
  `cfg` predicates its arm writes, as tokens; `Facts::macro_rules`, the outer attributes each
  `macro_rules!` of a file writes on the items it passes through, as tokens; `Facts::invokes`,
  the names of the macros a file invokes with braces), and what a
  provider document states (`documents.summary`: format, version, title, normalized route paths
  and methods, channel sides (broker kind, name, role), the field paths of their response and
  payload shapes (fields, open parts, scalar leaves) and the external references met, the
  hosts and server paths of the root `servers` URLs, and the parser's reason and line for a
  document it could not read), and what spec #22 stores of the build and package files: the
  `manifests` summaries of each `package.json` (name, version, the dependency ranges as
  written, `exports` as written, `types`, `module` and `main`) and, spec #24, of each
  `pyproject.toml` (normalized name, version, the dependency specifiers as written, the source
  roots and the console scripts it declares, a `[project] dependencies` entry's uv source as
  `via` or `inherits` (#533), and a uv workspace root's `members` and `exclude` globs as
  `uv_workspace`) and `requirements*.txt` (the dependency specifiers as written), of
  each `Cargo.toml` (spec #30: name, version or the flag that it inherits one, the library root
  as a path of the repository and the `[lib] name` when it differs from the package's, the
  entries with the version requirement as written, the `alias` when the key renames the crate
  and the `via`: a `path` folded from the manifest's directory, which keeps its leading `../`
  when it leaves the repository, or the normalized `git` URL; `workspace = true` as a flag; the
  `[workspace.dependencies]` entries; the `[workspace.package]` version), and of JVM build files
  (spec #26: literal Maven coordinates and versions, dependency coordinate fragments with `*`
  for computed parts, declared ranges and a `computed` source flag, source roots with the test ones marked, a pom's
  `<properties>` and `<parent>` coordinate, with the group and artifact of a computed entry as
  written (`Entry::template`, for `Maven::inherit` to fill from the parent poms), and Gradle's
  literal group, root project name and included project names/directories in `Summary::build`,
  with the version catalogs a settings file names as `(accessor, path)`; a Gradle dependency's
  `template` for a hole `gradle.properties` may fill, or a catalog accessor as a placeholder
  entry; and, as `Summary::properties`, the `name=value` lines of a `gradle.properties` and the
  `[libraries]` of a `*.versions.toml` as `alias -> group:artifact[:version]`),
  of each SDK-style `.csproj` (spec #28: the package id, else assembly name, else file name, the
  literal version, each `PackageReference` with its range as written, each `ProjectReference`
  as a `path` folded from the project's directory, each with `Entry::private` when it does not
  pass on (#369), the package properties as written (`Summary::properties`, #381), and
  `Summary::project`, the mark that it is an SDK-style project that owns its files), of each
  `Directory.Build.props` (#381: its `PackageReference`s, its `ProjectReference`s by the path as
  written, its package properties and the props file it imports, `Summary::import`) and of each
  `Package.swift` (spec #29, #124: the literal name, the library products with the targets each
  holds, `Summary::products`, each target's module and directory, `Summary::targets`, and each
  `.package` dependency with its requirement as written as the range and its source as `via`:
  `git <identity>` of a `url:`, `path <manifest>` of a `path:`, or `computed git <pattern>` and
  `computed path <pattern>`, with `*` for a computed part), the
  `tsconfig.json` rows
  (`outDir` and `rootDir`, as written) and the `exports` table (each file's exported names and
  the definition, constant, type or file they name), the `families` table (spec #26: each public
  Java class's ancestors past it, `Resolver::ancestries`, a repository class by file and name and
  an opaque one as `null`), the `flexible` table (spec #27, #106: each function that takes more
  than one count of arguments, a default parameter or a vararg, by file and qualified name with
  its parameter types, with the least and the most count, `resolve::Flexible`), the `entry_points` table (spec #31:
  each entry point's identity, kind, label and start symbol, rewritten with the contracts),
  each community's name and member identities (`communities`, spec #31), and the paths of the
  files whose values the last contract extraction read (`valued`, #391). `Call::args`,
  `Call::kwargs`, `Call::uses`, `Call::arg_types`, `Def::returns`, `Def::return_values`, `Param::default`,
  `Field::default`, `Const::value` and `Const::ty` are `serde(skip)`, so unchanged files come
  back from SQLite without them. An `interface` or `type` declaration is a `Const` too, so its
  name is stored (its members are not). `Code` records every file it needs but lacks as a miss, and
  every file of `full` it looks into (`Code::read`); `refresh` re-parses the files the last
  extraction read (`valued`) before its first pass, then the missed ones, and extracts again the
  packs that missed until none does. A contract kind that reads a
  value from a new place relies on this loop.

- `workspace.rs`, `config.rs`: `Workspace::discover` finds `Repository { root, identity, config }`
  from the `meada.toml` files (peers by `path` hint, then sibling clones; siblings that name a
  repository already in join, transitively; a sibling with an invalid `meada.toml` or
  `meada.decisions.toml` is skipped). Peers not found go to `Workspace::missing`. `config` parses
  (the `[serve]` and `[[resource]]` shapes and their deserializers are `meada_core::manifest`'s,
  re-exported)
  `[[peer]]`, `[serve] hosts`, `[serve] prefix` and `strip` (each must start with `/`, normalized
  by `gateway_key`), `[serve] documents` (`repo_paths`: repository-relative, none absolute or
  with a `..` segment, `./` and doubled `/` dropped; added to the documents discovery finds;
  `Serve::declared` is set when the `[serve]` table is present, even empty or with only
  `documents`, which makes the repository's documents provider sources) and `[[resource]]`
  (`kind` checked as a lowercase identifier, `id` not empty,
  `role` read, write or read-write, optional `via`) with `deny_unknown_fields`. `config::gateway_path(prefix,
  strip, path) -> Option<(gateway path, reason)>` is the one gateway rule (spec #19); `Serve::
  gateway_path(path)` applies it with `[serve]` prefix and strip, `Http::declare` with a
  document's server path as the prefix. `config::read<T>(root, name)` is the shared TOML reader (file, line and key in
  every error), also used for the decisions file; `Repository` carries both `config` and
  `decisions`, loaded together at discovery.
- `repo.rs`: root, identity (normalized `origin`, else `local/<dir>`), `uncommitted()`, `head`,
  `unclean` (status with ignored files, for snapshots and server-mode recording) and `changed`
  (status without them, for a local export's `dirty`), all by shelling out to `git`.
- `entry.rs`: entry points (spec #31): `find(&Local)` reads what `refresh` has at its end (the
  symbols, the targets of every call, the `exports` rows, the providers and consumers the code gives, the
  `Facts::programs` the packs set, and the console scripts of the `manifests` rows through
  the package seam's `programs`, and `Layout::programs`, Rust's binary roots) and
  returns the repository's `EntryPoint`s (identity, `EntryKind`, label, `site`), which
  `refresh` rewrites into `entry_points` and `export` loads into `Export::entry_points`.
  It imports no kind module: which side of a contract is a door, of which `EntryKind`, and how it
  is labelled are `ContractKind::entry` and `label`, pure functions of the contract since entry
  points are stored (an answer that changes needs a reindex). The index writer reads the
  `package` manifests and tsconfigs directly (`package::read_manifest` and the others it calls
  stay `pub`), and hands the kinds what it read (`Indexed`); the kinds' query-time questions
  (`declare`, `workspace_step`, `link`, `after_links`) never run at index time.
- `index.rs`: SQLite schema, `refresh`, `export` and the export types. Freshness is a pipeline in
  `refresh`: stat (mtime/size, trusted only if older than `RACY` = 2 s), hash with BLAKE3, parse
  only changed files (or those whose `settings` line moved, under the rest of their stored
  setting), parse again each file whose setting `LanguagePack::extend` now makes differ
  (`settle`, once: a kept file so parsed joins the changed ones), resolve in memory the calls of every file of a group not kept (below), rewrite rows only when a file's
  `resolved` digest changed, re-extract contracts for every group not kept (one row per side, the
  `Found::shape`s of a side reached several ways merged, as JSON in `contracts.shape`, which `export` loads into `Provider::shape` and
  `Consumer::shape`). The extraction reads values the index does not keep, so it parses again
  the files whose values the last extraction read (the `valued` table, `Code::read`) before its
  first pass, then each file a kind misses (`Code::misses`) until none does; a pack that missed
  nothing in a pass is done and is not extracted again (#391). A group of packs that resolve
  together (`lang::resolver_of`) none of whose files was changed, deleted or stamped by a join is
  kept when `meta.configs` (which folds Meada's version in), the member files and the stored
  `exports` match what this refresh found (`unchanged_groups`): its files are neither re-parsed
  for values, extracted nor resolved, its `contracts`, `symbols` and `calls` rows stay, and the
  entry points read its sides and reach from them (`stored_reach`). That rests on a group's
  extraction reading only its own files and finding contracts only in them (debug assertions
  in `refresh`); `Names::without` marks a question about an inline definition of a kept file,
  which makes the refresh start over keeping nothing (`refresh_keeping`). `refresh(db, root,
  identity, serve)` first brings the `documents` table (path, hash, mtime, size, `summary`) up
  to date the same way (`refresh_documents`, sharing `stat`), before the early exit and apart
  from the code rows: a document edit re-extracts no code. Without `serve.declared` the
  repository has no documents and stored rows are deleted. The walk yields every file the
  ignore rules keep; `lang::pack_for` and `document::is_document` split it, and
  `documents_found` adds each `[serve] documents` path that is a file, stat'ed outside the walk
  so `.gitignore` does not hide it; a listed path that is not a file, or that has a symbolic
  link in any segment (`plain_file`; the walk follows none either), has no row. `export` loads
  `Export::documents` (`Document {path, summary}`, path order). `refresh_stamped(tx, table, ...)`
  is the shared pass behind `documents` and `manifests` (spec #22), reading each file up to one
  byte past its reader's size limit (`read_up_to`): the `manifests` table (path,
  hash, mtime, size, `summary` as `package::Summary` JSON) holds every `package.json`,
  `pyproject.toml` and `requirements*.txt` the walk finds (`manifests_found`, through
  `package::is_manifest`; `package::read_manifest` reads each by its ecosystem; any path with a
  `node_modules` segment is skipped, since the walk
  yields it unless `.gitignore` lists it), a nameless one too, whatever `serve.declared` says;
  `export` loads them as `Export::manifests` (`serde(skip)`), and the `tsconfig.json` rows
  (`manifests_found` takes them too, `package::read_tsconfig`) as `Export::tsconfigs`. With the contracts, `refresh`
  rewrites the `exports` table (`Resolver::exported` of every pack: per file, each exported
  name with the `file` and `target` definition, constant or type it names, `target` NULL for a whole file and
  `file` NULL for nothing the repository holds, and each `export * from` a repository file as a
  row with `name` NULL), which `export` loads as `Export::exported`, the `families` table
  (`Resolver::ancestries`: each public class of a pack whose overloads merge across its family,
  Java, with its ancestors in method resolution order), loaded as `Export::ancestries`, and the
  `import` rows of `contracts`, loaded as `Export::import_calls` (all `serde(skip)`). Before extracting,
  `refresh` builds `Indexed` once (the `manifests` and tsconfigs as `read_manifests` returns
  them, the `exports` rows of every pack and the files), and the extraction loop runs only
  `contract::KINDS`, each `extract` taking the pack's `Code` and `Indexed`: the package kind
  builds the packages' public surface from it and adds the exported shapes, whose `shape` rows
  `export` loads as `Export::shapes` (`serde(skip)`, issue #175). Every `manifests` row is folded
  into the configs digest, so an edited `package.json` or `tsconfig.json` extracts again. `sort_providers` and
  `sort_consumers` put the lists back in `export`'s order after a kind's `declare` added sides.
- `community.rs`: communities (spec #31). Last in `refresh`, after the code rows,
  `write_communities` rewrites the `communities` table (name, member) from the `calls` rows at
  `exact` or `inferred`: each symbol in one is a node (a top-level caller is a file, no node),
  so an unchanged repository keeps its rows through the early exit and a
  changed one recomputes them from scratch. `communities(calls, dir)` builds the undirected
  graph (a call site adds 1 to the edge between two distinct symbols, a call to self none, so a
  node with no edge is a singleton community) and runs Leiden as spec #31 fixes
  it, in `i128` only: `local_moving` (identity-order queue, largest strictly positive gain
  `2m · w_iC − k_i · K_C` minus the stay quantity, ties to the lowest smallest member),
  `refine` (best choice over singletons in identity order, into a well-connected
  subcommunity) and aggregation (nodes numbered by smallest member), until an iteration moves
  and merges nothing, with no cap. `export` loads the rows as `Export::communities`.
- `lang/mod.rs`: the **language adapter**, `LanguagePack` (extensions, `parse` to `Facts`,
  `config_files`, `modules`, `receiver`: a keyword naming the enclosing class's
  instance, `this` for TS/JS, none for Python, `base_receiver`: a keyword naming its base, whose
  members are not resolved, `super` for TS/JS, none for Python, whose `super()` is a call, and
  `globals`: names bound everywhere that contract kinds read through `Code::binds_everywhere`,
  `fetch` for TS/JS, none for Python), and the `Facts`/`Value` types, plus the tree-sitter
  helpers (`named`, `children`, `text`) every pack shares. `PACKS` lists `lang/ts.rs` (`.ts .tsx
  .js .jsx .mjs .cjs`, reads `tsconfig.json`), `lang/python.rs` (`.py`, reads
  `pyproject.toml`: `.pyi` and notebooks are not read), `lang/rust.rs` (`.rs`, reads every
  `Cargo.toml` through the `**/Cargo.toml` config name, receiver keyword `Self`, see below) and
  `lang/go.rs` (`.go`, reads every `go.mod` through `**/go.mod`, see below), then the Java,
  Kotlin and `lang/csharp.rs` packs (`.cs`, reads every project file through `**/*.csproj`
  with `dotnet::msbuild`).
  `refresh` reads a `**/<name>` config at any depth from the walk, in path order (and a
  `**/*.ext` one, every file ending so, `lang::names_config`), and any
  config through `read_config` (up to one byte past `lang::MAX_BYTES`, no symbolic link,
  as a manifest is); it joins each pack's files first (`LanguagePack::join`, built from the
  configs and every file's facts as parsed, applied to each file and to each file the
  contracts parse again: C#'s project-wide usings and `Facts::unit`), then groups the
  parsed files by pack, asks each pack once for its `layout(configs, parsed files)`, keeps the
  files `Layout::members` names (default: all; a left-out file keeps its `files` row and facts
  with an empty `resolved`, its symbol and call rows are deleted, and the export and `status`
  skip it), and builds each `Resolver` from `Layout::modules`, `Layout::published` and
  `Layout::reaches`; the `Resolver` reads a type declared in parts in several files of one unit
  (`Facts::partial`, `Facts::unit`) as one type (`Resolver::parts`). `parse` sees one file at a time; no pack
  method touches the index. `Facts::locals` holds every name a binding makes besides definitions, `const`s and
  `Def::params` (`let`, `var`, destructuring, `catch`, `for ... of`, a nested, array or rest
  parameter pattern, any parameter of a function that is no definition), qualified by the
  enclosing definition; `Resolver::lexical` checks it first at each scope, so it hides an outer
  name and a definition, constant or parameter of its own scope alike (`Binding::Hidden`: a
  call falls to a name match, a value is not known), the hidden names probe. `Call::uses` (spec #23, `serde(skip)`) is the pack's optional shape
  facts: each chain of `Step`s (`Field`, `Element`, `Call(method)`, `Typed(Ty)`, `Argument(tys)`,
  the type arguments a call is given, `Return`) the function
  making the call applies to its result; the TS/JS pack fills it in `lang/ts/uses.rs` (through
  `await`, parentheses, casts, member and literal element accesses, method calls, and the uses
  of the names a `const` binds, a destructuring pattern (`PATTERN_DEPTH` deep; each field or
  element it names is a read), the variable of a `for...of` and the first parameter of an
  array method's callback written in place (`ELEMENT_METHODS`), each only when nothing else in
  the function binds or assigns the name; `Return` only from the call's own function), within
  `USES_BUDGET` per file. `Def::returns` holds a function's return type annotation as a `Ty` (`serde(skip)`, reparsed). Types (spec #23): `Ty` (`Object`, `Named`, `Intersection`, `Array`, `Schema`, `Param`, a type parameter of the
  enclosing function, `Unknown`) is what `lang/ts/types.rs` reads from a type; an `interface` or `type` declaration is a `Const` holding `Value::Type(Ty)` (so lexical, import and export resolution find it; `Code::held` skips it and `Code::declared_type` finds it), `Const::ty` and `Call::arg_types` hold the type a constant or argument is annotated or cast to (`serde(skip)`), and `declared.rs` (`Types`) expands a `Ty` into its field paths, optional marks and open (index signature) paths, with `TYPE_DEPTH` and `TYPES_BUDGET`. `Value` has `New(callee, args)` (`new X(...)` as a value), `List` (an array literal),
  `Call(Invocation)` (the callee, the receiver value of a method call and its arguments, so a
  chain such as `z.object({..}).optional()` is read, and the call's range in `Facts::calls`,
  which `Code::origins` resolves to follow what it returns), `Member(object, property)` (`schema.shape`),
  `Schema(name)` (a constant annotated `z.infer<typeof name>`, from `lang/ts/types.rs`) and
  `Scalar(Option<u32>)` (a number or boolean literal, spec #23; a plain integer carries its value,
  a status), and `Code::is_param` and `Code::is_global` tell a name that is the handler's own
  parameter, or a global such as `Response`, from one a local binding hides;
  `property(object, key)` (the last key, through spreads; `Err(Depends)` when a spread of a
  parameter may set it) and `literal(parts)` are shared by kinds and packs. An anonymous function
  written as a property of an options object passed as an argument (`run({ eachMessage: () =>
  {} })`, or an object method there) is a `Kind::Inline` definition and a `Value::Function`,
  like a function argument. The channel kind's readers (`channel/{ts,python,rust,go}.rs`, each
  `messages(code) -> Vec<Message>`, answering for its own pack; the types are in
  `channel/message.rs`) give the **producer and consumer output** (spec #20): `Message {broker, name: Option<String>, path, call, role}`,
  `role` (`Side`) `Producer` or `Consumer(Handler)`, `Handler::Def {path, name}` (a definition; a name
  that folds to nothing is the top level, so a pack passes a call's `scope` for "the enclosing
  function") or `Handler::Inline {path, def}`; `Side::Publish {exchange}` (a publish to a named
  exchange, `name` the routing key) and `Side::Binding {exchange, pattern}` (`name` the queue)
  are the RabbitMQ roles that are neither a plain producer nor a consumer. `name` is `None`
  when it cannot be evaluated (an unresolved side). A reader asks `Code` (values, `Code::origins`,
  `Code::outside`, `Code::expand`) and reports; the pack holds none of it, and the channel kind turns the output into contracts. TS/JS library
  forms live in `channel/ts.rs`: kafkajs `send({topic})` on a `new Kafka().producer()`,
  `subscribe({topic})` or `({topics})` on a `new Kafka().consumer()`, with the handler of the
  first `run({eachMessage|eachBatch})` on the same consumer binding (else the function whose
  call subscribes); AWS SDK v3 `SendMessageCommand` / `ReceiveMessageCommand` (the queue is the
  last segment of the `QueueUrl`, so only that tail has to evaluate; the receive's handler is
  its enclosing function); `sqs-consumer` `Consumer.create({queueUrl, handleMessage})`; NATS
  `publish(subject)` / `subscribe(subject, {callback})` on a `connect()` connection (the
  handler is the `callback`, else the enclosing function); BullMQ `add` on a binding that holds
  `new Queue(name)` (the queue is named by the binding, read with `Code::expand` over `Expand::Constructed`:
  the `add` call is the producer, or, when `name` is a parameter of the function that makes the
  binding, each call site of that wrapper) and `new Worker(name,
  processor)` (the processor, a definition or a function in place, is the handler); ioredis
  `xadd(stream, ...)` and `xreadgroup(..., 'STREAMS', s1, s2, id1, id2)` (every stream of the
  first half after `STREAMS`; the enclosing function handles) on a `new Redis()` or default
  import; node-redis `xAdd(stream, ...)` and `xReadGroup(group, consumer, {key} | [{key}])` on a
  `createClient()` binding; and amqplib on a channel that
  traces to `connect().createChannel()` or `createConfirmChannel()` (default import, namespace
  or named `connect`): `sendToQueue(queue)`, `consume(queue, handler)` (a function in place or
  a definition; else the enclosing function), `publish(exchange, key)` and `bindQueue(queue,
  exchange, pattern)`, each name read through `Code::expand` like a topic. `new X(...)` is a
  call fact like `X(...)` (the `new_expression` arm of `ts.rs`'s `calls`, callee `X`), so a
  form can start at a constructor, and `value` reads through `await`, so `const nc = await
  connect()` or `const channel = await conn.createChannel()` is a traced binding. Both are in
  every export (`Kafka#0` and the like: unresolved calls, no target).
- Python message forms live in `channel/python.rs`, which finds the client of a call with
  `Code::origins` with `Follow::Values` (which follows `self.x`, parameters and returns, as
  `Follow::Constants` does not) and
  names the constructor it reached from the file's imports (`Code::imported`:
  `confluent_kafka.Producer`, however it is imported; `Code::outside` also answers for a method
  of a typed local, so it is not used). One `channel()` or `client()` above a constructor is
  followed (`pika.BlockingConnection.channel`, `boto3.Session.client`), also when the client is a
  call written in place (`boto3.client("sqs").send_message(...)`): `head` takes the call that
  starts where the outer one does and ends last inside it, since the callee of such a call is
  `(call).method`. A `KafkaConsumer` is told by its resolved name, so an alias counts. Forms: confluent-kafka
  `Producer.produce(topic)` and `Consumer.subscribe(topics)`; kafka-python `KafkaProducer.send(topic)`,
  the topics a `KafkaConsumer(...)` constructor takes and `subscribe(topics=)` (a `^` topic is a regular expression, so it is unresolved); pika
  `basic_publish(exchange, routing_key)` (the empty exchange is a producer of the key),
  `basic_consume(queue, on_message_callback)` and `queue_bind(queue, exchange, routing_key)`;
  boto3 `send_message`, `send_message_batch` and `receive_message` with `QueueUrl=` on a client
  made for `"sqs"`; nats-py `publish(subject)` and `subscribe(subject, cb=)` on `nats.connect()`
  or `nats.aio.client.Client()`; redis-py `xadd(name)` and the keys of `xreadgroup(...,
  streams={...})` on `Redis` or `StrictRedis` (`redis` or `redis.asyncio`, also `from_url`). A polling
  client's handler is the function that subscribes. The pack's `value` reads `await x` as `x`
  (a traced `nc = await nats.connect()`) and a dict literal as `Value::Object` (a key that is
  no string literal is the entry `[]` holding the key and the value, so `Code::expand` folds
  it). `message::channel_name` and `message::queue_name` (`channel/message.rs`) evaluate a channel name and an SQS queue URL
  for every reader.
- `http/rust/frameworks.rs` (`FRAMEWORKS`): per object of axum, actix-web, Rocket, reqwest and
  ureq, the outside names of its constructors and the methods that hand it back; the HTTP
  kind's readers hand `passes_on` to the question (`Follow::Values { passes }`) and ask what a
  constructor makes (`frameworks::made`). `VERBS` (the `Method` constants, in `lang/rust.rs`) is
  the one pack constant a kind reads: the pack folds `Method::GET` at parse time with it.
- Rust message forms live in `channel/rust.rs`, beside the TS and Python ones and without
  a parallel path into `channel.rs`. The `LIBRARIES` table names each crate's constructors
  (suffix-matched on `Code::outside`, without the path flag, over the constructor the
  `Follow::Values` origin reached) and the methods that hand back the same object
  (`passes`, the predicate the channel reader hands the question, so `flow` follows `set`, `create_channel`,
  `connect` and the like). Forms: rdkafka `send` of a `FutureRecord::to(topic)` or
  `BaseRecord::to(topic)` (the record is found from the argument, down its chain) and
  `subscribe(&[topics])`, on a client from `ClientConfig::new()`; lapin `basic_publish`,
  `basic_consume` and `queue_bind` on a channel of `Connection::connect`; aws-sdk-sqs
  `queue_url` of a `send_message`, `send_message_batch` or `receive_message` builder (the
  queue is the last URL segment); async-nats `publish`, `subscribe`, `queue_subscribe`; redis
  `xadd` and the keys of `xread_options`, only with a `group` option. A polling or stream
  client's handler is the function that holds the call. The shared evaluation (`produce`,
  `consume`, `topic`, `topics`, `publication`, `queue_name`) lives in `channel/message.rs`, used by
  the four readers. A `let mut` bound once keeps its call as `Facts::mutable`, read by
  `Code::mutable`, so a redis connection held by one is traced.
- `lang/python.rs` yields `Kind::Method` for a function directly in a class body, keeps the first of
  two definitions of one name, spans a decorated definition with its decorators, and exports
  every module-level name (imports included), as an explicit import or `m.x` sees them, and
  its star imports, last binding first, since a later one rebinds the name (`stars_bind`: the
  resolver takes the first export that gives a name, so a star import after a definition
  rebinds it when its module exports it); what `import *` takes is `Facts::star`: the public
  names, or a literal `__all__` alone, grown by the literal names a statement of the module
  itself adds (`+=`, `.extend`, `.append`: `statement`), else `Star::Computed` (also for an
  item set or deleted, `Module::items`, and any other binding of `__all__`). The last binding of a name in its scope
  decides: a definition another binding follows (at the top level, `Module::hidden`; in a
  function, where a nested function's `nonlocal` binding follows any, `Locals::finish`), one
  not `in_place` (under `if`, `try`, a loop, `with`, `match`) after another binding of its
  name, and a name `del` unbinds (in any scope), are `Facts::locals`, which hide the name. A class body's
  binding hides no method, which `Resolver::member` reads through its class.
  Its imports are the module as written (`.`, `..pkg`, `a.b`): `import a.b.c` binds `a`, `a.b`
  and `a.b.c` (dotted `Import::local`s); `from p import m` is an `Imported::Member`, a name
  `p` exports or else the submodule `p.m`; `from p import *` is `Imported::All`. `modules` resolves them against the repository root, then the roots
  `pyproject.toml` declares (setuptools `package-dir` and `packages.find.where`, Poetry
  `packages.from`, read by `lang/python/pyproject.rs` through the PyPI manifest reader, so an
  unreadable file declares nothing), or `src` when none is declared (`pyproject::source_roots`,
  which the PyPI adapter shares); a package comes before a module of the same name.
  An `if __name__ == "__main__":` block at the top level (either operand order, either quote,
  nothing else) yields the definition `__main__`, spanning the `if` and its body (an `else` is
  not part of it), owns the calls in that body, and is named in `Facts::programs`.
  It also yields the facts receivers resolve through: `Def::bases` (a class's bases as
  written, `B[T]` read as `B`, any other expression `(<kind>)`) and `Facts::receivers`, one
  `Receiver {scope, name, class}` per name a scope binds: a method's first parameter unless
  `@staticmethod` (`Class::Enclosing`), an annotated parameter or name, `x = C(...)`
  (`Class::Named("C")`, whatever `C` turns out to be), `self.x: T` in a method (an attribute,
  at the class's scope), and `Class::Unknown`, which hides an outer binding, for any other
  binding: other assignments, `for` and comprehension targets, `with` and `except ... as`,
  `:=`, `case` captures, a lambda's parameters (in the enclosing scope, which owns its calls)
  and a parameter the body assigns. A name its scope binds only once, by `name = value`, is
  also a `Const` holding that value; so is the name of `with C(...) as x` (also
  `Class::Named("C")`, as `x = C(...)` is) and a `self.x = C(...)` attribute (a `Const` of the
  class, only for a call). `xs += ...` and `xs = xs + ...` on a name whose latest
  value in that scope is a list literal do not bind it again: a list literal added joins the
  `Const`'s items, anything else added (`+= static(...)`) is left out. Calls carry their positional `args` and keyword `kwargs`
  as `Value`s (strings, f-strings and `+` folded into parts, a name or a member chain in them,
  `config.BASE`, a `Part::Ref`; `[...]`/`(...)` a `Value::List`, a call
  a `Value::Call` with its callee and range, no receiver or arguments), a decorator call names the definition it decorates
  (`Call::decorates`), a decorator written as a bare name is in `Def::decorators` (as a
  callee, `http.require_safe`), and a function lists the values it returns (`Def::return_values`).
  A function's parameters carry their defaults as written (`Param::default`) and which
  argument binds them (`Param::binds`, a `Binds`): `*args` and `**kwargs` are `Rest` (no single
  argument), the parameters after them or after the bare `*` (which is no parameter) are
  `Keyword`, a method's first one is `Instance` (`Class` for a `@classmethod`, neither for a
  `@staticmethod`), any other `Any`. A target that is `self.x`, also inside unpacking, a `for`
  or an `as`, and also written in a function nested in the method, binds the class's attribute
  `x`. An import in a function or a class is `Class::Imported`: it hides an outer binding there
  and resolves through the file's imports. An annotation decides; other bindings that
  disagree make the class unknown. `global x` sends the function's bindings of `x` to the
  module, `nonlocal x` to the nearest enclosing function that binds `x` (class bodies
  skipped). Annotations are read as written: a name, a string holding one, `Optional[...]`
  and `... | None`.
- `lang/rust.rs` also yields `Facts::macro_use` (the crates of `#[macro_use] extern crate x;`,
  stored) and `Facts::mentions` (`lang/rust/mentions.rs`: the names a file refers to, apart
  from those a pattern, a parameter, a field or an item declares, a reference a local hides and
  its route attributes; not stored, so the providers get it from the files parsed again).
  A function does not hide the crate a path begins with (`rocket::build()` beside a
  `fn rocket`), and a module of the same name as a function still hides the crate.
- `lang/rust.rs` (spec #30) yields free functions (`Kind::Function`), structs, enums, unions
  and traits (`Kind::Class`), modules written in place (`Kind::Module`), methods of an `impl`
  (`Type.method`, or `Type.Trait.method` in a trait's block, `Kind::Method`) and trait methods
  and required methods (`Trait.method`); closures are no definitions, their calls belong to the function
  around them and their parameters are its locals, as `let`, `for`, `if let`/`while let` and
  match arm patterns are. A method's `self` is a `Param` with `Binds::Instance` and a
  `Receiver` of `Class::Enclosing`. Receivers (`Facts::receivers`) also come from a parameter
  or a `let` of a plain name with a written type, from a struct's named field (at the struct's
  scope), and from a `let` holding a call of a name or a path (`Class::Returned`, the callee as
  the call spells it: the resolver gives the class `Def::result` names, `Self` included, the
  class itself for a constructor, and an outside crate's call stays outside). A type is read
  as written (`written`): `&T` and `&mut T` as `T`, `dyn Trait`, `impl Trait` and a type
  parameter (`<T: Trait>` or `where T: Trait`, of the function, `impl` or struct around)
  bounded by exactly one trait as that trait, a primitive or a path as a path normalized like a
  call's (an outside crate's or the prelude's type, `Box<T>` and `Vec<T>` included, is in `Facts::externs`, so opaque); anything else (tuples,
  slices, several bounds, `T::Item`) names no type, and a `let` of it is a local
  that hides the name. A `let` with a written type and one that disagrees, in a scope, leave
  the class unknown. `Def::result` is the same reading of a function's return type. Each `impl`
  is a `Facts::impls` entry (its type and trait as callee-like names, from the scope around
  it); a block whose method names one already defined takes the next free `#n` scope
  (`E.From#2`), so each body is its own symbol. A `mod x;` declaration is a `Kind::Module`
  definition `x` (`inner.x` in a module written in place, the declaration's span) and an
  import of the whole file under that name, its specifier `mod x` / `mod inner/x` or, with
  `#[path = "p"]`, `path |p` / `path inner|p`; at the top level the file also exports it.
  Every top-level item and `use` name is exported (`Facts::exports`, `Facts::private` for the
  ones not `pub`, which the exports table leaves out); the `pub` ones are `Facts::star`, and a
  `pub` item of a `pub` module written in place is also exported under its dotted name. Calls: an identifier, a
  member chain (`Call::by_path` false) or a path (`by_path` true, dotted by `normalize` once
  the file is read: `crate.…` and `super.…` with their synthetic imports `crate` and
  `super(.super)`, `Self.…`, `self::` and in-file `super::` as `mod.<module>.…` with the
  synthetic import `mod` of the file itself (`Tree::lookup("self")`), and a first segment no
  item of the module or `use` of the scope binds a crate outside the file, `Facts::externs`).
  A `use` leaf is an `Imported::Path` import (`use a::{b, c as d, e::{self}}` binds `b`,
  `d`, `e`; `local` is qualified by the scope holding the `use`; `as _` and `use serde;` bind
  nothing) or an `Imported::Glob` import (`local` is `*` qualified the same way), its path
  normalized like a call's, `use super::*` and `use self::*` being `inside`. Macro invocations
  are not read, apart from item macros (`lang/rust/macros.rs`, #398): a `macro_rules!` whose
  first arm's matcher is exactly `$($x:item)*` (no separator) and whose transcriber is exactly
  `$( #[..] .. $x )*` (outer attributes only, no `#[path]`, no metavariable in them) passes items
  through. `record` reads, whatever the setting and the `cfg`, each file's definitions
  (`Facts::macro_rules`: the attributes as tokens, or `None` for any other definition, two that
  disagree, or one written inside macro tokens) and the names it invokes with braces
  (`Facts::invokes`, inside macro tokens too). `Rust::extend` gives each file the item macros it
  invokes whose every definition in the repository agrees, a line each after the features (the
  name, then the `cfg` predicates of its attributes, tab-separated); `table` reads them back.
  `bodies` parses, before the walk, the braced body of each such invocation written as `m!` or
  `crate::m!` (`invoked_name`; any other path is skipped) among the items of the file, of a
  module written in place, or among the associated items of an `impl` or a trait in either,
  over the file's bytes (`set_included_ranges`, so spans are the file's), `ITEM_MACRO_DEPTH`
  deep; a body with an error is left out. The decider reads the setting the index stored for a
  file (`files.setting`, `Engine::stored_setting`), so a function from such a body is
  function-like there. `built` gives a built invocation's body items in its place, each judged by
  its own attributes after the arm's predicates (`Built::keep`), so a `mod x;` in one is a `mod`
  declaration of the module and reaches its file. Conditional compilation (`lang/rust/cfg.rs`): `parse` skips a node whose
  preceding `#[cfg(..)]` attributes are not all true (`compiled`), for the fixed target
  `x86_64` Linux (`unix`, `target_os = "linux"`, `target_family = "unix"`,
  `target_arch = "x86_64"`, `target_pointer_width = "64"`, `target_endian = "little"`,
  `target_env = "gnu"`, `target_vendor = "unknown"`, `panic = "unwind"`, `debug_assertions`,
  `target_has_atomic` (8 to 64, `ptr`), `target_feature` (`sse`, `sse2`, `fxsr`), the literal
  `true`; `test` and any other predicate are false) and the crate's default features:
  `[features] default` followed through features that enable others (`dep:x` entries ignored;
  `x/feat` enables the feature `x` of an optional dependency that no `dep:x` names, `x?/feat`
  does not). `test` is false, as in a build of the crate: `cfg(not(test))` items are indexed.
  The exception is a predicate that is exactly `test`: a `mod` under it is indexed, any other
  item under it is not (a `#[cfg(test)] use` would shadow the production one).
  `#[cfg_attr]` is no skip; a cfg-false `mod x;` makes no import, so its file is no member.
  `built` decides the children of a node in one pass over the list, carrying the pending
  attributes (and the `#[path]` value) forward. An inner `#![cfg(..)]` that is false drops the
  module written in place, or the whole file (`Facts::unbuilt`, which keeps it out of the
  module tree). The features reach `parse` as `Facts::setting` (`LanguagePack::settings` gives
  it per path, `parse_with` takes it; the index keeps it in `files.setting`): `refresh` parses a
  file again when the setting it was parsed under differs, so a toggled default feature gives
  the cold export. The decider's `function_like` parses under the setting the index stored for
  the file (`Engine::stored_setting`), and reads the nearest `Cargo.toml` files from disk for it
  only when the index has none. `lang/rust/crates.rs` (`Tree`) is the module tree behind the
  Rust `layout`, built once per refresh (each `Cargo.toml` parsed once for it; `settings` parses
  them once more, before any file is): the crate roots of every `Cargo.toml` with a `[package]`, at any
  depth (`[lib] path`, else `src/lib.rs`, `src/main.rs`, `[[bin]] path`, `src/bin/*.rs`,
  `src/bin/*/main.rs`), walked breadth first
  through each file's `mod` imports (`x.rs` or `x/mod.rs` under the directory the file owns:
  its own for a root, a `mod.rs` or a `#[path]` file, the one named after it otherwise; a
  `#[path]` from the file's directory, or from the inline module's directory inside one).
  `lookup` answers `crate` (the root), `self` (the file), `super` (`super::super`, through the top-level
  declarations that reached the file), a `mod` specifier (its file) and a crate's name (the
  root of its library, `[lib] name` else the package name, hyphens as underscores; a binary is
  no library), reached only from the crates whose manifests list it as a dependency
  (`Tree::libs`, per crate root; `dependencies()` reads the three tables, a `package =` rename and
  `workspace = true`). `Tree::reaches` (`Layout::reaches`) answers whether a name match from
  one file may land in another: one crate builds both, or the second is in a library of
  `Tree::reach` for the first's crate root (its `libs`, then each one's `[dependencies]`, all
  the way down: a dev or build dependency of a dependency is not part of it).
  `Tree::published` (`Layout::published`) walks from each library root through `pub mod`
  declarations (`Export::From` whose source is the specifier, not in `Facts::private`): the exports table keeps the `pub` items of those files alone, and a binary
  root exports nothing (no other crate can import it), so no `export` entry point either.
  `lang/rust/values.rs` (`Reader`) reads values for contract kinds (axum, spec #30 #129): each
  call's `Call::args` and `Call::receiver` (the value a builder call is made on,
  `LanguagePack::fluent`), a closure argument as a `Kind::Inline` def named `@<byte offset>` and a
  `Value::Function`, a `const`, `static` and a `let` bound once in its scope as a `Const`, and a
  function's `return_values` (its tail expression and `return`s); `Put` queues each until the
  file's `use`s are known, so a path normalizes like a call's. Depth is bounded by `VALUE_DEPTH`.
  Strings fold (spec #30 #131): `+`, `format!`/`concat!`, `String::from` and the conversions
  that keep the text, `Method::GET` of the HTTP crates (`Scan::targets` holds what each `use`
  imports), and a `let mut` String that `lang/rust/builders.rs` finds a plain builder
  (`parse` collects them per scope and name; the Reader substitutes the text the pushes before
  the read position hold, memoized per push).
  `args`, `receiver` and `return_values` are not stored (the constants arrived with schema version 28).
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
  `Facts::package`.
- `resolve.rs`: the shared resolution behind the adapter. `Resolver` connects calls to definitions
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

## Java receivers and supertypes (#85)

Each class writes `Def::bases` from `extends` and `implements` (type arguments dropped; an
unreadable base is `(type)`, opaque), and each method its declared return class as
`Def::result`. Fields, parameters and locals become `Facts::receivers` with `Class::Type` of
their written type; `var` takes the class of `new T(...)` without a body, or the result of
a call (`Class::Result`, the initializer call's place). A chained call (`make().save()`)
writes `Call::on`. Primitives, arrays, varargs and type parameters are `Class::Unknown`. A local is
typed only when it is the one binding of its name in its method scope, not when a field of the
file has its name (a use before the declaration reads the field), and not in an initializer
block or a field's lambda. The pack sets `base_receiver` (`super`), `unordered_bases` and
`implicit_members` (`java.lang.Object`'s methods). The shared resolver reads overload families
per signature across the C3 family (`Resolver::overloaded`, strict about opaque bases), bare
calls through `Reading::Method` (`Resolver::inherited`), bare fields through
`Resolver::inherited_field`, and name matches through the owning family (`Resolver::matched`).
Fields and methods are separate namespaces where class members are in scope: a name read, not
called (`Reading::Read`: the head of `repo.save()`, a member read on the way in
`this.repo.save()`, and every value `Code` folds or traces), sees the field `repo`, never a
method `repo()`, while a call (`repo()`, `this.repo()`) sees the method (#89).

## Java source facts (#84)

`lang/java.rs` reads `.java` with tree-sitter-java, with no JDK or build tool. Types carry their
package-qualified names; methods and constructors carry the parameter types as written.
`Facts::overloads` maps method names to their definitions with signatures; `Facts::constructors`
keeps constructor families under their class names. The shared resolver selects the family by
the stored `Call::constructs` and narrows it by `Call::arity`, so cached and cold resolutions
agree. Comments do not count as arguments, and type-use annotations and comments do not enter
written signatures; literals recovered from malformed types are excluded too.
`members_in_scope` admits bare class members, and `Imported::Qualified`
resolves a class import to its full name while preserving the exact outside spelling.
`qualified_types` admits a fully qualified repository type without an import only when lexical
lookup leaves the path head unbound. Enum and interface constants bind values too.
Constructor calls read the type namespace, so methods and values of the same name do not hide
an imported class. Only class candidates can supply constructor families, including after a
static wildcard import yields several bindings. A method named after its own class has `#method` after its written signature,
even when that class declares no constructor, keeping its identity distinct and stable when
a constructor is added. Constructors keep their class-named signatures.
Package groups come from `Facts::package`, independently of directory layout. The package's
inside glob sees its non-public types; public star sets and hidden members define the exported
surface. Static class globs take `Facts::statics` alone, and a pack whose `static_imports_inherit()` is false (C#: "inherited members aren't imported") offers only the statics the named class declares itself, not its bases'. `unordered_globs` combines candidates from
on-demand imports, explicit imports take priority, and the current package takes priority over
public package globs including `java.lang`. A shared package checks each declaring file's star
set, so its first file does not hide another file's public types. Qualified imports go through
shared exports, preserving duplicate fully qualified types as ambiguous candidates. `meada-core::parent` ignores dots
inside signatures when finding the enclosing scope. Calls in lambdas and anonymous classes
stay with the enclosing definition. Generic annotation calls carry `decorates`, positional
values and named arguments for the contract kinds to read. The annotations of a field or a
parameter, which no definition carries, are `Facts::annotated` (transient: the binding they
mark and the annotation as a call), and every name a file assigns (`f = v`, `x.f = v`) is a
marker constant `.f` (stored, like Go's), which `Code::field_written` reads (#89). A `static`
field's annotations are not recorded. A `final` field a constructor assigns keeps that
constructor as `Const::scope`. `this(..)` and `super(..)` are constructor calls
(`Call::constructs`) of the class's own name and of its `extends` type.
