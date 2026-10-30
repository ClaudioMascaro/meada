# Enterprise identity

Identity automation ([S4, #319](https://github.com/ClaudioMascaro/meada/issues/319), [ADR 0016](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0016-identity-automation.md)) is delivered. It adds to [server mode](server.md) and the [console](console.md) a Postgres data layer, an audit, identity provider types, the GitHub grant, team additions, a SCIM directory and Entra ID overage. Every feature is opt-in: without the new sections, every process behaves as before. The [recipes](identity-providers.md) for Entra ID, Okta and Keycloak show the provider side of the setup.

The identity provider issues the token. The effective grant combines the identities allowed by claims and, when configured, by GitHub permissions. The refresh job checks GitHub roles, never the console, the nodes or indexing. Owners and human roles do not widen that grant. Named workspaces, context creation derived from effective repository roles (direct or group-derived) and viewer access inherited without a second approval belong to [S5, #339](named-workspaces.md), still planned (only the [repository catalog](github-app.md#the-repository-catalog) is delivered); GitHub's `role_name` does not identify a team, and authoring through the UI or MCP is not delivered either.

<!-- meada-graph:enterprise -->

This page follows the order of a deployment: data layer, audit, provider type, GitHub grant, additions, directory, overage and ceilings.

## Data layer

Postgres 15 or later holds the durable mutable state: audit, directory and permissions. The snapshot store keeps immutable documents and bundles, and Valkey keeps pointers, locks, fences and caches. Enable the `postgres` feature (included in `cloud`) and the `[postgres]` section of the bootstrap manifest:

```toml
[postgres]
url_env = "MEADA_POSTGRES_URL"      # the default; or url_file = "/run/secrets/postgres-url"
ca_file = "/etc/meada/ca/ca.pem"    # optional: verifies against this file alone
schema = "meada"                    # the default; lowercase letters, digits and _, 1 to 63 characters
pool_size = 4                       # the default
connect_timeout_seconds = 5         # the default
statement_timeout_seconds = 10      # the default
```

- **The URL** comes from `url_env` or `url_file` (at most one), never the manifest, which stays free of secrets. It is `postgres://` or `postgresql://` with a TCP host, and may carry a user, a password, a port and a database, but no query parameter: `sslmode` and the rest are errors, because TLS is Meada's decision. An error names the key, never the URL.
- **TLS.** A host outside loopback (`localhost`, `127.0.0.0/8`, `::1`) always uses TLS with the host name verified; only loopback uses plain TCP (unless `ca_file` is set). Without `ca_file` the system roots apply; with it, only that file's.
- **The schema** `[postgres] schema` holds every table, and Meada writes nothing outside it; several deployments can share a database with different schemas.
- **Migrations.** Only `meada postgres migrate --workspace <bootstrap manifest>` creates or alters a table. It creates the schema when absent, applies each migration in its own transaction under an advisory lock of the schema (two runs serialize) and prints the version reached. Run it **before every upgrade of the binary**. Every other command that connects reads the version: one lower than the binary's is the error naming `meada postgres migrate`, one higher names both versions. The console and the CLI commands refuse to start or exit 1; a node writes the error once on stderr and treats Postgres as unreachable, leaving its readiness unchanged.
- **Three Postgres roles** are recommended: `migrate` (owns the schema), one that writes (the console, the job's role check and the CLI twins) and one that reads (the nodes). Example, with the database `meada` and the roles already created:

    ```sql
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT USAGE ON SCHEMAS TO meada_write, meada_read;
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO meada_write;
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT USAGE, SELECT ON SEQUENCES TO meada_write;
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT SELECT ON TABLES TO meada_read;
    ```

- **Who connects:** the console (audit, directory, permissions, overage), nodes with `[oidc.github]` or overage through the directory (reads only), `meada github-app roles` and the CLI twins that write. `meada index` never connects.
- **Backups and availability are the operator's**, as for Valkey and the store. A lost database costs the audit's only copy (stderr remains); the directory comes back with the identity provider's next full cycle, and the permissions with the next refresh.
- **Upgrade window.** From `meada postgres migrate` until every process runs the new binary, older processes lose the data layer: writes fail on the audit and nodes let the GitHub grant age out.
- `meada status --workspace` with `[postgres]` prints `postgres: reachable, schema <name> version <n>` or `postgres: unreachable (<reason>)`, without changing the exit code.

The [Compose example](https://github.com/ClaudioMascaro/meada/tree/main/deploy/compose) and the [Helm chart](https://github.com/ClaudioMascaro/meada/tree/main/deploy/helm/meada) ship a Postgres and the migrate step, off by default.

## Audit

With `[postgres]`, every console write and every CLI twin leaves a record: `workspace.save`, `workspace.import`, `workspace.apply`, `pointer.set`, `pointer.retain`, `pointer.drop` and `fence.recover` (plus `pointer`, only for a role refusal before the body is read, with arguments `{}`). A record carries `id`, `time` (Postgres's clock), `via` (`console` or `cli`), `subject`, `login` when there is one, `role` (`viewer`, `operator` or `team_operator`), a team operator's `owners`, `action`, `arguments`, `outcome` (`pending`, `accepted`, `refused` or `failed`) and `message`. Refusals are recorded too. The job's publish, `meada github-app roles` and SCIM writes are not audited: each leaves its own record.

The audit **fails closed**: the `pending` record is written before the write touches Valkey or the store, and if it cannot be written the write is refused and nothing changes (the console answers 503 `audit_unavailable`; the CLI exits 1 naming Postgres). A Postgres outage stops writes, never reads. The global operator reads the records on the console's **Audit log** page or at `GET /api/v1/audit?before=<id>&limit=<1 to 100>`. The audit line on stderr does not change. Meada never deletes a record: pruning is the operator's, in SQL, for example `DELETE FROM meada.audit WHERE time < now() - interval '1 year'`. More in [Console](console.md#audit).

## Identity provider

`[oidc]` is the identity provider, the deployment's one token issuer; all its keys of today stay as they are. `type` says which one it is: `generic` (the default), `entra`, `okta` or `keycloak`. `okta` and `keycloak` behave as `generic` and only name the [recipe](identity-providers.md); `entra` has the `[oidc.entra]` table with one key, `overage` (`ignore`, the default, or `directory`; see [Entra overage](#entra-overage)). `[oidc.entra]` under another `type` is an error. See [Authentication and grants](admission.md#configure-oidc).

## GitHub grant

`[oidc.github]` makes GitHub roles a second source of the grant, unioned with the claims grant: what a subject's GitHub login reads on GitHub, it reads in Meada.

```toml
[oidc.github]
host = "github.com"
login_claim = "github_login"       # the token claim holding the GitHub login
roles = ["read", "triage", "write", "maintain", "admin"]   # the default; custom roles too
max_age_seconds = 5400             # required, at least 60
```

- **The login** comes from the `login_claim` claim (each identity provider maps it; see the [recipes](identity-providers.md)). Only a string of 1 to 39 ASCII letters, digits, `-` and `_` counts, compared in lowercase. A missing or malformed claim gives no GitHub grant, and the claims grant is unaffected. The identity provider's administrator controls that mapping: use a verified attribute, never one the user edits.
- **Accepted roles** are GitHub's `role_name`, compared exactly, case and spaces included; custom repository roles go in the list. The default is the five built-in roles.
- **`max_age_seconds`** is "one refresh interval": the job's schedule plus its longest run. Processes honor the newest permissions set for that long past its `finished`, on Postgres's clock; after that the GitHub grant is empty, whether the job stopped or Postgres became unreachable.
- **The job's refresh** becomes, under `meada snapshot lock`: `meada workspace pull`, `meada github-app roles`, the fetch of the mirrors `meada workspace show --pulled` lists (a fetch script reads that output), `meada index --workspace` and `meada snapshot publish`.

    ```sh
    meada snapshot lock --workspace job.workspace.toml -- sh -ec '
      meada workspace pull --workspace job.workspace.toml
      meada github-app roles --workspace job.workspace.toml
      meada workspace show --pulled --workspace job.workspace.toml > pulled.toml
      sh fetch-pulled.sh pulled.toml
      meada index --workspace job.workspace.toml
      meada snapshot publish --workspace job.workspace.toml'
    ```

- **A failed role check stops the refresh.** `roles` exits 1 and writes no set (every installation throttled, a 5xx, a timeout, Postgres unreachable), the previous set stays and ages until nodes refuse it, and no generation is published with roles the job could not read. It also removes the exclusions file, so `index`, `status` and `snapshot publish` refuse until a check succeeds: the recovery is to run `meada github-app roles` again (a run that a newer fence refuses leaves the file alone). A repository GitHub does not show (404, or no installation covers the owner) gets no rows and a warning, and the refresh goes on. This is not a fetch failure under [#316](https://github.com/ClaudioMascaro/meada/issues/316): an eligibility exclusion is a decision made before the fetch.
- **The session** shows `login`, `github_host` and `github_grant` (`current`, `expired` or `unread`), and the token check separates identities into `sources`.
- The measurement against a real organization with teams and a base role is [#338](https://github.com/ClaudioMascaro/meada/issues/338); until then, the cost per refresh is in the [known ceilings](#known-ceilings).

Details in [GitHub App credentials](github-app.md#role-check).

## Additions under the provider

Under `[oidc.github]`, a team operator may add, for its team, an entry of a repository on the `host` that its grant does not read yet, with no `[oidc.groups]` mapping.

- **`added_by`** is an optional entry key: the GitHub login of whoever added it (1 to 39 lowercase letters, digits, `-` and `_`). It enters the version id and is an error in an entry of the bootstrap manifest itself.
- **Who sets it.** A team operator's save sets the `added_by` of each addition on the `host` to the saver's login, whatever text was submitted, and clears it on the others. The global operator and the CLI store the value as written: that is how the global operator adopts a team's addition (sets or replaces its `added_by`, or clears it); its own additions carry none and are fetched as before.
- **Refusals.** `unattributed`: an addition on the `host` without `added_by`, or from a token without a login. `added_by_change`: an edit that changes, clears or sets an entry's `added_by`. They join `entries_refused` in the order `outside_grant`, `unattributed`, `ownerless`, `owner_change`, `added_by_change`, `not_your_team`.
- **The job** fetches, indexes and publishes the entry only while its `added_by` holds an accepted role. An excluded entry is absent from the workspace; the console marks it `excluded: true` in the workspace views, for those who see it, and the job's stderr names the identity and the reason, never the login.
- **`outside_grant` reveals.** An identity the workspace already holds outside the team's grant is refused with `outside_grant` on save. That tells the team it exists, and the editor cannot predict that refusal (see the [ceilings](#known-ceilings)).

More in [Console, Teams](console.md#teams).

## Directory

`[directory]` (feature `directory`, included in `cloud`; needs `[postgres]` and `[oidc]`) turns on a SCIM 2.0 endpoint on the console, fed by the identity provider:

```toml
[directory]
token_env = "MEADA_SCIM_TOKEN"   # or token_file; read by the console at start
user_claim = "oid"               # the token claim that identifies the user
user_attribute = "externalId"    # or "userName"
group_attribute = "externalId"   # "displayName" by default
```

- **The SCIM URL** is `<the console's public URL>/scim/v2/`, with `Authorization: Bearer <SCIM token>`. The token has at least 32 characters; the console does not start with a short, missing or unreadable token, and **rotating it means restarting the console**. No console token works there, the SCIM token does not work under `/api/v1/`, and a request with `Origin` gets 403.
- **Deprovisioning in the console only.** A user whose `user_claim` finds an inactive user, or one deleted and not provisioned again with `active` true, gets 403 `deprovisioned` on every route but `GET /api/v1/config`. SCIM deletion of a provisioned subject leaves a durable block; a never-provisioned subject keeps normal admission. Nodes do not check this: a deprovisioned user's live token reads until it expires.

<!-- meada-graph:scim-revocation -->

- **The owner picker.** `GET /api/v1/directory/groups?prefix=<text>` (global operator only) lists up to 100 groups, and the editor offers them while editing an `owner`. Once any group has been provisioned, an entry whose `owner` names no group shows `owner_missing: true`.

More in [Console, Directory](console.md#directory-scim).

## Entra overage

An Entra ID user in more groups than a token can carry gets `_claim_names` instead of `groups`. With `type = "entra"`, `[oidc.entra] overage = "directory"` gives such a token the groups the directory knows: the `group_attribute` values of the groups the active user (the `user_attribute` equal to `user_claim`) belongs to. That is why it needs `[directory]` and `groups_claim`: Meada never calls the source endpoint (Microsoft Graph), and only the directory the provider pushes can answer. The groups replace the missing claim everywhere it is read (grant, operators, teams, session). The node or console looks the token up once and keeps the answer for `OVERAGE_TTL`. With `overage = "ignore"`, the default, a token in overage has no groups.

## Known ceilings

- **GitHub's cost per refresh:** one listing per repository on the host, `ceil(collaborators / 100)` requests each. An installation token gets 5,000 requests an hour, plus 50 per repository or user past 20, at most 12,500 (15,000 on GitHub Enterprise Cloud), and 900 points a minute of REST under the secondary limits. With an organization base role, every member is a collaborator of every repository: 1,000 repositories and 2,000 members are 20,000 requests per refresh, past one installation's hour. Spreading over installations works only across owners. Past the budget a listing is throttled, `roles` exits 1 and no refresh publishes until the budget returns, for every team. The upgrade path, sized by the #338 measurement: when the organization's base role is accepted, list its members once and, per repository, only the collaborators that do not come from the base role.
- **Revocation latency:** a revocation on GitHub reaches Meada at the next refresh that succeeds plus at most `PERMISSIONS_POLL` (60 seconds); without a successful refresh, at most `max_age_seconds` after the last one.
- **Memory:** a process holds every accepted-role row of the newest set (logins times repositories under a base role).
- **`OVERAGE_TTL`** (5 minutes): a membership change reaches an overage subject that late.
- **Live token:** a deprovisioned user's token keeps reading on nodes until it expires.
- **`outside_grant`** tells a team operator that the identity is already in the workspace, and the editor cannot predict it.
- **One GitHub host** per deployment; a login is at most 39 characters and a role name at most 100.
- **SCIM:** the token is read at start (rotating means restarting the console); bodies at most `SCIM_BODY_MAX` (1 MiB), one stored resource at most `SCIM_RESOURCE_MAX` (64 KiB), pages at most 100.
- **The audit grows without bound** until the operator prunes it.
- **Postgres 15 and later.** From `meada postgres migrate` until every process runs the new binary, older processes lose the data layer: writes fail on the audit and nodes let the GitHub grant age out.
