# Diretório SCIM

Diretório de usuários e grupos provisionados por SCIM, consultado pelo console para admissão e seleção de responsáveis.

## Interface

A interface SCIM atende usuários, grupos e metadados sob `/scim/v2/`. O leitor do console consulta desprovisionamento, grupos e contagens. `[scim.rs:Scim]` `[scim.rs:answer]` `[read.rs:Directory]` `[read.rs:deprovisioned]` `[read.rs:counts]`

## Invariantes

A admissão SCIM valida a credencial de provisionamento. Os tamanhos do corpo e das páginas são limitados. `[scim.rs:admits]` `[scim.rs:SCIM_BODY_MAX]` `[scim.rs:SCIM_PAGE_MAX]`

Cada alteração precisa manter o JSON do recurso armazenado em até 64 KiB. Exceder esse limite devolve HTTP 413 mesmo para um PATCH pequeno. `[scim.rs:candidate]` `[scim.rs:SCIM_RESOURCE_MAX]`

## Efeitos colaterais

O provisionamento altera registros de usuários, grupos e membros no Postgres. As consultas do console abrem conexão para ler o estado atual. `[scim.rs:answer]` `[read.rs:groups]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features ui,oidc,directory --test it ui::oidc::directory::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
