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

São três ferramentas, sempre em inglês, e nenhuma escreve:

- `search` acha um símbolo, uma rota ou um contrato pelo nome; com o tipo `resource`, um
  recurso pelo tipo ou pelo id; com o tipo `channel`, um canal (fila ou tópico) pelo broker ou
  pelo nome; com o tipo `ambiguous` ou `unresolved`, lista os itens abertos
  (veja Decisões);
- `context` descreve um símbolo: o trecho de código lido do disco, quem o chama, quem ele
  chama e os contratos que ele toca; as anotações, atributos e decoradores escritos sobre uma
  definição (`@Override`, `[Fact]`, `@app.get("/x")`) são chamadas do escopo que a envolve, e por
  isso o `context` as lista à parte, na seção `annotations:`, cada uma com a definição que ela
  decora (o ponto de chamada do export traz isso em `decorates`); num recurso (`resource::postgres::orders`), lista quem
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
