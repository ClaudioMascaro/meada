# Identity provider recipes

Three recipes to set up Meada's [identity provider](enterprise.md#identity-provider): Entra ID, Okta and Keycloak. Each lists the bootstrap manifest keys, the steps on the provider's side and the provider's documentation. The claim and mapping names below come from what Meada requires and what each product documents; check them against the provider's current documentation before applying them, since products rename screens and options.

In all of them the SCIM URL is `<the console's public URL>/scim/v2/`, with the SCIM token as the bearer, and the GitHub login reaches the token through a claim that **the provider's administrator maps from a verified attribute**, never one the user edits.

## Entra ID

```toml
[oidc]
type = "entra"
issuer = "https://login.microsoftonline.com/<tenant>/v2.0"
public_url = "https://mcp.example.internal"
groups_claim = "groups"

[oidc.groups]
"<group object id>" = ["github.com/acme/api"]

[oidc.entra]
overage = "directory"        # or "ignore"

[oidc.github]
login_claim = "github_login"
max_age_seconds = 5400

[directory]
token_env = "MEADA_SCIM_TOKEN"
user_claim = "oid"
user_attribute = "externalId"
group_attribute = "externalId"
```

In Entra ID:

1. Register Meada's application and emit groups in the `groups` claim (the app registration's groups claim). `oid`, the user's immutable object id, is already in the token.
2. Create an enterprise provisioning application in **Automatic** mode, with the tenant URL `https://<console>/scim/v2/` and the SCIM token as the secret token.
3. In the **user** attribute mapping, send `objectId` as the user's `externalId`. In the **group** mapping, the group's `externalId` is the group's `objectId`, the value the `groups` claim carries.
4. Map the GitHub login to an outgoing claim `github_login` (a claims mapping policy, from an extension attribute the administrator fills).
5. Assign users and groups to the application and start provisioning. A user in many groups gets `_claim_names` instead of `groups`; with `overage = "directory"` the groups come from the directory, so the groups that matter to Meada must be assigned to the application.

Documentation: [SCIM provisioning](https://learn.microsoft.com/entra/identity/app-provisioning/use-scim-to-provision-users-and-groups), [attribute mapping](https://learn.microsoft.com/entra/identity/app-provisioning/customize-application-attributes), [optional claims and claims mapping](https://learn.microsoft.com/entra/identity-platform/optional-claims) and [groups overage](https://learn.microsoft.com/entra/identity-platform/id-token-claims-reference).

## Okta

```toml
[oidc]
type = "okta"
issuer = "https://<org>.okta.com/oauth2/<server>"
public_url = "https://mcp.example.internal"
groups_claim = "groups"

[oidc.groups]
"meada-readers" = ["github.com/acme/api"]

[oidc.github]
login_claim = "github_login"
max_age_seconds = 5400

[directory]
token_env = "MEADA_SCIM_TOKEN"
user_claim = "preferred_username"
user_attribute = "userName"
group_attribute = "displayName"
```

In Okta:

1. Create a **SCIM 2.0** app (SCIM provisioning mode) with the base URL `https://<console>/scim/v2/` and **HTTP Header** authentication carrying the SCIM token as the bearer. Okta sends user updates with `PUT`, which the endpoint accepts.
2. Enable creating, updating and deactivating users, and pushing groups.
3. The `user_claim` claim must carry the same value as the SCIM `userName` (the Okta username); Meada compares it ignoring case. Adjust the claim on the authorization server if the token lacks it.
4. On the authorization server, add a `groups` claim (groups filter) and a `github_login` claim with the value of the profile attribute that holds the GitHub login.
5. With `scopes` set, remember Okta sends `scp` while Meada reads `scope`: create the `scope` claim ([Authentication and grants](admission.md)).

Documentation: [SCIM at Okta](https://developer.okta.com/docs/concepts/scim/) and [custom claims](https://developer.okta.com/docs/guides/customize-tokens-returned-from-okta/).

## Keycloak

```toml
[oidc]
type = "keycloak"
issuer = "https://id.example.internal/realms/agents"
public_url = "https://mcp.example.internal"
groups_claim = "groups"

[oidc.groups]
"meada-readers" = ["github.com/acme/api"]

[oidc.github]
login_claim = "github_login"
max_age_seconds = 5400
```

In Keycloak:

1. Under **Client scopes**, create or edit the Meada client's scope and add a **User Attribute** mapper that reads the user attribute holding the GitHub login into the access token's `github_login` claim.
2. Add a **Group Membership** mapper for the `groups` claim (without the full path, if the group names are plain).

Keycloak **has no SCIM push of its own**. `[directory]` works only with a provisioning step that sends users and groups to the SCIM endpoint (a provisioning bridge you maintain). Without it, leave `[directory]` off: the claims grant and the GitHub grant work the same, but the owner picker, `owner_missing`, deprovisioning in the console and Entra overage (which does not apply to Keycloak) are left out.

Documentation: [protocol mappers](https://www.keycloak.org/docs/latest/server_admin/#_protocol-mappers).
