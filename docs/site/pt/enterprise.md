# Identidade empresarial

!!! warning "Planejado: S4"
    [S4, #319](https://github.com/ClaudioMascaro/meada/issues/319) especifica automação de identidade, provedores de identidade, provedores de grant e camada de dados. Nenhuma configuração ou comando novo desta proposta deve ser tratado como disponível. A [admissão OIDC por claims](admission.md) já está entregue.

## Autenticar, conceder acesso e registrar

O provedor de identidade continuará emitindo o token. O grant efetivo reunirá as identidades permitidas pelas claims e, quando configurado, pelas permissões do GitHub. A consulta de papéis no GitHub caberá ao job de refresh, nunca ao console ou à indexação. Owners e papéis humanos não ampliarão esse grant.

Postgres será um módulo opt-in para estado mutável durável: auditoria, diretório e permissões. O snapshot store continuará guardando documentos e bundles imutáveis; Valkey continuará guardando ponteiros, locks, fences e caches. `meada index` continuará sem rede.

## Diretório e revogação

SCIM alimentará o diretório de usuários e grupos, incluindo os grupos que excedem o limite de claims do Entra. A exclusão de um sujeito provisionado deixará um bloqueio durável. Um sujeito que nunca foi provisionado continuará na admissão normal: ausência no diretório não será, por si, uma exclusão.

A auditoria deverá registrar a tentativa antes da mutação. Se esse registro falhar, a escrita será recusada. Os detalhes de cache, expiração, indisponibilidade e revogação pertencem à spec S4; não representam uma garantia do runtime entregue.

<!-- meada-graph:scim-revocation -->

## Provedores de identidade, grants e dados

As linhas tracejadas abaixo representam as integrações planejadas. O provedor de identidade emite o token; o job mantém permissões; console e nós consultam os dados necessários à admissão. O diretório e a auditoria têm estado durável separado do conteúdo de código.

<!-- meada-graph:enterprise -->
