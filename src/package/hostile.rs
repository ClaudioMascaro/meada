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
