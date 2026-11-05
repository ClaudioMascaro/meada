# Management console

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

## The console process (`ui.rs`)

- `ui.rs` (feature `ui`, no crate of its own; `cloud` enables it): the management console's
  process (ADR 0014), `meada ui --listen <addr> --workspace <bootstrap manifest>`,
  on the node's listener (`mcp::serve_http`, `mcp::address`, the same drain); never a node's
  route, never an MCP server. With `[directory]` (feature `directory`) a request under
  `/scim/v2/` goes to the directory's endpoint (`Console::scim`, `directory/scim.rs`) with its
  own admission, before the console's. `File` is `[ui]` and `validated` lists every problem of it at
  once (`dir` and `public_url` required, `public_url` `https` or loopback `http` with nothing
  past the host through `modules::reachable`; `audience` defaults to `public_url`; `client_id`,
  `operations` defaults to false and requires `[oidc]` plus one operator key when true, or
  `[accounts]` without `[oidc]` and no operator key (ADR 0024; under break glass `[oidc]` is
  there, so the operator key is still required);
  `operator_groups` and `operator_claim` are mutually exclusive and require `[oidc]`, and the
  groups key also requires `oidc.groups_claim`). `Console::start` refuses, listing every problem: a manifest error, no `[ui]`, `[oidc]`
  without `[ui] client_id`, `[oidc]` whose issuer's metadata or JWKS cannot be read
  (`Oidc::start`, as a node), under break glass an `[oidc] issuer` equal to `[ui] public_url`
  (`Msg::AccountsBreakGlassIssuer`, at `accounts.break_glass`, so an issuer always tells the two
  apart), a `[ui] dir` that is not a directory, and the version check:
  `meada-ui.json` at the root of `[ui] dir` must be `{"version": "<this Meada version>"}`, and
  a missing, oversized (`UI_VERSION_MAX`) or unreadable file counts as another version, both
  versions named. Each request goes, in order, through the `Host` check (403 unless `Host` is
  `public_url`'s host and port, or the bound address on loopback), the `Origin` check (403 for
  an `Origin` other than `public_url`, and for a write, any method but `GET` and `HEAD`, without
  one), admission (`Console::admit`), then routing; the 403 bodies are fixed text. Admission:
  behind the operator's proxy every request is a viewer's that reads everything; with
  `[accounts]` (feature `accounts`, #554) the console's own session tokens (`ui/accounts.rs`,
  below); with `[oidc]`
  (feature `oidc`) a bearer token for `[ui] audience` through the nodes' module
  (`Oidc::admit_console`), whose neutral `grant::Admitted` the session is built from alone
  (`Session::admitted`, as the proposal server's is), a 401 with `WWW-Authenticate: Bearer` (and
  `error="invalid_token"` when a token was sent) or a 403 for a missing scope, both as JSON
  errors. The console's grant providers (`Console::providers`, `grant::Providers::of` the
  bootstrap manifest) are shared with the OIDC module and answer the catalog, the exclusions and
  the token check's `sources` switch. The static files
  and `config` need no token, since the browser needs them to log in. A verified subject in
  `operator_groups`, or with the top-level `operator_claim` equal to JSON boolean true, is an
  operator; string `"true"` is a viewer. With `[ui] operations = true`, `[workspace] source =
  "store"` and a token carrying groups, a non-operator is a team operator (ADR 0015, #324) when
  one of its groups owns an entry of the current workspace version whose identity its grant
  reads: `Console::teams` (`ui/read.rs`) reads the pointer `{<key_prefix>}:workspace` and its
  document on each `session` and write request (the reads need no role, since a team operator
  reads what a viewer reads), through the readers' permits and two-second deadline, and keeps the
  last document by id (a version is immutable); `meada_core::authorize::teams` gives the
  groups, sorted, as `Session::teams`. No pointer, or a pointer or document that cannot be read
  or fails its checks, makes a viewer, so a read failure never grants a write. The global operator
  stays one whatever it owns. `Session::reads` and `whole` let operators see the
  entire fleet regardless of grant. The request's `Session` carries the token's grant, and
  `Session::reads` is the one check every view of a repository goes through, as #290 bounds a
  node's answers: a repository outside the grant is absent, state that belongs to no repository
  is shown whole, a `*` grant reads everything; the read routes (#304) and the playground
  (#305) bound theirs through it. `/api/v1/` is the JSON API (`config`, `session`, `manifest`,
  and with `[oidc]` `admission` and `POST admission/check`; with `[accounts]` `admission` too
  (#558, `Console::accounts_admission`: `accounts`, `{mode (accounts or break_glass),
  public_key (Keys::fingerprint)}`, added to the identity provider's document under break glass,
  and without `[oidc]` the whole document with `groups`, `[accounts.groups]` each group's
  identities the session reads, `*` kept (`grant::bounded`, shared with the identity provider's
  view), and the GitHub grant provider's `github`); errors
  `{"error": {"code", "message"}}`, an unknown path a 404 in that shape, as the admission routes
  are without either section); `config` adds `sign_in` (`accounts` with `[accounts]`, `oidc`
  with `[oidc]`, and
  then `break_glass: true` under break glass) and, with `[oidc]`, the issuer, `client_id`, the
  scopes and the audience
  the browser's authorization code flow with PKCE needs; `session` is the subject and role (`viewer`, `team_operator`, `operator`) and, with `[oidc]`, `groups`
  (sorted), and with a local account's session token `account: true`, `grant` (`"*"` or sorted
  identities, the union of the grant providers', which every
  bounded view, the team operator role and its teams, and `authorize_entries` and
  `compose_entries` read through `Session::reads` and `core_grant`) and, for a team operator,
  `teams`; with `[oidc.github]` (#332) `login` when the token has a valid one, `github_host` and
  `github_grant` (the cache's state, `Oidc::github`, whatever the token's login);
  `manifest` answers the file as loaded at start (a TOML table as JSON: secrets' variable and
  file names only), the module table with what this build has, and the errors of the file as it
  now stands on disk, read again per request, for a session that does not read everything
  without the `[[repository]]` entries, nested peers or group identities (`[oidc.groups]` and
  `[accounts.groups]`, `GROUP_TABLES`) outside its grant
  (group wildcards remain visible), and with only the file and line of an error whose text
  could name one, including an identity known only through groups; `admission/check` reads `{"token": ...}` (`CHECK_MAX`) and answers each check's outcome and the identities, of the
  file's entries or the checked current workspace version's entries for `source = "store"`,
  that both the token's grant and the session's admit, and with `[oidc.github]` `sources`
  (`{"claims": "*" | [...], "github": [...]}`, each side of those identities, sorted; `null` when
  a check fails); the token lives
  only for that request, and a body that does not parse is a fixed 400, never serde's text.
  `ui/write.rs` owns four POST routes, all disabled unless `operations` is true and the
  admitted operator has a nonempty subject. Host, Origin, admission and role are checked before
  reading any body, carrying that same session into routing. A team operator passes the role
  check for `workspace/versions` and `workspace/apply` only; `pointer` and `fence/recover` give
  it the 403 `Msg::UiGlobalOperatorRequired`, before the body is read, and a viewer keeps S2's
  `Msg::UiOperatorRequired`. `pointer` takes `{action,
  generation}` (`set`, `retain`, `drop`) and calls `snapshot::operation::pointer_set`,
  `pointer_retain` or `pointer_drop`; `fence/recover` takes `{at_least?}` (u64 or checked decimal
  string) and calls `fence_recover`; `workspace/versions` takes `{entries, message}` and calls
  `versions::save`; `workspace/apply` takes `{version}` and calls `versions::apply`. These
  owner functions keep the CLI checks, refresh lock and holder (operation plus escaped subject),
  with the admitted subject as author. Success is 200 `{result: "<CLI line>"}`; operational
  refusals are 409 with the CLI message; entry validation is 422 with CLI line/key diagnostics;
  malformed JSON, extra keys or malformed ids/fences are safe fixed 400s; caps are 413s. A team
  operator's save runs `versions::save` as `Role::TeamOperator` (`versions::Acting`, with the
  session's login and `[oidc.github] host` under the GitHub grant provider): after validation
  (422), under the refresh lock and against the version then current, its role recomputed, the
  submission composed over the current entries its grant reads (the attribution step,
  `compose_entries`), `authorize_entries` over that change, every addition of an identity the
  whole current version holds outside the grant refused `outside_grant` whatever else it failed
  (`versions::collisions`, #333: the one refusal the editor cannot predict, which tells the
  subject the identity exists, named as it submitted it), and `compose_entries` over the whole
  current version makes the stored one (author its subject). The global operator's text is
  stored as written, `added_by` included. Its apply is `versions::apply`'s authorized path.
  A refused change is a 403 `{"error": {"code": "entries_refused", "message":
  <Msg::WorkspaceRefused>, "refused": [{"identity", "change", "reason"}], "hidden": <n>}}` with
  nothing written and the lock released: a save names every refused entry (it refuses only what
  the subject submitted or reads), an apply names those whose identity the grant reads and counts
  the rest in `hidden`. A role lost at admission, or under the lock, gets the viewer's refusal: the same 403 `forbidden` text, but under the lock the stderr line keeps
  the teams admission found (`as [...]`), which tells the two apart (a test knob,
  `MEADA_PAUSE_BEFORE_LOCK`, debug builds only, holds a save or apply before the lock).
  Every accepted or refused write logs one stderr line `ui: <subject> <action> <arguments>:
  <outcome>` (a write refused at admission because the directory cannot be read logs only the
  directory's `ui: directory: unreadable (<reason>)` line, #451), with controls escaped throughout; a team operator's is `ui: <subject> as <owners>
  <action> <arguments>: <outcome>`, `<owners>` a JSON array: the owner groups of the entries an
  accepted change touched (`meada_core::workspace::touched_owners`, sorted, without repeats, also the browser's `touchedOwners`: an entry counts when its JSON differs by identity, so a reorder alone touches none), the subject's
  teams for a refusal, whose outcome is the message with the named identities and the hidden
  count. Pre-authentication refusals use absent subject `-` and safe
  fixed arguments, never body or token text. Save validation keeps its full CLI diagnostics in
  the 422 response but logs only the fixed outcome `workspace entries failed validation`, so
  invalid identities cannot copy editor contents into stderr. No route releases locks or
  triggers refresh.
  The audit (#328, ADR 0016, `audit.rs`): with `[postgres]` every write route and every CLI twin
  of one leaves a record in `<schema>.audit`: `id` (an identity column), `time` (Postgres's
  `now()`, answered RFC 3339, UTC, microseconds), `via` (`console` or `cli`), `subject`, `login`
  (the session's GitHub login under `[oidc.github]`, #332, else null; a stored one that fails the
  login pattern leaves the row out), `role` (`viewer`, `operator`, `team_operator`; the
  CLI acts as the operator), `owners` (a team operator's, as its stderr line names them, else
  null), `action` (`audit::ACTIONS`: `workspace.save`, `workspace.import`, `workspace.apply`,
  `pointer.set`, `pointer.retain`, `pointer.drop`, `fence.recover`, and `pointer` for a pointer
  request refused before its body; the named workspaces' `workspaces.*`; `account.login` and
  `account.password`, #554, `account.reset_admin`, #555, and account administration's
  `account.create`, `account.role`, `account.suspend`, `account.reactivate`,
  `account.reset_password` and the route-level `account`, #557, `account.groups`, #558,
  `account.token`, #560, and `account.github` and `account.remove_github`, #561, below),
  `arguments`
  (`{"version", "message"}` for a save or an import,
  the id of the entries as submitted; `{"version"}` for an apply; `{"generation"}` for the
  pointer actions; `{"at_least"}` for a recovery with a floor, its decimal digits; `{}` for a
  refusal before the body), `outcome` (`pending`, `accepted`, `refused`, `failed`) and `message`
  (the new state, or the refusal's or failure's message as the stderr line has it). The order is
  fail closed: a write that passed its role and its body's checks (400, 413 and 422 are never
  recorded, nor the `Host`, `Origin` and admission refusals) inserts its record as `pending`
  (`Audit::pending`, `Console::pending` in `ui/write.rs`) before it touches Valkey or the store;
  an insert that fails answers 503 `audit_unavailable` (`Msg::AuditUnavailable`), its stderr
  line carrying that message after one `ui: postgres: unreachable (<reason>)` line, and writes
  nothing; on the CLI (`audit::cli`, which `main.rs` wraps around each twin) it exits 1 with
  `Msg::AuditCli` naming Postgres and the reason. The helpers that print a write's stderr line
  then finish its record (`Audit::finish`) with that line's outcome text and owners: `accepted`,
  `refused` for a held or lost lock, an operation's own refusal (`snapshot::generations::OpError
  ::Refused`: a generation not retained, the current one dropped, the kept set full, a fence it
  may not lower or raise, a version not stored, `ApplyError::Conflict`, `SaveError::Refused`),
  `entries_refused` or a role lost under the lock, and `failed` for anything else (Valkey, the
  store, the manifest). A failed update leaves the record `pending`. A role refusal in
  `Console::checked` is inserted directly as `refused` with the route's action (`write::recorded`)
  and `{}`, before any body is read; a refusal that finds Postgres unreachable is answered as
  before and leaves only its stderr line. Each call opens its own connection through
  `postgres::connected`, so a schema version this binary does not know counts as unreachable;
  `pool_size` is still unread. The stderr lines are unchanged, with and without `[postgres]`.
  `GET audit?before=<id>&limit=<n>` (`Console::audit_page`, only with `[postgres]`, when `config`
  adds `"audit": true`; a 404 without) is the global operator's alone (anyone else gets the 403 `Msg::UiOperatorRequired`):
  records with an id below `before`, newest first, at most `limit` (1 to `AUDIT_LIMIT_MAX`, 100; `AUDIT_LIMIT`, 50, by default), as
  `{"records": [...], "next": <id or null>}`, `next` the `before` of the following page; any other
  query, a repeated key or a value out of range is a 400 (`Msg::UiAuditQuery`), Postgres
  unreachable a 503 `audit_unavailable`. Every row is hostile: one whose columns fail their
  checks (`audit::record`) is left out with one stderr line naming the table
  (`Msg::AuditBadRecord`), never a value. Records are never deleted by Meada.
  Local accounts at a glance (ADR 0024; the entries below carry the detail). Routes: `POST login`
  and `POST account/password` (#554, #555; no token, then a session token), `POST account/tokens`
  (#560), `POST account/github` and `POST account/github/remove` (#561), `GET accounts`, `POST
  accounts` and `POST accounts/<username>` (#557, #558), and `GET admission` (#558, which stays under break glass and shows
  its mode). Under break glass (#563) every route but `login`, `account/password` and `admission`
  is absent: administration, the token route and the GitHub routes. Seams: `accounts.rs` (keys, tokens, the
  policy, one `Keys::verify` for every surface), `accounts/store.rs` (the one table, the only SQL),
  `ui/accounts.rs` and `ui/accounts/` (the console's routes), `mcp.rs`'s gate and the proposal
  server's `admit` (agent tokens, public half only), `Providers::local` (the grant from a token's
  groups and login), `ui::reset_admin` (the CLI twin), and `Audit` (every route writes before it
  answers).
  Local accounts (#554, ADR 0024, `ui/accounts.rs`, feature `accounts`): with `[accounts]`,
  `Console::start` runs `accounts::Local::start` once every other check passed: the key pair
  (`Config::keys`), `accounts::dummy`, then `store::bootstrap`, whose initial password is
  `accounts::generated` with a change required, or the deployer's (`Config::admin_password`), which must
  pass the policy (else the start is refused naming `accounts.admin_password_env` or `_file`)
  and then needs no change; a generated one comes back from `bootstrap` once the row is
  committed and `Local::announce` prints it on one stderr line
  (`accounts: default account admin created; first password: `), right after the listening line
  or, when the console stops before it, as `Local` drops; Postgres unreachable there refuses the start
  (`Msg::AccountsBootstrap`, at `postgres`). The proposal server's `Console` has no `Local`.
  Admission (`Console::admit_local`, before the OIDC branch): the static files, `config` and
  `login` need no token; every other route a bearer `Keys::verify` passes for `[ui] public_url`
  and `CONSOLE`, else a 401 `unauthorized` with `WWW-Authenticate: Bearer` (`error=
  "invalid_token"` when a token was sent) before routing; a token with `change_required` opens
  `account/password` alone, every other route a 403 `password_change_required`. The session is
  the token's: operator for the role `operator`, its groups sorted, the grant
  `Providers::local` of its groups and its verified login (#561; the login, `github_host` and
  `github_grant` in `session` as an identity provider's session has them), or `*` when no table
  bounds it (`Providers::bounds`), a team operator's teams as with `[oidc]` (`Console::teams_on`),
  `account: true` (`Session::local`, built from a `grant::Admitted`). A write keeps every check it
  has, but with `[accounts]` admission runs before `operations` (`Console::checked`), so a write
  without a token is a 401
  and one with a token awaiting its change a 403 `password_change_required` whatever
  `operations` says; with `[oidc]` the order is unchanged. `POST login` (`LOGIN_MAX`, a fixed
  400 `Msg::AccountsLoginBody` for a body that is not `{username, password}`, 413 past the cap):
  `Console::credentials` reads the account (`store::attempt`; a username failing `username_ok` is
  not looked up; Postgres unreachable is a 503 `audit_unavailable`), takes a hash permit of
  `HASHES_MAX` (`try_acquire`, else 503 `busy`, nothing hashed), then makes exactly one hash:
  the account's, under the row's lock of `store::attempt`, which counts a wrong one toward the
  lockout (#555), or `dummy` for an unknown username, a bad row, a locked and a suspended
  account, after the row is released, so a locked refusal costs about what a wrong password
  does; a refusal is the one 401 `invalid_credentials` (`Msg::AccountsInvalid`). The record
  (`Audit::record`, the final outcome inserted at once) is written before the answer:
  `account.login`, `subject` `local:<username>` and the account's role, or no subject and the
  role `none` for an unknown username, `arguments` `{"account"}` (absent for an unknown
  username) and for a refusal `"reason"` (`unknown`, `password`, `suspended`, `locked`,
  `locked_now`); an insert that fails is a 503 `audit_unavailable` and no token. A success is
  `{token, expires_at (RFC 3339), change_required}`, and then `Console::rehash` redoes a
  `weaker` stored hash under a further permit (none left: the next sign-in does it) through
  `store::rehash`. `POST account/password`
  (`PASSWORD_BODY_MAX`, `{current, new}`, `Msg::AccountsPasswordBody`): the policy on `new`, a
  422 `password_policy` with its `rule` (not recorded, as every body check), then
  `Console::credentials` on the session's own account and `current` (a wrong one counts toward
  the same lockout, a locked account is refused unchecked); a refusal is the 401
  `invalid_credentials`, recorded `refused` with its reason; `new` equal to `current`, once
  `current` is known right, is the 422 of the rule `current_password`; then a second permit for
  the new hash, `account.password` inserted `pending` before `store::set_password`, finished
  `accepted`, and a fresh token without the mark. Tokens issued before stay valid until they
  expire. Each sign-in and change
  writes one line `ui: <subject or -> <action> <username or ->: accepted` or `refused (<reason>)`;
  no line, record or error holds a password, a hash or a token.
  `POST account/tokens` (#560, `Console::agent_token`, `TOKENS_BODY_MAX`, a fixed 400
  `Msg::AccountsTokenBody`, absent under break glass): `{surface, hours}`, `surface` `nodes`
  (or `proposals` when the bootstrap has `[proposals]`, #562, audience `accounts::PROPOSALS`; else a
  422 `invalid_surface`), `hours` 1 to `accounts::AGENT_HOURS_MAX` (168,
  `AGENT_HOURS`, 8, when absent; else a 422 `invalid_hours`); the account's row read
  (`store::standing`): suspended, unknown or hostile is the 403 `forbidden`
  (`Msg::AccountsTokenRefused`), recorded `refused`; active, `Keys::agent` with the row's role
  and groups, recorded `account.token` (`account`, `surface`, `hours`, `token_id`, never the
  token, `Audit::record` before the answer, a failed insert the 503 `audit_unavailable`), then
  `{token, expires_at, surface}`. The console admits no agent token (its audience is not
  `CONSOLE`), and a change-required token never reaches the route (admission's 403).
  The verified login (#561, `ui/accounts/github.rs`, with `[accounts.github]` alone and never
  under break glass; without the table both routes are the 404): `POST account/github`
  (`GITHUB_BODY_MAX`, `{token}`, a fixed 400 `Msg::AccountsGithubBody`, a 422 `invalid_token`
  for a token that is not visible ASCII, neither recorded) reads the account's row first
  (`store::standing`: suspended, unknown or hostile is the 403 `forbidden`,
  `Msg::AccountsGithubForbidden`, recorded), then `Verifier::login` calls `GET <api>/user` once
  through `modules::agent` (`GITHUB_TIMEOUT`, at most `GITHUB_ANSWER_MAX` bytes), the pasted
  token as its bearer and nowhere else (scrubbed from ureq's error); a 4xx but a 429 is the 422
  `github_refused`, a `login` `grant::login_ok` refuses once lowercased the 422 `invalid_login`,
  anything else (unreachable, a 429, a 5xx, no `{login}`, past the cap) the 503
  `github_unavailable` with one `Msg::AccountsGithubUnreachable` line naming the URL and the
  reason, never the body; each recorded `account.github` with its `reason`. A login is recorded
  `pending` (`{"account", "login"}`) before `store::change` (`Change::Github`, the column's
  unique violation `Changed::LoginTaken`, the 409 `login_taken`; a row suspended since it was
  read `Changed::Suspended`, the 403 `forbidden`); a success answers `{login, token, expires_at,
  change_required}`, a fresh session token signed from the row the change returned, so a
  suspension or a role change made while GitHub answered is never signed over.
  `POST account/github/remove` does the same with `Change::RemoveGithub`, recorded
  `account.remove_github`, and answers a token without a login. A token issued before either
  keeps its login until it expires; a console write reads the row's login (`Console::standing`).
  Break glass (#563, `[accounts] break_glass = true` beside `[oidc]`): `Local::break_glass`.
  `Console::admit` asks `Console::admit_break_glass` first: the static files, `config` and
  `login` need no token; a bearer whose `iss` is `[ui] public_url` (`accounts::issued_by`,
  before any key is tried) is checked by `Keys::verify` alone and must be `local:admin`'s,
  else the 401 of local accounts, so a colleague's token from before the switch admits
  nothing; the change-required mark opens `account/password` alone; the session is the global
  operator's (`Session::local`, no group, `*`), with no directory lookup. Any other request,
  a bearer of another issuer included, goes to the OIDC branch unchanged, so the identity
  provider's JWKS never sees a Meada token nor the signing key an identity provider's. With
  break glass `operations` is checked before admission, as with `[oidc]`. `POST login` with
  any username but `admin` is `Console::not_break_glass`: the dummy hashed under a permit, the
  table never read, the one 401, recorded with no subject, role `none` and the reason
  `not_break_glass` (`Refusal::NotBreakGlass`). The tokens it signs, and the password change's,
  carry the role `operator` and no group (`Local::signed_as`). `login` and `account/password`
  are the only routes of local accounts, the password change only for a local session (an
  identity provider's gets the 404); nodes (`mcp`) and the proposal server, which has no
  `Local`, never read `[accounts]` and admit only the identity provider.
  `meada ui reset-admin --workspace <bootstrap manifest>` (#555, `ui::reset_admin`, features
  `ui` and `accounts`; `UiCommand::ResetAdmin`, an optional subcommand of `meada ui` whose
  `--listen` and `--workspace` it negates) is the one CLI twin of account administration:
  `Manifest::read` (no `[accounts]` is `Msg::AccountsResetNoSection`), the password from
  `Config::admin_password` through the policy (a refusal names the key and exits 1, nothing
  recorded) or `accounts::generated`, its hash, then `audit::cli` (`account.reset_admin`,
  `{"account": "admin"}`, `via` `cli`, the subject `modules::cli_author`, role `operator`):
  the record inserted `pending` before `store::reset_admin`, a record that cannot be inserted
  exiting 1 with `Msg::AuditCli` and nothing written. Its one stdout line is the generated
  password; stderr names what it did (`Msg::AccountsResetDone`) and, for the setting's value,
  only its key. It never reads the signing keys.
  Every console write of a local account acts as its row stands (#557, `Console::standing`, in
  `Console::checked` after `operations` and before any role check): `store::standing` reads the
  role, groups, verified login and suspension of the session's username; an active account's
  session is rebuilt
  from them (`Console::local_session`, as `admit_local` builds one from a token; under break
  glass an active row keeps the global operator's session `admit_break_glass` made), a suspended,
  unknown or hostile one (one `Msg::AccountsBadRow` line) writes as a viewer with no group and so
  gets its write's role refusal; Postgres unreachable is a 503 `unavailable`. Reads, `login` and
  `account/password` (which reads the row itself) keep the token's session, so a suspended or
  demoted account's reads last until its token expires. Account administration
  (`ui/accounts/administration.rs`, #557), the global operator's whatever `operations` says,
  and absent under break glass (`Console::has_administration` false, so each route is the 404 of
  an unknown one, #563): `Console::administration_refused` refuses anyone but a local account of the role `operator`
  with a 403 `Msg::AccountsOperatorRequired` before the body is read (`Console::administer`
  checks it again), a write's refusal printed and recorded `refused` as `account.create`, or
  `account` for a change (its `{"account"}` when the route names a valid username).
  `GET accounts` lists `store::list` (`{"accounts": [{username, role, state (active, suspended,
  locked), locked_until, change_required, password_changed, created, created_by, groups,
  github_login}]}`,
  never a hash; a row failing its checks left out with one line). `POST accounts`
  (`ADMIN_BODY_MAX`, `{username, role, groups, password, change_required}`, `groups` optional, a
  fixed 400 `Msg::AccountsCreateBody` for any other shape, a 422 `invalid_username` for a
  username failing `username_ok`, a 422 `invalid_group` (`Msg::AccountsGroupInvalid`,
  `checked_groups`, #558) for a group that is no owner name, which also sorts them without
  repeats, the policy's 422 on the password against that username) hashes under a permit of
  `HASHES_MAX`, records `account.create` `pending` (`{"account", "role", "groups",
  "change_required"}`, the groups a JSON array in a string, `change_required` as `"true"` or
  `"false"`, since every argument is a string), then `store::create` (a taken username a 409
  `username_taken`, the record
  `refused`; `created_by` the session's subject). `POST accounts/<username>` (a username failing
  `username_ok` a 404 before the body is read; `{"action": "role", "role"}`, `{"action":
  "groups", "groups"}` (#558, replacing the list, checked as a creation's), `suspend`,
  `reactivate`, `{"action": "reset_password", "password", "change_required"}` or `{"action":
  "remove_github"}` (#561, `account.remove_github`, the verified login cleared), each refused a
  fixed 400 `Msg::AccountsChangeBody` with any other field, so no body deletes or renames) records
  `account.<action>` `pending` (`{"account"}`, with `"role"`, `"groups"` or
  `"change_required"`) before
  `store::change`; an unknown or hostile account is a 404 `not_found`, the last active operator's
  suspension or demotion a 409 `last_operator`, each finishing the record `refused` with the line
  `refused (<code>)`; a success
  answers `{"account": ...}` as listed. A record that cannot be inserted is a 503
  `audit_unavailable` and nothing changes. No method but `GET` and `POST` and no other route
  reaches the table: a `DELETE` or a rename's path is the 404 of an unknown route.
  Operational reads live in `ui/read.rs`: `GET fleet` reads the checked pointer/retained/kept
  state (`generations::inspect`), live heartbeats (`heartbeat::read`), holder and remaining
  lock TTL, and fence counter from Valkey; workspace ids come from generation documents and
  last publish from the current document's modification time in the store. Malformed
  heartbeats are shown with fixed reasons, never used for a key or diagnostic. The two stuck
  cases name `meada snapshot fence recover`; the fleet maximum includes the current workspace
  pointer's fence, and a stuck node also names deleting its own
  `<local_dir>/current`. `GET store` lists every generation document name and fully reads
  current/retained documents only; `store/generations/<id>` reads one. Both use the pull's
  checked document reader, with validated ids, bundle keys, identities, hashes and blocks.
  `GET workspace`, `workspace/versions` and `workspace/versions/<id>` read the optional
  Valkey pointer and checked versions from the store (`versions::stored`, `documents`, the
  listing shared with the CLI), newest first, malformed documents listed as such. Repository
  entries and nested peers are bounded by `Session::reads`; malformed reasons cannot name
  hidden repositories. `GET repositories` lists commits per live served generation, schema
  versions from the console's own held indexes, unknown elsewhere; without snapshot it
  compares read-only index freshness and optional indexed commit with mirror `HEAD`.
  `index_workspace` atomically records an optional `index.commit.json` beside each index
  after a clean indexing pass; its commit is accepted only while its fingerprint of the
  index's existing metadata and file/document/manifest hashes matches. Older indexes,
  interrupted writes and stale sidecars give an unknown commit; the SQLite schema is unchanged.
  `index::indexed_commit` reads and checks it in every build: a server-mode export names it as
  its `commit` (#512).
  `GET credentials` uses `github_app::inspect` to read installations, token expiry and
  throttle expiry times from Valkey only, filtering owners by the granted workspace identities;
  expiry is extracted beside the token in Valkey, so no token or private key is read by the
  console. All operational service reads share a two-second request deadline and report
  the service that failed; permit waits share that deadline. At most `READERS_MAX` operational
  readers run at once, and a worker retains its permit until blocking I/O actually finishes,
  even after its request timed out. The managed-workspace identities of `admission/check` use
  this same worker and deadline boundary, including the store connection and runtime teardown.
  Config/session/manifest use no such permit. Missing module sections are explicit `requires`
  responses; without `[generations]` there is no per-node fleet view.
  The playground and the exports (#305) answer in process from what the console holds
  (`Holds`), never from a node: with `[snapshot]`, the generations its own `meada snapshot pull`
  (an init pull and a `pull --every` companion on a `local_dir` of its own) keeps, read per
  request as `--generations` serving reads them (`serve::Current`, `Engine::open_held`), the one
  `<local_dir>/current` names by default; without it, the bootstrap manifest's `[[repository]]`
  entries as read at start, each index under `index_dir`, as `meada mcp --listen --workspace`
  opens them (`Engine::open_listed`). `POST query` reads `{"repository", "generation", "tool",
  "arguments"}` (`QUERY_MAX`), the arguments as the MCP tool takes them and checked by the
  tools' own code (`mcp::served_call`, so `trace` with `changes: uncommitted` is refused with
  the node's text), and answers `{"text", "generation"}` (`generation` null without
  `[snapshot]`); `GET repositories/<identity>/export?generation=<id>` answers the bytes
  `meada export` prints, the identity being everything between `repositories/` and the last
  `/export`. Both open one engine per request whose workspace holds only what `Session::reads`
  admits (peers outside the grant dropped before the answer is built); a repository outside
  the grant is the 404 of one not served, a generation not held a 404 naming the held ones, and
  nothing servable yet (no pull, no index) a 503 naming `meada snapshot pull` or `meada index
  --workspace`, every other view answering, and the next request after the pull or the index
  answering with no restart. A tool error is a 422 with the node's text.
  Every other path is a file of `[ui] dir`: percent-decoded, a 404 for a
  `..`, `.` or empty segment, a NUL or a backslash, a resolved path (symbolic links followed)
  outside the canonical `[ui] dir`, or an extension outside the fixed table (`content_type`); a
  path that names no file, a directory included (none is listed), answers `index.html` unless it
  is under `/api/`. Files under `assets/` are cacheable for a year and immutable, every other one
  `no-cache`. Every answer carries `nosniff`, `Referrer-Policy: no-referrer` and the content
  security policy (`csp`, whose `connect-src` adds the issuer's origin with `[oidc]`); none sets
  a cookie or an `Access-Control-*` header.

## Console frontend (`ui/`)

The static console (ADR 0014) is a separate Vite/React/strict TypeScript build. `npm ci`,
`npm run wasm`, `npm run build`, `npm run check:version` and `npm test` run inside `ui/`, with
Node pinned by `.nvmrc` and wasm-pack by `scripts/wasm.mjs`. Cargo never invokes them. The
WASM bundle builds from `meada-core` into ignored `ui/wasm/`; Vite puts content-hashed scripts,
styles and WASM under `dist/assets/`, with `meada-ui.json` carrying the workspace version.

`Dockerfile` builds the frontend in a pinned build-platform stage. Only its `cloud` target
copies `dist/` to `/usr/share/meada/ui`; the default `runtime` target skips Node entirely.
`scripts/ui-tarball` packages that directory from the cloud image under one versioned root.
The container workflow checks both images, compares the archive's files and bytes with the
image, and attaches it to the published release. cargo-dist's default binary artifacts stay
in their separate release workflow.

`src/api.ts` owns the same-origin `/api/v1/` client; `auth.ts` owns OIDC discovery, PKCE and the
memory-only bearer. Authorization redirects retain the issuer endpoint's query parameters and
replace generated OAuth keys through `URL.searchParams.set`. Only the pending redirect's verifier and state enter sessionStorage, and
both are removed on callback. `main.tsx` handles `/auth/callback` before rendering the shell;
`App.tsx` keeps page navigation in process so changing pages does not discard the bearer.
`pages.tsx` provides semantic navigation and the configuration, session and bootstrap manifest
views, with view models in `models.ts` and navigation copy in `i18n.ts`. `PlaygroundPage.tsx`
sends the MCP-shaped query that `playground.ts` builds, displaying the answer unchanged.
`GraphPage.tsx` fetches an export through `Api.export`, and `graph.ts` checks it with WASM
`parseExport` before deriving communities, contracts, cross-repository links, drift and
applied/stale decisions. View changes are local; explicit generation exports are cached in the
page and a current-generation load reads again. `graph-i18n.ts` holds this area's pt-BR/en copy.
Provisioner sides (#656) join their contracts with the IaC address and root label; provisioning
links have a separate view with every side, root, level and reason. The export parser in
meada-core validates these optional arrays before the graph reads them.
`OperationalPage.tsx` reads the fleet, store, repositories, credentials and admission routes;
`operations.ts` projects their checked DTOs, with bilingual copy in `operations-i18n.ts`.
Missing modules name their sections, errors remain local to the page, and the pasted token
exists only in the uncontrolled field and its request body, cleared on completion.
`AuditPage.tsx` (#328) reads `GET audit` a page at a time, newest first, following `next` (older
records) and back to the newest; `audit.ts` projects each record's time, via, subject, login,
role, owners, action, arguments, outcome and message into text, with copy in `audit-i18n.ts`.
`pages.tsx`'s `offered` lists the page for the global operator alone when `config` says
`"audit": true` (the console has `[postgres]`), and `App.tsx` answers its path with the
not-found page otherwise.
Local accounts (#556, ADR 0024): when `config` says `"sign_in": "accounts"`, `main.tsx` builds a
`LocalAuth` (`local.ts`) instead of `Auth` and `App.tsx` follows its `stage` (`out`, `forced`,
`in`) through `useSyncExternalStore`. `LocalAuth` posts `login` and `account/password` itself
and keeps the session token and its marks (`{token, expires_at, change_required}`) under the one
`sessionStorage` key `meada.session`, never `localStorage`, a URL or a log; a record past its
`expires_at` or unreadable is dropped at load. `Api` (`isLocal`) clears the token on any 401
(`LocalAuth.expire`, so the sign-in page returns, saying the session ended) and binds the tab to
the forced change on a 403 `password_change_required` (`LocalAuth.requireChange`); the one 401
that keeps the session is the `invalid_credentials` of a refused current password, which only
`LocalAuth.changePassword` sees; sign-out is `LocalAuth.clear`. `AccountPages.tsx` holds
`SignInPage` (labelled username and password, one generic refusal, `busy` and
`audit_unavailable` named), `PasswordForm` (current password, the new one twice; the server's
`rule` code shown as the page's own copy) with `ForcedChangePage` (a `Shell` with `locked`: no
menu, no link, no session read, reached only while the stage is `forced`; the change opens the
workspace) and `ProfilePage` (the session's fields, the voluntary change, sign-out), `account.ts`
their view model and `account-i18n.ts` their pt-BR/en copy. The shell offers `/profile` with
local accounts only. `AgentTokenForm` (#560, #562: the surface, `nodes` or `proposals` when `config` says
`proposals: true`, the validity, the token shown once with a copy control and its expiry, gone when
the page is left) and `GithubLoginForm` (#561: the session's login, a personal access token sent
once and cleared, the removal) sit on the profile page; each success of the second swaps the session
token.
The accounts page (#559, `AccountsPage.tsx`, view model `accounts.ts`, copy `accounts-i18n.ts`) is
the global operator's: `pages.tsx`'s `offered` lists `/accounts` for the role `operator` when
`App.tsx` says the console has account administration (`config.sign_in === "accounts"` with a
`LocalAuth`, so never under break glass), and the path answers the not-found page to any other
role. It reads `GET accounts`; the create form checks the username pattern and the password
length (Unicode scalar values) before `POST accounts`, with `change_required` checked by default;
role, groups, suspend, reactivate and reset password (the same checkbox) each open the shared
`ConfirmDialog`, which asks what the change needs, sends `POST accounts/<username>` on confirm
and shows a refusal inside it, the server's `password_policy` rule as the page's own copy
(`ruleProblem`) and any other refusal (`last_operator`, `username_taken`, `invalid_group`) with
the server's message unchanged. A suspended account is marked in text and a locked one says
until when. The audit page's `audit-i18n.ts` names every `account.*` action (shown with its code)
and the role `none` reads as the page's `none`.
The admission page (#558, `operations.ts`): `Admission` is the identity provider's document
(`OidcAdmission`, with `accounts` under break glass) or local accounts' alone (`AccountsAdmission`,
told apart by `isOidcAdmission`); `admissionModel` adds the sign-in mode and the public key's
fingerprint, and the issuer's metadata and the token check show only for the identity provider.
Break glass (#563): when `config` says `"sign_in": "oidc"` with `"break_glass": true`, `main.tsx`
builds a `LocalAuth` instead for a tab on `/break-glass` (`BREAK_GLASS_PATH`) or holding a
session token (`breakGlassTab`); any other tab without a token gets `SignInChoicePage` before the
identity provider's redirect (its button first, a discreet link to `/break-glass`, which stays
when the identity provider cannot be reached), and the start's error page links there too. On
that page `SignInPage` with `breakGlass` names the default account's door and links back to the
identity provider; signed out, the tab returns to `/break-glass`, and signed in from it, to the
workspace.
The API reader preserves large JSON integers as decimal strings instead of rounding fences.
`actions.ts` offers generation operations only to an operator while operations are enabled;
`ActionControls.tsx` displays their CLI commands and the optional fence floor without rounding
u64 values. `ConfirmDialog.tsx` uses a native modal dialog with labelled controls, Escape
cancellation and focus restoration. `OperationalPage.tsx` freezes the confirmed payload,
shows CLI results and refusals unchanged and reloads the fleet after success. A generation
pointer rollback lasts until the next publish, as every set control explains.
`WorkspacePage.tsx` reads workspace versions and their current fence, caches selected details
and compares them with the core's `workspaceDiff`. `workspace.ts` owns the editor projection
and captured save/apply payloads and commands; `workspace-i18n.ts` owns its pt-BR/en copy.
Every edit validates through WASM `validate_import`; JSON entries become TOML through the
same `entries_text` as `workspace show`. Saves require a message, accepted saves read the
immutable first metadata independently of read refresh failures, and older applies are labelled
as rollback for the next refresh. The shared confirmation dialog sends nothing before acceptance.
A team operator (#325) edits the entries its grant reads, as `GET .../versions/<id>` returns
them (`workspace.ts`: `teamEditorModel` composes the text with `composeEntries`, diffs the
composed version and runs `authorizeEntries`, so a refusal is marked with its reason in the diff
and the save disabled with no request; `entryRows` shows each entry's owner and the answer to
changing it alone). Under the GitHub grant provider (#333) `authority` carries the session's
`login` and `github_host` into both functions, so `unattributed` and `added_by_change` are
marked as ADR 0015's reasons are; the entries tables show each entry's `added_by` once one has it, and
the editor and the versions page mark the entries a version's `excluded` names (the global
operator's editor lists them in a labelled status), in text. `plainEntries` projects the flags
a view adds (`may_change`, `refusal`, `owner_missing`, `excluded`) out
before any TOML or id, and a partial view's hash is never a saved id: a team save reads the
server's `result`. Its save and apply confirmations name the teams that authorize them (the
owners of the entries touched) instead of a CLI command, an apply is offered only when
`authorizeEntries` over the entries it reads refuses nothing, `ApiError.body` carries a 403
`entries_refused` (entries and `hidden`) to the dialog, and any other 403 reloads the session
and turns the editor read-only. Every role sees owners in the versions page and its diffs.
The frontend container stage copies the one shared validation fixture because TypeScript also
checks its tests; Rust/default stages gain no frontend input.
Vitest exercises the built WASM and the public client, view-model and DOM
seams, without a browser end-to-end suite.
