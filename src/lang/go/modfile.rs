//! `go.mod` files, read as the line format they are, with no parser crate: the module path and
//! the `replace` directives that point to a local directory. A directive is one line
//! (`replace a => ./a`) or a block of them (`replace (` ... `)`), and `//` starts a comment.

/// What one `go.mod` declares.
#[derive(Debug, Default, PartialEq)]
pub struct ModFile {
    /// The `module` path.
    pub module: Option<String>,
    /// Each `replace`, as the module path it replaces and the path or module that replaces it,
    /// as written.
    pub replaces: Vec<(String, String)>,
}

/// The directives of `text`. A line it cannot read is skipped.
pub fn read(text: &str) -> ModFile {
    let mut file = ModFile::default();
    // The directive whose block the line is in.
    let mut block: Option<String> = None;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        let tokens: Vec<&str> = (line.split_whitespace())
            .map(|t| t.trim_matches(|c| c == '"' || c == '`'))
            .collect();
        let (directive, args) = match (&block, tokens.as_slice()) {
            (Some(_), [")"]) => {
                block = None;
                continue;
            }
            (Some(directive), args) => (directive.clone(), args),
            (None, [directive, "("]) => {
                block = Some(directive.to_string());
                continue;
            }
            (None, [directive, args @ ..]) => (directive.to_string(), args),
            (None, []) => continue,
        };
        match (directive.as_str(), args) {
            ("module", [path, ..]) => file.module = Some(path.to_string()),
            ("replace", args) => {
                let Some(arrow) = args.iter().position(|a| *a == "=>") else {
                    continue;
                };
                if let (Some(old), Some(new)) = (args.first(), args.get(arrow + 1))
                    && arrow > 0
                {
                    file.replaces.push((old.to_string(), new.to_string()));
                }
            }
            _ => {}
        }
    }
    file
}

/// The modules of a repository: each `go.mod`'s directory, module path and what its `replace`
/// directives name.
pub struct Modules {
    modules: Vec<Module>,
}

struct Module {
    dir: String,
    path: Option<String>,
    /// The module paths each `replace` replaces, with the directory of the repository that
    /// replaces it; `None` for any other target (a module, a directory outside), which makes the
    /// replaced module one from outside.
    replaces: Vec<(String, Option<String>)>,
}

impl Modules {
    /// From the `go.mod` files as `(path, contents)`.
    pub fn new(configs: &[(&str, &[u8])]) -> Self {
        let modules = (configs.iter())
            .filter(|(path, _)| path.rsplit('/').next() == Some("go.mod"))
            .map(|(path, bytes)| {
                let dir = path.rsplit_once('/').map_or("", |(d, _)| d).to_string();
                let file = read(&String::from_utf8_lossy(bytes));
                let replaces = (file.replaces.into_iter())
                    .map(|(old, new)| {
                        let target = local(&dir, &new);
                        (old, target)
                    })
                    .collect();
                Module {
                    dir,
                    path: file.module,
                    replaces,
                }
            })
            .collect();
        Self { modules }
    }

    /// The module that owns `dir`: the nearest `go.mod` at or above it.
    fn owner(&self, dir: &str) -> Option<&Module> {
        (self.modules.iter())
            .filter(|m| within(dir, &m.dir))
            .max_by_key(|m| m.dir.len())
    }

    /// The package directory an import of `specifier` from a file of `from` names: through a
    /// `replace` of the importing file's module (none when it replaces the module with one from
    /// outside), else through the module path of the repository's modules, the longest that
    /// matches (none when two modules declare it). The directory must belong to that module, not
    /// to a module nested in it. `None` for the standard library and the modules outside.
    pub fn dir(&self, from: &str, specifier: &str) -> Option<String> {
        let own = self.owner(from)?;
        let replaced = (own.replaces.iter())
            .filter_map(|(old, dir)| Some((rest(specifier, old)?, dir.as_deref(), old.len())))
            .max_by_key(|(_, _, len)| *len);
        let replaced = match replaced {
            Some((rest, dir, len)) => Some((rest, dir?, len)),
            None => None,
        };
        let found = replaced.or_else(|| {
            let matches: Vec<(&str, &str, usize)> = (self.modules.iter())
                .filter_map(|m| {
                    let path = m.path.as_deref()?;
                    Some((rest(specifier, path)?, m.dir.as_str(), path.len()))
                })
                .collect();
            let longest = matches.iter().map(|(_, _, len)| *len).max()?;
            // Two modules that declare one path (a fork) name no directory.
            match &matches
                .iter()
                .filter(|m| m.2 == longest)
                .collect::<Vec<_>>()[..]
            {
                [one] => Some(**one),
                _ => None,
            }
        });
        let (rest, base, _) = found?;
        if !rest.is_empty() && rest.split('/').any(|s| matches!(s, "" | "." | "..")) {
            return None;
        }
        let target = match (base, rest) {
            ("", rest) => rest.to_string(),
            (base, "") => base.to_string(),
            (base, rest) => format!("{base}/{rest}"),
        };
        (self.owner(&target)?.dir == base).then_some(target)
    }
}

/// What follows the module path `module` in the import path `specifier`, if it starts with it.
fn rest<'s>(specifier: &'s str, module: &str) -> Option<&'s str> {
    match specifier.strip_prefix(module)? {
        "" => Some(""),
        rest => rest.strip_prefix('/'),
    }
}

/// Whether `dir` is `parent` or below it.
fn within(dir: &str, parent: &str) -> bool {
    parent.is_empty()
        || dir == parent
        || dir.strip_prefix(parent).is_some_and(|r| r.starts_with('/'))
}

/// The repository directory a `replace` target names, relative to the `go.mod` in `dir`: only a
/// local path (`./x`, `../x`) that stays inside the repository.
fn local(dir: &str, target: &str) -> Option<String> {
    if !(target.starts_with("./") || target.starts_with("../") || target == "." || target == "..") {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_go_mod_gives_its_module_path_and_replaces() {
        let file = read(
            "// a comment\nmodule \"example.com/shop\" // trailing\n\ngo 1.22\n\n\
             require (\n\texample.com/billing v0.1.0\n)\n\
             replace example.com/billing => ./billing\n\
             replace (\n\texample.com/legacy v1.0.0 => ../legacy\n\texample.com/up => example.com/up v1.2.0\n)\n\
             replace broken =>\n",
        );
        assert_eq!(
            file,
            ModFile {
                module: Some("example.com/shop".into()),
                replaces: vec![
                    ("example.com/billing".into(), "./billing".into()),
                    ("example.com/legacy".into(), "../legacy".into()),
                    ("example.com/up".into(), "example.com/up".into()),
                ],
            }
        );
    }

    #[test]
    fn a_replace_names_a_local_directory_inside_the_repository_alone() {
        assert_eq!(local("", "./billing").as_deref(), Some("billing"));
        assert_eq!(local("app", "../lib/x").as_deref(), Some("lib/x"));
        assert_eq!(local("app", "./").as_deref(), Some("app"));
        assert_eq!(local("", "../outside"), None);
        assert_eq!(local("", "/abs/path"), None);
        assert_eq!(local("", "example.com/up"), None);
    }

    #[test]
    fn an_import_names_the_directory_its_module_owns() {
        let configs: &[(&str, &[u8])] = &[
            (
                "go.mod",
                b"module example.com/shop\nreplace example.com/billing => ./billing\n",
            ),
            ("billing/go.mod", b"module example.com/billing\n"),
            ("worker/go.mod", b"module example.com/worker\n"),
            ("forks/billing/go.mod", b"module example.com/billing\n"),
            ("forks/go.mod", b"module example.com/forks\n"),
        ];
        let modules = Modules::new(configs);
        let dir = |from, spec| modules.dir(from, spec);
        assert_eq!(dir("", "example.com/shop/store").as_deref(), Some("store"));
        assert_eq!(dir("cmd", "example.com/shop").as_deref(), Some(""));
        assert_eq!(
            dir("", "example.com/billing/tax").as_deref(),
            Some("billing/tax")
        );
        assert_eq!(dir("forks", "example.com/forks").as_deref(), Some("forks"));
        // A nested module owns its subtree.
        assert_eq!(dir("", "example.com/shop/worker/jobs"), None);
        assert_eq!(
            dir("worker", "example.com/worker/jobs").as_deref(),
            Some("worker/jobs")
        );
        // A replace by a module from outside makes the replaced one outside.
        let forked: &[(&str, &[u8])] = &[
            ("go.mod", b"module example.com/shop\nreplace example.com/billing v1.0.0 => example.com/fork v1.0.0\n"),
            ("billing/go.mod", b"module example.com/billing\n"),
        ];
        let forked = Modules::new(forked);
        assert_eq!(forked.dir("", "example.com/billing/tax"), None);
        assert_eq!(
            forked.dir("billing", "example.com/billing/tax").as_deref(),
            Some("billing/tax")
        );
        // Two modules that declare one path, with no replace to pick one, name nothing.
        assert_eq!(dir("worker", "example.com/billing/tax"), None);
        // A module of the repository is found by its path from any other.
        assert_eq!(dir("worker", "example.com/forks").as_deref(), Some("forks"));
        assert_eq!(dir("", "fmt"), None);
        assert_eq!(dir("", "example.com/shopping"), None);
        assert_eq!(dir("", "example.com/shop/../x"), None);
    }
}
