# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · [Español](README.es.md)

Meada lee tus repositorios, arma el mapa de quién llama a quién y cose un repositorio con otro
a través de los contratos que comparten: rutas HTTP, colas, paquetes, recursos compartidos. Los
agentes de código, y también las personas, consultan ese mapa por MCP. Ninguna etapa de la
indexación usa un LLM.

## Instalación

Elija el canal que encaje con su entorno. Todos entregan el mismo binario `meada`.

```sh
# Cargo (compila con el toolchain de Rust)
cargo install meada

# Instalador de shell (Linux y macOS)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh

# npm: descarga el binario de su plataforma y lo ejecuta, sin instalar nada antes
npx @meada/meada --version
```

- **Binario listo**: descargue el archivo de su sistema (Linux, macOS o Windows, en x86_64 o
  arm64) de los [releases de GitHub](https://github.com/ClaudioMascaro/meada/releases) y ponga
  `meada` en el `PATH`.
- **Contenedor**: `ghcr.io/claudiomascaro/meada:<versión>`, para Linux amd64 y arm64.

La versión del paquete npm es siempre la del release. Para indexar, Meada solo necesita el
binario y git. Los detalles de cada canal, de la consola y del contenedor están en
[Instalación](https://claudiomascaro.github.io/meada/es/installation/) y en la
[guía del usuario](docs/user-guide.es.md) (sección 1, Instalación).

## Primeros pasos

En la raíz de un repositorio (aquí `web`, que consume una ruta de `api`):

```sh
meada index     # monta el índice en .meada/, del repositorio y de sus pares
meada status    # el estado de cada índice y los pares no encontrados
```

Para que Meada una `web` con `api`, el `meada.toml` de `web` nombra al par:

```toml
[[peer]]
repo = "example.com/acme/api"       # remoto git de origin, sin esquema ni .git
base_url_env = ["API_URL"]          # variable de donde el cliente lee la URL base
```

La referencia completa de `meada.toml` está en la
[guía del usuario](docs/user-guide.es.md) (sección 4, referencia de `meada.toml`).

## Conectar un agente (MCP)

`npx @meada/meada mcp` es la línea de lanzamiento del servidor MCP: el cliente la ejecuta y habla con
Meada por stdio. En la configuración de su agente (por ejemplo el `.mcp.json` de Claude Code):

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["@meada/meada", "mcp"] }
  }
}
```

En Claude Code, lo mismo en una línea: `claude mcp add meada -- npx @meada/meada mcp`. Con `meada` ya en
el `PATH`, use `"command": "meada", "args": ["mcp"]`.

El agente recibe tres herramientas, `search`, `context` y `trace`, que solo leen. Vea
[Agentes y MCP](https://claudiomascaro.github.io/meada/es/mcp/) y la
[guía del usuario](docs/user-guide.es.md) (sección 2, Conectar un agente).

## Estado

Meada 1.0 entrega todo el alcance de la v1 del [ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md):

- **Contratos**: rutas HTTP, recursos declarados, documentos OpenAPI y AsyncAPI, colas y
  tópicos, paquetes.
- **Niveles**: un nivel en cada enlace, cadenas que atraviesan repositorios y decisiones con
  decisor opcional (`meada decide`).
- **Análisis**: drift de forma, comunidades y procesos.
- **Lenguajes en 1.0**: TypeScript y JavaScript, Python, Go y Rust. Java, Kotlin, C# y Swift son
  trabajo planificado de la Release 2 y llegan después como versiones 1.x (#26 a #29, ADR 0003).

**Promesa de estabilidad.** Desde 1.0 rige semver: un cambio incompatible en alguno de estos
elementos exige una versión major.

- los nombres, los parámetros y la gramática de salida de las herramientas MCP (`search`,
  `context` y `trace` hoy);
- los comandos y las flags de la CLI;
- los esquemas de `meada.toml` y de `meada.decisions.toml`;
- el formato de `meada export`.

Añadir una herramienta MCP, un parámetro opcional, un valor aceptado a un parámetro, un tipo de
búsqueda, un campo al final de una línea de la salida, una clave de configuración, un campo del
export o un lenguaje es un cambio menor. No entran en la promesa el texto de los mensajes de la
CLI, el texto de las descripciones de las herramientas, el archivo de índice y el layout de
`.meada/` (reconstruidos cuando cambia el esquema) y qué enlaces encuentra Meada en un código
(mejorar la resolución es una corrección, no una ruptura). El detalle de lo que se congela y de
lo que no está en [Estabilidad](https://claudiomascaro.github.io/meada/es/stability/).

## Alimentar una base de conocimiento

`meada export` imprime el índice de un repositorio como JSON ordenado, bueno para `diff` y para
alimentar una base de conocimiento. Por repositorio trae los símbolos (identidad estable y
span), las llamadas con sus niveles, proveedores y consumidores, links, imports, comunidades,
procesos y puntos de entrada. No trae texto fuente: quien necesita el fragmento lee el span
en el archivo.

Un registro por símbolo, con su comunidad, quién lo llama y a quién llama, contratos y procesos:

```sh
meada export > web.export.json
jq -c '. as $e | $e.symbols[] | .id as $id | {
  id, hash, span,
  community: ([$e.communities[]? | select(.members | index($id)) | .name] | first),
  callers: [$e.calls[] | select(.target == $id) | .caller],
  callees: [$e.calls[] | select(.caller == $id and .target) | .target],
  contracts: ([($e.providers[]?, $e.consumers[]?) | select(.site | startswith($id + "::")) | .contract] | unique),
  processes: [$e.processes[]? | select(any(.steps[]; .node == $id)) | .entry_point]
}' web.export.json

# duplicates: symbols that share a hash
jq '[.symbols[] | {id, hash}] | group_by(.hash) | map(select(length > 1))' web.export.json
```

Un registro por comunidad (`communities`) o por contrato (`providers`, `consumers`, `links`)
sigue las mismas listas.

- **Fan-in** entre repositorios: cuente, por destino, las `calls`, los `links` (por `provider`) y
  los `imports` de todos los exports juntos.
- **Exposición**: los puntos de entrada en `processes` (`route`, `export`, `channel`, `program`),
  los nombres exportados y los consumidores de paquete (`packages`, `dependencies`, `imports`).

```sh
jq -s '[.[] | (.calls[]? | select(.target) | {site: .id, target, kind: "calls"}),
              (.links[]? | {site: .consumer, target: .provider, kind: "links"}),
              (.imports[]? | {site, target, kind: "imports"})]
  | unique_by([.kind, .site, .target])
  | group_by(.target)
  | map(. as $g | {target: $g[0].target}
      + ([$g[].kind] | group_by(.) | map({(.[0]): length}) | add))' *.export.json
```

El export es por repositorio: una organización ejecuta `meada export` en cada uno y une los
resultados por la identidad de los símbolos. Los embeddings y cualquier paso con modelo quedan
aguas abajo, fuera de Meada (invariante 1, [ADR 0002](docs/adr/0002-product-invariants.md)).

## Documentación

- [Guía del usuario](docs/user-guide.es.md)
- [Configuración](https://claudiomascaro.github.io/meada/es/configuration/)
- [Agentes y MCP](https://claudiomascaro.github.io/meada/es/mcp/)
- [Modo servidor](https://claudiomascaro.github.io/meada/es/server/)
- [Consola](https://claudiomascaro.github.io/meada/es/console/)
- [Capacidades y límites](https://claudiomascaro.github.io/meada/es/capabilities/)
- [Estabilidad](https://claudiomascaro.github.io/meada/es/stability/)
- [Participar](https://claudiomascaro.github.io/meada/es/contributing/)

[Apache-2.0](LICENSE).
