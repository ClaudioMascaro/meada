# Contratos de paquetes

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Lee manifiestos para vincular dependencias con paquetes suministrados por otros repositorios del
espacio de trabajo.

## Interfaz

Los adaptadores de `Ecosystem` cubren npm, PyPI, Cargo, Go, Maven/Gradle, NuGet y SwiftPM.
`read_manifest` genera el resumen. El lector de `yarn.lock` de Yarn Berry compara resoluciones y
encuentra espacios de trabajo afectados.
[../package.rs:Ecosystem](../package.rs) [../package.rs:ECOSYSTEMS](../package.rs) [../package.rs:read_manifest](../package.rs) [lock.rs:diff](./lock.rs) [lock.rs:reached](./lock.rs)

El adaptador de Maven trata POM y Gradle en un ecosistema común. En NuGet, `PackageId`,
`AssemblyName` y el nombre del archivo determinan el identificador en ese orden. En Go, la
directiva `module` identifica al proveedor.
[maven.rs:Maven](./maven.rs) [nuget.rs:Nuget](./nuget.rs) [go.rs:Go](./go.rs)

- [Registro de plugins npm en Backstage](./npm/README.es.md)
- [Paquetes en el manifiesto SwiftPM](./swiftpm/README.es.md)

## Invariantes

La identidad combina ecosistema y nombre. Una dependencia solo puede enlazar con un proveedor del
espacio de trabajo del mismo ecosistema. El adaptador PyPI normaliza nombres según PEP 503 y
comprueba versiones con PEP 440.
[../package.rs:consume](../package.rs) [pypi.rs:normalize](./pypi.rs) [pep440.rs:accepts](./pep440.rs)

El lector de lockfiles de Yarn Berry devuelve `TooLarge` si el archivo supera los 16 MiB, de modo
que el análisis de impacto no dispone de sus resoluciones.
[lock.rs:read](./lock.rs) [lock.rs:MAX_LOCK_BYTES](./lock.rs)

## Efectos secundarios

La declaración y el cruce de dependencias añaden proveedores y consumidores a la exportación en
memoria. Los lectores de manifiestos usan los bytes recibidos.
[../package.rs:declare](../package.rs) [../package.rs:consume](../package.rs) [../package.rs:read_manifest](../package.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- package:: packages::
```
