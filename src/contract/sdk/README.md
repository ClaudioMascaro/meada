# Origem de clientes e argumentos de SDK

Evidência compartilhada de construção de clientes dos SDKs e leitura dos argumentos de uma requisição.

## Interface

A política de cada serviço informa como reconhecer o cliente. Os leitores de linguagem interpretam inicializadores C#, literais compostos Go, builders Java e mutações de mapas. `[origins.rs:Policy]` `[csharp.rs:field_of]` `[go.rs:field_of]` `[java.rs:Builders]` `[mutations.rs:writes]`

## Invariantes

Todas as alternativas precisam produzir a mesma evidência. Uma alternativa opaca impede confirmar a origem. O orçamento limita novas expansões e a leitura pode reutilizar evidência já calculada. `[origins.rs:Evidence]`

## Efeitos colaterais

A evidência e os resultados de análise de builders são mantidos em memória durante a leitura. `[origins.rs:Evidence]` `[java.rs:Builders]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it aws_sdk::`.
