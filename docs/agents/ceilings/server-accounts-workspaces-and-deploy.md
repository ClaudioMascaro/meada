# Server, accounts, workspaces and deploy

- `tests/it/docs.rs` (#644): `MAP_BUDGET` (8 KiB, the architecture, contracts and
  resolution/index/shape ceiling indexes), `DETAIL_BUDGET` (24 KiB, their relocated topic
  leaves under `map/overview/`, `map/contracts/` and
  `ceilings/resolution-the-index-and-shape-readers/`) and `AREA_BUDGET` (64 KiB, the other
  Markdown files under `docs/agents`) bound a file in bytes, keeping mandatory routing small
  while untouched areas await their own split;
  a file past its budget fails `every_agents_markdown_file_stays_within_its_budget` with its path,
  its size, its budget and `cargo run --example split_agents -- <file>`, which splits it by its
  `## ` headings (the splitter does not split inside a section: a part still past the budget needs
  `## ` headings of its own first, and the check keeps failing until it has them). The remedy is
  the split, never dropping entries.
- `tests/it/docs.rs` (#491): the check that a limit is marked reads only a `const` named `MAX_*`,
  `*_MAX`, `*_LIMIT` or `*_CAP` (a minimum or a test's allowance goes in `NOT_CEILINGS` with its
  reason), one line each; a limit under another name (`*_DEPTH`, `*_BUDGET`, `*_TTL`) is marked
  by hand, and only a limit-named marked `const` must be unique (each language pack keeps its own
  `VALUE_DEPTH`).
- `deploy/` (#568): the fetch scripts' awk program (Compose `fetch-pulled.sh`, the chart's
  `workspace.fetch`) reads bare and double-quoted `repo` and `path` keys, not single-quoted literal
  keys (`'repo' = ...`): a `'` would end the shell's single-quoted program.
- `deploy/` (#504): the proposal server's Helm block renders no Ingress (the operator exposes its
  Service behind their own TLS at `proposals.publicUrl`), and Compose's `proposals.workspace.toml`
  keeps `[oidc]`, `[catalog]` and `[workspaces]` commented, so the service does not start until the
  operator copies the console's sections (the local accounts profile has no proposal server). An
  Okta authorization server stamps one `aud`, so the proposal server needs a second authorization
  server as its `[oidc] issuer` (untested); the chart renders one `[oidc]` for every pod and cannot
  give it one.
- `accounts.rs` (#554, ADR 0024): a password holds `PASSWORD_MIN` (12) to `PASSWORD_MAX` (128)
  characters and a username at most `USERNAME_MAX` (64); an agent token lives `AGENT_HOURS` (8) by
  default and `AGENT_HOURS_MAX` (168) at most. `KEY_MAX` (16 KiB) bounds a key file of `[accounts]` and
  `PASSWORD_FILE_MAX` (1 KiB) `admin_password_file`, each refused past it naming the file;
  `HASH_MEMORY_MAX_KIB` (64 MiB), `HASH_ITERATIONS_MAX` (8) and `HASH_LANES_MAX` (4) bound the
  argon2id parameters a stored hash may carry (a row past one is hostile and admits nothing);
  `ui/accounts.rs`: `HASHES_MAX` (4) bounds the argon2id hashes the console computes at once,
  19 MiB each (a further sign-in or password change is a 503 `busy`, nothing hashed), and
  `LOGIN_MAX` and `PASSWORD_BODY_MAX` (4 KiB each) the bodies of `login` and `account/password`
  (413 past them); `ui/accounts/administration.rs` (#557): `ADMIN_BODY_MAX` (4 KiB) the body of
  an account's creation or change (413 past it); `TOKENS_BODY_MAX` (1 KiB, #560) the body of
  `account/tokens` (413 past it); `ui/accounts/github.rs` (#561): `GITHUB_BODY_MAX` (1 KiB) the
  body of `account/github` (413 past it), `GITHUB_TIMEOUT` (10 s) the one call to GitHub and
  `GITHUB_ANSWER_MAX` (64 KiB) its answer (a 503 `github_unavailable` past either). The lockout
  (#555, `accounts.rs`): `LOCKOUT_FAILURES` (10) consecutive failed password checks, counted under
  the account row's lock, lock an account for `LOCKOUT_MINUTES` (15), never extended by an attempt
  while it holds. The generated first password (#579, `ui/accounts.rs`) is printed after its row
  commits, by `Local::announce` or on drop: a console killed in between (SIGKILL, a crash) leaves
  the account with a password nobody saw, and no later start prints it; `meada ui reset-admin`
  recovers it.
- `ui/workspaces/proposals.rs` (#346, ADR 0022): `BODY_MAX` (80 KiB) bounds a request's body
  (413), `OUTPUT_MAX` (192 KiB) a tool's answer (`too_large` with its counts, never cut),
  `CALLS_MAX` (32) the calls in flight (503 `busy`; `MEADA_PROPOSALS_CALLS` lowers it in a debug
  build for a test), `HINTS_MAX` (50) and `NOTE_MAX` (256 characters) a proposal's hints; the
  catalog tool takes the console's `PREFIX_MAX`, `CURSOR_MAX` and `LIMIT_MAX` (`ui/catalog.rs`),
  and a manifest is `named::TEXT_MAX`.
- `ui/workspaces.rs`, `workspaces.rs`, `snapshot/versions.rs`, `meada-core/src/named.rs` (#342):
  `CREATE_MAX` (80 KiB) bounds a creation's body (413), `TEXT_MAX` (64 KiB) a named manifest's
  text, `NAME_MAX` (80) and `DESCRIPTION_MAX` (500) their characters, `LIST_LIMIT_MAX` (100) and
  `AFTER_MAX` (256) a list page and its cursor (`LIST_LIMIT`, 25, a page without `limit`), `WORKSPACES_MAX` (2000) and `ENTRIES_MAX` (500)
  (`WORKSPACES_DEFAULT`, 200, and `ENTRIES_DEFAULT`, 100, are their defaults)
  what `max_workspaces` and `max_entries` may allow and what a reader of the contexts file or a
  version document accepts whatever they say now (a version document is also bounded by the
  deployment version's `VERSION_MAX`), `OPERATORS_MAX` (20) a workspace's operators (the
  console's lifecycle bodies are capped by `APPLY_BODY_MAX` (1 KiB), `RENAME_BODY_MAX` (4 KiB),
  `DELETE_BODY_MAX` (1 KiB) and `OPERATORS_BODY_MAX` (16 KiB), a delegated subject holds at most `SUBJECT_MAX` (256) bytes and `PAGE_MAX` (50)
  bounds a page of versions or history, #343),
  `CONTEXTS_MAX` (256 MiB) the contexts file `meada workspace pull` writes and `EXCLUDED_MAX`
  (128 MiB) the exclusions file; past one the file counts as hostile. Their product is what a
  refresh pulls and publishes: the single failure domain of one fleet (ADR 0021).
- `directory/read.rs`: the owner picker lists at most `PICKER_MAX` (100) group names and says
  nothing when it cut the list; the global operator narrows it by typing more of the prefix
  (#335).
- `meada-core/src/manifest.rs`: an entry's `owner` holds at most `OWNER_MAX` (256) characters
  (ADR 0015); a longer name is an error naming its line and key, wherever the entry is read
  (import, a stored version, a pull, the browser). A named entry's peer names environment
  variables of at most `ENV_NAME_MAX` (128) bytes in `base_url_env` (#342).
- `decider/mod.rs`: an item with more than `MAX_CANDIDATES` (50) candidates is skipped in decider
  mode and reported as such; human mode still offers it.
- `ui/catalog.rs` and `catalog.rs` (#341): `GET catalog` answers at most `LIMIT_MAX` (100)
  records a page (`LIMIT`, 50, without `limit`), takes a `q` of at most `PREFIX_MAX` (128) characters and an `after` of at most
  `CURSOR_MAX` (256); `POST catalog/check` takes at most `CHECK_IDENTITIES` (50) identities in a body of at most
  `CHECK_BODY_MAX` (16 KiB);
  `[catalog] max_repositories` may allow at most `MAX_REPOSITORIES` (5000).
- `meada-core/src/authorize.rs`: a refusal's `refused` list (ADR 0015) holds one `Refusal` per
  changed entry, so it is bounded by the version's entries, which `VERSION_MAX` in `src/snapshot/versions.rs` (4 MiB, S2's
  document size cap) bounds; it is not truncated.
- `snapshot/pull.rs`: a generation's bundles hold at most `GENERATION_BYTES_MAX` (256 GiB)
  together, as the document records their sizes (checked before any download) and as the
  regular files extracted from them add up; at most `GENERATION_ENTRIES_MAX` (4,000,000) archive
  entries together; and each GNU long name or link, or pax header, at most
  `HEADER_EXTENSION_MAX` (64 KiB), which a scan of the headers checks before the tar crate,
  which reads them into memory whole, sees the archive. Past any of them the pull fails and
  changes nothing. A symbolic link whose target goes through another symbolic link is refused
  even when both stay inside the mirror. A sparse file is refused. The node pointer file holds at
  most `NODE_POINTER_MAX` (64 bytes); past it the pointer file is malformed.
- `snapshot/generations.rs`: `{<key_prefix>}:kept` holds at most `KEPT_MAX` (8) generations,
  since every node holds every retained generation on disk: `pointer retain` refuses past it,
  and a pull refuses a retained list longer than `keep` plus `KEPT_MAX`, naming the key and
  changing nothing.
- `snapshot.rs`: a generation document holds at most `DOCUMENT_MAX` (16 MiB), in the store and
  as a node holds it, and the store's `current` at most `CURRENT_MAX` (64 bytes); past either
  the object is refused, naming its key or file, and a pull changes nothing.
- `snapshot/store.rs`: a credential file (key id, secret, token) holds at most `SECRET_MAX`
  (16 KiB), and `ca_file` at most `CA_MAX` (1 MiB); past either the file is refused, naming it.
  An empty CA file or invalid certificate is also refused before connecting.
- `valkey.rs`: `password_file` holds at most `VALKEY_PASSWORD_MAX` (4096 bytes) and `ca_file` at most
  `VALKEY_CA_MAX` (1 MiB); past either the connection fails, naming the file.
- `postgres.rs`: `url_file` holds at most `URL_MAX` (4096 bytes) and `ca_file` at most
  `POSTGRES_CA_MAX` (1 MiB); past either the connection fails, naming the file. `schema` holds at
  most `SCHEMA_MAX` (63 bytes, Postgres's own identifier length) and `statement_timeout` at most
  `STATEMENT_MAX` (about 2.1 million seconds, `INT_MAX` milliseconds), which Postgres would refuse
  anyway.
- `postgres.rs`: SCRAM's channel binding (`end_point`) knows the RSA PKCS#1, ECDSA and RSASSA-PSS
  signatures with MD5, SHA-1 or SHA-2 (SHA-224 apart, which ring does not hash); a server certificate signed otherwise
  (Ed25519, for which RFC 5929 names no hash and Postgres binds nothing either) gets no binding,
  and SCRAM runs as `SCRAM-SHA-256` without `-PLUS`. An RSASSA-PSS certificate on a Postgres that
  cannot hash one (an old build or OpenSSL) fails `-PLUS` authentication, as libpq does.
  `tls-unique` and client certificates are not supported.
- `grant/permissions.rs`: a process holds every accepted-role row of the newest permissions set in
  memory (logins times repositories under an organization base role), loaded whole when the set
  changes; a revocation on GitHub reaches it at the next successful refresh plus at most
  `PERMISSIONS_POLL` (60 seconds), and without one at most `max_age_seconds` after the last.
- `oidc/overage.rs`: an overage answer is kept `OVERAGE_TTL` (5 minutes), so a membership change
  reaches an overage subject that late; the cache has no cap on live entries, since only a token
  that passed every check adds one, and holds at most the subjects seen in the last two TTLs
  (an expired entry is dropped when found and swept once per TTL); a failed lookup is not cached,
  so with Postgres blackholed every uncached overage request waits up to the connect and
  statement timeouts and writes one stderr line; a console whose directory cannot be read admits
  reads (the deprovisioning rule of #335), so within the TTL a user deactivated meanwhile keeps
  its cached overage groups for reads, while writes are refused (503 `audit_unavailable`).
- `oidc/overage.rs` (feature `directory`, #336): Entra overage through the directory. A token is
  in overage (`in_overage`) when it has no claim named `groups_claim` and its `_claim_names` is an
  object whose value for `groups_claim` is a string; anything else, `groups` present with any
  value included, is a token without groups and is never looked up. The groups of such a token
  are the `group_attribute` values (`displayName` or `externalId`) of the groups whose members
  include the active `directory_users` row whose `user_attribute` equals the token's
  `user_claim` value (`externalId` exactly, `userName` through `lower()`), one statement after
  the version check (`postgres::versioned`), bounded by `Config::bound`; an inactive or absent
  user has none, and a value failing `owner_problem` is skipped with one `Msg::PermissionsBadRow`
  line naming `directory_groups`. `Overage::groups` looks up at admission, on nodes (`Oidc::admit`,
  inside `block_in_place`) and in the console (`admit_console`, the token check's `check`), and
  keeps the answer in memory by `user_claim` value for `OVERAGE_TTL` (5 minutes; in debug builds
  `MEADA_OVERAGE_TTL_MS` shortens it, a test knob as `MEADA_DRAIN_MS` is): an answer of no groups
  is kept like any other, a failed lookup is not (one `Msg::OverageUnreadable` line, the token
  having no groups, whether Postgres is unreachable or its schema version unknown), and an entry
  is dropped when found expired and by a sweep of all the expired ones that a store triggers once
  per TTL (`Cache::put`). Only a token that passed every check reaches the lookup. It is the
  only lookup on a node's request path. The console's `directory_refusal` runs after
  `admit_console` and before any role is computed from the groups, so an inactive or deleted
  provisioned user is refused (`deprovisioned`) before cached overage authority is used; nodes do
  not check deprovisioning.
- `oidc.rs`: each metadata and JWKS request takes at most `FETCH_TIMEOUT` (10 seconds), a
  metadata document at most `METADATA_MAX` (64 KiB) and a JWKS at most `JWKS_MAX` (256 KiB);
  past any of them the request fails (at start, the start is refused; on a refresh, the cached
  keys stay). An identity whose path starts with `.well-known/oauth-protected-resource` is
  unreachable with `[oidc]`, since that path serves the metadata. A role name of
  `[oidc.github] roles`, and a `role_name` the role check stores, holds at most `ROLE_MAX` (100,
  in `grant.rs`) characters; a longer `role_name` from GitHub is a skipped row. The `scope`
  claim is read as a
  space-separated string; `scp` and arrays are not read. With `audience` unset, the root's 404
  takes a token whose `aud` is `public_url` itself. The caps are inclusive: a body of exactly
  the cap is read. The `oidc` build carries `rsa`, whose RUSTSEC-2023-0071 (Marvin timing) hits
  private-key operations; Meada only verifies with public keys, so it does not apply.
- `meada-core/src/manifest.rs`: a GitHub login (an entry's `added_by`, and a token's `login_claim`
  value, which `grant/permissions.rs` reads through `added_by_problem`) holds at most `ADDED_BY_MAX`
  (39) characters; a longer one is an error in an entry and no GitHub grant in a token.
- `github_app.rs` (#454): an `added_by_time` is compared with the first permissions set's start
  to the second, and a time in that same second counts as before it, so an addition the provider
  attributed within the second the first role check started is `unattributed` too. An addition
  the console attributes after `[oidc.github]` is enabled but before the first role check runs is
  older than the first set and `unattributed`. The stamp is the console host's clock and the first
  set's start Postgres's: a console clock ahead of Postgres by k seconds lets an addition the
  provider attributed in the k seconds before the first set pass as later (and a console behind
  it, one just after as earlier); both are the provider's own attributions, since a save without
  the provider writes no time. Entries saved before `added_by_time` existed carry none and are
  `unattributed` under the provider. The global operator's text is stored as written, so an
  `added_by_time` it writes is trusted as is.
- `audit.rs`: the audit grows without bound, one `<schema>.audit` row per write and refusal, until
  the operator prunes it in SQL; Meada never deletes a record, and a page of `GET /api/v1/audit`
  answers at most 100.
- `github_app.rs`: each REST request takes at most `REQUEST_TIMEOUT` (10 seconds) and reads at
  most `ANSWER_MAX` (4 MiB), a 403's body at most `DENIAL_MAX` (4 KiB, one that fills it counts as a
  throttle, #452), and a 403 with no `x-ratelimit-remaining` at all (a GitHub Enterprise Server
  with rate limiting off) stays a throttle whatever its body; a 403 is a permission denial only
  when a `retry-after` header is absent in every form; an App's installations are listed over at most `PAGES_MAX` pages of
  `PER_PAGE` (100 each; 10,000 installations), the rest unseen; the role check lists one repository's
  collaborators over at most `COLLABORATOR_PAGES_MAX` pages of 100 (100,000 collaborators), past
  which the run fails, writing nothing; with `[catalog]` (#341) it lists one installation's
  repositories over at most `PAGES_MAX` pages of 100 (10,000 repositories), past which it fails
  too (a truncated catalog would revoke), and refuses a candidate set above `max_repositories`
  (at most 5000, no default), costing `ceil(covered / 100)` more requests per installation
  (every repository it covers, allowed or not) and the collaborator pages of each candidate;
  the permissions cache keeps a catalog identity of at most `CATALOG_IDENTITY_MAX` (192) bytes,
  so that the pagination cursor (at most 256 characters of base64url) can name it, and counts
  a longer row as hostile. It costs one listing per repository on the host,
  `ceil(collaborators / 100)` requests each, against the installation's REST budget (#319's
  Known ceilings), and it holds every row of the set in memory once, as Postgres receives it; a `private_key_file` holds at most `APP_KEY_MAX` (16
  KiB) and git's request at most `INPUT_MAX` (64 KiB). The time margins are fixed, not
  configured: `TOKEN_MARGIN` and `REFRESH_EVERY` (10 minutes each), the JWT's `JWT_LIFE` (9
  minutes) and `JWT_BACKDATE` (60 s), `INSTALLATIONS_TTL` (the listed installations stay usable
  an hour while listing fails), `LAST_TTL` (`throttled` finds the installation of a `get` within
  an hour), `LOCK_TTL` (30 s, a listing or mint lock and the wait for another run's) and
  `THROTTLE_MAX` (a mark lasts at most an hour). A lock that expires under its holder lets a
  second run list or mint too. The `github-app` build signs with RustCrypto's `rsa`, whose
  RUSTSEC-2023-0071 (Marvin timing) applies to private-key operations; the JWT is signed in the
  job's process, at most once per App per run, where no remote party times it.
- `mcp.rs`: the stop signal drains for at most `DRAIN` (25 seconds, no flag, under Kubernetes'
  30-second default grace period); past it the connections left are dropped, an answer still
  waiting on an index lock included, and the exit is 0.
- `ui.rs`: `meada-ui.json` is read up to `UI_VERSION_MAX` (4 KiB); a larger file counts as front
  end files built for another version, and the console refuses to start. A token check's body
  is read up to `CHECK_MAX` (16 KiB), and a playground query's up to `QUERY_MAX` (16 KiB); past
  either the answer is a 413. `GET audit` answers at most `AUDIT_LIMIT_MAX` (100) records, `AUDIT_LIMIT`
  (50) without `limit`. Without `[snapshot]` the playground and the exports serve the
  `[[repository]]` entries as read at start (a mirror missing then stays unservable until a
  restart, as on a node). Operational service reads and managed-workspace token checks share
  a two-second request bound, including the wait for one of `READERS_MAX` (4) workers;
  timed-out workers retain their slots until I/O completes, so overload starts no additional
  work. `index.rs` reads optional `index.commit.json` up to
  `PROVENANCE_MAX` (4 KiB); anything larger has no trusted indexed commit.
- `directory/scim.rs`: a request's body is read up to `SCIM_BODY_MAX` (1 MiB), and a stored
  resource (the JSON the provider sent, a group without its members) holds at most
  `SCIM_RESOURCE_MAX` (64 KiB) after every change; past either the answer is a 413. A list
  answers at most `SCIM_PAGE_MAX` (100) resources, `count` above it is lowered to it. Each request opens
  its own Postgres connection (`pool_size` is still unread), and every write holds one advisory
  lock of the schema, so the directory's writes run one at a time: per-resource locks when a
  provider's throughput needs them. A group's answer lists every member.
- `directory.rs`: `token_file` holds at most `TOKEN_MAX` (4096 bytes); past it the console
  refuses to start, naming the file.
- `ui/write.rs`: `POINTER_MAX`, `FENCE_MAX` and `APPLY_MAX` bound their respective POST bodies
  to 1 KiB; `SAVE_MAX` bounds a version save's JSON body to 4 MiB. Over-cap authorized uploads
  get 413 before any operation; unauthorized uploads are never consumed.
- `snapshot/versions.rs`: a workspace version document holds at most `VERSION_MAX` (4 MiB),
  checked both before immutable creation and while reading;
  past it the document is refused (on import, apply and pull, and read from
  `<index_dir>/workspace/`) or listed as malformed, naming its key or file.
- `render.rs`: a source file an answer reads (`render::source`, for `path:line` and a symbol's
  source) is read only through plain relative parts of its path, through no symbolic link, as a
  regular file of at most `SOURCE_MAX` (16 MiB); anything else renders as unavailable, the path
  without its line, as a missing file does. An answer keeps each file it reads, so several
  large files add up.
- `partial.rs` (#396): a query reads every symbol of a repository with a named manifest, since
  an import resolves into it through any of them (`package::Provided`, `ContractKind::
  symbols_read`), and every contract row of every repository, links deriving from them at each
  query; `trace` reads every node its walk may reach, as the walk visits them all before the
  budget cuts the chain, and `context` every node upstream of the symbol for its processes. On
  junit5 (96k calls), a `trace` up that reaches a fifth of the calls reads them through
  indexed lookups and takes longer than a whole read did (no marker).
- `workspace.rs`: a sibling with an invalid `meada.toml` or `meada.decisions.toml` is skipped
  silently. Every read of a mirror's `meada.toml` and `meada.decisions.toml`, in a pulled
  generation and in a server-mode manifest's entry (`mirror()`, behind `Manifest::read`) alike,
  reads only regular files of at most `GENERATION_CONFIG_MAX` (1 MiB, `config.rs`), through no
  symbolic link (`config::read_unlinked`, refused unopened, then the opened file checked against
  the one checked), one that stays inside the mirror included; past the bound, or through a
  link, the generation or the entry is not servable, the error naming the file. A local checkout
  (`Repository::local`, the sibling scan of `Workspace::discover`) is the developer's own tree
  and follows links, and so does an operator's file (a secret, a CA, `config::read_bounded`),
  as Kubernetes mounts secrets through one. Both reads check a file and then open it
  (`read_unlinked` then compares the opened file with the checked one), so a regular file
  replaced by a FIFO between the check and the open blocks the read instead of being refused.
  Reaching the window takes write access to the directory, which already allows worse;
  repository content cannot (git stores no FIFO, and the generation extraction refuses one); a
  FIFO given as the path is refused by the check. Closing it would take
  `O_NONBLOCK | O_NOFOLLOW` and a direct `libc` dependency, declined in #548.
- `decisions.rs`: a repository serving a decision's contract from several sites links to the
  first by site; the target names no site.
- `decider/decisions.rs`: the upstream documentation gives no range for `confidence`; [0,1] is
  assumed.
- `drift_view.rs`: `context`, `search` and `status` find each shown link's consumer call by
  scanning the calls of every repository, once per link (no marker); a query has read only those
  calls (`drift_view::sites`), `status` every call.
