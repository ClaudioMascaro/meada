# Console de gerenciamento

A console usa a feature `ui`, incluída em `cloud`, e roda em um processo próprio. Ela tem editor, diff, playground e grafo. O editor salva versões de configuração; aplicar uma versão e publicar os índices são etapas separadas.

Proteja o acesso antes de expor a console. Sem `[oidc]`, o proxy controla a admissão: quem alcançar a console fora dessa proteção poderá ler o grafo. O proxy deve preservar `Host`, `Origin` e `Authorization`. A console aceita apenas seu Host público e, se houver Origin, sua própria origem.

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

## Frota e diagnóstico

Cada `snapshot pull --every` envia um heartbeat, o registro periódico de estado do nó. Ele informa geração, fence, gerações locais, prontidão, versão e horário.

Use um `snapshot.node_name` único para cada nó e para a console, com até 63 letras minúsculas, dígitos, pontos ou hífens. Sem ele, o processo usa um hostname válido. Nomes de pods podem cumprir esse papel.

Um nó sem heartbeat não aparece na frota. Heartbeats malformados aparecem separados e não entram no diagnóstico. As páginas mostram frota, store, retenção, commits, schemas, cache de Apps e admissão; se faltar uma seção, a console informa qual.

## Papéis e operações

Com OIDC, leitores veem somente as identidades admitidas pelo token. Para conceder o papel de operador, escolha **uma** configuração: grupo em `ui.operator_groups` ou claim em `ui.operator_claim` com valor booleano `true`. Operadores veem toda a frota.

Com `[workspace] source = "store"`, `operations = true` e `oidc.groups_claim`, quem tem um grupo que é dono (`owner = "<grupo>"`) de uma entrada da versão atual do workspace que o seu token lê é operador de time. Vê o que um leitor com o mesmo grant vê e pode salvar e aplicar mudanças nas entradas do seu time; mudar qualquer outra entrada, um owner ou uma entrada sem owner é recusado com um 403 `entries_refused` que nomeia as entradas (um apply conta as que estão fora do grant). Pointer e fence continuam com o operador. Mapeie o grupo do time para as identidades dele em `[oidc.groups]`: ser dono nunca amplia o que um token lê. A página de versões mostra o time responsável por cada entrada a todos os papéis, e o editor do operador de time contém só as entradas que seu token lê, marca uma alteração que a API recusaria (com o motivo) antes de ela poder ser salva e nomeia os times que a autorizam nas confirmações, em vez de um comando da CLI.

As quatro operações HTTP exigem OIDC e `operations = true`. Cada ação mostra o comando CLI e pede confirmação. Se recusada, mostra a mensagem do CLI, incluindo o titular de uma trava ocupada. Cada tentativa gera uma linha de auditoria em stderr, sem tokens nem bodies.

<details markdown="1" class="reference-details">
<summary>Grant, login e limites da recuperação</summary>

As operações da console exigem OIDC e ficam desligadas por padrão. Quando habilitadas, usam a mesma trava e as mesmas verificações do CLI para alterar o ponteiro, recuperar o fence e gerenciar versões. Cada tentativa gera uma linha de auditoria. O grant também limita grupos e erros do manifesto; o login preserva os parâmetros do issuer, e a recuperação respeita o relógio e o limite do contador do Valkey.

</details>

## Auditoria

Com `[postgres]` no bootstrap (e o schema em dia com `meada postgres migrate`), cada escrita da console e cada comando do CLI que a espelha (`meada workspace import` e `apply`, `meada snapshot pointer set`, `retain` e `drop`, `meada snapshot fence recover`) deixa um registro no Postgres: quem (sujeito, papel e, para um operador de time, os owners), o quê (ação e argumentos), quando (o relógio do Postgres) e o resultado (`pending`, `accepted`, `refused` ou `failed`). O registro é gravado antes de a escrita tocar o Valkey ou o store: se ele não puder ser gravado, a escrita é recusada e nada muda (a console responde 503 `audit_unavailable`, o CLI sai com 1 nomeando o Postgres). Recusas de papel e recusas sob a trava também são registradas; bodies inválidos (400, 413, 422) não. A console sobe com o Postgres fora do ar (as leituras continuam) e se recusa a subir com uma versão de schema que não conhece. O operador global lê os registros, do mais recente ao mais antigo, na página **Registro de auditoria** ou em `GET /api/v1/audit?before=<id>&limit=<1 a 100>`. A linha de auditoria em stderr não muda, com ou sem `[postgres]`, e a Meada nunca apaga um registro.

## Diretório (SCIM)

Com a feature `directory` (incluída em `cloud`), `[directory]` no bootstrap, junto de `[postgres]` e `[oidc]`, faz a console servir um endpoint SCIM 2.0 em `/scim/v2/`, para onde o provisionamento do Entra ID ou do Okta empurra usuários e grupos:

```toml
[directory]
token_env = "MEADA_SCIM_TOKEN"   # ou token_file; lido pela console ao subir
user_claim = "oid"
user_attribute = "externalId"    # ou "userName"
group_attribute = "externalId"   # "displayName" por padrão
```

O endpoint aceita só o próprio bearer token, com pelo menos 32 caracteres: a console não sobe com um token curto, ausente ou ilegível, e trocar o token exige reiniciá-la. Um token da console não serve ali, o token SCIM não serve em `/api/v1/`, e qualquer requisição com `Origin` recebe 403. Usuários e grupos ficam no schema do Postgres (rode `meada postgres migrate` antes), como o provedor os mandou, nunca com `password`; um corpo acima de 1 MiB ou um recurso acima de 64 KiB recebe 413. Apagar um usuário o tira dos grupos e deixa um marcador de revogação até ele ser provisionado de novo com `active` verdadeiro. Cada escrita deixa uma linha em stderr, `scim: <método> <recurso> <id>: <status>`, sem valores. Ainda nada lê o diretório.

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

    Uma entrada também pode ter `added_by = "<login do GitHub>"`, o login de quem a adicionou: de 1 a 39 caracteres entre letras ASCII minúsculas, dígitos, `-` e `_` (um login de Enterprise Managed Users como `ana_acme` serve), comparado exatamente, então `Ana` é recusado. Ele vem depois do `owner` em `meada workspace show`, entra no id da versão, e nada o lê ainda para decidir algo. Nas entradas `[[repository]]` do próprio manifest de bootstrap é erro, como o `owner`.

3. **Quem altera o quê.** O operador de time pode editar e remover as entradas que seus times possuem e adicionar entradas cujo owner seja um de seus times. O operador global mantém as mudanças de owner (definir, limpar ou substituir), as entradas sem owner, as entradas de outros owners e todas as operações de geração (`pointer` e `fence`).
4. **Delegação.** Um grupo vira time quando o operador global lhe atribui a primeira entrada. Um time que remove a última entrada deixa de ser time até o operador global lhe atribuir uma entrada de novo.
5. **Configuração.** Ser dono nunca amplia o que um token lê. Mapeie o grupo de cada time em `[oidc.groups]` para as identidades que ele possui, e para uma identidade antes de o time poder adicioná-la; sem isso, o time não vê as próprias entradas, e uma adição fora do grant é recusada com `outside_grant`.

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
