# Management console

The console uses the `ui` feature, included in `cloud`, and runs as its own process. It has an editor, diff, playground and graph. The editor saves configuration versions; applying a version and publishing indexes are separate steps.

Protect access before exposing the console. Without `[oidc]`, the proxy controls admission: anyone reaching the console outside that protection can read the graph. The proxy must preserve `Host`, `Origin` and `Authorization`. The console accepts only its public Host and, if Origin is present, its own origin.

<!-- meada-graph:console-authoring -->

## Configure the bootstrap

The bootstrap is the manifest that starts the console. It holds module settings and uses its own directories, separate from nodes. Repository entries come from the managed workspace.

```toml
index_dir = "/var/lib/meada/console/indexes"

[workspace]
source = "store"

[snapshot]
endpoint = "https://store.exemplo.internal"
region = "us-east-1"
bucket = "meada"
prefix = "production"
path_style = true
credentials = "static"
access_key_id_env = "AWS_ACCESS_KEY_ID"
secret_access_key_env = "AWS_SECRET_ACCESS_KEY"
local_dir = "/var/lib/meada/console/generations"
node_name = "meada-console-0"
# ca_file = "/run/secrets/ca.crt"

[valkey]
url = "rediss://meada@cache.exemplo.internal:6379"
key_prefix = "meada"
password_env = "MEADA_VALKEY_PASSWORD"
# ca_file = "/run/secrets/ca.crt"

[generations]
keep = 3

[refresh_lock]
ttl_seconds = 300

[oidc]
issuer = "https://id.exemplo.internal/realms/agentes"
public_url = "https://mcp.exemplo.internal"
audience = "https://mcp.exemplo.internal"
identities_claim = "meada_repositories"
groups_claim = "groups"
scopes = ["meada:read"]

[oidc.groups]
"meada-readers" = ["github.exemplo.internal/acme/api"]
"meada-operators" = ["*"]

[[github_app]]
host = "github.com"
api = "https://api.github.com"
app_id = 123456
private_key_env = "MEADA_GITHUB_APP_KEY"

[ui]
dir = "/usr/share/meada/ui"
public_url = "https://console.exemplo.internal"
client_id = "meada-console"
audience = "https://console.exemplo.internal"
operations = false
operator_groups = ["meada-operators"]
# operator_claim = "meada_operator"
```

`[oidc]` reproduces node policy, with the MCP public URL and audience. `[ui]` defines the console origin and audience. Export credentials through the environment or the indicated secret files.

<details markdown="1" class="reference-details">
<summary>Certificates, GitHub App and console indexes</summary>

`snapshot.ca_file` adds the private CA to system roots. `valkey.ca_file` uses only that file's roots. Omit `[[github_app]]` if the job uses no Apps.

The console reads the GitHub App cache but neither opens the private key nor calls GitHub. It does not call nodes either: playground and exports use indexes downloaded by the console's own pull.

</details>

## Start the console

[Initialize the managed workspace](workspaces.md) and wait for the job to publish its first generation. Perform the initial pull into the console's `local_dir`. Run periodic pull and the server as separate processes, sharing this console-only directory:

```sh
meada snapshot pull --workspace /etc/meada/console.workspace.toml
meada snapshot pull --every 30 --workspace /etc/meada/console.workspace.toml
# Run in a separate process, alongside the pull companion.
meada ui --listen 0.0.0.0:7334 --workspace /etc/meada/console.workspace.toml
```

## Browser login

Register a public client at the issuer, without a secret, using Authorization Code and PKCE S256. Configure the audience and exact redirect URI `https://console.exemplo.internal/auth/callback`.

The browser must reach the issuer's metadata and token endpoint, with CORS for the console origin. It exchanges the code directly with the issuer. The bearer stays in memory; `state` and `verifier` stay in sessionStorage until the callback removes them. Reloading the page restarts login.

## Fleet and diagnostics

Each `snapshot pull --every` sends a heartbeat, the node's periodic status record. It reports generation, fence, local generations, readiness, version and time.

Use a unique `snapshot.node_name` for each node and the console, with up to 63 lowercase letters, digits, dots or hyphens. Without it, the process uses a valid hostname. Pod names can serve this purpose.

A node without a heartbeat is absent from the fleet. Malformed heartbeats appear separately and do not enter diagnostics. Pages show fleet, store, retention, commits, schemas, App cache and admission; the console identifies missing sections.

## Roles and operations

With OIDC, viewers see only identities admitted by their token. To grant the operator role, choose **one** setting: membership in `ui.operator_groups` or a `ui.operator_claim` with boolean value `true`. Operators see the whole fleet.

With `[workspace] source = "store"`, `operations = true` and `oidc.groups_claim`, a subject one of whose groups owns (`owner = "<group>"`) an entry of the current workspace version that its token reads is a team operator. It sees what a viewer with its grant sees and may save and apply changes to its team's entries; a change to any other entry, to an owner or to an entry without an owner is refused with a 403 `entries_refused` that names the entries (an apply counts the ones outside the grant). Pointer and fence operations stay with the operator. Map the team's group to its identities in `[oidc.groups]`: ownership never widens what a token reads. The versions page shows each entry's owner to every role, and the team operator's editor holds only the entries its token reads, marks a change the API would refuse (with its reason) before it can be saved, and names the authorizing teams in its confirmations instead of a CLI command.

The four HTTP operations require OIDC and `operations = true`. Each action shows its CLI command and asks for confirmation. Refusals show the CLI message, including a busy lock's holder. Every attempt writes an audit line to stderr, without tokens or bodies.

<details markdown="1" class="reference-details">
<summary>Grant, login and recovery limits</summary>

Console operations require OIDC and are off by default. When enabled, they use the CLI's lock and checks to change the pointer, recover the fence and manage versions. Each attempt writes an audit line. Grants also bound manifest groups and errors; login preserves issuer parameters, and recovery respects Valkey's clock and counter ceiling.

</details>

## Audit

With `[postgres]` in the bootstrap (and the schema up to date through `meada postgres migrate`), every console write and every CLI command that mirrors one (`meada workspace import` and `apply`, `meada snapshot pointer set`, `retain` and `drop`, `meada snapshot fence recover`) leaves a record in Postgres: who (subject, role and, for a team operator, the owners), what (action and arguments), when (Postgres's clock) and the outcome (`pending`, `accepted`, `refused` or `failed`). The record is written before the write touches Valkey or the store: when it cannot be written, the write is refused and nothing changes (the console answers 503 `audit_unavailable`, the CLI exits 1 naming Postgres). Role refusals and refusals under the lock are recorded too; invalid bodies (400, 413, 422) are not. The console starts with Postgres down (reads keep working) and refuses to start on a schema version it does not know. The global operator reads the records, newest first, on the **Audit log** page or through `GET /api/v1/audit?before=<id>&limit=<1 to 100>`. The stderr audit line is unchanged, with or without `[postgres]`, and Meada never deletes a record.

## Repository catalog

With `[catalog]` in the bootstrap (feature `workspaces`, beside `[oidc.github]`; see [the catalog](github-app.md#the-repository-catalog)),
the console offers the **Repository catalog** to every admitted subject, and `GET /api/v1/config` carries `"catalog": true`.
It is the picker that comes before any manifest: it lists only the repositories the person may select, that is, those
the catalog holds and on which their login holds an accepted role in the newest permissions set (the platform
administrator sees the whole catalog). It creates no workspace, access list or index.

- `GET /api/v1/catalog?limit=&after=&q=` answers `{"state", "reason", "set", "repositories": [{"identity"}], "next"}`,
  ordered by identity, `limit` 1 to 100 (50 by default), `q` a prefix of at most 128 characters and `after` the opaque
  cursor (at most 256 characters) that `next` returned. Any other parameter, a repeated one or an out-of-range value is a
  400 with a fixed message.
- `POST /api/v1/catalog/check` takes `{"identities": [...]}` (at most 50, 16 KiB) and answers `{"state", "reason",
  "results": [{"identity", "state"}]}` in the order asked: `selectable`, `unavailable` or `pending`.
- `state` is `ready` or `pending`. **Pending** is a condition of the platform or of the caller, the same for every
  identity: `no_set` (the job has not written a set yet), `stale_set` (the set is past `max_age_seconds`, for the
  administrator too) or `missing_login` (the token carries no valid login). While pending nothing is listed and nothing
  new can be selected; entries a manifest already holds stay.
- `unavailable` is every other case (no accepted role, not in the catalog, not covered, new since the last set) and **has
  no reason**: a reason would be an oracle on the catalog. The read never lists an unavailable repository, and an
  identity that does not exist answers like one the person may not select.

Both routes need an admitted, non-deprovisioned subject (the SCIM block applies here as on every route), and the console
answers them from the set it already holds in memory, calling neither GitHub nor Postgres. `GET /api/v1/manifest` shows
`catalog.allow` to the global operator alone.

## Named workspaces

With `[workspaces]` in the bootstrap (feature `workspaces`, beside `[ui] operations = true`, `[oidc.github]`, `[catalog]` and `[workspace] source = "store"`; `mirrors` is the absolute root where the repositories only named workspaces select are mirrored, `max_workspaces` defaults to 200 and `max_entries` to 100), `GET /api/v1/config` carries `"workspaces": {"max_entries": n}` and the console offers the **Named workspaces** page to every admitted subject.

- `POST /api/v1/workspaces` takes `{"name", "description"?, "entries", "message"}` (80 KiB), `entries` being the manifest text of `[[repository]]` entries with a `repo` and, at most, `serve`, `resource` and `peer` blocks (`path`, `owner`, `added_by` and `serve.documents` are refused, 422 `invalid_entries`). The creator is the authenticated subject: an `actor`, `role` or any other key in the body is a 400. It answers 201 `{"id", "version", "created": true}`, or 200 with `created: false` when the same creator, name and entries made it already. A selection with a repository outside the person's eligibility is a 403 `entries_refused` (`refused`: the identities the person's grant reads, `hidden`: how many others) and stores nothing; a set that cannot decide is a 409 `pending` with `reason` (`no_set`, `stale_set`, `missing_login`). Creation is recorded in the audit as `workspaces.create`, refusals included.
- `GET /api/v1/workspaces?limit=&after=` lists the workspaces the subject sees, ordered by id (`limit` 1 to 100, 25 by default), and `GET /api/v1/workspaces/<id>` shows one with its `base`, its `state` (`applied`: stored, ingestion pending; `live`: the current generation records it) and each entry's `ingestion` (`pending`, `ready` or `excluded`, the last shown to the operators). A workspace is visible to its operators, the platform administrator, and whoever's grant reads every one of its repositories; for anyone else it answers the same 404 as an id that does not exist. Operators also get the operator list.
- The playground accepts `"workspace": "<id>"` and `GET /api/v1/workspaces/<id>/repositories/<identity>/export` returns the export through that workspace, under the same rule and always bounded by the caller's grant.
- The **lifecycle** (#343): `POST /api/v1/workspaces/<id>/versions` `{"entries", "message", "base"}` (80 KiB) saves a version and moves nothing (201, or 200 `saved: false` when the version was stored already); `.../apply` `{"version", "base"}` (1 KiB) makes a stored version current and answers `action` `apply` or `rollback` (a version that was current before) with the new `base`; `.../rename` `{"name", "description"?, "base"}`; `.../delete` `{"base"}` (the versions and history stay); `.../operators` `{"add": [{"subject", "login"}], "remove": [subject], "base"}`, the platform administrator's alone. `base` is the one `GET /api/v1/workspaces/<id>` returns; a stale one is a 409 `stale_base` that names the current base. Other refusals: 403 `not_operator` or `not_administrator`, 403 `entries_refused` (with `hidden`) for an entry the person is not eligible for, 409 `pending` when the permissions set cannot decide, 409 `already_current`, 404 `unknown_version`, 422 `invalid_entries`, `invalid_workspace` or `invalid_operators` (`last_operator`, `too_many_operators`); a caller who may not see the workspace gets the 404 of an unknown id. An operator submits the entries it reads and the version stored keeps the ones it cannot read; the platform administrator submits the whole manifest. Each operation is one audit record, `workspaces.save`, `.apply`, `.rollback`, `.rename`, `.delete` or `.operators`, refusals included, and the SCIM block of a deleted user holds for all of them.
- `GET /api/v1/workspaces/<id>/versions?after=` lists the versions (50 per page, newest first, the current one marked; operators also see the author), `GET .../versions/<version>?against=<version or current>` shows one with its entries, its manifest `text` and, with `against`, the diff (`added`, `removed`, `changed`), and `GET .../history?after=<seq>` lists the append-only history to operators and the administrator. All of them hide what the caller's grant does not read: a viewer sees only the versions it reads whole, an operator the entries it reads and a `hidden` count.
- `meada workspaces list|show|versions|diff|create|save|apply|rename|delete|operators --workspace <bootstrap manifest>` are the command-line twins, acting as the platform administrator with `cli:<login>@<host>` as the actor and the same audit; every write takes the `--base` it read.
- The **effective configuration** (#344): `GET /api/v1/workspaces/<id>/effective[?version=]` shows, per entry the caller reads, the `source` of its configuration (`repository`: the mirror's own `meada.toml` wins whole; `context`: the workspace's blocks; `none`), the resolved `blocks`, the `ignored` ones, the `coverage` (`indexed`, `pending_ingestion`, `excluded`) and `diagnostics` (`ignored_by_repository`, `unknown_until_ingested`, `excluded_no_sponsor`, `peer_missing`, `documents_not_indexed`), with the `generation` the facts come from, the workspace's `state` and `recorded` (whether that generation recorded the context at the version shown; when not, the blocks reach queries after the next refresh); `POST /api/v1/workspaces/effective` `{"entries"}` (80 KiB) shows the same view for manifest text before it is saved, stores and audits nothing, and shows a repository the caller's grant does not read as pending. A creation, a save and an apply answer with the same `diagnostics` (`[{repo, code, detail?}]`) beside their result. The workspace page has a text editor for the manifest of a workspace the subject operates: it validates with the same WebAssembly as the server, shows the effective configuration, then saves and applies at the base it read.
- **Guided authoring** (#345): the same page edits the manifest through form controls (repositories from the catalog, serve settings, resources, peers and environment-variable names) that are one document with the text: both go through `meada-core`'s WebAssembly (`namedEntriesJson`, `namedEntriesText`, `namedDiff`, `namedChanged`, `namedAuthorize`), so the form adds no policy. It shows each entry's eligibility, ingestion and effective configuration in text, the diff of the draft against the stored version, and freezes the reviewed payload (text, message, `base`) until *Save and apply* is confirmed. It needs no new route: it reads the catalog (`GET /api/v1/catalog`, `POST /api/v1/catalog/check`) and uses the routes above; a `403 entries_refused` reads the catalog again, a `409 stale_base` offers a reload and a `403 not_operator` removes the editor. Viewers see no mutation control.

## Directory (SCIM)

With the `directory` feature (part of `cloud`), `[directory]` in the bootstrap, beside `[postgres]` and `[oidc]`, makes the console serve a SCIM 2.0 endpoint under `/scim/v2/`, which Entra ID's or Okta's provisioning pushes users and groups into:

```toml
[directory]
token_env = "MEADA_SCIM_TOKEN"   # or token_file; read by the console at start
user_claim = "oid"
user_attribute = "externalId"    # or "userName"
group_attribute = "externalId"   # "displayName" by default
```

The endpoint accepts only its own bearer token, at least 32 characters long: the console refuses to start with a short, missing or unreadable token, and rotating it means restarting the console. A console token is no good there, the SCIM token is no good under `/api/v1/`, and any request carrying `Origin` gets a 403. Users and groups live in the Postgres schema (run `meada postgres migrate` first) as the provider sent them, never with `password`; a body over 1 MiB or a resource over 64 KiB gets a 413. Deleting a user removes it from every group and leaves a revocation marker until it is provisioned again with `active` true. Each write leaves one stderr line, `scim: <method> <resource> <id>: <status>`, with no values. The console reads the directory on every request: a user whose `user_claim` value finds an inactive user (or a revocation marker) gets a 403 `deprovisioned` on every route but `GET /api/v1/config`, whatever its role, and the console shows a deactivated-account page; a user never provisioned is admitted. With the directory unreadable, reads are admitted and writes are a 503 `audit_unavailable`. The workspace editor offers the directory's groups as owners to the global operator alone, `owner_missing` flags in text the entries whose owner names no group (once a group was ever provisioned), and the admission page shows the users and groups counts and the last SCIM write.

## Teams

Several teams can share one workspace: each changes its own entries and the global operator answers for the rest. The numbered rules below are the whole contract.

1. **Shared trust.** Team ownership is delegation, not isolation. An entry a team adds is fetched by the job with the fleet's credentials (a GitHub App installation or the job's git credentials) and read by every `*` grant. Accept this before delegating.
2. **The `owner` key.** An entry of a workspace version may carry `owner = "<OIDC group>"`: 1 to 256 characters, no control character, no whitespace at either end, compared exactly, case included. A group the provider sends with a control character, or longer than 256 characters, cannot own an entry.

    ```toml
    [[repository]]
    repo = "github.exemplo.internal/acme/billing"
    path = "/var/lib/meada/mirrors/billing"
    owner = "team-billing"

    [[repository]]
    repo = "github.exemplo.internal/acme/api"
    path = "/var/lib/meada/mirrors/api"
    ```

    ```sh
    meada workspace import repositories.toml --message "Assign billing to its team" --workspace /etc/meada/job.workspace.toml
    ```

    An entry may also carry `added_by = "<GitHub login>"`, the login that added it: 1 to 39 characters of lowercase ASCII letters, digits, `-` and `_` (an Enterprise Managed Users login such as `ana_acme` fits), compared exactly, so `Ana` is refused. It follows `owner` in `meada workspace show` and is part of the version id. With `[oidc.github]`, a team operator's save in the console sets the `added_by` of each addition on the GitHub grant provider's `host` to the saver's login, whatever the submitted text says (and clears it on additions off it); the refresh fetches the entry only while that login holds an accepted role, and the workspace views mark `excluded` the entry the last role check left out. An addition on the `host` without `added_by`, or by a token without a login, is refused `unattributed`, and an edit that changes, clears or sets `added_by` is refused `added_by_change`. The global operator and the CLI store `added_by` as written: setting, clearing or replacing it is how they adopt a team's addition. In a bootstrap manifest's own `[[repository]]` entries it is an error, as `owner` is.

3. **Who changes what.** The team operator may edit and remove the entries its teams own and add entries owned by its teams. The global operator keeps owner changes (set, clear or replace), entries with no owner, other owners' entries and every generation operation (`pointer` and `fence`).
4. **Delegation.** A group becomes a team when the global operator assigns it its first entry. A team that removes its last entry stops being one until the global operator assigns it an entry again.
5. **Configuration.** Ownership never widens what a token reads. Map each team's group in `[oidc.groups]` to the identities it owns, and to an identity before the team can add it; without that, the team cannot see its entries, and an addition outside the grant is refused `outside_grant`. With `[oidc.github]`, an addition on the GitHub grant provider's `host` needs no such mapping, but an identity the workspace already holds outside the grant is still refused `outside_grant` on save, which tells the team it exists.

    ```toml
    [oidc.groups]
    "meada-operators" = ["*"]
    # A team: map its group to the identities it owns.
    "team-billing" = ["github.exemplo.internal/acme/billing"]
    ```

6. **Static manifests.** `owner` in a bootstrap manifest's own `[[repository]]` entries is an error: a static manifest has no editor to delegate to. Use `[workspace] source = "store"` and import the entries.
7. **CLI.** `meada workspace import` and `apply` act as the global operator: they hold the store's and Valkey's credentials, which already allow every write.
8. **Owner-only apply.** Indexing never reads `owner`, so applying a version that only changes owners makes no new generation. The fleet view keeps naming the version that first produced the generation being served; this is not a stuck refresh.
9. **Position.** A team operator's own saves never move an entry (the stored version keeps the entries outside its grant in place). It may apply a stored version whose only difference is the order of entries.
10. **One failing repository.** Under the whole-publication abort rule, one team's failing repository (a revoked credential, a deleted remote, a parse failure) stops new generations for every team until its cause is repaired. Nodes keep serving the last complete generation. See [Refresh](refresh.md).

## CLI operations

```sh
meada snapshot pointer set <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot pointer retain <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot pointer drop <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot fence recover --workspace /etc/meada/console.workspace.toml
meada snapshot fence recover --at-least <fence> --workspace /etc/meada/console.workspace.toml
```

| Operation | Effect |
| --- | --- |
| `set` | Selects a retained generation. The next publish can replace it; suspend the job to keep the rollback. |
| `retain` | Protects a generation from trimming. The set holds up to eight generations. |
| `drop` | Removes that protection. The current generation cannot be dropped. |
| `fence recover` | Raises the counter to the highest recorded fence, or the supplied floor, under the lock. The next publish uses a higher fence. |

If an isolated node's fence appears in no valid heartbeat, pass it with `--at-least` or remove that node's `local_dir/current` so it receives the current generation. Do not edit the counter manually.
