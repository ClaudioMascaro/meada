# Identity provider recipes

Three recipes to set up Meada's [identity provider](enterprise.md#identity-provider): Entra ID, Okta and Keycloak. Each lists the bootstrap manifest keys, the steps on the provider's side and the provider's documentation. The claim and mapping names below come from what Meada requires and what each product documents; check them against the provider's current documentation before applying them, since products rename screens and options.

Start with [local accounts](console.md#local-accounts) if you do not have an identity provider yet; these recipes are the next step, and the [migration](console.md#migrating-from-local-accounts-to-the-identity-provider) says what changes (`break_glass`, groups, tokens).

In all of them the SCIM URL is `<the console's public URL>/scim/v2/`, with the SCIM token as the bearer, and the GitHub login reaches the token through a claim that **the provider's administrator maps from a verified attribute**, never one the user edits.

## From local accounts to the identity provider

Each recipe below is the `[oidc]` that replaces `[accounts]`. Keep `[accounts] break_glass = true` with the keys, so that `admin` can sign in when the provider is down; `[accounts.groups]` and `[accounts.github]` go away, and the groups become the recipe's `[oidc.groups]`, with the values the provider's `groups_claim` claim carries. The full steps are in [Migrating from local accounts](console.md#migrating-from-local-accounts-to-the-identity-provider).

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

## A token for the proposal server

The [proposal server](named-workspaces.md#running-the-proposal-server) admits a bearer JWT whose `aud` is `[proposals] audience` (its `public_url` when unset), signed by the `[oidc] issuer` of its bootstrap (the nodes' and the console's, unless you give it its own, as Okta needs). A token for a node or for the console is refused there (401), and this server's token is refused by them. The token must be **a person's**: the login, the groups and the role come from it, so a token with no person (the client credentials grant) is `pending` with `missing_login` and selects nothing. An agent acts for an operator by holding that operator's token; with a short lifetime, since the provider's token lifetime is the abuse bound ([ADR 0022](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0022-authenticated-workspace-proposals.md)).

For each provider below, register the proposal server as a resource of its own, then get the token with a **public client with no secret**: Authorization Code with PKCE S256 for a person in a browser, or the device authorization grant for an agent on a terminal (the person approves the code, and the agent receives the token). Before configuring the agent, decode the token's payload and compare `aud` with `[proposals] audience`, `iss` with `[oidc] issuer`, `exp`, and the login claim (`github_login`) and groups the other recipes require: `printf '%s' "$TOKEN" | cut -d. -f2 | tr '_-' '/+' | base64 -d 2>/dev/null`. The claim and screen names come from what each product documents; check them against its current documentation.

### Entra ID

1. Register a second application for the proposal server. Under **Expose an API**, set an Application ID URI and one scope (for example `propose`). In its manifest, set `requestedAccessTokenVersion` to `2`, so that `aud` is the application's client (application) ID: `[proposals] audience = "<application id>"`.
2. Emit the same claims the node application emits: `groups` and `github_login` (the claims mapping policy and the groups claim apply to each application on its own).
3. In the agent's client registration (a public client, with the redirect URI or the device flow enabled), grant the delegated permission `propose` of the proposal application.
4. A person gets the token with scope `<Application ID URI>/propose` (by default `api://<application id>/propose`) at `https://login.microsoftonline.com/<tenant>/oauth2/v2.0/` (`authorize` and `token`, or `devicecode` and `token` for an agent). Do not use `.default` with a client secret: that is the client credentials grant, which carries no person.

Documentation: [expose an API](https://learn.microsoft.com/entra/identity-platform/quickstart-configure-app-expose-web-apis), [device code flow](https://learn.microsoft.com/entra/identity-platform/v2-oauth2-device-code) and [access token claims](https://learn.microsoft.com/entra/identity-platform/access-token-claims-reference).

### Okta

An Okta access token's `aud` is the **Audience** of the custom authorization server that issued it, a single value, and the nodes, the console and the proposal server need three different audiences. The proposal server is a process of its own with its own bootstrap, so it can read a second custom authorization server: create one whose Audience is `[proposals] audience`, with the same `groups` and `github_login` claims and the same users, and set `[oidc] issuer` in the proposal server's bootstrap to it (the other `[oidc]` keys as before). A person or an agent then gets the token from that server with the flows above. Meada's tests do not cover this route, and the Helm chart renders one `[oidc]` for every pod, so through the chart the proposal server reads the nodes' issuer and, with Okta, is reachable only with local accounts' agent tokens ([known ceilings](enterprise.md#known-ceilings)). Do not set `[proposals] audience` to a node's audience to work around it: that makes a node's token valid on the proposal server, which closes nothing of the confused deputy.

Documentation: [authorization servers](https://developer.okta.com/docs/concepts/auth-servers/).

### Keycloak

1. Under **Client scopes**, create a scope `meada-proposals` and add an **Audience** mapper with **Included Custom Audience** equal to `[proposals] audience` and **Add to access token** on.
2. Create the agent's client as a **public** client with the standard flow (PKCE method `S256`) or the OAuth 2.0 device authorization grant, and add `meada-proposals` to it as an **optional** scope. The `github_login` and `groups` mappers of the other recipe must reach this client too (put them in a scope the client has, or in its dedicated scope).
3. Ask for `scope=meada-proposals` at the realm's `protocol/openid-connect/auth` and `token` (or `auth/device` and `token`). A token without that scope carries no such audience and is refused.

Documentation: [audience mapper and client scopes](https://www.keycloak.org/docs/latest/server_admin/#_audience) and [device authorization grant](https://www.keycloak.org/docs/latest/securing_apps/#_oauth2_device_authorization_grant).

### Local accounts

With [local accounts](console.md#local-accounts) there is no provider: the console's profile page (or `POST /api/v1/account/tokens` with `{"surface": "proposals"}`, when the console's bootstrap has `[proposals]`) issues an agent token for the proposal server, with audience `meada:proposals`.
