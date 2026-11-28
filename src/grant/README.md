# Concessões de acesso do GitHub

Concessões de leitura derivadas do conjunto de permissões GitHub mais recente no Postgres.

## Interface

O leitor mantém as permissões por login e entrega a concessão e o estado do conjunto para a admissão. Com workspaces, também expõe o catálogo disponível para a seleção. `[permissions.rs:Permissions]` `[permissions.rs:grant_and_state]` `[permissions.rs:catalog]`

## Invariantes

Antes da primeira leitura válida e quando o conjunto expira, a concessão GitHub fica vazia. A idade combina o relógio do Postgres na leitura com o tempo monotônico local transcorrido. `[permissions.rs:grant_and_state]` `[permissions.rs:admission]`

Por padrão, o leitor consulta novos conjuntos a cada 60 segundos. `max_age_seconds` determina por quanto tempo o último conjunto pode conceder acesso. `[permissions.rs:PERMISSIONS_POLL]` `[permissions.rs:grant_and_state]`

## Efeitos colaterais

A atualização periódica consulta o Postgres em uma transação de leitura e substitui o estado em memória. A admissão consulta esse estado local. `[permissions.rs:poll]` `[permissions.rs:grant_and_state]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features workspaces --test it github_app::roles::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
