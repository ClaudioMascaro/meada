//! Backstage's `$include` and `$env` in `app-config*.yaml` (#689), as its config loader applies
//! them (<https://backstage.io/docs/conf/writing>, the `$include` transform of
//! `@backstage/config-loader`). An object whose only key is `$include` is replaced by the file its
//! value names, resolved against the including file's directory, or by the value a `#` key path
//! (`.`-separated) selects in it; an object whose only key is `$env` becomes `${VAR}`, which the
//! config reader reads as the variable. Only `.json`, `.yaml` and `.yml` files are read, and the
//! included value's children are read the same way from the included file's own directory, so
//! includes nest (the included value itself and the document's root are not, as in Backstage).
//!
//! An include is not followed when its path is absolute, holds `${`, has another extension or
//! leaves the repository, when the index gave no file for it (a symbolic link, or no file the walk
//! sees), when the file is past `MAX_BYTES` or cannot be read, when its key path is missing, when
//! the object holds another key besides, when it closes a cycle, past `DEPTH`, past `INCLUDES`
//! from one root, or past `MAX_BYTES` grafted into one root. Its subtree is then unread, so a read
//! under it stays a hole, and `Unfollowed` says which include and why (`meada status`). An object
//! holding `$env` with another key is unread as well. `$file` is not read: its object stays as
//! written, and no read reaches a value through it.
// ponytail: a list is not read (the flattened keys leave lists out), so an include inside one is
// neither followed nor named.

use serde_json::Value;

use crate::document::events::{self, Syntax};
use crate::lang::{ConfigInput, MAX_BYTES};

/// How deep includes nest: an include in a file eight includes away from its root is not
/// followed.
// ponytail: real configurations include once or twice; 8 leaves room and bounds a chain.
pub(crate) const DEPTH: usize = 8;
/// How many includes one root follows, all files it reaches counted; the next is not followed.
// ponytail: a root splits into a few partials; 64 bounds an include fanned out to many keys.
pub(crate) const INCLUDES: usize = 64;
/// How many bytes of included files one root grafts, each include counting its file again:
/// `MAX_BYTES`, the most one config file reads.
// ponytail: a partial is a few KB; one file's limit bounds a partial grafted from many keys.
const GRAFTED_MAX: usize = MAX_BYTES;

/// Why an include is not followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// An absolute path.
    Absolute,
    /// A path that leaves the repository.
    Outside,
    /// A `${` in the path.
    Variable,
    /// An extension other than `.json`, `.yaml` and `.yml`.
    Extension,
    /// A symbolic link on the way to the file.
    Symlink,
    /// No file the walk sees there.
    Missing,
    /// A file that does not parse.
    Unreadable,
    /// A key path the file does not hold.
    KeyPath,
    /// Another key beside `$include`.
    Sibling,
    /// A value that is not text.
    NotPath,
    /// A file already on the way from the root.
    Cycle,
    /// Past `DEPTH`.
    Depth,
    /// Past `INCLUDES` from one root.
    Count,
    /// A file past `MAX_BYTES`, or past `GRAFTED_MAX` grafted into one root.
    Size,
}

impl Reason {
    /// Every reason, in the order `name` lists them.
    pub(crate) const ALL: [Reason; 14] = [
        Reason::Absolute,
        Reason::Outside,
        Reason::Variable,
        Reason::Extension,
        Reason::Symlink,
        Reason::Missing,
        Reason::Unreadable,
        Reason::KeyPath,
        Reason::Sibling,
        Reason::NotPath,
        Reason::Cycle,
        Reason::Depth,
        Reason::Count,
        Reason::Size,
    ];

    /// The name the index stores (the `method` of an `unfollowed` row).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Reason::Absolute => "absolute",
            Reason::Outside => "outside",
            Reason::Variable => "variable",
            Reason::Extension => "extension",
            Reason::Symlink => "symlink",
            Reason::Missing => "missing",
            Reason::Unreadable => "unreadable",
            Reason::KeyPath => "key-path",
            Reason::Sibling => "sibling",
            Reason::NotPath => "not-a-path",
            Reason::Cycle => "cycle",
            Reason::Depth => "depth",
            Reason::Count => "count",
            Reason::Size => "size",
        }
    }

    /// The reason `name` gives.
    pub(crate) fn parse(name: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.name() == name)
    }
}

/// An include not followed: the file it is written in, the include as written (its text, or the
/// JSON of a value that is no text) and why.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Unfollowed {
    pub(crate) file: String,
    pub(crate) include: String,
    pub(crate) reason: Reason,
}

/// The first document of the root `path` (`src`) with its includes grafted and its `$env` read,
/// `None` when it is past `MAX_BYTES` (dropped, not parsed by half) or does not parse; and every
/// include not followed on the way. `read` gives the file at a repository path.
pub(super) fn tree<'b>(
    path: &str,
    src: &[u8],
    read: &dyn Fn(&str) -> ConfigInput<'b>,
) -> (Option<Value>, Vec<Unfollowed>) {
    let mut graft = Graft {
        read,
        includes: 0,
        bytes: 0,
        stack: vec![path.to_string()],
        unfollowed: Vec::new(),
    };
    if src.len() > MAX_BYTES {
        return (None, Vec::new());
    }
    let Ok(mut root) = events::first(Syntax::of(path), src) else {
        return (None, Vec::new());
    };
    graft.children(&mut root, path, 0);
    (Some(root), graft.unfollowed)
}

/// One root's grafting: what it has spent, the files on the way to where it reads, and the
/// includes it did not follow.
struct Graft<'r, 'b> {
    read: &'r dyn Fn(&str) -> ConfigInput<'b>,
    includes: usize,
    bytes: usize,
    stack: Vec<String>,
    unfollowed: Vec<Unfollowed>,
}

impl Graft<'_, '_> {
    /// Reads the values of an object (`value`) of `file`, `depth` includes from the root.
    fn children(&mut self, value: &mut Value, file: &str, depth: usize) {
        if let Value::Object(map) = value {
            for child in map.values_mut() {
                self.value(child, file, depth);
            }
        }
    }

    /// Replaces an include or an `$env` object of `file` by what it gives (null when unread),
    /// and reads the children of any other object.
    fn value(&mut self, value: &mut Value, file: &str, depth: usize) {
        let Value::Object(map) = value else {
            return;
        };
        if !map.contains_key("$include") && !map.contains_key("$env") {
            return self.children(value, file, depth);
        }
        let include = map.get("$include").map(|v| match v {
            Value::String(text) => Ok(text.clone()),
            other => Err(other.to_string()),
        });
        let read = match include {
            _ if map.len() > 1 => {
                if let Some(written) = include {
                    self.refuse(file, written.unwrap_or_else(|e| e), Reason::Sibling);
                }
                Value::Null
            }
            None => match &map["$env"] {
                Value::String(var) => Value::String(format!("${{{var}}}")),
                _ => Value::Null,
            },
            Some(Err(written)) => {
                self.refuse(file, written, Reason::NotPath);
                Value::Null
            }
            Some(Ok(text)) => match self.follow(&text, file, depth + 1) {
                Ok(tree) => tree,
                Err(reason) => {
                    self.refuse(file, text, reason);
                    Value::Null
                }
            },
        };
        *value = read;
    }

    /// The value the include `text` of `file`, `depth` includes from the root, gives, with its
    /// children read from the included file's directory; or why it is not followed.
    fn follow(&mut self, text: &str, file: &str, depth: usize) -> Result<Value, Reason> {
        if text.contains("${") {
            return Err(Reason::Variable);
        }
        let (target, key) = text.split_once('#').unwrap_or((text, ""));
        if target.starts_with('/') {
            return Err(Reason::Absolute);
        }
        if ![".json", ".yaml", ".yml"]
            .iter()
            .any(|e| target.ends_with(e))
        {
            return Err(Reason::Extension);
        }
        let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
        let path = joined(dir, target).ok_or(Reason::Outside)?;
        if depth > DEPTH {
            return Err(Reason::Depth);
        }
        if self.stack.contains(&path) {
            return Err(Reason::Cycle);
        }
        if self.includes == INCLUDES {
            return Err(Reason::Count);
        }
        self.includes += 1;
        let src = match (self.read)(&path) {
            ConfigInput::Read(src) => src,
            ConfigInput::Symlink => return Err(Reason::Symlink),
            ConfigInput::Missing => return Err(Reason::Missing),
        };
        if src.len() > MAX_BYTES || self.bytes + src.len() > GRAFTED_MAX {
            return Err(Reason::Size);
        }
        self.bytes += src.len();
        let document = events::first(Syntax::of(&path), &src).map_err(|_| Reason::Unreadable)?;
        let mut picked = selected(document, key).ok_or(Reason::KeyPath)?;
        self.stack.push(path.clone());
        self.children(&mut picked, &path, depth);
        self.stack.pop();
        Ok(picked)
    }

    fn refuse(&mut self, file: &str, include: String, reason: Reason) {
        self.unfollowed.push(Unfollowed {
            file: file.to_string(),
            include,
            reason,
        });
    }
}

/// `target` resolved against the directory `dir` (repository-relative, `""` at the root),
/// normalized lexically; `None` when it leaves the repository.
fn joined(dir: &str, target: &str) -> Option<String> {
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    Some(segments.join("/"))
}

/// The value the `.`-separated `key` path selects in `document`, the whole of it for an empty
/// one.
fn selected(document: Value, key: &str) -> Option<Value> {
    if key.is_empty() {
        return Some(document);
    }
    let mut value = document;
    for step in key.split('.') {
        value = match value {
            Value::Object(mut map) => map.remove(step)?,
            _ => return None,
        };
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::jvm::spring::flatten;

    type Keys = BTreeMap<String, String>;
    /// An include not followed: its file, the include and the reason's name.
    type Refused = (String, String, &'static str);

    /// The flattened keys of the root `root` among `files`, and the includes not followed as
    /// `(file, include, reason)`. A path listed in `links` is a symbolic link.
    fn read(root: &str, files: &[(&str, &str)], links: &[&str]) -> (Keys, Vec<Refused>) {
        let files: BTreeMap<&str, &str> = files.iter().copied().collect();
        let fetch = |path: &str| match files.get(path) {
            Some(src) => ConfigInput::Read(src.as_bytes().into()),
            None if links.contains(&path) => ConfigInput::Symlink,
            None => ConfigInput::Missing,
        };
        let (tree, unfollowed) = tree(root, files[root].as_bytes(), &fetch);
        let keys = tree.as_ref().map(flatten).unwrap_or_default();
        let unfollowed = (unfollowed.into_iter())
            .map(|u| (u.file, u.include, u.reason.name()))
            .collect();
        (keys, unfollowed)
    }

    fn keys(pairs: &[(&str, &str)]) -> Keys {
        (pairs.iter())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn an_include_resolves_against_the_including_files_directory() {
        let files = [
            (
                "packages/app/app-config.yaml",
                "a: { $include: ./partials/a.yaml }\nb: { $include: ../shared/b.json#x.y }\n",
            ),
            ("packages/app/partials/a.yaml", "url: ${A_URL}\n"),
            (
                "packages/shared/b.json",
                r#"{"x": {"y": {"url": "${B_URL}"}}}"#,
            ),
        ];
        let (keys_read, unfollowed) = read("packages/app/app-config.yaml", &files, &[]);
        assert_eq!(
            keys_read,
            keys(&[("a.url", "${A_URL}"), ("b.url", "${B_URL}")])
        );
        assert!(unfollowed.is_empty(), "{unfollowed:?}");
    }

    #[test]
    fn a_key_path_selects_a_subtree_and_a_missing_one_leaves_it_unread() {
        let files = [
            (
                "app-config.yaml",
                "a: { $include: p.yaml#api }\nb: { $include: p.yaml#api.url }\nc: { $include: p.yaml#nope }\nd: { $include: p.yaml#api.url.more }\n",
            ),
            ("p.yaml", "api:\n  url: ${API_URL}\n"),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert_eq!(
            keys_read,
            keys(&[("a.url", "${API_URL}"), ("b", "${API_URL}")])
        );
        assert_eq!(
            unfollowed,
            [
                ("app-config.yaml".into(), "p.yaml#nope".into(), "key-path"),
                (
                    "app-config.yaml".into(),
                    "p.yaml#api.url.more".into(),
                    "key-path"
                ),
            ]
        );
    }

    #[test]
    fn env_is_the_variable_and_a_sibling_key_makes_an_object_unread() {
        let files = [
            (
                "app-config.yaml",
                "a: { $env: A_URL }\nb: { $env: B_URL, x: 1 }\nc: { $include: p.yaml, x: 1 }\nd: { $file: ./secret.txt }\ne: { $include: 3 }\n",
            ),
            ("p.yaml", "url: u\n"),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        // `$file` stays as written: no read of `d` reaches its value.
        assert_eq!(
            keys_read,
            keys(&[("a", "${A_URL}"), ("d.$file", "./secret.txt")])
        );
        assert_eq!(
            unfollowed,
            [
                ("app-config.yaml".into(), "p.yaml".into(), "sibling"),
                ("app-config.yaml".into(), "3".into(), "not-a-path"),
            ]
        );
    }

    #[test]
    fn includes_nest_from_their_own_directory() {
        let files = [
            ("app-config.yaml", "a: { $include: one/a.yaml }\n"),
            ("one/a.yaml", "b: { $include: two/b.yaml }\n"),
            ("one/two/b.yaml", "c: { $include: ../../c.yaml }\n"),
            ("c.yaml", "url: ${C_URL}\n"),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read, keys(&[("a.b.c.url", "${C_URL}")]));
        assert!(unfollowed.is_empty(), "{unfollowed:?}");
    }

    #[test]
    fn a_path_that_escapes_the_root_or_is_not_plain_is_not_followed() {
        let files = [(
            "config/app-config.yaml",
            "a: { $include: ../../up.yaml }\nb: { $include: /etc/x.yaml }\nc: { $include: './${DIR}/x.yaml' }\nd: { $include: x.txt }\ne: { $include: x.YAML }\nf: { $include: ../config/../ok.yaml }\n",
        )];
        let (_, unfollowed) = read("config/app-config.yaml", &files, &[]);
        let file = "config/app-config.yaml".to_string();
        assert_eq!(
            unfollowed,
            [
                (file.clone(), "../../up.yaml".into(), "outside"),
                (file.clone(), "/etc/x.yaml".into(), "absolute"),
                (file.clone(), "./${DIR}/x.yaml".into(), "variable"),
                (file.clone(), "x.txt".into(), "extension"),
                (file.clone(), "x.YAML".into(), "extension"),
                // Inside the repository once normalized, but not there.
                (file, "../config/../ok.yaml".into(), "missing"),
            ]
        );
        assert_eq!(
            joined("config", "../config/../ok.yaml").as_deref(),
            Some("ok.yaml")
        );
        assert_eq!(joined("", "./a//b/./c.yaml").as_deref(), Some("a/b/c.yaml"));
        assert_eq!(joined("a", "../../b.yaml"), None);
    }

    #[test]
    fn a_symlink_a_missing_and_an_unreadable_file_are_not_followed() {
        let files = [
            (
                "app-config.yaml",
                "a: { $include: link.yaml }\nb: { $include: gone.yaml }\nc: { $include: bad.yaml }\n",
            ),
            ("bad.yaml", "a: [unclosed\n"),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &["link.yaml"]);
        assert!(keys_read.is_empty(), "{keys_read:?}");
        let reasons: Vec<_> = unfollowed.iter().map(|u| u.2).collect();
        assert_eq!(reasons, ["symlink", "missing", "unreadable"]);
    }

    #[test]
    fn a_cycle_is_cut_where_it_closes() {
        let files = [
            ("app-config.yaml", "a: { $include: a.yaml }\nok: 1\n"),
            ("a.yaml", "url: u\nb: { $include: b.yaml }\n"),
            (
                "b.yaml",
                "back: { $include: a.yaml }\nroot: { $include: app-config.yaml }\n",
            ),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read, keys(&[("a.url", "u"), ("ok", "1")]));
        assert_eq!(
            unfollowed,
            [
                ("b.yaml".into(), "a.yaml".into(), "cycle"),
                ("b.yaml".into(), "app-config.yaml".into(), "cycle"),
            ]
        );
        // The same file twice side by side is no cycle.
        let files = [
            (
                "app-config.yaml",
                "a: { $include: p.yaml }\nb: { $include: p.yaml }\n",
            ),
            ("p.yaml", "url: u\n"),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read, keys(&[("a.url", "u"), ("b.url", "u")]));
        assert!(unfollowed.is_empty());
    }

    #[test]
    fn a_chain_stops_past_the_depth() {
        // `c0.yaml` includes `c1.yaml` and so on: the root's include is the first.
        let mut files: Vec<(String, String)> = vec![(
            "app-config.yaml".into(),
            "k: { $include: c1.yaml }\n".into(),
        )];
        for i in 1..=DEPTH + 2 {
            files.push((
                format!("c{i}.yaml"),
                format!("v{i}: {i}\nk: {{ $include: c{}.yaml }}\n", i + 1),
            ));
        }
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, s)| (p.as_str(), s.as_str()))
            .collect();
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        let deepest = format!("{}v{DEPTH}", "k.".repeat(DEPTH));
        assert_eq!(
            keys_read.get(&deepest).map(String::as_str),
            Some(DEPTH.to_string().as_str())
        );
        assert_eq!(keys_read.len(), DEPTH);
        let file = format!("c{DEPTH}.yaml");
        assert_eq!(
            unfollowed,
            [(file, format!("c{}.yaml", DEPTH + 1), "depth")]
        );
    }

    #[test]
    fn a_root_follows_so_many_includes_and_grafts_so_many_bytes() {
        let mut root = String::new();
        for i in 0..=INCLUDES {
            root += &format!("k{i:03}: {{ $include: p.yaml }}\n");
        }
        let files = [("app-config.yaml", root.as_str()), ("p.yaml", "url: u\n")];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read.len(), INCLUDES);
        assert_eq!(
            unfollowed,
            [("app-config.yaml".into(), "p.yaml".into(), "count")]
        );

        // Two thirds of the limit fit once, not twice; a file past it never.
        let big = format!("url: u\npad: '{}'\n", "x".repeat(MAX_BYTES * 2 / 3));
        let huge = format!("url: u\npad: '{}'\n", "x".repeat(MAX_BYTES));
        let files = [
            (
                "app-config.yaml",
                "a: { $include: big.yaml }\nb: { $include: big.yaml }\nc: { $include: huge.yaml }\n",
            ),
            ("big.yaml", big.as_str()),
            ("huge.yaml", huge.as_str()),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read.get("a.url").map(String::as_str), Some("u"));
        assert!(!keys_read.contains_key("b.url"));
        let reasons: Vec<_> = unfollowed.iter().map(|u| (u.1.as_str(), u.2)).collect();
        assert_eq!(reasons, [("big.yaml", "size"), ("huge.yaml", "size")]);
    }

    #[test]
    fn a_root_past_the_limit_is_dropped_and_its_root_is_never_an_include() {
        let huge = format!(
            "a: {{ $include: p.yaml }}\npad: '{}'\n",
            "x".repeat(MAX_BYTES)
        );
        let files = [("app-config.yaml", huge.as_str()), ("p.yaml", "url: u\n")];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert!(keys_read.is_empty() && unfollowed.is_empty());
        // `$include` at the root of a document, or as the value an include gives, is not applied.
        let files = [
            ("app-config.yaml", "$include: p.yaml\n"),
            ("p.yaml", "url: u\n"),
        ];
        let (keys_read, _) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read, keys(&[("$include", "p.yaml")]));
        let files = [
            ("app-config.yaml", "a: { $include: p.yaml#inner }\n"),
            ("p.yaml", "inner: { $include: q.yaml }\n"),
            ("q.yaml", "url: u\n"),
        ];
        let (keys_read, _) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read, keys(&[("a.$include", "q.yaml")]));
    }

    #[test]
    fn every_reason_has_a_name_it_parses_back_from() {
        for reason in Reason::ALL {
            assert_eq!(Reason::parse(reason.name()), Some(reason));
        }
        assert_eq!(Reason::parse("nope"), None);
    }
}
