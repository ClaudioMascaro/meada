# Contratos de pacotes

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Lê manifestos para ligar dependências aos pacotes que outros repositórios do workspace fornecem.

## Interface

Os adaptadores de `Ecosystem` cobrem npm, PyPI, Cargo, Go, Maven/Gradle, NuGet e SwiftPM.
`read_manifest` produz o resumo. O leitor de `yarn.lock` Berry compara resoluções e encontra
workspaces afetados.
[../package.rs:Ecosystem](../package.rs) [../package.rs:ECOSYSTEMS](../package.rs) [../package.rs:read_manifest](../package.rs) [lock.rs:diff](./lock.rs) [lock.rs:reached](./lock.rs)

O adaptador Maven atende POM e Gradle com o mesmo ecossistema. Em NuGet, `PackageId`,
`AssemblyName` e o nome do arquivo determinam o identificador nessa ordem. Em Go, a diretiva
`module` identifica o provedor.
[maven.rs:Maven](./maven.rs) [nuget.rs:Nuget](./nuget.rs) [go.rs:Go](./go.rs)

- [Registro de plugins npm no Backstage](./npm/README.md)
- [Pacotes no manifesto SwiftPM](./swiftpm/README.md)

## Invariantes

A identidade combina ecossistema e nome. Uma dependência só pode ligar a um provedor do workspace
no mesmo ecossistema. O adaptador PyPI normaliza nomes segundo PEP 503 e verifica versões com PEP
440.
[../package.rs:consume](../package.rs) [pypi.rs:normalize](./pypi.rs) [pep440.rs:accepts](./pep440.rs)

O leitor de lockfiles do Yarn Berry retorna `TooLarge` quando o arquivo excede 16 MiB. Nesse caso,
a análise de impacto não dispõe das resoluções do lockfile.
[lock.rs:read](./lock.rs) [lock.rs:MAX_LOCK_BYTES](./lock.rs)

## Efeitos colaterais

A declaração e o cruzamento de dependências acrescentam provedores e consumidores à exportação em
memória. A leitura dos manifestos usa os bytes fornecidos.
[../package.rs:declare](../package.rs) [../package.rs:consume](../package.rs) [../package.rs:read_manifest](../package.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- package:: packages::
```
