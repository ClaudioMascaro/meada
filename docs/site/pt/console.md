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

As quatro operações HTTP exigem OIDC e `operations = true`. Cada ação mostra o comando CLI e pede confirmação. Se recusada, mostra a mensagem do CLI, incluindo o titular de uma trava ocupada. Cada tentativa gera uma linha de auditoria em stderr, sem tokens nem bodies.

<details markdown="1" class="reference-details">
<summary>Grant, login e limites da recuperação</summary>

As operações da console exigem OIDC e ficam desligadas por padrão. Quando habilitadas, usam a mesma trava e as mesmas verificações do CLI para alterar o ponteiro, recuperar o fence e gerenciar versões. Cada tentativa gera uma linha de auditoria. O grant também limita grupos e erros do manifesto; o login preserva os parâmetros do issuer, e a recuperação respeita o relógio e o limite do contador do Valkey.

</details>

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
