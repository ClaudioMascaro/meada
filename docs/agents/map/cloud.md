# Cloud modules

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

The cloud modules of ADR 0012, each behind a cargo feature (`cloud` enables them all) and a
section of `meada.workspace.toml`:

- `modules.rs`, `valkey.rs`: the cloud modules (ADR 0012). `modules.rs` is the section table,
  in every build: each module section of `meada.workspace.toml` (`[valkey]`, `[snapshot]`,
  `[generations]`, `[refresh_lock]`, `[oidc]`, `[[github_app]]`, `[ui]`, `[postgres]`,
  `[directory]`, `[catalog]`, `[workspaces]`, `[proposals]`, `[accounts]`) with the cargo feature that
  owns it and whether this build has it. `Manifest::read` checks it before the typed parse, so a
  section whose feature is missing is an error naming the file, the line, the section and the
  feature, and a section the table lacks stays an unknown key. A module ticket adds its row and
  sets `built` to `cfg!(feature = ..)`. The `cloud` feature turns on every module at once and,
  as `refresh-lock` is, is Unix only. `modules.rs` also holds what the modules share, each
  behind the features that use it: `Problem` (a section key and its message) and `validated`,
  which turns one into the `file:line` error; `at_least` (an optional integer with a default
  and a minimum, `Msg::AtLeast`); `needs` (a section missing the sections it needs);
  `agent` (the ureq client of every module's outbound HTTP: no redirect, no proxy, a timeout, the
  platform verifier); `loopback` (`localhost`, `127.0.0.0/8` or `::1`, the one host test of every
  `http` exception); `reachable` (an endpoint URL a module may name: `https` or loopback `http`, a
  host, no user or fragment, and past the host what its `Shape` allows: a path, nothing, or a path
  and a query); `Secret` (a variable or a file, at most one of the pair, and `read_secret`, a
  bounded regular file whose error names only the file); `block_on` (a current-thread runtime shut
  down without waiting), `within` (a `status` line's bounded wait) and `fold` (an identity as
  GitHub compares it: lowercase on the `[oidc.github]` host, as written elsewhere, #508).
  `valkey.rs` (feature
  `valkey`, internal; the only place
  the `redis` crate is used) is the one Valkey client the modules share: `File` is the
  `[valkey]` section, `File::validate` turns it into a `Config` (no password in the URL, `rediss`
  outside loopback, at most one of `password_env` and `password_file`, no `{` or `}` in
  `key_prefix`; each problem names its key), and `Valkey::connect` opens a connection manager or
  a cluster connection, reading the password from the variable or the file only when it
  connects. Every key goes through `Config::key(name, tagged)`: `<key_prefix>:<name>`, or
  `{<key_prefix>}:<name>` for the keys one script touches together (pointer, lock, fence). An
  error from the client has the password scrubbed. `status --workspace` prints `valkey:
  reachable` or `valkey: unreachable (<reason>)` after waiting at most `PING_TIMEOUT`; the
  line is informational and never changes the probe's exit code. With a `ca_file` the client
  trusts that file alone, not the system roots as well (redis-rs replaces them).
- `postgres.rs` (feature `postgres`, enabled by `cloud`; ADR 0016's row of ADR 0012's module
  table, section `[postgres]`, needing no other section; the only place `tokio-postgres` and
  `tokio-rustls` are used): the data layer, the durable mutable state of server mode in Postgres
  15 and later. `File` is the `[postgres]` section and `File::validate` its `Config`: `url_env`
  (default `MEADA_POSTGRES_URL`) or `url_file`, at most one; `ca_file`; `schema` (default
  `meada`, lowercase letters, digits and `_`, starting with a letter, 1 to 63 characters);
  `pool_size` (4), `connect_timeout_seconds` (5) and `statement_timeout_seconds` (10), each at
  least 1, the last at most `STATEMENT_MAX` (Postgres's `INT_MAX` milliseconds); every problem names its key. The URL is read only when a process connects
  (`Config::url`), kept in memory, and `parse` accepts only `postgres://` or `postgresql://` with
  a TCP host (a name or an address rustls can verify, so no Unix socket path and no host list),
  optionally a user, a password, a port and a database, and no query or fragment, so no
  `sslmode`: such a refusal, an unset variable or an unreadable file is an error naming
  `postgres.url_env` or `postgres.url_file`, never the URL or a part of it; once connected an
  error never carries the password, but may name the host (a certificate for another name) or
  the user (a refused login). The TLS rule is `over_tls`: a host outside loopback (`modules::loopback`) always connects
  over TLS with its name verified, against `ca_file` alone when set, else the platform verifier;
  a loopback host connects in plain TCP unless `ca_file` is set, then over TLS against that file.
  `open` makes the TCP connection itself and hands it to tokio-postgres (`connect_raw`, no
  `runtime` feature) with its own `TlsConnect` over tokio-rustls and ring (no channel binding),
  the URL read, the TCP connection and the handshake bounded together by
  `connect_timeout_seconds`, every statement bounded by `statement_timeout_seconds`
  (`-c statement_timeout`), the password scrubbed from every error. Every table lives in
  `[postgres] schema` and every statement names it, quoted (`Config::quoted`): Meada writes
  nothing outside it (ADR 0002, invariant 3). `MIGRATIONS` lists the migrations in order; a
  ticket appends the next number when it integrates, never editing one before it. Migration 1
  creates the version table, `<schema>.schema_version` (one row, `version`), the schema itself
  created in its transaction when absent; migration 2 the audit's table, `<schema>.audit`
  (`audit.rs`); migration 3 the directory's tables (`directory/scim.rs`); migration 4 its state row,
  `<schema>.directory_state` (`groups_provisioned`, `last_write`; #335); migration 5 the
  permissions set (#331): `<schema>.permission_sets` (`id`, 32 lowercase hex characters from
  `gen_random_uuid`, `fence`, `version`, `started`, `finished` from Postgres's clock,
  `repositories`), `<schema>.permission_rows` (`identity`, `login` lowercase, `role` as GitHub
  spells it) and `<schema>.permission_exclusions` (`identity`, `added_by`), both cascading from
  their set; migration 6 `permission_repositories.source` (#341); migration 7 the named
  workspaces' tables (#342); migration 8 `workspace_versions.entries` (#343); migration 9 the
  local accounts' table, `<schema>.accounts` (#554, `accounts/store.rs`).
  `connected` is `connect` keeping the password for scrubbing a later statement's
  error (`Opened::scrubber` a copy of it for a transaction, which borrows the client), and `probe` is the console's start check (a version refusal refuses the start, Postgres
  unreachable does not). `migrate` (`meada postgres migrate --workspace
  <bootstrap manifest>`, reading `Manifest::postgres`, no mirror) is the only command that
  creates or alters a table: it takes `pg_advisory_lock` on `lock_key(schema)` (the first eight
  bytes of BLAKE3 of `meada postgres migrate <schema>`, so two runs serialize; the wait runs
  with `statement_timeout` off, so a run waits for the one before it whatever it takes), applies each migration from the stored
  version to the binary's in its own transaction, prints the version reached and refuses a
  stored version above the binary's, never lowering it. The version check, `connect`, is the
  one function every other connection goes through: it reads the version (`stored`, 0 for an
  absent schema or table) and `check` refuses one the binary does not know, lower naming `meada
  postgres migrate`, higher naming both versions. A CLI command or the console refuses on it
  (exit 1, or no start); a node writes it once on stderr and treats Postgres as unreachable,
  keeping its readiness (the permissions cache tells that refusal apart through `versioned`). In debug builds
  `MEADA_POSTGRES_VERSION_MAX` caps the version the binary knows (`known`), a test knob as
  `MEADA_DRAIN_MS` is, ignored by release builds. `status --workspace` prints `postgres:
  reachable, schema <name> version <n>` or `postgres: unreachable (<reason>)` within
  `connect_timeout_seconds` (a readiness probe running `status --workspace` waits that long for
  a Postgres that does not answer, where the Valkey line waits at most `PING_TIMEOUT`), a version refusal being the reason, since every process treats it
  as unreachable; the line never changes the exit code, as the Valkey line does. Who connects:
  the console (audit, directory, permissions, overage, local accounts), nodes with `[oidc.github]` or overage
  through the directory (reads only), the job's `meada github-app roles` and the CLI twins that
  write; `meada index` never does. Three Postgres roles are recommended: one for `migrate`,
  which owns the schema; one that writes (the console, the role check, the CLI twins); one that
  reads (nodes). `pool_size` is read by the callers that pool connections; `migrate` and
  `status` open one.
- `directory/read.rs` (feature `ui` on top of `directory`; #335): the console's readers of the
  directory, one statement per call on `postgres::connected`, never cached. `deprovisioned`
  (the token's `user_claim` value against `user_attribute`: `userName` ignoring case,
  `externalId` exact; an inactive user or a revocation marker) answers the admission;
  `groups(prefix)` is the owner picker (`starts_with`, exact, byte order, at most `PICKER_MAX`
  100); `named` is which of the given owners name a group, `None` while `groups_provisioned` is
  false; `counts` is the admission view's users, groups and last write. `ui/directory.rs` is the
  console's side: `directory_refusal` (a deprovisioned user is a 403 `deprovisioned` on every
  route but `config`, before the role is computed and for a team operator too; an unreadable
  directory admits a read, and refuses a write as 503 `audit_unavailable` before any audit
  insert, with one `directory: unreadable (<reason>)` line that names no claim), the route
  `GET /api/v1/directory/groups?prefix=` (the global operator alone; a prefix over `OWNER_MAX`
  is a 400), `flag_missing_owners` (`owner_missing: true` on the entries of `workspace` and
  `workspace/versions/<id>` whose owner names no group; nothing flagged before a group was ever
  provisioned, ownerless entries never flagged, no flags and one stderr line when the read
  fails), `flag_excluded` (`ui/read.rs`, #333: `excluded: true` on the entries of `workspace`
  and `workspace/versions/<id>` whose identity the newest permissions set excluded, only for the
  version that set checked and only after the grant bounded the entries; the field is present
  only when true, so without `[oidc.github]` every body is as before) and `with_directory_counts`
  (`GET /api/v1/admission` gains `directory`: `users`,
  `groups`, `last_write`; `null` when unreadable, absent without `[directory]`).
- `directory.rs`, `directory/scim.rs` (feature `directory`, implying `postgres`, enabled by
  `cloud`, no crate beyond `postgres`'s; ADR 0016's `directory` row of ADR 0012's module table;
  #334): the directory, users and groups the identity provider pushes in over SCIM 2.0.
  `directory.rs` is the `[directory]` section, which needs `[postgres]` and `[oidc]`
  (`modules::needs`); `File::validate` gives its `Config`: `token_env` (default
  `MEADA_SCIM_TOKEN`) or `token_file`, at most one (`Msg::DirectoryBoth`); `user_claim`,
  required and non-empty; `user_attribute`, required, `userName` or `externalId`;
  `group_attribute`, `displayName` by default or `externalId` (`Msg::OidcOneOf`). Every problem
  names its key, never a value. `Config::token` reads the token, only in the console and only at
  start (`Console::start`): unset, unreadable or shorter than `TOKEN_MIN` (32) characters refuses
  the start naming the key (`Msg::DirectoryTokenShort`), never the value; rotating it means a
  restart. Nodes parse the section and never read the token; `user_claim` is read by the console's
  deprovisioning (`ui/directory.rs`) and, for nodes and the console alike, Entra overage
  (`oidc/overage.rs`, #336).
  `directory/scim.rs` (with `ui`) is the endpoint the console serves under `/scim/v2/` with
  `[ui]` and `[directory]` (`Console::scim`, routed before the console's admission). Admission
  in order: S2's `Host` check (the plain 403); a 403 for any request carrying `Origin`; the
  `Authorization: Bearer <token>` (`Scim::admits`, comparing BLAKE3 hashes, which is constant
  time and keeps no token in memory), else a 401 with `WWW-Authenticate: Bearer`. OIDC admission
  never applies there, and the SCIM token is no console token under `/api/v1/` (a 401 there).
  The body of a `POST`, `PUT` or `PATCH` is read only after admission, at most `SCIM_BODY_MAX`
  (1 MiB, else a 413 without `scimType`), and must be `application/scim+json` or
  `application/json` (else a 415). Operations (`Scim::answer`): `/Users` and `/Groups` take
  `POST` (201, `Location`), `GET` with `filter` (`userName eq "<v>"`, compared ignoring case, or
  `externalId eq`; `displayName eq` or `externalId eq` for groups; attribute names and `eq`
  ignoring case), `startIndex` (1 by default) and `count` (`PAGE_MAX`, 100, by default and at
  most), ordered by creation, and `GET`, `PUT`, `PATCH` and `DELETE` (204) of `/<kind>/<id>`. A
  user `PATCH` answers 200 with the user, a group `PATCH` 204; `excludedAttributes=members`
  leaves a group's members out. `PATCH` (`operations`, `apply`) takes `add`, `replace` and
  `remove` (ignoring case) with or without a path; a path (`path`) is an optional schema URN
  (the core schemas' dropped), an attribute, an optional `[<sub> eq <value>]` filter and an
  optional `.<sub>`, names compared ignoring case; a `remove` of `members` with a value list
  removes those values, an `add` through a filter that matches nothing adds the value it
  describes, a `replace` through one is `noTarget`. Every operation applies to the resource in
  memory and the result is stored as one write, so a later failure writes nothing.
  `/ServiceProviderConfig`, `/ResourceTypes` and `/Schemas` are fixed documents (`patch`,
  `filter` with `maxResults` 100; no `bulk`, `sort`, `etag` or `changePassword`; the bearer
  token scheme). Storage: migration 3's `directory_users`, `directory_groups`,
  `directory_members` and `directory_revoked`; a resource is the JSON the provider sent
  (`candidate`) without `id`, `meta` and `password` (dropped wherever they appear, so the
  provider can change none of them), a group without `members` (rows of `directory_members`,
  each naming a stored user, else a 400 `invalidValue`; a deleted user's rows go with it), at
  most `SCIM_RESOURCE_MAX` (64 KiB, else a 413) after every change; `userName` is required, `active`
  is a boolean or `"True"`/`"False"`, true when absent, and an inactive user is returned like
  any other. Ids are Postgres's (`gen_random_uuid` without dashes: 32 lowercase hex characters),
  checked wherever read (`is_id`, `Stored::of`): a path id that fails is a 404, a row that fails
  is skipped. Each answer carries `meta` (`resourceType`, `created` and `lastModified` from
  Postgres's clock, `location` from `[ui] public_url`). Every write is one transaction holding
  one advisory lock of the schema (`Scim::begin`), so the uniqueness checks (`store`: a
  `userName` ignoring case or an `externalId` another user holds, a `group_attribute` another
  group holds, exactly; a 409 `uniqueness`), the members check and the write cannot interleave
  with another write. A deleted user leaves every group and a revocation marker (its `userName`
  and `externalId` in `directory_revoked`), separate from the deleted resource, which a write
  naming `active` true for a user whose `user_attribute` matches it (a `userName` ignoring case)
  clears (active by default is not enough); the console's admission reads the markers
  (`directory/read.rs`). Every committed write also stamps `directory_state.last_write`, and the
  first group stored sets `groups_provisioned` for good (a deleted group does not unset it).
  Errors are RFC 7644's, with `scimType` (`invalidFilter`,
  `invalidPath`, `invalidValue`, `noTarget`, `uniqueness`, `invalidSyntax`) for a 400 or 409, an
  unknown id a 404, and a fixed detail that echoes no value; a value Postgres cannot store
  (SQLSTATE class 22, as a NUL) is a 400 `invalidValue`. Postgres unreachable, or a schema
  version this binary does not know (`postgres::connected`), is a 503 in the same schema after
  one `ui: postgres: unreachable (<reason>)` line; the console's other routes are unaffected.
  Each `POST`, `PUT`, `PATCH` or `DELETE` under the prefix leaves one stderr line `scim:
  <method> <resource> <id>: <status>`, `<resource>` `Users`, `Groups` or `-`, `<id>` the stored
  resource the write named or `-` (a failed `POST`, an unknown id, a refusal before routing):
  no payload, attribute value, group name or token reaches stderr. The console's `manifest`
  route shows `[directory]` as written, the token's variable or file name (the default
  `token_env` filled in) and never its value. `/Schemas` describes the comparisons: `userName`
  ignoring case, the `group_attribute` unique.
- `oidc.rs` (feature `oidc`; with `github_app.rs` the only places `jsonwebtoken` is used, with
  RustCrypto as its backend, never aws-lc-rs): server mode as an OAuth 2.1 resource server. `File`
  is the `[oidc]` section, the identity provider (ADR 0016: the one token issuer of the
  deployment), and `File::validate` its `Config` (`type` one of `TYPES`, `generic` by default,
  `entra`, `okta` or `keycloak`, any other value or a value that is not a string the error naming
  the four and never the value; `[oidc.entra]` only under `type = "entra"`, its one key `overage`
  one of `OVERAGES`: `ignore`, a token in overage having no groups, or `directory` (#336), which
  needs `groups_claim` (`Msg::KeyMissing`), a build with `directory` (`Msg::NeedsFeature`) and
  `[directory]` (`Manifest::read`, which hands it `[directory]` and `[postgres]`,
  `Config::overage`, `Msg::NeedsSections`), each error at `oidc.entra.overage`; `okta` and
  `keycloak` take no table and, as `entra`, change no answer; `[oidc.github]` the GitHub grant
  provider, `GithubFile::validate` its `login_claim` and its `grant::Github` (`host` a lowercase
  host name, `github.com` by default; `login_claim` required and not empty; `roles` a non-empty
  array of distinct names that `grant::role_ok` takes, 1 to `ROLE_MAX` characters with no
  control character, `ROLES`, GitHub's
  five built-in roles, by default; `max_age_seconds` required, at least `MAX_AGE_MIN`), in a
  build without `postgres` the error naming the feature, without `[postgres]` the error naming
  both (`workspace::Manifest::read`, which also hands it its `[postgres]`, `Github::postgres`);
  beside it neither claim is needed; `issuer` `https` or loopback `http`
  with no user, query or fragment, `public_url` the same with no path (`modules::reachable`),
  `identities_claim` and `audience` not empty, `groups_claim` (not empty) and its `[oidc.groups]`
  table together, at least one of the two claims or `[oidc.github]`, scopes RFC 6749 scope-tokens,
  `jwks_refresh_seconds` at least 60). `Oidc::start`,
  called by `mcp::Gate::new` before the listener binds, finds the metadata at the URLs the MCP
  specification tries in order (`metadata_urls`: RFC 8414 and OpenID discovery with the path
  inserted, then OpenID's appended, passing over a document that does not parse or names
  another issuer), checks its `jwks_uri` (which may carry a query), reads the JWKS (only
  signing keys: RSA, or EC on P-256 and P-384) and starts a thread that reads it again every
  `jwks_refresh_seconds` until the module is dropped; every request goes through ureq with no
  redirect, no proxy, `FETCH_TIMEOUT` and a cap, and the platform verifier for `https`.
  `Oidc::admit` (reached only without `Origin`, which `mcp::route` refuses first) serves the
  metadata of any path under
  `.well-known/oauth-protected-resource` (`resource` is `<public_url>/<path>`, percent-encoded),
  and otherwise reads the one `Authorization: Bearer` header (another scheme counts as no
  token): `ALGORITHMS` only, no `crit`, a cached key that `fits` the header (`kid` when both
  carry one, the key's own `alg`, its type and curve), the JWKS fetched again
  when none does (`refetch`, at most once per `KID_REFRESH_EVERY`), `exp` and `nbf` with
  `LEEWAY`, `iss`, `aud` against the configured audience or the path's and its identity's
  resources, then the scopes (403 when one is missing). The grant (`Oidc::granted`, once per
  request, on nodes and in the console; `Granted::union`) is computed by the grant providers
  (`grant.rs`, below) from what the claims name: `Oidc::groups` and `login_claim`'s value when it
  is a string go to `Providers::grant`, and the identities `identities_claim` lists join its
  groups side (`claims_listed`, `Granted::listing`); a claim counts only as an array of strings,
  `*` from either side for all. That groups side is the identity provider's claims grant, and
  `Checked::sources` shows it beside the GitHub grant. Under `[oidc.entra] overage = "directory"`
  the groups are `Oidc::groups`: the `groups_claim` array, or, for a token in overage, the
  directory's (see Entra overage in [Known ceilings](../ceilings.md)). The OIDC module reads no permissions set and keeps no
  cache: `Oidc::start` takes the process's `grant::Providers`. `[oidc.github]` validates into
  `grant::Github` (`Config::github`, read through `Manifest::github`) and the OIDC-only
  `Config::login_claim`; the admission view's `github` is `Providers::admission` with
  `login_claim` added. Routing checks the grant and it bounds the workspace of each answer and
  each 404 list (`mcp.rs`). No token, error text
  of `jsonwebtoken` or claim value is ever printed. With feature `ui`, the console reaches the
  same checks through `admit_console` (one audience, `[ui] audience`; a `Bearer` challenge with
  no `resource_metadata`; its `groups`, the session's, and the `operator_groups` match read the
  same effective groups, those `Oidc::granted` read; it returns a `grant::Admitted`), `check`
  (a pasted token's outcome per check, signature, issuer,
  audience as a node checks it, lifetime and scopes, and its grant when every check passes,
  against the cached keys only, so it never fetches; with `[oidc.github]` its two sides,
  `Checked::sources`, from the cache) and `admission` (the identity provider's
  `type`, always present, the issuer's metadata as read at start, the cached key ids, the last
  read and a refresh that failed since, with its reason and never the answer's body, the
  groups table bounded by the caller's grant, and with `[oidc.github]` `github`: the host,
  `login_claim`, `roles`, `max_age_seconds`, the grant's state and the loaded set's id,
  `finished`, `age_seconds` and the counts the job wrote, `repositories`, `rows` and
  `exclusions`, or `null` before one is loaded).
- `grant.rs` (feature `oidc`): the grant providers (ADR 0016), fed by neutral inputs so that any
  verifier of a subject is a producer of them and none duplicates a grant. `Grant` is what an
  admitted subject reads (`All` or `Only`, `reads`, `core`); `Github` the GitHub grant provider's
  settings (`host`, `roles`, `max_age`, its `postgres` and `catalog` filled by `Manifest::read`),
  which every reader (the node's and the console's grant providers, the session and the admission
  view, `meada github-app roles`, the catalog, named workspaces, the proposal server, `status`
  through `Manifest::excludes`) reaches through `Manifest::github`, the one accessor a later
  section can also fill; `role_ok` and `ROLE_MAX` the role names it takes. `Providers` is a
  process's providers, built once from its manifest (`Providers::of`: the groups table of
  `[oidc.groups]`, or of `[accounts.groups]` (#558; never both, since `[accounts]` sits beside
  `[oidc]` only under break glass, which refuses its groups), and `Manifest::github`) by
  `mcp::Gate::new`, `ui::Console::start` and
  `Console::for_proposals`, shared through an `Arc` with the verifier, and its permissions
  cache's poll starts at `Providers::listening` (called by `mcp::run`, `ui::listen` and the
  proposal server's listener once the address is printed). `Providers::grant(groups, login)` is
  the grant, with no reference to a token: the groups side (`Granted::claims`, what the table
  maps `groups` to, a group it lacks granting nothing, `*` all) and, with the GitHub grant
  provider, the GitHub side (`Granted::github`, `GithubGrant`), never `*`: the identities the
  login holds an accepted role on in the permissions cache, with the grant's state read at the
  same instant. The login is kept (`Granted::login`) only under the GitHub grant provider and
  only when it is 1 to 39 ASCII letters, digits, `-` and `_`, lowercased
  (`permissions::login`); any other value gives no GitHub grant. `Granted::union` is the grant
  every check reads. `Providers::github` (the host), `catalog`, `excluded`, `excluded_in` and
  `admission` are what the console reads of the cache. `Admitted` (feature `ui`) is the neutral
  admitted subject (subject, global operator flag, groups sorted without repeats, login, grant,
  and under the GitHub grant provider its host and the grant's state), built by
  `Admitted::new` from a `Granted`: `Oidc::admit_console` produces one (with the token's
  `user_claim` beside it, the directory's question), and `ui::Session::admitted` builds the
  console's and the proposal server's session from it alone. The permissions cache
  (`grant/permissions.rs`, feature `postgres`, `Permissions`) is empty until the listener
  printed its address, then a thread reads, every `PERMISSIONS_POLL` (60 s),
  in one `REPEATABLE READ READ ONLY` transaction through `postgres::versioned`, the newest set's
  id, `finished` and age on Postgres's clock (`clock_timestamp() - finished`) and, only when the
  id changed, its counts, its rows whose `role` is in `roles`, keyed by login, its `version`
  and its exclusions' identities (`Permissions::excluded`, read by the console whatever the
  set's age, #333; a `version` that is no version id marks nothing, with one
  `Msg::PermissionsBadRow` line, and the set still grants). The age rule:
  the age at the last successful read plus the process's monotonic time since, a read of the
  same set never lowering it (`Set::seen`), so neither clock moving back extends a set; at most
  `max_age_seconds` the GitHub grant is honored (`current`), past it empty (`expired`), and
  before a set is loaded empty (`unread`). A read is bounded by `postgres::Config::bound` (the
  connect timeout plus one statement timeout per statement), so a Postgres that stops answering
  without closing the connection cannot stall the poll. A failed read is one `Msg::PermissionsUnreadable`
  line per poll and keeps the set aging; a schema version this binary does not know is one
  `Msg::PermissionsVersion` line, once, and counts as unreachable, readiness unchanged. Every row
  is hostile: a set whose id, `finished` (ahead of Postgres's clock included) or counts fail
  their checks, and a row whose identity is not on `host` or not a safe identity, whose login or
  role fails its pattern, is skipped with one `Msg::PermissionsBadRow` line naming its table.
  In debug builds `MEADA_PERMISSIONS_POLL_MS` shortens the poll and lowers the floor of
  `max_age_seconds` to 1 (`oidc::max_age_min`), a test knob as `MEADA_DRAIN_MS` is.
- `accounts.rs`, `accounts/store.rs` (feature `accounts`, implying `oidc` and `postgres`, enabled
  by `cloud`; ADR 0024's `accounts` row of ADR 0012's module table; the only place `argon2` is
  used; #554): local accounts. `File` is the `[accounts]` section and `File::validate` its
  `Config`: `public_key_env` or `public_key_file`, `private_key_env` or `private_key_file`,
  `admin_password_env` (`MEADA_ADMIN_PASSWORD` by default) or `admin_password_file`, at most one
  of each pair (`Msg::AccountsBoth`), names only, `break_glass` (false by default, #563) and
  `groups` (`[accounts.groups]`, #558: a group to the identities it grants, `*` for all, read as
  `[oidc.groups]` is and handed to `grant::Providers::of`; without it, nor the GitHub grant
  provider, every local account reads every repository) and `github` (`[accounts.github]`, #561:
  `host`, `roles` and `max_age_seconds` checked by `oidc::github_provider`, the one check
  `[oidc.github]` shares, with this table's keys in its errors, and `api`, `GITHUB_API` by default
  on `github.com` and required on any other host (`Msg::KeyMissing` at `accounts.github`), `https`
  or loopback `http` (`modules::reachable`, `Msg::OidcUrl` at `accounts.github.api`), kept without
  its trailing slash in `Config::api`; `Manifest::read` gives `Config::github` its `[postgres]`,
  and `Manifest::github` returns it when there is no `[oidc.github]`, so the grant, the role
  check, the exclusions, `status` and the console read either table, never both).
  `Manifest::read` refuses, before the sections are parsed, `[accounts.groups]` or
  `[accounts.github]` under `break_glass = true` (`under_break_glass`,
  `Msg::AccountsUnderBreakGlass`, naming the table); then the section without `[postgres]`
  (`modules::needs`), beside `[oidc]` without `break_glass` (`Msg::AccountsBesideOidc`), and
  `break_glass` without `[oidc]` (`Msg::AccountsBreakGlassNeedsOidc`, at `accounts.break_glass`),
  and gives it its `[postgres]`. `issued_by` reads a token's `iss` without any check
  (`jsonwebtoken::dangerous::insecure_decode_claims`), only to route a bearer to its one
  verifier under break glass. `Config::keys` reads the key pair, only in the console and only at
  start (`Keys::fingerprint`, #558: `blake3:` and the hex BLAKE3 hash of the raw 32-byte public
  key, what the admission view shows): a key
  not named (`Msg::AccountsKeyMissing`), an unset variable, an unreadable file (`KEY_MAX`), a key
  that is no Ed25519 key in PEM (`jsonwebtoken`'s `from_ed_pem`) or a private key that does not
  sign what the public key verifies (a probe token) refuses the start naming the key, never its
  value; the proposal server parses the section and reads neither key, and a node with no
  `[oidc]` reads the public half alone (`Config::public`, `Config::agents`, #560), never the
  private one.
  `Config::admin_password` reads the initial password only when the default account is created
  (an unset or empty variable is none; a file past `PASSWORD_FILE_MAX` refused naming it). `Keys`
  signs and verifies session tokens: EdDSA, `iss` the console's `[ui] public_url`, `aud`
  `CONSOLE` (`meada:console`), `sub` `local:<username>` (`SUBJECT_PREFIX`), `role` (`Role`,
  `operator` or `viewer`), `groups`, `login` (the verified login, #561) only when the account has
  one, `change_required` only when set, `iat`, `exp` `SESSION_HOURS` (8) later, for an
  `accounts::Holder` (username, role, groups, login); `Keys::agent` signs an agent token (#560): the
  same claims with the surface's audience (`NODES`, `meada:nodes`), a random `jti` (16 bytes from
  `OsRng`, hex), no
  change-required mark, `exp` the hours asked later; `Public::verify` (the public half, which
  `Keys::verify` and `Agents::admit` call) takes EdDSA alone, no `crit`, the issuer, the audience,
  `exp` and `nbf` with `oidc::LEEWAY`, a local subject whose username passes `username_ok` (1 to
  `USERNAME_MAX`, 64, lowercase ASCII letters, digits, `.`, `-`, `_`), groups that are owner
  names and a `login`, when present, that `grant::login_ok` takes (a lowercase GitHub login).
  `policy(password, username)` is the password policy, one pure function returning the first `Rule`
  failed: fewer than `PASSWORD_MIN` (12) or more than `PASSWORD_MAX` (128) Unicode
  scalar values, the default password, the username ignoring case, an entry of the
  common-password list ignoring case (`Rule::Common`, #555: `accounts/common-passwords.txt`,
  embedded with `include_str!`, the entries of 12 to 128 characters of SecLists' million most
  common passwords, lowercase and sorted by bytes for a binary search, its source, commit,
  checksum, filter and MIT licence in `accounts/common-passwords.md` beside it); `Rule::Current`,
  the new password equal to the current one, is the change route's own check; each rule has a
  `code` and a message naming it, never the password. `hash` is argon2id at 19 MiB, two iterations, one
  lane, a fresh salt from `OsRng`, the parameters in the PHC string; `verify` checks with the
  stored string's own parameters, and `is_hash` is what a stored hash must be (argon2id, its
  parameters within `HASH_MEMORY_MAX_KIB`, `HASH_ITERATIONS_MAX` and `HASH_LANES_MAX`, so a
  hostile row never costs more than a bounded hash, `verify` refusing any other); `weaker` is a
  stored hash with less memory, fewer iterations or an argon2 version before 0x13 than the
  console's own, redone at the next sign-in that proves it; `dummy` is a fixed hash computed once
  at start, which an unknown username, a locked and a suspended account are checked against;
  `generated` is the password `meada ui reset-admin` makes when the setting names none
  (`GENERATED_LENGTH`, 24, of 32 symbols from `OsRng`, 120 bits, one the policy takes). `store.rs` is the
  table, every call its own connection through `postgres::connected` bounded as the audit's:
  `attempt` (#555: one transaction holding the account's row, `SELECT ... FOR UPDATE`; a row is
  hostile: a username other than the one asked or failing `username_ok`, a hash that is no
  argon2id string, a role or a group that fails its check is `Attempt::Bad`, admitting nothing,
  its reader writing one `Msg::AccountsBadRow` line; a lock in force (`locked_until` past
  Postgres's `now()`) or a suspended account is `Attempt::Unchecked`, the row released at once,
  nothing counted and the lock never extended; otherwise the caller's check hashes the password
  under the row's lock, `Attempt::Busy` when it has no permit; a wrong one adds to `failures`,
  and the one that reaches `LOCKOUT_FAILURES` (10) sets `locked_until` `LOCKOUT_MINUTES` (15)
  later and `failures` back to 0, `Refusal::LockedNow`; a right one clears both, so parallel
  attempts are counted one after another and never pass ten), `rehash` (the new hash only while
  the weaker one is still stored), `reset_admin` (an upsert of `admin`: an operator, active, its
  count and lock cleared, the given hash with a change required, `password_changed` now; whether
  it created the row), `set_password` (the new hash, no
  change required, `password_changed` now, only on an account not suspended), account
  administration's `list` (every row by username with `Listed`'s columns, never the hash; a
  timestamp as the audit's, a lock that has ended as none, `created_by` none or a local subject,
  the row checked as `attempt` checks one, its hash and groups read for that alone),
  `create` (`ON CONFLICT DO NOTHING`, `Changed::Taken`), `change` (`Change::Role`, `Suspend`,
  `Reactivate`, `ResetPassword`, which also clears `failures` and `locked_until`, in one
  transaction under the same advisory lock as `bootstrap`, the row held `FOR UPDATE`: a
  suspension or demotion of an active operator with no other active operator whose row passes
  its checks is `Changed::LastOperator`, nothing written, so two at once are decided one after the other),
  `standing` (the role, groups and suspension a console write acts with) and `bootstrap`
  (in one transaction under `pg_advisory_xact_lock` of the first eight bytes of BLAKE3 of
  `meada accounts <schema>`: when the table is empty, `admin`, an operator, with the hash and
  change required its caller gives; a table holding a row is never touched and the initial
  password never read).

- `github_app.rs` (feature `github-app`, which needs `valkey`; `jsonwebtoken` with `use_pem`
  only here): git's credential helper for the indexing job (ADR 0013). `File` is one
  `[[github_app]]` block and `validated` checks them all (`host` a git host name, lowercase; `api`
  `https` or loopback `http` with no user, query or fragment (`modules::reachable`); `app_id` at
  least 1; one of `private_key_file` and `private_key_env`; one App once per host; `[valkey]`
  required through `modules::needs`), each problem at the line of its own block (`error_in`).
  `workspace::Manifest::github_apps` reads only `[valkey]` and the blocks, never a mirror, since
  the helper runs on every git request. `credential` reads git's request strictly (`request`:
  `key=value` lines to a blank line, at most `INPUT_MAX`, a fixed error that echoes nothing) and
  the path as `<owner>/<repo>` (`repository`, lowercase); `get` lists each App's installations
  when its `listed` mark has expired (`Run::refresh`, under a lock, at most every
  `REFRESH_EVERY`, replaced in one `EVAL`), takes the owner's installations ordered by App id
  and installation id, skips the throttled ones (all of them are tried when every one is),
  picks one through an `INCR` counter, and answers its cached token or mints one under a
  per-installation lock (`Run::token`), caching it until `TOKEN_MARGIN` before it expires; a
  throttled mint passes to the next installation, and a `get` fails when every mint is throttled.
  `erase` drops the cached token equal to the password git sends; `store` does nothing. `throttled`
  marks the installation the repository's `last` key names; a URL it refuses is never echoed, since
  it may carry a token. A 403 or 429 marks for `retry-after`, else until `x-ratelimit-reset` when
  `x-ratelimit-remaining` is 0, else for a minute doubled while the mark holds (`Run::mark`).
  Every key is `<key_prefix>:github-app:<host>:...` through `valkey::Config::key` (`Run::key`,
  `app_key`, `installation_key`, `last_key`, one helper per shape): `app:<id>:`
  `installations` (a hash, owner to installation id), `listed`, `listing`, `throttled`;
  `installation:<id>:` `token` (`<expiry> <token>`), `mint`, `throttled`;
  `owner:<owner>:next` (the counter, the only key with no TTL); `repo:<owner>/<repo>:last`. A
  token must be visible ASCII before it is cached or answered, so the API cannot inject a
  protocol line. Errors carry an HTTP status or a URL, never a body, and the bearer (the JWT, or
  an installation token) is scrubbed from ureq's (`valkey::scrub`).
  The role check (#331, features `github-app`, `postgres` and `oidc`): `roles` (`meada
  github-app roles --workspace <bootstrap manifest>`) needs `[oidc.github]` or `[accounts.github]`
  (#561, through `Manifest::github`; `Msg::RolesNoGithub`)
  and a `[[github_app]]` block for its `host` (`Msg::RolesNoApp`, at the table's line), reads
  the workspace as `index --workspace` does but whole (`checked_entries`: with `source = "store"`
  the pulled version through `versions::pulled`, never `eligible`, its id the set's `version`),
  and takes its fence from `MEADA_REFRESH_FENCE` under `[refresh_lock]` (`run_fence`, a positive
  decimal within `bigint`, else `Msg::RolesFenceEnv`), 0 without. `check` then runs in this
  order: `postgres::connected` (the schema version refusal), the newest set's fence read with
  Postgres's `now()` as `started` and a higher one refused (`Msg::RolesFenceBelow`); the
  exclusions file removed; every identity on the host listed (`listed`: the owner's
  installations in `Run::rotation`'s order, the round robin `get` shares; a 403 or 429 marks the
  installation and passes to the next, every one throttled, a 5xx, a timeout, an answer past
  `ANSWER_MAX` or a page that is not a JSON array fails the run with `Msg::RolesFailed` naming the
  identity and why; a 404 (`Failure::Missing`) and an owner no installation covers are a warning
  and no rows; an identity on the host that is not `<host>/<owner>/<repo>` likewise; with
  `[catalog]` (#341, `github_app/catalog.rs`) the identities are the workspace's own, in order,
  then the catalog's candidates alone, sorted, each recorded with its `source`: the candidates
  come first, from every installation of the host's Apps whose owner a pattern names, every
  page of `GET /installation/repositories`, filtered by `[catalog] allow`, and a failed or
  throttled listing, a body that is not the documented object, more than `PAGES_MAX` pages or
  more candidates than `max_repositories` besides the selected ones fails the run before the
  transaction; the line on stdout then reads `Msg::RolesSetCatalog`; `targets_of` compares an
  identity on the host with the candidates and the other selected ones folded (`modules::fold`,
  #508), so one GitHub repository is one `source` row keyed by its first written spelling, `both`
  when a candidate; `spelled` copies the role rows GitHub listed under that key to every other
  selected spelling, since grants match exactly, and the exclusion check and the sponsor rule read
  each spelling's own rows); one
  transaction under `LOCK TABLE permission_sets IN EXCLUSIVE MODE`, which serializes writers and
  lets readers see the old set or the new one, checking the fence again, inserting the set, its
  repositories' coverage (`Shown`: `listed`, `not_found`, `uncovered`, plus `not_a_repository`),
  its rows (`unnest` of three arrays) and its exclusions with their reason (`no_role`,
  `role_not_accepted`) and deleting every older set, all under
  `<index_dir>/workspace/excluded.lock` (`versions::lock_excluded`; the file is removed again
  once the fence passes, a refused run leaving it as it is), held until the file is written by rename (`versions::write_excluded`) only while
  the set still exists, else `Msg::RolesReplaced`: on one machine a file is always that of the
  last run to commit. Postgres and the file are never
  written atomically together: a run that fails anywhere after the removal leaves no file, so no
  reader admits a previous check's exclusions for the same version, and the local recovery is
  to run the role check again. `collaborators` lists one repository with its token: the URL is
  always `Listing::url` (`affiliation=all`, `PER_PAGE`, the page), and `Listing::next` reads the
  `Link` header's `rel="next"` as hostile, never echoed: on the REST base's origin with no user,
  at `/repos/<owner>/<repo>/collaborators` or GitHub's `/repositories/<id>/collaborators` (one
  id for every page), naming a `page` not requested yet (`Msg::RolesLinkForeign`,
  `Msg::RolesLinkCycle`), at most `COLLABORATOR_PAGES_MAX` (`Msg::RolesPages`); only the page
  number is followed, so the token reaches no other URL. `collaborator` keeps a row whose login
  `meada_core::manifest::added_by_problem` takes once lowercased and whose `role_name`
  `grant::role_ok` takes, else one line naming the identity (`Msg::RolesSkipped`); rows are keyed
  by identity and login, the first kept. An entry with `added_by` on the host whose login holds
  no role in `roles` among its repository's rows is excluded (`Msg::RolesExcluded`, the reason
  and never the login). The line on stdout is `Msg::RolesSet`: the set's id, the repositories
  listed, the rows and the exclusions.
