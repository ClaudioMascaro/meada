# Named workspaces

!!! warning "Planned: S5, contracts decided"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) is accepted sequential scope after S3 and S4. [Contract consolidation, #340](https://github.com/ClaudioMascaro/meada/issues/340), is done: ADRs 0020, 0021 and 0022 fix the contracts described here. The runtime is not delivered. The console you can run today still operates with one workspace per deployment, and nothing below is a configuration option yet.

## Compose without duplicating indexing

Named workspaces overlap over shared indexing. A repository does not gain another index, job, refresh lock or generation by belonging to two workspaces: one refresh fetches, indexes and publishes every selected repository once, and each workspace is a context that selects some of them. There are three human roles: platform administrator, workspace operator and viewer.

The effective repository grant remains authoritative. With Y readable by groups A and B, repository A only by group A and repository B only by group B, an operator in A alone can compose Y+A and is refused Y+B; an operator in B alone can compose Y+B and is refused Y+A; someone in both can compose all three. Sharing Y grants A to nobody and B to nobody, and a link from Y to B does not appear in the workspace of Y+A.

<!-- meada-graph:workspace-overlap -->

## Who can do what

- **Platform administrator**: the existing global operator. It configures the deployment, delegates operators and administers any workspace. No serialized role changes name.
- **Workspace operator**: whoever created a workspace, or was delegated it. It edits, applies, rolls back, renames and deletes that workspace and no other. Being in the same group as the creator grants none of this.
- **Viewer**: nobody assigns it. A person views a workspace when their own current grant reads every one of its repositories, so group peers need no invitation, no identity provider group and no access package.

Creating a workspace requires an accepted GitHub role, held directly or through a repository group, on every repository selected. The claims grant still counts for reading, not for creating. Eligibility is never stored: every decision reads the newest permissions set, and a set older than `max_age_seconds` makes nothing selectable until the job writes another.

## What a partial reader sees

A workspace is hidden from someone whose grant does not read every repository in it: it is absent from lists and answers the same 404 as an id that does not exist. Names, descriptions, versions and diagnostics follow the same rule, because a title can disclose a repository the reader cannot read. Operators and the platform administrator are exempt so they can repair their own workspace: an operator whose role on A was revoked sees the repositories it can read and a count of the hidden ones, and every graph answer stays filtered by the grant. Workspaces are identified by a deterministic id, and names may repeat, so creating one reveals nothing about hidden ones.

Loss of a role or group membership removes access at the next permissions set and never revives from an older one. The workspace, its versions and its history are not deleted.

## Distinct contracts

1. **Catalog**: the repositories the GitHub App covers and an administrator's allow list admits, listed by the job in the same permissions set as the roles. A caller sees only repositories it can select; coverage is not entitlement.
2. **Selected entries**: a versioned manifest of repositories with peers, provider relationships, environment-variable references, resources and serve settings. It carries no mirror path, secret value, owner or document list.
3. **Shared ingestion**: one entry per repository across all workspaces. An entry that no operator of its workspace holds an accepted role on is excluded before the fetch, which is not a failed fetch. A failed fetch or index still aborts the whole publication and every workspace keeps the last complete generation.
4. **Versions**: save, apply, roll back, rename and delete, each checked in one transaction against the base version, the author's authority and current eligibility, and each audited.
5. **Query selection**: `/-/workspaces/<id>/<identity>` answers from the workspace as a generation recorded it, bounded by the manifest and by the grant. A pinned generation reproduces it exactly. The plain `/<identity>` route keeps serving the deployment workspace.

## Effective configuration

A repository's own `meada.toml` still wins whole over any workspace's declarations, and the editor shows when a declaration is ignored. Declarations for repositories without one are per workspace: two workspaces sharing Y may declare different peers or serve hosts without a second index, because those are applied when answering and never enter the bundle. What documents are indexed belongs to ingestion and is the same in every workspace.

<!-- meada-graph:mcp-proposal -->

## MCP-assisted authoring

A separate proposal server lets a CLI agent, using its own GitHub read connector, list the repositories its operator can select and ask whether a manifest is valid and authorized. It derives identity and grants from its own authentication, never from arguments, forwards no credential and treats everything the agent supplies as untrusted. It returns the canonical manifest, a diff, diagnostics and the base version, and writes nothing: no draft, no version, no moved pointer. A person pastes the proposal into the console, which validates it again and rechecks the base, the authority and the grants when saving and applying.

The distinction between a [workspace version and generation](workspaces.md), and between [admission and grant](admission.md), remains essential to this design.

## Delivery order

Contracts, then the eligible catalog, the first workspace, versions and overlap, and context configuration. Guided authoring and MCP proposals then advance in parallel, and enterprise validation depends on both. The [roadmap](roadmap.md) tracks each step.
