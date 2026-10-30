# Guia do usuário

Este guia leva uma pessoa que fala português do zero ao Meada funcionando: instalar, conectar um
agente, configurar um workspace, decidir o que o código não deixa claro, usar a linha de comando
e ler o export. Os exemplos usam os repositórios `web`, `api` e `worker` e o host
`api.example.com`.

A segunda metade (seções 8 a 11) explica o que o Meada liga e onde ele para: cada tipo de
contrato, cada linguagem, o drift de forma, as comunidades e os processos. Use-a para saber se um
elo que falta é um limite conhecido ou um bug.

Sumário:

1. [Instalação](#1-instalação)
2. [Conectar um agente (MCP)](#2-conectar-um-agente-mcp)
3. [O workspace e o primeiro uso](#3-o-workspace-e-o-primeiro-uso)
4. [`meada.toml`: referência](#4-meadatoml-referência)
5. [Decisões e decisores](#5-decisões-e-decisores)
6. [Comandos da CLI](#6-comandos-da-cli)
7. [O export](#7-o-export)
8. [Tipos de contrato](#8-tipos-de-contrato)
9. [Linguagens](#9-linguagens)
10. [Drift de forma](#10-drift-de-forma)
11. [Comunidades e processos](#11-comunidades-e-processos)

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

## 8. Tipos de contrato

Um **contrato** é o que um repositório expõe e outro usa. O Meada lê cinco tipos, e cada um
tem a sua própria regra de ligação. Esta seção diz o que cada tipo liga e onde ele para, para você
distinguir um elo que falta por limite conhecido de um elo que falta por bug.

Toda identidade de contrato começa pelo seu tipo, e é essa identidade que você passa ao `context`
e ao `trace`:

| Tipo | Identidade | Exemplo |
| --- | --- | --- |
| Rota HTTP | `http::<MÉTODO>::<caminho>` | `http::GET::/api/orders/{}` |
| Canal | `channel::<broker>::<nome>` | `channel::kafka::orders.created` |
| Pacote | `package::<ecossistema>::<nome>` | `package::npm::@example/shared` |
| Recurso | `resource::<tipo>::<id>` | `resource::postgres::orders` |

Os documentos OpenAPI e AsyncAPI não são um tipo à parte: eles dão rotas e canais aos tipos
acima (seção 8.3).

Como saber se um elo que falta é bug:

- Se o código usa uma biblioteca, um framework ou uma forma que as seções 8 e 9 não listam, o elo
  falta por limite conhecido, não por bug.
- Se a biblioteca está na lista e o código usa a forma descrita, o elo deveria existir. Rode
  `meada status` e `search` com `kind=unresolved` e `kind=ambiguous` para ver se o Meada leu o
  lado e não o ligou, e então abra uma issue com a consulta, a resposta e a esperada.
- Uma chamada dentro de uma macro Rust não é indexada: um chamador que falta ali é "não provado".
- Um nome que o Meada não consegue calcular (uma URL montada em tempo de execução, um tópico
  lido do ambiente) não gera elo por si só. Ele aparece como lado não resolvido, e você decide com
  `meada decide` (seção 5).

### 8.1 Rotas HTTP

Uma rota liga quem a **serve** (o provedor, o código que a trata) a quem a **chama** (o
consumidor, uma chamada de cliente HTTP). Em todas as linguagens, o Meada normaliza o caminho:
`/users/:id`, `/users/{id}` e `/users/<int:id>` viram o mesmo `/users/{}`, e o método fica
em maiúsculas. Maiúsculas e minúsculas do caminho e versões de API são mantidas.

Como a ligação é escolhida, conforme o que a URL mostra:

1. **Host.** A URL tem um host (`https://api.example.com/orders`) que algum repositório lista em
   `[serve] hosts`. O elo é `exact`. Se nenhum repositório serve esse host, não há elo.
2. **Variável de ambiente.** A URL começa por uma leitura de ambiente (`API_URL`) que um par
   declara em `base_url_env`. O elo é `inferred`, e o Meada nunca lê o valor da variável.
3. **Host desconhecido.** A URL não diz de onde vem (um caminho relativo, por exemplo) e só um
   repositório do workspace serve esse método e esse caminho. O elo é `inferred`, com o motivo
   `the only repository serving it is`.
4. **Vários provedores.** O elo é `ambiguous`, e o Meada lista todos os candidatos, um elo por
   candidato. É um item aberto que `meada decide` resolve.

Um provedor que serve todos os métodos tem o método `ANY` (`http::ANY::/health`) e liga a
consumidores de qualquer método, com `any method` no motivo. Um consumidor cuja URL o Meada não
consegue calcular, mesmo depois de dobrar constantes e argumentos de funções que embrulham o
cliente, é um **consumidor não resolvido**: aparece no `search`, no `context` e no export, nunca
liga, e é um item aberto. Uma função que monta a URL a partir dos seus parâmetros vira um
consumidor em cada chamada dela, com os argumentos substituídos, em qualquer profundidade.

Por que um elo pode faltar:

- O caminho que um **provedor** declara tem uma parte que o Meada não lê (uma interpolação, um
  parâmetro, uma leitura de ambiente no meio do caminho): a rota não existe para o Meada. Um
  prefixo ilegível não vira a raiz, ele deixa a rota sem prefixo algum. (No consumidor, um buraco
  no meio da URL vira um parâmetro `{}` e ainda liga.)
- O framework ou o cliente não está na lista da linguagem (seção 9).
- O repositório que serve a rota não está entre os pares, e o workspace não o tem.

### 8.2 Recursos declarados e prefixos de gateway

**Recursos compartilhados.** O que nenhum código nomeia, como uma tabela ou um bucket, você
declara em `[[resource]]` (seção 4). O Meada não lê código para esse tipo: ele liga cada escritor
a cada leitor do mesmo `kind` e `id` em outro repositório, no nível `declared`, com o contrato
como motivo.

```toml
# api/meada.toml
[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]

# worker/meada.toml
[[resource]]
kind = "postgres"
id = "orders"
role = "read"
```

Com `via`, o lado é o símbolo; sem `via`, ou se nenhum símbolo corresponde, o lado é o
repositório inteiro e aparece como `(repository-level)`: um `trace` mostra o elo, mas não entra
nesse lado. O `meada status` lista, sob cada repositório, os recursos sem ligação e os `via` sem
símbolo. Como o Meada não interpreta `kind`, nada garante que `postgres` e `postgresql` sejam o
mesmo recurso: escreva do mesmo jeito nos dois lados.

**Prefixos de gateway.** Quando um gateway reescreve os caminhos, o consumidor chama um caminho
que o código do provedor não tem. Você declara a reescrita em `[serve]` com `prefix` e `strip`
(seção 4), e o Meada também aceita o caminho do servidor de cada URL de `servers` de um
documento OpenAPI como prefixo daquele documento. Uma rota liga também pelo seu caminho de
gateway, e esse elo é `inferred`, com o motivo `gateway prefix /shop`, `gateway strip /v1` ou
os dois. Dentro de um repositório, o caminho do código vence o do gateway.

### 8.3 Documentos OpenAPI e AsyncAPI

Um repositório com a tabela `[serve]` (mesmo vazia) tem os seus documentos lidos como fonte de
provedores. O Meada lê **OpenAPI 3.0 e 3.1** e **AsyncAPI 2.x e 3.0**, em JSON ou YAML, achados
pelo nome do arquivo ou listados em `[serve] documents`. Sem `[serve]`, os documentos são
ignorados.

- **Rotas.** Cada rota do documento é um provedor. Se o código já tem uma rota com o mesmo
  contrato, as duas viram um provedor só, com o ponto do código como sítio. Uma rota que só o
  documento lista aparece como `<documento> (documented only)` e nenhum `trace` entra nela. Um
  provedor vindo de um documento é um contrato escrito: não rebaixa o nível (um host que
  casa continua `exact`).
- **Canais.** Cada canal do AsyncAPI vira um lado de produtor ou consumidor, mesclado ao lado do
  código quando o contrato é o mesmo. Num documento, o nome de um canal `amqp` é o de uma fila,
  nunca o de uma exchange.
- **Formato dos dados.** O que o documento diz do corpo de uma resposta ou de uma mensagem é a
  forma (seção 10) daquele lado. Só a forma de uma rota entra na comparação de drift; a de uma
  mensagem é registrada, mas nenhum drift é calculado para canais.
- **Hosts.** O host de cada URL de `servers` conta como um `[serve] hosts`.

O `meada status` mostra, por repositório, o que foi lido e o que foi ignorado. Uma parte da forma
que o Meada não consegue listar (um objeto que aceita propriedades extras, um esquema atrás de
uma referência externa) fica **aberta**: o que estiver abaixo dela pode existir.
Quando o documento e o código discordam sobre a forma de uma rota, vale a forma do documento.

### 8.4 Canais de mensageria

Um **canal** é uma fila ou um tópico de um broker. Cada produtor liga a cada consumidor do mesmo
canal, também dentro do mesmo repositório. A mensagem vai do produtor ao handler, então o
`trace` desce nessa direção.

O broker faz parte da identidade (`channel::<broker>::<nome>`), e o mesmo nome em dois brokers
são dois canais:

| Broker | Valor em `<broker>` | Observação |
| --- | --- | --- |
| Kafka | `kafka` | |
| RabbitMQ | `amqp` | exchanges, filas e bindings |
| Amazon SQS | `sqs` | |
| NATS | `nats` | curingas `*` e `>` |
| Redis streams | `redis` | |
| BullMQ | `bullmq` | só em TypeScript e JavaScript |

Os clientes de cada linguagem estão na seção 9. JMS (`jms`) e Azure Service Bus (`servicebus`)
são brokers de Java, Kotlin e C#, trabalho planejado da Release 2 (seção 9), e não fazem parte
do 1.0 (um documento AsyncAPI com esse protocolo ainda dá lados de canal desses brokers).

Regras que valem para todos:

- **Nome calculado como uma URL.** Constantes, argumentos de funções que embrulham o cliente e
  interpolações simples são dobrados. Um nome que não se calcula (lido do ambiente, montado em
  tempo de execução) é um **lado não resolvido**: contrato `channel::kafka`, mostrado como
  `(unresolved)`, presente no export, nunca ligado e nunca um item aberto. É um limite
  conhecido, e `meada decide` não o vê.
- **RabbitMQ.** Um binding de fila a exchange (`bindQueue`, `QueueBind`, `queue_bind`) é uma regra de roteamento,
  não um lado.
  Uma publicação numa exchange nomeada chega às filas cujo binding casa com a chave de
  roteamento, vindo de qualquer repositório do workspace, em qualquer tipo de exchange: `exact`
  quando o padrão é a chave, `inferred` com curinga. Uma publicação na exchange padrão (vazia) é
  produtora da fila que a chave nomeia.
- **NATS.** Um consumidor com `*` (um token) ou `>` (o resto) liga aos produtores cujo assunto
  casa, no nível `inferred`.
- **Clusters não são modelados.** Um nome de um tipo de broker é um canal em todo o workspace:
  tópicos de mesmo nome em clusters diferentes ligam como se fossem de um cluster só. Se isso
  produz um elo falso, é a limitação, não um bug.
- **O handler** de um consumidor é a função que a assinatura nomeia.

### 8.5 Pacotes

Um **pacote** liga quem o declara em um manifesto (o provedor) a quem o lista como dependência
(o consumidor), e vai além: um `import` de uma função do pacote chega ao código do provedor,
então o `context` de uma função exportada lista os chamadores de todo o workspace e o `trace`
cruza o salto. Só entram pacotes que algum repositório do workspace fornece; dependências de fora
do workspace não aparecem em lugar nenhum, o Meada não consulta registro algum e não lê
lockfiles (`go.sum`, `Cargo.lock` e semelhantes).

O motivo de um elo de pacote traz a faixa como escrita e, quando é o caso, termina com
`range unchecked` (a faixa não foi conferida) ou, no npm, `range mismatch` (a faixa rejeita a
versão local do provedor). No npm, uma faixa que aceita a versão local não leva sufixo. Uma
importação que chega ao código acrescenta `; import <o que foi usado>` ao motivo.

| Ecossistema | Manifesto | Identidade | Observações |
| --- | --- | --- | --- |
| npm | `package.json` com `name` | `package::npm::<nome>` | Lê `dependencies`, `devDependencies`, `peerDependencies` e `optionalDependencies`. A faixa é conferida (semver do npm) |
| PyPI | `pyproject.toml` com nome em `[project]` ou `[tool.poetry]` | `package::pypi::<nome>` | Normalizado pela PEP 503. Lê também `requirements*.txt`. Faixas não são conferidas |
| Cargo | `Cargo.toml` com `[package] name` | `package::cargo::<nome>` | Cada membro do workspace Cargo é um provedor. Faixas não são conferidas |
| Go | `go.mod` com `module` | `package::go::<caminho do módulo>` | Lê `require` e `replace` para um diretório local |

Detalhes por ecossistema:

- **npm.** Um `import` entra no código do provedor por `exports`, `types`, `module`, `main` e
  reexportações. Se a entrada aponta para a saída de um build, o `outDir` do `tsconfig.json`
  leva de volta ao `rootDir`. Um pacote com o mesmo nome no npm e no PyPI não se liga. Uma faixa
  `workspace:` só vale dentro do próprio repositório. O `search` com `kind=package` informa nome,
  repositório, diretório, versão e consumidores.
- **PyPI.** O `import` chega ao código como `exact` quando o módulo coincide com o nome da
  distribuição, `inferred` quando é um dos seus módulos de topo e `ambiguous` quando várias
  distribuições o expõem.
- **Cargo.** Uma dependência por `path` resolve o crate daquele diretório, no mesmo repositório
  ou em outro do workspace (o caminho é resolvido pelo texto, sem sair das raízes dos
  repositórios nem seguir links simbólicos); por `git`, o repositório de URL normalizada;
  por versão, o provedor de mesmo nome. `package =` dá o nome real de uma dependência
  renomeada, e `workspace = true` lê `[workspace.dependencies]`. Um `use` segue `[lib] name`
  ou o nome renomeado e os `pub use` até os itens públicos.
- **Go.** Um `replace` que sai do repositório é resolvido pelo texto, como o `path` do Cargo, e
  uma substituição por outro módulo fica de fora. Um `import` de `<módulo>/<diretório>` chega às
  funções, tipos e métodos exportados do diretório.

Quando dois provedores servem o mesmo nome, o elo é `ambiguous`, um por candidato, com o motivo
iniciado por `<n> providers fit`.

Maven e Gradle, NuGet e SwiftPM são os gerenciadores de Java e Kotlin, C# e Swift, trabalho
planejado da Release 2 (seção 9): chegam com essas linguagens, depois do 1.0.

## 9. Linguagens

O Meada 1.0 lê **TypeScript e JavaScript, Python, Go e Rust**. Cada seção abaixo lista os
arquivos lidos, os frameworks e clientes HTTP, as leituras de ambiente, os manifestos, os
clientes de canais e os limites conhecidos. Uma biblioteca fora destas listas não gera ligação.

Regras que valem para as quatro linguagens:

- O Meada lê o texto com tree-sitter. Ele não roda compilador, interpretador, gerenciador de
  pacotes nem rede, e nunca lê o valor de uma variável de ambiente.
- Uma chamada é `exact` quando o código mostra o tipo do receptor ou o caminho escrito, e
  `inferred` ou `ambiguous` quando só o nome decide (seção 2, "Como ler os níveis").
- Uma biblioteca conta quando o arquivo a importa (o `fetch` global do JavaScript é a exceção).
- Um decorador, um atributo ou uma anotação escrito sobre uma definição é uma chamada do escopo
  que a envolve. O `context` os lista à parte, em `annotations:`, e não entre os chamados.
- Uma URL ou um nome de canal é dobrado: literais, concatenações, interpolações simples,
  constantes (também de outros arquivos do repositório) e os argumentos de funções que embrulham
  o cliente, em qualquer profundidade. O que não se dobra é um consumidor não resolvido (HTTP) ou
  um lado não resolvido (canais).

**Java, Kotlin, C# e Swift** são trabalho planejado da Release 2 do ADR 0003 e não fazem parte do
1.0. Eles chegam depois, como versões menores 1.x, sob as issues #26 a #29. Este guia e a
promessa de estabilidade do 1.0 não os cobrem; para um repositório nessas linguagens, conte
apenas com o que ele declara: um `meada.toml`, um documento OpenAPI ou AsyncAPI.

### 9.1 TypeScript e JavaScript

**Arquivos.** `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs` e `.cjs`. Um `import` usa os `paths` e o
`baseUrl` do `tsconfig.json` (ou `jsconfig.json`) mais próximo, no diretório do arquivo ou acima,
relativos ao diretório daquele arquivo de configuração. O `node_modules` é ignorado.

**O que vira definição.** Funções, classes e métodos, os `const`, `let` e `var` de uma função ou
classe e o `export default`. No topo de um arquivo, funções atribuídas também contam:
`obj.y = function () {}` define `obj.y`, `X.prototype.y = function () {}` define o método `X.y`,
`exports.y = ...` e `module.exports.y = ...` definem a exportação `y`, e `module.exports =
function () {}` define a exportação padrão. Um `let` ou `var` só é definição se o arquivo nunca
atribui o nome de novo. As funções do objeto literal de um `const` do topo definem `o.m`.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `fetch` global, e uma chamada a um parâmetro cujo padrão é `fetch` |
| Provedor | Next.js (app router: arquivos de rota) e Express |

No Express, uma rota vale sob cada prefixo em que o roteador é montado (`use`, `Router()`). Um
prefixo ou um caminho com uma parte ilegível (um buraco de template literal, um parâmetro, uma
leitura de ambiente) não gera rota, e um roteador montado só sob um prefixo assim fica sem prefixo
algum. Um `:id` literal continua sendo parâmetro. Nenhum cliente além do `fetch` é lido: `axios`,
`got` e `XMLHttpRequest` não geram consumidor.

**Ambiente.** `process.env.X`, `process.env["X"]` e `import.meta.env.X`, sozinhos ou passando por
métodos de texto como `trim()`, `replace(...)` e `toLowerCase()`. A URL que começa por uma leitura
assim liga pelo `base_url_env` do par. Arquivos `.env` não são lidos.

**Manifestos.** `package.json` (`name`, `version`, os quatro campos de dependências, `exports`,
`types`, `module` e `main`) e `tsconfig.json` (`paths`, `baseUrl`, `outDir`, `rootDir`). Veja o
npm na seção 8.5.

**Canais.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `kafkajs` |
| RabbitMQ | `amqplib` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` |
| NATS | `nats` |
| Redis streams | `ioredis`, `node-redis` |
| BullMQ | `bullmq` |

**Formato dos dados.** É a única linguagem em que o Meada lê o formato de uma resposta do próprio
código, nos dois lados: o que um consumidor `fetch` lê do `.json()` e o que um provedor
Next.js ou Express devolve, por esquemas zod ou por objetos literais. Veja a seção 10.

**Limites conhecidos.**

- `require` e `module.exports` não são importações entre arquivos, e `new F()` de uma função
  construtora não alcança os métodos de `F.prototype`.
- Um `let` ou `var` reatribuído em qualquer lugar do arquivo não vale como uma definição só.
- Um módulo que o `tsconfig.json` não mapeia e que está fora do repositório é externo.
- Rotas montadas por um valor que o Meada não lê (um roteador vindo de uma chamada de fora) não
  são servidas em lugar nenhum.

### 9.2 Python

**Arquivos.** `.py`. Cada `pyproject.toml` dá raízes de código (o seu diretório, as raízes que
ele declara ou `src`) aos arquivos abaixo dele, e é por elas que um `import` encontra o módulo.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `requests`, `httpx`, `aiohttp` |
| Provedor | FastAPI, Flask, Django |

- **Consumidores.** Uma função do módulo (`requests.get`, `httpx.post`, também sob um alias) ou
  um método de um cliente que o código mostra ter vindo do construtor da biblioteca (`Session()`,
  `Client()`, `AsyncClient()`, `ClientSession()`), guardado numa variável, num atributo de
  `self`, aberto por `with` ou recebido por parâmetro. A `base_url=` do construtor é a base das
  URLs relativas. Um `get` em qualquer outro objeto não é nada.
- **FastAPI e Flask.** Os decoradores de método valem num objeto que remonta ao construtor do
  framework (`FastAPI()`, `APIRouter()`, `Flask()`, `Blueprint()`). No FastAPI: `@app.get`,
  `@router.post` e os outros métodos, e `api_route`. No Flask: `@app.route(..., methods=[...])`
  (o atalho `@app.get` não gera rota). O prefixo de `APIRouter(prefix=)`, de `include_router` e
  de `mount` é somado; no Flask, o `url_prefix` dado a `register_blueprint` substitui o do
  próprio `Blueprint`. O que nada monta é servido na raiz, exceto um `APIRouter` ou `Blueprint`, que só serve
  onde uma montagem que o Meada lê o coloca.
- **Django.** O Meada parte da URLconf raiz (um arquivo com `urlpatterns` que nenhum `include()`
  nomeia) e desce por `path()`, `re_path()` e `include()`. Um `re_path()` só vale com texto literal
  e grupos nomeados (`(?P<nome>...)`); qualquer outro padrão não gera rota. Uma view de função
  serve os métodos que `require_http_methods`, `require_GET`, `require_POST` e `require_safe`
  permitem, ou todos; `C.as_view()` serve os métodos (`get`, `post`...) que `C` define.

**Ambiente.** `os.environ["X"]`, `os.environ.get("X")` e `os.getenv("X")` (também `environ` e
`getenv` soltos, casados como escritos e não pelos imports), sozinhos ou num buraco de f-string,
com fallback `or`, com argumento padrão e com métodos de texto como `strip` e `replace`.

**Manifestos.** `pyproject.toml` (`[project]` e `[tool.poetry]`, incluindo `[project.scripts]`) e
`requirements*.txt`. Veja o PyPI na seção 8.5.

**Canais.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `confluent-kafka`, `kafka-python` |
| RabbitMQ | `pika` |
| Amazon SQS | `boto3` |
| NATS | `nats-py` |
| Redis streams | `redis-py` |

**Limites conhecidos.**

- Um decorador de rota num objeto que não remonta ao construtor do framework não gera rota: o
  Meada não adivinha pelo nome.
- Um prefixo ou um caminho com uma parte ilegível (um buraco de f-string, um parâmetro) não gera
  rota. `str.format` e `%` não dobram uma URL; f-strings e `+` sim.
- Um roteador montado por um import que não resolve não é presumido na raiz.
- No Django, sem uma URLconf raiz determinável (um `include()` que o Meada não lê), nenhuma rota é
  servida.
- Não há leitura de arquivos de configuração do Python (`settings.py`, `.env`) além das leituras de
  ambiente no código.

### 9.3 Go

**Arquivos.** `.go` incluídos pelas regras de build do pacote. O `module` de cada `go.mod`
resolve os imports para diretórios de pacote, inclusive um `replace` para um diretório do
repositório. Um módulo aninhado é dono da sua subárvore, e os arquivos de um diretório com o mesmo
`package` compartilham escopo (os `_test.go` também). O Meada não roda a toolchain do Go.

A análise mira linux/amd64: arquivos `_windows.go`, com um `//go:build` falso ou que usam cgo
(`import "C"`) ficam de fora.

**O que vira definição.** Funções, tipos, interfaces e métodos (`Tipo.Método`). Chamadas
resolvem dentro do pacote e por imports (`pkg.F()`, com alias ou ponto). A biblioteca padrão e os
módulos externos permanecem externos.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `net/http` (`Get`, `Head`, `Post`, `PostForm`, `NewRequest`, `NewRequestWithContext`), resty |
| Provedor | `net/http`, gin, echo, chi, gorilla/mux |

Um roteador é a chamada que o cria (`gin.Default()`, `echo.New()`, `chi.NewRouter()`,
`http.NewServeMux()`, `mux.NewRouter()`). Os grupos de prefixo (`Group(prefixo)` do gin e do echo,
`Route(prefixo, fn)` e `Mount(prefixo, roteador)` do chi, `PathPrefix(...).Subrouter()` do
gorilla/mux) somam o prefixo, e o gorilla/mux restringe os métodos com
`.Methods(..)`. Os parâmetros de caminho de cada framework (`{nome}`, `:nome`, `*resto`) viram `{}`.
A base de um cliente resty é o argumento de `SetBaseURL`.

**Ambiente.** `os.Getenv` e `os.LookupEnv`, inclusive o padrão escrito com `if v == ""`,
`len(v) == 0`, `!ok` ou `!ok || v == ""` quando o `ok` vem do mesmo `os.LookupEnv`, e depois
`strings.TrimSuffix`, `TrimRight` e `TrimSpace`.

**Manifestos.** `go.mod` (`module`, `require`, `replace`). `go.sum` não é lido. Veja o Go na
seção 8.5.

**Canais.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` |
| RabbitMQ | `amqp091-go` |
| Amazon SQS | `aws-sdk-go-v2` |
| NATS | `nats.go` |
| Redis streams | `go-redis` |

**Limites conhecidos.**

- Uma chamada `recv.M()` é `exact` quando o código dá o tipo do receptor (um parâmetro, campo,
  `var`, literal composto ou resultado de função). Sem isso, a resolução por nome é `inferred`
  ou `ambiguous`.
- Uma chamada por uma interface para no método da interface: o Meada não escolhe a
  implementação.
- Um handler embrulhado de um jeito que o Meada não lê (`http.StripPrefix`, `echo.WrapHandler`, um
  middleware) monta o roteador onde ele não alcança, e as rotas dele não aparecem.
- O `Add(método, caminho, handler)` do echo não é lido.
- Código com cgo, `_windows.go` e `//go:build` falso não é indexado.

### 9.4 Rust

**Arquivos.** `.rs` alcançáveis pela árvore de módulos de cada crate. Todo `Cargo.toml` com
`[package]` define um crate, em qualquer diretório, e o Meada segue os `mod` sem rodar o `cargo`
nem o `rustc`. Um arquivo que nenhum `mod` alcança fica de fora.

**O que vira definição.** Funções, tipos, traits, módulos e métodos de `impl`. Chamadas como
`crate::a::f()`, `Tipo::new()` e `Self::f()` seguem o caminho escrito; a resolução também segue
`use`, grupos, renomeações, globs, `pub use` e outros crates do workspace pelo nome. Em `x.m()`, o
tipo do receptor vem de um `let`, parâmetro, campo, `self` ou retorno de função escrito; uma
chamada sobre o resultado de outra (`Router::new().route(..)`, também por `.await` ou
parênteses) resolve pelo tipo de retorno escrito da chamada interna; um `dyn Trait` ou genérico
limitado por trait chega ao método do trait. Sem tipo escrito, a resolução é por nome.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `reqwest` (incluindo `blocking`), `ureq` |
| Provedor | axum, actix-web, Rocket |

No axum, `route`, `nest` e `merge` somam os prefixos pela cadeia do `Router::new()`. No actix-web e
no Rocket, a função com o atributo de rota (`#[get]`, `#[route(...)]`) ou nomeada por
`.route(path, web::get().to(h))` é o handler, e `web::scope`, `web::resource`, `.configure(f)` e a
instância do Rocket montam prefixos. O corpo de `format!` e `concat!` dobra uma URL quando os
argumentos são literais ou, no `format!`, nomes simples; qualquer outro argumento vira um
parâmetro (o `concat!` só aceita literais). Um `let
mut` String só lido e acrescido por `push_str`, `push` e `+=` também dobra.

**Ambiente.** `std::env::var("X")`, `dotenvy::var("X")` e `env!("X")` (também como argumento de
`concat!` ou `format!`), com `unwrap`, `expect`, `?`, `trim`, `to_string` e semelhantes.
`option_env!` não é uma leitura.

**Manifestos.** `Cargo.toml` (`[package]`, `[dependencies]`, `[dev-dependencies]`,
`[build-dependencies]`, `[workspace.dependencies]`, `[lib] name`). Veja o Cargo na seção 8.5.

**Canais.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `rdkafka` |
| RabbitMQ | `lapin` |
| Amazon SQS | `aws-sdk-sqs` |
| NATS | `async-nats` |
| Redis streams | crate `redis` |

**Limites conhecidos.**

- **Chamadas dentro de macros não são indexadas** (`assert!`, `format!`, `vec!`, `println!`).
  Um chamador que só chama de dentro de uma macro não aparece, e o `context` o trata como "não
  provado". A única exceção é o corpo entre chaves de uma macro de itens do próprio repositório
  (um `macro_rules!` cuja primeira regra recebe `$($x:item)*` e reescreve cada item só com
  atributos externos, como o `cfg_rt!` do tokio): ele é lido como itens do módulo. Macros de outros
  crates continuam sem leitura.
- Métodos que não estão nos blocos `impl` do tipo, incluindo os gerados por `derive` ou fornecidos
  por traits externos, não geram ligação.
- A análise usa a configuração padrão de cada crate para x86_64 Linux: itens sob um `#[cfg(..)]`
  falso e features fora de `default` ficam de fora; módulos compilados num build de teste entram,
  não só os `#[cfg(test)]`.
- Um roteador axum entregue a uma chamada que o Meada não lê (`route_service`, `fallback_service`,
  um embrulho de fora) não é montado em lugar nenhum.
- `option_env!` e um `env!` que o próprio arquivo define não são leituras.

## 10. Drift de forma

A **forma** de um contrato é o que ele carrega, registrado como um conjunto de caminhos de
campos (`items[].id`; `[].id` quando o corpo é uma lista). O **drift** é um campo que um consumidor
lê e que a forma do provedor ligado a ele não contém: um campo que a rota deixou de devolver. Um
elo pode estar de pé e mesmo assim quebrar, e o drift é o aviso para isso.

De onde vem cada forma:

| Lado | Fonte | Nível do campo |
| --- | --- | --- |
| Provedor | Documento OpenAPI (qualquer linguagem) | `exact` |
| Provedor | Esquema zod pelo qual a resposta de um handler Express ou Next.js passa | `exact` |
| Provedor | Objetos literais que um handler Express ou Next.js devolve | `inferred` |
| Consumidor | Tipo declarado ou esquema (`z.infer`) do corpo lido | `exact` |
| Consumidor | Campos lidos do `.json()` de um `fetch` | `inferred` |

Cada caminho que falta traz o seu nível: `exact` quando a forma do provedor e o caminho do
consumidor são ambos declarados (documento, esquema, tipo), e `inferred` quando um dos dois vem do
que o código faz (leituras de campo, literais devolvidos). O nível do elo não muda por causa do
drift. Hoje só código TypeScript e JavaScript gera a forma de um consumidor, e só consumidores
`fetch` e provedores Next.js e Express geram a de um provedor a partir do código; provedores de
outras linguagens entram pelo documento.

Como o Meada mostra o drift:

| Consulta | Resultado |
| --- | --- |
| `trace` | Acrescenta `drift: missing <caminho>` à linha do elo |
| `context` numa rota | Mostra a origem da forma, os consumidores, o drift, o nível e as leituras não verificadas |
| `search` com `kind=drift` | Lista os drifts, um por caminho que falta |
| `meada status` | Conta as leituras que faltam, sob o repositório do consumidor; o provedor não recebe contagem |

Exemplo de linha de `trace`:

```text
example.com/acme/api src/orders.ts:12 GET /api/orders
  →[inferred: base URL from API_URL, a base_url_env of peer example.com/acme/api] example.com/acme/web src/orders.ts:8 loadOrders drift: missing discount
```

(Exemplo ilustrativo no formato do `trace`: o `web` lê `discount` da resposta de `GET
/api/orders`, e o `api` não a devolve.)

Os limites desta análise, para você não tomar um silêncio por ausência de drift:

- **Só presença, nunca tipos.** O Meada compara a presença dos caminhos. Ele não compara tipos,
  formatos, `required` nem `nullable`.
- **Partes abertas não são verificadas.** Um objeto que aceita propriedades extras
  (`additionalProperties`, `.passthrough()`, `z.record`, `z.any`), um esquema atrás de uma
  referência externa e um corpo que o Meada não consegue listar deixam a parte **aberta**: tudo
  abaixo dela pode existir, e a leitura aparece em "não verificadas" em vez de drift.
- **Nada abaixo de um escalar.** `name.length` não é um campo, e `lista.length` abaixo de um
  array não é drift.
- **Só o caminho mais raso.** De cada ramo, só o primeiro caminho que falta é listado.
- **Elos ambíguos não têm drift.** Sem um provedor único, não há forma para comparar.
- **O handler precisa responder de um jeito lido.** Um corpo que vem de um valor que o Meada não
  segue (uma chamada de fora, um spread de algo desconhecido) abre aquele caminho. Só respostas 2xx
  contam; uma resposta com status literal fora de 2xx é ignorada.
- **Sem documento nem esquema, a forma é só o que o código mostra.** Os literais e as leituras são
  um palpite sobre o que o código devolve, por isso `inferred`.

## 11. Comunidades e processos

Duas análises dão um mapa do código além das ligações pontuais. Nenhuma usa modelo de linguagem,
e as duas saem iguais em qualquer máquina.

### Comunidades

Uma **comunidade** é um grupo de símbolos de um repositório que se chamam mais entre si do que
chamam o resto. O Meada as acha pelas chamadas de nível `exact` ou `inferred`, com o algoritmo de
Leiden, sem aleatoriedade e com aritmética de inteiros. O nome vem do diretório que tem mais
membros (`src`, e `src#2` quando outra comunidade do repositório já tem esse nome). Todo símbolo
que está em pelo menos uma dessas chamadas, como quem chama ou como quem é chamado, é membro de
alguma comunidade; um símbolo cuja única chamada é para si mesmo, ou que só é chamado do topo de
um arquivo, forma uma comunidade só; um símbolo fora de toda chamada não pertence a nenhuma.

- `search` com `kind=community` acha comunidades pelo nome.
- `context` de um símbolo diz a sua comunidade, ou `community: none`.
- `meada export` inclui as comunidades.

### Processos

Um **ponto de entrada** é uma porta de um repositório. Cada um tem o seu processo; nenhuma
pontuação ou limiar escolhe quais valem.

| Tipo | O que é |
| --- | --- |
| `route` | Cada rota HTTP que o repositório serve; o handler é a função |
| `channel` | Cada canal que um handler consome |
| `program` | Um início de programa que a linguagem ou o manifesto declara |
| `export` | Cada função ou método exportado que nada chama no próprio repositório |

Os inícios de programa, por linguagem:

- **Python:** `if __name__ == "__main__":` (símbolo `__main__`) e as funções de
  `[project.scripts]` na forma `módulo:função`. Os nomes exportados são os públicos, sem `_` à
  frente, ou os de um `__all__` literal.
- **Rust:** o `fn main` de cada binário, em `src/main.rs`, `src/bin/` ou `[[bin]] path`.
- **Go:** `func main` num `package main`.
- **TypeScript e JavaScript:** não há tipo `program`; as portas são as rotas, os handlers de
  canal e as funções exportadas que ninguém chama.

Um **processo** é a árvore de chamadas que um ponto de entrada põe em movimento: as chamadas
dentro do repositório em todos os níveis e as ligações para outros repositórios, até as folhas.
Ele termina numa chamada ou ligação ambígua (um passo que lista os candidatos), num consumidor não
resolvido, num ciclo ou num par ausente. O processo não é guardado: é percorrido na consulta, a
partir das chamadas e ligações já guardadas.

- `trace` com `process::<identidade do ponto de entrada>` lê a árvore. A identidade é a do
  handler, mais o contrato para uma rota ou um canal, por exemplo
  `process::example.com/acme/worker::src/consumer.ts::onOrderCreated::channel::kafka::orders.created`.
- `search` com `kind=process` acha processos pelo rótulo do ponto de entrada (`GET /users/{}` ou o
  nome qualificado).
- `context` de um símbolo lista os processos de que ele faz parte, andando pelas chamadas e
  ligações em direção aos chamadores.
- `meada export` inclui cada processo.

### Duplicatas

O `search` com `kind=duplicate` lista os símbolos cujo texto é idêntico em dois ou mais lugares do
workspace (em repositórios diferentes ou no mesmo), em grupos ordenados pelo tamanho do trecho. O
export traz o `hash` BLAKE3 de cada símbolo. Cópias quase iguais (uma diferença de espaço em
branco) não são duplicatas.

## Para onde ir depois

- [Capacidades e limites](https://claudiomascaro.github.io/meada/capabilities/): o estado
  detalhado de cada linguagem e contrato.
- [Estabilidade](https://claudiomascaro.github.io/meada/stability/): o que o 1.x congela.
- [Modo servidor](https://claudiomascaro.github.io/meada/server/): operar o Meada para uma
  equipe.
- Um resultado errado no código do próprio Meada é um bug: abra uma issue com a consulta, a
  resposta e a esperada.
