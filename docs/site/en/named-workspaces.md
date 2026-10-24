# Named workspaces

!!! warning "Planned: S5"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) is accepted sequential scope after S3 and S4. [Contract consolidation, #340](https://github.com/ClaudioMascaro/meada/issues/340), will define the final runtime contracts. The delivered console still operates with one workspace per deployment.

## Compose without duplicating indexing

The proposal allows named workspaces with overlapping repositories over shared indexing. A repository does not gain another index by belonging to two workspaces. There will be three human roles: platform administrator, workspace operator and viewer.

The effective repository grant will remain authoritative. An operator with access only to A will be able to compose Y+A; another with access only to B will be able to compose Y+B. Sharing Y will not grant A to the second or B to the first. Reading Y will also depend on each subject’s effective grant.

Members of the same group will inherit the viewer role without another identity provider, Meada or access-package approval. That role will remain bounded by repository grants.

<!-- meada-graph:workspace-overlap -->

## MCP-assisted authoring

Authenticated MCP authoring will be stateless and proposal-only. It will neither save nor apply configuration. A person will review the proposal and use the authorized console flow to persist and apply changes.

The distinction between a [workspace version and generation](workspaces.md), and between [admission and grant](admission.md), remains essential to this proposal.

<!-- meada-graph:mcp-proposal -->
