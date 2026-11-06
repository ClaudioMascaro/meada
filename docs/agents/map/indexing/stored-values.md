# Stored values

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
  method of an unresolved consumer (`contracts.fragments`, `contracts.method`), the SDK operation
  of a code use (`contracts.method`, #660) and its unresolved environment variable (`contracts.via`,
  also carried by providers in the export), the field paths
  of the shape the code gives a side and its source file (`contracts.shape`: the fields an HTTP
  consumer reads from the parsed response, and those of the zod schema or object literals a
  route handler returns, spec #23, with the parts a package import gives as the specifier, the
  name imported, the importing file and the view, issue #175), the shapes a repository with a
  named manifest keeps for the constants and types its files export (`contracts` rows of role
  `shape`: the exported name in `method`, the view (`shape::Want`) in `via`, and field paths, open
  parts, scalar leaves, declared and optional paths and the imports they leave to fill, issue
  #175; what crosses repositories is this shape of an exported name,
  never its source text), the Backstage plugins whose `pluginId` cannot be read (`contracts`
  rows of role `unmounted`, no contract, at the `createBackendPlugin` call, issue #615), the
  Backstage `$include`s not followed (`contracts` rows of role `unfollowed`, no contract, at the
  TS group's first file: the including config file's path in `via`, the include as written in
  `fragments` and the reason's name in `method`, #689; never a value the include resolves to), the names of the Cargo features a Rust crate enables by default
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
  files whose values the last contract extraction read (`valued`, #391), and, ADR 0004 amended
  by ADR 0025, the config values (#639): `config_files`, each YAML, JSON or Terraform file the walk visits
  or the hidden CI paths hold, stamped, with what reading it came to (`config_values::Outcome`:
  read with its counts of values and redacted ones (and, for a `.tfvars` file, the sensitive variable names it was read against, `sensitive`), a lockfile, or unreadable with the line and
  the reason), and `config_values`, each of its scalars but nulls with its key path, its value cut
  at `config_values::VALUE_MAX` (`cut`) or NULL when a Kubernetes `Secret` holds it, its type,
  its line (`string`, `number`, `boolean`, and for HCL `expression` and `template`, whose value is
  the source text as written), and the key path and value lowercased for search (`folded_key`,
  `folded_value`; `id` is its primary key), with `config_search` beside them (#647), an
  external-content FTS5 table with the trigram tokenizer (`case_sensitive 1`) over both folded
  columns, written by `config_values::store` and emptied row by row by `forget` (the FTS5 `delete`
  command, which needs the old text); and the IaC nodes (#649): `iac_nodes`, each node's address,
  root module label, type, physical name (the argument selected by the type table, statically
  evaluated, #650, #656), `pattern` (1 when a part of the name is unresolved, each such part a `*`),
  `overridable` (a JSON list of `iac::Overridable`: path, line, `var.<name>` and whether a var file
  gives the value), file, first and last line, the key path of its block and, for a Compose service
  (#652), its image and build context with an overridable flag each (NULL elsewhere), and for a
  Crossplane object its `role` (#655, NULL elsewhere), and `subscription` (#658, NULL elsewhere), a JSON summary with source and target channel
  contracts, an inferred flag and the reason identifying the subscription/target, rule, root
  and overridable values (no payloads or filter contents), and `iac_references`, each reference's root,
  source and target addresses and `via` (the `var`, `local` or module output it went through), both
  rewritten root by root (`iac::refresh`) for the roots whose evaluation unit holds a Terraform
  file, var file or Compose file that changed; the CloudFormation and SAM resources of
  `iac/cloudformation.rs` (#653) share them too (a node per resource: its logical id as address, its
  type `AWS::...`, its template's path as root and path, its name with `*` for each unresolved part
  and the parameter defaults it went through as `overridable`), rewritten per template
  (`iac::refresh_template`) when that template's file changes or leaves (a Serverless service of
  `iac/serverless.rs`, #654, is a unit of the same kind: its functions are nodes of type
  `Serverless::Function`, with `handler`, `handler_overridable` and `events`, a JSON list of
  `iac::Resolved`, in the columns of the same names, and its `resources` section's nodes are those
  of a template), and told apart from the Kubernetes nodes by a `::` in the type (`iac::Owner`); the
  Kubernetes objects of `kube.rs` (#651) share them, told apart by the path of their file
  (`iac::forget`), and are rewritten whole when a YAML or JSON file that holds, held or may hold an
  object changes, with `iac_images` (each workload's images once its overlay applied its `images`)
  and `iac_unresolved` (the Kustomize entries not followed, with their reason). Only IaC nodes
  holding contract sides reach the export (#656). `Call::args`,
  `Call::kwargs`, `Call::uses`, `Call::arg_types`, `Def::returns`, `Def::return_values`, `Param::default`,
  `Field::default`, `Const::value` and `Const::ty` (and `Facts::rebound`, `mutable`, `mentions`,
  `annotated` and `bound`) serialize only inside `lang::with_values` (`skip_serializing_if =
  "lean"`), so the index stores and unchanged files come back from SQLite without them; the
  values cache (`index/values.rs`, ADR 0027) stores them, whole, per `valued` file, beside the
  index. An `interface` or `type` declaration is a `Const` too, so its
  name is stored (its members are not). `Code` records every file it needs but lacks as a miss, and
  every file of `full` it looks into (`Code::read`); `refresh` re-parses the files the last
  extraction read (`valued`) before its first pass, then the missed ones, and extracts again the
  packs that missed until none does. A contract kind that reads a
  value from a new place relies on this loop.
