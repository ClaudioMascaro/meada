# Receitas de provedores de identidade

Três receitas para configurar o [provedor de identidade](enterprise.md#provedor-de-identidade) do Meada: Entra ID, Okta e Keycloak. Cada uma lista as chaves do manifesto de bootstrap, os passos do lado do provedor e a documentação dele. Os nomes de claim e de mapeamento abaixo vêm do que o Meada exige e do que cada produto documenta; confira-os na documentação vigente do provedor antes de aplicá-los, porque os produtos renomeiam telas e opções.

Em todas, a URL SCIM é `<URL pública da console>/scim/v2/`, com o token SCIM como bearer, e o login do GitHub chega ao token por uma claim que **o administrador do provedor mapeia de um atributo verificado**, nunca de um que o usuário edita.

## Entra ID

```toml
[oidc]
type = "entra"
issuer = "https://login.microsoftonline.com/<tenant>/v2.0"
public_url = "https://mcp.exemplo.internal"
groups_claim = "groups"

[oidc.groups]
"<object id do grupo>" = ["github.com/acme/api"]

[oidc.entra]
overage = "directory"        # ou "ignore"

[oidc.github]
login_claim = "github_login"
max_age_seconds = 5400

[directory]
token_env = "MEADA_SCIM_TOKEN"
user_claim = "oid"
user_attribute = "externalId"
group_attribute = "externalId"
```

No Entra ID:

1. Registre o aplicativo do Meada e emita grupos na claim `groups` (a claim de grupos do registro de aplicativo). O `oid`, o id de objeto imutável do usuário, já vai no token.
2. Crie um aplicativo empresarial de provisionamento com o modo **Automático**, a URL do locatário `https://<console>/scim/v2/` e o token SCIM como token secreto.
3. No mapeamento de atributos de **usuário**, faça `objectId` ser enviado como `externalId` do usuário. No de **grupo**, o `externalId` do grupo é o `objectId` do grupo, o valor que a claim `groups` carrega.
4. Mapeie o login do GitHub para uma claim de saída `github_login` (uma política de mapeamento de claims, a partir de um atributo de extensão que o administrador preenche).
5. Atribua usuários e grupos ao aplicativo e inicie o provisionamento. Um usuário em muitos grupos recebe `_claim_names` em vez de `groups`; com `overage = "directory"`, os grupos vêm do diretório, então os grupos que importam ao Meada precisam estar atribuídos ao aplicativo.

Documentação: [provisionamento SCIM](https://learn.microsoft.com/entra/identity/app-provisioning/use-scim-to-provision-users-and-groups), [mapeamento de atributos](https://learn.microsoft.com/entra/identity/app-provisioning/customize-application-attributes), [claims opcionais e mapeamento de claims](https://learn.microsoft.com/entra/identity-platform/optional-claims) e [excesso de grupos](https://learn.microsoft.com/entra/identity-platform/id-token-claims-reference).

## Okta

```toml
[oidc]
type = "okta"
issuer = "https://<org>.okta.com/oauth2/<servidor>"
public_url = "https://mcp.exemplo.internal"
groups_claim = "groups"

[oidc.groups]
"meada-leitores" = ["github.com/acme/api"]

[oidc.github]
login_claim = "github_login"
max_age_seconds = 5400

[directory]
token_env = "MEADA_SCIM_TOKEN"
user_claim = "preferred_username"
user_attribute = "userName"
group_attribute = "displayName"
```

No Okta:

1. Crie um aplicativo **SCIM 2.0** (modo de provisionamento por SCIM) com a URL base `https://<console>/scim/v2/` e autenticação por **HTTP Header** carregando o bearer do token SCIM. O Okta envia as atualizações de usuário com `PUT`, que o endpoint aceita.
2. Habilite criar, atualizar e desativar usuários, e o push de grupos.
3. A claim `user_claim` deve ter o mesmo valor que o `userName` do SCIM (o nome de usuário do Okta); o Meada o compara sem diferenciar maiúsculas. No servidor de autorização, ajuste a claim se o token não a traz.
4. No servidor de autorização, adicione uma claim `groups` (filtro de grupos) e uma claim `github_login` com o valor do atributo de perfil que guarda o login do GitHub.
5. Com `scopes` definido, lembre que o Okta manda `scp` e o Meada lê `scope`: crie a claim `scope` ([Autenticação e grants](admission.md)).

Documentação: [SCIM no Okta](https://developer.okta.com/docs/concepts/scim/) e [claims personalizadas](https://developer.okta.com/docs/guides/customize-tokens-returned-from-okta/).

## Keycloak

```toml
[oidc]
type = "keycloak"
issuer = "https://id.exemplo.internal/realms/agentes"
public_url = "https://mcp.exemplo.internal"
groups_claim = "groups"

[oidc.groups]
"meada-leitores" = ["github.com/acme/api"]

[oidc.github]
login_claim = "github_login"
max_age_seconds = 5400
```

No Keycloak:

1. Em **Client scopes**, crie ou edite o escopo do cliente do Meada e adicione um mapper **User Attribute** que leia o atributo do usuário com o login do GitHub e o ponha na claim `github_login` do token de acesso.
2. Adicione um mapper **Group Membership** para a claim `groups` (sem o caminho completo, se os nomes de grupo são simples).

O Keycloak **não tem push SCIM próprio**. O `[directory]` só funciona com um provisionamento que envie usuários e grupos ao endpoint SCIM (uma ponte de provisionamento mantida por você). Sem ela, deixe `[directory]` desligado: o grant das claims e o grant do GitHub funcionam do mesmo jeito, mas o seletor de owners, `owner_missing`, o desprovisionamento na console e o overage do Entra (que não se aplica ao Keycloak) ficam de fora.

Documentação: [protocol mappers](https://www.keycloak.org/docs/latest/server_admin/#_protocol-mappers).
