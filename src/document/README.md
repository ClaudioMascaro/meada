# Documentos OpenAPI e AsyncAPI

Documentos OpenAPI e AsyncAPI como fontes de contratos e formas de payload, com um leitor compartilhado de eventos YAML e JSON.

## Interface

A entrada recebe os bytes de um documento e produz um resumo de rotas HTTP ou lados de canais. Os leitores de eventos também expõem posições de escalares para outros leitores de configuração. `[mod.rs:read]` `[openapi.rs:read]` `[asyncapi.rs:read]` `[events.rs:read_located]`

## Invariantes

As referências de schema são resolvidas dentro do próprio documento. A leitura limita o trabalho e deixa abertas as partes da forma que não consegue enumerar. `[schema.rs:read]` `[schema.rs:Work]`

O leitor de eventos rejeita documentos com aninhamento superior a 128 níveis, contando os nós de um alias na profundidade em que são inseridos. `[events.rs:MAX_NESTING]`

## Efeitos colaterais

Os leitores constroem resultados em memória a partir dos bytes e entregam eventos aos callbacks fornecidos. `[mod.rs:read]` `[events.rs:read]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it documents::`.
