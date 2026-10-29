# Authentication and grants

The token proves identity; the grant determines which repositories may appear in an answer. OIDC is opt-in. Without it, admission belongs to the proxy. Even after a call is admitted, a cross-repository link appears only when the grant includes both ends.

<!-- meada-graph:admission -->

## Configure OIDC

**OIDC** (feature `oidc`; `cargo install meada --features oidc`). The default admission is still
the reverse proxy, as [server mode](server.md) explains; the OIDC module is the
alternative for operators who want Meada itself to check who calls. With an `[oidc]` section the
server becomes an OAuth 2.1 resource server, as the MCP authorization specification
(2026-07-28) describes: every request carries `Authorization: Bearer <JWT>` issued by the
operator's identity provider. Meada issues no token, registers no client and never forwards a token.

```toml
[oidc]
type = "generic"                                # default; "entra", "okta" or "keycloak"
issuer = "https://id.example.internal/realms/agents"
public_url = "https://meada.example.internal"   # the URL clients use, behind the proxy
audience = "https://meada.example.internal"     # optional; default: the endpoint's canonical URL
identities_claim = "meada_repositories"          # optional with groups_claim or [oidc.github]
groups_claim = "groups"                          # optional, together with [oidc.groups]
scopes = ["meada:read"]                          # optional, announced and required
jwks_refresh_seconds = 3600                      # default; at least 60

[oidc.groups]                                    # a token's group: the identities it grants
"team-payments" = ["github.example.internal/acme/payments-api", "github.example.internal/acme/ledger"]
"platform" = ["*"]
```

`type` says which identity provider issues the tokens: `generic` (the default), `entra`, `okta`
or `keycloak`. None changes the admission, the grant or an answer; the type shows on the
console's admission page. Only `entra` takes a table of its own, `[oidc.entra]`, with one key,
`overage`, which for now accepts only `"ignore"` (the default): a token in overage, with no
`groups`, has no groups. `[oidc.entra]` under another `type` is an error. `[oidc.github]`, the
GitHub grant provider, has its own section in [GitHub App credentials](github-app.md#role-check);
with it neither claim is required, and a token's grant becomes the union of its claims grant and
what its GitHub login reads.

## Prepare the identity provider

In the identity provider: an audience equal to `audience` (or, without it, to each endpoint's URL,
`<public_url>/<identity>`); a claim mapper that puts in `identities_claim` the list of identities
the client may read (`"*"` grants all), or in `groups_claim` its groups; and client credentials
for headless agents and for a portal's backend, which ask the identity provider for their tokens. A browser
page cannot call the MCP server directly: with `[oidc]`, any request carrying `Origin` gets 403, on every
path, so a portal calls from its backend. The proxy forwards `Authorization` and the SEP-2243
headers (`MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`, `Mcp-Param-*`) unchanged.

## Compute the grant

With `groups_claim`, the manifest's `[oidc.groups]` table maps each group to the identities it
grants (`"*"` for all), and `identities_claim` becomes optional: the two keys come together, and
a section with neither claim refuses the start. A token reads the union of the identities its
`identities_claim` lists and those its groups map to. A missing claim or one that is not an array
of strings, a group the table does not map and an identity the server does not serve grant
nothing; a typo shows up to the caller as a 403.

## Discovery and keys

<details markdown="1" class="reference-details">
<summary>Discovery, key caching and failures</summary>

At start, before it listens, Meada reads the issuer's metadata (RFC 8414 or OpenID Discovery, at
the paths and in the order the MCP specification gives, checking that the document's `issuer`
is the configured one) and the JWKS it names; a failure refuses the start. `issuer`,
`public_url` and the `jwks_uri` must be `https` unless loopback; neither request follows a
redirect or goes through a proxy, each has 10 s and a size cap, and certificates are verified
against the system's roots. The JWKS is read again every `jwks_refresh_seconds` and when a
token names an unknown `kid`, at most once a minute; when a refresh fails the cached keys stay
and stderr says so.

</details>

## Metadata and validation

<details markdown="1" class="reference-details">
<summary>Metadata and token validation rules</summary>

`GET /.well-known/oauth-protected-resource/<identity>` answers the resource's metadata (RFC
9728) without a token: `resource` is `<public_url>/<identity>`, `authorization_servers` is
`[issuer]`, `bearer_methods_supported` is `["header"]`, and `scopes_supported` is there when
`scopes` is set; `/.well-known/oauth-protected-resource/<identity>@<generation>` answers the
same with the pinned `resource`. A token passes with an asymmetric algorithm (RS256, RS384,
RS512, ES256, ES384, PS256) that fits its key, `iss` equal to `issuer`, an `aud` equal to
`audience` or, without it, to the URL of `/<identity>` (which pins accept too, so one token
reads a repository at every generation), `exp` present and `nbf` honoured with 60 s of leeway,
and every configured scope in its `scope`. A token in the query string is never read. `scope`
is read only as a space-separated string; `scp` and an array-valued `scope` are not read, so,
with `scopes` set, an identity provider that sends `scp` (Okta, for example) needs a claim mapper to
`scope`.

</details>

## Authorization responses

No token, or one that does not pass: 401 with `WWW-Authenticate: Bearer
resource_metadata="<public_url>/.well-known/oauth-protected-resource/<path>"`, plus
`error="invalid_token"` when a token was sent and `scope` when configured. A valid token that may
not read the endpoint's identity, on a pin too: 403 with `error="insufficient_scope"`; missing a
required scope, the same 403 with `scope`. Behind an endpoint the token may read, every answer
covers only the repositories it may read. Authentication runs before routing: the 404 at the root,
on an unknown path and for a generation the node does not hold needs a valid token for that path
(without `audience`, the root's `aud` is `public_url`) and lists only the identities it may read.
No token, and no part of one, appears on stderr or in an error.
