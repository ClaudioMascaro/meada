# Meada on Kubernetes

[Português](README.md) · [Español](README.es.md)

Configure admission at the Ingress or auth proxy before exposing the service. Without
`[oidc]`, that component admits clients; a wrong configuration exposes the code graph.
With OIDC, preserve `Authorization`. Also preserve the SEP-2243 headers
`MCP-Protocol-Version`, `MCP-Session-Id`, `Mcp-Method`, `Mcp-Name` and `Mcp-Param-*`,
return `Meada-Generation`, and never add `Origin`.
The chart hard-codes no controller annotations: configure your Ingress's authentication.

## Installation

Releases publish the chart at `oci://ghcr.io/claudiomascaro/charts/meada`. Use matching chart
and Meada versions:

```sh
helm upgrade --install meada oci://ghcr.io/claudiomascaro/charts/meada --version <version> -f my-values.yaml
```

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
