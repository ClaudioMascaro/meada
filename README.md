# Meada

**Português (Brasil)** · [English](README.en.md) · [Español](README.es.md)

Meada lê os seus repositórios, monta o mapa de quem chama quem e costura um repositório no
outro pelos contratos que eles compartilham: rotas HTTP, filas, pacotes, recursos compartilhados.
Agentes de código, e gente também, consultam esse mapa por MCP. Nenhuma etapa da indexação usa
LLM.

## Instalação

Escolha uma das cinco formas.

- **Cargo**: `cargo install meada`.
- **Binário pronto**: baixe o arquivo do seu sistema nas
  [releases do GitHub](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS e
  Windows, em x86_64 e arm64) e ponha `meada` no `PATH`.
- **Instalador** (Linux e macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.
- **npm**: `npx meada <comando>` baixa o binário pronto da sua plataforma e o executa, sem
  instalar nada antes.
- **Contêiner**: `ghcr.io/claudiomascaro/meada:<versão>` para o build padrão, ou
  `<versão>-cloud` com todos os módulos cloud, ambos para Linux amd64 e arm64. `latest` e
  `latest-cloud` acompanham a última release estável; pré-releases só têm tags de versão.
  A base Debian bookworm-slim (git e certificados CA) só é atualizada numa release do Meada.
  A imagem roda sem root e não define comando: passe os argumentos explicitamente, por
  exemplo `docker run --rm ghcr.io/claudiomascaro/meada:latest --version`. Para servir,
  monte o manifesto, os espelhos e os índices, e passe
  `mcp --listen 0.0.0.0:7333 --workspace /etc/meada/meada.workspace.toml`, seguindo
  [Modo servidor](#modo-servidor) e sua configuração de admissão.

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

## Modo servidor

> **Atenção.** Em modo servidor o Meada devolve trechos de código-fonte a quem consegue
> conectar, e por padrão não autentica ninguém: o proxy reverso do operador admite os clientes
> (TLS, tokens, SSO, política de rede); o módulo OIDC, abaixo, é a alternativa opt-in. Sem `host`, o servidor escuta só no loopback; escutar além
> dele é uma escolha explícita do operador, e um proxy mal configurado expõe o código.
> O Meada não confere o cabeçalho `Host` (um proxy com nome próprio funciona) e responde 403 a
> qualquer requisição com `Origin` (proteção contra navegadores e DNS rebinding), em qualquer
> caminho e antes de qualquer outra resposta. O proxy não deve expor o Meada além dos clientes
> que admite.

O modo servidor roda o Meada uma vez para uma organização: `meada mcp --listen` serve as
ferramentas por HTTP a partir de espelhos dos repositórios que um manifesto de workspace lista,
com cada índice fora do checkout. O manifesto `meada.workspace.toml` vive num repositório de
operações, versionado, e só é lido do caminho que `--workspace` dá:

```toml
index_dir = "/var/lib/meada"

[[repository]]
repo = "github.com/acme/web"
path = "/srv/mirrors/web"

[[repository]]
repo = "github.com/acme/api"
path = "/srv/mirrors/api"

[repository.serve]
hosts = ["api.acme.com"]
prefix = "/api"

[[repository.resource]]
kind = "postgres"
id = "orders"
role = "write"

[[repository.peer]]
repo = "github.com/acme/web"
base_url_env = "WEB_URL"
```

`index_dir` é obrigatório; `path` é absoluto ou relativo à pasta do manifesto. Os blocos
`serve`, `resource` e `peer` (o par sem `path`, pois o manifesto localiza todos) são opcionais e
valem para um espelho sem `meada.toml`: o `meada.toml` do próprio repositório vence, inteiro, e
os blocos do manifesto são ignorados (o `meada status` os nomeia). Um espelho é um checkout com
árvore de trabalho; um clone bare não tem o que indexar e é recusado.

- `meada index --workspace <manifesto>` atualiza o índice de cada repositório, em
  `<index_dir>/<identidade>/index.db`. É o refresh do operador, rodado por um job de fetch ou
  por um webhook: o Meada nunca busca nem clona, e o servidor só lê. Depois de instalar um
  binário novo, rode `meada index --workspace` antes de reiniciar o servidor.
- `meada mcp --listen <endereço> --workspace <manifesto>` serve. O endereço é `host:porta`,
  `:porta` ou `porta`; o host é um IP ou `localhost`, nunca resolvido, e o padrão é o loopback.
  A primeira linha do stderr é `listening on http://<host>:<porta>`. SIGINT ou SIGTERM param o
  servidor: ele deixa de aceitar conexões, termina as respostas em andamento e sai com código 0. A espera tem limite fixo de 25 segundos,
  abaixo dos 30 s de carência padrão do Kubernetes: passado o limite, as conexões restantes são
  descartadas e a saída continua sendo 0.
- Cada repositório é um endpoint, `http://<host>:<porta>/<identidade>` (por exemplo
  `http://meada.internal:7333/github.com/acme/web`); o cliente configura uma URL por repositório
  em que trabalha. Qualquer outro caminho responde 404 com a lista das identidades servidas.
- O transporte HTTP é sem estado, com respostas em JSON: qualquer um de vários servidores sobre
  os mesmos índices responde a qualquer chamada.
- O servidor lê o manifesto e o `meada.toml` e o `meada.decisions.toml` de cada espelho uma vez,
  ao iniciar: mudar esses arquivos pede reiniciar o servidor. Uma mudança de código num espelho
  pede só `meada index --workspace`; a chamada seguinte já vê o índice novo.
- `context` imprime `index behind the mirror: run meada index --workspace` quando o arquivo de
  um trecho mudou depois da indexação. `trace` com `changes: uncommitted` é recusado, pois a
  árvore de trabalho está na máquina do cliente.
- `meada status --workspace <manifesto>` mostra o estado de cada índice, e
  `meada export --workspace <manifesto> --repo <identidade>` imprime o índice de um repositório,
  por exemplo para alimentar uma base de conhecimento.

### Vários nós

O [exemplo Docker Compose](deploy/compose/README.md) oferece os dois layouts abaixo, com dois
nós, TLS e admissão por token no proxy.

O [chart Helm](deploy/helm/meada/README.md) configura init, readiness, drain e o perfil de gerações no Kubernetes.

Vários servidores sobre os mesmos índices servem sem módulo de nuvem, desde que o operador
respeite um único escritor por índice.

- **Layout A, cada nó indexa os próprios espelhos.** Cada nó tem um `index_dir` local e roda
  `meada index --workspace` sobre os espelhos que busca. É o mais simples, mas os nós divergem:
  cada um enxerga o commit que buscou, e duas chamadas ao mesmo endpoint podem responder
  diferente até todos buscarem.
- **Layout B, um job constrói e os nós recebem.** Um job busca os espelhos e indexa tudo num
  único commit; os nós recebem os espelhos e os índices prontos e os trocam por `rename`. A
  reconstrução também usa essa troca (`index.db.new.<pid>` sob `index.db.lock`, renomeado sobre
  `index.db`), então um leitor vê o índice antigo ou o novo inteiro. Troque um índice por
  `rename` só enquanto nenhum `meada index` o escreve: o `index.db-journal` vivo de um refresh no
  lugar seria tomado como journal do arquivo novo.
- **Sem suporte: `index_dir` em sistema de arquivos de rede (NFS, EFS) compartilhado por um
  escritor e leitores.** O SQLite não é confiável sobre rede (veja
  [sqlite.org/useovernet.html](https://sqlite.org/useovernet.html)). Cada nó precisa de um
  `index_dir` local, ou de um volume que só ele escreve.
- **Somente leitura.** `index_dir` e os espelhos podem ser somente leitura (um volume preenchido
  por um init, uma camada de imagem): o servidor não cria nada ali. Um espelho de outro usuário
  é aceito com `-c safe.directory`, só para os espelhos do manifesto, o que exige git 2.38 ou
  posterior; no Ubuntu 22.04 e no RHEL 8, com git mais antigo, defina `safe.directory` na
  configuração do sistema.
- **Sem sessão.** O transporte é sem estado: nenhuma afinidade de sessão (sticky session) é
  necessária, e clientes no protocolo 2026-07-28 nem têm sessão.
- **Proxy reverso.** Encaminhe sem alterar `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` e
  todo cabeçalho `Mcp-Param-*` (SEP-2243: o rmcp rejeita uma requisição cujos cabeçalhos
  discordam do corpo); o proxy pode rotear por eles. Encaminhe o cabeçalho de resposta
  `Meada-Generation`. Não adicione `Origin`: o Meada responde
  403 a qualquer requisição que o traga, e assim uma página de navegador não o chama
  diretamente. O Meada não confere `Host`, e `--listen` aceita um IP ou `localhost`, nunca um
  nome que ele resolva.
- **Prontidão.** Primeiro um passo de init (o `meada index --workspace` ou a cópia dos índices).
  Depois, uma sonda TCP ou `meada status --workspace <manifesto>` como sonda exec: sai com 0
  quando todo repositório pode ser servido e com 1 caso contrário, espera no máximo 50 ms por um
  commit em andamento e conta um índice travado como pronto. `--listen` só abre a porta depois
  das verificações de início, então uma porta aberta indica índices presentes. O 404 em `/` lista
  identidades e não é um health check.
- **Escritor único.** Rode o job de indexação sem sobreposição (por exemplo um CronJob com
  `concurrencyPolicy: Forbid`). Defina `terminationGracePeriodSeconds` acima do limite de 25 s do
  SIGTERM e um atraso de `preStop` igual ao tempo que o balanceador leva para tirar o nó do ar.

### Módulos de nuvem

Módulos opt-in, desligados por padrão; cada um é documentado abaixo quando chegar. Um módulo é
uma feature do cargo mais uma seção de `meada.workspace.toml`; os binários de release são o build
padrão, sem nenhum, e uma seção num build sem a sua feature é um erro que nomeia a feature. A
feature `cloud` liga todos de uma vez (`cargo install meada --features cloud`) e, como
`refresh-lock`, só compila em Unix.

**Snapshot** (feature `snapshot`, `cargo install meada --features snapshot`). Um job indexa e
publica; os nós recebem o resultado inteiro, em vez de cada um indexar os próprios espelhos.
Depois de `meada index --workspace`, que continua sem rede, o job roda
`meada snapshot publish --workspace <manifesto>`. Ele confere, só lendo, que cada índice é igual
ao seu espelho e que cada espelho está limpo (`git status` sem nada, nem arquivo ignorado), e
recusa listando cada repositório e o comando `meada index --workspace` que resolve. Então envia
uma geração: um bundle por repositório (um tar do espelho, `.git` incluído, e do seu
`index.db`), um documento que os lista com tamanho e hash BLAKE3, e por último o ponteiro
`current`. A única linha em stdout é o id da geração. Um bundle ou uma geração já guardados não
são enviados de novo, e nada é escrito na máquina que publica.

```toml
[snapshot]
endpoint = "https://s3.us-east-1.amazonaws.com"   # qualquer endpoint compatível com S3
region = "us-east-1"
bucket = "exemplo-meada"
prefix = "prod"                                    # opcional
path_style = false                                 # true na maioria dos stores self-hosted
# ca_file = "ca.pem"                              # CA privada opcional, relativa ao manifesto
credentials = "static"                             # ou "web-identity", "pod-identity", "instance"
access_key_id_env = "AWS_ACCESS_KEY_ID"            # padrões; ou as chaves *_file
secret_access_key_env = "AWS_SECRET_ACCESS_KEY"
session_token_env = "AWS_SESSION_TOKEN"            # opcional
local_dir = "/var/lib/meada/generations"           # para o pull e o serviço das gerações
```

O publish só conecta ao `endpoint`, com TLS fora de loopback, e ao endpoint de credencial da
fonte escolhida; nenhuma requisição segue um redirecionamento nem passa por proxy. As
credenciais vêm de uma fonte só, a que `credentials` nomeia: `static` lê a chave e o segredo das
variáveis ou dos arquivos que a seção nomeia (nunca do manifesto); `web-identity` (IRSA) lê
`AWS_WEB_IDENTITY_TOKEN_FILE` e `AWS_ROLE_ARN` e troca o token no STS da `region`, por https;
`pod-identity` (EKS Pod Identity) chama o agente em `AWS_CONTAINER_CREDENTIALS_FULL_URI`, só
`http://169.254.170.23` ou `http://[fd00:ec2::23]`, com o token de
`AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`; `instance` usa o serviço de metadados da instância
(IMDSv2, só por IPv4, `http://169.254.169.254`) e só quando escrito na seção. As credenciais ficam só em
memória, e nenhum segredo aparece num erro.

`ca_file` acrescenta certificados PEM às raízes de confiança do sistema. O arquivo tem no
máximo 1 MiB; certificados inválidos ou um arquivo vazio são recusados, nomeando o arquivo.

Em cada nó, `meada snapshot pull --workspace <manifesto>` lê o ponteiro `current` do store e,
quando ele nomeia uma geração que o nó ainda não tem, baixa os bundles para `local_dir`, confere
o tamanho e o hash BLAKE3 de cada um contra o documento, extrai e troca o ponteiro do nó por
rename. A única linha em stdout é o id da geração atual. `--every <segundos>` repete o pull (e só
imprime quando o ponteiro muda); um segundo pull no mesmo `local_dir` sai com 0 avisando que já
há um em andamento. O nó guarda a geração atual e a anterior e apaga as mais antigas.

Tudo o que vem do store é tratado como entrada hostil: ids e identidades são conferidos antes de
virar caminho; o pull recusa no arquivo um caminho absoluto, um `..`, um link que sai do
repositório, uma entrada gravada através de um link simbólico e dispositivos ou FIFOs, com tetos
de bytes e de entradas por geração. O `.git/config` extraído é trocado por um mínimo que o Meada
escreve (só `[core]` e o `origin` da identidade), `.git/hooks` é apagado, e o `git` do Meada
roda nesses espelhos sem a configuração do sistema nem a global. Um pull que falha em qualquer
passo deixa o ponteiro e as gerações guardadas como estavam.

```
<local_dir>/
  current                     # "<id da geração> <fence>", fence 0 sem a trava de refresh
  pull.lock
  <id da geração>/
    generation.json           # o documento, como guardado no store
    <id do bundle>/index.db
    <id do bundle>/mirror/    # o checkout, .git incluído
```

Para servir o que o nó recebeu, `meada mcp --listen <endereço> --workspace <manifesto>
--generations`. O manifesto do nó contribui só com as seções de módulo (`[snapshot]`); os
repositórios são os que a geração atual registrou, com os espelhos e índices dentro de
`<local_dir>/<id da geração>/`. Cada chamada lê `<local_dir>/current` e abre o motor sobre a
geração que ele nomeia: uma chamada já aberta termina na geração em que começou, a seguinte vê a
nova, e um `meada snapshot pull --every` troca a geração sem reiniciar o servidor (um
repositório novo numa geração nova ganha o seu endpoint na hora). Nada é escrito por chamada,
os índices abrem só para leitura e o servidor não abre conexão alguma com o store. Os nós nunca
rodam `meada index`: quem indexa é o job, e o nó só recebe. Sem geração atual, ou com um índice
que não está na versão de schema deste binário, o servidor recusa subir e nomeia
`meada snapshot pull` ou o repositório e a geração.

`meada status --workspace <manifesto> --generations` é a sonda de prontidão do nó. Imprime o id
da geração atual, os ids guardados e uma linha `store: reachable (current <id>)` ou
`store: unreachable (<motivo>)`. O código de saída depende só do estado local: 1 sem uma
geração atual servível (os problemas vão para stderr), 0 do contrário, com o store alcançável
ou não, de modo que um store fora do ar não tira o nó da rotação. `meada export --workspace
<manifesto> --repo <identidade> --generations` exporta do mesmo jeito.

**Ponteiro de geração e pins** (feature `generation-pointer`, que inclui `snapshot`;
`cargo install meada --features generation-pointer`). Com a seção `[generations]`, que exige
`[snapshot]` e `[valkey]`, o ponteiro da geração atual sai do store e vai para o Valkey, e a
frota inteira guarda o mesmo conjunto de gerações.

```toml
[valkey]
url = "rediss://meada@cache.exemplo.internal:6379"   # TLS fora de loopback
password_file = "/run/secrets/valkey"                 # ou password_env
key_prefix = "meada:prod"                             # padrão "meada"

[generations]
keep = 3                                              # gerações retidas na frota, no mínimo 2
```

O `meada snapshot publish` termina gravando, num só script, `{<key_prefix>}:current` (um hash
`{id, fence}`) e `{<key_prefix>}:retained` (uma lista de `<id>:<fence>`, da mais nova para a
mais antiga, cada id uma vez, cortada em `keep`), e não grava mais o `current` do store; o
fence é `0` sem a trava de refresh. Cada `meada snapshot pull` lê os dois do Valkey e confere
cada id (16 caracteres hexadecimais minúsculos) e cada fence (decimal) antes de mexer no nó;
então baixa toda geração retida que o nó não tem e apaga as que saíram da lista, nunca a atual
nem a que acabou de substituir. O arquivo de ponteiro do nó leva o fence lido com o id. Uma
entrada malformada, um ponteiro ausente ou uma lista vazia param o pull com um erro que nomeia
a chave, e o nó fica como estava. A geração atual é baixada e trocada primeiro: uma geração
retida que falha depois dela vira um aviso em stderr com o id e a causa, não bloqueia nada, e o
próximo pull tenta de novo. Use o mesmo `keep` no job e nos nós. `meada status --generations`
lê a geração atual do Valkey e confere que o store guarda o seu documento; a linha
`store: unreachable (...)` diz se falhou o Valkey ou o store.

Um run de workflow fixa as commits em que começou com um pin no caminho:
`/<identidade>@<geração>`, a geração sendo o que vem depois do último `@`, serve a geração
guardada, em qualquer nó, enquanto ela estiver retida, e `/<identidade>` continua servindo a
atual. Uma geração que o nó não guarda responde 404 `text/plain` nomeando a geração pedida, a
atual e as guardadas. Cada resposta HTTP nomeia a sua geração no cabeçalho `Meada-Generation`
(um 404 nomeia a atual) e cada resultado de ferramenta no `_meta`, `{"meada/generation":
"<id>"}`: o run lê a geração da primeira resposta e fixa as seguintes nela. As ferramentas, os
parâmetros e o texto das respostas não mudam. O proxy reverso repassa `Meada-Generation` nas
respostas. Uma chamada fixada ainda aberta numa geração que a retenção de um pull apaga pode
falhar no meio.

**Trava de refresh** (feature `refresh-lock`, que inclui `generation-pointer`, só Unix;
`cargo install meada --features refresh-lock`). Com a seção `[refresh_lock]`, que exige
`[generations]`, o refresh inteiro roda sob uma trava no Valkey, e cada aquisição ganha um
fence, de modo que um job sobreposto ou travado nunca troca uma geração nova por uma mais
antiga.

```toml
[refresh_lock]
ttl_seconds = 300                                     # padrão; no mínimo 10
```

```sh
meada snapshot lock --workspace ops/meada.workspace.toml -- \
  sh -c 'git -C api fetch && git -C api reset --hard origin/main &&
         meada index --workspace ops/meada.workspace.toml &&
         meada snapshot publish --workspace ops/meada.workspace.toml'
```

`meada snapshot lock` toma a trava com `SET {<key_prefix>}:lock <token aleatório> NX PX <ttl>`,
soma um em `{<key_prefix>}:fence` e roda o comando como filho, com `MEADA_REFRESH_LOCK` (o
token) e `MEADA_REFRESH_FENCE` no ambiente. A trava é renovada a cada terço do TTL (comparar e
estender) e liberada no fim (comparar e apagar); o código de saída é o do filho (128 mais o
sinal quando um sinal o encerrou). Uma trava já tomada sai com 1 nomeando a chave, sem rodar
nada. Quando uma renovação acha outro token, ou nenhuma deu certo por um TTL inteiro, a trava
está perdida: o grupo de processos do filho recebe SIGTERM (e SIGKILL 10 s depois, se ainda
estiver vivo), e `lock` sai com 1 dizendo que a perdeu. Um SIGTERM, SIGINT, SIGHUP ou SIGQUIT em `lock`
chega ao grupo do mesmo jeito; quando o filho termina, o que sobrou no grupo de processos recebe
SIGKILL (um neto que chama `setsid` sai do grupo e escapa), e no Linux o filho recebe SIGTERM se
`lock` morrer antes. A indexação continua sem rede: o `meada index` do
filho não abre conexão alguma.

Com `[refresh_lock]`, `meada snapshot publish` exige as duas variáveis, e antes de cada envio
confere que a trava ainda guarda o token: se não, para sem enviar mais nada e sem gravar o
ponteiro. O script do ponteiro só grava quando a trava guarda o token e o fence é maior que o de
`{<key_prefix>}:current`, numa só operação. Os nós nunca trocam para uma geração de fence menor
que o do seu arquivo de ponteiro; sem o arquivo, tomam a atual seja qual for o fence. O Meada
nunca baixa nem apaga `{<key_prefix>}:fence`. Se um flush ou um failover do Valkey o perder, os
fences seguintes ficam menores que os registrados e a frota para: o publish recusado sai com 1
nomeando `{<key_prefix>}:fence`, o pull de um nó com fence maior sai com 1 sem trocar, e
`meada status --generations` nesse nó imprime uma linha `fence: travado: ...` (`fence: stuck: ...` fora do português) com os dois fences
(o código de saída não muda). Para sair disso, defina `{<key_prefix>}:fence` igual ou acima do
maior fence registrado (no Valkey ou no `<local_dir>/current` de algum nó), ou apague o
`<local_dir>/current` do nó, que então toma a geração atual. Desligar `[refresh_lock]` depois
que os nós gravaram fences acima de 0 também trava a frota, já que sem a trava todo fence é 0:
apague o `<local_dir>/current` de cada nó.

**OIDC** (feature `oidc`; `cargo install meada --features oidc`). A admissão padrão continua
sendo o proxy reverso, como diz o aviso no início desta seção; o módulo OIDC é a alternativa
para quem quer que o próprio Meada confira quem chama. Com a seção `[oidc]`, o servidor vira um
resource server OAuth 2.1, como a especificação de autorização do MCP (2026-07-28) descreve:
toda requisição leva `Authorization: Bearer <JWT>` emitido pelo provedor do operador. O Meada
não emite token, não registra cliente e nunca repassa um token.

```toml
[oidc]
issuer = "https://id.exemplo.internal/realms/agentes"
public_url = "https://meada.exemplo.internal"   # a URL que os clientes usam, atrás do proxy
audience = "https://meada.exemplo.internal"     # opcional; padrão: a URL canônica do endpoint
identities_claim = "meada_repositories"          # opcional com groups_claim
groups_claim = "groups"                          # opcional, junto com [oidc.groups]
scopes = ["meada:read"]                          # opcional, anunciado e exigido
jwks_refresh_seconds = 3600                      # padrão; no mínimo 60

[oidc.groups]                                    # um grupo do token: as identidades que ele libera
"time-pagamentos" = ["github.exemplo.internal/acme/payments-api", "github.exemplo.internal/acme/ledger"]
"plataforma" = ["*"]
```

No provedor: um audience igual a `audience` (ou, sem ela, à URL de cada endpoint,
`<public_url>/<identidade>`); um claim mapper que põe em `identities_claim` a lista das
identidades que o cliente pode ler (`"*"` libera todas), ou em `groups_claim` os grupos dele; e
client credentials para os agentes headless e para o backend de um portal, que pedem o token ao
provedor. Uma página no navegador não chama o Meada direto: com `[oidc]`, toda requisição com
`Origin` recebe 403, em qualquer caminho, então um portal chama do seu backend. O proxy repassa
sem mudar o `Authorization` e os cabeçalhos do SEP-2243 (`MCP-Protocol-Version`, `Mcp-Method`,
`Mcp-Name`, `Mcp-Param-*`).

Com `groups_claim`, a tabela `[oidc.groups]` do manifesto mapeia cada grupo para as identidades
que ele libera (`"*"` para todas), e `identities_claim` fica opcional: as duas chaves vêm juntas,
e uma seção sem nenhum dos dois claims recusa a subida. O token lê a união das identidades do seu
`identities_claim` e das que os seus grupos mapeiam. Um claim ausente ou que não é um array de
strings, um grupo que a tabela não mapeia e uma identidade que o servidor não serve não liberam
nada; um erro de digitação aparece para quem chama como 403.

Na subida, antes de escutar, o Meada lê os metadados do issuer (RFC 8414 ou OpenID Discovery,
nos caminhos e na ordem que a especificação do MCP define, conferindo que o `issuer` do
documento é o configurado) e o JWKS que eles nomeiam; uma falha recusa a subida. `issuer`,
`public_url` e o `jwks_uri` precisam ser `https` fora de loopback; nenhuma das requisições segue
um redirecionamento nem passa por proxy, cada uma tem 10 s e um teto de tamanho, e os
certificados são conferidos contra as raízes do sistema. O JWKS é relido a cada
`jwks_refresh_seconds` e quando chega um token com um `kid` desconhecido, no máximo uma vez por
minuto; se a releitura falha, as chaves em cache continuam e o stderr diz isso.

`GET /.well-known/oauth-protected-resource/<identidade>` responde sem token os metadados do
recurso (RFC 9728): `resource` igual a `<public_url>/<identidade>`, `authorization_servers`
igual a `[issuer]`, `bearer_methods_supported` igual a `["header"]` e `scopes_supported` quando
há `scopes`; `/.well-known/oauth-protected-resource/<identidade>@<geração>` responde o mesmo
com o `resource` fixado. Um token passa com um algoritmo assimétrico (RS256, RS384, RS512,
ES256, ES384, PS256) que case com a chave, `iss` igual a `issuer`, um `aud` igual a `audience`
ou, sem ela, à URL de `/<identidade>` (aceita também nos pins, de modo que um token lê o
repositório em toda geração), `exp` obrigatório e `nbf` respeitado com 60 s de folga, e todo
escopo configurado no seu `scope`. Um token na query string nunca é lido. O `scope` é lido só
como string separada por espaços; `scp` e um `scope` em array não são lidos, então, com
`scopes` definido, um provedor que manda `scp` (o Okta, por exemplo) precisa de um claim mapper
para `scope`.

Sem token, ou com um token que não passa: 401 com `WWW-Authenticate: Bearer
resource_metadata="<public_url>/.well-known/oauth-protected-resource/<caminho>"`, mais
`error="invalid_token"` quando veio um token e `scope` quando configurado. Um token válido que não
pode ler a identidade do endpoint, num pin também: 403 com `error="insufficient_scope"`; sem um
escopo exigido, o mesmo 403 com `scope`. Atrás de um endpoint que o token pode ler, toda resposta
cobre só os repositórios que ele pode ler. A autenticação vem antes do roteamento: o 404 da raiz,
de um caminho desconhecido e de uma geração que o nó não guarda exige um token válido para aquele
caminho (sem `audience`, o da raiz tem `aud` igual a `public_url`) e lista só as identidades que
ele pode ler. Nenhum token nem pedaço de um token aparece em stderr ou num erro.

**GitHub App** (feature `github-app`, que inclui o Valkey; `cargo install meada --features
github-app`). O Meada continua sem buscar nada: quem faz o fetch dos espelhos é o git do job de
indexação, e o Meada só responde ao pedido de credencial do git com tokens de instalação de um
ou mais GitHub Apps, emitidos e renovados por ele. As instalações são descobertas pela API do
GitHub, não declaradas; a carga se reparte entre os Apps e as instalações, e uma instalação
limitada fica de lado até liberar. Só o job usa o módulo: `meada mcp --listen` lê os blocos com
o manifesto e nunca chama o GitHub.

```toml
[valkey]                                # obrigatório com [[github_app]]
url = "rediss://meada@cache.exemplo.internal:6379"
password_file = "/run/secrets/valkey"

[[github_app]]
host = "github.com"                     # o host git que ele atende, como o git o nomeia
api = "https://api.github.com"          # a URL base da API REST; no GHES, https://<host>/api/v3
app_id = 123456
private_key_file = "/run/secrets/github-app.pem"   # ou private_key_env, nunca os dois
```

Pode haver vários blocos `[[github_app]]`, um por App (dois blocos do mesmo App no mesmo host são
um erro). `api` precisa ser `https` fora de loopback, e um bloco sem `[valkey]`, sem chave ou com
as duas, ou com uma chave desconhecida recusa a execução nomeando a chave ou a seção. A chave
privada só vem do arquivo ou da variável e nunca aparece em stderr nem num erro, como o JWT do
App e os tokens.

No job, antes do fetch, o git usa o Meada como credential helper do host, com o caminho do
repositório na requisição (use o caminho absoluto do manifesto, já que o git roda o helper no
diretório do espelho):

```sh
git config --global credential.https://github.com.helper \
  "!meada github-app credential --workspace /etc/meada/meada.workspace.toml"
git config --global credential.useHttpPath true
```

Para `https://github.com/<owner>/<repo>`, o `get` responde `username=x-access-token`, um token
de uma instalação que cobre o `<owner>` e o seu `password_expiry_utc`. As instalações de cada App
são listadas com o JWT dele (RS256, emitido 60 s no passado, expirando em 9 minutos) e guardadas
no Valkey, de novo no máximo a cada 10 minutos; um bloco novo entra no próximo `get`, sem
reiniciar nada. Entre as instalações que cobrem o owner, em ordem estável (id do App, depois id
da instalação), o Meada pula as marcadas como limitadas e escolhe uma em rodízio, por um contador
no Valkey. Um token só é emitido quando o Valkey não guarda um daquela instalação com pelo menos
10 minutos de vida; uma trava por instalação faz `get`s simultâneos emitirem uma vez só, e o
token fica em cache até 10 minutos antes de expirar. Quem lê esse Valkey lê o código-fonte por
esse tempo. Um repositório que nenhuma instalação cobre não recebe resposta, com saída 0 (o git
tenta o próximo helper) e uma linha em stderr com o host e o owner. `store` não faz nada; o
`erase`, que o git manda depois de um 401, descarta o token recusado, e o próximo `get` emite
outro.

Nem o git nem o GitHub avisam o helper de um limite secundário (403 ou 429). Por isso, depois de
um fetch que falhou assim, o job roda:

```sh
meada github-app throttled --workspace /etc/meada/meada.workspace.toml \
  https://github.com/<owner>/<repo> --retry-after <segundos>
```

Isso marca como limitada a instalação que respondeu por último para aquele repositório (o Meada
lembra por uma hora), até o `--retry-after` ou, sem ele, por 1 minuto, dobrando a cada repetição
enquanto a marca vale. As chamadas REST do próprio módulo que recebem 403 ou 429 marcam a sua
instalação (um token) ou o seu App (a listagem) do mesmo jeito, pelo `retry-after`, senão pelo
`x-ratelimit-reset` quando `x-ratelimit-remaining` é 0, senão por 1 minuto; um `get` cujo token
foi limitado passa para a próxima instalação. Com git 2.54 ou mais novo, `http.maxRetries`
faz o próprio git respeitar o `Retry-After` de um 429 nas suas requisições GET.

No GitHub.com e no GitHub Enterprise Cloud, `api` fica fora da fronteira do operador; isso é
aceito porque os fetches do job já vão para esse host (ADR 0013).

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
| `MEADA_DECIDER_MODEL` | modelo | `typesafe/jev-1.14` (`decisions`); obrigatória com `chat` |

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
não são indexadas, e os contratos Rust são os pacotes, o HTTP e os canais (abaixo); só a configuração padrão de cada crate entra, para um alvo fixo x86_64 Linux: item sob um `#[cfg(..)]` falso, ou sob uma feature fora do `default` do `Cargo.toml`, fica de fora, e módulos `#[cfg(test)]` entram), lê o código Go (`.go`, sem toolchain Go: um import chega à pasta do pacote pelo caminho de módulo de cada `go.mod`, também por um `replace` para uma pasta do repositório, e um módulo aninhado é dono da sua subárvore; os arquivos de uma pasta com o mesmo `package`, `_test.go` incluídos, dividem um escopo; as restrições de build valem para um alvo fixo linux/amd64, e um `_windows.go`, um `//go:build` falso ou um arquivo cgo (`import "C"`) fica de fora) com as suas funções, tipos, interfaces e métodos (`Tipo.Metodo`) e as chamadas a funções do mesmo pacote e por import (`pkg.F()`, com alias ou com ponto; a biblioteca padrão e os módulos de fora ficam externos) e a métodos (`recv.M()`: `exact` quando o código escreve o tipo do receptor, num parâmetro, campo, `var`, literal composto ou resultado de função, e por nome, `inferred` ou `ambiguous`, quando não escreve; uma chamada por uma interface para no método da interface), e liga repositórios, também de uma
linguagem para a outra, por contratos HTTP (`fetch` e, em Python, `requests`, `httpx` e `aiohttp` e, em Rust, `reqwest` e `ureq` e, em Go, `net/http` e resty no lado que
consome; Next.js (app router) e Express e, em Python, FastAPI, Flask e Django e, em Rust, axum, actix-web e Rocket e, em Go, `net/http`, gin, echo, chi e gorilla/mux no lado que serve,
também atrás de um gateway; em Rust, uma URL base lida do ambiente com `std::env::var`, `env!` ou `dotenvy::var` e, em Go, com `os.Getenv` ou `os.LookupEnv` (com um valor padrão `if v == "" { v = ... }`, também com `len(v) == 0`, `!ok` ou `!ok || v == ""` quando `ok` vem do mesmo `os.LookupEnv`, e através de `strings.TrimSuffix`, `TrimRight` ou `TrimSpace`) liga pelo `base_url_env`), pelos recursos
declarados no `meada.toml` e por canais: tópicos Kafka (`kafkajs` e, em Python, `confluent-kafka` e `kafka-python` e, em Rust, `rdkafka` e, em Go, `segmentio/kafka-go`, `confluent-kafka-go` e `sarama`), filas RabbitMQ
(`amqplib` e, em Python, `pika` e, em Rust, `lapin` e, em Go, `amqp091-go`, com o `publish` numa exchange chegando às filas que os bindings
ligam a ela), filas Amazon SQS (AWS SDK v3, `sqs-consumer` e, em Python, `boto3` e, em Rust, `aws-sdk-sqs` e, em Go, `aws-sdk-go-v2`), subjects NATS
(`nats` e, em Python, `nats-py` e, em Rust, `async-nats` e, em Go, `nats.go`, com os curingas `*` e `>`), jobs BullMQ e streams Redis
(`ioredis`, `node-redis` e, em Python, `redis-py` e, em Rust, o crate `redis` e, em Go, `go-redis`), com o produtor ligado ao handler de cada consumidor, inclusive
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
aparece. Em Go, cada `go.mod` com `module` vira o contrato `package::go::<caminho do módulo>`, e cada
`require`, e cada `replace` para uma pasta local, se liga ao provedor desse módulo no workspace: o
`replace` que sai do repositório (`../outro`) é resolvido só pelo texto, como o `path` do Cargo, e um
`replace` por outro módulo deixa o módulo de fora. A ligação é `exact`, com a versão do `require`
como faixa, `(local none)` (um `go.mod` não tem versão) e `range unchecked`, e um import de
`<módulo>/<pasta>` chega às funções, tipos e métodos exportados da pasta correspondente no
provedor, também `exact` quando a pasta existe, com `; import module path` no fim da razão. Não lê
`go.sum` nem consulta o proxy de módulos.
Não lê lockfile (`Cargo.lock` incluído) nem consulta o registro npm, o PyPI ou o crates.io. O Meada também aponta o *drift* de formato: os campos que um consumidor `fetch` lê da resposta já interpretada e que a rota que ele chama não devolve, comparados com o formato da rota (documento OpenAPI, schema zod ou objetos literais do handler). O `trace` termina a linha da ligação com `drift: missing <caminho>`, o `context` de uma rota diz de onde vem o formato e lista cada consumidor que lê campos da resposta, com o drift e o nível e as leituras não conferidas sob uma parte aberta quando há, o `search` com o tipo `drift` lista todo o drift, e o `meada status` conta as leituras ausentes dos consumidores de cada repositório (quem conta é o repositório do consumidor; o do provedor não mostra nada). O `meada export` traz as comunidades de cada repositório: grupos de símbolos que se chamam mais entre si do que com o resto, achados pelo algoritmo de Leiden sem nada aleatório e só com aritmética inteira, e nomeados pela pasta onde vive a maioria dos seus membros (`src`, `src#2`), iguais em toda máquina. Cada ligação tem o nível `exact`,
`declared`, `inferred`, `decided` ou `ambiguous`; uma chamada cuja URL não dá para resolver
aparece como não resolvida e só vira ligação por uma decisão, no nível `decided`, e uma rota que o Meada não sabe onde é servida fica fora do mapa em vez de
ser adivinhada. Clusters de
broker não são modelados: dois clusters com um tópico de mesmo nome se ligam como se fossem um
só. Cada rota que um handler serve, cada canal que um handler consumidor consome e cada função ou método exportado que nada chama no próprio repositório é um ponto de entrada, com o seu processo: o `trace` com `process::<identidade do ponto de entrada>` lê a árvore de chamadas que ele dispara, atravessando os repositórios, e o `meada export` traz o processo de cada ponto de entrada. Em Python, o bloco `if __name__ == "__main__":` de um arquivo (o símbolo `__main__`) e cada função que um `[project.scripts]` do `pyproject.toml` nomeia (`módulo:função`) também são pontos de entrada, e só os nomes públicos (sem `_` na frente, ou os do `__all__` literal) contam como exportados. Em Rust, o `fn main` de cada binário (`src/main.rs`, `src/bin/`, `[[bin]] path`) também é. Em Go, cada `func main` de um `package main` também é. O `search` com o tipo `community` acha a comunidade pelo nome e com o tipo `process` acha o processo pelo rótulo do ponto de entrada, e o `context` de um símbolo diz a sua comunidade (ou `community: none`) e os processos a que ele pertence, achados subindo pelas mesmas chamadas e ligações que o processo percorre. Ainda não há: outras linguagens, outros clientes HTTP além desses, outros clientes de
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
