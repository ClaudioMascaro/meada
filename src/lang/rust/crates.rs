//! The crates of a Cargo repository and their module trees (spec #30): the files each crate
//! builds, from its roots through `mod` declarations, and the file a `mod` declaration,
//! `crate` and `super` name. Nothing is opened on disk: every path is matched against the files
//! the walk found, so it stays inside the repository and follows no symlink.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::lang::{Export, Facts};

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
    /// The libraries of the repository each crate root may name, by the name it writes:
    /// the root file of each.
    libs: HashMap<String, HashMap<String, String>>,
    /// The roots of binary targets: every root but a library's.
    bins: BTreeSet<String>,
    /// The file each `mod` declaration names, by declaring file and specifier.
    children: HashMap<(String, String), String>,
    /// The files whose `pub` items another crate can name.
    published: BTreeSet<String>,
}

impl Tree {
    /// The module trees of the crates, one per `Cargo.toml` with a `[package]` among `configs`
    /// (every `Cargo.toml` the walk found, by path).
    pub fn new(configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Tree {
        let mut tree = Tree {
            nodes: BTreeMap::new(),
            libs: HashMap::new(),
            bins: BTreeSet::new(),
            children: HashMap::new(),
            published: BTreeSet::new(),
        };
        let packages = packages(configs, files);
        let dependencies = dependencies(configs, &packages);
        let mut queue = VecDeque::new();
        for (root, dir, bin) in roots(configs, files) {
            if bin {
                tree.bins.insert(root.clone());
            }
            let node = Node {
                root: root.clone(),
                parent: None,
                owns_dir: true,
            };
            let mut libs = dependencies.get(&dir).cloned().unwrap_or_default();
            // A binary may name the library of its own package.
            let package = packages.iter().find(|p| p.dir == dir);
            if let Some(Package {
                lib_name,
                lib_root: Some(lib),
                ..
            }) = package
                && *lib != root
            {
                libs.insert(lib_name.clone(), lib.clone());
            }
            tree.libs.insert(root.clone(), libs);
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
        tree.published = tree.publish(&packages, files);
        tree
    }

    /// The files reached from a library root through `pub mod` declarations: what a crate
    /// outside the library can name. A binary root exports nothing, and a module declared
    /// inside a module written in place is not followed.
    fn publish(&self, packages: &[Package], files: &BTreeMap<&str, &Facts>) -> BTreeSet<String> {
        let roots = packages.iter().filter_map(|p| p.lib_root.clone());
        let mut out: BTreeSet<String> = BTreeSet::new();
        let mut queue: VecDeque<String> = roots.collect();
        while let Some(file) = queue.pop_front() {
            if !out.insert(file.clone()) {
                continue;
            }
            let facts = files[file.as_str()];
            for ((from, specifier), child) in &self.children {
                if *from != file {
                    continue;
                }
                let name = facts.exports.iter().find_map(|e| match e {
                    Export::From {
                        exported, source, ..
                    } if source == specifier => Some(exported),
                    _ => None,
                });
                if name.is_some_and(|n| !facts.private.contains(n)) {
                    queue.push_back(child.clone());
                }
            }
        }
        out
    }

    /// The files whose `pub` items are the exports of a library: its root and the files
    /// `pub mod` declarations reach from it.
    pub fn published(&self) -> BTreeSet<String> {
        self.published.clone()
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
    /// its file, a crate's name (hyphens as underscores) the root of its library.
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
            name => {
                let root = self.nodes.get(from).map(|n| &n.root);
                root.and_then(|r| self.libs.get(r)?.get(name).cloned())
            }
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

/// Every `Cargo.toml` among `configs` that parses, by directory.
fn tables<'c>(configs: &[(&'c str, &'c [u8])]) -> Vec<(&'c str, toml::Table)> {
    let mut tables: Vec<(&str, toml::Table)> = configs
        .iter()
        .filter_map(|(path, src)| {
            let dir = path.strip_suffix("Cargo.toml")?;
            let dir = dir.strip_suffix('/').unwrap_or(dir);
            Some((dir, std::str::from_utf8(src).ok()?.parse().ok()?))
        })
        .collect();
    tables.sort_by_key(|(dir, _)| *dir);
    tables
}

/// The `Cargo.toml` with a `[package]` table among `configs`, at any depth, by directory.
fn manifests<'c>(configs: &[(&'c str, &'c [u8])]) -> Vec<(&'c str, toml::Table)> {
    let mut manifests = tables(configs);
    manifests.retain(|(_, manifest)| manifest.contains_key("package"));
    manifests
}

/// A package of the repository: where it lives, its `[package] name`, the name a path writes
/// for its library (`[lib] name`, else the package name, hyphens turned into underscores),
/// and the library's root file when the walk found one.
struct Package {
    dir: String,
    name: String,
    lib_name: String,
    lib_root: Option<String>,
}

fn packages(configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Vec<Package> {
    let mut out = Vec::new();
    for (dir, manifest) in manifests(configs) {
        let lib = manifest.get("lib");
        let named = |t: &toml::Value| t.get("name")?.as_str().map(String::from);
        let Some(name) = manifest.get("package").and_then(named) else {
            continue;
        };
        let lib_name = lib.and_then(named).unwrap_or_else(|| name.clone());
        let root = (lib.and_then(|l| l.get("path")?.as_str()))
            .map_or_else(|| join(dir, "src/lib.rs"), |p| join(dir, p));
        out.push(Package {
            dir: dir.to_string(),
            name,
            lib_name: lib_name.replace('-', "_"),
            lib_root: root.filter(|r| files.contains_key(r.as_str())),
        });
    }
    out
}

/// The libraries of the repository each package may name, by package directory: those its
/// `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]` list (also under
/// `[target.*]`), by key or through `package = "..."`, and `workspace = true` entries the
/// nearest `[workspace.dependencies]` names. A dependency is named by the key when it is
/// renamed, else by the name of its library. The first package in path order keeps a name
/// two share.
fn dependencies(
    configs: &[(&str, &[u8])],
    packages: &[Package],
) -> HashMap<String, HashMap<String, String>> {
    let tables = tables(configs);
    let workspace = |dir: &str| {
        let holds = |w: &str| w.is_empty() || dir == w || dir.starts_with(&format!("{w}/"));
        let near = tables
            .iter()
            .filter(|(w, t)| holds(w) && workspace_deps(t).is_some());
        near.max_by_key(|(w, _)| w.len())
            .and_then(|(_, t)| workspace_deps(t))
    };
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (dir, manifest) in &tables {
        if !manifest.contains_key("package") {
            continue;
        }
        let mut lists: Vec<&toml::Table> = Vec::new();
        for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
            lists.extend(manifest.get(kind).and_then(|d| d.as_table()));
            let targets = manifest.get("target").and_then(|t| t.as_table());
            for target in targets.into_iter().flat_map(|t| t.values()) {
                lists.extend(target.get(kind).and_then(|d| d.as_table()));
            }
        }
        for (key, spec) in lists.into_iter().flatten() {
            let inherits = spec.get("workspace").and_then(|w| w.as_bool()) == Some(true);
            let spec = match inherits {
                true => workspace(dir).and_then(|w| w.get(key)).unwrap_or(spec),
                false => spec,
            };
            let renamed = spec.get("package").and_then(|p| p.as_str());
            let package = crate_name(renamed.unwrap_or(key));
            let Some(found) = packages.iter().find(|p| crate_name(&p.name) == package) else {
                continue;
            };
            let Some(root) = &found.lib_root else {
                continue;
            };
            let name = match renamed {
                Some(_) => crate_name(key),
                None => found.lib_name.clone(),
            };
            out.entry(dir.to_string())
                .or_default()
                .entry(name)
                .or_insert_with(|| root.clone());
        }
    }
    out
}

/// The name a path writes for a package: the hyphens turned into underscores.
fn crate_name(name: &str) -> String {
    name.replace('-', "_")
}

/// The `[workspace.dependencies]` table of a manifest.
fn workspace_deps(manifest: &toml::Table) -> Option<&toml::Table> {
    manifest.get("workspace")?.get("dependencies")?.as_table()
}

/// The crate roots, in order: per `Cargo.toml` with a `[package]` table, at any depth, by
/// path, its library (`[lib] path`, else `src/lib.rs`), `src/main.rs`, each `[[bin]] path`,
/// then `src/bin/*.rs` and `src/bin/*/main.rs`; only those the walk found, each with the
/// directory of its manifest and whether it is a binary target (every root but the library).
/// A workspace's members are such manifests too, so `[workspace] members` adds none.
fn roots(configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (dir, manifest) in &manifests(configs) {
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
                out.push((file, dir.to_string(), bin));
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
        let found: Vec<(String, bool)> = (roots(&configs, &files).into_iter())
            .map(|(file, _, bin)| (file, bin))
            .collect();
        assert_eq!(
            found,
            [
                ("backend/src/lib.rs".to_string(), false),
                ("backend/src/main.rs".to_string(), true),
                ("backend/src/bin/sync/main.rs".to_string(), true),
                ("backend/src/bin/tool.rs".to_string(), true),
                ("src-tauri/src/main.rs".to_string(), true),
            ]
        );
    }

    #[test]
    fn a_library_is_reached_by_the_crates_that_depend_on_it() {
        let facts = Facts::default();
        let files = BTreeMap::from([
            ("ledger-core/src/lib.rs", &facts),
            ("ledger-core/src/main.rs", &facts),
            ("app/src/main.rs", &facts),
            ("app/src/bin/tool.rs", &facts),
            ("other/src/x.rs", &facts),
            ("shared/src/lib.rs", &facts),
            ("loose/src/lib.rs", &facts),
        ]);
        let configs: [(&str, &[u8]); 6] = [
            (
                "ledger-core/Cargo.toml",
                b"[package]\nname = \"ledger-core\"\n",
            ),
            (
                "app/Cargo.toml",
                b"[package]\nname = \"app\"\n[dependencies]\nledger-core = { path = \"../ledger-core\" }\n\
                  alias = { package = \"other\", path = \"../other\" }\n\
                  shared = { workspace = true }\nserde = \"1\"\n\
                  [dev-dependencies]\nmissing = { path = \"../missing\" }\n",
            ),
            (
                "other/Cargo.toml",
                b"[package]\nname = \"other\"\n[lib]\nname = \"re-named\"\npath = \"src/x.rs\"\n",
            ),
            ("shared/Cargo.toml", b"[package]\nname = \"shared-lib\"\n"),
            ("loose/Cargo.toml", b"[package]\nname = \"loose\"\n"),
            (
                "Cargo.toml",
                b"[workspace]\nmembers = [\"*\"]\n[workspace.dependencies]\n\
                  shared = { package = \"shared-lib\", path = \"shared\" }\n",
            ),
        ];
        let tree = Tree::new(&configs, &files);
        // A dependency by its key, a renamed one by the key it is renamed to, one a workspace
        // names through `workspace = true`.
        for from in ["app/src/main.rs", "app/src/bin/tool.rs"] {
            assert_eq!(tree.lookup(from, "ledger_core"), ["ledger-core/src/lib.rs"]);
            assert_eq!(tree.lookup(from, "alias"), ["other/src/x.rs"]);
            assert_eq!(tree.lookup(from, "shared"), ["shared/src/lib.rs"]);
            // Not by the package's own name once renamed, nor by one no manifest lists, nor a
            // binary, nor with the hyphen.
            assert!(tree.lookup(from, "other").is_empty());
            assert!(tree.lookup(from, "re_named").is_empty());
            assert!(tree.lookup(from, "loose").is_empty());
            assert!(tree.lookup(from, "ledger-core").is_empty());
            assert!(tree.lookup(from, "serde").is_empty());
        }
        // A binary names the library of its own package, the library itself does not.
        let bin = "ledger-core/src/main.rs";
        assert_eq!(tree.lookup(bin, "ledger_core"), ["ledger-core/src/lib.rs"]);
        assert!(
            tree.lookup("ledger-core/src/lib.rs", "ledger_core")
                .is_empty()
        );
        // A crate that lists nothing reaches nothing.
        assert!(tree.lookup("loose/src/lib.rs", "ledger_core").is_empty());
    }

    #[test]
    fn only_pub_modules_from_a_library_root_are_published() {
        let parse = |src: &str| super::super::parse(src.as_bytes()).unwrap();
        let (root, open, shut, below, main) = (
            parse("pub mod open;\nmod shut;\npub(crate) mod narrow;\n"),
            parse("pub mod below;\npub fn f() {}\n"),
            parse("pub fn g() {}\n"),
            parse("pub fn h() {}\n"),
            parse("mod open;\npub fn main() {}\n"),
        );
        let files = BTreeMap::from([
            ("lib/src/lib.rs", &root),
            ("lib/src/open.rs", &open),
            ("lib/src/open/below.rs", &below),
            ("lib/src/shut.rs", &shut),
            ("lib/src/narrow.rs", &shut),
            ("bin/src/main.rs", &main),
            ("bin/src/open.rs", &open),
        ]);
        let configs: [(&str, &[u8]); 2] = [
            ("lib/Cargo.toml", b"[package]\nname = \"lib\"\n"),
            ("bin/Cargo.toml", b"[package]\nname = \"bin\"\n"),
        ];
        let published = Tree::new(&configs, &files).published();
        let published: Vec<&str> = published.iter().map(String::as_str).collect();
        assert_eq!(
            published,
            ["lib/src/lib.rs", "lib/src/open.rs", "lib/src/open/below.rs"]
        );
    }
}
