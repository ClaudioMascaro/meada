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

3. **Who changes what.** The team operator may edit and remove the entries its teams own and add entries owned by its teams. The global operator keeps owner changes (set, clear or replace), entries with no owner, other owners' entries and every generation operation (`pointer` and `fence`).
4. **Delegation.** A group becomes a team when the global operator assigns it its first entry. A team that removes its last entry stops being one until the global operator assigns it an entry again.
5. **Configuration.** Ownership never widens what a token reads. Map each team's group in `[oidc.groups]` to the identities it owns, and to an identity before the team can add it; without that, the team cannot see its entries, and an addition outside the grant is refused `outside_grant`.

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
