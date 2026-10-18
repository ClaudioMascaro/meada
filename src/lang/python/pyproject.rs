//! `pyproject.toml`, parsed once for every reader of it: the source roots the Python pack
//! resolves imports through, and the distribution and dependencies a package of the index
//! stores (`package::pypi`). A later reader (`[project.scripts]`) adds an accessor here.

use std::collections::HashSet;

use crate::Result;

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
    /// are not read; an entry of another type is left out.
    pub fn requirements(&self) -> Vec<Requirement<'_>> {
        let pep621 = self
            .at(&["project", "dependencies"])
            .and_then(|v| v.as_array());
        let poetry = (self.at(&["tool", "poetry", "dependencies"])).and_then(|v| v.as_table());
        let pep621 =
            (pep621.into_iter().flatten()).filter_map(|v| v.as_str().map(Requirement::Pep508));
        let poetry = (poetry.into_iter().flatten())
            .filter(|(name, _)| *name != "python")
            .map(|(name, v)| {
                let version = match v {
                    toml::Value::String(s) => Some(s.as_str()),
                    toml::Value::Table(t) => t.get("version").and_then(|v| v.as_str()),
                    _ => None,
                };
                Requirement::Poetry(name, version)
            });
        pep621.chain(poetry).collect()
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
[tool.poetry.group.dev.dependencies]
h = "1"
"#,
        );
        use Requirement::*;
        assert_eq!(
            p.requirements(),
            [
                Pep508("a>=1"),
                Pep508("b"),
                Poetry("d", Some("^1.0")),
                Poetry("e", Some("~2")),
                Poetry("f", None),
                Poetry("g", None),
            ]
        );
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
