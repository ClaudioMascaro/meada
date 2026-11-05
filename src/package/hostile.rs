//! Hostile input (`CODING_STANDARDS.md`): the readers of the manifests and configurations a
//! repository holds, each fed a file built to explode (many keys, deep nesting, wide fan-out)
//! within `lang::MAX_BYTES`, the most any of them reads. `probes` is the table a new reader adds
//! its row to: a row reads such a file and answers the size of what the reader returned, and the
//! test holds every row to `BUDGET` in time and to `MAX_OUT` in size. A file past a format's own
//! limit (`XML_NODES`, `MAX_NESTING`) is unreadable, which is what that row proves; a row just
//! under the limit proves the reader.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::maven::Maven;
use super::nuget::Nuget;
use super::{Ecosystem, Export, Manifest, Providers, read_manifest};
use crate::config_values;
use crate::document::events::{self, Syntax};
use crate::dotnet::{appsettings, msbuild};
use crate::jvm::spring;
use crate::lang::csharp::CSharp;
use crate::lang::swift::Swift;
use crate::lang::{Facts, LanguagePack, MAX_BYTES};

/// The time of the whole table. A row takes about a second in debug; a reader that does work per
/// pair of nodes, or per link of a chain, misses any bound.
const BUDGET: Duration = Duration::from_secs(30);

/// What one reader may return for a file of `MAX_BYTES`: a few times its input, since a key is
/// joined with its parents (`YAML_KEY_BUDGET`) and a summary repeats a name.
const MAX_OUT: usize = 4 * MAX_BYTES;

/// How deep a row nests a file whose format puts no limit of its own on its size.
const NESTING: usize = 100_000;

/// The projects of a repository a row puts under one props file.
const PROJECTS: usize = 200;

/// `unit(0)`, `unit(1)`, ... until the text reaches the `share`th part of `MAX_BYTES` less the
/// room a row needs around it.
fn fill(share: usize, unit: impl Fn(usize) -> String) -> String {
    let mut text = String::new();
    let mut i = 0;
    while text.len() < (MAX_BYTES - (1 << 12)) / share {
        text += &unit(i);
        i += 1;
    }
    text
}

/// `unit(0)` to `unit(n - 1)`.
fn count(n: usize, unit: impl Fn(usize) -> String) -> String {
    (0..n).map(unit).collect()
}

/// A file as the reader gets it: within `MAX_BYTES`, else a row would test the size check.
fn file(text: String) -> Vec<u8> {
    assert!(text.len() <= MAX_BYTES, "{} bytes", text.len());
    text.into_bytes()
}

/// The size of the summary the manifest at `path` reads as.
fn manifest(path: &str, text: String) -> usize {
    let summary = read_manifest(path, &file(text));
    serde_json::to_string(&summary).unwrap().len()
}

/// The size of what the Spring configuration `name` (`application.yml`) reads as.
fn configured(name: &str, text: String) -> usize {
    let path = format!("src/main/resources/{name}");
    let configs = spring::Configs::read(&[(&path, &file(text))], &[]);
    let found = configs.of("src/main/java/A.java");
    (found.into_iter().flatten())
        .map(|(k, v)| k.len() + v.len())
        .sum()
}

/// The size of a project as MSBuild files read it, in the bytes of its names.
fn project_size(project: Option<msbuild::Project>) -> usize {
    let project = project.unwrap_or_default();
    let usings = (project.usings.iter()).map(|u| match u {
        msbuild::Using::Namespace(n) | msbuild::Using::Static(n) => n.len(),
        msbuild::Using::Alias(a, n) => a.len() + n.len(),
    });
    let references = (project.packages.iter().chain(&project.projects))
        .map(|r| r.include.len() + r.version.as_ref().map_or(0, String::len));
    usings.chain(references).sum()
}

/// How many documents and scalars the event reader gives for `text` read as `syntax`, every
/// document and every scalar with its position; a stream it cannot read gives the scalars handed
/// out before it stopped.
fn stream(syntax: Syntax, text: String) -> usize {
    let mut handed = 0;
    let documents = events::read(syntax, &file(text), |_| handed += 1);
    handed + documents.map_or(0, |d| d.len())
}

/// The bytes of the key paths and values config values store for `text` at `path`.
fn stored(path: &str, text: String) -> usize {
    let (_, rows) = config_values::read(path, &file(text));
    (rows.iter())
        .map(|r| r.key.len() + r.value.as_ref().map_or(0, String::len))
        .sum()
}

/// The bytes of the addresses and names the IaC nodes and references of `text`, a lone
/// `main.tf`, store.
fn nodes(text: String) -> usize {
    tree(vec![("main.tf".into(), text)])
}

/// The bytes of the addresses and names the IaC nodes and references of the Terraform files
/// `files` (path and text) store.
fn tree(files: Vec<(String, String)>) -> usize {
    let files: Vec<(String, Vec<u8>)> = (files.into_iter())
        .map(|(path, text)| (path, file(text)))
        .collect();
    let (nodes, references) = crate::iac::evaluate(&files);
    let nodes = (nodes.iter()).map(|n| n.address.len() + n.name.as_ref().map_or(0, String::len));
    let references = (references.iter())
        .map(|r| r.source.len() + r.target.len() + r.via.as_ref().map_or(0, String::len));
    nodes.chain(references).sum()
}

/// The bytes of the addresses the IaC nodes and references of `text`, a lone `compose.yaml`,
/// store.
fn compose(text: String) -> usize {
    let (nodes, references) = crate::iac::evaluate(&[("compose.yaml".into(), file(text))]);
    let nodes = nodes.iter().map(|n| n.address.len());
    let references = (references.iter())
        .map(|r| r.source.len() + r.target.len() + r.via.as_ref().map_or(0, String::len));
    nodes.chain(references).sum()
}

/// The bytes of the addresses, names and references the IaC nodes of `text`, a lone
/// `template.yaml` of CloudFormation (#653), store.
fn template(text: String) -> usize {
    let head = "AWSTemplateFormatVersion: x\n";
    let (nodes, references) =
        crate::iac::evaluate(&[("template.yaml".into(), file(format!("{head}{text}")))]);
    let nodes = (nodes.iter()).map(|n| n.address.len() + n.name.as_ref().map_or(0, String::len));
    let references = (references.iter())
        .map(|r| r.source.len() + r.target.len() + r.via.as_ref().map_or(0, String::len));
    nodes.chain(references).sum()
}

/// The bytes of the addresses, references, images and unresolved entries `kube::evaluate` stores
/// for the files `files` (path and text) of one repository.
fn objects(files: Vec<(&str, String)>) -> usize {
    let files: Vec<(String, Vec<u8>)> = (files.into_iter())
        .map(|(path, text)| (path.to_string(), file(text)))
        .collect();
    let out = crate::kube::evaluate(&files);
    let nodes = out.nodes.iter().map(|n| n.address.len());
    let references = (out.references.iter())
        .map(|r| r.source.len() + r.target.len() + r.via.as_ref().map_or(0, String::len));
    let images = out.images.iter().map(|i| i.address.len() + i.image.len());
    let unresolved = out.unresolved.iter().map(|u| u.entry.len());
    nodes
        .chain(references)
        .chain(images)
        .chain(unresolved)
        .sum()
}

/// A kustomization file naming `entries`.
fn kustomization(entries: impl Iterator<Item = String>) -> String {
    entries
        .map(|e| format!("  - {e}\n"))
        .fold("resources:\n".into(), |a, e| a + &e)
}

/// An SDK-style project file with `body` in it.
fn project(body: &str) -> String {
    format!("<Project Sdk=\"Microsoft.NET.Sdk\">{body}</Project>")
}

/// How many imports a row resolves from the files of one project: `entries` is asked again for
/// each, so a walk of the project's references per import costs the imports times the walk.
const IMPORTS: usize = 1000;

/// The size of the entries `Nuget.entries` gives a file of the project `App`, asked for each of
/// `IMPORTS` files of it, in a repository of the projects `projects` (`(name, project file)`).
fn referenced(projects: Vec<(String, String)>) -> usize {
    let manifests = (projects.into_iter())
        .map(|(name, text)| {
            let path = format!("{name}/{name}.csproj");
            Manifest {
                summary: read_manifest(&path, &file(text)),
                path,
            }
        })
        .collect();
    let exports = [Export {
        manifests,
        ..Export::default()
    }];
    let providers = Providers::new(&exports);
    let files: Vec<String> = (0..IMPORTS).map(|i| format!("App/A{i}.cs")).collect();
    let sizes = (files.iter()).map(|file| {
        let entries = Nuget.entries(&exports, &providers, (0, file), "c");
        entries.iter().map(|(e, _)| e.name.len()).sum::<usize>()
    });
    sizes.max().unwrap_or(0)
}

/// The size of the entries `Maven.entries` gives a file of the module `app`, asked for each of
/// `IMPORTS` files of it, in a repository of the modules `poms` (`(name, pom.xml)`).
fn modules(poms: Vec<(String, String)>) -> usize {
    let manifests = (poms.into_iter())
        .map(|(name, text)| {
            let path = format!("{name}/pom.xml");
            Manifest {
                summary: read_manifest(&path, &file(text)),
                path,
            }
        })
        .collect();
    let exports = [Export {
        manifests,
        ..Export::default()
    }];
    let providers = Providers::new(&exports);
    let files: Vec<String> = (0..IMPORTS)
        .map(|i| format!("app/src/main/java/A{i}.java"))
        .collect();
    let sizes = (files.iter()).map(|file| {
        let entries = Maven.entries(&exports, &providers, (0, file), "c");
        entries.iter().map(|(e, _)| e.name.len()).sum::<usize>()
    });
    sizes.max().unwrap_or(0)
}

fn dependency(artifact: &str) -> String {
    let (group, version) = ("<groupId>g</groupId>", "<version>${v}</version>");
    format!("<dependency>{group}<artifactId>{artifact}</artifactId>{version}</dependency>")
}

fn pom(artifact: &str, body: &str) -> String {
    format!("<project><groupId>g</groupId><artifactId>{artifact}</artifactId>{body}</project>")
}

/// A probe: its name, and a row that reads one file built to explode and answers the size of what
/// the reader returned.
type Probe = (&'static str, fn() -> usize);

fn probes() -> Vec<Probe> {
    vec![
        // npm: `package.json`
        ("npm: many dependencies", || {
            let deps = fill(1, |i| format!("\"p{i}\":\"^1.0.{i}\","));
            let text = format!("{{\"dependencies\":{{{deps}\"z\":\"1\"}}}}");
            manifest("package.json", text)
        }),
        ("npm: many exports", || {
            let exports = fill(1, |i| {
                format!("\"./e{i}\":{{\"import\":{{\"types\":\"./t\",\"default\":\"./d\"}}}},")
            });
            manifest(
                "package.json",
                format!("{{\"exports\":{{{exports}\"z\":1}}}}"),
            )
        }),
        ("npm: deep exports", || {
            let nest = "{\"a\":".repeat(NESTING) + "1" + &"}".repeat(NESTING);
            manifest("package.json", format!("{{\"exports\":{nest}}}"))
        }),
        // tsconfig.json
        ("tsconfig: many options and comments", || {
            let options = fill(1, |i| {
                format!("/* c{i} */ \"o{i}\": {{\"a\": [1, 2,],}},\n")
            });
            let text = format!("{{\"compilerOptions\":{{{options}\"outDir\":\"d\"}}}}");
            format!("{:?}", super::read_tsconfig(&file(text))).len()
        }),
        // PyPI: `pyproject.toml`, `requirements.txt`
        ("pyproject: many dependencies", || {
            let deps = fill(1, |i| format!("\"dep{i} >=1.{i}; python_version>'3'\",\n"));
            let text = format!("[project]\nname=\"p\"\ndependencies=[{deps}]\n");
            manifest("pyproject.toml", text)
        }),
        ("pyproject: many poetry dependencies and scripts", || {
            let deps = fill(2, |i| format!("dep{i} = \"^1.{i}\"\n"));
            let scripts = fill(4, |i| format!("s{i} = \"m{i}:main\"\n"));
            let text = format!(
                "[tool.poetry]\nname=\"p\"\n[tool.poetry.dependencies]\n{deps}[project.scripts]\n{scripts}"
            );
            manifest("pyproject.toml", text)
        }),
        ("pyproject: many dependencies and uv sources", || {
            let deps = fill(2, |i| format!("\"d{i}\",\n"));
            let sources = fill(2, |i| format!("s{i} = {{ path = \"s\" }}\n"));
            let text = format!(
                "[project]\nname=\"p\"\ndependencies=[{deps}]\n[tool.uv.sources]\n{sources}"
            );
            manifest("pyproject.toml", text)
        }),
        ("pyproject: deep arrays", || {
            let nest = "[".repeat(NESTING) + &"]".repeat(NESTING);
            manifest("pyproject.toml", format!("[project]\nx={nest}\n"))
        }),
        ("requirements: many lines", || {
            manifest(
                "requirements.txt",
                fill(1, |i| format!("dep{i}==1.{i} # c\n")),
            )
        }),
        ("requirements: one continued line", || {
            manifest("requirements.txt", fill(1, |_| "dep>=1,\\\n".into()))
        }),
        // Cargo: `Cargo.toml`
        ("cargo: many dependencies", || {
            let deps = fill(1, |i| {
                format!("dep{i} = {{ version = \"1\", features = [\"a\"] }}\n")
            });
            manifest(
                "Cargo.toml",
                format!("[package]\nname=\"p\"\n[dependencies]\n{deps}"),
            )
        }),
        (
            "cargo: many target tables and workspace dependencies",
            || {
                let targets = fill(2, |i| {
                    format!("[target.'cfg(x{i})'.dependencies]\nt{i} = \"1\"\n")
                });
                let work = fill(4, |i| format!("w{i} = {{ path = \"../w{i}\" }}\n"));
                let text =
                    format!("[package]\nname=\"p\"\n{targets}[workspace.dependencies]\n{work}");
                manifest("Cargo.toml", text)
            },
        ),
        ("cargo: deep arrays", || {
            let nest = "[".repeat(NESTING) + &"]".repeat(NESTING);
            manifest("Cargo.toml", format!("[package]\nname=\"p\"\nx={nest}\n"))
        }),
        // Go: `go.mod`
        ("go.mod: many requires and replaces", || {
            let requires = fill(2, |i| format!("\texample.com/m{i} v1.{i}.0\n"));
            let replaces = fill(4, |i| format!("\texample.com/m{i} => ../r{i}\n"));
            let text =
                format!("module example.com/h\nrequire (\n{requires})\nreplace (\n{replaces})\n");
            manifest("go.mod", text)
        }),
        ("go.mod: unclosed blocks", || {
            manifest(
                "go.mod",
                fill(1, |i| format!("require (\nexample.com/m{i} v1.0.0\n")),
            )
        }),
        // Maven: `pom.xml`
        ("pom: many dependencies", || {
            let deps = fill(1, |i| dependency(&format!("a{i}")));
            manifest(
                "pom.xml",
                pom("app", &format!("<dependencies>{deps}</dependencies>")),
            )
        }),
        ("pom: dependencies up to the node limit", || {
            let deps = count(9000, |i| dependency(&format!("a{i}")));
            manifest(
                "pom.xml",
                pom("app", &format!("<dependencies>{deps}</dependencies>")),
            )
        }),
        ("pom: one dependency of many elements", || {
            let more = count(30000, |i| format!("<x{i}>1</x{i}>"));
            let dep = format!(
                "<dependency><groupId>g</groupId><artifactId>a</artifactId>{more}</dependency>"
            );
            manifest(
                "pom.xml",
                pom("app", &format!("<dependencies>{dep}</dependencies>")),
            )
        }),
        ("pom: deep elements", || {
            let nest = "<a>".repeat(NESTING) + &"</a>".repeat(NESTING);
            manifest("pom.xml", pom("app", &nest))
        }),
        ("pom: modules passing on to each other", || {
            // `app` depends on thousands of modules, the first two hundred of which exist and
            // depend on each other.
            let deps = count(9000, |i| dependency(&format!("m{i}")));
            let mut poms = vec![(
                "app".to_string(),
                pom("app", &format!("<dependencies>{deps}</dependencies>")),
            )];
            let some = count(200, |i| dependency(&format!("m{}", (i * 7) % 200)));
            for i in 0..200 {
                let text = pom(
                    &format!("m{i}"),
                    &format!("<dependencies>{some}</dependencies>"),
                );
                poms.push((format!("m{i}"), text));
            }
            modules(poms)
        }),
        // Gradle: `build.gradle`, `settings.gradle`
        ("gradle: many dependencies", || {
            let deps = fill(1, |i| format!("  implementation 'g:a{i}:1.{i}'\n"));
            manifest("build.gradle", format!("dependencies {{\n{deps}}}\n"))
        }),
        ("gradle: deep blocks", || {
            manifest("build.gradle.kts", "a {\n".repeat(NESTING))
        }),
        ("gradle: one statement of many arguments", || {
            let includes = fill(1, |i| format!("'a{i}', "));
            manifest("settings.gradle", format!("include({includes})\n"))
        }),
        ("gradle: one unclosed statement", || {
            manifest(
                "build.gradle",
                format!("foo(\n{}", fill(1, |i| format!("'a{i}' + \n"))),
            )
        }),
        ("gradle: many properties", || {
            let lines = fill(1, |i| format!("p{i}=v{i}\n"));
            manifest("gradle.properties", lines)
        }),
        ("gradle: one property of a long value", || {
            manifest(
                "gradle.properties",
                format!("a={}\n", "v".repeat(MAX_BYTES - 16)),
            )
        }),
        ("gradle: catalog many libraries", || {
            let libs = fill(1, |i| {
                format!("a{i} = {{ module = \"g:a{i}\", version.ref = \"v{i}\" }}\n")
            });
            manifest("gradle/libs.versions.toml", format!("[libraries]\n{libs}"))
        }),
        ("gradle: catalog many versions", || {
            let versions = fill(1, |i| format!("v{i} = \"1.{i}\"\n"));
            let library = "a = { module = \"g:a\", version.ref = \"v0\" }\n";
            manifest(
                "libs.versions.toml",
                format!("[versions]\n{versions}[libraries]\n{library}"),
            )
        }),
        ("gradle: catalog deep arrays", || {
            let nest = format!("{}{}", "[".repeat(NESTING), "]".repeat(NESTING));
            manifest(
                "gradle/libs.versions.toml",
                format!("[libraries]\nx={nest}\n"),
            )
        }),
        (
            "gradle: many catalog accessors and one property web",
            || {
                let deps = fill(1, |i| {
                    format!("  implementation libs.a{i}\n  api \"g:${{p{i}}}\"\n")
                });
                manifest("build.gradle", format!("dependencies {{\n{deps}}}\n"))
            },
        ),
        // NuGet: `*.csproj`, `Directory.Build.props`
        ("csproj: many references", || {
            let refs = fill(1, |i| {
                format!(
                    "<PackageReference Include=\"P{i}\" Version=\"1\"/><ProjectReference Include=\"..\\P{i}\\P{i}.csproj\"/>"
                )
            });
            manifest(
                "src/App/App.csproj",
                project(&format!("<ItemGroup>{refs}</ItemGroup>")),
            )
        }),
        ("csproj: many elements", || {
            manifest(
                "src/App/App.csproj",
                project(&fill(1, |i| format!("<A{i}/>"))),
            )
        }),
        ("csproj: elements up to the node limit", || {
            manifest(
                "src/App/App.csproj",
                project(&count(60000, |i| format!("<A{i}/>"))),
            )
        }),
        ("props: many references", || {
            let refs = fill(1, |i| {
                format!("<PackageReference Include=\"P{i}\" Version=\"1\"/>")
            });
            let text = format!("<Project><ItemGroup>{refs}</ItemGroup></Project>");
            manifest("Directory.Build.props", text)
        }),
        (
            "gradle: properties and a catalog filling many builds",
            || {
                // Each build asks the one catalog and `gradle.properties` for every dependency.
                let properties = fill(2, |i| format!("p{i}=v{i}\n"));
                let libraries = fill(2, |i| format!("a{i} = \"g:a{i}:1\"\n"));
                let build = count(100, |i| {
                    format!("  implementation libs.a{i}\n  api \"g:${{p{i}}}\"\n")
                });
                let mut manifests = vec![
                    ("gradle.properties".to_string(), properties),
                    (
                        "gradle/libs.versions.toml".to_string(),
                        format!("[libraries]\n{libraries}"),
                    ),
                ];
                for i in 0..PROJECTS {
                    manifests.push((
                        format!("m{i}/build.gradle"),
                        format!("dependencies {{\n{build}}}\n"),
                    ));
                }
                let mut manifests: Vec<_> = (manifests.into_iter())
                    .map(|(path, text)| Manifest {
                        summary: read_manifest(&path, &file(text)),
                        path,
                    })
                    .collect();
                super::inherit(&mut manifests);
                (manifests.iter().skip(2))
                    .map(|m| serde_json::to_string(&m.summary).unwrap().len())
                    .sum()
            },
        ),
        ("nuget: a props file inherited by many projects", || {
            // Each project takes the props file's references: the size is all of theirs.
            let refs = fill(1, |i| format!("<PackageReference Include=\"P{i}\"/>"));
            let props = format!("<Project><ItemGroup>{refs}</ItemGroup></Project>");
            let path = "Directory.Build.props".to_string();
            let mut manifests = vec![Manifest {
                summary: read_manifest(&path, props.as_bytes()),
                path,
            }];
            for i in 0..PROJECTS {
                let path = format!("src/P{i}/P{i}.csproj");
                let summary = read_manifest(&path, project("").as_bytes());
                manifests.push(Manifest { summary, path });
            }
            super::inherit(&mut manifests);
            (manifests.iter().skip(1))
                .map(|m| serde_json::to_string(&m.summary).unwrap().len())
                .sum()
        }),
        ("nuget: projects passing on to each other", || {
            // `App` references thousands of projects, the first two hundred of which exist and
            // reference each other.
            let refs = |n: usize, to: &dyn Fn(usize) -> usize| {
                count(n, |i| {
                    format!(
                        "<ProjectReference Include=\"..\\P{0}\\P{0}.csproj\"/>",
                        to(i)
                    )
                })
            };
            let app = project(&format!("<ItemGroup>{}</ItemGroup>", refs(9000, &|i| i)));
            let mut projects = vec![("App".to_string(), app)];
            let some = refs(200, &|i| (i * 7) % 200);
            for i in 0..200 {
                projects.push((
                    format!("P{i}"),
                    project(&format!(
                        "<ItemGroup>{some}<PackageReference Include=\"K{i}\"/></ItemGroup>"
                    )),
                ));
            }
            referenced(projects)
        }),
        // MSBuild: `msbuild::read`, `msbuild::props`
        ("msbuild: usings added and removed in one file", || {
            let added = fill(2, |i| format!("N{i};"));
            let removed = fill(2, |i| format!("N{};", 2 * i));
            let text = project(&format!(
                "<ItemGroup><Using Include=\"{added}\"/><Using Remove=\"{removed}\"/></ItemGroup>"
            ));
            project_size(msbuild::read(&file(text), &[]))
        }),
        ("msbuild: a chain of props files of many usings", || {
            // The most props files `chain` follows, each of the most usings one adds, and a
            // project removing thousands: the average is per file.
            let names = count(msbuild::PROPS_ITEMS, |i| format!("N{i};"));
            let added =
                format!("<Project><ItemGroup><Using Include=\"{names}\"/></ItemGroup></Project>");
            let added = msbuild::props(msbuild::PROPS, &file(added)).unwrap();
            let names = fill(2, |i| format!("N{i};"));
            let removed = project(&format!(
                "<ItemGroup><Using Remove=\"{names}\"/></ItemGroup>"
            ));
            let layers = vec![&added; msbuild::PROPS_DEPTH];
            project_size(msbuild::read(&file(removed), &layers)) / msbuild::PROPS_DEPTH
        }),
        ("msbuild: a props file of many imports", || {
            let imports = fill(1, |_| {
                "<Import Project=\"$(MSBuildThisFileDirectory)../Directory.Build.props\"/>".into()
            });
            let read = msbuild::props(
                "a/b/Directory.Build.props",
                &file(format!("<Project>{imports}</Project>")),
            );
            project_size(read.map(|p| p.project))
        }),
        (
            "msbuild: a props file of many elements over many projects",
            || {
                // The C# pack's `join` reads every project under the props files above it: a props
                // file read again for each project costs its bytes times the projects, past `BUDGET`
                // at a thousand.
                let elements = count(60000, |i| format!("<A{i}/>"));
                let props = file(format!(
                    "<Project><PropertyGroup>{elements}</PropertyGroup></Project>"
                ));
                let csproj = project("<ItemGroup><Using Include=\"N\"/></ItemGroup>");
                let paths: Vec<String> = (0..5 * PROJECTS)
                    .map(|i| format!("P{i}/P{i}.csproj"))
                    .collect();
                let mut configs: Vec<(&str, &[u8])> = vec![("Directory.Build.props", &props)];
                configs.extend(paths.iter().map(|p| (p.as_str(), csproj.as_bytes())));
                let join = CSharp.join(&configs, &BTreeMap::new());
                (paths.iter())
                    .map(|p| {
                        let mut facts = Facts::default();
                        join(&p.replace(".csproj", ".cs"), &mut facts);
                        facts.imports.len()
                    })
                    .sum()
            },
        ),
        // Backstage: `app-config.yaml`, its proxy table (#616)
        ("app-config: many proxy endpoints and rewrite rules", || {
            // A quarter of the file stays under `YAML_KEY_BUDGET`, so the endpoints are read.
            let endpoints = fill(4, |i| {
                format!(
                    "    '/e{i}':\n      target: https://h{i}\n      pathRewrite:\n        '^/api/proxy/e{i}': ''\n        '^/r{i}': '/'\n"
                )
            });
            crate::http::app_config_probe(file(format!("proxy:\n  endpoints:\n{endpoints}")))
        }),
        // Spring: `application.properties`, `application.yml`
        ("spring: many properties", || {
            configured(
                "application.properties",
                fill(1, |i| format!("k{i}.a.b=v{i}\n")),
            )
        }),
        ("spring: continued lines", || {
            configured("application.properties", fill(1, |_| "k=a\\\n".into()))
        }),
        ("spring: placeholders doubling each other", || {
            // 40 keys, each two of the next: 2^40 copies of the last, were each read.
            let keys = count(40, |i| format!("k{i}=${{k{0}}}${{k{0}}}\n", i + 1));
            let text = keys + "k40=" + &"x".repeat(1000) + "\n";
            let path = "src/main/resources/application.properties";
            let configs = spring::Configs::read(&[(path, &file(text))], &[]);
            let parts = spring::resolve("${k0}", configs.of("src/main/java/A.java"));
            format!("{parts:?}").len()
        }),
        ("spring: many yaml keys", || {
            configured(
                "application.yml",
                fill(1, |i| format!("k{i}:\n  a: v{i}\n  b:\n    c: {i}\n")),
            )
        }),
        ("spring: deep yaml", || {
            // One space more for each level, as deep as the file allows.
            configured(
                "application.yml",
                fill(1, |i| format!("{}a:\n", " ".repeat(i))),
            )
        }),
        ("spring: a long yaml key over many children", || {
            // Joined with each child's, the keys would weigh gigabytes (`YAML_KEY_BUDGET`).
            let children = count(3000, |n| format!("  c{n}: v\n"));
            configured(
                "application.yml",
                format!("? {}\n:\n{children}", "k".repeat(200_000)),
            )
        }),
        ("spring: yaml aliases expanding", || {
            // Nine levels of ten aliases each: 10^9 nodes from a few hundred bytes.
            let first = "a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n".to_string();
            let levels = count(9, |n| {
                format!(
                    "a{0}: &a{0} [{1}]\n",
                    n + 1,
                    vec![format!("*a{n}"); 10].join(", ")
                )
            });
            configured("application.yml", first + &levels)
        }),
        // YAML and JSON: the event reader (`document::events`), every document and scalar
        ("events: many documents", || {
            stream(
                Syntax::Yaml,
                fill(1, |i| format!("--- {{k{i}: [a, &a{i} b, *a{i}]}}\n")),
            )
        }),
        ("events: deep and wide", || {
            // At the nesting limit, every scalar is handed out at the full depth.
            let wide = fill(1, |_| "1, ".into());
            stream(Syntax::Yaml, format!("{}[{wide}1]\n", "- ".repeat(127)))
        }),
        ("events: aliases fanning out", || {
            let anchor = format!("a: &a [{}]\n", vec!["x"; 1000].join(", "));
            stream(Syntax::Yaml, anchor + &fill(1, |i| format!("b{i}: *a\n")))
        }),
        ("events: aliases one level deeper each", || {
            // Each anchor holds the one before: the nesting limit stops it, not the stack.
            let first = "a0: &a0 [x]\n".to_string();
            stream(
                Syntax::Yaml,
                first + &fill(1, |i| format!("a{}: &a{0} [*a{i}]\n", i + 1)),
            )
        }),
        ("events: deep and wide json", || {
            let wide = fill(1, |_| "1,".into());
            let nest = |c: &str| c.repeat(100);
            stream(Syntax::Json, format!("{}{wide}1{}", nest("["), nest("]")))
        }),
        ("events: json nested past serde_json's limit", || {
            let nest = |c: &str| c.repeat(NESTING);
            stream(Syntax::Json, format!("{}1{}", nest("["), nest("]")))
        }),
        // Config values (#639): the rows a YAML or JSON file stores
        ("config values: long keys nested deep", || {
            let key = "k".repeat(1000);
            let deep = format!("{{\"{key}\": ").repeat(100);
            let leaves = fill(2, |i| format!("\"v{i}\": 1, "));
            let text = format!("{deep}{{{leaves}\"z\": 1}}{}", "}".repeat(100));
            stored("a.json", text)
        }),
        ("config values: many scalars", || {
            stored("a.yaml", fill(1, |i| format!("- {}\n", i % 10)))
        }),
        ("config values: secrets in many documents", || {
            let secret = "---\napiVersion: v1\nkind: Secret\ndata: {a: b, c: d}\n";
            stored("a.yaml", count(999, |_| secret.to_string()))
        }),
        ("hcl: long labels nested deep", || {
            let label = "k".repeat(4000);
            let open = format!("b \"{label}\" {{\n").repeat(50);
            let leaves = fill(2, |i| format!("v{i} = 1\n"));
            stored("a.tf", format!("{open}{leaves}{}", "}\n".repeat(50)))
        }),
        ("hcl: many attributes", || {
            stored("a.tf", fill(1, |i| format!("a{i} = {}\n", i % 10)))
        }),
        ("hcl: many blocks of one type", || {
            stored("a.tf", fill(1, |i| format!("b {{ a = {} }}\n", i % 10)))
        }),
        ("hcl: deep brackets", || {
            stored("a.tf", format!("x = {}", "[".repeat(NESTING)))
        }),
        ("hcl: long heredoc", || {
            let line = format!("{}\n", "a".repeat(79));
            stored(
                "a.tf",
                format!("x = <<EOT\n{}EOT\n", fill(1, |_| line.clone())),
            )
        }),
        ("hcl: heredoc of interpolations", || {
            let body = fill(1, |i| format!("${{var.a{i}}}\n"));
            stored("a.tf", format!("x = <<EOT\n{body}EOT\n"))
        }),
        ("hcl: long expression", || {
            stored(
                "a.tf",
                format!("x = f({}0)\n", fill(1, |i| format!("a{i}, "))),
            )
        }),
        ("hcl: many tfvars", || {
            stored("a.tfvars", fill(1, |i| format!("v{i} = \"x\"\n")))
        }),
        // IaC nodes (#649): what the evaluation of one root module stores
        ("iac: a chain of resources", || {
            nodes(fill(1, |i| {
                format!("resource \"a\" \"r{}\" {{ x = a.r{i}.id }}\n", i + 1)
            }))
        }),
        ("iac: locals naming each other twice", || {
            let locals = fill(2, |i| format!("  l{} = [local.l{i}, local.l{i}]\n", i + 1));
            let users = fill(2, |i| {
                format!("resource \"b\" \"u{i}\" {{ x = local.l{i} }}\n")
            });
            nodes(format!(
                "resource \"a\" \"t\" {{}}\nlocals {{\n  l0 = a.t.id\n{locals}}}\n{users}"
            ))
        }),
        ("iac: every resource naming a wide local", || {
            let all = count(5000, |i| format!("a.r{i}, "));
            let users = fill(2, |i| {
                format!("resource \"a\" \"r{i}\" {{ x = local.all }}\n")
            });
            nodes(format!("locals {{\n  all = [{all}]\n}}\n{users}"))
        }),
        ("iac: a module calling itself", || {
            nodes(fill(1, |i| {
                format!("module \"m{i}\" {{ source = \"./\" }}\n")
            }))
        }),
        // Static evaluation (#650): what the names and instances one root evaluates store
        (
            "iac eval: a list doubled by locals under a for_each",
            || {
                let locals = count(40, |i| {
                    format!("  l{} = concat(local.l{i}, local.l{i})\n", i + 1)
                });
                let users = fill(2, |i| {
                    format!(
                        "resource \"a\" \"r{i}\" {{\n  for_each = toset(local.l{})\n  name = \"${{each.key}}-${{local.s40}}\"\n}}\n",
                        i % 41
                    )
                });
                let strings = count(40, |i| {
                    format!("  s{} = \"${{local.s{i}}}${{local.s{i}}}\"\n", i + 1)
                });
                nodes(format!(
                    "locals {{\n  l0 = [\"a\", \"b\"]\n  s0 = \"ab\"\n{locals}{strings}}}\n{users}"
                ))
            },
        ),
        ("iac eval: a for in a for in a for over a wide list", || {
            let items = count(1000, |i| format!("\"k{i}\", "));
            let users = fill(2, |i| {
                format!(
                    "resource \"a\" \"r{i}\" {{ name = length([for a in local.l : [for b in local.l : [for c in local.l : 1]]]) }}\n"
                )
            });
            nodes(format!("locals {{\n  l = [{items}]\n}}\n{users}"))
        }),
        ("iac eval: names through cyclic locals", || {
            let n = 2000;
            let locals = count(n, |i| {
                format!("  a{i} = \"${{local.a{}}}-x\"\n", (i + 1) % n)
            });
            let users = fill(2, |i| {
                format!("resource \"a\" \"r{i}\" {{ name = local.a{} }}\n", i % n)
            });
            nodes(format!("locals {{\n{locals}}}\n{users}"))
        }),
        (
            "iac eval: modules nested past the ceiling, each passing a name down",
            || {
                let depth = 20;
                let files = (0..depth)
                .map(|level| {
                    let calls = count(2, |c| {
                        format!(
                            "module \"m{c}\" {{\n  source = \"../l{}\"\n  name = \"${{var.name}}-{c}\"\n}}\n",
                            level + 1
                        )
                    });
                    let resources = fill(depth, |i| {
                        format!("resource \"a\" \"r{i}\" {{ name = \"${{var.name}}-{i}\" }}\n")
                    });
                    let text = format!("variable \"name\" {{\n  default = \"q\"\n}}\n{calls}{resources}");
                    (format!("l{level}/main.tf"), text)
                })
                .collect();
                tree(files)
            },
        ),
        ("iac: wide counts", || {
            nodes(fill(1, |i| {
                format!("resource \"a\" \"r{i}\" {{ count = 1000 }}\n")
            }))
        }),
        ("compose: many services on one network", || {
            let services = fill(1, |i| format!("  s{i}:\n    networks: [n]\n"));
            compose(format!("services:\n{services}"))
        }),
        ("compose: a default naming a default", || {
            let nest = "${A:-".repeat(5000) + "x" + &"}".repeat(5000);
            compose(format!("services:\n  a:\n    image: {nest}\n"))
        }),
        ("compose: an alias per service", || {
            let services = fill(1, |i| format!("  s{i}: *base\n"));
            compose(format!(
                "x: &base {{image: a, depends_on: [s0]}}\nservices:\n{services}"
            ))
        }),
        // CloudFormation and SAM (#653): what one template's evaluation stores
        (
            "cloudformation: tagged intrinsics nested to the limit",
            || {
                let nest = "!Join ['', [".repeat(60) + "x" + &"]]".repeat(60);
                let resources = fill(1, |i| {
                    format!(
                        "  R{i}:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: {nest}\n"
                    )
                });
                template(format!("Resources:\n{resources}"))
            },
        ),
        ("cloudformation: every resource depends on many", || {
            let deps = (0..100)
                .map(|i| format!("R{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            let resources = fill(4, |i| {
                format!("  R{i}:\n    Type: AWS::SQS::Queue\n    DependsOn: [{deps}]\n")
            });
            template(format!("Resources:\n{resources}"))
        }),
        ("cloudformation: names built from a long default", || {
            let long = "n".repeat(900);
            let resources = fill(4, |i| {
                format!(
                    "  R{i}:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: !Sub '${{P}}${{P}}${{P}}'\n"
                )
            });
            template(format!(
                "Parameters:\n  P:\n    Default: {long}\nResources:\n{resources}"
            ))
        }),
        (
            "cloudformation: a chain of resources naming the last",
            || {
                let resources = fill(1, |i| {
                    format!(
                        "  R{i:06}:\n    Type: AWS::S3::Bucket\n    Properties:\n      BucketName: !Sub '${{R{:06}}}-x'\n",
                        i + 1
                    )
                });
                template(format!("Resources:\n{resources}"))
            },
        ),
        ("cloudformation: an alias per resource", || {
            let resources = fill(4, |i| format!("  R{i}: *base\n"));
            template(format!(
                "x: &base {{Type: AWS::SQS::Queue, DependsOn: R0}}\nResources:\n{resources}"
            ))
        }),
        (
            "cloudformation: an aliased section of many resources",
            || {
                let resources = fill(4, |i| {
                    format!(
                        "  R{i}:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: q\n"
                    )
                });
                template(format!("x: &r\n{resources}Resources: *r\n"))
            },
        ),
        (
            "cloudformation: an aliased section of one-line resources",
            || {
                let resources = fill(2, |i| format!("  R{i}: {{Type: AWS::SQS::Queue}}\n"));
                template(format!("x: &r\n{resources}Resources: *r\n"))
            },
        ),
        ("cloudformation: flat JSON of many resources", || {
            let resources = fill(4, |i| {
                format!("\"R{i}\": {{\n\"Type\": \"AWS::SQS::Queue\"\n}},\n")
            });
            template(format!(
                "{{\n\"Resources\": {{\n{resources}\"Z\": {{}}\n}}\n}}\n"
            ))
        }),
        ("compose: flat JSON of many services", || {
            let services = fill(4, |i| format!("\"s{i}\": {{\n\"image\": \"a\"\n}},\n"));
            compose(format!(
                "{{\n\"services\": {{\n{services}\"z\": {{}}\n}}\n}}\n"
            ))
        }),
        ("compose: a long blank line above an alias", || {
            let services = fill(4, |i| format!("  s{i}: {{image: a}}\n"));
            let blanks = " ".repeat(MAX_BYTES / 2);
            compose(format!("x: &all\n{services}{blanks}\nservices: *all\n"))
        }),
        ("compose: an aliased section of many services", || {
            let services = fill(4, |i| format!("  s{i}:\n    image: a\n"));
            compose(format!("x: &all\n{services}services: *all\n"))
        }),
        (
            "cloudformation: tagged aliases that expand past the budget",
            || {
                let mut text = String::from("x0: &l0 !Ref [x, x, x, x, x, x, x, x, x, x]\n");
                for n in 1..=9 {
                    let p = n - 1;
                    text += &format!(
                        "x{n}: &l{n} !Ref [{}]\n",
                        vec![format!("*l{p}"); 10].join(", ")
                    );
                }
                template(format!("{text}Resources: {{}}\n"))
            },
        ),
        // Kubernetes and Kustomize (#651): what one repository's evaluation stores
        ("kustomize: a chain of bases", || {
            let mut files: Vec<(String, String)> = Vec::new();
            for i in 0..4000 {
                let (dir, next) = (format!("d{i}"), format!("../d{}", i + 1));
                files.push((
                    format!("{dir}/kustomization.yaml"),
                    kustomization([next].into_iter()),
                ));
            }
            let files = files.iter().map(|(p, t)| (p.as_str(), t.clone())).collect();
            objects(files)
        }),
        ("kustomize: bases naming each other", || {
            let mut files: Vec<(String, String)> = Vec::new();
            for i in 0..4000 {
                let next = (1..=3).map(|k| format!("../d{}", (i + k) % 4000));
                files.push((format!("d{i}/kustomization.yaml"), kustomization(next)));
            }
            files.push((
                "root/kustomization.yaml".into(),
                kustomization(["../d0".to_string()].into_iter()),
            ));
            let files = files.iter().map(|(p, t)| (p.as_str(), t.clone())).collect();
            objects(files)
        }),
        ("kustomize: a base named many times", || {
            let base = count(900, |i| {
                format!("---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: c{i}\n")
            });
            let names = fill(2, |_| "  - ../base\n".to_string());
            objects(vec![
                (
                    "base/kustomization.yaml",
                    "resources:\n  - all.yaml\n".into(),
                ),
                ("base/all.yaml", base),
                ("o/kustomization.yaml", format!("resources:\n{names}")),
            ])
        }),
        ("kustomize: many overlays over one base", || {
            let base = count(900, |i| {
                format!("---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: c{i}\n")
            });
            let mut files = vec![
                (
                    "base/kustomization.yaml",
                    "resources:\n  - all.yaml\n".to_string(),
                ),
                ("base/all.yaml", base),
            ];
            let overlays: Vec<(String, String)> = (0..300)
                .map(|i| {
                    (
                        format!("o{i}/kustomization.yaml"),
                        "resources:\n  - ../base\n".to_string(),
                    )
                })
                .collect();
            files.extend(overlays.iter().map(|(p, t)| (p.as_str(), t.clone())));
            objects(files)
        }),
        ("kubernetes: services selecting many workloads", || {
            let pair = |i: usize| {
                format!(
                    "---\napiVersion: v1\nkind: Service\nmetadata:\n  name: s{i}\nspec:\n  selector:\n    app: a\n\
                     ---\napiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: d{i}\nspec:\n  template:\n    \
                     metadata:\n      labels:\n        app: a\n    spec:\n      containers: []\n"
                )
            };
            let files: Vec<(String, String)> = (0..9)
                .map(|n| (format!("f{n}.yaml"), count(450, |i| pair(n * 450 + i))))
                .collect();
            objects(files.iter().map(|(p, t)| (p.as_str(), t.clone())).collect())
        }),
        // Crossplane (#655): compositions, their patch sets, and the kinds XRDs define
        ("crossplane: resources naming many patch sets", || {
            let sets = fill(2, |i| {
                format!(
                    "    - name: p{i}\n      patches:\n        - fromFieldPath: a\n          \
                     toFieldPath: metadata.name\n"
                )
            });
            let resources = fill(2, |i| {
                format!(
                    "    - name: r{i}\n      base: {{apiVersion: s3.aws.upbound.io/v1, kind: B}}\n      \
                     patches:\n        - {{type: PatchSet, patchSetName: p{i}}}\n"
                )
            });
            let text = format!(
                "apiVersion: apiextensions.crossplane.io/v1\nkind: Composition\nmetadata:\n  name: c\n\
                 spec:\n  patchSets:\n{sets}  resources:\n{resources}"
            );
            objects(vec![("c.yaml", text)])
        }),
        ("crossplane: claims of a kind many XRDs define", || {
            let pair = |i: usize| {
                format!(
                    "---\napiVersion: apiextensions.crossplane.io/v1\nkind: CompositeResourceDefinition\n\
                     metadata:\n  name: x{i}\nspec:\n  group: acme.io\n  names: {{kind: X}}\n  \
                     claimNames: {{kind: C}}\n---\napiVersion: acme.io/v1\nkind: C\nmetadata:\n  name: c{i}\n"
                )
            };
            let files: Vec<(String, String)> = (0..9)
                .map(|n| (format!("f{n}.yaml"), count(450, |i| pair(n * 450 + i))))
                .collect();
            objects(files.iter().map(|(p, t)| (p.as_str(), t.clone())).collect())
        }),
        // .NET: `appsettings.json`
        ("appsettings: many keys", || {
            let keys = fill(1, |i| format!("\"Key{i}\": {{\"A\": \"v{i}\"}}, // c\n"));
            let text = format!("{{{keys}\"Z\": \"1\",}}");
            let settings = appsettings::Settings::read(&[("appsettings.json", &file(text))]);
            settings.get("", "Missing:A").map_or(0, str::len)
                + settings.get("", "Z").map_or(0, str::len)
        }),
        ("appsettings: deep nesting", || {
            // Past `serde_json`'s limit the file is not read, within it a key reads to the end.
            let nest = |n: usize| "{\"A\":".repeat(n) + "\"x\"" + &"}".repeat(n);
            let key = "A:".repeat(NESTING);
            let size = |n: usize| {
                let settings = appsettings::Settings::read(&[("appsettings.json", &file(nest(n)))]);
                settings.get("", &key).map_or(0, str::len)
            };
            size(NESTING) + size(120)
        }),
        ("appsettings: many comments", || {
            let comments = fill(1, |_| "/* a */ // b\n".into());
            let text = format!("{{{comments}\"A\": [1,],}}");
            let settings = appsettings::Settings::read(&[("appsettings.json", &file(text))]);
            settings.get("", "A").map_or(0, str::len)
        }),
        // Swift: `Package.swift`, read by the pack's `join` for every file of the repository.
        ("Package.swift: many targets", || {
            let targets = fill(1, |i| {
                format!(".target(name: \"T{i}\", path: \"S/T{i}\"),\n")
            });
            swift_modules(format!("let package = Package(targets: [{targets}])"))
        }),
        ("Package.swift: many dependencies and products", || {
            let deps = fill(2, |i| {
                format!(".package(url: \"https://h/p{i}.git\", from: \"1.{i}\"),\n")
            });
            let libs = fill(2, |i| {
                format!(".library(name: \"L{i}\", targets: [\"T{i}\"]),\n")
            });
            manifest(
                "Package.swift",
                format!(
                    "let package = Package(name: \"P\", products: [{libs}], dependencies: [{deps}])"
                ),
            )
        }),
        ("Package.swift: deep nesting in the package reader", || {
            let nest = "[".repeat(NESTING) + ".package(path: \"a\")" + &"]".repeat(NESTING);
            manifest(
                "Package.swift",
                format!("let package = Package(name: \"P\", dependencies: {nest})"),
            )
        }),
        ("Package.swift: deep nesting", || {
            let nest = "[".repeat(NESTING) + ".target(name: \"T\")" + &"]".repeat(NESTING);
            swift_modules(format!("let package = Package(targets: {nest})"))
        }),
    ]
}

/// The size of the imports the Swift pack's `join` gives each of `IMPORTS` files under the
/// manifest `text`, deep in the tree.
fn swift_modules(text: String) -> usize {
    let manifest = file(text);
    let join = Swift.join(&[("Package.swift", &manifest)], &BTreeMap::new());
    (0..IMPORTS)
        .map(|i| {
            let mut facts = Facts::default();
            join(&format!("S/T{i}/{}/a.swift", "d/".repeat(64)), &mut facts);
            facts.imports.len() + facts.unit.len()
        })
        .sum()
}

/// Every reader, given a file built to explode, returns within `BUDGET` and gives back no more
/// than `MAX_OUT`. The rows run side by side, each on a thread of its own, and the test names the
/// ones that missed.
#[test]
fn every_reader_stays_bounded_on_a_file_built_to_explode() {
    let started = Instant::now();
    let runs: Vec<_> = (probes().into_iter())
        .map(|(name, probe)| {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let start = Instant::now();
                let _ = tx.send((probe(), start.elapsed()));
            });
            (name, rx)
        })
        .collect();
    let mut failed = Vec::new();
    for (name, rx) in runs {
        match rx.recv_timeout(BUDGET.saturating_sub(started.elapsed())) {
            Ok((size, took)) => {
                eprintln!("hostile {name}: {took:?}, {size} bytes");
                if size > MAX_OUT {
                    failed.push(format!("{name}: {size} bytes"));
                }
            }
            Err(_) => failed.push(format!("{name}: no end within {BUDGET:?}, or a panic")),
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
