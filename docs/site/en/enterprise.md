# Enterprise identity

!!! warning "Planned: S4"
    [S4, #319](https://github.com/ClaudioMascaro/meada/issues/319) specifies identity automation, identity providers, grant providers and the data layer. No new configuration or command in this proposal should be treated as available. [OIDC admission through claims](admission.md) is already delivered.

## Authenticate, grant access and record

The identity provider will keep issuing the token. The effective grant will combine identities allowed by claims and, when configured, by GitHub permissions. The refresh job will check GitHub roles, never the console or indexing. Owners and human roles will not widen this grant.

Postgres will be an opt-in module for durable mutable state: audit, directory and permissions. The snapshot store will keep immutable documents and bundles; Valkey will keep pointers, locks, fences and caches. `meada index` will remain network-free.

## Directory and revocation

SCIM will populate the user and group directory, including groups exceeding Entra’s token claim limit. Deleting a provisioned subject will leave a durable block. A never-provisioned subject will retain normal admission: absence from the directory alone will not mean deletion.

The audit will record the attempt before the mutation. If that record fails, the write will be refused. Cache, expiration, outage and revocation details belong to the S4 spec; they are not guarantees of the delivered runtime.

<!-- meada-graph:scim-revocation -->

## Identity providers, grants and data

Dashed lines below represent planned integrations. The identity provider issues the token; the job maintains permissions; the console and nodes read the data required for admission. The directory and audit keep durable state separate from code content.

<!-- meada-graph:enterprise -->
