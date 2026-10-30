# Named workspaces

!!! warning "Partly delivered: S5, contracts decided; the catalog, the first workspace, its versions, context configuration, guided authoring and MCP proposals exist"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) is accepted sequential scope after S3 and S4. [Contract consolidation, #340](https://github.com/ClaudioMascaro/meada/issues/340), is done: ADRs 0020, 0021 and 0022 fix the contracts described here. Of the runtime two slices are delivered: the **catalog** ([#341](https://github.com/ClaudioMascaro/meada/issues/341)), described in [the catalog](github-app.md#the-repository-catalog) and the [console](console.md#repository-catalog), and the **first workspace** ([#342](https://github.com/ClaudioMascaro/meada/issues/342)): `[workspaces]`, creation, list and show, shared ingestion with the sponsor rule, and the node route, described in [the console](console.md#named-workspaces). The **version lifecycle** ([#343](https://github.com/ClaudioMascaro/meada/issues/343)) is delivered too: save, apply, rollback, rename, delete and delegation, with the versions and history reads, described in [the console](console.md#named-workspaces) and below. The **context configuration** ([#344](https://github.com/ClaudioMascaro/meada/issues/344)) is delivered as well: peers, environment-variable references, resources and serve settings per workspace, with the effective-configuration view and a text editor in the console, described in [effective configuration](#effective-configuration). **Guided authoring** ([#345](https://github.com/ClaudioMascaro/meada/issues/345)) is delivered too: form controls over the same manifest, described in [guided authoring](#guided-authoring). The **MCP proposals** ([#346](https://github.com/ClaudioMascaro/meada/issues/346)) are delivered too, described in [MCP-assisted authoring](#mcp-assisted-authoring). The rest below is not a configuration option yet. Without `[workspaces]` the console still operates with one workspace per deployment.

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

Computing these local rules opens no connection. Both console languages (English and Brazilian Portuguese) have the same controls. Not yet built: a page over a workspace's versions with rollback (the `versions` and `history` routes exist); the proposal interface is [MCP-assisted authoring](#mcp-assisted-authoring), below.

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

## Delivery order

Contracts, then the eligible catalog, the first workspace, versions and overlap, and context configuration. Guided authoring and MCP proposals then advance in parallel, and enterprise validation depends on both. The [roadmap](roadmap.md) tracks each step.
