# Identidades e extração de contratos

Regras de identidade física e mecanismos de extração compartilhados pelos tipos de contrato.

## Interface

Os normalizadores de GCP e Azure convertem nomes documentados nas identidades usadas por configuração, IaC e SDKs. A extração compartilhada combina clientes, operações e argumentos. `[gcp.rs:pubsub]` `[azure.rs:servicebus]` `[sdk.rs:extract]`

A identidade persistida de um receptor registra o tipo e os argumentos de um SDK ou as remoções de barra e o sufixo de uma URL base HTTP. `[receiver.rs:ReceiverIdentity]`

## Invariantes

Uma assinatura Pub/Sub identifica a própria assinatura. Ao combinar valores de ambiente, o escopo identifica workload, raiz e contêiner. `[gcp.rs:pubsub]` `[receiver.rs:EnvironmentScope]`

A normalização de identificadores ARM aceita de 1 a 1.024 bytes. `[azure.rs:arm]` `[azure.rs:AZURE_IDENTIFIER_MAX]`

## Efeitos colaterais

Os extratores recebem fatos de código e devolvem os lados encontrados em memória. `[sdk.rs:extract]`

## Componentes

- [Extração dos SDKs do Azure](./azure/README.md)
- [Extração dos SDKs do Google Cloud](./gcp/README.md)
- [Origem de clientes e argumentos de SDK](./sdk/README.md)

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it -- aws_sdk:: azure_sdk:: gcp_sdk::`.
