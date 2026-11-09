# Engine, index and queries

- `engine.rs` (`Engine::refresh`, #443): the peers' connections are opened once, with the engine, and
  every query refreshes and reads through them. A peer whose index schema changes while the
  process runs (another Meada version rebuilding it) is not rebuilt mid-session: the engine keeps
  the file it opened, and the rebuilt one is read from the next process start. Reopening a peer per
  query is what #443 removed (it split the refresh from the read); a mid-session reopen would
  have to swap the handle `exports` reads, and the ticket left it out.

- `engine.rs`: `search` scans every symbol. Kind `duplicate` folds no nested groups (a duplicated
  class and each of its methods are groups of their own) and has no minimum span size (#254).

- `index.rs`: a refresh keeps the rows of a group of packs that resolve together only while no
  file of it changed and nothing every group reads moved (config files and manifests, Meada's
  version, the file list, the exports of every group): a file added or deleted anywhere, or an
  edit that changes what a file exports, extracts and resolves every group again. A group holding
  a changed file is extracted and resolved whole, and every file of it whose values the last
  extraction read (`valued`) is parsed again: on this repository, a one-line edit of a Rust file
  parses the 220 Rust files of the crate's module trees again (2.0 s of a 3.3 s refresh), since
  the actix-web and Rocket reader reads every call's values (`Web::read`, under one `ITEM_WORK`
  budget spent in walk order) and every file's mentions (`Web::mentioned`). Narrowing a group per
  file needs each kind to say what its contracts read (#391). A dev build that changes a kind
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

- `index.rs` (`refresh_stamped`): a `package.json` or document that cannot be statted or read
  (it vanished after the walk, or is unreadable) has no row instead of failing the refresh; a
  `[serve] documents` entry in that state is then reported as missing.

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
