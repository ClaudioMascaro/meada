//! NuGet packages (spec #28): each SDK-style `.csproj` provides `package::nuget::<id>`, the id
//! being its `PackageId`, else its `AssemblyName`, else its file name, and each `PackageReference`
//! or cross-repository `ProjectReference` naming a provider of the workspace consumes it. The
//! project file is read by `dotnet::msbuild`, the reader the C# pack shares. The module answers
//! the trait's questions as follows:
//!
//! - `entries`: the references of the nearest project holding the file, then those the projects
//!   it references in its repository pass on (`dotnet::references`), nearest first, once each.
//! - `reaches`: a `ProjectReference` (an entry with a `path` source, as Cargo's) only the project
//!   at that path, and only in another repository: inside its own repository it stays inside it,
//!   where the pack resolves it. A `PackageReference` reaches every provider of the id.
//! - `settle`: a `ProjectReference` names the project's file, so the workspace step first gives
//!   the entry the id of the project at that path, and a `PackageReference` the provider's own
//!   spelling of an id that differs by case only.
//! - `owns`: an SDK-style project only (`Summary::project`), as the pack's `join`.
//! - `resolutions` and `declares`: Java's rule (`maven`), with a namespace as the package: a type
//!   a `using` or a written name brings is looked up under the qualified name the provider's
//!   `exports` rows give its public types, among the files of the projects the file depends on,
//!   and the method is picked by name and argument count. `import namespace` is the reason.
//!
//! Package ids are matched without case; neither `Directory.Packages.props` nor a feed is read.

use super::maven;
use super::{
    Ecosystem, Entry, Export, How, Manifest, Provided, Providers, Resolution, Summary,
    bare_directory, folded, owner, sourced, valid_name,
};
use crate::dotnet::{msbuild, references};
use crate::lang::Call;
use crate::resolve::Code;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

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

/// The `ProjectReference` `reference`, written in the project at `dir`: its entry, named by the
/// project file's name until `settle` knows the project's id. `None` for a path that is not a
/// `.csproj` of the repository or beyond it (absolute, or computed).
fn project(dir: &str, reference: &msbuild::Reference) -> Option<Entry> {
    let at = references::located(dir, &reference.include)?;
    Some(Entry {
        field: PROJECT.to_string(),
        name: stem(&at).to_string(),
        via: Some(format!("path {at}")),
        private: reference.private,
        ..Entry::default()
    })
}

/// The project file of the repository the `ProjectReference` entry names.
fn in_repository(entry: &Entry) -> Option<&str> {
    let at = entry.via.as_deref()?.strip_prefix("path ")?;
    (entry.field == PROJECT && !at.starts_with("../")).then_some(at)
}

// ponytail: a `ProjectReference` path is matched in the case written (ids are case-folded), a
// project's files are those of its directory (two projects in one share it), and an extension
// method, a `var` holding a call's result and a fully qualified name with no `using` are not
// followed; a case-folded path key and the pack's project globs lift them.
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
            .filter(|r| literal(&r.include))
            .map(|r| Entry {
                field: PACKAGE.to_string(),
                name: r.include.clone(),
                range: r.version.clone(),
                private: r.private,
                ..Entry::default()
            });
        let projects = project_file.projects.iter().filter_map(|r| project(dir, r));
        let mut dependencies: Vec<Entry> = packages.chain(projects).collect();
        dependencies.sort_by(|a, b| (&a.field, &a.name, &a.via).cmp(&(&b.field, &b.name, &b.via)));
        dependencies.dedup();
        Summary {
            name: literal(&id).then_some(id),
            version: project_file.version.filter(|v| !v.contains("$(")),
            dependencies,
            project: true,
            ..Summary::default()
        }
    }

    fn sources(&self) -> &'static [&'static str] {
        &["cs"]
    }

    /// Only an SDK-style project bounds a package, as the pack's `join` has it.
    fn owns(&self, manifest: &Manifest) -> bool {
        manifest.summary.project
    }

    fn names_package(&self, name: &str) -> bool {
        name.split('.')
            .all(|s| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_'))
    }

    fn imports<'a>(&self, code: &Code<'a>, path: &'a str, call: &'a Call) -> Vec<(String, String)> {
        // ponytail: a workspace type in `System.*` (a polyfill) is not followed.
        maven::imports(code, path, call, &["System."])
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

    /// The references of the project file `from.1` belongs to (the nearest `.csproj` above it),
    /// then those that pass on of each project it references in its repository, directly or not
    /// (`references::referenced`), nearest first, once for each package: the nearer entry wins.
    fn entries<'e>(
        &self,
        exports: &'e [Export],
        _providers: &Providers,
        (n, from): (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)> {
        let manifests = &exports[n].manifests;
        let Some(own) = owner(manifests, from, self) else {
            return Vec::new();
        };
        let projects: HashMap<&str, &Summary> = (manifests.iter())
            .filter(|m| self.owns(m))
            .map(|m| (m.path.as_str(), &m.summary))
            .collect();
        let walked = references::referenced(&own.path, |path| {
            (projects.get(path).copied().into_iter())
                .flat_map(|s| &s.dependencies)
                .filter_map(|e| Some((in_repository(e)?, e.private)))
                .collect()
        });
        let passed = (walked.into_iter())
            .filter_map(|path| projects.get(path).copied())
            .flat_map(|s| s.dependencies.iter().filter(|e| !e.private));
        let mut seen = HashSet::new();
        (own.summary.dependencies.iter().chain(passed))
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
    /// its file name does not always say, and each `PackageReference` the spelling of the one
    /// provider whose id differs from it by case only (ids are case-insensitive).
    fn settle(&self, exports: &mut [Export]) {
        let nuget = |m: &Manifest| self.is_manifest(m.path.rsplit('/').next().unwrap_or(""));
        // Each named project by its folded location, with its repository, and each id by its
        // lowercase spelling.
        let mut located: HashMap<PathBuf, (String, String)> = HashMap::new();
        let mut spellings: HashMap<String, BTreeSet<String>> = HashMap::new();
        for export in exports.iter() {
            let (repository, root) = export.place();
            for m in export.manifests.iter().filter(|m| nuget(m)) {
                let Some(name) = &m.summary.name else {
                    continue;
                };
                let at = folded(&root.join(&m.path));
                located
                    .entry(at)
                    .or_insert((repository.to_string(), name.clone()));
                spellings
                    .entry(name.to_lowercase())
                    .or_default()
                    .insert(name.clone());
            }
        }
        for export in exports.iter_mut() {
            let (repository, root) = (export.repository.clone(), export.root.clone());
            let manifests = export.manifests.iter_mut().filter(|m| nuget(m));
            for entry in manifests.flat_map(|m| &mut m.summary.dependencies) {
                if entry.field == PROJECT {
                    // Only a path that leaves its repository names another one's project.
                    let path = (entry.via.as_deref())
                        .and_then(|v| v.strip_prefix("path "))
                        .filter(|p| p.starts_with("../"));
                    let found = path.and_then(|p| located.get(&folded(&root.join(p))));
                    if let Some((_, name)) = found.filter(|(other, _)| *other != repository) {
                        entry.name = name.clone();
                    }
                } else if let Some(ids) = spellings.get(&entry.name.to_lowercase())
                    && !ids.contains(&entry.name)
                    && let [only] = ids.iter().collect::<Vec<_>>()[..]
                {
                    entry.name = only.clone();
                }
            }
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
        assert_eq!(
            named(&format!("<PackageId>{}</PackageId>", "a".repeat(300))),
            None
        );
        assert_eq!(read("Kit.csproj", "<Project/>").name, None);
    }

    #[test]
    fn only_a_project_with_an_sdk_owns_the_files_under_it() {
        let owns = |src: &str| {
            let summary = read("src/Kit/Kit.csproj", src);
            Nuget.owns(&Manifest {
                path: "src/Kit/Kit.csproj".to_string(),
                summary,
            })
        };
        assert!(owns("<Project Sdk=\"Microsoft.NET.Sdk\"/>"));
        assert!(!owns("<Project><PropertyGroup/></Project>"));
        assert!(!owns("not xml"));
    }

    #[test]
    fn a_project_reference_keeps_an_entry_whatever_its_file_is_called() {
        let src = r#"<Project Sdk="S"><ItemGroup>
            <ProjectReference Include="..\My Lib\My Lib.csproj" />
          </ItemGroup></Project>"#;
        let entries = read("src/App/App.csproj", src).dependencies;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "My Lib");
        assert_eq!(
            entries[0].via.as_deref(),
            Some("path src/My Lib/My Lib.csproj")
        );
    }

    #[test]
    fn hostile_project_files_read_as_empty() {
        let dtd = "<!DOCTYPE p [<!ENTITY e SYSTEM 'file:///etc/passwd'>]><Project Sdk=\"S\"/>";
        let deep = format!(
            "<Project Sdk=\"S\">{}{}</Project>",
            "<a>".repeat(100),
            "</a>".repeat(100)
        );
        let hostile: [&str; 4] = [dtd, "<Project Sdk=\"S\">", "not xml", &deep];
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
    fn a_file_sees_what_its_referenced_projects_pass_on_and_the_nearer_entry_wins() {
        let project = |dir: &str, items: &str| {
            let path = format!("src/{dir}/{dir}.csproj");
            let src = format!("<Project Sdk=\"S\"><ItemGroup>{items}</ItemGroup></Project>");
            Manifest {
                summary: read(&path, &src),
                path,
            }
        };
        let package = |id: &str, more: &str| format!("<PackageReference Include=\"{id}\" {more}/>");
        let reference = |dir: &str, more: &str| {
            format!("<ProjectReference Include=\"..\\{dir}\\{dir}.csproj\" {more}/>")
        };
        let manifests = vec![
            project(
                "App",
                &[package("X", "Version=\"1\""), reference("Lib", "")].concat(),
            ),
            project(
                "Lib",
                &[
                    package("X", "Version=\"2\""),
                    package("Y", ""),
                    package("P", "PrivateAssets=\"all\""),
                    reference("Mid", ""),
                    reference("Hidden", "PrivateAssets=\"all\""),
                ]
                .concat(),
            ),
            project("Mid", &package("Z", "")),
            project("Hidden", &package("H", "")),
        ];
        let exports = [Export {
            manifests,
            ..Export::default()
        }];
        let providers = Providers::new(&exports);
        let found: Vec<_> = (Nuget
            .entries(&exports, &providers, (0, "src/App/A.cs"), "c")
            .iter())
        .map(|(e, _)| format!("{}{}", e.name, e.range.as_deref().unwrap_or("")))
        .collect();
        assert_eq!(found, ["X1", "Lib", "Y", "Mid", "Z"]);
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
