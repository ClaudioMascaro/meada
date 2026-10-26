//! .NET configuration (spec #28, #112), shared by the C# contract kinds: the base
//! `appsettings.json` of a project, read for the sources of that project (`Facts::unit`, the
//! project directory the pack's `join` gives a file; a file in no project reads the nearest
//! `appsettings.json` above it). The file is JSON that .NET reads with comments (`//`, `/* */`)
//! and trailing commas, which this reader accepts too. A key `A:B` is looked up by descending the
//! nested objects (`"A": {"B": "x"}`), each segment without case, as .NET compares them; the
//! parsed document is kept as it is and never flattened, so a hostile one costs what it weighs.
//! A JSON key that holds a `:` itself is not matched. Only string values are read: a number, a
//! boolean, a null and an array are left out. Environment-specific files
//! (`appsettings.<Environment>.json`), user secrets and the other providers are not read. Nothing
//! read here is kept in the index: the bytes come from the pack's config files on every refresh,
//! and a value lives only while the contracts are extracted. Meada reads no environment value.

use std::collections::BTreeMap;

use serde_json::Value;

/// How deep a document may nest: `serde_json`'s own recursion limit, past which the file is not
/// read (the key is then the variable alone).
#[cfg(test)]
const JSON_DEPTH: usize = 128;

/// The configurations of a repository's projects, by the project's directory (`api/`, or empty
/// for the repository root).
#[derive(Default)]
pub struct Settings {
    files: BTreeMap<String, serde_json::Map<String, Value>>,
}

impl Settings {
    /// The configurations among `configs` (repository path and bytes, as `Indexed::configs`
    /// lists them): the `appsettings.json` files. A file past `lang::MAX_BYTES`, which the
    /// index reads cut, is not read.
    pub fn read(configs: &[(&str, &[u8])]) -> Self {
        let mut found = Self::default();
        for &(path, src) in configs {
            let (dir, name) = path.rsplit_once('/').map_or(("", path), |(d, n)| (d, n));
            if name == "appsettings.json"
                && src.len() <= crate::lang::MAX_BYTES
                && let Some(read) = std::str::from_utf8(src).ok().and_then(parse)
            {
                let dir = if dir.is_empty() {
                    String::new()
                } else {
                    format!("{dir}/")
                };
                found.files.insert(dir, read);
            }
        }
        found
    }

    /// The string `key` (`A:B`) holds in the configuration of the project `unit` (a directory,
    /// or the path of a source in no project).
    pub fn get(&self, unit: &str, key: &str) -> Option<&str> {
        let own = (unit.is_empty() || unit.ends_with('/')).then(|| self.files.get(unit));
        let root = own.unwrap_or_else(|| self.above(unit))?;
        let mut node: Option<&Value> = None;
        let mut children = Some(root);
        for segment in key.split(':') {
            let found = children?.iter().find(|(k, _)| same(k, segment));
            node = found.map(|(_, v)| v);
            children = node.and_then(Value::as_object);
        }
        node?.as_str()
    }

    /// The configuration of the nearest directory above `source` that has one.
    fn above(&self, source: &str) -> Option<&serde_json::Map<String, Value>> {
        let dirs = source.match_indices('/').map(|(at, _)| &source[..=at]);
        (dirs.rev().chain(std::iter::once(""))).find_map(|dir| self.files.get(dir))
    }
}

/// Whether two keys are the same without case.
fn same(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// The root object of a JSON text as .NET reads it, or `None` when it is no JSON object.
fn parse(src: &str) -> Option<serde_json::Map<String, Value>> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    match serde_json::from_str(&clean(src)).ok()? {
        Value::Object(root) => Some(root),
        _ => None,
    }
}

/// `src` without its comments and without the commas that precede a closing bracket, both
/// outside strings.
fn clean(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                while let Some(s) = chars.next() {
                    out.push(s);
                    match s {
                        '\\' => out.extend(chars.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for s in chars.by_ref() {
                    if s == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for s in chars.by_ref() {
                    if last == '*' && s == '/' {
                        break;
                    }
                    last = s;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    trailing_commas(&out)
}

/// `src` without the commas that precede a `}` or `]`, outside strings.
fn trailing_commas(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let (mut in_string, mut escaped) = (false, false);
    // A comma and the whitespace after it, not yet written.
    let mut held = String::new();
    for c in src.chars() {
        if in_string {
            in_string = !(c == '"' && !escaped);
            escaped = c == '\\' && !escaped;
            out.push(c);
            continue;
        }
        if c == ',' {
            out.push_str(&held);
            held = String::from(',');
            continue;
        }
        if c.is_whitespace() && !held.is_empty() {
            held.push(c);
            continue;
        }
        let closing = matches!(c, '}' | ']');
        out.push_str(if closing {
            &held[held.len().min(1)..]
        } else {
            &held
        });
        held.clear();
        in_string = c == '"';
        out.push(c);
    }
    out.push_str(&held);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(json: &str) -> Settings {
        Settings::read(&[("appsettings.json", json.as_bytes())])
    }

    #[test]
    fn comments_trailing_commas_and_a_byte_order_mark_are_read_and_keys_descend_with_colons() {
        let src = "\u{feff}{\n  // a line comment with \"quotes\" and a , comma\n\
                   \"Api\": { \"Url\": \"http://api.example.com/\", /* a block */ \"Port\": 80, },\n\
                   \"Mixed\": \"has // and /* inside, } and ,]\",\n\
                   \"List\": [\"a\", \"b\",],\n  \"Null\": null,\n  \"Esc\": \"q\\\"//\",\n}\n";
        let found = settings(src);
        let get = |key| found.get("", key);
        assert_eq!(get("Api:Url"), Some("http://api.example.com/"));
        assert_eq!(get("esc"), Some("q\"//"));
        assert_eq!(get("MIXED"), Some("has // and /* inside, } and ,]"));
        // Not a string, not there, or a section.
        for key in [
            "Api:Port",
            "List",
            "Null",
            "Api",
            "Api:Url:More",
            "Nope:Url",
            "",
        ] {
            assert_eq!(get(key), None, "{key}");
        }
    }

    #[test]
    fn keys_compare_without_case_at_every_level_and_what_is_no_object_is_not_read() {
        assert_eq!(settings(r#"{"A": {"B": "x"}}"#).get("", "a:b"), Some("x"));
        assert_eq!(settings(r#"{"a": {"B": "x"}}"#).get("", "A:b"), Some("x"));
        assert_eq!(settings(r#"{"\u00c9": "x"}"#).get("", "\u{e9}"), Some("x"));
        for src in ["[1, 2]", "{ unclosed", ""] {
            assert_eq!(settings(src).get("", "A"), None, "{src}");
        }
    }

    #[test]
    fn a_source_reads_the_file_of_its_project_alone_and_a_loose_one_the_nearest_above() {
        let a = br#"{"K": "a"}"#.as_slice();
        let b = br#"{"K": "b"}"#.as_slice();
        let found = Settings::read(&[
            ("api/appsettings.json", a),
            ("api/appsettings.Development.json", b),
            ("appsettings.json", b),
            ("loose/appsettings.json", a),
        ]);
        let key = |unit| found.get(unit, "k");
        // A project (its directory is the unit) reads its own file, whatever lies above it.
        assert_eq!(key("api/"), Some("a"));
        assert_eq!(key("api/tools/"), None);
        assert_eq!(key(""), Some("b"));
        // A source in no project (the unit is its path) reads the nearest file above it.
        assert_eq!(key("loose/deep/C.cs"), Some("a"));
        assert_eq!(key("api/C.cs"), Some("a"));
        assert_eq!(key("other/C.cs"), Some("b"));
    }

    /// The document is kept as parsed, not flattened to full keys: a long key with many children
    /// costs what the document weighs, not its length times its children.
    #[test]
    fn a_hostile_document_costs_what_it_weighs() {
        let long = "k".repeat(400_000);
        let children: String = (0..8_000).map(|n| format!("\"{n}\":\"\",")).collect();
        let src = format!("{{\"{long}\": {{{children}}}, \"A\": {{\"B\": \"x\"}}}}");
        assert!(src.len() <= crate::lang::MAX_BYTES);
        let start = std::time::Instant::now();
        let found = settings(&src);
        assert_eq!(found.get("", "A:B"), Some("x"));
        assert_eq!(found.get("", &format!("{long}:7")), Some(""));
        assert_eq!(found.get("", &format!("{long}:nope")), None);
        assert!(start.elapsed().as_secs() < 2, "{:?}", start.elapsed());
    }

    #[test]
    fn a_document_nested_past_the_depth_limit_is_not_read() {
        let nested = |n: usize| {
            let (open, close) = ("{\"a\":".repeat(n), "}".repeat(n));
            format!("{open}\"x\"{close}")
        };
        let key = vec!["a"; JSON_DEPTH - 1].join(":");
        assert_eq!(settings(&nested(JSON_DEPTH - 1)).get("", &key), Some("x"));
        assert_eq!(settings(&nested(JSON_DEPTH + 1)).get("", "a"), None);
        assert!(parse(&nested(100_000)).is_none());
    }
}
