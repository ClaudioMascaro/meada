# Credenciais de GitHub App

**GitHub App** (feature `github-app`, que inclui o Valkey; `cargo install meada --features
github-app`). O Meada continua sem buscar nada: quem faz o fetch dos espelhos é o git do job de
indexação, e o Meada só responde ao pedido de credencial do git com tokens de instalação de um
ou mais GitHub Apps, emitidos e renovados por ele. As instalações são descobertas pela API do
GitHub, não declaradas; a carga se reparte entre os Apps e as instalações, e uma instalação
limitada fica de lado até liberar. Só o job usa o módulo: `meada mcp --listen` lê os blocos com
o manifesto e nunca chama o GitHub.


<!-- meada-graph:app-credentials -->

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

## Apps e instalações

Pode haver vários blocos `[[github_app]]`, um por App (dois blocos do mesmo App no mesmo host são
um erro). `api` precisa ser `https` fora de loopback, e um bloco sem `[valkey]`, sem chave ou com
as duas, ou com uma chave desconhecida recusa a execução nomeando a chave ou a seção. A chave
privada só vem do arquivo ou da variável e nunca aparece em stderr nem num erro, como o JWT do
App e os tokens.

## Configurar o helper

No job, antes do fetch, o git usa o Meada como credential helper do host, com o caminho do
repositório na requisição (use o caminho absoluto do manifesto, já que o git roda o helper no
diretório do espelho):

```sh
git config --global credential.https://github.com.helper \
  "!meada github-app credential --workspace /etc/meada/meada.workspace.toml"
git config --global credential.useHttpPath true
```

<details markdown="1" class="reference-details">
<summary>Seleção de instalação, cache e renovação do token</summary>

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

</details>

## Limites de requisições

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

## Checagem de papéis

Com `[oidc.github]` (features `github-app`, `postgres` e `oidc`), o job pergunta ao GitHub, a
cada refresh, quem tem qual papel em cada repositório do workspace no `host` do provedor, com as
mesmas instalações, marcas de limite e cache de tokens do helper. Nenhuma permissão do App além
de Metadata (leitura) é necessária.

```toml
[oidc.github]
host = "github.com"                     # o padrão; um nome de host em minúsculas
login_claim = "github_login"            # a claim com o login GitHub do sujeito
roles = ["read", "triage", "write", "maintain", "admin"]   # o padrão; papéis customizados também
max_age_seconds = 5400                  # obrigatório, pelo menos 60
```

`[oidc.github]` exige `[postgres]` e um Meada compilado com `postgres`; ao lado dele, `[oidc]`
não precisa de `identities_claim` nem de `groups_claim`. `meada github-app roles --workspace
<manifesto>` lista `GET /repos/{owner}/{repo}/collaborators?affiliation=all&per_page=100`, todas
as páginas, e grava no Postgres, numa transação, um conjunto de permissões que substitui o
anterior: cada colaborador listado com o seu `role_name`. Sob `[refresh_lock]` o conjunto leva o
`MEADA_REFRESH_FENCE` do job, e um conjunto de fence maior já gravado recusa a execução. Um 403
ou 429 passa para a próxima instalação; com todas limitadas, num 5xx, num timeout ou num
cabeçalho `Link` que sai da listagem do repositório, nada é gravado e a saída é 1. Um 404, ou um
owner que nenhuma instalação cobre, dá um aviso e nenhuma linha.

Com `source = "store"`, toda adição cujo `added_by` não tem um papel de `roles` fica de fora:
o comando escreve `<index_dir>/workspace/excluded` (o id da versão, depois as identidades
excluídas, uma por linha), e `meada index`, `status`, `export` e `snapshot publish` a tratam como
ausente do workspace. Sem esse arquivo, ou com um de outra versão, eles recusam e nomeiam `meada
github-app roles`; uma checagem que falhou não deixa arquivo. `meada workspace show --pulled`
imprime a versão baixada sem as exclusões, para o script de fetch. O refresh do job, sob `meada
snapshot lock`, fica: `workspace pull`, `github-app roles`, o fetch do que `show --pulled` lista,
`index --workspace` e `snapshot publish`; um refresh que para na checagem não publica nada.

### O catálogo de repositórios

Com a feature `workspaces` (incluída em `cloud`) e `[catalog]` no bootstrap, o mesmo job
descobre também os repositórios que um mantenedor poderia selecionar antes de existir um
manifesto, independentemente do workspace aplicado ([#341](https://github.com/ClaudioMascaro/meada/issues/341),
ADR 0021):

```toml
[catalog]
allow = ["github.com/acme/*", "github.com/outro/ferramenta"]   # obrigatório; no host de [oidc.github]
max_repositories = 200                                           # obrigatório, de 1 a 5000, sem padrão
```

`allow` lista padrões em minúsculas, `<host>/<dono>/*` ou `<host>/<dono>/<repositório>`,
comparados exatamente, sem repetição; `[catalog]` exige `[oidc.github]`, e o erro nomeia as duas
seções. Com `[catalog]`, `meada github-app roles` lista, para cada instalação dos Apps do `host`
cujo dono algum padrão nomeia, `GET /installation/repositories?per_page=100` (todas as páginas,
com as mesmas instalações, marcas de limite e cache de tokens), mantém o que `allow` admite, soma
as identidades que o workspace seleciona e lista os colaboradores de cada uma, como antes. O
resultado é um único conjunto de permissões, na mesma transação, com `source` (`selected`,
`catalog` ou `both`) em cada repositório (migração 6: rode `meada postgres migrate` antes). O
catálogo são os repositórios `catalog` ou `both` que o GitHub mostrou (`listed`).

Uma listagem de instalação que falha, uma instalação limitada (que fica marcada, como em toda
chamada REST do módulo), uma resposta que não é o objeto documentado, mais de 100 páginas ou mais
candidatos do que `max_repositories`, sem contar os repositórios do workspace, fazem o comando
falhar com saída 1 e **nada é gravado**: um conjunto parcial revogaria ou misturaria
repositórios, e o anterior continua envelhecendo. A linha de sucesso passa a ser `conjunto de
permissões <id>: N repositórios (M no catálogo), R linhas, E exclusões`; sem `[catalog]` nada
muda, byte a byte.

**Custo.** Além da carga da checagem de papéis, o catálogo paga `ceil(cobertos / 100)` páginas
de instalação por instalação listada (`cobertos` conta todo repositório que a instalação cobre,
não só os que `allow` admite) e, por candidato, `ceil(colaboradores / 100)` páginas de
colaboradores. Por isso `max_repositories` não tem padrão: quem o define aceita o custo. O custo
real contra uma organização real do GitHub ainda não foi medido
([#338](https://github.com/ClaudioMascaro/meada/issues/338) não bloqueia o S5); a suposição que
essa medição confirmaria é que a listagem das instalações mais a dos colaboradores com
`affiliation=all` cabe na hora de uma instalação no limite que você definiu, ou que antes é preciso
listar uma vez os membros do papel-base. Meça com a sua organização antes de ampliar a cobertura.

**Cobertura não é direito.** O catálogo só diz o que *poderia* ser selecionado. Uma pessoa só
seleciona um repositório do catálogo em que o seu login do GitHub tem um papel aceito no conjunto
mais recente, direto ou por um grupo do repositório; cobertura do App, nomes de grupo em um token
e qualquer coisa que um agente afirme não concedem nada. A console e os nós continuam sem chamar
o GitHub: a leitura do catálogo ([console](console.md#catalogo-de-repositorios)) usa só o conjunto
que o job gravou.

### O grant na admissão

Nós e a console com `[oidc.github]` leem, a cada minuto, o conjunto de permissões mais novo, e
carregam as linhas cujo papel está em `roles` só quando o conjunto muda; a admissão nunca espera
pelo Postgres. O login de um token é o valor de `login_claim` quando é uma string de 1 a 39
letras ASCII, dígitos, `-` e `_`, comparado em minúsculas; qualquer outro valor não dá grant do
GitHub. Use uma claim que o administrador do provedor de identidade mapeia de um atributo
verificado, nunca uma que o usuário edita. O grant do GitHub é o que esse login lê no conjunto
enquanto a idade dele (o relógio do Postgres menos `finished`, mais o tempo monotônico do
processo desde a leitura) é no máximo `max_age_seconds`; depois disso, com o job parado ou o
Postgres inalcançável, ele fica vazio e o grant das claims continua valendo. Ele nunca vale `*`,
e um token de máquina com `*` não depende do Postgres. A sessão da console mostra `login`,
`github_host` e `github_grant` (`current`, `expired` ou `unread`), a verificação de token separa
as identidades em `sources` (`claims` e `github`) e a página de admissão mostra o conjunto
carregado. Um nó que acha uma versão de schema que não conhece diz isso uma vez no stderr e trata
o Postgres como inalcançável.
