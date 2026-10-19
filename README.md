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
`.jsx`, `.mjs`, `.cjs`) e Python (`.py`), lê o código Rust (`.rs`) dos crates do repositório
(cada `Cargo.toml` com `[package]`, em qualquer pasta, cada crate pela árvore de módulos que os
`mod` montam, sem toolchain Rust: um arquivo que nenhum `mod` alcança fica de
fora) com as suas funções, tipos, traits, módulos e métodos de `impl` e as chamadas escritas
como caminho (`crate::a::f()`, `Tipo::new()`, `Self::f()`), também por `use` (grupos,
renomeações, globs, `pub use` e os outros crates do workspace pelo nome; as de método (`x.m()`) pelo tipo do receptor quando o código o escreve (`let`, parâmetro, campo, `self`, retorno de função; um `dyn Trait` ou genérico com trait vai ao método do trait; um método que os `impl` de um tipo do repositório não têm, de um `derive` ou de um trait de fora, não liga a nada, e um atributo de rota nunca liga a uma função), senão pelo nome; as de dentro de macros
não são indexadas, e os contratos Rust são os pacotes, o HTTP e os canais (abaixo); só a configuração padrão de cada crate entra, para um alvo fixo x86_64 Linux: item sob um `#[cfg(..)]` falso, ou sob uma feature fora do `default` do `Cargo.toml`, fica de fora, e módulos `#[cfg(test)]` entram), lê o código Go (`.go`, sem toolchain Go: um import chega à pasta do pacote pelo caminho de módulo de cada `go.mod`, também por um `replace` para uma pasta do repositório, e um módulo aninhado é dono da sua subárvore; os arquivos de uma pasta com o mesmo `package`, `_test.go` incluídos, dividem um escopo; as restrições de build valem para um alvo fixo linux/amd64, e um `_windows.go` ou um `//go:build` falso fica de fora) com as suas funções, tipos, interfaces e métodos (`Tipo.Metodo`) e as chamadas a funções do mesmo pacote e por import (`pkg.F()`, com alias ou com ponto; a biblioteca padrão e os módulos de fora ficam externos), e liga repositórios, também de uma
linguagem para a outra, por contratos HTTP (`fetch` e, em Python, `requests`, `httpx` e `aiohttp` e, em Rust, `reqwest` e `ureq` no lado que
consome; Next.js (app router) e Express e, em Python, FastAPI, Flask e Django e, em Rust, axum, actix-web e Rocket e, em Go, `net/http`, gin e echo no lado que serve,
também atrás de um gateway; em Rust, uma URL base lida do ambiente com `std::env::var`, `env!` ou `dotenvy::var` liga pelo `base_url_env`), pelos recursos
declarados no `meada.toml` e por canais: tópicos Kafka (`kafkajs` e, em Python, `confluent-kafka` e `kafka-python` e, em Rust, `rdkafka`), filas RabbitMQ
(`amqplib` e, em Python, `pika` e, em Rust, `lapin`, com o `publish` numa exchange chegando às filas que os bindings
ligam a ela), filas Amazon SQS (AWS SDK v3, `sqs-consumer` e, em Python, `boto3` e, em Rust, `aws-sdk-sqs`), subjects NATS
(`nats` e, em Python, `nats-py` e, em Rust, `async-nats`, com os curingas `*` e `>`), jobs BullMQ e streams Redis
(`ioredis`, `node-redis` e, em Python, `redis-py` e, em Rust, o crate `redis`), com o produtor ligado ao handler de cada consumidor, inclusive
no mesmo repositório. Um repositório com a tabela `[serve]` também lê seus documentos OpenAPI
(3.0 e 3.1) e AsyncAPI (2.x e 3.0), achados pelo nome ou listados em `[serve] documents`: as rotas
e os canais que eles descrevem viram contratos, com os campos de cada resposta ou payload, e o
`meada status` diz o que leu e o que ignorou. Cada `package.json` com nome vira o contrato
`package::npm::<nome>`, e cada dependência (`dependencies`, `devDependencies`,
`peerDependencies`, `optionalDependencies`) que nomeia um pacote do workspace (de
qualquer repositório, o próprio inclusive) se liga a ele, com a faixa de versão conferida contra a versão local (`range
mismatch` quando não a aceita, `range unchecked` quando não dá para conferir). Uma chamada a
uma função importada desse pacote atravessa até o código do provedor, pelos campos `exports`,
`types`, `module` e `main` e pelos re-exports, também quando o ponto de entrada aponta para a
pasta de build (o `outDir` do `tsconfig.json`, mapeado de volta para o `rootDir`). O `search`
com o tipo `package` acha o pacote pelo nome, com o repositório, a pasta, a versão e o número
de consumidores, e o `context` de uma função exportada lista quem a chama, em qualquer
repositório do workspace. Em Python, cada `pyproject.toml` com nome (`[project]` ou
`[tool.poetry]`) vira o contrato `package::pypi::<nome>`, com o nome normalizado como o PEP 503
manda, e cada dependência de `[project] dependencies`, `[tool.poetry.dependencies]` ou de um
`requirements*.txt` que nomeia um pacote do workspace se liga a ele, com a faixa como escrita e
`range unchecked` (faixas PyPI não são conferidas); um `import` de um módulo desse pacote
atravessa até o código do provedor, `exact` quando o módulo tem o nome da distribuição,
`inferred` quando é só um módulo de topo dela e `ambiguous` quando várias o expõem. Um pacote
npm e um PyPI de mesmo nome nunca se ligam. Em Rust, cada `Cargo.toml` com `[package] name` (cada
membro de um workspace Cargo é um provedor) vira o contrato `package::cargo::<nome>`, e cada
entrada de `[dependencies]`, `[dev-dependencies]` e `[build-dependencies]` se liga a ele: por
`path`, ao crate dessa pasta, no mesmo repositório ou, se o caminho sai dele (`../outro`), em outro
repositório do workspace (o caminho é resolvido só pelo texto, a partir das raízes dos
repositórios, sem ler nada fora deles nem seguir link simbólico; um caminho que não cai em nenhum
repositório do workspace não aparece); por `git`, ao repositório cuja identidade é a URL
normalizada; por versão, ao provedor de mesmo nome no workspace; `package =` dá o nome real de uma
dependência renomeada, `workspace = true` lê `[workspace.dependencies]` e `version.workspace =
true` lê `[workspace.package]`. A ligação é `exact`, com a faixa como escrita e `range unchecked`
(faixas Cargo não são conferidas), e um `use` do crate (pelo nome de `[lib] name`, se houver, ou
pela chave de uma dependência renomeada) chega aos itens públicos do provedor pelos seus `pub use`,
também `exact`, com `; import crate name` no fim da razão. Uma dependência fora do workspace não
aparece.
Não lê lockfile (`Cargo.lock` incluído) nem consulta o registro npm, o PyPI ou o crates.io. O Meada também aponta o *drift* de formato: os campos que um consumidor `fetch` lê da resposta já interpretada e que a rota que ele chama não devolve, comparados com o formato da rota (documento OpenAPI, schema zod ou objetos literais do handler). O `trace` termina a linha da ligação com `drift: missing <caminho>`, o `context` de uma rota diz de onde vem o formato e lista cada consumidor que lê campos da resposta, com o drift e o nível e as leituras não conferidas sob uma parte aberta quando há, o `search` com o tipo `drift` lista todo o drift, e o `meada status` conta as leituras ausentes dos consumidores de cada repositório (quem conta é o repositório do consumidor; o do provedor não mostra nada). O `meada export` traz as comunidades de cada repositório: grupos de símbolos que se chamam mais entre si do que com o resto, achados pelo algoritmo de Leiden sem nada aleatório e só com aritmética inteira, e nomeados pela pasta onde vive a maioria dos seus membros (`src`, `src#2`), iguais em toda máquina. Cada ligação tem o nível `exact`,
`declared`, `inferred`, `decided` ou `ambiguous`; uma chamada cuja URL não dá para resolver
aparece como não resolvida e só vira ligação por uma decisão, no nível `decided`, e uma rota que o Meada não sabe onde é servida fica fora do mapa em vez de
ser adivinhada. Clusters de
broker não são modelados: dois clusters com um tópico de mesmo nome se ligam como se fossem um
só. Cada rota que um handler serve, cada canal que um handler consumidor consome e cada função ou método exportado que nada chama no próprio repositório é um ponto de entrada, com o seu processo: o `trace` com `process::<identidade do ponto de entrada>` lê a árvore de chamadas que ele dispara, atravessando os repositórios, e o `meada export` traz o processo de cada ponto de entrada. Em Python, o bloco `if __name__ == "__main__":` de um arquivo (o símbolo `__main__`) e cada função que um `[project.scripts]` do `pyproject.toml` nomeia (`módulo:função`) também são pontos de entrada, e só os nomes públicos (sem `_` na frente, ou os do `__all__` literal) contam como exportados. Em Rust, o `fn main` de cada binário (`src/main.rs`, `src/bin/`, `[[bin]] path`) também é. O `search` com o tipo `community` acha a comunidade pelo nome e com o tipo `process` acha o processo pelo rótulo do ponto de entrada, e o `context` de um símbolo diz a sua comunidade (ou `community: none`) e os processos a que ele pertence, achados subindo pelas mesmas chamadas e ligações que o processo percorre. Ainda não há: outras linguagens, outros clientes HTTP além desses, outros clientes de
mensageria além desses, nem o pacote npm do Meada. O escopo e a ordem do trabalho estão no
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
