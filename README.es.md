# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · **Español**

> Esta es una traducción. El [README](README.md) en portugués de Brasil es la fuente; si no
> coinciden, vale el original, y cualquier corrección a esta página es bienvenida.

Meada lee tus repositorios, arma el mapa de quién llama a quién y cose un repositorio con otro
a través de los contratos que comparten: rutas HTTP, colas, paquetes, recursos compartidos. Los
agentes de código, y también las personas, consultan ese mapa por MCP. Ninguna etapa de la
indexación usa un LLM.

## Instalación

Elige una de tres formas.

- **Cargo**: `cargo install meada`.
- **Binario listo**: descarga el archivo de tu sistema desde los
  [releases de GitHub](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS y
  Windows, en x86_64 y arm64) y pon `meada` en el `PATH`.
- **Instalador** (Linux y macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.

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

`meada mcp` habla MCP por stdio y debe iniciarse dentro del repositorio. En la configuración de
tu agente (por ejemplo el `.mcp.json` de Claude Code):

```json
{
  "mcpServers": {
    "meada": { "command": "meada", "args": ["mcp"] }
  }
}
```

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
| `MEADA_DECIDER_MODEL` | modelo | `typesafe/jev-1.13` (`decisions`); obligatoria con `chat` |

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
renombres, globs, `pub use` y los otros crates del workspace por nombre; las de método (`x.m()`) por el tipo del receptor cuando el código lo escribe (`let`, parámetro, campo, `self`, retorno de función; un `dyn Trait` o genérico con trait va al método del trait), si no por el nombre; las de dentro de macros
no se indexan, y contratos Rust además de los paquetes aún no; solo entra la configuración por defecto de cada crate, para un destino fijo x86_64 Linux: un ítem bajo un `#[cfg(..)]` falso, o bajo una feature fuera del `default` del `Cargo.toml`, queda fuera, y los módulos `#[cfg(test)]` entran), y enlaza repositorios, también de un
lenguaje al otro,
por contratos HTTP (`fetch` y, en Python, `requests`, `httpx` y `aiohttp` del lado que consume;
Next.js (app router) y Express y, en Python, FastAPI, Flask y Django del lado que sirve, también
detrás de un gateway), por los
recursos declarados en el `meada.toml` y por canales: tópicos Kafka (`kafkajs` y, en Python, `confluent-kafka` y `kafka-python`), colas RabbitMQ
(`amqplib` y, en Python, `pika`, con el `publish` a un exchange llegando a las colas que los
bindings le atan), colas Amazon SQS (AWS SDK v3, `sqs-consumer` y, en Python, `boto3`), subjects
NATS (`nats` y, en Python, `nats-py`, con los comodines `*` y `>`), jobs BullMQ y streams Redis
(`ioredis`, `node-redis` y, en Python, `redis-py`), con el productor enlazado al handler de cada
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
name` al final de la razón. Una dependencia fuera del workspace no aparece. No lee lockfile
(`Cargo.lock` incluido) ni consulta el registro npm, PyPI ni crates.io. Meada también señala el *drift* de forma: los campos que un consumidor `fetch` lee de la respuesta ya interpretada y que la ruta que llama no devuelve, comparados con la forma de la ruta (un documento OpenAPI, un schema zod o los objetos literales de su handler). `trace` termina la línea del enlace con `drift: missing <camino>`, `context` de una ruta dice de dónde viene su forma y lista cada consumidor que lee campos de la respuesta, con su drift y nivel y las lecturas no comprobadas bajo una parte abierta cuando las hay, `search` con el tipo `drift` lista todo el drift, y `meada status` cuenta las lecturas ausentes de los consumidores de cada repositorio (las cuenta el repositorio del consumidor; el del proveedor no muestra nada). `meada export` trae las comunidades de cada repositorio: grupos de símbolos que se llaman más entre sí que con el resto, hallados con el algoritmo de Leiden sin nada aleatorio y solo con aritmética entera, y nombrados por la carpeta donde vive la mayoría de sus miembros (`src`, `src#2`), iguales en cada máquina. Cada enlace tiene el nivel `exact`, `declared`, `inferred`, `decided` o `ambiguous`; una llamada cuya URL no
se puede resolver aparece como no resuelta y solo se vuelve enlace por una decisión, en el nivel
`decided`, y una ruta que Meada no sabe dónde se sirve queda fuera del mapa en vez de ser
adivinada. Los clústeres de broker no se modelan: dos clústeres que tienen un tópico con el
mismo nombre se enlazan como si fueran uno solo. Cada ruta que sirve un handler, cada canal que consume un handler consumidor y cada función o método exportado que nada llama en su propio repositorio es un punto de entrada, con su proceso: `trace` con `process::<identidad del punto de entrada>` lee el árbol de llamadas que dispara, a través de los repositorios, y `meada export` trae el proceso de cada punto de entrada. En Python, el bloque `if __name__ == "__main__":` de un archivo (el símbolo `__main__`) y cada función que nombra un `[project.scripts]` del `pyproject.toml` (`módulo:función`) también son puntos de entrada, y solo los nombres públicos (sin `_` al inicio, o los del `__all__` literal) cuentan como exportados. En Rust, el `fn main` de cada binario (`src/main.rs`, `src/bin/`, `[[bin]] path`) también lo es. `search` con el tipo `community` halla una comunidad por su nombre y con el tipo `process` halla un proceso por la etiqueta de su punto de entrada, y `context` de un símbolo dice su comunidad (o `community: none`) y los procesos a los que pertenece, hallados subiendo por las mismas llamadas y enlaces que recorre el proceso. Aún no hay: otros lenguajes, otros clientes HTTP
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
