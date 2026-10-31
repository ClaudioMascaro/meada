# Meada no Kubernetes

[English](README.en.md) · [Español](README.es.md)

Configure a admissão no Ingress ou no proxy de autenticação antes de expor o serviço.
Sem `[oidc]`, esse componente admite os clientes; uma configuração incorreta expõe o grafo
de código. Com OIDC, preserve `Authorization`. Preserve também os headers SEP-2243
`MCP-Protocol-Version`, `MCP-Session-Id`, `Mcp-Method`, `Mcp-Name` e `Mcp-Param-*`,
devolva `Meada-Generation` e nunca acrescente `Origin` nas rotas MCP.
O chart não fixa annotations de um controller: configure a autenticação do seu Ingress.

## Instalação

O chart é publicado junto das releases em `oci://ghcr.io/claudiomascaro/charts/meada`.
Escolha a mesma versão do chart e da Meada:

```sh
helm upgrade --install meada oci://ghcr.io/claudiomascaro/charts/meada --version <versão> -f meus-valores.yaml
```

Para usar o checkout, substitua a referência OCI por `deploy/helm/meada`.
Os valores padrão criam dois nós sem módulos cloud. Cada pod faz o fetch de seus próprios
mirrors e a indexação em um init container, usando seu próprio `emptyDir`; só então o servidor
inicia. Ajuste `workspace.repositories` (TOML) e `workspace.fetch` (script executado fora da
Meada). `workspace.gitCredentialsSecret` permite passar variáveis de um Secret ao fetch.
Nada executa fetch ou acessa a rede dentro de `meada index`. O `workspace.fetch` padrão lê os
repositórios de `$1` quando recebido (o arquivo de `meada workspace show --pulled`, que o refresh
passa com `modules.oidc.github`), senão de `workspace.repositories` (montado como
`repositories.toml`, também com `workspace.source: store`, cujo bootstrap não lista nenhum);
recusa um repositório com credencial na URL, e um mirror cujo `origin` traz uma (rode `git remote
set-url origin <URL sem credenciais>` nele); e, se um fetch falha, imprime `refresh: fetch failed
for <identidade>; the refresh stops before publication` (sem URL nem token) e sai com erro, antes
de `index` e `publish`.

## Gerações

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-generations.yaml -f meus-valores.yaml
```

Defina endpoints externos `https://` para o store e `rediss://` para Valkey. Crie os Secrets
`meada-valkey` (chave `password`) e `meada-store` (chaves `access-key-id`, `secret-access-key` e,
opcionalmente, `session-token`), ou configure os nomes em `modules`. O usuário AUTH fica na URL
de Valkey; a senha vem somente do Secret. Para uma CA privada, configure `tls.caSecret` ou
`tls.caConfigMap`, com a chave `ca.crt`. O chart não cria esses serviços nem guarda credenciais
nos manifests ou nos valores.

Cada pod faz um pull inicial e mantém um companion `pull --every 30` no mesmo `local_dir`,
que pertence apenas àquele pod. O CronJob tem `concurrencyPolicy: Forbid`, mirrors e index_dir
próprios e executa fetch, index e publish dentro de `meada snapshot lock`. Não compartilhe os
diretórios de índices entre pods. A readiness depende do estado local, com `meada status`,
ou de TCP quando `readiness.type=tcp`.

`modules.snapshot.credentials` aceita `static`, `web-identity` ou `pod-identity`. Para IRSA,
configure a annotation da role em `serviceAccount.annotations`; o webhook da plataforma
injeta `AWS_ROLE_ARN` e `AWS_WEB_IDENTITY_TOKEN_FILE`. Para Pod Identity, associe a service
account à role na plataforma. Não há suporte a IAM AUTH do ElastiCache.

Habilite OIDC em `modules.oidc.enabled`, com issuer, publicUrl e os claims/grupos do provedor.
O módulo desligado não produz seção TOML. `image.cloud=true` seleciona a tag `-cloud`;
`image.tag` vazio segue `appVersion`. Uma seção cloud exige a imagem cloud.

## Operação e validação

O Service é ClusterIP, sem afinidade de sessão. A carência padrão é 40 s, incluindo o preStop
de 5 s e o drain de até 25 s. Ajuste `resources`, `nodeSelector`, `tolerations` e `affinity`
conforme o ambiente. Uma alteração de ConfigMap faz rollout pelo checksum do manifest.

```sh
helm lint deploy/helm/meada
helm template meada deploy/helm/meada -f deploy/helm/meada/values-generations.yaml
python3 scripts/helm-check
```

`helm-check` usa Python 3.11+ e PyYAML. O CI valida os templates, sem criar cluster.
O smoke local usa um cluster kind descartável, a imagem default carregada e um repositório
de teste criado no init container, depois consulta `search` por port-forward.

## Console opcional e workspace gerenciado

A console fica desligada por padrão e não produz recursos até `console.enabled=true`.
O perfil abaixo habilita cloud, módulos de gerações, refresh lock, workspace gerenciado e OIDC.
Edite `meus-valores.yaml` com endpoints, Secrets, política OIDC dos nós e os valores próprios
`console.publicUrl`, `clientId`, `audience`, `operations`, `operatorGroups` ou `operatorClaim`.
Escolha apenas um seletor de operador; operações ficam desligadas até habilitadas explicitamente.
A origem/audience da console não substituem `modules.oidc.publicUrl`/audience do MCP.

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-console.yaml -f meus-valores.yaml
```

A console tem Deployment, ConfigMap de bootstrap, Service e Ingress próprios, com
`console.ingress` separado do Ingress MCP. Preserve `Host`, `Origin` e `Authorization` e não
herde um bearer estático que bloqueie assets, `/api/v1/config` ou `/auth/callback` antes do
login. Registre o redirect URI `<console.publicUrl>/auth/callback` no cliente público do issuer,
sem segredo, com PKCE S256; discovery e token endpoint devem aceitar o browser com CORS para
a origem configurada. A API exige OIDC; o browser troca o código diretamente com o issuer.
O init faz snapshot pull, e um companion pull periódico usa o mesmo `local_dir` exclusivo do
pod da console. A readiness verifica o listener TCP. O hostname único do pod nomeia o heartbeat.

O writer usa `[workspace] source = "store"` e executa `workspace pull`, `workspace.fetch`,
`index` e `snapshot publish` dentro de uma única `snapshot lock`. O fetch continua sendo o
script do operador: ao adicionar repos ou alterar paths no editor, atualize esse script,
credenciais e mirrors também. Não há clone dinâmico pela console; um mirror ausente impede o
publish. Cada job/pod tem seus próprios diretórios. Uma CA montada acrescenta raízes ao trust do
snapshot; Valkey usa somente as raízes desse arquivo.

Inicialize o workspace uma vez antes do primeiro publish. Salve as entradas abaixo em
`repositories.toml` (ajuste junto com `workspace.repositories`, a lista que o `workspace.fetch`
padrão busca). O exemplo usa release `meada` e namespace atual. O pod temporário reaproveita
volumes e Secrets do CronJob, mas substitui
seu comando por sleep para o operador executar import/apply. Nenhum deploy reaplica o seed:

```toml
[[repository]]
repo = "github.com/claudiomascaro/meada"
path = "/data/mirrors/meada"
```

```sh
kubectl create job meada-seed --from=cronjob/meada-meada-refresh --dry-run=client -o json | \
  python3 -c 'import json,sys; j=json.load(sys.stdin); c=j["spec"]["template"]["spec"]["containers"][0]; c["command"]=["sleep","3600"]; c.pop("args",None); print(json.dumps(j))' | \
  kubectl apply -f -
kubectl wait --for=condition=Ready pod -l job-name=meada-seed --timeout=120s
POD=$(kubectl get pod -l job-name=meada-seed -o jsonpath='{.items[0].metadata.name}')
kubectl cp repositories.toml "$POD:/tmp/repositories.toml"
VERSION=$(kubectl exec "$POD" -- meada workspace import /tmp/repositories.toml \
  --message "Initial managed workspace" --workspace /etc/meada/meada.workspace.toml)
kubectl exec "$POD" -- meada workspace apply "$VERSION" --workspace /etc/meada/meada.workspace.toml
kubectl delete job meada-seed
kubectl create job meada-first-refresh --from=cronjob/meada-meada-refresh
```

Espere o primeiro refresh e os pulls iniciais terminarem; então os dois nós e a console devem
ficar prontos. Aplicar outra versão prepara o próximo refresh, não publica imediatamente.
Para inspeção local, faça port-forward do Service `meada-meada-console` e envie o Host público
configurado. `scripts/helm-check` valida o perfil OIDC, seus seletores e a rota independente.
O smoke implantado usa viewer admitido por loopback; login OIDC completo é coberto pelos testes
HTTP/Rust e não é simulado com um IdP no cluster.

## Postgres e identidade empresarial, opcional

`values-identity.yaml` (sobre `values-console.yaml`) liga a camada de dados e a identidade empresarial
(ADR 0016): `modules.postgres`, `modules.githubApp`, `modules.oidc.github`, `modules.directory` e,
comentado, `modules.oidc.type: entra` com `entra.overage: directory`. Tudo vem desligado por padrão
e nada de Postgres é renderizado. As URLs (que levam senhas), a chave do App e o token SCIM vêm só
de Secrets: `meada-postgres` (chaves `migrate-url`, `write-url`, `read-url`), `meada-github-app`
(`private-key`) e `meada-scim` (`token`, pelo menos 32 caracteres); nenhum valor guarda segredo.

```sh
helm upgrade --install meada deploy/helm/meada -f deploy/helm/meada/values-console.yaml \
  -f deploy/helm/meada/values-identity.yaml -f meus-valores.yaml
```

Com `modules.postgres.enabled`, um Job de hook roda `meada postgres migrate` com o papel
`migrate-url`, em `pre-install,pre-upgrade`, antes de o chart criar os Deployments e o CronJob; a
console e o refresh ainda esperam num init container (`wait-postgres`) até o schema estar migrado,
enquanto os nós sobem e servem sem Postgres (o grant do GitHub deles fica vazio até lerem um
conjunto). Os nós leem
`read-url`; a console e o refresh, `write-url`. Com `modules.oidc.github.enabled`, o CronJob usa o
bootstrap `job.workspace.toml` (o único com `[[github_app]]`) e roda `meada workspace pull`,
`meada github-app roles`, `meada workspace show --pulled`, o seu `workspace.fetch` (que recebe esse
arquivo em `$1`), `index` e `publish`.

`postgres.enabled=true` renderiza um Postgres de exemplo, para avaliação (uma réplica; Secrets
`meada-postgres-server` e `meada-postgres-tls`, com um certificado que nomeie o Service). Como o
banco não existe antes do release, o hook de migração passa a `post-install,post-upgrade` e a
console espera o schema: `helm install --wait` esperaria a console antes de o hook rodar, então
instale sem `--wait` nesse caso. Em produção, use o seu próprio Postgres 15 ou mais novo, com TLS e os três
papéis, e a mesma `tls.caSecret` valida o certificado (`ca_file`).

### Workspaces nomeados

`values-workspaces.yaml` (sobre `values-console.yaml` e `values-identity.yaml`) habilita
`modules.workspaces`: as seções `[catalog]` e `[workspaces]` entram no bootstrap da console e no
`job.workspace.toml` do job de refresh, com o bloco `[ui]` de que o job passa a precisar, e nunca no
de um nó. O chart a recusa sem `console.operations`, `modules.oidc.github`, um `allow` não vazio e um
`maxRepositories` definido por você (não tem padrão). Atualize todas as imagens, nós incluídos, antes
do primeiro refresh que registre um workspace. Seu `workspace.fetch` deve clonar os repositórios que
só workspaces nomeados selecionam sob `modules.workspaces.mirrors`, nos caminhos que `meada workspace
show --pulled` lista. O servidor de propostas não faz parte do chart.
