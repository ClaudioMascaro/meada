# Chamadas aos SDKs do Google Cloud por linguagem

Formas dos SDKs do Google Cloud em C#, Go, Java, Python e JS/TS para Pub/Sub, Cloud Storage e Secret Manager.

## Interface

Os leitores identificam o cliente e retornam o papel, o tipo de alvo e o argumento que o nomeia. Em Go, tópicos e assinaturas são identificados a partir do cliente de Pub/Sub. `[csharp.rs:read]` `[go.rs:step]` `[java.rs:read]` `[python.rs:read]` `[ts.rs:read]`

## Invariantes

Em Node, o método `on` de uma assinatura só representa consumo quando recebe o evento literal `message`. Cada linguagem verifica o estágio do receptor antes de atribuir direção. `[ts.rs:read]` `[go.rs:read]`

## Efeitos colaterais

Os leitores consultam fatos do código em memória. `[java.rs:constructed]` `[python.rs:read]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it gcp_sdk::`.
