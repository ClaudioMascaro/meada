# .NET projects and configuration

Shared readers for MSBuild projects, project references and .NET configuration.

## Interface

The reader extracts properties, usings and references from SDK projects and supplied `Directory.Build.props` files. Configuration resolves keys such as `A:B` in the project’s `appsettings.json`. `[msbuild.rs:read]` `[msbuild.rs:chain]` `[references.rs:referenced]` `[appsettings.rs:Settings]`

## Invariants

Configuration lookup compares segments case-insensitively and accepts only string values. Project reference traversal bounds its depth. `[appsettings.rs:get]` `[references.rs:REFERENCE_DEPTH]`

The `Directory.Build.props` import chain includes up to eight files. `[msbuild.rs:chain]` `[msbuild.rs:PROPS_DEPTH]`

## Side Effects

Readers use caller-supplied bytes and projects without starting MSBuild or the .NET SDK. `[msbuild.rs:read]` `[references.rs:referenced]`

## Verification

From the repository root, run `cargo test -p meada --test it nugetpkg::`.
