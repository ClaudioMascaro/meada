# 21. Named workspaces: the contracts of spec S5

- Status: accepted
- Date: 2026-10-30

## Context

ADR 0020 accepted a direction on 2026-10-23: named query contexts over shared indexing, with
access derived from repository roles, and left the contracts to spec S5 (#339). S3 (ADR 0015)
and S4 (ADR 0016) have since merged, so the contracts can be written against the code that
exists: the workspace version documents and the fenced `{<key_prefix>}:workspace` pointer
(`snapshot/versions.rs`), the pure `authorize_entries` and `compose_entries` of `meada-core`, the
Postgres data layer with its permissions sets, exclusions, audit and directory, the job's
`meada github-app roles`, the generation document and its ids, and the nodes' routes and pins.

This ADR is the consolidation #340 owes. It fixes five contracts that must not be fused (catalog,
selected entries, shared ingestion membership, version lifecycle, query selection), the
effective configuration, the identifiers, the console API and the order in which the remaining
slices build them. ADR 0020 holds the domain decision it relies on (roles, eligibility, the
Y/A/B matrix, discovery, revocation); ADR 0022 holds the proposal interface.

The maintainer's decision of 2026-10-29 applies: the real-organization measurement of `meada
github-app roles` (#338) does not gate S5. This ADR consolidates against the merged code and
the existing fakes, and the assumption real evidence would confirm is named under Open.

Implementation status: #341 built contract 1 (the catalog, below) and #342 the creation and the
query of the first workspace (contracts 2 to 5 as far as create, list, show, the shared ingestion
and the node route go, "As built (#342)" below); the cargo feature `workspaces` has `[catalog]`
and `[workspaces]`. Save, apply, rollback, rename, delete and delegation (#343), the effective
configuration (#344), guided authoring (#345) and proposals (#346) are not built. Current behavior
without `[workspaces]` is ADR 0014's one workspace per deployment.

## Decision

### Capability and sections

- The capability is a cloud module in ADR 0012's shape: the cargo feature `workspaces`, off by
  default, implying `ui`, `oidc`, `postgres`, `github-app`, `generation-pointer` and
  `refresh-lock`, enabled by `cloud`. A `[workspaces]` section in a build without the feature is
  the usual error naming the file, line, section and feature.
- `[workspaces]` holds `mirrors` (required: an absolute directory, the root of the mirrors of
  repositories only named workspaces select), `max_workspaces` (default 200, 1 to 2000) and
  `max_entries` (default 100, 1 to 500, per workspace). It needs `[ui]` with `operations =
  true`, `[oidc.github]` (so `[postgres]`), `[catalog]` and `[workspace] source = "store"`, each
  error naming both sections.
- `[catalog]` holds `allow` (required, non-empty: patterns of the form `<host>/<owner>/*` or
  `<host>/<owner>/<repo>`, compared exactly; the host is `[oidc.github] host`) and
  `max_repositories` (required, 1 to 5000, no default, since the cost of expanded coverage is
  unmeasured).
- Without `[workspaces]` every process behaves as ADR 0014, 0015 and 0016 leave it, byte for
  byte: sessions, routes, generation documents, ids, audit and every golden. The sections are
  read by the console, the job and the CLI twins; a node needs the binary, not the section.
- The deployment workspace (ADR 0014's one workspace, the pointer `{<key_prefix>}:workspace`)
  keeps its owners, teams, `added_by`, exclusions and CLI twins unchanged. With `[workspaces]`, a
  deployment with no current version is an empty deployment workspace instead of the error
  `meada workspace pull` gives, so a deployment may start from named workspaces alone.

### Contract 1: the catalog (what could be selected)

The catalog says which repositories could enter a manifest. It is not an entitlement, not a
workspace and not an index.

- **Coverage** is produced by the job. `meada github-app roles` (ADR 0016) lists, per
  `[[github_app]]` of the `[oidc.github]` host, the repositories each installation covers
  (`GET /installation/repositories`, repository permission Metadata (read), under ADR 0013's
  selection, throttle marks and token cache) and keeps those matching `[catalog] allow`. It adds
  the identities the deployment workspace and every named workspace currently select, so selected
  repositories always have rows. It then lists collaborators of each as it does today. The whole
  result is one permissions set in one transaction, fenced as today. A candidate set above
  `max_repositories` (selected identities not counted), a throttled or failed listing, or an
  installation listing that fails makes the command fail and write nothing: a partial set would
  revoke or mix, and the previous set ages out.
- The set's `permission_repositories` rows gain `source` (`selected`, `catalog` or `both`,
  default `selected`) in migration 6. Their `coverage` stays `listed`, `not_found`, `uncovered`
  or `not_a_repository`. The catalog is the rows with `source` `catalog` or `both` and coverage
  `listed`, in the newest set. It is independent of the applied indexing manifest: a repository
  nobody selected is in it.
- **Freshness** is the set's age against `[oidc.github] max_age_seconds`, measured as the grant
  measures it (Postgres's clock plus monotonic time since the last read).
- **Selectability.** A repository is selectable for a subject when the catalog holds it, the
  newest set is fresh, the subject has a login (`login_claim`) and that login holds an accepted
  role on it in that set. The platform administrator's selectable set is the whole fresh
  catalog. Nothing else makes a repository selectable: App coverage, a raw group name from a
  token, a claims-granted identity and anything an agent says are not entitlement.
- **States.** The answer to a subject has two global states, `ready` and `pending`, and, for an
  identity the caller names, `selectable`, `unavailable` or `pending`:
  - `pending` is a condition of the platform or of the caller, the same for every identity:
    `no_set` (no set yet), `stale_set` (past `max_age_seconds`) or `missing_login`. While
    pending nothing new can be selected (fail closed) and entries already in a manifest stay.
  - `unavailable` is every other non-selectable case (no accepted role, not in the catalog, not
    covered, new since the last set) with no reason: a reason would be an oracle on the
    catalog. The catalog read never lists an unavailable repository.
- **Reads.** `GET /api/v1/catalog?limit=&after=&q=` returns `{"state","reason","set":
  {"finished","age_seconds","max_age_seconds"},"repositories":[{"identity"}],"next"}`: only
  selectable identities, ordered by identity, `limit` 1 to 100 (default 50), `q` a prefix of at
  most 128 characters, `after` an opaque cursor of at most 256 characters that must decode to an
  identity. `POST /api/v1/catalog/check` takes `{"identities":[...]}` (at most 50, each checked
  as an identity) and returns `{"state","reason","results":[{"identity","state"}]}`. Both need an
  admitted, non-deprovisioned subject. Answers carry no identity the caller cannot select.
- **Cost.** The job pays `ceil(covered / 100)` pages per installation listed (`covered` counts
  every repository the installation covers, not only those `allow` admits) and, per candidate,
  `ceil(collaborators / 100)` collaborator pages, on top of ADR 0016's ceiling. Expanded
  coverage is enabled only by an operator who set `max_repositories`; the base-role upgrade path
  ADR 0016 names (list the base role's members once) is the first remedy when measurement says
  the hour is exceeded.
- Discovery creates no workspace, ACL or index, and the console and the nodes still never call
  GitHub (ADR 0012, ADR 0016).
- **As built (#341).** The listing lives in `github_app/catalog.rs` and only asks installations
  whose owner some `allow` pattern names. An App with no installations held and no recent
  listing (throttled, or another run's listing unfinished when the lock expires) fails the run;
  while it is throttled, the installations it holds (at most an hour old) are used, as for the
  selected repositories. `Permissions` (`oidc/permissions.rs`) loads the catalog
  with the rows only when `[catalog]` is present, and answers per subject; its identities are at
  most 192 bytes (the cursor is base64url of the identity within the 256 characters above). A
  `check` repeating an identity answers it once, and a body or query past its bounds is one fixed
  400. `GET /api/v1/manifest` hides `catalog.allow` from everyone but the platform administrator,
  since the patterns name owners nobody may have been granted. The platform administrator is the
  session the console already treats as the global operator.

### Contract 2: selected entries (what a workspace declares)

- A **named workspace** is a record with an `id`, a `name`, a `description`, its operators and a
  current version. It is stored in Postgres (below) because Valkey is not durable and the store
  is immutable.
- **`id`** is the first 16 hex characters of the BLAKE3 hash of the canonical JSON array
  `["meada workspace", <creator subject>, <name>, <first version id>]`: deterministic, immutable
  after creation, unaffected by a rename, delegation or deletion. Creating the same triple again
  returns the existing workspace to its creator and creates nothing; if it was deleted the
  answer is 409 `workspace_deleted`, and a new workspace needs a different name or entries. **Names are not unique**:
  two workspaces may share one, and `name_taken` does not exist, so creating a workspace reveals
  nothing about workspaces the caller cannot see (ADR 0020). `name` is 1 to 80 characters,
  `description` 0 to 500, both without control characters and without whitespace at either end.
- A **manifest** is the text of `[[repository]]` entries, the same grammar, parser, diff and
  composition as the deployment workspace (`meada-core`), shared by the console, WebAssembly, the
  CLI twins and the proposal interface, with these rules for a named workspace's entries,
  checked where the entry is read so every save, import, pull and proposal checks them alike:
  - `repo` is required, an identity (`identity_problem`), unique in the workspace, and in the
    catalog at the moment of an addition.
  - `path` is refused (`reserved_key`): physical mirror paths are the platform's (below).
  - `owner` and `added_by` are refused: an owner is not a named-workspace ACL (ADR 0015, ADR
    0020), and eligibility is read from the permissions set, not from an adder.
  - `[repository.serve]` takes `hosts`, `prefix` and `strip`; `documents` is refused
    (`reserved_key`), since it decides what is indexed (Contract 6).
  - `[[repository.resource]]` and `[[repository.peer]]` take their `meada.toml` keys. A peer's
    `path` is refused. A peer's `base_url_env` entries are references: names matching
    `^[A-Za-z_][A-Za-z0-9_]{0,127}$`, never a value, URL or `NAME=value`. Meada never reads the
    variables' values (as today).
  - At most `max_entries` entries and 64 KiB of text.
- The **version id** is `workspace_version_id` over the entries. A named entry's canonical text
  has no `path` key, so a named version never equals a deployment version. That is a change to
  the function (`VersionEntry` takes the path as optional, `entries_text` and the entry builder
  likewise) that leaves every existing entry's text and id unchanged, and the same entries in two
  workspaces are one version.
- A named version's **store document** is `workspaces/<version id>.json` holding the id and the
  entries only: no author, time or message, which would let one workspace read another's through
  the shared document. Those live in the workspace's own `workspace_versions` rows.
- Environment references, resources and serve settings are declarations of the context. Physical
  mirror paths, credentials and module settings stay in the bootstrap manifest.

### Contract 3: shared ingestion membership (what the fleet indexes)

- The **ingestion membership** is the set of repositories one refresh fetches, indexes and
  publishes: the deployment workspace's entries after ADR 0016's exclusions, plus the entries of
  every active named workspace after the sponsor rule, **one entry per identity**. A repository
  selected by three workspaces is fetched, indexed and bundled once, and the fleet, refresh lock,
  fence, generation domain and job stay singular. No per-workspace job, lock, fence or index
  exists.
- **Reading the contexts.** With `[workspaces]`, `meada workspace pull` also reads, in one
  `REPEATABLE READ` Postgres transaction, every non-deleted workspace's `id`, `current` version
  and operators' logins, loads each version document from the store (hostile input, bounded as
  today), and writes `<index_dir>/workspace/contexts` beside the pulled version and the
  exclusions file. Every other reader (`index`, `status`, `export`, `publish`, `show --pulled`)
  reads that file from disk and refuses to run without it when the section is on, so `meada
  index` never connects to Postgres (invariant 1). A workspace saved or applied after the pull
  joins the next refresh, so a refresh sees one consistent set, as the lock gives the deployment
  workspace.
- **Paths.** A member listed by the deployment workspace keeps that entry's `path`. A member only
  named workspaces select lives at `<[workspaces] mirrors>/<identity>`; the identity's safe
  segments make it a path under that root. `meada workspace show --pulled` lists the members with
  their paths, so the operator's fetch script provisions them as today. Applying a manifest does
  not clone anything.
- **Sponsor rule (the eligibility exclusion).** After the pull and before the fetch, `meada
  github-app roles` excludes from a named workspace every entry no operator of that workspace
  holds an accepted role on in the set it just wrote. The exclusion is per (workspace,
  identity), written into the set (migration 7 changes `permission_exclusions`: `context text
  NOT NULL DEFAULT ''` joins the primary key, `''` meaning the deployment workspace, `added_by`
  becomes nullable since a named entry has none, and the reason `no_sponsor` is added) and into
  the exclusions file (the existing
  lines keep their meaning for the deployment workspace; a context's lines name it). An
  identity is a member while any workspace includes it. An excluded entry is never fetched on
  its workspace's account, and is shown `excluded` to that workspace's operators. This is a
  successful policy decision before the fetch, not a failed fetch, and takes no part in #316.
- **Indexed configuration.** The only configuration a bundle depends on is the **ingestion
  blocks** of the member: the deployment entry's blocks if it has one, else `{}` (Contract 6).
  Named workspaces' blocks never enter a bundle id, so the shared Y has one bundle id in every
  context and changing a context's peers, resources or serve hosts never re-indexes.
- **Failure.** Unchanged and shared (ADR 0012, #316): a `roles`, fetch or index failure for any
  member stops the job before `meada snapshot publish`, the pointer does not move, and every
  workspace's queries stay on the last complete generation. A permissions set committed before
  the failure keeps its freshness and revocation.

### Contract 4: version lifecycle (what changes when)

Mutable workspace state is four Postgres tables (migration 7), in the one `[postgres] schema`:

- `workspaces(id pk, name, description, creator, creator_login, created, deleted, seq, current)`;
- `workspace_operators(workspace_id, subject, login, added_by, added, pk (workspace_id,
  subject))`, at most 20 per workspace, the creator first;
- `workspace_versions(workspace_id, version, author, saved, message, pk (workspace_id,
  version))`, one row per version stored for that workspace (the document is the store's
  `workspaces/<id>.json`, shared and immutable), so a workspace lists only its own versions;
- `workspace_history(workspace_id, seq, action, version, actor, authority, time, message, pk
  (workspace_id, seq))`, append only, `action` one of `create`, `apply`, `rollback`,
  `rename`, `delete`, `operators`.

`seq` starts at 1 at creation and increases by one on every history row. It is the **base**: the
fact a client read and a write must still find unchanged. Saving a version does not move `seq`
or `current`.

| Operation | Route | Authority | Checks at the atomic boundary |
| --- | --- | --- | --- |
| create | `POST /api/v1/workspaces` `{name, description?, entries, message}` | any admitted subject eligible for every entry; the platform administrator | eligibility, catalog, validation, limits |
| save (import) | `POST /api/v1/workspaces/<id>/versions` `{entries, message, base}` | operator of the workspace, platform administrator | base, authority, eligibility of added or changed entries; stores the version, moves nothing |
| apply, rollback | `POST /api/v1/workspaces/<id>/apply` `{version, base}` | same | base, authority, eligibility of the entries the version changes against the current one |
| rename | `POST /api/v1/workspaces/<id>/rename` `{name, description?, base}` | same | base, authority |
| delete | `POST /api/v1/workspaces/<id>/delete` `{base}` | same | base, authority |
| operators | `POST /api/v1/workspaces/<id>/operators` `{add, remove, base}` | platform administrator | base |

- **Atomic apply boundary.** One Postgres transaction: `SELECT ... FOR UPDATE` on the workspace
  row, then in this order: the workspace is not deleted, `seq` equals `base` (else 409
  `stale_base` naming the current `seq`), the actor is an operator of it or the administrator
  (else 403 `not_operator`), the newest permissions set is read **in that transaction** (not the
  admission cache) and every entry the operation adds or changes is eligible for that operator itself
  (else 403 `entries_refused`), then `current` and `seq` move, the history row is inserted and
  the transaction commits. A stale page, a revoked grant or a refused entry leaves every row
  as it was. A write needs no refresh lock: the job reads a consistent snapshot at its pull.
- **Hidden entries.** An operator that cannot read some entries edits the entries it reads: the
  version stored keeps the others in place, in order (`compose_entries`' rule, ADR 0015, which
  the named-workspace function generalizes without owners), so a save never drops what the
  operator cannot see. A refusal names the identities the actor's grant reads and counts the
  others (`hidden`), as ADR 0015 does. An operator never learns the identity of an entry it
  cannot read, so it removes one only by rolling back to a version it reads that lacks it, by
  asking the platform administrator (whose view is not filtered), or by deleting the
  workspace.
- **Rollback** is an apply of an earlier version of the same workspace, marked `rollback` in the
  answer and the history. It is checked like any apply: an entry it adds or changes against the
  current version that the operator is not eligible for refuses it; entries both versions hold
  are not rechecked.
- **Delete** sets `deleted`, drops the workspace from the next refresh's membership and from
  every list, and keeps versions and history. Its id is never reused. The shared repositories it
  selected stay in the membership while another workspace or the deployment selects them. A
  generation already published and retained keeps the context until the operator drops it
  (ADR 0014's retention): delete is not erasure.
- **Rename** changes the name and description only; ids, routes and versions do not change.
- **Audit.** Every operation above is an ADR 0016 audit record: inserted `pending` before the
  transaction, completed after, refusals included, 503 `audit_unavailable` and no write when the
  insert fails. `action` is `workspaces.<operation>`; `arguments` carry `workspace`, `base`,
  `version` and `authority` (`platform_administrator` or `workspace_operator`); `role` is the
  session role, unchanged. Reads are not audited. CLI twins are `meada workspaces create|save|
  apply|rename|delete|operators|list|show`, acting as the platform administrator with
  `cli:<login>@<host>` as author, as `meada workspace` does.
- **State.** `GET /api/v1/workspaces/<id>` reports `state`: `applied` (the current version is
  stored but the current generation records another or none: pending ingestion) or `live` (the
  current generation records the current version), and each entry `ingestion`: `pending`,
  `ready` or `excluded`. Pending ingestion is shown, never presented as ready.
- **Reads.** `GET /api/v1/workspaces` (list, `limit` 1 to 100 default 25, opaque `after`),
  `GET /api/v1/workspaces/<id>`, `.../versions` (50 per page), `.../versions/<version>` and
  `.../effective`. Everything a read returns follows the discovery rule of ADR 0020: an unknown
  id, a deleted one and a hidden one are one identical 404. Operators and the administrator also
  see the operator list and the hidden-entry count.
- **Caps.** Request bodies: create and save 80 KiB, apply 1 KiB, rename 4 KiB, delete 1 KiB,
  operators 16 KiB, catalog check 16 KiB. Answers are capped at the page sizes above.

### Contract 5: query selection (what an answer can see)

- The generation document gains optional fields that older binaries ignore:
  - `repositories` keeps its meaning (the deployment workspace's members and their bundles,
    hashed as today).
  - `members`: the bundles of repositories only named workspaces select, in the shape of an
    entry of `repositories`.
  - `contexts`: per workspace, sorted by `id`: `{id, version, repositories: [{repo, blocks}]}`,
    the non-excluded entries in manifest order with their blocks as `entry_blocks` gives them.
    No name or description is ever written to the store.
- **Ids.** `generation_id` hashes, besides today's sorted lines, the lines `# member <identity>
  <bundle id>` and `# context <id> <digest>` (the digest: the first 16 hex of the BLAKE3 hash
  of the context's canonical JSON as recorded). A deployment with no named workspaces adds no
  line, so every existing generation id is unchanged. A context change is a new generation with
  the same bundles: nothing is re-indexed.
- **Route.** `/-/workspaces/<id>/<identity>` and `/-/workspaces/<id>/<identity>@<generation>`.
  The first segment `-` is reserved: with `[workspaces]` the start check refuses a manifest
  whose identity has it as its first segment. The plain route `/<identity>` serves the deployment
  workspace's members only, exactly as today; a member only named workspaces select is a 404
  there, so a named workspace never widens another route. Protected resource metadata is served
  at `/.well-known/oauth-protected-resource/-/workspaces/<id>/<identity>` and its pinned form.
- **Admission.** Unchanged and in this order: `Origin`, then OIDC (401 before any 404), then the
  gate. The gate admits a workspace route only when the held generation has the context and
  the token's grant reads **every** repository the context records (nodes apply the viewer rule
  alone; the operator exemption belongs to the console, which knows operators). Otherwise the
  answer is the 404 an unknown id gets; its body lists only the workspace ids the token may
  view in that generation. (The plain route answers 403 for an identity the token may not read,
  as today; a workspace route answers 404, since a 403 would confirm the id exists.)
- **The workspace of a query** is the context's recorded repositories, the named one first, then
  recorded order, bounded by the grant as today; links need both endpoints in it and in the
  grant. Tools, parameters, descriptions and text are unchanged (ADR 0008). Each answer names
  its generation in `Meada-Generation` and `meada/generation`, and the workspace in
  `meada/workspace` in `_meta`.
- **Pins.** A pinned generation reproduces the context as that generation recorded it, with its
  recorded blocks and its bundles' own `meada.toml`. A later edit, rollback, rename, delegation
  or delete changes no pin. A pin of a generation that has no such context is a 404 naming the
  requested and the current generation, never another workspace's id.
- The console answers the same queries in process: the playground's body gains optional
  `workspace`, and `GET /api/v1/workspaces/<id>/repositories/<identity>/export` returns the
  export of that context, both under the discovery rule, with the operator and administrator
  exemptions: management views (metadata, entries, versions) are unfiltered for the platform
  administrator, while every graph answer, the administrator's included, is bounded by the
  caller's grant.

**As built (#342).** The record, the lifecycle's first row and the query selection are built
as follows; where a line differs from the contract above it says so.

- `[workspaces]` is read by `Manifest::read`; its errors name both sections. A deployment with no
  current version is an empty deployment workspace under it (`meada workspace pull` does not
  fail, removes the current file, and the exclusions file names the version `0000000000000000`).
- Migration 7 adds `workspaces.entries text[]`, the identities of the current version in manifest
  order, a copy that moves with `current` so that a list reads no store document; the document
  stays the authority. `permission_exclusions` gains `context` as contract 3 says.
- **Create** (`POST /api/v1/workspaces` `{name, description?, entries, message}`, `entries` the
  manifest text) checks the admission cache before anything is stored (a refusal stores nothing),
  writes `workspaces/<version id>.json` to the store with create-if-absent, and then runs the
  atomic boundary: one transaction under an advisory lock that reads the newest set on Postgres's
  clock (pending `no_set`, `stale_set` or `missing_login` is a 409 `pending` with `reason`), keeps
  the selected identities that are in the catalog and, for anyone but the administrator, on which
  the login holds an accepted role, refuses the rest with 403 `entries_refused` (`refused` names
  the identities the caller's grant reads, `hidden` counts the others), and inserts the
  workspace (`seq` 1), its first operator, the version row and the `create` history row. The same
  creator, name and entries are the same workspace (200, `created: false`); a deleted one is 409
  `workspace_deleted`. The audit record is `workspaces.create` with `workspace`, `version` and
  `authority`. A manifest with `path`, `owner`, `added_by` or `serve.documents` is a 422
  `invalid_entries`.
- **List and show** apply `meada_core::named::visible` to the identities of the current version:
  a workspace the caller neither operates nor reads whole is absent, and `show` answers it as it
  answers an unknown id. `state` is `live` when the console's held generation records the
  context at the current version, else `applied`; an entry is `excluded` when the newest set
  excludes it for that workspace, `ready` when the live generation records it, else `pending`.
- **Pull and membership.** `meada workspace pull` writes `<index_dir>/workspace/contexts` (sorted
  by id, with the operators' logins) and `meada github-app roles` reads it, lists the context
  identities as `selected` rows, applies the sponsor rule and writes the exclusions file with a
  `# contexts <digest>` line naming the contexts file it decided for; a reader that finds another
  digest refuses, so the order is pull, roles, then fetch, index and publish. `meada workspace
  show --pulled` lists the members after the deployment entries with their paths.
- **Generation.** `members` and `contexts` are in `Document` in every `snapshot` build; `Context`
  is `{id, version, repositories: [{repo, blocks}]}` and its digest is
  `context_digest` of that JSON. A context left with no entry is not recorded. The node start
  check also vets the contexts' repositories, and a generation with contexts and no deployment
  repositories is servable.
- **Route and admission** are as contract 5 says: the pass must read every repository the context
  records, a pin of a generation that has no such context is a 404, and a result's `_meta` names
  `meada/workspace`. The console's playground takes `workspace`, and `GET
  /api/v1/workspaces/<id>/repositories/<identity>/export` returns the context's export; an operator
  that cannot read every repository asks through its workspace with every answer bounded by its
  grant.
- The version id of a named entry is `meada_core::ids::named_version_id`, a function beside
  `workspace_version_id` rather than an optional path in it; the hash is the one the contract
  describes (`{"blocks","repo"}`), and no deployment version id changes.
- A pin of a held generation that records no such workspace for the caller answers a 404 naming
  that generation and the current one; an unpinned miss lists the workspaces the token may view.
- The console writes an audit record for a creation refused by the admission cache too
  (`refused`), and shows an entry `excluded` to the workspace's operators alone.
- Not built here: the CLI twins `meada workspaces ...`, the `versions`, `effective` and
  `operators` routes, and every other row of the lifecycle table.

### Contract 6: effective configuration

A repository's configuration in a context is decided by layers with provenance, never merged:

1. **The repository's own `meada.toml`**, when its bundle's mirror has one, wins whole. This is
   ADR 0011's rule for every context, deployment workspace included: "a merge would give one
   fact two sources" stands. The context's blocks for that repository are **ignored**, and the
   effective-configuration view says so (`ignored_by_repository`) before the save is accepted
   as meaningful, so an edit never looks effective when another source overrides it.
2. **The context's blocks** for the entry, when the mirror has none: `serve` (`hosts`,
   `prefix`, `strip`), `resource` and `peer` of the named entry. Two contexts may declare
   different blocks for the same repository; each sees only its own.
3. Nothing else. A repository with neither declares nothing, as today.

Two kinds of configuration, kept apart:

- **Indexed-document coverage** (`[serve] documents` and the declaration of `[serve]` that turns
  documents on, the only configuration `index::refresh` reads) decides what is in a bundle.
  It is a property of ingestion, one per repository: the repository's `meada.toml`, else the
  deployment entry's blocks (the platform's ingestion blocks), else none. A named workspace
  cannot set it (`reserved_key`); the effective-configuration view says `documents` are not
  indexed when a context declares serve hosts for a repository with no coverage. Documents are
  source facts like code: every context that includes the repository sees the same ones.
- **Query-time declarations** (peers, resources, serve hosts, prefix, strip) are applied when
  the answer is built from the context's recorded blocks. They never enter a bundle id, so
  changing them re-indexes nothing and costs a new generation id only (Contract 5).

The **effective-configuration view** (`GET /api/v1/workspaces/<id>/effective[?version=]`) returns
per entry: `source` (`repository`, `context` or `none`), the resolved blocks, `ignored` (the
context blocks the repository overrides) and `coverage` (`indexed`, `pending_ingestion`,
`excluded`), with the diagnostics `ignored_by_repository`, `unknown_until_ingested` (a repository
no held generation has), `excluded_no_sponsor`, `peer_missing` (a peer outside the workspace is
reported missing as today, never discovered) and `documents_not_indexed`. The decision function
is pure and lives in `meada-core` (inputs: whether the mirror has a `meada.toml`, the context's
blocks, the ingestion blocks), exported to WebAssembly so the guided form and the console agree;
the console supplies the per-entry presence flag from the generation it holds and reads nothing
else of a mirror. The text editor, the form and the proposal interface produce and validate one
canonical manifest, and **computed view annotations never enter canonical text or any id**.

### The console and the front end

- The pages are the catalog picker, the workspace list, a workspace's detail (state, operators,
  hidden count), its versions with the diff, and the effective configuration. A subject that
  operates nothing sees no mutation control; the API refuses what the front end hides.
- The front end adds no policy. Text and form editing produce one canonical manifest through
  `meada-core` compiled to WebAssembly: named-entry validation, diff and composition, and a
  `named_authorize` that marks the refusals before a save from the session's `github_grant`
  (ADR 0016: the identities the login holds an accepted role on, already in the session) and the
  catalog answer. The server decides again at the atomic boundary, so a stale page, a revoked
  grant or a forged payload authorizes nothing.
- A save or apply shows the diff and freezes the reviewed payload (entries, message and `base`)
  until the person confirms; a `stale_base`, an `entries_refused` (with `hidden`) or a lost
  authority is shown as the server's structured refusal and reloads the workspace. Pending
  ingestion, excluded entries and effective-configuration diagnostics are text, not color alone,
  in both console languages. Computing local rules opens no connection.

### Deterministic identifiers

| Identifier | Inputs | Changes when |
| --- | --- | --- |
| bundle id | identity, `HEAD`, schema, binary version, ingestion blocks | unchanged formula; named blocks never enter |
| version id | the entries (`path` absent for named entries) | the entries change |
| workspace id | creator subject, name, first version id | never after creation |
| generation id | sorted bundle lines, `# member` lines, `# context` lines | a member, a bundle or a recorded context changes |
| context digest | the context as recorded in the generation | its entries, order or blocks change |

None reads a clock, host, address, name, description or operator list. Names and descriptions
exist only in Postgres.

### Migration and upgrade

1. `meada postgres migrate` brings the schema to version 7 (migration 6: `source` on
   `permission_repositories`; migration 7: the four tables and the changed key of
   `permission_exclusions`). Processes at the old version refuse the new schema as ADR 0016
   says.
2. Upgrade every binary, nodes included, before the first refresh that records a context. A
   node checks a pulled document by recomputing its generation id from `repositories`
   (`snapshot/pull.rs`), so an older node refuses a generation whose id also hashes `members`
   and `contexts`: it does not ignore the fields, it keeps its last generation and its
   readiness, and it never serves a named workspace's repository on any route (fail closed).
   Generations with no named workspace are unchanged and every node reads them.
3. Enable `[catalog]` and `[workspaces]` in the job and the console, run a refresh, then create
   workspaces. Nodes serve a context from the first generation that records it.
4. Disabling `[workspaces]` stops new generations from recording contexts. Retained generations
   keep theirs for binaries that understand them.

## Slices, dependencies and what each one inherits

The eight slices keep the order the roadmap states. A slice does not start before its
predecessors merge; #345 and #346 run in parallel.

```
#340 contracts (this ADR)
  -> #341 catalog
  -> #342 first workspace and query
  -> #343 versions and overlap
  -> #344 context configuration
       -> #345 guided authoring
       -> #346 proposals (MCP)
            -> #347 validation and documentation   (needs #345 and #346)
```

- **#341** builds Contract 1: `[catalog]`, the roles command's installation listing, migration
  6, `GET /api/v1/catalog` and `POST /api/v1/catalog/check` and a minimal picker. It replaces the
  ticket's "stale/pending/unavailable per repository" with the states above, which reveal no
  repository the caller cannot select. The measurement bullet is a follow-up (#338 is not a
  gate).
- **#342** builds creation and query: `[workspaces]`, migration 7, create, list and show, the
  sponsor rule and the contexts file, the generation document's `members` and `contexts`, the
  `/-/workspaces/` route and the reserved segment, the fixture table wired into the Rust,
  public-process and WebAssembly seams. Group peers need no code for viewing: the discovery
  rule is the grant.
- **#343** builds Contract 4's save, apply, rollback, rename, delete and operators, the stale
  base, the partial-revocation behavior of ADR 0020, history and audit. The ticket's
  "import" is save from a file.
- **#344** builds Contract 6: the named-entry validation, the pure effective-configuration
  function, the view and its diagnostics, the context digest in the generation id. Its
  "indexed-document coverage model" is the ingestion-blocks rule: no new index key.
- **#345** builds the guided controls over #341 and #344. It adds no TypeScript policy: eligibility
  and effective configuration come from WebAssembly and the server.
- **#346** builds ADR 0022 over the same functions.
- **#347** accepts the flow end to end, writes the upgrade procedure above as documentation and
  carries the real-provider evidence.

## Considered options

- **A route by unique name** (`/-/workspaces/<name>/...`). Friendlier, and an enumeration oracle:
  `name_taken` would confirm hidden workspaces. Ids cost one copy and paste.
- **Filtered discovery of a partially readable workspace.** Names, descriptions and counts are not
  bounded by repository grants. Rejected in ADR 0020.
- **The workspace pointer in Valkey, one per workspace.** Not durable, and operator lists and
  history need transactions with the permissions set. Postgres is S4's module for durable
  mutable state.
- **A per-workspace refresh lock, fence, job or generation.** Out of scope: it duplicates
  indexing or publication, which this ADR exists to share.
- **Named blocks in the bundle id.** Gives every context its own bundle of the same commit, the
  duplicate index the spec forbids, and re-indexes on a peer edit.
- **A per-key merge of the repository's `meada.toml` with a context's blocks.** Useful, and it
  reopens ADR 0011's decision. The whole-wins rule with visible provenance keeps that decision;
  a merge would need its own ADR and is listed as a follow-up.
- **Context blocks inside the generation id only for the changed context.** A deployment-wide
  digest would make every workspace's id depend on all others; per-context lines keep a context's
  contribution local.
- **Nodes reading Postgres to know operators.** A Postgres read on every request, against ADR
  0016's stateless nodes. The operator exemption is console-only.
- **Eligibility from the claims grant.** It would make a static `[oidc.groups]` table a second
  creation policy beside the accepted repository roles. It stays a reading rule only.

## Consequences

- What each invariant of ADR 0002 keeps:
  1. `meada index` reads the pulled version, the exclusions file and the contexts file from
     disk and never connects. `meada workspace pull` and `meada github-app roles` are separate
     commands.
  2. Source stays within the operator's boundary. The console and the proposal server call
     neither GitHub nor the identity provider; the job's two REST uses are ADR 0013 and 0016's,
     now with the installation repositories listing under the same account and throttles.
  3. Meada writes only in its own state: the Postgres schema (four tables, columns), the store's
     `workspaces/` and generation documents, the pulled files under `<index_dir>/workspace/`.
     Nothing in a mirror or a manifest.
  4. The nodes' MCP tools are unchanged. ADR 0022 adds a separate server whose tools write
     nothing.
  5. One index per repository: unchanged, and now explicit across contexts.
  6. Determinism: ids derive from inputs only; the permissions set, operators and names reach no
     answer, export, trace or id.
- ADR 0014's roles gain the platform administrator as a name for its operator; its "one
  workspace per deployment" and ADR 0015's rejections carry amendments pointing here.
- Everything is additive under Spec R1's stability rule (a feature, sections, routes, tables, a
  document field, refusal reasons): a minor change. Without `[workspaces]` nothing changes.
- The operator owns more: the allow list and cap, a `mirrors` root and its provisioning, the
  single failure domain (one workspace's failing repository stops new generations for all, as
  teams accept), and the cost of expanded coverage.

## Open

- **Real-organization evidence (#338).** The fakes establish the contracts; they cannot show
  installation coverage or the rate cost of `max_repositories` candidates. The assumption real
  evidence would confirm is that `GET /installation/repositories` plus `affiliation=all`
  collaborator listing stays within one installation's hour at the operator's cap, or the
  base-role upgrade is needed first. The check against a real organization is a follow-up that
  gates enterprise acceptance in #347, not these slices.
- **Per-key merge of repository and context configuration**, if teams need a context to extend a
  repository's own declarations.
- **A directory-backed operator picker** for delegation (the SCIM directory already holds users).
- **Provenance display of group-derived roles**, which no contract here needs.
