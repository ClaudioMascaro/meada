# Desplegar

Tres formas de poner Meada en marcha, de la máquina de quien desarrolla al clúster. El CI del
repositorio comprueba cada ejemplo, con el script indicado en su sección.

## Docker, sin servidor

En la raíz del repositorio:

```sh
docker run --rm --network none --user "$(id -u):$(id -g)" -v "${PWD%/*}:${PWD%/*}" -w "$PWD" ghcr.io/claudiomascaro/meada:1.0.0 index
docker run --rm --network none --user "$(id -u):$(id -g)" -v "${PWD%/*}:${PWD%/*}" -w "$PWD" ghcr.io/claudiomascaro/meada:1.0.0 status
```

- `--user` escribe `.meada/` con tu usuario, no con el de la imagen (uid 10001).
- `-v` monta la carpeta que contiene el repositorio en la misma ruta: los pares vecinos entran
  en el workspace, y la ruta que un cliente MCP envía como raíz existe dentro del contenedor.
  Para un repositorio sin pares, basta `-v "$PWD:$PWD"`.
- `--network none`: indexar y consultar no usan red.

El servidor MCP para tu agente es la misma línea con `-i` y `mcp`, en la pestaña Docker de
[Agentes y MCP](mcp.md). `scripts/container-smoke` ejecuta esa línea con un cliente MCP que envía
su raíz, en cada cambio de la imagen.

## Docker Compose

`deploy/compose` levanta dos nodos detrás de un Caddy con TLS y admisión por bearer token, y solo
publica `127.0.0.1:8443`. El perfil predeterminado, `local`, busca e indexa los repositorios de
`REPOSITORIES` en volúmenes propios; los perfiles de generaciones y de la consola son opcionales.

```sh
git clone --depth 1 --branch v1.0.0 https://github.com/ClaudioMascaro/meada.git
cd meada/deploy/compose
cp .env.example .env    # versión, un token aleatorio y, si quieres, REPOSITORIES
docker compose up -d
docker compose ps       # espera a que los servidores estén healthy
```

La prueba con `curl` por TLS, la actualización de los índices y los otros perfiles están en el
[README de Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/README.es.md).
`scripts/compose-check` valida todos los perfiles con `docker compose config`.

## Helm

El chart sale con cada release en `oci://ghcr.io/claudiomascaro/charts/meada`, en la misma
versión de Meada. `values-minimal.yaml` levanta un nodo, sin módulos cloud, que busca e indexa un
repositorio público en el init container:

```sh
curl -fsSLO https://raw.githubusercontent.com/ClaudioMascaro/meada/v1.0.0/deploy/helm/meada/values-minimal.yaml
helm install meada oci://ghcr.io/claudiomascaro/charts/meada --version 1.0.0 -f values-minimal.yaml
kubectl rollout status deployment/meada-meada
kubectl port-forward service/meada-meada 7333:7333
```

En otra terminal, una búsqueda:

```sh
curl -s http://127.0.0.1:7333/github.com/claudiomascaro/meada \
  -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-06-18' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search","arguments":{"query":"main"}}}'
```

Cambia los dos bloques de `values-minimal.yaml` por tus repositorios. Sin OIDC el servidor admite
a quien alcanza el Service: configura la admisión ([Autenticación y grants](admission.md)) antes
de activar el Ingress. Generaciones, consola e identidad empresarial están en el
[README del chart](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/README.es.md).
`scripts/helm-check` pasa `helm lint` y comprueba lo que el chart renderiza con cada archivo de
valores, este incluido.
