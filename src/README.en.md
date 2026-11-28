# Engine, CLI and MCP server

The CLI and both MCP transports share the query engine. Language and contract adapters feed the index.

## Interface

The engine provides search, context, trace and export. The server exposes the three queries over stdio or HTTP. `[engine.rs:Engine]` `[mcp.rs:serve]` `[mcp.rs:listen]`

## Invariants

In server mode the engine opens existing indexes for reading. In local mode it refreshes peers before the primary repository. `[engine.rs:open_manifest]` `[engine.rs:refresh]`

## Side effects

Indexing writes SQLite. The HTTP listener opens a socket and the stdio transport uses the process streams. `[index.rs:refresh]` `[mcp.rs:listen]` `[mcp.rs:serve]`

In local mode, `meada decide` can write `meada.decisions.toml` after checking the destination. `[decide.rs:open]` `[decisions.rs:save]`

## Components

- [accounts](./accounts/README.en.md)
- [alert](./alert/README.en.md)
- [channel](./channel/README.en.md)
- [config_values](./config_values/README.en.md)
- [contract](./contract/README.en.md)
- [decider](./decider/README.en.md)
- [directory](./directory/README.en.md)
- [document](./document/README.en.md)
- [dotnet](./dotnet/README.en.md)
- [github_app](./github_app/README.en.md)
- [grant](./grant/README.en.md)
- [http](./http/README.en.md)
- [iac](./iac/README.en.md)
- [image](./image/README.en.md)
- [index](./index/README.en.md)
- [jvm](./jvm/README.en.md)
- [kube](./kube/README.en.md)
- [lang](./lang/README.en.md)
- [names](./names/README.en.md)
- [oidc](./oidc/README.en.md)
- [package](./package/README.en.md)
- [resolve](./resolve/README.en.md)
- [resource](./resource/README.en.md)
- [snapshot](./snapshot/README.en.md)
- [trace](./trace/README.en.md)
- [ui](./ui/README.en.md)
- [workspaces](./workspaces/README.en.md)

## Verification

From the repository root, run `cargo test -p meada`.
