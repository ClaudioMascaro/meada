# Named workspaces: versions and overlap (#343)

`tests/it/ui/oidc/workspaces/lifecycle.rs` runs on the same case as `workspaces.rs` (console, job,
fake GitHub, Valkey, Postgres, S3-compatible store, the Y/A/B table): the lifecycle along the base
(save moves nothing, apply, rollback marked as such, stale base, `already_current`, unknown
version, rename, delegation by the administrator alone, the last operator, delete as one 404
for everyone, the same triple still `workspace_deleted`, versions and history in order, one
audit record per operation with its actor, authority and base); two writers of one base (one
wins, the other is `stale_base`, one history row per accepted change); partial revocation (the
creator keeps the workspace with a hidden count, saves without dropping what it cannot read, is
refused B with `hidden`, cannot apply what a lost role no longer covers, nothing moves; a
peer sees one 404 on the workspace, its versions and its history); overlapping Y+A and Y+B
(independent edits and deletes, one member and one bundle of Y, graph answers through the context
bounded by the grant, and once A loses its sponsor a generation recording the context with A
`excluded`, which the console and a node both hide from a reader of Y alone, #510); a
SCIM-deleted operator blocked on every lifecycle route until
provisioned active, a never-provisioned subject admitted (`directory`); and the `meada
workspaces` twins as the administrator with `cli` audit records. `meada-core/tests/
named_workspaces.rs` checks the `lifecycle` rows of the table (compose keeps hidden entries,
`changed` is what is rechecked) and `meada-core/src/named.rs` the diff.
