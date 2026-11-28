# Operações da API do console

Operações do console para ler o estado da plataforma, aplicar mudanças e administrar acesso e workspaces.

## Interface

Os handlers de leitura consultam frota, armazenamento e versões. Outras rotas oferecem catálogo, diretório, login local e operações sobre workspaces quando os módulos estão habilitados. `[read.rs:operational]` `[catalog.rs:catalog]` `[directory.rs:directory_refusal]` `[accounts.rs:login]` `[workspaces.rs:workspaces]`

## Invariantes

As leituras operacionais têm prazo e limite de concorrência. Com auditoria no Postgres, uma escrita validada registra o estado pendente antes de alterar o Valkey ou o armazenamento. `[read.rs:READERS_MAX]` `[read.rs:operational]` `[write.rs:pending]` `[write.rs:write]`

O login local devolve HTTP 413 se o corpo do pedido exceder 4 KiB. `[accounts.rs:LOGIN_MAX]`

## Efeitos colaterais

As rotas usam os serviços habilitados no manifesto, entre eles Postgres, Valkey e armazenamento de objetos. As operações auditadas registram o resultado da tentativa. `[read.rs:operational]` `[write.rs:operation]` `[write.rs:finish]`

## Componentes

- [Administração de contas no console](./accounts/README.md)
- [Workspaces e propostas no console](./workspaces/README.md)

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features cloud --test it ui::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
