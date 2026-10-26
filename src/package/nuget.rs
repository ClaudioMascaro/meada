//! NuGet packages (spec #28): each SDK-style `.csproj` provides `package::nuget::<id>`, the id
//! being its `PackageId`, else its `AssemblyName`, else its file name, and each `PackageReference`
//! or cross-repository `ProjectReference` naming a provider of the workspace consumes it. The
//! project file is read by `dotnet::msbuild`, the reader the C# pack shares. The module answers
//! the trait's questions as follows:
//!
//! - `entries`: the references of the nearest project holding the file, once each.
//! - `reaches`: a `ProjectReference` (an entry with a `path` source, as Cargo's) only the project
//!   at that path, and only in another repository: inside its own repository it stays inside it,
//!   where the pack resolves it. A `PackageReference` reaches every provider of the id.
//! - `settle`: a `ProjectReference` names the project's file, so the workspace step first gives
//!   the entry the id of the project at that path.
//! - `resolutions` and `declares`: Java's rule (`maven`), with a namespace as the package: a type
//!   a `using` or a written name brings is looked up under the qualified name the provider's
//!   `exports` rows give its public types, among the files of the projects the file depends on,
//!   and the method is picked by name and argument count. `import namespace` is the reason.
//!
//! Package ids are matched as written, and neither `Directory.Packages.props` nor a feed is read.

use super::maven;
use super::{
    Ecosystem, Entry, Export, How, Manifest, Provided, Providers, Resolution, Summary,
    bare_directory, owner, sourced, valid_name,
};
use crate::dotnet::msbuild;
use crate::lang::Call;
use crate::repo::relative;
use crate::resolve::Code;
use std::collections::HashSet;
use std::path::Path;

/// The field a `PackageReference` is kept under in `Summary::dependencies`.
const PACKAGE: &str = "PackageReference";
/// The field a `ProjectReference` is kept under; its entry's `via` is `path <project file>`.
const PROJECT: &str = "ProjectReference";
const EXTENSION: &str = ".csproj";

/// Whether `value` can be a name: valid, and no `$(Property)` MSBuild would evaluate.
fn literal(value: &str) -> bool {
    valid_name(value) && !value.contains("$(")
}

/// The project file name of `path`, less its extension.
fn stem(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(EXTENSION).unwrap_or(file)
}

/// The `ProjectReference` to `include`, written in the project at `dir`: its entry, named by the
/// project file's name until `settle` knows the project's id. `None` for a path that is not a
/// `.csproj` of the repository or beyond it (absolute, or computed).
fn project(dir: &str, include: &str) -> Option<Entry> {
    let path = include.replace('\\', "/");
    let at = relative(dir, &path)?;
    let written = !include.contains("$(") && !include.contains(':');
    (written && at.ends_with(EXTENSION) && literal(stem(&at))).then(|| Entry {
        field: PROJECT.to_string(),
        name: stem(&at).to_string(),
        via: Some(format!("path {at}")),
        ..Entry::default()
    })
}

// ponytail: package ids are matched as written (NuGet's are case-insensitive), a project's
// files are those of its directory (two projects in one share it), and an extension method or a
// fully qualified name with no `using` is not followed; a case-folded key and the pack's
// project globs lift them.
pub struct Nuget;

impl Ecosystem for Nuget {
    fn name(&self) -> &'static str {
        "nuget"
    }

    fn is_manifest(&self, file_name: &str) -> bool {
        file_name.ends_with(EXTENSION)
    }

    /// The project's id and version as it writes them (a computed one makes no provider), and its
    /// references. Hostile bytes, and a file with no SDK, read as empty.
    fn read(&self, path: &str, src: &[u8]) -> Summary {
        let Some(project_file) = msbuild::read(src) else {
            return Summary::default();
        };
        let id = (project_file.package_id.clone())
            .or(project_file.assembly_name.clone())
            .unwrap_or_else(|| stem(path).to_string());
        let dir = bare_directory(path);
        let packages = (project_file.packages.iter())
            .filter(|(name, _)| literal(name))
            .map(|(name, range)| Entry {
                field: PACKAGE.to_string(),
                name: name.clone(),
                range: range.clone(),
                ..Entry::default()
            });
        let projects = project_file.projects.iter().filter_map(|i| project(dir, i));
        let mut dependencies: Vec<Entry> = packages.chain(projects).collect();
        dependencies.sort_by(|a, b| (&a.field, &a.name, &a.via).cmp(&(&b.field, &b.name, &b.via)));
        dependencies.dedup();
        Summary {
            name: literal(&id).then_some(id),
            version: project_file.version.filter(|v| !v.contains("$(")),
            dependencies,
            ..Summary::default()
        }
    }

    fn sources(&self) -> &'static [&'static str] {
        &["cs"]
    }

    fn names_package(&self, name: &str) -> bool {
        name.split('.')
            .all(|s| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_'))
    }

    fn imports<'a>(&self, code: &Code<'a>, path: &'a str, call: &'a Call) -> Vec<(String, String)> {
        maven::imports(code, path, call)
    }

    /// A constructor or a method of the class itself, by its id: the class's id, a dot, then a
    /// signature; a nested type's member is not.
    fn member_of(&self, target: &str, class: &str) -> bool {
        (target.strip_prefix(class))
            .and_then(|rest| rest.strip_prefix('.'))
            .and_then(|rest| rest.split_once('('))
            .is_some_and(|(name, _)| !name.is_empty() && !name.contains('.'))
    }

    fn reaches(
        &self,
        (via, _): (Option<&str>, Option<&str>),
        from: (&str, &Path),
        to: ((&str, &Path), &str),
    ) -> bool {
        via.is_none() || from.0 != (to.0).0 && sourced(via, from, to)
    }

    /// The references of the project file `from.1` belongs to: the nearest `.csproj` above it,
    /// once for each package.
    fn entries<'e>(
        &self,
        exports: &'e [Export],
        _providers: &Providers,
        (n, from): (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)> {
        let mut seen = HashSet::new();
        (owner(&exports[n].manifests, from, self).into_iter())
            .flat_map(|m| &m.summary.dependencies)
            .filter(|e| seen.insert(e.name.as_str()))
            .map(|e| (e, specifier.to_string()))
            .collect()
    }

    fn resolutions<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        from: (usize, &str),
        import: (&str, &str),
        known: &dyn Fn(&Provided<'e>, &str) -> bool,
    ) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
        maven::resolutions(
            (self, How::Namespace),
            exports,
            providers,
            at,
            from,
            import,
            known,
        )
    }

    fn declares<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        from: (usize, &str),
        import: (&str, &str),
    ) -> bool {
        maven::declares(self, exports, providers, at, from, import)
    }

    /// Each `ProjectReference` to a project of another repository takes that project's id, which
    /// its file name does not always say.
    fn settle(&self, exports: &mut [Export]) {
        let projects: Vec<(usize, usize, usize, String)> = (exports.iter().enumerate())
            .flat_map(|(n, export)| {
                export
                    .manifests
                    .iter()
                    .enumerate()
                    .map(move |(i, m)| (n, i, m))
            })
            .filter(|(_, _, m)| self.is_manifest(m.path.rsplit('/').next().unwrap_or("")))
            .flat_map(|(n, i, m)| {
                let entries = m.summary.dependencies.iter().enumerate();
                entries
                    .filter(|(_, e)| e.field == PROJECT)
                    .map(move |(k, e)| (n, i, k, e))
            })
            .filter_map(|(n, i, k, e)| {
                let from = exports[n].place();
                let named =
                    |m: &Manifest| m.summary.name.clone().filter(|_| self.is_manifest(&m.path));
                let id = (exports.iter()).find_map(|x| {
                    let mut found = x.manifests.iter().filter(|m| {
                        self.reaches((e.via.as_deref(), None), from, (x.place(), &m.path))
                    });
                    found.find_map(named)
                })?;
                Some((n, i, k, id))
            })
            .collect();
        for (n, i, k, id) in projects {
            exports[n].manifests[i].summary.dependencies[k].name = id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &str, src: &str) -> Summary {
        Nuget.read(path, src.as_bytes())
    }

    #[test]
    fn a_project_is_named_by_its_package_id_else_assembly_name_else_file() {
        let named = |props: &str| {
            let src =
                format!("<Project Sdk=\"S\"><PropertyGroup>{props}</PropertyGroup></Project>");
            read("src/Kit/Kit.csproj", &src).name
        };
        assert_eq!(named("").as_deref(), Some("Kit"));
        assert_eq!(
            named("<AssemblyName>A.B</AssemblyName>").as_deref(),
            Some("A.B")
        );
        let both = "<AssemblyName>A.B</AssemblyName><PackageId>P</PackageId>";
        assert_eq!(named(both).as_deref(), Some("P"));
        // A computed or invalid id makes no provider, and does not fall back to another name.
        assert_eq!(named("<PackageId>$(Id)</PackageId>"), None);
        assert_eq!(named("<AssemblyName>a b</AssemblyName>"), None);
        assert_eq!(read("Kit.csproj", "<Project/>").name, None);
    }

    #[test]
    fn hostile_project_files_read_as_empty() {
        let dtd = "<!DOCTYPE p [<!ENTITY e SYSTEM 'file:///etc/passwd'>]><Project Sdk=\"S\"/>";
        let deep = format!(
            "<Project Sdk=\"S\">{}{}</Project>",
            "<a>".repeat(100),
            "</a>".repeat(100)
        );
        let long = format!(
            "<Project Sdk=\"S\"><PropertyGroup><PackageId>{}</PackageId></PropertyGroup></Project>",
            "a".repeat(300)
        );
        let hostile: [&str; 5] = [dtd, "<Project Sdk=\"S\">", "not xml", &deep, &long];
        for src in hostile {
            assert_eq!(read("Kit.csproj", src), Summary::default(), "{src}");
        }
        assert_eq!(Nuget.read("Kit.csproj", &[0xff, 0xfe]), Summary::default());
    }

    #[test]
    fn references_keep_their_range_and_a_project_reference_its_location() {
        let src = r#"<Project Sdk="S"><ItemGroup>
            <PackageReference Include="Acme.Kit" Version="[1,2)" />
            <PackageReference Include="$(Computed)" Version="1" />
            <ProjectReference Include="..\Core\Core.csproj" />
            <ProjectReference Include="..\..\..\other\Money\Money.csproj" />
            <ProjectReference Include="$(Root)\X.csproj" />
            <ProjectReference Include="..\Core\Core.vbproj" />
            <ProjectReference Include="C:\abs\X.csproj" />
          </ItemGroup></Project>"#;
        let rows: Vec<_> = read("src/App/App.csproj", src)
            .dependencies
            .into_iter()
            .map(|e| (e.field, e.name, e.range, e.via))
            .collect();
        let row = |f: &str, n: &str, r: Option<&str>, v: Option<&str>| {
            (
                f.to_string(),
                n.to_string(),
                r.map(String::from),
                v.map(String::from),
            )
        };
        assert_eq!(
            rows,
            [
                row("PackageReference", "Acme.Kit", Some("[1,2)"), None),
                row(
                    "ProjectReference",
                    "Core",
                    None,
                    Some("path src/Core/Core.csproj")
                ),
                row(
                    "ProjectReference",
                    "Money",
                    None,
                    Some("path ../other/Money/Money.csproj")
                ),
            ]
        );
    }

    #[test]
    fn a_project_reference_reaches_a_project_of_another_repository_only() {
        let at = |id, root| (id, Path::new(root));
        let path = (Some("path ../lib/Kit/Kit.csproj"), None);
        let kit = (at("lib", "/w/lib"), "Kit/Kit.csproj");
        assert!(Nuget.reaches(path, at("app", "/w/app"), kit));
        assert!(!Nuget.reaches(
            path,
            at("app", "/w/app"),
            (at("lib", "/w/lib"), "Other.csproj")
        ));
        // Inside its own repository the reference stays inside it.
        let own = (Some("path Kit/Kit.csproj"), None);
        assert!(!Nuget.reaches(own, at("lib", "/w/lib"), kit));
        assert!(Nuget.reaches((None, Some("1")), at("lib", "/w/lib"), kit));
    }
}
