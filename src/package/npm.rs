//! The npm ecosystem (spec #22): a `package.json` with a valid name provides
//! `package::npm::<name>`. The adapter answers by npm's own rules:
//! - `read`: the name, version, the four dependency tables and the entry fields (`exports`,
//!   `types`, `module`, `main`) of a `package.json`.
//! - `names_package`, `entries`: a bare specifier splits into its package (scoped or not) and
//!   the subpath, and names the entry of the importing file's nearest `package.json`.
//! - `flag`: npm semver, by the `nodejs-semver` crate behind a comparator grammar check.
//! - `reaches`: a `workspace:` range keeps a dependency inside its own repository.
//! - `shapes`: shapes cross npm packages alone.
//!
//! Everything else is the shared default, Node's rule (`ways`, the candidates, the provider
//! view's `resolve`), which `exported` serves with the `exports` map of a manifest.

use std::path::Path;

use serde_json::Value;

use super::{Ecosystem, Entry, Flag, MAX_BYTES, Providers, Summary, nearest, valid_name};
use crate::index::Export;
use crate::lang::ts::EXTENSIONS;

/// The dependency fields of a manifest that `read` collects.
const FIELDS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

/// The prefix of an npm range that keeps a dependency inside its own repository.
const WORKSPACE: &str = "workspace:";

/// The conditions of an `exports` target Meada reads, in order: at each level the first one
/// present is taken, and no other is followed.
const CONDITIONS: [&str; 4] = ["types", "import", "default", "require"];

/// The deepest an `exports` target nests conditions; deeper, the subpath has no target.
// ponytail: real maps nest two or three levels; this bounds the walk over a hostile one.
const MAX_CONDITIONS: usize = 32;

/// The longest range checked, in bytes; a longer one is `range unchecked`. The crate sets no
/// limit of its own on a range, and a real one is a few dozen bytes.
// ponytail: a flat 256 (the crate's own limit for versions); a measured need lifts it.
const MAX_RANGE: usize = 256;

/// The npm ecosystem.
pub struct Npm;

impl Ecosystem for Npm {
    fn name(&self) -> &'static str {
        "npm"
    }

    fn is_manifest(&self, file_name: &str) -> bool {
        file_name == "package.json"
    }

    /// Reads a `package.json` from its bytes. Hostile input: a file too large, not JSON, nested
    /// past `serde_json`'s depth limit or not an object reads as empty; a field of another type
    /// counts as absent; a name that breaks npm's rules makes no provider.
    fn read(&self, _path: &str, src: &[u8]) -> Summary {
        if src.len() > MAX_BYTES {
            return Summary::default();
        }
        let Ok(Value::Object(root)) = serde_json::from_slice(src) else {
            return Summary::default();
        };
        let text = |v: Option<&Value>| match v {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let mut dependencies = Vec::new();
        for field in FIELDS {
            let Some(Value::Object(table)) = root.get(field) else {
                continue;
            };
            for (name, range) in table {
                if valid_name(name) {
                    dependencies.push(Entry {
                        field: field.to_string(),
                        name: name.clone(),
                        range: text(Some(range)),
                        ..Entry::default()
                    });
                }
            }
        }
        dependencies.sort_by(|a, b| (&a.field, &a.name).cmp(&(&b.field, &b.name)));
        Summary {
            name: text(root.get("name")).filter(|n| valid_name(n)),
            version: text(root.get("version")),
            dependencies,
            exports: root.get("exports").filter(|v| !v.is_null()).cloned(),
            types: text(root.get("types")),
            module: text(root.get("module")),
            main: text(root.get("main")),
            ..Summary::default()
        }
    }

    /// The TS/JS pack's.
    fn sources(&self) -> &'static [&'static str] {
        &EXTENSIONS
    }

    fn names_package(&self, specifier: &str) -> bool {
        split(specifier).is_some()
    }

    /// The entry of the split name in the importing file's nearest `package.json`.
    fn entries<'e>(
        &self,
        exports: &'e [Export],
        _providers: &Providers,
        (n, from): (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)> {
        let Some((name, subpath)) = split(specifier) else {
            return Vec::new();
        };
        let entry = (nearest(&exports[n].manifests, from, self))
            .and_then(|m| m.summary.dependencies.iter().find(|e| e.name == name));
        entry.map(|e| (e, subpath)).into_iter().collect()
    }

    /// Whether the dependency `range` accepts the provider's `version`, by npm's semantics: `None`
    /// when it does, else the `Flag`. A missing range or version, a `workspace:`, `file:`, `link:`
    /// or `npm:` range, a git or URL source (anything holding `:` or `/`), a tag such as `latest`
    /// and a range past `MAX_RANGE` are `Unchecked`. The crate lets garbage through in a range of
    /// several tokens (`foo 1.2.3` parses as `1.2.3`), so each token must also look like a
    /// comparator.
    fn flag(&self, range: Option<&str>, version: Option<&str>) -> Option<Flag> {
        let (Some(range), Some(version)) = (range, version) else {
            return Some(Flag::Unchecked);
        };
        let parsed = (range.len() <= MAX_RANGE && comparators(range))
            .then(|| nodejs_semver::Range::parse(range).ok())
            .flatten()
            .zip(nodejs_semver::Version::parse(version).ok());
        match parsed {
            None => Some(Flag::Unchecked),
            Some((range, version)) if range.satisfies(&version) => None,
            Some(_) => Some(Flag::Mismatch),
        }
    }

    /// A `workspace:` range keeps only the dependent's own repository.
    fn reaches(
        &self,
        (_, range): (Option<&str>, Option<&str>),
        from: (&str, &Path),
        ((to, _), _): ((&str, &Path), &str),
    ) -> bool {
        from.0 == to || !range.is_some_and(|r| r.starts_with(WORKSPACE))
    }

    /// Shapes cross npm packages alone.
    fn shapes(&self) -> bool {
        true
    }
}

/// The target the `exports` field of a manifest gives `subpath` (`.` for the bare name, else
/// `./<rest>`), as written, `*` replaced: exact keys first, then the pattern with one `*` whose
/// prefix is the longest. A string, or an object without subpath keys, is the target of `.`. A
/// `null` target, an array of fallbacks, a key or a target with two `*`, keys that mix subpaths
/// and conditions, or none of `CONDITIONS` present give no target.
pub(super) fn exported(exports: &Value, subpath: &str) -> Option<String> {
    let map = match exports {
        Value::Object(map) if map.keys().any(|k| k.starts_with('.')) => map,
        _ => return (subpath == ".").then(|| target(exports, None, 0))?,
    };
    if !map.keys().all(|k| k.starts_with('.')) {
        return None;
    }
    if let Some(value) = map.get(subpath) {
        return target(value, None, 0);
    }
    let (_, star, value) = (map.iter())
        .filter_map(|(key, value)| {
            let (prefix, suffix) = key.split_once('*')?;
            let star = subpath.strip_prefix(prefix)?.strip_suffix(suffix)?;
            (!suffix.contains('*') && !star.is_empty()).then_some((
                (prefix.len(), key.len()),
                star,
                value,
            ))
        })
        .max_by_key(|(rank, _, _)| *rank)?;
    target(value, Some(star), 0)
}

/// The path a target of `exports` names, through its conditions (see `exported`).
fn target(value: &Value, star: Option<&str>, depth: usize) -> Option<String> {
    match value {
        // One `*` at most, as in a key: the expansion stays the size of the subpath.
        Value::String(t) if t.matches('*').nth(1).is_none() => {
            Some(star.map_or_else(|| t.clone(), |s| t.replace('*', s)))
        }
        Value::Object(conditions) if depth < MAX_CONDITIONS => {
            let taken = CONDITIONS.iter().find_map(|c| conditions.get(*c))?;
            target(taken, star, depth + 1)
        }
        _ => None,
    }
}

/// The package a bare specifier names and the subpath inside it: `@a/b/c` is `@a/b` and
/// `./c`, `x` is `x` and `.`. `None` for a relative or absolute specifier, or a name npm does not
/// accept (`node:fs` among them).
pub(super) fn split(specifier: &str) -> Option<(&str, String)> {
    if specifier.starts_with(['.', '/']) {
        return None;
    }
    let segments = if specifier.starts_with('@') { 2 } else { 1 };
    let end = (specifier.match_indices('/').nth(segments - 1)).map_or(specifier.len(), |(i, _)| i);
    let (name, rest) = specifier.split_at(end);
    (valid_name(name) && (segments == 1 || name.contains('/'))).then(|| (name, format!(".{rest}")))
}

/// Whether every token of `range` looks like part of an npm range: `||` separates sets of at
/// least one token, `-` joins a hyphen range, and a comparator is an optional operator then a
/// version body that begins with a digit, or is a wildcard (`x`, `X`, `*`) alone or followed by
/// `.` (so no `:` or `/` gets through, nor `xyz`).
fn comparators(range: &str) -> bool {
    range.split("||").all(|set| {
        let mut tokens = set.split_whitespace().peekable();
        tokens.peek().is_some()
            && tokens.all(|t| {
                let body = t.trim_start_matches(['^', '~', '<', '>', '=', 'v']);
                t == "-"
                    || body.is_empty()
                    || ((body.starts_with(|c: char| c.is_ascii_digit())
                        || matches!(body, "x" | "X" | "*")
                        || body.starts_with(['x', 'X', '*']) && body[1..].starts_with('.'))
                        && body
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || ".+-*".contains(c)))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::super::MAX_NAME;
    use super::*;

    #[test]
    fn a_workspace_range_reaches_its_own_repository_only() {
        let (a, b) = (("a", Path::new("/w/a")), ("b", Path::new("/w/b")));
        let workspace = (None, Some("workspace:*"));
        assert!(!Npm.reaches(workspace, a, (b, "package.json")));
        assert!(Npm.reaches(workspace, b, (b, "package.json")));
        assert!(Npm.reaches((None, Some("^1")), a, (b, "package.json")));
    }

    #[test]
    fn npm_checks_its_ranges_and_crosses_shapes() {
        assert_eq!(Npm.flag(Some("^2"), Some("1.0.0")), Some(Flag::Mismatch));
        assert_eq!(Npm.flag(Some("^1"), Some("1.0.0")), None);
        assert_eq!(
            Npm.flag(Some("latest"), Some("1.0.0")),
            Some(Flag::Unchecked)
        );
        assert!(Npm.shapes());
    }

    fn entries(s: &Summary) -> Vec<(&str, &str, Option<&str>)> {
        (s.dependencies.iter())
            .map(|e| (e.field.as_str(), e.name.as_str(), e.range.as_deref()))
            .collect()
    }

    const MISMATCH: Option<Flag> = Some(Flag::Mismatch);
    const UNCHECKED: Option<Flag> = Some(Flag::Unchecked);

    #[test]
    fn a_manifest_reads_its_name_version_and_dependency_entries() {
        let src = br#"{"name": "@a/b", "version": "1.0.0",
            "devDependencies": {"z": "^1"}, "dependencies": {"y": "~2", "x": "*"}}"#;
        let s = Npm.read("package.json", src);
        assert_eq!(s.name.as_deref(), Some("@a/b"));
        assert_eq!(s.version.as_deref(), Some("1.0.0"));
        assert_eq!(
            entries(&s),
            [
                ("dependencies", "x", Some("*")),
                ("dependencies", "y", Some("~2")),
                ("devDependencies", "z", Some("^1")),
            ]
        );
    }

    #[test]
    fn a_field_of_another_type_counts_as_absent() {
        let s = Npm.read(
            "package.json",
            br#"{"name": 3, "version": ["1"], "dependencies": {"a": true, "b": null, "c": ""},
                "devDependencies": ["a"], "peerDependencies": "a"}"#,
        );
        assert_eq!((&s.name, &s.version), (&None, &None));
        assert_eq!(
            entries(&s),
            [
                ("dependencies", "a", None),
                ("dependencies", "b", None),
                ("dependencies", "c", None),
            ]
        );
    }

    #[test]
    fn a_name_that_breaks_the_rules_makes_no_provider() {
        let long = "a".repeat(MAX_NAME + 1);
        for name in [long.as_str(), "", "a b", "a\nb", "a\u{7}b", "a::b", "a\tb"] {
            let src = format!(r#"{{"name": {}}}"#, serde_json::to_string(name).unwrap());
            assert_eq!(
                Npm.read("package.json", src.as_bytes()).name,
                None,
                "{name:?}"
            );
        }
        let longest = "a".repeat(MAX_NAME);
        let src = format!(r#"{{"name": "{longest}"}}"#);
        assert_eq!(
            Npm.read("package.json", src.as_bytes()).name.as_deref(),
            Some(longest.as_str())
        );
    }

    #[test]
    fn a_dependency_with_an_invalid_name_is_left_out() {
        let s = Npm.read(
            "package.json",
            br#"{"dependencies": {"a b": "1", "ok": "1"}}"#,
        );
        assert_eq!(entries(&s), [("dependencies", "ok", Some("1"))]);
    }

    #[test]
    fn a_file_too_large_or_not_an_object_reads_as_empty() {
        let pad = " ".repeat(MAX_BYTES);
        let big = format!(r#"{{"name": "a"{pad}}}"#);
        assert_eq!(Npm.read("package.json", big.as_bytes()), Summary::default());
        for src in ["", "not json", "[1]", "\"a\"", "null"] {
            assert_eq!(
                Npm.read("package.json", src.as_bytes()),
                Summary::default(),
                "{src}"
            );
        }
        let at_limit = format!(r#"{{"name": "a"{}}}"#, " ".repeat(MAX_BYTES - 13));
        assert_eq!(at_limit.len(), MAX_BYTES);
        assert_eq!(
            Npm.read("package.json", at_limit.as_bytes())
                .name
                .as_deref(),
            Some("a")
        );
    }

    #[test]
    fn deeply_nested_input_reads_as_empty() {
        let n = 10_000;
        let src = format!(
            r#"{{"name": "a", "x": {}{}}}"#,
            "[".repeat(n),
            "]".repeat(n)
        );
        assert_eq!(Npm.read("package.json", src.as_bytes()), Summary::default());
    }

    #[test]
    fn npm_ranges_are_checked_against_the_local_version() {
        let table = [
            ("^1.0.0", "1.2.0", None),
            ("^1.0.0", "2.0.0", MISMATCH),
            ("^0.2.3", "0.2.9", None),
            ("^0.2.3", "0.3.0", MISMATCH),
            ("~1.1.0", "1.1.9", None),
            ("~1.1.0", "1.2.0", MISMATCH),
            ("1.x", "1.9.0", None),
            ("1.x", "2.0.0", MISMATCH),
            ("*", "7.0.0", None),
            ("1.2.0 - 1.4.0", "1.3.0", None),
            ("1.2.0 - 1.4.0", "1.5.0", MISMATCH),
            ("^1.0.0 || ^3.0.0", "3.1.0", None),
            ("^1.0.0 || ^3.0.0", "2.0.0", MISMATCH),
            (">=1.0.0 <2.0.0", "1.5.0", None),
            (">=1.0.0 <2.0.0", "2.0.0", MISMATCH),
            ("1.2.0", "1.2.0", None),
            ("1.2.0", "1.2.1", MISMATCH),
            // A prerelease matches only a comparator on the same major.minor.patch.
            ("^1.0.0", "1.2.0-beta.1", MISMATCH),
            ("^1.2.0-beta.0", "1.2.0-beta.1", None),
            ("^1.2.0-beta.0", "1.3.0-beta.1", MISMATCH),
        ];
        for (range, version, want) in table {
            assert_eq!(
                Npm.flag(Some(range), Some(version)),
                want,
                "{range} / {version}"
            );
        }
    }

    #[test]
    fn a_range_that_cannot_be_checked_is_unchecked() {
        let ranges = [
            "workspace:*",
            "workspace:^1.0.0",
            "file:../shared",
            "link:../shared",
            "npm:other@^1.0.0",
            "git+https://example.com/a.git",
            "github:user/repo",
            "user/repo",
            "https://example.com/a.tgz",
            "latest",
            "next",
            "foo 1.2.3",
            "xyz 1.2.3",
            "x-foo",
            "1.2.3 foo",
            "^1.0.0 || bar",
            "",
        ];
        for range in ranges {
            assert_eq!(Npm.flag(Some(range), Some("1.2.0")), UNCHECKED, "{range:?}");
        }
        assert_eq!(Npm.flag(None, Some("1.2.0")), UNCHECKED);
        assert_eq!(Npm.flag(Some("^1.0.0"), None), UNCHECKED);
        // A local version that is not a version cannot be compared either.
        assert_eq!(Npm.flag(Some("^1.0.0"), Some("not-a-version")), UNCHECKED);
        let long = "^1.0.0 || ".repeat(MAX_RANGE);
        assert!(long.len() > MAX_RANGE);
        assert_eq!(Npm.flag(Some(&long), Some("1.2.0")), UNCHECKED);
    }

    #[test]
    fn the_exports_map_gives_each_subpath_its_target() {
        let map = serde_json::json!({
            ".": {"types": "./src/index.ts", "require": "./dist/index.cjs"},
            "./format": "./src/format.ts",
            "./conditions": {"node": "./node.ts", "import": "./import.ts", "default": "./d.ts"},
            "./nested": {"default": {"require": "./r.ts", "import": "./i.ts"}},
            "./required": {"require": "./required.ts"},
            "./none": {"node": "./node.ts"},
            "./utils/*": {"import": "./src/utils/*.ts"},
            "./utils/special/*": "./src/special/*.js",
            "./utils/blocked/*": null,
            "./blocked": null,
            "./typed": {"types": null, "default": "./typed.ts"},
            "./fallbacks": ["./a.ts", "./b.ts"],
            "./many/*/x/*": "./many.ts",
            "./two/*": "./src/*/*.ts",
        });
        let cases: [(&str, Option<&str>); 18] = [
            // The first of `types`, `import`, `default`, `require` present is taken.
            (".", Some("./src/index.ts")),
            ("./format", Some("./src/format.ts")),
            ("./conditions", Some("./import.ts")),
            ("./nested", Some("./i.ts")),
            ("./required", Some("./required.ts")),
            // Other conditions are not followed.
            ("./none", None),
            // Exact keys first, then the pattern with the longest prefix; `*` is replaced.
            ("./utils/slug", Some("./src/utils/slug.ts")),
            ("./utils/a/b", Some("./src/utils/a/b.ts")),
            ("./utils/special/x", Some("./src/special/x.js")),
            ("./utils/", None),
            // A `null` target blocks the subpath, even under a condition taken.
            ("./utils/blocked/x", None),
            ("./blocked", None),
            ("./typed", None),
            // Fallback arrays and keys with two `*` are not read.
            ("./fallbacks", None),
            ("./many/a/x/b", None),
            // A target with two `*` is not read either.
            ("./two/x", None),
            ("./missing", None),
            ("./format/more", None),
        ];
        for (subpath, expected) in cases {
            assert_eq!(exported(&map, subpath).as_deref(), expected, "{subpath}");
        }
        // A string or an object of conditions is the target of `.` alone; keys that mix
        // subpaths and conditions make no target.
        let sugar = serde_json::json!("./main.ts");
        assert_eq!(exported(&sugar, ".").as_deref(), Some("./main.ts"));
        assert_eq!(exported(&sugar, "./x"), None);
        let conditions = serde_json::json!({"import": "./i.ts", "require": "./r.ts"});
        assert_eq!(exported(&conditions, ".").as_deref(), Some("./i.ts"));
        let mixed = serde_json::json!({".": "./a.ts", "import": "./b.ts"});
        assert_eq!(exported(&mixed, "."), None);
    }

    #[test]
    fn a_target_of_many_stars_is_not_expanded() {
        let stars = format!("./{}.ts", "*".repeat(1 << 20));
        let map = serde_json::json!({ "./h/*": stars, ".": stars });
        assert_eq!(exported(&map, &format!("./h/{}", "x".repeat(64))), None);
        assert_eq!(exported(&map, "."), None);
    }

    #[test]
    fn conditions_nested_past_the_limit_give_no_target() {
        let nest = |n: usize| {
            let mut v = serde_json::json!("./deep.ts");
            for _ in 0..n {
                v = serde_json::json!({ "default": v });
            }
            v
        };
        assert_eq!(
            exported(&nest(MAX_CONDITIONS), ".").as_deref(),
            Some("./deep.ts")
        );
        assert_eq!(exported(&nest(MAX_CONDITIONS + 1), "."), None);
    }

    #[test]
    fn a_specifier_splits_into_its_package_and_subpath() {
        assert_eq!(split("x"), Some(("x", ".".into())));
        assert_eq!(split("x/a/b"), Some(("x", "./a/b".into())));
        assert_eq!(split("@a/b"), Some(("@a/b", ".".into())));
        assert_eq!(split("@a/b/c"), Some(("@a/b", "./c".into())));
        for none in ["./x", "../x", "/x", "node:fs", "@a", "a b"] {
            assert_eq!(split(none), None, "{none}");
        }
    }

    #[test]
    fn npm_names_a_package_by_the_split_of_a_bare_specifier() {
        for named in ["x", "x/a/b", "@a/b", "@a/b/c"] {
            assert!(Npm.names_package(named), "{named}");
        }
        for none in ["./x", "/x", "node:fs", "@a"] {
            assert!(!Npm.names_package(none), "{none}");
        }
    }
}
