# Agentes (MCP)

`meada mcp` fala MCP por stdio. Ele responde sobre a primeira raiz (`roots`) que o cliente
informa, no Claude Code a pasta da sessão; sem raízes, sobre a pasta em que foi iniciado, que
então precisa estar dentro do repositório. Se o cliente falhar ou não responder ao `roots/list`
em 5 s, a chamada usa a última raiz que ele enviou nessa conexão, ou devolve um erro dizendo
isso; nunca cai na pasta do servidor por isso (que, numa sessão em um worktree, seria outro
checkout). Uma resposta sem raiz local vale como sem raízes (a pasta do servidor). Na
configuração do seu agente (por exemplo o `.mcp.json` do Claude Code), na forma do seu canal:

=== "npx"

    ```json
    {
      "mcpServers": {
        "meada": { "command": "npx", "args": ["-y", "@meada/meada", "mcp"] }
      }
    }
    ```

=== "meada no PATH"

    ```json
    {
      "mcpServers": {
        "meada": { "command": "meada", "args": ["mcp"] }
      }
    }
    ```

=== "Docker"

    ```json
    {
      "mcpServers": {
        "meada": {
          "command": "sh",
          "args": ["-c", "exec docker run -i --rm --network none --user \"$(id -u):$(id -g)\" -v \"${PWD%/*}:${PWD%/*}\" -w \"$PWD\" ghcr.io/claudiomascaro/meada:1.0.0 mcp"]
        }
      }
    }
    ```

    O cliente inicia o servidor no repositório; o contêiner monta a pasta que o contém, e os pares
    vizinhos, no mesmo caminho da raiz que o cliente envia, roda com o seu usuário e sem rede.
    Só Linux e macOS.

São três ferramentas, sempre em inglês, e nenhuma escreve:

- `search` acha um símbolo, uma rota ou um contrato pelo nome; com o tipo `resource`, um
  recurso pelo tipo ou pelo id; com o tipo `channel`, um canal (fila ou tópico) pelo broker ou
  pelo nome; com o tipo `duplicate`, grupos de símbolos com texto idêntico em dois ou mais lugares;
  com o tipo `ambiguous` ou `unresolved`, lista os itens abertos
  (veja Decisões);
- `context` descreve um símbolo: o trecho de código lido do disco, quem o chama, quem ele
  chama e os contratos que ele toca; as anotações, atributos e decoradores escritos sobre uma
  definição (`@Override`, `[Fact]`, `@app.get("/x")`) são chamadas do escopo que a envolve, e por
  isso o `context` as lista à parte, na seção `annotations:`, cada uma com a definição que ela
  decora (o ponto de chamada do export traz isso em `decorates`); num recurso (`resource::postgres::orders`), lista quem
  escreve e quem lê, com o símbolo e a linha, ou `(repository-level)` para o repositório
  inteiro; num canal (`channel::kafka::orders.created`), lista produtores e consumidores,
  inclusive os que chegam a ele por um curinga ou por um binding, com o nível e o motivo, e um
  lado cujo nome Meada não consegue avaliar aparece como `(unresolved)` e nunca é ligado; os candidatos de uma chamada ambígua que
  são sobrecargas de um método (um arquivo, uma classe, um nome) aparecem como um conjunto,
  `(one of N overloads)`, e o `trace` os escreve como `Class.name (N overloads)`; a chamada
  continua `ambiguous` e o export lista cada candidato como antes;
- `trace` segue uma cadeia de chamadas por vários repositórios, rio abaixo (`down`) ou rio
  acima (`up`); com `changes: uncommitted`, parte do diff da árvore de trabalho e serve para
  analisar o impacto do que acabou de ser editado.

As respostas são linhas compactas dentro de um orçamento de tokens (2.000 por padrão, ajustável
a cada chamada). Cada ligação entre repositórios traz o nível e o motivo (`→[level: reason]`),
e o `trace` termina com um rodapé: o nível mais fraco da cadeia, o número de saltos, os
repositórios envolvidos e os tokens usados.

## Skill para agentes

A skill [`skills/meada/SKILL.md`](https://github.com/ClaudioMascaro/meada/blob/main/skills/meada/SKILL.md)
ensina o agente a escolher entre `search`, `context` e `trace` e a ler os níveis de uma resposta:
`inferred` e `ambiguous` são casamentos de nome, e um chamador ausente não está provado. Ela vale
para a versão em que foi publicada; baixe a da sua versão.

- **Claude Code**: copie para `~/.claude/skills/meada/` (todas as sessões) ou para
  `.claude/skills/meada/` de um projeto:

    ```sh
    mkdir -p ~/.claude/skills/meada
    curl -fsSL https://raw.githubusercontent.com/ClaudioMascaro/meada/v1.0.0/skills/meada/SKILL.md -o ~/.claude/skills/meada/SKILL.md
    ```

- **Outro agente que lê pastas de skills** (`SKILL.md` com frontmatter): copie a pasta
  `skills/meada/` para a pasta de skills dele.
- **Agente sem skills**: cole o corpo do `SKILL.md`, sem o frontmatter, no `AGENTS.md` do
  projeto.
