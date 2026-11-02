# Deployment

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

## Deployment profiles

`deploy/compose/compose.yaml` supplies local and generations nodes; the explicit
`compose.console.yaml` overlay adds the cloud console, its initial pull and companion, its own
bootstrap/data volume and its own TLS proxy route. `deploy/helm/meada/` follows the same split:
`console.enabled` is false by default, with a separate ConfigMap, Deployment, Service and
optional Ingress when enabled. Neither profile mounts a node's data into the console. The
console's public assets, config and PKCE callback reach its listener before OIDC admission;
its proxy preserves Host, Origin and Authorization. MCP keeps its separate admission rules.

`compose.accounts.yaml` (with `.env.accounts.example`, whose `COMPOSE_FILE` and `COMPOSE_PROFILES`
select the whole recipe) is the local accounts proof of concept (ADR 0024, #564): the
`certificates` step also makes the Ed25519 pair (`accounts-private` mounted in the console alone,
`accounts-public` in the nodes and the console), `console.accounts.workspace.toml` and
`generations.accounts.workspace.toml` carry `[accounts]` (the nodes the public half and the
console's `[ui] public_url` as issuer), `seed.sh` applies the first workspace version only while
the store has none, and `Caddyfile.accounts` forwards `Authorization` without the static bearer.
`scripts/compose-check` renders it and checks the order, the mounts and the manifests.

`compose.proposals.yaml` (over the console and Postgres overlays) and the chart's `proposals` block
(off by default) run the proposal server (`meada workspaces mcp`, ADR 0022, #504) as a process of
its own beside the console: its own pull and volume (`node_name = "proposals"`), the read role
(`MEADA_POSTGRES_READ_URL` in Compose, the Secret's `read-url` in the chart), a Caddy on
`127.0.0.1:8445` in Compose and a Service in the chart (its limits are in Known ceilings). The chart
repeats the console's `[oidc]`, `[catalog]`, `[workspaces]` and `[ui]` and adds `[proposals]` (also
to the console's, which issues the agent tokens under local accounts). `compose-check` and
`helm-check` assert both, and that the block is absent when off.

Managed writers pull the workspace before the external fetch, index and publish, inside one
refresh lock. A failed stage ends the job before publish, so the pointer stays (#316, ADR
0012): the compose `fetch.sh` and `fetch-pulled.sh` and the chart's default `workspace.fetch`
refuse URLs carrying credentials, and an existing mirror whose `origin` carries them (#445),
and name the identity whose fetch failed (#444), and `meada index --workspace` ends a failure
with the stage and the count. The fetch reads each `[[repository]]` table by its own keys, one
awk program that `fetch-pulled.sh` and `workspace.fetch` carry and a test compares (#544).
Without `$1` (the pulled list under `[oidc.github]`) the chart's fetch reads `workspace.repositories`, mounted as `repositories.toml`, since a store-sourced
bootstrap has no `[[repository]]`.
Seeding and applying the first version are explicit operator actions. The fetch list/script and
mirror provisioning still belong to the operator: applying a new identity or path does not
clone it. `scripts/compose-check` and `scripts/helm-check` check the rendered
configuration, module dependencies, storage, selectors and refresh order.

## Deployment (`deploy/`)

- `deploy/helm/meada/` renders a module-free, two-node Deployment by default. Its init
  container fetches mirrors outside Meada and indexes its own `emptyDir` before MCP starts.
  `values-generations.yaml` enables the cloud image, an initial pull and pull companion per
  pod, plus a `Forbid` refresh CronJob with separate mirrors and indexes under the refresh lock.
- The chart's ConfigMap owns the workspace manifest. Disabled modules emit no TOML section;
  external store and Valkey endpoints require TLS, credentials come from Secrets or platform
  identity, and the optional Ingress leaves admission and header forwarding to the controller.
- Local accounts (#565): `modules.accounts` (`values-accounts.yaml`, `_helpers.tpl`'s
  `meada.accounts` per process) renders `[accounts]` with the key Secrets as file mounts: the private
  key only in the console container, the public key in the console and the nodes, none in the
  refresh job (with `modules.githubApp` it gets `job.workspace.toml` with only `[accounts.github]` for
  the role check, else the nodes' bootstrap, which names the public key's file and mounts none); under break glass only the
  console reads `[accounts]`. A node's `[ui]` carries `console.publicUrl`, its tokens' issuer.
- `scripts/helm-check` checks public renders with Helm and parses the resulting TOML. The
  Helm workflow runs it without a cluster; a local kind smoke checks MCP search after install.
- The job's refresh under `[oidc.github]` (#331), under `meada snapshot lock`: `meada workspace
  pull`, `meada github-app roles`, the fetch of the mirrors `meada workspace show --pulled`
  lists, `meada index --workspace` and `meada snapshot publish`; one that stops on the role
  check's exit publishes nothing, and its exclusion of an addition is a decision before the
  fetch, not a failed fetch (#316). The compose and Helm profiles gain the step, Postgres and
  `migrate` with the documentation and profiles slice of #319.
