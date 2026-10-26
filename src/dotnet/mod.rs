//! What the .NET pack and its ecosystems share (spec #28): the MSBuild project file reader
//! (`msbuild`), which the C# pack reads projects and their usings with, and which the NuGet
//! package ecosystem reads manifests with. The readers read only the supplied bytes, never
//! the .NET SDK, MSBuild or another file.

pub mod msbuild;
