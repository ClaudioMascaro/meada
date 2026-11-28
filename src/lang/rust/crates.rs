use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::lang::{Export, Facts};

pub const MOD: &str = "mod ";
pub const PATH: &str = "path ";

struct Node {
    root: String,
    parent: Option<String>,
    owns_dir: bool,
}

pub struct Tree {
    nodes: BTreeMap<String, Node>,
    libs: HashMap<String, HashMap<String, String>>,
    // ponytail: one set per crate root, quadratic in a workspace's crates; share the sets per
    // package if a workspace of thousands of crates shows up in profiles.
    reach: HashMap<String, BTreeSet<String>>,
    bins: BTreeSet<String>,
    children: HashMap<(String, String), String>,
    published: BTreeSet<String>,
}

impl Tree {
    pub fn new(configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Tree {
        let mut tree = Tree {
            nodes: BTreeMap::new(),
            libs: HashMap::new(),
            reach: HashMap::new(),
            bins: BTreeSet::new(),
            children: HashMap::new(),
            published: BTreeSet::new(),
        };
        let tables = tables(configs);
        let packages = packages(&tables, files);
        let normal = dependencies(&tables, &packages, &["dependencies"]);
        let dependencies = dependencies(&tables, &packages, &KINDS);
        let mut queue = VecDeque::new();
        for (root, dir, bin) in roots(&tables, files) {
            if bin {
                tree.bins.insert(root.clone());
            }
            let node = Node {
                root: root.clone(),
                parent: None,
                owns_dir: true,
            };
            let mut libs = dependencies.get(&dir).cloned().unwrap_or_default();
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
        let uses: HashMap<&str, &HashMap<String, String>> = (packages.iter())
            .filter_map(|p| Some((p.lib_root.as_deref()?, normal.get(&p.dir)?)))
            .collect();
        for (root, libs) in &tree.libs {
            let mut reach = BTreeSet::new();
            let mut next: Vec<&String> = libs.values().collect();
            while let Some(lib) = next.pop() {
                if reach.insert(lib.clone()) {
                    next.extend(uses.get(lib.as_str()).into_iter().flat_map(|d| d.values()));
                }
            }
            tree.reach.insert(root.clone(), reach);
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

    pub fn published(&self) -> BTreeSet<String> {
        self.published.clone()
    }

    pub fn members(&self) -> BTreeSet<String> {
        self.nodes.keys().cloned().collect()
    }

    pub fn bins(&self) -> impl Iterator<Item = &str> {
        self.bins.iter().map(String::as_str)
    }

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

    pub fn reaches(&self, from: &str, to: &str) -> bool {
        let root = |file: &str| self.nodes.get(file).map(|n| &n.root);
        match (root(from), root(to)) {
            (Some(from), Some(to)) => {
                from == to || self.reach.get(from).is_some_and(|r| r.contains(to))
            }
            _ => false,
        }
    }
}

fn declared(
    file: &str,
    owns_dir: bool,
    specifier: &str,
    files: &BTreeMap<&str, &Facts>,
) -> Option<(String, bool, bool)> {
    let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
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
        let (child, owns) = candidates.into_iter().find(|(c, _)| built(files, c))?;
        return Some((child, owns, top));
    }
    let (chain, value) = specifier.strip_prefix(PATH)?.split_once('|')?;
    let child = match chain {
        "" => join(dir, value)?,
        chain => join(&join(&base, chain)?, value)?,
    };
    built(files, &child).then_some((child, true, chain.is_empty()))
}

fn built(files: &BTreeMap<&str, &Facts>, file: &str) -> bool {
    files.get(file).is_some_and(|f| !f.unbuilt)
}

pub fn join(dir: &str, rel: &str) -> Option<String> {
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

fn tables<'c>(configs: &[(&'c str, &'c [u8])]) -> Vec<(&'c str, toml::Table)> {
    let mut tables: Vec<(&str, toml::Table)> = configs
        .iter()
        .filter(|(_, src)| src.len() <= crate::lang::MAX_BYTES)
        .filter_map(|(path, src)| {
            let dir = path.strip_suffix("Cargo.toml")?;
            let dir = dir.strip_suffix('/').unwrap_or(dir);
            Some((dir, std::str::from_utf8(src).ok()?.parse().ok()?))
        })
        .collect();
    tables.sort_by_key(|(dir, _)| *dir);
    tables
}

fn manifests<'t, 'c>(
    tables: &'t [(&'c str, toml::Table)],
) -> impl Iterator<Item = (&'c str, &'t toml::Table)> {
    let packages = tables.iter().filter(|(_, t)| t.contains_key("package"));
    packages.map(|(dir, t)| (*dir, t))
}

pub fn features(configs: &[(&str, &[u8])]) -> Vec<(String, String)> {
    (manifests(&tables(configs)))
        .map(|(dir, manifest)| {
            let on: Vec<String> = (super::cfg::defaults(manifest).into_iter())
                .filter(|f| !f.chars().any(char::is_control))
                .collect();
            (dir.to_string(), on.join(","))
        })
        .collect()
}

struct Package {
    dir: String,
    name: String,
    lib_name: String,
    lib_root: Option<String>,
}

fn packages(tables: &[(&str, toml::Table)], files: &BTreeMap<&str, &Facts>) -> Vec<Package> {
    let mut out = Vec::new();
    for (dir, manifest) in manifests(tables) {
        let Some(name) = (manifest.get("package")).and_then(|p| p.get("name")?.as_str()) else {
            continue;
        };
        out.push(Package {
            dir: dir.to_string(),
            name: name.to_string(),
            lib_name: lib_name(manifest, name),
            lib_root: lib_root(manifest, dir).filter(|r| files.contains_key(r.as_str())),
        });
    }
    out
}

pub fn lib_name(manifest: &toml::Table, name: &str) -> String {
    let named = manifest.get("lib").and_then(|l| l.get("name")?.as_str());
    crate_name(named.unwrap_or(name))
}

pub fn lib_root(manifest: &toml::Table, dir: &str) -> Option<String> {
    let path = manifest.get("lib").and_then(|l| l.get("path")?.as_str());
    join(dir, path.unwrap_or("src/lib.rs"))
}

const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

fn dependencies(
    tables: &[(&str, toml::Table)],
    packages: &[Package],
    kinds: &[&str],
) -> HashMap<String, HashMap<String, String>> {
    let workspace = |dir: &str| {
        let holds = |w: &str| w.is_empty() || dir == w || dir.starts_with(&format!("{w}/"));
        let near = tables
            .iter()
            .filter(|(w, t)| holds(w) && workspace_deps(t).is_some());
        near.max_by_key(|(w, _)| w.len())
            .and_then(|(_, t)| workspace_deps(t))
    };
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (dir, manifest) in manifests(tables) {
        let listed = entries(manifest).into_iter();
        for (_, key, spec) in listed.filter(|(kind, ..)| kinds.contains(kind)) {
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

pub fn entries(manifest: &toml::Table) -> Vec<(&'static str, &str, &toml::Value)> {
    let mut found = Vec::new();
    let targets = manifest.get("target").and_then(|t| t.as_table());
    for kind in KINDS {
        let plain = manifest.get(kind).and_then(|d| d.as_table());
        let nested = targets.into_iter().flat_map(|t| t.values());
        let lists = plain
            .into_iter()
            .chain(nested.filter_map(|t| t.get(kind)?.as_table()));
        for (key, spec) in lists.flatten() {
            found.push((kind, key.as_str(), spec));
        }
    }
    found
}

pub fn crate_name(name: &str) -> String {
    name.replace('-', "_")
}

pub fn workspace_deps(manifest: &toml::Table) -> Option<&toml::Table> {
    manifest.get("workspace")?.get("dependencies")?.as_table()
}

fn roots(
    tables: &[(&str, toml::Table)],
    files: &BTreeMap<&str, &Facts>,
) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (dir, manifest) in manifests(tables) {
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
            if built(files, &file) && seen.insert(file.clone()) {
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
    fn a_manifest_past_the_size_limit_is_no_crate() {
        let facts = Facts::default();
        let files = BTreeMap::from([("a/src/lib.rs", &facts), ("b/src/lib.rs", &facts)]);
        let head = "[package]\nname = \"a\"\n#";
        let at = format!("{head}{}", " ".repeat(crate::lang::MAX_BYTES - head.len()));
        let past = format!(
            "[package]\nname = \"b\"\n{}",
            "#".repeat(crate::lang::MAX_BYTES)
        );
        let past = &past.as_bytes()[..crate::lang::MAX_BYTES + 1];
        let configs: [(&str, &[u8]); 2] = [("a/Cargo.toml", at.as_bytes()), ("b/Cargo.toml", past)];
        let found: Vec<String> = (roots(&tables(&configs), &files).into_iter())
            .map(|(file, _, _)| file)
            .collect();
        assert_eq!(found, ["a/src/lib.rs"]);
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
            (
                "backend/Cargo.toml",
                b"[package]\nname = \"b\"\n[lib]\nproc-macro = true\n",
            ),
            ("src-tauri/Cargo.toml", b"[package]\nname = \"t\"\n"),
            ("virtual/Cargo.toml", b"[workspace]\nmembers = []\n"),
        ];
        let found: Vec<(String, bool)> = (roots(&tables(&configs), &files).into_iter())
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
        for from in ["app/src/main.rs", "app/src/bin/tool.rs"] {
            assert_eq!(tree.lookup(from, "ledger_core"), ["ledger-core/src/lib.rs"]);
            assert_eq!(tree.lookup(from, "alias"), ["other/src/x.rs"]);
            assert_eq!(tree.lookup(from, "shared"), ["shared/src/lib.rs"]);
            assert!(tree.lookup(from, "other").is_empty());
            assert!(tree.lookup(from, "re_named").is_empty());
            assert!(tree.lookup(from, "loose").is_empty());
            assert!(tree.lookup(from, "ledger-core").is_empty());
            assert!(tree.lookup(from, "serde").is_empty());
        }
        let bin = "ledger-core/src/main.rs";
        assert_eq!(tree.lookup(bin, "ledger_core"), ["ledger-core/src/lib.rs"]);
        assert!(
            tree.lookup("ledger-core/src/lib.rs", "ledger_core")
                .is_empty()
        );
        assert!(tree.lookup("loose/src/lib.rs", "ledger_core").is_empty());
    }

    #[test]
    fn a_crate_reaches_its_dependencies_and_theirs_but_not_their_dev_dependencies() {
        let facts = Facts::default();
        let files = BTreeMap::from([
            ("a/src/lib.rs", &facts),
            ("a/src/main.rs", &facts),
            ("b/src/lib.rs", &facts),
            ("c/src/lib.rs", &facts),
            ("d/src/lib.rs", &facts),
            ("t/src/lib.rs", &facts),
            ("u/src/lib.rs", &facts),
        ]);
        let configs: [(&str, &[u8]); 6] = [
            (
                "a/Cargo.toml",
                b"[package]\nname = \"a\"\n[dependencies]\nb = { path = \"../b\" }\n\
                  [dev-dependencies]\nt = { path = \"../t\" }\n",
            ),
            (
                "b/Cargo.toml",
                b"[package]\nname = \"b\"\n[dependencies]\nc = { path = \"../c\" }\n\
                  [dev-dependencies]\nd = { path = \"../d\" }\n",
            ),
            ("c/Cargo.toml", b"[package]\nname = \"c\"\n"),
            ("d/Cargo.toml", b"[package]\nname = \"d\"\n"),
            ("t/Cargo.toml", b"[package]\nname = \"t\"\n"),
            ("u/Cargo.toml", b"[package]\nname = \"u\"\n"),
        ];
        let tree = Tree::new(&configs, &files);
        let reached = |from: &str| -> Vec<&str> {
            let all = files.keys().copied();
            all.filter(|to| tree.reaches(from, to)).collect()
        };
        assert_eq!(
            reached("a/src/lib.rs"),
            [
                "a/src/lib.rs",
                "b/src/lib.rs",
                "c/src/lib.rs",
                "t/src/lib.rs"
            ]
        );
        assert_eq!(
            reached("a/src/main.rs"),
            [
                "a/src/lib.rs",
                "a/src/main.rs",
                "b/src/lib.rs",
                "c/src/lib.rs",
                "t/src/lib.rs"
            ]
        );
        assert_eq!(
            reached("b/src/lib.rs"),
            ["b/src/lib.rs", "c/src/lib.rs", "d/src/lib.rs"]
        );
        assert_eq!(reached("u/src/lib.rs"), ["u/src/lib.rs"]);
        assert!(!tree.reaches("x/src/lib.rs", "x/src/lib.rs"));
        assert!(!tree.reaches("a/src/lib.rs", "x/src/lib.rs"));
    }

    #[test]
    fn only_pub_modules_from_a_library_root_are_published() {
        let parse = |src: &str| super::super::parse(src.as_bytes(), "").unwrap();
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
