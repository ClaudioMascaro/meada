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
```

Meada acha os pares pelo remoto git, mesmo que o clone fique em outro caminho em cada máquina.
Uma chave desconhecida ou mal escrita dá um erro com o arquivo, a linha e a chave. Sem
`meada.toml`, o repositório não declara nenhum par.

## Uso

Na raiz do repositório:

- `meada index` monta o índice, em `.meada/`, do repositório e dos pares;
- `meada status` mostra o conjunto de repositórios, o estado de cada índice e os pares que não
  foram encontrados;
- `meada export` imprime o índice inteiro em JSON ordenado, bom para `diff` entre duas execuções;
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

- `search` acha um símbolo, uma rota ou um contrato pelo nome;
- `context` descreve um símbolo: o trecho de código lido do disco, quem o chama, quem ele
  chama e os contratos que ele toca;
- `trace` segue uma cadeia de chamadas por vários repositórios, rio abaixo (`down`) ou rio
  acima (`up`); com `changes: uncommitted`, parte do diff da árvore de trabalho e serve para
  analisar o impacto do que acabou de ser editado.

As respostas são linhas compactas dentro de um orçamento de tokens (2.000 por padrão, ajustável
a cada chamada). Cada ligação entre repositórios traz o nível e o motivo (`→[level: reason]`),
e o `trace` termina com um rodapé: o nível mais fraco da cadeia, o número de saltos, os
repositórios envolvidos e os tokens usados.

## Estado

Versão 0.1, a primeira. Indexa repositórios TypeScript e JavaScript (`.ts`, `.tsx`, `.js`,
`.jsx`, `.mjs`, `.cjs`) e liga repositórios por contratos HTTP: `fetch` no lado que consome,
Next.js (app router) e Express no lado que serve. Cada ligação tem o nível `exact`, `inferred`
ou `ambiguous`; uma chamada cuja URL não dá para resolver aparece como não resolvida, nunca
como ligação. Ainda não há: outras linguagens, outros clientes HTTP além do `fetch`, filas,
pacotes, o comando `meada decide` nem o pacote npm. O escopo e a ordem do trabalho estão no
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
