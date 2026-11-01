# 24. Local accounts and Meada-signed tokens

- Status: accepted
- Date: 2026-10-31

## Context

Server mode reaches a writable console, team operators, per-agent grants, named workspaces and
the proposal server only through an identity provider:

- the console without `[oidc]` is read only, every request a viewer's behind the operator's
  proxy, and `[ui] operations = true` without `[oidc]` is a configuration error (ADR 0014);
- nodes without `[oidc]` admit whoever the proxy admits, which in the Compose recipe is one
  static bearer every agent shares (ADR 0011, ADR 0012);
- the proposal server admits only an identity provider's token (ADR 0022);
- the GitHub grant provider grants from a login claim only an identity provider carries
  (ADR 0016), and named workspaces need it (ADR 0021).

Wiring an identity provider (an application registration, groups, claims, one audience for the
nodes and another for the proposal server, a login claim) takes a customer days and a security
review, so a proof of concept stalls before anyone has seen the console write a workspace
version, a team operator edit its entries or an agent query a node under its own grant.

The maintainer settled the shape in the triage of spec #551, in fourteen decisions: local
accounts kept in the data layer and managed by the global operator in the console,
modelled on OWASP Dependency-Track; a default account `admin` with a published password that
must be changed first; short-lived session tokens and agent tokens Meada signs with Ed25519; no
cookie; groups assigned by the global operator; a GitHub login proved once with a personal
access token; local sign-in off once an identity provider is configured, with one explicit
break-glass account; and a Compose recipe that starts the whole proof of concept with one
command. Writing the spec added choices and narrowed two decisions (an agent token carries the
inputs of its owner's grant rather than the grant, and the console refuses agent tokens and reads
the account on every write). This ADR records all of them; the spec fixes the details the
tickets build.

## Decision

### The `accounts` module and `[accounts]`

- Local accounts are a cloud module in ADR 0012's shape: the cargo feature `accounts`, off by
  default, implying `oidc` (its JWT code and grant providers) and `postgres`, enabled by `cloud`,
  plus an `[accounts]` section of the bootstrap manifest. It adds one crate, `argon2`, which
  joins the default build's forbidden-crate canary. Ed25519 signing and verification come from
  the JWT crate already in the build, with PEM keys.
- `[accounts]` takes:
  - `public_key_env` or `public_key_file` (at most one of the two, and every process that
    admits a Meada-signed token needs one): the signing key's public half, PEM, read at start by
    the console, nodes and the proposal server;
  - `private_key_env` or `private_key_file` (at most one, which the console requires): the
    private half, PEM, read by the console only, only at start, and refused unless it is Ed25519
    and matches the public key. Nodes and the proposal server, sharing the section, check only
    that the key is named and never read the variable or the file;
  - `admin_password_env` (`MEADA_ADMIN_PASSWORD` by default) or `admin_password_file`: the
    default account's initial password, read only when the console creates the default account
    and by `meada ui reset-admin`;
  - `break_glass` (`false` by default), meaningful only beside `[oidc]`;
  - `[accounts.groups]`: group name to identities (`*` for all), with the rules and comparison
    of `[oidc.groups]`;
  - `[accounts.github]`: the GitHub grant provider for local accounts, the keys of
    `[oidc.github]` without `login_claim` (`host`, `roles`, `max_age_seconds`) plus `api`, the
    GitHub REST endpoint of that host (`https://api.github.com` by default for `github.com`,
    required otherwise).
- Every secret comes from the environment or a file the section names, bounded and never echoed;
  errors name the key, never the value (ADR 0012).
- Section rules, each an error naming the file, the line and the sections: `[accounts]` needs
  `[postgres]`; `[accounts]` beside `[oidc]` needs `break_glass = true`; `break_glass` without
  `[oidc]` is an error; `[accounts.groups]` and `[accounts.github]` are errors under
  `break_glass`; `[accounts.github]` needs `[postgres]`, as `[oidc.github]` does.
  `[ui] operations = true` is accepted with `[accounts]` and no `[oidc]`; the operator keys
  (`operator_groups`, `operator_claim`) keep requiring `[oidc]`.
- `[accounts.groups]` and `[accounts.github]` are the configuration home of local accounts'
  grants, outside `[oidc]`: `[oidc]` present still means an identity provider is configured, and
  its keys keep their meaning. Wherever a module reads the GitHub grant provider (the grant, the
  job's role check, the catalog, named workspaces, the proposal server, the session, the
  admission view, `status`), it reads `[oidc.github]` or `[accounts.github]`, never both.

### Local accounts and the default account

- A local account is one row of a new table in the `[postgres]` schema, added by a migration:
  a username (1 to 64 characters of lowercase ASCII letters, digits, `.`, `-` and `_`, unique,
  compared exactly), an argon2id password hash, a role (`operator` or `viewer`), groups (each
  checked as an owner name is, ADR 0015), whether a password change is required, whether it is
  suspended, its consecutive failures and lock, its last password change, its verified login,
  and who created it when. No email, no self-signup, no deletion, no rename, so an audit
  record's subject always names the same person. Every row is hostile input when read, as every
  data layer row is: a row that fails its checks is skipped with one stderr line naming the
  table, never a value, and admits nothing.
- A local account's subject, wherever a subject is recorded (the `session` document, an audit
  record, a workspace version's author, a workspace operator list, stderr lines), is
  `local:<username>`, so it never equals an identity provider's subject.
- On its start, the console, and only the console, creates the default account when the table
  is empty, in one transaction under an advisory lock so two consoles create it once: `admin`,
  role operator, password `admin123` with a change required. When the deployer set
  `admin_password_env` or `admin_password_file`, that value is the initial password, it must pass
  the password policy (else the start is refused naming the key), and no change is required: the
  forced change exists to retire the published default, and the deployer chose this one. A table
  that is not empty is never touched at start.
- A session token carrying the change-required mark is admitted only by the password change;
  every other route answers 403 `password_change_required`.

### Passwords

- The policy follows OWASP ASVS: 12 to 128 characters counted as Unicode scalar values, any
  character, no composition rule, no truncation; refused when equal to the default password,
  equal to the username ignoring case, or in an embedded common-password list (ignoring case).
  It is one pure function returning the first failed rule, shared by every route and the CLI;
  a refusal names its rule, never the password.
- argon2id at OWASP's recommended parameters (19 MiB, two iterations, one lane), stored in the
  hash string so they can be raised later; a hash with weaker parameters is rehashed at the next
  successful sign-in. At most a fixed small number of hashes run at once in the console; a
  further sign-in or password change answers 503 `busy` without hashing.
- The common-password list is a published list under a licence compatible with Apache-2.0,
  filtered to the entries the length rule would not already refuse, embedded in the binary with
  its source and licence recorded beside it, so the check needs no network.
- Sign-in answers an unknown username, a wrong password, a suspended account and a locked
  account with the same 401 `invalid_credentials`, after hashing against a fixed dummy hash when
  there is no account, so the answer and its time reveal no account; the audit record keeps the
  real reason. Ten consecutive failures lock the account for fifteen minutes, counted in a
  transaction that locks the row; a success resets the count, and a sign-in while locked neither
  checks the password nor extends the lock. A password reset and `meada ui reset-admin` clear
  the lock.

### The signing key and the three surfaces

- Meada signs its own tokens: JWTs with EdDSA over the signing key, issued by the console, whose
  issuer is the console's `[ui] public_url`. A Meada-signed token names exactly one surface by its
  audience, and each surface admits its own audience only:

  | Token | Surface | Validity | Admitted by |
  | --- | --- | --- | --- |
  | session token | the console | eight hours | the console |
  | agent token for the nodes | the nodes | 1 to 168 hours, 8 by default | `meada mcp --listen` |
  | agent token for the proposal server | the proposal server | 1 to 168 hours, 8 by default | `meada workspaces mcp` |

- A session token carries the subject, the role, the groups, the verified login when there is
  one, a change-required mark when set, the issue time and the expiry, and nothing else. An agent
  token carries the same claims with its surface's audience and a random token id, never a
  change-required mark.
- The console refuses agent tokens. An agent token lives up to seven days and nothing revokes it
  before its expiry, so a seven-day operator token must not make console writes; the console
  admits only its eight-hour session tokens.
- One verifier checks a Meada-signed token on every surface: EdDSA only, no `crit`, the
  configured public key, the issuer, the surface's audience, `exp` and `nbf` with the existing
  leeway. A token that fails is a 401 with `WWW-Authenticate: Bearer`, without
  `resource_metadata` since there is no authorization server to discover, before routing. (Amended
  2026-11-01, #560: a node learns the issuer from the `[ui] public_url` of its own manifest,
  which carries the console's `[ui]` section beside `[accounts]` as the refresh job's does, and
  refuses to start without it; no key is added, so the cargo feature `accounts` implies `ui`.)
- The private key lives only in the console, so a compromised node or proposal server cannot
  mint a token. Meada stores no session or agent token and keeps no revocation list: a token's
  expiry is its revocation. Suspension stops new tokens and every console write at once; tokens
  already issued to agents last until they expire; rotating the key pair (a restart of every
  process with the new keys) ends every outstanding token at once, the emergency stop.
- The browser holds the session token in `sessionStorage` for the tab's life and sends it as a
  bearer header, as it sends an identity provider's token. The console still sets and reads no
  cookie, so the same-origin rule stays its whole CSRF surface; `POST /api/v1/login` passes the
  `Host` and `Origin` checks like every write.

### Admission, roles and grants

- The console, with `[accounts]` and no `[oidc]`: every route but the static files, `config`
  and `login` needs a session token. The session's operator flag is the token's role. Every
  console write (generation and workspace operations, named workspace administration, account
  administration, agent token issue, login verification) reads the account's row before it
  acts: a suspended account, or one whose role no longer allows the write, gets the refusal the
  write already has, so suspension and a role change take effect on writes at once. Reads are
  bounded by the token, as an identity provider's are.
- Nodes, with `[accounts]` and no `[oidc]`: the gate admits an agent token for the nodes and
  hands routing the same pass an identity provider's token gets, so routing, the 403, the 404
  listings and the answers bound by the grant are unchanged; the `Origin` 403 still comes first.
  With `[oidc]`, nodes ignore `[accounts]`; without either, the proxy admits, as before.
- The proposal server, with `[accounts]` and no `[oidc]`: it admits an agent token for its
  surface and derives the subject, login, groups, grant and role from that token alone
  (ADR 0022's rule); it reads Postgres as before for workspaces and eligibility.
- Roles: an operator account is the global operator, and so the platform administrator of named
  workspaces (ADR 0020); a viewer reads. A viewer one of whose groups owns an entry of the
  current workspace version that its grant reads is a team operator (ADR 0015): a team is a group
  of a token or of a local account, and the global operator gives entries and accounts the same
  group names.
- The grant of a local account is computed at admission on every surface, from the token's
  inputs: the union of what `[accounts.groups]` maps its groups to and, with `[accounts.github]`,
  the GitHub grant of its verified login from the permissions set (never `*`). The token
  carries the grant's inputs, not its identities, so a role the job's next role check revokes
  stops granting before the token expires, and a token's size does not grow with its grant.
  With neither table, every local account reads every repository (the single-team proof of
  concept), as server mode without `[oidc]` does.
- `session` answers the same document an identity provider's session gets, plus `account: true`.
  `GET admission` shows the mode (local accounts or break glass), the public key's fingerprint,
  the groups table bounded by the caller's grant and the GitHub grant provider's state.
  `POST admission/check` stays an identity provider feature.

### The verified login: the console's one call to GitHub

- A person proves their GitHub login once by pasting a GitHub personal access token (a
  fine-grained one with no permission suffices) in their profile. The console calls `GET /user`
  on `[accounts.github] api` once, through the agent every module uses (no redirect, no proxy,
  a timeout, a size cap, the platform verifier), reads `login`, checks it against the login
  pattern and stores it lowercase on the account. The personal access token is never stored,
  logged, audited or sent anywhere else, and is dropped when the request ends.
- A login another account holds is a 409 `login_taken`. GitHub's refusal or unreachability is a
  fixed 422 or 503 naming GitHub, never its body.
- A person removes their own verified login; the global operator removes anyone's. A token issued
  before a removal keeps the login until it expires. (Amended 2026-11-01, #561: a person's own
  verification or removal answers a fresh session token from the account's row, so their own
  session shows the change at once; other tokens keep what they carry.)
- The GitHub grant provider then grants from the verified login as it grants from `login_claim`
  under an identity provider, which brings named workspaces, their catalog and eligibility, and
  the proposal server to deployments with local accounts. The job's role check and the
  permissions set are unchanged.
- This is the console's only call to GitHub, and the only endpoint it adds: the GitHub grant
  provider's host, already outside the operator's boundary for the job (ADR 0013), reached with a
  credential the person supplies for that one request. The console still holds no GitHub App
  key and never calls the identity provider's API.

### Account administration and `meada ui reset-admin`

- The global operator administers accounts in the console, whatever `[ui] operations` says, so
  a read-only console can still have viewers: list, create (username, role, groups, a temporary
  password and "change on first login", checked by default), change the role or the groups,
  suspend, reactivate, reset the password (which also clears a lock) and remove a verified login.
  Suspending or demoting the last active operator is a 409 `last_operator`.
- A person changes their own password with the current one, which counts toward the lock, and
  gets a fresh session token. Tokens issued before the change last until they expire.
- `meada ui reset-admin --workspace <bootstrap manifest>` (feature `accounts`) is the one CLI
  twin: it connects to Postgres, creates or reactivates `admin` as an operator, clears its lock
  and sets a password with a change required (the value of `admin_password_env` or
  `admin_password_file` when set, else a random one printed once on stdout), and leaves one
  audit record with `via` `cli`, refusing with exit 1 when the record cannot be written.
- The rest of account administration has no CLI twin, against ADR 0014's rule that every
  console write is first a CLI subcommand. That rule exists so recovery and scripts need no
  browser. Recovery needs only `reset-admin`: once the default account is back, the console does
  the rest. A CLI that creates or promotes accounts would make anyone holding the data layer's
  credentials a minter of operators, with no signed-in subject to audit, and would invite
  scripts that keep local accounts as a permanent directory beside or instead of an identity
  provider, which local accounts are not.

### Break glass and the move to an identity provider

- Configuring `[oidc]` turns local sign-in off for every account, so a leftover local password
  never survives the identity provider's deprovisioning. `[accounts]` beside `[oidc]` needs
  `break_glass = true`, which keeps one door: only `admin` may sign in to the console (any other
  account gets the same 401 as a wrong password, recorded with its reason), as the global
  operator. Its session token is admitted beside the identity provider's tokens, told apart by
  issuer before any key is tried. It may change its own password; account administration, agent
  tokens and login verification do not exist under break glass. Nodes and the proposal server
  admit only the identity provider. `config` names the sign-in mode (the identity provider,
  local accounts, or the identity provider with break glass), so the front end offers a discreet
  break-glass sign-in beside the identity provider's.
- Local accounts are not migrated into the identity provider: groups and logins move by
  configuring the identity provider's own.

### Deployment

- The Compose recipe gains a proof-of-concept profile that starts, with one command, the
  generations fleet, Postgres with its migration, the console with `[accounts]` and
  `operations = true`, the signing key pair generated by the one-shot certificates step, nodes
  with the public key, and a node proxy without the static bearer; the static-bearer proxy stays
  for deployments without `[accounts]`.
- The Helm chart carries `[accounts]`, with the private key as a secret mounted only in the
  console and the public key in the nodes and the proposal server.
- The documentation presents local accounts as the start and the identity provider as the move
  to production, with the break-glass flag and the key rotation.

### Audit

- Every sign-in, refused sign-in, lockout, password change, account change, agent token issue,
  login verification and recovery leaves an audit record (ADR 0016), written before the token or
  the change is returned; a record that cannot be written answers 503 `audit_unavailable` and
  issues nothing. A Postgres outage therefore stops sign-in and account changes, never a node's
  answers to tokens already issued.
- New actions: `account.login`, `account.password`, `account.create`, `account.role`,
  `account.groups`, `account.suspend`, `account.reactivate`, `account.reset_password`,
  `account.remove_github`, `account.github`, `account.token` and `account.reset_admin`; `via` is
  `console` or `cli`. (Amended 2026-11-01, #557: a thirteenth action, `account`, records an
  account change refused before its body is read, so before its action is known, as `pointer`
  records an operation's.) Arguments name the account, the fields changed, a token's surface, validity
  and id, the verified login, and a refused sign-in's reason (`unknown`, `password`, `suspended`,
  `locked`, `locked_now`, `not_break_glass`). An attempt naming no account has no subject and the
  role `none`. No password, hash, personal access token or token appears in a record, a stderr
  line, an error or a URL.

## Considered options

- **A cookie session.** An `HttpOnly`, `SameSite` cookie keeps the token out of the page's
  script, but makes the browser send it on cross-site requests the console would then have to
  tell apart, with CSRF tokens beside the same-origin rule ADR 0014 made the console's whole
  CSRF surface, and gives the console a second admission path beside the identity provider's
  bearer. A bearer in `sessionStorage` keeps one admission shape; the console's content security
  policy (`default-src 'self'`) is what guards the page's script, as it already guards an identity
  provider's token held in memory.
- **Stored per-account API keys, as Dependency-Track keeps them.** Revocable one by one, but a
  node would look each key up in Postgres on every request, or keep a copy to age, and become
  stateful about credentials; ADR 0016 keeps nodes off Postgres on the request path. A signed
  token is verified with the public key alone, and its bounded validity replaces the revocation.
- **A revocation list.** Every node and the proposal server would read it per request or poll
  it, the state the signed token avoids. Agent tokens last at most seven days, suspension stops
  console writes at once through the row read, and rotating the key pair ends every token.
- **Carrying the grant's identities in the token.** What the triage decision said literally. It
  freezes the grant for up to seven days, so a GitHub role the job's role check revokes keeps
  granting, and the token grows with the grant. Carrying the groups and the verified login lets
  each surface compute the grant as it does for an identity provider's token.
- **Reusing `[oidc.groups]` and `[oidc.github]`.** `[oidc]` present means an identity provider
  is configured, and the section rules (local sign-in off, break glass, the operator keys) key on
  it; `[oidc.github]` needs a `login_claim` no local token has. An `[oidc]` without an issuer
  would become a state of its own. A home under `[accounts]` keeps the move to an identity
  provider explicit: write `[oidc]` and its tables, and keep `[accounts]` only for break glass.
- **Local accounts beside the identity provider.** Two places to revoke a person, and a local
  password that outlives the identity provider's deprovisioning. Only the break-glass account
  stays, as a console door for the global operator with no agent token, no account
  administration and no grant table.
- **HS256 with a shared secret.** Every process that verifies could also mint, so a compromised
  node could issue operator tokens, and ADR 0012 accepts asymmetric algorithms only. With
  Ed25519 the private key stays in the console.
- **RS256 or ES256 instead of Ed25519.** Supported as well, but with larger keys, parameters to
  choose (key size, curve) and, for ECDSA, randomized signatures. Ed25519 has one parameter set
  and deterministic signatures, from the JWT crate already in the build.
- **Meada as an OAuth authorization server.** Still rejected (ADR 0012): discovery, client
  registration and the authorization code flow are a security surface an identity provider
  already covers. Meada signs tokens for its own three surfaces from its own sign-in and nothing
  else, with no flow, no client and no discovery.
- **CLI twins for all of account administration.** Rejected above; `reset-admin` is the one twin
  recovery needs.
- **Forcing a change of the `MEADA_ADMIN_PASSWORD` value.** The forced change retires the
  published default password; a password the deployer chose for an automated install is not
  published, and forcing its change would stall that install's first scripted sign-in.
- **Refresh tokens.** A longer-lived credential to store and revoke; a signed-in person signs in
  again after eight hours, and an agent token is issued for the run's length.

## Consequences

- What each invariant of ADR 0002 keeps, with no amendment of it:
  1. Indexing never calls an LLM or the network: `meada index` reads no account and opens no
     connection.
  2. Local-first: a local account is a row in the operator's own Postgres, created by the
     operator's console inside the operator's boundary, signed by a key pair the operator
     generates and holds. It is not an account with Meada or with a vendor: nothing registers
     anywhere, and the default build and local use still need no account and no key. The one new
     outbound call is the console's `GET /user` on `[accounts.github] api`, the GitHub grant
     provider's host the job's fetches already reach, made only when a person verifies their
     login, with the credential they paste. Source code does not move.
  3. Meada writes only inside its own state: one new table in the `[postgres]` schema, added by
     `meada postgres migrate`. Meada never writes a key, a secret or a bootstrap manifest; the
     key pair is generated by the deployment (the Compose recipe's one-shot step, a chart's
     secret).
  4. The MCP server has no tool that writes: the nodes and the proposal server gain no tool and
     no tool parameter; only their admission accepts a new token. Account writes are console
     routes and one CLI command.
  5. One index per repository: unchanged.
  6. Determinism: a local account's grant gives the answers an identity provider's token with the
     same grant gets; no token, account, subject or time reaches an answer or an export.
- Inline amendments point here: ADR 0011 (Meada checks its own tokens, still without rmcp's
  `auth`); ADR 0012 (the `accounts` row of the module table, the outbound endpoint
  `[accounts.github] api` called by the console, "issues none ... is no authorization server",
  the whole workspace without `[oidc]`, and the authorization server option, still rejected);
  ADR 0014 (operators managed in the identity provider, Meada issuing no token, the read-only
  console and `operations = true` without `[oidc]`, the console never calling GitHub, every
  console write first a CLI subcommand, the author of a console save, writes behind the reverse
  proxy alone, and the no-cookie sentence restated as still true); ADR 0015 (a team is a group of
  a token or of a local account, team operators without `[oidc]`, and the team table option);
  ADR 0016 (the identity provider as the one token issuer, who connects to Postgres, `added_by`
  from the verified login, the console never calling GitHub, and the several issuers option);
  ADR 0022 (what the proposal server connects to, admission by an agent token for its surface,
  and the subject derived from it). The GLOSSARY gains Local account, Default account, Break-glass
  account, Session token, Agent token, Signing key and Verified login, and amends Operator,
  Owner, Identity provider and Grant provider.
- Everything is additive under Spec R1's stability rule (a feature, a section, a table, routes,
  audit actions, one subcommand): without `accounts`, or without `[accounts]`, every ADR reads as
  before, and the OIDC tests stay green and unchanged.
- The deployer owns more: generating, holding and rotating the signing key pair, keeping the
  private key in the console only, changing or setting the default password, and rate-limiting
  `POST /api/v1/login` at the proxy, since refused sign-ins of unknown usernames grow the audit
  as every refusal does.
- A security reviewer trades revocation for statelessness: an agent token issued before a
  suspension, a role change or a login removal keeps its claims until it expires (at most seven
  days on the nodes and the proposal server), and the console's writes are the only surface that
  reads the account. Rotation is the stop that ends them all.
- Out of scope: email, self-signup and password reset by email, multi-factor authentication and
  passkeys, refresh tokens, token introspection, listing or revoking issued tokens, deleting or
  renaming accounts, teams as a table, SCIM for local accounts, key rotation with overlapping
  keys, and the token check for Meada-signed tokens.
- The spec's tickets land as a loop tier of their own, this ADR first.
