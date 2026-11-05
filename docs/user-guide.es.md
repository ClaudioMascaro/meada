# Guía del usuario

> Esta es una traducción. La [guía](user-guide.md) en portugués de Brasil es la fuente; si no
> coinciden, vale el original, y cualquier corrección a esta página es bienvenida.

Esta guía lleva a una persona que habla español de cero a Meada funcionando: instalar, conectar
un agente, configurar un workspace, decidir lo que el código no deja claro, usar la línea de
comandos y leer el export. Los ejemplos usan los repositorios `web`, `api` y `worker` y el host
`api.example.com`.

La segunda mitad (secciones 8 a 11) explica qué une Meada y dónde se detiene: cada tipo de
contrato, cada lenguaje, el drift de forma, las comunidades y los procesos. Úsela para saber si
un enlace que falta es un límite conocido o un bug.

Índice:

1. [Instalación](#1-instalación)
2. [Conectar un agente (MCP)](#2-conectar-un-agente-mcp)
3. [El workspace y el primer uso](#3-el-workspace-y-el-primer-uso)
4. [`meada.toml`: referencia](#4-meadatoml-referencia)
5. [Decisiones y decisores](#5-decisiones-y-decisores)
6. [Comandos de la CLI](#6-comandos-de-la-cli)
7. [El export](#7-el-export)
8. [Tipos de contrato](#8-tipos-de-contrato)
9. [Lenguajes](#9-lenguajes)
10. [Drift de forma](#10-drift-de-forma)
11. [Comunidades y procesos](#11-comunidades-y-procesos)

## 1. Instalación

Hay cinco canales, y todos entregan el mismo binario `meada`. Elija el que encaje con su
entorno.

### Cargo

```sh
cargo install meada
```

Compila desde crates.io y exige el toolchain de Rust.

### Binario listo

Descargue el archivo de su sistema de los
[releases de GitHub](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS y Windows,
en x86_64 y arm64), extráigalo y ponga `meada` en el `PATH`.

### Instalador de shell

Para Linux y macOS:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh
```

### npm (`npx @meada/meada`)

```sh
npx @meada/meada --version
npx @meada/meada index
```

El paquete `meada` de npm contiene solo el instalador: descarga del release de GitHub el binario
de su plataforma y lo ejecuta con los argumentos que usted pasó. La versión del paquete es
siempre la del release. La instalación descarga el binario en el momento, así que necesita red
(no hay instalación offline por npm). Es el canal que más interesa a quien configura un agente,
porque `npx -y @meada/meada mcp` es la línea de lanzamiento del servidor MCP (sección 2).

### Contenedor

```sh
docker run --rm ghcr.io/claudiomascaro/meada:latest --version
```

`ghcr.io/claudiomascaro/meada:<versión>` es el build predeterminado y `<versión>-cloud` trae
todos los módulos de nube, ambos para Linux amd64 y arm64. La imagen corre sin root y no define
comando: pase los argumentos explícitamente. Para servir a una organización, vea
[Modo servidor](https://claudiomascaro.github.io/meada/es/server/).

### Qué necesita Meada

Solo el binario y `git`. La indexación lee los archivos del repositorio sin ejecutar el código
ni el toolchain del lenguaje, no usa LLM y no abre red. Compruebe la instalación con
`meada --version`.

La consola de gestión (módulo opcional, `--features ui`) y los demás módulos de nube tienen
instalación propia, descrita en
[Instalación](https://claudiomascaro.github.io/meada/es/installation/).

## 2. Conectar un agente (MCP)

`meada mcp` habla MCP por stdio. El cliente (el agente) ejecuta el comando y conversa con él. La
línea de lanzamiento documentada es `npx -y @meada/meada mcp`, que evita una instalación separada.

### Configuración del cliente

En el `.mcp.json` de Claude Code, o en el archivo equivalente de su cliente:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["-y", "@meada/meada", "mcp"] }
  }
}
```

En Claude Code, lo mismo en un comando: `claude mcp add meada -- npx -y @meada/meada mcp`.

Con `meada` ya en el `PATH` (por Cargo, binario o instalador), cambie el comando por
`"command": "meada", "args": ["mcp"]`, que evita la consulta a npm en cada arranque.

### Sobre qué repositorio responde el servidor

El servidor responde sobre la primera raíz (`roots`) que el cliente informa. Claude Code envía la
carpeta de la sesión, así que abrir una sesión en `web` hace que Meada responda sobre `web`,
aunque el servidor haya sido iniciado en otro lugar. Sin raíces, responde sobre la carpeta en que
fue iniciado, que debe estar dentro del repositorio. Si el cliente no responde a `roots/list` en
5 segundos, la llamada usa la última raíz enviada en esa conexión o devuelve un error que lo
dice; nunca cae en la carpeta del servidor por ese motivo.

Toda consulta actualiza antes el índice con lo que cambió en los archivos, incluso lo que aún no
fue commiteado. No existe un paso de reindexación para el agente.

### Las tres herramientas

Son tres, con nombres y descripciones en inglés, y ninguna escribe. Las respuestas son líneas
compactas dentro de un presupuesto de tokens (`budget`, 2.000 por defecto, ajustable en cada
llamada; un token equivale a cuatro bytes de salida). Cuando la respuesta pasa del presupuesto,
termina con `… cut after N of M lines (budget B tokens); raise budget for more`.

#### `search`: encontrar el punto de partida

| Parámetro | Tipo | Predeterminado | Papel |
| --- | --- | --- | --- |
| `query` | texto, obligatorio | | El nombre que buscar, sin distinguir mayúsculas |
| `kind` | texto | todos los tipos | Restringe el tipo de resultado |
| `budget` | entero | 2000 | Presupuesto de tokens de la respuesta |

Los valores de `kind`:

| `kind` | Busca |
| --- | --- |
| `symbol` | un símbolo por el nombre |
| `contract` | un contrato por el identificador (`http::GET::/orders`) |
| `route` | un contrato HTTP |
| `resource` | un recurso declarado, por el tipo y por el id |
| `channel` | una cola o un tópico, por el broker y por el nombre |
| `document` | un archivo OpenAPI o AsyncAPI, por la ruta o por el título |
| `package` | un paquete por el nombre, con sus consumidores |
| `drift` | campos que los consumidores leen y una ruta no devuelve, por la ruta o por la ruta del campo |
| `community` | una comunidad por el nombre |
| `process` | un proceso por la etiqueta del punto de entrada, con la identidad `process::` desde la que parte `trace` |
| `duplicate` | símbolos con texto idéntico en dos o más lugares, por el nombre o por la ruta |
| `value` | un valor de configuración de un archivo YAML o JSON, por el valor o por la ruta de la clave |
| `iac` | un nodo IaC (un resource, un bloque data o una llamada de módulo de Terraform, o un objeto Kubernetes), por la dirección, el tipo o el nombre físico |
| `ambiguous`, `unresolved` | los ítems abiertos, con el comando `meada decide` de cada uno |

Cada resultado es una línea: repositorio, `ruta:línea` y nombre.

Sin `kind`, la búsqueda responde con símbolos y contratos; solo cuando ninguno coincide responde
con valores de configuración. Con `kind = "value"`, cada línea es `<repositorio> <ruta>:<línea>
<ruta de la clave> = <valor>`, un texto entre comillas como en JSON:

```text
example.com/acme/app chart/values-prd.yaml:6 alerts.loki[0].labels.support = "team-b"
example.com/acme/app deploy/app.yaml:26 [1].data.password = redacted
```

La ruta de la clave sigue el estilo de jq: claves unidas por puntos, un elemento de lista `[n]`
desde 0, una clave que no es un identificador simple entre `["..."]`
(`metadata.labels["app.kubernetes.io/name"]`) y, en un archivo de varios documentos, el índice del
documento primero (`[2].spec.replicas`). La consulta, en minúsculas, coincide con el valor o con
la ruta de la clave: iguales primero, luego los que empiezan con ella, luego los que la
contienen. Un valor de más de 1 KiB se guarda cortado y aparece seguido de `(cut)`.

Los iguales y los que empiezan con la consulta salen de B-trees sobre la ruta de la clave y el
valor en minúsculas; los que la contienen, de una tabla FTS5 con el tokenizador de trigramas,
cuando la consulta tiene tres caracteres o más (una consulta menor recorre las filas, con la misma
respuesta). Ese índice cuesta espacio: guarda cerca de cuatro veces el texto de las rutas de clave
y de los valores (en un repositorio de 92 mil valores y 5 MB de texto, 21 MB de
`.meada/index.db`), además de lo que el índice ya tenía.

Con `kind = "iac"`, cada línea es un nodo IaC: `<repositorio> <ruta>:<línea> <dirección> (<tipo>,
name "<nombre físico>", root <módulo raíz>)`, sin el `name` cuando el bloque no tiene atributo
`name`, y con `pattern` en su lugar cuando una parte del nombre no se resuelve (`pattern
"orders-*"`). La búsqueda sin `kind` no responde con nodos IaC.

```text
example.com/acme/infra envs/prod/main.tf:35 aws_sqs_queue.dlq[0] (aws_sqs_queue, name "dead-letters", root envs/prod)
example.com/acme/infra modules/queue/main.tf:9 module.orders.aws_sqs_queue.this (aws_sqs_queue, name "orders", root envs/prod)
```

#### `context`: entender un símbolo

| Parámetro | Tipo | Predeterminado | Papel |
| --- | --- | --- | --- |
| `symbol` | texto, obligatorio | | Un nombre, `<ruta>::<nombre>` o la identidad completa; un recurso (`resource::postgres::orders`), un canal (`channel::kafka::orders.created`) o la dirección de un nodo IaC (`aws_sqs_queue.orders`, `<ruta>::<dirección>`) |
| `budget` | entero | 2000 | Presupuesto de tokens de la respuesta |

Devuelve la comunidad y los procesos del símbolo, el fragmento de código leído del disco, quién lo
llama (`callers:`), a quién llama (`callees:`) y los contratos que toca (`contracts:`). En un
recurso, lista quién escribe y quién lee; en un canal, los productores y los consumidores. En un
nodo IaC, su tipo, su nombre físico (seguido de `(pattern)` cuando una parte no se resuelve), su
módulo raíz, las variables de la raíz por las que pasó el nombre (`overridable:`, cada una con
`(default)` o `(tfvars)`), los nodos que referencia (`references:`) y
los que lo referencian (`referenced by:`), cada uno con el nombre por el que pasó (`via
var.queue_arn`), y los valores de configuración de su bloque (`values:`). Una dirección que
tienen varios módulos raíz lista los nodos, cada uno con lo que pasar: `<dirección> (root <módulo
raíz>)`. Un símbolo del código con el mismo nombre va antes del nodo; `<ruta>::<dirección>` llega
al nodo.

```text
example.com/acme/web src/status.ts:2-4 health
community: src#3
processes: 1 (1 export; 1 repository): dashboard (example.com/acme/web)
function health() {
  return fetch("/api/health");
}
callers:
example.com/acme/web src/dashboard.ts:5 dashboard
callees:
example.com/acme/web src/status.ts:3 (unresolved) fetch
contracts:
example.com/acme/web src/status.ts:3 consumes http::GET::/api/health →[exact: host api.example.com is served by example.com/acme/api] example.com/acme/api::src/server.ts::GET /api/health
```

#### `trace`: seguir una cadena

| Parámetro | Tipo | Predeterminado | Papel |
| --- | --- | --- | --- |
| `from` | texto | | El inicio: un símbolo, una ruta (`GET /users/{}` o el contrato), un call site, la dirección de un nodo IaC o un proceso (`process::<identidad>`, solo río abajo) |
| `changes` | texto | | `uncommitted`: parte de los símbolos que toca el diff del árbol de trabajo |
| `direction` | `down` o `up` | `down` | `down` es lo que el inicio alcanza, `up` es lo que lo alcanza |
| `budget` | entero | 2000 | Presupuesto de tokens de la respuesta |

Pase `from` o `changes: uncommitted`, nunca los dos. El segundo sirve para medir el impacto de lo
que acaba de editarse, y solo funciona en el Meada local (el modo servidor no tiene árbol de
trabajo). Desde un nodo IaC, `down` sigue sus referencias y `up` las que llegan a él, dentro de su
módulo raíz, entrando en los módulos locales por sus entradas y salidas.

```text
example.com/acme/web src/orders.ts:5 listOrders
  example.com/acme/web src/api/client.ts:4 request
  →[inferred: base URL from API_URL, a base_url_env of peer example.com/acme/api] example.com/acme/api src/orders.ts:5 listOrders
chain: inferred · 1 hops · 2 repos · 93 tokens
```

(Ejemplo ilustrativo: el pie escribe `hops` incluso con un solo salto, y la cuenta de tokens varía
con la respuesta.)

#### Cómo leer los niveles

Cada enlace entre repositorios trae `→[nivel: motivo]`. El nivel dice cuán firme es el enlace:

| Nivel | Significado |
| --- | --- |
| `exact` | cierto: el código o la configuración prueba el enlace |
| `declared` | viene de una declaración suya en `meada.toml`, como un recurso compartido |
| `inferred` | una coincidencia por nombre o por convención |
| `decided` | viene de `meada.decisions.toml`; el motivo dice quién decidió |
| `ambiguous` | varios candidatos sirven, y Meada los lista todos sin elegir |

`trace` termina con un pie: el nivel más débil de la cadena, el número de saltos, los
repositorios involucrados y los tokens usados. Una cadena vale tanto como su eslabón más débil.
Un caller que falta es "no probado", no "inexistente": las llamadas dentro de macros (como
`assert!` y `format!` en Rust) no se indexan.

## 3. El workspace y el primer uso

El **workspace** es el conjunto de repositorios que una consulta ve. Reúne el repositorio donde
corre Meada, los pares que su `meada.toml` nombra (y los pares de estos, transitivamente) y los
clones git hermanos (en la misma carpeta que un repositorio del conjunto) cuyo propio
`meada.toml` nombra un repositorio que ya está en él. El descubrimiento lee solo archivos
locales. Un repositorio se identifica por el remoto git de `origin`, sin esquema ni `.git`
(`example.com/acme/api`), así que el clon puede estar en otra ruta en cada máquina.

En la raíz de `web`:

```sh
meada index
meada status
```

- `meada index` monta el índice de cada repositorio del workspace, en `.meada/` dentro de él. Es
  opcional antes de consultar, pues toda consulta actualiza el índice, pero deja rápida la
  primera respuesta.
- `meada status` muestra cada repositorio, el estado del índice y los pares que no se
  encontraron (`missing peer: example.com/acme/api, named by example.com/acme/web`).

Para que `web` vea a `api`, el `meada.toml` de `web` declara el par (sección 4). La indexación
solo escribe dentro de `.meada/`, que Meada mantiene fuera de git; el único otro archivo que
graba es `meada.decisions.toml`, y solo cuando usted ejecuta `meada decide`.

### Qué se indexa

El recorrido cubre todos los archivos del repositorio, versionados o no, excepto:

- **Ocultos**: los archivos y directorios cuyo nombre empieza con `.` no se leen, salvo la
  configuración de CI en la raíz (`.github/`, `.gitlab-ci.yml`, `.gitlab/`, `.circleci/`,
  `.buildkite/` y `.drone.yml`), cuyos archivos YAML y JSON se vuelven valores de configuración.
- **Siempre ignorados**: los directorios `node_modules` y `Pods`, un `Checkouts` dentro de
  `Carthage`, los archivos `*.min.js` y `*.min.css` y la salida de build de .NET (`obj` o `bin`
  junto a un `.csproj`).
- **`.gitignore` y `.ignore`**, respetados con o sin git, en la raíz y en subdirectorios.
- **`.meadaignore`**, con la sintaxis de `.gitignore`. No devuelve lo que la lista de siempre
  ignorados de arriba excluye; en cambio, un patrón `!` suyo reincluye lo que `.gitignore`
  ignora.

`vendor`, `dist`, `build` y el código generado no se ignoran por defecto, porque cualquiera
puede ser código propio. Liste en `.meadaignore` lo que no quiera indexar:

```gitignore
vendor/
dist/
Migrations/
*.Designer.cs
```

Un archivo que una nueva regla excluye sale del índice en el próximo refresh, como un archivo
borrado.

### Valores de configuración

Cada escalar (un texto, un número o un booleano; un nulo queda fuera) de cada archivo `*.yaml`,
`*.yml` y `*.json` que el recorrido lee, y de los archivos YAML y JSON de la configuración de CI
de arriba, es un **valor de configuración**: archivo, ruta de la clave, valor, tipo y línea. No
hay clave para activarlo: la búsqueda con `kind = "value"` los encuentra, y la búsqueda sin
`kind` responde con ellos cuando nada más coincide. No entran en `meada export`.

- **Lockfiles** (`package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml` y
  `packages.lock.json`) y los archivos que no se leen (una plantilla Helm, que no es YAML hasta
  renderizarse; un archivo de más de 1 MiB; un archivo de más de 1.000 documentos) quedan fuera, y
  `meada status` los cuenta:
  `archivos de configuración: 5 leídos, 2 omitidos (lockfiles: 1, ilegibles: 1); valores: 46, ocultados: 3`.
- **Terraform y OpenTofu**: los archivos `*.tf`, `*.tofu`, `*.tfvars`, `*.auto.tfvars`,
  `*.tf.json`, `*.tofu.json` y `*.tfvars.json` también son valores de configuración. La ruta de
  la clave pasa por el tipo y las etiquetas del bloque y por el nombre del atributo
  (`resource.aws_sqs_queue.orders.name`), y un bloque repetido con el mismo tipo y las mismas
  etiquetas es `[n]` (`ingress[0].from_port`). Un literal conserva su tipo; una expresión
  (`var.environment`) o una plantilla (`"${var.environment}-orders"`, un heredoc con
  interpolación) se guarda tal como está escrita, con el tipo `expression` o `template`. Un
  archivo `.tofu` oculta el archivo `.tf` del mismo nombre en su carpeta (y el `.tofu.json`, el
  `.tf.json`), y el archivo oculto no se lee. Un archivo que no se lee, o que anida corchetes más
  allá de 64 niveles, se omite y se cuenta como los demás; `.terraform/` es oculto y queda fuera.
- **Secretos**: los valores de `data` y `stringData` de un `Secret` de Kubernetes (un documento
  con `apiVersion` y `kind: Secret`, o un elemento así de una lista `items`), el `default` de una
  `variable` de Terraform con `sensitive = true` y todo valor que un archivo `.tfvars` da a una
  variable de ese nombre (todo valor, cuando un archivo Terraform en su carpeta o por encima no se
  lee) se guardan como `redacted`, con su ruta de la clave, su tipo y su línea. Nada más se filtra: el índice guarda lo
  que el checkout ya tiene. Pon en `.meadaignore` los archivos que no deben entrar en el índice
  (`secrets/`, `*.env.json`).

### Nodos IaC

Cada `resource`, bloque `data` y llamada `module` de los archivos `*.tf` y `*.tofu` es un **nodo
IaC**, con dirección, tipo, archivo, líneas y el módulo raíz que empieza su evaluación. Una
carpeta de archivos Terraform es un módulo; una carpeta que ninguna otra llama como módulo local
(un `source` `./` o `../` que queda dentro del repositorio) es un módulo raíz, etiquetado por su
ruta (`.` en la raíz).

- **Direcciones**: `<tipo>.<nombre>`, `data.<tipo>.<nombre>` y `module.<nombre>`. Cada llamada de
  un módulo local instancia sus bloques con la dirección de la llamada delante
  (`module.orders.aws_sqs_queue.this`); un `count` o un `for_each` sobre valores que se
  resuelven en literales da un nodo por instancia, con `[clave]` al final (`aws_sqs_queue.dlq[0]`,
  `aws_cloudwatch_metric_alarm.depth["orders"]`). Un `count` o `for_each` que no se resuelve
  (abajo) da un solo nodo, sin clave.
- **Evaluación estática**: el nombre físico (el argumento `name`) y cada `count` y `for_each` se
  evalúan como lo haría Terraform, sin ejecutarlo ni acceder a la red: los defaults de las
  variables, los valores del `terraform.tfvars`, de los `*.auto.tfvars` y de sus formas JSON de la
  raíz (en ese orden, ganando el archivo siguiente), los locals, las entradas y salidas de un módulo
  local por llamada, `each`, `count.index`, plantillas y las funciones puras `format`, `formatlist`,
  `join`, `split`, `lower`, `upper`, `title`, `trim`, `trimprefix`, `trimsuffix`, `trimspace`,
  `chomp`, `replace` (una subcadena literal), `substr`, `concat`, `coalesce`, `element`, `keys`,
  `values`, `lookup`, `merge`, `length`, `toset`, `tolist`, `tomap`, `tostring`, `tonumber` y
  `jsonencode`. Data sources, atributos de resources, módulos del registro, `terraform.workspace`,
  `-var`, `-var-file`, `TF_VAR_`, variables sensibles y cualquier otra función nunca se evalúan: esa
  parte del nombre es un `*` y el nombre un **patrón** (`"orders-${terraform.workspace}"` es
  `orders-*`), que por ahora no enlaza nada. Un nombre que pasó por una variable del módulo raíz la
  lista en `overridable:`, ya que quien despliega puede pasar otro valor. Un archivo Terraform o de
  variables cambiado evalúa de nuevo solo los módulos raíz cuya unidad (la raíz y los módulos
  locales que llama) lo contiene.
- **Referencias**: una expresión que nombra un resource, un bloque data o un módulo es una
  referencia de su nodo, guardada como una llamada. Una que pasa por `var.<nombre>`,
  `local.<nombre>` o la salida de un módulo (`module.<nombre>.<salida>`) se sigue hasta lo que
  nombra el argumento de la llamada, el local o la salida, y muestra por dónde pasó (`via
  var.queue_arn`). Un módulo del registro queda como el nodo de la llamada, sin nada dentro.
- **Consultas**: `search` con `kind = "iac"` encuentra los nodos, `context` explica uno y `trace`
  sigue sus referencias en los dos sentidos. Los nodos quedan fuera de la búsqueda sin `kind` y de
  `meada export`.

### Objetos Kubernetes y overlays de Kustomize

Todo documento YAML o JSON con `apiVersion`, `kind` y `metadata.name` también es un nodo IaC,
con dirección `<Kind>[.<grupo>]/<namespace>/<nombre>` (`Deployment.apps/shop/api`,
`Service/shop/api`), sin namespace para un kind de alcance de clúster
(`ClusterRole.rbac.authorization.k8s.io/reader`) o para un objeto que no define ninguno.

- **Overlays**: una carpeta con `kustomization.yaml` que ningún otro kustomization nombra como
  base es un overlay y una unidad de evaluación. Sus `resources`, `bases` y `components` dentro
  del repositorio se instancian bajo ella, cada nodo etiquetado por el overlay (`root
  overlays/dev`), con el `namePrefix`, el `nameSuffix`, el `namespace` y las `images` de cada
  kustomization en el camino aplicados. Un `..` que queda dentro del repositorio se sigue
  (`../../base`); una base remota, por encima de la raíz, absoluta, ausente, por un symlink, en
  ciclo o anidada más allá de 16 niveles no se sigue, y `meada status` la cuenta (`entradas de
  kustomize no seguidas: N`). Los manifiestos que ningún kustomization lista son simples: los
  manifiestos de una carpeta son una unidad.
- **Referencias**: un Service a las cargas de trabajo cuyas etiquetas del pod contienen su
  selector (`via selector app=api`), una carga de trabajo a los ConfigMaps y Secrets de `env`,
  `envFrom` y volúmenes (`via env DATABASE_URL`) y a su `serviceAccountName`, un Ingress a los
  Services de sus reglas (`via backend shop.example.com/api`), todo en un namespace y una
  unidad, por el nombre como se escribió. `context` en una carga de trabajo añade `images:` con
  lo que cada contenedor ejecuta una vez que el overlay aplicó las `images`.
- **No leídos**: patches, generadores, `commonLabels`, plantillas Helm y `kind: List`.

### Compose

Un archivo Compose (`compose.yaml`, `compose.yml`, `docker-compose.yaml` o
`docker-compose.yml`) con su archivo de override (`compose.override.*`,
`docker-compose.override.*`) en la misma carpeta se lee como un proyecto, con raíz en la carpeta.
Cada servicio es un nodo IaC `services.<nombre>` de tipo `service`.

- **Referencias**: `depends_on`, `links` y los servicios que comparten una red (`networks:`; un servicio sin ninguna está en `default`),
  mostradas con `via depends_on`, `via links` o `via networks.<nombre>`. Un nombre que ningún
  servicio tiene se descarta.
- **Variables**: `${VAR:-valor}` se resuelve a su valor por defecto y `context` lo marca
  `(overridable)`; `${VAR}` queda en el texto como parte sin resolver, marcada `(unresolved)`.
- **Imagen y build**: `context` muestra `image:` y `build:` del servicio, guardados para el
  contrato de imagen. Un contexto de build se guarda como ruta dentro del repositorio, desde la
  raíz; una ruta absoluta, una URL, una que sale de la raíz o una con una parte sin resolver no.
- **Consultas**: como en los nodos Terraform, `search` con `kind = "iac"`, `context` y `trace`.

### CloudFormation y SAM

Una plantilla YAML (las etiquetas cortas `!Ref`, `!Sub`, `!GetAtt`, `!Join` y las demás se leen) o
JSON con un mapa `Resources` que lleve `AWSTemplateFormatVersion` o la transformación SAM
(`AWS::Serverless-2016-10-31`) se lee como una unidad de evaluación, con raíz en su ruta. Cada
recurso es un nodo IaC direccionado por su id lógico, y su tipo (`AWS::SQS::Queue`,
`AWS::Serverless::Function`) es el tipo del nodo.

- **Referencias**: `Ref`, `Fn::GetAtt` (con el atributo en el `via`, `via Fn::GetAtt.Arn`), las
  variables de `Fn::Sub` y `DependsOn`.
- **Nombres**: el nombre físico de cola, tema, bucket, tabla, función, stream, secreto y algunos
  tipos más se evalúa dentro de la plantilla a partir del valor por defecto de un parámetro
  (`context` lo lista en `overridable:`), `Ref`, `Fn::Sub`, `Fn::GetAtt` y `Fn::Join`. Un pseudo
  parámetro (`AWS::Region`), un import y cualquier otra función son partes sin resolver: el
  nombre pasa a ser un patrón, mostrado `pattern "assets-prod-*"`.
- **Secretos**: un parámetro con `NoEcho` tiene su valor por defecto guardado como `redacted` y
  ningún nombre pasa por él.
- **Consultas**: como en los nodos Terraform, `search` con `kind = "iac"`, `context` y `trace`.
- **No seguidos**: stacks anidados, macros, condiciones y mapeos.

## 4. `meada.toml`: referencia

Cada repositorio puede tener un `meada.toml` en la raíz, versionado en git, que dice con quién
conversa. El archivo es opcional: sin él, el repositorio no declara ningún par. Hay tres tipos de
tabla, `[[peer]]`, `[serve]` y `[[resource]]`, y ninguna otra. Una clave desconocida o mal
escrita es un error que nombra el archivo, la línea y la clave.

```toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]

[serve]
hosts = ["web.example.com"]

[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

### `[[peer]]`: de quién consume este repositorio

Repetible, una tabla por par. Quien consume una ruta nombra al par.

| Clave | Tipo | Obligatoria | Papel |
| --- | --- | --- | --- |
| `repo` | texto | sí | La identidad del par: el remoto git de `origin`, sin esquema ni `.git` |
| `path` | texto | no | Pista de dónde suele estar el clon, relativa a la raíz de este repositorio |
| `base_url_env` | lista de textos | no (vacía) | Variables de donde este repositorio lee la URL base del par |

```toml
[[peer]]
repo = "example.com/acme/api"
path = "../api"
base_url_env = ["API_URL"]
```

Meada nunca lee el valor de las variables, solo los nombres. Cuando el código arma una URL a
partir de `API_URL`, el par que declara `API_URL` en `base_url_env` es el proveedor, con el nivel
`inferred` y el motivo `base URL from API_URL, a base_url_env of peer example.com/acme/api`.

### `[serve]`: qué sirve este repositorio

Una sola tabla. Quien sirve rutas declara los hosts en que responde y cómo un gateway reescribe
las rutas.

| Clave | Tipo | Obligatoria | Papel |
| --- | --- | --- | --- |
| `hosts` | lista de textos | no (vacía) | Los hosts en que responde este repositorio |
| `prefix` | texto | no | Lo que un gateway antepone a toda ruta servida |
| `strip` | texto | no | Lo que un gateway quita del comienzo de toda ruta servida |
| `documents` | lista de textos | no (vacía) | Documentos OpenAPI y AsyncAPI además de los hallados por el nombre del archivo |

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"
strip = "/v1"
documents = ["docs/openapi.yaml"]
```

- **`hosts`**: una llamada a `https://api.example.com/...` enlaza con el repositorio que lista
  ese host, en el nivel `exact`.
- **`prefix` y `strip`**: deben empezar con `/`. Meada normaliza el valor y quita la barra final;
  `/` solo no reescribe nada. Con los dos de arriba, una llamada a `/shop/orders` llega a la
  ruta `/v1/orders` del código, en el nivel `inferred`, con el motivo
  `gateway prefix /shop, gateway strip /v1`. `strip` casa segmentos completos (`/v1` no quita
  `/v10`), y un segmento de versión solo sale cuando usted lo escribe. Quien llama a la ruta del
  propio código sigue enlazando como antes.
- **`documents`**: rutas relativas a la raíz del repositorio, sin `..` y sin ruta absoluta.
  Meada ya halla documentos OpenAPI y AsyncAPI por el nombre del archivo; use la clave para los
  que tienen otro nombre. Un documento listado se lee aunque `.gitignore` lo ignore.
- La sola presencia de la tabla `[serve]`, aunque esté vacía, hace que los documentos del
  repositorio cuenten como fuente de proveedores.

### `[[resource]]`: recursos compartidos

Repetible. Un recurso que ningún código nombra, como una tabla o un bucket, entra aquí.

| Clave | Tipo | Obligatoria | Papel |
| --- | --- | --- | --- |
| `kind` | texto | sí | El tipo, un identificador en minúsculas (letras, dígitos y guiones, empezando por letra) sin significado para Meada: `postgres`, `s3-bucket` |
| `id` | texto | sí | El id del recurso, no vacío |
| `role` | `read`, `write` o `read-write` | sí | Qué hace este repositorio con él |
| `via` | lista de textos | no (vacía) | Los símbolos que hacen el acceso, como `<ruta>::<nombre calificado>` |

```toml
[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]
```

Cada escritor enlaza con cada lector del mismo recurso en otro repositorio, en el nivel
`declared`. Sin `via`, o si ningún `via` corresponde a un símbolo, el lado es el repositorio
entero. `meada status` muestra, bajo cada repositorio, los recursos sin enlace y los `via` que no
corresponden a ningún símbolo. La identidad del recurso en `search` y en `context` es
`resource::postgres::orders`.

### Un ejemplo completo: `web`, `api` y `worker`

`web` consume la API:

```toml
# web/meada.toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]
```

`api` sirve rutas detrás de un gateway y escribe en una tabla:

```toml
# api/meada.toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"
strip = "/v1"

[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]
```

`worker` consume `api` y lee la misma tabla:

```toml
# worker/meada.toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]

[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

Al ejecutar `meada status` en `web`, Meada halla `api` por el remoto git (o por el `path`, si lo
hay), y `worker` entra como clon hermano de `web` o de `api`, porque su `meada.toml` nombra a
`api`, que ya está en el workspace. `worker` lee la tabla `orders` que `api` escribe, y Meada une
los dos por el recurso, en el nivel `declared`.

### En modo servidor

En modo servidor, un espejo sin `meada.toml` recibe los mismos bloques `serve`, `resource` y
`peer` (este sin `path`) por el manifiesto `meada.workspace.toml`. El `meada.toml` del propio
repositorio, cuando existe, vale entero y los bloques del manifiesto se ignoran. Vea
[Modo servidor](https://claudiomascaro.github.io/meada/es/server/).

## 5. Decisiones y decisores

Un **enlace ambiguo** (varios proveedores sirven la ruta) o una **URL dinámica** (la llamada arma
la URL en tiempo de ejecución) queda como ítem abierto. Meada no adivina: lista los candidatos y
espera una decisión. Decide usted, o un modelo, si usted lo pide.

Para ver los ítems abiertos:

- en MCP, `search` con `kind: ambiguous` o `kind: unresolved` lista cada ítem con el número de
  candidatos;
- en la CLI, `meada decide --list` lista los ítems con los candidatos numerados.

### Decidir como persona

`meada decide`, en la raíz del repositorio, muestra cada ítem abierto con los candidatos
numerados. Elija un número, o `s` para saltar. Al final, el comando graba las elecciones en
`meada.decisions.toml`; si usted no eligió nada, no graba.

```text
example.com/acme/web::src/status.ts::health::fetch#0
src/status.ts:3  fetch("/api/health")
  1) example.com/acme/api GET /api/health
  2) example.com/acme/worker GET /api/health
```

### El archivo `meada.decisions.toml`

Está en la raíz, se versiona en git como un lockfile, y su diff es lo que se revisa. La
indexación solo lo relee: ninguna decisión pasa por un modelo ni por la red en ese momento.

```toml
[[decision]]
kind = "ambiguous-link"
call = "example.com/acme/web::src/status.ts::health::fetch#0"
target = "example.com/acme/api::http::GET::/api/health"
by = "human"

[[decision]]
kind = "dynamic-url"
call = "example.com/acme/web::src/status.ts::track::fetch#0"
target = "example.com/acme/worker::http::POST::/api/events"
by = "example/model"
p = 0.62
```

| Clave | Tipo | Obligatoria | Papel |
| --- | --- | --- | --- |
| `kind` | `ambiguous-link` o `dynamic-url` | sí | El tipo del ítem: varios proveedores, o una URL que Meada no evalúa |
| `call` | texto | sí | El call site, como lo nombran `meada export` y `meada decide --list` |
| `target` | texto | sí | `<identidad del repositorio proveedor>::<identificador del contrato>` |
| `by` | texto | sí | `human`, o el id del modelo |
| `p` | número de 0 a 1 | solo para un modelo | La probabilidad que el modelo declaró; prohibida con `by = "human"` |

El archivo tiene una entrada por `call`: repetir una llamada es un error que nombra la línea. El
enlace decidido recibe el nivel `decided`, y el motivo muestra quién decidió.

Cuando el código cambia y una decisión deja de encajar (la llamada desapareció, el ítem ya no
está abierto, el proveedor desapareció o ya no es candidato), `meada status` la muestra como
**obsoleta** bajo el repositorio, y `meada decide --prune` la quita. Una decisión que espera un
par aún no encontrado aparece aparte (`decision waiting on missing peer ...`) y se conserva.

### Decisor por modelo

`meada decide --decider` cambia la persona por un modelo. Es el único camino de Meada hacia un
modelo de lenguaje, y solo corre cuando usted lo pide con esa flag. Ninguna otra parte de Meada,
indexación y consulta incluidas, usa LLM ni red.

#### Variables de entorno

El decisor se configura solo con variables de entorno (nada viene de un archivo):

| Variable | Papel | Valor por defecto |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` o `chat` | `decisions` |
| `MEADA_DECIDER_URL` | URL base | `https://openrouter.ai` (`decisions`); obligatoria con `chat` |
| `MEADA_DECIDER_KEY` | clave de la API, enviada solo en el encabezado `Authorization` | obligatoria con `decisions`; opcional con `chat` |
| `MEADA_DECIDER_MODEL` | modelo | `typesafe/jev-1.14` (`decisions`); obligatoria con `chat` |

#### Protocolos

- **`decisions`** (el predeterminado) usa la API de decisiones de OpenRouter con el modelo Jev.
  Una única solicitud lleva todos los ítems, y la respuesta trae la probabilidad de cada
  candidato. La solicitud pide cero retención de datos (`zdr: true`, `data_collection: deny`); si
  OpenRouter rechaza esa preferencia, la solicitud falla y no se graba nada.

  ```sh
  export MEADA_DECIDER_KEY=...
  meada decide --decider
  ```

- **`chat`** habla con cualquier endpoint compatible con la API de chat de OpenAI
  (`<URL>/chat/completions`), incluso un modelo local, por ejemplo con Ollama. Una solicitud por
  ítem. La probabilidad es la que el propio modelo dice de sí, no una medida calibrada.

  ```sh
  MEADA_DECIDER_PROTOCOL=chat \
  MEADA_DECIDER_URL=http://localhost:11434/v1 \
  MEADA_DECIDER_MODEL=<modelo> \
  meada decide --decider
  ```

Sirve cualquier gateway que hable uno de los dos protocolos: basta cambiar la URL. Se exige HTTPS
en cualquier lugar; HTTP solo hacia `localhost` o una dirección de loopback.

#### Qué se envía

Por ítem, y nada más que esto:

- la llamada (el fragmento de código del call site);
- la función que la contiene, leída del disco (sin ella, cuando la llamada está al tope del
  archivo o el símbolo que la envuelve no es una función);
- la lista de candidatos (repositorio, método y ruta);
- la identidad del repositorio que hace la llamada, el id del modelo y una frase fija que
  describe la tarea.

Ningún código del proveedor y ningún otro archivo salen de la máquina. Para ver exactamente qué
se enviaría, ejecute `meada decide --decider --show-request`: imprime cada solicitud, con la clave
cambiada por `<hidden>`, y no envía ninguna.

#### Privacidad

- Sin `--decider`, ningún código sale de la máquina. Indexar, consultar por MCP y decidir como
  persona no usan red. Dos cosas escapan a esto y no son decisión: `npx @meada/meada` descarga el
  binario, y `mcp --listen` abre un servidor HTTP (vea el modo servidor).
- Con `--decider`, el código de arriba sale hacia el endpoint que usted configuró. Para mantener
  todo en la máquina, use el protocolo `chat` con un modelo local (`http://localhost:...`).
- La clave solo viaja en el encabezado `Authorization` y nunca se imprime.
- Cada respuesta se valida por ítem. Una respuesta inválida o una solicitud que falló se informa
  sin grabar ese ítem, y el comando sale con error.
- Lo que el modelo propone solo llega a `meada.decisions.toml`, que es un archivo versionado: la
  decisión pasa por la revisión del diff como cualquier otro cambio.

#### Límites fijos

- `--min-p` (de 0 a 1) vale 0,5 por defecto: por debajo de él, la propuesta se imprime como
  sugerencia y no se graba.
- Como máximo 50 candidatos por ítem: con más, el ítem se salta en el modo decisor (el modo
  humano aún lo ofrece).
- 120 segundos por solicitud, desde la conexión hasta el último byte, sin nuevo intento.
- En el protocolo `decisions`, las probabilidades de una respuesta deben sumar 1, con tolerancia
  de 0,01.

En modo servidor, las decisiones se toman en un pipeline, no en el servidor; vea
[Decidir en un pipeline](https://claudiomascaro.github.io/meada/es/server/#decidir-en-un-pipeline).

## 6. Comandos de la CLI

```text
meada <COMANDO>
```

Los mensajes siguen el `LANG` (portugués, inglés o español); los de MCP son siempre en inglés. Toda
invocación acepta `--help` y `--version`.

### Los cinco comandos del build predeterminado

| Comando | Qué hace |
| --- | --- |
| `meada index` | Monta los índices del workspace del repositorio en el directorio actual |
| `meada status` | Muestra el workspace: cada repositorio, el estado del índice y los pares no encontrados |
| `meada export` | Imprime el índice como JSON ordenado, con los links que lo tocan |
| `meada mcp` | Sirve el repositorio del directorio actual a agentes por MCP (stdio) |
| `meada decide` | Resuelve los ítems abiertos de este repositorio y registra las elecciones en `meada.decisions.toml` |

#### `meada index`

```sh
meada index [--workspace <manifiesto>]
```

Actualiza el índice del repositorio actual y de los pares. Con `--workspace`, actualiza el de
todo repositorio que el manifiesto `meada.workspace.toml` lista (el refresh del operador, en modo
servidor).

#### `meada status`

```sh
meada status [--workspace <manifiesto>]
```

Muestra los repositorios del workspace y el estado de cada índice. Bajo cada repositorio, muestra
los recursos sin enlace, los `via` que no corresponden a ningún símbolo, las decisiones obsoletas
o a la espera de un par y cada archivo fuente que un language pack descartó por pasar de su
presupuesto. Los pares que no se encontraron aparecen como `missing peer`.

#### `meada export`

```sh
meada export [--workspace <manifiesto> --repo <identidad>]
```

Imprime el índice del repositorio actual en JSON ordenado (formato en la sección 7). Con
`--workspace`, la flag `--repo` es obligatoria y elige el repositorio del manifiesto; una exige la
otra.

#### `meada mcp`

```sh
meada mcp
meada mcp --listen <dirección> --workspace <manifiesto>
```

Sin flags, sirve por MCP en stdio (sección 2). Con `--listen`, sirve por HTTP en modo servidor,
en `host:puerto`, `:puerto` o `puerto` (loopback sin host); `--listen` y `--workspace` van
juntos.

#### `meada decide`

```sh
meada decide [--item <id>] [--dry-run] [--list] [--prune]
             [--decider [--show-request] [--min-p <p>]]
```

| Flag | Papel |
| --- | --- |
| `--item <id>` | Trata solo ese ítem (el id que imprimen `--list` o `search`), aunque ya esté decidido, para rehacerlo |
| `--dry-run` | Imprime el archivo que se grabaría y no escribe |
| `--list` | Imprime los ítems abiertos, las decisiones obsoletas y las que esperan un par ausente; no pregunta ni graba |
| `--prune` | Quita las decisiones obsoletas; conserva las que esperan un par ausente. No se combina con `--item`, `--dry-run` ni `--list` |
| `--decider` | Pregunta al modelo configurado por las variables `MEADA_DECIDER_*`, en vez de a una persona |
| `--show-request` | Solo con `--decider`: imprime cada solicitud que se enviaría y no envía ninguna |
| `--min-p <p>` | Solo con `--decider`: la probabilidad mínima, de 0 a 1, para grabar (predeterminado 0,5) |

### Comandos de los módulos de nube

Los builds con módulos opcionales (la imagen `-cloud` los trae todos) añaden comandos para operar
Meada como servicio de una organización. No forman parte del uso local y se describen en las
páginas de operación.

| Comando | Módulo | Papel |
| --- | --- | --- |
| `meada snapshot` | `snapshot` | El store de snapshots: `publish`, `pull`, `lock`, `pointer` y `fence` |
| `meada workspace` | `generation-pointer` | Las versiones del workspace en el store de snapshots |
| `meada workspaces` | `workspaces` | Los workspaces nombrados, con `mcp` en el módulo `proposals` |
| `meada ui` | `ui` | La consola de gestión |
| `meada github-app` | `github-app` | Credenciales de git a partir de tokens de GitHub App, para el job de indexación |
| `meada postgres` | `postgres` | La capa de datos: `migrate` crea el esquema o lo lleva a la versión del binario |

Las flags `--generations` de `status`, `export` y `mcp` existen solo en el módulo `snapshot`. Vea
[Modo servidor](https://claudiomascaro.github.io/meada/es/server/) y
[Módulos de nube](https://claudiomascaro.github.io/meada/es/cloud/).

## 7. El export

`meada export` imprime el índice de un repositorio como JSON, listo para `diff` entre dos
ejecuciones y para alimentar una base de conocimiento.

```sh
meada export > web.export.json
```

- **Determinista**: el mismo árbol de archivos produce los mismos bytes. Toda lista viene
  ordenada y el texto termina con un salto de línea.
- **Sin texto fuente**: quien necesita el fragmento lee el `span` en el archivo.
- **Un repositorio por export**: una organización ejecuta `meada export` en cada repositorio y
  une los resultados por la identidad de los símbolos.
- **Con el formato del export y el commit indexado**: `format` (1 hoy) solo sube cuando quien lee el
  formato anterior lo leería mal, así que quien lee debe ignorar los campos que no conoce: un campo
  opcional nuevo es un cambio menor. `commit` y `dirty` dicen de qué estado del repositorio se leyó
  el índice (ver [Uso](https://claudiomascaro.github.io/meada/es/cli/)).

Las recetas con `jq` (un registro por símbolo, duplicados, fan-in entre repositorios) están en el
[README](../README.es.md#alimentar-una-base-de-conocimiento).

### Campos del nivel superior

Las listas vacías se omiten, salvo `files`, `symbols`, `calls`, `providers`, `consumers` y
`links`, que siempre aparecen (como `repository`, que es un texto).

| Campo | Contenido |
| --- | --- |
| `format` | El formato del export, hoy 1 |
| `repository` | La identidad del repositorio (`example.com/acme/web`) |
| `commit`, `dirty` | El commit indexado y si el checkout difería de él; se omiten juntos fuera de git, antes del primer commit y cuando no se sabe nada fiable |
| `files` | Los archivos leídos: `path` y `hash` |
| `symbols` | Los símbolos: `id`, `span` (`path`, `start`, `end`, desplazamientos en bytes) y `hash` (BLAKE3 en hexadecimal de los bytes del span) |
| `calls` | Los call sites: `id`, `caller`, `target` y `level` (vacíos mientras no resuelve), `candidates` (si es ambigua), `span`, `narrowed` y `decorates` |
| `providers` | El lado proveedor de cada contrato: `contract`, `site`, y si los hay `gateway` y `shape` |
| `consumers` | El lado consumidor: `contract` (nulo si la URL no se evaluó), `site`, `via` (cómo nombra al repositorio del proveedor, como `host:api.example.com` o `env:API_URL`) y `shape` |
| `links` | Los enlaces: `consumer`, `provider`, `contract`, `level`, `reason`, y si los hay `by`, `p` y `drift` |
| `bindings`, `publishes` | Los bindings (`contract`, `exchange`, `pattern`, `site`) y las publicaciones (`exchange`, `key`, `site`) de RabbitMQ que el repositorio declara |
| `documents` | Los documentos OpenAPI y AsyncAPI leídos: `path`, `format`, `version` y `contracts` |
| `packages` | Los paquetes que el repositorio provee: `name`, `version`, `repository`, `directory` y `ecosystem` |
| `dependencies` | Las entradas de dependencia que nombran un paquete del workspace: `manifest`, `field`, `name`, y si los hay `alias` y `range` |
| `imports` | Los imports resueltos hacia un paquete del workspace: `site`, `specifier`, `name`, `level`, `how` y `target` |
| `communities` | Las comunidades: `name` y `members` (ordenados) |
| `processes` | Un proceso por punto de entrada: `entry_point`, `kind` (`route`, `export`, `channel` o `program`), `label` y `steps` |
| `stale_decisions` | Las decisiones que ya no se aplican: `call`, `target` y `cause` (`call-gone`, `item-not-open`, `target-gone` o `not-candidate`) |

### Valores que se repiten

- **`level`**: `exact`, `declared`, `inferred`, `decided` o `ambiguous`, del más firme al más
  débil.
- **Identificadores de contrato**: siempre empiezan por el tipo, `http::GET::/api/health`,
  `channel::kafka::orders.created`, `package::...` o `resource::postgres::orders`.
- **Identidad de símbolo**: `<repositorio>::<ruta>::<nombre>`, por ejemplo
  `example.com/acme/web::src/status.ts::health`. Un call site añade el destino y el índice dentro
  del símbolo: `...::health::fetch#0`.
- **`shape`** (en `providers` y `consumers`): las rutas de campos que una respuesta o un mensaje
  lleva, con `source` y `fields`, entre otras claves. El drift, en el `link`, lista en `missing`
  los campos que el consumidor lee y el proveedor no devuelve.
- **`steps`** de un proceso: `depth`, `repository`, `level`, `via` (`entry`, `call` o `link`),
  `node`, y si los hay `reason`, `mark`, `candidates` y `contract`.

### Ejemplo

```json
{
  "repository": "example.com/acme/web",
  "files": [{ "path": "src/status.ts", "hash": "d61eae8a..." }],
  "symbols": [
    {
      "id": "example.com/acme/web::src/status.ts::health",
      "span": { "path": "src/status.ts", "start": 31, "end": 99 },
      "hash": "5cc3a63c..."
    }
  ],
  "calls": [],
  "providers": [],
  "consumers": [],
  "links": []
}
```

(Fragmento ilustrativo: un export real trae todos los archivos, símbolos y enlaces del
repositorio.)

## 8. Tipos de contrato

Un **contrato** es lo que un repositorio expone y otro usa. Meada lee cinco tipos, y cada uno
tiene su propia regla de enlace. Esta sección dice qué une cada tipo y dónde se detiene, para que
usted distinga un enlace que falta por un límite conocido de un enlace que falta por un bug.

Toda identidad de contrato empieza por su tipo, y esa identidad es la que usted pasa a `context`
y a `trace`:

| Tipo | Identidad | Ejemplo |
| --- | --- | --- |
| Ruta HTTP | `http::<MÉTODO>::<ruta>` | `http::GET::/api/orders/{}` |
| Canal | `channel::<broker>::<nombre>` | `channel::kafka::orders.created` |
| Paquete | `package::<ecosistema>::<nombre>` | `package::npm::@example/shared` |
| Recurso | `resource::<tipo>::<id>` | `resource::postgres::orders` |

Los documentos OpenAPI y AsyncAPI no son un tipo aparte: aportan rutas y canales a los tipos de
arriba (sección 8.3).

Cómo saber si un enlace que falta es un bug:

- Si el código usa una biblioteca, un framework o una forma que las secciones 8 y 9 no listan, el
  enlace falta por un límite conocido, no por un bug.
- Si la biblioteca está en la lista y el código usa la forma descrita, el enlace debería existir.
  Ejecute `meada status` y `search` con `kind=unresolved` y `kind=ambiguous` para ver si Meada
  leyó el lado y no lo enlazó, y entonces abra un issue con la consulta, la respuesta y la
  esperada.
- Una llamada dentro de una macro de Rust no se indexa: un caller que falta ahí es "no probado".
- Un nombre que Meada no consigue calcular no genera un enlace por sí solo. Una URL armada en
  tiempo de ejecución es un consumidor no resuelto, que usted decide con `meada decide` (sección
  5). Un nombre de canal leído del entorno es un lado no resuelto, un límite conocido que ninguna
  decisión resuelve (sección 8.4).

### 8.1 Rutas HTTP

Una ruta une a quien la **sirve** (el proveedor, el código que la trata) con quien la **llama**
(el consumidor, una llamada de cliente HTTP). En todos los lenguajes, Meada normaliza la ruta:
`/users/:id`, `/users/{id}` y `/users/<int:id>` pasan a ser el mismo `/users/{}`, y el método
queda en mayúsculas. Las mayúsculas y minúsculas de la ruta y las versiones de API se mantienen.

Cómo se elige el enlace, según lo que muestra la URL:

1. **Host.** La URL tiene un host (`https://api.example.com/orders`) que algún repositorio lista
   en `[serve] hosts`. El enlace es `exact`. Si ningún repositorio sirve ese host, no hay
   enlace.
2. **Variable de entorno.** La URL empieza por una lectura de entorno (`API_URL`) que un par
   declara en `base_url_env`. El enlace es `inferred`, y Meada nunca lee el valor de la
   variable.
3. **Host desconocido.** La URL no dice de dónde viene (una ruta relativa, por ejemplo) y solo un
   repositorio del workspace sirve ese método y esa ruta. El enlace es `inferred`, con el motivo
   `the only repository serving it is`.
4. **Varios proveedores.** El enlace es `ambiguous`, y Meada lista todos los candidatos, un
   enlace por candidato. Es un ítem abierto que `meada decide` resuelve.

Un proveedor que sirve todos los métodos tiene el método `ANY` (`http::ANY::/health`) y enlaza
con consumidores de cualquier método, con `any method` en el motivo. Un consumidor cuya URL
Meada no consigue calcular, incluso después de plegar constantes y argumentos de funciones que
envuelven el cliente, es un **consumidor no resuelto**: aparece en `search`, en `context` y en
el export, nunca enlaza, y es un ítem abierto. Una función que arma la URL a partir de sus
parámetros pasa a ser un consumidor en cada llamada a ella, con los argumentos sustituidos, a
cualquier profundidad.

Por qué puede faltar un enlace:

- La ruta que declara un **proveedor** tiene una parte que Meada no lee (una interpolación, un
  parámetro, una lectura de entorno en medio de la ruta): la ruta no existe para Meada. Un
  prefijo ilegible no pasa a ser la raíz, deja la ruta sin prefijo alguno. (En el consumidor, un
  hueco en medio de la URL pasa a ser un parámetro `{}` y aún enlaza.)
- El framework o el cliente no está en la lista del lenguaje (sección 9).
- El repositorio que sirve la ruta no está entre los pares, y el workspace no lo tiene.

### 8.2 Recursos declarados y prefijos de gateway

**Recursos compartidos.** Lo que ningún código nombra, como una tabla o un bucket, usted lo
declara en `[[resource]]` (sección 4). Meada no lee código para este tipo: enlaza cada escritor
con cada lector del mismo `kind` e `id` en otro repositorio, en el nivel `declared`, con el
contrato como motivo.

```toml
# api/meada.toml
[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]

# worker/meada.toml
[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

Con `via`, el lado es el símbolo; sin `via`, o si ningún símbolo corresponde, el lado es el
repositorio entero y aparece como `(repository-level)`: un `trace` muestra el enlace, pero no
entra en ese lado. `meada status` lista, bajo cada repositorio, los recursos sin enlace y los
`via` sin símbolo. Como Meada no interpreta `kind`, nada garantiza que `postgres` y `postgresql`
sean el mismo recurso: escríbalo igual en los dos lados.

**Prefijos de gateway.** Cuando un gateway reescribe las rutas, el consumidor llama a una ruta
que el código del proveedor no tiene. Usted declara la reescritura en `[serve]` con `prefix` y
`strip` (sección 4), y Meada también acepta la ruta del servidor de cada URL de `servers` de un
documento OpenAPI como prefijo de ese documento. Una ruta enlaza también por su ruta de gateway,
y ese enlace es `inferred`, con el motivo `gateway prefix /shop`, `gateway strip /v1` o los dos.
Dentro de un repositorio, la ruta del código vence a la del gateway.

### 8.3 Documentos OpenAPI y AsyncAPI

Un repositorio con la tabla `[serve]` (aunque esté vacía) tiene sus documentos leídos como fuente
de proveedores. Meada lee **OpenAPI 3.0 y 3.1** y **AsyncAPI 2.x y 3.0**, en JSON o YAML,
hallados por el nombre del archivo o listados en `[serve] documents`. Sin `[serve]`, los
documentos se ignoran.

- **Rutas.** Cada ruta del documento es un proveedor. Si el código ya tiene una ruta con el mismo
  contrato, las dos pasan a ser un solo proveedor, con el punto del código como sitio. Una ruta
  que solo el documento lista aparece como `<documento> (documented only)` y ningún `trace`
  entra en ella. Un proveedor que viene de un documento es un contrato escrito: no rebaja el
  nivel (un host que casa sigue siendo `exact`).
- **Canales.** Cada canal de AsyncAPI pasa a ser un lado de productor o de consumidor, fusionado
  con el lado del código cuando el contrato es el mismo. En un documento, el nombre de un canal
  `amqp` es el de una cola, nunca el de una exchange.
- **Formato de los datos.** Lo que el documento dice del cuerpo de una respuesta o de un mensaje
  es la forma (sección 10) de ese lado. Solo la forma de una ruta entra en la comparación de
  drift; la de un mensaje se registra, pero no se calcula ningún drift para canales.
- **Hosts.** El host de cada URL de `servers` cuenta como un `[serve] hosts`.

`meada status` muestra, por repositorio, lo que se leyó y lo que se ignoró. Una parte de la forma
que Meada no consigue listar (un objeto que acepta propiedades extra, un esquema detrás de una
referencia externa) queda **abierta**: lo que esté debajo de ella puede existir. Cuando el
documento y el código discrepan sobre la forma de una ruta, vale la forma del documento.

### 8.4 Canales de mensajería

Un **canal** es una cola o un tópico de un broker. Cada productor enlaza con cada consumidor del
mismo canal, también dentro del mismo repositorio. El mensaje va del productor al handler, así
que `trace` desciende en esa dirección.

El broker forma parte de la identidad (`channel::<broker>::<nombre>`), y el mismo nombre en dos
brokers son dos canales:

| Broker | Valor en `<broker>` | Observación |
| --- | --- | --- |
| Kafka | `kafka` | |
| RabbitMQ | `amqp` | exchanges, colas y bindings |
| Amazon SQS | `sqs` | |
| NATS | `nats` | comodines `*` y `>` |
| Redis streams | `redis` | |
| BullMQ | `bullmq` | solo en TypeScript y JavaScript |
| JMS | `jms` | solo en Java y Kotlin (Spring JMS) |
| Azure Service Bus | `servicebus` | solo en C# |

Los clientes de cada lenguaje están en la sección 9. Swift no tiene lectura de canales en 1.0.

Reglas que valen para todos:

- **Nombre calculado como una URL.** Las constantes, los argumentos de funciones que envuelven el
  cliente y las interpolaciones simples se pliegan. Un nombre que no se calcula (leído del
  entorno, armado en tiempo de ejecución) es un **lado no resuelto**: contrato `channel::kafka`,
  mostrado como `(unresolved)`, presente en el export, nunca enlazado y nunca un ítem abierto. Es
  un límite conocido, y `meada decide` no lo ve.
- **RabbitMQ.** Un binding de cola a exchange (`bindQueue`, `QueueBind`, `queue_bind`) es una
  regla de enrutamiento, no un lado. Una publicación en una exchange nombrada llega a las colas
  cuyo binding casa con la clave de enrutamiento, venga de cualquier repositorio del workspace,
  en cualquier tipo de exchange: `exact` cuando el patrón es la clave, `inferred` con comodín.
  Una publicación en la exchange predeterminada (vacía) es productora de la cola que la clave
  nombra.
- **NATS.** Un consumidor con `*` (un token) o `>` (el resto) enlaza con los productores cuyo
  asunto casa, en el nivel `inferred`.
- **Los clusters no se modelan.** Un nombre de un tipo de broker es un canal en todo el
  workspace: tópicos del mismo nombre en clusters distintos enlazan como si fueran de un solo
  cluster. Si eso produce un enlace falso, es la limitación, no un bug.
- **El handler** de un consumidor es la función que la suscripción nombra.

### 8.5 Paquetes

Un **paquete** une a quien lo declara en un manifiesto (el proveedor) con quien lo lista como
dependencia (el consumidor), y va más allá: un `import` de una función del paquete llega al
código del proveedor, de modo que el `context` de una función exportada lista los llamadores de
todo el workspace y `trace` cruza el salto. Solo entran los paquetes que algún repositorio del
workspace provee; las dependencias de fuera del workspace no aparecen en ningún lugar, Meada no
consulta ningún registro y no lee lockfiles (`go.sum`, `Cargo.lock` y similares).

El motivo de un enlace de paquete trae el rango tal como está escrito y, cuando corresponde,
termina con `range unchecked` (el rango no se comprobó) o, en npm, `range mismatch` (el rango
rechaza la versión local del proveedor). En npm, un rango que acepta la versión local no lleva
sufijo. Un import que llega al código añade `; import <lo que se usó>` al motivo.

| Ecosistema | Manifiesto | Identidad | Observaciones |
| --- | --- | --- | --- |
| npm | `package.json` con `name` | `package::npm::<nombre>` | Lee `dependencies`, `devDependencies`, `peerDependencies` y `optionalDependencies`. El rango se comprueba (semver de npm) |
| PyPI | `pyproject.toml` con nombre en `[project]` o `[tool.poetry]` | `package::pypi::<nombre>` | Normalizado por la PEP 503. Lee también `requirements*.txt`. Los rangos no se comprueban |
| Cargo | `Cargo.toml` con `[package] name` | `package::cargo::<nombre>` | Cada miembro del workspace de Cargo es un proveedor. Los rangos no se comprueban |
| Go | `go.mod` con `module` | `package::go::<ruta del módulo>` | Lee `require` y `replace` hacia un directorio local |
| Maven | `pom.xml` con coordenadas literales, o proyecto Gradle con `group` y nombre literales | `package::maven::<grupo>:<artefacto>` | Cada módulo es un artefacto. Lee también `settings.gradle` y `settings.gradle.kts`. Los rangos no se comprueban |
| NuGet | `.csproj` de estilo SDK | `package::nuget::<id>` | El id es el `PackageId`, si no el `AssemblyName`, si no el nombre del archivo. Los rangos no se comprueban |
| SwiftPM | `Package.swift` | `package::swiftpm::<nombre>` | El nombre es el literal de `Package(name:)`. Los rangos no se comprueban |

Detalles por ecosistema:

- **npm.** Un `import` entra al código del proveedor por `exports`, `types`, `module`, `main` y
  reexportaciones. Si la entrada apunta a la salida de un build, el `outDir` de `tsconfig.json`
  lleva de vuelta al `rootDir`. Un paquete con el mismo nombre en npm y en PyPI no se enlaza. Un
  rango `workspace:` solo vale dentro del propio repositorio. `search` con `kind=package` informa
  nombre, repositorio, directorio, versión y consumidores.
- **PyPI.** El `import` llega al código como `exact` cuando el módulo coincide con el nombre de
  la distribución, `inferred` cuando es uno de sus módulos de nivel superior y `ambiguous`
  cuando varias distribuciones lo exponen.
- **Cargo.** Una dependencia por `path` resuelve el crate de ese directorio, en el mismo
  repositorio o en otro del workspace (la ruta se resuelve por el texto, sin salir de las raíces
  de los repositorios ni seguir enlaces simbólicos); por `git`, el repositorio de URL
  normalizada; por versión, el proveedor del mismo nombre. `package =` da el nombre real de una
  dependencia renombrada, y `workspace = true` lee `[workspace.dependencies]`. Un `use` sigue
  `[lib] name` o el nombre renombrado y los `pub use` hasta los ítems públicos.
- **Go.** Un `replace` que sale del repositorio se resuelve por el texto, como el `path` de
  Cargo, y una sustitución por otro módulo queda fuera. Un `import` de `<módulo>/<directorio>`
  llega a las funciones, tipos y métodos exportados del directorio.
- **Maven y Gradle.** Una dependencia, también en la notación de mapa de Gradle (`group: 'g',
  name: 'a'`), enlaza con el artefacto del workspace como `exact`, con `range unchecked`. Un grupo
  o artefacto escrito con una propiedad o una variable enlaza cuando el build declara el valor
  como literal, o cuando un `gradle.properties` (de la raíz, del directorio del settings o del
  módulo) lo define; un accesor de version catalog (`libs.x`) se lee del
  `gradle/libs.versions.toml` o del catálogo que nombra el settings. Si no, el enlace es
  `ambiguous`. Un `import` de una clase del artefacto llega al
  código del proveedor por la declaración `package` de los archivos, y no por una ruta (en Kotlin,
  también el de una función de nivel superior o de extensión): `exact` cuando un solo proveedor
  declara el paquete y el tipo, `ambiguous` cuando varios lo declaran o quedan sobrecargas con el
  mismo número de argumentos, `inferred` cuando un ancestro de la clase viene de fuera del
  repositorio del proveedor. El motivo termina en `; import package`.
- **NuGet.** Un `PackageReference` que nombra un paquete del workspace enlaza como `exact`, con
  `Version` como rango, y un `ProjectReference` a un proyecto de otro repositorio del workspace
  enlaza con el paquete de ese proyecto. Un `using` de un namespace de un proyecto del que el
  archivo depende llega a sus tipos: `exact` cuando un proveedor declara el namespace y el tipo,
  `ambiguous` cuando varios lo declaran, `inferred` cuando un ancestro viene de fuera del
  repositorio del proveedor; el motivo termina en `; import namespace`. Meada no consulta ningún
  feed ni lee `Directory.Packages.props`.
- **SwiftPM.** Un `.package(url:)` enlaza con el repositorio del workspace de la misma identidad
  de remoto, y un `.package(path:)` con el repositorio en esa ruta. Solo cuentan los valores
  literales: una URL, una ruta o un nombre calculado dejan el enlace `ambiguous`. Un `import
  Modulo` de un módulo de un paquete del que depende el `Package.swift` llega a las declaraciones
  `public` y `open`, elegidas por el nombre y por las etiquetas de los argumentos; el motivo
  termina en `; import module`. Los módulos de la plataforma (`Foundation`, `SwiftUI`) nunca son
  un enlace, y `Package.resolved` no se lee.

Cuando dos proveedores sirven el mismo nombre, el enlace es `ambiguous`, uno por candidato, con el
motivo iniciado por `<n> providers fit`.

## 9. Lenguajes

Meada 1.0 lee **TypeScript y JavaScript, Python, Go, Rust, Java, Kotlin, C# y Swift**, nueve
lenguajes. Cada sección de abajo lista los archivos leídos, los frameworks y clientes HTTP, las
lecturas de entorno, los manifiestos, los clientes de canales y los límites conocidos. Una
biblioteca fuera de estas listas no genera enlace.

Reglas que valen para todos los lenguajes:

- Meada lee el texto con tree-sitter. No ejecuta compilador, intérprete, gestor de paquetes ni
  red, y nunca lee el valor de una variable de entorno.
- Una llamada es `exact` cuando el código muestra el tipo del receptor o la ruta escrita, y
  `inferred` o `ambiguous` cuando solo el nombre decide (sección 2, "Cómo leer los niveles").
- Una biblioteca cuenta cuando el archivo la importa (el `fetch` global de JavaScript es la
  excepción).
- Un decorador, un atributo o una anotación escrito sobre una definición es una llamada del
  ámbito que lo envuelve. `context` los lista aparte, en `annotations:`, y no entre los llamados.
- Una URL o un nombre de canal se pliega: literales, concatenaciones, interpolaciones simples,
  constantes (también de otros archivos del repositorio) y los argumentos de funciones que
  envuelven el cliente, a cualquier profundidad. Lo que no se pliega es un consumidor no
  resuelto (HTTP) o un lado no resuelto (canales).

La promesa de estabilidad de 1.0 vale para los nueve lenguajes: congela la superficie
(herramientas, CLI, configuración y export), y resolver mejor lo que un lenguaje encuentra es una
corrección, no una ruptura.

### 9.1 TypeScript y JavaScript

**Archivos.** `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs` y `.cjs`. Un `import` usa los `paths` y el
`baseUrl` del `tsconfig.json` (o `jsconfig.json`) más próximo, en el directorio del archivo o
más arriba, relativos al directorio de ese archivo de configuración. Si tiene un `extends` a otro `tsconfig.json`
o `jsconfig.json` del repositorio (una ruta relativa), `paths` y `baseUrl` vienen de la cadena, y gana
el config más cercano que declara cada opción; un `extends` a un paquete o fuera del repositorio no se
lee, y una cadena demasiado larga o circular vale solo por el config más cercano. `node_modules` se ignora.

**Qué pasa a ser definición.** Funciones, clases y métodos, los `const`, `let` y `var` de una
función o clase y el `export default`. En el nivel superior de un archivo, las funciones
asignadas también cuentan: `obj.y = function () {}` define `obj.y`,
`X.prototype.y = function () {}` define el método `X.y`, `exports.y = ...` y
`module.exports.y = ...` definen la exportación `y`, y `module.exports = function () {}` define
la exportación predeterminada. Un `let` o `var` solo es definición si el archivo nunca vuelve a
asignar el nombre. Las funciones del objeto literal de un `const` de nivel superior definen
`o.m`.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `fetch` global, y una llamada a un parámetro cuyo valor predeterminado es `fetch` |
| Proveedor | Next.js (app router: archivos de ruta) y Express |

En Express, una ruta vale bajo cada prefijo en que se monta el router (`use`, `Router()`). El
`Router()` de `express-promise-router` (el predeterminado en los plugins de backend de Backstage)
vale como el `express.Router()`. Un prefijo o una ruta con una parte ilegible (un hueco de template literal, un parámetro, una
lectura de entorno) no genera ruta, y un router montado solo bajo un prefijo así queda sin
prefijo alguno. Un `:id` literal sigue siendo parámetro. Ningún cliente además de `fetch` se
lee: `axios`, `got` y `XMLHttpRequest` no generan consumidor.

En Backstage, el router que un plugin de backend entrega a su `httpRouter` (`httpRouter.use(r)`,
con el `httpRouter` tomado de `coreServices.httpRouter` en los `deps` de `registerInit`) vale bajo
`/api/<pluginId>`, la ruta en que el framework lo monta, cuando `createBackendPlugin`, importado de
un paquete `@backstage/`, escribe el `pluginId` como literal o como una `const` del mismo archivo
ligada a un literal. Un plugin cuyo `pluginId` no se lee así (una lectura de entorno, un import)
deja su router en la raíz, como antes, y `meada status` señala el plugin. Del lado del
consumidor, una URL que empieza por `discovery.getBaseUrl('<id>')` (en una constante o escrito en
la propia URL) empieza en `/api/<id>`, y el motivo del enlace nombra el plugin y ese montaje; una
URL sin host de una ruta que sirve el montaje de un plugin del mismo repositorio también lo
nombra.

El plugin `proxy` de Backstage vale por la tabla `proxy.endpoints` de un `app-config*.yaml` del
repositorio: una URL que empieza por `/api/proxy/<endpoint>` (también la que escribe
`discovery.getBaseUrl('proxy')` más `/<endpoint>`) es una llamada al `target` de ese endpoint, y el
resto de la URL sigue al destino. Un `target` que es exactamente `${VAR}` enlaza por el
`base_url_env` del par, y el motivo nombra la variable y el endpoint; un `target` que es una URL
`http://` o `https://` literal vale como el host (con su ruta). Cualquier otro `target` (texto
alrededor de la variable, un valor por defecto) deja la llamada sin enlace, y `meada status` señala
el endpoint y cuántas llamadas pasan por él. Un `pathRewrite` que solo quita el endpoint (una regla,
`'^/api/proxy/<endpoint>': ''`, o `'^/api/proxy/<endpoint>/': '/'` para una URL con ruta) es lo que
el proxy hace de todos modos, así que el resto de la URL va al destino; cualquier otro
`pathRewrite` (otro reemplazo, varias reglas) se lee como un `target` ilegible: sin enlace, y
`meada status` lo señala. `ignorePath`, `prependPath: false` y un endpoint al que los archivos
`app-config*.yaml` dan valores distintos (no se sabe cuál carga el despliegue) se leen igual.

**Entorno.** `process.env.X`, `process.env["X"]` e `import.meta.env.X`, solos o pasando por
métodos de texto como `trim()`, `replace(...)` y `toLowerCase()`. La URL que empieza por una
lectura así enlaza por el `base_url_env` del par. Los archivos `.env` no se leen. Lo mismo vale
para la lectura de configuración de Backstage, `config.getString('a.b')` o
`config.getOptionalString('a.b')`, cuando algún `app-config*.yaml` del repositorio guarda
`a.b: ${VAR}` (primero `app-config.yaml`, y vale el primer archivo que tiene la clave): la lectura
es la variable `VAR`. Un valor literal o una clave que ningún archivo tiene dejan la base sin
resolver.

**Manifiestos.** `package.json` (`name`, `version`, los cuatro campos de dependencias, `exports`,
`types`, `module` y `main`) y `tsconfig.json` (`paths`, `baseUrl`, `outDir`, `rootDir`). Vea npm
en la sección 8.5.

**Canales.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `kafkajs` |
| RabbitMQ | `amqplib` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` |
| NATS | `nats` |
| Redis streams | `ioredis`, `node-redis` |
| BullMQ | `bullmq` |

**Formato de los datos.** Es el único lenguaje en que Meada lee el formato de una respuesta del
propio código, en los dos lados: lo que un consumidor `fetch` lee del `.json()` y lo que un
proveedor Next.js o Express devuelve, por esquemas zod o por objetos literales. Vea la sección 10.

**Límites conocidos.**

- `require` y `module.exports` no son importaciones entre archivos, y `new F()` de una función
  constructora no alcanza los métodos de `F.prototype`.
- Un `let` o `var` reasignado en cualquier lugar del archivo no vale como una sola definición.
- Un módulo que `tsconfig.json` no mapea y que está fuera del repositorio es externo.
- Las rutas montadas por un valor que Meada no lee (un router que viene de una llamada de fuera)
  no se sirven en ningún lugar.

### 9.2 Python

**Archivos.** `.py`. Cada `pyproject.toml` da raíces de código (su directorio, las raíces que
declara o `src`) a los archivos debajo de él, y por ellas un `import` encuentra el módulo.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `requests`, `httpx`, `aiohttp` |
| Proveedor | FastAPI, Flask, Django |

- **Consumidores.** Una función del módulo (`requests.get`, `httpx.post`, también bajo un alias)
  o un método de un cliente que el código muestra haber venido del constructor de la biblioteca
  (`Session()`, `Client()`, `AsyncClient()`, `ClientSession()`), guardado en una variable, en un
  atributo de `self`, abierto con `with` o recibido por parámetro. La `base_url=` del constructor
  es la base de las URL relativas. Un `get` en cualquier otro objeto no es nada.
- **FastAPI y Flask.** Los decoradores de método valen en un objeto que se remonta al constructor
  del framework (`FastAPI()`, `APIRouter()`, `Flask()`, `Blueprint()`). En FastAPI: `@app.get`,
  `@router.post` y los demás métodos, y `api_route`. En Flask: `@app.route(..., methods=[...])`
  (el atajo `@app.get` no genera ruta). El prefijo de `APIRouter(prefix=)`, de `include_router` y
  de `mount` se suma; en Flask, el `url_prefix` dado a `register_blueprint` sustituye al del
  propio `Blueprint`. Lo que nada monta se sirve en la raíz, salvo un `APIRouter` o `Blueprint`,
  que solo sirve donde lo coloca un montaje que Meada lee.
- **Django.** Meada parte de la URLconf raíz (un archivo con `urlpatterns` que ningún `include()`
  nombra) y desciende por `path()`, `re_path()` e `include()`. Un `re_path()` solo vale con texto
  literal y grupos nombrados (`(?P<nombre>...)`); cualquier otro patrón no genera ruta. Una view
  de función sirve los métodos que permiten `require_http_methods`, `require_GET`, `require_POST`
  y `require_safe`, o todos; `C.as_view()` sirve los métodos (`get`, `post`...) que `C` define.

**Entorno.** `os.environ["X"]`, `os.environ.get("X")` y `os.getenv("X")` (también `environ` y
`getenv` sueltos, casados tal como están escritos y no por los imports), solos o en un hueco de
f-string, con fallback `or`, con argumento predeterminado y con métodos de texto como `strip` y
`replace`. El campo `f` de una subclase de `BaseSettings` (pydantic-settings) del mismo archivo es
la variable `<env_prefix><F>` en mayúsculas, o su `alias` / `validation_alias` si existe; una
clase que no es `BaseSettings` no se lee.

**Manifiestos.** `pyproject.toml` (`[project]` y `[tool.poetry]`, incluido `[project.scripts]`) y
`requirements*.txt`. Vea PyPI en la sección 8.5.

**Canales.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `confluent-kafka`, `kafka-python` |
| RabbitMQ | `pika` |
| Amazon SQS | `boto3` |
| NATS | `nats-py` |
| Redis streams | `redis-py` |

**Límites conocidos.**

- Un decorador de ruta en un objeto que no se remonta al constructor del framework no genera
  ruta: Meada no adivina por el nombre.
- Un prefijo o una ruta con una parte ilegible (un hueco de f-string, un parámetro) no genera
  ruta. `str.format` y `%` no pliegan una URL; las f-strings y `+` sí.
- Un router montado por un import que no resuelve no se presume en la raíz.
- En Django, sin una URLconf raíz determinable (un `include()` que Meada no lee), no se sirve
  ninguna ruta.
- No hay lectura de archivos de configuración de Python (`settings.py`, `.env`) más allá de las
  lecturas de entorno en el código.

### 9.3 Go

**Archivos.** `.go` incluidos por las reglas de build del paquete. El `module` de cada `go.mod`
resuelve los imports hacia directorios de paquete, incluido un `replace` hacia un directorio del
repositorio. Un módulo anidado es dueño de su subárbol, y los archivos de un directorio con el
mismo `package` comparten ámbito (los `_test.go` también). Meada no ejecuta el toolchain de Go.

El análisis apunta a linux/amd64: los archivos `_windows.go`, con un `//go:build` falso o que
usan cgo (`import "C"`) quedan fuera.

**Qué pasa a ser definición.** Funciones, tipos, interfaces y métodos (`Tipo.Método`). Las
llamadas se resuelven dentro del paquete y por imports (`pkg.F()`, con alias o punto). La
biblioteca estándar y los módulos externos permanecen externos.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `net/http` (`Get`, `Head`, `Post`, `PostForm`, `NewRequest`, `NewRequestWithContext`), resty |
| Proveedor | `net/http`, gin, echo, chi, gorilla/mux |

Un router es la llamada que lo crea (`gin.Default()`, `echo.New()`, `chi.NewRouter()`,
`http.NewServeMux()`, `mux.NewRouter()`). Los grupos de prefijo (`Group(prefijo)` de gin y de
echo, `Route(prefijo, fn)` y `Mount(prefijo, router)` de chi, `PathPrefix(...).Subrouter()` de
gorilla/mux) suman el prefijo, y gorilla/mux restringe los métodos con `.Methods(..)`. Los
parámetros de ruta de cada framework (`{nombre}`, `:nombre`, `*resto`) pasan a ser `{}`. La base
de un cliente resty es el argumento de `SetBaseURL`.

**Entorno.** `os.Getenv` y `os.LookupEnv`, incluido el patrón escrito con `if v == ""`,
`len(v) == 0`, `!ok` o `!ok || v == ""` cuando el `ok` viene del mismo `os.LookupEnv`, y después
`strings.TrimSuffix`, `TrimRight` y `TrimSpace`. Un campo de struct con la etiqueta `env:"X"`
(cleanenv, caarlos0/env) o `envconfig:"X"` (kelseyhightower/envconfig) es la lectura de `X`
donde se lee el campo (`cfg.API.URL`), con structs anidados, cuando el struct se declara en el
mismo archivo.

**Manifiestos.** `go.mod` (`module`, `require`, `replace`). `go.sum` no se lee. Vea Go en la
sección 8.5.

**Canales.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` |
| RabbitMQ | `amqp091-go` |
| Amazon SQS | `aws-sdk-go-v2` |
| NATS | `nats.go` |
| Redis streams | `go-redis` |

**Límites conocidos.**

- Una llamada `recv.M()` es `exact` cuando el código da el tipo del receptor (un parámetro,
  campo, `var`, literal compuesto o resultado de función). Sin eso, la resolución por nombre es
  `inferred` o `ambiguous`.
- Una llamada por una interfaz se detiene en el método de la interfaz: Meada no elige la
  implementación.
- Un handler envuelto de una forma que Meada no lee (`http.StripPrefix`, `echo.WrapHandler`, un
  middleware) monta el router donde no alcanza, y sus rutas no aparecen.
- El `Add(método, ruta, handler)` de echo no se lee.
- El código con cgo, `_windows.go` y `//go:build` falso no se indexa.

### 9.4 Rust

**Archivos.** `.rs` alcanzables por el árbol de módulos de cada crate. Todo `Cargo.toml` con
`[package]` define un crate, en cualquier directorio, y Meada sigue los `mod` sin ejecutar
`cargo` ni `rustc`. Un archivo que ningún `mod` alcanza queda fuera.

**Qué pasa a ser definición.** Funciones, tipos, traits, módulos y métodos de `impl`. Las
llamadas como `crate::a::f()`, `Tipo::new()` y `Self::f()` siguen la ruta escrita; la resolución
también sigue `use`, grupos, renombres, globs, `pub use` y otros crates del workspace por el
nombre. En `x.m()`, el tipo del receptor viene de un `let`, parámetro, campo, `self` o retorno de
función escrito; una llamada sobre el resultado de otra (`Router::new().route(..)`, también por
`.await` o paréntesis) se resuelve por el tipo de retorno escrito de la llamada interna; un
`dyn Trait` o genérico limitado por trait llega al método del trait. Sin tipo escrito, la
resolución es por nombre.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `reqwest` (incluido `blocking`), `ureq` |
| Proveedor | axum, actix-web, Rocket |

En axum, `route`, `nest` y `merge` suman los prefijos por la cadena de `Router::new()`. En
actix-web y en Rocket, la función con el atributo de ruta (`#[get]`, `#[route(...)]`) o nombrada
por `.route(path, web::get().to(h))` es el handler, y `web::scope`, `web::resource`,
`.configure(f)` y la instancia de Rocket montan prefijos. El cuerpo de `format!` y `concat!`
pliega una URL cuando los argumentos son literales o, en `format!`, nombres simples; cualquier
otro argumento pasa a ser un parámetro (`concat!` solo acepta literales). Un `let mut` String
solo leído y ampliado por `push_str`, `push` y `+=` también pliega.

**Entorno.** `std::env::var("X")`, `dotenvy::var("X")` y `env!("X")` (también como argumento de
`concat!` o `format!`), con `unwrap`, `expect`, `?`, `trim`, `to_string` y similares.
`option_env!` no es una lectura.

**Manifiestos.** `Cargo.toml` (`[package]`, `[dependencies]`, `[dev-dependencies]`,
`[build-dependencies]`, `[workspace.dependencies]`, `[lib] name`). Vea Cargo en la sección 8.5.

**Canales.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `rdkafka` |
| RabbitMQ | `lapin` |
| Amazon SQS | `aws-sdk-sqs` |
| NATS | `async-nats` |
| Redis streams | crate `redis` |

**Límites conocidos.**

- **Las llamadas dentro de macros no se indexan** (`assert!`, `format!`, `vec!`, `println!`). Un
  caller que solo llama desde dentro de una macro no aparece, y `context` lo trata como "no
  probado". La única excepción es el cuerpo entre llaves de una macro de ítems del propio
  repositorio (un `macro_rules!` cuya primera regla recibe `$($x:item)*` y reescribe cada ítem
  solo con atributos externos, como el `cfg_rt!` de tokio), invocada como `m!` o `crate::m!`: se
  lee como ítems del módulo, o del bloque `impl` o trait donde está la invocación. Las macros de
  otros crates siguen sin lectura.
- Los métodos que no están en los bloques `impl` del tipo, incluidos los generados por `derive` o
  provistos por traits externos, no generan enlace.
- El análisis usa la configuración predeterminada de cada crate para x86_64 Linux: los ítems bajo
  un `#[cfg(..)]` falso y las features fuera de `default` quedan fuera; los módulos compilados en
  un build de prueba entran, no solo los `#[cfg(test)]`.
- Un router axum entregado a una llamada que Meada no lee (`route_service`, `fallback_service`,
  un envoltorio de fuera) no se monta en ningún lugar.
- `option_env!` y un `env!` que el propio archivo define no son lecturas.

### 9.5 Java

**Archivos.** `.java`, resueltos por las declaraciones de `package`. Meada no necesita JDK, Maven
ni Gradle y no ejecuta código del repositorio.

**Qué pasa a ser definición.** Clases, interfaces, enums, records, métodos y constructores,
también los anidados. Los nombres llevan el paquete y, en los métodos, los tipos escritos de los
parámetros (`example.Orders.place(String,int)`). Las llamadas de una lambda o de una clase anónima
pertenecen al símbolo que las contiene. Los tipos públicos y sus miembros públicos o protegidos
forman la superficie exportada. Los imports simples, de paquete, estáticos y con `*` resuelven por
las declaraciones de `package`. Una llamada sin receptor, con `this` o estática es `exact` cuando
queda un método por la cantidad de argumentos (`varargs` incluido), y las sobrecargas con la misma
cantidad quedan `ambiguous`. En un campo, parámetro o variable local de tipo escrito, la llamada
resuelve en ese tipo y en los supertipos de `extends` e `implements`. Un supertipo de fuera del
repositorio tiene miembros desconocidos: la llamada resuelve por nombre, salvo que sea un tipo de
la plataforma de miembros conocidos (`Serializable`, `Comparable` y similares).

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | RestTemplate, WebClient, `java.net.http`, OkHttp, OpenFeign, Retrofit |
| Proveedor | Spring MVC, JAX-RS |

- **Proveedores.** Un método de una clase `@RestController` o `@Controller` con `@GetMapping`,
  `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` o `@RequestMapping(method =
  ...)` es proveedor, bajo el prefijo del `@RequestMapping` de la clase; un `@RequestMapping` sin
  `method` atiende todos los métodos (`http::ANY::`). En JAX-RS, es proveedor un método con
  `@GET`, `@POST` y los demás en una clase con `@Path`. Las constantes `static final` se pliegan
  en la ruta. `server.servlet.context-path` no se aplica.
- **Consumidores.** Los métodos de RestTemplate, el `.uri(...)` de una petición de WebClient y el
  `build()` de un `HttpRequest.newBuilder()` o de un `Request.Builder()` de OkHttp (sin método en
  la cadena, vale `GET`). El cliente vale por el tipo que el código escribe para él o por la
  llamada que lo crea, y la base de un WebClient viene del `WebClient.create(base)` o del
  `baseUrl(base)` de donde nace. Un método que arma la URL a partir de sus parámetros es un
  wrapper.
- **Clientes declarativos.** En una interfaz `@FeignClient` o de Retrofit (`@GET("x")` y los
  demás), cada método es un consumidor, en el propio método de la interfaz. La base viene de `url`
  y `path` en Feign (con placeholders `${clave}` de la configuración de Spring) y del
  `Retrofit.Builder().baseUrl(base)` que crea la interfaz.

**Entorno.** `System.getenv("X")` y `@Value("${clave}")` de Spring en un campo o parámetro de
constructor, con la clave leída de `application.properties`, `application.yml` o
`application.yaml` en `src/main/resources` del módulo. Un placeholder `${VAR}` que ninguna clave
llena es la variable `VAR`, que el `base_url_env` del par mapea. Los perfiles
(`application-<perfil>`) y SpEL no se leen.

**Manifiestos.** `pom.xml` y los archivos de build de Gradle (`build.gradle`, `build.gradle.kts`,
`settings.gradle`, `settings.gradle.kts`, `gradle.properties` y los version catalogs
`*.versions.toml`). Vea Maven y Gradle en la sección 8.5.

**Canales.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | Spring Kafka |
| RabbitMQ | Spring AMQP |
| Amazon SQS | AWS SDK v2 |
| NATS | jnats |
| Redis streams | Jedis, Lettuce |
| JMS | Spring JMS |

El cliente vale por el tipo que el código escribe para él, y un listener por la anotación de
Spring (`@KafkaListener`, `@RabbitListener`, `@JmsListener`). El nombre del canal se pliega como
una URL, con los `${clave}` de las anotaciones leídos de la configuración de Spring.

**Límites conocidos.**

- Un tipo del repositorio con el nombre de una anotación o de un cliente de la biblioteca la
  esconde.
- `@RequestLine` de Feign, `@HTTP` de Retrofit y una anotación heredada de una interfaz no se
  leen.
- Un nombre de canal que no se calcula (variable de entorno, SpEL) es un lado no resuelto.
- Un archivo cuyos nombres envolventes repetidos pasan de 4 MiB en total queda sin símbolos ni
  llamadas, y `meada status` lo nombra.

### 9.6 Kotlin

**Archivos.** `.kt` y `.kts` (salvo `build.gradle.kts` y `settings.gradle.kts`), resueltos por las
declaraciones de `package`. Meada no necesita compilador de Kotlin, JDK ni Gradle.

**Qué pasa a ser definición.** Clases (también `data`, `sealed`, `enum` y `annotation`),
interfaces, objetos, companions, funciones de nivel superior, funciones de extensión, propiedades
con getter y métodos, también los anidados. Los nombres llevan el paquete y, en las funciones, los
tipos escritos de los parámetros (`example.helper(Int)`); una función de extensión lleva el tipo
del receptor (`example.Point.norm()`). Las llamadas de una lambda pertenecen al símbolo que la
contiene. Las declaraciones que no son `private` ni `internal` forman la superficie exportada. Los
imports simples, con `as` y con `*` resuelven por las declaraciones de `package`, y el propio
paquete es visible sin import. Llamar al nombre de una clase la construye. Kotlin y Java del mismo
repositorio se llaman por el nombre totalmente calificado. Dentro de una lambda, una llamada sin
receptor puede ser de un receptor implícito, y resuelve solo por nombre.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | RestTemplate, WebClient, OkHttp, Retrofit, cliente Ktor |
| Proveedor | Spring MVC, Ktor |

- **Spring, OkHttp y Retrofit.** Siguen las reglas de Java (sección 9.5), escritas a la manera de
  Kotlin: `RestTemplate()` sin `new`, argumentos con nombre y `Api::class.java` como servicio
  Retrofit.
- **Ktor, proveedor.** Una ruta dentro de `routing { }` (`get`, `post`, `put`, `patch`, `delete`,
  `head`, `options` y `route(ruta, HttpMethod.X)`) es proveedor bajo el prefijo de cada bloque
  `route(prefijo) { }` que la envuelve, también en una función de extensión de `Route` o
  `Routing` llamada dentro del bloque. La lambda de la ruta es el handler. Los nombres de Ktor
  solo valen con el import de Ktor, y el enrutamiento tipado (clases de recurso) no se lee.
- **Ktor, consumidor.** Una llamada (`get`, `post` y las demás, o `request(url) { method =
  HttpMethod.X }`; sin método, vale `GET`) sobre un `HttpClient`. Un `defaultRequest { url(base)
  }` es la base de las URL relativas. `url { path(..) }` no se lee.

**Entorno.** `System.getenv("X")` y `System.getenv()["X"]`, también con `?:`, `!!`, paréntesis y
métodos de string como `trim` y `removeSuffix`, y `@Value("\${clave}")` de Spring (el `$`
escapado), con la configuración de Java. La lectura puede estar en una propiedad, en un `val`
local o en la propia URL; un `var` no se sigue.

**Manifiestos.** Los de Java: vea Maven y Gradle en la sección 8.5.

**Canales.** Los mismos brokers y clientes de Java (sección 9.5), escritos a la manera de Kotlin:
argumentos con nombre y arrays (`@KafkaListener(topics = ["a", "b"])`), un `const val` de nivel
superior o de un `companion object` y un cliente recibido por el constructor.

**Límites conocidos.**

- Una llamada sobre un receptor implícito (función de extensión del cliente, bloques `apply` y
  `with`) no se lee en los canales.
- En un bloque `request`, una asignación de método que puede no ejecutarse deja el método
  desconocido, nunca `GET`.
- Un nombre de canal con una parte que no se evalúa (`"orders-$env"`) es un lado no resuelto.

### 9.7 C#

**Archivos.** `.cs`, agrupados en proyectos por los `.csproj` de estilo SDK (con `Sdk`; un
`.csproj` antiguo no es proyecto, y los `.sln` no cambian nada) y resueltos por las declaraciones
de `namespace`. Las carpetas `obj/` y `bin/` junto a un `.csproj` quedan fuera del índice. Meada no
necesita .NET SDK ni MSBuild.

**Qué pasa a ser definición.** Clases, records, structs e interfaces, también los anidados y
`partial`, métodos y constructores con los tipos escritos de los parámetros
(`Acme.Orders.OrderService.Place(string,int)`) y funciones locales. Los tipos públicos y sus
miembros públicos o protegidos forman la superficie exportada, y un tipo `internal` solo existe
para los archivos de su proyecto. `using`, `using static`, alias y `global using` resuelven por los
namespaces de todos los proyectos; los `global using`, los `<Using>` del `.csproj` y los implicit
usings valen en los archivos del proyecto. Una llamada sin receptor, con `this`, `base`, estática
o por `using static` es `exact` cuando queda un método por la cantidad de argumentos (`params` y
valores por defecto incluidos). Una base de fuera del repositorio deja las llamadas a los miembros
de la clase sin `exact`, salvo la de un tipo de miembros conocidos (`IDisposable`, `IEquatable` y
similares). Un atributo es una llamada sobre lo que decora.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `HttpClient`, typed clients, Refit |
| Proveedor | ASP.NET Core (controllers y minimal APIs) |

- **Controllers.** Un método de una clase controller (con `[ApiController]`, o derivada de
  `ControllerBase` o `Controller`) con `[HttpGet]`, `[HttpPost]` y los demás es proveedor, bajo el
  `[Route]` de la clase; `[controller]`, `[action]` y `{id:int}` se entienden. El enrutamiento
  convencional (`MapControllerRoute`) no se lee.
- **Minimal APIs.** `MapGet`, `MapPost`, `MapPut`, `MapPatch`, `MapDelete` y `MapMethods` sobre la
  aplicación o sobre un `MapGroup(prefijo)`, con grupos anidados; el prefijo sigue al grupo por
  variables, argumentos, retornos y métodos de extensión. Un grupo que Meada no lee (parámetro de
  lambda, campo, variable reasignada) no genera ruta.
- **Consumidores.** `GetAsync`, `PostAsync`, `GetFromJsonAsync`, `SendAsync(new
  HttpRequestMessage(HttpMethod.X, url))` y similares sobre un `HttpClient`, por el tipo declarado
  o por la construcción, nunca solo por el nombre. La URL pliega literales, interpolación, `+` y
  campos `const` y `static readonly`. Un `BaseAddress = new Uri(base)` es la base de las URL
  relativas, y un typed client registrado con `AddHttpClient<T>(c => c.BaseAddress = ...)` da la
  base al `HttpClient` de `T`. Cada método de una interfaz Refit (`[Get("/x")]` y los demás) es un
  consumidor, con la base del `RestService.For<T>(url)` que la crea.

**Entorno.** `Environment.GetEnvironmentVariable("X")`, con `??`, `!` y `Trim`, y
`configuration["Seccion:Clave"]` sobre un `IConfiguration` o sobre `builder.Configuration`, que es
la variable `Seccion__Clave`. Una URL literal en el `appsettings.json` del proyecto es el valor
por defecto de esa variable. `appsettings.<Entorno>.json`, `GetValue`, `GetSection`, el binding de
opciones y los user secrets no se leen.

**Manifiestos.** `.csproj` de estilo SDK y `Directory.Build.props`. Vea NuGet en la sección 8.5.

**Canales.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `Confluent.Kafka` |
| RabbitMQ | `RabbitMQ.Client` |
| Amazon SQS | `AWSSDK.SQS` |
| NATS | `NATS.Client` |
| Redis streams | `StackExchange.Redis` |
| Azure Service Bus | `Azure.Messaging.ServiceBus` |

El cliente vale por el tipo que el código escribe para él o por la construcción que lo creó,
resuelto por los `using`, nunca solo por el nombre.

**Límites conocidos.**

- Un controller que hereda de otra clase del repositorio, una clase que es base de otra y una
  clase `partial` repartida en varios archivos no generan ruta.
- `IHttpClientFactory.CreateClient(name)` no se lee, y una interfaz Refit registrada con
  `AddRefitClient` tiene base desconocida.
- MassTransit, `BasicGet`, `SubscribeSync` y NATS.Net v2 no se leen en los canales.
- Un nombre de canal leído del entorno o de `IConfiguration` es un lado no resuelto.

### 9.8 Swift

**Archivos.** `.swift` (salvo `Package.swift` y `Package@swift-<versión>.swift`), agrupados en
módulos por los targets del `Package.swift`; los archivos fuera de todo target forman un solo
módulo. Meada no necesita toolchain de Swift ni Xcode.

**Qué pasa a ser definición.** Clases, structs, enums, actors y protocolos, también los anidados,
y funciones, métodos e inicializadores con las etiquetas de los argumentos en el nombre
(`OrderService.place(order:quantity:)`). Los miembros de una extensión son del tipo extendido. Las
llamadas de un closure pertenecen al símbolo que lo contiene. Los nombres resuelven primero en el
módulo del archivo y después en los módulos que importa, por las declaraciones `public` y `open`
(y `internal` con `@testable`). Una llamada es `exact` cuando queda una declaración por las
etiquetas, y las sobrecargas con las mismas etiquetas quedan `ambiguous`. Un receptor de tipo de
fuera del repositorio (`session: URLSession`) solo alcanza las extensiones de ese tipo que el
repositorio escribe, y una llamada por un receptor de tipo protocolo se detiene en el requisito.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `URLSession`, Alamofire, AsyncHTTPClient |
| Proveedor | Vapor, Hummingbird |

- **Vapor.** Una ruta (`get`, `post`, `put`, `patch`, `delete` y `on(.METHOD, ruta)`) llamada
  sobre la aplicación o sobre un `grouped(prefijo)` suyo es proveedor bajo el prefijo de cada
  `grouped` que la envuelve. Un `RouteCollection` registrado con `register(collection:)` sirve las
  rutas de su `boot(routes:)`. Los nombres solo valen con el import `Vapor`.
- **Hummingbird.** Una ruta (`get`, `post`, `put`, `patch`, `delete` y `on(ruta, method: .get)`)
  llamada sobre un `Router()` o sobre un `group(prefijo)` suyo es proveedor bajo el prefijo de
  cada `group` que la envuelve. Los nombres solo valen con el import `Hummingbird`.
- **Consumidores.** `data(from:)` y `data(for:)` sobre una `URLSession`, `AF.request(url,
  method:)` y `execute` de un `HTTPClientRequest(url:)`. El método es GET cuando no se escribe, y
  el `httpMethod` asignado a un `URLRequest` vale. La URL pliega `URL(string:)`,
  `appendingPathComponent`, literales, interpolación, `+` y constantes `let`.

**Entorno.** `ProcessInfo.processInfo.environment["X"]`, con `!`, paréntesis, `??` con un literal
de string simple y métodos de string como `trimmingCharacters` y `lowercased`. La lectura puede
estar guardada en un `let`. Un `var` reasignado no se sigue, y el `Environment.get("X")` de Vapor
no es una lectura.

**Manifiestos.** `Package.swift`. Vea SwiftPM en la sección 8.5.

**Canales.** Swift no tiene lectura de canales en 1.0.

**Límites conocidos.**

- `dataTask(with:)` y el `addRoutes` de un controlador de Hummingbird no se leen.
- Un middleware construido en `grouped(..)` no suma ruta, y una ruta que Meada no lee no genera
  ruta.
- Un módulo que no es target del repositorio es externo.
- Todas las dependencias del `Package.swift` más cercano valen para los archivos debajo de él, sin
  separar lo que declara cada target.

## 10. Drift de forma

La **forma** de un contrato es lo que él lleva, registrado como un conjunto de rutas de campos
(`items[].id`; `[].id` cuando el cuerpo es una lista). El **drift** es un campo que un consumidor
lee y que la forma del proveedor enlazado a él no contiene: un campo que la ruta dejó de devolver.
Un enlace puede estar en pie y aun así romperse, y el drift es el aviso para eso.

De dónde viene cada forma:

| Lado | Fuente | Nivel del campo |
| --- | --- | --- |
| Proveedor | Documento OpenAPI (cualquier lenguaje) | `exact` |
| Proveedor | Esquema zod por el que pasa la respuesta de un handler de Express o Next.js | `exact` |
| Proveedor | Objetos literales que devuelve un handler de Express o Next.js | `inferred` |
| Consumidor | Tipo declarado o esquema (`z.infer`) del cuerpo leído | `exact` |
| Consumidor | Campos leídos del `.json()` de un `fetch` | `inferred` |

Cada ruta que falta trae su nivel: `exact` cuando la forma del proveedor y la ruta del consumidor
son ambas declaradas (documento, esquema, tipo), e `inferred` cuando una de las dos viene de lo
que hace el código (lecturas de campo, literales devueltos). El nivel del enlace no cambia por
causa del drift. Hoy solo el código TypeScript y JavaScript genera la forma de un consumidor, y
solo los consumidores `fetch` y los proveedores Next.js y Express generan la de un proveedor a
partir del código; los proveedores de otros lenguajes entran por el documento.

Cómo muestra Meada el drift:

| Consulta | Resultado |
| --- | --- |
| `trace` | Añade `drift: missing <ruta>` a la línea del enlace |
| `context` en una ruta | Muestra el origen de la forma, los consumidores, el drift, el nivel y las lecturas no verificadas |
| `search` con `kind=drift` | Lista los drifts, uno por ruta que falta |
| `meada status` | Cuenta las lecturas que faltan, bajo el repositorio del consumidor; el proveedor no recibe cuenta |

Ejemplo de línea de `trace`:

```text
example.com/acme/api src/orders.ts:12 GET /api/orders
  →[inferred: base URL from API_URL, a base_url_env of peer example.com/acme/api] example.com/acme/web src/orders.ts:8 loadOrders drift: missing discount
```

(Ejemplo ilustrativo en el formato de `trace`: `web` lee `discount` de la respuesta de
`GET /api/orders`, y `api` no la devuelve.)

Los límites de este análisis, para que usted no tome un silencio por ausencia de drift:

- **Solo presencia, nunca tipos.** Meada compara la presencia de las rutas. No compara tipos,
  formatos, `required` ni `nullable`.
- **Las partes abiertas no se verifican.** Un objeto que acepta propiedades extra
  (`additionalProperties`, `.passthrough()`, `z.record`, `z.any`), un esquema detrás de una
  referencia externa y un cuerpo que Meada no consigue listar dejan la parte **abierta**: todo lo
  que esté debajo de ella puede existir, y la lectura aparece en "no verificadas" en vez de como
  drift.
- **Nada debajo de un escalar.** `name.length` no es un campo, y `lista.length` debajo de un
  array no es drift.
- **Solo la ruta más superficial.** De cada rama, solo se lista la primera ruta que falta.
- **Los enlaces ambiguos no tienen drift.** Sin un proveedor único, no hay forma que comparar.
- **El handler debe responder de una manera que se lea.** Un cuerpo que viene de un valor que
  Meada no sigue (una llamada de fuera, un spread de algo desconocido) abre esa ruta. Solo
  cuentan las respuestas 2xx; una respuesta con status literal fuera de 2xx se ignora.
- **Sin documento ni esquema, la forma es solo lo que muestra el código.** Los literales y las
  lecturas son una conjetura sobre lo que el código devuelve, por eso `inferred`.

## 11. Comunidades y procesos

Dos análisis dan un mapa del código más allá de los enlaces puntuales. Ninguno usa un modelo de
lenguaje, y los dos salen iguales en cualquier máquina.

### Comunidades

Una **comunidad** es un grupo de símbolos de un repositorio que se llaman más entre sí que al
resto. Meada las halla por las llamadas de nivel `exact` o `inferred`, con el algoritmo de
Leiden, sin aleatoriedad y con aritmética de enteros. El nombre viene del directorio que tiene
más miembros (`src`, y `src#2` cuando otra comunidad del repositorio ya tiene ese nombre). Todo
símbolo que está en al menos una de esas llamadas, como quien llama o como quien es llamado, es
miembro de alguna comunidad; un símbolo cuya única llamada es a sí mismo, o que solo es llamado
desde el nivel superior de un archivo, forma una comunidad por sí solo; un símbolo fuera de toda llamada no
pertenece a ninguna.

- `search` con `kind=community` halla comunidades por el nombre.
- `context` de un símbolo dice su comunidad, o `community: none`.
- `meada export` incluye las comunidades.

### Procesos

Un **punto de entrada** es una puerta de un repositorio. Cada uno tiene su proceso; ninguna
puntuación ni umbral elige cuáles valen.

| Tipo | Qué es |
| --- | --- |
| `route` | Cada ruta HTTP que el repositorio sirve; el handler es la función |
| `channel` | Cada canal que un handler consume |
| `program` | Un inicio de programa que el lenguaje o el manifiesto declara |
| `export` | Cada función o método exportado que nada llama en el propio repositorio |

Los inicios de programa, por lenguaje:

- **Python:** `if __name__ == "__main__":` (símbolo `__main__`) y las funciones de
  `[project.scripts]` en la forma `módulo:función`. Los nombres exportados son los públicos, sin
  `_` al frente, o los de un `__all__` literal.
- **Rust:** el `fn main` de cada binario, en `src/main.rs`, `src/bin/` o `[[bin]] path`.
- **Go:** `func main` en un `package main`.
- **Java:** `public static void main` con un parámetro `String[]` o `String...`.
- **Kotlin:** un `fun main` de nivel superior.
- **C#:** un `static Main` (cualquier firma) y, en un archivo con instrucciones de nivel
  superior, el símbolo `Main`.
- **Swift:** un tipo marcado `@main` (por su `main()` cuando declara uno) y las instrucciones de
  nivel superior de un `main.swift`, en el símbolo `main`.
- **TypeScript y JavaScript:** no hay tipo `program`; las puertas son las rutas, los handlers de
  canal y las funciones exportadas a las que nadie llama.

Un **proceso** es el árbol de llamadas que un punto de entrada pone en movimiento: las llamadas
dentro del repositorio en todos los niveles y los enlaces hacia otros repositorios, hasta las
hojas. Termina en una llamada o enlace ambiguo (un paso que lista los candidatos), en un
consumidor no resuelto, en un ciclo o en un par ausente. El proceso no se guarda: se recorre en
la consulta, a partir de las llamadas y enlaces ya guardados.

- `trace` con `process::<identidad del punto de entrada>` lee el árbol. La identidad es la del
  handler, más el contrato para una ruta o un canal, por ejemplo
  `process::example.com/acme/worker::src/consumer.ts::onOrderCreated::channel::kafka::orders.created`.
- `search` con `kind=process` halla procesos por la etiqueta del punto de entrada
  (`GET /users/{}` o el nombre calificado).
- `context` de un símbolo lista los procesos de los que forma parte, andando por las llamadas y
  enlaces hacia los llamadores.
- `meada export` incluye cada proceso.

### Duplicados

`search` con `kind=duplicate` lista los símbolos cuyo texto es idéntico en dos o más lugares del
workspace (en repositorios distintos o en el mismo), en grupos ordenados por el tamaño del
fragmento. El export trae el `hash` BLAKE3 de cada símbolo. Las copias casi iguales (una
diferencia de espacio en blanco) no son duplicados.

## Adónde ir después

- [Capacidades y límites](https://claudiomascaro.github.io/meada/es/capabilities/): el estado
  detallado de cada lenguaje y contrato.
- [Estabilidad](https://claudiomascaro.github.io/meada/es/stability/): lo que 1.x congela.
- [Modo servidor](https://claudiomascaro.github.io/meada/es/server/): operar Meada para un
  equipo.
- Un resultado erróneo en el código del propio Meada es un bug: abra un issue con la consulta, la
  respuesta y la esperada.
