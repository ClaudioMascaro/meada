# Estabilidad en 1.x

Este árbol declara **0.1.0** y prepara la 1.0. La serie 0.x no tiene promesa de estabilidad.
La política siguiente entra en vigor **cuando se publique la 1.0** y cubre las versiones 1.x;
este documento no anuncia una release ni afirma que se hayan superado los gates de aceptación.

## Superficie congelada

Los inventarios enlazados abajo forman parte de esta política. Registran la superficie
existente, con sus tipos, valores predeterminados, validaciones y significados.

| Superficie | Qué queda congelado en 1.x | Inventario completo |
| --- | --- | --- |
| MCP | Los nombres `search`, `context` y `trace`, los nombres/tipos/valores predeterminados de los parámetros de abajo y la gramática de salida. | [Agentes y MCP](mcp.md) y la [gramática normativa de salida](https://github.com/ClaudioMascaro/meada/blob/main/docs/reference/output-grammar.md). |
| CLI | Comandos, subcomandos y flags, incluidos los disponibles solo con la feature correspondiente compilada. Incluye comandos locales, `snapshot`, `github-app`, `ui`, `workspace`, ayuda y versión. | [Comandos](cli.md), con argumentos, valores predeterminados y restricciones. |
| `meada.toml` local | `[[peer]]`: `repo`, `path`, `base_url_env`; `[serve]`: `hosts`, `prefix`, `strip`, `documents`; `[[resource]]`: `kind`, `id`, `role`, `via`. | [Configuración](configuration.md), cada tabla y clave con su tipo, valor predeterminado y validación. |
| `meada.decisions.toml` | `[[decision]]`: `kind`, `call`, `target`, `by`, `p`, incluida la unicidad de `call` y las reglas condicionales de `p`. | [Decisiones](decisions.md), con los tipos y valores aceptados. |
| `meada export` | Campos, tipos y significados del JSON canónico, incluidos registros anidados, enums, `null`, omisiones y ordenación. | [Exportación JSON](export.md). |

### Parámetros MCP

| Herramienta | Parámetro | Tipo JSON | Valor predeterminado y significado |
| --- | --- | --- | --- |
| `search` | `query` | string obligatorio | Sin valor predeterminado; nombre que buscar, sin distinguir mayúsculas/minúsculas. |
| `search` | `kind` | string o `null`, opcional | Ausente o `null`: símbolos y contratos de proveedores/consumidores. Un string elige uno de los tipos de búsqueda del [inventario MCP](mcp.md). |
| `context` | `symbol` | string obligatorio | Sin valor predeterminado; símbolo o identidad de recurso/canal que describir. |
| `trace` | `from` | string o `null`, opcional | Ausente o `null`: no elige una identidad como inicio. Acepta símbolo, ruta, llamada o `process::<identidad-del-punto-de-entrada>`. |
| `trace` | `changes` | string o `null`, opcional | Ausente o `null`: no elige el diff. Solo acepta `uncommitted`. |
| `trace` | `direction` | string o `null`, opcional | Ausente o `null`: `down`; también acepta `up`. Los procesos solo aceptan `down`. |
| Todas | `budget` | entero de 0 a 4.294.967.295 o `null`, opcional | Ausente o `null`: **2.000 tokens** por respuesta. |

`trace` exige que exactamente uno de `from` y `changes` tenga un valor distinto de `null`.
El modo servidor rechaza `changes: uncommitted`. Los valores predeterminados anteriores son
el comportamiento efectivo, incluso si el cliente omite la propiedad o envía `null`.

La gramática normativa está escrita en inglés, como la superficie MCP. Especifica los tipos
de línea, identificadores de contrato, niveles/motivos, pie, marcador de corte y estimación
de tokens: un token por cuatro bytes UTF-8, redondeando hacia arriba. Los lectores de 1.x
deben **ignorar los tipos de línea y campos que no conocen**, incluidos campos nuevos en
tipos de línea conocidos. Los lectores del export también deben ignorar campos desconocidos.

## Cuándo cambiar la versión

Después de publicar la 1.0, una ruptura en cualquier elemento congelado exige una versión
**major**, como 2.0. Es una ruptura:

- eliminar o renombrar un elemento;
- cambiar el tipo, el valor predeterminado o el significado de un parámetro;
- cambiar la gramática o el significado de un tipo de línea, campo o clave existente;
- invalidar una configuración que era válida.

Son cambios **minor**, como 1.1: añadir un parámetro opcional, un tipo de `search`, un tipo
de línea, un campo del export, una clave de configuración, un tipo de contrato o un paquete
de lenguaje. Las adiciones preservan los elementos existentes según las reglas anteriores.

## Qué no queda congelado

- El texto de los mensajes de la CLI, en cualquier idioma.
- El texto de las descripciones de las herramientas MCP, todavía dentro del presupuesto de
  **1.000 tokens** de la lista de herramientas (descripciones y schemas de parámetros).
- El archivo del índice y la disposición de `.meada/`; un cambio de schema puede reconstruir
  el índice.
- Qué enlaces encuentra Meada para un código concreto. Mejorar la resolución es una
  corrección, no una ruptura de la promesa de estabilidad.

## Gate de revisión de la superficie

Antes de aprobar un cambio, compáralo con los inventarios y revisa los diffs de estos
[golden files](https://github.com/ClaudioMascaro/meada/tree/main/tests/golden):

- [`surface.mcp-tools.json`](https://github.com/ClaudioMascaro/meada/blob/main/tests/golden/surface.mcp-tools.json): nombres, schemas y descripciones de las tres herramientas. [`tests/it/mcp.rs`](https://github.com/ClaudioMascaro/meada/blob/main/tests/it/mcp.rs) también verifica el techo de 1.000 tokens y el valor predeterminado efectivo de 2.000 por herramienta.
- [`surface.cli.default.en.txt`](https://github.com/ClaudioMascaro/meada/blob/main/tests/golden/surface.cli.default.en.txt) y [`surface.cli.cloud.en.txt`](https://github.com/ClaudioMascaro/meada/blob/main/tests/golden/surface.cli.cloud.en.txt): ayuda en inglés de todos los comandos de los builds predeterminado y completo. [`tests/it/cli.rs`](https://github.com/ClaudioMascaro/meada/blob/main/tests/it/cli.rs) recorre los subcomandos.
- [`tests/it/grammar.rs`](https://github.com/ClaudioMascaro/meada/blob/main/tests/it/grammar.rs): comprueba los inventarios de la referencia normativa y valida cada golden de respuesta renderizada contra la gramática.

Estos checks hacen visible un cambio para revisión; actualizar un golden no hace compatible
una ruptura con 1.x. Los cambios solo de redacción siguen fuera del congelamiento descrito
arriba, aunque aparezcan en el diff de un golden.
