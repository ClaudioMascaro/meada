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
