# Motor, CLI e servidor MCP

A CLI e os dois transportes MCP compartilham o motor de consultas. Adaptadores por linguagem e por tipo de contrato alimentam o índice.

## Interface

O motor expõe busca, contexto, rastreamento e exportação. O servidor publica as três consultas por stdio ou HTTP. `[engine.rs:Engine]` `[mcp.rs:serve]` `[mcp.rs:listen]`

## Invariantes

No modo servidor, o motor abre índices existentes para leitura. No modo local, atualiza os pares antes do repositório principal. `[engine.rs:open_manifest]` `[engine.rs:refresh]`

## Efeitos colaterais

A indexação grava SQLite. O listener HTTP abre um socket, e o transporte stdio usa os fluxos do processo. `[index.rs:refresh]` `[mcp.rs:listen]` `[mcp.rs:serve]`

No modo local, `meada decide` pode gravar `meada.decisions.toml` após verificar o destino. `[decide.rs:open]` `[decisions.rs:save]`

## Componentes

- [accounts](./accounts/README.md)
- [alert](./alert/README.md)
- [channel](./channel/README.md)
- [config_values](./config_values/README.md)
- [contract](./contract/README.md)
- [decider](./decider/README.md)
- [directory](./directory/README.md)
- [document](./document/README.md)
- [dotnet](./dotnet/README.md)
- [github_app](./github_app/README.md)
- [grant](./grant/README.md)
- [http](./http/README.md)
- [iac](./iac/README.md)
- [image](./image/README.md)
- [index](./index/README.md)
- [jvm](./jvm/README.md)
- [kube](./kube/README.md)
- [lang](./lang/README.md)
- [names](./names/README.md)
- [oidc](./oidc/README.md)
- [package](./package/README.md)
- [resolve](./resolve/README.md)
- [resource](./resource/README.md)
- [snapshot](./snapshot/README.md)
- [trace](./trace/README.md)
- [ui](./ui/README.md)
- [workspaces](./workspaces/README.md)

## Verificação

Na raiz do repositório, execute `cargo test -p meada`.
