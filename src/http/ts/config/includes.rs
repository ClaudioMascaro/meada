// ponytail: a list is not read (the flattened keys leave lists out), so an include inside one is
// neither followed nor named.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::document::events::{self, Syntax};
use crate::lang::{ConfigInput, MAX_BYTES};

// ponytail: real configurations include once or twice; 8 leaves room and bounds a chain.
pub(crate) const DEPTH: usize = 8;
// ponytail: a root splits into a few partials; 64 bounds an include fanned out to many keys.
pub(crate) const INCLUDES: usize = 64;
// ponytail: a partial is a few KB; one file's limit bounds a partial grafted from many keys.
const GRAFTED_MAX: usize = MAX_BYTES;
// ponytail: a repository's roots graft a few partials of a few KB; four files at their limit bound
// hundreds of small roots each grafting one large partial again (20 took 26 s cold and 10 s on
// every refresh, debug).
const READING_MAX: usize = 4 * MAX_BYTES;

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
    /// A file past `MAX_BYTES`, or past `GRAFTED_MAX` grafted into one root or `READING_MAX` into
    /// the roots read together, its aliases expanded.
    Size,
}

impl Reason {
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

    pub(crate) fn parse(name: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Unfollowed {
    pub(crate) file: String,
    pub(crate) include: String,
    pub(crate) reason: Reason,
}

#[derive(Default)]
pub(super) struct Grafted {
    pub(super) tree: Option<Value>,
    pub(super) unfollowed: Vec<Unfollowed>,
    pub(super) watched_unread: bool,
}

#[derive(Default)]
pub(super) struct Reading {
    grafted: usize,
    charges: HashMap<String, usize>,
    unreadable: HashSet<String>,
}

pub(super) fn tree<'b>(
    path: &str,
    src: &[u8],
    read: &dyn Fn(&str) -> ConfigInput<'b>,
    watched: &[&[&str]],
    reading: &mut Reading,
) -> Grafted {
    let mut graft = Graft {
        read,
        includes: 0,
        bytes: 0,
        reading,
        stack: Vec::new(),
        unfollowed: Vec::new(),
        at: Vec::new(),
        watched,
        watched_unread: false,
    };
    if src.len() > MAX_BYTES {
        return Grafted::default();
    }
    let Ok(mut root) = document(path, src) else {
        return Grafted::default();
    };
    if !is_transform(&root) {
        graft.stack.push((path.to_string(), String::new()));
    }
    graft.value(&mut root, path, 0);
    Grafted {
        tree: Some(root),
        unfollowed: graft.unfollowed,
        watched_unread: graft.watched_unread,
    }
}

struct Graft<'r, 'b> {
    read: &'r dyn Fn(&str) -> ConfigInput<'b>,
    includes: usize,
    bytes: usize,
    reading: &'r mut Reading,
    stack: Vec<(String, String)>,
    unfollowed: Vec<Unfollowed>,
    at: Vec<String>,
    watched: &'r [&'r [&'r str]],
    watched_unread: bool,
}

impl Graft<'_, '_> {
    fn children(&mut self, value: &mut Value, file: &str, depth: usize) {
        if let Value::Object(map) = value {
            for (key, child) in map.iter_mut() {
                self.at.push(key.clone());
                self.value(child, file, depth);
                self.at.pop();
            }
        }
    }

    fn value(&mut self, value: &mut Value, file: &str, depth: usize) {
        if !is_transform(value) {
            return self.children(value, file, depth);
        }
        let Value::Object(map) = value else {
            return;
        };
        let include = map.get("$include").map(|v| match v {
            Value::String(text) => Ok(text.clone()),
            other => Err(other.to_string()),
        });
        let read = match include {
            _ if map.len() > 1 => {
                if let Some(written) = include {
                    self.refuse(file, written.unwrap_or_else(|e| e), Reason::Sibling);
                }
                None
            }
            None => match &map["$env"] {
                Value::String(var) => Some(Value::String(format!("${{{var}}}"))),
                _ => None,
            },
            Some(Err(written)) => {
                self.refuse(file, written, Reason::NotPath);
                None
            }
            Some(Ok(text)) => match self.follow(&text, file, depth + 1) {
                Ok(Some(tree)) => Some(tree),
                Ok(None) => {
                    self.refuse(file, text, Reason::KeyPath);
                    (!self.at.is_empty()).then_some(Value::Null)
                }
                Err(reason) => {
                    self.refuse(file, text, reason);
                    None
                }
            },
        };
        *value = read.unwrap_or_else(|| {
            self.watched_unread |= (self.watched.iter()).any(|w| w.iter().eq(self.at.iter()));
            Value::Null
        });
    }

    fn follow(&mut self, text: &str, file: &str, depth: usize) -> Result<Option<Value>, Reason> {
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
        let read_from = (path.clone(), key.to_string());
        if self.stack.contains(&read_from) {
            return Err(Reason::Cycle);
        }
        if self.includes == INCLUDES {
            return Err(Reason::Count);
        }
        self.includes += 1;
        let charge = self.reading.charges.get(&path).copied();
        if !self.fits(charge.unwrap_or(0)) {
            return Err(Reason::Size);
        }
        if self.reading.unreadable.contains(&path) {
            self.charge(charge.unwrap_or(0));
            return Err(Reason::Unreadable);
        }
        let src = match (self.read)(&path) {
            ConfigInput::Read(src) => src,
            ConfigInput::Symlink => return Err(Reason::Symlink),
            ConfigInput::Missing => return Err(Reason::Missing),
        };
        if !self.fits(src.len()) {
            self.reading.charges.insert(path, src.len());
            return Err(Reason::Size);
        }
        let document = match document(&path, &src) {
            Ok(document) => document,
            Err(spent) => {
                self.reading.charges.insert(path.clone(), spent);
                self.reading.unreadable.insert(path);
                match self.fits(spent) {
                    true => self.charge(spent),
                    false => self.reading.grafted += spent,
                }
                return Err(Reason::Unreadable);
            }
        };
        let charge = src.len().max(size(&document));
        self.reading.charges.insert(path.clone(), charge);
        if !self.fits(charge) {
            self.reading.grafted += charge;
            return Err(Reason::Size);
        }
        self.charge(charge);
        let mut picked = match selected(document, key) {
            Ok(picked) => picked,
            Err(Selection::Absent) => return Ok(None),
            Err(Selection::NoObject) => return Err(Reason::KeyPath),
        };
        self.stack.push(read_from);
        self.children(&mut picked, &path, depth);
        self.stack.pop();
        Ok(Some(picked))
    }

    fn fits(&self, charge: usize) -> bool {
        self.bytes + charge <= GRAFTED_MAX && self.reading.grafted + charge <= READING_MAX
    }

    fn charge(&mut self, charge: usize) {
        self.bytes += charge;
        self.reading.grafted += charge;
    }

    fn refuse(&mut self, file: &str, include: String, reason: Reason) {
        self.unfollowed.push(Unfollowed {
            file: file.to_string(),
            include,
            reason,
        });
    }
}

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

fn is_transform(value: &Value) -> bool {
    matches!(value, Value::Object(map) if map.contains_key("$include") || map.contains_key("$env"))
}

fn document(path: &str, src: &[u8]) -> Result<Value, usize> {
    let mut documents = events::read(Syntax::of(path), src, |_| {}).map_err(|_| src.len())?;
    if documents.len() > 1 {
        return Err(src.len().max(documents.iter().map(size).sum()));
    }
    Ok(documents.pop().unwrap_or(Value::Null))
}

fn size(value: &Value) -> usize {
    let mut size = 0;
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        size += 1;
        match value {
            Value::String(text) => size += text.len(),
            Value::Array(items) => pending.extend(items),
            Value::Object(map) => {
                for (key, value) in map {
                    size += key.len();
                    pending.push(value);
                }
            }
            _ => {}
        }
    }
    size
}

#[derive(Debug, PartialEq)]
enum Selection {
    Absent,
    NoObject,
}

fn selected(document: Value, key: &str) -> Result<Value, Selection> {
    if key.is_empty() {
        return Ok(document);
    }
    let mut value = document;
    let mut steps = key.split('.').peekable();
    while let Some(step) = steps.next() {
        let Value::Object(mut map) = value else {
            return Err(Selection::NoObject);
        };
        value = match map.remove(step) {
            Some(value) => value,
            None if steps.peek().is_none() => return Err(Selection::Absent),
            None => return Err(Selection::NoObject),
        };
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::jvm::spring::flatten;

    type Keys = BTreeMap<String, String>;
    type Refused = (String, String, &'static str);

    fn read(root: &str, files: &[(&str, &str)], links: &[&str]) -> (Keys, Vec<Refused>) {
        let files: BTreeMap<&str, &str> = files.iter().copied().collect();
        let fetch = |path: &str| match files.get(path) {
            Some(src) => ConfigInput::Read(src.as_bytes().into()),
            None if links.contains(&path) => ConfigInput::Symlink,
            None => ConfigInput::Missing,
        };
        let Grafted {
            tree, unfollowed, ..
        } = tree(
            root,
            files[root].as_bytes(),
            &fetch,
            &[],
            &mut Reading::default(),
        );
        let keys = tree.as_ref().map(flatten).unwrap_or_default();
        let unfollowed = (unfollowed.into_iter())
            .map(|u| (u.file, u.include, u.reason.name()))
            .collect();
        (keys, unfollowed)
    }

    fn fetcher<'f>(files: &'f BTreeMap<&str, &str>) -> impl Fn(&str) -> ConfigInput<'f> + 'f {
        move |path| match files.get(path) {
            Some(src) => ConfigInput::Read(src.as_bytes().into()),
            None => ConfigInput::Missing,
        }
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
    fn a_value_left_unread_at_a_watched_key_path_is_told() {
        let files = [
            (
                "app-config.yaml",
                "ok: { $env: B }\nproxy: { $include: p.yaml }\nx: { $env: A, b: 1 }\n",
            ),
            (
                "p.yaml",
                "endpoints: { $include: gone.yaml }\nkept: { $include: q.yaml }\n",
            ),
            ("q.yaml", "v: 1\n"),
        ];
        let files: BTreeMap<&str, &str> = files.iter().copied().collect();
        let fetch = fetcher(&files);
        let root = files["app-config.yaml"].as_bytes();
        let told = |watched: &[&[&str]]| {
            tree(
                "app-config.yaml",
                root,
                &fetch,
                watched,
                &mut Reading::default(),
            )
        };
        assert!(told(&[&["proxy", "endpoints"]]).watched_unread);
        assert!(told(&[&["x"]]).watched_unread);
        assert!(!told(&[&["proxy"], &["ok"], &["proxy", "kept"], &["endpoints"]]).watched_unread);
        let grafted = told(&[]);
        let keys = grafted.tree.as_ref().map(flatten).unwrap_or_default();
        assert_eq!(keys, self::keys(&[("ok", "${B}"), ("proxy.kept.v", "1")]));
    }

    #[test]
    fn a_root_include_is_followed_and_a_file_of_several_documents_is_unreadable() {
        let files = [
            ("app-config.yaml", "$include: all.yaml\n"),
            ("all.yaml", "a: { $env: A }\nb: 1\n"),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read, keys(&[("a", "${A}"), ("b", "1")]));
        assert!(unfollowed.is_empty(), "{unfollowed:?}");
        let files = [
            ("app-config.yaml", "a: { $include: two.yaml }\nb: 1\n"),
            ("two.yaml", "x: 1\n---\ny: 2\n"),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read, keys(&[("b", "1")]));
        let reasons: Vec<_> = unfollowed.iter().map(|u| u.2).collect();
        assert_eq!(reasons, ["unreadable"]);
        let files = [("app-config.yaml", "b: 1\n---\nc: 2\n")];
        let (keys_read, _) = read("app-config.yaml", &files, &[]);
        assert!(keys_read.is_empty(), "{keys_read:?}");
    }

    #[test]
    fn a_key_path_whose_last_key_is_absent_leaves_nothing_there_but_is_named() {
        let files = [
            (
                "app-config.yaml",
                "last: { $include: p.yaml#a.nope }\nmid: { $include: p.yaml#nope.b }\n",
            ),
            ("p.yaml", "a: { other: 1 }\n"),
        ];
        let files: BTreeMap<&str, &str> = files.iter().copied().collect();
        let fetch = fetcher(&files);
        let root = files["app-config.yaml"].as_bytes();
        let told = |watched: &[&[&str]]| {
            tree(
                "app-config.yaml",
                root,
                &fetch,
                watched,
                &mut Reading::default(),
            )
        };
        assert!(!told(&[&["last"]]).watched_unread);
        assert!(told(&[&["mid"]]).watched_unread);
        let reasons: Vec<_> = (told(&[]).unfollowed.iter())
            .map(|u| (u.include.clone(), u.reason.name()))
            .collect();
        let named = [
            ("p.yaml#a.nope".to_string(), "key-path"),
            ("p.yaml#nope.b".to_string(), "key-path"),
        ];
        assert_eq!(reasons, named);
        let files = [
            ("app-config.yaml", "$include: q.yaml#nope\n"),
            ("q.yaml", "a: 1\n"),
        ];
        let files: BTreeMap<&str, &str> = files.iter().copied().collect();
        let fetch = fetcher(&files);
        let grafted = tree(
            "app-config.yaml",
            files["app-config.yaml"].as_bytes(),
            &fetch,
            &[&[]],
            &mut Reading::default(),
        );
        assert!(grafted.watched_unread);
    }

    #[test]
    fn a_file_that_includes_itself_keeps_the_include_its_own_value_holds() {
        let watched: &[&[&str]] = &[&[], &["proxy"]];
        for (root, kept) in [
            (
                "$include: app-config.yaml\n",
                ("$include", "app-config.yaml"),
            ),
            (
                "proxy: { $include: app-config.yaml#proxy }\n",
                ("proxy.$include", "app-config.yaml#proxy"),
            ),
        ] {
            let files: BTreeMap<&str, &str> = [("app-config.yaml", root)].into_iter().collect();
            let fetch = fetcher(&files);
            let grafted = tree(
                "app-config.yaml",
                root.as_bytes(),
                &fetch,
                watched,
                &mut Reading::default(),
            );
            let keys_read = grafted.tree.as_ref().map(flatten).unwrap_or_default();
            assert_eq!(keys_read, keys(&[kept]), "{root}");
            assert!(
                grafted.unfollowed.is_empty() && !grafted.watched_unread,
                "{root}"
            );
        }
        let root = "proxy: { $include: app-config.yaml }\n";
        let files: BTreeMap<&str, &str> = [("app-config.yaml", root)].into_iter().collect();
        let fetch = fetcher(&files);
        let grafted = tree(
            "app-config.yaml",
            root.as_bytes(),
            &fetch,
            watched,
            &mut Reading::default(),
        );
        let reasons: Vec<_> = grafted.unfollowed.iter().map(|u| u.reason.name()).collect();
        assert_eq!((reasons, grafted.watched_unread), (vec!["cycle"], true));
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

    fn bomb() -> String {
        let level = |name: &str, anchor: &str, item: &str| {
            let items: Vec<String> = (0..10).map(|i| format!("k{i}: {item}")).collect();
            format!("{name}: {anchor}{{{}}}\n", items.join(", "))
        };
        [
            level("a", "&a ", "x"),
            level("b", "&b ", "*a"),
            level("c", "&c ", "*b"),
            level("d", "", "*c"),
        ]
        .concat()
    }

    #[test]
    fn an_include_is_charged_the_size_its_aliases_expand_it_to() {
        let bomb = bomb();
        assert_eq!(bomb.len(), 335);
        let root: String = (0..INCLUDES)
            .map(|i| format!("k{i:02}: {{ $include: bomb.yaml }}\n"))
            .collect();
        let files = [
            ("app-config.yaml", root.as_str()),
            ("bomb.yaml", bomb.as_str()),
        ];
        let (_, unfollowed) = read("app-config.yaml", &files, &[]);
        let size = (
            "app-config.yaml".to_string(),
            "bomb.yaml".to_string(),
            "size",
        );
        assert_eq!(unfollowed, vec![size; INCLUDES - 21]);
    }

    #[test]
    fn a_reading_parses_a_file_that_does_not_parse_once_and_parses_no_more_than_its_limit() {
        let reads = std::cell::Cell::new(0);
        let root: String = (0..INCLUDES)
            .map(|i| format!("k{i:02}: {{ $include: bad.yaml }}\n"))
            .collect();
        let files = BTreeMap::from([("app-config.yaml", root.as_str()), ("bad.yaml", "a: [\n")]);
        let grafted = tree(
            "app-config.yaml",
            root.as_bytes(),
            &|path| {
                reads.set(reads.get() + 1);
                fetcher(&files)(path)
            },
            &[],
            &mut Reading::default(),
        );
        assert_eq!(reads.get(), 1);
        let reasons = grafted.unfollowed.iter().map(|u| u.reason);
        assert_eq!(reasons.collect::<Vec<_>>(), [Reason::Unreadable; INCLUDES]);

        let bomb = bomb();
        let roots: Vec<(String, String)> = (0..4)
            .map(|r| {
                let root = (0..INCLUDES)
                    .map(|i| format!("k{i:02}: {{ $include: p{i:02}.yaml }}\n"))
                    .collect();
                (format!("r{r}/app-config.yaml"), root)
            })
            .collect();
        let partials: Vec<String> = (0..4)
            .flat_map(|r| (0..INCLUDES).map(move |i| format!("r{r}/p{i:02}.yaml")))
            .collect();
        let files: BTreeMap<&str, &str> = (partials.iter())
            .map(|p| (p.as_str(), bomb.as_str()))
            .collect();
        let mut reading = Reading::default();
        reads.set(0);
        let grafts: Vec<usize> = (roots.iter())
            .map(|(path, root)| {
                let fetch = |p: &str| {
                    reads.set(reads.get() + 1);
                    fetcher(&files)(p)
                };
                let grafted = tree(path, root.as_bytes(), &fetch, &[], &mut reading);
                let tree = grafted.tree.unwrap();
                (tree.as_object().unwrap().values())
                    .filter(|v| !v.is_null())
                    .count()
            })
            .collect();
        assert_eq!(grafts, [21, 21, 0, 0]);
        assert_eq!(reads.get(), INCLUDES + 24);

        let unparsed = format!("{bomb}\n---\n");
        let files: BTreeMap<&str, &str> = (partials.iter())
            .map(|p| (p.as_str(), unparsed.as_str()))
            .collect();
        let mut reading = Reading::default();
        reads.set(0);
        for (path, root) in &roots {
            let fetch = |p: &str| {
                reads.set(reads.get() + 1);
                fetcher(&files)(p)
            };
            tree(path, root.as_bytes(), &fetch, &[], &mut reading);
        }
        assert_eq!(reads.get(), INCLUDES + 24);
    }

    #[test]
    fn a_root_past_the_limit_is_dropped_and_an_included_value_is_never_an_include() {
        let huge = format!(
            "a: {{ $include: p.yaml }}\npad: '{}'\n",
            "x".repeat(MAX_BYTES)
        );
        let files = [("app-config.yaml", huge.as_str()), ("p.yaml", "url: u\n")];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert!(keys_read.is_empty() && unfollowed.is_empty());
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
