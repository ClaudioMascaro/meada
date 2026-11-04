# Workspace editor and shared validation (#310)

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

### Local accounts: sign-in and session tokens (#554)

`src/accounts.rs` holds the unit tests of the section (one key of each pair, names only), the
key pair (a ring-generated Ed25519 pair accepted; a mismatched pair, a public key that is a
private one, a garbage private key, a key not named, an unset variable and an unreadable file
refused naming the key and no value; a key file past `KEY_MAX` and a password file past
`PASSWORD_FILE_MAX` refused naming the file), the session token (issuer, audience, other key,
expired and not yet valid past the leeway, a non-local subject, an unknown role, no `exp`,
HS256, a `crit` header), the password policy table (every rule, characters not bytes) and
argon2id (OWASP parameters in the PHC string, a fresh salt, a non-argon2id string matching
nothing, a hash past one of the parameter ceilings neither a stored hash nor computed), the
common-password list (sorted without repeats, lowercase, every entry within the length rule, its
first and last entries refused ignoring case), `weaker` (less memory, fewer iterations or version
0x10 weaker, the console's own and a costlier one not) and `generated` (24 symbols the policy
takes, never twice the same).
`tests/it/ui/accounts.rs` runs the console as a process. Without a service: the section rules
through `meada status` (`[accounts]` without `[postgres]`, beside `[oidc]`, both keys of a pair,
`operations = true` naming no further problem), the start refused on a mismatched, non-Ed25519,
unreadable or missing key and, with a good pair, on the default account Postgres cannot take,
never a key or the URL in stderr, and a node over the chain fixture whose `[accounts]` names a
private key file that does not exist starting and answering 401 without a token (#560). Against Postgres (`postgres::audited`, a schema
of each test's own behind the relay): the tracer bullet (`config`'s `sign_in`, the 401 before
routing, the `Origin` 403s of `login`, fixed 400s and the 413, the generated first password,
read from its one stderr line (`first_password`, absent from the rest of stderr), giving a token that
opens only the password change (a write without a token a 401, with that token a 403
`password_change_required`, the change's body past its cap a 413), one body for an unknown
username and a wrong password, a
tampered token's `invalid_token`, a token another key signed and an expired one each an
`invalid_token` 401 where the console's key signing the same claims passes, the policy's 422
and the wrong current password's 401, then a
fresh operator token whose `session` is `local:admin`, an old token keeping its mark, a viewer
seeded with the same hash reading, refused the audit page and a write, and refused reusing its
password), the audit page listing every sign-in, refusal and change, a hostile row (an unknown
role) answering as an unknown username with one line naming the table, no cookie or CORS header
and no password, token part, hash, key or URL in any error body, record or stderr line; the
default account created from the deployer's password without a change required, never touched
once the table holds a row, refused at start for a password failing the policy (the key named,
never the value), and created once by two consoles started together, one of them printing the
generated first password on one line, a third start printing none and the password demanding its
change; a burst of sign-ins past
`HASHES_MAX` answered 503 `busy`, no token when the audit table cannot take the record, and every
sign-in and change a 503 `audit_unavailable` with the relay closed. With the fleet's Valkey and
store too, the default account saves a workspace version after its change, audited as
`workspace.save` under `local:admin` and listed with that author. The lockout and recovery (#555): nine
failures on either side of a success never lock; fifteen wrong passwords sent at once are
recorded nine `password`, one `locked_now` and five `locked` (a `busy` one sent again), each with
the wrong password's body; the right password while locked gets that body too, its fastest
answer at least six tenths of the fastest wrong one's; a lock shortened by SQL ends on time
although an attempt came meanwhile (never extended); stderr names `locked_now` and `locked`.
Nine wrong current passwords of a change and a wrong sign-in lock `admin`, then neither route
admits the right password. A viewer seeded with a weak argon2id hash keeps it through a wrong
password and has it redone at the console's parameters by the right one (the one test reading
the hash column, since no route shows a hash). `meada ui reset-admin` after ten bad sign-ins:
the generated password alone on stdout, a fixed stderr line, a sign-in with it that must change
it, then the global operator; the record `account.reset_admin` `via` `cli`; a suspended viewer
`admin` reactivated as an operator from the setting's value (its key named, never the value); a
common password refused exit 1 naming the key; a missing audit table exit 1 with nothing changed;
an empty table getting `admin` created; no generated or set password in any record or stderr.
Without a service: `reset-admin` on a manifest without `[accounts]`, Postgres closed (exit 1, no
password printed, no URL) and a short setting, and `meada ui --help` naming it.
`tests/it/cli.rs` holds that a build with `ui` and without `accounts` has no `reset-admin`.
`tests/it/postgres.rs` holds
migration 9's columns, and the tests that name the schema version moved to 9.

### Local accounts: account administration (#557)

`src/ui/accounts/administration.rs` holds the unit tests of the bodies: each action parses with
its own fields and none other (an unknown action, a deletion or rename among them, an unknown
field beside `suspend`, a role that is neither `operator` nor `viewer`, a missing field are
refused), a creation's fields, and which routes are administration's.
`tests/it/ui/accounts/administration.rs` runs the console against Postgres (skipping without it):
the list's fields and its default account (no hash, no creator); a creation's 400s, 413, 422s
(`invalid_username`, the policy against the new username), a change required by default, a
taken username's 409; an unknown or malformed account's 404, no action, method (`DELETE`,
`PUT`, `PATCH`) or rename route that removes or renames; the role both ways; a lock listed with
its end and cleared by a reset that requires a change by default (and not when told not to);
suspension and reactivation; a viewer's 403s on the list, a creation and a change, the last two
recorded; every record's action, arguments and outcome; no password, token, hash or key in any
answer, record or stderr line. The demo: a suspended viewer's next write is the role refusal
and its sign-in the one 401, recorded `suspended`, its token still reading; a suspended, then
demoted operator's writes are a viewer's refusal at once (recorded as `viewer`), its token still
saying operator, writing again once reactivated. The last active operator: alone beside a hostile
operator row (no hash), its own suspension and demotion are 409 `last_operator`; two operators removing themselves at once, five
rounds, one passes and the other is the 409, leaving one active operator each time. With
`operations = false` the routes answer and the four operations are the 403 naming
`ui.operations`. A change whose record cannot be inserted changes nothing (503
`audit_unavailable`), and with Postgres gone a write and the list are a 503 `unavailable`.

### Local accounts: the console's pages (#556)

`ui/tests/local.test.ts` holds the client: the sign-in posting to `login` and keeping the token in
`sessionStorage` alone (no `localStorage`, cookie or address), the bearer on the next request, a
change-required sign-in bound to the change also after a reload, an expired or malformed stored
session dropped, a refused sign-in keeping nothing, sign-out, the password change's bearer and
fresh token, a refused current password (401 `invalid_credentials`) keeping the session while any
other 401 ends it, any 401 of the API ending it, a 403 `password_change_required` binding the tab to
the forced change (kept across a reload), no `console` call carrying the token. `ui/tests/account.test.ts` is the view
model (the one generic refusal, `busy` and `audit_unavailable` distinct, each password rule in
both languages, the form's local checks) and the copy's parity. `ui/tests/AccountPages.test.tsx`
is the DOM of each page in both languages (labels, input types and autocomplete, keyboard
reachability, the locked shell with no link, the refusals, the unbinding change, the profile's
voluntary change and sign-out); `ui/tests/LocalAccounts.test.tsx` runs the app over the built
WebAssembly: the default account's first sign-in through the forced change to the workspace
with no other request before the change, a 401 returning to the sign-in page with the token
gone, the profile and sign-out in Portuguese, and the identity provider's flow unchanged.
`ui/tests/AccountsPage.test.tsx` (#559) is the accounts page: the view model (suspended marked,
a lock's end), the create form's local checks and its box checked by default, the server's rule
and message shown on a refused creation, each change through the dialog (nothing sent before
confirming, the payload, `last_operator` unchanged inside the dialog, a short password never
sent), `offered` for the global operator alone, labelled and keyboard reachable controls in both
languages, and the audit page's copy for every `account.*` action and the role `none`.

### Local accounts: agent tokens for the nodes (#560)

`src/accounts.rs` holds the unit test of `Keys::agent` and `Agents::admit`: the claims (the nodes'
audience, the subject, role, groups, a 32-hex random `jti`, no mark, the hours asked), a node
admitting it with the public half alone (the private file removed), the console's audience
refusing it, a node refusing a session token, an empty bearer and another scheme; the proposal
server's surface (#562) admitting its own token (role and login kept) and refusing the nodes'
and a session token, a node refusing the proposal server's.
`tests/it/oidc/accounts.rs` runs a node over the chain fixture with `[accounts]`, `[ui]` and
tokens the test signs, with no service: no token a 401 `Bearer`; another key, a session token,
the proposal server's audience, expired, `nbf` ahead, another issuer, no `exp`, a non-local
subject, a change-required mark, `crit`, ES256, HS256 and `alg: none` each a 401
`invalid_token` before routing (two `Authorization` headers too); the group `web` reading `WEB`
alone through `trace`, `search` and `context`, a 403 `insufficient_scope` on the catalog, the
group `*` reading as the golden has it, the 404s listing what the token reads; the `Origin` 403
first; no token part on stderr. Without `[accounts.groups]` a token reads everything. The start
refused, nothing listening, on a public key missing, unreadable or no Ed25519 key and on a
manifest with no `[ui]`. With `generation-pointer`, on a hand-made generation with pins: a pin
and a named workspace's route admitted as their identity is, a pin not held only to a token
reading it, and the 503 of no servable generation to an admitted token, the 401 without one.
`tests/it/ui/accounts/tokens.rs` (Postgres, skipping without it) is the demo: a viewer issuing a
24-hour token answered `{token, expires_at, surface}`, its claims, the 8-hour default and a new
id each time, the console's 401 to it, a node holding the console's public key reading `WEB`
with it and answering 403 on the catalog and 401 to the session token; the body refusals (0 and
169 hours, another surface, an unknown field, no surface, no JSON, the 413); a change-required
account's 403 `password_change_required`; a suspended account's role refusal with its live session
token; the operator's token carrying its role; every `account.token` record with the surface,
the hours and the token id; no token, password or key in the audit page or either stderr.
`ui/tests/LocalAccounts.test.tsx` runs the profile's section over the built WebAssembly: the
surface and the 8-hour default, the request's body and bearer, the token shown once with its
expiry and copied, in no storage, gone after navigating away and back; the refusals (hours, the
role refusal, any other) in Portuguese, the session kept.

### The verified login of `[accounts.github]` (#561)

`src/accounts.rs` holds the unit test of `[accounts.github]` (its defaults, `api` GitHub's on
`github.com` and required elsewhere, `https` or loopback `http`, each bad key named with the
table's prefix, `login_claim` refused) and the `login` claim (carried by an agent token, a token
whose login is no lowercase login refused). `src/ui/accounts/github.rs` holds the unit tests of
the pasted token's shape and of `GET /user` against a one-shot loopback server: the login
lowercased, a 401 or 403 refused, a 429, a 502, an answer with no `login` and one past
`GITHUB_ANSWER_MAX` unavailable, a `login` that is no login invalid, nothing listening
unavailable, the token sent as the bearer and in no reason. `tests/it/github_app.rs`'s fake
answers `GET /user` for the tokens a test registers (`users`, `user_status`) and logs every
bearer it saw (`bearers`). `tests/it/ui/accounts/github.rs` (Postgres, Valkey and the fake,
skipping without them) is the demo: the role check reading `[accounts.github]`; ana's session
before and after the verification (`login`, `github_host`, `github_grant`), her grant bounding
the manifest view and the repositories, the admission view's `github`; a fresh agent token
carrying the login answered by a node on the web and refused on the catalog; GitHub's refusal,
GitHub failing, another account's login, the body checks; ana's own removal and the global
operator's `remove_github`; the `account.github` and `account.remove_github` records with the
actor's `login` column; the personal access tokens in `GET /user` alone and in no answer,
record or stderr line. `tests/it/ui/oidc/additions.rs`'s
`a_local_team_operator_s_addition_is_attributed_from_its_verified_login` (Valkey, the store,
Postgres and the fake) runs a local team operator's additions on the team fixture: `unattributed`
before a verified login, `added_by` from it after (the audit's `login` column too), and the
exclusions mark the role check over `[accounts.github]` leaves, for the global operator and a
viewer whose verified login reads the entry. `tests/it/ui/accounts.rs` names the two
`[accounts.github]` manifest errors. `ui/tests/AccountPages.test.tsx` drives the profile's
section (the token sent once in the body, the fresh session token kept, the session read again,
the token in no storage, each refusal as the page's own copy, a 401 ending the session, the
removal) and `ui/tests/AccountsPage.test.tsx` the accounts page's login column and its
`remove_github` behind the dialog.

### Named workspaces and the proposal server with local accounts (#562)

`tests/it/ui/oidc/workspaces.rs`'s `Case::local` runs the named-workspace suites with local
accounts (Valkey, the store, Postgres and the fake GitHub, skipping without them): no `[oidc]`,
`[accounts]` with `[accounts.groups]` (`claims` mapped to the claims caller's grant) and
`[accounts.github]` over the fake's API, `[ui] operations = true`; every caller of the Y/A/B table
is created by the default account through `POST accounts` (the platform administrator an
operator, the claims caller in the group `claims`), signs in and verifies its login with a
personal access token the fake answers on `GET /user`; its node and proposal server tokens are
agent tokens from `POST account/tokens`, and `Case::subject` is `local:<name>`. Each
`local_accounts_*` test runs the body of its identity provider twin: the creation table with the
catalog and its check (`creation_follows_...`), list, show and the peers' refusals
(`peers_view_...`), the job's membership, the node route and its 404 listings
(`a_created_workspace_becomes_queryable_...`), save, apply, rollback, rename, delete and
operators with `local:` authors, operator lists, history actors and audit subjects
(`versions_are_saved_...`), the effective view and the preview (`an_ignored_or_unsupported_...`)
and the proposal server's table (`proposals_follow_...`: no metadata, a 401 `Bearer` to no token,
a forged one, a session token and a node's agent token, the forged arguments invalid, the stderr
line naming `local:alice`, and a suspended account's token still answering, since nothing but the
token decides). `local_accounts_a_suspended_operator_operates_no_workspace` (`lifecycle.rs`, no
twin) has the default account suspend alice: her unexpired token still shows her workspace, its
rename is the outsider's 404 `not_found` (a viewer with no group sees nothing), and once
reactivated the same token renames it.
`tests/it/manifest.rs` holds `[catalog]` checked against `[accounts.github] host`
and `[workspaces]` accepted over local accounts, and the new needs text;
`tests/it/ui/accounts/tokens.rs` the 422 `invalid_surface` for `proposals` without
`[proposals]`; `ui/tests/LocalAccounts.test.tsx` the profile offering the proposal server's
surface only with `config.proposals`.

### Break glass beside the identity provider (#563)

`src/accounts.rs` holds the unit tests of `break_glass` (off by default) and `under_break_glass`
(the grant tables refused only under `break_glass = true`), and `issued_by` routing a token by
its unchecked issuer. `tests/it/ui/accounts/break_glass.rs` runs the console. Without a service:
the section rules through `meada status` (`break_glass` without `[oidc]`, `[accounts]` beside
`[oidc]` with `break_glass = false`, `[accounts.groups]` and `[accounts.github]` under it, each
one line; the accepted section stopping only at Postgres) and the start refused for an
`[oidc] issuer` equal to `[ui] public_url`, before any key is read. Against the local Postgres
(skipped without it): `config` naming `oidc` with `break_glass`, the identity provider's viewer
and operator tokens admitted as before, `admin` through its generated first password and its forced change to the
global operator's session (`account: true`), a colleague's seeded account and an unknown username
answered with the wrong password's body and recorded `not_break_glass` with no subject, the
routes of account administration, agent tokens and login verification answering 404, a fence
recovery authorized and recorded as `local:admin` (feature `refresh-lock`), and the crossed
bearers refused (an identity provider's signature on the console's issuer, the console's key on
the identity provider's issuer, a colleague's session token, a nodes token, an expired one); no
password, hash, token or key in any answer, record or stderr line. `tests/it/oidc.rs` holds a
node with `[oidc]` and `[accounts]` under break glass naming missing key files: it starts,
answers the identity provider's token as the golden has it and refuses Ed25519 tokens of the
console's issuer. `tests/it/ui/oidc/workspaces/proposals.rs` holds the proposal server's twin
(the local services). `ui/tests/BreakGlass.test.tsx` holds `breakGlassTab`, the choice page (the
identity provider first, the discreet link kept after a failure, both languages), and the app
on `/break-glass`: the default account through its forced change to the workspace and, signed
out, back to the break-glass page, never the local accounts' copy.

### The directory (#334)

`tests/it/ui/oidc/directory.rs` runs a console with `[oidc]` (the `oidc` module's loopback
issuer), `[postgres]` and `[directory]`. Against the local Postgres (a schema of each test's
own through `postgres::audited` and its relay; skipped without `MEADA_TEST_POSTGRES_URL`,
failed with `MEADA_TEST_SERVICES_REQUIRED=1`; the stalled-peer case points `ops/pg-url` at
`postgres::Stalled` first, a Postgres that starts up and answers no statement, and back at the
relay once the 503 has come after 15 s): an Entra ID sequence (the `userName` lookup,
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
ignores case. With the relay closed, reads (`workspace` among them) are admitted, the picker is a
503 `unavailable` taken from the admission, a write is a 503 `audit_unavailable` with no audit
insert, and each request writes exactly one stderr line, the directory's, which never names the
claim (#451). The picker lists
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
stderr line carries a token part, a login or the Postgres URL. `src/grant/permissions.rs`'s unit
tests hold the exact expiry (`max_age_seconds` grants, a millisecond past it does not), an age
that never moves back when Postgres's clock does, the login's shape and the row identity check,
and `Providers::grant` with a GitHub grant provider (the union of the groups side and the login's
rows, `*` from a group, an unmapped group, a missing or malformed login, an expired and an unread
set); `src/grant.rs`'s own tests hold the groups side without one (the union of the groups, `*`,
an unmapped group, no table, a login kept only under the GitHub grant provider) and the
identities a verifier grants by name (`Granted::listing`).

`tests/it/ui/oidc/github.rs` runs the team fixture's console with `[oidc.github]` and
`[postgres]`: the session is `unread` with the login lowercased before any set, then `current`
with the GitHub identity in `grant`, a group owning an entry only GitHub grants making a
`team_operator` with that team, every bounded route (`workspace`, its versions, `repositories`,
`manifest`, `store`, `credentials`) naming no identity outside the union; the token check's
`sources` (`claims` and `github`, sorted, `*` for a machine token) with no request to the
loopback issuer or the fake GitHub; `admission`'s `github` with the set's id, `finished`, age,
`max_age_seconds` and the counts the job wrote; an audited write's record carrying the login;
then `expired`, a viewer with an empty grant. A second case (#316) commits a set through a
`snapshot lock` refresh (role check, then an index that cannot be written) that exits 1: the
pointer and the node's held generation stay, the grant is `current`, then `expired` past
`max_age_seconds` with no refresh to renew it, `current` again only through a role check, and
empty under a later set without the login.

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
`grant::permissions` unit test reads the same table against the cache. In
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
carries a token, a login or the forged value. #454: a team operator's `added_by` saved while the
console has no provider, then `[oidc.github]` enabled, is excluded `unattributed` by the role check
though its login holds an accepted role, and an addition the provider attributes past the first
set's second is checked by its roles as before; `tests/it/postgres.rs` moves a schema at 9 with a
set to 10, the set kept and its start the first set's time. `tests/it/ui/oidc/team.rs` checks that without
`[oidc.github]` no workspace body carries `excluded`. `tests/it/ui/oidc/audit.rs` also leaves out a record whose stored login fails the login
pattern. `ui/tests/models.test.ts`,
`ui/tests/operations.test.ts` and `ui/tests/operations.test.tsx` hold the session view's login
and state, the admission page's set rows (and `unread` with no set) and the token check's
sources, in both languages.
