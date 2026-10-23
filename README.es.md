# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · **Español**

> Esta es una traducción. El [README](README.md) en portugués de Brasil es la fuente; si no
> coinciden, vale el original, y cualquier corrección a esta página es bienvenida.

Meada lee tus repositorios, arma el mapa de quién llama a quién y cose un repositorio con otro
a través de los contratos que comparten: rutas HTTP, colas, paquetes, recursos compartidos. Los
agentes de código, y también las personas, consultan ese mapa por MCP. Ninguna etapa de la
indexación usa un LLM.

## Instalación

Elige una de cinco formas.

- **Cargo**: `cargo install meada`.
- **Binario listo**: descarga el archivo de tu sistema desde los
  [releases de GitHub](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS y
  Windows, en x86_64 y arm64) y pon `meada` en el `PATH`.
- **Instalador** (Linux y macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.
- **npm**: `npx meada <comando>` descarga el binario listo de tu plataforma y lo ejecuta, sin
  instalar nada antes.
- **Contenedor**: `ghcr.io/claudiomascaro/meada:<versión>` para el build predeterminado, o
  `<versión>-cloud` con todos los módulos cloud, ambos para Linux amd64 y arm64. `latest` y
  `latest-cloud` siguen la última release estable; las prereleases solo tienen tags de versión.
  La base Debian bookworm-slim (git y certificados CA) solo se actualiza con una release de Meada.
  La imagen se ejecuta sin root y no define comando: pasa los argumentos explícitamente, por
  ejemplo `docker run --rm ghcr.io/claudiomascaro/meada:latest --version`. Para servir,
  monta el manifiesto, los espejos y los índices, y pasa
  `mcp --listen 0.0.0.0:7333 --workspace /etc/meada/meada.workspace.toml`, siguiendo
  [Modo servidor](#modo-servidor) y su configuración de admisión.

Para la consola, ejecuta `cargo install meada --features ui`, descarga
`meada-ui-<versión>.tar.gz` de la misma release y descomprímelo. Apunta `[ui] dir` en el
manifiesto al directorio `meada-ui-<versión>/` extraído. La consola rechaza iniciar con una UI
de otra versión e indica ambas versiones. La imagen `-cloud` ya incluye estos archivos en
`/usr/share/meada/ui`; la imagen predeterminada y los binarios listos siguen sin la UI.
Node solo es necesario para compilar el frontend desde los fuentes, nunca para instalar
mediante Cargo o ejecutar la consola.

## Configuración

Cada repositorio puede tener un `meada.toml` en la raíz, versionado en git, que dice con quién
conversa. Quien consume una ruta nombra al par:

```toml
[[peer]]
repo = "example.com/acme/api"            # remoto git de origin, sin esquema ni .git
base_url_env = ["API_URL"]               # variable de la que el cliente lee la URL base
# path = "../api"                        # opcional: pista de dónde está el clon
```

Quien sirve la ruta declara los hosts en los que responde:

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"                         # opcional: lo que el gateway pone delante de las rutas
strip = "/v1"                            # opcional: lo que les quita del comienzo
```

Con `prefix` y `strip`, una llamada a `/shop/orders` llega a la ruta `/v1/orders` del código, en
el nivel `inferred` y con el motivo `gateway prefix /shop, gateway strip /v1`. Un segmento de
versión solo se quita cuando lo escribes en `strip`, y quien llama a la ruta del código sigue
enlazando como antes.

Un recurso compartido que ningún código nombra, como una tabla o un bucket, entra como
`[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # identificador en minúsculas
id = "orders"
role = "write"                           # read, write o read-write
via = ["src/orders.ts::cancelOrder"]     # opcional: los símbolos que hacen el acceso
```

Cada escritor enlaza con cada lector del mismo recurso en otro repositorio, en el nivel
`declared`. Sin `via`, o si ningún `via` corresponde a un símbolo, el lado es el repositorio
entero.

Meada encuentra los pares por su remoto git, aunque el clon esté en otra ruta en cada máquina.
Una clave desconocida o mal escrita da un error con el archivo, la línea y la clave. Sin
`meada.toml`, el repositorio no declara ningún par.

## Uso

En la raíz del repositorio:

- `meada index` arma el índice, en `.meada/`, del repositorio y de los pares;
- `meada status` muestra el conjunto de repositorios, el estado de cada índice y los pares que
  no se encontraron; bajo cada repositorio, los recursos sin enlace y los `via` que no
  corresponden a ningún símbolo;
- `meada export` imprime todo el índice como JSON ordenado, útil para comparar dos ejecuciones
  con `diff`;
- `meada decide` resuelve enlaces ambiguos y URLs dinámicas, ver Decisiones;
- `meada mcp` sirve las herramientas de abajo por MCP.

Meada solo escribe dentro de `.meada/`, que se ignora a sí mismo en git. Toda consulta primero
actualiza el índice con lo que cambió en los archivos, incluso lo que aún no se hizo commit. Los
mensajes de la línea de comandos siguen `LANG` (portugués o inglés); los del MCP son siempre en
inglés.

## Agentes (MCP)

`meada mcp` habla MCP por stdio. Responde sobre la primera raíz (`roots`) que el cliente envía,
en Claude Code la carpeta de la sesión; sin raíces, sobre la carpeta en la que se inició, que
entonces debe estar dentro del repositorio. Si el cliente falla o no responde a `roots/list` en 5 s,
la llamada usa la última raíz que envió en esa conexión, o devuelve un error que lo dice; nunca
recurre a la carpeta del servidor por eso (que, en una sesión en un worktree, sería otro
checkout). Una respuesta sin raíz local vale como sin raíces (la carpeta del servidor). En la
configuración de tu agente (por ejemplo el `.mcp.json` de Claude Code), con `npx`:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["meada", "mcp"] }
  }
}
```

Con `meada` ya en el `PATH`, usa `"command": "meada", "args": ["mcp"]`.

Son tres herramientas, siempre en inglés, y ninguna escribe:

- `search` encuentra un símbolo, una ruta o un contrato por el nombre; con el tipo `resource`,
  un recurso por su tipo o su id; con el tipo `channel`, un canal (cola o tópico) por el broker
  o el nombre; con el tipo `ambiguous` o `unresolved`, lista los ítems
  abiertos (ver Decisiones);
- `context` describe un símbolo: su código leído del disco, quién lo llama, a quién llama y los
  contratos que toca; en un recurso (`resource::postgres::orders`), lista quién lo escribe y
  quién lo lee, con el símbolo y la línea, o `(repository-level)` para el repositorio entero; en
  un canal (`channel::kafka::orders.created`), lista productores y consumidores, también los
  que llegan a él por un comodín o por un binding, con el nivel y el motivo, y un lado cuyo
  nombre Meada no puede evaluar aparece como `(unresolved)` y nunca se enlaza;
- `trace` sigue una cadena de llamadas por varios repositorios, río abajo (`down`) o río arriba
  (`up`); con `changes: uncommitted`, parte del diff del árbol de trabajo y sirve para analizar
  el impacto de lo que se acaba de editar.

Las respuestas son líneas compactas dentro de un presupuesto de tokens (2.000 por defecto,
ajustable en cada llamada). Cada enlace entre repositorios lleva el nivel y el motivo
(`→[level: reason]`), y `trace` termina con un pie: el nivel más débil de la cadena, el número
de saltos, los repositorios involucrados y los tokens usados.

## Modo servidor

> **Atención.** En modo servidor Meada devuelve fragmentos de código fuente a quien logre
> conectarse, y por defecto no autentica a nadie: el proxy inverso del operador admite a los
> clientes (TLS, tokens, SSO, política de red); el módulo OIDC, abajo, es la alternativa opt-in. Sin host, el servidor escucha solo en el loopback; escuchar
> más allá es una elección explícita del operador, y un proxy mal configurado expone el código.
> El servidor MCP no revisa el encabezado `Host` (un proxy con nombre propio funciona) y responde 403 a
> cualquier solicitud con `Origin` (protección contra navegadores y DNS rebinding), en
> cualquier ruta y antes de cualquier otra respuesta. El proxy no debe exponer Meada más allá
> de los clientes que admite.

El modo servidor ejecuta Meada una vez para una organización: `meada mcp --listen` sirve las
herramientas por HTTP a partir de espejos de los repositorios que lista un manifiesto de
workspace, con cada índice fuera del checkout. El manifiesto `meada.workspace.toml` vive en un
repositorio de operaciones, versionado, y solo se lee de la ruta que da `--workspace`:

```toml
index_dir = "/var/lib/meada"

[[repository]]
repo = "github.com/acme/web"
path = "/srv/mirrors/web"

[[repository]]
repo = "github.com/acme/api"
path = "/srv/mirrors/api"

[repository.serve]
hosts = ["api.acme.com"]
prefix = "/api"

[[repository.resource]]
kind = "postgres"
id = "orders"
role = "write"

[[repository.peer]]
repo = "github.com/acme/web"
base_url_env = "WEB_URL"
```

`index_dir` es obligatorio; `path` es absoluto o relativo a la carpeta del manifiesto. Los
bloques `serve`, `resource` y `peer` (el par no lleva `path`, pues el manifiesto ubica todos los
repositorios) son opcionales y valen para un espejo sin `meada.toml`: el `meada.toml` del propio
repositorio gana, completo, y los bloques del manifiesto se ignoran (`meada status` los nombra).
Un espejo es un checkout con árbol de trabajo; un clon bare no tiene qué indexar y se rechaza.

- `meada index --workspace <manifiesto>` actualiza el índice de cada repositorio, en
  `<index_dir>/<identidad>/index.db`. Es el refresh del operador, ejecutado por un job de fetch
  o un webhook: Meada nunca hace fetch ni clona, y el servidor solo lee. Tras instalar un
  binario nuevo, ejecute `meada index --workspace` antes de reiniciar el servidor.
- `meada mcp --listen <dirección> --workspace <manifiesto>` sirve. La dirección es
  `host:puerto`, `:puerto` o `puerto`; el host es una IP o `localhost`, nunca resuelto, y el
  valor por defecto es el loopback. La primera línea de stderr es
  `listening on http://<host>:<puerto>`. SIGINT o SIGTERM detienen el servidor: deja de aceptar
  conexiones, termina las respuestas en curso y sale con código 0. La espera tiene un límite fijo de 25
  segundos, por debajo de los 30 s de gracia por defecto de Kubernetes: pasado el límite, las
  conexiones restantes se descartan y la salida sigue siendo 0.
- Cada repositorio es un endpoint, `http://<host>:<puerto>/<identidad>` (por ejemplo
  `http://meada.internal:7333/github.com/acme/web`); el cliente configura una URL por
  repositorio en que trabaja. Cualquier otra ruta responde 404 con las identidades servidas.
- El transporte HTTP es sin estado, con respuestas JSON: cualquiera de varios servidores sobre
  los mismos índices responde a cualquier llamada.
- El servidor lee el manifiesto y el `meada.toml` y el `meada.decisions.toml` de cada espejo una
  vez, al iniciar: cambiar esos archivos exige reiniciar el servidor. Un cambio de código en un
  espejo solo pide `meada index --workspace`; la siguiente llamada ya ve el índice nuevo.
- `context` imprime `index behind the mirror: run meada index --workspace` cuando el archivo de
  un fragmento cambió después de indexar. `trace` con `changes: uncommitted` se rechaza, pues el
  árbol de trabajo está en la máquina del cliente.
- `meada status --workspace <manifiesto>` muestra el estado de cada índice, y
  `meada export --workspace <manifiesto> --repo <identidad>` imprime el índice de un
  repositorio, por ejemplo para alimentar una base de conocimiento.

### Varios nodos

El [ejemplo Docker Compose](deploy/compose/README.es.md) ofrece los dos layouts siguientes, con
dos nodos, TLS y admisión por token en el proxy.

El [chart Helm](deploy/helm/meada/README.es.md) configura init, readiness, drain y el perfil de generaciones en Kubernetes.

Varios servidores sobre los mismos índices funcionan sin módulo de nube, siempre que el
operador respete un único escritor por índice.

- **Layout A, cada nodo indexa sus propios espejos.** Cada nodo tiene un `index_dir` local y
  ejecuta `meada index --workspace` sobre los espejos que obtiene. Es lo más simple, pero los
  nodos divergen: cada uno ve el commit que obtuvo, y dos llamadas al mismo endpoint pueden
  responder distinto hasta que todos obtengan.
- **Layout B, un job construye y los nodos reciben.** Un job obtiene los espejos e indexa todo
  en un único commit; los nodos reciben los espejos e índices terminados y los cambian por
  `rename`. La reconstrucción usa el mismo cambio (`index.db.new.<pid>` bajo `index.db.lock`,
  renombrado sobre `index.db`), así que un lector ve el índice antiguo o el nuevo completo.
  Cambie un índice por `rename` solo mientras ningún `meada index` lo escribe: el
  `index.db-journal` vivo de un refresh in situ se tomaría como journal del archivo nuevo.
- **Sin soporte: `index_dir` en un sistema de archivos de red (NFS, EFS) compartido por un
  escritor y lectores.** SQLite no es fiable sobre la red (vea
  [sqlite.org/useovernet.html](https://sqlite.org/useovernet.html)). Dé a cada nodo un
  `index_dir` local, o un volumen que solo él escribe.
- **Solo lectura.** `index_dir` y los espejos pueden ser de solo lectura (un volumen llenado por
  un init, una capa de imagen): el servidor no crea nada allí. Un espejo de otro usuario se
  acepta con `-c safe.directory`, solo para los espejos del manifiesto, lo que exige git 2.38 o
  posterior; Ubuntu 22.04 y RHEL 8 traen un git más antiguo, así que defina `safe.directory` en
  la configuración del sistema.
- **Sin sesión.** El transporte es sin estado: no hacen falta sesiones pegajosas (sticky
  sessions), y los clientes en el protocolo 2026-07-28 ni siquiera tienen sesión.
- **Proxy inverso.** Reenvíe sin cambios `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` y todo
  encabezado `Mcp-Param-*` (SEP-2243: rmcp rechaza una petición cuyos encabezados discrepan del
  cuerpo); el proxy puede enrutar por ellos. Reenvíe el encabezado de respuesta
  `Meada-Generation`. No añada `Origin`: Meada responde 403 a toda
  petición que lo lleve, así una página de navegador no puede llamarlo directamente. El servidor MCP no
  comprueba `Host`, y `--listen` acepta una IP o `localhost`, nunca un nombre que resuelva.
- **Disponibilidad.** Primero un paso de init (`meada index --workspace` o la copia de los
  índices). Luego una sonda TCP, o `meada status --workspace <manifiesto>` como sonda exec: sale
  con 0 cuando todo repositorio puede servirse y con 1 en caso contrario, espera como máximo
  50 ms por un commit en curso y cuenta un índice bloqueado como listo. `--listen` abre el
  puerto solo tras las comprobaciones de inicio, así que un puerto abierto indica índices
  presentes. El 404 en `/` lista identidades y no es un health check.
- **Escritor único.** Ejecute el job de indexación sin solapamiento (por ejemplo un CronJob con
  `concurrencyPolicy: Forbid`). Defina `terminationGracePeriodSeconds` por encima del límite de
  25 s del SIGTERM y un retraso de `preStop` igual al tiempo que el balanceador tarda en
  retirar el nodo.

### Módulos de nube

Módulos opt-in, desactivados por defecto y documentados abajo. Un módulo
es una feature de cargo más una sección de `meada.workspace.toml`; los binarios de release son el
build por defecto, sin ninguno, y una sección en un build sin su feature es un error que nombra la
feature. La feature `cloud` los activa todos a la vez (`cargo install meada --features cloud`) y,
como `refresh-lock`, solo compila en Unix.

**Snapshot** (feature `snapshot`, `cargo install meada --features snapshot`). Un job indexa y
publica; los nodos reciben el resultado entero, en vez de indexar cada uno sus propios espejos.
Después de `meada index --workspace`, que sigue sin red, el job ejecuta
`meada snapshot publish --workspace <manifiesto>`. Comprueba, solo leyendo, que cada índice es
igual a su espejo y que cada espejo está limpio (`git status` no lista nada, ni un archivo
ignorado), y rechaza listando cada repositorio y el comando `meada index --workspace` que lo
resuelve. Luego sube una generación: un bundle por repositorio (un tar del espejo, `.git`
incluido, y de su `index.db`), un documento que los lista con su tamaño y su hash BLAKE3, y al
final el puntero `current`. Su única línea en stdout es el id de la generación. Un bundle o una
generación ya guardados no se suben de nuevo, y nada se escribe en la máquina que publica.

```toml
[snapshot]
endpoint = "https://s3.us-east-1.amazonaws.com"   # cualquier endpoint compatible con S3
region = "us-east-1"
bucket = "ejemplo-meada"
prefix = "prod"                                    # opcional
path_style = false                                 # true en la mayoría de los stores self-hosted
# ca_file = "ca.pem"                              # CA privada opcional, relativa al manifiesto
credentials = "static"                             # o "web-identity", "pod-identity", "instance"
access_key_id_env = "AWS_ACCESS_KEY_ID"            # valores por defecto; o las claves *_file
secret_access_key_env = "AWS_SECRET_ACCESS_KEY"
session_token_env = "AWS_SESSION_TOKEN"            # opcional
local_dir = "/var/lib/meada/generations"           # para el pull y el servicio de generaciones
```

El publish solo se conecta al `endpoint`, con TLS fuera de loopback, y al endpoint de credencial
de la fuente elegida; ninguna petición sigue una redirección ni pasa por un proxy. Las
credenciales vienen de una sola fuente, la que nombra `credentials`: `static` lee la clave y el
secreto de las variables o los archivos que nombra la sección (nunca del manifiesto);
`web-identity` (IRSA) lee `AWS_WEB_IDENTITY_TOKEN_FILE` y `AWS_ROLE_ARN` e intercambia el token
con STS en la `region`, por https; `pod-identity` (EKS Pod Identity) llama al agente en
`AWS_CONTAINER_CREDENTIALS_FULL_URI`, solo `http://169.254.170.23` o `http://[fd00:ec2::23]`,
con el token de `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`; `instance` usa el servicio de metadatos
de la instancia (IMDSv2, solo por IPv4, `http://169.254.169.254`), y solo cuando la sección lo indica. Las
credenciales quedan solo en memoria, y ningún secreto aparece en un error.

`ca_file` añade certificados PEM a las raíces de confianza del sistema. El archivo tiene como
máximo 1 MiB; los certificados inválidos y un archivo vacío se rechazan, nombrando el archivo.

En cada nodo, `meada snapshot pull --workspace <manifiesto>` lee el puntero `current` del store
y, cuando nombra una generación que el nodo no tiene, descarga sus bundles en `local_dir`,
comprueba el tamaño y el hash BLAKE3 de cada uno contra el documento, los extrae y cambia el
puntero del nodo por rename. La única línea en stdout es el id de la generación actual.
`--every <segundos>` repite el pull (y solo imprime cuando el puntero cambia); un segundo pull en
el mismo `local_dir` sale con 0 avisando que ya hay uno en curso. El nodo guarda la generación
actual y la anterior y borra las más antiguas.

Todo lo que viene del store se trata como entrada hostil: ids e identidades se comprueban antes
de volverse rutas; el pull rechaza en el archivo una ruta absoluta, un `..`, un enlace que sale
del repositorio, una entrada escrita a través de un enlace simbólico y dispositivos o FIFOs, con
techos de bytes y de entradas por generación. El `.git/config` extraído se reemplaza por uno
mínimo que Meada escribe (solo `[core]` y el `origin` de la identidad), `.git/hooks` se borra, y
el `git` de Meada corre en esos espejos sin la configuración del sistema ni la global. Un pull
que falla en cualquier paso deja el puntero y las generaciones guardadas como estaban.

```
<local_dir>/
  current                     # "<id de generación> <fence>", fence 0 sin el bloqueo de refresh
  pull.lock
  <id de generación>/
    generation.json           # el documento, como está guardado
    <id de bundle>/index.db
    <id de bundle>/mirror/    # el checkout, .git incluido
```

Para servir lo que el nodo recibió, `meada mcp --listen <dirección> --workspace <manifiesto>
--generations`. El manifiesto del nodo aporta solo las secciones de módulo (`[snapshot]`); los
repositorios son los que registró la generación actual, con los espejos e índices dentro de
`<local_dir>/<id de generación>/`. Cada llamada lee `<local_dir>/current` y abre el motor sobre
la generación que nombra: una llamada ya abierta termina en la generación en que empezó, la
siguiente ve la nueva, y `meada snapshot pull --every` cambia la generación sin reiniciar el
servidor (un repositorio nuevo en una generación nueva recibe su endpoint al instante). No se
escribe nada por llamada, los índices se abren solo para lectura y el servidor no abre ninguna
conexión con el store. Los nodos nunca ejecutan `meada index`: el job indexa y el nodo solo
recibe. Sin generación actual, o con un índice que no está en la versión de esquema de este
binario, el servidor se niega a arrancar y nombra `meada snapshot pull` o el repositorio y la
generación.

`meada status --workspace <manifiesto> --generations` es la sonda de disponibilidad del nodo.
Imprime el id de la generación actual, los ids guardados y una línea
`store: reachable (current <id>)` o `store: unreachable (<motivo>)`. El código de salida
depende solo del estado local: 1 sin una generación actual servible (los problemas van a
stderr), 0 en caso contrario, con el store alcanzable o no, de modo que un store caído no saca
al nodo de la rotación. `meada export --workspace <manifiesto> --repo <identidad> --generations`
exporta del mismo modo.

**Puntero de generación y pins** (feature `generation-pointer`, que incluye `snapshot`;
`cargo install meada --features generation-pointer`). Con la sección `[generations]`, que exige
`[snapshot]` y `[valkey]`, el puntero de la generación actual pasa del store a Valkey, y toda la
flota guarda el mismo conjunto de generaciones.

```toml
[valkey]
url = "rediss://meada@cache.ejemplo.internal:6379"   # TLS fuera de loopback
password_file = "/run/secrets/valkey"                 # o password_env
key_prefix = "meada:prod"                             # por defecto "meada"

[generations]
keep = 3                                              # generaciones retenidas en la flota, al menos 2
```

`meada snapshot publish` termina escribiendo, en un solo script, `{<key_prefix>}:current` (un
hash `{id, fence}`) y `{<key_prefix>}:retained` (una lista de `<id>:<fence>`, de la más nueva a
la más antigua, cada id una vez, recortada a `keep`), y ya no escribe el `current` del store; el
fence es `0` sin el candado de refresh. Cada `meada snapshot pull` lee ambos de Valkey y
comprueba cada id (16 caracteres hexadecimales en minúscula) y cada fence (decimal) antes de
tocar el nodo; luego descarga toda generación retenida que el nodo no tiene y borra las que
salieron de la lista, nunca la actual ni la que acaba de reemplazar. El archivo de puntero del
nodo lleva el fence leído con el id. Una entrada malformada, un puntero ausente o una lista
vacía detienen el pull con un error que nombra la clave, y el nodo queda como estaba. La
generación actual se descarga y se activa primero: una generación retenida que falla después se
vuelve un aviso en stderr con el id y la causa, no bloquea nada, y el siguiente pull lo
reintenta. Use el mismo `keep` en el job y en los nodos. `meada status --generations` lee la
generación actual de Valkey y comprueba que el store guarda su documento; la línea
`store: unreachable (...)` dice si falló Valkey o el store.

Un run de workflow fija los commits en que empezó con un pin en la ruta:
`/<identidad>@<generación>`, siendo la generación lo que sigue al último `@`, sirve esa
generación guardada en cualquier nodo mientras siga retenida, y `/<identidad>` sigue sirviendo la
actual. Una generación que el nodo no guarda responde 404 `text/plain` nombrando la generación
pedida, la actual y las guardadas. Cada respuesta HTTP nombra su generación en el encabezado
`Meada-Generation` (un 404 nombra la actual) y cada resultado de herramienta en su `_meta`,
`{"meada/generation": "<id>"}`: el run lee la generación de su primera respuesta y fija las
siguientes en ella. Las herramientas, los parámetros y el texto de las respuestas no cambian. El
proxy inverso reenvía `Meada-Generation` en las respuestas. Una llamada fijada aún abierta en
una generación que la retención de un pull borra puede fallar a mitad de camino.

**Candado de refresh** (feature `refresh-lock`, que incluye `generation-pointer`, solo Unix;
`cargo install meada --features refresh-lock`). Con la sección `[refresh_lock]`, que exige
`[generations]`, todo el refresh corre bajo un candado en Valkey, y cada adquisición recibe un
fence, de modo que un job superpuesto o trabado nunca reemplaza una generación nueva por una más
antigua.

```toml
[refresh_lock]
ttl_seconds = 300                                     # por defecto; al menos 10
```

```sh
meada snapshot lock --workspace ops/meada.workspace.toml -- \
  sh -c 'git -C api fetch && git -C api reset --hard origin/main &&
         meada index --workspace ops/meada.workspace.toml &&
         meada snapshot publish --workspace ops/meada.workspace.toml'
```

`meada snapshot lock` toma el candado con `SET {<key_prefix>}:lock <token aleatorio> NX PX
<ttl>`, suma uno a `{<key_prefix>}:fence` y corre el comando como hijo, con `MEADA_REFRESH_LOCK`
(el token) y `MEADA_REFRESH_FENCE` en el entorno. El candado se renueva cada tercio del TTL
(comparar y extender) y se libera al final (comparar y borrar); el código de salida es el del
hijo (128 más la señal cuando una señal lo terminó). Un candado ya tomado sale con 1 nombrando la
clave, sin correr nada. Cuando una renovación encuentra otro token, o ninguna tuvo éxito durante
un TTL entero, el candado está perdido: el grupo de procesos del hijo recibe SIGTERM (y SIGKILL
10 s después si sigue vivo), y `lock` sale con 1 diciendo que lo perdió. Un SIGTERM, SIGINT, SIGHUP o
SIGQUIT a `lock` llega al grupo del mismo modo; cuando el hijo termina, lo que quedó en su grupo
de procesos recibe SIGKILL (un nieto que llama a `setsid` sale del grupo y escapa), y en Linux el hijo recibe SIGTERM si `lock` muere antes. La indexación sigue sin red:
el `meada index` del hijo no abre ninguna conexión.

Con `[refresh_lock]`, `meada snapshot publish` exige las dos variables, y antes de cada envío
comprueba que el candado aún guarda el token: si no, se detiene sin enviar nada más y sin
escribir el puntero. El script del puntero solo escribe cuando el candado guarda el token y el
fence es mayor que el de `{<key_prefix>}:current`, en una sola operación. Los nodos nunca pasan
a una generación con fence menor que el de su archivo de puntero; sin el archivo, toman la
actual sea cual sea su fence. Meada nunca baja ni borra `{<key_prefix>}:fence`. Si un flush o un
failover de Valkey lo pierde, los fences siguientes quedan por debajo de los registrados y la
flota se detiene: el publish rechazado sale con 1 nombrando `{<key_prefix>}:fence`, el pull de un
nodo con fence mayor sale con 1 sin cambiar, y `meada status --generations` en ese nodo imprime
una línea `fence: stuck: ...` con los dos fences (su código de salida no cambia). Ejecute
`meada snapshot fence recover --workspace <manifiesto>`: bajo el lock, eleva el contador hasta
el mayor fence del pointer, las generaciones retenidas, el workspace y los heartbeats válidos,
usando el reloj de Valkey para seleccionar los nodos vivos. El siguiente publish usa un fence
mayor y desbloquea los nodos. `INCR` de Valkey tiene un límite de 9223372036854775807; la
recuperación rechaza pisos o fences registrados superiores a 9223372036854775806 y no consume
la última adquisición disponible. La lectura y el transporte siguen aceptando fences `u64` exactos. Para un nodo aislado cuyo fence
no aparece en un heartbeat válido, páselo como `--at-least <fence>`, o borre su
`<local_dir>/current` para que tome la generación actual. Apagar `[refresh_lock]` después de que los nodos registraron fences
por encima de 0 también detiene la flota, ya que sin el candado todo fence es 0: borre el
`<local_dir>/current` de cada nodo.

**OIDC** (feature `oidc`; `cargo install meada --features oidc`). La admisión por defecto sigue
siendo el proxy inverso, como dice el aviso al inicio de esta sección; el módulo OIDC es la
alternativa para quien quiere que el propio Meada verifique quién llama. Con la sección `[oidc]`,
el servidor se vuelve un resource server OAuth 2.1, como la especificación de autorización de
MCP (2026-07-28) lo describe: toda solicitud lleva `Authorization: Bearer <JWT>` emitido por el
proveedor del operador. Meada no emite tokens, no registra clientes y nunca reenvía un token.

```toml
[oidc]
issuer = "https://id.ejemplo.internal/realms/agentes"
public_url = "https://meada.ejemplo.internal"   # la URL que usan los clientes, detrás del proxy
audience = "https://meada.ejemplo.internal"     # opcional; por defecto: la URL canónica del endpoint
identities_claim = "meada_repositories"          # opcional con groups_claim
groups_claim = "groups"                          # opcional, junto con [oidc.groups]
scopes = ["meada:read"]                          # opcional, anunciado y exigido
jwks_refresh_seconds = 3600                      # por defecto; como mínimo 60

[oidc.groups]                                    # un grupo del token: las identidades que permite
"equipo-pagos" = ["github.ejemplo.internal/acme/payments-api", "github.ejemplo.internal/acme/ledger"]
"plataforma" = ["*"]
```

En el proveedor: un audience igual a `audience` (o, sin ella, a la URL de cada endpoint,
`<public_url>/<identidad>`); un claim mapper que pone en `identities_claim` la lista de
identidades que el cliente puede leer (`"*"` las permite todas), o en `groups_claim` sus grupos; y
client credentials para los agentes headless y para el backend de un portal, que piden su token al
proveedor. Una página en el navegador no puede llamar al servidor MCP directamente: con `[oidc]`, toda
solicitud con `Origin` recibe 403, en cualquier ruta, así que un portal llama desde su backend. El
proxy reenvía sin cambios `Authorization` y los encabezados del SEP-2243 (`MCP-Protocol-Version`,
`Mcp-Method`, `Mcp-Name`, `Mcp-Param-*`).

Con `groups_claim`, la tabla `[oidc.groups]` del manifiesto mapea cada grupo a las identidades
que permite (`"*"` para todas), y `identities_claim` pasa a ser opcional: las dos claves van
juntas, y una sección sin ninguno de los dos claims impide el arranque. Un token lee la unión de
las identidades de su `identities_claim` y de las que mapean sus grupos. Un claim ausente o que no
es un array de strings, un grupo que la tabla no mapea y una identidad que el servidor no sirve no
permiten nada; un error de tipeo le aparece a quien llama como un 403.

Al arrancar, antes de escuchar, Meada lee los metadatos del issuer (RFC 8414 u OpenID
Discovery, en las rutas y el orden que define la especificación de MCP, verificando que el
`issuer` del documento es el configurado) y el JWKS que nombran; una falla impide el arranque.
`issuer`, `public_url` y el `jwks_uri` deben ser `https` fuera de loopback; ninguna de las
solicitudes sigue una redirección ni pasa por un proxy, cada una tiene 10 s y un tope de tamaño,
y los certificados se verifican contra las raíces del sistema. El JWKS se vuelve a leer cada
`jwks_refresh_seconds` y cuando llega un token con un `kid` desconocido, como mucho una vez por
minuto; si la relectura falla, las claves en caché siguen y stderr lo dice.

`GET /.well-known/oauth-protected-resource/<identidad>` responde sin token los metadatos del
recurso (RFC 9728): `resource` igual a `<public_url>/<identidad>`, `authorization_servers` igual
a `[issuer]`, `bearer_methods_supported` igual a `["header"]` y `scopes_supported` cuando hay
`scopes`; `/.well-known/oauth-protected-resource/<identidad>@<generación>` responde lo mismo con
el `resource` fijado. Un token pasa con un algoritmo asimétrico (RS256, RS384, RS512, ES256,
ES384, PS256) que coincida con la clave, `iss` igual a `issuer`, un `aud` igual a `audience` o,
sin ella, a la URL de `/<identidad>` (que los pins también aceptan, de modo que un token lee el
repositorio en toda generación), `exp` obligatorio y `nbf` respetado con 60 s de margen, y todo
scope configurado en su `scope`. Un token en la query string nunca se lee. `scope` se lee solo
como string separado por espacios; `scp` y un `scope` en array no se leen, así que, con `scopes`
definido, un proveedor que envía `scp` (Okta, por ejemplo) necesita un claim mapper a `scope`.

Sin token, o con uno que no pasa: 401 con `WWW-Authenticate: Bearer
resource_metadata="<public_url>/.well-known/oauth-protected-resource/<ruta>"`, más
`error="invalid_token"` cuando llegó un token y `scope` cuando está configurado. Un token válido
que no puede leer la identidad del endpoint, también en un pin: 403 con
`error="insufficient_scope"`; sin un scope exigido, el mismo 403 con `scope`. Detrás de un
endpoint que el token puede leer, toda respuesta cubre solo los repositorios que puede leer. La
autenticación va antes del enrutamiento: el 404 de la raíz, de una ruta desconocida y de una
generación que el nodo no guarda exige un token válido para esa ruta (sin `audience`, el de la
raíz tiene `aud` igual a `public_url`) y lista solo las identidades que puede leer. Ningún token,
ni parte de uno, aparece en stderr o en un error.

**GitHub App** (feature `github-app`, que incluye Valkey; `cargo install meada --features
github-app`). Meada sigue sin buscar nada: el git del job de indexación hace el fetch de los
espejos, y Meada solo responde a la petición de credencial de git con tokens de instalación de
uno o más GitHub Apps, que él emite y renueva. Las instalaciones se descubren por la API de
GitHub, no se declaran; la carga se reparte entre los Apps y las instalaciones, y una instalación
limitada queda de lado hasta liberarse. Solo el job usa el módulo: `meada mcp --listen` lee los
bloques con el manifiesto y nunca llama a GitHub.

```toml
[valkey]                                # obligatorio con [[github_app]]
url = "rediss://meada@cache.ejemplo.internal:6379"
password_file = "/run/secrets/valkey"

[[github_app]]
host = "github.com"                     # el host git que atiende, como git lo nombra
api = "https://api.github.com"          # la URL base de la API REST; en GHES, https://<host>/api/v3
app_id = 123456
private_key_file = "/run/secrets/github-app.pem"   # o private_key_env, nunca los dos
```

Puede haber varios bloques `[[github_app]]`, uno por App (dos bloques del mismo App en el mismo
host son un error). `api` tiene que ser `https` fuera de loopback, y un bloque sin `[valkey]`,
sin clave o con las dos, o con una clave desconocida rechaza la ejecución nombrando la clave o la
sección. La clave privada solo viene del archivo o de la variable y nunca aparece en stderr ni en
un error, igual que el JWT del App y los tokens.

En el job, antes del fetch, git usa Meada como credential helper del host, con la ruta del
repositorio en la petición (use la ruta absoluta del manifiesto, ya que git ejecuta el helper en
el directorio del espejo):

```sh
git config --global credential.https://github.com.helper \
  "!meada github-app credential --workspace /etc/meada/meada.workspace.toml"
git config --global credential.useHttpPath true
```

Para `https://github.com/<owner>/<repo>`, el `get` responde `username=x-access-token`, un token
de una instalación que cubre a `<owner>` y su `password_expiry_utc`. Las instalaciones de cada
App se listan con su JWT (RS256, emitido 60 s en el pasado, que expira en 9 minutos) y se guardan
en Valkey, de nuevo como mucho cada 10 minutos; un bloque nuevo entra en el siguiente `get`, sin
reiniciar nada. Entre las instalaciones que cubren al owner, en un orden estable (id del App,
luego id de la instalación), Meada salta las marcadas como limitadas y elige una por turnos, con
un contador en Valkey. Un token solo se emite cuando Valkey no guarda uno de esa instalación con
al menos 10 minutos de vida; un lock por instalación hace que `get`s simultáneos emitan una sola
vez, y el token queda en caché hasta 10 minutos antes de expirar. Quien lee ese Valkey lee el
código fuente durante ese tiempo. Un repositorio que ninguna instalación cubre no recibe
respuesta, con salida 0 (git prueba el siguiente helper) y una línea en stderr con el host y el
owner. `store` no hace nada; el `erase`, que git manda después de un 401, descarta el token
rechazado, y el siguiente `get` emite otro.

Ni git ni GitHub avisan al helper de un límite secundario (403 o 429). Por eso, después de un
fetch que falló así, el job ejecuta:

```sh
meada github-app throttled --workspace /etc/meada/meada.workspace.toml \
  https://github.com/<owner>/<repo> --retry-after <segundos>
```

Eso marca como limitada la instalación que respondió por última vez para ese repositorio (Meada
lo recuerda durante una hora), hasta el `--retry-after` o, sin él, durante 1 minuto, duplicando
en cada repetición mientras la marca vale. Las llamadas REST del propio módulo que reciben 403 o
429 marcan su instalación (un token) o su App (el listado) del mismo modo, por el `retry-after`,
si no por el `x-ratelimit-reset` cuando `x-ratelimit-remaining` es 0, si no durante 1 minuto; un
`get` cuyo token fue limitado pasa a la siguiente instalación. Con git 2.54 o posterior,
`http.maxRetries` hace que el propio git respete el `Retry-After` de un 429 en sus peticiones
GET.

En GitHub.com y GitHub Enterprise Cloud, `api` queda fuera de la frontera del operador; se acepta
porque los fetches del job ya van a ese host (ADR 0013).

#### Consola de administración

La consola usa la feature `ui` (incluida en `cloud`) y escucha en un proceso propio.
Configure la admisión en el proxy inverso antes de exponerla: sin `[oidc]`, el proxy admite
los clientes, y una consola accesible más allá de esa admisión expone el grafo de código.
Las operaciones requieren `[oidc]` y `operations = true`. El proxy conserva `Host`, `Origin`
y `Authorization`; la consola acepta solo su Host público y, cuando existe, su propio Origin.

La consola tiene su manifiesto de bootstrap, separado de los manifiestos y directorios de
los nodos. Contiene las secciones de módulos; las entradas de repositorios vienen del
workspace administrado:

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

El `[oidc]` del bootstrap reproduce la política de los nodos, con la URL pública y el audience
del MCP; `[ui]` usa el origen y el audience propios de la consola.

Exporte las credenciales en el entorno del proceso o use los archivos de secretos
correspondientes. `snapshot.ca_file` añade una CA privada a las raíces del sistema;
`valkey.ca_file` usa solo las raíces de ese archivo.
El ejemplo incluye todos los módulos; omita `[[github_app]]` si el job no usa Apps. La consola
lee solo el estado de la caché de ese módulo, sin abrir la clave privada ni consultar GitHub.
Tampoco llama a los nodos: el playground y las exportaciones usan los índices de su propio pull.

Inicialice primero el workspace administrado, como se describe abajo. Después de que el job
publique la primera generación, haga el pull inicial y mantenga un companion en el `local_dir`
propio de la consola. Ese directorio no se comparte con los nodos. Ejecute el companion y la
consola en procesos separados:

```sh
meada snapshot pull --workspace /etc/meada/console.workspace.toml
meada snapshot pull --every 30 --workspace /etc/meada/console.workspace.toml
# Run in a separate process, alongside the pull companion.
meada ui --listen 0.0.0.0:7334 --workspace /etc/meada/console.workspace.toml
```

Registre en el issuer un cliente público, sin secreto de cliente, con Authorization Code y
PKCE S256, el audience configurado y el redirect URI exacto
`https://console.exemplo.internal/auth/callback`. El navegador debe acceder a los metadatos
y al endpoint de token del issuer, con CORS para el origen de la consola. El intercambio del
código ocurre directamente entre navegador e issuer. El bearer queda solo en memoria; state
y verifier temporales quedan en sessionStorage hasta que el callback los elimina. Recargar
la página reinicia el flujo de inicio de sesión.

Cada `snapshot pull --every` publica un heartbeat con generación, fence, generaciones locales,
preparación, versión y hora. `snapshot.node_name` debe ser único entre nodos y consola, con
hasta 63 letras minúsculas, dígitos, puntos o guiones; si se omite, usa el hostname válido.
Los nombres de pods sirven para ello. Un nodo sin heartbeat no aparece en la flota; los
heartbeats malformados aparecen por separado y no participan en los diagnósticos. Las páginas
muestran la flota, el store y su retención, los commits y schemas servidos, la caché de Apps
y la admisión. Las secciones ausentes se nombran explícitamente.

Con OIDC, los lectores ven solo las identidades admitidas por el token. Un grupo de
`ui.operator_groups`, o un claim de `ui.operator_claim` cuyo valor sea el booleano JSON `true`,
confiere el papel de operador. Elija solo una de esas dos claves; los operadores ven toda la
flota. Habilite `operations = true` para los cuatro endpoints HTTP de escritura. Cada acción
muestra su comando CLI y exige confirmación; un rechazo muestra el mensaje del CLI, incluido
el titular de un lock ocupado. Una línea de auditoría en stderr registra cada operación
aceptada o rechazada sin tokens ni bodies.

Las mismas operaciones siguen disponibles en el CLI:

```sh
meada snapshot pointer set <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot pointer retain <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot pointer drop <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot fence recover --workspace /etc/meada/console.workspace.toml
meada snapshot fence recover --at-least <fence> --workspace /etc/meada/console.workspace.toml
```

`set` selecciona una generación retenida. El rollback dura hasta que el siguiente publish
mueva el pointer; suspenda el job para permanecer en la generación anterior. `retain` mantiene
una generación en el conjunto kept, de hasta ocho generaciones que el trim no elimina, y
`drop` la libera (la actual no puede eliminarse). `fence recover` eleva el contador hasta el
mayor fence registrado, o hasta el mínimo opcional, bajo el lock. El siguiente publish usa un
fence mayor y desbloquea los nodos. Para un nodo aislado cuyo fence no aparece en un heartbeat
válido, pase ese fence como `--at-least`, o elimine su `local_dir/current` para que tome la
generación actual. No edite el contador manualmente.

El workspace administrado separa las entradas de repositorios de los módulos locales.
Prepare un archivo con `[[repository]]`, usando las rutas de los mirrors del job, e importe
una versión inmutable:

```toml
[[repository]]
repo = "github.exemplo.internal/acme/api"
path = "/var/lib/meada/mirrors/api"
```

```sh
meada workspace import repositories.toml --message "Add API repository" --workspace /etc/meada/job.workspace.toml
meada workspace list --workspace /etc/meada/job.workspace.toml
meada workspace show <version> --workspace /etc/meada/job.workspace.toml
meada workspace apply <version> --workspace /etc/meada/job.workspace.toml
meada workspace pull --workspace /etc/meada/job.workspace.toml
```

El import valida las mismas entradas que el editor de la consola; reimportar el mismo contenido
conserva el id y los metadatos de la primera escritura. `apply` mueve el pointer del workspace
bajo el lock, sin publicar una generación. `pull` instala esa versión localmente; los comandos
siguientes la leen sin abrir conexiones durante la indexación. El job tiene su propio bootstrap,
`index_dir`, mirrors y `local_dir`; comparte con los nodos y la consola solo las secciones de
módulos que apuntan al mismo store y Valkey. Ejecute todo su refresh bajo un único lock, con
el fetch fuera de Meada:

```sh
meada snapshot lock --workspace /etc/meada/job.workspace.toml -- sh -ec '
  meada workspace pull --workspace /etc/meada/job.workspace.toml
  ./fetch-mirrors.sh
  meada index --workspace /etc/meada/job.workspace.toml
  meada snapshot publish --workspace /etc/meada/job.workspace.toml
'
```

La generación publicada registra la versión del workspace que la originó. Aplicar una versión
anterior prepara el siguiente refresh; los nodos siguen sirviendo la generación que guardan
hasta otro publish y pull. El editor permite validar, comparar, guardar una versión y
aplicarla por separado.

La imagen `ghcr.io/claudiomascaro/meada:<versión>-cloud` incluye el frontend en
`/usr/share/meada/ui`. Para instalar con Cargo, use `cargo install meada --features cloud`,
descargue `meada-ui-<versión>.tar.gz` de la misma release, extráigalo y apunte `[ui] dir`
al directorio extraído. La versión de `meada-ui.json` debe coincidir con el binario; un frontend
de otra versión impide el inicio. Los perfiles de consola de [Compose](deploy/compose/) y
[Helm](deploy/helm/meada/) están deshabilitados por defecto y tienen bootstrap, admisión y
almacenamiento propios.

## Decisiones

Un enlace ambiguo (varios proveedores sirven la ruta) o una URL dinámica (la llamada arma la URL
en tiempo de ejecución) queda como ítem abierto. La herramienta `search` del MCP, con el tipo
`ambiguous` o `unresolved`, lista esos ítems con cuántos candidatos tiene cada uno, no los
candidatos; `meada decide --list` lista los ítems con sus candidatos. Los resuelves tú, en el
modo humano, o un modelo, si lo pides.

- `meada decide` muestra cada ítem abierto del repositorio, con los candidatos numerados; eliges
  un número, o `s` para saltar. Al final graba las elecciones en `meada.decisions.toml`, en la
  raíz. Si no elegiste nada, no graba.
- `--item <id>` trata solo ese ítem (el id que imprime `meada decide --list` o la herramienta
  `search`), aunque ya esté decidido, para rehacer su decisión; `--dry-run` imprime el archivo
  que se grabaría y no escribe; `--list` imprime los ítems abiertos, más las decisiones
  obsoletas, y no pregunta nada; `--prune` quita las decisiones obsoletas (no se combina con
  las tres anteriores).
- `--decider` cambia la persona por un modelo (ver abajo); `--show-request` (solo con
  `--decider`) imprime cada solicitud que se enviaría y no envía ninguna; `--min-p <p>` (solo
  con `--decider`, de 0 a 1) es la probabilidad mínima para que el modelo grabe una decisión.

`meada.decisions.toml` es un archivo versionado en git, como un lockfile: su diff es lo que se
revisa. Cada `[[decision]]` trae la llamada (`call`), el proveedor elegido (`target`), quién
decidió (`by`: `human` o el id del modelo) y, para un modelo, la probabilidad (`p`). La
indexación solo vuelve a leer el archivo: el enlace decidido recibe el nivel `decided` y su
motivo dice quién decidió. Cuando el código cambia y una decisión deja de encajar (la llamada
desapareció, el ítem ya no está abierto, el proveedor desapareció o ya no es candidato),
`meada status` la muestra como obsoleta, bajo su repositorio, y `meada decide --prune` la quita.
Una decisión que espera un par aún no encontrado aparece aparte y se conserva.

### Decisor por modelo

`meada decide --decider` es el único camino de Meada hacia la red, y solo corre cuando lo
pides. Cuatro variables de entorno lo configuran (nada viene de un archivo):

| Variable | Papel | Valor por defecto |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` o `chat` | `decisions` |
| `MEADA_DECIDER_URL` | URL base | `https://openrouter.ai` (`decisions`); obligatoria con `chat` |
| `MEADA_DECIDER_KEY` | clave de la API, enviada solo en el encabezado `Authorization` | obligatoria con `decisions`; opcional con `chat` |
| `MEADA_DECIDER_MODEL` | modelo | `typesafe/jev-1.14` (`decisions`); obligatoria con `chat` |

- **`decisions`** (el predeterminado) usa la API de decisiones de OpenRouter, con Jev: una sola
  solicitud lleva todos los ítems, y la respuesta trae la probabilidad de cada candidato. La
  solicitud pide cero retención de datos (`zdr: true`, `data_collection: deny`); si OpenRouter
  rechaza esa preferencia, la solicitud falla y no se graba nada.
- **`chat`** habla con cualquier endpoint compatible con la API de chat de OpenAI
  (`<URL>/chat/completions`), incluso un modelo local, por ejemplo con Ollama:
  `MEADA_DECIDER_PROTOCOL=chat MEADA_DECIDER_URL=http://localhost:11434/v1
  MEADA_DECIDER_MODEL=<modelo> meada decide --decider`. Una solicitud por ítem. La probabilidad
  es la que el propio modelo dice de sí, no una medida calibrada.

Sirve cualquier gateway que hable uno de los dos protocolos: basta cambiar la URL. HTTPS en
cualquier lugar; HTTP solo hacia `localhost` o una dirección de loopback.

Lo que sale de la máquina, por ítem: la llamada, la función que la contiene (leída del disco) y
la lista de candidatos (repositorio, método y ruta), más el id del repositorio que hace la
llamada. Nada de código del proveedor, ningún otro archivo. `--show-request` muestra todo eso
antes, con la clave cambiada por `<hidden>`. Cada respuesta se valida por ítem; una inválida, o
una solicitud que falló, se informa sin grabar ese ítem, y el comando termina con error.

Constantes fijas:

- `--min-p` vale 0,5 por defecto: por debajo, la propuesta se imprime como sugerencia y no se
  graba. Cambia solo mediante un PR con datos.
- Como máximo 50 candidatos por ítem: con más, el ítem se salta en el modo decisor (el modo
  humano aún lo ofrece).
- 120 segundos por solicitud, de la conexión al último byte, sin reintento.
- Con el protocolo `decisions`, las probabilidades de una respuesta deben sumar 1 con una
  tolerancia de 0,01.

## Estado

El modo servidor incluye la consola de administración, heartbeats y recuperación de la
flota, retención de generaciones y workspace administrado mediante módulos opt-in. La
consola ofrece editor, diff, playground y grafo; [Compose](deploy/compose/) y
[Helm](deploy/helm/meada/) incluyen perfiles de despliegue. La admisión y las operaciones
se describen arriba.

La consola ofrece operaciones autenticadas por OIDC para operadores: pointer, recuperación
de fence y versiones del workspace, con el mismo lock y las mismas comprobaciones del CLI.
Las operaciones están desactivadas por defecto y cada intento genera una línea de
auditoría. El grant también limita los grupos y errores del manifiesto; el login conserva los
parámetros del issuer y la recuperación respeta el reloj y el límite del contador de Valkey.

Versión 0.1, la primera. Indexa repositorios TypeScript y JavaScript (`.ts`, `.tsx`, `.js`,
`.jsx`, `.mjs`, `.cjs`) y Python (`.py`), lee el código Rust (`.rs`) de los crates del
repositorio (cada `Cargo.toml` con `[package]`, en cualquier carpeta, cada crate
por el árbol de módulos que arman sus `mod`, sin toolchain Rust: un archivo al que ningún `mod`
llega queda fuera) con sus funciones, tipos, traits, módulos y métodos de `impl` y las llamadas
escritas como ruta (`crate::a::f()`, `Tipo::new()`, `Self::f()`), también por `use` (grupos,
renombres, globs, `pub use` y los otros crates del workspace por nombre; las de método (`x.m()`) por el tipo del receptor cuando el código lo escribe (`let`, parámetro, campo, `self`, retorno de función; un `dyn Trait` o genérico con trait va al método del trait; un método que los `impl` de un tipo del repositorio no tienen, de un `derive` o de un trait de fuera, no se liga a nada, y un atributo de ruta nunca se liga a una función), si no por el nombre; las de dentro de macros
no se indexan, y los contratos Rust son los paquetes, el HTTP y los canales (abajo); solo entra la configuración por defecto de cada crate, para un destino fijo x86_64 Linux: un ítem bajo un `#[cfg(..)]` falso, o bajo una feature fuera del `default` del `Cargo.toml`, queda fuera, y los módulos `#[cfg(test)]` entran), lee el código Go (`.go`, sin toolchain Go: un import llega a la carpeta del paquete por la ruta de módulo de cada `go.mod`, también por un `replace` a una carpeta del repositorio, y un módulo anidado es dueño de su subárbol; los archivos de una carpeta con el mismo `package`, `_test.go` incluidos, comparten un ámbito; las restricciones de build valen para un destino fijo linux/amd64, y un `_windows.go`, un `//go:build` falso o un archivo cgo (`import "C"`) queda fuera) con sus funciones, tipos, interfaces y métodos (`Tipo.Metodo`) y las llamadas a funciones del mismo paquete y por import (`pkg.F()`, con alias o con punto; la biblioteca estándar y los módulos de fuera quedan externos) y a métodos (`recv.M()`: `exact` cuando el código escribe el tipo del receptor, en un parámetro, campo, `var`, literal compuesto o resultado de función, y por nombre, `inferred` o `ambiguous`, cuando no lo escribe; una llamada por una interfaz se detiene en el método de la interfaz), y enlaza repositorios, también de un
lenguaje al otro,
por contratos HTTP (`fetch` y, en Python, `requests`, `httpx` y `aiohttp` y, en Rust, `reqwest` y `ureq` y, en Go, `net/http` y resty del lado que consume;
Next.js (app router) y Express y, en Python, FastAPI, Flask y Django y, en Rust, axum, actix-web y Rocket y, en Go, `net/http`, gin, echo, chi y gorilla/mux del lado que sirve, también
detrás de un gateway; en Rust, una URL base leída del entorno con `std::env::var`, `env!` o `dotenvy::var` y, en Go, con `os.Getenv` o `os.LookupEnv` (con un valor por defecto `if v == "" { v = ... }`, también con `len(v) == 0`, `!ok` o `!ok || v == ""` cuando `ok` viene del mismo `os.LookupEnv`, y a través de `strings.TrimSuffix`, `TrimRight` o `TrimSpace`) enlaza por `base_url_env`), por los
recursos declarados en el `meada.toml` y por canales: tópicos Kafka (`kafkajs` y, en Python, `confluent-kafka` y `kafka-python` y, en Rust, `rdkafka` y, en Go, `segmentio/kafka-go`, `confluent-kafka-go` y `sarama`), colas RabbitMQ
(`amqplib` y, en Python, `pika` y, en Rust, `lapin` y, en Go, `amqp091-go`, con el `publish` a un exchange llegando a las colas que los
bindings le atan), colas Amazon SQS (AWS SDK v3, `sqs-consumer` y, en Python, `boto3` y, en Rust, `aws-sdk-sqs` y, en Go, `aws-sdk-go-v2`), subjects
NATS (`nats` y, en Python, `nats-py` y, en Rust, `async-nats` y, en Go, `nats.go`, con los comodines `*` y `>`), jobs BullMQ y streams Redis
(`ioredis`, `node-redis` y, en Python, `redis-py` y, en Rust, el crate `redis` y, en Go, `go-redis`), con el productor enlazado al handler de cada
consumidor, también en el mismo repositorio. Un repositorio con la tabla `[serve]` también lee
sus documentos OpenAPI (3.0 y 3.1) y AsyncAPI (2.x y 3.0), encontrados por el nombre o listados
en `[serve] documents`: las rutas y los canales que describen se vuelven contratos, con los
campos de cada respuesta o payload, y `meada status` dice qué leyó y qué ignoró. Cada
`package.json` con nombre se vuelve el contrato `package::npm::<nombre>`, y cada dependencia
(`dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies`) que nombra un
paquete del workspace (de cualquier repositorio, el propio incluido) se enlaza a él, con el rango de versión comprobado
contra la versión local (`range mismatch` cuando no la acepta, `range unchecked` cuando no se
puede comprobar). Una llamada a una función importada de ese paquete atraviesa hasta el código
del proveedor, por los campos `exports`, `types`, `module` y `main` y por los re-exports,
también cuando el punto de entrada apunta a la carpeta de build (el `outDir` del
`tsconfig.json`, mapeado de vuelta al `rootDir`). `search` con el tipo `package` encuentra el
paquete por el nombre, con el repositorio, la carpeta, la versión y el número de consumidores,
y `context` de una función exportada lista quién la llama, en cualquier repositorio del workspace. En
Python, cada `pyproject.toml` con nombre (`[project]` o `[tool.poetry]`) se vuelve el contrato
`package::pypi::<nombre>`, con el nombre normalizado como manda el PEP 503, y cada dependencia
de `[project] dependencies`, `[tool.poetry.dependencies]` o de un `requirements*.txt` que nombra
un paquete del workspace se enlaza a él, con el rango tal como está escrito y `range unchecked`
(los rangos de PyPI no se comprueban); un `import` de un módulo de ese paquete cruza hasta el
código del proveedor, `exact` cuando el módulo lleva el nombre de la distribución, `inferred`
cuando es solo uno de sus módulos de nivel superior y `ambiguous` cuando varias lo exponen. Un
paquete npm y uno de PyPI con el mismo nombre nunca se enlazan. En Rust, cada `Cargo.toml` con
`[package] name` (cada miembro de un workspace Cargo es un proveedor) se vuelve el contrato
`package::cargo::<nombre>`, y cada entrada de `[dependencies]`, `[dev-dependencies]` y
`[build-dependencies]` se enlaza a él: por `path`, al crate de esa carpeta, en el mismo repositorio
o, si la ruta sale de él (`../otro`), en otro repositorio del workspace (la ruta se resuelve solo
por el texto, desde las raíces de los repositorios, sin leer nada fuera de ellos ni seguir enlaces
simbólicos; una ruta que no cae en ningún repositorio del workspace no aparece); por `git`, al
repositorio cuya identidad es la URL normalizada; por versión, al proveedor del mismo nombre en el
workspace; `package =` da el nombre real de una dependencia renombrada, `workspace = true` lee
`[workspace.dependencies]` y `version.workspace = true` lee `[workspace.package]`. El enlace es
`exact`, con el rango como está escrito y `range unchecked` (los rangos Cargo no se comprueban), y
un `use` del crate (por el `[lib] name`, si lo hay, o por la clave de una dependencia renombrada)
llega a los ítems públicos del proveedor por sus `pub use`, también `exact`, con `; import crate
name` al final de la razón. Una dependencia fuera del workspace no aparece. En Go, cada `go.mod`
con `module` se vuelve el contrato `package::go::<ruta del módulo>`, y cada `require`, y cada
`replace` a una carpeta local, se enlaza con el proveedor de ese módulo en el workspace: un
`replace` que sale del repositorio (`../otro`) se resuelve solo por el texto, como el `path` de
Cargo, y un `replace` por otro módulo deja el módulo fuera. El enlace es `exact`, con la versión
del `require` como rango, `(local none)` (un `go.mod` no tiene versión) y `range unchecked`, y un
import de `<módulo>/<carpeta>` llega a las funciones, tipos y métodos exportados de esa carpeta en
el proveedor, también `exact` cuando la carpeta existe, con `; import module path` al final de la
razón. No lee `go.sum` ni consulta el proxy de módulos. No lee lockfile
(`Cargo.lock` incluido) ni consulta el registro npm, PyPI ni crates.io. Meada también señala el *drift* de forma: los campos que un consumidor `fetch` lee de la respuesta ya interpretada y que la ruta que llama no devuelve, comparados con la forma de la ruta (un documento OpenAPI, un schema zod o los objetos literales de su handler). `trace` termina la línea del enlace con `drift: missing <camino>`, `context` de una ruta dice de dónde viene su forma y lista cada consumidor que lee campos de la respuesta, con su drift y nivel y las lecturas no comprobadas bajo una parte abierta cuando las hay, `search` con el tipo `drift` lista todo el drift, y `meada status` cuenta las lecturas ausentes de los consumidores de cada repositorio (las cuenta el repositorio del consumidor; el del proveedor no muestra nada). `meada export` trae las comunidades de cada repositorio: grupos de símbolos que se llaman más entre sí que con el resto, hallados con el algoritmo de Leiden sin nada aleatorio y solo con aritmética entera, y nombrados por la carpeta donde vive la mayoría de sus miembros (`src`, `src#2`), iguales en cada máquina. Cada enlace tiene el nivel `exact`, `declared`, `inferred`, `decided` o `ambiguous`; una llamada cuya URL no
se puede resolver aparece como no resuelta y solo se vuelve enlace por una decisión, en el nivel
`decided`, y una ruta que Meada no sabe dónde se sirve queda fuera del mapa en vez de ser
adivinada. Los clústeres de broker no se modelan: dos clústeres que tienen un tópico con el
mismo nombre se enlazan como si fueran uno solo. Cada ruta que sirve un handler, cada canal que consume un handler consumidor y cada función o método exportado que nada llama en su propio repositorio es un punto de entrada, con su proceso: `trace` con `process::<identidad del punto de entrada>` lee el árbol de llamadas que dispara, a través de los repositorios, y `meada export` trae el proceso de cada punto de entrada. En Python, el bloque `if __name__ == "__main__":` de un archivo (el símbolo `__main__`) y cada función que nombra un `[project.scripts]` del `pyproject.toml` (`módulo:función`) también son puntos de entrada, y solo los nombres públicos (sin `_` al inicio, o los del `__all__` literal) cuentan como exportados. En Rust, el `fn main` de cada binario (`src/main.rs`, `src/bin/`, `[[bin]] path`) también lo es. En Go, cada `func main` de un `package main` también lo es. `search` con el tipo `community` halla una comunidad por su nombre y con el tipo `process` halla un proceso por la etiqueta de su punto de entrada, y `context` de un símbolo dice su comunidad (o `community: none`) y los procesos a los que pertenece, hallados subiendo por las mismas llamadas y enlaces que recorre el proceso. Aún no hay: otros lenguajes, otros clientes HTTP
además de esos, otros clientes de mensajería además de estos, ni el paquete npm de Meada. El alcance y el orden del trabajo están
en el [ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md); las decisiones, en
[`docs/adr`](docs/adr). Mira el [CHANGELOG](CHANGELOG.md).

## Por qué otra herramienta

Quien trabaja con varios servicios sabe que el bug casi nunca vive en un solo repositorio. El
front llama a una ruta que cambió en el back; el worker consume una cola que otro equipo
renombró. De las herramientas de grafo de código que existen hoy, algunas miran un repositorio
a la vez, otras solo conectan repositorios cuando el texto de la ruta coincide exactamente, y
otras prohíben el uso comercial.

Meada quiere seguir el hilo de punta a punta, atravesando todos los repositorios que haga falta,
y decir con qué confianza se hizo cada conexión.

## Principios

- **Tu código se queda contigo.** Corre en tu máquina, sin cuenta, sin clave de API y sin
  telemetría.
- **Sin LLM para indexar.** El mapa sale del árbol sintáctico del código. No hay factura en
  dólares por token, funciona sin conexión y cabe en una laptop común.
- **Determinista.** El mismo código con la misma configuración siempre da el mismo mapa.
  Cuando una conexión es ambigua, la decisión queda en un archivo versionado en git, tomada
  por una persona o, si lo prefieres, por un modelo de lenguaje en un comando aparte. La
  indexación solo vuelve a leer esa decisión.
- **Portugués primero.** La documentación nace en portugués de Brasil; inglés y español son
  traducciones. Las issues y los PR pueden abrirse en cualquiera de los tres idiomas. El código
  se escribe en inglés para que cualquier persona pueda leerlo.
- **Juntar sin mezclar.** "No todo lo que se junta se mezcla", enseñó Antônio Bispo dos Santos,
  conocido como Nêgo Bispo. Cada repositorio guarda su propio índice; Meada solo cose las
  puntas.
- **Poco ruido para el agente.** Meada no escribe en tu `CLAUDE.md`, no instala hooks ni toca
  la configuración de tu editor sin un comando tuyo. Las respuestas son cortas.
- **Crédito a quienes vinieron antes.** Meada aprende de GitNexus, CodeGraph,
  codebase-memory-mcp y kivgraph.

## Cómo participar

El proyecto está empezando. El alcance de la v1 y el orden del trabajo están en el
[ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md). Ya puedes ayudar:

- abriendo una issue con tu caso de uso entre repositorios, en portugués, español o inglés;
- revisando las traducciones: si el inglés o el español suenan raros, la corrección es
  bienvenida;
- leyendo y cuestionando los ADR.

## El nombre

*Meada* es una madeja de hilo. *Puxar o fio da meada*, "tirar del hilo de la madeja", es
encontrar la punta que deshace el nudo, y eso es lo que intenta hacer la herramienta cuando
sigue una llamada de un repositorio a otro.

## Licencia

[Apache-2.0](LICENSE).
