# Exportación JSON

`meada export` imprime el JSON canónico de **un repositorio**: su índice, los enlaces que
tocan sus consumidores/proveedores y los procesos de sus puntos de entrada. Los procesos
pueden cruzar pares. Para elegir un repositorio en modo servidor, usa
`meada export --workspace meada.workspace.toml --repo example.com/acme/api`;
consulta [Comandos](cli.md) para `--generations` y las features necesarias.

La salida tiene sangría y un salto de línea final. No contiene código fuente ni valores
de variables de entorno. No hay campo de versión del formato: identifica al productor por
la versión del paquete/binario. Los lectores deben tolerar campos adicionales. Los tipos
compartidos están en
[`meada-core/src/export.rs`](https://github.com/ClaudioMascaro/meada/blob/main/meada-core/src/export.rs)
y [`shape.rs`](https://github.com/ClaudioMascaro/meada/blob/main/meada-core/src/shape.rs).

## Objeto raíz

`T[]` significa una lista de registros de tipo `T`; `string[]`, una lista de strings. Los
nombres de tipos explican los objetos, no son claves adicionales del JSON.

| Campo | Tipo | Emisión |
| --- | --- | --- |
| `repository` | string | Siempre: identidad del repositorio. |
| `files` | `File[]` | Siempre, incluso `[]`. |
| `symbols` | `Symbol[]` | Siempre, incluso `[]`. |
| `calls` | `CallSite[]` | Siempre, incluso `[]`. |
| `providers` | `Provider[]` | Siempre, incluso `[]`. |
| `consumers` | `Consumer[]` | Siempre, incluso `[]`. |
| `links` | `Link[]` | Siempre, incluso `[]`. |
| `bindings` | `Binding[]` | Se omite si está vacío. |
| `documents` | `Document[]` | Se omite si está vacío. |
| `publishes` | `Publish[]` | Se omite si está vacío. |
| `packages` | `Package[]` | Se omite si está vacío. |
| `dependencies` | `Dependency[]` | Se omite si está vacío. |
| `imports` | `Import[]` | Se omite si está vacío. |
| `communities` | `Community[]` | Se omite si está vacío. |
| `processes` | `Process[]` | Se omite si está vacío. |
| `stale_decisions` | `Stale[]` | Se omite si está vacío. |

Las listas omitidas se leen como `[]`. Un repositorio sin registros puede representarse así:

```json
{
  "repository": "example.com/acme/web",
  "files": [],
  "symbols": [],
  "calls": [],
  "providers": [],
  "consumers": [],
  "links": []
}
```

## Archivos, símbolos y llamadas

| Registro | Campos siempre emitidos | Campos omitidos |
| --- | --- | --- |
| `File` | `path: string`, ruta relativa; `hash: string`, BLAKE3 de los bytes del archivo, 64 dígitos hexadecimales en minúsculas. | Ninguno. |
| `Span` | `path: string`; `start: integer`; `end: integer` (almacenados como enteros de 64 bits con signo). | Ninguno. |
| `Symbol` | `id: string`, identidad del símbolo; `span: Span`. | Ninguno. |
| `CallSite` | `id: string`, identidad de la llamada; `caller: string`, quien la contiene; `target: string o null`, símbolo resuelto; `level: Level o null`; `span: Span`. | `candidates: string[]` si está vacío. |

**`Span.start` y `Span.end` son offsets de bytes UTF-8 desde cero, con fin exclusivo**,
no números de línea ni posiciones de caracteres. El fragmento es `bytes[start..end]` en
`path`; comprueba el hash antes de usar posiciones de una exportación antigua. Las líneas
mostradas por MCP se calculan a partir de esos offsets.

Una identidad de símbolo combina `<repositorio>::<ruta>::<nombre-cualificado>`; una llamada
añade la identidad de su call site, como
`example.com/acme/web::src/orders.ts::load::fetch#0`. Las rutas son relativas al repositorio.

## Contratos y paquetes

| Registro | Campos siempre emitidos | Campos omitidos si están ausentes |
| --- | --- | --- |
| `Provider` | `contract: string`; `site: string`, lado proveedor. | `gateway: string`, ruta reescrita; `shape: Shape`. |
| `Consumer` | `contract: string o null`; `site: string`, lado consumidor; `via: string o null`, como `host:api.example.com` o `env:API_URL`. | `shape: Shape`. |
| `Binding` | `contract: string`, canal de la cola; `exchange: string`; `pattern: string`, patrón de enrutamiento; `site: string`, llamada que declara el binding. | Ninguno. |
| `Publish` | `exchange: string`; `key: string`, clave de enrutamiento; `site: string`, llamada que publica. | Ninguno. |
| `Document` | `path: string`; `format: Format`; `version: string o null`, versión declarada por el documento; `contracts: string[]`, únicos y ordenados. | Ninguno. |
| `Package` | `name: string`; `repository: string`; `directory: string`, directorio del manifiesto relativo a la raíz (`.` en la raíz). | `version: string`; `ecosystem: string`, omitido para npm, actualmente `pypi`, `cargo` o `go` para los demás. |
| `Dependency` | `manifest: string`, ruta; `field: string`, clave del manifiesto; `name: string`, nombre del paquete. | `alias: string`, clave renombrada de Cargo; `range: string`, restricción tal como se escribió en el manifiesto. |
| `Import` | `site: string`, llamada mediante el nombre importado; `specifier: string`, módulo importado; `name: string`, nombre usado; `target: string`, símbolo alcanzado; `level: Level`; `how: How`. | Ninguno. |
| `Link` | `consumer: string`; `provider: string`; `contract: string`; `level: Level`; `reason: string`. | `by: string`, persona/modelo de un enlace decidido; `p: number`, probabilidad declarada por el modelo; `drift: Drift`. |

`site`, `consumer` y `provider` pueden identificar una llamada, símbolo, archivo, manifiesto
o repositorio entero, según el tipo de contrato. Un lado solo documentado también tiene una
identidad, por ejemplo `<repositorio>::openapi.yaml::GET /orders/{}`. Los contratos tienen
identificadores como `http::GET::/orders/{}`, `resource::postgres::orders`,
`channel::kafka::orders.created` y `package::npm::@example/shared`.

Una llamada HTTP no resuelta tiene `Consumer.contract: null`. Un lado de canal no resuelto
puede tener `channel::<broker>` sin nombre; se muestra, pero nunca se enlaza. Los campos
anulables `target`, `level`, `contract`, `via` y `Document.version` de las tablas se emiten
como `null`, en vez de omitirse. Eso los distingue de los opcionales de la última columna.

## Comunidades, procesos y decisiones obsoletas

| Registro | Campos siempre emitidos | Campos omitidos |
| --- | --- | --- |
| `Community` | `name: string`, nombre local de la comunidad; `members: string[]`, identidades ordenadas de los miembros. | Ninguno. |
| `Process` | `entry_point: string`, identidad del punto de entrada; `kind: EntryKind`; `label: string`; `steps: Step[]`, pasos en profundidad. | Ninguno. |
| `Step` | `depth: integer` sin signo; `repository: string`; `level: Level o null`, nivel del paso que lo alcanzó; `via: Via`; `node: string`, identidad del nodo o de la llamada a la que se refiere una nota. | `reason: string`, `mark: Mark`, `contract: string` si están ausentes; `candidates: string[]` si está vacío. |
| `Stale` | `call: string`; `target: string`; `cause: Cause`. | Ninguno. |

`Step.level` se emite incluso si es `null`. Las marcas de ciclo, nodo ya visitado o ambigüedad
explican por qué una rama se detiene. Una decisión que espera un par ausente no es obsoleta
y no entra en `stale_decisions`; consulta [Decisiones](decisions.md).

## Shapes y drift

| Campo de `Shape` | Tipo y significado | Emisión |
| --- | --- | --- |
| `source` | `Source`, descrito abajo. | Siempre. |
| `fields` | `string[]`, rutas de campos presentes. | Siempre, incluso `[]`. |
| `open` | `string[]`, partes cuyos campos no pueden enumerarse. | Se omite si está vacío. |
| `scalars` | `string[]`, hojas escalares; no se compara nada por debajo. | Se omite si está vacío. |
| `declared` | `string[]`, campos del consumidor procedentes de un tipo declarado. | Se omite si está vacío. |
| `optional` | `string[]`, campos opcionales del tipo que una lectura no hizo obligatorios. | Se omite si está vacío. |
| `guessed` | `string[]`, partes del proveedor de una importación de paquete resuelta por debajo de `exact`. | Se omite si está vacío. |

Todas las listas de rutas son únicas y ordenadas. Las rutas usan puntos y `[]`: `items`,
`items[]`, `items[].id`; un payload que ya es un array usa `[].id`; `""` representa toda
la raíz en partes abiertas/escalares. No se exportan valores ni se comparan tipos.
`Shape.imports` pertenece al almacenamiento interno: las consultas lo rellenan y eliminan
antes de exportar. **No es un campo del export canónico**; no lo confundas con `imports`
en la raíz, que contiene registros públicos `Import`.

`Source` es un objeto con **una** clave y una ruta string, como `{"document":"openapi.yaml"}`,
`{"zod":"src/orders.ts"}`, `{"literals":"src/orders.ts"}` o `{"reads":"src/orders.ts"}`.
No es una string ni un par `type/path`. La prioridad del proveedor es documento, después
zod, después literales; `reads` describe lecturas del consumidor.

`Drift` tiene dos mapas opcionales, omitidos si están vacíos (leídos como `{}`):

- `missing: { ruta: Level }`: primer campo ausente de cada rama. Actualmente `exact` si
  ambos lados son declarados y la parte del proveedor no fue adivinada, o `inferred` en
  los demás casos. Este nivel de diagnóstico es independiente del nivel del enlace.
- `unchecked: { rutaDelConsumidor: rutaAbiertaDelProveedor }`: strings de partes no
  verificadas, que nunca cuentan como drift. La ruta abierta puede ser `""` para la raíz.

Las claves de los mapas están ordenadas. Los campos opcionales ausentes no producen
`missing`; una parte `guessed` limita su diagnóstico a `inferred`. Un enlace puede contener
solo `unchecked`; si todos los campos fueron verificados y están presentes, `drift` se omite.

## Valores enumerados del JSON

| Tipo | Valores exactos |
| --- | --- |
| `Level` | `exact`, `declared`, `inferred`, `decided`, `ambiguous` |
| `Format` | `openapi`, `asyncapi` |
| `How` | `exports`, `types`, `module`, `main`, `path`, `outDir`, `moduleName`, `topLevel`, `crateName`, `modulePath` |
| `EntryKind` | `route`, `export`, `channel`, `program` |
| `Via` | `entry`, `call`, `link` |
| `Mark` | `cycle`, `seen`, `ambiguous`, `unresolved`, `no-provider`, `produces`, `consumes`, `depth-limit` |
| `Cause` | `call-gone`, `item-not-open`, `target-gone`, `not-candidate` |

`How` usa camelCase en JSON aunque la salida de texto diga `module name` o `crate name`.
`Cause` distingue llamada eliminada, ítem resuelto/no abierto, destino eliminado y destino
fuera de los candidatos, respectivamente.

## Orden determinista

El mismo workspace produce los mismos bytes. El orden es semántico, no alfabético en todo
el JSON: archivos por ruta; símbolos/llamadas por identidad y span; proveedores por
contrato/site; consumidores por site/contrato/via; bindings por contrato/exchange/pattern/site;
publicaciones por exchange/key/site; documentos por ruta; paquetes por nombre/directorio;
dependencias por manifiesto/campo/nombre/alias; imports por site/destino; comunidades por
nombre y miembros; enlaces por consumidor/proveedor; procesos por punto de entrada y pasos
en profundidad. Consulta ejemplos completos en los
[goldens públicos](https://github.com/ClaudioMascaro/meada/tree/main/tests/golden).
