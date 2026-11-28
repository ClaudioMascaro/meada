# Catálogo de instalações GitHub

Descoberta das identidades de repositório candidatas à verificação de papéis do GitHub App.

## Interface

A partir das instalações dos proprietários configurados e da seleção existente, produz um conjunto ordenado de identidades para a etapa de autorização. `[catalog.rs:candidates]`

## Invariantes

Integrar o catálogo só torna o repositório candidato. Erros de paginação ou de consulta abortam a listagem para que uma resposta parcial não substitua o conjunto anterior. `[catalog.rs:candidates]` `[catalog.rs:repositories]`

Se a última página permitida vier cheia, a listagem falha porque ainda pode haver repositórios por ler. `[catalog.rs:repositories]`

## Efeitos colaterais

Consulta a API de repositórios da instalação com seu token e usa as marcas de limitação de chamadas compartilhadas pelo cliente. `[catalog.rs:repositories]` `[catalog.rs:candidates]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features workspaces --test it github_app::roles::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
