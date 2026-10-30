# Estabilidad

Este documento dice sobre qué puedes construir en la serie 1.x y cuándo un cambio en Meada exige una versión mayor. Rige desde la 1.0: la serie 0.x no lleva promesa de estabilidad, y cualquier elemento de aquí puede cambiar entre versiones 0.x.

Meada sigue el [versionado semántico](https://semver.org/lang/es/). En la serie 1.x, un cambio que rompe un elemento congelado solo entra en una versión mayor (2.0). Un cambio aditivo entra en una versión menor (1.x).

Este documento describe el build predeterminado (`cargo install meada`). Los [módulos de nube](cloud.md) solo existen en builds con la feature correspondiente y solo añaden a la superficie; ninguno cambia lo que se lista aquí.

## Qué está congelado

### Herramientas MCP

Tres herramientas, siempre con estos nombres. Ninguna escribe.

| Herramienta | Parámetro | Tipo | Predeterminado | Significado |
| --- | --- | --- | --- | --- |
| `search` | `query` | texto, obligatorio | | Nombre que buscar, sin distinguir mayúsculas. |
| `search` | `kind` | texto, opcional | todos | Restringe a un tipo (lista abajo). |
| `search` | `budget` | entero sin signo de 32 bits, opcional | 2.000 | Presupuesto de tokens de la respuesta. |
| `context` | `symbol` | texto, obligatorio | | Nombre del símbolo, `<ruta>::<nombre>` o identidad completa; un recurso (`resource::<tipo>::<id>`) o un canal (`channel::<broker>::<nombre>`). |
| `context` | `budget` | entero sin signo de 32 bits, opcional | 2.000 | Presupuesto de tokens de la respuesta. |
| `trace` | `from` | texto, opcional | | Inicio: un símbolo, una ruta (`GET /users/{}` o su contrato), un call site o un proceso (`process::<identidad del punto de entrada>`, solo `down`). |
| `trace` | `changes` | texto, opcional | | `uncommitted`: parte de los símbolos que toca el diff del árbol de trabajo. Es el único valor aceptado. |
| `trace` | `direction` | texto, opcional | `down` | `down` (lo que alcanza) o `up` (lo que lo alcanza). |
| `trace` | `budget` | entero sin signo de 32 bits, opcional | 2.000 | Presupuesto de tokens de la respuesta. |

- `trace` pide exactamente uno entre `from` y `changes`.
- El presupuesto predeterminado de 2.000 tokens es parte de la superficie. Un token es una estimación: un token por cuatro bytes de salida en UTF-8.
- Los tipos de `search`: `symbol`, `contract`, `route`, `resource`, `channel`, `document`, `package`, `drift`, `community`, `process`, `duplicate`, `ambiguous` y `unresolved`.

### Gramática de la salida

La salida de las tres herramientas es texto en líneas, y su gramática está especificada en el [documento de referencia de la gramática de salida](https://github.com/ClaudioMascaro/meada/blob/main/docs/reference/output-grammar.md) (en inglés, como la superficie MCP, [ADR 0008](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0008-mcp-surface-and-output-languages.md)). Están congelados:

- cada tipo de línea de cada herramienta y el significado de cada uno de sus campos;
- la notación de link `→[nivel: motivo]`, con los niveles `exact`, `declared`, `inferred`, `decided` y `ambiguous`;
- los identificadores de contrato, cada uno comenzando por su tipo de contrato: `http::`, `channel::`, `package::`, `resource::`;
- el pie de `trace` (`chain: <nivel más débil> · <n> hops · <n> repos · <n> tokens`);
- la nota que cierra una respuesta cortada por el presupuesto;
- la estimación de tokens.

Quien lee la salida debe saltar los tipos de línea y los campos que no conoce: así una versión menor añade algo sin romper a nadie.

### Línea de comandos

Comandos y flags del build predeterminado:

| Comando | Flags |
| --- | --- |
| `meada index` | `--workspace <manifiesto>` |
| `meada status` | `--workspace <manifiesto>` |
| `meada export` | `--workspace <manifiesto>`, `--repo <identidad>` |
| `meada mcp` | `--listen <dirección>`, `--workspace <manifiesto>` |
| `meada decide` | `--item <id>`, `--dry-run`, `--list`, `--prune`, `--decider`, `--show-request`, `--min-p <p>` |

Rigen también `meada help`, `--help` y `--version` (`-h`, `-V`).

- `--workspace`, `--repo` y `--listen` son del [modo servidor](server.md). En `meada export`, `--workspace` y `--repo` van juntos; en `meada mcp`, `--listen` y `--workspace` van juntos, y `--listen` sirve MCP por HTTP streamable, un endpoint por identidad de repositorio.
- `meada decide`: `--prune` no se combina con `--item`, `--dry-run` ni `--list`; `--show-request` y `--min-p` solo valen con `--decider`; `--min-p` va de 0 a 1 y su valor predeterminado es 0,5.
- Las variables de entorno del decisor: `MEADA_DECIDER_PROTOCOL` (`decisions` o `chat`, predeterminado `decisions`); `MEADA_DECIDER_URL` (predeterminada `https://openrouter.ai` con `decisions`, obligatoria con `chat`); `MEADA_DECIDER_KEY` (obligatoria con `decisions`, opcional con `chat`); `MEADA_DECIDER_MODEL` (predeterminado `typesafe/jev-1.14` con `decisions`, obligatorio con `chat`). Consulta [Decisiones](decisions.md).

### `meada.toml`

Todas las tablas y claves. Una clave desconocida es un error, con el archivo, la línea y la clave.

| Tabla | Clave | Tipo | Nota |
| --- | --- | --- | --- |
| `[[peer]]` | `repo` | texto, obligatorio | Identidad del repositorio par. |
| `[[peer]]` | `path` | texto, opcional | Pista de dónde está el clon, relativa a la raíz. |
| `[[peer]]` | `base_url_env` | lista de textos, opcional | Variables de donde este repositorio lee la URL base del par. |
| `[serve]` | `hosts` | lista de textos, opcional | Hosts en los que responde el repositorio. |
| `[serve]` | `prefix` | texto, opcional | Lo que el gateway añade delante de las rutas. |
| `[serve]` | `strip` | texto, opcional | Lo que el gateway quita del inicio de las rutas. |
| `[serve]` | `documents` | lista de textos, opcional | Documentos OpenAPI o AsyncAPI además de los hallados por nombre, relativos a la raíz. |
| `[[resource]]` | `kind` | texto, obligatorio | Identificador en minúsculas (letras, dígitos y guiones, empezando por letra). |
| `[[resource]]` | `id` | texto, obligatorio | No vacío. |
| `[[resource]]` | `role` | texto, obligatorio | `read`, `write` o `read-write`. |
| `[[resource]]` | `via` | lista de textos, opcional | Símbolos que hacen el acceso, como `<ruta>::<nombre calificado>`. |

Consulta la [configuración](configuration.md).

### `meada.decisions.toml`

Una tabla, `[[decision]]`, con estas claves. Una clave desconocida es un error.

| Clave | Tipo | Nota |
| --- | --- | --- |
| `kind` | texto, obligatorio | `ambiguous-link` o `dynamic-url`. |
| `call` | texto, obligatorio | El call site del ítem, como `meada export` lo nombra. |
| `target` | texto, obligatorio | `<identidad del repositorio proveedor>::<identificador del contrato>`. |
| `by` | texto, obligatorio | `human` o el id del modelo. |
| `p` | número, de 0 a 1 | Obligatorio cuando `by` es un modelo; prohibido cuando es `human`. |

Cada `call` aparece en como máximo una decisión; repetirlo es un error.

### `meada.workspace.toml`

El manifiesto de workspace del [modo servidor](server.md), leído por el build predeterminado:

| Tabla | Clave | Tipo | Nota |
| --- | --- | --- | --- |
| (raíz) | `index_dir` | texto, obligatorio | Dónde viven los índices, `<index_dir>/<identidad>/index.db`. |
| `[workspace]` | `source` | texto, opcional | `file` (predeterminado) o `store`; `store` exige un build con la feature `generation-pointer`, y el build predeterminado lo rechaza con un error. |
| `[[repository]]` | `repo` | texto, obligatorio | Identidad del repositorio. |
| `[[repository]]` | `path` | texto, obligatorio | Dónde está el mirror. |
| `[[repository]]` | `owner` | texto, opcional | Grupo OIDC responsable; metadato de la entrada. |
| `[[repository]]` | `added_by` | texto, opcional | Login de GitHub que añadió la entrada; metadato. |
| `[repository.serve]` | `hosts`, `prefix`, `strip`, `documents` | | Como `[serve]` de `meada.toml`. |
| `[[repository.resource]]` | `kind`, `id`, `role`, `via` | | Como `[[resource]]` de `meada.toml`. |
| `[[repository.peer]]` | `repo`, `base_url_env` | | Como `[[peer]]`, sin `path`. |

Las secciones de los módulos de nube (`[valkey]`, `[snapshot]`, `[oidc]` y las demás) solo existen en builds con la feature correspondiente.

### Formato de `meada export`

El export de un repositorio es JSON con sangría y terminado en un salto de línea, con toda lista ordenada: los mismos archivos dan bytes idénticos. Una lista vacía del nivel superior desaparece del documento, salvo `files`, `symbols`, `calls`, `providers`, `consumers` y `links`, que siempre aparecen; las listas anidadas (`members`, `steps`, `contracts`, `fields`) siempre se escriben. El documento no trae un campo de versión del formato: el formato solo crece por adiciones, y quien lee el export ignora los campos que no conoce.

| Campo | Contenido |
| --- | --- |
| `repository` | Identidad del repositorio. |
| `files` | `path`, `hash`. |
| `symbols` | `id`, `span` (`path`, `start`, `end`), `hash`. |
| `calls` | `id`, `caller`, `target`, `level`, `candidates`, `span`, `narrowed`, `decorates`. |
| `providers` | `contract`, `site`, `gateway`, `shape`. |
| `consumers` | `contract`, `site`, `via`, `shape`. |
| `bindings` | `contract`, `exchange`, `pattern`, `site`. |
| `documents` | `path`, `format` (`openapi` o `asyncapi`), `version`, `contracts`. |
| `publishes` | `exchange`, `key`, `site`. |
| `packages` | `name`, `version`, `repository`, `directory`, `ecosystem`. |
| `dependencies` | `manifest`, `field`, `name`, `alias`, `range`. |
| `imports` | `site`, `specifier`, `name`, `level`, `how`, `target`. |
| `communities` | `name`, `members`. |
| `links` | `consumer`, `provider`, `contract`, `level`, `reason`, `by`, `p`, `drift` (`missing`, `unchecked`). |
| `processes` | `entry_point`, `kind` (`route`, `export`, `channel` o `program`), `label`, `steps`. |
| `stale_decisions` | `call`, `target`, `cause`. |

Los campos de `shape` (`source`, `fields`, `open`, `scalars`, `declared`, `optional`, `guessed`) y de cada paso de `steps` (`depth`, `repository`, `level`, `via`, `node`, `reason`, `mark`, `candidates`, `contract`) son parte del formato, y también estos valores:

- `level`: `exact`, `declared`, `inferred`, `decided` y `ambiguous`;
- `shape.source`: un objeto con una clave, `document`, `zod`, `literals` o `reads`, cuyo valor es la ruta del archivo;
- `drift.missing`: cada ruta con su nivel; `drift.unchecked`: cada ruta con la ruta abierta que la cubre;
- `imports[].how`: `exports`, `types`, `module`, `main`, `path`, `outDir`, `moduleName`, `topLevel`, `crateName`, `modulePath`, `package` y `namespace`;
- `steps[].via`: `entry`, `call` y `link`;
- `steps[].mark`: `cycle`, `seen`, `ambiguous`, `unresolved`, `no-provider`, `produces`, `consumes` y `depth-limit`;
- `stale_decisions[].cause`: `call-gone`, `item-not-open`, `target-gone` y `not-candidate`.

## Qué es una ruptura

Después de la 1.0, una ruptura en cualquier elemento congelado exige una versión mayor. Romper es:

- quitar o renombrar un elemento (herramienta, parámetro, comando, flag, tabla, clave, campo del export);
- cambiar el tipo, el valor predeterminado o el significado de un parámetro, incluido el presupuesto predeterminado de 2.000 tokens;
- cambiar la gramática o el significado de un tipo de línea, de un campo o de una clave que ya existe;
- volver inválida una configuración que era válida.

## Qué es un cambio menor

Estos cambios entran en una versión menor:

- añadir un parámetro opcional;
- añadir un tipo de `search`;
- añadir un tipo de línea a la salida;
- añadir un campo al export;
- añadir una clave de configuración;
- añadir un tipo de contrato;
- añadir un language pack.

El [modo servidor](server.md) es un ejemplo: `--workspace`, `--repo` y `--listen`, el `meada.workspace.toml` y la línea `index behind the mirror: run meada index --workspace` de `context` solo añaden a la superficie, y nada de lo que existía cambia por ellos.

## Qué no está congelado

- La redacción de los mensajes de la línea de comandos, en cualquier idioma.
- El texto de las descripciones de las herramientas MCP, que sigue dentro del presupuesto de 1.000 tokens de la lista de herramientas.
- El archivo de índice y la organización de `.meada/`, reconstruidos cuando cambia el esquema ([ADR 0004](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0004-index-storage-and-freshness.md)).
- Qué links encuentra Meada para un repositorio. Mejorar la resolución es una corrección, no una ruptura: un link nuevo, un nivel más fuerte o un falso positivo menos pueden llegar en cualquier versión.

## La puerta de revisión

Todo cambio en la superficie congelada aparece como diff en la revisión, por dos mecanismos:

- los **goldens de la superficie**: la lista de herramientas MCP (nombres, esquemas de los parámetros y descripciones, con la verificación del límite de 1.000 tokens) y la ayuda en inglés de cada comando de la CLI;
- la **verificación de gramática**: los goldens de las respuestas renderizadas se comprueban contra la gramática de referencia, de modo que todo tipo de línea que aparece en ellos esté documentado.

Quien abre un PR que altera uno de esos goldens, un esquema de configuración o el formato del export dice en el PR si el cambio es una ruptura o un cambio menor, según las listas de arriba. Una ruptura solo entra en el próximo release mayor.
