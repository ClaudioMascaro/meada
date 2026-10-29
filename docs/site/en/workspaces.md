# Workspace versions and owners

## Import and apply a version

The managed workspace separates repository entries from local modules. Prepare a file with
`[[repository]]`, using the job's mirror paths, and import an immutable version:

```toml
[[repository]]
repo = "github.exemplo.internal/acme/api"
path = "/var/lib/meada/mirrors/api"
```

```sh
meada workspace import repositories.toml --message "Add API repository" --workspace /etc/meada/job.workspace.toml
meada workspace list --workspace /etc/meada/job.workspace.toml
meada workspace show <version> --workspace /etc/meada/job.workspace.toml
meada workspace apply <version> --workspace /etc/meada/job.workspace.toml
meada workspace pull --workspace /etc/meada/job.workspace.toml
```

Import validates the same entries as the console editor; importing the same content again
preserves the id and the first write's metadata. `apply` moves the workspace pointer under
the lock without publishing a generation. `pull` installs that version locally; subsequent
commands read it without opening a connection during indexing. The job owns its bootstrap,
`index_dir`, mirrors and `local_dir`; it shares only the module sections pointing to the same
store and Valkey with the nodes and console. Run its entire refresh under one lock, with fetch
outside Meada:

```sh
meada snapshot lock --workspace /etc/meada/job.workspace.toml -- sh -ec '
  meada workspace pull --workspace /etc/meada/job.workspace.toml
  ./fetch-mirrors.sh
  meada index --workspace /etc/meada/job.workspace.toml
  meada snapshot publish --workspace /etc/meada/job.workspace.toml
'
```

## From version to generation

The published generation records the workspace version it came from. Applying an older version
prepares the next refresh; nodes keep serving the generation they hold until another publish
and pull. The editor validates, compares, saves a version and applies it separately.

The `ghcr.io/claudiomascaro/meada:<version>-cloud` image includes the frontend at
`/usr/share/meada/ui`. For a Cargo installation, use `cargo install meada --features cloud`,
download `meada-ui-<version>.tar.gz` from the same release, extract it and point `[ui] dir`
to the extracted directory. The version in `meada-ui.json` must match the binary; a frontend
from another version refuses startup. The [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) and [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/)
console profiles are off by default and own their bootstrap, admission and storage.

## Team ownership

!!! warning "Planned: S3"
    [S3, #294](https://github.com/ClaudioMascaro/meada/issues/294) defines team ownership and authorization. The versions and editor above are delivered in S2; the rules below are not yet available behavior.

An entry's `owner` is available as entry metadata (#322): one string of 1 to 256 characters, with no control character and no whitespace at either end, compared exactly, case included. `meada workspace import` stores it, `show` prints it right after `path`, `pull` writes it and it changes the version id; an entry without it keeps its id. Indexing and the bundle and generation ids never read it, and a bootstrap manifest's own `[[repository]]` entries refuse it, naming `source = "store"`. No rule reads it yet to decide who may write.

An entry will be able to have an `owner`, the exact name of an OIDC group. The global operator will delegate the first entry; a team operator will be able to change only entries owned by its groups that its grant lets it read. A viewer will query what its grant admits. Ownership will never widen the grant.

Setting, replacing **or clearing** an owner will be `owner_change`, reserved for the global operator. Saving a proposal will create an immutable version; applying the version and publishing a generation will remain separate actions. Applying only owner changes will not require indexing code again.

The diagram separates the delivered path (solid lines) from planned authorization (dashed lines). [S5](named-workspaces.md) will add named workspaces over shared indexing.

<!-- meada-graph:workspaces -->
