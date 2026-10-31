# Meada con Docker Compose

[Português](README.md) | [English](README.en.md)

Ejemplo local con dos nodos, Caddy con TLS y admisión por bearer token. Requiere Docker
Compose v2 e imágenes de la misma versión publicada. Solo se publica `127.0.0.1:8443`.
Los servicios no usan OIDC y la consola queda desactivada. Para acceso remoto, configure DNS,
certificado y reglas de admisión propios en Caddy.

```sh
cd deploy/compose
cp .env.example .env
# Edite .env: versión publicada, token aleatorio y, para generations, credenciales propias.
docker compose config -q
docker compose up -d
docker compose logs -f local-indexer-one local-indexer-two
```

El perfil predeterminado, `local`, usa la imagen default y el layout A: cada indexer obtiene su
espejo e indexa en volúmenes propios; su servidor empieza solo cuando ese job termina bien.
Ambos servidores montan manifiesto, espejos e índices en modo de solo lectura. Pueden servir
commits distintos. Para actualizar, ejecute `docker compose run --rm local-indexer-one` y
después `docker compose run --rm local-indexer-two`; los servidores detectan los índices
nuevos. No ejecute dos indexers sobre el mismo volumen a la vez.

`REPOSITORIES` contiene URLs Git HTTP(S), separadas por espacios. Al cambiar repositorios,
ajuste también los bloques `[[repository]]` de los manifiestos: identidad sin `.git` y ruta
bajo `/srv/mirrors`. El fetch pertenece al job; `meada index` sigue sin usar la red.
Las URLs no llevan credenciales (use un credential helper de git). Un espejo clonado antes
desde una URL con token lo guarda en `origin` y se rechaza igual (#445): ejecute
`git remote set-url origin <URL sin credenciales>` en él. Un fetch que falla nombra la
identidad y termina el refresh antes de `index` y `publish`, conservando la generación actual
(#316).
Los datos viven en volúmenes Docker; `docker compose down` los conserva y `down -v` los borra.

## TLS, admisión y herramientas

Espere a que los servidores estén `healthy` en `docker compose ps`. Copie la CA local de Caddy:

```sh
docker compose cp proxy:/data/caddy/pki/authorities/local/root.crt ./ca.crt
curl --cacert ca.crt -i https://localhost:8443/github.com/claudiomascaro/meada
# Esperado: HTTP 401 sin token.
printf 'Bearer token: '; read -r -s MEADA_TOKEN; printf '\n'; export MEADA_TOKEN
curl --cacert ca.crt -i https://localhost:8443/github.com/claudiomascaro/meada \
  -H "Authorization: Bearer $MEADA_TOKEN" -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' -H 'MCP-Protocol-Version: 2025-03-26' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search","arguments":{"query":"main"}}}'
```

Esperado: HTTP 200 con un resultado de `search`. Las llamadas repetidas alternan nodos por
round robin. Con una identidad de símbolo del resultado, llame `trace` con
`{"from":"<identidad>","direction":"down"}`; espere una cadena de llamadas y su pie de
certeza. Use una función que llame a otra para ver saltos. Caddy conserva `Authorization`,
los encabezados MCP y `Meada-Generation`; no crea `Origin`. No use `curl -k` ni registre el
token en trazas del shell.

## Generaciones compartidas

Este perfil usa la imagen cloud y el layout B. Un job periódico obtiene, indexa y publica bajo
`snapshot lock`. Valkey guarda pointer, retención y fence; RustFS guarda bundles y documentos.
Ambos aceptan solo TLS en la red interna, sin puertos publicados. El job de certificados crea
una CA privada, descarta su clave al terminar y monta en cada servicio solo su propia clave.
Los clientes verifican la CA mediante `ca_file`. Las contraseñas vienen de `.env`, nunca del
manifiesto.

```sh
docker compose down
docker compose --profile generations config -q
docker compose --profile generations up -d
docker compose --profile generations logs -f generation-indexer first-pull-one first-pull-two
docker compose --profile generations ps
```

Seleccione un perfil por vez; `--profile generations` sustituye `COMPOSE_PROFILES=local` de
`.env`. El primer pull de cada nodo debe terminar antes de iniciar el servidor; después un
compañero ejecuta `snapshot pull --every 30` sobre el volumen exclusivo de ese nodo. El servidor
lo monta en modo de solo lectura. El healthcheck ejecuta `status --generations`.
`REFRESH_SECONDS` controla el intervalo del job, 300 por defecto; la traba renueva su TTL
durante el refresh y recibe SIGTERM al terminar. Los fallos aparecen en el log y se vuelven
a intentar en el ciclo siguiente.

Para verificar convergencia, repita el `search` anterior y anote `Meada-Generation` como `G1`:
las dos respuestas alternadas deben nombrar la misma generación. Publique un commit nuevo
en el repositorio remoto configurado, espere el refresh y hasta 30 s de pull y repita: ambos
deben pasar a `G2`. Añada `@G1` a la misma ruta (`/.../meada@G1`): debe seguir respondiendo con
`Meada-Generation: G1` mientras G1 esté entre las tres generaciones retenidas. Un `trace`
fijado en esa URL debe mantener el mismo grafo tras la actualización. Una generación fuera de
retención devuelve 404 con la actual y las conservadas. El ejemplo no publica artefactos:
la publicación de imágenes sigue vinculada al workflow de release.

## Consola de gestión opcional

El overlay de la consola usa la imagen cloud y requiere el perfil `generations`. Sin el overlay,
no se inicia ningún servicio, puerto ni volumen de la consola. Este ejemplo admite un viewer
local por loopback, con operaciones desactivadas. Configure OIDC o admisión propia en el proxy
antes del acceso remoto; una consola accesible fuera de esa admisión expone el grafo.

Ejecute desde el directorio del ejemplo con `.env` configurado. Edite `console.repositories.toml`
para el seed inicial, `REPOSITORIES` para fetch, y `job.workspace.toml` y
`console.workspace.toml` con el mismo store, Valkey, prefijos y CA de los nodos. El seed es una
acción explícita del operador, realizada una vez; el despliegue nunca reaplica versiones:

```sh
compose_console() {
  docker compose -f compose.yaml -f compose.console.yaml --profile generations --profile console "$@"
}
compose_console config -q
compose_console up -d bucket valkey
VERSION=$(compose_console run --rm --entrypoint meada generation-indexer workspace import \
  /etc/meada/repositories.toml --message "Initial managed workspace" --workspace /etc/meada/meada.workspace.toml)
compose_console run --rm --entrypoint meada generation-indexer workspace apply "$VERSION" \
  --workspace /etc/meada/meada.workspace.toml
compose_console up -d
compose_console logs -f generation-indexer console-first-pull console-pull console
```

El writer usa `[workspace] source = "store"`. El loop mantiene **una** `snapshot lock` durante
`workspace pull`, fetch externo, `index` y `snapshot publish`. Fetch sigue usando
`REPOSITORIES`, sin derivar URLs ni rutas arbitrarias del editor. Al añadir un repositorio o
cambiar su ruta, ajuste esa lista, las credenciales y el mirror antes del refresh;
un mirror ausente impide publish sin avanzar el pointer. Aplicar una versión prepara el
próximo refresh; llega a la generación tras publish y pull. Detenga el writer para mantener un
rollback de generación después del próximo refresh.

La consola tiene bootstrap, `index_dir`, `local_dir`, volumen y `node_name = "console"` propios.
El pull inicial termina antes del arranque; un companion `snapshot pull --every 30` actualiza
solo ese volumen y escribe su heartbeat. Los dos nodos mantienen volúmenes e identidades separados.
`snapshot.ca_file` añade raíces a las del sistema; `valkey.ca_file` usa solo las del archivo.

La ruta TLS propia `https://localhost:8444` no usa el bearer estático del puerto MCP 8443.
Caddy conserva `Host`, `Origin` y `Authorization`. Assets, `/api/v1/config` y `/auth/callback`
deben llegar al backend antes del login PKCE:

```sh
compose_console cp console-proxy:/data/caddy/pki/authorities/local/root.crt ./console-ca.crt
curl --cacert console-ca.crt https://localhost:8444/api/v1/config
curl --cacert console-ca.crt https://localhost:8444/auth/callback
```

Para OIDC, descomente y ajuste la política en `console.workspace.toml`: issuer, claims, grupos
y scopes iguales a los nodos; `[oidc] public_url`/audience describen MCP. `[ui] public_url`,
`client_id` y `audience` son propios de la consola. Registre un cliente público sin secreto,
Authorization Code con PKCE S256 y el redirect URI exacto `https://localhost:8444/auth/callback`.
El navegador debe acceder al discovery y token endpoint del issuer con CORS para ese origen;
intercambia el código directamente con el issuer. La API protegida exige su bearer.
Para operaciones, active `operations = true` y elija `operator_groups` u `operator_claim`,
solo una opción; los grupos requieren `oidc.groups_claim`. El claim debe ser booleano true.

El smoke local usa un viewer por loopback, sin simular un IdP. OIDC y sus rechazos están
cubiertos por pruebas HTTP/Rust; el perfil OIDC del chart pasa lint/template. Valide con
`python3 ../../scripts/compose-check` desde este directorio. Detenga con `compose_console down`;
`down -v` también borra los datos y requiere un nuevo seed.

## Postgres e identidad empresarial, opcional

`compose.postgres.yaml` suma al overlay de la consola un Postgres 15 con TLS y el paso único
`meada postgres migrate` (ADR 0016). Sin ese archivo no arranca ningún Postgres. Define en `.env`
`POSTGRES_PASSWORD` y las contraseñas de los tres roles (`POSTGRES_MIGRATE_PASSWORD`,
`POSTGRES_WRITE_PASSWORD`, `POSTGRES_READ_PASSWORD`; solo letras y dígitos, porque van en URLs).
El job `certificates` emite también el certificado de Postgres (la CA se recrea una vez si el
volumen `postgres-tls` aún no existe: reinicia los servicios después). Los roles se crean en el
primer arranque del volumen con `postgres-init.sh`.

```sh
compose_postgres() {
  docker compose -f compose.yaml -f compose.console.yaml -f compose.postgres.yaml \
    --profile generations --profile console "$@"
}
compose_postgres config -q
compose_postgres up -d postgres postgres-migrate
compose_postgres logs postgres-migrate   # espera a que el paso termine con exit 0
compose_postgres up -d
```

`postgres-migrate` ejecuta `meada postgres migrate` con el rol `meada_migrate` y termina antes del
job (`generation-indexer`), la consola y los nodos, que dependen de él. Cada uno lee su URL de una
variable (`MEADA_POSTGRES_URL` con el rol de escritura en el job y la consola,
`MEADA_POSTGRES_READ_URL` con el de lectura en los nodos), nunca del manifiesto. Nada conecta
hasta que descomentes `[postgres]` y las secciones que dependen de él en los bootstraps:
`[oidc.github]` y `[[github_app]]` en `job.workspace.toml`, `[oidc.github]`, `[directory]` y
`[oidc.entra]` en `console.workspace.toml`, `[postgres]` y `[oidc.github]` en
`generations.workspace.toml`. Con `[oidc.github]` en el bootstrap del job, `managed-refresh.sh`
ejecuta `meada github-app roles` y busca los espejos que lista `meada workspace show --pulled`
(`fetch-pulled.sh`); sin la sección el fetch sigue siendo el de `REPOSITORIES`. Para `[directory]`,
exporta `MEADA_SCIM_TOKEN` (al menos 32 caracteres). Las recetas de Entra ID, Okta y Keycloak
están en la documentación del sitio.

### Workspaces nombrados

La spec S5 (workspaces nombrados sobre indexación compartida) es opt-in sobre las secciones de
identidad anteriores: `[catalog]` y `[workspaces]` (comentadas en `job.workspace.toml` y
`console.workspace.toml`, con el bloque `[ui]` que el job también necesita, porque todo proceso que
lee `[workspaces]` lee `[ui]` con `operations = true`). Actualiza todas las imágenes, nodos
incluidos, antes del primer refresh que registre un workspace: un nodo antiguo rechaza esa
generación y conserva la última. `mirrors = "/srv/mirrors"` es el volumen de espejos del job, donde
`fetch-pulled.sh` clona cada repositorio que solo seleccionan workspaces nombrados. El servidor de
propuestas (`meada workspaces mcp`) no forma parte de este ejemplo. El procedimiento y sus modos de
fallo están en la página de workspaces nombrados del sitio.
