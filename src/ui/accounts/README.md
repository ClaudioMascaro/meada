# Administração de contas no console

Administração das contas locais e associação de um login GitHub verificado pelo console.

## Interface

O operador global lista, cria, altera ou suspende contas. Cada pessoa pode vincular ou remover seu próprio login GitHub quando a integração está configurada. `[administration.rs:administer]` `[github.rs:verify_github]` `[github.rs:remove_own_github]`

## Invariantes

A identidade GitHub vem da resposta de `GET /user`. A listagem administrativa omite os hashes de senha. `[github.rs:Verifier]` `[administration.rs:administer]`

Os pedidos de criação ou alteração de contas têm limite de 4 KiB e recebem HTTP 413 se o ultrapassarem. `[administration.rs:ADMIN_BODY_MAX]`

## Efeitos colaterais

A verificação consulta a API configurada usando o token fornecido. As alterações atualizam as contas no Postgres e passam pela auditoria do console. `[github.rs:verify_github]` `[administration.rs:administer]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features accounts,github-app --test it ui::accounts::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
