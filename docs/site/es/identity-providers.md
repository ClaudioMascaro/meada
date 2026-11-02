# Recetas de proveedores de identidad

Tres recetas para configurar el [proveedor de identidad](enterprise.md#proveedor-de-identidad) de Meada: Entra ID, Okta y Keycloak. Cada una lista las claves del manifiesto de bootstrap, los pasos del lado del proveedor y su documentación. Los nombres de claim y de mapeo de abajo salen de lo que Meada exige y de lo que cada producto documenta; contrástalos con la documentación vigente del proveedor antes de aplicarlos, porque los productos renombran pantallas y opciones.

Empieza por las [cuentas locales](console.md#cuentas-locales) si aún no tienes un proveedor de identidad; estas recetas son el paso siguiente, y la [migración](console.md#migrar-de-las-cuentas-locales-al-proveedor-de-identidad) dice qué cambia (`break_glass`, grupos, tokens).

En todas, la URL SCIM es `<URL pública de la consola>/scim/v2/`, con el token SCIM como bearer, y el login de GitHub llega al token por una claim que **el administrador del proveedor mapea desde un atributo verificado**, nunca uno que el usuario edite.

## De las cuentas locales al proveedor de identidad

Cada receta de abajo es el `[oidc]` que reemplaza a `[accounts]`. Mantén `[accounts] break_glass = true` con las claves, para que `admin` entre cuando el proveedor esté caído; `[accounts.groups]` y `[accounts.github]` salen, y los grupos pasan a ser el `[oidc.groups]` de la receta, con los valores que la claim `groups_claim` del proveedor lleva. Los pasos completos están en [Migrar de las cuentas locales](console.md#migrar-de-las-cuentas-locales-al-proveedor-de-identidad).

## Entra ID

```toml
[oidc]
type = "entra"
issuer = "https://login.microsoftonline.com/<tenant>/v2.0"
public_url = "https://mcp.ejemplo.internal"
groups_claim = "groups"

[oidc.groups]
"<object id del grupo>" = ["github.com/acme/api"]

[oidc.entra]
overage = "directory"        # o "ignore"

[oidc.github]
login_claim = "github_login"
max_age_seconds = 5400

[directory]
token_env = "MEADA_SCIM_TOKEN"
user_claim = "oid"
user_attribute = "externalId"
group_attribute = "externalId"
```

En Entra ID:

1. Registra la aplicación de Meada y emite los grupos en la claim `groups` (la claim de grupos del registro de aplicación). `oid`, el id de objeto inmutable del usuario, ya va en el token.
2. Crea una aplicación empresarial de aprovisionamiento en modo **Automático**, con la URL del inquilino `https://<consola>/scim/v2/` y el token SCIM como token secreto.
3. En el mapeo de atributos de **usuario**, envía `objectId` como `externalId` del usuario. En el de **grupo**, el `externalId` del grupo es el `objectId` del grupo, el valor que lleva la claim `groups`.
4. Mapea el login de GitHub a una claim de salida `github_login` (una política de mapeo de claims, desde un atributo de extensión que el administrador rellena).
5. Asigna usuarios y grupos a la aplicación e inicia el aprovisionamiento. Un usuario en muchos grupos recibe `_claim_names` en lugar de `groups`; con `overage = "directory"` los grupos salen del directorio, así que los grupos que importan a Meada deben estar asignados a la aplicación.

Documentación: [aprovisionamiento SCIM](https://learn.microsoft.com/entra/identity/app-provisioning/use-scim-to-provision-users-and-groups), [mapeo de atributos](https://learn.microsoft.com/entra/identity/app-provisioning/customize-application-attributes), [claims opcionales y mapeo de claims](https://learn.microsoft.com/entra/identity-platform/optional-claims) y [exceso de grupos](https://learn.microsoft.com/entra/identity-platform/id-token-claims-reference).

## Okta

```toml
[oidc]
type = "okta"
issuer = "https://<org>.okta.com/oauth2/<servidor>"
public_url = "https://mcp.ejemplo.internal"
groups_claim = "groups"

[oidc.groups]
"meada-lectores" = ["github.com/acme/api"]

[oidc.github]
login_claim = "github_login"
max_age_seconds = 5400

[directory]
token_env = "MEADA_SCIM_TOKEN"
user_claim = "preferred_username"
user_attribute = "userName"
group_attribute = "displayName"
```

En Okta:

1. Crea una app **SCIM 2.0** (modo de aprovisionamiento SCIM) con la URL base `https://<consola>/scim/v2/` y autenticación por **HTTP Header** con el token SCIM como bearer. Okta envía las actualizaciones de usuario con `PUT`, que el endpoint acepta.
2. Habilita crear, actualizar y desactivar usuarios, y el push de grupos.
3. La claim `user_claim` debe tener el mismo valor que el `userName` de SCIM (el nombre de usuario de Okta); Meada lo compara sin distinguir mayúsculas. Ajusta la claim en el servidor de autorización si el token no la trae.
4. En el servidor de autorización, añade una claim `groups` (filtro de grupos) y una claim `github_login` con el valor del atributo de perfil que guarda el login de GitHub.
5. Con `scopes` definido, recuerda que Okta envía `scp` y Meada lee `scope`: crea la claim `scope` ([Autenticación y grants](admission.md)).

Documentación: [SCIM en Okta](https://developer.okta.com/docs/concepts/scim/) y [claims personalizadas](https://developer.okta.com/docs/guides/customize-tokens-returned-from-okta/).

## Keycloak

```toml
[oidc]
type = "keycloak"
issuer = "https://id.ejemplo.internal/realms/agentes"
public_url = "https://mcp.ejemplo.internal"
groups_claim = "groups"

[oidc.groups]
"meada-lectores" = ["github.com/acme/api"]

[oidc.github]
login_claim = "github_login"
max_age_seconds = 5400
```

En Keycloak:

1. En **Client scopes**, crea o edita el scope del cliente de Meada y añade un mapper **User Attribute** que lea el atributo del usuario con el login de GitHub hacia la claim `github_login` del token de acceso.
2. Añade un mapper **Group Membership** para la claim `groups` (sin la ruta completa, si los nombres de grupo son simples).

Keycloak **no tiene push SCIM propio**. `[directory]` solo funciona con un aprovisionamiento que envíe usuarios y grupos al endpoint SCIM (un puente de aprovisionamiento que mantienes tú). Sin él, deja `[directory]` desactivado: el grant de las claims y el de GitHub funcionan igual, pero el selector de owners, `owner_missing`, el desaprovisionamiento en la consola y el overage de Entra (que no aplica a Keycloak) quedan fuera.

Documentación: [protocol mappers](https://www.keycloak.org/docs/latest/server_admin/#_protocol-mappers).

## Un token para el servidor de propuestas

El [servidor de propuestas](named-workspaces.md#ejecutar-el-servidor-de-propuestas) admite un JWT bearer cuyo `aud` es `[proposals] audience` (su `public_url` si no se define), firmado por el `[oidc] issuer` de su bootstrap (el de los nodos y de la consola, salvo que le dé uno propio, como exige Okta). Un token de nodo o de la consola se rechaza allí (401), y el token de este servidor lo rechazan ellos. El token debe ser **de una persona**: el login, los grupos y el rol salen de él; un token sin persona (el grant de credenciales del cliente) queda `pending` con `missing_login` y no selecciona nada. Un agente actúa por un operador al portar el token de ese operador, con vida corta, pues la vida del token del proveedor es el límite contra el abuso ([ADR 0022](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0022-authenticated-workspace-proposals.md)).

En cada proveedor siguiente, registre el servidor de propuestas como un recurso propio y obtenga el token con un **cliente público sin secreto**: Authorization Code con PKCE S256 para una persona en el navegador, o el device authorization grant para un agente en la terminal (la persona aprueba el código y el agente recibe el token). Antes de configurar el agente, decodifique el payload del token y compare `aud` con `[proposals] audience`, `iss` con `[oidc] issuer`, `exp`, y la claim de login (`github_login`) y los grupos que exigen las otras recetas: `printf '%s' "$TOKEN" | cut -d. -f2 | tr '_-' '/+' | base64 -d 2>/dev/null`. Los nombres de claim y de pantalla vienen de lo que cada producto documenta; contrástelos con su documentación vigente.

### Entra ID

1. Registre una segunda aplicación para el servidor de propuestas. En **Exponer una API**, defina un URI de ID de aplicación y un ámbito (por ejemplo `propose`). En su manifiesto, defina `requestedAccessTokenVersion` en `2`, para que `aud` sea el ID de cliente (de la aplicación): `[proposals] audience = "<id de la aplicación>"`.
2. Emita las mismas claims que emite la aplicación de los nodos: `groups` y `github_login` (la política de asignación de claims y la claim de grupos valen para cada aplicación por separado).
3. En el registro del cliente del agente (un cliente público, con el URI de redirección o el flujo de dispositivo habilitado), conceda el permiso delegado `propose` de la aplicación de propuestas.
4. Una persona obtiene el token con el ámbito `<URI de ID de aplicación>/propose` (por defecto `api://<id de la aplicación>/propose`) en `https://login.microsoftonline.com/<tenant>/oauth2/v2.0/` (`authorize` y `token`, o `devicecode` y `token` para un agente). No use `.default` con un secreto de cliente: ese es el grant de credenciales del cliente, que no lleva persona.

Documentación: [exponer una API](https://learn.microsoft.com/entra/identity-platform/quickstart-configure-app-expose-web-apis), [flujo de código de dispositivo](https://learn.microsoft.com/entra/identity-platform/v2-oauth2-device-code) y [claims del token de acceso](https://learn.microsoft.com/entra/identity-platform/access-token-claims-reference).

### Okta

El `aud` de un token de acceso de Okta es el **Audience** del servidor de autorización personalizado que lo emitió, un valor único, y los nodos, la consola y el servidor de propuestas necesitan tres audiences distintas. El servidor de propuestas es un proceso propio, con bootstrap propio, así que puede leer un segundo servidor de autorización personalizado: cree uno cuyo Audience sea `[proposals] audience`, con las mismas claims `groups` y `github_login` y los mismos usuarios, y apunte a él el `[oidc] issuer` del bootstrap del servidor de propuestas (las demás claves de `[oidc]` como antes). Una persona o un agente obtiene entonces el token de ese servidor con los flujos anteriores. Las pruebas de Meada no cubren este camino, y el chart Helm renderiza un solo `[oidc]` para todos los pods, así que por el chart el servidor de propuestas lee el emisor de los nodos y, con Okta, solo es alcanzable con los tokens de agente de las cuentas locales ([límites conocidos](enterprise.md#limites-conocidos)). No defina `[proposals] audience` como la audience de un nodo para sortearlo: eso vuelve válido en el servidor de propuestas el token de un nodo y no cierra nada del confused deputy.

Documentación: [servidores de autorización](https://developer.okta.com/docs/concepts/auth-servers/).

### Keycloak

1. En **Client scopes**, cree un ámbito `meada-proposals` y agregue un mapper **Audience** con **Included Custom Audience** igual a `[proposals] audience` y **Add to access token** activado.
2. Cree el cliente del agente como cliente **público** con el flujo estándar (método PKCE `S256`) o el OAuth 2.0 device authorization grant, y agréguele `meada-proposals` como ámbito **opcional**. Los mappers `github_login` y `groups` de la otra receta también deben alcanzar a ese cliente (póngalos en un ámbito que el cliente tenga, o en su ámbito dedicado).
3. Pida `scope=meada-proposals` en `protocol/openid-connect/auth` y `token` del realm (o `auth/device` y `token`). Un token sin ese ámbito no lleva esa audience y se rechaza.

Documentación: [mapper de audience y client scopes](https://www.keycloak.org/docs/latest/server_admin/#_audience) y [device authorization grant](https://www.keycloak.org/docs/latest/securing_apps/#_oauth2_device_authorization_grant).

### Cuentas locales

Con las [cuentas locales](console.md#cuentas-locales) no hay proveedor: la página de perfil de la consola (o `POST /api/v1/account/tokens` con `{"surface": "proposals"}`, cuando el bootstrap de la consola tiene `[proposals]`) emite un token de agente para el servidor de propuestas, con audience `meada:proposals`.
