# Export JSON

`meada export` imprime o JSON canônico de **um repositório**: seu índice, as ligações que
tocam seus consumidores/provedores e os processos de seus pontos de entrada. Os processos
podem atravessar os pares. Para escolher um repositório em modo servidor, use
`meada export --workspace meada.workspace.toml --repo example.com/acme/api`; veja
[Comandos](cli.md) para `--generations` e as features necessárias.

A saída tem indentação e uma quebra de linha final. Não contém texto-fonte nem valores de
variáveis de ambiente. Não há campo de versão do formato: identifique o produtor pela versão
do pacote/binário. Leitores devem tolerar campos adicionais. Os tipos compartilhados estão em
[`meada-core/src/export.rs`](https://github.com/ClaudioMascaro/meada/blob/main/meada-core/src/export.rs)
e [`shape.rs`](https://github.com/ClaudioMascaro/meada/blob/main/meada-core/src/shape.rs).

## Objeto raiz

`T[]` significa uma lista de registros `T`; `string[]`, uma lista de strings. Os nomes de
tipos abaixo explicam os objetos, não são chaves adicionais no JSON.

| Campo | Tipo | Emissão |
| --- | --- | --- |
| `repository` | string | Sempre: identidade do repositório. |
| `files` | `File[]` | Sempre, inclusive `[]`. |
| `symbols` | `Symbol[]` | Sempre, inclusive `[]`. |
| `calls` | `CallSite[]` | Sempre, inclusive `[]`. |
| `providers` | `Provider[]` | Sempre, inclusive `[]`. |
| `consumers` | `Consumer[]` | Sempre, inclusive `[]`. |
| `links` | `Link[]` | Sempre, inclusive `[]`. |
| `bindings` | `Binding[]` | Omitido se vazio. |
| `documents` | `Document[]` | Omitido se vazio. |
| `publishes` | `Publish[]` | Omitido se vazio. |
| `packages` | `Package[]` | Omitido se vazio. |
| `dependencies` | `Dependency[]` | Omitido se vazio. |
| `imports` | `Import[]` | Omitido se vazio. |
| `communities` | `Community[]` | Omitido se vazio. |
| `processes` | `Process[]` | Omitido se vazio. |
| `stale_decisions` | `Stale[]` | Omitido se vazio. |

As listas omitidas são lidas como `[]`. Um repositório sem registros pode ser representado por:

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

## Arquivos, símbolos e chamadas

| Registro | Campos sempre emitidos | Campos omitidos |
| --- | --- | --- |
| `File` | `path: string`, caminho relativo; `hash: string`, BLAKE3 dos bytes do arquivo, 64 dígitos hexadecimais minúsculos. | Nenhum. |
| `Span` | `path: string`; `start: integer`; `end: integer` (armazenados como inteiros de 64 bits com sinal). | Nenhum. |
| `Symbol` | `id: string`, identidade do símbolo; `span: Span`. | Nenhum. |
| `CallSite` | `id: string`, identidade da chamada; `caller: string`, quem a contém; `target: string ou null`, símbolo resolvido; `level: Level ou null`; `span: Span`. | `candidates: string[]` se vazio. |

**`Span.start` e `Span.end` são offsets de bytes UTF-8 desde zero, com fim exclusivo**,
não números de linha nem posições em caracteres. O trecho é `bytes[start..end]` no arquivo
identificado por `path`; confira o hash antes de usar posições de um export antigo.
As linhas exibidas por MCP são calculadas a partir desses offsets.

Uma identidade de símbolo combina `<repositório>::<caminho>::<nome-qualificado>`; uma chamada
acrescenta a identidade do call site, como
`example.com/acme/web::src/orders.ts::load::fetch#0`. Caminhos são relativos ao repositório.

## Contratos e pacotes

| Registro | Campos sempre emitidos | Campos omitidos quando ausentes |
| --- | --- | --- |
| `Provider` | `contract: string`, contrato; `site: string`, lado que o provê. | `gateway: string`, caminho reescrito; `shape: Shape`. |
| `Consumer` | `contract: string ou null`; `site: string`, lado consumidor; `via: string ou null`, como `host:api.example.com` ou `env:API_URL`. | `shape: Shape`. |
| `Binding` | `contract: string`, canal da fila; `exchange: string`; `pattern: string`, padrão de roteamento; `site: string`, chamada que declara o binding. | Nenhum. |
| `Publish` | `exchange: string`; `key: string`, chave de roteamento; `site: string`, chamada que publica. | Nenhum. |
| `Document` | `path: string`; `format: Format`; `version: string ou null`, versão declarada pelo documento; `contracts: string[]`, únicos e ordenados. | Nenhum. |
| `Package` | `name: string`; `repository: string`; `directory: string`, diretório do manifesto relativo à raiz (`.` na raiz). | `version: string`; `ecosystem: string`, omitido para npm, presente como `pypi`, `cargo` ou `go` nos outros ecossistemas atuais. |
| `Dependency` | `manifest: string`, caminho; `field: string`, chave do manifesto; `name: string`, nome do pacote. | `alias: string`, chave renomeada do Cargo; `range: string`, restrição escrita no manifesto. |
| `Import` | `site: string`, chamada pelo nome importado; `specifier: string`, módulo importado; `name: string`, nome usado; `target: string`, símbolo alcançado; `level: Level`; `how: How`. | Nenhum. |
| `Link` | `consumer: string`; `provider: string`; `contract: string`; `level: Level`; `reason: string`. | `by: string`, pessoa/modelo de ligação decidida; `p: number`, probabilidade declarada pelo modelo; `drift: Drift`. |

`site`, `consumer` e `provider` podem nomear chamada, símbolo, arquivo, manifesto ou o
repositório inteiro, conforme o tipo de contrato. Um lado só documentado também tem identidade,
por exemplo `<repositório>::openapi.yaml::GET /orders/{}`. Contratos têm identificadores como
`http::GET::/orders/{}`, `resource::postgres::orders`, `channel::kafka::orders.created` e
`package::npm::@example/shared`.

Uma chamada HTTP não avaliada tem `Consumer.contract: null`. Um lado de canal não avaliado
pode ter `channel::<broker>` sem nome; é mostrado, mas não ganha ligação. `target`, `level`,
`contract`, `via` e `Document.version` marcados como anuláveis na tabela são emitidos com
`null`, não simplesmente omitidos. Isso difere dos campos opcionais da última coluna.

## Comunidades, processos e decisões obsoletas

| Registro | Campos sempre emitidos | Campos omitidos |
| --- | --- | --- |
| `Community` | `name: string`, nome local da comunidade; `members: string[]`, identidades ordenadas dos membros. | Nenhum. |
| `Process` | `entry_point: string`, identidade do ponto de entrada; `kind: EntryKind`; `label: string`; `steps: Step[]`, passos em profundidade. | Nenhum. |
| `Step` | `depth: integer` sem sinal; `repository: string`; `level: Level ou null`, nível do passo que o alcançou; `via: Via`; `node: string`, identidade do nó ou da chamada a que uma nota se refere. | `reason: string`, `mark: Mark`, `contract: string` quando ausentes; `candidates: string[]` quando vazio. |
| `Stale` | `call: string`; `target: string`; `cause: Cause`. | Nenhum. |

`Step.level` é emitido mesmo quando `null`. Marcas de ciclo, nó já visitado ou ambiguidade
mostram por que um ramo não continua. Uma decisão que espera um par ausente não é obsoleta
e não entra em `stale_decisions`; veja [Decisões](decisions.md).

## Shapes e drift

| Campo de `Shape` | Tipo e significado | Emissão |
| --- | --- | --- |
| `source` | `Source`, origem descrita abaixo. | Sempre. |
| `fields` | `string[]`, caminhos de campos presentes. | Sempre, inclusive `[]`. |
| `open` | `string[]`, partes cujos campos não podem ser enumerados. | Omitido se vazio. |
| `scalars` | `string[]`, folhas escalares; nada abaixo delas é comparado. | Omitido se vazio. |
| `declared` | `string[]`, campos do consumidor vindos de um tipo declarado. | Omitido se vazio. |
| `optional` | `string[]`, campos opcionais do tipo que uma leitura não tornou obrigatórios. | Omitido se vazio. |
| `guessed` | `string[]`, partes do provedor vindas de importação de pacote sem resolução `exact`. | Omitido se vazio. |

Todas essas listas de caminhos são únicas e ordenadas. Caminhos usam pontos e `[]`:
`items`, `items[]`, `items[].id`; um payload que já é array usa `[].id`; `""` representa a
raiz inteira em partes abertas/escalares. Não se exportam valores nem se comparam tipos.
`Shape.imports` pertence ao armazenamento interno: as consultas o preenchem e o removem
antes de exportar. Ele **não é campo do export canônico**; não o confunda com `imports`
na raiz, que contém registros públicos `Import`.

`Source` é um objeto com **uma** chave e caminho string, como `{"document":"openapi.yaml"}`,
`{"zod":"src/orders.ts"}`, `{"literals":"src/orders.ts"}` ou `{"reads":"src/orders.ts"}`.
Não é uma string nem um par de chaves `type/path`. A origem do provedor dá preferência a
documento, depois zod, depois literais; `reads` descreve leituras do consumidor.

`Drift` é um objeto com dois mapas opcionais, omitidos se vazios (lidos como `{}`):

- `missing: { caminho: Level }`: o primeiro campo ausente de cada ramo. Hoje o nível é
  `exact` quando ambos os lados são declarados e o provedor não foi adivinhado naquela parte;
  nos demais casos é `inferred`. É o nível do diagnóstico do campo, separado do nível da ligação.
- `unchecked: { caminhoDoConsumidor: caminhoAbertoDoProvedor }`: strings; partes não verificadas,
  que nunca contam como drift. O caminho aberto pode ser `""` para a raiz.

As chaves dos mapas são ordenadas. Campos opcionais ausentes não geram `missing`; uma parte
`guessed` limita seu diagnóstico a `inferred`. Uma ligação pode ter só `unchecked`; se todos
os campos foram verificados e estão presentes, `drift` é omitido.

## Valores enumerados no JSON

| Tipo | Valores exatos |
| --- | --- |
| `Level` | `exact`, `declared`, `inferred`, `decided`, `ambiguous` |
| `Format` | `openapi`, `asyncapi` |
| `How` | `exports`, `types`, `module`, `main`, `path`, `outDir`, `moduleName`, `topLevel`, `crateName`, `modulePath` |
| `EntryKind` | `route`, `export`, `channel`, `program` |
| `Via` | `entry`, `call`, `link` |
| `Mark` | `cycle`, `seen`, `ambiguous`, `unresolved`, `no-provider`, `produces`, `consumes`, `depth-limit` |
| `Cause` | `call-gone`, `item-not-open`, `target-gone`, `not-candidate` |

`How` usa camelCase no JSON, mesmo quando a saída de texto diz `module name` ou `crate name`.
`Cause` distingue chamada removida, item já resolvido/não aberto, alvo removido e alvo fora
dos candidatos, respectivamente.

## Ordem determinística

O mesmo workspace produz os mesmos bytes. A ordem é semântica, não uma ordenação alfabética
de todo JSON: arquivos por caminho; símbolos/chamadas por identidade e span; provedores por
contrato/site; consumidores por site/contrato/via; bindings por contrato/exchange/pattern/site;
publicações por exchange/key/site; documentos por caminho; pacotes por nome/diretório;
dependências por manifesto/campo/nome/alias; imports por site/alvo; comunidades por nome e
membros; ligações por consumidor/provedor; processos por ponto de entrada e passos em
profundidade. Veja exemplos completos nos
[goldens públicos](https://github.com/ClaudioMascaro/meada/tree/main/tests/golden).
