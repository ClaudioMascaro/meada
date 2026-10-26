//! What the .NET pack and its ecosystems share (spec #28): the MSBuild project file reader
//! (`msbuild`), which the C# pack reads projects and their usings with, and which the NuGet
//! package ecosystem is to read manifests with, and the `appsettings.json` reader (`appsettings`)
//! of the C# contract kinds. The readers read only the supplied bytes, never
//! the .NET SDK, MSBuild or another file.

pub mod appsettings;
pub mod msbuild;
