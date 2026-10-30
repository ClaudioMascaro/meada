# Test files and fixtures

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams, invariants and Known ceilings. The code wins over this file, and a change to these
modules updates it.

Reviewing a change to an `*.export.json` golden: `cargo run --example golden_diff` (no argument
diffs every export golden changed in `origin/main...HEAD`; see `examples/golden_diff.rs` for the
`--range` forms) prints added, removed and moved entries per section instead of the raw JSON.

- `ui/tests/operations.test.ts` projects operational API fixtures into fleet diagnostics,
  retention/bundle metadata, repository provenance, cached credential lifetime, token-check
  outcomes and the admission page's fields, the identity provider's type beside the issuer. `operations.test.tsx` exercises the five pages through their DOM and HTTP client,
  including bilingual accessible controls and the pasted token's lifetime.
- `ui/tests/actions.test.ts` checks the operator/operations gates, eligible generation actions,
  shell-quoted commands and exact u64 recovery floors. `action-pages.test.tsx` exercises those
  actions through the DOM and HTTP client: no write before confirmation or after Escape,
  named holder refusals, successful writes followed by fleet reload, missing bootstrap reads,
  and bilingual keyboard controls. `ConfirmDialog.test.tsx` checks focus restoration,
  cancellation and the busy state through the native dialog's DOM boundary.
- `scripts/compose-check` checks the local, generations and explicit console profiles without
  starting them, including parsed bootstraps, independent console storage and proxy admission.
  `scripts/helm-check` covers default and enabled console resources, optional Ingress, private
  CA and OIDC values, negative module/operation combinations and managed refresh ordering.
- `src/package/hostile.rs` (unit, `cfg(test)`): the table `probes` of the readers of manifests and
  configurations, a row each for `package.json`, `tsconfig.json`, `pyproject.toml`,
  `requirements.txt`, `Cargo.toml`, `go.mod`, `pom.xml`, Gradle builds, `.csproj` and
  `Directory.Build.props` (also `Nuget::inherit`), `msbuild::read` and `props`, Spring's
  `application.properties` and `.yml` (also placeholders) and `appsettings.json`, and the walks
  `Maven::entries` and `Nuget::entries` over modules and projects that pass on to each other.
  Each row builds a file within `lang::MAX_BYTES` (many keys, deep nesting, wide fan-out), reads
  it on a thread of its own and answers the size of what came back; the test holds every row to
  30 s and four times `MAX_BYTES`, and names the rows that missed. A new reader adds its row.
- Test modules of `tests/it/` by subject: `decorations` (the `decorations` fixture: Java, Kotlin, C#,
  Python and Rust calls that decorate a definition carry `CallSite::decorates`, ordinary calls do
  not, and `context` lists them under `annotations:`), `docs` (the repository map against the code:
  `ponytail:` limits in Known ceilings, entry point kinds and process marks in their lists),
  `conformance` (pack; for Python also its HTTP providers and consumers
  and its message channels, over `conformance-python` (`messages/`), `python-mounts` and
  `python-django`; for Rust, `conformance-rust`: a Cargo workspace whose members are `api`
  (`src/main.rs` with `mod.rs`, `<name>.rs` and `#[path]` modules, inline modules holding a
  `mod x;` and a `#[path]` one, `self::`, `super::` past a module's own name and `crate::`, a
  `let` named like the module a path names, `src/bin/tool.rs`, `src/bin/sync/main.rs`, a
  `[[bin]]` at `tools/admin.rs`, an inherent `impl` in another file than its struct, a trait's
  `impl` and a default body, two traits giving `label` and two `impl From<_>` blocks
  (`ambiguous`), a `#[cfg(test)] mod tests` that sees the file's `Store` only through a
  `use`, a `println!` whose call is not indexed, a closure, and `orphan.rs`, which no `mod`
  reaches; every `use` form (`crate::`, `self::`, a group, a rename, a crate name, a glob,
  `use super::*` taking a private item), a `use` that binds in an inline module or a function
  alone, a `pub use` chain (`ring/c.rs` to `d.rs` to `e.rs`), a re-export cycle
  (`ring/x.rs`, `ring/y.rs`), mutual aliases, a glob beaten by an explicit import or a local item, a parameter hiding a
  `used` name, a top-level `use` unseen in an inline module, a module and an item of one name
  (`launch`); the `exports` table read back over `rusqlite`: a binary crate has no row), `crates/billing` (`[lib] path`, a `pub use`,
  a call to another member) and `crates/ledger-core` (a library reached by its name with an
  underscore from a crate that lists it as a dependency, a `pub(crate)` item that stays inside
  it, a `pub` module holding `pub` and private items and a `pub use` of a private module's
  item, a private module and an item of one name, `mod meter; pub use meter::meter;`), and `crates/scratch`, which the workspace's `exclude` names and is a crate all the same (a call to a crate it does not list stays external);
  `freshness` moves a `mod`, declares the orphan, adds a module, drops the `[[bin]]`, edits a
  `pub use`, moves an item between files and renames a package, and
  exports it with the walk reversed), `xlang` (a TypeScript and a Python repository linked both ways, over HTTP
  and over a Kafka topic: exports, `context`, `trace`; and a TypeScript and a Rust repository, `xlang-rust-web` and `xlang-rust-api`, linked both ways over HTTP through `base_url_env` and a declared host: exports, `trace` down, up and from uncommitted changes, `freshness` editing, adding and deleting `.rs` files; and a TypeScript and a Java one, `xlang-java-web` and `xlang-java-api`, the Java bases from Spring `@Value` fields and a constructor parameter (a placeholder, a literal URL, a placeholder with a default a peer serves and one no peer serves) and `System.getenv`, and a Kafka topic produced in TypeScript and consumed by a `@KafkaListener` whose topic the configuration names: exports, `trace` down (also from the producer, `xlang-java.trace-channel-down.txt`), up and from uncommitted changes, `freshness` editing, adding and deleting `.java` and configuration files, and a configuration edited alone), `index` (export, schema rebuild, writes only in `.meada/`), `self_index` (a copy of this repository's tracked files, indexed in about 6 s in debug: no call from `src/` reaches `tests/fixtures/`, and `Resolver.call` has an `exact` caller), `hostile` (per pack, repositories of pathological files sized past the limits of Known ceilings: deep nesting, long chains, wide files and manifests, values traced through chains and lattices of constants, wrappers, parameters and return values; each indexed on a thread with a small stack within a time budget, so a recursion with no limit aborts the run and a loop with none fails it; `--nocapture` names each case as it starts; and one test over the table `chained`, a row for each pack of `PACKS`, which indexes a chain of 80 locals each holding a call on the one before, from a receiver typed outside the repository and from a type of it, side by side within the budget: a pack that reads a result twice per link misses it, as Java's and C#'s did before `result_of` read exact targets alone), `query` (search/context), `partial` (#396: every `search` kind, `context` and `trace` both ways over the fixture workspaces and every fixture alone, a sample of each repository's symbols, call sites and contracts and every process, reading the rows of the answer and every row, `engine::with_every_row`, give the same bytes), `workspace` (peers, status, config errors, an `owner` key in a `meada.toml` as an unknown key), `manifest` (server mode's workspace manifest: `index`, `status` and `export --repo` with `--workspace`, the refused entries, manifest errors, entry blocks configuring mirrors with no `meada.toml` and ignored under one, decisions replayed, reads that write nothing and see the index until `meada index` refreshes it, nothing written into a mirror, refreshed equals cold, `[workspace]`'s unknown key and source, `source = "file"` as before, `source = "store"` naming `generation-pointer` without it and its sections or `[[repository]]` with it; `[postgres]` naming the `postgres` feature without it, and with it each key error (an unknown key, `url_env` with `url_file`, a `schema` with an uppercase letter, a leading digit or 64 characters, `pool_size = 0`, a zero timeout) naming the file, the line and the key in `index`, `status` and `postgres migrate`; an `owner` in a static manifest's entry naming the file, the line, the key and `source = "store"`, and each `owner` rule it breaks; with `oidc`, `type = "azure"`, `type = 1`, `[oidc.entra]` with no `type` and under `generic` and `okta`, `overage = "directory"` and `"always"`, an unknown key in `[oidc.entra]` and `[oidc.github]` each naming the file, the line and the key, never the value; `[oidc.github]` (#331) without `[postgres]` naming both, in a build without `postgres` naming the feature, without `login_claim` or `max_age_seconds`, with an empty `login_claim`, `max_age_seconds = 59`, an uppercase `host`, repeated or no `roles` and an unknown key, each naming the file, the line and the key or the section, the table alone accepted without `identities_claim` or `groups_claim`, and `github-app roles` without a `[[github_app]]` block for its host (none, or one for another host) naming it; without `directory`, `[directory]` naming the feature, and with `directory` and `oidc`, `[directory]` without `[postgres]`, without `[oidc]` and without both, with both token keys, with an unknown key, with `user_attribute = "email"` and without `user_claim`, each naming the file, the line and the sections or the key, never a value; with `snapshot`, a bad `[snapshot] node_name` naming the file, the line and the key), `gateway`
  (gateway prefixes), `urls` (constants, wrappers, Express, inferred links, routes of every method), `trace`, `processes`
  (entry points and `trace` from `process::...` over the chain: the processes each export
  carries (`chain-<repo>.processes.json`), a route process crossing into a peer, a channel handler's process, a cycle, an
  export process, an ambiguous call, a missing peer, a budget cut), `community_queries`
  (`search` kinds `community` and `process`, and the community and processes lines of `context`
  over the chain: processes in three repositories, `community: none`, a decided link that counts
  and an ambiguous call that does not, the processes line cut under the budget), `duplicates`
  (`search` kind `duplicate` over `duplicates-a` and its peer `duplicates-b`: three exact copies in
  one group, the whitespace near copy left out, groups by span length, a match by path, and the
  export's `hash`), `decisions`
  (replay, parse errors), `stale` (stale and waiting decisions in `status` and `export`), `decide`
  (the binary in human mode over scripted stdin), `decider` (decider mode against the stub),
  `freshness` (cold vs refreshed, also an edited zod schema dropping a field, edited calls moving
  every repository's communities and processes, and the chain's export byte-identical under
  `meada::with_reversed_walk`), `items` (open items and their `search` goldens, fixtures
  `open-few` and `open-generated`), `cli` (the binary under pt_BR, en_US and de_DE `LANG`;
  with `oidc`, the identity provider's errors: an unknown `type`, `[oidc.entra]` under `okta` and
  an `overage` other than `ignore` and `directory`; with `oidc` and `directory`,
  `overage = "directory"` without `groups_claim` and without `[directory]` (#336); without `directory`, `[directory]` naming the feature; with
  `directory` and `oidc`, `[directory]` without `[postgres]` and `[oidc]`, with both token keys
  and with `user_attribute = "email"`; without `postgres`, `[postgres]` naming the feature and no `meada postgres`; with it, a
  `[postgres]` key error, the URL refusals (an unset variable, a query, a Unix socket) and,
  against the local Postgres, both version refusals; with
  `refresh-lock`, the operations' refusals: a malformed id, no `[refresh_lock]`, a generation
  not retained, the current generation dropped, a full kept set, a lock held by a named or an
  unknown holder; without it, no `snapshot pointer`
  or `snapshot fence`; `[workspace]`'s errors and, with `generation-pointer`, `source = "store"`
  without its sections or beside `[[repository]]`, `index` without a pulled version naming
  `meada workspace pull`, `mcp --listen` naming `--generations`, and `meada workspace`'s
  refusals of a malformed id and of a manifest without `[generations]` (`pull` included); with
  `refresh-lock`, `workspace apply`'s malformed id and missing `[refresh_lock]`, and against the
  services `apply` of a version not stored, `pull` with no current version, below the current
  file's fence and already current; with `snapshot`, the `[snapshot] node_name` error in both
  languages; `owner`'s and `added_by`'s errors and the static manifest's `owner` and `added_by` errors in both languages; an `added_by` stored, shown after the owner, pulled and refused when malformed, #330),
  `mcp`
  (the binary over stdio), `server` (`meada mcp --listen` as a child process over the chain's
  mirrors, through rmcp's streamable HTTP client and plain HTTP requests: the stdio tools and
  `trace` byte for byte, the 404 listing, the `uncommitted` refusal, the behind line before and
  after `meada index`, two stateless servers answering alternate calls, the refused start, and
  `Origin` refused with 403 before the 404 listing), `snapshot` (the first of the five cloud
  module modules, ADR 0012, each built with its feature; a test that needs Valkey or an
  S3-compatible server, `MEADA_TEST_VALKEY_URL` or `MEADA_TEST_S3_ENDPOINT`, prints why and
  skips without it, or fails with `MEADA_TEST_SERVICES_REQUIRED=1`; here `snapshot publish`
  and `snapshot pull`: a republish or a second machine uploads nothing, the refused publish, no
  secret in an authentication failure, no redirect followed, `[snapshot]` errors, a pull that
  keeps the previous generation, corrupted and hostile generations (bundles, tar entries, ids,
  identities, index schemas, a pointer and a document past their bounds) changing nothing, a
  pulled mirror that runs nothing its bundle configured, a nested repository refused;
  `snapshot/tls.rs` runs binary publish/pull through a TLS front of the local store with a
  private CA, refusing absent, wrong, empty, invalid and oversized CA files without exposing
  fixture secrets),
  `serving` (`mcp --listen --generations`, `status --generations` and `export --generations`:
  the chain goldens from a pulled generation, a swap between two calls, the probe and the start
  depending only on the node, the fixed 503, an index naming files outside its mirror, the same goldens, header and `_meta` from a node whose companion writes heartbeats),
  `generations` (the heartbeat of each `pull --every` round: its fields equal the node pointer file, the held directories, the readiness and the version, its TTL of three periods and the node set scored by the expiry, a swap writing the new generation and fence, a failed round and an unreachable Valkey (one warning, no password, nothing else changed), expiry and the next heartbeat removing the stale name, no heartbeat from a single `pull` or without `[generations]`, the host name by default; the generation pointer and pins: a pin answered on every node until it is no
  longer retained, the 404 of a pin not held, `Origin` refused before a pin, every id and fence
  from Valkey checked, a retained list longer than `keep` plus `KEPT_MAX`, a retained
  generation that fails to pull, the probe reading Valkey,
  `[generations]` errors, `snapshot pointer set` rolling two nodes back to G1's goldens until
  the next publish, a generation not retained and malformed ids refused, a generation kept by
  `pointer retain` held and pinned on two nodes past `keep` until `pointer drop`; the workspace
  versions: `workspace import` of the chain's manifest with owned and ownerless entries storing the owners in the document, `workspace show` printing each owner right after `path` and `import` reading that output back to the same id, a stored document with a malformed owner listed as malformed and refused by `show` and `pull` with the pulled directory unchanged, an `owner` error refused before any connection in both languages; `workspace import` of the chain's manifest storing `workspaces/<id>.json` with every
  field and its metadata, the same entries again uploading nothing and answering the first
  author and message, two entries swapped giving another id, an invalid file refused with its
  file, line and key before any connection, its other sections named as not imported; `workspace
  list` newest first, the current one marked once `{<key_prefix>}:workspace` names it, a
  malformed pointer refused, a corrupted and a forged document listed as malformed; `workspace
  show` of entries with blocks read back by `import` to the same id, and with no current
  version naming `meada workspace apply`; `workspace pull` refused with no current version,
  writing the document and the current file, current on a second run, swapping after the
  pointer moves and keeping only the current version and the one it replaced, refusing a
  pointer whose fence is not above the current file's; a malformed pointer id or fence, a
  document of another id, one whose id does not derive from its entries, an oversized one and
  one with invalid entries each refused, the directory unchanged; `index` over a pulled version
  opening no connection to its `[snapshot]` and `[valkey]` ports, the export goldens after it (the pulled version holds owners and `added_by` logins);
  with the chain stored, applied and pulled, `index` and `export` giving the export goldens,
  `status` the file's report, `publish` recording the version in the document's `workspace`
  field with the ids of a file's publish, the stored document unchanged by a file's publish and
  after another version and after one that only changes owners or only `added_by` (the document still naming the first version), no field from a file's new generation, a node pulling and serving it
  and refusing a field that is no version id; and without a pulled version `index`, `status`,
  `export` and `snapshot publish` naming `meada workspace pull` and `mcp --listen` naming
  `--generations`, while the `--generations` commands serve),
  `refresh_lock`
  (`snapshot lock`: the child terminated past the TTL, killed past the grace, stopped when the
  lock changes hands, a SIGTERM reaching it, a stale fence refused, a lost fence stopping a node
  with the recovery named in both languages, `[refresh_lock]` errors; the operations: each
  refused while a lock holds, naming the holder, the holder's fields, TTL, renewal and release,
  no lock or holder left by an operation, `retain` and `drop` changing only the retained list
  and the kept set (already retained, appended with its fence and counted by `fence recover`,
  not stored, past `KEPT_MAX`, current, not retained), a stuck fleet moving again after `fence
  recover`, the
  highest recorded fence and the `--at-least` floor, never lowered, live, expired and malformed
  heartbeats written by hand; `workspace apply` moving `{<key_prefix>}:workspace` with a fence
  above the previous one and rolling back with a new one, refused for a version not stored,
  while a job holds the lock and with the fence lowered, `fence recover` counting the workspace
  pointer, and a malformed id refused before any connection; `apply` of versions that set, change and clear owners and touch an ownerless entry, and of versions that set, change and clear `added_by`), `postgres` (the data layer, ADR 0016, feature `postgres`; a test that needs Postgres, `MEADA_TEST_POSTGRES_URL`, prints why and skips without it, or fails with `MEADA_TEST_SERVICES_REQUIRED=1`, each in a schema `meada_t<pid>_<case>` of its own: each URL refusal (another scheme, a Unix socket path, a query, `sslmode=disable`, a zero port, an unset variable, an unreadable file, a refused URL in a file) naming `postgres.url_env` or `postgres.url_file` and neither the URL, its password nor its host in `migrate`'s stderr or `status`'s line; `migrate` without `[postgres]` naming the manifest, and `meada index` with it opening no connection; `migrate` on an empty schema printing the binary's version, a second run printing it again with the schema unchanged (its columns and the version row's `xmin`) and no table outside it, a schema the operator created beforehand migrated; a run waiting on the advisory lock the test holds, past a one-second `statement_timeout_seconds`, writing nothing until it is released, and two runs started together both succeeding; with `MEADA_POSTGRES_VERSION_MAX`, an absent schema making `status` print the unreachable form naming `meada postgres migrate`, a higher version the form naming both, and a capped `migrate` refusing to lower it, naming both, the schema unchanged; `status`'s `postgres:` line, unreachable against a closed port and reachable against the local Postgres, with the exit code it has without `[postgres]`, indexed or not, and a loopback `ca_file` asking for TLS), `oidc` (`[oidc]`
  against an issuer and a JWKS served on loopback: a token admits what its claim lists and leaks
  nowhere, a rotated key, an oversized JWKS refusing the start, a pinned path admitted as its
  identity; the identity provider's `type` (no key, `generic`, `okta`, `keycloak`, and `entra`
  with and without `[oidc.entra] overage = "ignore"`) answering every case, the metadata (of a pin
  too), the 401s, 403s and 404 lists, the groups' grant, a token in overage with no groups and a
  key added after the start, with the same status, headers and body and the same stderr, which
  holds no token part, group or claim value, as the manifest without the key, and an oversized
  JWKS refusing the start alike; in `oidc/overage.rs` (with `ui,directory`; the debug knob
  `MEADA_OVERAGE_TTL_MS` shortens the answers' lifetime to three seconds, and a loopback relay
  the test closes stands for Postgres stopped), Entra overage through the directory on nodes,
  the directory fed through the console's SCIM endpoint: a token with `_claim_names` and no
  `groups` reading the identities `[oidc.groups]` maps its directory groups to, with the claims
  grant beside them and nothing under `overage = "ignore"`, an inactive, an unknown and a
  groupless user reading nothing, a token with `groups` or a `_claim_names` that is no object or
  maps to a non-string never looked up (no line with Postgres closed), an answer, of no groups
  too, kept for the TTL and no longer, a cached subject keeping its groups with Postgres stopped
  until expiry and an uncached one having none, each failed lookup one stderr line, readiness
  unchanged and a schema version newer than the node knows named, group values failing the owner
  name rule skipped with a line naming `directory_groups`, `userName` matched ignoring case and
  `externalId` exactly, and no token, object id, group or URL on stderr), `github_app` (`github-app credential` and `throttled` against a GitHub API served
  on loopback that verifies each App's JWT against the fixture keys of
  `tests/fixtures/github-app/`: a minted token cached and reused, two Apps taken in turn, a
  throttled installation skipped until its retry time, a mint answered 429 passing to the next
  installation, a token minted again after its cache runs out or `erase`, an uncovered host or
  owner, a new block and new installations after the refresh, `[[github_app]]` errors, no
  request from `mcp --listen`, and no key, JWT or token on stderr; and in
  `tests/it/github_app/roles.rs`, the role check `meada github-app roles` (#331) against the
  fake's collaborators listing, each connection on a thread of its own so one listing can be
  held, the local Valkey and the local Postgres in a schema of each test's own, observed through
  the command's line, its fence refusal and the readers, never the tables: every page of a
  250-collaborator repository, each request with `affiliation=all` and `per_page=100`, a login
  checked ignoring case, a 404 and an uncovered owner each a warning and no rows, the exclusions
  file holding the version id alone, a second run a new set id; with `source = "file"` no
  exclusions file, `MEADA_REFRESH_FENCE` required under `[refresh_lock]`, a lower fence refused
  naming the higher (also in `pt_BR`), a 403 without `retry-after` passing to the next
  installation, a 429 with it leaving every one throttled and the run failing naming the
  identity and the status, every one marked failing before any request, a 503 failing alike, the
  set staying each time; an addition whose adder GitHub lists with no role, or a role outside
  `roles`, excluded with its reason and no login, in the file, absent from `show --pulled`, not
  indexed, absent from `status` (a peer naming it misses it) and `export`, an entry without
  `added_by` never excluded, and both included once GitHub lists an accepted role, a custom one
  of `roles` included; every reader (`index`, `status`, `export`, `snapshot publish`, `show
  --pulled`) refusing at the first pull with no file, after a new pull with the old file, and on
  each malformed line naming the file and the line, `index` with a good file recording no
  connection on the store's, Valkey's and Postgres's loopback listeners, `--pulled` with a version
  a usage error and without `[oidc.github]` the version whole (no service needed); a failed
  check, a commit whose file cannot be written and a check killed mid-listing each leaving no
  file a reader admits, the next success writing it, and a low fence held mid-listing while a high
  one completes refused at its commit, the high check's file kept; a schema never migrated and
  one a newer Meada migrated refused; a `Link` to another origin, with a user, to another
  repository or to a page listed already failing the run with no request to the other origin;
  and the job's refresh under `meada snapshot lock` against the local store: an excluded
  addition absent from the published generation, a refresh stopping at the role check
  publishing nothing, `show` and `import` carrying `added_by` to the same id, an adopted entry
  published once its index can be written (the failed index keeping the current generation,
  #316), and an attribution-only change publishing the same generation), `ui` (`meada ui` as a child
  process over a `[ui] dir` it writes, with a `meada-ui.json` naming this build's version, in
  plain HTTP over raw sockets so no path is normalised: the `config`, `session` and `manifest`
  documents, the JSON 404, the `manifest` errors of a file broken after the start; the
  `--listen` forms and both flags required; the `Host` 403 before the `Origin` 403, a write
  without `Origin` refused; static files with their content type, cache rule and security
  headers, `..`, `%2e%2e`, an empty segment, a symbolic link out of the directory and an unknown
  extension refused, no listing, an unknown path answering `index.html`; no cookie or
  `Access-Control-*` header on any answer; every start refusal listed at once; a
  `password_file`'s content in no answer, with `valkey`; and an answer in flight, blocked on a
  FIFO, received after SIGTERM, then exit 0; in `ui/playground.rs`, the playground and the
  exports over the chain: started before `meada index --workspace`, both a 503 naming it while
  `config` and `manifest` answer, then with no restart `search`, `context` and `trace` equal to
  the chain goldens with `generation` null and every export equal to its golden, `index_dir`
  byte-identical after them; with `snapshot` (skipped without the store), the same before and
  after an init `meada snapshot pull` into the console's `local_dir` (naming `meada snapshot
  pull`, then the goldens with the current id, by default and named), a second pull's
  generation answering the next query, the retained one when named, an unheld generation a 404
  naming both held ids, `local_dir` byte-identical; in both, `trace` with `changes:
  uncommitted` refused with the node's text, an unknown repository and an unheld generation a
  404, a body over `QUERY_MAX` a 413, malformed JSON, an unknown tool, arguments not the tool's
  or an unknown key a fixed 400, a query without the console's `Origin` or with a foreign one
  a 403; in `ui/reads.rs`, local repository freshness and commit provenance, unknown commits
  for absent, oversized or stale sidecars; explicit missing modules; four blocked readers
  timing out without releasing worker permits early, bounded overload and independent basic
  routes; non-regular credentials refused before open; with `generation-pointer`,
  two pull companions and the console's own companion, the live fleet, generation/workspace
  documents, retention and kept marks, modification time, schemas from held copies, malformed
  names/ids/fences excluded from diagnostics and both stuck recoveries; with `github-app`,
  the cached installation/token-expiry/throttle view against the module's loopback API without
  any additional API request or private key read; with `oidc`, in `ui/oidc.rs`, against the `oidc`
  module's loopback issuer, which logs every request: no token, an expired one, the nodes'
  audience, another issuer and `HS256` get 401 with `WWW-Authenticate`, a missing scope 403, a
  valid token the viewer's `session`; static files and `config` (the issuer, `client_id`, the
  scopes) without a token, the issuer's origin in every answer's `connect-src`; the start
  refused without `[ui] client_id`, metadata or JWKS; `admission` with the metadata, the key
  ids, the last read, the groups table bounded by the grant and, after the JWKS fails, the
  failed refresh; `admission/check` per check for a valid, an expired, a wrong-audience, a
  forged and an unknown-`kid` token, the identities bounded by both grants, no request to the
  issuer and no pasted token in its log, a 413 past the cap and a fixed 400; a viewer's grant
  bounding the manifest's entries and its error text, whole for `*`; the playground and the
  exports under a grant, the goldens for `*`, a `WEB`-only trace and export with no peer
  outside the grant, the repositories outside it the 404 of one not served, a 401 without a
  token; `admission`'s `type`, `generic` with no key and `entra` when set, every read route,
  `session` and `admission/check` answering the same tokens alike under both, a token in overage
  with no groups; in `ui/oidc/overage.rs` (with `directory,refresh-lock`), a token in overage
  granted through directory groups, made `operator` through `operator_groups` and `team_operator`
  through a group owning an entry it reads, its `session` listing the groups, and none of it under
  `overage = "ignore"`; a token with `groups` or an odd `_claim_names` never looked up; an
  inactive or deleted user refused with 403 `deprovisioned` before its cached answer is used, on
  `session` and a write, and a viewer once reprovisioned in no group and the answer expired; the
  token check of a pasted overage token listing the identities its directory groups grant,
  none under `ignore`, with no request on the issuer; the admission routes
  absent without `[oidc]`; manifest peers and error text bounded even for an unlisted peer;
  operator groups and strict boolean claims, all five configuration errors, whole operator
  views despite restricted grants, and header-only refused writes proving bodies stay unread;
  in `ui/oidc/writes.rs` (with `ui,oidc,refresh-lock`), real pointer retain/set/drop, exact
  decimal fence recovery, save/CLI validation agreement, admitted author, workspace apply and
  catalog refusals, per-route caps, held-lock refusals and one escaped audit line per attempt,
  including unauthenticated refusals and tokens pasted into ids/floors; save422 exactly matches
  CLI diagnostics while its fixed audit outcome excludes JWT fixtures pasted into entries;
  malformed heartbeat
  MAX fences ignored by CLI and HTTP recovery, and workspace-pointer fences in fleet diagnostics;
  in `ui/oidc/team.rs` (with `ui,oidc,refresh-lock`, #324), the team operator: the `session`
  role and fields (`team_operator` with `groups`, `grant` and `teams`; a viewer for groups that
  own nothing or only entries outside the grant, with `operations = false`, with a file
  workspace, with no pointer, a pointer to a missing document and a malformed pointer; the
  global operator stays one), a team operator's views bounded by its grant with every role
  seeing `owner`, a save that edits, removes and adds entries (composed with the entries
  outside the grant kept in place, its subject as author) and its apply and rollback, two teams
  in one save, each refusal reason as a 403 `entries_refused` with nothing written and the lock
  free, an apply naming what the grant reads and counting the rest in `hidden`, an apply checked
  against the version current then, a team that removes its last entry turning viewer, the
  generation operations refused by role before the body is read, and the `as <owners>` stderr
  line;
  serialized workspace versions refused before storage even when their uploads fit the cap;
  managed workspace views and nested peers bounded by grants, the
  token check using current workspace identities; no token, claim value or identity on stderr), `resources` (resource `search`, `context`, `trace` start and `status`
  over the chain), `channels` (the `channels-api`, `channels-worker`, `channels-billing`
  fixture: kafkajs forms, fan-out, same-repository links, an unrelated `send`; amqplib queues,
  publishes through a binding of another repository, the empty exchange, queues named like the
  exchange and like an unbound key that no publish reaches, and a Kafka topic that shares a
  queue's name; the `channels-queues-api` and `channels-queues-worker` fixtures: SQS by queue
  name and NATS wildcards at `inferred`, also in `context` on a subject and on a pattern),
  `channel_queries` (`search`, `context` and `trace` on channels over the first fixture),
  `channels_jobs` (the `channels-jobs-api` and `channels-jobs-worker` fixtures: BullMQ queues,
  a `new Queue(name)` wrapper, and Redis streams in both client styles),
  `drift` (shape drift over `drift_chain`: the web export with every consumer shape and
  link drift, and `trace` up from `GET /api/orders` with `drift: missing` on the drifting
  consumers' lines only; an edited client wrapper changing its callers' drift like a cold
  chain is in `freshness`; the overlay's gateway Express routes and catalog Next.js routes
  return object literals, so the gateway and catalog exports carry shapes with source
  `literals`, `trace` up from those routes shows drift and not-checked reads, and `context`
  names the source, and a Next.js route answering through a helper and an Express one that may
  redirect are open at the root, while error answers in other forms (`sendStatus`, a literal
  status before `end`, `next(err)`, `new Response(null, { status: 404 })`) are left out, so
  the web's `price` and `reviews` show drift; the limits of `literals.rs` are unit tests there;
  a decisions file (`chain-drift-web`) links the unresolved `src/open.ts::rate` to a route with
  a shape (drift on a `decided` link) beside the ambiguous `ping` (none), golden
  `chain-drift.trace-decided.txt`; `trace` down from `src/stock.ts::price` ends its link line
  with the drift (`chain-drift.trace-down.txt`); the catalog's
  zod routes (`app/api/comments`) give shapes with source `zod`, `trace` up from them (golden
  `chain-drift.trace-zod.txt`) shows the drift of the web reads, and a document wins over a zod
  schema, which wins over literals; the reading rules and limits of `zod.rs` are unit tests
  there; the catalog's `app/api/batches` route and the web's `src/batches.ts` consumers
  (golden `chain-drift.trace-batches.txt`) destructure, iterate and map over the response,
  with the hidden names and the values handed to a function that are not followed; the
  catalog's `app/api/profiles` route and the web's `src/declared.ts` consumers (golden
  `chain-drift.trace-declared.txt`) read declared types (the removed `nickname` read through
  an interface cast, `exact`, and with plain member access, `inferred`), with the optional fields, the package
  type and the freshness of an edited shared type in `freshness`, the limits being unit tests in
  `declared.rs` and `lang/ts/types.rs`; the catalog's shared package and the routes and
  consumers importing it (golden `chain-drift.trace-shared.txt`) cross repositories through
  package imports, with an edited package and a package that gets its name back in
  `freshness`, the views, the levels and the limits (`PACKAGE_HOPS`, `GRAFT_BUDGET`) being unit
  tests in `shape.rs`, and the surface, `kept` and `export *` collisions in `package.rs`; the rules
  and the pattern depth limit are unit tests in `lang/ts/uses.rs`; `context` on the documented,
  zod, literals and drift-free routes (goldens `chain-drift.context-route-*.txt`), `search` with
  the kind `drift` (`chain-drift.search-drift.txt`) and `status` (`chain-drift.status.txt`, and
  `cli.status-drift.*` in `cli` for pt-BR and English) show it),
  `documents` (provider documents over `documented_chain`: export, `trace` to "documented
  only", `search` (routes and kind `document`), `status`, listed documents, `context` on the
  merged route with its documented fields, `context` on a route (documented only and merged);
  inline repositories written by the test (`repo`, `client`, `openapi` helpers): each document
  giving its own gateway forms under `[serve] prefix` and `strip`, a `[serve]` table with only
  `documents` and an unlisted gitignored document left out, a listed path through a symbolic
  link left unread; over
  the `documents-shapes` fixture, a repository with no code whose OpenAPI 3.0 JSON document
  has local `$ref`s into `components`, `allOf`, `oneOf`, an array response,
  `additionalProperties: true`, a cycle and an external `$ref`, plus a later
  `v2.openapi.json` listing one route again: the export with every shape and `status` with
  the external reference; freshness of an edited document and of a dropped `[serve]` is in
  `freshness`), `asyncapi` (AsyncAPI documents over `asyncapi-events`, a repository with a
  `[serve]` table whose `asyncapi.yaml` (2.6, `kafka-secure`), `billing.asyncapi.yaml` (3.0,
  `amqp`), `sensors.asyncapi.yml` (`mqtt`), `routing.asyncapi.yaml` (a server without
  `protocol`, a channel naming a server the document lacks), `serverless.asyncapi.yaml` (no root
  `servers`) and `legacy.asyncapi.json` (1.2) sit beside two
  code sides that merge with a documented one, and `asyncapi-services`, which names it as a
  peer and holds the code on the other side of each documented one: both exports with the
  channel contracts, shapes and links, the merge of a documented and a code side, `status`
  (protocol, parameterized channel, version), `trace` down to a documented consumer and up
  from a handler to a documented producer, `context` on documented channels with their payload
  fields, and `search` of channels and of documents). Table tests for discovery, format detection, routes, response selection, the
  schema-to-shape reader (`schema.rs`) and the YAML reader are unit tests in `src/document/`,
  with the hostile inputs each limit stops (server variables naming each other, 200,000 nested
  block sequences, a 10,000-long reference chain, one wide schema read for many routes).
- `tests/it/packages.rs` (spec #22): the package fixtures `pkg-{shared,web,api,mono,fork}` as
  siblings (`web`, `api` and `mono` name `shared` as a peer, `fork` names `mono`), through
  `Engine::export`. `shared` holds `@example/shared` and an ignored-by-rule
  `node_modules/left-pad/package.json`; `web` and `api` depend on it (`api`'s manifest has no
  name and a non-string range; `web` accepts the `shared` version, `api` does not (goldens carry
  `range mismatch` and `range unchecked`); `web` also names `express` and `left-pad`, outside the workspace);
  `mono` has `packages/{a,b,c}` (`b` on `a` through `workspace:*`, `c` on `a` with `^1.0.0`) and a
  `.gitignore`d `ignored/`, and `fork` holds another `@example/a`. Goldens `pkg-*.export.json`;
  a freshness test edits manifests in three repositories (`shared`, `web`, `mono`), and renames
  an export and an `exports` subpath of the kit, and compares the exports of all four with a cold
  set; a test shows an ambiguous package link is no open item. Imports (spec #22): `shared` also
  holds `kit/`, `@example/kit`, whose `exports` (conditions, a `./utils/*` pattern, a `null`
  `./internal/*`) and `main` point straight to sources, with a re-export, an `export *` cycle, a
  default and a class; `web/src/app.ts` calls it through a named, default, aliased, namespace,
  pattern and member import, plus a blocked subpath, an unexported name, an unlisted subpath,
  `express` and a name never imported (none cross), and the hidden names probe (a parameter, a
  `catch` binding, a destructuring and a `let` named like the import: no hop); `api/src/report.js`
  calls `greet` too; `mono`'s `a` has `main`, called from `b` (`workspace:*`, exact) and `c`
  (ambiguous with `fork`). `shared` also holds `@example/typed` (`types` to a source) and
  `@example/esm` (`module` to a `.mjs`, and subpaths read as paths, one through `index`), which
  `web/src/entries.ts` calls; the root package `@example/shared` ships build output, with an `exports`
  map into `dist` (conditions, a `./utils/*` pattern), a `tsconfig.json` (`outDir`, `rootDir`) and
  `src/`, which `web/src/text.ts` and `api/src/label.js` call (`inferred`, `import outDir`), and
  `@example/sealed` (`main` into `dist`, a `tsconfig.json` with no `rootDir`, two files exporting
  `seal`) is `ambiguous` with both listed; `web/e2e/` declares the kit in `devDependencies` and crosses, and
  `web/tools/`, whose own manifest does not declare the kit though the root one does, does not.
  `shared` also holds `@example/dts` (`types` to a declaration file that is no source, `main` to
  `index.js`: `exact`, `import main`), `@example/plain` (no entry field, an `index.ts`: `exact`,
  `path`) and `@example/bare` (no entry field and no `index`, one file exporting the name:
  `ambiguous`, `path`), called from `web/src/entries.ts`; `@example/prec` (`types` into `dist`,
  mapped through its own `tsconfig.json`, and a later `main` that is a source: the first field
  wins, `inferred`) and `@example/nest` (no entry field, with `index/` and `sub/` nested
  packages: `index/index.ts` is not its default and a subpath into `sub/` does not cross),
  called from `web/src/nest.ts`. The freshness test also asserts the calls through the renamed export and subpath no longer
  cross. Hidden names in the trunk: `conformance-ts-js/src/shadow.ts` (`catch`, `for ... of`,
  `let`, `var`, an arrow parameter without parentheses, nested and array parameter patterns, a
  same-scope constant and parameter, each never `exact`) and two value-flow cases in
  `http-fetch-param` (a `for ... of` hiding a constant URL, a `catch` hiding a parameter that
  defaults to `fetch`). Goldens `pkg-web.trace-down.txt` (from the call into `greet` and on to
  `shout`) and `pkg-shared.trace-up.txt` (to `web` and `api`). `search` and `context` (ticket
  T5): `pkg.search-package*.txt` (kind `package`, all and by name), `pkg.search-contract-shared.txt`
  (dependency entries placed at their manifest and field), `pkg-shared.context-greet.txt` (the
  `consumers:` of the kit's `greet`) and `pkg-shared.context-greet-flagged.txt` (the same after
  editing two ranges to `^3.0.0` and `latest`: `[range mismatch]`, `[range unchecked]`),
  `pkg.context-package-kit.txt` and `pkg.trace-package-kit.txt` (a package contract as a
  `context` subject, and as a `trace` start), and `pkg-mono.context-add.txt` (the consumers of
  `mono`'s `add`, both in its own repository).
  Ticket T4 adds `pkg-web.trace-down-outdir.txt` and `pkg-shared.trace-up-outdir.txt` (the calls
  through `@example/shared`); an invalid, hostile or edited nested `tsconfig.json` is tested the
  same way as a `package.json` (refreshed equals cold).
- `tests/it/pypi.rs` (spec #24): the PyPI fixtures `pkg-pylib` and `pkg-pyweb` as siblings
  (`pyweb` names `pylib` as its peer). `pylib` provides `Shared_Lib` (`shared-lib`, PEP 621, a
  `src/` layout: `shared_lib/__init__.py` re-exports `greet` from `core.py`, which calls
  `text.py`'s `shout`; `api.py` holds `fetch` and `Client.create`; `src/sl_extras/` is a second
  top-level module; `src/shared_lib/requirements.txt` takes none of its files),
  `tools/pyproject.toml` provides `shared.tools` (`shared-tools`, Poetry) whose modules are
  `tools/sl_extras/` and `tools/toolkit/`, and `js/package.json` is an npm `shared-lib`. `pyweb`
  depends on `shared-lib` in `pyproject.toml` (extras and markers) and `requirements-dev.txt`, on
  `shared-tools` in `requirements.txt` (with option lines, a comment and a URL) and in
  `worker/pyproject.toml` (Poetry, a dev group naming `shared-lib` not read), and its
  `ui/package.json` names `shared-tools` on npm (no link). `app/main.py` crosses `exact` through
  the re-export, a submodule reached as `shared_lib.api.fetch` and `from shared_lib.api import
  Client`, `ambiguous` through `sl_extras` (both distributions expose it), and not at all
  through a local `greet` or `json`; `app/rel.py`'s relative imports named after the
  distribution never cross; `tests/test_api.py` crosses `exact` through the root's manifests,
  though `tests/requirements.txt` names only `pytest`; `worker/job.py` crosses `inferred`
  through `toolkit`. Goldens `pkg-{pyweb,pylib}.export.json`, `pkg-pylib.context-greet.txt`,
  `pkg-pylib.search-package.txt` (the PyPI and the npm `shared-lib` apart) and
  `pkg-pyweb.trace-down.txt`; the freshness test edits both repositories' `pyproject.toml`, the
  root `requirements.txt` and adds `worker/requirements.txt`. Unit tests in `package/pypi.rs`
  (PEP 503 names, requirement lines and continuations, both readers, the limits) and
  `lang/python/pyproject.rs`.
- `tests/it/cargo.rs` (spec #30): the Cargo fixtures `cargo-lib` and `cargo-web` as siblings
  (`web` names `lib` as its peer). `lib` is a workspace of `shared-core` (`pub use text::{greet,
  shout}` of a private module, `pub mod api` with `Client::create`), `shared-extras` and
  `inner-app`, which depends on both by path. `web` depends on `shared-core` by version (`app`),
  through `workspace = true` and a renamed key (`worker`), on `shared-extras` by a renamed `git`
  key, on a `path` that lands in `lib` (`../../cargo-lib/extras`, linked) and one that lands in
  no workspace repository (`ghost`, not linked), and on `serde` (no link). The tests check each
  form's link and reason, every `use` into `lib` (`exact`, `; import crate name`), the path
  dependency that stays inside `lib`, a `git` URL of another repository that links to nothing,
  a private module that is not reached until it is made `pub`, a `[lib] name` and a
  `version.workspace = true` written into copies of `lib`, and the `cargo-fork` fixture (a second
  provider of `shared-core`, added as a peer) that makes the version dependency `ambiguous`.
  Goldens `cargo-{web,lib}.export.json`, `cargo-lib.context-greet.txt` and
  `cargo-web.trace-down.txt`; the freshness test edits the library's version, `app`'s
  requirement, the workspace requirement and a dependency key. Unit tests in `package/cargo.rs`
  (the reader, unreadable sources, the dedup, `workspace = true`, hostile manifests) and
  `package.rs` (`reaches`).
- `tests/it/gopkg.rs` (spec #25): the Go fixtures `gopkg-lib` (modules `kit`, with `text` and `api`
  packages and a `cmd/app`, and a nested `extras`) and `gopkg-web` as siblings (`web` names `lib`
  as its peer; its nested `worker` module requires `extras` with a `replace` to
  `../../gopkg-lib/extras`), and `gopkg-fork` (a second provider of `kit`, added as a peer, which
  makes the `require` and the import `ambiguous`). The tests check each `require` link and reason,
  every import into `lib` (`exact`, `; import module path`), a directory the provider lacks, a
  module outside the workspace, a local that hides the import, a name only a test file, a file built for another target or two files of the directory define, a directory of a nested module, and a `replace` by another module (no link), a module path with no dot, and the freshness of
  an edited `go.mod`. Goldens `gopkg-{web,lib}.export.json`, `gopkg-lib.context-greet.txt` and
  `gopkg-web.trace-down.txt`. Unit tests in `package/go.rs` and `lang/go/modfile.rs`.

- `tests/it/jvmpkg.rs` (specs #26, tickets #93 and #94): the Maven and Gradle fixtures `jvm-maven`
  (modules `models` with `Order`, `Repo extends Base`, `Event` with an opaque base, `Note` with
  a method named after it, `Ledger`, a second `example.shared.Stamp` and a test-only
  `Fixtures`), `jvm-gradle` (`kit` with a second `Ledger` in another package, and `tools` whose
  `main` and `test` source sets are literal directories), `jvm-client` and `jvm-gradle-client`
  as the dependents. Manifest links (build files) and imports into provider classes (exact,
  ambiguous by provider, by type and by overload, overloads merged over the provider-side
  family, `inferred` beside an opaque base, constructors, on-demand and static imports, two
  on-demand imports that both bring a class, a same-package class and a parameter or local
  that hide the import, test roots and outside types left out); an edited source set
  relinks like a cold workspace; `context` on the class `Order` lists the calls that reach its
  members. Goldens `jvm-*.export.json`, `jvm-maven.context-total.txt`,
  `jvm-maven.context-order.txt` and `jvm-client.trace-down.txt`. Unit tests in
  `package/maven.rs` and `jvm/gradle.rs`.

- `tests/it/swiftpkg.rs` (spec #29, ticket #124): the fixtures `swift-pkg-kit` (a `Kit` package
  with a library product of two targets, an executable product, internal declarations and
  overloads of `greet`), `swift-pkg-lib` (a `Shared` package also exposing `KitMoney`, and an `Examples` package that depends on its root), `swift-pkg-dyn` (a package whose name is computed) and
  `swift-pkg-app` (a `.package(url:)`, a `.package(path:)`, computed URLs, an outside
  dependency, a `Package.resolved` that must stay unread and names that hide the imported ones). Manifest links, imports narrowed by labels (a bare, a module-qualified and an
  unlabelled call, a constructor, a method on a local and on a typed parameter), the ambiguous
  module, `context` on a public function and method, `trace` down across the hop, and edited
  manifests refreshing like a cold workspace. Goldens `swift-pkg-{kit,lib,app}.export.json`,
  `swift-pkg-kit.context-{greet,hello}.txt`, `swift-pkg-app.trace-down.txt` and
  `swift-packages.search.txt`; `conformance-swift.export.json` gains the `Shop` package. Unit
  tests in `package/swiftpm.rs`, `package/swiftpm/manifest.rs` and `lang/swift/manifest.rs`.
- `tests/it/nugetpkg.rs` (spec #28, ticket #116): the fixtures `dotnet-lib` (a `PackageId`
  project `Example.Kit` with `Order`, and a `Money` project whose `AssemblyName` differs from its
  file) and `dotnet-app` (a `PackageReference` project, a `ProjectReference` one to the other
  repository and to a project of its own, an outside `Newtonsoft.Json`), and `dotnet-twin`
  (a second provider of the same type, for the ambiguous hop). Manifest links, `using` and
  `using static` imports, `context` on the type and a method, `trace` down across the hop, and an
  edited, renamed and added `.csproj` refreshing like a cold workspace. Goldens
  `dotnet-{lib,app}.export.json`, `dotnet-lib.context-{order,total}.txt` and
  `dotnet-app.trace-down.txt`. Unit tests in `package/nuget.rs` and `dotnet/msbuild.rs`.
- C# channels (spec #28, ticket #115): `conformance-csharp` gains `Channels.cs` and
  `Channels.Nats.cs` (a client of each library: Confluent.Kafka, RabbitMQ.Client with a
  default-exchange publish and a named one routed by a binding, AWSSDK.SQS, NATS.Client with a
  wildcard subject, StackExchange.Redis streams, Azure.Messaging.ServiceBus with a queue, a
  topic and two processors on it, and names read from the environment on both sides) and
  `Channels.Writes.cs` (a sender written more than once, one made for a parameter,
  `String.Empty`, `ConfigureAwait`, a consumer-first `BasicConsume` and two handlers on one
  event), checked by
  `csharp` (the definitions and the golden) and `csharp_channel_clients_link_their_producers_to_their_handlers`
  (every link with its level, and the unresolved sides); `xlang-csharp-web` gains `src/events.ts`
  (`announceShipment`, kafkajs) and `xlang-csharp-api` `Messaging/ShipmentListener.cs`, and
  `trace_down_from_a_typescript_producer_reaches_the_csharp_kafka_consumer` holds
  `xlang-csharp.trace-channel-down.txt`. Unit tests in `channel/csharp.rs` (each library, a
  repository type that hides the library's, two namespaces for one name, a name assigned twice, a
  wrapper, the client depth, every write of a sender) and in `lang/csharp.rs` (`await`, `+=`,
  one `name=` per write, a linear pass over thousands of them).
- **The loopback decider stub** (`tests/it/common/stub.rs`): `Stub::start(script: Vec<Reply>)`
  listens on `127.0.0.1:<free port>` (`Stub::url`, the value for `MEADA_DECIDER_URL`), answers
  the n-th request to `.../chat/completions` or `.../api/alpha/decisions` with `script[n]` (500
  once it runs out, 404 on other paths) and records every request it receives
  (`requests() -> Vec<Request {method, path, headers, body}>`, `header(name)`, `json()`).
  `Reply::chat(model, content)`, `Reply::ok(json)`, `Reply::status(code, body)`.
  `tests/it/decider.rs` runs the binary with only the decider variables it sets and asserts the
  key is in no output; `--show-request`, `--list`, human mode, `index`, `export`, `status` and
  the MCP server leave the stub with zero requests. Answer cases (malformed, threshold,
  `--min-p`, `--dry-run`) script the stub per test; a malformed first answer must leave the
  second item written.

### Immutable workspace import and managed status

`generations/imports.rs` uses the binary and real local RustFS: two concurrent imports see
missing through a reply barrier, but only one conditionally creates the immutable document;
the other reports the first author, time and message. A local workspace-pointer swap during
status's informational Valkey probe proves the report and readiness exit code share one
managed workspace read. CLI import is also checked with `generation-pointer` alone.

## Workspace editor and shared validation (#310)

`meada-core/tests/workspace.rs` exercises the public diff seam: entry/key order, unchanged
versions, nested keys, reordered entries and absent fields moving between array-table rows, and
`entries_text` printing an entry's `owner` after its `path`, an owner set, cleared or replaced
alone being the changed key `repository.owner`, and `version_entries` reading a text as a
document's entries (#325). `meada-core/tests/ids.rs` and
`meada-core/tests/manifest.rs` hold the `owner` and `added_by` cases (canonical text, version id,
each rule and the names accepted; an `added_by` printed after the owner, its change alone the
changed key `repository.added_by`, and `authorize_entries` treating it as an edit, #330, which
under the GitHub grant provider is `added_by_change`, #333); the
shared validation table below carries the `owner` and `added_by` rows in both languages.
`tests/fixtures/named-workspaces.json` (#340) is the Y/A/B table of ADR 0020 and ADR 0021 for spec
S5: callers with their effective per-login rows (direct and group-derived alike), creation,
stale-set and missing-login rows, discovery, authority, queries, links, revocation, the sponsor
rule and the shared membership. `meada-core/tests/named_workspaces.rs` checks every row against a
small reference of the ADRs' rules and that the table covers the spec's scenarios; no runtime
reads it in the catalog's tests (below), and #342 to #346 add their public-process, UI and
WebAssembly readers and rows.
`tests/fixtures/workspace-validation.json` is the single pt-BR/English validation table used
by the real WASM in `ui/tests/core.test.ts` and the binary `workspace import` in
`tests/it/generations/imports.rs` with generation-pointer and real configured store services.
Its authorization rows (#323: `role`, `groups`, `grant`, `current`, `new` in, `refused` out)
cover each reason for `add`, `edit` and `remove` (`owner_change` is only an edit), their
precedence, an owner set on an ownerless entry, cleared and replaced, several teams in one
change, a group that owns nothing yet, an owned entry outside the grant, moved entries, the
refused list's order and a viewer passed in; every row is also refused nothing for the global
operator. Its composition rows (`grant`, `current`, `new` in, `composed` out) cover an edit, a
removal and additions around entries outside the grant, and a reordering submission. Under the
GitHub grant provider (#333) rows also carry `login` and `github_host` (none on S3's rows, which
answer as before): an addition on the host outside the grant with `added_by` accepted, the same
by a subject without a login and one without `added_by` (inside the grant too) `unattributed`,
an addition off the host outside the grant `outside_grant` and inside it needing no `added_by`,
an edit changing, clearing or setting `added_by` `added_by_change`, a kept `added_by` and the
removal of an attributed entry accepted, the full precedence (`outside_grant`, `unattributed`,
`ownerless`, `owner_change`, `added_by_change`, `not_your_team`) in one change, the global
operator refused nothing, and without the GitHub grant provider the same addition `outside_grant` and an
`added_by` change alone ADR 0015's edit; composition rows with a login and a host: a forged `added_by`
on an addition on the host replaced by the login, cleared off the host, left absent with no
login, an edited entry kept as submitted, and additions kept as submitted without the
GitHub grant provider.
`meada-core/tests/authorize.rs` drives `authorize_entries` and `compose_entries` through those
rows and `role` on inline entries; `ui/tests/core.test.ts` drives `authorizeEntries` and
`composeEntries` through the same rows, and the binary's import test reads only the text rows.
`tests/it/refresh_lock.rs` keeps `meada workspace apply` (the global operator) moving the pointer
for owner changes and ownerless entries with its refusals unchanged.
`ui/tests/workspace.test.ts` checks local editor validation/diff and captured commands;
`ui/tests/WorkspacePage.test.tsx` covers the labelled versions/editor/diff DOM and HTTP boundary.
The team operator's editor (#325): `ui/tests/workspace.test.ts` holds the model seam with the real
WASM (annotated entries never reaching a text, an id or a diff key; each entry's state; each
refusal reason computed locally and cleared by undoing it, with no request; the diff of the
composed version with entries outside the grant in place; the apply offer and its teams), and
`ui/tests/TeamWorkspace.test.tsx` the DOM in both languages: the held entries with owner and
state, each refusal marked in text with the save disabled, a save of an edit, a removal and an
addition naming its teams with nothing sent before confirmation, a 403 `entries_refused` with
its `hidden` count, a viewer refusal turning the editor read-only, apply and rollback offered
by authorization, owners and an owner change for a viewer, no save or apply for a viewer or with
operations off, the global operator's unchanged editor and CLI command, one unreadable stored
version leaving the other applies, and a long version list read two versions at a time.
`ui/tests/GithubWorkspace.test.tsx` (#333) renders the views under the GitHub grant provider:
`unattributed` (a session without a login adding on the host) and `added_by_change` (an
`added_by` changed or set) marked in text in both languages with the save disabled and no
request while typing; an addition on the host outside the grant with a forged `added_by`
accepted with a login and sent as typed, the same off the host `outside_grant`; a server
`outside_grant` for an identity already present listed with its change and reason; each entry's
`added_by` in the team editor's table; and the `excluded` mark, with its reason in text, in the
team editor's table, the global operator's labelled status and the versions page (only for the
version that carries it), never in the editor's text or a save. `ui/tests/workspace.test.ts`
keeps `excluded` out of a text, an id and a payload.
`ui/tests/actions.test.ts` and `ui/tests/action-pages.test.tsx` keep the fleet and store pages
action-free for a team operator.

### The audit (#328)

`tests/it/ui/oidc/audit.rs` runs the team fixture's console with `[postgres]` (a schema of each
test's own, its URL in `ops/pg-url` through `postgres::Relay`, a loopback relay the test closes;
`postgres::audited` migrates it and reads the records back). The global operator's `pointer`
`retain`, `set` and `drop`, `fence/recover`, `workspace/versions` and `workspace/apply` leave one
`accepted` record each and `GET audit` lists them newest first with Postgres's time, paged by
`next` with every record once; a write while `meada snapshot lock` holds the lock ends
`refused`; a hostile row is left out with one line naming the table; a team operator and a
viewer get the role refusal and bad queries a 400; the stderr lines are S2's. A team operator's
accepted save and apply record `team_operator` and the touched owners, its refused save its teams
and the message with the named identity and the hidden count; a viewer's apply and a team
operator's `pointer` and `fence/recover` with a body that never completes are refused and
recorded with the route's action and `{}`. With the relay closed every write is a 503
`audit_unavailable` with Valkey and the store unchanged, reads, `session` and a fresh start still
answer, a viewer's write keeps its refusal and line and leaves no record; a schema version the
binary does not know refuses the start naming both. No record, line or body carries a token part
or the Postgres URL. `tests/it/refresh_lock.rs`
(`every_cli_twin_is_audited_and_none_writes_without_its_record`) holds the CLI twins: one record
each with `via = "cli"`, the author, the operator's role and the outcome, an exit 1 naming
Postgres and nothing written with the relay closed, and an exit 1 naming both versions on an
unknown schema version; their behavior without `[postgres]` stays covered, output and exit
code, by the existing `refresh_lock` and `generations` cases. `tests/it/cli.rs`
(`the_audit_refusal_follows_lang`) checks the CLI refusal in pt-BR and English, and
`tests/it/postgres.rs` migration 2's columns. `ui/tests/audit.test.tsx` holds the page's view
model from `fixtures/audit.json`, `offered`, the labelled and keyboard-reachable table paging
through `next` in both languages, the 503's message and the copy keys.

Review regressions for #293 cover manifest-wide grant filtering (including groups-only
identities in validation errors), issuer query preservation at `Auth.login`, exact recovery
ceilings through CLI and HTTP, and live-heartbeat selection with a controlled RESP `TIME`
response while all other commands reach the real Valkey fixture. The ceiling tests preserve
one final locked publish and compare English and Brazilian Portuguese refusals.

### The directory (#334)

`tests/it/ui/oidc/directory.rs` runs a console with `[oidc]` (the `oidc` module's loopback
issuer), `[postgres]` and `[directory]`. Against the local Postgres (a schema of each test's
own through `postgres::audited` and its relay; skipped without `MEADA_TEST_POSTGRES_URL`,
failed with `MEADA_TEST_SERVICES_REQUIRED=1`): an Entra ID sequence (the `userName` lookup,
`POST` with its id, `Location` and `meta`, a `PATCH` of attributes through a filtered path, a
sub-attribute and the enterprise extension, `PATCH active` with `"False"`, the user shown
inactive, a group with members added and removed by path and by value list answered 204,
`excludedAttributes=members` on a list and a read, the user's `DELETE` leaving the group and a
revocation marker, the group's `DELETE`), then the relay closed: a 503 in RFC 7644's schema and
`config` still answering; each write's one `scim:` line, exactly, and no token, attribute value
or Postgres URL on stderr. An Okta sequence (the lookup with `startIndex` and `count`, `POST`
with a `password` that neither the answer nor the stored row holds, `PUT` of the whole user,
`PATCH active=false` without a path, a group with no members, `PUT` replacing its members, a
rename by a path-less `PATCH`, `DELETE`) and paging over 150 users in creation order, with
`count` capped at 100. The refusals: 409 `uniqueness` for a `userName` differing in case on
`POST`, `PUT` and `PATCH`, a repeated `externalId` and a repeated group name; 400
`invalidValue` for a member naming no user or no id; `invalidFilter`, `invalidPath` and
`invalidSyntax` for an unsupported filter, path and operation; 404 for an unknown or malformed
id; a two-operation `PATCH` whose second fails (`noTarget`) writing nothing; `id` and `meta`
unchanged by `PATCH` and `PUT`; 413 without `scimType` for a body over 1 MiB and a resource
over 64 KiB on `POST` and after a `PATCH`; eight concurrent `POST`s of one `userName` storing
one; no error body or stderr line echoing a value. The revocation marker: none after a
deactivation; one after deleting an inactive and an active user, which `filter` does not show;
none for an unknown id; kept by an inactive reprovisioning and by a `POST` active only by
default, cleared by one naming `active` true (by `PATCH`, `userName` differing in case, and by a
path-less `add`). Four concurrent `POST`s of one group name store one; a group `PUT` and `PATCH`
renaming to a held name are 409 `uniqueness`; a user deleted while a `PATCH` adds it to a group
leaves no membership; a NUL in a value is a 400 `invalidValue`, not a 503. Without a service (a
`[postgres]` naming a closed port): no token, a console token, a wrong token, the token one
character short and a `Basic` scheme each a 401 with `WWW-Authenticate: Bearer`; `Origin` a 403
with and without the token; a foreign `Host` a 403; the SCIM token under `/api/v1/session` a
401; another media type a 415; the three discovery documents' fixed content; a 405; a 503 for a
resource with `config` answering; the `manifest` route naming the default `token_env` and not the token; `/Schemas` marking the
`group_attribute` unique; the
console refusing to start with a 31-character token and with none, naming the key and its line
in English under `en_US` and `pt_BR`. `tests/it/postgres.rs` holds migration 3's columns.

### Deprovisioning, the owner picker and `owner_missing` (#335)

`tests/it/ui/oidc/deprovision.rs` runs the same console against the local Postgres (the `directory`
and `refresh-lock` features). A deactivated user is a 403 `deprovisioned` on every route but
`config` (a global and a team operator, a read and a write), the static files unchanged, the
audit holding no row for the refusal and a never-provisioned user admitted; a deleted user stays
refused until it is provisioned active; an `externalId` lookup is exact where a `userName` lookup
ignores case. With the relay closed, reads are admitted, a write is a 503 `audit_unavailable` with
no audit insert and one stderr line that names the directory and never the claim. The picker lists
group names by exact prefix, sorted, an empty prefix the first 100, a prefix over 256 a 400, for
the global operator alone (a team operator and a viewer are refused). `owner_missing` appears
only when true in `workspace` and `workspace/versions/<id>`: nothing flagged before a group was
provisioned, owned entries flagged after the last group is deleted, ownerless entries never,
and no flags plus one stderr line with the directory unreadable. `GET admission` carries the
counts and the last write. `ui/tests/Directory.test.tsx` holds the editor's picker (the global
operator alone, the owner escaped into the TOML string, no request for a team operator or a
viewer), the flag in text in the editor, the comparison and a team operator's table in both
languages, the save and editor text without the flag, the admission page's rows and the
deactivated-account page. `tests/it/postgres.rs` holds migration 4's columns, and migration
5's, the permissions set (#331). `tests/it/cli.rs` holds the role check's refusals under
`pt_BR` and `en_US` (`[oidc.github]` without `[postgres]`, a `host` and a `roles` error, the
role check without `[oidc.github]`, without a `[[github_app]]` block for its host and without
`MEADA_REFRESH_FENCE` under `[refresh_lock]`, and a reader without the exclusions file).

### The GitHub grant at admission (#332)

`tests/it/oidc/github.rs` runs `meada mcp --listen` over the chain with `[oidc]` (one audience)
and `[oidc.github]`, its permissions set seeded by `meada github-app roles` against
`github_app`'s fake (`oidc::github::Github`: the fake with the owners `chain` and `team`, the
local Valkey and Postgres in a schema and key prefix of each test's own; skipped without them,
failed with `MEADA_TEST_SERVICES_REQUIRED=1`). `MEADA_PERMISSIONS_POLL_MS`, a debug-build knob
as `MEADA_DRAIN_MS` is, shortens the 60-second poll to 200 ms and lowers the 60-second floor of
`max_age_seconds` to one second. The union: a login holding `write` and a custom role of
`roles` (`Custom Reviewer`) and a claims grant read all three in the root's 404, a trace keeps
the gateway's link, a role outside `roles` (`triage`) is a 403, the login in another case than
GitHub's; no login claim, an empty, 40-character, numeric, `ana.souza` or array value reads the
claims grant only; a machine token's `*` reads everything; a replacing set without a role
revokes it. The age, through
`postgres::Relay` (closed, then opened again on its port): the GitHub grant holds past the
relay's close until the set is `max_age_seconds` old on Postgres's clock, then only the claims
grant answers, every stderr line one failed poll and their count bounded by the polls; a new set
after the relay opens restores it. A node never reaching Postgres starts and serves, its GitHub
grant empty and `*` whole; over a schema one version newer than `MEADA_POSTGRES_VERSION_MAX` it
names both versions on exactly one stderr line. Hostile rows (written by the test): a set whose
`finished` is ahead of Postgres's clock grants nothing and is named by its table, and once valid
a `*` row and an off-host row are skipped with one line each naming `permission_rows`. No
stderr line carries a token part, a login or the Postgres URL. `src/oidc/permissions.rs`'s unit
tests hold the exact expiry (`max_age_seconds` grants, a millisecond past it does not), an age
that never moves back when Postgres's clock does, the login's shape and the row identity check.

`tests/it/ui/oidc/github.rs` runs the team fixture's console with `[oidc.github]` and
`[postgres]`: the session is `unread` with the login lowercased before any set, then `current`
with the GitHub identity in `grant`, a group owning an entry only GitHub grants making a
`team_operator` with that team, every bounded route (`workspace`, its versions, `repositories`,
`manifest`, `store`, `credentials`) naming no identity outside the union; the token check's
`sources` (`claims` and `github`, sorted, `*` for a machine token) with no request to the
loopback issuer or the fake GitHub; `admission`'s `github` with the set's id, `finished`, age,
`max_age_seconds` and the counts the job wrote; an audited write's record carrying the login;
then `expired`, a viewer with an empty grant.

`tests/it/ui/oidc/catalog.rs` (#341, feature `workspaces`) runs a console with `[oidc.github]`,
`[postgres]` and `[catalog]` over a permissions set the real `meada github-app roles` wrote
against the fake GitHub (an installation on the owner `acme` covering the table's repositories, a
repository nobody collaborates on and a covered owner no pattern admits), with a token per caller
of the shared Y/A/B table: `GET catalog` lists exactly the identities the caller's login holds an
accepted role on, direct or through a group alike, nothing through a rejected role, a claims grant
or a raw group name, `missing_login` pending without a login, the whole catalog for the platform
administrator, and names no repository the caller may not select anywhere in the answer; `POST
catalog/check` agrees with every creation row of the table (`pending` included), reads an
identity outside the catalog like one the caller may not select (`unavailable`, no reason), and
refuses bodies and queries past their bounds with one fixed 400; pagination by opaque cursor and
prefix; a set past `max_age_seconds` pending `stale_set` for everyone, the administrator
included, a failed run leaving the set to age, a new set restoring it; `config` naming the
picker, `manifest` hiding `catalog.allow` from everyone but the global operator, `admission`
counting the catalog, and the fake seeing no request while the console answers. The
`oidc::permissions` unit test reads the same table against the cache. In
`tests/it/github_app/roles.rs` the catalog cases check the installation listing (the owners a
pattern names, every page, the allow list, names compared lowercase, an owner no pattern names
never asked, a repository the list refuses never listed for collaborators), the cap (the count and
the cap in both languages) and that a cap, a throttle (marking the installation), a 5xx or a
body that is not the documented object each fail the run and write nothing (a run at a fence
between them is accepted); `tests/it/manifest.rs` checks each `[catalog]` error at its line and
key without echoing a pattern; the Postgres tests count migration 6. The front end's
`ui/tests/catalog.test.tsx` checks the picker's routes, pages, selection, verification, pending
states and refusals in both languages.

`tests/it/ui/oidc/additions.rs` (#333) runs the same console with `[oidc.github]` and
`[postgres]` (every write audited) and the role check over the store's version: a team
operator's addition on the host outside its grant stored with `added_by` set from its login over
a forged `mallory`, the entries outside the grant kept; without a login `unattributed`; an
`added_by` set on its entry `added_by_change`; an identity the workspace holds outside its grant
`outside_grant`, named as submitted, with a login and without one; each refusal writing nothing
and recorded `refused`, the accepted save `accepted`, with the login when there is one; the
global operator setting, replacing and clearing `added_by` and its own additions kept as written;
a team operator's apply of a version whose addition on the host inside its grant lacks
`added_by` refused `unattributed` with the pointer unmoved, and applied once the addition has
one; after `meada workspace pull` and `meada github-app roles` exclude an addition, `workspace`
and `workspace/versions/<id>` of that version marking it `excluded: true` for the global operator
and for a viewer the GitHub grant lets read it, another version holding the same entry and every
other entry unmarked, and a team operator that does not read it never seeing it. No stderr line
carries a token, a login or the forged value. `tests/it/ui/oidc/team.rs` checks that without
`[oidc.github]` no workspace body carries `excluded`. `tests/it/ui/oidc/audit.rs` also leaves out a record whose stored login fails the login
pattern. `ui/tests/models.test.ts`,
`ui/tests/operations.test.ts` and `ui/tests/operations.test.tsx` hold the session view's login
and state, the admission page's set rows (and `unread` with no set) and the token check's
sources, in both languages.

## Java declarative clients (#90)

`conformance-java` gains `example.declarative` (Feign with and without `url`, a Retrofit
interface built by `Remotes`, shadowed annotations and `@RequestLine`, which give nothing);
`conformance::java` keeps the export and `conformance-java.context-feign.txt`. The
`xlang-java-api` fixture calls the TypeScript provider through two Feign clients, and
`xlang-java.trace-up-feign.txt` is the trace up from the route through the interface method.
`freshness` edits a Retrofit base and a Feign `path` and compares with a cold index.
`xlang::a_declarative_client_method_is_an_open_item_at_its_definition` adds a mirror of the web
app (`common::xlang_java_mirror`), so the Feign client without `url` is ambiguous, and a
Retrofit method with `@Url`, which is unresolved: both are open items at the method, in
`search` and `meada decide --list`; `decider::the_excerpt_of_a_declarative_client_method_is_the_method`
checks the excerpt is the method.

## Java conformance (#84)

`conformance-java` holds Maven modules and a Gradle source tree, with `example.clients.Injected`
(Spring `@Value` with a literal, a placeholder the module's configuration holds and a placeholder
with a default) and a computed Gradle coordinate. `conformance::java` checks
all Java definitions and its golden export; the companion engine tests check package and
static imports and their priority/ambiguity (including duplicate fully qualified types),
overloads and constructors in separate namespaces, fully qualified calls with shadowing,
class-named method identities, qualified parameter types, visibility, anonymous
class and lambda ownership, and search/context goldens. `index` checks byte-identical exports
from independent copies; `freshness` edits, adds and deletes sources and compares the refreshed
export with a cold one, including overload arity from an unchanged file's cached facts.
The hostile-depth tests exceed Java's definition, value and member-path limits and reject
exact links that escape an omitted scope. A long package prefix exceeds the qualification
budget and leaves the whole file out; annotation values, comments and malformed type literals
never enter exported names. `schema` has a Java sample for the adapter facts the index persists.
The `example.channels` package (#92) holds a client of each broker kind (Spring Kafka, Spring
AMQP with a listener's and a configuration's bindings, JMS, SQS, jnats, Jedis and Lettuce), a
topic from a `${key}` placeholder and one that cannot be evaluated;
`java_channel_clients_link_their_producers_to_their_handlers` checks every link and the
unresolved sides, and `freshness` edits the configuration key and a topic constant.

The `example.calls` package (#85) adds `JAVA_CALLS`, each receiver form at each level: field,
`this.field`, parameter, local, `var` with `new T(...)` and with a call's declared return type, a
fully qualified field type, an interface receiver, a parameter and a type-named local that
shadow, inherited overloads of another arity, a written-signature override, a same-count tie
across the family, an outside base, and unknown receivers matched in the calling family or
across the repository; `conformance-java.context-ping.txt` lists exact, inferred and ambiguous
callers, with a narrowed match's "one of several" reason; the fixture also has a chain and a
`super` call. Resolver unit tests (`java_calls`) cover strict outside bases (`ArrayList`'s
`add`), Python's first-defining-class rule, a redundant interface and its subclass, name matches
through the receiver's family and never the caller, an inherited field against an outer one,
chains and `var` through the overload called, `super`, initializer locals, `Object`'s implicit
members, and receivers left unknown: a type parameter, a local named like a field, same-named
locals of different types, an anonymous class and a cyclic family. `java_static_imports_an_outside_base_may_shadow_are_name_matches` keeps a static import `exact` in a plain class and
`inferred` under an outside base.

## Named workspaces: creation to query (#342)

`tests/it/ui/oidc/workspaces.rs` drives the console, the job and a node as processes over the
shared Y/A/B table (`tests/fixtures/named-workspaces.json`), the loopback issuer, the fake GitHub
API and the local Valkey, Postgres and S3-compatible store: the creation rows (allowed, refused
with `hidden`, pending, forged keys), peers listing and showing through their own grants, the
whole pipeline (pull, roles, index, publish, node pull, `/-/workspaces/<id>/<identity>` per
caller, one bundle of Y, `members` and `contexts` in the document, `state` `applied` then `live`)
and the sponsor rule (`excluded`, not a failure). `meada-core/tests/named_workspaces.rs` reads
the same table through `meada_core::named`, and `ui/tests/named-core.test.ts` through the built
WebAssembly; `ui/tests/NamedWorkspaces.test.tsx` drives the page.

## Named workspaces: versions and overlap (#343)

`tests/it/ui/oidc/workspaces/lifecycle.rs` runs on the same case as `workspaces.rs` (console, job,
fake GitHub, Valkey, Postgres, S3-compatible store, the Y/A/B table): the lifecycle along the base
(save moves nothing, apply, rollback marked as such, stale base, `already_current`, unknown
version, rename, delegation by the administrator alone, the last operator, delete as one 404
for everyone, the same triple still `workspace_deleted`, versions and history in order, one
audit record per operation with its actor, authority and base); two writers of one base (one
wins, the other is `stale_base`, one history row per accepted change); partial revocation (the
creator keeps the workspace with a hidden count, saves without dropping what it cannot read, is
refused B with `hidden`, cannot apply what a lost role no longer covers, nothing moves; a
peer sees one 404 on the workspace, its versions and its history); overlapping Y+A and Y+B
(independent edits and deletes, one member and one bundle of Y, graph answers through the context
bounded by the grant); a SCIM-deleted operator blocked on every lifecycle route until
provisioned active, a never-provisioned subject admitted (`directory`); and the `meada
workspaces` twins as the administrator with `cli` audit records. `meada-core/tests/
named_workspaces.rs` checks the `lifecycle` rows of the table (compose keeps hidden entries,
`changed` is what is rechecked) and `meada-core/src/named.rs` the diff.

## Named workspaces: context configuration (#344)

`tests/fixtures/effective-configuration.json` is the table of ADR 0021 contract 6: entries as a
version document holds them, the facts the console reads from a generation (`held`, `own`,
`documents`, `excluded`) and the answer per entry (source, ignored blocks, coverage, diagnostic
codes), plus the context pair that shares Y. `meada-core/tests/effective.rs` reads it through
`meada_core::effective` (and the module's unit tests cover the same rules inline), and
`ui/tests/named-core.test.ts` through the built WebAssembly (`namedEffective`), with the check that
the view's annotations never enter a manifest's text or version id.

`tests/it/ui/oidc/workspaces/configuration.rs` runs on the same case as `workspaces.rs` (console,
job, fake GitHub, Valkey, Postgres, S3-compatible store, the Y/A/B table) with the mirror of Y
committed without a `meada.toml`: two contexts sharing Y declare different peers and serve hosts
and each view, recorded context and context export shows only its own (the peer links of Y exist
through one and not the other); before the job ingests, the view says pending and the workspace is
applied; after it, a repository's own `meada.toml` wins whole and the ignored block is said beside
the accepted apply; a configuration change is a new generation with the same `members` bundles,
the other context's recorded entry unchanged and a pinned generation's export keeping the old
configuration; a stale base is refused and the other workspace's base does not move; an earlier
version is viewed with `?version=`; an unrelated caller, a peer who operates nothing and an
unknown id get the discovery rule's answers. The refusals test posts reserved keys, a value or a
URL where a variable name belongs, a peer `path` and an unknown key (all 422 with nothing
stored), the stateless preview before saving (no version stored, a missing peer and an ignored
block shown, a repository the caller cannot read shown pending), a forged key (400) and an
anonymous call (401). `ui/tests/NamedWorkspaces.test.tsx` drives the editor: the canonical text,
the effective configuration shown in text before any POST of a save, the frozen reviewed payload
at the base read, the findings beside an applied change, a stale base in words, and no editor for
a viewer.

`tests/it/ui/oidc/workspaces/proposals.rs` (feature `proposals`) runs on the same case and adds the
proposal server (`meada workspaces mcp`, `[proposals]` appended to the bootstrap the console
read), called over MCP with tokens for its own audience: no token, a forged one, a console's and a
node's are a 401 naming the metadata, a browser a 403, and its token opens nothing on the
console; `tools/list` gives four read-only tools with closed schemas naming no authority; every
creation row of the Y/A/B table through `propose` is the console's decision, each refusal the
console's own error byte for byte; a hidden repository is counted and never named, hints and
`_meta` or headers asserting a role change nothing, a forged argument and every bound are
refused as invalid arguments, a body past 80 KiB is a 413; the catalog is the console's, empty
for a claims grant and pending for a missing login; the fake GitHub counts no request and stderr
holds no token, note or manifest. Over existing workspaces it lists and shows under the discovery
rule, proposes a change with its base, diff and effective view, refuses B and a peer, gives
equal bytes for equal calls, leaves the console's records, versions, history and audit as they
were, and the manifest is then saved and applied by a person in the console, after which the old
base is stale; the table's discovery rows go through `show`; once the creator loses A, its
proposals and `show` compose over A without naming it, even when it names A; a creation of a
workspace created and deleted is refused `workspace_deleted`. A set past `max_age_seconds` makes
every proposal pending `stale_set`; a server allowed no call in flight (`MEADA_PROPOSALS_CALLS`)
answers 503 `busy`; with `[directory]`, a deleted user is a 403 `deprovisioned` until it is
provisioned active again.

## Named workspaces: acceptance (#347)

`tests/it/ui/oidc/workspaces/acceptance.rs` runs on the same case as `workspaces.rs` and forces the
stages of a refresh to fail in turn, with a node serving the first generation: the role check
(GitHub answering 500) exits non-zero and writes no set, so the pointer and the catalog stay; a
revocation committed by a later complete check is effective at the node while the generation is
unchanged; a role check that fails again while GitHub already shows the role restored brings
nothing back; a missing member mirror stops `index` (an entry left without a sponsor needs none)
and a missing index directory stops `snapshot publish`, each leaving the pointer on the first
generation and the other group's workspace answering; only a later complete set restores the
grant. `Case::grant_all_but` and `Case::await_check` (in `lifecycle.rs`) are the revocation helper
split in two. `scripts/helm-check` renders `values-workspaces.yaml` and checks the sections are in
the console's and the job's bootstrap and not a node's, and the three refusals of the chart. The
site's named-workspaces page maps each acceptance criterion to the tests that show it.
