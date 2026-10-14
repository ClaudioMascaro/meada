# Architecture map

Which module owns what, the seams between them, and what the code relies on. Verified against
`src/` and `tests/` of the v0.1 trunk; the code wins over this file, and a change to a seam
updates it. Terms are `GLOSSARY.md`'s; decisions are in `docs/adr/`.

## Flow

`Engine::open(dir)` finds the **workspace**, creates or recreates each **repository**'s **index**
in its own `.meada/index.db`, and ATTACHes every **peer** to the first repository's connection
as `peer<n>`. Every query (`export`, `search`, `context`, `trace`) calls `refresh()` first, then
reads each index through `index::export`, derives **links** from the stored contracts, and renders.
Nothing outside `.meada/` is written. Everything the engine returns is sorted: the same files
give byte-identical output.

## Modules (`src/`)

- `engine.rs`: `Engine` is the one query seam. The CLI, the MCP server and almost every test call
  it: `open`, `refresh`, `export`, `search`, `context`, `trace`, plus `status(dir)` (reads only,
  never indexes). It owns the `context` and `search` rendering; `trace` lives in `trace.rs`.
- `main.rs`: clap, with `index`, `status`, `export`, `mcp` only. `search`, `context` and `trace`
  are reachable through MCP and the library, not the CLI.
- `mcp.rs`: three tools (`search`, `context`, `trace`) over rmcp, each a thin call to `Engine`
  that reopens it per call (so it refreshes). Stdout is the protocol: log to stderr. No tool writes.
- `workspace.rs`, `config.rs`: `Workspace::discover` finds `Repository { root, identity, config }`
  from the `meada.toml` files (peers by `path` hint, then sibling clones; siblings that name a
  repository already in join, transitively). Peers not found go to `Workspace::missing`. `config`
  parses the v0.1 subset (`[[peer]]`, `[serve] hosts`) with `deny_unknown_fields`; keys track A
  adds are listed in `LATER` so the error says "not in v0.1".
- `repo.rs`: root, identity (normalized `origin`, else `local/<dir>`) and `uncommitted()`, all by
  shelling out to `git`.
- `index.rs`: SQLite schema, `refresh`, `export` and the export types. Freshness is a pipeline in
  `refresh`: stat (mtime/size, trusted only if older than `RACY` = 2 s), hash with BLAKE3, parse
  only changed files, resolve every file's calls in memory, rewrite rows only when a file's
  `resolved` digest changed, re-extract contracts for the whole repository.
- `lang/mod.rs`: the **language adapter**, `LanguagePack` (extensions, `parse` to `Facts`,
  `config_files`, `modules`), and the `Facts`/`Value` types. `lang/ts.rs` is the only pack
  (`.ts .tsx .js .jsx .mjs .cjs`). A pack sees one file at a time and never touches the index.
- `resolve.rs`: the shared resolution behind the adapter. `Resolver` connects calls to definitions (scope,
  imports, re-exports, `this.method`, member chains) and gives each a `Resolution`. `Code` folds
  constants and client wrapper arguments into values for contract kinds.
- `contract.rs`: `ContractKind` trait (`prefix`, `extract`, `link`), `KINDS`, `Level`, `Link`,
  `Names`. `http.rs` is the only kind: `fetch` consumers, Next.js app-router and Express
  providers, `normalize_path`, and the link rules (host, `base_url_env`, unique provider).
- `trace.rs`: `Graph` over all exports and links, depth-first `Walk`, chain footer.
- `render.rs`: token budget (`budgeted`, `cut`, `tokens`; 1 token = 4 bytes; `DEFAULT_BUDGET`
  2000) and the line helpers `Files`, `label`, `callee`, `unresolved`. Display only.
- `msg.rs`: the CLI message catalog (pt-BR and English, one arm per `Msg`). Only `main.rs` calls
  `msg::init`; `meada mcp` forces English, so the MCP surface and library tests are English.

## Identities and formats

- Repository identity: `github.com/acme/web` (lower-cased host and path, no scheme, user, port
  or `.git`) or `local/<dir>`.
- Symbol: `<repository>::<path>::<qualified name>` (`Class.method`, `outer.inner`).
- Call site: `<caller>::<normalized callee>#<ordinal>`; a top-level call's caller is
  `<repository>::<path>`. The normalized callee keeps identifier and member chains only; any
  other expression becomes `(<node kind>)`.
- Inline route handlers: a pack emits anonymous function arguments as `Kind::Inline` named
  `@<byte offset>`; `Names` renames the ones a kind claims to `<METHOD> <full path>` (`#<n>` from
  the second equal name) and folds the rest into the enclosing symbol.
- Contract: `<prefix>::...`, `http::GET::/users/{}`. A path parameter of any spelling is `{}`.
- Export JSON: `{repository, files, symbols, calls, providers, consumers, links}`. A call has
  `level` (`exact`, `inferred`, `ambiguous` or null) and `target`; an ambiguous call has
  `target: null` and a sorted `candidates`. `export` keeps only the links that touch the
  repository where Meada runs.

## Seams and what extends them

- New **contract kind**: implement `ContractKind`, add it to `KINDS`. `extract` reads `Code`
  (never the syntax tree) and returns `Found` with a `Site` (`Def`, `Inline`, `File`, `Call`);
  `link` receives the whole workspace's providers. A kind's rules stay in its own file.
- New **level**: add the variant to `Level` in `contract.rs` and keep the order strongest first,
  since `Ord` makes a chain's weakest link the max. `Level::name`/`parse` are the only place names
  live; `engine.rs` and `trace.rs` render through them.
- New **language pack**: add it to `PACKS` in `lang/mod.rs`, a fixture
  `tests/fixtures/conformance-<language>/` and one `conform(...)` test in `tests/conformance.rs`.
  A pack is done when that suite passes.
- New **config key**: add the field to `Config`, delete it from `LATER`. Parse errors name file,
  line and dotted key by re-parsing the TOML for spans (`config::describe`).
- New **MCP tool**: `mcp.rs`, one `Engine` method, and watch `tests/mcp.rs`: the tool list
  (descriptions plus parameter schemas) must stay within 1,000 tokens.
- New **CLI text**: a `Msg` variant with both languages; command descriptions are set at runtime
  in `main.rs` from `Msg`.

## Invariants the code relies on

- Determinism: all export lists are sorted, hashes are BLAKE3, no clock or random value reaches
  an answer. A refreshed index must equal a cold index of the same files (`tests/freshness.rs`).
- `SCHEMA_VERSION` (`index.rs`) bumps on any change to `SCHEMA` or to the facts a pack stores. An
  index with another version, or an unreadable one, is deleted and rebuilt, and `status` says so.
- The index holds no source text and no values. `Call::args`, `Param::default` and `Const::value`
  are `serde(skip)`, so unchanged files come back from SQLite without them. `Code` records every
  file it needs but lacks as a miss, and `refresh` re-parses those and extracts again until none
  is missed. A contract kind that reads a value from a new place relies on this loop.
- Links are never stored. They derive from the stored provider and consumer contracts when a
  query runs, so a change on either side needs no relinking.
- One index per identity: a second clone of a repository already in the workspace is skipped.
- `context` shows span text only while the file's BLAKE3 still equals the indexed hash; on a
  mismatch it forgets the file's mtime, refreshes and retries once.
- Facts resolve only among files of the same pack, and inference runs only when resolution
  fails; an import from outside the repository is never inferred.

## Tests (`tests/`)

- `tests/common/mod.rs`: `fixture_repo(case, fixture, origin)` copies `tests/fixtures/<fixture>`
  to `$CARGO_TARGET_TMPDIR/<case>/<fixture>` and makes it a git repository (`init`, `origin`,
  one commit). `case` must be unique per test, since tests run in parallel. `chain(case)` builds
  the three-repository chain as siblings. `git()` pins config, author and dates; `age()` pushes
  mtimes into the past so a later edit always changes them.
- **Golden files** live in `tests/golden/<fixture>.<what>.{txt,json}` and are compared with
  `assert_golden(name, actual)`. `MEADA_BLESS=1 cargo test` rewrites them; the diff is the
  review. `.gitattributes` marks fixtures and goldens `-text`, since BLAKE3 hashes and byte
  ranges are in them.
- Test files by subject: `conformance` (pack), `index` (export, schema rebuild, writes only in
  `.meada/`), `query` (search/context), `workspace` (peers, status, config errors), `urls`
  (constants, wrappers, Express, inferred links), `trace`, `freshness` (cold vs refreshed),
  `cli` (the binary under pt_BR, en_US and de_DE `LANG`), `mcp` (the binary over stdio).
- A new behavior gets a fixture repo plus a golden; reuse a fixture before adding one. Fixture
  `meada.toml` files declare the peers and `serve.hosts`.

## Known ceilings

Each is marked `ponytail:` at its site unless noted.

- `engine.rs`: one connection attaches every peer, so a workspace reads at most 10 peers
  (`MAX_PEERS`, SQLite's attach limit). `search` scans every symbol.
- `index.rs`: every contract is extracted again and every call resolved again on any change
  (rows rewritten only when they differ).
- `workspace.rs`: a sibling with an invalid `meada.toml` is skipped silently.
- `resolve.rs`: wrapper expansion walks every simple path up the callers; a parameter captured by
  a closure is unknown.
- `lang/ts.rs`: tsconfig `extends` and nested configs are not followed. `require` and
  `module.exports` are not imports, and `new X()` is not a call (no `ponytail:` marker).
- `http.rs`: an Express app or router passed as a parameter, `router.route(path)` chains and
  `require` are not followed.
- `repo.rs`: a path git still quotes (tab, newline or quote in the name) is skipped by
  `uncommitted()`.
- No `busy_timeout`: two concurrent refreshes of one repository may hit `SQLITE_BUSY` (no marker).
