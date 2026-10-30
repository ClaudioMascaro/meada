# Identidade empresarial

A automação de identidade ([S4, #319](https://github.com/ClaudioMascaro/meada/issues/319), [ADR 0016](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0016-identity-automation.md)) está entregue. Ela soma ao [modo servidor](server.md) e ao [console](console.md) uma camada de dados em Postgres, uma auditoria, os tipos de provedor de identidade, o grant do GitHub, as adições de times, um diretório SCIM e o overage do Entra ID. Cada recurso é opt-in: sem as novas seções, todo processo se comporta como antes. As [receitas](identity-providers.md) de Entra ID, Okta e Keycloak mostram a configuração do lado do provedor.

O provedor de identidade emite o token. O grant efetivo reúne as identidades permitidas pelas claims e, quando configurado, pelas permissões do GitHub. A consulta de papéis no GitHub cabe ao job de refresh, nunca ao console, aos nós ou à indexação. Owners e papéis humanos não ampliam esse grant. Workspaces nomeados, a criação de contextos derivada dos papéis efetivos de repositório (diretos ou por grupo) e o acesso de viewer herdado sem segunda aprovação são o [S5, #339](named-workspaces.md), entregue com o [catálogo de repositórios](github-app.md#o-catalogo-de-repositorios), a autoria do console e as propostas MCP; `role_name` do GitHub não identifica um time.

<!-- meada-graph:enterprise -->

Esta página segue a ordem de uma implantação: camada de dados, auditoria, tipo do provedor, grant do GitHub, adições, diretório, overage e limites.

## Camada de dados

Postgres 15 ou mais novo guarda o estado mutável durável: auditoria, diretório e permissões. O store continua guardando documentos e bundles imutáveis, e o Valkey ponteiros, travas, fences e caches. Habilite a feature `postgres` (incluída em `cloud`) e a seção `[postgres]` do manifesto de bootstrap:

```toml
[postgres]
url_env = "MEADA_POSTGRES_URL"      # padrão; ou url_file = "/run/secrets/postgres-url"
ca_file = "/etc/meada/ca/ca.pem"    # opcional: valida só contra este arquivo
schema = "meada"                    # padrão; minúsculas, dígitos e _, 1 a 63 caracteres
pool_size = 4                       # padrão
connect_timeout_seconds = 5         # padrão
statement_timeout_seconds = 10      # padrão
```

- **A URL** vem de `url_env` ou de `url_file` (no máximo um), nunca do manifesto, que continua sem segredos. É `postgres://` ou `postgresql://` com host TCP, e pode levar usuário, senha, porta e banco, mas nenhum parâmetro de query: `sslmode` e os demais são erros, porque o TLS é decisão do Meada. Um erro nomeia a chave, nunca a URL.
- **TLS.** Um host fora de loopback (`localhost`, `127.0.0.0/8`, `::1`) sempre usa TLS com o nome do host verificado; só o loopback usa TCP simples (a menos que `ca_file` esteja definido). Sem `ca_file`, valem as raízes do sistema; com ele, só as do arquivo.
- **O schema** `[postgres] schema` contém todas as tabelas, e o Meada não escreve fora dele; vários deployments podem dividir um banco com schemas diferentes.
- **Migrações.** Só `meada postgres migrate --workspace <manifesto de bootstrap>` cria ou altera uma tabela. Ele cria o schema se faltar, aplica cada migração em sua transação sob um advisory lock do schema (duas execuções se serializam) e imprime a versão alcançada. Rode-o **antes de cada atualização do binário**. Qualquer outro comando que conecta lê a versão: uma versão menor que a do binário é o erro que nomeia `meada postgres migrate`, uma maior nomeia as duas versões. A console e os comandos do CLI recusam a subida ou saem com 1; um nó escreve o erro uma vez no stderr e trata o Postgres como inalcançável, sem mudar a prontidão.
- **Três papéis do Postgres** são recomendados: `migrate` (dono do schema), um que escreve (a console, a checagem de papéis do job e os gêmeos do CLI) e um que lê (os nós). Exemplo, com o banco `meada` e os papéis já criados:

    ```sql
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT USAGE ON SCHEMAS TO meada_write, meada_read;
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO meada_write;
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT USAGE, SELECT ON SEQUENCES TO meada_write;
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT SELECT ON TABLES TO meada_read;
    ```

- **Quem conecta:** a console (auditoria, diretório, permissões, overage), os nós com `[oidc.github]` ou overage pelo diretório (só leitura), `meada github-app roles` e os gêmeos do CLI que escrevem. `meada index` nunca conecta.
- **Backups e disponibilidade são do operador**, como no Valkey e no store. Perder o banco custa a única cópia da auditoria (o stderr continua); o diretório volta no próximo ciclo completo do provedor de identidade, e as permissões, no próximo refresh.
- **Janela de atualização.** Entre o `meada postgres migrate` e o momento em que todo processo roda o binário novo, os processos antigos perdem a camada de dados: as escritas falham na auditoria e os nós deixam o grant do GitHub envelhecer.
- `meada status --workspace` com `[postgres]` imprime `postgres: reachable, schema <nome> version <n>` ou `postgres: unreachable (<motivo>)`, sem mudar o código de saída.

A [composição do Compose](https://github.com/ClaudioMascaro/meada/tree/main/deploy/compose) e o [chart do Helm](https://github.com/ClaudioMascaro/meada/tree/main/deploy/helm/meada) trazem um Postgres e o passo de migração prontos, desligados por padrão.

## Auditoria

Com `[postgres]`, cada escrita da console e cada gêmeo do CLI deixa um registro: `workspace.save`, `workspace.import`, `workspace.apply`, `pointer.set`, `pointer.retain`, `pointer.drop` e `fence.recover` (mais `pointer`, só para uma recusa de papel antes da leitura do corpo, com argumentos `{}`). O registro traz `id`, `time` (o relógio do Postgres), `via` (`console` ou `cli`), `subject`, `login` quando há, `role` (`viewer`, `operator` ou `team_operator`), `owners` de um operador de time, `action`, `arguments`, `outcome` (`pending`, `accepted`, `refused` ou `failed`) e `message`. Recusas também são registradas. O publish do job, `meada github-app roles` e as escritas SCIM não são auditados: cada um deixa o seu próprio registro.

A auditoria **falha fechada**: o registro `pending` é gravado antes de a escrita tocar o Valkey ou o store, e, se não puder ser gravado, a escrita é recusada e nada muda (a console responde 503 `audit_unavailable`; o CLI sai com 1 nomeando o Postgres). Uma queda do Postgres para as escritas, nunca as leituras. O operador global lê os registros na página **Registro de auditoria** do console ou em `GET /api/v1/audit?before=<id>&limit=<1 a 100>`. A linha de auditoria em stderr não muda. A Meada nunca apaga um registro: a poda é do operador, em SQL, por exemplo `DELETE FROM meada.audit WHERE time < now() - interval '1 year'`. Mais em [Console](console.md#auditoria).

## Provedor de identidade

`[oidc]` é o provedor de identidade, o único emissor de tokens do deployment; todas as suas chaves de hoje continuam iguais. `type` diz qual é: `generic` (o padrão), `entra`, `okta` ou `keycloak`. `okta` e `keycloak` se comportam como `generic` e só nomeiam a [receita](identity-providers.md); `entra` tem a tabela `[oidc.entra]` com uma chave, `overage` (`ignore`, o padrão, ou `directory`; veja [Overage do Entra](#overage-do-entra)). `[oidc.entra]` sob outro `type` é erro. Veja [Autenticação e grants](admission.md#configurar-o-oidc).

## Grant do GitHub

`[oidc.github]` torna os papéis do GitHub uma segunda fonte do grant, somada ao das claims: o que o login do GitHub de um sujeito lê no GitHub ele lê no Meada.

```toml
[oidc.github]
host = "github.com"
login_claim = "github_login"       # a claim do token com o login do GitHub
roles = ["read", "triage", "write", "maintain", "admin"]   # o padrão; papéis customizados também
max_age_seconds = 5400             # obrigatório, pelo menos 60
```

- **O login** vem da claim `login_claim` (cada provedor de identidade a mapeia; veja as [receitas](identity-providers.md)). Só vale uma string de 1 a 39 letras ASCII, dígitos, `-` e `_`, comparada em minúsculas. Uma claim ausente ou malformada não dá grant do GitHub, e o grant das claims continua. Quem administra o provedor de identidade controla esse mapeamento: use um atributo verificado, nunca um que o usuário edita.
- **Papéis aceitos** são os `role_name` do GitHub, comparados exatamente, com maiúsculas e espaços; papéis customizados de repositório entram na lista. O padrão são os cinco papéis nativos.
- **`max_age_seconds`** é o "intervalo de um refresh": o agendamento do job mais a sua execução mais longa. Os processos honram o conjunto de permissões mais novo por esse tempo depois do seu `finished`, no relógio do Postgres; depois, o grant do GitHub fica vazio, seja porque o job parou, seja porque o Postgres ficou inalcançável.
- **O refresh do job** passa a ser, sob `meada snapshot lock`: `meada workspace pull`, `meada github-app roles`, o fetch dos espelhos que `meada workspace show --pulled` lista (um script de fetch lê essa saída), `meada index --workspace` e `meada snapshot publish`.

    ```sh
    meada snapshot lock --workspace job.workspace.toml -- sh -ec '
      meada workspace pull --workspace job.workspace.toml
      meada github-app roles --workspace job.workspace.toml
      meada workspace show --pulled --workspace job.workspace.toml > pulled.toml
      sh fetch-pulled.sh pulled.toml
      meada index --workspace job.workspace.toml
      meada snapshot publish --workspace job.workspace.toml'
    ```

- **Uma checagem de papéis que falha interrompe o refresh.** `roles` sai com 1 sem gravar conjunto (todas as instalações limitadas, 5xx, timeout, Postgres inalcançável), o conjunto anterior continua e envelhece até os nós o recusarem, e nenhuma geração é publicada com papéis que o job não pôde ler. Ela também remove o arquivo de exclusões, então `index`, `status` e `snapshot publish` recusam até uma checagem dar certo: a recuperação é rodar `meada github-app roles` de novo (uma execução que um fence mais novo recusa deixa o arquivo como está). Um repositório que o GitHub não mostra (404, ou nenhuma instalação cobre o owner) não dá linhas e um aviso, e o refresh segue. Isso não é uma falha de fetch do [#316](https://github.com/ClaudioMascaro/meada/issues/316): uma exclusão por elegibilidade é uma decisão antes do fetch.
- **A sessão** mostra `login`, `github_host` e `github_grant` (`current`, `expired` ou `unread`), e a verificação de token separa as identidades em `sources`.
- A medição contra uma organização real, com times e papel base, é o [#338](https://github.com/ClaudioMascaro/meada/issues/338); até lá, o custo por refresh está nos [limites conhecidos](#limites-conhecidos).

Detalhes em [Credenciais de GitHub App](github-app.md#checagem-de-papeis).

## Adições sob o provedor

Sob `[oidc.github]`, um operador de time pode adicionar, para o seu time, uma entrada de um repositório do `host` que o seu grant ainda não lê, sem mapeamento em `[oidc.groups]`.

- **`added_by`** é uma chave opcional da entrada: o login do GitHub de quem a adicionou (1 a 39 caracteres entre letras minúsculas, dígitos, `-` e `_`). Entra no id da versão e é erro em uma entrada do próprio manifesto de bootstrap.
- **Quem a define.** O salvamento de um operador de time define o `added_by` de cada adição no `host` como o login de quem salva, seja qual for o texto enviado, e o limpa nas demais. O operador global e o CLI gravam o valor como escrito: assim o operador global adota a adição de um time (define ou substitui o `added_by`, ou o limpa); suas próprias adições não levam `added_by` e são buscadas como antes.
- **Recusas.** `unattributed`: uma adição no `host` sem `added_by`, ou de um token sem login. `added_by_change`: uma edição que muda, limpa ou define o `added_by` de uma entrada. Elas entram em `entries_refused` na ordem `outside_grant`, `unattributed`, `ownerless`, `owner_change`, `added_by_change`, `not_your_team`.
- **O job** só busca, indexa e publica a entrada enquanto o `added_by` tem um papel aceito. A entrada excluída fica ausente do workspace; a console a marca com `excluded: true` nas visões do workspace, para quem a vê, e o stderr do job nomeia a identidade e o motivo, nunca o login.
- **`outside_grant` revela.** Uma identidade que o workspace já tem fora do grant do time é recusada com `outside_grant` ao salvar. Isso mostra ao time que ela existe, e o editor não consegue prever essa recusa (veja os [limites](#limites-conhecidos)).

Mais em [Console, Times](console.md#times).

## Diretório

`[directory]` (feature `directory`, incluída em `cloud`; exige `[postgres]` e `[oidc]`) liga um endpoint SCIM 2.0 na console, alimentado pelo provedor de identidade:

```toml
[directory]
token_env = "MEADA_SCIM_TOKEN"   # ou token_file; lido pela console ao subir
user_claim = "oid"               # a claim do token que identifica o usuário
user_attribute = "externalId"    # ou "userName"
group_attribute = "externalId"   # "displayName" por padrão
```

- **A URL SCIM** é `<URL pública da console>/scim/v2/`, com `Authorization: Bearer <token SCIM>`. O token tem pelo menos 32 caracteres; a console não sobe com um token curto, ausente ou ilegível, e **trocar o token exige reiniciá-la**. Nenhum token da console serve ali, o token SCIM não serve em `/api/v1/`, e uma requisição com `Origin` recebe 403.
- **Desprovisionamento só na console.** Um usuário cujo `user_claim` acha um usuário inativo, ou excluído e não provisionado de novo com `active` verdadeiro, recebe 403 `deprovisioned` em toda rota menos `GET /api/v1/config`. A exclusão SCIM de um sujeito provisionado deixa um bloqueio durável; um sujeito nunca provisionado segue a admissão normal. Os nós não verificam isso: o token vivo de um usuário desprovisionado lê até expirar.

<!-- meada-graph:scim-revocation -->

- **O seletor de owners.** `GET /api/v1/directory/groups?prefix=<texto>` (só o operador global) lista até 100 grupos, e o editor os oferece ao editar um `owner`. Depois que algum grupo foi provisionado, uma entrada cujo `owner` não nomeia grupo aparece com `owner_missing: true`.

Mais em [Console, Diretório](console.md#diretorio-scim).

## Overage do Entra

Um usuário do Entra ID em mais grupos do que um token carrega recebe `_claim_names` no lugar de `groups`. Com `type = "entra"`, `[oidc.entra] overage = "directory"` dá a esse token os grupos que o diretório conhece: os `group_attribute` dos grupos de que o usuário ativo (o `user_attribute` igual ao `user_claim`) é membro. Por isso exige `[directory]` e `groups_claim`: o Meada nunca chama o endpoint da fonte (Microsoft Graph), e só o diretório que o provedor empurra pode responder. Os grupos substituem a claim ausente em todo lugar em que ela é lida (grant, operadores, times, sessão). O nó ou a console consulta o token uma vez e guarda a resposta por `OVERAGE_TTL`. Com `overage = "ignore"`, o padrão, um token em overage não tem grupos.

## Limites conhecidos

- **Custo do GitHub por refresh:** uma listagem por repositório do `host`, `ceil(colaboradores / 100)` requisições cada. Um token de instalação recebe 5.000 requisições por hora, mais 50 por repositório ou usuário acima de 20, no máximo 12.500 (15.000 no GitHub Enterprise Cloud), e 900 pontos por minuto de REST nos limites secundários. Com um papel base da organização, todo membro é colaborador de todo repositório: 1.000 repositórios e 2.000 membros dão 20.000 requisições por refresh, além da hora de uma instalação. Repartir por instalações só funciona entre owners diferentes. Passado o orçamento, uma listagem é limitada, `roles` sai com 1 e nenhum refresh publica até o orçamento voltar, para todos os times. O caminho de evolução, dimensionado pela medição do #338: quando o papel base da organização é aceito, listar seus membros uma vez e, por repositório, só os colaboradores que não vêm do papel base.
- **Latência de revogação:** uma revogação no GitHub chega ao Meada no próximo refresh bem-sucedido mais, no máximo, `PERMISSIONS_POLL` (60 segundos); sem refresh bem-sucedido, no máximo `max_age_seconds` depois do último.
- **Memória:** um processo guarda todas as linhas de papel aceito do conjunto mais novo (logins vezes repositórios, sob um papel base).
- **`OVERAGE_TTL`** (5 minutos): uma mudança de membros chega a um sujeito em overage com esse atraso.
- **Token vivo:** o token de um usuário desprovisionado continua lendo nos nós até expirar.
- **`outside_grant`** diz a um operador de time que a identidade já está no workspace, e o editor não prevê isso.
- **Um host do GitHub** por deployment; um login tem no máximo 39 caracteres e um nome de papel, 100.
- **SCIM:** o token é lido ao subir (trocar exige reiniciar a console); corpos de até `SCIM_BODY_MAX` (1 MiB), um recurso armazenado de até `SCIM_RESOURCE_MAX` (64 KiB) e páginas de até 100.
- **A auditoria cresce sem limite** até o operador podá-la.
- **Postgres 15 ou mais novo.** Do `meada postgres migrate` até todo processo rodar o binário novo, os processos antigos perdem a camada de dados: as escritas falham na auditoria e os nós deixam o grant do GitHub envelhecer.
