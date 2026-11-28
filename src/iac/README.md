# Infraestrutura como código

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Converte declarações de infraestrutura em nós e referências que a busca, o contexto e o
rastreamento conseguem consultar.

## Interface

Os leitores avaliam Terraform/OpenTofu, Compose, CloudFormation/SAM, Serverless Framework e GitHub
Actions. A tabela incorporada associa tipos a contratos. A consulta usa os nomes avaliados para
ligar provisionadores aos usos.
[json.rs:read](./json.rs) [compose.rs:evaluate](./compose.rs) [cloudformation.rs:evaluate](./cloudformation.rs) [serverless.rs:evaluate](./serverless.rs) [actions.rs:evaluate](./actions.rs) [types.rs:entry](./types.rs) [provision.rs:links](./provision.rs)

Projeções específicas de Azure e GCP usam os tipos de recurso declarados. Os leitores de cargas e
assinaturas avaliam as referências na instância correta. A lista de órfãos considera as
exportações visíveis na consulta.
[azure.rs:azure_projection](./azure.rs) [gcp.rs:gcp_projection](./gcp.rs) [workload.rs:workload](./workload.rs) [subscription.rs:prepare_subscriptions](./subscription.rs) [orphans.rs:Scope](./orphans.rs)

- [Leitura dos comandos de GitHub Actions](./actions/README.md)
- [Rotas REST do API Gateway](./http/README.md)
- [Módulos Terraform mapeados](./mapping/README.md)
- [Containers ECS em Terraform](./workload/README.md)

## Invariantes

Cada raiz Terraform é avaliada no seu próprio contexto. As funções puras registradas só usam
argumentos. Usos derivados de configuração exigem valores completos e sem redação para identificar
o contrato.
[../iac.rs:Eval](../iac.rs) [functions.rs:declare](./functions.rs) [config_uses.rs:read](./config_uses.rs)

Ao ligar um uso a seu provisionador, o cálculo respeita o limite de certeza do uso derivado do
ambiente. Na avaliação do CloudFormation, uma função intrínseca além de 32 níveis deixa essa parte
do nome desconhecida.
[provision.rs:directed_uses](./provision.rs) [../contract.rs:use_level](../contract.rs) [cloudformation.rs:INTRINSIC_DEPTH_MAX](./cloudformation.rs)

## Efeitos colaterais

A atualização persiste os nós e as referências no índice via transação SQLite. Em Actions ela
remove antes as linhas anteriores daquele arquivo. Os vínculos de provisionamento são derivados na
consulta.
[../iac.rs:insert](../iac.rs) [actions.rs:refresh](./actions.rs) [provision.rs:links](./provision.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- iac:: custom_iac:: provision:: helm_env::
```
