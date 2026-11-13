//! `pyproject.toml`, parsed once for every reader of it: the source roots the Python pack
//! resolves imports through, and the distribution and dependencies a package of the index
//! stores (`package::pypi`), and the console scripts that start a program (`scripts`).

use std::collections::HashSet;

use crate::Result;

/// The most `members` or `exclude` globs read of a uv workspace; the rest are left out.
// ponytail: real workspaces list a few; this bounds the member walk over a hostile file.
const MAX_GLOBS: usize = 256;

/// A parsed `pyproject.toml`.
pub struct Pyproject(toml::Table);

/// A dependency a `pyproject.toml` declares, as written.
#[derive(Debug, PartialEq)]
pub enum Requirement<'a> {
    /// An entry of `[project] dependencies`: a PEP 508 string.
    Pep508(&'a str),
    /// An entry of `[tool.poetry.dependencies]`: the name, and the version constraint when it
    /// is a string or the `version` of an inline table.
    Poetry(&'a str, Option<&'a str>),
}

/// Where `[tool.uv.sources]` says a dependency comes from, as written.
#[derive(Debug, PartialEq)]
pub enum UvSource<'a> {
    /// `{ workspace = true }`: the member of the consumer's uv workspace.
    Workspace,
    /// `{ path = "..." }`, relative to the directory of the `pyproject.toml`.
    Path(&'a str),
    /// `{ git = "..." }`: the remote's URL.
    Git(&'a str),
    /// `{ url = "..." }` or `{ index = "..." }`: from outside the workspace.
    Outside,
}

impl Pyproject {
    /// Parses the bytes; an error for a file that is not UTF-8 or not TOML.
    pub fn parse(src: &[u8]) -> Result<Self> {
        Ok(Self(std::str::from_utf8(src)?.parse()?))
    }

    /// The value at the dotted `path` of tables.
    fn at(&self, path: &[&str]) -> Option<&toml::Value> {
        let (first, rest) = path.split_first()?;
        rest.iter()
            .try_fold(self.0.get(*first)?, |v, key| v.get(key))
    }

    fn text(&self, path: &[&str]) -> Option<&str> {
        self.at(path)?.as_str()
    }

    /// The distribution name as written: `[project] name` (PEP 621), else `[tool.poetry] name`.
    pub fn name(&self) -> Option<&str> {
        (self.text(&["project", "name"])).or_else(|| self.text(&["tool", "poetry", "name"]))
    }

    /// The version as written, from the same table as `name`.
    pub fn version(&self) -> Option<&str> {
        (self.text(&["project", "version"])).or_else(|| self.text(&["tool", "poetry", "version"]))
    }

    /// The dependencies of `[project] dependencies` and `[tool.poetry.dependencies]` (its
    /// `python` key excepted), in order. Poetry groups and `[project.optional-dependencies]`
    /// are not read; an entry of another type, and a Poetry `git` or `url` source, is left out.
    pub fn requirements(&self) -> Vec<Requirement<'_>> {
        let pep621 = self
            .at(&["project", "dependencies"])
            .and_then(|v| v.as_array());
        let poetry = (self.at(&["tool", "poetry", "dependencies"])).and_then(|v| v.as_table());
        let pep621 =
            (pep621.into_iter().flatten()).filter_map(|v| v.as_str().map(Requirement::Pep508));
        let poetry = (poetry.into_iter().flatten())
            .filter(|(name, _)| *name != "python")
            .filter_map(|(name, v)| {
                let version = match v {
                    toml::Value::String(s) => Some(s.as_str()),
                    // A git or URL source is a direct reference, as `name @ url` is in PEP 508:
                    // no entry, unless a `path` keeps the package local.
                    toml::Value::Table(t)
                        if !t.contains_key("path")
                            && (t.contains_key("git") || t.contains_key("url")) =>
                    {
                        return None;
                    }
                    toml::Value::Table(t) => t.get("version").and_then(|v| v.as_str()),
                    _ => None,
                };
                Some(Requirement::Poetry(name, version))
            });
        pep621.chain(poetry).collect()
    }

    /// The sources of `[tool.uv.sources]`, by the key as written: a table that names exactly one
    /// of `workspace = true`, `path`, `git`, `url` and `index` (as a string). A list of sources
    /// (one per marker), `workspace = false` and any other value give none.
    pub fn uv_sources(&self) -> Vec<(&str, UvSource<'_>)> {
        let table = self
            .at(&["tool", "uv", "sources"])
            .and_then(|v| v.as_table());
        (table.into_iter().flatten())
            .filter_map(|(name, v)| {
                let t = v.as_table()?;
                let text = |key: &str| t.get(key).and_then(|v| v.as_str());
                let found: Vec<UvSource> = [
                    (t.get("workspace").and_then(|v| v.as_bool()) == Some(true))
                        .then_some(UvSource::Workspace),
                    text("path").map(UvSource::Path),
                    text("git").map(UvSource::Git),
                    text("url").map(|_| UvSource::Outside),
                    text("index").map(|_| UvSource::Outside),
                ]
                .into_iter()
                .flatten()
                .collect();
                let [source] = <[UvSource; 1]>::try_from(found).ok()?;
                Some((name.as_str(), source))
            })
            .collect()
    }

    /// The `members` and `exclude` globs of `[tool.uv.workspace]`, the first `MAX_GLOBS` strings
    /// of each list, when the file declares that table: it is then a uv workspace root.
    pub fn uv_workspace(&self) -> Option<(Vec<String>, Vec<String>)> {
        let table = self.at(&["tool", "uv", "workspace"])?.as_table()?;
        let globs = |key: &str| -> Vec<String> {
            (table
                .get(key)
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten())
            .filter_map(|g| Some(g.as_str()?.to_string()))
            .take(MAX_GLOBS)
            .collect()
        };
        Some((globs("members"), globs("exclude")))
    }

    /// The console scripts of `[project.scripts]` (else Poetry's `[tool.poetry.scripts]`) as
    /// `(name, module, function)`, sorted by name: the key as written, which is the command
    /// pip installs, and its `module:function` value, extras (`module:function [extra]`)
    /// dropped. An empty name, and a value that is not a dotted module, a colon and a dotted
    /// name, are left out.
    pub fn scripts(&self) -> Vec<(String, String, String)> {
        let table = (self.at(&["project", "scripts"]))
            .or_else(|| self.at(&["tool", "poetry", "scripts"]))
            .and_then(|t| t.as_table());
        let dotted = |s: &str| {
            (s.split('.')).all(|part| {
                !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_')
            })
        };
        let mut found: Vec<(String, String, String)> = (table.into_iter().flatten())
            .filter_map(|(name, target)| {
                let (module, function) = target.as_str()?.split_once(':')?;
                let function = function.split(['[', ' ']).next()?;
                (!name.is_empty() && dotted(module) && dotted(function))
                    .then(|| (name.clone(), module.to_string(), function.to_string()))
            })
            .collect();
        found.sort();
        found
    }

    /// The source roots it declares besides the directory holding it: setuptools'
    /// `package-dir` and `packages.find` `where`, and Poetry's `packages` with `from`, each
    /// relative to that directory and inside it.
    // ponytail: other build backends (hatch, pdm, flit) are not read.
    pub fn roots(&self) -> Vec<String> {
        let setuptools = self.at(&["tool", "setuptools"]);
        let mut roots: Vec<String> = Vec::new();
        // `"" = "src"` roots every package in `src`; `"pkg" = "lib/pkg"` roots `pkg` in `lib`,
        // and `"a.b" = "lib/a/b"` roots `a.b` in `lib`. A directory not named after its package
        // says nothing about a root.
        let dirs = setuptools.and_then(|s| s.get("package-dir"));
        for (package, dir) in dirs.and_then(toml::Value::as_table).into_iter().flatten() {
            let (Some(dir), Some(package)) = (
                dir.as_str().and_then(clean),
                clean(&package.replace('.', "/")),
            ) else {
                continue;
            };
            if package.is_empty() {
                roots.push(dir);
            } else if let Some(root) = dir.strip_suffix(&format!("/{package}")) {
                roots.push(root.to_string());
            }
        }
        let find = setuptools.and_then(|s| s.get("packages")?.get("find"));
        let wheres = find.and_then(|f| f.get("where")?.as_array());
        roots.extend(
            wheres
                .into_iter()
                .flatten()
                .filter_map(|w| clean(w.as_str()?)),
        );
        let poetry = self
            .at(&["tool", "poetry", "packages"])
            .and_then(|p| p.as_array());
        roots.extend(
            poetry
                .into_iter()
                .flatten()
                .filter_map(|p| clean(p.get("from")?.as_str()?)),
        );
        let mut seen = HashSet::new();
        roots.retain(|root| !root.is_empty() && seen.insert(root.clone()));
        roots
    }
}

/// The source roots of the package whose `pyproject.toml` sits in `dir` (`""` at the repository
/// root) and declares `declared` (`Pyproject::roots`): `dir`, then the declared roots, or its
/// `src` when none is. Both the pack's imports and the PyPI packages' modules use it.
// ponytail: `src` is a root whether or not it holds packages, since neither reader sees the
// file list.
pub fn source_roots(dir: &str, declared: &[String]) -> Vec<String> {
    let join = |root: &str| match dir {
        "" => root.to_string(),
        dir => format!("{dir}/{root}"),
    };
    let mut roots = vec![dir.to_string()];
    match declared.is_empty() {
        true => roots.push(join("src")),
        false => roots.extend(declared.iter().map(|r| join(r))),
    }
    roots
}

/// A relative directory in its plain form, `None` when it leaves the directory it is relative
/// to.
fn clean(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Pyproject {
        Pyproject::parse(src.as_bytes()).unwrap()
    }

    #[test]
    fn the_name_and_version_come_from_pep_621_then_poetry() {
        let both = parse(
            "[project]\nname = \"a\"\nversion = \"1\"\n[tool.poetry]\nname = \"b\"\nversion = \"2\"\n",
        );
        assert_eq!((both.name(), both.version()), (Some("a"), Some("1")));
        let poetry = parse("[tool.poetry]\nname = \"b\"\nversion = \"2\"\n");
        assert_eq!((poetry.name(), poetry.version()), (Some("b"), Some("2")));
        let neither = parse("[project]\nname = 3\ndynamic = [\"version\"]\n");
        assert_eq!((neither.name(), neither.version()), (None, None));
    }

    #[test]
    fn requirements_come_from_pep_621_and_poetry_but_not_groups_or_extras() {
        let p = parse(
            r#"
[project]
dependencies = ["a>=1", 3, "b"]
[project.optional-dependencies]
dev = ["c"]
[tool.poetry.dependencies]
python = "^3.11"
d = "^1.0"
e = { version = "~2", extras = ["x"] }
f = { path = "../f" }
g = [{ version = "1" }]
i = { git = "https://example.com/i.git", version = "1" }
j = { url = "https://example.com/j.whl" }
k = { git = "https://example.com/k.git", path = "../k" }
[tool.poetry.group.dev.dependencies]
h = "1"
"#,
        );
        use Requirement::*;
        // A git or URL source is a direct reference, as `name @ url` is: no entry. A path keeps
        // it one.
        assert_eq!(
            p.requirements(),
            [
                Pep508("a>=1"),
                Pep508("b"),
                Poetry("d", Some("^1.0")),
                Poetry("e", Some("~2")),
                Poetry("f", None),
                Poetry("g", None),
                Poetry("k", None),
            ]
        );
    }

    #[test]
    fn uv_sources_and_a_uv_workspace_are_read_as_written() {
        let p = parse(
            r#"
[tool.uv.sources]
a = { workspace = true }
b = { path = "../b", editable = true }
c = { git = "https://example.com/c.git", tag = "v1" }
d = { url = "https://example.com/d.whl" }
e = { index = "pytorch" }
f = { workspace = false }
g = [{ index = "a", marker = "sys_platform == 'linux'" }]
h = { path = "../h", git = "https://example.com/h.git" }
i = "1.0"
[tool.uv.workspace]
members = ["packages/*", 3]
exclude = ["packages/old"]
"#,
        );
        use UvSource::*;
        assert_eq!(
            p.uv_sources(),
            [
                ("a", Workspace),
                ("b", Path("../b")),
                ("c", Git("https://example.com/c.git")),
                ("d", Outside),
                ("e", Outside),
            ]
        );
        let globs = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            p.uv_workspace(),
            Some((globs(&["packages/*"]), globs(&["packages/old"])))
        );
        assert_eq!(parse("[tool.uv]\n").uv_workspace(), None);
        let many = format!(
            "[tool.uv.workspace]\nmembers = [{}]\n",
            "\"a\",".repeat(1000)
        );
        let (members, _) = parse(&many).uv_workspace().unwrap();
        assert_eq!(members.len(), MAX_GLOBS);
        assert_eq!(
            parse("[tool.uv.workspace]\n").uv_workspace(),
            Some((vec![], vec![]))
        );
    }

    #[test]
    fn scripts_are_the_name_module_and_function_of_each_console_script() {
        let p = parse(
            r#"
[project.scripts]
serve = "app.cli:main"
sync = "app.cli:main"
extra = "tools:run [fast]"
nested = "app.cli:Group.start"
"Acme_Tool" = "app.cli:main"
"acme.dotted" = "app.cli:main"
"" = "app.cli:main"
no-colon = "app.cli"
no-module = ":main"
bad-module = "app/../x:main"
empty-part = "app..cli:main"
not-text = 3
"#,
        );
        let script = |n: &str, m: &str, f: &str| (n.to_string(), m.to_string(), f.to_string());
        // The name is kept as written (#832): pip installs the command under it, byte for byte.
        assert_eq!(
            p.scripts(),
            [
                script("Acme_Tool", "app.cli", "main"),
                script("acme.dotted", "app.cli", "main"),
                script("extra", "tools", "run"),
                script("nested", "app.cli", "Group.start"),
                script("serve", "app.cli", "main"),
                script("sync", "app.cli", "main"),
            ]
        );
    }

    #[test]
    fn poetry_scripts_count_when_there_is_no_project_table() {
        let poetry = "[tool.poetry.scripts]\nrun = \"a:b\"\n";
        let script = |m: &str, f: &str| [("run".to_string(), m.to_string(), f.to_string())];
        assert_eq!(parse(poetry).scripts(), script("a", "b"));
        let both = format!("[project.scripts]\nrun = \"c:d\"\n{poetry}");
        assert_eq!(parse(&both).scripts(), script("c", "d"));
        assert!(parse("[project]\nname = \"x\"\n").scripts().is_empty());
    }

    #[test]
    fn a_file_that_is_not_toml_is_an_error() {
        for bad in [&b"[tool\n"[..], b"\xff", b"a = "] {
            assert!(Pyproject::parse(bad).is_err());
        }
    }

    #[test]
    fn deeply_nested_input_does_not_overflow_the_stack() {
        let n = 100_000;
        for src in [
            format!("a = {}{}", "[".repeat(n), "]".repeat(n)),
            format!("a = {}1{}", "{b = ".repeat(n), "}".repeat(n)),
        ] {
            let _ = Pyproject::parse(src.as_bytes());
        }
    }
}
