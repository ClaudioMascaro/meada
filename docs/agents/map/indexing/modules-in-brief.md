# Modules in brief

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
  (`Msg::WorkspaceOwnerStatic`), and an `added_by` or an `added_by_time` likewise
  (`Msg::WorkspaceAddedByStatic`);
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
