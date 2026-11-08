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

### npm (`npx @meada/meada`)

```sh
npx @meada/meada --version
npx @meada/meada index
```

O pacote `meada` do npm contém só o instalador: ele baixa da release do GitHub o binário da sua
plataforma e o executa com os argumentos que você passou. A versão do pacote é sempre a da
release. A instalação baixa o binário na hora, então ela precisa de rede (não há instalação
offline pelo npm). É o canal que mais interessa a quem configura um agente, porque
`npx -y @meada/meada mcp` é a linha de lançamento do servidor MCP (seção 2).

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
linha de lançamento documentada é `npx -y @meada/meada mcp`, que dispensa uma instalação separada.

### Configuração do cliente

No `.mcp.json` do Claude Code, ou no arquivo equivalente do seu cliente:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["-y", "@meada/meada", "mcp"] }
  }
}
```

No Claude Code, o mesmo em um comando: `claude mcp add meada -- npx -y @meada/meada mcp`.

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
| `value` | um valor de configuração de um arquivo YAML ou JSON, pelo valor ou pelo caminho da chave |
| `iac` | um nó IaC (um resource, um bloco data ou uma chamada de módulo do Terraform, ou um objeto Kubernetes), pelo endereço, pelo tipo ou pelo nome físico |
| `ambiguous`, `unresolved` | os itens abertos, com o comando `meada decide` de cada um |

Cada resultado é uma linha: repositório, `caminho:linha` e nome.

Sem `kind`, a busca responde com símbolos e contratos; só quando nenhum dos dois casa ela
responde com valores de configuração. Com `kind = "value"`, cada linha é `<repositório>
<caminho>:<linha> <caminho da chave> = <valor>`, um texto entre aspas como em JSON:

```text
example.com/acme/app chart/values-prd.yaml:6 alerts.loki[0].labels.support = "team-b"
example.com/acme/app deploy/app.yaml:26 [1].data.password = redacted
```

O caminho da chave segue o estilo do jq: chaves unidas por pontos, um item de lista `[n]` a
partir de 0, uma chave que não é um identificador simples entre `["..."]` (`metadata.labels["app.kubernetes.io/name"]`)
e, num arquivo com mais de um documento, o índice do documento primeiro (`[2].spec.replicas`). A
consulta, em minúsculas, casa o valor ou o caminho da chave: iguais primeiro, depois os que
começam com ela, depois os que a contêm. Um valor acima de 1 KiB é guardado cortado e aparece
seguido de `(cut)`.

Os iguais e os que começam com a consulta saem de B-trees sobre o caminho da chave e o valor em
minúsculas; os que a contêm, de uma tabela FTS5 com o tokenizador de trigramas, quando a consulta
tem três caracteres ou mais (uma consulta menor varre as linhas, com a mesma resposta). Esse
índice custa espaço: guarda cerca de quatro vezes o texto dos caminhos de chave e dos valores
(num repositório de 92 mil valores e 5 MB de texto, 21 MB de `.meada/index.db`), além do que o
índice já tinha.

Com `kind = "iac"`, cada linha é um nó IaC: `<repositório> <caminho>:<linha> <endereço> (<tipo>,
name "<nome físico>", root <módulo raiz>)`, sem o `name` quando o bloco não tem atributo `name`,
e com `pattern` no lugar quando uma parte do nome não se resolve (`pattern "orders-*"`). A busca
sem `kind` não responde com nós IaC.

```text
example.com/acme/infra envs/prod/main.tf:35 aws_sqs_queue.dlq[0] (aws_sqs_queue, name "dead-letters", root envs/prod)
example.com/acme/infra modules/queue/main.tf:9 module.orders.aws_sqs_queue.this (aws_sqs_queue, name "orders", root envs/prod)
```

#### `context`: entender um símbolo

| Parâmetro | Tipo | Padrão | Papel |
| --- | --- | --- | --- |
| `symbol` | texto, obrigatório | | Um nome, `<caminho>::<nome>` ou a identidade completa; um recurso (`resource::postgres::orders`), um canal (`channel::kafka::orders.created`) ou o endereço de um nó IaC (`aws_sqs_queue.orders`, `<caminho>::<endereço>`) |
| `budget` | inteiro | 2000 | Orçamento de tokens da resposta |

Devolve a comunidade e os processos do símbolo, o trecho de código lido do disco, quem o chama
(`callers:`), quem ele chama (`callees:`) e os contratos que ele toca (`contracts:`). Num recurso,
lista quem escreve e quem lê; num canal, os produtores e os consumidores. Num nó IaC, o tipo, o
nome físico (seguido de `(pattern)` quando uma parte não se resolve), o módulo raiz, as
variáveis da raiz por onde o nome passou (`overridable:`, cada uma com `(default)` ou
`(tfvars)`), os nós que ele referencia (`references:`), os que o referenciam
(`referenced by:`), cada um com o nome por onde passou (`via var.queue_arn`), e os valores de
configuração do bloco (`values:`). Um endereço que mais de um módulo raiz tem lista os nós, cada
um com o que passar: `<endereço> (root <módulo raiz>)`. Um símbolo do código com o mesmo nome
vem antes do nó; `<caminho>::<endereço>` chega no nó.

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
| `from` | texto | | O início: um símbolo, uma rota (`GET /users/{}` ou o contrato), um ponto de chamada, o endereço de um nó IaC ou um processo (`process::<identidade>`, só rio abaixo) |
| `changes` | texto | | `uncommitted`: parte dos símbolos que o diff da árvore de trabalho toca |
| `direction` | `down` ou `up` | `down` | `down` é o que o início alcança, `up` é o que o alcança |
| `budget` | inteiro | 2000 | Orçamento de tokens da resposta |

Passe `from` ou `changes: uncommitted`, nunca os dois. O segundo serve para medir o impacto do
que acabou de ser editado, e só funciona no Meada local (o modo servidor não tem árvore de
trabalho). De um nó IaC, `down` segue as referências dele e `up` as que chegam nele, dentro do
seu módulo raiz, entrando nos módulos locais pelas entradas e saídas deles.

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

- **Ocultos**: arquivos e diretórios cujo nome começa com `.` não são lidos, exceto a configuração
  de CI na raiz (`.github/`, `.gitlab-ci.yml`, `.gitlab/`, `.circleci/`, `.buildkite/` e
  `.drone.yml`), cujos arquivos YAML e JSON viram valores de configuração.
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

### Valores de configuração

Cada escalar (texto, número ou booleano; um nulo fica de fora) de cada arquivo `*.yaml`, `*.yml`
e `*.json` que a varredura lê, e dos arquivos YAML e JSON da configuração de CI acima, é um
**valor de configuração**: arquivo, caminho da chave, valor, tipo e linha. Não há chave para
ligar: a busca `kind = "value"` os encontra, e a busca sem `kind` responde com eles quando nada
mais casa. Eles não entram no `meada export`.

- **Lockfiles** (`package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml` e
  `packages.lock.json`) e arquivos que não são lidos (um template Helm, que não é YAML até ser
  renderizado; um arquivo acima de 1 MiB; um arquivo de mais de 1.000 documentos) ficam de fora,
  contados pelo `meada status`:
  `arquivos de configuração: 5 lidos, 2 ignorados (lockfiles: 1, ilegíveis: 1); valores: 46, ocultados: 3`.
- **Terraform e OpenTofu**: os arquivos `*.tf`, `*.tofu`, `*.tfvars`, `*.auto.tfvars`,
  `*.tf.json`, `*.tofu.json` e `*.tfvars.json` também são valores de configuração. O caminho da
  chave passa pelo tipo e pelos rótulos do bloco e pelo nome do atributo
  (`resource.aws_sqs_queue.orders.name`), e um bloco repetido com o mesmo tipo e os mesmos rótulos
  é `[n]` (`ingress[0].from_port`). Um literal mantém o tipo; uma expressão (`var.environment`)
  ou um template (`"${var.environment}-orders"`, um heredoc com interpolação) é guardado como foi
  escrito, com o tipo `expression` ou `template`. Um arquivo `.tofu` esconde o arquivo `.tf` de
  mesmo nome na sua pasta (e o `.tofu.json`, o `.tf.json`), e o arquivo escondido não é lido. Um
  arquivo que não é lido, ou que aninha colchetes além de 64 níveis, é ignorado e contado como os
  outros; `.terraform/` é oculto e fica de fora.
- **Segredos**: os valores de `data` e `stringData` de um `Secret` do Kubernetes (um documento
  com `apiVersion` e `kind: Secret`, ou um item assim de uma lista `items`), o `default` de uma
  `variable` do Terraform com `sensitive = true` e todo valor que um arquivo `.tfvars` dá a uma
  variável desse nome (todo valor, quando um arquivo Terraform na pasta dele ou acima não é lido)
  ficam guardados como `redacted`. Valores SSM com tipo SecureString ou não resolvido, em HCL
  e no JSON de Terraform/OpenTofu, também são ocultados, com o caminho da chave, o tipo e a linha. Fora disso nada é filtrado: o índice
  guarda o que o checkout já tem. Ponha no `.meadaignore` os arquivos que não devem entrar no
  índice (`secrets/`, `*.env.json`).

Um ARN AWS reconhecido pela tabela de tipos, uma URL SQS ou `s3://archive/caminho` em configuração vira um lado de uso `exact`, localizado por `<arquivo>#<caminho da chave>`. Um nome simples sob a última chave `queue`, `topic`, `bucket` ou `table` vira, respectivamente, um uso SQS, SNS, S3 ou DynamoDB `inferred`. Por exemplo, `queue: orders` em valores Helm usa `channel::sqs::orders`; `other: orders` não liga nada. O uso não tem direção: liga somente aos provisionadores e conta para órfãos, sem virar produtor, consumidor, escritor ou leitor. `context` e a busca de contratos mostram esses usos, e `trace` segue a ligação de provisionamento até a chave e de volta. Valores ocultados, cortados ou não resolvidos ficam de fora; o atributo que define o nome de um provisionador não usa o próprio recurso. O export inclui os lados em `uses`, sem o texto dos valores, e o console os mostra separados dos lados com direção.

### Nós IaC

Cada `resource`, bloco `data` e chamada `module` dos arquivos `*.tf`, `*.tf.json`, `*.tofu` e `*.tofu.json` é um **nó IaC**,
com endereço, tipo, arquivo, linhas e o módulo raiz que começa a sua avaliação. Uma pasta de
arquivos Terraform é um módulo; uma pasta que nenhuma outra chama como módulo local (um `source`
`./` ou `../` que fica dentro do repositório) é um módulo raiz, rotulado pelo caminho (`.` na
raiz).

O JSON Terraform usa as mesmas regras de avaliação e referência. Strings comuns são templates; uma interpolação sozinha mantém o tipo do resultado, então `${var.items}` pode fornecer uma coleção. Defaults de variáveis e fontes de módulos são literais. Strings estáticas de `depends_on` nomeiam referências. Arrays de blocos mantêm os índices nos caminhos de configuração. As duas extensões JSON aceitam endereços de nós nas localizações `via` de recursos declarados.

- **Endereços**: `<tipo>.<nome>`, `data.<tipo>.<nome>` e `module.<nome>`. Cada chamada de um
  módulo local instancia os blocos dele com o endereço da chamada na frente
  (`module.orders.aws_sqs_queue.this`); um `count` ou um `for_each` sobre valores que se
  resolvem em literais dá um nó por instância, com `[chave]` no fim (`aws_sqs_queue.dlq[0]`,
  `aws_cloudwatch_metric_alarm.depth["orders"]`).
  Um `count` ou `for_each` que não se resolve (abaixo) dá um nó só, sem chave.
- **Avaliação estática**: o nome físico (o argumento `name`) e cada `count` e `for_each` são
  avaliados como o Terraform faria, sem rodá-lo nem acessar a rede: os defaults das variáveis, os
  valores do `terraform.tfvars`, dos `*.auto.tfvars` e das suas formas JSON da raiz (nessa ordem, o
  arquivo seguinte vencendo), os locals, as entradas e saídas de um módulo local por chamada,
  `each`, `count.index`, templates e as funções puras `format`, `formatlist`, `join`, `split`,
  `lower`, `upper`, `title`, `trim`, `trimprefix`, `trimsuffix`, `trimspace`, `chomp`, `replace`
  (uma substring literal), `substr`, `concat`, `coalesce`, `element`, `keys`, `values`, `lookup`,
  `merge`, `length`, `toset`, `tolist`, `tomap`, `tostring`, `tonumber` e `jsonencode`. Data
  sources, atributos de resources, módulos do registro, `terraform.workspace`, `-var`, `-var-file`,
  `TF_VAR_`, variáveis sensíveis e qualquer outra função nunca são avaliados: essa parte do nome
  vira um `*` e o nome um **padrão** (`"orders-${terraform.workspace}"` é `orders-*`), que liga
  nomes literais pelas regras de padrões abaixo. Um nome que passou por uma variável do módulo raiz a lista em
  `overridable:`, já que quem faz o deploy pode passar outro valor. Um arquivo Terraform ou de
  variáveis alterado avalia de novo só os módulos raiz cuja unidade (a raiz e os módulos locais que
  ela chama) o contém.
- **Referências**: uma expressão que nomeia um resource, um bloco data ou um módulo é uma
  referência do nó dela, guardada como uma chamada. Uma que passa por `var.<nome>`, `local.<nome>`
  ou a saída de um módulo (`module.<nome>.<saída>`) é seguida até o que o argumento da chamada,
  o local ou a saída nomeia, e mostra por onde passou (`via var.queue_arn`). Um módulo do
  registro fica como o nó da chamada, sem nada dentro.
- **Consultas**: `search` com `kind = "iac"` acha os nós, `context` explica um e `trace` segue as
  referências dele nos dois sentidos. Os nós ficam fora da busca sem `kind`; só os nós com um
  lado de contrato entram no `meada export`.

### Objetos Kubernetes e overlays do Kustomize

Todo documento YAML ou JSON com `apiVersion`, `kind` e `metadata.name` também é um nó IaC, com
endereço `<Kind>[.<grupo>]/<namespace>/<nome>` (`Deployment.apps/shop/api`,
`Service/shop/api`), sem namespace para um kind de escopo de cluster
(`ClusterRole.rbac.authorization.k8s.io/reader`) ou para um objeto que não define nenhum.

- **Overlays**: uma pasta com `kustomization.yaml` que nenhum outro kustomization nomeia como
  base é um overlay e uma unidade de avaliação. Os `resources`, `bases` e `components` dela
  dentro do repositório são instanciados sob ela, cada nó rotulado pelo overlay (`root
  overlays/dev`), com o `namePrefix`, o `nameSuffix`, o `namespace` e as `images` de cada
  kustomization no caminho aplicados. Um `..` que fica dentro do repositório é seguido
  (`../../base`); uma base remota, acima da raiz, absoluta, ausente, por um symlink, em ciclo
  ou aninhada além de 16 níveis não é seguida, e o `meada status` a conta (`entradas do
  kustomize não seguidas: N`). Manifestos que nenhum kustomization lista são simples: os
  manifestos de uma pasta são uma unidade.
- **Referências**: um Service para as cargas de trabalho cujos labels do pod contêm o selector
  dele (`via selector app=api`), uma carga de trabalho para os ConfigMaps e Secrets de `env`,
  `envFrom` e volumes (`via env DATABASE_URL`) e para o `serviceAccountName` dela, um Ingress
  para os Services das regras (`via backend shop.example.com/api`), tudo num namespace e numa
  unidade, pelo nome como foi escrito. `context` numa carga de trabalho acrescenta `images:`
  com o que cada contêiner roda depois que o overlay aplicou as `images`.
- **Não lidos**: patches, geradores, `commonLabels`, templates Helm e `kind: List`.

### Crossplane

Objetos Crossplane são objetos Kubernetes, lidos como acima, e cada um é reconhecido pelo que é
para o Crossplane, que o `context` mostra em `role:`.

- **Papéis**: um `CompositeResourceDefinition` (XRD) e uma `Composition` (as duas de escopo de
  cluster, sem namespace), um claim ou um recurso composto (um objeto do kind de claim ou
  composto que um XRD do repositório define) e um recurso gerenciado (um objeto com um mapa
  `spec.forProvider`).
- **Recursos criados**: cada recurso que uma composition cria a partir de uma `base` literal (em
  `spec.resources`, ou no `input.resources` de um passo de pipeline que roda a função
  patch-and-transform) é um nó sob a composition, `<composition>/<nome do recurso>`
  (`Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws/instance`), do kind da
  base. O nome dele é a anotação `crossplane.io/external-name` da base ou o `metadata.name`
  como foi escrito; o nome de um recurso gerenciado também é o external name.
- **Referências**: um claim para o XRD do kind composto dele (`via composite
  XPostgreSQLInstance.database.acme.io`), um recurso composto para o XRD dele, um claim ou
  recurso composto para a composition que o `compositionRef` nomeia, um XRD para o
  `defaultCompositionRef` e o `enforcedCompositionRef`, uma composition para o XRD do
  `compositeTypeRef` e para cada recurso que cria (`via resources`, `via pipeline <passo>`), e
  um recurso criado ou gerenciado para a definição do kind dele (um XRD ou um
  `CustomResourceDefinition`) quando há uma. Tudo numa unidade, como toda referência
  Kubernetes: um claim numa pasta diferente da do XRD mantém o papel e não referencia nada.
  Assim o `trace` a partir de um claim desce até o XRD, a composition padrão e os recursos que
  ela cria.
- **Não avaliados**: patches e funções. Um recurso cujo nome ou external name um patch (ou um
  patch set) define, ou que não tem nenhum dos dois, tem o nome `*`, um padrão; um passo de
  pipeline que roda qualquer outra função (Go templating, KCL) não cria nó.

### Compose

Um arquivo Compose (`compose.yaml`, `compose.yml`, `docker-compose.yaml` ou
`docker-compose.yml`) com o arquivo de override (`compose.override.*`, `docker-compose.override.*`)
na mesma pasta é lido como um projeto, enraizado na pasta. Cada serviço é um nó IaC
`services.<nome>` do tipo `service`.

- **Referências**: `depends_on`, `links` e os serviços que compartilham uma rede (`networks:`; um serviço sem nenhuma está em `default`),
  mostradas com `via depends_on`, `via links` ou `via networks.<nome>`. Um nome que nenhum
  serviço tem é descartado.
- **Variáveis**: `${VAR:-padrão}` resolve para o padrão e o `context` o marca `(overridable)`;
  `${VAR}` fica no texto como parte não resolvida, marcada `(unresolved)`.
- **Imagem e build**: o `context` mostra `image:` e `build:` do serviço, guardados para o
  contrato de imagem. Um contexto de build é guardado como caminho dentro do repositório, a
  partir da raiz; caminho absoluto, URL, um que sobe para fora da raiz ou um com parte não
  resolvida não são.
- **Consultas**: como nos nós Terraform, `search` com `kind = "iac"`, `context` e `trace`.

### Imagens e código de workloads

O contrato `image::<registry>/<repositório>` liga um workload ao repositório que constrói sua
imagem. Nomes curtos do Docker Hub são expandidos, letras ficam minúsculas e tag e digest são
removidos. O builder vem de tags do docker/build-push-action, imagens do docker/metadata-action,
Compose com `build` e `image`, artefatos Skaffold, destinos Jib e comandos ko simples com
`KO_DOCKER_REPO` escrito na configuração e `--bare`. Scripts ko com sintaxe de shell não lida
não declaram builder. O comando aceita um `--bare`, no máximo um pacote literal e nenhuma outra
opção; um shell explícito deve ser `bash` ou `sh`. Runners vêm de containers e init containers Kubernetes, serviços Compose,
`image.repository` de values Helm e imagens após overrides Kustomize.

`search` com `kind = "image"` encontra esses contratos. `context` no workload mostra sua imagem
e o repositório builder; `trace` desce do workload ao código e sobe do código aos workloads.
Nomes literais ligam em `exact`; padrões e valores sobrescrevíveis em `inferred`, com o motivo.
Vários candidatos ficam `ambiguous`. Um contexto Compose local e seguro liga o serviço ao seu
código diretamente em `exact`, mesmo sem imagem. Caminhos não atravessam links simbólicos.

`meada status` só lista imagens órfãs quando o escopo contém builder e runner. Com apenas um
papel, `search` e `context` mostram `no builder in the workspace` ou `no runner in the workspace`.
O export conserva `format: 1`, guarda locais dos builders e raízes dos workloads e inclui
`executions` para caminhos diretos; o grafo da console mostra contratos e código dos workloads.

### CloudFormation e SAM

Um template qualificado chamado `compose.yaml` mantém seus nós e referências no índice inicial e nas atualizações; o nome do arquivo não o torna Compose.

Um template YAML (as tags curtas `!Ref`, `!Sub`, `!GetAtt`, `!Join` e as demais são lidas) ou
JSON com um mapa `Resources` que traga `AWSTemplateFormatVersion` ou a transformação SAM
(`AWS::Serverless-2016-10-31`) é lido como uma unidade de avaliação, enraizada no seu caminho.
Cada recurso é um nó IaC endereçado pelo seu id lógico, e seu tipo (`AWS::SQS::Queue`,
`AWS::Serverless::Function`) é o tipo do nó.

- **Referências**: `Ref`, `Fn::GetAtt` (com o atributo no `via`, `via Fn::GetAtt.Arn`), as
  variáveis de `Fn::Sub` e `DependsOn`.
- **Nomes**: o nome físico de fila, tópico, bucket, tabela, função, stream, segredo e mais alguns
  tipos é avaliado dentro do template a partir do padrão de um parâmetro (o `context` o lista em
  `overridable:`), `Ref`, `Fn::Sub`, `Fn::GetAtt` e `Fn::Join`. Um pseudo parâmetro
  (`AWS::Region`), um import e qualquer outra função são partes não resolvidas: o nome vira um
  padrão, mostrado `pattern "assets-prod-*"`.
- **Segredos**: um parâmetro com `NoEcho` tem o padrão guardado como `redacted` e nenhum nome
  passa por ele.
- **Consultas**: como nos nós Terraform, `search` com `kind = "iac"`, `context` e `trace`.
- **Não seguidos**: stacks aninhadas, macros, condições e mapeamentos.

### Serverless Framework

Um `serverless.yml`, `serverless.yaml` ou `serverless.json` com um `service` é lido como uma
unidade de avaliação, enraizada no seu caminho. Cada função é um nó IaC `functions.<nome>` do
tipo `Serverless::Function`.

- **Função**: o nome físico é o `name` da função ou `<service>-<stage>-<função>` (o `stage` é
  `provider.stage` ou `dev`, listado em `overridable:` como `opt:stage`, pois o Serverless nomeia a
  função pelo `--stage` primeiro). O `context` mostra `handler:` e `event:` (`"http GET /orders"`,
  `"sqs Queue.Arn"`, `"schedule rate(5 minutes)"`) e as referências aos recursos que o corpo
  nomeia com `Ref`, `Fn::GetAtt` ou `Fn::Sub`.
- **Recursos**: a seção `resources` é lida como CloudFormation (nós no id lógico, como acima).
- **Variáveis**: `${self:custom.stage}` resolve para o valor naquele caminho; o padrão de
  `${opt:stage, 'dev'}` resolve e o `context` o lista em `overridable:` e marca `(overridable)`.
  `${env:...}`, `${ssm:...}`, `${sls:...}`, `${file(...)}` e as demais fontes são partes não
  resolvidas: o nome vira um padrão (`pattern "*-dead"`).
- **Consultas**: como nos nós Terraform, `search` com `kind = "iac"`, `context` e `trace`.
- **Não lidos**: `serverless.ts`, plugins e valores de outros arquivos.

### Contratos órfãos

`search` sem filtro, com `kind = "contract"`, `channel` ou `resource`, também encontra os lados provisionadores pelo contrato.

`meada status` lista contratos órfãos por tipo e broker (ou tipo de recurso): provisionadores
sem uso e usos sem provisionador. Só há linhas de órfãos quando o repositório ou workspace
consultado contém pelo menos um provisionador e um uso daquele grupo. Um repositório só de
código ou só de IaC não gera essas linhas; `search` e `context` marcam o próprio lado com
`no provisioner in the workspace` ou `no use in the workspace`. As ligações de provisionamento,
inclusive padrões e módulos raiz independentes, determinam se há contraparte. Os avisos
existentes de recursos declarados sem escritor e leitor continuam separados.

### Provisionadores Terraform

Um `resource` `aws_sqs_queue` provisiona `channel::sqs::<nome>` pelo argumento `name`;
um `aws_s3_bucket` provisiona `resource::s3::<nome>` pelo argumento `bucket`. A tabela de tipos
em `src/iac/types.toml`, embutida no binário, também lê nomes em ARNs e URLs. Um `[[resource]]`
com `kind = "s3"` e o mesmo `id` participa desse contrato, mantendo o nível `declared`.

CloudFormation e SAM usam os mesmos contratos para SQS, SNS, EventBridge, Kinesis, S3,
DynamoDB (Table, GlobalTable e SimpleTable), RDS (instância ou cluster), ElastiCache
(cluster ou grupo), SSM e Secrets Manager, quando o nome físico é conhecido. A propriedade
que define esse nome não cria um uso do próprio recurso; segredos contribuem só com a identidade.

Cada produtor, consumidor, escritor e leitor se liga a cada provisionador, identificado pelo
endereço e pelo módulo raiz. Duas raízes como `envs/dev` e `envs/prod` não são ambíguas entre si.
Nomes escritos dão `exact`; nomes que passam por valores sobrescrevíveis dão `inferred`, com
as variáveis e suas origens no motivo. `context` no bloco lista os lados do contrato; no produtor,
lista os provisionadores. `trace` segue do bloco para o código com `down` e do código para a
infraestrutura com `up`. O export mantém `format: 1` e acrescenta `iac_nodes`, `provisioners` e
`provisions`, omitidos quando vazios. Tipos não listados e blocos `data` não provisionam contratos.
Um padrão
com pelo menos três caracteres literais e curingas nos limites de segmentos (`-`, `_`, `.`, `/`)
se liga a um nome literal em `inferred`, ou `ambiguous` quando vários nomes correspondem. Padrões
nunca se ligam a padrões. O grafo da console lista os provisionadores e suas ligações próprias.

### GCP e Cloud Run

Terraform/OpenTofu reconhece `google_pubsub_topic` e `google_pubsub_subscription` como canais
`pubsub`, `google_storage_bucket` como `gcs`, `google_sql_database_instance` e
`google_sql_database` como `cloudsql`, e `google_secret_manager_secret` como
`gcpsecretmanager`. O nome vem de `name`, exceto no segredo, que usa `secret_id`.
Instâncias Cloud SQL usam `<instância>`; bancos usam `<instância>/<banco>`, com a instância
literal ou uma referência resolvida a uma instância conhecida. Um banco não provisiona sua
instância. Projetos ficam fora da identidade.

Um tópico usa `channel::pubsub::<id>`; uma assinatura usa
`channel::pubsub::subscriptions/<id>`, mesmo quando ambos têm o mesmo id. A assinatura liga
consumidores de seu destino aos produtores do tópico indicado por `topic`: `exact` sem
filtro, `inferred` com filtro ou nomes sobrescrevíveis. Uma assinatura sem tópico resolvido
não inventa uma ligação nem um tópico com seu nome.

`gs://<bucket>/objeto`, `projects/<projeto>/topics/<id>`,
`projects/<projeto>/subscriptions/<id>` e `projects/<projeto>/secrets/<id>` (também com
`/versions/<versão>`) são usos de configuração `exact`. Os nomes simples sob as chaves finais
`pubsub_topic`, `pubsub_subscription`, `gcs_bucket`, `cloudsql_instance`, `cloudsql_database`
e `gcpsecretmanager_secret` são `inferred`; `cloudsql_database` exige `<instância>/<banco>`.
As chaves genéricas `topic` e `bucket` conservam seus significados SNS e S3. Identificadores
malformados, valores ocultados ou cortados e uma simples igualdade de texto não criam usos.

`google_cloud_run_service`, `google_cloud_run_v2_service` e `google_cloud_run_v2_job` são
workloads: cada contêiner guarda sua imagem e seu ambiente. Imagens ligam ao repositório que
as publica; um padrão sobrescrevível enfraquece a ligação. Leituras de ambiente só recebem
valores do contêiner que executa seu código e ficam `inferred`. Referências Secret Manager
continuam alvos não resolvidos, sem ler o conteúdo do segredo. Busca, `context`, `trace` nos
dois sentidos, órfãos por escopo e export incluem os novos contratos e runners; entradas de
ambiente e valores de configuração ficam fora do export.

### Uso dos SDKs GCP em cinco grupos de linguagens

Clientes GCP com origem comprovada no construtor criam produtores Pub/Sub, consumidores de assinaturas, escritores/leitores do Cloud Storage e leitores do Secret Manager. JavaScript e TypeScript formam um grupo, com cada linguagem de origem verificada separadamente. O inventário de operações é explícito; outras sobrecargas ficam fora desta lista.

| Grupo | Produtor / consumidor Pub/Sub | Escritor / leitor Cloud Storage | Leitor Secret Manager |
| --- | --- | --- | --- |
| JavaScript + TypeScript | `new PubSub().topic(name).publishMessage(...)` / `.subscription(name).on("message", handler)` | `new Storage().bucket(name).file(object).save(...)` / `.download(...)` | `new SecretManagerServiceClient().accessSecretVersion({name})` |
| Python | `PublisherClient().publish(topic, ...)` / `SubscriberClient().subscribe(subscription, callback=...)` | `storage.Client().bucket(name).blob(object).upload_from_string(...)` / `.download_as_bytes()` | `SecretManagerServiceClient().access_secret_version(request={"name": name})` |
| Java | `Publisher.newBuilder(topic).build().publish(...)` / `Subscriber.newBuilder(subscription, handler).build().startAsync()` | `StorageOptions.newBuilder().build().getService().create(BlobInfo.newBuilder(BlobId.of(bucket, object)).build(), ...)` / `.readAllBytes(bucket, object)` | `SecretManagerServiceClient.create().accessSecretVersion(name)` |
| Go | Pub/Sub v2 `NewClient`: `Publisher(topic).Publish(...)` / `Subscriber(subscription).Receive(ctx, handler)`; v1 `Topic` / `Subscription` | `storage.NewClient`: `Bucket(bucket).Object(object).NewWriter(ctx)` / `.NewReader(ctx)` | `secretmanager.NewClient`: `AccessSecretVersion(ctx, &secretmanagerpb.AccessSecretVersionRequest{Name: name})` |
| C# | `client = await PublisherClient.CreateAsync(topic); client.PublishAsync(...)` / `client = await SubscriberClient.CreateAsync(subscription); client.StartAsync(handler)` | `StorageClient.Create()`: `UploadObject`, `UploadObjectAsync` / `DownloadObject`, `DownloadObjectAsync`, bucket as first argument | `SecretManagerServiceClient.Create()`: `AccessSecretVersion`, `AccessSecretVersionAsync` |

Identidades literais de tópicos, assinaturas, buckets e segredos usam a mesma normalização do Terraform e da configuração. Uma assinatura é `channel::pubsub::subscriptions/<id>` e nunca um tópico adivinhado. Nomes lidos do ambiente continuam não resolvidos sem uma carga que execute o código; valores nesse escopo resolvem em `inferred`, preservando o subtipo da operação. Construtores ocultos, tipos injetados sem construção, fábricas encontradas só pelo nome, clientes reatribuídos e alternativas de retorno opacas não fornecem um lado de SDK.

Campos de requisição alterados antes do uso deixam o nome não resolvido; leituras anteriores à alteração e objetos independentes preservam sua identidade. Helpers tipados precisam corresponder ao recurso usado. Em C#, primeiro aguarde `CreateAsync`, depois invoque a operação no cliente retornado.

### Canais, dados, segredos e assinaturas AWS

A tabela Terraform/OpenTofu também reconhece os tipos abaixo. ARNs são reduzidos ao nome
físico, incluindo caminhos de parâmetros e segredos; o sufixo de seis caracteres do ARN do
Secrets Manager é removido. As mesmas regras de provisionamento, `context`, `trace` e export
valem para todos esses contratos.

| Tipo Terraform | Nome físico | Contrato |
| --- | --- | --- |
| `aws_sns_topic` | `name` | `channel::sns` |
| `aws_cloudwatch_event_bus` | `name` | `channel::eventbridge` |
| `aws_kinesis_stream` | `name` | `channel::kinesis` |
| `aws_dynamodb_table` | `name` | `resource::dynamodb` |
| `aws_db_instance` | `identifier` | `resource::rds` |
| `aws_rds_cluster` | `cluster_identifier` | `resource::rds` |
| `aws_elasticache_cluster` | `cluster_id` | `resource::elasticache` |
| `aws_elasticache_replication_group` | `replication_group_id` | `resource::elasticache` |
| `aws_ssm_parameter` | `name`, preservando `/` inicial | `resource::ssm` |
| `aws_secretsmanager_secret` | `name` | `resource::secretsmanager` |

Um parâmetro SSM `SecureString` tem `value` armazenado como `redacted` em HCL e no JSON de Terraform/OpenTofu. Quando `type` é uma
expressão que o leitor de valores não resolve, o valor também é ocultado por precaução.

`aws_sns_topic_subscription` com `protocol = "sqs"` liga consumidores da fila aos produtores
do tópico. `aws_cloudwatch_event_rule` e `aws_cloudwatch_event_target` fazem o mesmo do bus para
a fila; sem `event_bus_name`, usam `default`. Sem filtro, o nível é `exact`; com `filter_policy`
ou `event_pattern`, é `inferred`. Nomes sobrescrevíveis também dão `inferred`, com as variáveis
no motivo. O motivo identifica a assinatura ou o target e a regra. Endpoints não resolvidos,
protocolos diferentes e targets sem uma regra única no mesmo bus não criam ligações.
Regras somente com agendamento não roteiam eventos publicados no bus.

`context` lista produtores e consumidores através da assinatura. `trace` sobe do consumidor
até o produtor e desce do provisionador até os consumidores; um canal de origem `sns`,
`eventbridge` ou `kinesis` também pode iniciar o trace. Os resumos entram no array opcional `subscriptions`, com origem, destino, nó e raiz.
As ligações entram em `links`, mantendo
as ligações de provisionamento separadas em `provisions`. `meada status` lista contratos
provisionados sem uso e usos sem provisionador, por kind e broker, somente quando o workspace
contém ao menos um provisionador e um uso daquele grupo. Os outros formatos IaC continuam
como nós e referências; esta tabela de contratos cobre arquivos `.tf`, `.tf.json`, `.tofu` e `.tofu.json`.

### Uso dos SDKs AWS em Java, Go e C#

Os SDKs Java 2.x, Go v2 e .NET reconhecem SQS, SNS, EventBridge e Kinesis como canais,
e S3, DynamoDB, SSM Parameter Store e Secrets Manager como recursos. O cliente precisa nascer
num construtor do SDK: `create()` ou `builder().build()` em Java, `New` ou `NewFromConfig`
em Go, e `new Amazon...Client(...)` em C#. Aliases e fábricas seguem apenas saltos `exact`;
um tipo injetado sozinho, um nome oculto ou um salto por nome não comprova o cliente. O SQS
mantém as formas anteriores: um campo, parâmetro ou local tipado declarado com o tipo de
cliente SQS do SDK (`SqsClient`, `SqsAsyncClient`, `*sqs.Client`, `IAmazonSQS`,
`AmazonSQSClient`) é um cliente, inclusive injetado; um tipo do repositório com esse nome não é.

Nomes literais e ARNs identificam o contrato; uma leitura do ambiente mantém o lado sem nome
resolvido e registra a variável, inclusive em produtores. Lotes preservam todos os nomes.
Leitores pertencem à função que os contém, escritores à chamada da operação, e os vínculos
SDK são `exact`; declarações de recursos continuam `declared`. Lados sem nome resolvido não
se vinculam, e `GetParametersByPath` não identifica um parâmetro físico. A tabela de
[capacidades](site/pt/capabilities.md) lista as operações e diferenças entre os SDKs.


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
| `via` | lista de textos | não (vazia) | Símbolos, endereços IaC ou `<arquivo>#<chave>` com seletores `[chave=valor]` |

```toml
[[resource]]
kind = "postgres"
id = "orders"
role = "write"
via = ["src/orders.ts::cancelOrder"]
```

Cada escritor liga a cada leitor do mesmo recurso em outro repositório, no nível `declared`. Sem
`via`, ou se nenhum `via` corresponde a um símbolo, nó IaC ou chave, o lado é o repositório inteiro. O `meada
status` mostra, sob cada repositório, os recursos sem ligação e os `via` que não correspondem a
nenhum símbolo, nó IaC ou chave. A identidade do recurso no `search` e no `context` é `resource::postgres::orders`.

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
  não usam rede. Duas coisas fogem a isso e não são decisão: o `npx @meada/meada` baixa o binário, e o
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

As mensagens seguem o `LANG` (português, inglês ou espanhol); as do MCP são sempre em inglês. Toda
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
recursos sem ligação, os `via` que não correspondem a nenhum símbolo, nó IaC ou chave, as decisões obsoletas ou à
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
| `meada snapshot` | `snapshot` | O store de snapshots: `publish`, `pull`, `lock`, `pointer` e `fence` |
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
- **Com o formato do export e o commit indexado**: `format` (1 hoje) só sobe quando quem lê o formato
  anterior o leria errado, então quem lê deve ignorar os campos que não conhece: um campo opcional
  novo é uma mudança menor. `commit` e `dirty` dizem de que estado do repositório o índice foi
  lido (veja [Uso](https://claudiomascaro.github.io/meada/cli/)).

Receitas com `jq` (um registro por símbolo, duplicados, fan-in entre repositórios) estão no
[README](../README.md#alimentar-uma-base-de-conhecimento).

### Campos do nível superior

As listas vazias são omitidas, menos `files`, `symbols`, `calls`, `providers`, `consumers` e
`links`, que sempre aparecem (como `repository`, que é um texto).

| Campo | Conteúdo |
| --- | --- |
| `format` | O formato do export, hoje 1 |
| `repository` | A identidade do repositório (`example.com/acme/web`) |
| `commit`, `dirty` | O commit indexado e se o checkout diferia dele; omitidos juntos fora do git, antes do primeiro commit e quando nada confiável se sabe |
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
- Um nome que o Meada não consegue calcular não gera elo por si só. Uma URL montada em tempo de
  execução é um consumidor não resolvido, que você decide com `meada decide` (seção 5). Um nome de
  canal lido do ambiente é um lado não resolvido, um limite conhecido que nenhuma decisão resolve
  (seção 8.4).

### 8.1 Rotas HTTP

Uma rota liga quem a **serve** (o provedor, o código que a trata) a quem a **chama** (o
consumidor, uma chamada de cliente HTTP). Em todas as linguagens, o Meada normaliza o caminho:
`/users/:id`, `/users/{id}` e `/users/<int:id>` viram o mesmo `/users/{}`, e o método fica
em maiúsculas. Maiúsculas e minúsculas do caminho e versões de API são mantidas.

Rotas podem vir da infraestrutura: API Gateway HTTP e REST em Terraform/OpenTofu, eventos
SAM `Api` e `HttpApi`, e eventos Serverless `http` e `httpApi`. Por exemplo, um evento SAM
com `Method: GET` e `Path: /orders/{id}` fornece `http::GET::/orders/{}`. Declare o host público
em `[serve] hosts`, como para uma rota em código. O destino é o símbolo do handler no mesmo
repositório quando ele resolve com certeza, inclusive por um export com alias; caso contrário,
é o repositório inteiro. Um arquivo ausente ou ambíguo, um ZIP, um caminho remoto, absoluto,
com `..` ou através de symlink nunca inventa um handler.

O método público da API REST vem de `http_method`, não do POST usado para invocar Lambda.
A integração precisa identificar uma única Lambda na mesma unidade e instância de avaliação;
referências incidentais, APIs incompatíveis e alvos desconhecidos não fornecem rotas. O modo
quick create exige `route_key` explícita com método e caminho. `ANY` (ou `method: '*'` no
Serverless) serve qualquer método nesse caminho; `$default` ou um catch-all sem caminho
finito não cria provider. Valores substituíveis mantêm a evidência `inferred`. A avaliação e
a validação dos caminhos acontecem durante a indexação, sem rede; consultas usam os fatos guardados.

Como a ligação é escolhida, conforme o que a URL mostra:

1. **Host.** A URL tem um host (`https://api.example.com/orders`) que algum repositório lista em
   `[serve] hosts`. O elo é `exact`. Se nenhum repositório serve esse host, não há elo.
2. **Variável de ambiente.** A URL começa por uma leitura de ambiente (`API_URL`) que um par
   declara em `base_url_env`. O elo é `inferred`; o valor no ambiente do processo nunca é lido.
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

Uma URL base literal da configuração ou de uma entrada do workload também pode apontar para
um Service Kubernetes: `http://orders:8080`, `orders.shop`, `orders.shop.svc` ou
`orders.shop.svc.cluster.local`, sempre com ou sem porta. O Meada segue o selector, o workload
selecionado e sua imagem até o repositório que serve a rota. Nomes Compose seguem o serviço
e sua imagem ou seu contexto de build local. O vínculo é `inferred`, com o Service no motivo;
para uma variável de ambiente, o motivo também identifica o workload e a variável. A leitura
usa somente entradas indexadas do workload que executa o código, respeitando container,
namespace, raiz de avaliação e caminho local. Candidatos distintos ficam `ambiguous`.

Um Ingress com host `shop.example.com`, prefixo `/store` e backend `orders` permite que
`https://shop.example.com/store/orders` alcance a rota `/orders` desse Service. O prefixo
mais específico prevalece, como um prefixo de gateway. Anotações de rewrite, backends não
resolvidos e `pathType` diferente de `Prefix` não criam vínculos; formas antigas sem
`pathType` são aceitas. Hosts com curinga e caminhos específicos de controller não são
interpretados. A resolução usa os fatos indexados, sem consultar um cluster.

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

Com `via`, o lado é o símbolo, nó IaC ou chave; sem `via`, ou se nenhum local corresponde, o lado é o
repositório inteiro e aparece como `(repository-level)`: um `trace` mostra o elo, mas não entra
nesse lado. O `meada status` lista, sob cada repositório, os recursos sem ligação e os `via` sem
local correspondente. Como o Meada não interpreta `kind`, nada garante que `postgres` e `postgresql` sejam o
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
| JMS | `jms` | só em Java e Kotlin (Spring JMS) |
| Azure Service Bus | `servicebus` | JS/TS, Python, Java, Go e C# |

Os clientes de cada linguagem estão na seção 9. O Swift não tem leitura de canais no 1.0.

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
| Maven | `pom.xml` com coordenadas literais, ou projeto Gradle com `group` e nome literais | `package::maven::<grupo>:<artefato>` | Cada módulo é um artefato. Lê também `settings.gradle` e `settings.gradle.kts`. Faixas não são conferidas |
| NuGet | `.csproj` no estilo SDK | `package::nuget::<id>` | O id é o `PackageId`, senão o `AssemblyName`, senão o nome do arquivo. Faixas não são conferidas |
| SwiftPM | `Package.swift` | `package::swiftpm::<nome>` | O nome é o literal de `Package(name:)`. Faixas não são conferidas |

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
- **Maven e Gradle.** Uma dependência, também na notação de mapa do Gradle (`group: 'g', name:
  'a'`), liga ao artefato do workspace em `exact`, com `range unchecked`. Um grupo ou artefato
  escrito com propriedade ou variável liga quando o build declara o valor como literal, ou quando
  um `gradle.properties` (da raiz, do diretório do settings ou do módulo) o define; um acessor de
  version catalog (`libs.x`) é lido do `gradle/libs.versions.toml` ou do catálogo que o settings
  nomeia. Senão o elo é `ambiguous`. Um `import` de uma classe do artefato chega ao código do provedor pela
  declaração `package` dos arquivos, e não por um caminho (em Kotlin, também o de uma função de
  topo ou de extensão): `exact` quando um único provedor declara o pacote e o tipo, `ambiguous`
  quando vários declaram ou restam sobrecargas com o mesmo número de argumentos, `inferred`
  quando um ancestral da classe vem de fora do repositório do provedor. O motivo termina em
  `; import package`.
- **NuGet.** Um `PackageReference` que nomeia um pacote do workspace liga em `exact`, com
  `Version` como faixa, e um `ProjectReference` para um projeto de outro repositório do workspace
  liga ao pacote desse projeto. Um `using` de um namespace de um projeto de que o arquivo depende
  chega aos tipos dele: `exact` quando um provedor declara o namespace e o tipo, `ambiguous`
  quando vários declaram, `inferred` quando um ancestral vem de fora do repositório do provedor;
  o motivo termina em `; import namespace`. O Meada não consulta feed nem lê
  `Directory.Packages.props`.
- **SwiftPM.** Um `.package(url:)` liga ao repositório do workspace de mesma identidade de remoto,
  e um `.package(path:)` ao repositório nesse caminho. Só valores literais contam: uma URL, um
  caminho ou um nome calculado deixam o elo `ambiguous`. Um `import Modulo` de um módulo de um
  pacote de que o `Package.swift` depende chega às declarações `public` e `open`, escolhidas pelo
  nome e pelos rótulos dos argumentos; o motivo termina em `; import module`. Módulos da
  plataforma (`Foundation`, `SwiftUI`) nunca viram elo, e o `Package.resolved` não é lido.

Quando dois provedores servem o mesmo nome, o elo é `ambiguous`, um por candidato, com o motivo
iniciado por `<n> providers fit`.

## 9. Linguagens

O Meada 1.0 lê **TypeScript e JavaScript, Python, Go, Rust, Java, Kotlin, C# e Swift**, nove
linguagens. Cada seção abaixo lista os arquivos lidos, os frameworks e clientes HTTP, as leituras
de ambiente, os manifestos, os clientes de canais e os limites conhecidos. Uma biblioteca fora
destas listas não gera ligação.

Regras que valem para todas as linguagens:

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

A promessa de estabilidade do 1.0 vale para as nove linguagens: ela congela a superfície
(ferramentas, CLI, configuração e export), e resolver melhor o que uma linguagem acha é uma
correção, não uma quebra.

### 9.1 TypeScript e JavaScript

**Arquivos.** `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs` e `.cjs`. Um `import` usa os `paths` e o
`baseUrl` do `tsconfig.json` (ou `jsconfig.json`) mais próximo, no diretório do arquivo ou acima,
relativos ao diretório daquele arquivo de configuração. Se ele tem `extends` para outro `tsconfig.json`
ou `jsconfig.json` do repositório (caminho relativo), `paths` e `baseUrl` vêm da cadeia, e o config mais
próximo que declara cada opção vence; um `extends` para pacote ou para fora do repositório não é lido,
e uma cadeia longa demais ou circular vale só pelo config mais próximo. O `node_modules` é ignorado.

**O que vira definição.** Funções, classes e métodos, os `const`, `let` e `var` de uma função ou
classe e o `export default`. No topo de um arquivo, funções atribuídas também contam:
`obj.y = function () {}` define `obj.y`, `X.prototype.y = function () {}` define o método `X.y`,
`exports.y = ...` e `module.exports.y = ...` definem a exportação `y`, e `module.exports =
function () {}` define a exportação padrão. Um `let` ou `var` só é definição se o arquivo nunca
atribui o nome de novo. As funções do objeto literal de um `const` do topo definem `o.m`.

**HTTP.**

URLs montadas por `new URL(x)` ou `new URL(ref, base)` são lidas somente para o `URL` global
sem reatribuição ou o export nomeado `URL` de `url` ou `node:url` por `import` ou
`require` não ocultado. Pacotes desconhecidos, aliases de repositório e outros exports ficam opacos.

`String(u)` respeita as declarações locais de `String` e só repassa a URL quando a chamada usa a conversão global. Atribuições e incrementos que podem substituir essa conversão deixam a URL sem resolver. Uma declaração local preserva a conversão global fora de seu escopo. Funções declaradas em blocos de JavaScript sem modo estrito também ocultam a conversão na função envolvente; módulos, classes e `use strict` preservam o escopo do bloco.

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `fetch` global, e uma chamada a um parâmetro cujo padrão é `fetch` |
| Provedor | Next.js (app router: arquivos de rota) e Express |

No Express, uma rota vale sob cada prefixo em que o roteador é montado (`use`, `Router()`). O
`Router()` de `express-promise-router` (o padrão dos plugins de backend do Backstage) vale como o
`express.Router()`. Um prefixo ou um caminho com uma parte ilegível (um buraco de template literal, um parâmetro, uma
leitura de ambiente) não gera rota, e um roteador montado só sob um prefixo assim fica sem prefixo
algum. Um `:id` literal continua sendo parâmetro. Nenhum cliente além do `fetch` é lido: `axios`,
`got` e `XMLHttpRequest` não geram consumidor.

No Backstage, o roteador que um plugin de backend entrega ao `httpRouter` (`httpRouter.use(r)`,
com o `httpRouter` vindo de `coreServices.httpRouter` nos `deps` do `registerInit`) vale sob
`/api/<pluginId>`, o caminho em que o framework o monta, quando o `createBackendPlugin`, importado
de um pacote `@backstage/`, escreve o `pluginId` como literal ou como uma `const` do mesmo arquivo
ligada a um literal. Um plugin cujo `pluginId` não se lê assim (uma leitura de ambiente, um
import) deixa o roteador na raiz, como antes, e o `meada status` aponta o plugin. Do lado do
consumidor, uma URL que começa por `discovery.getBaseUrl('<id>')` (numa constante ou escrito na
própria URL) começa em `/api/<id>`, e o motivo da ligação nomeia o plugin e essa montagem; uma URL
sem host de uma rota que a montagem de um plugin do mesmo repositório serve também o nomeia.

O plugin `proxy` do Backstage vale pela tabela `proxy.endpoints` de um `app-config*.yaml` do
repositório: uma URL que começa por `/api/proxy/<endpoint>` (também a que
`discovery.getBaseUrl('proxy')` somada a `/<endpoint>` escreve) é uma chamada ao `target` desse
endpoint, e o resto da URL segue o alvo. Um `target` que é exatamente `${VAR}` liga pelo
`base_url_env` do par, e o motivo nomeia a variável e o endpoint; um `target` que é uma URL
`http://` ou `https://` literal vale como o host (com o seu caminho). Qualquer outro `target` (um
texto em volta da variável, um valor padrão) deixa a chamada sem ligação, e o `meada status` aponta
o endpoint e quantas chamadas passam por ele. Um `pathRewrite` que só tira o endpoint (uma regra,
`'^/api/proxy/<endpoint>': ''`, ou `'^/api/proxy/<endpoint>/': '/'` para uma URL com caminho) é o
que o proxy faz de qualquer jeito, então o resto da URL segue para o alvo; qualquer outro
`pathRewrite` (outra substituição, várias regras) é lido como um `target` ilegível: sem ligação, e
o `meada status` aponta. `ignorePath`, `prependPath: false` e um endpoint a que os arquivos
`app-config*.yaml` dão valores diferentes (não se sabe qual deles o deploy carrega) são lidos do
mesmo jeito. Como o proxy do Backstage lê a configuração, os endpoints são os de `proxy.endpoints`
e, quando nenhum arquivo tem essa chave, as chaves de `proxy` que começam por `/` (a forma antiga,
aceita desde antes do Backstage 1.17); com as duas formas, em um arquivo ou em vários, só vale
`endpoints`, e também um `proxy.endpoints`, um `proxy` inteiro ou um `app-config` inteiro atrás
de um `$include` que o Meada não segue (o Backstage lê o que ele trouxer), e então as chaves
antigas não são lidas; um `$include` cujo caminho de chave depois de `#` termina numa chave
ausente não conta, pois o Backstage deixa essa chave de fora. Um endpoint escrito como texto
(`'/<endpoint>': https://...`) é `{ target }` com o
`pathRewrite` padrão, nas duas formas, e as regras acima valem para ele. Um endpoint dentro de um
parcial incluído por `$include` vale como se estivesse no arquivo.

**Ambiente.** `process.env.X`, `process.env["X"]` e `import.meta.env.X`, sozinhos ou passando por
métodos de texto como `trim()`, `replace(...)` e `toLowerCase()`. A URL que começa por uma leitura
assim liga pelo `base_url_env` do par. Arquivos `.env` não são lidos. O mesmo vale para a
leitura de configuração do Backstage, `config.getString('a.b')` ou `config.getOptionalString('a.b')`,
quando algum `app-config*.yaml` do repositório guarda `a.b: ${VAR}` (o `app-config.yaml` primeiro, e
vale o primeiro arquivo que tem a chave): a leitura é a variável `VAR`. Um valor literal ou uma
chave que nenhum arquivo tem deixam a base sem resolver. Um bloco levado para um arquivo parcial
com `{ $include: ./parciais/a.yaml }` (ou `./parciais/a.yaml#a.b`, para só a chave `a.b` dele) e um
valor `{ $env: VAR }` são lidos como se estivessem no próprio arquivo, como o Backstage os aplica:
o caminho parte da pasta do arquivo que inclui, só arquivos `.json`, `.yaml` e `.yml` são lidos, e
um parcial pode incluir outro, a partir da sua própria pasta, e um `app-config` que é um único
`$include` é o arquivo que ele nomeia. Um parcial é entrada do índice:
editar só ele muda a resposta na próxima consulta. Não são seguidos um caminho absoluto, um que
sai do repositório, um link simbólico, um `${` no caminho, outra extensão, um arquivo ou caminho
de chave que não existe, um objeto com outra chave ao lado de `$include`, um ciclo, mais de 8
inclusões de profundidade, mais de 64 inclusões a partir de um `app-config` e mais de 1 MiB de
parciais enxertados nele: as chaves sob esse `$include` ficam sem leitura, e o `meada status`
nomeia cada um com o motivo. `$file` não é lido, e um `app-config` ou parcial com mais de 1 MiB,
ou com mais de um documento YAML (o Backstage o recusa), é ignorado.

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
O campo `f` de uma subclasse de `BaseSettings` (pydantic-settings) do mesmo arquivo é a variável
`<env_prefix><F>` em maiúsculas, ou o `alias` / `validation_alias` quando existe; uma classe que
não é `BaseSettings` não é lida.

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
`strings.TrimSuffix`, `TrimRight` e `TrimSpace`. Um campo de struct com a tag `env:"X"`
(cleanenv, caarlos0/env) ou `envconfig:"X"` (kelseyhightower/envconfig) é a leitura de `X` onde
o campo é lido (`cfg.API.URL`), com structs aninhados, quando o struct é declarado no mesmo
arquivo.

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
  atributos externos, como o `cfg_rt!` do tokio), invocada como `m!` ou `crate::m!`: ele é lido
  como itens do módulo, ou do bloco `impl` ou trait onde a invocação está. Macros de outros crates
  continuam sem leitura.
- Métodos que não estão nos blocos `impl` do tipo, incluindo os gerados por `derive` ou fornecidos
  por traits externos, não geram ligação.
- A análise usa a configuração padrão de cada crate para x86_64 Linux: itens sob um `#[cfg(..)]`
  falso e features fora de `default` ficam de fora; módulos compilados num build de teste entram,
  não só os `#[cfg(test)]`.
- Um roteador axum entregue a uma chamada que o Meada não lê (`route_service`, `fallback_service`,
  um embrulho de fora) não é montado em lugar nenhum.
- `option_env!` e um `env!` que o próprio arquivo define não são leituras.

### 9.5 Java

**Arquivos.** `.java`, resolvidos pelas declarações de `package`. O Meada não precisa de JDK,
Maven nem Gradle e não roda código do repositório.

**O que vira definição.** Classes, interfaces, enums, records, métodos e construtores, inclusive
aninhados. Os nomes levam o pacote e, nos métodos, os tipos escritos dos parâmetros
(`example.Orders.place(String,int)`). As chamadas de uma lambda ou de uma classe anônima pertencem
ao símbolo que as contém. Os tipos públicos e seus membros públicos ou protegidos formam a
superfície exportada. Imports simples, de pacote, estáticos e com `*` resolvem pelas declarações de
`package`. Uma chamada sem receptor, com `this` ou estática é `exact` quando resta um método pela
quantidade de argumentos (`varargs` incluído), e sobrecargas com a mesma quantidade ficam
`ambiguous`. Num campo, parâmetro ou variável local de tipo escrito, a chamada resolve nesse tipo e
nos supertipos de `extends` e `implements`. Um supertipo de fora do repositório tem membros
desconhecidos: a chamada resolve por nome, a menos que ele seja um tipo da plataforma de membros
conhecidos (`Serializable`, `Comparable` e semelhantes).

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | RestTemplate, WebClient, `java.net.http`, OkHttp, OpenFeign, Retrofit |
| Provedor | Spring MVC, JAX-RS |

- **Provedores.** Um método de uma classe `@RestController` ou `@Controller` com `@GetMapping`,
  `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` ou `@RequestMapping(method =
  ...)` é provedor, sob o prefixo do `@RequestMapping` da classe; um `@RequestMapping` sem `method`
  atende todos os métodos (`http::ANY::`). No JAX-RS, é provedor um método com `@GET`, `@POST` e os
  demais numa classe com `@Path`. Constantes `static final` entram no caminho.
  `server.servlet.context-path` não é aplicado.
- **Consumidores.** Os métodos do RestTemplate, o `.uri(...)` de um pedido do WebClient e o
  `build()` de um `HttpRequest.newBuilder()` ou de um `Request.Builder()` do OkHttp (sem método na
  cadeia, vale `GET`). O cliente vale pelo tipo que o código escreve para ele ou pela chamada que o
  cria, e a base de um WebClient vem do `WebClient.create(base)` ou do `baseUrl(base)` de onde ele
  nasce. Um método que monta a URL a partir dos seus parâmetros é um wrapper.
- **Clientes declarativos.** Numa interface `@FeignClient` ou de Retrofit (`@GET("x")` e os
  demais), cada método é um consumidor, no próprio método da interface. A base vem de `url` e
  `path` no Feign (com placeholders `${chave}` da configuração do Spring) e do
  `Retrofit.Builder().baseUrl(base)` que cria a interface.

**Ambiente.** `System.getenv("X")` e `@Value("${chave}")` do Spring num campo ou parâmetro de
construtor, com a chave lida do `application.properties`, `application.yml` ou `application.yaml`
em `src/main/resources` do módulo. Um placeholder `${VAR}` que nenhuma chave preenche é a variável
`VAR`, que o `base_url_env` do par mapeia. Perfis (`application-<perfil>`) e SpEL não são lidos.

**Manifestos.** `pom.xml` e os arquivos de build do Gradle (`build.gradle`, `build.gradle.kts`,
`settings.gradle`, `settings.gradle.kts`, `gradle.properties` e os version catalogs
`*.versions.toml`). Veja Maven e Gradle na seção 8.5.

**Canais.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | Spring Kafka |
| RabbitMQ | Spring AMQP |
| Amazon SQS | AWS SDK v2 |
| NATS | jnats |
| Redis streams | Jedis, Lettuce |
| JMS | Spring JMS |

Fora dos SDKs AWS, o cliente vale pelo tipo que o código escreve para ele, e um listener pela anotação do Spring
(`@KafkaListener`, `@RabbitListener`, `@JmsListener`). O nome do canal se dobra como uma URL, com
os `${chave}` das anotações lidos da configuração do Spring.

**Limites conhecidos.**

- Um tipo do repositório com o nome de uma anotação ou de um cliente da biblioteca a esconde.
- `@RequestLine` do Feign, `@HTTP` do Retrofit e uma anotação herdada de uma interface não são
  lidos.
- Um nome de canal que não se calcula (variável de ambiente, SpEL) é um lado não resolvido.
- Um arquivo cujos nomes envolventes repetidos passam de 4 MiB no total fica sem símbolos nem
  chamadas, e o `meada status` o nomeia.

### 9.6 Kotlin

**Arquivos.** `.kt` e `.kts` (exceto `build.gradle.kts` e `settings.gradle.kts`), resolvidos pelas
declarações de `package`. O Meada não precisa de compilador Kotlin, JDK nem Gradle.

**O que vira definição.** Classes (também `data`, `sealed`, `enum` e `annotation`), interfaces,
objetos, companions, funções de topo, funções de extensão, propriedades com getter e métodos,
inclusive aninhados. Os nomes levam o pacote e, nas funções, os tipos escritos dos parâmetros
(`example.helper(Int)`); uma função de extensão leva o tipo do receptor (`example.Point.norm()`).
As chamadas de uma lambda pertencem ao símbolo que a contém. Declarações que não são `private` nem
`internal` formam a superfície exportada. Imports simples, com `as` e com `*` resolvem pelas
declarações de `package`, e o próprio pacote é visível sem import. Chamar o nome de uma classe a
constrói. Kotlin e Java do mesmo repositório se chamam pelo nome totalmente qualificado. Dentro de
uma lambda, uma chamada sem receptor pode ser de um receptor implícito, e resolve só por nome.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | RestTemplate, WebClient, OkHttp, Retrofit, cliente Ktor |
| Provedor | Spring MVC, Ktor |

- **Spring, OkHttp e Retrofit.** Seguem as regras do Java (seção 9.5), escritas à moda do Kotlin:
  `RestTemplate()` sem `new`, argumentos nomeados e `Api::class.java` como serviço Retrofit.
- **Ktor, provedor.** Uma rota dentro de `routing { }` (`get`, `post`, `put`, `patch`, `delete`,
  `head`, `options` e `route(caminho, HttpMethod.X)`) é provedor sob o prefixo de cada bloco
  `route(prefixo) { }` em volta, também numa função de extensão de `Route` ou `Routing` chamada
  dentro do bloco. O lambda da rota é o handler. Os nomes só valem com o import do Ktor, e o
  roteamento tipado (classes de recurso) não é lido.
- **Ktor, consumidor.** Uma chamada (`get`, `post` e os demais, ou `request(url) { method =
  HttpMethod.X }`; sem método, vale `GET`) num `HttpClient`. Um `defaultRequest { url(base) }` é a
  base das URLs relativas. `url { path(..) }` não é lido.

**Ambiente.** `System.getenv("X")` e `System.getenv()["X"]`, também por `?:`, `!!`, parênteses e
métodos de string como `trim` e `removeSuffix`, e `@Value("\${chave}")` do Spring (o `$` escapado),
com a configuração do Java. A leitura pode estar numa propriedade, num `val` local ou na própria
URL; um `var` não é seguido.

**Manifestos.** Os do Java: veja Maven e Gradle na seção 8.5.

**Canais.** Os mesmos brokers e clientes do Java (seção 9.5), escritos à moda do Kotlin:
argumentos nomeados e arrays (`@KafkaListener(topics = ["a", "b"])`), `const val` de topo ou de um
`companion object` e um cliente recebido pelo construtor.

**Limites conhecidos.**

- Uma chamada num receptor implícito (função de extensão do cliente, blocos `apply` e `with`) não
  é lida nos canais.
- Num bloco `request`, uma atribuição de método que pode não rodar deixa o método desconhecido,
  nunca `GET`.
- Um nome de canal com uma parte que não se avalia (`"orders-$env"`) é um lado não resolvido.

### 9.7 C#

**Arquivos.** `.cs`, agrupados em projetos pelos `.csproj` no estilo SDK (com `Sdk`; um `.csproj`
antigo não é projeto, e os `.sln` não mudam nada) e resolvidos pelas declarações de `namespace`. As
pastas `obj/` e `bin/` ao lado de um `.csproj` ficam fora do índice. O Meada não precisa de .NET
SDK nem de MSBuild.

**O que vira definição.** Classes, records, structs e interfaces, inclusive aninhados e
`partial`, métodos e construtores com os tipos escritos dos parâmetros
(`Acme.Orders.OrderService.Place(string,int)`) e funções locais. Os tipos públicos e seus membros
públicos ou protegidos formam a superfície exportada, e um tipo `internal` só existe para os
arquivos do seu projeto. `using`, `using static`, aliases e `global using` resolvem pelos
namespaces de todos os projetos; os `global using`, os `<Using>` do `.csproj` e os implicit usings
valem nos arquivos do projeto. Uma chamada sem receptor, com `this`, `base`, estática ou por `using
static` é `exact` quando resta um método pela quantidade de argumentos (`params` e valores padrão
incluídos). Uma base de fora do repositório deixa as chamadas aos membros da classe sem `exact`,
salvo a de um tipo de membros conhecidos (`IDisposable`, `IEquatable` e semelhantes). Um atributo
é uma chamada no que ele decora.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `HttpClient`, typed clients, Refit |
| Provedor | ASP.NET Core (controllers e minimal APIs) |

- **Controllers.** Um método de uma classe controller (com `[ApiController]`, ou derivada de
  `ControllerBase` ou `Controller`) com `[HttpGet]`, `[HttpPost]` e os demais é provedor, sob o
  `[Route]` da classe; `[controller]`, `[action]` e `{id:int}` são entendidos. A rota convencional
  (`MapControllerRoute`) não é lida.
- **Minimal APIs.** `MapGet`, `MapPost`, `MapPut`, `MapPatch`, `MapDelete` e `MapMethods` na
  aplicação ou num `MapGroup(prefixo)`, com grupos aninhados; o prefixo segue o grupo por
  variáveis, argumentos, retornos e métodos de extensão. Um grupo que o Meada não lê (parâmetro de
  lambda, campo, variável reatribuída) não gera rota.
- **Consumidores.** `GetAsync`, `PostAsync`, `GetFromJsonAsync`, `SendAsync(new
  HttpRequestMessage(HttpMethod.X, url))` e semelhantes num `HttpClient`, pelo tipo declarado ou
  pela construção, nunca só pelo nome. A URL dobra literais, interpolação, `+` e campos `const` e
  `static readonly`. Um `BaseAddress = new Uri(base)` é a base das URLs relativas, e um typed client
  registrado com `AddHttpClient<T>(c => c.BaseAddress = ...)` dá a base ao `HttpClient` de `T`.
  Cada método de uma interface Refit (`[Get("/x")]` e os demais) é um consumidor, com a base do
  `RestService.For<T>(url)` que a cria.

**Ambiente.** `Environment.GetEnvironmentVariable("X")`, por `??`, `!` e `Trim`, e
`configuration["Secao:Chave"]` num `IConfiguration` ou em `builder.Configuration`, que é a
variável `Secao__Chave`. Uma URL literal no `appsettings.json` do projeto é o padrão dessa
variável. `appsettings.<Ambiente>.json`, `GetValue`, `GetSection`, binding de opções e user secrets
não são lidos.

**Manifestos.** `.csproj` no estilo SDK e `Directory.Build.props`. Veja NuGet na seção 8.5.

**Canais.**

| Broker | Biblioteca |
| --- | --- |
| Kafka | `Confluent.Kafka` |
| RabbitMQ | `RabbitMQ.Client` |
| Amazon SQS | `AWSSDK.SQS` |
| NATS | `NATS.Client` |
| Redis streams | `StackExchange.Redis` |
| Azure Service Bus | `Azure.Messaging.ServiceBus` |

Os clientes AWS exigem origem em construtor, exceto os `IAmazonSQS` e `AmazonSQSClient`
declarados do SQS, como descrito na seção de SDKs AWS. Os demais clientes valem pelo tipo que o
código escreve ou pela construção que os criou, resolvidos pelos `using`s, nunca só pelo nome.

**Limites conhecidos.**

- Um controller que herda de outra classe do repositório, uma classe que é base de outra e uma
  classe `partial` em vários arquivos não geram rota.
- `IHttpClientFactory.CreateClient(name)` não é lido, e uma interface Refit registrada com
  `AddRefitClient` tem base desconhecida.
- MassTransit, `BasicGet`, `SubscribeSync` e o NATS.Net v2 não são lidos nos canais.
- Um nome de canal lido do ambiente ou do `IConfiguration` é um lado não resolvido.

### 9.8 Swift

**Arquivos.** `.swift` (exceto `Package.swift` e `Package@swift-<versão>.swift`), agrupados em
módulos pelos targets do `Package.swift`; os arquivos fora de qualquer target formam um módulo só.
O Meada não precisa de toolchain Swift nem de Xcode.

**O que vira definição.** Classes, structs, enums, actors e protocolos, inclusive aninhados, e
funções, métodos e inicializadores com os rótulos dos argumentos no nome
(`OrderService.place(order:quantity:)`). Os membros de uma extensão são do tipo estendido. As
chamadas de um closure pertencem ao símbolo que o contém. Os nomes resolvem primeiro no módulo do
arquivo e depois nos módulos que ele importa, pelas declarações `public` e `open` (e `internal`
com `@testable`). Uma chamada é `exact` quando resta uma declaração pelos rótulos, e sobrecargas
com os mesmos rótulos ficam `ambiguous`. Um receptor de tipo de fora do repositório
(`session: URLSession`) só alcança as extensões desse tipo que o repositório escreve, e uma chamada
por um receptor de tipo protocolo para no requisito.

**HTTP.**

| Papel | Biblioteca |
| --- | --- |
| Consumidor | `URLSession`, Alamofire, AsyncHTTPClient |
| Provedor | Vapor, Hummingbird |

- **Vapor.** Uma rota (`get`, `post`, `put`, `patch`, `delete` e `on(.METHOD, caminho)`) chamada na
  aplicação ou num `grouped(prefixo)` dela é provedor sob o prefixo de cada `grouped` em volta. Um
  `RouteCollection` registrado com `register(collection:)` serve as rotas do seu `boot(routes:)`.
  Os nomes só valem com o import `Vapor`.
- **Hummingbird.** Uma rota (`get`, `post`, `put`, `patch`, `delete` e `on(caminho, method:
  .get)`) chamada num `Router()` ou num `group(prefixo)` dele é provedor sob o prefixo de cada
  `group` em volta. Os nomes só valem com o import `Hummingbird`.
- **Consumidores.** `data(from:)` e `data(for:)` numa `URLSession`, `AF.request(url, method:)` e
  `execute` de um `HTTPClientRequest(url:)`. O método é GET quando não escrito, e o `httpMethod`
  atribuído a um `URLRequest` vale. A URL dobra `URL(string:)`, `appendingPathComponent`,
  literais, interpolação, `+` e constantes `let`.

**Ambiente.** `ProcessInfo.processInfo.environment["X"]`, por `!`, parênteses, `??` com um literal
de string e métodos de string como `trimmingCharacters` e `lowercased`. A leitura pode estar
guardada num `let`. Um `var` atribuído de novo não é seguido, e o `Environment.get("X")` do Vapor
não é uma leitura.

**Manifestos.** `Package.swift`. Veja SwiftPM na seção 8.5.

**Canais.** O Swift não tem leitura de canais no 1.0.

**Limites conhecidos.**

- `dataTask(with:)` e o `addRoutes` de um controlador do Hummingbird não são lidos.
- Um middleware construído em `grouped(..)` não soma caminho, e um caminho que o Meada não lê não
  gera rota.
- Um módulo que não é target do repositório é externo.
- Todas as dependências do `Package.swift` mais próximo valem para os arquivos abaixo dele, sem
  separar o que cada alvo declara.

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
- **Java:** `public static void main` com um parâmetro `String[]` ou `String...`.
- **Kotlin:** `fun main` de topo.
- **C#:** um `static Main` (qualquer assinatura) e, em um arquivo com instruções de topo, o
  símbolo `Main`.
- **Swift:** um tipo marcado `@main` (pelo seu `main()` quando declara um) e as instruções de
  topo de um `main.swift`, no símbolo `main`.
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

## AWS SDK em código

Em leitores de recursos, `trace` para cima alcança os escritores; para baixo, a partir de um escritor, alcança o leitor e segue o corpo da função. Quando o lado é uma chamada, sua identidade é preservada e a resposta mostra seu local e a função que a contém. Isso também vale para chamadas sem escritor e para a participação em processos.

TypeScript e JavaScript reconhecem comandos v3 enviados com `cliente.send(new Comando(...))`,
com cliente e comando importados do pacote AWS correspondente. Python reconhece métodos de
clientes criados por `boto3.client(...)` ou por uma `Session`. Os nomes podem vir de constantes
e dos argumentos de wrappers chamados exatamente. Um cliente recebido só por correspondência
de nome, ou escondido por um parâmetro, `catch`, desestruturação ou outro binding, não cria lado.

| Serviço | Produz ou escreve | Lê | Argumento do nome |
| --- | --- | --- | --- |
| SNS | `Publish`, `PublishBatch` | | `TopicArn` |
| EventBridge | `PutEvents` | | `Entries[].EventBusName`, `default` quando omitido |
| Kinesis | `PutRecord`, `PutRecords` | | `StreamARN` ou `StreamName` |
| S3 | `PutObject` | `GetObject` | `Bucket` |
| DynamoDB | `PutItem`, `UpdateItem`, `DeleteItem`, `BatchWriteItem`, `TransactWriteItems` | `GetItem`, `Query`, `Scan`, `BatchGetItem`, `TransactGetItems` | `TableName`, chaves de `RequestItems`, ou tabelas de `TransactItems` |
| SSM | | `GetParameter`, `GetParameters`, `GetParametersByPath` | `Name`, `Names`, `Path` |
| Secrets Manager | | `GetSecretValue`, `BatchGetSecretValue` | `SecretId`, `SecretIdList` |

Em JavaScript, cada operação acima tem o sufixo `Command`; em boto3, usa snake case
(`put_object`, `get_secret_value`). SNS, EventBridge e Kinesis são produtores; S3 e DynamoDB
usam os lados escritor e leitor de recursos. Os nomes literais e ARNs compatíveis se ligam à
infraestrutura pelo nome físico, sem conta ou região. Lados de SDK literais se ligam em
`exact`, inclusive a outros lados de SDK do mesmo repositório; um lado declarado em
`meada.toml` conserva `declared`. Valores de infraestrutura substituíveis continuam `inferred`.

Sem um ambiente correspondente do workload, uma leitura como `process.env.BUCKET` ou
`os.getenv("BUCKET")` fica não resolvida, com
`via: "env:BUCKET"` no export e o nome da variável no contexto da função. Um produtor ou
escritor de SDK também tem `via`, opcional no formato 1; para um literal é `sdk:aws`.
Meada não lê o ambiente do processo. A construção isolada de um comando, um publish SNS para
telefone ou endpoint, os clientes de alto nível `boto3.resource` e `DynamoDBDocumentClient`
e operações fora da tabela não são reconhecidos por estas formas.

`GetParametersByPath` consulta uma hierarquia, por isso seu lado fica não resolvido; o caminho não é tratado como nome de um parâmetro. Um `ConditionCheck` de `TransactWriteItems` é leitor, mesmo quando a transação também escreve outras tabelas. Chaves computadas de `RequestItems` preservam o literal ou o nome da variável de ambiente.

### Nomes parcialmente resolvidos

Um canal, recurso ou imagem com partes desconhecidas preserva um padrão como `*-orders`.
`${terraform.workspace}-orders` corresponde a `prod-orders` em `inferred`: são necessários
ao menos três caracteres literais, e cada `*` deve ocupar segmentos inteiros delimitados por
`-`, `_`, `.` ou `/`. Um padrão nunca corresponde a outro padrão. Vários nomes literais
compatíveis ficam `ambiguous`, todos listados, e viram itens abertos. `meada decide` grava
a escolha em `meada.decisions.toml`; o vínculo escolhido fica `decided`. Decisões de
provisionamento pertencem ao lado que contém o padrão e conservam sua raiz de avaliação.

Itens em chaves de configuração usam metadados de arquivo, linha, chave e contrato, inclusive quando o valor é `redacted` ou cortado.

Um builder de imagem com padrão e vários nomes literais de runners também mantém seu próprio item aberto no repositório que faz o build.

## Locais de recursos declarados

O `via` de `[[resource]]` aceita símbolos (`src/orders.ts::cancelOrder`), endereços IaC e
`<arquivo>#<caminho da chave>`. O lado fica no bloco ou na chave, alcançável por `context` e
`trace`, e suas ligações continuam `declared`. Os endereços incluem Terraform/OpenTofu
(`aws_sqs_queue.orders`), Kubernetes/Crossplane (`Deployment.apps/shop/api`), Compose
(`services.api`), CloudFormation/SAM (`template.yaml::Orders`) e Serverless
(`serverless.yml::functions.api`). Use `<arquivo>::<endereço>` para distinguir arquivos. O local identifica o bloco de origem;
o qualificador ` (root ...)` das consultas IaC não é uma forma de `via`.
Um endereço que corresponde a vários blocos de origem precisa do qualificador de arquivo;
sem ele, o `via` é obsoleto.

Caminhos de chave usam pontos, índices (`groups[0].name`) e chaves entre aspas JSON
(`metadata.labels["app.kubernetes.io/name"]`). Em listas, `[chave=valor]` seleciona os itens
pela igualdade do valor completo, por exemplo
`values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support`.
A localização vem de valores indexados e seus caminhos pais.
Valores nulos e contêineres vazios não têm valor indexado; seu `via` é obsoleto.
Um seletor pode encontrar vários itens; cada chave encontrada recebe um lado. Valores
marcados como `redacted` ou cortados não servem para selecionar um item. Expressões e templates
HCL não satisfazem a igualdade de um seletor. Os arquivos seguem as regras do
índice, incluindo `.meadaignore`; nenhum arquivo é aberto por causa de um `via`.

Um `via` sem correspondência é obsoleto e aparece nas linhas existentes de `meada status`.
Sem nenhum `via` válido, o lado volta ao repositório inteiro. Isso permite declarar o vínculo
entre rótulos em valores Helm e regras de roteamento sem renderizar templates.

```toml
[[resource]]
kind = "routing"
id = "team-b"
role = "write"
via = ["values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support"]
```

## Funções e tarefas AWS

Lambda em Terraform/OpenTofu executa `image_uri`; SAM executa `ImageUri`, e tarefas ECS
executam as imagens de cada container em `container_definitions`, inclusive via `jsonencode`.
Esses executores entram nos contratos de imagem, no contexto, nos rastros e nos órfãos.
O ambiente acompanha o container e a raiz de avaliação, sem alcançar código de outra imagem.

Um `filename` local de Lambda ou `CodeUri` de SAM liga diretamente o código do mesmo
repositório. Uma referência exata a `data.archive_file.<nome>.output_path` pode seguir somente
seu `source_dir` ou `source_file` local explícito. Arquivos ZIP ficam como locais opacos;
seu conteúdo não é extraído. Um handler Serverless como `worker/send.dispatch` seleciona
um único arquivo local existente (`.ts`, `.js`, `.mjs`, `.cjs` ou `.py`); extensões ambíguas
ficam sem resolver. Caminhos novos devem ser relativos, sem nenhum componente `..` e sem
atravessar links simbólicos. S3, URLs e código remoto não viram caminhos locais.

SAM combina `Globals.Function.Environment.Variables` com o ambiente da função; Serverless
combina `provider.environment` com `functions.<nome>.environment`. A função prevalece,
mesmo quando o valor que substitui o padrão não pode ser lido. Terraform Lambda e ECS também
fornecem seus ambientes. Referências conhecidas a recursos podem carregar a identidade do
contrato sem inventar um ARN ou URL do deploy. Segredos, referências dinâmicas e valores
externos continuam sem conteúdo. A leitura em código resolve em `inferred`, com workload e
variável no motivo; `context` e `trace` mostram tanto a execução por imagem como o caminho direto.

Referências como `aws_sqs_queue.orders.url` e `aws_s3_bucket.assets.bucket` identificam o
contrato sem fornecer o URL do deploy; atributos de conteúdo como `value` continuam sem resolver.

## Ambiente dos workloads

Uma leitura de ambiente que nomeia uma fila, tópico ou recurso, como `process.env.QUEUE_URL`,
é resolvida somente pelas entradas do workload que executa aquele código. O vínculo de
execução vem da imagem publicada pelo repositório ou de um caminho local verificado do workload.
Containers diferentes mantêm seus próprios ambientes. A resolução fica em `inferred`, com
o workload e a variável no motivo.

O Kubernetes fornece valores literais, referências a chaves de ConfigMap ou Secret e `envFrom`;
o Compose fornece entradas em mapa ou lista e defaults como `${QUEUE_URL:-orders}`.
`context` no workload mostra os nomes, os destinos e o valor disponível. Um Secret continua
`redacted`, e uma referência ausente ou um valor não resolvido não cria vínculo. Nomes só
coincidem dentro do escopo do workload: os valores vêm do IaC e da configuração dos
repositórios, nunca do ambiente de um processo.

## Alertas e rotas do Alertmanager

O contrato `alert::<chave>::<valor>` liga os rótulos de regras Prometheus e Loki
(inclusive `PrometheusRule`) e Grafana aos matchers de rotas do Alertmanager, em arquivos
de configuração e objetos `AlertmanagerConfig`. Igualdade gera um link `exact`; uma
expressão regular gera `inferred`. As condições de uma rota são verificadas em conjunto.

`search` com kind `alert` encontra o contrato; `context` na rota mostra seu receiver,
e `trace` percorre da regra à rota e de volta. `meada export` inclui emissores, matchers
e links. `meada status` só lista órfãos quando o escopo visível tem os dois lados;
sem isso, `search` e `context` indicam o lado ausente. Valores Helm que ainda dependem
de templates não são emissores: Meada não renderiza templates.

```json
{"tool":"search","arguments":{"query":"alert::support::team-b","kind":"alert"}}
{"tool":"context","arguments":{"symbol":"alertmanager.yaml::route.routes[0]"}}
{"tool":"trace","arguments":{"from":"rules.yaml::groups[0].rules[0]","direction":"down"}}
```

Regras Grafana também são lidas de exports JSON e de blocos `rule` de `grafana_rule_group` em Terraform/OpenTofu, com rótulos literais.

Uma rota raiz sem matchers não cria um lado de alerta: não há chave e valor para o contrato. O grafo descreve os matchers escritos, sem calcular notificações finais, inibições ou horários.

Prometheus e Loki também emitem o `alertname` literal do nome da regra, sobrescrevendo um rótulo com esse nome. Rótulos que possam vir da expressão permanecem desconhecidos, não vazios. Arquivos sob `templates/` não são renderizados nem tratados como emissores de alertas.

### OpsGenie e PagerDuty em Terraform

Tags literais `chave:valor` de `datadog_monitor` são emissores. Condições de tags em
`opsgenie_team_routing_rule` e condições PCL de
orquestrações PagerDuty, são matchers. Igualdade é `exact`; regex, prefixo e
contenção são `inferred`. As condições e a ordem das regras são avaliadas juntas;
valores desconhecidos ou operações não lidas não escolhem um destino por palpite.

Tags do Datadog chegam ao OpsGenie. Rótulos de regras só chegam a tags por um receiver
OpsGenie selecionado no Alertmanager que configure `tags` explicitamente. Templates
simples de `.CommonLabels.<chave>` ou `.GroupLabels.<chave>` usam rótulos fixados pelo
`group_by` efetivo. Um receiver PagerDuty v2 pode encaminhar esses rótulos por `details`,
que vira `event.custom_details.<chave>`; não há encaminhamento implícito de tags ao
PagerDuty. Templates não lidos e encaminhamentos ausentes não geram links.

`context` mostra o escopo escrito da regra, e `trace` mantém o caminho até o bloco de
roteamento. Um matcher compatível não comprova entrega nem uma notificação final a
uma equipe. No exemplo público #638, a regra testa `service:app`: o monitor também
emite `support:team-b`, mas não existe ali um matcher para `support`. O YAML de
monitor não padronizado e os valores Helm originais continuam apenas configuração.

## Infraestrutura Azure

Terraform e OpenTofu provisionam canais Service Bus (filas, tópicos e assinaturas) e Event
Hubs, além de recursos Storage Account (`azurestorage`), container Blob (`azureblob`) e
segredo Key Vault (`keyvault`). Uma assinatura se chama
`channel::servicebus::subscription:orders/audit`, distinta da fila legal
`orders/subscriptions/audit`. Regras da assinatura enfraquecem a entrega para `inferred`;
sem regra, ela é `exact`. Processors C# existentes mantêm o vínculo pelo tópico.

Um ID ARM completo ou URL reconhecida de blob/segredo é uso `exact`. Chaves finais
`servicebus_queue`, `servicebus_topic`, `servicebus_subscription`, `eventhubs_hub`,
`azurestorage_account`, `azureblob_container` e `keyvault_secret` são usos `inferred`;
a assinatura exige valor `tópico/assinatura`. `queue`, `topic`, `bucket` e `table` mantêm
os significados AWS. Conta, namespace, vault, versão e objeto não entram na identidade,
mas o identificador completo é validado antes de removê-los.

Functions Linux leem `site_config.application_stack.docker` com `registry_url`,
`image_name` e `image_tag`. Functions Linux e Windows leem `zip_deploy_file` e
`app_settings`; só um arquivo local seguro fornece execução direta, sem abrir o ZIP.
Um caminho ausente, remoto, com `..` ou symlink não fornece código. App settings resolvem
leituras somente no código executado pela imagem ou caminho correspondente, com nível
`inferred`. Referências a segredos e valores desconhecidos ficam sem resolução. Consulte
[as capacidades](site/pt/capabilities.md#azure-contratos-e-functions) para os tipos e
limites; `context`, `trace`, export e órfãos seguem as mesmas regras de provisionamento.

### Uso dos SDKs Azure nos cinco grupos de linguagens

Construtores conhecidos dos SDKs Azure criam os lados das operações abaixo. JavaScript e TypeScript formam um grupo, com verificações separadas. Java usa os builders `com.azure` listados; JavaScript usa os construtores de `@azure/service-bus`, `@azure/event-hubs`, `@azure/storage-blob` e `@azure/keyvault-secrets`. Python usa `azure.servicebus`, `azure.eventhub`, `azure.storage.blob` e `azure.keyvault.secrets`, incluindo as versões `aio` e as fábricas `from_connection_string` dos três primeiros serviços. Go usa os pacotes correspondentes de `github.com/Azure/azure-sdk-for-go/sdk`: Service Bus e Event Hubs também aceitam fábricas `FromConnectionString`; Blob aceita `NewClient`, `NewClientWithNoCredential`, `NewClientWithSharedKeyCredential` e `NewClientFromConnectionString`. .NET usa `Azure.Messaging.ServiceBus`, `Azure.Messaging.EventHubs.Producer` / `.Consumer`, `Azure.Storage.Blobs` e `Azure.Security.KeyVault.Secrets`. Blob em JS também aceita `BlobServiceClient.fromConnectionString`.

| Grupo | Service Bus produtor / consumidor | Event Hubs produtor / consumidor | Blob escritor / leitor | Key Vault leitor |
| --- | --- | --- | --- | --- |
| JavaScript + TypeScript | `createSender(name).sendMessages` / `createReceiver(queue)` or `(topic, subscription)`, then `receiveMessages` or `subscribe` | `EventHubProducerClient(..., hub).sendBatch` / `EventHubConsumerClient(group, ..., hub).subscribe` | service → `getContainerClient(container)` → `getBlockBlobClient(object)`: `upload`, `uploadData`, `uploadFile`, `uploadStream`; blob `download`, `downloadToBuffer`, `downloadToFile` | `SecretClient.getSecret(name)` |
| Python | `get_queue_sender` / `get_topic_sender` → `send_messages`; `get_queue_receiver` / `get_subscription_receiver` → `receive_messages` | `eventhub_name` explícito: producer `send_batch` / consumer `receive` | service `get_blob_client(container, blob)` or `get_container_client(container).get_blob_client(blob)`: `upload_blob` / `download_blob` | `SecretClient.get_secret(name)` |
| Java | `ServiceBusClientBuilder.sender().queueName` or `topicName` → `buildClient`: `sendMessage`, `sendMessages`; receiver `queueName` or `topicName` + `subscriptionName` → `buildClient().receiveMessages` | builder `connectionString(cs, hub)` or `credential(namespace, hub, credential)`: `buildProducerClient().send` / `consumerGroup(...).buildConsumerClient().receiveFromPartition` | `BlobServiceClientBuilder...buildClient().getBlobContainerClient(container).getBlobClient(object)`: `upload`, `uploadFromFile` / `downloadContent`, `downloadStream`, `downloadToFile` | `SecretClientBuilder...buildClient().getSecret(name)` |
| Go | `NewSender` → `SendMessage`, `SendMessageBatch`; `NewReceiverForQueue` / `NewReceiverForSubscription` → `ReceiveMessages` | `NewProducerClient` → `SendEventDataBatch`; `NewConsumerClient` → `NewPartitionClient` → `ReceiveEvents` | `UploadBuffer`, `UploadFile`, `UploadStream` / `DownloadBuffer`, `DownloadFile`, `DownloadStream`, contêiner após `ctx` | `GetSecret(ctx, name, version, ...)` |
| C# | envios, agendamentos e processadores existentes mantidos; novos `CreateReceiver(queue)` / `(topic, subscription)` → `ReceiveMessageAsync`, `ReceiveMessagesAsync` | hub explícito: producer `SendAsync` / consumer `ReadEventsAsync` | service → `GetBlobContainerClient(container)` → `GetBlobClient(object)`: `Upload`, `UploadAsync` / `DownloadTo`, `DownloadToAsync`; container `UploadBlob`, `UploadBlobAsync` | `SecretClient.GetSecret`, `GetSecretAsync` |

Os nomes identificam filas/tópicos, hubs, contêineres de blobs e segredos. Grupo, partição, objeto e versão do segredo não alteram o contrato. IDs ARM e URLs de Blob/Key Vault suportados seguem a mesma normalização da infraestrutura e configuração. Uma nova assinatura de tópico é `channel::servicebus::subscription:<topic>/<subscription>`; uma fila com `/subscriptions/` continua distinta. Os processadores C# existentes mantêm a identidade do tópico.

Um nome do ambiente continua não resolvido, com `env:NAME`, sem uma carga armazenada que execute o código. Tópico e assinatura só se combinam na mesma carga, raiz e contêiner; valores desconhecidos ou referências tipadas incompatíveis continuam não resolvidos. Links resolvidos pelo ambiente são `inferred`. Chamadas exatas de fábricas mantêm os nomes próprios dos receptores. Construtores ocultos, clientes apenas tipados, saltos por coincidência de nome, receptores reatribuídos e retornos opacos não criam novos lados SDK.

O inventário é limitado. Strings de conexão e credenciais não são analisadas para obter nomes; Event Hubs exige um argumento explícito de hub. Construtores diretos de URL de Blob/Container, processadores de sessão, outras famílias de polling/streaming, operações de listagem/exclusão/metadados e outros SDKs ficam fora da lista. Apenas criar um cliente ou handle não dá direção.

As formas de fila `createReceiver(queue, options)` em JS/TS e `CreateReceiver(queue, options)` em C# mantêm a identidade da fila quando as opções são comprovadas: um objeto JS/TS ou uma construção conhecida de `ServiceBusReceiverOptions`. JS/TS também aceita `undefined` explícito sem ocultação local; C# aceita `options: null`. Reatribuições e segundos argumentos opacos continuam não resolvidos. Um segundo argumento string continua nomeando uma assinatura, com opções opcionais no terceiro argumento.

### Recursos personalizados e módulos de infraestrutura

A infraestrutura definida por Strimzi, Config Connector, ACK ou pelas famílias Crossplane listadas pode aparecer como provisionador do mesmo contrato usado pelo código. Por exemplo, `KafkaTopic` com `metadata.name: topic-object` e `spec.topicName: shipments` aparece em `search` com `kind = "iac"` pelo objeto e por `shipments`; `context` mostra `channel::kafka::shipments`. `trace` desce do nó até seus usos e sobe do uso até a infraestrutura. Para um módulo privado, declare `[[iac.module]]` no `meada.toml`; para seu próprio CRD, use `[[iac.custom_resource]]`. Consulte o inventário exato e os limites em [capacidades](site/pt/capabilities.md#recursos-personalizados-e-módulos-de-infraestrutura) e a [configuração](site/pt/configuration.md#mapeamentos-privados-de-iac). Nomes incompletos continuam pesquisáveis, sem receber um provisionador inventado.
