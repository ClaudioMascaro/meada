//! Backstage's `$include` and `$env` in `app-config*.yaml` (#689), as its config loader applies
//! them (<https://backstage.io/docs/conf/writing>, the `$include` transform of
//! `@backstage/config-loader`). An object whose only key is `$include` is replaced by the file its
//! value names, resolved against the including file's directory, or by the value a `#` key path
//! (`.`-separated) selects in it; an object whose only key is `$env` becomes `${VAR}`, which the
//! config reader reads as the variable. Only `.json`, `.yaml` and `.yml` files are read, and the
//! included value's children are read the same way from the included file's own directory, so
//! includes nest (the included value itself is not, as in Backstage); a root that is one
//! `$include` is the file it names (#709), as Backstage's loader transforms the root too. A root
//! or a partial of more than one YAML document is unreadable, as Backstage's `yaml.parse` refuses
//! it.
//!
//! An include is not followed when its path is absolute, holds `${`, has another extension or
//! leaves the repository, when the index gave no file for it (a symbolic link, or no file the walk
//! sees), when the file is past `MAX_BYTES` or cannot be read, when its key path is missing, when
//! the object holds another key besides, when it closes a cycle, past `DEPTH`, past `INCLUDES`
//! from one root, or past `MAX_BYTES` grafted into one root or `READING_MAX` into the roots read
//! together (an include counting its file's bytes, or its value's size when aliases make it
//! larger, `Graft::follow`). Its subtree is then unread, so a read
//! under it stays a hole, and `Unfollowed` says which include and why (`meada status`). A key path
//! whose last key alone is absent is named so too, but Backstage leaves its key out rather than
//! failing, so nothing stands there (`Grafted::watched_unread` does not count it). An object
//! holding `$env` with another key is unread as well. `$file` is not read: its object stays as
//! written, and no read reaches a value through it.
// ponytail: a list is not read (the flattened keys leave lists out), so an include inside one is
// neither followed nor named.

use std::collections::{HashMap, HashSet};

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
/// How many bytes of included files one root grafts, each include counting its file again (or its
/// value's size, `Graft::follow`): `MAX_BYTES`, the most one config file reads.
// ponytail: a partial is a few KB; one file's limit bounds a partial grafted from many keys.
const GRAFTED_MAX: usize = MAX_BYTES;
/// How many bytes of included files all the roots of one reading parse together, each include
/// counting its file again (or its value's size, `Graft::follow`), grafted or refused past its
/// root's limit, the roots read in the reader's order (`config::roots`).
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

/// One root read with its includes: the tree, every include not followed, and whether a value
/// left unread sits at a key path the caller watches.
#[derive(Default)]
pub(super) struct Grafted {
    /// The first document of the root with its includes grafted and its `$env` read, `None` when
    /// it is past `MAX_BYTES` (dropped, not parsed by half) or does not parse.
    pub(super) tree: Option<Value>,
    /// The includes not followed on the way.
    pub(super) unfollowed: Vec<Unfollowed>,
    /// Whether a value left unread (an include not followed, an `$env` with another key) sits at
    /// one of the watched key paths: `tree` holds null there though the file writes something.
    pub(super) watched_unread: bool,
}

/// What the roots of one reading spent together: the bytes they parsed, what each file read so
/// far is charged (its bytes, or its value's size once parsed) and the files that did not parse,
/// so an include past a limit, or of a file that does not parse, parses no file already read.
#[derive(Default)]
pub(super) struct Reading {
    grafted: usize,
    charges: HashMap<String, usize>,
    unreadable: HashSet<String>,
}

/// The root `path` (`src`) with its includes grafted and its `$env` read. `read` gives the file at
/// a repository path; `watched` are the key paths, from the root, whose unread values
/// `Grafted::watched_unread` tells; `reading` is what the roots read together spent, this one's
/// added.
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
    let Some(mut root) = document(path, src) else {
        return Grafted::default();
    };
    // Backstage's loader transforms the root as it does any value, so a root that is one
    // `$include` is the file it names; any other root's children are read from it.
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

/// One root's grafting: what it has spent, the files on the way to where it reads, the keys from
/// the root to the value it reads (each key copied once, as it is visited), the includes it did
/// not follow, and whether it left a watched value unread. Where an unread value sits is compared
/// with `watched`, never copied: a crafted file leaves many values unread under long keys.
struct Graft<'r, 'b> {
    read: &'r dyn Fn(&str) -> ConfigInput<'b>,
    includes: usize,
    bytes: usize,
    /// What the roots read together spent.
    reading: &'r mut Reading,
    /// The files and key paths whose children are being read, from the root.
    stack: Vec<(String, String)>,
    unfollowed: Vec<Unfollowed>,
    at: Vec<String>,
    watched: &'r [&'r [&'r str]],
    watched_unread: bool,
}

impl Graft<'_, '_> {
    /// Reads the values of an object (`value`) of `file`, `depth` includes from the root.
    fn children(&mut self, value: &mut Value, file: &str, depth: usize) {
        if let Value::Object(map) = value {
            for (key, child) in map.iter_mut() {
                self.at.push(key.clone());
                self.value(child, file, depth);
                self.at.pop();
            }
        }
    }

    /// Replaces an include or an `$env` object of `file` by what it gives (null when unread),
    /// and reads the children of any other object.
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
                // A key path whose last key is absent gives Backstage `undefined`, and its loader
                // leaves the key out, so nothing stands there (but at the root, where it fails).
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

    /// The value the include `text` of `file`, `depth` includes from the root, gives, with its
    /// children read from the included file's directory, `None` when its key path's last key is
    /// absent; or why it is not followed.
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
        // Backstage never transforms the value an include gives, only its children, so it loops
        // only when a child names a file and key path whose children are being read.
        let read_from = (path.clone(), key.to_string());
        if self.stack.contains(&read_from) {
            return Err(Reason::Cycle);
        }
        if self.includes == INCLUDES {
            return Err(Reason::Count);
        }
        self.includes += 1;
        // A file already read whose charge is past a limit, or any once the reading is spent, is
        // refused unread; one that did not parse is not parsed again.
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
        let Some(document) = document(&path, &src) else {
            self.reading.charges.insert(path.clone(), src.len());
            self.reading.unreadable.insert(path);
            self.charge(src.len());
            return Err(Reason::Unreadable);
        };
        // An alias repeats its anchor's node: a value may graft far more than its file holds.
        let charge = src.len().max(size(&document));
        self.reading.charges.insert(path.clone(), charge);
        if !self.fits(charge) {
            // Parsing it was spent: the roots parse no more than `READING_MAX`, grafted or not.
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

    /// Whether an include charged `charge` fits in its root's `GRAFTED_MAX` (`MAX_BYTES`, the
    /// most one file holds) and the reading's `READING_MAX`.
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

/// Whether `value` is an object Backstage's loader replaces: one holding `$include` or `$env`.
fn is_transform(value: &Value) -> bool {
    matches!(value, Value::Object(map) if map.contains_key("$include") || map.contains_key("$env"))
}

/// The one document of the file `path` (`src`), as Backstage's loader parses it: `yaml.parse`
/// refuses a stream of more than one document. `None` when it does not parse or holds several.
fn document(path: &str, src: &[u8]) -> Option<Value> {
    let mut documents = events::read(Syntax::of(path), src, |_| {}).ok()?;
    (documents.len() <= 1).then(|| documents.pop().unwrap_or(Value::Null))
}

/// The size of `value` as grafted: one for each node, and the bytes of each key and text besides.
/// Without aliases it stays within the bytes of the text it was read from, but for a file of a few
/// bytes or of many `\L` and `\P` escapes.
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

/// Why a key path selects nothing, as Backstage's include transform walks it.
#[derive(Debug, PartialEq)]
enum Selection {
    /// Its last key is absent from an object: `undefined`, a key the loader leaves out.
    Absent,
    /// A step meets no object: the loader fails.
    NoObject,
}

/// The value the `.`-separated `key` path selects in `document`, the whole of it for an empty
/// one.
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

    /// The file at a path of `files`, `Missing` where it holds none.
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
        // The include not followed sits at `proxy.endpoints`, inside the partial `proxy` grafts;
        // the `$env` beside another key at `x`; nothing at `proxy` or `ok`.
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
        // A root that is one `$include` is the file it names, as Backstage transforms the root.
        let files = [
            ("app-config.yaml", "$include: all.yaml\n"),
            ("all.yaml", "a: { $env: A }\nb: 1\n"),
        ];
        let (keys_read, unfollowed) = read("app-config.yaml", &files, &[]);
        assert_eq!(keys_read, keys(&[("a", "${A}"), ("b", "1")]));
        assert!(unfollowed.is_empty(), "{unfollowed:?}");
        // `yaml.parse` refuses a stream of two documents, a partial's or a root's.
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
        // Backstage gives `undefined` for an absent last key and leaves the key out: nothing is
        // unread there. An absent key before the last one fails its loader: unread.
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
        // At the root, `undefined` fails the load: unread.
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
        // Backstage transforms only the children of what an include gives, so a root that is one
        // include of itself, or a `proxy` selecting its own `proxy`, keeps the include as written:
        // nothing loops, nothing is unread.
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
        // A child naming the file whose children are being read loops in Backstage: a cycle.
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

    /// 335 bytes whose aliases expand to a value of 48,139, a node counting one and a key or a
    /// text its bytes besides: `a` is 1 + 10 × (2 + 2) = 41, `b` 1 + 10 × (2 + 41) = 431, `c`
    /// 4,331, `d` 43,331, and the document 1 + 42 + 432 + 4,332 + 43,332.
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
        // `MAX_BYTES` grafted into the root holds 21 bombs, where their bytes would hold all 64.
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
        // A partial that does not parse, named from every key of a root, is read once.
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

        // Four roots each naming 64 distinct bombs of 48,139: a root grafts 21 within
        // `MAX_BYTES`, and the reading parses those past it too, so the first root spends
        // 64 × 48,139 = 3,080,896 of `READING_MAX` (4,194,304). The second parses 24 more
        // (grafting 21) before the rest is spent, and the last two read nothing.
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
        // `$include` as the value an include gives is not applied (at the root of a document it
        // is: `a_root_include_is_followed_and_a_file_of_several_documents_is_unreadable`).
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
