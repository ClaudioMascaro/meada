# 16. Identity automation: providers, the GitHub role grant, the data layer and the directory

- Status: proposed
- Date: 2026-10-22

## Context

ADR 0015 lets teams own entries of the one workspace version, and every identity fact it relies
on stays manual and static. A token's grant is the union of the identities `identities_claim`
lists and those its groups map to in `[oidc.groups]` (ADR 0012, #290, #291), a table of the
bootstrap manifest that changes by pull request and redeploy. A team operator adds only
identities its grant already reads, so a team's new repository waits for the global operator to
map it, while who may read it is already decided on GitHub, by its collaborators and their
roles. Groups and users exist only inside a token's claims at admission, so Meada cannot list
teams, offer an owner picker, notice an owner group deleted or renamed in the identity provider,
or see a deprovisioned user. A Microsoft Entra ID user in more groups than a token carries gets
`_claim_names` in place of `groups`, and has no groups at all. `[oidc]` is one section for one
issuer, with no notion of which identity provider it is. Every write leaves one stderr line
(ADR 0014), and nothing durable answers who changed what, when, which an enterprise audit
expects. Meada keeps no durable mutable state to hold any of this: Valkey is not durable, and
the snapshot store holds immutable documents.

Triage of #319 on 2026-10-22 settled eleven decisions with the maintainer: GitHub roles as a
grant source unioned with the claims grant, accepted by their exact names and checked by the job
at every refresh, never by the console; a login claim; `[oidc]` as providers; Postgres as a cloud
module the console and the nodes connect to, a node's GitHub grant bounded by one refresh
interval; an audit of every write; an inbound SCIM directory; Entra overage through it; ADR
0015's addition rule; and the build order. Writing spec S4 (#319) settled the rest. This ADR
records the decisions and their reasons; the spec fixes the keys, routes and formats the tickets
build, and wins where the two could disagree.

## Decision

### The data layer: Postgres as a cloud module

- Postgres is a cloud module in ADR 0012's shape: the feature `postgres`, off by default and
  enabled by `cloud`, plus a `[postgres]` section that needs no other section. It is the durable
  mutable state server mode lacked, beside the immutable snapshot store and the volatile Valkey.
  The client is an asynchronous Postgres client on the tokio runtime the binary already runs,
  with the rustls provider it already has: no ORM and no second TLS stack. Postgres 15 and later.
- The connection URL comes only from the environment or a file the section names, as every
  secret does (ADR 0012), and carries no query parameter: TLS is decided by the host, never by
  the URL. A host outside loopback always connects over TLS with the host name verified.
- Every table lives in one schema the operator names (`[postgres] schema`), every statement
  names it, and Meada writes nothing outside it. The schema joins the store prefix, the Valkey
  key prefix and the local directories as Meada's own state (ADR 0002, invariant 3).
- Migrations are explicit: `meada postgres migrate` creates the schema and brings it to the
  binary's version, under an advisory lock, and no serving process creates or alters a table.
  Every other command and process reads the schema version when it connects and refuses one it
  does not know: lower names `migrate`, higher names both versions. The console and the CLI
  refuse to start or exit 1; a node writes one stderr line, treats Postgres as unreachable and
  keeps its readiness, as an unreachable store or Valkey changes no readiness today.
- Who connects: the console (audit, directory, permissions, overage), nodes with `[oidc.github]`
  or overage through the directory (reads only), the job's `meada github-app roles`, and the
  CLI twins that write. Nothing else connects, and `meada index` never does. Three Postgres
  roles are recommended: one for `migrate`, one that writes, one that reads. Backups and
  availability are the operator's, as for Valkey and the store: a lost database costs the
  audit's only copy, while the directory and the permissions come back with the next push and
  the next refresh.

### The audit

- With `[postgres]`, every write the console makes and every CLI twin of one leaves an audit
  record: who (the subject, its GitHub login when there is one, its role and, for a team
  operator, the owner groups that authorized it), what (the action and its arguments), the
  outcome and the time from Postgres's clock. Refusals are recorded too: attempts stay visible.
- The audit fails closed. The record is inserted as `pending` before the write touches Valkey or
  the store, and its outcome is set after; when the insert fails, the write is refused and
  writes nothing (503 `audit_unavailable` in the console, exit 1 on the CLI). An enterprise
  audit expects no change without a record, and a record written after the fact can be lost
  with the change already made. The cost is accepted: a Postgres outage stops writes, never
  reads or answers, since nodes need no Postgres to serve.
- The job's publish, the role check and SCIM writes are not audited: each leaves its own record
  (the generation document, the permissions set, the identity provider's own log). The stderr
  line of ADR 0014 and ADR 0015 is unchanged. The global operator reads the audit through one
  route and one console page; it grows until the operator prunes it, in SQL.

### `[oidc]` as the identity provider plus grant providers

- `[oidc]` is the identity provider: the one token issuer of the deployment, now typed
  (`generic`, `entra`, `okta`, `keycloak`) with a table of settings per type that needs one
  (`[oidc.entra]`). Every key it takes today keeps its name, meaning and default, so no bootstrap
  manifest changes and there is no migration.
- The claims grant (`identities_claim` and `[oidc.groups]`) is the identity provider's grant and
  stays where it is. A grant provider is a source of a token's grant; `[oidc.github]` adds the
  second one. A token's grant is the union of every configured grant provider's identities, `*`
  from the claims for all.
- The restructuring changes no behavior: without the new keys a bootstrap manifest gives the same
  admission, grant, answers, refusals and stderr lines, byte for byte, on nodes and in the console.

### The GitHub grant provider

- `[oidc.github]` grants a subject what its GitHub login reads on GitHub: a host (one per
  deployment), the claim carrying the login (`login_claim`, required, since no identity provider
  carries it under one name), the accepted roles (compared exactly with GitHub's `role_name`,
  custom repository roles included, the five built-in roles by default) and `max_age_seconds`.
  It needs `[postgres]`. With it, a deployment may grant from GitHub alone.
- Roles are checked by the job, at every refresh. `meada github-app roles` runs between `meada
  workspace pull` and the fetch, lists each repository's collaborators through the installation
  ADR 0013's selection picks (its throttle marks, round robin and token cache), with
  `affiliation=all` so members through teams and the organisation's base role are listed with
  their highest role, and needs no App permission beyond Metadata (read). It writes one
  permissions set per run, in one transaction (every row GitHub listed whatever its role, so one
  set also serves the token check and the admission view), replaces the previous set, and is
  fenced by the refresh lock's fence, so a stalled job cannot undo a revocation. A throttled or
  failed listing fails the command, which writes nothing, and the refresh stops: a partial set
  would revoke every repository left unlisted or mix stale rows with fresh ones. A repository
  GitHub does not show gives no rows and a warning, and the run goes on: a missing installation
  is a configuration fact that fails closed without stopping every team's refresh.
- The GitHub grant of a token is the identities of the newest set for which its login holds an
  accepted role, never `*`, unioned with the claims grant, which stays for machine tokens and
  repositories outside GitHub. A missing or malformed login gives no GitHub grant and leaves the
  claims grant alone. Ownership still widens nothing (ADR 0015).
- Nodes and the console poll the newest set's id once a minute and load the set only when it
  changes, so admission never waits on Postgres. The set grants for at most `max_age_seconds`
  past its completion, measured on Postgres's clock plus the process's monotonic time since the
  last read, then the GitHub grant is empty until a newer set arrives. The bound is the
  operator's "one refresh interval": a node cannot learn the job's schedule, and one age rule
  covers a dead job and an unreachable database without comparing two clocks. It has no
  default, since a wrong one silently revokes or over-grants.
- The console never calls GitHub: it would need the App's private key in a browser-facing
  process and call a host outside the operator's boundary (ADR 0012, ADR 0014). The job already
  holds the credentials and runs at every refresh, so one check feeds the grant, the exclusions
  and the views.

### Additions under the GitHub grant provider

- A team operator may add, for its team, a repository of the provider's host that its grant does
  not read yet: a repository outside the workspace has no permissions listed. The entry records
  the adder's login in `added_by`, part of the entries' canonical text after `owner`, so it
  changes the version id and never a generation or bundle id, and an error in a static manifest,
  as `owner` is.
- The console sets `added_by` from the saver's `login_claim` on a team operator's additions on
  the host, whatever the submitted text says, so nobody attributes an addition to someone else;
  the global operator and the CLI twins keep the text as written, so the global operator may
  adopt a team's addition, and its own carry none. Two refusal reasons join ADR 0015's:
  `unattributed` (an addition on the host without a login) and `added_by_change` (a team
  operator changing the attribution). `outside_grant` is not applied to an addition on the host
  that carries `added_by`, with one exception: an identity the workspace already holds outside
  the grant is refused `outside_grant`, which tells the team operator it is present. ADR 0015
  avoided that; it is accepted under shared trust, and it is the one refusal the editor cannot
  predict. Authorization and composition stay one pure function pair of `meada-core`, given the
  login and the host, so the browser and the server answer alike.
- The job leaves unverified additions out before the fetch: the role check computes, from the
  rows it just listed, every addition whose `added_by` holds no accepted role, writes them into
  the set and into an exclusions file beside the pulled version, and every reader of the pulled
  version (`index`, `status`, `export`, `publish`, `show --pulled`) treats them as absent and
  refuses to run without the file, so `meada index` reads a file, never Postgres (invariant 1).
  An excluded entry is never fetched, indexed or published: the fleet's credentials never fetch
  a repository for someone GitHub does not let read it. It is an exclusion decided before the
  fetch from a fact GitHub states, not a failed fetch, and takes no part in #316. (Amended
  2026-10-30; #340: ADR 0021 applies the same rule to a named workspace's entries, with its
  operators as sponsors: an entry no operator holds an accepted role on is excluded
  `no_sponsor`, per workspace and identity, in the set and in the exclusions file.)

### The directory

- `[directory]` is a cloud module of its own (feature `directory`, implying `postgres`, needing
  `[postgres]` and `[oidc]`): a SCIM 2.0 endpoint on the console's listener under `/scim/v2/`,
  which the identity provider pushes users and groups into. It is inbound only: Meada never
  pulls from the identity provider's API. The endpoint has its own admission, a long-lived bearer
  credential read from the environment or a file and compared in constant time, refuses any
  request carrying `Origin`, accepts no console token, and its token is accepted nowhere else.
  The surface is what Entra ID's and Okta's provisioning send, users and groups, no more.
- The directory feeds three things, all in the console: an owner picker for the global operator;
  a flag on every entry whose owner names no group, so a team renamed or deleted in the identity
  provider is noticed; and deprovisioning, where a user the identity provider deactivated is
  refused on every route and its memberships count for nothing. A user the directory does not
  hold is admitted as before, since a deleted user, a machine token and a provider that does not
  push everyone look alike. Only the console checks: nodes admit bearer tokens statelessly, and
  a deprovisioned user's live token reads there until it expires.

### Entra ID overage through the directory

- With `type = "entra"`, `[oidc.entra] overage = "directory"` gives a token in overage (no
  `groups_claim` claim, `_claim_names` mapping it to a source) its groups from the directory:
  the groups whose members include the active user the token's `user_claim` matches. Meada
  never calls the source's endpoint. The groups found stand in for the missing claim everywhere
  it is read: the claims grant, the operator groups, the team operator's teams and the session.
- Nodes and the console look the subject up once at admission and cache the answer per subject
  for a bounded time: the only lookup on a node's request path, made by overage tokens only, and
  with Postgres unreachable and nothing cached the token has no groups, as under the default
  `overage = "ignore"`.

## Considered options

- **A grant derived from ownership.** Triage's first idea: a team's owner group would read what it
  owns. Ownership would widen the read grant, which ADR 0015 rejected, and the console would
  hold two rules for what a subject reads. GitHub roles widen the grant from the source that
  already decides who reads the repository; ownership still grants nothing.
- **Ownership read from GitHub** (CODEOWNERS, teams). Rejected by ADR 0015: the owner is a fact
  of the workspace version. Roles are read for the grant, not for ownership, and in the job.
- **Pulling users or groups from the identity provider's API** (Microsoft Graph, Okta's and
  Keycloak's admin APIs, Entra's `_claim_sources` endpoint). An outbound call to a host outside
  the operator's boundary with a credential of the identity provider, against ADR 0012's
  outbound rule and ADR 0014's console that calls nothing it discovers. SCIM is the provider's
  own push, inbound, with a credential Meada only verifies.
- **The console calling GitHub at apply time.** The App's private key in a browser-facing
  process, an outbound call outside the boundary, and a role that is stale by the next refresh;
  the job already holds the credentials and runs at every refresh.
- **The audit as append-only documents in the snapshot store, possibly with Object Lock.** One
  object per write, no sequence across writers, no query without listing the prefix, and Object
  Lock is a feature of some buckets rather than of every S3-compatible store. The maintainer
  chose Postgres, which the permissions set and the directory need in any case.
- **Entra's app-assigned-groups filter instead of the directory.** It still overflows past the
  token's limit, covers only the groups assigned to the application, leaves assignments to the
  tenant's administrators, and gives no owner picker and no deprovisioning.
- **Postgres read by the console only, with nodes fed by a snapshot** (in Valkey or the store). A
  second copy of the permissions to keep consistent and to age, in a Valkey that is not durable
  or a store with no fenced write. Nodes read one id a minute instead.
- **A Postgres requirement for every server-mode deployment** (an enterprise profile). It would
  break every S2 and S3 deployment and tie the console to a database it may not need. A module
  in ADR 0012's shape, with the audit only when `[postgres]` is present, keeps those as they are.
- **Several token issuers.** One audience, one JWKS and one grant per deployment keep admission
  one function; a second issuer is out of scope.
- **A partial permissions set on a throttled listing.** It would revoke every repository left
  unlisted, or mix stale rows with fresh ones; failing the run keeps the previous set, which
  ages out on its own.
- **Nodes checking deprovisioning per request.** A Postgres read on every request of every node,
  which admission would wait on, for a token the identity provider stops renewing anyway. Nodes
  stay stateless; the console checks.

## Consequences

- What each invariant of ADR 0002 keeps:
  1. Indexing never touches the network: `meada index` reads the pulled version and the
     exclusions file from disk and never connects to Postgres; `meada github-app roles` is a
     separate command.
  2. Source stays within the operator's boundary: Postgres is an endpoint the operator configures
     and holds no source; GitHub's REST was accepted by ADR 0013 and only the job calls it; SCIM
     is inbound.
  3. Meada writes only in its own state: invariant 3 gains, inline, the Postgres schema
     `[postgres] schema` names; the exclusions file lives under `<index_dir>/workspace/`.
     Nothing else.
  4. The MCP server has no tool that writes: nodes only read Postgres. The console's SCIM writes
     are the identity provider's explicit pushes into Meada's own schema.
  5. One index per repository: unchanged.
  6. Determinism: nothing read from Postgres reaches an export, a trace, a version id, a
     generation id or a bundle id. `added_by` enters the version id as part of the entries. Which
     repositories a generation holds depends on the exclusions as on the entries, and its id
     hashes what it holds; the grant decides which repositories answer, as the claims grant does.
- Inline amendments, each dated and naming #319: ADR 0002 invariant 3 (the Postgres schema); ADR
  0012 (the `postgres` and `directory` rows of the module table, the Postgres endpoint and its
  TLS rule, `meada github-app roles` beside the job's other calls, the grant's mapping rule
  holding for the claims grant with GitHub roles unioned with it); ADR 0013 (the collaborators
  listing as the module's second REST use, under the same selection and throttle marks, with no
  permission beyond Metadata (read)); ADR 0014 (the console connects to Postgres; the SCIM
  prefix with its own admission; the audit route and page; the job's refresh with `roles` and
  `show --pulled`; the session fields; the console still calls neither GitHub nor the identity
  provider's API); ADR 0015 (the addition rule with `added_by`, `unattributed` and
  `added_by_change`).
- The operator owns more: Postgres, its three roles, its backups and availability, `meada
  postgres migrate` before every upgrade (until every process runs the new binary, older ones
  lose the data layer: writes fail on the audit and nodes let the GitHub grant age out), a
  `max_age_seconds` that fits the job's schedule, and the SCIM token, rotated by restarting the
  console.
- Known ceilings that shape the decision: the role check costs one listing per repository on the
  host, `ceil(collaborators / 100)` requests each, and under an organisation base role every
  member is a collaborator of every repository, so a large organisation exceeds one
  installation's hour; a throttled refresh then publishes nothing, for every team, until the
  budget returns. The upgrade path, sized by the first measurement, lists the base role's
  members once and per repository only the collaborators outside it. A revocation reaches Meada
  at the next refresh that succeeds plus one poll, or at most `max_age_seconds` after the last
  one. A deprovisioned user's live token keeps reading on nodes until it expires. An
  `outside_grant` refusal under the provider reveals an identity already in the workspace.
- Everything is additive under Spec R1's stability rule (features, sections, keys, an entry key
  of the workspace version, subcommands, a flag, routes, session fields, refusal reasons): a
  minor change. Without the new sections, every process behaves as ADRs 0014 and 0015 leave it.
