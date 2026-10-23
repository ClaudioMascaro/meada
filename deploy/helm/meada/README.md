# Meada no Kubernetes

[English](README.en.md) · [Español](README.es.md)

Configure a admissão no Ingress ou no proxy de autenticação antes de expor o serviço.
Sem `[oidc]`, esse componente admite os clientes; uma configuração incorreta expõe o grafo
de código. Com OIDC, preserve `Authorization`. Preserve também os headers SEP-2243
`MCP-Protocol-Version`, `MCP-Session-Id`, `Mcp-Method`, `Mcp-Name` e `Mcp-Param-*`,
devolva `Meada-Generation` e nunca acrescente `Origin`.
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
Nada executa fetch ou acessa a rede dentro de `meada index`.

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
