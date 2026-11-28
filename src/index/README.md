# Cache de valores do índice

Cache de valores que evita repetir a análise de arquivos inalterados durante a atualização do índice.

## Interface

O índice abre o cache sob demanda, consulta os fatos por caminho e chave, enfileira novos fatos e retém as entradas necessárias após a extração. `[values.rs:beside]` `[values.rs:get]` `[values.rs:put]` `[values.rs:keep]`

## Invariantes

A chave inclui bytes, configuração da linguagem, pack e build do Meada. Um cache ausente ou ilegível equivale a uma consulta sem resultado e permite refazer a análise. `[values.rs:key]` `[values.rs:get]`

Uma entrada que ultrapassa 8 MiB de JSON descompactado não é reutilizada. O índice volta a analisar o arquivo quando precisar desses valores. `[values.rs:ENTRY_BYTES]` `[values.rs:facts_of]`

## Efeitos colaterais

Grava e remove entradas comprimidas em `values.db`, junto ao índice SQLite. Um índice em memória não cria esse arquivo. `[values.rs:FILE]` `[values.rs:beside]` `[values.rs:keep]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it freshness::a_refresh_over_the_values_cache`.
