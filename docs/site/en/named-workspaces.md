# Named workspaces

!!! success "Delivered: S5, validated end to end"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) is delivered in eight slices ([#340](https://github.com/ClaudioMascaro/meada/issues/340) to [#347](https://github.com/ClaudioMascaro/meada/issues/347)): the contracts (ADRs 0020 to 0022), the [catalog](github-app.md#the-repository-catalog), the first workspace, the version lifecycle, per-workspace configuration, guided authoring, MCP proposals and this validation. Everything below is built behind the cargo feature `workspaces` (and `proposals` for the proposal server), both enabled by `cloud`. Without `[workspaces]` the console still operates with one workspace per deployment. One check is still open: [#338](https://github.com/ClaudioMascaro/meada/issues/338), the measurement against a real GitHub organization, which the maintainer decided does not gate S5 ([discovery evidence](#discovery-evidence)).

## Compose without duplicating indexing

Named workspaces overlap over shared indexing. A repository does not gain another index, job, refresh lock or generation by belonging to two workspaces: one refresh fetches, indexes and publishes every selected repository once, and each workspace is a context that selects some of them. There are three human roles: platform administrator, workspace operator and viewer.

The effective repository grant remains authoritative. With Y readable by groups A and B, repository A only by group A and repository B only by group B, an operator in A alone can compose Y+A and is refused Y+B; an operator in B alone can compose Y+B and is refused Y+A; someone in both can compose all three. Sharing Y grants A to nobody and B to nobody, and a link from Y to B does not appear in the workspace of Y+A.

<!-- meada-graph:workspace-overlap -->

## The first workspace

An admitted maintainer with no owned entry opens **Named workspaces** in the console, picks repositories from the catalog, names the workspace and confirms the manifest the page shows. The server decides again in one transaction against the newest permissions set: every selected repository must be in the catalog and the person's GitHub login must hold an accepted role on it, directly or through a group. A refusal names the repositories the person's grant reads and only counts the others. The workspace is stored under the person's identity, who operates it; colleagues whose own grants read the same repositories see it without any invitation and cannot change it.

The workspace becomes queryable through the shared job: `meada workspace pull` reads it, `meada github-app roles` leaves out, before the fetch, any entry that none of its operators holds an accepted role on (that is a policy decision, not a failed fetch), and the repositories only named workspaces select are fetched, indexed and published once, at `<[workspaces] mirrors>/<identity>`. Until a generation records the workspace, the console shows it as `applied` with its entries `pending`; once recorded it is `live` and its entries `ready`. A failed fetch or index still aborts the whole publication and keeps the current generation.

## Maintaining a workspace

An operator edits its workspace as a sequence of versions. **Saving** stores a version and moves nothing; **applying** makes a stored version the current one, and applying one that was current before is a **rollback**. Every change carries the **base** the page read (the number `GET /api/v1/workspaces/<id>` shows): if anyone changed the workspace since, the change is refused (`stale_base`, naming the current base) and nothing is written, so two people editing at once never overwrite each other. At the moment of the write the server checks again, in one transaction, that the workspace still exists, that the base is the current one, that the person still operates it (or is the platform administrator) and that every repository the change **adds or changes** is still eligible for that person in the newest permissions set; repositories a version keeps unchanged, and removals, are not rechecked. A revoked role, a refused entry, a stale page or a version of another workspace changes nothing.

An operator that lost the role on one repository keeps the workspace, reads the entries its grant reads and a count of the others, and can still edit them: the version it saves keeps the entries it cannot read in place, in order, and it can neither read nor overwrite them. It removes one by rolling back to a version that lacks it, by asking the platform administrator (whose management view is not filtered) or by deleting the workspace. Colleagues who lost a role on any of the repositories no longer see the workspace, its versions or its history: they get the 404 of an id that does not exist. A viewer, that is anyone who reads the workspace but does not operate it, cannot write: it gets a 403 `not_operator`; and reading a repository never edits another person's workspace.

**Rename** changes the name and description only (the id stays). **Delete** removes the workspace from lists and from the next refresh, keeping its versions and history in Postgres; the repositories it selected stay indexed once for every other workspace that selects them, and the retained generations keep the context until dropped. Only the platform administrator **delegates** operators (a workspace keeps at least one). Overlapping workspaces over the same repository are independent: editing, rolling back or deleting one moves nothing in the other. Every operation, refusals included, is one audit record with the person, the workspace and the base; `meada workspaces list|show|versions|diff|create|save|apply|rename|delete|operators` do the same from the command line as the platform administrator.

## Who can do what

- **Platform administrator**: the existing global operator. It configures the deployment, delegates operators and administers any workspace. No serialized role changes name.
- **Workspace operator**: whoever created a workspace, or was delegated it. It edits, applies, rolls back, renames and deletes that workspace and no other. Being in the same group as the creator grants none of this.
- **Viewer**: nobody assigns it. A person views a workspace when their own current grant reads every one of its repositories, so group peers need no invitation, no identity provider group and no access package.

Creating a workspace requires an accepted GitHub role, held directly or through a repository group, on every repository selected. The claims grant still counts for reading, not for creating. Eligibility is never stored: every decision reads the newest permissions set, and a set older than `max_age_seconds` makes nothing selectable until the job writes another.

## What a partial reader sees

A workspace is hidden from someone whose grant does not read every repository in it: it is absent from lists and answers the same 404 as an id that does not exist. Names, descriptions, versions and diagnostics follow the same rule, because a title can disclose a repository the reader cannot read. Operators and the platform administrator are exempt so they can repair their own workspace: an operator whose role on A was revoked sees the repositories it can read and a count of the hidden ones, and every graph answer stays filtered by the grant. Workspaces are identified by a deterministic id, and names may repeat, so creating one reveals nothing about hidden ones.

Loss of a role or group membership removes access at the next permissions set and never revives from an older one. The workspace, its versions and its history are not deleted.

## Distinct contracts

1. **Catalog** (delivered): the repositories the GitHub App covers and an administrator's allow list admits, listed by the job in the same permissions set as the roles. A caller sees only repositories it can select; coverage is not entitlement.
2. **Selected entries**: a versioned manifest of repositories with peers, provider relationships, environment-variable references, resources and serve settings. It carries no mirror path, secret value, owner or document list.
3. **Shared ingestion**: one entry per repository across all workspaces. An entry that no operator of its workspace holds an accepted role on is excluded before the fetch, which is not a failed fetch. A failed fetch or index still aborts the whole publication and every workspace keeps the last complete generation.
4. **Versions**: save, apply, roll back, rename and delete, each checked in one transaction against the base version, the author's authority and current eligibility, and each audited.
5. **Query selection**: `/-/workspaces/<id>/<identity>` answers from the workspace as a generation recorded it, bounded by the manifest and by the grant. A pinned generation reproduces it exactly. The plain `/<identity>` route keeps serving the deployment workspace.

## Effective configuration

A repository's own `meada.toml` still wins whole over any workspace's declarations, and the editor shows when a declaration is ignored. Declarations for repositories without one are per workspace: two workspaces sharing Y may declare different peers or serve hosts without a second index, because those are applied when answering and never enter the bundle. What documents are indexed belongs to ingestion and is the same in every workspace.

Each entry of a workspace's manifest may carry, besides `repo`, `[repository.serve]` (`hosts`, `prefix`, `strip`), `[[repository.resource]]` and `[[repository.peer]]` blocks with the keys of `meada.toml`. A peer's `base_url_env` lists the **names** of environment variables, never a value, a URL or `NAME=value`; Meada does not read them. `path`, `owner`, `added_by` and `[repository.serve] documents` are refused (422), because mirror paths and credentials are the platform's and the documents decide what is indexed. Layers are never merged: the repository's own `meada.toml` wins whole, else the workspace's blocks apply, else the repository declares nothing.

**Before saving**, the console's editor (and `POST /api/v1/workspaces/effective`, which stores nothing) shows the effective configuration of the text; **afterwards**, `GET /api/v1/workspaces/<id>/effective[?version=]` shows it for a stored version. Per entry it says the `source` (`repository`, `context` or `none`), the `ignored` blocks, the `coverage` (`indexed`, `pending_ingestion` or `excluded`) and diagnostics: `ignored_by_repository`, `unknown_until_ingested` (the repository is in no held generation, so whether its own file overrides is not known yet), `excluded_no_sponsor`, `peer_missing` (a peer outside the workspace is reported, never discovered) and `documents_not_indexed` (serve hosts for a repository whose ingestion declares no `[serve]`). A save, an apply and a creation carry the same diagnostics beside their result, so an ignored declaration is never shown as plainly applied. The view is computed by the same function the browser runs (WebAssembly) and never enters the manifest's text or any id.

A change of configuration is a new generation of the same bundles: nothing is re-indexed, a generation pinned before the change keeps the configuration it recorded, and another workspace that shares the repository does not move. The workspace's `base` is checked as for any change.

## Guided authoring

The console's workspace page edits a workspace you operate through **form controls** and through the **manifest text**, and the two are one document: an edit in either rewrites the other, both go through the same WebAssembly as the server (`meada-core`), and what is saved is the canonical text it writes. The form holds no rule of its own: what an entry may contain, which repositories you may select, the diff and the entries a change must find eligible again are the core's, and the server decides again at the transaction. If the text has errors, the form becomes read-only until they are fixed.

- **Repositories** are added from the catalog (search by prefix, page by page) and removed from the manifest. A repository the catalog did not list must be verified first (`POST /api/v1/catalog/check`, asked for only when you press *Verify eligibility*; the form's other requests are the catalog pages and *Show effective configuration*, and everything else is computed locally), and each changed entry says in text whether it is *already in the workspace* (rechecked only if you change it), *selectable*, *unavailable*, *pending verification* or *not verified yet*; a change that touches a repository you are not eligible for cannot be saved and names the ones your grant reads and counts the rest.
- **Serve settings** (`hosts`, `prefix`, `strip`), **resources** (`kind`, `id`, `role`, `via`) and **peers** (the peer repository and the **names** of the environment variables holding its base URL, never a value) are fields of each repository. An emptied field is an absent one, and a value the core refuses is shown as the core's message.
- **Scope and effective configuration** are shown in text, never by color alone: whether you operate the workspace, how many entries you edit and how many you cannot read (kept as they are when you save), each entry's ingestion (`pending`, `ready`, `excluded`, or new), and, after *Show effective configuration*, which source wins and what the repository's own `meada.toml` ignores.
- **Review and save.** The page lists the diff (added, removed and changed entries, with the changed parts) and *Save and apply* opens a confirmation that **freezes** the reviewed payload: the manifest text, the message and the base. Nothing else is sent when you confirm, whatever happens to the draft meanwhile. A refusal is shown as the server words it: `entries_refused` names the repositories (and counts the hidden ones) and reads your roles again so the eligibility shown is current, `stale_base` offers to reload the workspace (a version stored whose apply was refused is said so), and losing the operator role removes the editor. A viewer sees the entries and their state but no control that changes anything.

Computing these local rules opens no connection. Both console languages (English and Brazilian Portuguese) have the same controls. Not built: a page over a workspace's versions with rollback (the `versions` and `history` routes exist); the proposal interface is [MCP-assisted authoring](#mcp-assisted-authoring), below.

<!-- meada-graph:mcp-proposal -->

## MCP-assisted authoring

A separate **proposal server** (feature `proposals`, [ADR 0022](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0022-authenticated-workspace-proposals.md)) lets a CLI agent acting for an operator list the repositories that operator can select and ask whether a manifest is valid and authorized. It derives the identity, the login, the grants, the role and the workspaces the operator runs from its own authentication, never from arguments; it forwards no credential and treats everything the agent supplies as untrusted. It returns the canonical manifest, a diff, diagnostics and the base, and writes nothing: no draft, no version, no moved pointer, no audit record. A person pastes the proposal into the console, which validates it again and rechecks the base, the authority and the grants when saving and applying.

### Running the proposal server

Add `[proposals]` to the bootstrap manifest the console and the job read (it needs `[workspaces]`):

```toml
[proposals]
public_url = "https://proposals.example.com"  # https unless loopback, no path
# audience = "https://proposals.example.com"  # the aud of its tokens, public_url by default
```

Run `meada workspaces mcp --listen 0.0.0.0:8090 --workspace meada.workspace.toml` in a process of its own, beside the console and never on a node. It reads what the console reads: Postgres (every session read-only), the version documents of the store, the generations its `[snapshot] local_dir` holds (keep a `meada snapshot pull` running for it as for the console, or the effective configuration shows every repository pending) and the issuer's metadata and keys. It never calls GitHub, a node or the identity provider's API, and holds no GitHub App key. The MCP endpoint is the root of `public_url`, and its protected resource metadata is at `/.well-known/oauth-protected-resource`. Register `[proposals] audience` as a resource in the identity provider: a node's or the console's token is refused here (401), and this server's token is refused by them.

| Tool | What it answers |
| --- | --- |
| `catalog` | The repositories the caller may select (`query`, `limit`, `after`), as the console's catalog: `state`, `reason`, the set's age and a page of identities. |
| `workspaces` | The workspaces the caller may see, with `seq` (the base) and whether the caller operates each. |
| `show` | One visible workspace: its record, `seq`, the entries the caller reads and their manifest text. |
| `propose` | `entries` (manifest text), `workspace` (the workspace to change, if any), `name`, `description` and `hints`: the proposal, below. |

A proposal is `{base, manifest, diff, diagnostics, eligibility, refusals, effective, applicable}`. `base` is `{workspace, seq, version}`, null for a new workspace: what the console's save must still find. `manifest` is the canonical text of the entries the caller reads, with no view annotation; `diff` compares it with the base's entries the caller reads; `diagnostics` are the validation problems in the CLI's words, with their line, entry and key when they name them; `eligibility` gives each named identity as `selectable`, `unavailable` or `pending`; `refusals` are the errors the console would answer now (`not_operator`; `entries_refused`, naming only what the caller reads and counting the rest in `hidden`; `pending` with its reason); `effective` is the effective configuration over the base; `applicable` says whether the console would accept it now. Every tool is read-only, and no tool has an argument naming an actor, a role, a grant or a login: one is refused as an invalid argument. Inputs are bounded (a body of 80 KiB, a manifest of 64 KiB, 50 hints with notes of 256 characters, pages of 100), and an answer past 192 KiB is `too_large`, never cut.

### An agent's session

1. Configure two MCP servers in the agent: the GitHub read MCP your organization already authorizes, with its own credentials, and Meada's proposal server, with a token for `[proposals] audience` from your identity provider. Meada never receives the GitHub credentials, and the GitHub MCP never receives Meada's token.
2. Ask `catalog` what the operator may select, and `workspaces` and `show` what exists.
3. Read the candidate repositories with the GitHub MCP: routes, clients, packages, which repository serves which contract. Whatever it says about access is a hint, not a grant.
4. Call `propose` with the manifest text, the `workspace` to change if any, and the repositories read as `hints`. A hint never adds an entry or changes a result, and its note is discarded unread.
5. Fix what `diagnostics` and `refusals` report and propose again until `applicable` is true. Equal calls give equal answers.
6. Hand the `manifest` and the `base` to the operator for review in the console. For a change, the operator pastes the text into the workspace's editor, checks the effective configuration and the diff, and saves and applies at that base. For a new workspace, the operator creates it from the catalog with the same repositories and then pastes the text into its editor. If someone changed the workspace in between, the console refuses the stale base and the agent proposes again.

The distinction between a [workspace version and generation](workspaces.md), and between [admission and grant](admission.md), remains essential to this design.

## Deploying and upgrading

The order matters because nodes verify what they pull.

1. **Back up Postgres, then migrate.** `meada postgres migrate --workspace <bootstrap>` brings the schema to version 8 (6: the `source` of the catalog's rows; 7: the four workspace tables and the context of the exclusions; 8: the identities of each version). A process still at an older version refuses the new schema, as [the data layer's upgrade window](enterprise.md#known-ceilings) says. Migrations only move forward: to go back, restore the backup.
2. **Upgrade every binary, nodes included, before the first refresh that records a context.** A node recomputes the generation id of what it pulls; an older node refuses a generation whose id also hashes the new `members` and `contexts` (and, from #510, a context's `excluded` identities, so upgrade nodes before or with the publisher again on that release), keeps its last generation and its readiness, and never serves a named workspace's repository on any route (it fails closed). A generation with no named workspace is read by every node.
3. **Configure the job and the console** with `[catalog]` (`allow`, `max_repositories`) and `[workspaces]` (`mirrors`), which need `[ui]` with `operations = true`, `[oidc.github]` and `[workspace] source = "store"`. Provision the `mirrors` root where the job's fetch step writes: `meada workspace show --pulled` lists each member and its path. The [Compose](https://github.com/ClaudioMascaro/meada/tree/main/deploy/compose) and [Helm](https://github.com/ClaudioMascaro/meada/tree/main/deploy/helm/meada) examples carry these sections.
4. **Run a refresh** (`workspace pull`, `github-app roles`, fetch, `index`, `snapshot publish`). The roles command writes the catalog in the same set as the roles; the console shows the catalog `ready` once it reads it.
5. **Let people create workspaces.** Nodes serve a context from the first generation that records it. Optionally run the [proposal server](#running-the-proposal-server) in a process of its own.
6. **Turning it off.** Removing `[workspaces]` stops new generations from recording contexts; retained generations keep theirs for binaries that understand them, and the tables stay.

### When a refresh fails

| Stage that fails | What the deployment keeps | Recovery |
| --- | --- | --- |
| `github-app roles` (GitHub 5xx, throttle, Postgres) | The previous permissions set decides and ages; no set is written; the pointer does not move. A grant GitHub already shows restored does not come back. | Run `roles` again; a later complete set is the only thing that changes a grant. |
| fetch or `index` of any member | Every workspace keeps answering from the last complete generation. A revocation the job committed earlier stays effective. | Fix the mirror and refresh. An entry excluded for lack of a sponsor is a decision before the fetch, not a failure, and needs no mirror. |
| `snapshot publish` (local publication) | The pointer does not move; nothing is partially published. | Rebuild the index and publish again. |

## Validated end to end

The flow is accepted by tests that drive the public binaries and transports (the console, the job's commands, a node and the proposal server over the loopback issuer, a fake GitHub REST API and local Valkey, Postgres and S3-compatible services) and by the same Y/A/B table in Rust and WebAssembly. They run in CI with the services required.

| What is shown | Where |
| --- | --- |
| Platform administrator, an A operator creating Y+A, a B operator creating Y+B, a caller in both, direct and group-derived roles, peers viewing without enrollment, operating nothing | `tests/it/ui/oidc/workspaces.rs` (`creation_follows_the_shared_table...`, `peers_view_a_workspace...`, `a_created_workspace_becomes_queryable...`), `meada-core/tests/named_workspaces.rs`, `ui/tests/named-core.test.ts` |
| Revocation after creation, partial-reader privacy, no widening through Y | `workspaces/lifecycle.rs` (`a_partial_revocation_hides...`), `catalog.rs` |
| Overlapping lifecycle | `workspaces/lifecycle.rs` (`overlapping_workspaces_live_and_end_independently...`, `concurrent_edits_of_one_base...`), `workspaces/configuration.rs` |
| SCIM deactivate, delete and reprovision; owner-clear is `owner_change` | `deprovision.rs`, `directory.rs`, `workspaces/lifecycle.rs` (`a_deleted_user_is_blocked...`), `workspaces/proposals.rs` (`a_deleted_user_is_refused...`) |
| Guided UI and stateless proposals with equal results; only a deliberate save changes state, and it is audited | `workspaces/proposals.rs` (`proposals_follow_the_shared_table...`, `a_proposal_changes_nothing...`), `ui/tests/GuidedWorkspace.test.tsx` |
| Failure of roles, fetch/index and local publication | `workspaces/acceptance.rs` (`every_failed_stage_keeps_the_generation...`), `workspaces.rs` (index failure, sponsor exclusion), `github_app/roles.rs` |
| Default build unchanged | `cargo tree` of the default features and the CLI, MCP, export and trace goldens, unchanged by every S5 slice |

## Discovery evidence

What the tests establish: the catalog and the role check against a fake GitHub (pages, throttling, the cap, failure writing nothing) and the cost formula in [the catalog](github-app.md#the-repository-catalog): `ceil(covered / 100)` installation pages per installation and `ceil(collaborators / 100)` collaborator pages per candidate. What they cannot establish is the installation coverage and the rate cost of a real organization, and the result is not substituted by the fake. [#338](https://github.com/ClaudioMascaro/meada/issues/338) stays open as that evidence: the assumption it would confirm is that the installations listing plus the `affiliation=all` collaborator listing fits one installation's hour at your `max_repositories`, or else that listing the base role's members once is needed first ([known ceilings](enterprise.md#known-ceilings)). Until it is measured, raise `max_repositories` in steps and watch whether `roles` finishes within the job's schedule.

## Delivery order

Contracts, then the eligible catalog, the first workspace, versions and overlap, context configuration, guided authoring and MCP proposals, and finally this validation. The [roadmap](roadmap.md) tracks each step.
