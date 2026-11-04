# Named workspaces: creation to query (#342)

`tests/it/ui/oidc/workspaces.rs` drives the console, the job and a node as processes over the
shared Y/A/B table (`tests/fixtures/named-workspaces.json`), the loopback issuer, the fake GitHub
API and the local Valkey, Postgres and S3-compatible store: the creation rows (allowed, refused
with `hidden`, pending, forged keys), peers listing and showing through their own grants, the
whole pipeline (pull, roles, index, publish, node pull, `/-/workspaces/<id>/<identity>` per
caller, one bundle of Y, `members` and `contexts` in the document, `state` `applied` then `live`)
and the sponsor rule (`excluded`, not a failure), and a repository selected in two spellings (the
deployment's `github.com/Acme/Y`, a workspace's `github.com/acme/y`) staying visible to the
workspace's non-operator grantees (#508). `meada-core/tests/named_workspaces.rs` reads
the same table through `meada_core::named`, and `ui/tests/named-core.test.ts` through the built
WebAssembly; `ui/tests/NamedWorkspaces.test.tsx` drives the page. Without services,
`tests/it/oidc.rs` serves a hand-made generation whose context records the web repository with
the gateway excluded (#510): a grant reading the web repository alone gets the unknown-id 404
and a list without the context, a grant reading both is admitted, the gateway is the 404 to every
grant, current and pinned alike; `snapshot/serve.rs`, `snapshot/pull.rs` and `workspaces.rs` hold
the units (`Held::admits`, the digest and checks of `excluded`, `membership`).
