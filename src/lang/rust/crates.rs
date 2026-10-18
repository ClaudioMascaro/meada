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

/// The largest `Cargo.toml` read; a larger one declares nothing.
// ponytail: a manifest is a few KiB; the limit keeps a crafted one from costing memory.
const MAX_MANIFEST: usize = 1 << 20;

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
    /// The file each `mod` declaration names, by declaring file and specifier.
    children: HashMap<(String, String), String>,
}

impl Tree {
    /// The module trees of the crates the root `Cargo.toml` declares: its own package and each
    /// workspace member with one, among `configs` (every `Cargo.toml`, by path).
    pub fn new(configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Tree {
        let mut tree = Tree {
            nodes: BTreeMap::new(),
            children: HashMap::new(),
        };
        let mut queue = VecDeque::new();
        for root in roots(configs, files) {
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

    /// The file `specifier` names from `from`: `crate` its crate root, `super` (`super::super`)
    /// the module declaring it (and so on up), a `mod` declaration's specifier its file.
    pub fn lookup(&self, from: &str, specifier: &str) -> Vec<String> {
        let found = match specifier {
            "crate" => self.nodes.get(from).map(|n| n.root.clone()),
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

/// The crate roots, in order: per crate (the root package, then the members by directory),
/// its library (`[lib] path` or `src/lib.rs`), `src/main.rs`, each `[[bin]] path`, then
/// `src/bin/*.rs` and `src/bin/*/main.rs`; only those the walk found.
fn roots(configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Vec<String> {
    let manifests: BTreeMap<&str, toml::Table> = configs
        .iter()
        .filter(|(_, src)| src.len() <= MAX_MANIFEST)
        .filter_map(|(path, src)| {
            let dir = path.strip_suffix("Cargo.toml")?.trim_end_matches('/');
            Some((dir, std::str::from_utf8(src).ok()?.parse().ok()?))
        })
        .collect();
    let Some(root) = manifests.get("") else {
        return Vec::new();
    };
    let strings = |table: &toml::Table, key: &str| -> Vec<String> {
        let list = table.get(key).and_then(|v| v.as_array());
        let items = list.into_iter().flatten().filter_map(|v| v.as_str());
        items.map(String::from).collect()
    };
    let workspace = root.get("workspace").and_then(|w| w.as_table());
    let members = workspace.map(|w| strings(w, "members")).unwrap_or_default();
    let exclude = workspace.map(|w| strings(w, "exclude")).unwrap_or_default();
    let crates = manifests.iter().filter(|(dir, manifest)| {
        manifest.contains_key("package")
            && (dir.is_empty()
                || (members.iter().any(|m| matches(m, dir))
                    && !exclude.iter().any(|e| within(e, dir))))
    });
    let mut out = Vec::new();
    for (dir, manifest) in crates {
        let at = |rel: &str| join(dir, rel);
        let path_of = |v: &toml::Value| v.get("path")?.as_str().and_then(at);
        let lib = manifest
            .get("lib")
            .map_or_else(|| at("src/lib.rs"), path_of);
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
        let candidates = (lib.into_iter())
            .chain(at("src/main.rs"))
            .chain(declared)
            .chain(found.map(|f| f.to_string()));
        for file in candidates {
            if files.contains_key(file.as_str()) && !out.contains(&file) {
                out.push(file);
            }
        }
    }
    out
}

/// Whether a `[workspace] members` pattern names the directory `dir`: segment by segment,
/// `*` matching any run of characters within one.
fn matches(pattern: &str, dir: &str) -> bool {
    let Some(pattern) = join("", pattern) else {
        return false;
    };
    let pattern: Vec<&str> = pattern.split('/').collect();
    let dir: Vec<&str> = dir.split('/').collect();
    pattern.len() == dir.len() && pattern.iter().zip(&dir).all(|(p, d)| glob(p, d))
}

/// Whether `name` fits the wildcard `pattern` (`*` any run of bytes), in time linear in their
/// product whatever the stars.
fn glob(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.as_bytes(), name.as_bytes());
    let (mut i, mut j, mut star, mut mark) = (0, 0, None, 0);
    while j < n.len() {
        if p.get(i) == Some(&b'*') {
            (star, mark, i) = (Some(i), j, i + 1);
        } else if p.get(i) == Some(&n[j]) {
            (i, j) = (i + 1, j + 1);
        } else if let Some(s) = star {
            (i, mark) = (s + 1, mark + 1);
            j = mark;
        } else {
            return false;
        }
    }
    p[i..].iter().all(|&c| c == b'*')
}

/// Whether the `exclude` entry `path` holds the directory `dir`.
fn within(path: &str, dir: &str) -> bool {
    join("", path).is_some_and(|p| dir == p || dir.starts_with(&format!("{p}/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_patterns_match_one_segment_per_star() {
        let cases = [
            ("crates/*", "crates/billing", true),
            ("crates/*", "crates/billing/inner", false),
            ("./api", "api", true),
            ("api/", "api", true),
            ("svc-*", "svc-web", true),
            ("svc-*", "web", false),
            ("*-api", "web-api", true),
            ("../api", "api", false),
            ("/api", "api", false),
        ];
        for (pattern, dir, expected) in cases {
            assert_eq!(matches(pattern, dir), expected, "{pattern} on {dir}");
        }
    }

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
    fn a_manifest_past_the_size_limit_declares_no_crate() {
        let facts = Facts::default();
        let files = BTreeMap::from([("src/lib.rs", &facts)]);
        let manifest = "[package]\nname = \"a\"\n";
        let big = format!("{manifest}{}", " ".repeat(MAX_MANIFEST));
        let roots_of = |src: &str| roots(&[("Cargo.toml", src.as_bytes())], &files);
        assert_eq!(roots_of(manifest), ["src/lib.rs"]);
        assert!(roots_of(&big).is_empty());
    }
}
