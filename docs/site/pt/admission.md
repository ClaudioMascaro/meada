# Autenticação e grants

O token prova a identidade; o grant define os repositórios que podem aparecer na resposta. OIDC é opt-in. Sem ele, a admissão é responsabilidade do proxy. Mesmo após uma chamada ser admitida, links entre repositórios só aparecem quando o grant inclui as duas pontas.

<!-- meada-graph:admission -->

## Configurar o OIDC

**OIDC** (feature `oidc`; `cargo install meada --features oidc`). A admissão padrão continua
sendo o proxy reverso, como diz o aviso no início desta seção; o módulo OIDC é a alternativa
para quem quer que o próprio Meada confira quem chama. Com a seção `[oidc]`, o servidor vira um
resource server OAuth 2.1, como a especificação de autorização do MCP (2026-07-28) descreve:
toda requisição leva `Authorization: Bearer <JWT>` emitido pelo provedor de identidade do operador. O Meada
não emite token, não registra cliente e nunca repassa um token.

```toml
[oidc]
issuer = "https://id.exemplo.internal/realms/agentes"
public_url = "https://meada.exemplo.internal"   # a URL que os clientes usam, atrás do proxy
audience = "https://meada.exemplo.internal"     # opcional; padrão: a URL canônica do endpoint
identities_claim = "meada_repositories"          # opcional com groups_claim
groups_claim = "groups"                          # opcional, junto com [oidc.groups]
scopes = ["meada:read"]                          # opcional, anunciado e exigido
jwks_refresh_seconds = 3600                      # padrão; no mínimo 60

[oidc.groups]                                    # um grupo do token: as identidades que ele libera
"time-pagamentos" = ["github.exemplo.internal/acme/payments-api", "github.exemplo.internal/acme/ledger"]
"plataforma" = ["*"]
```

## Preparar o provedor de identidade

No provedor de identidade: um audience igual a `audience` (ou, sem ela, à URL de cada endpoint,
`<public_url>/<identidade>`); um claim mapper que põe em `identities_claim` a lista das
identidades que o cliente pode ler (`"*"` libera todas), ou em `groups_claim` os grupos dele; e
client credentials para os agentes headless e para o backend de um portal, que pedem o token ao
provedor de identidade. Uma página no navegador não chama o servidor MCP direto: com `[oidc]`, toda requisição com
`Origin` recebe 403, em qualquer caminho, então um portal chama do seu backend. O proxy repassa
sem mudar o `Authorization` e os cabeçalhos do SEP-2243 (`MCP-Protocol-Version`, `Mcp-Method`,
`Mcp-Name`, `Mcp-Param-*`).

## Calcular o grant

Com `groups_claim`, a tabela `[oidc.groups]` do manifesto mapeia cada grupo para as identidades
que ele libera (`"*"` para todas), e `identities_claim` fica opcional: as duas chaves vêm juntas,
e uma seção sem nenhum dos dois claims recusa a subida. O token lê a união das identidades do seu
`identities_claim` e das que os seus grupos mapeiam. Um claim ausente ou que não é um array de
strings, um grupo que a tabela não mapeia e uma identidade que o servidor não serve não liberam
nada; um erro de digitação aparece para quem chama como 403.

## Descoberta e chaves

<details markdown="1" class="reference-details">
<summary>Descoberta, cache de chaves e falhas</summary>

Na subida, antes de escutar, o Meada lê os metadados do issuer (RFC 8414 ou OpenID Discovery,
nos caminhos e na ordem que a especificação do MCP define, conferindo que o `issuer` do
documento é o configurado) e o JWKS que eles nomeiam; uma falha recusa a subida. `issuer`,
`public_url` e o `jwks_uri` precisam ser `https` fora de loopback; nenhuma das requisições segue
um redirecionamento nem passa por proxy, cada uma tem 10 s e um teto de tamanho, e os
certificados são conferidos contra as raízes do sistema. O JWKS é relido a cada
`jwks_refresh_seconds` e quando chega um token com um `kid` desconhecido, no máximo uma vez por
minuto; se a releitura falha, as chaves em cache continuam e o stderr diz isso.

</details>

## Metadados e validação

<details markdown="1" class="reference-details">
<summary>Metadados e regras de validação do token</summary>

`GET /.well-known/oauth-protected-resource/<identidade>` responde sem token os metadados do
recurso (RFC 9728): `resource` igual a `<public_url>/<identidade>`, `authorization_servers`
igual a `[issuer]`, `bearer_methods_supported` igual a `["header"]` e `scopes_supported` quando
há `scopes`; `/.well-known/oauth-protected-resource/<identidade>@<geração>` responde o mesmo
com o `resource` fixado. Um token passa com um algoritmo assimétrico (RS256, RS384, RS512,
ES256, ES384, PS256) que case com a chave, `iss` igual a `issuer`, um `aud` igual a `audience`
ou, sem ela, à URL de `/<identidade>` (aceita também nos pins, de modo que um token lê o
repositório em toda geração), `exp` obrigatório e `nbf` respeitado com 60 s de folga, e todo
escopo configurado no seu `scope`. Um token na query string nunca é lido. O `scope` é lido só
como string separada por espaços; `scp` e um `scope` em array não são lidos, então, com
`scopes` definido, um provedor de identidade que manda `scp` (o Okta, por exemplo) precisa de um claim mapper
para `scope`.

</details>

## Respostas de autorização

Sem token, ou com um token que não passa: 401 com `WWW-Authenticate: Bearer
resource_metadata="<public_url>/.well-known/oauth-protected-resource/<caminho>"`, mais
`error="invalid_token"` quando veio um token e `scope` quando configurado. Um token válido que não
pode ler a identidade do endpoint, num pin também: 403 com `error="insufficient_scope"`; sem um
escopo exigido, o mesmo 403 com `scope`. Atrás de um endpoint que o token pode ler, toda resposta
cobre só os repositórios que ele pode ler. A autenticação vem antes do roteamento: o 404 da raiz,
de um caminho desconhecido e de uma geração que o nó não guarda exige um token válido para aquele
caminho (sem `audience`, o da raiz tem `aud` igual a `public_url`) e lista só as identidades que
ele pode ler. Nenhum token nem pedaço de um token aparece em stderr ou num erro.
