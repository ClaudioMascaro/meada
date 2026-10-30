# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · [Español](README.es.md)

Meada lee tus repositorios, arma el mapa de quién llama a quién y cose un repositorio con otro
a través de los contratos que comparten: rutas HTTP, colas, paquetes, recursos compartidos. Los
agentes de código, y también las personas, consultan ese mapa por MCP. Ninguna etapa de la
indexación usa un LLM.

## Instalación

```sh
cargo install meada
```

Elija Cargo, un binario, el instalador, npm o un contenedor en la guía completa de instalación.

[Instalación](https://claudiomascaro.github.io/meada/es/installation/) · [GitHub releases](https://github.com/ClaudioMascaro/meada/releases)

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

- [Configuración](https://claudiomascaro.github.io/meada/es/configuration/)
- [Agentes y MCP](https://claudiomascaro.github.io/meada/es/mcp/)
- [Modo servidor](https://claudiomascaro.github.io/meada/es/server/)
- [Consola](https://claudiomascaro.github.io/meada/es/console/)
- [Capacidades y límites](https://claudiomascaro.github.io/meada/es/capabilities/)
- [Estabilidad](https://claudiomascaro.github.io/meada/es/stability/)
- [Participar](https://claudiomascaro.github.io/meada/es/contributing/)

[Apache-2.0](LICENSE).
