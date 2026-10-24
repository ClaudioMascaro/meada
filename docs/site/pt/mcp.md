# Agentes (MCP)

`meada mcp` fala MCP por stdio. Ele responde sobre a primeira raiz (`roots`) que o cliente
informa, no Claude Code a pasta da sessão; sem raízes, sobre a pasta em que foi iniciado, que
então precisa estar dentro do repositório. Se o cliente falhar ou não responder ao `roots/list`
em 5 s, a chamada usa a última raiz que ele enviou nessa conexão, ou devolve um erro dizendo
isso; nunca cai na pasta do servidor por isso (que, numa sessão em um worktree, seria outro
checkout). Uma resposta sem raiz local vale como sem raízes (a pasta do servidor). Na
configuração do seu agente (por exemplo o `.mcp.json` do Claude Code), com `npx`:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["meada", "mcp"] }
  }
}
```

Com `meada` já no `PATH`, use `"command": "meada", "args": ["mcp"]`.
Os exemplos com `npx` pressupõem o pacote publicado e verificado, etapa ainda pendente desta
preparação de release; veja [Instalação](installation.md). Para HTTP e autenticação, siga
[Modo servidor](server.md); a configuração acima é a conexão local por stdio.

São três ferramentas, sempre em inglês, e nenhuma escreve:

- `search` acha um símbolo, uma rota ou um contrato pelo nome; com o tipo `resource`, um
  recurso pelo tipo ou pelo id; com o tipo `channel`, um canal (fila ou tópico) pelo broker ou
  pelo nome; com o tipo `ambiguous` ou `unresolved`, lista os itens abertos
  (veja Decisões);
- `context` descreve um símbolo: o trecho de código lido do disco, quem o chama, quem ele
  chama e os contratos que ele toca; num recurso (`resource::postgres::orders`), lista quem
  escreve e quem lê, com o símbolo e a linha, ou `(repository-level)` para o repositório
  inteiro; num canal (`channel::kafka::orders.created`), lista produtores e consumidores,
  inclusive os que chegam a ele por um curinga ou por um binding, com o nível e o motivo, e um
  lado cujo nome Meada não consegue avaliar aparece como `(unresolved)` e nunca é ligado;
- `trace` segue uma cadeia de chamadas por vários repositórios, rio abaixo (`down`) ou rio
  acima (`up`); com `changes: uncommitted`, parte do diff da árvore de trabalho e serve para
  analisar o impacto do que acabou de ser editado.

As respostas são linhas compactas dentro de um orçamento de tokens (2.000 por padrão, ajustável
a cada chamada). Cada ligação entre repositórios traz o nível e o motivo (`→[level: reason]`),
e o `trace` termina com um rodapé: o nível mais fraco da cadeia, o número de saltos, os
repositórios envolvidos e os tokens usados.

## Parâmetros e padrões

| Ferramenta | Parâmetro | Tipo JSON e padrão | Uso |
| --- | --- | --- | --- |
| `search` | `query` | string obrigatória | Nome sem distinção de maiúsculas/minúsculas; `""` busca tudo dentro do recorte escolhido. |
| `search` | `kind` | string ou `null`, opcional | Restringe ao tipo listado abaixo; ausente ou `null` busca símbolos e contratos de provedores/consumidores. |
| `context` | `symbol` | string obrigatória | Nome, `caminho::nome` ou identidade completa; também recurso ou canal, como descrito acima. |
| `trace` | `from` | string ou `null`, opcional | Símbolo, rota (`GET /orders/{}` ou contrato), chamada ou `process::<id-do-ponto-de-entrada>`. |
| `trace` | `changes` | string ou `null`, opcional | Só aceita `uncommitted`; substitui `from` como ponto de partida. |
| `trace` | `direction` | string ou `null`, padrão `down` | `down` mostra o que o ponto alcança; `up`, o que o alcança. Processos só aceitam `down`. |
| Todas | `budget` | inteiro de 0 a 4.294.967.295 ou `null`, padrão efetivo 2.000 | Orçamento da resposta; ausente e `null` usam o padrão. |

Passe exatamente um entre `from` e `changes` com valor não nulo. Modo servidor não aceita
`changes: uncommitted`, pois não tem árvore de trabalho para comparar.

Tipos de `search`:

| `kind` | Resultado |
| --- | --- |
| `symbol` | Símbolos pelo nome. |
| `contract` | Contratos dos lados provedores/consumidores. |
| `route` | Contratos HTTP por método/caminho. |
| `resource` | Recursos declarados por tipo/id. |
| `channel` | Filas e tópicos por broker/nome. |
| `document` | Documentos OpenAPI/AsyncAPI por caminho/título. |
| `package` | Pacotes pelo nome e seus consumidores. |
| `drift` | Campos lidos que faltam à resposta, por rota/caminho do campo. |
| `community` | Comunidades pelo nome. |
| `process` | Processos pelo rótulo do ponto de entrada, com a identidade para iniciar `trace`. |
| `ambiguous` | Itens de ligação ambígua e o comando para decidir. |
| `unresolved` | Itens de URL dinâmica e o comando para decidir. |

Sem `kind`, famílias especializadas como processos e documentos não são incluídas só por
existirem: escolha seu tipo para consultá-las. O índice não guarda texto-fonte; `context`
lê do disco o trecho que mostra. Nenhuma destas ferramentas chama um modelo.

A [referência normativa da gramática de saída](https://github.com/ClaudioMascaro/meada/blob/main/docs/reference/output-grammar.md)
especifica cada tipo de linha, identificadores, anotações, cortes e o cálculo de tokens.
Ela é escrita em inglês, como a superfície MCP.
