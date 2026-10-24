# Autenticación y grants

El token demuestra la identidad; el grant determina qué repositorios pueden aparecer en la respuesta. OIDC es opt-in. Sin él, la admisión corresponde al proxy. Incluso después de admitir una llamada, los enlaces entre repositorios aparecen solo cuando el grant incluye ambos extremos.

<!-- meada-graph:admission -->

## Configurar OIDC

**OIDC** (feature `oidc`; `cargo install meada --features oidc`). La admisión por defecto sigue
siendo el proxy inverso, como dice el aviso al inicio de esta sección; el módulo OIDC es la
alternativa para quien quiere que el propio Meada verifique quién llama. Con la sección `[oidc]`,
el servidor se vuelve un resource server OAuth 2.1, como la especificación de autorización de
MCP (2026-07-28) lo describe: toda solicitud lleva `Authorization: Bearer <JWT>` emitido por el
proveedor de identidad del operador. Meada no emite tokens, no registra clientes y nunca reenvía un token.

```toml
[oidc]
issuer = "https://id.ejemplo.internal/realms/agentes"
public_url = "https://meada.ejemplo.internal"   # la URL que usan los clientes, detrás del proxy
audience = "https://meada.ejemplo.internal"     # opcional; por defecto: la URL canónica del endpoint
identities_claim = "meada_repositories"          # opcional con groups_claim
groups_claim = "groups"                          # opcional, junto con [oidc.groups]
scopes = ["meada:read"]                          # opcional, anunciado y exigido
jwks_refresh_seconds = 3600                      # por defecto; como mínimo 60

[oidc.groups]                                    # un grupo del token: las identidades que permite
"equipo-pagos" = ["github.ejemplo.internal/acme/payments-api", "github.ejemplo.internal/acme/ledger"]
"plataforma" = ["*"]
```

## Preparar el proveedor de identidad

En el proveedor de identidad: un audience igual a `audience` (o, sin ella, a la URL de cada endpoint,
`<public_url>/<identidad>`); un claim mapper que pone en `identities_claim` la lista de
identidades que el cliente puede leer (`"*"` las permite todas), o en `groups_claim` sus grupos; y
client credentials para los agentes headless y para el backend de un portal, que piden su token al
proveedor de identidad. Una página en el navegador no puede llamar al servidor MCP directamente: con `[oidc]`, toda
solicitud con `Origin` recibe 403, en cualquier ruta, así que un portal llama desde su backend. El
proxy reenvía sin cambios `Authorization` y los encabezados del SEP-2243 (`MCP-Protocol-Version`,
`Mcp-Method`, `Mcp-Name`, `Mcp-Param-*`).

## Calcular el grant

Con `groups_claim`, la tabla `[oidc.groups]` del manifiesto mapea cada grupo a las identidades
que permite (`"*"` para todas), y `identities_claim` pasa a ser opcional: las dos claves van
juntas, y una sección sin ninguno de los dos claims impide el arranque. Un token lee la unión de
las identidades de su `identities_claim` y de las que mapean sus grupos. Un claim ausente o que no
es un array de strings, un grupo que la tabla no mapea y una identidad que el servidor no sirve no
permiten nada; un error de tipeo le aparece a quien llama como un 403.

## Descubrimiento y claves

<details markdown="1" class="reference-details">
<summary>Descubrimiento, caché de claves y fallos</summary>

Al arrancar, antes de escuchar, Meada lee los metadatos del issuer (RFC 8414 u OpenID
Discovery, en las rutas y el orden que define la especificación de MCP, verificando que el
`issuer` del documento es el configurado) y el JWKS que nombran; una falla impide el arranque.
`issuer`, `public_url` y el `jwks_uri` deben ser `https` fuera de loopback; ninguna de las
solicitudes sigue una redirección ni pasa por un proxy, cada una tiene 10 s y un tope de tamaño,
y los certificados se verifican contra las raíces del sistema. El JWKS se vuelve a leer cada
`jwks_refresh_seconds` y cuando llega un token con un `kid` desconocido, como mucho una vez por
minuto; si la relectura falla, las claves en caché siguen y stderr lo dice.

</details>

## Metadatos y validación

<details markdown="1" class="reference-details">
<summary>Metadatos y reglas de validación del token</summary>

`GET /.well-known/oauth-protected-resource/<identidad>` responde sin token los metadatos del
recurso (RFC 9728): `resource` igual a `<public_url>/<identidad>`, `authorization_servers` igual
a `[issuer]`, `bearer_methods_supported` igual a `["header"]` y `scopes_supported` cuando hay
`scopes`; `/.well-known/oauth-protected-resource/<identidad>@<generación>` responde lo mismo con
el `resource` fijado. Un token pasa con un algoritmo asimétrico (RS256, RS384, RS512, ES256,
ES384, PS256) que coincida con la clave, `iss` igual a `issuer`, un `aud` igual a `audience` o,
sin ella, a la URL de `/<identidad>` (que los pins también aceptan, de modo que un token lee el
repositorio en toda generación), `exp` obligatorio y `nbf` respetado con 60 s de margen, y todo
scope configurado en su `scope`. Un token en la query string nunca se lee. `scope` se lee solo
como string separado por espacios; `scp` y un `scope` en array no se leen, así que, con `scopes`
definido, un proveedor de identidad que envía `scp` (Okta, por ejemplo) necesita un claim mapper a `scope`.

</details>

## Respuestas de autorización

Sin token, o con uno que no pasa: 401 con `WWW-Authenticate: Bearer
resource_metadata="<public_url>/.well-known/oauth-protected-resource/<ruta>"`, más
`error="invalid_token"` cuando llegó un token y `scope` cuando está configurado. Un token válido
que no puede leer la identidad del endpoint, también en un pin: 403 con
`error="insufficient_scope"`; sin un scope exigido, el mismo 403 con `scope`. Detrás de un
endpoint que el token puede leer, toda respuesta cubre solo los repositorios que puede leer. La
autenticación va antes del enrutamiento: el 404 de la raíz, de una ruta desconocida y de una
generación que el nodo no guarda exige un token válido para esa ruta (sin `audience`, el de la
raíz tiene `aud` igual a `public_url`) y lista solo las identidades que puede leer. Ningún token,
ni parte de uno, aparece en stderr o en un error.
