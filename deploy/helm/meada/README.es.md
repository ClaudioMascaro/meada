# Meada en Kubernetes

[Português](README.md) · [English](README.en.md)

Configure la admisión en el Ingress o proxy de autenticación antes de exponer el servicio.
Sin `[oidc]`, ese componente admite clientes; una configuración incorrecta expone el grafo
de código. Con OIDC, preserve `Authorization`. Preserve también los headers SEP-2243
`MCP-Protocol-Version`, `MCP-Session-Id`, `Mcp-Method`, `Mcp-Name` y `Mcp-Param-*`,
devuelva `Meada-Generation` y nunca añada `Origin` a las rutas MCP.
El chart no fija annotations de un controller: configure la autenticación de su Ingress.

## Instalación

Las releases publican el chart en `oci://ghcr.io/claudiomascaro/charts/meada`. Use la misma
versión del chart y de Meada:

```sh
helm upgrade --install meada oci://ghcr.io/claudiomascaro/charts/meada --version <versión> -f mis-valores.yaml
```

Desde el checkout, sustituya la referencia OCI por `deploy/helm/meada`. Los valores por defecto
crean dos nodos sin módulos cloud. Cada pod obtiene sus propios mirrors e indexa en su propio
`emptyDir` mediante un init container antes de iniciar el servidor. Ajuste
`workspace.repositories` (TOML) y `workspace.fetch` (script ejecutado fuera de Meada).
`workspace.gitCredentialsSecret` aporta variables de un Secret al fetch. `meada index` nunca
hace fetch ni accede a la red. El `workspace.fetch` por defecto lee los repositorios de `$1` si se
le da (el archivo de `meada workspace show --pulled`, que el refresh pasa con
`modules.oidc.github`), si no del manifiesto de bootstrap; rechaza un repositorio con credencial en
la URL; y, si un fetch falla, imprime `refresh: fetch failed for <identidad>; the refresh stops
before publication` (sin URL ni token) y sale con error, antes de `index` y `publish`.

## Generaciones

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-generations.yaml -f mis-valores.yaml
```

Configure endpoints externos `https://` para el store y `rediss://` para Valkey. Cree los
Secrets `meada-valkey` (`password`) y `meada-store` (`access-key-id`, `secret-access-key` y,
opcionalmente, `session-token`), o configure sus nombres en `modules`. La URL de Valkey nombra
el usuario AUTH; la contraseña viene solamente del Secret. Para una CA privada, configure
`tls.caSecret` o `tls.caConfigMap`, con la clave `ca.crt`. El chart no crea esos servicios ni
guarda credenciales en manifests o valores.

Cada pod hace un pull inicial y mantiene un companion `pull --every 30` que comparte solamente
el `local_dir` de ese pod. El CronJob usa `concurrencyPolicy: Forbid`, sus propios mirrors e
index_dir, y ejecuta fetch, index y publish dentro de `meada snapshot lock`. No comparta índices
entre pods. La readiness consulta el estado local con `meada status`, o TCP con
`readiness.type=tcp`.

`modules.snapshot.credentials` acepta `static`, `web-identity` y `pod-identity`. Para IRSA,
configure la annotation de la role en `serviceAccount.annotations`; el webhook de la plataforma
inyecta `AWS_ROLE_ARN` y `AWS_WEB_IDENTITY_TOKEN_FILE`. Para Pod Identity, asocie la service
account a su role en la plataforma. No hay soporte para IAM AUTH de ElastiCache.

Habilite OIDC en `modules.oidc.enabled`, configurando issuer, publicUrl y claims/grupos del
proveedor. Los módulos deshabilitados no generan secciones TOML. `image.cloud=true` selecciona
la tag `-cloud`; `image.tag` vacío sigue `appVersion`. Una sección cloud necesita la imagen cloud.

## Operación y validación

El Service es ClusterIP sin afinidad de sesión. La gracia por defecto es 40 s, incluyendo el
preStop de 5 s y el drain de hasta 25 s. Ajuste `resources`, `nodeSelector`, `tolerations` y
`affinity`. El checksum del manifest hace rollout al cambiar el ConfigMap.

```sh
helm lint deploy/helm/meada
helm template meada deploy/helm/meada -f deploy/helm/meada/values-generations.yaml
python3 scripts/helm-check
```

`helm-check` necesita Python 3.11+ y PyYAML. CI valida los templates sin crear un cluster.
El smoke local usa un cluster kind descartable, la imagen default cargada, un repositorio de
prueba creado por el init container y `search` por port-forward.

## Consola opcional y workspace gestionado

La consola está desactivada por defecto y no genera recursos hasta `console.enabled=true`.
Este perfil activa cloud, módulos de generaciones, refresh lock, workspace gestionado y OIDC.
Edite `meus-valores.yaml` con endpoints, Secrets, política OIDC de los nodos y valores propios
`console.publicUrl`, `clientId`, `audience`, `operations`, `operatorGroups` u `operatorClaim`.
Elija un selector de operador; las operaciones siguen desactivadas hasta activarlas explícitamente.
El origen/audience de la consola no sustituye `modules.oidc.publicUrl`/audience de MCP.

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-console.yaml -f meus-valores.yaml
```

La consola tiene Deployment, ConfigMap de bootstrap, Service e Ingress propios, con
`console.ingress` separado del Ingress MCP. Conserve `Host`, `Origin` y `Authorization`;
no herede un bearer estático que bloquee assets, `/api/v1/config` o `/auth/callback` antes del
login. Registre `<console.publicUrl>/auth/callback` en el cliente público del issuer, sin secreto,
con PKCE S256; discovery y token endpoint deben permitir el navegador con CORS para ese origen.
La API exige OIDC; el navegador intercambia el código directamente con el issuer.
El init hace snapshot pull y un companion periódico usa el `local_dir` exclusivo de ese pod.
La readiness comprueba el listener TCP. El hostname único del pod identifica su heartbeat.

El writer usa `[workspace] source = "store"` y ejecuta `workspace pull`, `workspace.fetch`,
`index` y `snapshot publish` dentro de una única `snapshot lock`. Fetch sigue siendo el script
del operador: al añadir repos o cambiar paths en el editor, actualice también ese script,
credenciales y mirrors. La consola no clona dinámicamente; un mirror ausente impide publish.
Cada job/pod tiene directorios propios. La CA montada añade raíces al trust del snapshot;
Valkey usa solo las raíces de ese archivo.

Inicialice el workspace una vez antes del primer publish. Guarde las entradas siguientes en
`repositories.toml` (ajústelas junto con `workspace.fetch`). El ejemplo usa release `meada` y el
namespace actual. El pod temporal reutiliza volúmenes y Secrets del CronJob, sustituyendo su
comando por sleep para que el operador haga import/apply. El deploy nunca reaplica el seed:

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

Espere el primer refresh y los pulls iniciales; los dos nodos y la consola deben quedar listos.
Aplicar otra versión prepara el próximo refresh, sin publicar inmediatamente. Para inspección
local, haga port-forward del Service `meada-meada-console` y envíe el Host público configurado.
`scripts/helm-check` valida el perfil OIDC, sus selectores y la ruta independiente.
El smoke desplegado usa un viewer admitido por loopback; el login OIDC completo está cubierto
por pruebas HTTP/Rust y no se simula con un IdP dentro del cluster.

## Postgres e identidad empresarial, opcional

`values-identity.yaml` (sobre `values-console.yaml`) activa la capa de datos y la identidad
empresarial (ADR 0016): `modules.postgres`, `modules.githubApp`, `modules.oidc.github`,
`modules.directory` y, comentado, `modules.oidc.type: entra` con `entra.overage: directory`. Todo
viene desactivado por defecto y no se renderiza nada de Postgres. Las URL (que llevan contraseñas),
la clave de la App y el token SCIM salen solo de Secrets: `meada-postgres` (claves `migrate-url`,
`write-url`, `read-url`), `meada-github-app` (`private-key`) y `meada-scim` (`token`, al menos 32
caracteres); ningún valor guarda secretos.

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-console.yaml \
  -f deploy/helm/meada/values-identity.yaml -f mis-valores.yaml
```

Con `modules.postgres.enabled`, un Job de hook ejecuta `meada postgres migrate` con el rol de
`migrate-url`, en `pre-install,pre-upgrade`, antes de que el chart cree los Deployments y el
CronJob; la consola y el refresh esperan además en un init container (`wait-postgres`) hasta que
el schema esté migrado, mientras los nodos arrancan y sirven sin Postgres (su grant de GitHub queda
vacío hasta que leen un conjunto). Los nodos leen `read-url`; la consola y el refresh, `write-url`. Con
`modules.oidc.github.enabled`, el CronJob usa el bootstrap `job.workspace.toml` (el único con
`[[github_app]]`) y ejecuta `meada workspace pull`, `meada github-app roles`,
`meada workspace show --pulled`, tu `workspace.fetch` (que recibe ese archivo en `$1`), `index` y
`publish`.

`postgres.enabled=true` renderiza un Postgres de ejemplo, para evaluación (una réplica; Secrets
`meada-postgres-server` y `meada-postgres-tls`, con un certificado que nombre el Service). Como la
base no existe antes del release, el hook de migración pasa a `post-install,post-upgrade` y la
consola espera el schema: `helm install --wait` esperaría a la consola antes de que corra el hook,
así que instala sin `--wait` en ese caso. En producción usa tu propio Postgres 15 o posterior, con TLS y los tres
roles; la misma `tls.caSecret` verifica su certificado (`ca_file`).

### Workspaces nombrados

`values-workspaces.yaml` (sobre `values-console.yaml` y `values-identity.yaml`) habilita
`modules.workspaces`: las secciones `[catalog]` y `[workspaces]` van al bootstrap de la consola y al
`job.workspace.toml` del job de refresh, con el bloque `[ui]` que el job pasa a necesitar, y nunca al
de un nodo. El chart lo rechaza sin `console.operations`, `modules.oidc.github`, un `allow` no vacío
y un `maxRepositories` definido por ti (no tiene valor por defecto). Actualiza todas las imágenes,
nodos incluidos, antes del primer refresh que registre un workspace. Tu `workspace.fetch` debe
clonar los repositorios que solo seleccionan workspaces nombrados bajo `modules.workspaces.mirrors`,
en las rutas que lista `meada workspace show --pulled`. El servidor de propuestas no forma parte del
chart.
