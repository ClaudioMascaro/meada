# Receitas de provedores de identidade

Três receitas para configurar o [provedor de identidade](enterprise.md#provedor-de-identidade) do Meada: Entra ID, Okta e Keycloak. Cada uma lista as chaves do manifesto de bootstrap, os passos do lado do provedor e a documentação dele. Os nomes de claim e de mapeamento abaixo vêm do que o Meada exige e do que cada produto documenta; confira-os na documentação vigente do provedor antes de aplicá-los, porque os produtos renomeiam telas e opções.

Comece pelas [contas locais](console.md#contas-locais) se ainda não tem um provedor de identidade; estas receitas são o passo seguinte, e a [migração](console.md#migrar-das-contas-locais-para-o-provedor-de-identidade) diz o que muda (`break_glass`, grupos, tokens).

Em todas, a URL SCIM é `<URL pública da console>/scim/v2/`, com o token SCIM como bearer, e o login do GitHub chega ao token por uma claim que **o administrador do provedor mapeia de um atributo verificado**, nunca de um que o usuário edita.

## Das contas locais ao provedor de identidade

Cada receita abaixo é o `[oidc]` que substitui o `[accounts]`. Mantenha `[accounts] break_glass = true` com as chaves, para que `admin` entre quando o provedor estiver fora do ar; `[accounts.groups]` e `[accounts.github]` saem, e os grupos viram o `[oidc.groups]` da receita, com os valores que a claim de `groups_claim` do provedor carrega. Os passos completos estão em [Migrar das contas locais](console.md#migrar-das-contas-locais-para-o-provedor-de-identidade).

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

## Um token para o servidor de propostas

O [servidor de propostas](named-workspaces.md#rodar-o-servidor-de-propostas) admite um JWT bearer cujo `aud` é `[proposals] audience` (o `public_url` quando não definido), assinado pelo `[oidc] issuer` do seu bootstrap (o dos nós e da console, salvo se você lhe der um próprio, como o Okta exige). Um token de nó ou da console é recusado ali (401), e o token deste servidor é recusado por eles. O token precisa ser **de uma pessoa**: o login, os grupos e o papel vêm dele; um token sem pessoa (o grant de credenciais do cliente) fica `pending` com `missing_login` e não seleciona nada. Um agente age por um operador ao portar o token desse operador, com validade curta, pois o tempo de vida do token do provedor é o limite contra abuso ([ADR 0022](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0022-authenticated-workspace-proposals.md)).

Em cada provedor abaixo, registre o servidor de propostas como um recurso próprio e obtenha o token com um **cliente público sem segredo**: Authorization Code com PKCE S256 para uma pessoa no navegador, ou o device authorization grant para um agente no terminal (a pessoa aprova o código e o agente recebe o token). Antes de configurar o agente, decodifique o payload do token e compare `aud` com `[proposals] audience`, `iss` com `[oidc] issuer`, `exp`, e a claim de login (`github_login`) e os grupos que as outras receitas exigem: `printf '%s' "$TOKEN" | cut -d. -f2 | tr '_-' '/+' | base64 -d 2>/dev/null`. Os nomes de claim e de tela vêm do que cada produto documenta; confira-os na documentação vigente.

### Entra ID

1. Registre um segundo aplicativo para o servidor de propostas. Em **Expor uma API**, defina um URI de ID do Aplicativo e um escopo (por exemplo `propose`). No manifesto, defina `requestedAccessTokenVersion` como `2`, para que `aud` seja o ID do cliente (do aplicativo): `[proposals] audience = "<id do aplicativo>"`.
2. Emita as mesmas claims que o aplicativo dos nós emite: `groups` e `github_login` (a política de mapeamento de claims e a claim de grupos valem para cada aplicativo separadamente).
3. No registro do cliente do agente (um cliente público, com o URI de redirecionamento ou o fluxo de dispositivo habilitado), conceda a permissão delegada `propose` do aplicativo de propostas.
4. Uma pessoa obtém o token com o escopo `<URI de ID do Aplicativo>/propose` (por padrão `api://<id do aplicativo>/propose`) em `https://login.microsoftonline.com/<tenant>/oauth2/v2.0/` (`authorize` e `token`, ou `devicecode` e `token` para um agente). Não use `.default` com um segredo de cliente: esse é o grant de credenciais do cliente, que não carrega pessoa.

Documentação: [expor uma API](https://learn.microsoft.com/entra/identity-platform/quickstart-configure-app-expose-web-apis), [fluxo de código de dispositivo](https://learn.microsoft.com/entra/identity-platform/v2-oauth2-device-code) e [claims do token de acesso](https://learn.microsoft.com/entra/identity-platform/access-token-claims-reference).

### Okta

O `aud` de um token de acesso do Okta é o **Audience** do servidor de autorização personalizado que o emitiu, um valor único, e os nós, a console e o servidor de propostas precisam de três audiences diferentes. O servidor de propostas é um processo próprio, com bootstrap próprio, então pode ler um segundo servidor de autorização personalizado: crie um cujo Audience seja `[proposals] audience`, com as mesmas claims `groups` e `github_login` e os mesmos usuários, e aponte para ele o `[oidc] issuer` do bootstrap do servidor de propostas (as outras chaves de `[oidc]` como antes). Uma pessoa ou um agente obtém então o token desse servidor pelos fluxos acima. Os testes do Meada não cobrem esse caminho, e o chart Helm renderiza um só `[oidc]` para todos os pods, então pelo chart o servidor de propostas lê o emissor dos nós e, com o Okta, só é alcançável com os tokens de agente das contas locais ([limites conhecidos](enterprise.md#limites-conhecidos)). Não defina `[proposals] audience` como o audience de um nó para contornar: isso torna válido no servidor de propostas o token de um nó e não fecha nada do confused deputy.

Documentação: [servidores de autorização](https://developer.okta.com/docs/concepts/auth-servers/).

### Keycloak

1. Em **Client scopes**, crie um escopo `meada-proposals` e adicione um mapper **Audience** com **Included Custom Audience** igual a `[proposals] audience` e **Add to access token** ligado.
2. Crie o cliente do agente como cliente **público** com o fluxo padrão (método PKCE `S256`) ou o OAuth 2.0 device authorization grant, e adicione `meada-proposals` a ele como escopo **opcional**. Os mappers `github_login` e `groups` da outra receita também precisam alcançar esse cliente (coloque-os num escopo que o cliente tenha, ou no escopo dedicado dele).
3. Peça `scope=meada-proposals` em `protocol/openid-connect/auth` e `token` do realm (ou `auth/device` e `token`). Um token sem esse escopo não carrega essa audience e é recusado.

Documentação: [mapper de audience e client scopes](https://www.keycloak.org/docs/latest/server_admin/#_audience) e [device authorization grant](https://www.keycloak.org/docs/latest/securing_apps/#_oauth2_device_authorization_grant).

### Contas locais

Com as [contas locais](console.md#contas-locais) não há provedor: a página de perfil da console (ou `POST /api/v1/account/tokens` com `{"surface": "proposals"}`, quando o bootstrap da console tem `[proposals]`) emite um token de agente para o servidor de propostas, com audience `meada:proposals`.
