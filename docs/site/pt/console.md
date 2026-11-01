# Console de gerenciamento

A console usa a feature `ui`, incluída em `cloud`, e roda em um processo próprio. Ela tem editor, diff, playground e grafo. O editor salva versões de configuração; aplicar uma versão e publicar os índices são etapas separadas.

Proteja o acesso antes de expor a console. Sem `[oidc]` nem `[accounts]` ([contas locais](#contas-locais)), o proxy controla a admissão: quem alcançar a console fora dessa proteção poderá ler o grafo. Para começar, ligue as contas locais; para produção, o provedor de identidade. O proxy deve preservar `Host`, `Origin` e `Authorization`. A console aceita apenas seu Host público e, se houver Origin, sua própria origem.

<!-- meada-graph:console-authoring -->

## Configurar o bootstrap

O bootstrap é o manifesto que inicia a console. Ele contém os módulos e usa diretórios próprios, separados dos nós. As entradas dos repositórios vêm do workspace gerenciado.

```toml
index_dir = "/var/lib/meada/console/indexes"

[workspace]
source = "store"

[snapshot]
endpoint = "https://store.exemplo.internal"
region = "us-east-1"
bucket = "meada"
prefix = "production"
path_style = true
credentials = "static"
access_key_id_env = "AWS_ACCESS_KEY_ID"
secret_access_key_env = "AWS_SECRET_ACCESS_KEY"
local_dir = "/var/lib/meada/console/generations"
node_name = "meada-console-0"
# ca_file = "/run/secrets/ca.crt"

[valkey]
url = "rediss://meada@cache.exemplo.internal:6379"
key_prefix = "meada"
password_env = "MEADA_VALKEY_PASSWORD"
# ca_file = "/run/secrets/ca.crt"

[generations]
keep = 3

[refresh_lock]
ttl_seconds = 300

[oidc]
issuer = "https://id.exemplo.internal/realms/agentes"
public_url = "https://mcp.exemplo.internal"
audience = "https://mcp.exemplo.internal"
identities_claim = "meada_repositories"
groups_claim = "groups"
scopes = ["meada:read"]

[oidc.groups]
"meada-readers" = ["github.exemplo.internal/acme/api"]
"meada-operators" = ["*"]

[[github_app]]
host = "github.com"
api = "https://api.github.com"
app_id = 123456
private_key_env = "MEADA_GITHUB_APP_KEY"

[ui]
dir = "/usr/share/meada/ui"
public_url = "https://console.exemplo.internal"
client_id = "meada-console"
audience = "https://console.exemplo.internal"
operations = false
operator_groups = ["meada-operators"]
# operator_claim = "meada_operator"
```

O `[oidc]` reproduz a política dos nós, com URL pública e audience do MCP. O `[ui]` define a origem e o audience da console. Exporte as credenciais no ambiente ou nos arquivos de segredo indicados.

<details markdown="1" class="reference-details">
<summary>Certificados, GitHub App e índices da console</summary>

`snapshot.ca_file` acrescenta a CA privada às raízes do sistema. `valkey.ca_file` usa somente as raízes daquele arquivo. Omita `[[github_app]]` se o job não usa Apps.

A console lê o cache de GitHub App, mas não abre a chave privada nem chama o GitHub. Também não chama os nós: playground e exports usam os índices baixados pelo pull da própria console.

</details>

## Iniciar a console

[Inicialize o workspace gerenciado](workspaces.md) e espere o job publicar a primeira geração. Faça o pull inicial no `local_dir` da console. Mantenha o pull periódico e o servidor em processos separados, usando esse mesmo diretório exclusivo da console:

```sh
meada snapshot pull --workspace /etc/meada/console.workspace.toml
meada snapshot pull --every 30 --workspace /etc/meada/console.workspace.toml
# Run in a separate process, alongside the pull companion.
meada ui --listen 0.0.0.0:7334 --workspace /etc/meada/console.workspace.toml
```

## Login no navegador

Registre no issuer um cliente público, sem segredo, com Authorization Code e PKCE S256. Configure o audience e o redirect URI exato `https://console.exemplo.internal/auth/callback`.

O navegador precisa alcançar os metadados e o endpoint de token do issuer, com CORS para a origem da console. Ele troca o código diretamente com o issuer. O bearer fica na memória; `state` e `verifier` ficam em sessionStorage até o callback removê-los. Recarregar a página reinicia o login.

## Contas locais

Sem um provedor de identidade, a console pode autenticar as próprias contas (feature `accounts`, parte de `cloud`; ADR 0024). `[accounts]` exige `[postgres]` e só fica junto de `[oidc]` como break glass (abaixo):

```toml
[accounts]
public_key_file = "/run/secrets/meada-signing.pub.pem"   # ou public_key_env
private_key_file = "/run/secrets/meada-signing.pem"      # ou private_key_env; só a console lê
# admin_password_env = "MEADA_ADMIN_PASSWORD"            # o padrão; ou admin_password_file
```

O par de chaves é Ed25519 em PEM (`openssl genpkey -algorithm ed25519 -out meada-signing.pem` e `openssl pkey -in meada-signing.pem -pubout -out meada-signing.pub.pem`). A console recusa a partida com uma chave ausente, ilegível, que não seja Ed25519 ou que não corresponda à outra, nomeando a chave e nunca o valor. Rode `meada postgres migrate` antes: na primeira partida com a tabela vazia, a console cria a conta padrão `admin` com a senha `admin123`, que precisa ser trocada antes de qualquer outra coisa, ou com o valor de `MEADA_ADMIN_PASSWORD`, sem troca obrigatória, se ele passar pela política de senhas (de 12 a 128 caracteres, nem a senha padrão, nem o nome de usuário, nem uma senha comum). Com `[accounts]`, `operations = true` não precisa de `[oidc]` nem de chave de operador: o papel é o da conta.

`POST /api/v1/login` recebe `{"username", "password"}` e responde `{"token", "expires_at", "change_required"}`: um token de sessão que a console assina e aceita por oito horas como bearer, em toda rota menos os arquivos estáticos, `config` e `login`. Um usuário desconhecido, uma senha errada e uma conta bloqueada recebem o mesmo 401 `invalid_credentials`. Com a troca pendente, só `POST /api/v1/account/password` (`{"current", "new"}`) aceita o token; ela devolve um token novo. `config` traz `"sign_in": "accounts"` e `session` traz `"account": true`. Cada login, recusa e troca de senha fica na auditoria (`account.login`, `account.password`) antes do token, sem senha nem token; sem o registro, a resposta é 503 `audit_unavailable`. Limite a taxa de `/api/v1/login` no proxy.

A política de senhas recusa as senhas de uma lista de senhas comuns embutida no binário (da SecLists, licença MIT, só as entradas de 12 caracteres ou mais), sem diferenciar maiúsculas, com a regra `common_password`. Dez falhas seguidas, no login ou na senha atual de uma troca, bloqueiam a conta por quinze minutos; a contagem é feita no Postgres segurando a linha da conta, então tentativas em paralelo nunca passam de dez, e um sucesso zera a contagem. Enquanto bloqueada, a conta recebe a mesma resposta de uma senha errada, no mesmo tempo, sem que a senha seja conferida e sem estender o bloqueio; a auditoria registra `locked_now` na falha que bloqueia e `locked` depois. Uma senha guardada com parâmetros mais fracos que os da console é refeita no próximo login que a acerta.

Para recuperar a conta padrão (senha perdida ou bloqueio), rode `meada ui reset-admin --workspace <manifesto de bootstrap>`: ele cria ou reativa `admin` como operador, remove o bloqueio e define o valor de `MEADA_ADMIN_PASSWORD` (ou de `admin_password_file`) como senha, ou, sem ele, uma senha aleatória mostrada uma vez na saída padrão; a troca fica obrigatória no próximo login. O comando grava `account.reset_admin` com `via` `cli` na auditoria antes de mudar a conta e sai com 1, sem mudar nada, se não puder gravar o registro.

O operador global administra as contas pela API, com `operations` ligado ou não, para que uma console só de leitura também tenha leitores. `GET /api/v1/accounts` lista todas (nome de usuário, papel, estado `active`, `suspended` ou `locked`, o fim de um bloqueio, se a troca é obrigatória, a última troca de senha, quando e por quem foi criada), nunca com o hash. `POST /api/v1/accounts` com `{"username", "role", "password", "change_required"}` cria uma conta `operator` ou `viewer` com uma senha temporária que passa pela política; `change_required` é verdadeiro se omitido. Um nome de usuário malformado recebe 422 `invalid_username` e um já usado 409 `username_taken`. `POST /api/v1/accounts/<usuário>` faz uma ação por vez: `{"action": "role", "role"}`, `{"action": "suspend"}`, `{"action": "reactivate"}` ou `{"action": "reset_password", "password", "change_required"}`, que também remove o bloqueio e torna a troca obrigatória se `change_required` for omitido; uma conta desconhecida recebe 404. Suspender ou rebaixar o último operador ativo recebe 409 `last_operator`, mesmo com duas requisições ao mesmo tempo. Nenhuma rota apaga ou renomeia uma conta. Quem não é o operador global recebe 403. Cada escrita da console lê a linha da conta antes de agir: uma conta suspensa ou rebaixada perde as escritas na hora, e as leituras continuam até o token expirar; uma conta suspensa recebe no login o mesmo 401 de uma senha errada. Cada mudança fica na auditoria (`account.create`, `account.role`, `account.suspend`, `account.reactivate`, `account.reset_password`) com a conta e os campos mudados, nunca uma senha, antes de ser feita; sem o registro, a resposta é 503 `audit_unavailable` e nada muda.

Os grupos de uma conta vêm do operador global (`"groups"` na criação, e `{"action": "groups", "groups"}`, que substitui a lista; cada grupo é um nome que o dono de uma entrada pode ter, senão 422 `invalid_group`), aparecem na lista e ficam na auditoria como `account.groups`. `[accounts.groups]` liga cada grupo às identidades que ele concede, como `[oidc.groups]` (`*` para todas):

```toml
[accounts.groups]
"team-billing" = ["github.com/acme/billing", "github.com/acme/ledger"]
"platform" = ["*"]
```

Sem a tabela, toda conta local lê todos os repositórios. Com ela, a concessão de uma sessão local é o que os grupos do token concedem, e limita cada visão (o manifesto, o workspace e as versões, o store, os repositórios, as credenciais, o playground, os exports e o grafo) como a de um provedor de identidade; o operador global lê tudo. Um leitor cujo grupo é dono de uma entrada da versão atual do workspace que a sua concessão lê é operador de time (ADR 0015): `session` lista os times, e ele salva e aplica as entradas do time. O token de sessão leva os grupos: uma mudança vale para as escritas na hora (a linha da conta é lida antes) e para as leituras no próximo login. Com `[accounts]`, `GET /api/v1/admission` mostra o modo (`accounts` ou `break_glass`), a impressão digital da chave pública (`blake3:` e o hash BLAKE3 da chave Ed25519 crua, como `openssl pkey -pubin -in meada-signing.pub.pem -outform DER | tail -c 32 | b3sum` calcula) e a tabela de grupos limitada pela concessão de quem pergunta.

No navegador, `"sign_in": "accounts"` faz a console mostrar a própria página de entrada em vez do redirecionamento do provedor de identidade. Uma entrada que precisa trocar a senha vai direto a uma página que pede a senha atual e a nova duas vezes, mostra a regra que a recusa nomeia e não oferece mais nada até a troca dar certo, e então abre o workspace. O token de sessão fica no `sessionStorage` da aba (nunca em `localStorage`, numa URL ou num log), é enviado como bearer e é esquecido ao sair, em qualquer 401 que não seja uma senha recusada (a console volta à entrada) e quando a aba fecha. A página de perfil oferece a troca voluntária de senha e a saída. O texto está em português e inglês.

A página de contas (`/accounts`) é listada só para o operador global quando `config` diz `"sign_in": "accounts"`; qualquer outro papel, e uma console em break glass, recebe a página de não encontrado nesse caminho. Ela lista cada conta com papel, grupos, estado (a conta suspensa é marcada em texto, a bloqueada diz até quando), se precisa trocar a senha e quando trocou por último. O formulário de criação (nome de usuário, papel, grupos, senha temporária e "deve trocar a senha na primeira entrada", marcado por padrão) confere o padrão do nome e o tamanho da senha antes de perguntar ao servidor, e mostra a regra do servidor numa recusa. Papel, grupos, suspender, reativar e redefinir a senha (com a mesma caixa) passam cada um pelo diálogo de confirmação compartilhado, e as recusas do servidor, como `last_operator`, aparecem sem alteração. A página de auditoria nomeia cada ação `account.*` nos dois idiomas.

### Tokens de agente

Uma pessoa com sessão aberta gera tokens de agente para os seus agentes, para que cada agente consulte os nós sob a concessão do seu dono em vez de um bearer compartilhado. `POST /api/v1/account/tokens` com `{"surface": "nodes", "hours": 24}` (de 1 a 168 horas, 8 quando `hours` falta) responde `{"token", "expires_at", "surface"}` uma vez só: a Meada não guarda token de agente nem o mostra de novo. O token leva a audiência dos nós (`meada:nodes`), o sujeito, o papel e os grupos como a linha da conta está agora, e um id aleatório. Uma sessão que precisa trocar a senha recebe o 403 `password_change_required`, e uma conta suspensa a recusa de papel (403 `forbidden`). Cada emissão fica registrada como `account.token` com a superfície, as horas e o id do token, nunca o token. A console recusa tokens de agente (401): um token que vive até sete dias e que nada revoga nunca faz escritas na console. A página de perfil oferece a superfície e a validade e depois mostra o token com um controle de cópia e a sua expiração; sair da página o apaga. Quando o manifesto de bootstrap da console tem `[proposals]`, `{"surface": "proposals"}` emite em vez disso um token de agente para o [servidor de propostas](named-workspaces.md#rodar-o-servidor-de-propostas) (audiência `meada:proposals`), que só esse servidor admite, e o `config` traz `"proposals": true` para o perfil oferecê-lo; sem a seção é um 422 `invalid_surface`.

Um nó com `[accounts]` e sem `[oidc]` admite esses tokens no `meada mcp --listen`. Ele lê a chave pública na partida (nunca a privada) e confere o emissor, a `[ui] public_url` da console, então o manifesto dele traz a seção `[ui]` da console junto de `[accounts]` e `[postgres]`, como o do job de atualização; uma chave ausente ou ilegível, ou a falta de `[ui]`, recusa a partida. Um token ausente, expirado, ainda não válido, assinado por outra chave ou que não seja EdDSA, um com `crit`, e um token de outra superfície (um token de sessão inclusive) recebem 401 com `WWW-Authenticate: Bearer` antes do roteamento. A concessão é o que `[accounts.groups]` dá aos grupos do token (todo repositório sem a tabela), e ela limita o roteamento (403 fora da concessão), as listagens 404 e toda resposta, pins e rotas de workspaces nomeados inclusive, como faz o token de um provedor de identidade. Trocar o par de chaves encerra todos os tokens em circulação de uma vez; sem isso, um token dura até expirar, mesmo depois de a conta ser suspensa. Configure um cliente MCP com o token como bearer:

```json
{"mcpServers": {"meada": {"type": "http", "url": "https://nodes.example/github.com/acme/web",
  "headers": {"Authorization": "Bearer <token de agente>"}}}}
```

### Login do GitHub verificado

Com `[accounts.github]`, o provedor de concessão do GitHub das contas locais, uma pessoa comprova o seu login do GitHub uma vez e os seus papéis no GitHub passam a lhe conceder repositórios, como o `login_claim` faz com um provedor de identidade. A seção tem as chaves de `[oidc.github]` sem `login_claim` (`host`, `roles`, `max_age_seconds`) e `api`, o endpoint REST do GitHub desse host (`https://api.github.com` por padrão em `github.com`, obrigatório em qualquer outro host, `https` ou `http` em loopback):

```toml
[accounts.github]
host = "github.com"
roles = ["write", "maintain", "admin"]
max_age_seconds = 5400
```

Na página de perfil, a pessoa cola um token de acesso pessoal do GitHub (um de granularidade fina sem nenhuma permissão basta). `POST /api/v1/account/github` com `{"token": "..."}` faz a console chamar `GET /user` em `api` uma vez só, guardar o login em minúsculas e descartar o token: ele nunca é guardado, registrado em log ou na auditoria, nem enviado a outro lugar. A resposta traz o login e um token de sessão novo que o carrega. Um login que outra conta já verificou é um 409 `login_taken`; a recusa do GitHub é um 422 `github_refused`, e o GitHub fora de alcance um 503 `github_unavailable`, nenhum dos dois repetindo a resposta do GitHub. `POST /api/v1/account/github/remove` remove o próprio login, e o operador global remove o de qualquer conta pela ação `remove_github` da página de contas. Os tokens de sessão e de agente levam o login verificado, e a concessão do GitHub, lida no conjunto de permissões que a checagem de papéis do job grava (`meada github-app roles` lê `[accounts.github]`), se junta à dos grupos na console, nos nós, nas adições de um operador de time (`added_by`) e na coluna `login` da auditoria. Um token emitido antes de uma remoção mantém o login até expirar. Cada verificação e remoção fica registrada como `account.github` (com o login) e `account.remove_github`.

### Break glass

Ao configurar `[oidc]`, o login local desliga para todas as contas: `[accounts]` junto de `[oidc]` é um erro, a menos que traga `break_glass = true`. Então só a conta padrão `admin` entra na console, como operador global, ao lado dos tokens do provedor de identidade, para quando ele está fora do ar:

```toml
[accounts]
break_glass = true
public_key_file = "/run/secrets/meada-signing.pub.pem"
private_key_file = "/run/secrets/meada-signing.pem"
```

`break_glass` sem `[oidc]` é um erro, e `[accounts.groups]` e `[accounts.github]` não podem ficar sob ele: a conta de break glass não precisa de concessão. Qualquer outro nome de usuário recebe o mesmo 401 de uma senha errada, e a auditoria registra `not_break_glass`. A console separa os bearers pelo emissor antes de tentar qualquer chave: um token cujo `iss` é a `public_url` da console só é conferido com a chave Ed25519 (e só vale o de `admin`), qualquer outro só com as chaves do provedor de identidade; por isso a console recusa a partida se o `issuer` de `[oidc]` for a sua própria `public_url`. A conta de break glass troca a própria senha; administração de contas, tokens de agente e verificação de login não existem (404). Nós e o servidor de propostas ignoram `[accounts]` quando há `[oidc]` e nunca leem a chave privada. `config` traz `"sign_in": "oidc"` com `"break_glass": true`, e o navegador, em vez de redirecionar direto ao provedor de identidade, mostra o botão dele e, discreto, o link para `/break-glass`, a página de entrada da conta padrão; o link continua lá quando o provedor de identidade não responde.

### Segurança das contas locais

- **Senhas:** guardadas com argon2id (19 MiB, duas iterações, uma lane; os parâmetros ficam no hash e um hash mais fraco é refeito no próximo login). Só um número pequeno de hashes roda ao mesmo tempo: o excedente recebe 503 `busy` sem calcular nada.
- **Bloqueio:** dez falhas seguidas bloqueiam a conta por quinze minutos; uma redefinição pelo operador global ou `meada ui reset-admin` remove o bloqueio.
- **Duração dos tokens:** o token de sessão vale oito horas; o de agente, de 1 a 168 horas (8 por padrão).
- **Sem lista de revogação:** a Meada não guarda tokens de agente, então a validade é a revogação. Suspender ou rebaixar uma conta barra as escritas dela na console na hora (a linha é lida a cada escrita) e a emissão de tokens novos, mas trocar a senha não invalida nada, e um token já emitido continua admitido nos nós e no servidor de propostas até expirar.
- **Rotação de chaves como parada de emergência:** troque o par Ed25519 e reinicie a console, os nós e o servidor de propostas: todo token em circulação, de sessão ou de agente, deixa de valer de uma vez. Não há rotação com chaves sobrepostas. Só a console lê a chave privada.
- **Token de acesso pessoal do GitHub:** lido uma vez, para obter o login em `GET /user`, e descartado; nunca é guardado, registrado em log ou na auditoria.
- **Limite de taxa no proxy:** a console não limita por endereço. Limite `POST /api/v1/login` (e, se quiser, `account/password`) no proxy, por exemplo com `limit_req_zone $binary_remote_addr zone=login:10m rate=5r/m;` e `limit_req zone=login burst=5;` no `location` do login no nginx. As falhas de nomes desconhecidos entram na auditoria e a fazem crescer sem limite, e o bloqueio por conta não as freia.
- **O que não existe:** e-mail, cadastro próprio, "esqueci a senha", autenticação multifator, lista de revogação, listagem de tokens emitidos, e apagar ou renomear contas. Quem perde a senha do `admin` usa `meada ui reset-admin`.

### Migrar das contas locais para o provedor de identidade

Ao configurar `[oidc]`, o login local desliga para todas as contas (veja [Break glass](#break-glass)). Passos:

1. Configure `[oidc]` na console e nos nós ([Autenticação e grants](admission.md)) e acrescente `break_glass = true` a `[accounts]`, para manter a conta padrão como saída de emergência. Remova `[accounts.groups]` e `[accounts.github]`: sob break glass elas são um erro.
2. Copie a tabela de `[accounts.groups]` para `[oidc.groups]`, trocando o nome de cada grupo local pelo valor que a claim `groups_claim` do provedor de identidade carrega (no Entra, o id do grupo). Os grupos locais e os do provedor são nomes independentes: o mapeamento é manual, e a concessão de um token passa a ser a dos grupos que o provedor emite. Os `owner` das entradas do workspace são nomes de grupo e continuam valendo se o provedor usar os mesmos nomes; senão, atualize-os.
3. Passe `[accounts.github]` para `[oidc.github]`: `host`, `roles` e `max_age_seconds` ficam, `api` sai, e o `login_claim` substitui o login verificado, que agora o administrador do provedor mapeia (veja [receitas](identity-providers.md)).
4. Quem era operador local passa a ser operador por `ui.operator_groups` ou `ui.operator_claim`.

O que deixa de funcionar: a entrada de qualquer conta que não seja `admin`; os tokens de sessão e de agente das contas locais (os nós e o servidor de propostas passam a admitir só os tokens do provedor, e os agentes passam a usar tokens do provedor de identidade); a página de contas, a emissão de tokens de agente e a verificação do login (404); e os sujeitos `local:<usuário>` registrados como autores, operadores de workspaces nomeados e registros de auditoria, que nunca coincidem com os do provedor, então refaça a lista de operadores de cada workspace nomeado. As contas continuam na tabela, inertes. Nenhuma conta ou grupo é migrado sozinho.

## Frota e diagnóstico

Cada `snapshot pull --every` envia um heartbeat, o registro periódico de estado do nó. Ele informa geração, fence, gerações locais, prontidão, versão e horário.

Use um `snapshot.node_name` único para cada nó e para a console, com até 63 letras minúsculas, dígitos, pontos ou hífens. Sem ele, o processo usa um hostname válido. Nomes de pods podem cumprir esse papel.

Um nó sem heartbeat não aparece na frota. Heartbeats malformados aparecem separados e não entram no diagnóstico. As páginas mostram frota, store, retenção, commits, schemas, cache de Apps e admissão; se faltar uma seção, a console informa qual.

## Papéis e operações

Com OIDC, leitores veem somente as identidades admitidas pelo token. Para conceder o papel de operador, escolha **uma** configuração: grupo em `ui.operator_groups` ou claim em `ui.operator_claim` com valor booleano `true`. Operadores veem toda a frota.

Com `[workspace] source = "store"`, `operations = true` e `oidc.groups_claim`, quem tem um grupo que é dono (`owner = "<grupo>"`) de uma entrada da versão atual do workspace que o seu token lê é operador de time. Vê o que um leitor com o mesmo grant vê e pode salvar e aplicar mudanças nas entradas do seu time; mudar qualquer outra entrada, um owner ou uma entrada sem owner é recusado com um 403 `entries_refused` que nomeia as entradas (um apply conta as que estão fora do grant). Pointer e fence continuam com o operador. Mapeie o grupo do time para as identidades dele em `[oidc.groups]`: ser dono nunca amplia o que um token lê. A página de versões mostra o time responsável por cada entrada a todos os papéis, e o editor do operador de time contém só as entradas que seu token lê, marca uma alteração que a API recusaria (com o motivo) antes de ela poder ser salva e nomeia os times que a autorizam nas confirmações, em vez de um comando da CLI.

As quatro operações HTTP exigem `operations = true` e um papel de operador, que vem do OIDC ou de uma [conta local](#contas-locais) (nunca só do proxy). Cada ação mostra o comando CLI e pede confirmação. Se recusada, mostra a mensagem do CLI, incluindo o titular de uma trava ocupada. Cada tentativa gera uma linha de auditoria em stderr, sem tokens nem bodies.

<details markdown="1" class="reference-details">
<summary>Grant, login e limites da recuperação</summary>

As operações da console exigem `[oidc]` ou `[accounts]` e ficam desligadas por padrão. Quando habilitadas, usam a mesma trava e as mesmas verificações do CLI para alterar o ponteiro, recuperar o fence e gerenciar versões. Cada tentativa gera uma linha de auditoria. O grant também limita grupos e erros do manifesto; o login preserva os parâmetros do issuer, e a recuperação respeita o relógio e o limite do contador do Valkey.

</details>

## Auditoria

Com `[postgres]` no bootstrap (e o schema em dia com `meada postgres migrate`), cada escrita da console e cada comando do CLI que a espelha (`meada workspace import` e `apply`, `meada snapshot pointer set`, `retain` e `drop`, `meada snapshot fence recover`) deixa um registro no Postgres: quem (sujeito, papel e, para um operador de time, os owners), o quê (ação e argumentos), quando (o relógio do Postgres) e o resultado (`pending`, `accepted`, `refused` ou `failed`). O registro é gravado antes de a escrita tocar o Valkey ou o store: se ele não puder ser gravado, a escrita é recusada e nada muda (a console responde 503 `audit_unavailable`, o CLI sai com 1 nomeando o Postgres). Recusas de papel e recusas sob a trava também são registradas; bodies inválidos (400, 413, 422) não. A console sobe com o Postgres fora do ar (as leituras continuam) e se recusa a subir com uma versão de schema que não conhece. O operador global lê os registros, do mais recente ao mais antigo, na página **Registro de auditoria** ou em `GET /api/v1/audit?before=<id>&limit=<1 a 100>`. A linha de auditoria em stderr não muda, com ou sem `[postgres]`, e a Meada nunca apaga um registro.

## Catálogo de repositórios

Com `[catalog]` no bootstrap (feature `workspaces`, junto de `[oidc.github]` ou `[accounts.github]`; veja [o catálogo](github-app.md#o-catalogo-de-repositorios)),
a console oferece o **Catálogo de repositórios** a todo sujeito admitido, e `GET /api/v1/config` traz `"catalog": true`.
É o seletor que vem antes de qualquer manifesto: lista só os repositórios que a pessoa pode selecionar, ou seja, que o
catálogo contém e nos quais o login dela tem um papel aceito no conjunto de permissões mais recente (o administrador da
plataforma vê o catálogo inteiro). Não cria workspace, lista de acesso nem índice.

- `GET /api/v1/catalog?limit=&after=&q=` responde `{"state", "reason", "set", "repositories": [{"identity"}], "next"}`,
  ordenado por identidade, com `limit` de 1 a 100 (50 por padrão), `q` um prefixo de até 128 caracteres e `after` o
  cursor opaco (até 256 caracteres) que `next` devolveu. Qualquer outro parâmetro, um repetido ou um valor fora do
  limite é um 400 com mensagem fixa.
- `POST /api/v1/catalog/check` recebe `{"identities": [...]}` (até 50, 16 KiB) e responde `{"state", "reason", "results":
  [{"identity", "state"}]}`, na ordem pedida: `selectable`, `unavailable` ou `pending`.
- `state` é `ready` ou `pending`. **Pendente** é uma condição da plataforma ou de quem pergunta, igual para toda
  identidade: `no_set` (o job ainda não gravou conjunto), `stale_set` (o conjunto passou de `max_age_seconds`, também
  para o administrador) ou `missing_login` (o token não traz um login válido). Pendente não lista nada e nada novo
  pode ser selecionado; as entradas que um manifesto já tem permanecem.
- `unavailable` é todo outro caso (sem papel aceito, fora do catálogo, não coberto, novo desde o último conjunto) e **não
  traz motivo**: um motivo seria um oráculo sobre o catálogo. A leitura nunca lista um repositório indisponível, e uma
  identidade que não existe responde igual a uma que a pessoa não pode selecionar.

As duas rotas exigem um sujeito admitido e não desativado (o bloqueio por SCIM vale aqui como em toda rota), e a
console as responde do conjunto que já tem em memória, sem chamar o GitHub nem o Postgres. `GET /api/v1/manifest` só
mostra `catalog.allow` ao operador global.

## Workspaces nomeados

Com `[workspaces]` no bootstrap (feature `workspaces`, junto de `[ui] operations = true`, `[oidc.github]` (ou `[accounts.github]`), `[catalog]` e `[workspace] source = "store"`; `mirrors` é a raiz absoluta onde ficam os espelhos dos repositórios que só workspaces nomeados selecionam, `max_workspaces` tem padrão 200 e `max_entries` 100), `GET /api/v1/config` traz `"workspaces": {"max_entries": n}` e a console oferece a página **Workspaces nomeados** a todo sujeito admitido.

- `POST /api/v1/workspaces` recebe `{"name", "description"?, "entries", "message"}` (80 KiB), sendo `entries` o texto do manifesto com entradas `[[repository]]` que têm `repo` e, no máximo, blocos `serve`, `resource` e `peer` (`path`, `owner`, `added_by` e `serve.documents` são recusados, 422 `invalid_entries`). O criador é o sujeito autenticado: `actor`, `role` ou qualquer outra chave no corpo é um 400. Responde 201 `{"id", "version", "created": true}`, ou 200 com `created: false` quando o mesmo criador, nome e entradas já o criaram. Uma seleção com repositório fora da elegibilidade da pessoa é um 403 `entries_refused` (`refused`: as identidades que a concessão da pessoa lê, `hidden`: quantas outras) e não grava nada; um conjunto que não consegue decidir é um 409 `pending` com `reason` (`no_set`, `stale_set`, `missing_login`). A criação é registrada na auditoria como `workspaces.create`, recusas incluídas.
- `GET /api/v1/workspaces?limit=&after=` lista os workspaces que o sujeito vê, ordenados por id (`limit` de 1 a 100, 25 por padrão), e `GET /api/v1/workspaces/<id>` mostra um com sua `base`, seu `state` (`applied`: gravado, ingestão pendente; `live`: a geração atual o registra) e a `ingestion` de cada entrada (`pending`, `ready` ou `excluded`). Um workspace é visível aos seus operadores, ao administrador da plataforma e a quem tem concessão que lê todos os seus repositórios; para os demais responde o mesmo 404 de um id que não existe. Os operadores também recebem a lista de operadores.
- O playground aceita `"workspace": "<id>"` e `GET /api/v1/workspaces/<id>/repositories/<identity>/export` devolve o export por esse workspace, sob a mesma regra e sempre limitado pela concessão de quem pergunta.
- O **ciclo de vida** (#343): `POST /api/v1/workspaces/<id>/versions` `{"entries", "message", "base"}` (80 KiB) salva uma versão sem mover nada (201, ou 200 `saved: false` quando a versão já estava guardada); `.../apply` `{"version", "base"}` (1 KiB) torna uma versão guardada a atual e responde `action` `apply` ou `rollback` (uma versão que já foi a atual) com a nova `base`; `.../rename` `{"name", "description"?, "base"}`; `.../delete` `{"base"}` (as versões e o histórico ficam); `.../operators` `{"add": [{"subject", "login"}], "remove": [subject], "base"}`, só do administrador da plataforma. `base` é a que `GET /api/v1/workspaces/<id>` devolve; uma base velha é um 409 `stale_base` que informa a base atual. Outras recusas: 403 `not_operator` ou `not_administrator`, 403 `entries_refused` (com `hidden`) para uma entrada para a qual a pessoa não é elegível, 409 `pending` quando o conjunto de permissões não consegue decidir, 409 `already_current`, 404 `unknown_version`, 422 `invalid_entries`, `invalid_workspace` ou `invalid_operators` (`last_operator`, `too_many_operators`); quem não pode ver o workspace recebe o 404 de um id desconhecido. Um operador envia as entradas que lê e a versão gravada mantém as que ele não lê; o administrador da plataforma envia o manifesto inteiro. Cada operação é um registro de auditoria, `workspaces.save`, `.apply`, `.rollback`, `.rename`, `.delete` ou `.operators`, recusas incluídas, e o bloqueio SCIM de um usuário apagado vale para todas.
- `GET /api/v1/workspaces/<id>/versions?after=` lista as versões (50 por página, da mais nova à mais antiga, a atual marcada; os operadores também veem o autor), `GET .../versions/<version>?against=<versão ou current>` mostra uma com suas entradas, seu `text` de manifesto e, com `against`, o diff (`added`, `removed`, `changed`), e `GET .../history?after=<seq>` lista o histórico, só de acréscimo, para os operadores e o administrador. Todas escondem o que a concessão de quem pergunta não lê: um visualizador vê só as versões que lê por inteiro, um operador as entradas que lê e uma contagem `hidden`.
- `meada workspaces list|show|versions|diff|create|save|apply|rename|delete|operators --workspace <manifesto de bootstrap>` são os gêmeos de linha de comando, que agem como administrador da plataforma com `cli:<login>@<host>` como ator e a mesma auditoria; toda escrita recebe a `--base` que leu.
- A **configuração efetiva** (#344): `GET /api/v1/workspaces/<id>/effective[?version=]` mostra, por entrada que quem chama lê, a `source` da configuração (`repository`: o `meada.toml` do mirror vale por inteiro; `context`: os blocos do workspace; `none`), os `blocks` resolvidos, os `ignored`, a `coverage` (`indexed`, `pending_ingestion`, `excluded`) e `diagnostics` (`ignored_by_repository`, `unknown_until_ingested`, `excluded_no_sponsor`, `peer_missing`, `documents_not_indexed`), com a `generation` de onde vêm os fatos, o `state` do workspace e `recorded` (se essa geração registrou o contexto na versão mostrada; se não, os blocos chegam às consultas depois do próximo refresh); `POST /api/v1/workspaces/effective` `{"entries"}` (80 KiB) mostra a mesma visão para um texto de manifesto antes de salvá-lo, não grava nem audita nada, e mostra como pendente um repositório que o grant de quem chama não lê. Uma criação, um salvamento e uma aplicação respondem com os mesmos `diagnostics` (`[{repo, code, detail?}]`) ao lado do resultado. A página de workspaces tem um editor de texto para o manifesto de um workspace que o sujeito opera: valida com o mesmo WebAssembly do servidor, mostra a configuração efetiva e então salva e aplica na base que leu.
- A **autoria guiada** (#345): a mesma página edita o manifesto por controles de formulário (repositórios do catálogo, configurações de serve, recursos, peers e nomes de variáveis de ambiente) que são um só documento com o texto: ambos passam pelo WebAssembly do `meada-core` (`namedEntriesJson`, `namedEntriesText`, `namedDiff`, `namedChanged`, `namedAuthorize`), então o formulário não acrescenta política. Mostra em texto a elegibilidade, a ingestão e a configuração efetiva de cada entrada, o diff do rascunho contra a versão gravada, e congela o conteúdo revisado (texto, mensagem, `base`) até *Salvar e aplicar* ser confirmado. Não precisa de rota nova: lê o catálogo (`GET /api/v1/catalog`, `POST /api/v1/catalog/check`) e usa as rotas acima; um `403 entries_refused` lê o catálogo de novo, um `409 stale_base` oferece recarregar e um `403 not_operator` retira o editor. Quem só vê não tem controle de alteração.

## Diretório (SCIM)

Com a feature `directory` (incluída em `cloud`), `[directory]` no bootstrap, junto de `[postgres]` e `[oidc]`, faz a console servir um endpoint SCIM 2.0 em `/scim/v2/`, para onde o provisionamento do Entra ID ou do Okta empurra usuários e grupos:

```toml
[directory]
token_env = "MEADA_SCIM_TOKEN"   # ou token_file; lido pela console ao subir
user_claim = "oid"
user_attribute = "externalId"    # ou "userName"
group_attribute = "externalId"   # "displayName" por padrão
```

O endpoint aceita só o próprio bearer token, com pelo menos 32 caracteres: a console não sobe com um token curto, ausente ou ilegível, e trocar o token exige reiniciá-la. Um token da console não serve ali, o token SCIM não serve em `/api/v1/`, e qualquer requisição com `Origin` recebe 403. Usuários e grupos ficam no schema do Postgres (rode `meada postgres migrate` antes), como o provedor os mandou, nunca com `password`; um corpo acima de 1 MiB ou um recurso acima de 64 KiB recebe 413. Apagar um usuário o tira dos grupos e deixa um marcador de revogação até ele ser provisionado de novo com `active` verdadeiro. Cada escrita deixa uma linha em stderr, `scim: <método> <recurso> <id>: <status>`, sem valores. A console lê o diretório a cada requisição: um usuário cujo valor de `user_claim` acha um usuário inativo (ou um marcador de revogação) recebe 403 `deprovisioned` em toda rota menos `GET /api/v1/config`, qualquer que seja o papel, e a console mostra uma página de conta desativada; um usuário nunca provisionado é admitido. Com o diretório ilegível, leituras são admitidas e escritas recebem 503 `audit_unavailable`. O editor do workspace oferece os grupos do diretório como owners só ao operador global, `owner_missing` marca em texto as entradas cujo owner não nomeia grupo (depois que algum grupo foi provisionado), e a página de admissão mostra as contagens de usuários e grupos e a última escrita SCIM.

## Times

Vários times podem dividir um workspace: cada um altera as próprias entradas e o operador global responde pelo resto. As regras numeradas abaixo são o contrato inteiro.

1. **Confiança compartilhada.** Ownership por time é delegação, não isolamento. Uma entrada que um time adiciona é buscada pelo job com as credenciais da frota (uma instalação de GitHub App ou as credenciais git do job) e lida por todo grant `*`. Aceite isso antes de delegar.
2. **A chave `owner`.** Uma entrada de versão do workspace pode ter `owner = "<grupo OIDC>"`: de 1 a 256 caracteres, sem caractere de controle nem espaço em branco nas pontas, comparada exatamente, com maiúsculas e minúsculas. Um grupo que o provedor envia com caractere de controle, ou com mais de 256 caracteres, não pode ser dono de uma entrada.

    ```toml
    [[repository]]
    repo = "github.exemplo.internal/acme/billing"
    path = "/var/lib/meada/mirrors/billing"
    owner = "team-billing"

    [[repository]]
    repo = "github.exemplo.internal/acme/api"
    path = "/var/lib/meada/mirrors/api"
    ```

    ```sh
    meada workspace import repositories.toml --message "Assign billing to its team" --workspace /etc/meada/job.workspace.toml
    ```

    Uma entrada também pode ter `added_by = "<login do GitHub>"`, o login de quem a adicionou: de 1 a 39 caracteres entre letras ASCII minúsculas, dígitos, `-` e `_` (um login de Enterprise Managed Users como `ana_acme` serve), comparado exatamente, então `Ana` é recusado. Ele vem depois do `owner` em `meada workspace show` e entra no id da versão. Com `[oidc.github]`, o salvamento de um operador de time no console define o `added_by` de cada adição no `host` do provedor de grant do GitHub como o login de quem salva, seja qual for o texto enviado (e o limpa nas adições fora dele); o refresh só busca a entrada enquanto esse login tiver um papel aceito, e as visões do workspace marcam `excluded` a entrada que a última verificação de papéis deixou de fora. Uma adição no `host` sem `added_by`, ou de um token sem login, é recusada com `unattributed`, e uma edição que muda, limpa ou define o `added_by` com `added_by_change`. O operador global e o CLI gravam o `added_by` como escrito: definir, limpar ou substituir é como ele adota a adição de um time. Nas entradas `[[repository]]` do próprio manifest de bootstrap é erro, como o `owner`.

3. **Quem altera o quê.** O operador de time pode editar e remover as entradas que seus times possuem e adicionar entradas cujo owner seja um de seus times. O operador global mantém as mudanças de owner (definir, limpar ou substituir), as entradas sem owner, as entradas de outros owners e todas as operações de geração (`pointer` e `fence`).
4. **Delegação.** Um grupo vira time quando o operador global lhe atribui a primeira entrada. Um time que remove a última entrada deixa de ser time até o operador global lhe atribuir uma entrada de novo.
5. **Configuração.** Ser dono nunca amplia o que um token lê. Mapeie o grupo de cada time em `[oidc.groups]` para as identidades que ele possui, e para uma identidade antes de o time poder adicioná-la; sem isso, o time não vê as próprias entradas, e uma adição fora do grant é recusada com `outside_grant`. Com `[oidc.github]`, uma adição no `host` do provedor de grant do GitHub dispensa esse mapeamento, mas uma identidade que o workspace já tem fora do grant ainda é recusada com `outside_grant` ao salvar, o que revela ao time que ela existe.

    ```toml
    [oidc.groups]
    "meada-operators" = ["*"]
    # Um time: mapeie o grupo para as identidades que ele possui.
    "team-billing" = ["github.exemplo.internal/acme/billing"]
    ```

6. **Manifests estáticos.** `owner` nas entradas `[[repository]]` do próprio manifest de bootstrap é erro: um manifest estático não tem editor para delegar. Use `[workspace] source = "store"` e importe as entradas.
7. **CLI.** `meada workspace import` e `apply` agem como operador global: guardam as credenciais do store e do Valkey, que já permitem toda escrita.
8. **Apply só de owners.** A indexação nunca lê `owner`, então aplicar uma versão que só muda owners não gera nova geração. A visão da frota continua nomeando a versão que produziu primeiro a geração servida; isso não é um refresh travado.
9. **Posição.** Os próprios saves do operador de time nunca movem uma entrada (a versão salva mantém no lugar as entradas fora do grant). Ele pode aplicar uma versão armazenada cuja única diferença seja a ordem das entradas.
10. **Um repositório com falha.** Pela regra de aborto da publicação inteira, o repositório com falha de um time (credencial revogada, remoto apagado, falha de parse) impede novas gerações para todos os times até a causa ser reparada. Os nós seguem servindo a última geração completa. Veja [Refresh](refresh.md).

## Operações pelo CLI

```sh
meada snapshot pointer set <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot pointer retain <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot pointer drop <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot fence recover --workspace /etc/meada/console.workspace.toml
meada snapshot fence recover --at-least <fence> --workspace /etc/meada/console.workspace.toml
```

| Operação | Efeito |
| --- | --- |
| `set` | Seleciona uma geração retida. O próximo publish pode substituí-la; suspenda o job para manter o rollback. |
| `retain` | Protege uma geração da poda. O conjunto aceita até oito gerações. |
| `drop` | Remove essa proteção. A geração atual não pode ser removida. |
| `fence recover` | Eleva o contador ao maior fence registrado, ou ao piso indicado, sob a trava. O próximo publish usa um fence maior. |

Se o fence de um nó isolado não aparece em heartbeat válido, informe o valor com `--at-least` ou remova o `local_dir/current` desse nó para que receba a geração atual. Não edite o contador manualmente.
