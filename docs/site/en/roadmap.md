# Roadmap

S1, S2 and S4 are delivered. S3 remains planned and S5 has started: the eligible catalog and the first named workspace are delivered. This map shows dependencies and scope boundaries without promising dates.

<!-- meada-graph:roadmap -->

## What is available

[S1, #259](https://github.com/ClaudioMascaro/meada/issues/259) delivers the [server-mode](server.md) foundations. [S2, #293](https://github.com/ClaudioMascaro/meada/issues/293) delivers the [console](console.md), immutable versions and fleet operations.

## Foundations in progress

[S3, #294](https://github.com/ClaudioMascaro/meada/issues/294) covers owners and entry authorization. [S4, #319](https://github.com/ClaudioMascaro/meada/issues/319), already delivered as [enterprise identity](enterprise.md), covers identity, grants, directory and audit. Parts of these foundations can advance in parallel; the ticket graph determines the actual blockers. This map does not make all of S3 a prerequisite of all of S4.

S5 depends on both foundations, [atomic publication, #316](https://github.com/ClaudioMascaro/meada/issues/316), and [measurement and validation, #338](https://github.com/ClaudioMascaro/meada/issues/338).

## After the foundations

[S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) will introduce [named workspaces](named-workspaces.md) over shared indexing. [Contract consolidation, #340](https://github.com/ClaudioMascaro/meada/issues/340), is done (ADRs 0020 to 0022) and precedes runtime work. The accepted sequence is contracts → eligible catalog → first workspace → versions and overlap → cross-repository configuration. The eligible catalog ([#341](https://github.com/ClaudioMascaro/meada/issues/341)) is delivered: discovering the selectable repositories before any manifest exists, creating no workspace and no index. The first workspace ([#342](https://github.com/ClaudioMascaro/meada/issues/342)) is delivered too: creation from the catalog, shared ingestion with the sponsor rule, and the node route `/-/workspaces/<id>/<identity>`; saving versions comes next. The measurement against a real GitHub organization ([#338](https://github.com/ClaudioMascaro/meada/issues/338)) is still pending and gates enterprise acceptance, not these steps. Guided UI and MCP proposals then advance in parallel; enterprise validation depends on both.

The linked specifications own the complete contracts. A planned roadmap item is not an available configuration option.
