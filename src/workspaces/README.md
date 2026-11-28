# Persistência e ciclo de vida de workspaces

Persistência de workspaces nomeados e comandos administrativos de criação, edição, aplicação e histórico.

## Interface

As operações do console e da CLI compartilham criação e transações de ciclo de vida. A criação registra o workspace, seu primeiro operador e a versão inicial. Aplicação e rollback alteram a versão corrente. `[store.rs:create]` `[lifecycle.rs:transact]` `[cli.rs:create]` `[cli.rs:apply]`

## Invariantes

A criação verifica a elegibilidade no conjunto de permissões atual antes dos registros SQL. As mudanças de ciclo de vida voltam a validar a versão base e a autorização sob bloqueio da linha. `[store.rs:eligible]` `[store.rs:create]` `[lifecycle.rs:transact]`

Uma alteração da lista deve deixar o workspace com ao menos um operador e no máximo 20. `[lifecycle.rs:OPERATORS_MAX]` `[lifecycle.rs:transact]`

## Efeitos colaterais

As tabelas de estado e histórico ficam no Postgres. Os documentos de versão imutáveis usam o armazenamento de objetos. A CLI audita as escritas e informa o resultado no terminal. `[store.rs:create]` `[store.rs:put_document]` `[lifecycle.rs:transact]` `[cli.rs:apply]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features workspaces --test it ui::oidc::workspaces::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
