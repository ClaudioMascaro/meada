# Meada on Kubernetes

[Português](README.md) · [Español](README.es.md)

Configure admission at the Ingress or auth proxy before exposing the service. Without
`[oidc]`, that component admits clients; a wrong configuration exposes the code graph.
With OIDC, preserve `Authorization`. Also preserve the SEP-2243 headers
`MCP-Protocol-Version`, `MCP-Session-Id`, `Mcp-Method`, `Mcp-Name` and `Mcp-Param-*`,
return `Meada-Generation`, and never add `Origin` to MCP routes.
The chart hard-codes no controller annotations: configure your Ingress's authentication.

## Installation

Releases publish the chart at `oci://ghcr.io/claudiomascaro/charts/meada`. Use matching chart
and Meada versions:

```sh
helm upgrade --install meada oci://ghcr.io/claudiomascaro/charts/meada --version <version> -f my-values.yaml
```

`values-minimal.yaml` is the smallest starting point: one node and one public repository
([Deploy](https://claudiomascaro.github.io/meada/en/deploy/)).

From a checkout, replace the OCI reference with `deploy/helm/meada`. Default values create
two nodes without cloud modules. Each pod fetches its own mirrors and indexes into its own
`emptyDir` in an init container before the server starts. Set `workspace.repositories` (TOML)
and `workspace.fetch` (a script run outside Meada). `workspace.gitCredentialsSecret` supplies
Secret environment variables to fetching. `meada index` never fetches or accesses the network.

## Generations

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-generations.yaml -f my-values.yaml
```

Configure external `https://` store and `rediss://` Valkey endpoints. Create Secrets
`meada-valkey` (`password`) and `meada-store` (`access-key-id`, `secret-access-key`, optional
`session-token`), or set their names under `modules`. Valkey's URL names its AUTH user; the
password comes only from a Secret. For a private CA, set `tls.caSecret` or `tls.caConfigMap`
with a `ca.crt` key. The chart creates neither service and keeps credentials out of manifests
and values.

Each pod performs an initial pull and runs a `pull --every 30` companion sharing only that
pod's `local_dir`. The CronJob uses `concurrencyPolicy: Forbid`, its own mirrors and index_dir,
and runs fetch, index and publish inside `meada snapshot lock`. Do not share index directories
between pods. Readiness checks local state with `meada status`, or TCP with `readiness.type=tcp`.

`modules.snapshot.credentials` accepts `static`, `web-identity` and `pod-identity`. For IRSA,
set the role annotation in `serviceAccount.annotations`; the platform webhook injects
`AWS_ROLE_ARN` and `AWS_WEB_IDENTITY_TOKEN_FILE`. For Pod Identity, associate the service
account with its role in the platform. ElastiCache IAM AUTH is unsupported.

Enable OIDC with `modules.oidc.enabled` and configure its issuer, publicUrl and provider
claims/groups. Disabled modules render no TOML section. `image.cloud=true` selects the
`-cloud` tag; an empty `image.tag` follows `appVersion`. Cloud sections require the cloud image.

## Operation and validation

The Service is ClusterIP without session affinity. Default grace is 40 seconds, including
the 5-second preStop and at most 25-second drain. Set `resources`, `nodeSelector`, `tolerations`
and `affinity` for your environment. A manifest checksum rolls pods on ConfigMap changes.

```sh
helm lint deploy/helm/meada
helm template meada deploy/helm/meada -f deploy/helm/meada/values-generations.yaml
python3 scripts/helm-check
```

`helm-check` needs Python 3.11+ and PyYAML. CI checks templates without creating a cluster.
The local smoke uses a disposable kind cluster, a loaded default image, a fixture repository
created by the init container, and `search` through a port-forward.

## Optional console and managed workspace

The console is disabled by default and emits no resources until `console.enabled=true`.
This profile enables cloud, generation modules, refresh lock, managed workspace and OIDC.
Edit `meus-valores.yaml` with endpoints, Secrets, the nodes' OIDC policy and separate
`console.publicUrl`, `clientId`, `audience`, `operations`, `operatorGroups` or `operatorClaim`.
Choose one operator selector; operations remain disabled until explicitly enabled.
The console origin/audience does not replace MCP's `modules.oidc.publicUrl`/audience.

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-console.yaml -f meus-valores.yaml
```

The console has its own Deployment, bootstrap ConfigMap, Service and Ingress, with
`console.ingress` separate from MCP ingress. Preserve `Host`, `Origin` and `Authorization`;
do not inherit a static bearer that blocks assets, `/api/v1/config` or `/auth/callback` before
login. Register `<console.publicUrl>/auth/callback` with the issuer's public client, without
a secret, using PKCE S256; discovery and the token endpoint must allow the browser with CORS
for that origin. The API requires OIDC; the browser exchanges the code directly with the issuer.
Init performs snapshot pull, and a periodic pull companion uses that console pod's exclusive
`local_dir`. Readiness checks the TCP listener. The pod's unique hostname names its heartbeat.

The writer uses `[workspace] source = "store"` and runs `workspace pull`, `workspace.fetch`,
`index` and `snapshot publish` inside one `snapshot lock`. Fetch remains the operator's script:
adding repositories or changing paths in the editor also requires updating that script,
credentials and mirrors. The console does not clone dynamically; a missing mirror prevents
publish. Every job/pod has its own directories. A mounted CA adds roots to snapshot platform
trust; Valkey uses only the roots in that file.

Seed the workspace once before first publish. Save the entries below in `repositories.toml`
(adjust them together with `workspace.fetch`). The example uses release `meada` and the current
namespace. The temporary pod reuses CronJob volumes and Secrets, replacing its command with
sleep so the operator can import/apply. Deployment never reapplies the seed:

```toml
[[repository]]
repo = "github.com/claudiomascaro/meada"
path = "/data/mirrors/meada"
```

```sh
kubectl create job meada-seed --from=cronjob/meada-meada-refresh --dry-run=client -o json | \
  python3 -c 'import json,sys; j=json.load(sys.stdin); c=j["spec"]["template"]["spec"]["containers"][0]; c["command"]=["sleep","3600"]; c.pop("args",None); print(json.dumps(j))' | \
  kubectl apply -f -
kubectl wait --for=condition=Ready pod -l job-name=meada-seed --timeout=120s
POD=$(kubectl get pod -l job-name=meada-seed -o jsonpath='{.items[0].metadata.name}')
kubectl cp repositories.toml "$POD:/tmp/repositories.toml"
VERSION=$(kubectl exec "$POD" -- meada workspace import /tmp/repositories.toml \
  --message "Initial managed workspace" --workspace /etc/meada/meada.workspace.toml)
kubectl exec "$POD" -- meada workspace apply "$VERSION" --workspace /etc/meada/meada.workspace.toml
kubectl delete job meada-seed
kubectl create job meada-first-refresh --from=cronjob/meada-meada-refresh
```

Wait for the first refresh and initial pulls to finish; then both nodes and the console should
be ready. Applying another version prepares the next refresh without publishing immediately.
For local inspection, port-forward Service `meada-meada-console` and send the configured public
Host. `scripts/helm-check` validates the OIDC profile, selectors and independent route.
The deployed smoke uses a loopback-admitted viewer; complete OIDC login is covered by HTTP/Rust
tests and is not simulated with an IdP inside the cluster.

## Postgres and enterprise identity, optional

`values-identity.yaml` (on top of `values-console.yaml`) turns on the data layer and enterprise
identity (ADR 0016): `modules.postgres`, `modules.githubApp`, `modules.oidc.github`,
`modules.directory` and, commented, `modules.oidc.type: entra` with `entra.overage: directory`.
Everything is off by default and nothing of Postgres renders. The URLs (which carry passwords), the
App's key and the SCIM token come only from Secrets: `meada-postgres` (keys `migrate-url`,
`write-url`, `read-url`), `meada-github-app` (`private-key`) and `meada-scim` (`token`, at least 32
characters); no value holds a secret.

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-console.yaml \
  -f deploy/helm/meada/values-identity.yaml -f my-values.yaml
```

With `modules.postgres.enabled`, a hook Job runs `meada postgres migrate` with the `migrate-url`
role, as `pre-install,pre-upgrade`, before the chart creates the Deployments and the CronJob; the
console and the refresh also wait in an init container (`wait-postgres`) until the schema is
migrated, while nodes start and serve without Postgres (their GitHub grant stays empty until they
read a set). Nodes read
`read-url`; the console and the refresh, `write-url`. With `modules.oidc.github.enabled`, the
CronJob uses the `job.workspace.toml` bootstrap (the only one with `[[github_app]]`) and runs
`meada workspace pull`, `meada github-app roles`, `meada workspace show --pulled`, your
`workspace.fetch` (which receives that file as `$1`), `index` and `publish`.

`postgres.enabled=true` renders an example Postgres for evaluation (one replica; Secrets
`meada-postgres-server` and `meada-postgres-tls`, with a certificate naming the Service). Since the
database does not exist before the release, the migrate hook becomes `post-install,post-upgrade`
and the console waits for the schema: `helm install --wait` would wait for the console before the
hook runs, so install without `--wait` there. In production use your own Postgres 15 or later, with TLS and the
three roles; the same `tls.caSecret` verifies its certificate (`ca_file`).

### Named workspaces

`values-workspaces.yaml` (on top of `values-console.yaml` and `values-identity.yaml`) enables
`modules.workspaces`: the `[catalog]` and `[workspaces]` sections go into the console's bootstrap and
the refresh job's `job.workspace.toml`, with the `[ui]` block the job then needs, and never into a
node's. The chart refuses it without `console.operations`, `modules.oidc.github`, a non-empty `allow`
and a `maxRepositories` you set (it has no default). Upgrade every image, the nodes included, before
the first refresh that records a workspace. Your `workspace.fetch` must clone the repositories only
named workspaces select under `modules.workspaces.mirrors`, at the paths `meada workspace show
--pulled` lists. The proposal server is not part of the chart.
