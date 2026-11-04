# Named workspaces: context configuration (#344)

`tests/fixtures/effective-configuration.json` is the table of ADR 0021 contract 6: entries as a
version document holds them, the facts the console reads from a generation (`held`, `own`,
`documents`, `excluded`) and the answer per entry (source, ignored blocks, coverage, diagnostic
codes), plus the context pair that shares Y. `meada-core/tests/effective.rs` reads it through
`meada_core::effective` (and the module's unit tests cover the same rules inline), and
`ui/tests/named-core.test.ts` through the built WebAssembly (`namedEffective`), with the check that
the view's annotations never enter a manifest's text or version id.

`tests/it/ui/oidc/workspaces/configuration.rs` runs on the same case as `workspaces.rs` (console,
job, fake GitHub, Valkey, Postgres, S3-compatible store, the Y/A/B table) with the mirror of Y
committed without a `meada.toml`: two contexts sharing Y declare different peers and serve hosts
and each view, recorded context and context export shows only its own (the peer links of Y exist
through one and not the other); before the job ingests, the view says pending and the workspace is
applied; after it, a repository's own `meada.toml` wins whole and the ignored block is said beside
the accepted apply; a configuration change is a new generation with the same `members` bundles,
the other context's recorded entry unchanged and a pinned generation's export keeping the old
configuration; a stale base is refused and the other workspace's base does not move; an earlier
version is viewed with `?version=`; an unrelated caller, a peer who operates nothing and an
unknown id get the discovery rule's answers. The refusals test posts reserved keys, a value or a
URL where a variable name belongs, a peer `path` and an unknown key (all 422 with nothing
stored), the stateless preview before saving (no version stored, a missing peer and an ignored
block shown, a repository the caller cannot read shown pending), a forged key (400) and an
anonymous call (401). `ui/tests/NamedWorkspaces.test.tsx` drives the editor: the canonical text,
the effective configuration shown in text before any POST of a save, the frozen reviewed payload
at the base read, the findings beside an applied change, a stale base in words, and no editor for
a viewer.

`tests/it/ui/oidc/workspaces/proposals.rs` (feature `proposals`) runs on the same case and adds the
proposal server (`meada workspaces mcp`, `[proposals]` appended to the bootstrap the console
read), called over MCP with tokens for its own audience: no token, a forged one, a console's and a
node's are a 401 naming the metadata, a browser a 403, and its token opens nothing on the
console; `tools/list` gives four read-only tools with closed schemas naming no authority; every
creation row of the Y/A/B table through `propose` is the console's decision, each refusal the
console's own error byte for byte; a hidden repository is counted and never named, hints and
`_meta` or headers asserting a role change nothing, a forged argument and every bound are
refused as invalid arguments, a body past 80 KiB is a 413; the catalog is the console's, empty
for a claims grant and pending for a missing login; the fake GitHub counts no request and stderr
holds no token, note or manifest. Over existing workspaces it lists and shows under the discovery
rule, proposes a change with its base, diff and effective view, refuses B and a peer, gives
equal bytes for equal calls, leaves the console's records, versions, history and audit as they
were, and the manifest is then saved and applied by a person in the console, after which the old
base is stale; the table's discovery rows go through `show`; once the creator loses A, its
proposals and `show` compose over A without naming it, even when it names A; a creation of a
workspace created and deleted is refused `workspace_deleted`. A set past `max_age_seconds` makes
every proposal pending `stale_set`; a server allowed no call in flight (`MEADA_PROPOSALS_CALLS`)
answers 503 `busy`; with `[directory]`, a deleted user is a 403 `deprovisioned` until it is
provisioned active again.
