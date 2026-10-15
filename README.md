# Meada

**Português (Brasil)** · [English](README.en.md) · [Español](README.es.md)

Meada lê os seus repositórios, monta o mapa de quem chama quem e costura um repositório no
outro pelos contratos que eles compartilham: rotas HTTP, filas, pacotes, recursos compartilhados.
Agentes de código, e gente também, consultam esse mapa por MCP. Nenhuma etapa da indexação usa
LLM.

## Instalação

Escolha uma das três formas.

- **Cargo**: `cargo install meada`.
- **Binário pronto**: baixe o arquivo do seu sistema nas
  [releases do GitHub](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS e
  Windows, em x86_64 e arm64) e ponha `meada` no `PATH`.
- **Instalador** (Linux e macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.

## Configuração

Cada repositório pode ter um `meada.toml` na raiz, versionado no git, dizendo com quem ele
conversa. Quem consome uma rota nomeia o par:

```toml
[[peer]]
repo = "example.com/acme/api"            # remoto git de origin, sem esquema nem .git
base_url_env = ["API_URL"]               # variável de onde o cliente lê a URL base
# path = "../api"                        # opcional: dica de onde o clone está
```

Quem serve a rota declara os hosts em que responde:

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"                         # opcional: o gateway põe isto na frente dos caminhos
strip = "/v1"                            # opcional: e tira isto do começo deles
```

Com `prefix` e `strip`, uma chamada a `/shop/orders` chega à rota `/v1/orders` do código, no
nível `inferred` e com o motivo `gateway prefix /shop, gateway strip /v1`. Um segmento de versão
só sai quando você o escreve em `strip`, e quem chama o caminho do código continua ligando como
antes.

Um recurso compartilhado que nenhum código nomeia, como uma tabela ou um bucket, entra como
`[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # identificador em minúsculas
id = "orders"
role = "write"                           # read, write ou read-write
via = ["src/orders.ts::cancelOrder"]     # opcional: os símbolos que fazem o acesso
```

Cada escritor liga a cada leitor do mesmo recurso em outro repositório, no nível `declared`.
Sem `via`, ou se nenhum `via` corresponde a um símbolo, o lado é o repositório inteiro.

Meada acha os pares pelo remoto git, mesmo que o clone fique em outro caminho em cada máquina.
Uma chave desconhecida ou mal escrita dá um erro com o arquivo, a linha e a chave. Sem
`meada.toml`, o repositório não declara nenhum par.

## Uso

Na raiz do repositório:

- `meada index` monta o índice, em `.meada/`, do repositório e dos pares;
- `meada status` mostra o conjunto de repositórios, o estado de cada índice e os pares que não
  foram encontrados; sob cada repositório, os recursos sem ligação e os `via` que não
  correspondem a nenhum símbolo;
- `meada export` imprime o índice inteiro em JSON ordenado, bom para `diff` entre duas execuções;
- `meada decide` resolve ligações ambíguas e URLs dinâmicas, veja Decisões;
- `meada mcp` serve as ferramentas abaixo por MCP.

Meada só escreve dentro de `.meada/`, que se ignora no git. Toda consulta primeiro atualiza o
índice com o que mudou nos arquivos, inclusive o que ainda não foi commitado. As mensagens da
linha de comando seguem o `LANG` (português ou inglês); as do MCP são sempre em inglês.

## Agentes (MCP)

`meada mcp` fala MCP por stdio e precisa ser iniciado dentro do repositório. Na configuração do
seu agente (por exemplo o `.mcp.json` do Claude Code):

```json
{
  "mcpServers": {
    "meada": { "command": "meada", "args": ["mcp"] }
  }
}
```

São três ferramentas, sempre em inglês, e nenhuma escreve:

- `search` acha um símbolo, uma rota ou um contrato pelo nome; com o tipo `resource`, um
  recurso pelo tipo ou pelo id; com o tipo `ambiguous` ou `unresolved`, lista os itens abertos
  (veja Decisões);
- `context` descreve um símbolo: o trecho de código lido do disco, quem o chama, quem ele
  chama e os contratos que ele toca; num recurso (`resource::postgres::orders`), lista quem
  escreve e quem lê, com o símbolo e a linha, ou `(repository-level)` para o repositório
  inteiro;
- `trace` segue uma cadeia de chamadas por vários repositórios, rio abaixo (`down`) ou rio
  acima (`up`); com `changes: uncommitted`, parte do diff da árvore de trabalho e serve para
  analisar o impacto do que acabou de ser editado.

As respostas são linhas compactas dentro de um orçamento de tokens (2.000 por padrão, ajustável
a cada chamada). Cada ligação entre repositórios traz o nível e o motivo (`→[level: reason]`),
e o `trace` termina com um rodapé: o nível mais fraco da cadeia, o número de saltos, os
repositórios envolvidos e os tokens usados.

## Decisões

Uma ligação ambígua (vários provedores servem a rota) ou uma URL dinâmica (a chamada monta a
URL em tempo de execução) fica como item aberto. A ferramenta `search` do MCP, com o tipo
`ambiguous` ou `unresolved`, lista esses itens com o número de candidatos de cada um, não os
candidatos; `meada decide --list` lista os itens com os candidatos. Quem resolve é você, no
modo humano, ou um modelo, se você pedir.

- `meada decide` mostra cada item aberto do repositório, com os candidatos numerados; você
  escolhe um número, ou `s` para pular. Ao fim grava as escolhas em `meada.decisions.toml`, na
  raiz. Não escolheu nada, não grava.
- `--item <id>` trata só aquele item (o id que `meada decide --list` ou a ferramenta `search`
  imprime), mesmo que já decidido, para refazer a decisão; `--dry-run` imprime o arquivo que
  seria gravado e não escreve; `--list` imprime os itens abertos, mais as
  decisões obsoletas, e não pergunta nada; `--prune` remove as decisões obsoletas (não combina
  com as três anteriores).
- `--decider` troca a pessoa por um modelo (veja abaixo); `--show-request` (só com `--decider`)
  imprime cada requisição que seria enviada e não envia nenhuma; `--min-p <p>` (só com
  `--decider`, de 0 a 1) é a probabilidade mínima para o modelo gravar.

`meada.decisions.toml` é um arquivo versionado no git, como um lockfile: o diff dele é o que se
revisa. Cada `[[decision]]` traz a chamada (`call`), o provedor escolhido (`target`), quem
decidiu (`by`: `human` ou o id do modelo) e, para um modelo, a probabilidade (`p`). A
indexação só relê o arquivo: a ligação decidida ganha o nível `decided` e o motivo mostra quem
decidiu. Quando o código muda e uma decisão deixa de caber (a chamada sumiu, o item não está
mais aberto, o provedor sumiu ou não é mais candidato), `meada status` a mostra como obsoleta,
sob o repositório, e `meada decide --prune` a remove. Uma decisão que espera um par ainda não
encontrado aparece à parte e é mantida.

### Decisor por modelo

`meada decide --decider` é o único caminho de Meada até a rede, e só roda quando você o pede.
Quatro variáveis de ambiente o configuram (nada vem de arquivo):

| Variável | Papel | Padrão |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` ou `chat` | `decisions` |
| `MEADA_DECIDER_URL` | URL base | `https://openrouter.ai` (`decisions`); obrigatória com `chat` |
| `MEADA_DECIDER_KEY` | chave da API, enviada só no cabeçalho `Authorization` | obrigatória com `decisions`; opcional com `chat` |
| `MEADA_DECIDER_MODEL` | modelo | `typesafe/jev-1.13` (`decisions`); obrigatória com `chat` |

- **`decisions`** (o padrão) usa a API de decisões do OpenRouter, com o Jev: uma única
  requisição leva todos os itens, e a resposta traz a probabilidade de cada candidato. A
  requisição pede retenção zero de dados (`zdr: true`, `data_collection: deny`); se o
  OpenRouter recusar essa preferência, a requisição falha e nada é gravado.
- **`chat`** fala com qualquer endpoint compatível com a API de chat da OpenAI
  (`<URL>/chat/completions`), inclusive um modelo local, por exemplo pelo Ollama:
  `MEADA_DECIDER_PROTOCOL=chat MEADA_DECIDER_URL=http://localhost:11434/v1
  MEADA_DECIDER_MODEL=<modelo> meada decide --decider`. Uma requisição por item. A
  probabilidade é a que o próprio modelo diz de si, não uma medida calibrada.

Qualquer gateway que fale um dos dois protocolos serve: basta trocar a URL. HTTPS em qualquer
lugar; HTTP só para `localhost` ou um endereço de loopback.

O que sai da máquina, por item: a chamada, a função que a contém (lida do disco) e a lista de
candidatos (repositório, método e caminho), mais o id do repositório que a chama. Nenhum
código do provedor, nenhum outro arquivo. `--show-request` mostra tudo isso antes, com a chave
trocada por `<hidden>`. Cada resposta é validada por item; uma inválida ou uma requisição
que falhou é relatada sem gravar aquele item, e o comando sai com erro.

Constantes fixas:

- `--min-p` padrão de 0,5: abaixo disso a proposta é impressa como sugestão e não é gravada.
  Muda só por um PR com dados.
- No máximo 50 candidatos por item: com mais, o item é pulado no modo decisor (o modo humano
  ainda o oferece).
- 120 segundos por requisição, da conexão ao último byte, sem tentar de novo.
- No protocolo `decisions`, as probabilidades de uma resposta devem somar 1 com tolerância de
  0,01.

## Estado

Versão 0.1, a primeira. Indexa repositórios TypeScript e JavaScript (`.ts`, `.tsx`, `.js`,
`.jsx`, `.mjs`, `.cjs`) e liga repositórios por contratos HTTP (`fetch` no lado que consome,
Next.js (app router) e Express no lado que serve, também atrás de um gateway) e pelos recursos
declarados no `meada.toml`. Cada ligação tem o nível `exact`, `declared`, `inferred`, `decided`
ou `ambiguous`; uma chamada cuja URL não dá para resolver aparece como não resolvida e só vira
ligação por uma decisão, no nível `decided`. Ainda não há: outras linguagens, outros clientes
HTTP além do `fetch`, filas, pacotes nem o pacote npm. O escopo e a ordem do trabalho estão no
[ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md); as decisões, em [`docs/adr`](docs/adr).
Veja o [CHANGELOG](CHANGELOG.md).

## Por que mais uma ferramenta

Quem trabalha com vários serviços sabe que o bug quase nunca mora num repositório só. O front
chama uma rota que mudou no back; o worker consome uma fila que outro time renomeou. Das
ferramentas de grafo de código que existem hoje, algumas olham um repositório por vez, outras
só ligam repositórios quando o texto da rota bate exatamente, e outras proíbem uso comercial.

Meada quer seguir o fio de ponta a ponta, atravessando quantos repositórios for preciso, e dizer
com que confiança cada ligação foi feita.

## Princípios

- **O seu código fica com você.** Roda na sua máquina, sem conta, sem chave de API e sem
  telemetria.
- **Sem LLM para indexar.** O mapa sai da árvore sintática do código. Não tem conta em dólar
  por token, funciona offline e cabe num notebook comum.
- **Determinístico.** O mesmo código com a mesma configuração sempre dá o mesmo mapa. Quando
  uma ligação é ambígua, a decisão fica num arquivo versionado no git, tomada por uma pessoa
  ou, se você quiser, por um modelo de linguagem em um comando separado. A indexação só relê
  essa decisão.
- **Português primeiro.** A documentação nasce em português do Brasil; inglês e espanhol são
  traduções. Issues e PRs podem ser abertos em qualquer uma das três línguas. O código é
  escrito em inglês para que qualquer pessoa consiga ler.
- **Juntar sem misturar.** "Nem tudo que se ajunta se mistura", ensinou Antônio Bispo dos
  Santos, o Nêgo Bispo. Cada repositório guarda o próprio índice; Meada só costura as pontas.
- **Pouco ruído para o agente.** Meada não escreve no seu `CLAUDE.md`, não instala hooks e não
  mexe na configuração do editor sem um comando seu. As respostas são curtas.
- **Crédito a quem veio antes.** Meada aprende com GitNexus, CodeGraph, codebase-memory-mcp e
  kivgraph.

## Como participar

O projeto está começando. O escopo da v1 e a ordem do trabalho estão no
[ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md). Já dá para ajudar:

- abrindo uma issue com o seu caso de uso entre repositórios, em português, español ou English;
- revisando as traduções: se o inglês ou o espanhol soarem estranhos, a correção é bem-vinda;
- lendo e questionando os ADRs.

## O nome

Meada é uma porção de fios enrolados. "Puxar o fio da meada" é achar a ponta que desfaz o nó,
e é isso que a ferramenta tenta fazer quando segue uma chamada de um repositório a outro.

## Licença

[Apache-2.0](LICENSE).
