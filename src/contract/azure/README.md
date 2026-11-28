# Extração dos SDKs do Azure

Transforma chamadas aos SDKs do Azure em lados de contratos de canal ou recurso.

## Interface

A política carrega a identidade do cliente até a operação de Service Bus, Event Hubs, Blob Storage ou Key Vault e entrega os resultados da família solicitada. `[sdk.rs:extract]`

## Invariantes

A extração exige evidência do receptor. Um construtor ou método isolado não basta para atribuir a direção de um contrato. `[sdk.rs:extract]`

## Efeitos colaterais

Acumula os lados encontrados em memória a partir dos fatos de cada arquivo. `[sdk.rs:extract]`

## Componentes

- [Chamadas aos SDKs do Azure por linguagem](./sdk/README.md)

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it azure_sdk::`.
