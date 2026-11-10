# Named workspaces

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

## Named workspaces (spec S5): built and validated end to end (#347)

ADR 0020 holds the domain decision, ADR 0021 the contracts (catalog, selected entries, shared
ingestion membership, version lifecycle in Postgres, query selection through the generation
document's `members` and `contexts` and the route `/-/workspaces/<id>/<identity>`, effective
configuration, ids) and ADR 0022 the proposal server. Slices #341 to #347 build them in that
order and each updates this map with what it delivers.

Delivered by #346 (ADR 0022's proposal server; ADR 0022 "As built (#346)"), feature `proposals`
(implies `workspaces`, enabled by `cloud`): `proposals.rs` is `[proposals]` (`public_url`,
`audience`), which needs `[workspaces]`; `ui/workspaces/proposals.rs` is `meada workspaces mcp`
(`ui::proposals`, a `WorkspacesCommand::Mcp` that `main` serves as it serves `meada ui`): a
`Console` started for it (`for_proposals`: every Postgres session read-only through
`Manifest::read_only` and `postgres::Config::read_only`, the audit off, no SCIM, front end or
GitHub App; `Holds::of`, `directory_of`, `grant::Providers::of` and `Session::admitted` are the
console's own), the hyper listener of `mcp::serve_http` with its own route (`Origin` 403, the protected resource metadata,
`Oidc::admit_console` with `[proposals] audience` and the directory's block, `/` only, a semaphore
of calls in flight, the body bound) and an rmcp `StreamableHttpService` of four read-only tools,
`catalog`, `workspaces`, `show` and `propose`, each calling the console's own functions
(`catalog` and `ui/catalog.rs` `checked`, `page` and `summary` of the list, `show`,
`visible_records`, `precheck`, `named::compose`, `changed`, `diff`, the effective view, and
`workspaces/store.rs` `creation_conflict`, the read of what a creation's transaction would refuse)
with the session in the request's extensions; a console answer becomes a tool's through
`from_console`. The node's `mcp.rs` and the console's routes are unchanged.

Delivered by #562 (named workspaces and the proposal server with local accounts, ADR 0024; no
new module): `Manifest::read` takes the GitHub grant provider of either table for `[catalog]`
(its `host`, the `catalog` flag set on whichever table it is, `workspace::PROVIDER` naming both
tables in the error of a build with `accounts`) and `[workspaces]` needs `[ui]` and `[catalog]`
(the catalog needing the provider), so `[proposals]` rides on either. The console code of
ADR 0021 was already provider-blind (`Session` carries the login, the operator flag and the
subject whichever verifier admitted it, #561), so eligibility reads the verified login, an
operator account is the platform administrator and operators, authors and history are `local:`
subjects with no change there. `POST account/tokens` (`Console::agent_token`) issues for
`accounts::PROPOSALS` (`meada:proposals`, surface `proposals`) when the bootstrap has
`[proposals]`, and `config` says `proposals: true`; `accounts::Agents` (formerly `Nodes`) is one
surface's admission (public half, issuer, audience), built by `Config::agents`. The proposal
server's `for_proposals` builds an `Agents` for `PROPOSALS` beside `[accounts]` with no `[oidc]`
(the public half read at start, the issuer the bootstrap's `[ui] public_url`) and `Server::admit`
hands it to `admit_agent`: a 401 `Bearer` (`error="invalid_token"` for a token sent) without
`resource_metadata`, else `Session::local` of an `Admitted` built from the claims alone
(subject, role, groups, login; `Providers::local` for the grant; no team, no row read). The
metadata path is then a 404, as it is without `[oidc]`.

Delivered by #345 (the guided controls; ADR 0021 "As built (#345)"), front end only plus two
WebAssembly exports (`namedDiff` and `namedChanged` in `meada-core/src/wasm.rs`, over `named::diff`
and `named::changed`): `EditWorkspace.tsx` is the operator's editor (it replaces #344's text-only
`EditWorkspace` in `NamedWorkspacesPage.tsx`), holding the manifest text and a model of its entries
that rewrite each other through `namedEntriesText` and `namedEntriesJson`, so the form and the text
are one canonical document; `GuidedForm.tsx` is the form (repositories from the catalog, per entry
serve settings, resources and peers with environment-variable names) and decides nothing;
`guided.ts` only reshapes entries (`normalize`) and words a diff, `guided-i18n.ts` is its pt-BR/en
copy; `useCatalog.ts` reads the catalog page by page, remembers the identities the server said
the session may select and the `catalog/check` results, and is shared by the creation form and the
editor; `EffectiveList.tsx` is #344's view, extracted. The editor marks each changed entry
(`namedChanged`) with `namedAuthorize`, shows the diff (`namedDiff`) and freezes the reviewed
payload until the confirmation. Tests: `ui/tests/guided.test.ts` (actual WebAssembly) and
`ui/tests/GuidedWorkspace.test.tsx` (DOM). Proposals are #346 (above); the page over versions
with rollback is #503 (below).

#501 moved the text rule and the role list to the same seam: `namedNameProblem`,
`namedDescriptionProblem` and `namedMessageProblem` (over `named::*_problem`) and `resourceRoles`
(over `manifest::Role::ALL`), so `ui/` keeps no copy of the rule, its bounds or the roles.
`message_problem` also refuses an empty message (1 to `DESCRIPTION_MAX`), so the console routes and
`meada workspaces` carry no empty check of their own. `ui/src/named-core.ts` (#522) is the one place
that parses the JSON of the `named*` and `resourceRoles` results, into typed values; `messageOf` in
`ui/src/api.ts` is the console's `ApiError` message with a fallback.

Delivered by #344 (contract 6, the effective configuration; ADR 0021 "As built (#344)"), same feature:
`meada-core/src/effective.rs` is the pure decision (`effective(entries, facts)`: per entry the
`Source` (the repository's own `meada.toml` whole, the context's blocks, none), the ignored
blocks, the `Coverage` and the diagnostics, with `labels` wording a block as `status` does),
exported to WebAssembly as `namedEffective`; `snapshot/serve.rs` `Held::facts` is the one place
that reads a mirror for it (its `meada.toml` and nothing else, bounded) and decides whether a
bundle indexes documents; `ui/workspaces/effective.rs` is `GET /api/v1/workspaces/<id>/effective`
(through `inspect`, under the discovery rule, entries filtered by the caller's grant, the
`excluded` marks for operators), the stateless `POST /api/v1/workspaces/effective` preview and the
`diagnostics` that creation, save and apply answers carry (`written_diagnostics`). Nothing enters
a bundle id or the generation id: the ingestion-blocks rule of #342 already keeps a context's
peers, resources and serve hosts query-time, so a configuration change is a new generation of the
same bundles. The front end's `NamedWorkspacesPage.tsx` shows the view in text and has the
manifest text editor (`EditWorkspace`: validate with the core, preview, confirm, save and apply at
the base read). Not built by #344: guided authoring (#345, above), proposals (#346, above),
acceptance (#347).

Delivered by #503, same feature, no route added: `ui/src/WorkspaceVersions.tsx`, rendered by
`NamedWorkspacesPage.tsx` under a workspace's detail for every role, reads `versions[?after=]`,
`versions/<version>?against=current` and, for `detail.operate` alone, `history[?after=<seq>]`,
and shows the answers as the server decides them (the diff is the route's, never recomputed).
An operator applies a listed version through `ConfirmDialog`, with the `base` of the answer it
read; `stale_base` and `entries_refused` show the server's message and read the list again. The
rollback audit is `workspaces.rollback` (`tests/it/ui/oidc/workspaces/lifecycle.rs`,
`versions_are_saved_applied_and_rolled_back_along_the_base_as`).

Delivered by #343 (contract 4's lifecycle; ADR 0021 "As built (#343)"), same feature:
`workspaces/lifecycle.rs` is the atomic boundary of every other write (`transact`: one
transaction, `SELECT ... FOR UPDATE` on the workspace row, then deleted, base, authority and the
eligibility of the entries an operation adds or changes from the newest set via
`store::eligible`, then the move and the history row) for `Op::Save`, `Apply` (a rollback when
the version was ever current), `Rename`, `Delete` and `Operators`, and the reads of versions and
history (`versions`, `version`, `histories`; the opaque cursor is a version's UTC microsecond time
and its id); `workspaces/store.rs` gained `eligible` (shared with `create`) and the store's
`put_document` and `get_document`; `ui/workspaces/lifecycle.rs` is the console's routes
(`POST /api/v1/workspaces/<id>/versions|apply|rename|delete|operators`, `GET .../versions[/<v>]`
and `.../history`, the body caps in `body_max` that `ui.rs`'s `read` asks for), which resolve the
operation against the stored current version (`named::compose` keeps the entries an operator
cannot read, `named::changed` is what is rechecked, `named::diff` the diff of a version read)
before the pending audit record; `workspaces/cli.rs` is `meada workspaces list|show|versions|diff|
create|save|apply|rename|delete|operators` (the platform administrator, `audit::cli`); migration 8 is
`workspace_versions.entries`; `audit.rs` knows the six new actions and the `base` argument. Not
built: `effective` (#344, above) and the guided pages over versions and the diff (#345).

Delivered by #342 (contracts 2 to 5 as far as create, list, show, shared ingestion and the node
route go; ADR 0021 "As built (#342)"), same feature: `workspaces.rs` is `[workspaces]` (`mirrors`,
`max_workspaces`, `max_entries`), the contexts file (`Context`, `read_contexts`, `write_contexts`,
checked as hostile input) and `membership` (the contexts the sponsor rule left entries in, and the
repositories only they select); `workspaces/store.rs` is the Postgres side (`create`, the one
transaction that decides eligibility from the newest set; `records`; `current_contexts` for the
pull; the version document's text and check); migration 7 adds the four tables, `workspaces.entries`
and the `context` column of `permission_exclusions`; `ui/workspaces.rs` is `POST` and `GET
/api/v1/workspaces[/<id>]` and the export through a context, and `Console::open` takes the
optional workspace the playground and the export ask through; `snapshot/versions.rs` pulls the
contexts beside the deployment version (`Pulled::Empty`), reads and writes the exclusions file
with its context lines and digest (`excluded_contexts`, `write_excluded_with`); `github_app.rs`
lists the contexts' identities and applies the sponsor rule (`no_sponsor`);
`workspace.rs` gives `Listed` its `member` flag, builds the members after the deployment entries
from `membership`, and `Manifest::of_context` builds a recorded context; `snapshot.rs` records
`members` and `contexts` in the document (`Context::digest`) and `generation_id_of` hashes them,
each context with the identities the sponsor rule excluded from its version (`excluded`, which
`membership` keeps and the digest covers, absent when there is none, #510);
`snapshot/pull.rs` checks them; `snapshot/serve.rs` `Held::context` builds a context once, and
`Held::admits` and `Held::viewable` decide a workspace route and its 404 list with
`named::visible` over every entry of the recorded version, excluded ones included, with no
operator exemption (the token's grant through `From<&grant::Grant>` for `meada_core::authorize::Grant`, as the console maps it);
`mcp.rs` routes `/-/workspaces/<id>/<identity>` and `Engine::open_context` opens it;
`meada-core/src/named.rs` holds the pure rules (`creation`, `visible`, `answers`, `shown`,
`unsponsored`, names, named entries and their version id) that the Rust table test, the console
and the front end's WebAssembly (`namedAuthorize`, `namedVisible`, `namedEntriesJson`,
`namedEntriesText`, `namedVersionId`) share, with `validate_named` in `manifest.rs`; the front
end's `NamedWorkspacesPage.tsx` lists, shows and creates. Not built by #342: save, apply,
rollback, rename, delete and delegation (#343, above), the effective configuration (#344), guided
authoring (#345) and proposals (#346).

Delivered by #341 (contract 1, the catalog), behind the cargo feature `workspaces` (it implies
`ui`, `oidc`, `postgres`, `github-app`, `generation-pointer` and `refresh-lock`; `cloud` turns it
on): `catalog.rs` is `[catalog]` (`allow` patterns on `[oidc.github] host`, `max_repositories`
1 to 5000, no default; `Manifest::read` needs `[oidc.github]` and reads it in the job, the CLI
and the console alike); `github_app/catalog.rs` is the job's installation listing
(`GET /installation/repositories`, every page, for each installation of the host's Apps whose
owner some pattern names, under ADR 0013's tokens and throttle marks; a failure, a throttle or a
set above the cap fails the run before anything is written); migration 6 is
`permission_repositories.source` (`selected`, `catalog` or `both`); `grant/permissions.rs` loads
the catalog beside the rows (`Catalog`, the identities `catalog` or `both` and `listed`, only
with `[catalog]`) and answers per subject (`Permissions::catalog`: the whole fresh catalog for the
platform administrator, else the catalog intersected with the identities the login holds an
accepted role on, pending `no_set`, `stale_set` or `missing_login`); `ui/catalog.rs` is `GET
/api/v1/catalog` and `POST /api/v1/catalog/check` (`Catalog` carries the host, and the listing's
prefix and the check compare folded, #508); the front end's `CatalogPage.tsx` is the
picker. Nothing else of S5 exists yet: no workspace record, route, table beyond that column or
manifest, and discovery creates no ACL and no index. Until #342 a reader of the code finds one
workspace per deployment (ADR 0014); `tests/fixtures/named-workspaces.json` is the shared Y/A/B
table the catalog's tests read.

Delivered by #347 (acceptance and operations; no new module): `tests/it/ui/oidc/workspaces/acceptance.rs`
forces the roles, fetch/index and local-publication failures of one refresh in turn and shows the
pointer stays, the previous permissions set keeps deciding, a revocation the job committed stays
effective whatever happens to the publication and a grant returns only through a later complete
set. The Compose and Helm profiles carry the sections (`[catalog]`, `[workspaces]` and the `[ui]`
block the job then needs, never a node's: `deploy/helm/meada/values-workspaces.yaml`,
`templates/_workspaces.tpl`, checked by `scripts/helm-check`), and the site's named-workspaces page
(`docs/site/<lang>/named-workspaces.md`, changed with the console's workspace pages and the
`[workspaces]` profile) holds the upgrade procedure, the failure table and the acceptance map. The proposal server is not
packaged in either profile. The real-organization measurement (#338) stays open and does not gate S5.
