# Architecture map

Read this routing index before exploring or changing code, then follow the area and topic
for the change. Read the relevant detail pages, not every leaf of an area. This index stays
within 8 KiB; its relocated details stay within 24 KiB (`tests/it/docs.rs`). The code wins over
the map. Terms are `GLOSSARY.md`'s; decisions are in `docs/adr/`.

A new module, seam or invariant gets a route here and detail in `map/`. A limit goes to the
relevant part of [ceilings.md](ceilings.md). Full module descriptions remain in the
[core catalogue](map/overview/crates.md) and [binary catalogue](map/overview/modules-src.md).

## Flow

Workspace opening, refresh, partial exports and deterministic output:
[flow overview](map/overview/flow.md), then [query and server flow](map/flow.md).

## Crates

The binary and deterministic `meada-core`, WASM exports, versioning and grammar ownership:
[crate rules](map/overview/crates.md), then [core detail](map/core.md).

- `meada-core/src/manifest.rs`, `meada-core/src/ids.rs`, `meada-core/src/workspace.rs`, `meada-core/src/authorize.rs`, `meada-core/src/msg.rs`, `meada-core/src/http.rs`, `meada-core/src/export.rs`, `meada-core/src/shape.rs`, `drift.rs`, `meada-core/src/render.rs`, `search.rs`, `context.rs`, `trace.rs`, `meada-core/src/lib.rs`, `meada-core/src/wasm.rs`: shared deterministic types, validation and renderers. [core](map/core.md).
- `meada-core/src/named.rs`, `effective.rs`: named workspaces, catalog and proposals. [workspaces](map/workspaces.md).

## Modules (`src/`)

Each line names modules and the area that owns their detail. Split area indexes route to the
relevant topic. The [full catalogue](map/overview/modules-src.md) retains each module's description.

- `engine.rs`, `items.rs`, `trace.rs`, `partial.rs`, `drift_view.rs`, `render.rs`, `mcp.rs`, `main.rs`, `msg.rs`, `lib.rs`: queries, rendering, MCP and CLI. [queries](map/queries.md).
- `workspace.rs`, `config.rs`, `repo.rs`, `index.rs`, `lang/`, `resolve.rs`, `community.rs`, `entry.rs`: workspace discovery, storage, packs and resolution. [indexing](map/indexing.md).
- `contract.rs`, `contract/`, `http.rs`, `http/`, `resource.rs`, `resource/`, `resource/via.rs`, `image.rs`, `image/config.rs`, `image/environment.rs`, `alert.rs`, `alert/`, `channel.rs`, `channel/`, `package.rs`, `package/`: contract kinds, library forms and SDK extraction. [contracts](map/contracts.md).
- `names.rs`, `names/`: external names and shared language readers. [languages](map/languages.md).
- `jvm/`: external names and shared language readers. [languages](map/languages.md), [seams](map/seams.md).
- `dotnet/`, `xml.rs`: shared build readers and extension rules. [seams](map/seams.md).
- `document/`, `config_values.rs`, `config_values/hcl.rs`, `config_values/sites.rs`, `iac.rs`, `iac/`, `kube.rs`, `literals.rs`, `zod.rs`, `declared.rs`: provider documents, config values, IaC and shapes. [documents](map/documents.md).
- `decisions.rs`, `decide.rs`, `decider/`: decision replay and opt-in decider. [decisions](map/decisions.md).
- `modules.rs`, `valkey.rs`, `postgres.rs`, `audit.rs`, `directory.rs`, `directory/`, `oidc.rs`, `oidc/`, `grant.rs`, `grant/`, `accounts.rs`, `accounts/`, `github_app.rs`, `github_app/`: cloud configuration, services, identity and grants. [cloud](map/cloud.md).
- `snapshot.rs`, `snapshot/`: snapshot store and generations. [snapshot](map/snapshot.md).
- `ui.rs`, `ui/`: management console API and frontend. [console](map/console.md).
- `workspaces.rs`, `workspaces/`, `catalog.rs`, `proposals.rs`: named workspaces, catalog and proposals. [workspaces](map/workspaces.md).

## Console frontend (`ui/`)

[Build and bundle rules](map/overview/console-frontend-ui.md), [console detail](map/console.md).

## Named workspaces (spec S5)

[Decisions and delivery](map/overview/named-workspaces-spec-s5.md), [workspaces](map/workspaces.md).

## Identities and formats

[Repository, symbol, call, contract and export identities](map/overview/identities-and-formats.md).

## Seams and what extends them

[Extension steps](map/overview/seams-and-what-extends-them.md) for kinds, packs, ecosystems,
documents, shapes, entry points, levels, config, MCP and CLI; [library and shared-reader
seams](map/seams.md). Read the relevant extension before changing it.

## Invariants the code relies on

[Storage, determinism, schemas, links, network and resolution invariants](map/overview/invariants-the-code-relies-on.md).

## Deployment (`deploy/`)

[Validation commands](map/overview/deployment-deploy.md), [deployment detail](map/deployment.md).

## Tests (`tests/`)

[Harness, goldens and fixtures](map/overview/tests-tests.md), [test subjects](map/tests.md).

## Split area files

[Navigation and byte budgets](map/overview/split-area-files.md). Prepare meaningful `## `
boundaries and use `cargo run --example split_agents -- <file>`; retain every entry.

## Known ceilings

[Limits by area](ceilings.md) and [how ceilings are recorded](map/overview/known-ceilings.md).
Read the relevant topic when a limit is touched; every `ponytail:` limit remains checked by
`tests/it/docs.rs`.
