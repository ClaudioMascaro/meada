# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · **Español**

> Esta es una traducción. El [README](README.md) en portugués de Brasil es la fuente; si no
> coinciden, vale el original, y cualquier corrección a esta página es bienvenida.

Meada lee tus repositorios, arma el mapa de quién llama a quién y cose un repositorio con otro
a través de los contratos que comparten: rutas HTTP, colas, paquetes, recursos compartidos. Los
agentes de código, y también las personas, consultan ese mapa por MCP. Ninguna etapa de la
indexación usa un LLM.

## Instalación

Elige una de cuatro formas.

- **Cargo**: `cargo install meada`.
- **Binario listo**: descarga el archivo de tu sistema desde los
  [releases de GitHub](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS y
  Windows, en x86_64 y arm64) y pon `meada` en el `PATH`.
- **Instalador** (Linux y macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.
- **npm**: `npx meada <comando>` descarga el binario listo de tu plataforma y lo ejecuta, sin
  instalar nada antes.

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
entonces debe estar dentro del repositorio. En la configuración de tu agente (por ejemplo el
`.mcp.json` de Claude Code), con `npx`:

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
> conectarse, y no autentica a nadie: el proxy inverso del operador admite a los clientes (TLS,
> tokens, SSO, política de red). Sin host, el servidor escucha solo en el loopback; escuchar
> más allá es una elección explícita del operador, y un proxy mal configurado expone el código.
> Meada no revisa el encabezado `Host` (un proxy con nombre propio funciona) y responde 403 a
> cualquier solicitud con `Origin` (protección contra navegadores y DNS rebinding). Brecha
> conocida: el 404 que lista las identidades servidas responde antes de esa revisión, así que
> una página de DNS rebinding puede leer la lista de identidades de repositorio. El proxy no
> debe exponer Meada más allá de los clientes que admite.

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
  cuerpo); el proxy puede enrutar por ellos. Reenvíe `Mcp-Session-Id` para clientes antiguos y
  el encabezado de respuesta `Meada-Generation`. No añada `Origin`: Meada responde 403 a toda
  petición que lo lleve, así una página de navegador no puede llamarlo directamente. Meada no
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

Módulos opt-in, desactivados por defecto; cada uno se documenta abajo cuando llegue. Un módulo
es una feature de cargo más una sección de `meada.workspace.toml`; los binarios de release son el
build por defecto, sin ninguno, y una sección en un build sin su feature es un error que nombra la
feature.

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

**Candado de refresh** (feature `refresh-lock`, que incluye `generation-pointer`;
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
10 s después si sigue vivo), y `lock` sale con 1 diciendo que lo perdió. Un SIGTERM o SIGINT a
`lock` llega al grupo del mismo modo; cuando el hijo termina, lo que quedó en su grupo recibe
SIGKILL, y en Linux el hijo recibe SIGTERM si `lock` muere antes. La indexación sigue sin red:
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
una línea `fence: stuck: ...` con los dos fences (su código de salida no cambia). Para salir de
ahí, defina `{<key_prefix>}:fence` igual o por encima del mayor fence registrado (en Valkey o en
el `<local_dir>/current` de algún nodo), o borre el `<local_dir>/current` del nodo, que entonces
toma la generación actual.

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
