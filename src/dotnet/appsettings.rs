use std::collections::BTreeMap;

use serde_json::Value;

#[cfg(test)]
const JSON_DEPTH: usize = 128;

#[derive(Default)]
pub struct Settings {
    files: BTreeMap<String, serde_json::Map<String, Value>>,
}

impl Settings {
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

    fn above(&self, source: &str) -> Option<&serde_json::Map<String, Value>> {
        let dirs = source.match_indices('/').map(|(at, _)| &source[..=at]);
        (dirs.rev().chain(std::iter::once(""))).find_map(|dir| self.files.get(dir))
    }
}

fn same(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

fn parse(src: &str) -> Option<serde_json::Map<String, Value>> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    match serde_json::from_str(&clean(src)).ok()? {
        Value::Object(root) => Some(root),
        _ => None,
    }
}

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

fn trailing_commas(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let (mut in_string, mut escaped) = (false, false);
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
        assert_eq!(key("api/"), Some("a"));
        assert_eq!(key("api/tools/"), None);
        assert_eq!(key(""), Some("b"));
        assert_eq!(key("loose/deep/C.cs"), Some("a"));
        assert_eq!(key("api/C.cs"), Some("a"));
        assert_eq!(key("other/C.cs"), Some("b"));
    }

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
