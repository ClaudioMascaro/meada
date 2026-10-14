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
```

Meada encuentra los pares por su remoto git, aunque el clon esté en otra ruta en cada máquina.
Una clave desconocida o mal escrita da un error con el archivo, la línea y la clave. Sin
`meada.toml`, el repositorio no declara ningún par.

## Uso

En la raíz del repositorio:

- `meada index` arma el índice, en `.meada/`, del repositorio y de los pares;
- `meada status` muestra el conjunto de repositorios, el estado de cada índice y los pares que
  no se encontraron;
- `meada export` imprime todo el índice como JSON ordenado, útil para comparar dos ejecuciones
  con `diff`;
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

- `search` encuentra un símbolo, una ruta o un contrato por el nombre;
- `context` describe un símbolo: su código leído del disco, quién lo llama, a quién llama y los
  contratos que toca;
- `trace` sigue una cadena de llamadas por varios repositorios, río abajo (`down`) o río arriba
  (`up`); con `changes: uncommitted`, parte del diff del árbol de trabajo y sirve para analizar
  el impacto de lo que se acaba de editar.

Las respuestas son líneas compactas dentro de un presupuesto de tokens (2.000 por defecto,
ajustable en cada llamada). Cada enlace entre repositorios lleva el nivel y el motivo
(`→[level: reason]`), y `trace` termina con un pie: el nivel más débil de la cadena, el número
de saltos, los repositorios involucrados y los tokens usados.

## Estado

Versión 0.1, la primera. Indexa repositorios TypeScript y JavaScript (`.ts`, `.tsx`, `.js`,
`.jsx`, `.mjs`, `.cjs`) y enlaza repositorios por contratos HTTP: `fetch` del lado que consume,
Next.js (app router) y Express del lado que sirve. Cada enlace tiene el nivel `exact`,
`inferred` o `ambiguous`; una llamada cuya URL no se puede resolver aparece como no resuelta,
nunca como enlace. Aún no hay: otros lenguajes, otros clientes HTTP además de `fetch`, colas,
paquetes, el comando `meada decide` ni el paquete npm. El alcance y el orden del trabajo están
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
