# NuGet packages

- NuGet packages (spec #28, ticket #116): `package/nuget.rs` implements `Ecosystem` for
  `*.csproj`, read by `dotnet/msbuild.rs` (`Project`: usings, the `PackageId`, `AssemblyName` and
  `PackageVersion`/`Version` properties as written, and the `PackageReference` and
  `ProjectReference` items), the reader the C# pack's `join` shares. A project with an SDK
  provides `package::nuget::<id>`: `PackageId`, else `AssemblyName`, else the file name; a
  computed (`$(..)`) or invalid one makes no provider. A `PackageReference` is an entry with its
  `Version` as range (`range unchecked`). A `ProjectReference` is an entry whose `via` is
  `path <project file>`, folded from the project's directory (backslashes read as `/`), named by
  the file name until `Ecosystem::settle`, at the start of `Packages::workspace_step`, gives it
  the id of the project at that path in another repository; `reaches` keeps it to that project
  and to other repositories (`sourced`), so a reference inside one repository adds no package
  link. `entries` adds what the projects of the repository the file's project references pass
  on (`dotnet::references::referenced`, #369; `Entry::private` marks a `PrivateAssets` one), and
  Maven's what the modules of the repository its poms depend on pass on (`compile`, not
  `optional`). Imports share Maven's code (`maven::{imports, resolutions, declares}`, with `How`
  `Namespace`, the reason's `; import namespace`): the `.cs` exports rows give a public type its
  qualified name, as Java's do. `How::Namespace` is a new value of the exported `how`; the
  stored format gains only a `manifests` summary kind, with `tests/fixtures/schema/sample.csproj`.
