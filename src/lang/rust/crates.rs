//! The crates of a Cargo repository and their module trees (spec #30): the files each crate
//! builds, from its roots through `mod` declarations, and the file a `mod` declaration,
//! `crate` and `super` name. Nothing is opened on disk: every path is matched against the files
//! the walk found, so it stays inside the repository and follows no symlink.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::lang::Facts;

/// The start of the specifier of a `mod x;` declaration: `mod x`, or `mod a/b/x` inside the
/// modules `a::b` written in place.
pub const MOD: &str = "mod ";
/// The start of the specifier of a `#[path = "p"] mod x;` declaration: `path |p`, or
/// `path a/b|p` inside the modules `a::b` written in place.
pub const PATH: &str = "path ";

/// A crate file reached from a root: its root, the file whose top-level `mod` declaration
/// reached it (none for a root, or for a module declared inside a module written in place),
/// and whether its `mod` declarations name files in its own directory (a root, a `mod.rs`, a
/// file a `#[path]` names) or in the directory named after it.
struct Node {
    root: String,
    parent: Option<String>,
    owns_dir: bool,
}

pub struct Tree {
    nodes: BTreeMap<String, Node>,
    /// The roots of binary targets: every root but a library's.
    bins: BTreeSet<String>,
    /// The file each `mod` declaration names, by declaring file and specifier.
    children: HashMap<(String, String), String>,
}

impl Tree {
    /// The module trees of the crates, one per `Cargo.toml` with a `[package]` among `configs`
    /// (every `Cargo.toml` the walk found, by path).
    pub fn new(configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Tree {
        let mut tree = Tree {
            nodes: BTreeMap::new(),
            bins: BTreeSet::new(),
            children: HashMap::new(),
        };
        let mut queue = VecDeque::new();
        for (root, bin) in roots(configs, files) {
            if bin {
                tree.bins.insert(root.clone());
            }
            let node = Node {
                root: root.clone(),
                parent: None,
                owns_dir: true,
            };
            if tree.nodes.insert(root.clone(), node).is_none() {
                queue.push_back(root);
            }
        }
        while let Some(file) = queue.pop_front() {
            let (root, owns_dir) = (tree.nodes[&file].root.clone(), tree.nodes[&file].owns_dir);
            for import in &files[file.as_str()].imports {
                let Some((child, owns, top)) = declared(&file, owns_dir, &import.source, files)
                else {
                    continue;
                };
                tree.children
                    .insert((file.clone(), import.source.clone()), child.clone());
                if tree.nodes.contains_key(&child) {
                    continue;
                }
                let node = Node {
                    root: root.clone(),
                    parent: top.then(|| file.clone()),
                    owns_dir: owns,
                };
                tree.nodes.insert(child.clone(), node);
                queue.push_back(child);
            }
        }
        tree
    }

    /// Every file a crate builds.
    pub fn members(&self) -> BTreeSet<String> {
        self.nodes.keys().cloned().collect()
    }

    /// The roots of the binary targets: the files whose `fn main` starts a program.
    pub fn bins(&self) -> impl Iterator<Item = &str> {
        self.bins.iter().map(String::as_str)
    }

    /// The file `specifier` names from `from`: `crate` its crate root, `self` itself, `super`
    /// (`super::super`) the module declaring it (and so on up), a `mod` declaration's specifier
    /// its file.
    pub fn lookup(&self, from: &str, specifier: &str) -> Vec<String> {
        let found = match specifier {
            "crate" => self.nodes.get(from).map(|n| n.root.clone()),
            "self" => self.nodes.contains_key(from).then(|| from.to_string()),
            s if s.starts_with(MOD) || s.starts_with(PATH) => self
                .children
                .get(&(from.to_string(), s.to_string()))
                .cloned(),
            s if s.split("::").all(|p| p == "super") => {
                let mut at = Some(from);
                for _ in s.split("::") {
                    at = at.and_then(|f| self.nodes.get(f)?.parent.as_deref());
                }
                at.map(String::from)
            }
            _ => None,
        };
        found.into_iter().collect()
    }
}

/// The file the `mod` declaration `specifier` in `file` names, whether it owns its directory,
/// and whether it was declared at the top level of `file`.
fn declared(
    file: &str,
    owns_dir: bool,
    specifier: &str,
    files: &BTreeMap<&str, &Facts>,
) -> Option<(String, bool, bool)> {
    let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    // Where the modules it declares live: its directory, or the one named after it.
    let base = match owns_dir {
        true => dir.to_string(),
        false => join(dir, file.rsplit('/').next()?.strip_suffix(".rs")?)?,
    };
    if let Some(rel) = specifier.strip_prefix(MOD) {
        let top = !rel.contains('/');
        let stem = join(&base, rel)?;
        let candidates = [
            (format!("{stem}.rs"), false),
            (format!("{stem}/mod.rs"), true),
        ];
        let (child, owns) = candidates
            .into_iter()
            .find(|(c, _)| files.contains_key(c.as_str()))?;
        return Some((child, owns, top));
    }
    let (chain, value) = specifier.strip_prefix(PATH)?.split_once('|')?;
    // Outside a module written in place, the path is relative to the file's own directory.
    let child = match chain {
        "" => join(dir, value)?,
        chain => join(&join(&base, chain)?, value)?,
    };
    files
        .contains_key(child.as_str())
        .then_some((child, true, chain.is_empty()))
}

/// `rel` joined to the directory `dir`, `.` and `..` folded: `None` when it is absolute or
/// leaves the repository.
fn join(dir: &str, rel: &str) -> Option<String> {
    if rel.starts_with('/') || rel.contains('\\') {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// The crate roots, in order: per `Cargo.toml` with a `[package]` table, at any depth, by
/// path, its library (`[lib] path`, else `src/lib.rs`), `src/main.rs`, each `[[bin]] path`,
/// then `src/bin/*.rs` and `src/bin/*/main.rs`; only those the walk found. Each comes with
/// whether it is a binary target (every root but the library). A workspace's
/// members are such manifests too, so `[workspace] members` adds none.
fn roots(configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Vec<(String, bool)> {
    let mut manifests: Vec<(&str, toml::Table)> = configs
        .iter()
        .filter_map(|(path, src)| {
            let dir = path.strip_suffix("Cargo.toml")?;
            let dir = dir.strip_suffix('/').unwrap_or(dir);
            Some((dir, std::str::from_utf8(src).ok()?.parse().ok()?))
        })
        .filter(|(_, manifest): &(&str, toml::Table)| manifest.contains_key("package"))
        .collect();
    manifests.sort_by_key(|(dir, _)| *dir);
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (dir, manifest) in &manifests {
        let at = |rel: &str| join(dir, rel);
        let path_of = |v: &toml::Value| v.get("path")?.as_str().and_then(at);
        let lib = (manifest.get("lib").and_then(path_of)).or_else(|| at("src/lib.rs"));
        let bins = manifest.get("bin").and_then(|b| b.as_array());
        let declared = bins.into_iter().flatten().filter_map(path_of);
        let src_bin = at("src/bin").unwrap_or_default();
        let found = files.keys().filter(|f| {
            let Some(rest) = f.strip_prefix(&src_bin).and_then(|r| r.strip_prefix('/')) else {
                return false;
            };
            match rest.split_once('/') {
                None => rest.ends_with(".rs"),
                Some((_, file)) => file == "main.rs",
            }
        });
        let bins = (at("src/main.rs").into_iter())
            .chain(declared)
            .chain(found.map(|f| f.to_string()));
        let candidates = (lib.into_iter().map(|f| (f, false))).chain(bins.map(|f| (f, true)));
        for (file, bin) in candidates {
            if files.contains_key(file.as_str()) && seen.insert(file.clone()) {
                out.push((file, bin));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_repository() {
        assert_eq!(
            join("api/src", "../extra/x.rs").as_deref(),
            Some("api/extra/x.rs")
        );
        assert_eq!(join("api/src", "./x.rs").as_deref(), Some("api/src/x.rs"));
        assert_eq!(join("api", "../../x.rs"), None);
        assert_eq!(join("api", "/etc/x.rs"), None);
    }

    #[test]
    fn a_bin_path_outside_the_repository_or_missing_is_no_root() {
        let facts = Facts::default();
        let files = BTreeMap::from([("api/src/lib.rs", &facts)]);
        let configs: [(&str, &[u8]); 1] = [(
            "api/Cargo.toml",
            b"[package]\nname = \"a\"\n[[bin]]\npath = \"../../x.rs\"\n[[bin]]\npath = \"gone.rs\"\n",
        )];
        let tree = Tree::new(&configs, &files);
        assert_eq!(tree.bins().count(), 0);
    }

    #[test]
    fn every_package_at_any_depth_is_a_crate_with_its_own_roots() {
        let facts = Facts::default();
        let files = BTreeMap::from([
            ("backend/src/lib.rs", &facts),
            ("backend/src/main.rs", &facts),
            ("backend/src/bin/sync/main.rs", &facts),
            ("backend/src/bin/tool.rs", &facts),
            ("src-tauri/src/main.rs", &facts),
            ("virtual/src/lib.rs", &facts),
        ]);
        let configs: [(&str, &[u8]); 3] = [
            // A `[lib]` table with no `path` keeps `src/lib.rs`.
            (
                "backend/Cargo.toml",
                b"[package]\nname = \"b\"\n[lib]\nproc-macro = true\n",
            ),
            ("src-tauri/Cargo.toml", b"[package]\nname = \"t\"\n"),
            // A virtual manifest builds nothing of its own.
            ("virtual/Cargo.toml", b"[workspace]\nmembers = []\n"),
        ];
        assert_eq!(
            roots(&configs, &files),
            [
                ("backend/src/lib.rs".to_string(), false),
                ("backend/src/main.rs".to_string(), true),
                ("backend/src/bin/sync/main.rs".to_string(), true),
                ("backend/src/bin/tool.rs".to_string(), true),
                ("src-tauri/src/main.rs".to_string(), true),
            ]
        );
    }
}
