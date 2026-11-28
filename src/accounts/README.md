# Contas locais no Postgres

Persistência das contas locais usadas no login, na administração do console e na recuperação da conta padrão.

## Interface

A tentativa de login recebe a função que verifica o hash e devolve o resultado tipado. As operações de criação, alteração e consulta de estado usam o nome de usuário. `[store.rs:attempt]` `[store.rs:Attempt]` `[store.rs:create]` `[store.rs:change]` `[store.rs:standing]`

## Invariantes

A verificação e a contagem de falhas ocorrem sob o mesmo bloqueio de linha. Uma linha inválida, suspensa ou já bloqueada impede o login. `[store.rs:attempt]`

## Efeitos colaterais

Lê e altera a tabela `accounts` do schema configurado no Postgres. A recuperação administrativa redefine a senha da conta padrão. `[store.rs:attempt]` `[store.rs:reset_admin]`

A atualização do hash após o login só grava se a linha ainda contiver o hash verificado. Uma troca de senha concorrente prevalece. `[store.rs:rehash]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features accounts --test it ui::accounts::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
