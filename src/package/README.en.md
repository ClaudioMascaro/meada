# Package contracts

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Reads manifests to connect dependencies to packages provided by other workspace repositories.

## Interface

`Ecosystem` adapters cover npm, PyPI, Cargo, Go, Maven/Gradle, NuGet and SwiftPM. `read_manifest`
produces the summary. The Yarn Berry `yarn.lock` reader compares resolutions and finds affected
workspaces.
[../package.rs:Ecosystem](../package.rs) [../package.rs:ECOSYSTEMS](../package.rs) [../package.rs:read_manifest](../package.rs) [lock.rs:diff](./lock.rs) [lock.rs:reached](./lock.rs)

The Maven adapter handles POM and Gradle files in one ecosystem. For NuGet, `PackageId`,
`AssemblyName` and the file name determine the ID in that order. In Go, the `module` directive
identifies the provider.
[maven.rs:Maven](./maven.rs) [nuget.rs:Nuget](./nuget.rs) [go.rs:Go](./go.rs)

- [Backstage npm plugin registration](./npm/README.en.md)
- [Packages in SwiftPM manifests](./swiftpm/README.en.md)

## Invariants

Identity combines ecosystem and name. A dependency can link only to a workspace provider in its
own ecosystem. The PyPI adapter normalizes names according to PEP 503 and checks versions with PEP
440.
[../package.rs:consume](../package.rs) [pypi.rs:normalize](./pypi.rs) [pep440.rs:accepts](./pep440.rs)

The Yarn Berry lockfile reader returns `TooLarge` for files over 16 MiB, leaving their lock
resolutions unavailable for impact analysis.
[lock.rs:read](./lock.rs) [lock.rs:MAX_LOCK_BYTES](./lock.rs)

## Side effects

Declaration and dependency matching add providers and consumers to the in-memory export. Manifest
readers use the supplied bytes.
[../package.rs:declare](../package.rs) [../package.rs:consume](../package.rs) [../package.rs:read_manifest](../package.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- package:: packages::
```
