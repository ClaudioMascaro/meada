# Flow

`Engine::open(dir)` finds the **workspace**, creates or recreates each **repository**'s **index**
where `Repository::index` says it lives, and keeps one connection per repository (`Engine::dbs`,
workspace order; SQLite attaches at most 10 databases to one connection, so none is attached):
`Engine::exports` reads each index through its own, so a workspace has any number of
repositories. Locally the index is the checkout's own `.meada/index.db`. In
**server mode**, `Engine::open_manifest(manifest, repo)` builds the workspace from the
**workspace manifest** instead (`workspace::Manifest`: every listed repository, `repo` first,
then manifest order, nothing discovered; under an OIDC grant only the granted ones) and each
index lives at `<index_dir>/<identity>/index.db`; `engine::index_workspace` and
`engine::probe_workspace` are `meada index --workspace` and `meada status --workspace`.
Every query (`export`, `search`, `context`, `trace`) calls `refresh()` first, reads the indexes
(`search`, `context` and `trace` a partial export, `Engine::partial`), adds the contracts each
`meada.toml` declares, derives the **links** and renders; nothing outside the index's directory
is written, and everything the engine returns is sorted, so the same files give byte-identical
output. The rest of the flow (the read-only open of server mode, generations, the SQL filters of
the partial export, `safe.directory`): [map/flow.md](../flow.md).
