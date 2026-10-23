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
