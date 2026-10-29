# Recetas de proveedores de identidad

Tres recetas para configurar el [proveedor de identidad](enterprise.md#proveedor-de-identidad) de Meada: Entra ID, Okta y Keycloak. Cada una lista las claves del manifiesto de bootstrap, los pasos del lado del proveedor y su documentación. Los nombres de claim y de mapeo de abajo salen de lo que Meada exige y de lo que cada producto documenta; contrástalos con la documentación vigente del proveedor antes de aplicarlos, porque los productos renombran pantallas y opciones.

En todas, la URL SCIM es `<URL pública de la consola>/scim/v2/`, con el token SCIM como bearer, y el login de GitHub llega al token por una claim que **el administrador del proveedor mapea desde un atributo verificado**, nunca uno que el usuario edite.

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
