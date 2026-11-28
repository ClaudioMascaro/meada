# Extração dos SDKs do Google Cloud

Converte evidência dos clientes do Google Cloud em contratos de Pub/Sub, Cloud Storage e Secret Manager.

## Interface

A extração escolhe o leitor da linguagem, segue o receptor e emite o lado correspondente à operação. A projeção de ambiente distingue nomes de tópicos e de assinaturas. `[sdk.rs:extract]` `[sdk.rs:environment_name]` `[sdk.rs:environment_target]`

## Invariantes

Obter um cliente ou um identificador de tópico ainda não define um produtor. A leitura mantém o estágio até a chamada que usa o alvo. `[sdk.rs:extract]`

## Efeitos colaterais

Produz uma coleção em memória a partir dos arquivos já analisados. `[sdk.rs:extract]`

## Componentes

- [Chamadas aos SDKs do Google Cloud por linguagem](./sdk/README.md)

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it gcp_sdk::`.
