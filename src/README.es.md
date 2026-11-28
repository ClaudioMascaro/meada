# Motor, CLI y servidor MCP

La CLI y los dos transportes MCP comparten el motor de consultas. Los adaptadores por lenguaje y tipo de contrato alimentan el índice.

## Interfaz

El motor ofrece búsqueda, contexto, trazado y exportación. El servidor expone las tres consultas por stdio o HTTP. `[engine.rs:Engine]` `[mcp.rs:serve]` `[mcp.rs:listen]`

## Invariantes

En modo servidor, el motor abre índices existentes para lectura. En modo local, actualiza los pares antes del repositorio principal. `[engine.rs:open_manifest]` `[engine.rs:refresh]`

## Efectos secundarios

La indexación escribe en SQLite. El listener HTTP abre un socket y el transporte stdio usa los flujos del proceso. `[index.rs:refresh]` `[mcp.rs:listen]` `[mcp.rs:serve]`

En modo local, `meada decide` puede escribir `meada.decisions.toml` tras comprobar el destino. `[decide.rs:open]` `[decisions.rs:save]`

## Componentes

- [accounts](./accounts/README.es.md)
- [alert](./alert/README.es.md)
- [channel](./channel/README.es.md)
- [config_values](./config_values/README.es.md)
- [contract](./contract/README.es.md)
- [decider](./decider/README.es.md)
- [directory](./directory/README.es.md)
- [document](./document/README.es.md)
- [dotnet](./dotnet/README.es.md)
- [github_app](./github_app/README.es.md)
- [grant](./grant/README.es.md)
- [http](./http/README.es.md)
- [iac](./iac/README.es.md)
- [image](./image/README.es.md)
- [index](./index/README.es.md)
- [jvm](./jvm/README.es.md)
- [kube](./kube/README.es.md)
- [lang](./lang/README.es.md)
- [names](./names/README.es.md)
- [oidc](./oidc/README.es.md)
- [package](./package/README.es.md)
- [resolve](./resolve/README.es.md)
- [resource](./resource/README.es.md)
- [snapshot](./snapshot/README.es.md)
- [trace](./trace/README.es.md)
- [ui](./ui/README.es.md)
- [workspaces](./workspaces/README.es.md)

## Verificación

Desde la raíz del repositorio, ejecuta `cargo test -p meada`.
