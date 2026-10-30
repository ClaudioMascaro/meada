# Guia do usuário

Este guia leva uma pessoa que fala português do zero ao Meada funcionando: instalar, conectar um
agente, configurar um workspace, decidir o que o código não deixa claro, usar a linha de comando
e ler o export. Os exemplos usam os repositórios `web`, `api` e `worker` e o host
`api.example.com`.

Esta é a primeira metade do guia. A segunda explica o que o Meada liga e onde ele para: cada
tipo de contrato, cada linguagem, o drift de forma, as comunidades e os processos.

Sumário:

1. [Instalação](#1-instalação)
2. [Conectar um agente (MCP)](#2-conectar-um-agente-mcp)
3. [O workspace e o primeiro uso](#3-o-workspace-e-o-primeiro-uso)
4. [`meada.toml`: referência](#4-meadatoml-referência)
5. [Decisões e decisores](#5-decisões-e-decisores)
6. [Comandos da CLI](#6-comandos-da-cli)
7. [O export](#7-o-export)

## 1. Instalação

Há cinco canais, e todos entregam o mesmo binário `meada`. Escolha o que combina com o seu
ambiente.

### Cargo

```sh
cargo install meada
```

Compila a partir do crates.io e exige o toolchain Rust.

### Binário pronto

Baixe o arquivo do seu sistema nas
[releases do GitHub](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS e Windows,
em x86_64 e arm64), extraia e ponha `meada` no `PATH`.

### Instalador de shell

Para Linux e macOS:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh
```

### npm (`npx meada`)

```sh
npx meada --version
npx meada index
```

O pacote `meada` do npm contém só o instalador: ele baixa da release do GitHub o binário da sua
plataforma e o executa com os argumentos que você passou. A versão do pacote é sempre a da
release. A instalação baixa o binário na hora, então ela precisa de rede (não há instalação
offline pelo npm). É o canal que mais interessa a quem configura um agente, porque
`npx meada mcp` é a linha de lançamento do servidor MCP (seção 2).

### Contêiner

```sh
docker run --rm ghcr.io/claudiomascaro/meada:latest --version
```

`ghcr.io/claudiomascaro/meada:<versão>` é o build padrão e `<versão>-cloud` traz todos os módulos
cloud, ambos para Linux amd64 e arm64. A imagem roda sem root e não define comando: passe os
argumentos explicitamente. Para servir uma organização, veja
[Modo servidor](https://claudiomascaro.github.io/meada/server/).

### O que o Meada precisa

Só do binário e do `git`. A indexação lê os arquivos do repositório sem executar o código nem o
toolchain da linguagem, não usa LLM e não abre rede. Confira a instalação com
`meada --version`.

A console de gestão (módulo opcional, `--features ui`) e os demais módulos de nuvem têm
instalação própria, descrita em
[Instalação](https://claudiomascaro.github.io/meada/installation/).

## 2. Conectar um agente (MCP)

`meada mcp` fala MCP por stdio. O cliente (o agente) executa o comando e conversa com ele. A
linha de lançamento documentada é `npx meada mcp`, que dispensa uma instalação separada.

### Configuração do cliente

No `.mcp.json` do Claude Code, ou no arquivo equivalente do seu cliente:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["meada", "mcp"] }
  }
}
```

No Claude Code, o mesmo em um comando: `claude mcp add meada -- npx meada mcp`.

Com `meada` já no `PATH` (por Cargo, binário ou instalador), troque o comando por
`"command": "meada", "args": ["mcp"]`, que evita a consulta ao npm a cada início.

### Sobre qual repositório o servidor responde

O servidor responde sobre a primeira raiz (`roots`) que o cliente informa. O Claude Code envia a
pasta da sessão, então abrir uma sessão em `web` faz o Meada responder sobre `web`, mesmo que o
servidor tenha sido iniciado em outro lugar. Sem raízes, ele responde sobre a pasta em que foi
iniciado, que precisa estar dentro do repositório. Se o cliente não responder ao `roots/list`
em 5 segundos, a chamada usa a última raiz enviada naquela conexão ou devolve um erro que diz
isso; ela nunca cai na pasta do servidor por esse motivo.

Toda consulta atualiza o índice antes com o que mudou nos arquivos, inclusive o que ainda não foi
commitado. Não existe passo de reindexação para o agente.

### As três ferramentas

São três, com nomes e descrições em inglês, e nenhuma escreve. As respostas são linhas compactas
dentro de um orçamento de tokens (`budget`, 2.000 por padrão, ajustável a cada chamada; um token
vale quatro bytes de saída). Quando a resposta passa do orçamento, ela termina com
`… cut after N of M lines (budget B tokens); raise budget for more`.

#### `search`: achar o ponto de partida

| Parâmetro | Tipo | Padrão | Papel |
| --- | --- | --- | --- |
| `query` | texto, obrigatório | | O nome a procurar, sem distinguir maiúsculas |
| `kind` | texto | todos os tipos | Restringe o tipo de resultado |
| `budget` | inteiro | 2000 | Orçamento de tokens da resposta |

Os valores de `kind`:

| `kind` | Procura |
| --- | --- |
| `symbol` | um símbolo pelo nome |
| `contract` | um contrato pelo identificador (`http::GET::/orders`) |
| `route` | um contrato HTTP |
| `resource` | um recurso declarado, pelo tipo e pelo id |
| `channel` | uma fila ou um tópico, pelo broker e pelo nome |
| `document` | um arquivo OpenAPI ou AsyncAPI, pelo caminho ou pelo título |
| `package` | um pacote pelo nome, com os seus consumidores |
| `drift` | campos que consumidores leem e uma rota não devolve, pela rota ou pelo caminho do campo |
| `community` | uma comunidade pelo nome |
| `process` | um processo pelo rótulo do ponto de entrada, com a identidade `process::` de onde o `trace` parte |
| `duplicate` | símbolos com texto idêntico em dois ou mais lugares, pelo nome ou pelo caminho |
| `ambiguous`, `unresolved` | os itens abertos, com o comando `meada decide` de cada um |

Cada resultado é uma linha: repositório, `caminho:linha` e nome.

#### `context`: entender um símbolo

| Parâmetro | Tipo | Padrão | Papel |
| --- | --- | --- | --- |
| `symbol` | texto, obrigatório | | Um nome, `<caminho>::<nome>` ou a identidade completa; um recurso (`resource::postgres::orders`) ou um canal (`channel::kafka::orders.created`) |
| `budget` | inteiro | 2000 | Orçamento de tokens da resposta |

Devolve a comunidade e os processos do símbolo, o trecho de código lido do disco, quem o chama
(`callers:`), quem ele chama (`callees:`) e os contratos que ele toca (`contracts:`). Num recurso,
lista quem escreve e quem lê; num canal, os produtores e os consumidores.

```text
example.com/acme/web src/status.ts:2-4 health
community: src#3
processes: 1 (1 export; 1 repository): dashboard (example.com/acme/web)
function health() {
  return fetch("/api/health");
}
callers:
example.com/acme/web src/dashboard.ts:5 dashboard
callees:
example.com/acme/web src/status.ts:3 (unresolved) fetch
contracts:
example.com/acme/web src/status.ts:3 consumes http::GET::/api/health →[exact: host api.example.com is served by example.com/acme/api] example.com/acme/api::src/server.ts::GET /api/health
```

#### `trace`: seguir uma cadeia

| Parâmetro | Tipo | Padrão | Papel |
| --- | --- | --- | --- |
| `from` | texto | | O início: um símbolo, uma rota (`GET /users/{}` ou o contrato), um ponto de chamada ou um processo (`process::<identidade>`, só rio abaixo) |
| `changes` | texto | | `uncommitted`: parte dos símbolos que o diff da árvore de trabalho toca |
| `direction` | `down` ou `up` | `down` | `down` é o que o início alcança, `up` é o que o alcança |
| `budget` | inteiro | 2000 | Orçamento de tokens da resposta |

Passe `from` ou `changes: uncommitted`, nunca os dois. O segundo serve para medir o impacto do
que acabou de ser editado, e só funciona no Meada local (o modo servidor não tem árvore de
trabalho).

```text
example.com/acme/web src/orders.ts:5 listOrders
  example.com/acme/web src/api/client.ts:4 request
  →[inferred: base URL from API_URL, a base_url_env of peer example.com/acme/api] example.com/acme/api src/orders.ts:5 listOrders
chain: inferred · 1 hops · 2 repos · 93 tokens
```

(Exemplo ilustrativo: o rodapé escreve `hops` mesmo com um salto, e a contagem de tokens varia
com a resposta.)

#### Como ler os níveis

Cada ligação entre repositórios traz `→[nível: motivo]`. O nível diz o quanto a ligação é firme:

| Nível | Significado |
| --- | --- |
| `exact` | certo: o código ou a configuração prova a ligação |
| `declared` | vem de uma declaração sua no `meada.toml`, como um recurso compartilhado |
| `inferred` | uma correspondência por nome ou por convenção |
| `decided` | vem do `meada.decisions.toml`; o motivo diz quem decidiu |
| `ambiguous` | vários candidatos servem, e o Meada lista todos sem escolher |

O `trace` termina com um rodapé: o nível mais fraco da cadeia, o número de saltos, os
repositórios envolvidos e os tokens usados. Uma cadeia vale tanto quanto o seu elo mais fraco.
Um caller que falta é "não provado", não "inexistente": chamadas dentro de macros (como `assert!`
e `format!` em Rust) não são indexadas.

## 3. O workspace e o primeiro uso

O **workspace** é o conjunto de repositórios que uma consulta enxerga. Ele reúne o repositório
onde o Meada roda, os pares que o `meada.toml` dele nomeia (e os pares destes, transitivamente) e
os clones git vizinhos (na mesma pasta que um repositório do conjunto) cujo próprio `meada.toml`
nomeia um repositório que já está nele. A descoberta lê só arquivos locais. Um repositório é
identificado pelo remoto git de `origin`, sem esquema nem `.git` (`example.com/acme/api`), então
o clone pode ficar em outro caminho em cada máquina.

Na raiz de `web`:

```sh
meada index
meada status
```

- `meada index` monta o índice de cada repositório do workspace, em `.meada/` dentro dele. É
  opcional antes de consultar, pois toda consulta atualiza o índice, mas deixa a primeira
  resposta rápida.
- `meada status` mostra cada repositório, o estado do índice e os pares que não foram
  encontrados (`missing peer: example.com/acme/api, named by example.com/acme/web`).

Para `web` enxergar `api`, o `meada.toml` de `web` declara o par (seção 4). A indexação só
escreve dentro de `.meada/`, que o Meada mantém fora do git; o único outro arquivo que ele grava
é o `meada.decisions.toml`, e só quando você roda `meada decide`.

### O que é indexado

A varredura cobre todos os arquivos do repositório, versionados ou não, exceto:

- **Ocultos**: arquivos e diretórios cujo nome começa com `.` não são lidos.
- **Sempre ignorados**: diretórios `node_modules` e `Pods`, um `Checkouts` dentro de `Carthage`,
  arquivos `*.min.js` e `*.min.css` e a saída de build do .NET (`obj` ou `bin` ao lado de um
  `.csproj`).
- **`.gitignore` e `.ignore`**, respeitados com ou sem git, na raiz e em subdiretórios.
- **`.meadaignore`**, com a sintaxe do `.gitignore`. Ele não traz de volta o que a lista de
  sempre ignorados acima exclui; já um padrão `!` dele reinclui o que o `.gitignore` ignora.

`vendor`, `dist`, `build` e código gerado não são ignorados por padrão, porque qualquer um pode
ser código próprio. Liste no `.meadaignore` o que você não quer indexar:

```gitignore
vendor/
dist/
Migrations/
*.Designer.cs
```

Um arquivo que uma nova regra exclui sai do índice no próximo refresh, como um arquivo apagado.

## 4. `meada.toml`: referência

Cada repositório pode ter um `meada.toml` na raiz, versionado no git, que diz com quem ele
conversa. O arquivo é opcional: sem ele, o repositório não declara nenhum par. Há três tipos de
tabela, `[[peer]]`, `[serve]` e `[[resource]]`, e nenhuma outra. Uma chave desconhecida ou mal
escrita é um erro que nomeia o arquivo, a linha e a chave.

```toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]

[serve]
hosts = ["web.example.com"]

[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

### `[[peer]]`: de quem este repositório consome

Repetível, uma tabela por par. Quem consome uma rota nomeia o par.

| Chave | Tipo | Obrigatória | Papel |
| --- | --- | --- | --- |
| `repo` | texto | sim | A identidade do par: o remoto git de `origin`, sem esquema nem `.git` |
| `path` | texto | não | Dica de onde o clone costuma ficar, relativa à raiz deste repositório |
| `base_url_env` | lista de textos | não (vazia) | Variáveis de onde este repositório lê a URL base do par |

```toml
[[peer]]
repo = "example.com/acme/api"
path = "../api"
base_url_env = ["API_URL"]
```

O Meada nunca lê o valor das variáveis, só os nomes. Quando o código monta uma URL a partir de
`API_URL`, o par que declara `API_URL` em `base_url_env` é o provedor, com o nível `inferred` e o
motivo `base URL from API_URL, a base_url_env of peer example.com/acme/api`.

### `[serve]`: o que este repositório serve

Uma tabela só. Quem serve rotas declara os hosts em que responde e como um gateway reescreve os
caminhos.

| Chave | Tipo | Obrigatória | Papel |
| --- | --- | --- | --- |
| `hosts` | lista de textos | não (vazia) | Os hosts em que este repositório responde |
| `prefix` | texto | não | O que um gateway põe na frente de todo caminho servido |
| `strip` | texto | não | O que um gateway tira do começo de todo caminho servido |
| `documents` | lista de textos | não (vazia) | Documentos OpenAPI e AsyncAPI além dos achados pelo nome do arquivo |

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"
strip = "/v1"
documents = ["docs/openapi.yaml"]
```

- **`hosts`**: uma chamada para `https://api.example.com/...` liga ao repositório que lista esse
  host, no nível `exact`.
- **`prefix` e `strip`**: devem começar com `/`. O Meada normaliza o valor e tira a barra final;
  `/` sozinho não reescreve nada. Com os dois acima, uma chamada a `/shop/orders` chega à rota
  `/v1/orders` do código, no nível `inferred`, com o motivo
  `gateway prefix /shop, gateway strip /v1`. `strip` casa segmentos inteiros (`/v1` não tira
  `/v10`), e um segmento de versão só sai quando você o escreve. Quem chama o caminho do próprio
  código continua ligando como antes.
- **`documents`**: caminhos relativos à raiz do repositório, sem `..` e sem caminho absoluto. O
  Meada já acha documentos OpenAPI e AsyncAPI pelo nome do arquivo; use a chave para os que têm
  outro nome. Um documento listado é lido mesmo que o `.gitignore` o ignore.
- A própria presença da tabela `[serve]`, mesmo vazia, faz os documentos do repositório contarem
  como fonte de provedores.

### `[[resource]]`: recursos compartilhados

Repetível. Um recurso que nenhum código nomeia, como uma tabela ou um bucket, entra aqui.

| Chave | Tipo | Obrigatória | Papel |
| --- | --- | --- | --- |
| `kind` | texto | sim | O tipo, um identificador em minúsculas (letras, dígitos e hífens, começando por letra) sem significado para o Meada: `postgres`, `s3-bucket` |
| `id` | texto | sim | O id do recurso, não vazio |
| `role` | `read`, `write` ou `read-write` | sim | O que este repositório faz com ele |
| `via` | lista de textos | não (vazia) | Os símbolos que fazem o acesso, como `<caminho>::<nome qualificado>` |

```toml
[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]
```

Cada escritor liga a cada leitor do mesmo recurso em outro repositório, no nível `declared`. Sem
`via`, ou se nenhum `via` corresponde a um símbolo, o lado é o repositório inteiro. O `meada
status` mostra, sob cada repositório, os recursos sem ligação e os `via` que não correspondem a
nenhum símbolo. A identidade do recurso no `search` e no `context` é `resource::postgres::orders`.

### Um exemplo completo: `web`, `api` e `worker`

`web` consome a API:

```toml
# web/meada.toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]
```

`api` serve rotas atrás de um gateway e escreve numa tabela:

```toml
# api/meada.toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"
strip = "/v1"

[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]
```

`worker` consome `api` e lê a mesma tabela:

```toml
# worker/meada.toml
[[peer]]
repo = "example.com/acme/api"
base_url_env = ["API_URL"]

[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

Rodando `meada status` em `web`, o Meada acha `api` pelo remoto git (ou pelo `path`, se houver),
e `worker` entra como clone vizinho de `web` ou de `api`, porque o seu `meada.toml` nomeia `api`,
que já está no workspace. O `worker` lê a tabela `orders` que `api` escreve, e o Meada liga os
dois pelo recurso, no nível `declared`.

### No modo servidor

Em modo servidor, um espelho sem `meada.toml` recebe os mesmos blocos `serve`, `resource` e
`peer` (este sem `path`) pelo manifesto `meada.workspace.toml`. O `meada.toml` do próprio
repositório, quando existe, vale inteiro e os blocos do manifesto são ignorados. Veja
[Modo servidor](https://claudiomascaro.github.io/meada/server/).

## 5. Decisões e decisores

Uma **ligação ambígua** (vários provedores servem a rota) ou uma **URL dinâmica** (a chamada monta
a URL em tempo de execução) fica como item aberto. O Meada não chuta: ele lista os candidatos e
espera uma decisão. Quem decide é você, ou um modelo, se você pedir.

Ver os itens abertos:

- no MCP, `search` com `kind: ambiguous` ou `kind: unresolved` lista cada item com o número de
  candidatos;
- na CLI, `meada decide --list` lista os itens com os candidatos numerados.

### Decidir como pessoa

`meada decide`, na raiz do repositório, mostra cada item aberto com os candidatos numerados.
Escolha um número, ou `s` para pular. Ao fim, o comando grava as escolhas em
`meada.decisions.toml`; se você não escolheu nada, não grava.

```text
example.com/acme/web::src/status.ts::health::fetch#0
src/status.ts:3  fetch("/api/health")
  1) example.com/acme/api GET /api/health
  2) example.com/acme/worker GET /api/health
```

### O arquivo `meada.decisions.toml`

Fica na raiz, é versionado no git como um lockfile, e o diff dele é o que se revisa. A
indexação só o relê: nenhuma decisão passa por modelo ou rede nessa hora.

```toml
[[decision]]
kind = "ambiguous-link"
call = "example.com/acme/web::src/status.ts::health::fetch#0"
target = "example.com/acme/api::http::GET::/api/health"
by = "human"

[[decision]]
kind = "dynamic-url"
call = "example.com/acme/web::src/status.ts::track::fetch#0"
target = "example.com/acme/worker::http::POST::/api/events"
by = "example/model"
p = 0.62
```

| Chave | Tipo | Obrigatória | Papel |
| --- | --- | --- | --- |
| `kind` | `ambiguous-link` ou `dynamic-url` | sim | O tipo do item: vários provedores, ou URL que o Meada não avalia |
| `call` | texto | sim | O ponto de chamada, como o `meada export` e o `meada decide --list` o nomeiam |
| `target` | texto | sim | `<identidade do repositório provedor>::<identificador do contrato>` |
| `by` | texto | sim | `human`, ou o id do modelo |
| `p` | número de 0 a 1 | só para um modelo | A probabilidade que o modelo declarou; proibida com `by = "human"` |

O arquivo tem uma entrada por `call`: repetir uma chamada é um erro que nomeia a linha. A ligação
decidida ganha o nível `decided`, e o motivo mostra quem decidiu.

Quando o código muda e uma decisão deixa de caber (a chamada sumiu, o item não está mais aberto,
o provedor sumiu ou não é mais candidato), o `meada status` a mostra como **obsoleta** sob o
repositório, e `meada decide --prune` a remove. Uma decisão que espera um par ainda não
encontrado aparece à parte (`decision waiting on missing peer ...`) e é mantida.

### Decisor por modelo

`meada decide --decider` troca a pessoa por um modelo. É o único caminho do Meada até um modelo
de linguagem, e só roda quando você o pede com essa flag. Nenhuma outra parte do Meada, indexação
e consulta inclusive, usa LLM ou rede.

#### Variáveis de ambiente

O decisor se configura só por variáveis de ambiente (nada vem de arquivo):

| Variável | Papel | Padrão |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` ou `chat` | `decisions` |
| `MEADA_DECIDER_URL` | URL base | `https://openrouter.ai` (`decisions`); obrigatória com `chat` |
| `MEADA_DECIDER_KEY` | chave da API, enviada só no cabeçalho `Authorization` | obrigatória com `decisions`; opcional com `chat` |
| `MEADA_DECIDER_MODEL` | modelo | `typesafe/jev-1.14` (`decisions`); obrigatória com `chat` |

#### Protocolos

- **`decisions`** (o padrão) usa a API de decisões do OpenRouter com o modelo Jev. Uma única
  requisição leva todos os itens, e a resposta traz a probabilidade de cada candidato. A
  requisição pede retenção zero de dados (`zdr: true`, `data_collection: deny`); se o OpenRouter
  recusar essa preferência, a requisição falha e nada é gravado.

  ```sh
  export MEADA_DECIDER_KEY=...
  meada decide --decider
  ```

- **`chat`** fala com qualquer endpoint compatível com a API de chat da OpenAI
  (`<URL>/chat/completions`), inclusive um modelo local, por exemplo pelo Ollama. Uma
  requisição por item. A probabilidade é a que o próprio modelo diz de si, não uma medida
  calibrada.

  ```sh
  MEADA_DECIDER_PROTOCOL=chat \
  MEADA_DECIDER_URL=http://localhost:11434/v1 \
  MEADA_DECIDER_MODEL=<modelo> \
  meada decide --decider
  ```

Qualquer gateway que fale um dos dois protocolos serve: basta trocar a URL. Exige-se HTTPS em
qualquer lugar; HTTP só para `localhost` ou um endereço de loopback.

#### O que é enviado

Por item, e nada além disto:

- a chamada (o trecho de código do ponto de chamada);
- a função que a contém, lida do disco (sem ela, quando a chamada está no topo do arquivo ou o
  símbolo que a envolve não é uma função);
- a lista de candidatos (repositório, método e caminho);
- a identidade do repositório que faz a chamada, o id do modelo e uma frase fixa que descreve a
  tarefa.

Nenhum código do provedor e nenhum outro arquivo saem da máquina. Para ver exatamente o que seria
enviado, rode `meada decide --decider --show-request`: ele imprime cada requisição, com a chave
trocada por `<hidden>`, e não envia nenhuma.

#### Privacidade

- Sem `--decider`, nenhum código sai da máquina. Indexar, consultar por MCP e decidir como pessoa
  não usam rede. Duas coisas fogem a isso e não são decisão: o `npx meada` baixa o binário, e o
  `mcp --listen` abre um servidor HTTP (veja o modo servidor).
- Com `--decider`, o código de cima sai para o endpoint que você configurou. Para manter tudo na
  máquina, use o protocolo `chat` com um modelo local (`http://localhost:...`).
- A chave só viaja no cabeçalho `Authorization` e nunca é impressa.
- Cada resposta é validada por item. Uma resposta inválida ou uma requisição que falhou é
  relatada sem gravar aquele item, e o comando sai com erro.
- O que o modelo propõe só chega ao `meada.decisions.toml`, que é um arquivo versionado: a
  decisão passa pela revisão do diff como qualquer outra mudança.

#### Limites fixos

- `--min-p` (de 0 a 1) vale 0,5 por padrão: abaixo dele, a proposta é impressa como sugestão e
  não é gravada.
- No máximo 50 candidatos por item: com mais, o item é pulado no modo decisor (o modo humano
  ainda o oferece).
- 120 segundos por requisição, da conexão ao último byte, sem nova tentativa.
- No protocolo `decisions`, as probabilidades de uma resposta devem somar 1, com tolerância de
  0,01.

No modo servidor, as decisões são feitas em um pipeline, não pelo servidor; veja
[Decidir em um pipeline](https://claudiomascaro.github.io/meada/server/#decidir-em-um-pipeline).

## 6. Comandos da CLI

```text
meada <COMANDO>
```

As mensagens seguem o `LANG` (português ou inglês); as do MCP são sempre em inglês. Toda
invocação aceita `--help` e `--version`.

### Os cinco comandos do build padrão

| Comando | O que faz |
| --- | --- |
| `meada index` | Monta os índices do workspace do repositório no diretório atual |
| `meada status` | Mostra o workspace: cada repositório, o estado do índice e os pares não encontrados |
| `meada export` | Imprime o índice como JSON ordenado, com os links que o tocam |
| `meada mcp` | Serve o repositório do diretório atual a agentes por MCP (stdio) |
| `meada decide` | Resolve os itens abertos deste repositório e registra as escolhas em `meada.decisions.toml` |

#### `meada index`

```sh
meada index [--workspace <manifesto>]
```

Atualiza o índice do repositório atual e dos pares. Com `--workspace`, atualiza o de todo
repositório que o manifesto `meada.workspace.toml` lista (o refresh do operador, no modo
servidor).

#### `meada status`

```sh
meada status [--workspace <manifesto>]
```

Mostra os repositórios do workspace e o estado de cada índice. Sob cada repositório, mostra os
recursos sem ligação, os `via` que não correspondem a nenhum símbolo, as decisões obsoletas ou à
espera de um par e cada arquivo-fonte que um language pack descartou por passar do seu
orçamento. Os pares que não foram encontrados aparecem como `missing peer`.

#### `meada export`

```sh
meada export [--workspace <manifesto> --repo <identidade>]
```

Imprime o índice do repositório atual em JSON ordenado (formato na seção 7). Com `--workspace`,
a flag `--repo` é obrigatória e escolhe o repositório do manifesto; uma exige a outra.

#### `meada mcp`

```sh
meada mcp
meada mcp --listen <endereço> --workspace <manifesto>
```

Sem flags, serve por MCP em stdio (seção 2). Com `--listen`, serve por HTTP no modo servidor, em
`host:porta`, `:porta` ou `porta` (loopback sem host); `--listen` e `--workspace` andam juntos.

#### `meada decide`

```sh
meada decide [--item <id>] [--dry-run] [--list] [--prune]
             [--decider [--show-request] [--min-p <p>]]
```

| Flag | Papel |
| --- | --- |
| `--item <id>` | Trata só esse item (o id que `--list` ou o `search` imprime), mesmo que já decidido, para refazê-lo |
| `--dry-run` | Imprime o arquivo que seria gravado e não escreve |
| `--list` | Imprime os itens abertos, as decisões obsoletas e as que esperam um par ausente; não pergunta nem grava |
| `--prune` | Remove as decisões obsoletas; mantém as que esperam um par ausente. Não combina com `--item`, `--dry-run` nem `--list` |
| `--decider` | Pergunta ao modelo configurado pelas variáveis `MEADA_DECIDER_*`, em vez de a uma pessoa |
| `--show-request` | Só com `--decider`: imprime cada requisição que seria enviada e não envia nenhuma |
| `--min-p <p>` | Só com `--decider`: a probabilidade mínima, de 0 a 1, para gravar (padrão 0,5) |

### Comandos dos módulos de nuvem

Os builds com módulos opcionais (a imagem `-cloud` os traz todos) acrescentam comandos para operar
o Meada como serviço de uma organização. Eles não fazem parte do uso local e são descritos nas
páginas de operação.

| Comando | Módulo | Papel |
| --- | --- | --- |
| `meada snapshot` | `snapshot` | O armazenamento de gerações: `publish`, `pull`, `lock`, `pointer` e `fence` |
| `meada workspace` | `generation-pointer` | As versões do workspace no store de snapshots |
| `meada workspaces` | `workspaces` | Os workspaces nomeados, com `mcp` no módulo `proposals` |
| `meada ui` | `ui` | A console de gestão |
| `meada github-app` | `github-app` | Credenciais do git a partir de tokens de GitHub App, para o job de indexação |
| `meada postgres` | `postgres` | A camada de dados: `migrate` cria o schema ou o leva à versão do binário |

As flags `--generations` de `status`, `export` e `mcp` existem só no módulo `snapshot`. Veja
[Modo servidor](https://claudiomascaro.github.io/meada/server/) e
[Módulos de nuvem](https://claudiomascaro.github.io/meada/cloud/).

## 7. O export

`meada export` imprime o índice de um repositório como JSON, pronto para `diff` entre duas
execuções e para alimentar uma base de conhecimento.

```sh
meada export > web.export.json
```

- **Determinístico**: a mesma árvore de arquivos produz os mesmos bytes. Toda lista vem ordenada
  e o texto termina com uma quebra de linha.
- **Sem texto-fonte**: quem precisa do trecho lê o `span` no arquivo.
- **Um repositório por export**: uma organização roda `meada export` em cada repositório e junta
  os resultados pela identidade dos símbolos.
- **Sem campo de versão do formato.** Quem lê deve ignorar os campos que não conhece: um campo
  novo é uma mudança menor.

Receitas com `jq` (um registro por símbolo, duplicados, fan-in entre repositórios) estão no
[README](../README.md#alimentar-uma-base-de-conhecimento).

### Campos do nível superior

As listas vazias são omitidas, menos `files`, `symbols`, `calls`, `providers`, `consumers` e
`links`, que sempre aparecem (como `repository`, que é um texto).

| Campo | Conteúdo |
| --- | --- |
| `repository` | A identidade do repositório (`example.com/acme/web`) |
| `files` | Os arquivos lidos: `path` e `hash` |
| `symbols` | Os símbolos: `id`, `span` (`path`, `start`, `end`, deslocamentos em bytes) e `hash` (BLAKE3 em hexadecimal dos bytes do span) |
| `calls` | Os pontos de chamada: `id`, `caller`, `target` e `level` (vazios enquanto não resolve), `candidates` (se ambígua), `span`, `narrowed` e `decorates` |
| `providers` | O lado provedor de cada contrato: `contract`, `site`, e se houver `gateway` e `shape` |
| `consumers` | O lado consumidor: `contract` (nulo se a URL não foi avaliada), `site`, `via` (como nomeia o repositório do provedor, como `host:api.example.com` ou `env:API_URL`) e `shape` |
| `links` | As ligações: `consumer`, `provider`, `contract`, `level`, `reason`, e se houver `by`, `p` e `drift` |
| `bindings`, `publishes` | Os bindings (`contract`, `exchange`, `pattern`, `site`) e as publicações (`exchange`, `key`, `site`) do RabbitMQ que o repositório declara |
| `documents` | Os documentos OpenAPI e AsyncAPI lidos: `path`, `format`, `version` e `contracts` |
| `packages` | Os pacotes que o repositório fornece: `name`, `version`, `repository`, `directory` e `ecosystem` |
| `dependencies` | As entradas de dependência que nomeiam um pacote do workspace: `manifest`, `field`, `name`, e se houver `alias` e `range` |
| `imports` | Os imports resolvidos para um pacote do workspace: `site`, `specifier`, `name`, `level`, `how` e `target` |
| `communities` | As comunidades: `name` e `members` (ordenados) |
| `processes` | Um processo por ponto de entrada: `entry_point`, `kind` (`route`, `export`, `channel` ou `program`), `label` e `steps` |
| `stale_decisions` | As decisões que não se aplicam mais: `call`, `target` e `cause` (`call-gone`, `item-not-open`, `target-gone` ou `not-candidate`) |

### Valores que se repetem

- **`level`**: `exact`, `declared`, `inferred`, `decided` ou `ambiguous`, do mais firme ao mais
  fraco.
- **Identificadores de contrato**: sempre começam pelo tipo, `http::GET::/api/health`,
  `channel::kafka::orders.created`, `package::...` ou `resource::postgres::orders`.
- **Identidade de símbolo**: `<repositório>::<caminho>::<nome>`, por exemplo
  `example.com/acme/web::src/status.ts::health`. Um ponto de chamada acrescenta o alvo e o
  índice dentro do símbolo: `...::health::fetch#0`.
- **`shape`** (em `providers` e `consumers`): os caminhos de campos que uma resposta ou uma
  mensagem carrega, com `source` e `fields`, entre outras chaves. O drift, na `link`, lista em `missing` os campos
  que o consumidor lê e o provedor não devolve.
- **`steps`** de um processo: `depth`, `repository`, `level`, `via` (`entry`, `call` ou `link`),
  `node`, e se houver `reason`, `mark`, `candidates` e `contract`.

### Exemplo

```json
{
  "repository": "example.com/acme/web",
  "files": [{ "path": "src/status.ts", "hash": "d61eae8a..." }],
  "symbols": [
    {
      "id": "example.com/acme/web::src/status.ts::health",
      "span": { "path": "src/status.ts", "start": 31, "end": 99 },
      "hash": "5cc3a63c..."
    }
  ],
  "calls": [],
  "providers": [],
  "consumers": [],
  "links": []
}
```

(Trecho ilustrativo: um export real traz todos os arquivos, símbolos e ligações do repositório.)

## Para onde ir depois

- A segunda metade do guia: cada tipo de contrato (HTTP, recursos e prefixos de gateway,
  documentos OpenAPI e AsyncAPI, canais, pacotes), cada linguagem com seus frameworks, clientes e
  limites conhecidos, drift de forma, comunidades e processos.
- [Capacidades e limites](https://claudiomascaro.github.io/meada/capabilities/): o estado
  detalhado de cada linguagem e contrato.
- [Modo servidor](https://claudiomascaro.github.io/meada/server/): operar o Meada para uma
  equipe.
- Um resultado errado no código do próprio Meada é um bug: abra uma issue com a consulta, a
  resposta e a esperada.
