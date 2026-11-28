# Grupos do Entra ID via diretório

Resolução de grupos do Entra ID quando o token indica excesso de grupos e o manifesto seleciona o diretório.

## Interface

O detector identifica a ausência do claim de grupos e sua indicação em `_claim_names`. A consulta usa o claim do usuário para buscar os grupos do usuário ativo no diretório. `[overage.rs:in_overage]` `[overage.rs:groups]`

## Invariantes

A consulta usa apenas o diretório configurado. Um usuário ausente ou inativo recebe zero grupos e uma consulta que falha não entra no cache. `[overage.rs:ask]` `[overage.rs:groups]`

## Efeitos colaterais

Consulta o Postgres e mantém em memória as respostas bem-sucedidas, inclusive vazias, por cinco minutos por padrão. Falhas de consulta geram uma mensagem de diagnóstico. `[overage.rs:groups]` `[overage.rs:OVERAGE_TTL]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features oidc,ui,directory --test it oidc::overage::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
