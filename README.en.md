# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · [Español](README.es.md)

Meada reads your repositories, maps who calls whom, and stitches one repository to the next
through the contracts they share: HTTP routes, queues, packages, shared resources. Coding
agents, and people too, query that map over MCP. No step of indexing uses an LLM.

## Installation

```sh
cargo install meada
```

Choose Cargo, a binary, the installer, npm or a container in the complete installation guide.

[Installation](https://claudiomascaro.github.io/meada/en/installation/) · [GitHub releases](https://github.com/ClaudioMascaro/meada/releases)

## Feed a knowledge base

`meada export` prints one repository's index as sorted JSON, good for `diff` and for feeding a
knowledge base. Per repository it carries the symbols (stable identity and span), the calls with
their levels, providers and consumers, links, imports, communities, processes and entry points.
It carries no source text: a reader who needs the snippet reads the span from the file.

One record per symbol, with its community, callers and callees, contracts and processes:

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

A record per community (`communities`) or per contract (`providers`, `consumers`, `links`)
follows the same lists.

- **Fan-in** across repositories: count, per target, the `calls`, the `links` (by `provider`) and
  the `imports` of all the exports together.
- **Exposure**: the entry points in `processes` (`route`, `export`, `channel`, `program`), the
  exported names and the package consumers (`packages`, `dependencies`, `imports`).

```sh
jq -s '[.[] | (.calls[]? | select(.target) | {site: .id, target, kind: "calls"}),
              (.links[]? | {site: .consumer, target: .provider, kind: "links"}),
              (.imports[]? | {site, target, kind: "imports"})]
  | unique_by([.kind, .site, .target])
  | group_by(.target)
  | map(. as $g | {target: $g[0].target}
      + ([$g[].kind] | group_by(.) | map({(.[0]): length}) | add))' *.export.json
```

The export is per repository: an organisation runs `meada export` on each one and joins the
results by symbol identity. Embeddings and any model step live downstream, outside Meada
(invariant 1, [ADR 0002](docs/adr/0002-product-invariants.md)).

## Documentation

- [Configuration](https://claudiomascaro.github.io/meada/en/configuration/)
- [Agents and MCP](https://claudiomascaro.github.io/meada/en/mcp/)
- [Server mode](https://claudiomascaro.github.io/meada/en/server/)
- [Console](https://claudiomascaro.github.io/meada/en/console/)
- [Capabilities and limits](https://claudiomascaro.github.io/meada/en/capabilities/)
- [Stability](https://claudiomascaro.github.io/meada/en/stability/)
- [Contribute](https://claudiomascaro.github.io/meada/en/contributing/)

[Apache-2.0](LICENSE).
