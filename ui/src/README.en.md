# Console application

React pages, HTTP client and WASM core integration.

## Interface

Startup loads configuration, selects the authentication mode and passes the API client to the application. `[main.tsx:start]` `[App.tsx:App]` `[api.ts:Api]`

## Invariants

The authentication callback removes the code from the URL before discovery and exchange. An origin mismatch triggers a redirect in the issuer flow. `[main.tsx:start]` The callback consumes the PKCE verifier and requires the returned `state` to match the value saved in the tab. `[auth.ts:callback]`

## Side effects

The interface sends HTTP requests, uses per-tab session storage and renders results into the DOM. `[api.ts:Api]` `[main.tsx:start]` `[local.ts:LocalAuth]`

## Verification

From the repository root, run `npm --prefix ui run typecheck && npm --prefix ui test`.
