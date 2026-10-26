//! .NET configuration (spec #28, #112), shared by the C# contract kinds: the base
//! `appsettings.json` of a project, read for the sources of that project (the nearest directory
//! at or above a source that holds a `.csproj`, else the nearest one that holds an
//! `appsettings.json`). The file is JSON that .NET reads with comments (`//`, `/* */`) and trailing
//! commas, which this reader accepts too; its nested objects join with `:` (`"A": {"B": "x"}` is
//! the key `A:B`), and keys compare without case, as .NET compares them. Only string values are
//! read: a number, a boolean, a null and an array are left out. Environment-specific files
//! (`appsettings.<Environment>.json`), user secrets and the other providers are not read. Nothing
//! read here is kept in the index: the bytes come from the pack's config files on every refresh,
//! and a value lives only while the contracts are extracted. Meada reads no environment value.

use std::collections::{BTreeMap, BTreeSet};

/// The string values of one project's configuration, by lower-cased key (`a:b`).
pub type Properties = BTreeMap<String, String>;

/// The configurations of a repository's projects, by the project's directory (`api/`, or empty
/// for the repository root).
#[derive(Default)]
pub struct Settings {
    projects: BTreeSet<String>,
    files: BTreeMap<String, Properties>,
}

impl Settings {
    /// The configurations among `configs` (repository path and bytes, as `Indexed::configs`
    /// lists them): the `appsettings.json` files, and the `.csproj` files that say where a
    /// project begins. A file past `lang::MAX_BYTES`, which the index reads cut, is not read.
    pub fn read(configs: &[(&str, &[u8])]) -> Self {
        let mut found = Self::default();
        for &(path, src) in configs {
            let (dir, name) = path.rsplit_once('/').map_or(("", path), |(d, n)| (d, n));
            let dir = if dir.is_empty() {
                String::new()
            } else {
                format!("{dir}/")
            };
            if name.ends_with(".csproj") {
                found.projects.insert(dir);
            } else if name == "appsettings.json"
                && src.len() <= crate::lang::MAX_BYTES
                && let Some(read) = std::str::from_utf8(src).ok().and_then(parse)
            {
                found.files.insert(dir, read);
            }
        }
        found
    }

    /// The configuration of the project whose sources hold `source`.
    pub fn of(&self, source: &str) -> Option<&Properties> {
        // The directories at or above the source, nearest first.
        let dirs: Vec<&str> = (source.match_indices('/').map(|(at, _)| &source[..=at]))
            .rev()
            .chain(std::iter::once(""))
            .collect();
        let dir = (dirs.iter().find(|d| self.projects.contains(**d)))
            .or_else(|| dirs.iter().find(|d| self.files.contains_key(**d)))?;
        self.files.get(*dir)
    }
}

/// The string values of a JSON text as .NET reads it, or `None` when it is no JSON object.
fn parse(src: &str) -> Option<Properties> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let serde_json::Value::Object(root) = serde_json::from_str(&clean(src)).ok()? else {
        return None;
    };
    let mut out = Properties::new();
    // serde_json bounds the nesting it parses; the walk is an explicit stack.
    let mut pending: Vec<(String, serde_json::Value)> = root
        .into_iter()
        .map(|(k, v)| (k.to_lowercase(), v))
        .collect();
    while let Some((key, value)) = pending.pop() {
        match value {
            serde_json::Value::Object(map) => {
                pending.extend(
                    map.into_iter()
                        .map(|(k, v)| (format!("{key}:{}", k.to_lowercase()), v)),
                );
            }
            serde_json::Value::String(s) => {
                out.insert(key, s);
            }
            _ => {}
        }
    }
    Some(out)
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

    fn props(pairs: &[(&str, &str)]) -> Properties {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn comments_trailing_commas_and_a_byte_order_mark_are_read_and_keys_join_with_colons() {
        let src = "\u{feff}{\n  // a line comment with \"quotes\" and a , comma\n\
                   \"Api\": { \"Url\": \"http://api.example.com/\", /* a block */ \"Port\": 80, },\n\
                   \"Mixed\": \"has // and /* inside, } and ,]\",\n\
                   \"List\": [\"a\", \"b\",],\n  \"Null\": null,\n  \"Esc\": \"q\\\"//\",\n}\n";
        assert_eq!(
            parse(src).unwrap(),
            props(&[
                ("api:url", "http://api.example.com/"),
                ("esc", "q\"//"),
                ("mixed", "has // and /* inside, } and ,]"),
            ])
        );
    }

    #[test]
    fn keys_are_lower_cased_and_what_is_no_object_is_not_read() {
        assert_eq!(
            parse(r#"{"A": {"B": "x"}}"#).unwrap(),
            props(&[("a:b", "x")])
        );
        assert!(parse("[1, 2]").is_none());
        assert!(parse("{ unclosed").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn a_source_reads_the_nearest_project_and_only_its_base_file() {
        let a = br#"{"K": "a"}"#.as_slice();
        let b = br#"{"K": "b"}"#.as_slice();
        let settings = Settings::read(&[
            ("api/Api.csproj", b""),
            ("api/appsettings.json", a),
            ("api/appsettings.Development.json", b),
            ("api/tools/Tools.csproj", b""),
            ("appsettings.json", b),
            ("loose/appsettings.json", a),
        ]);
        let key = |source| settings.of(source).and_then(|p| p.get("k")).cloned();
        assert_eq!(key("api/Clients/C.cs"), Some("a".into()));
        // A project without a file of its own has no configuration, whatever lies above it.
        assert_eq!(key("api/tools/C.cs"), None);
        assert_eq!(key("C.cs"), Some("b".into()));
        // No project above: the nearest directory with a file.
        assert_eq!(key("loose/deep/C.cs"), Some("a".into()));
    }
}
