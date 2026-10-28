# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · [Español](README.es.md)

Meada lê os seus repositórios, monta o mapa de quem chama quem e costura um repositório no
outro pelos contratos que eles compartilham: rotas HTTP, filas, pacotes, recursos compartilhados.
Agentes de código, e gente também, consultam esse mapa por MCP. Nenhuma etapa da indexação usa
LLM.

## Instalação

```sh
cargo install meada
```

Comece pela instalação completa e escolha entre Cargo, binário, instalador, npm ou contêiner.

[Instalação](https://claudiomascaro.github.io/meada/installation/) · [GitHub releases](https://github.com/ClaudioMascaro/meada/releases)

## Alimentar uma base de conhecimento

`meada export` imprime o índice de um repositório como JSON ordenado, bom para `diff` e para
alimentar uma base de conhecimento. Por repositório, ele traz os símbolos (identidade estável e
span), as chamadas com seus níveis, provedores e consumidores, links, imports, comunidades,
processos e pontos de entrada. Não traz texto-fonte: quem precisa do trecho lê o span no arquivo.

Um registro por símbolo, com comunidade, quem chama e quem é chamado, contratos e processos:

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

Um registro por comunidade (`communities`) ou por contrato (`providers`, `consumers`, `links`)
segue as mesmas listas.

- **Fan-in** entre repositórios: conte, por alvo, as `calls`, os `links` (pelo `provider`) e os
  `imports` de todos os exports juntos.
- **Exposição**: os pontos de entrada em `processes` (`route`, `export`, `channel`, `program`), os
  nomes exportados e os consumidores de pacote (`packages`, `dependencies`, `imports`).

```sh
jq -s '[.[] | (.calls[]? | select(.target) | {target, kind: "calls"}),
              (.links[]? | {target: .provider, kind: "links"}),
              (.imports[]? | {target, kind: "imports"})]
  | group_by(.target)
  | map(. as $g | {target: $g[0].target}
      + ([$g[].kind] | group_by(.) | map({(.[0]): length}) | add))' *.export.json
```

O export é por repositório: uma organização roda `meada export` em cada um e junta os resultados
pela identidade dos símbolos. Embeddings e qualquer etapa com modelo ficam a jusante, fora do
Meada (invariante 1, [ADR 0002](docs/adr/0002-product-invariants.md)).

## Documentação

- [Configuração](https://claudiomascaro.github.io/meada/configuration/)
- [Agentes e MCP](https://claudiomascaro.github.io/meada/mcp/)
- [Modo servidor](https://claudiomascaro.github.io/meada/server/)
- [Console](https://claudiomascaro.github.io/meada/console/)
- [Capacidades e limites](https://claudiomascaro.github.io/meada/capabilities/)
- [Participar](https://claudiomascaro.github.io/meada/contributing/)

[Apache-2.0](LICENSE).
