//! Spring Boot configuration (spec #26), shared by the JVM packs' contract kinds: the base
//! `application.properties` and `application.yml` (or `.yaml`) of a module's
//! `src/main/resources`, read for the sources of that module (those under a main source root its
//! Maven or Gradle build declares, else under `<module>/src/main/`), and the placeholders a
//! value holds.
//! `.properties` files are read line by line for literal assignments; YAML (`.yml`, `.yaml`)
//! through the hardened reader of the provider documents, its nested keys joined with dots.
//! Where a module has several, `.properties` wins, then `.yml`, as in Spring. Profiles
//! (`application-<profile>` files, a document that activates on one, the sections after a
//! `.properties` file's `#---`) and relaxed binding are not read. Nothing read here is kept in the index:
//! the bytes come from the pack's config files on every refresh, and a value lives only while
//! the contracts are extracted.
//!
//! A placeholder `${key}` takes the value of `key` when the module's configuration holds one;
//! otherwise it names the environment variable `key` (`Part::Env`), whose value Meada never
//! reads, and `${key:default}` names it with the default (`Part::EnvOr`). `inject` reads Spring's
//! `@Value("${key}")` on a field or a parameter that way (`Code::expand_with`), and `resolved` the
//! text of any other annotation Spring resolves (a listener's topic or queue).

use std::collections::BTreeMap;

use crate::jvm::names::known;
use crate::lang::{Annotated, Marked, Part, Value};
use crate::package::Manifest;
use crate::resolve::Code;

/// The keys and raw values of one module's configuration.
pub type Properties = BTreeMap<String, String>;

/// Spring Boot's base configuration files, which the JVM packs list as their config files
/// (`LanguagePack::config_files`) so the kinds get them through `Indexed::configs`; imports
/// do not read them.
pub const CONFIG_FILES: &[&str] = &[
    "**/application.properties",
    "**/application.yml",
    "**/application.yaml",
];

/// Spring's annotation that injects a value into a field or a parameter.
const VALUE: &str = "org.springframework.beans.factory.annotation.Value";
const RESOURCES: &str = "src/main/resources/";
/// The keys whose presence makes a YAML document a profile's.
const PROFILE_KEYS: &[&str] = &["spring.config.activate.on-profile", "spring.profiles"];

/// How many placeholders deep a value is resolved, through the keys it names; deeper (a cycle
/// among keys included), the placeholder is an unknown part.
// ponytail: real configurations chain a few keys; 32 leaves room and bounds a cycle.
const PLACEHOLDER_DEPTH: usize = 32;
/// What resolving one value may cost: each placeholder visited and each byte of text copied.
/// Past it the rest of the value is one unknown part, so keys that double each other cannot
/// expand a small file into a huge value.
// ponytail: a flat 64 KiB per value; a base URL is a few dozen bytes.
const PLACEHOLDER_BUDGET: usize = 1 << 16;

/// The configurations of a repository's modules, by the module's directory (`api/`, or empty
/// for the repository root), and the main source roots the builds declare, as
/// `(module, root/)` paths from the repository root.
#[derive(Default)]
pub struct Configs {
    modules: BTreeMap<String, Properties>,
    roots: Vec<(String, String)>,
}

impl Configs {
    /// The configurations among `files` (repository path and bytes): those at
    /// `<module>/src/main/resources/application.properties`, `.../application.yml` or
    /// `.../application.yaml`, in Spring's order of precedence (`.properties`, then `.yml`).
    /// A file past `lang::MAX_BYTES`, which the index reads cut, is not read. The Maven and
    /// Gradle builds among `manifests` give their modules' main source roots (a Gradle source
    /// set's `srcDirs` included), which `of` maps a source to its module by.
    pub fn read(files: &[(&str, &[u8])], manifests: &[Manifest]) -> Self {
        let mut by_rank: [BTreeMap<&str, Properties>; 3] = Default::default();
        for &(path, src) in files {
            if src.len() > crate::lang::MAX_BYTES {
                continue;
            }
            let Some((module, name)) = path.rsplit_once(RESOURCES) else {
                continue;
            };
            let Some(module) = module_dir(module) else {
                continue;
            };
            let Ok(src) = std::str::from_utf8(src) else {
                continue;
            };
            let (rank, read) = match name {
                "application.yaml" => (0, read_yaml(src)),
                "application.yml" => (1, read_yaml(src)),
                "application.properties" => (2, read_properties(src)),
                _ => continue,
            };
            by_rank[rank].insert(module, read);
        }
        // The one of higher precedence is laid over the others.
        let mut modules: BTreeMap<String, Properties> = BTreeMap::new();
        for files in by_rank {
            for (module, read) in files {
                modules.entry(module.to_string()).or_default().extend(read);
            }
        }
        let mut roots = Vec::new();
        for m in manifests {
            let (dir, name) = m.path.rsplit_once('/').unwrap_or(("", &m.path));
            if !matches!(name, "pom.xml" | "build.gradle" | "build.gradle.kts") {
                continue;
            }
            let module = match dir {
                "" => String::new(),
                dir => format!("{dir}/"),
            };
            for root in (m.summary.roots.iter()).filter(|r| !m.summary.test_roots.contains(r)) {
                roots.push((module.clone(), format!("{module}{root}/")));
            }
        }
        Self { modules, roots }
    }

    /// The configuration of the module whose sources hold `source`: the module of the deepest
    /// main source root a build declares for it, else the one whose `src/main/` it is under.
    pub fn of(&self, source: &str) -> Option<&Properties> {
        let built = (self.roots.iter())
            .filter(|(_, root)| source.starts_with(root.as_str()))
            .max_by_key(|(_, root)| root.len());
        if let Some((module, _)) = built {
            return self.modules.get(module);
        }
        let mut from = 0;
        while let Some(at) = source[from..].find("src/main/") {
            let at = from + at;
            if let Some(module) = module_dir(&source[..at]) {
                return self.modules.get(module);
            }
            from = at + 1;
        }
        None
    }
}

/// `prefix` as a module's directory: empty, or a path ending at a `/`.
fn module_dir(prefix: &str) -> Option<&str> {
    (prefix.is_empty() || prefix.ends_with('/')).then_some(prefix)
}

/// The literal assignments of a `.properties` file's first document: `key=value`, `key: value`
/// or `key value`, one per line, the last of a key winning. A comment (`#`, `!`), a line
/// continued with an odd number of trailing `\` and the lines it continues onto, and a key or a
/// value with an escape (`\`) are not literal, and are left out. A `#---` or `!---` line starts
/// another document (a profile's, in Spring Boot), which is not read.
fn read_properties(src: &str) -> Properties {
    let mut out = Properties::new();
    let mut continued = false;
    for line in src.lines() {
        let line = line.trim_start();
        let comment = !continued && line.starts_with(['#', '!']);
        if comment && matches!(line.trim_end(), "#---" | "!---") {
            break;
        }
        let trailing = line.len() - line.trim_end_matches('\\').len();
        let was = std::mem::replace(&mut continued, !comment && trailing % 2 == 1);
        if was || continued || comment || line.is_empty() || line.contains('\\') {
            continue;
        }
        let at = line
            .find(['=', ':', ' ', '\t', '\x0c'])
            .unwrap_or(line.len());
        let (key, rest) = line.split_at(at);
        let rest = rest.trim_start_matches([' ', '\t', '\x0c']);
        let value = rest.strip_prefix(['=', ':']).unwrap_or(rest);
        out.insert(key.to_string(), value.trim_start().to_string());
    }
    out
}

/// The scalars of a YAML file's first document, by their keys joined with dots (`api.url`);
/// a list, a null and anything the reader cannot read are left out, and so is a document
/// that activates on a profile.
fn read_yaml(src: &str) -> Properties {
    let Ok(root) = crate::document::yaml::parse(src) else {
        return Properties::new();
    };
    let mut out = Properties::new();
    // The YAML reader bounds the nesting; the walk is an explicit stack.
    let mut pending = vec![(String::new(), &root)];
    while let Some((key, value)) = pending.pop() {
        let scalar = match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    let joined = if key.is_empty() {
                        k.clone()
                    } else {
                        format!("{key}.{k}")
                    };
                    pending.push((joined, v));
                }
                continue;
            }
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => continue,
        };
        out.insert(key, scalar);
    }
    if PROFILE_KEYS.iter().any(|k| out.contains_key(*k)) {
        return Properties::new();
    }
    out
}

/// `text` with its placeholders resolved against `config` (see the module): literal text,
/// environment reads, and an unknown part for a placeholder that cannot be read (unclosed,
/// with no name, past `PLACEHOLDER_DEPTH` or `PLACEHOLDER_BUDGET`). A SpEL expression
/// (`#{...}`) makes the whole value unknown.
pub fn resolve(text: &str, config: Option<&Properties>) -> Vec<Part> {
    if text.contains("#{") {
        return vec![Part::Hole];
    }
    let mut out = Vec::new();
    let mut budget = PLACEHOLDER_BUDGET;
    expand(text, config, 0, &mut budget, &mut out);
    out
}

fn expand(
    text: &str,
    config: Option<&Properties>,
    depth: usize,
    budget: &mut usize,
    out: &mut Vec<Part>,
) {
    let mut rest = text;
    while !rest.is_empty() {
        let (literal, placeholder) = match rest.find("${") {
            Some(at) => rest.split_at(at),
            None => (rest, ""),
        };
        if !charge(budget, literal.len().max(1)) {
            push(out, Part::Hole);
            return;
        }
        if !literal.is_empty() {
            push(out, Part::Text(literal.to_string()));
        }
        if placeholder.is_empty() {
            return;
        }
        let Some(end) = closing(placeholder) else {
            push(out, Part::Hole);
            return;
        };
        let inner = &placeholder[2..end];
        rest = &placeholder[end + 1..];
        let (name, default) = match separator(inner) {
            Some(at) => (&inner[..at], Some(&inner[at + 1..])),
            None => (inner, None),
        };
        let named = !name.is_empty()
            && (name.chars()).all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        if !named || depth >= PLACEHOLDER_DEPTH {
            push(out, Part::Hole);
            continue;
        }
        match (config.and_then(|c| c.get(name)), default) {
            (Some(value), _) => expand(value, config, depth + 1, budget, out),
            (None, None) => push(out, Part::Env(name.to_string())),
            (None, Some(default)) => {
                let mut parts = Vec::new();
                expand(default, config, depth + 1, budget, &mut parts);
                push(out, Part::EnvOr(name.to_string(), parts));
            }
        }
    }
}

fn charge(budget: &mut usize, cost: usize) -> bool {
    match budget.checked_sub(cost) {
        Some(left) => {
            *budget = left;
            true
        }
        None => false,
    }
}

/// `part` after `out`, joined to the text before it.
fn push(out: &mut Vec<Part>, part: Part) {
    match (out.last_mut(), part) {
        (Some(Part::Text(before)), Part::Text(text)) => before.push_str(&text),
        (_, part) => out.push(part),
    }
}

/// The place of the `}` that closes the placeholder `${...` starting `text`, nested ones
/// skipped.
fn closing(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'$' if bytes.get(at + 1) == Some(&b'{') => {
                depth += 1;
                at += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

/// The place of the `:` that starts a placeholder's default, outside nested placeholders.
fn separator(inner: &str) -> Option<usize> {
    let mut depth = 0usize;
    let bytes = inner.as_bytes();
    for (at, &b) in bytes.iter().enumerate() {
        match b {
            b'$' if bytes.get(at + 1) == Some(&b'{') => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b':' if depth == 0 => return Some(at),
            _ => {}
        }
    }
    None
}

/// What Spring's `@Value` written in `path` (the annotation as the pack reads it, with what it
/// marks) injects: its text, constants folded, with its placeholders resolved against the
/// configuration of the file's module (`resolved`). `None` for any other annotation, which
/// counts only when `Code::outside` says an import from Spring binds its name, and for a
/// `static` field, which Spring injects nothing into.
pub fn inject<'a>(
    configs: &Configs,
    code: &Code<'a>,
    path: &'a str,
    annotated: &'a Annotated,
) -> Option<Value> {
    if let Marked::Field {
        is_static: true, ..
    } = annotated.on
    {
        return None;
    }
    let annotation = &annotated.annotation;
    known(code, path, &annotation.scope, &annotation.callee, |s| {
        s == VALUE
    })?;
    let written = annotation.args.first().or(annotation.keyword("value"))?;
    Some(resolved(
        &code.value(path, &annotation.scope, written),
        configs.of(path),
    ))
}

/// A folded value as Spring reads an annotation's text: a string's literal text with its
/// placeholders resolved against `config` (`resolve`) and an environment read kept as it is;
/// any other part (a parameter, a part not read) is unknown (`Part::Hole`), and so is a value
/// that is no string. An annotation's value is a constant, so a part that is not text means
/// the text was not read whole.
pub fn resolved(value: &Value, config: Option<&Properties>) -> Value {
    let Value::Str(parts) = value else {
        return Value::Unknown;
    };
    let mut out = Vec::new();
    for part in parts {
        match part {
            Part::Text(text) => resolve(text, config)
                .into_iter()
                .for_each(|p| push(&mut out, p)),
            Part::Env(_) | Part::EnvOr(..) => out.push(part.clone()),
            _ => out.push(Part::Hole),
        }
    }
    Value::Str(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> Part {
        Part::Text(t.into())
    }

    fn env(name: &str) -> Part {
        Part::Env(name.into())
    }

    fn props(pairs: &[(&str, &str)]) -> Properties {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn properties_are_read_line_by_line_for_literal_assignments() {
        let src = "# a comment\n! another\n\
                   api.url=http://api.example.com\n\
                   web.url : https://web.example.com/v1\n\
                   spaced   http://spaced.example.com\n\
                   \tindented=x\n\
                   empty=\n\
                   escaped=a\\:b\n\
                   long=first \\\n  second\n\
                   after=kept\n\
                   # a comment ends in \\\n\
                   evenly=a\\\\\n\
                   next=kept\n\
                   api.url=http://last.example.com\n\
                   #---\n\
                   spring.config.activate.on-profile=dev\n\
                   api.url=http://dev.example.com\n";
        assert_eq!(
            read_properties(src),
            props(&[
                ("after", "kept"),
                ("api.url", "http://last.example.com"),
                ("empty", ""),
                ("indented", "x"),
                ("next", "kept"),
                ("spaced", "http://spaced.example.com"),
                ("web.url", "https://web.example.com/v1"),
            ])
        );
    }

    #[test]
    fn yaml_keys_join_with_dots_and_a_profile_document_is_not_read() {
        let src = "api:\n  url: http://api.example.com\n  port: 8080\n  tls: true\n\
                   \x20 hosts: [a, b]\n  none: ~\nweb.url: x\n---\nother: y\n";
        assert_eq!(
            read_yaml(src),
            props(&[
                ("api.port", "8080"),
                ("api.tls", "true"),
                ("api.url", "http://api.example.com"),
                ("web.url", "x"),
            ])
        );
        let profile = "spring:\n  config:\n    activate:\n      on-profile: dev\napi.url: x\n";
        assert!(read_yaml(profile).is_empty());
        assert!(read_yaml("a: [unclosed\n").is_empty());
    }

    #[test]
    fn each_module_reads_its_own_base_files_and_properties_win() {
        let files: &[(&str, &[u8])] = &[
            (
                "api/src/main/resources/application.yaml",
                b"a: yaml\nb: yaml\nc: yaml\n",
            ),
            (
                "api/src/main/resources/application.yml",
                b"a: yml\nb: yml\n",
            ),
            (
                "api/src/main/resources/application.properties",
                b"a=properties\n",
            ),
            (
                "api/src/main/resources/application-dev.properties",
                b"a=dev\n",
            ),
            ("api/src/test/resources/application.properties", b"a=test\n"),
            ("src/main/resources/application.properties", b"a=root\n"),
            ("xsrc/main/resources/application.properties", b"a=no\n"),
        ];
        let configs = Configs::read(files, &[]);
        let api = configs.of("api/src/main/java/a/B.java").unwrap();
        assert_eq!(
            api,
            &props(&[("a", "properties"), ("b", "yml"), ("c", "yaml")])
        );
        let root = configs.of("src/main/java/a/B.java").unwrap();
        assert_eq!(root, &props(&[("a", "root")]));
        // Test sources, and sources outside a module's `src/main`, take none.
        assert!(configs.of("api/src/test/java/a/B.java").is_none());
        assert!(configs.of("other/src/main/java/a/B.java").is_none());
        assert!(configs.of("lib/a/B.java").is_none());
        // A file past the size the index reads whole is not read.
        let big = format!("a=cut{}", "x".repeat(crate::lang::MAX_BYTES));
        let path = "src/main/resources/application.properties";
        let files: &[(&str, &[u8])] = &[(path, big.as_bytes())];
        assert!(
            Configs::read(files, &[])
                .of("src/main/java/a/B.java")
                .is_none()
        );
    }

    #[test]
    fn a_placeholder_takes_a_key_of_the_configuration_or_names_a_variable() {
        let config = props(&[
            ("api.url", "http://api.example.com"),
            ("web.url", "${WEB_URL}"),
            ("audit.url", "${AUDIT_URL:https://web.example.com/api}/v1"),
            ("chained", "${api.url}/x"),
        ]);
        let c = Some(&config);
        assert_eq!(resolve("plain", c), [text("plain")]);
        assert_eq!(resolve("${api.url}", c), [text("http://api.example.com")]);
        assert_eq!(
            resolve("${chained}/y", c),
            [text("http://api.example.com/x/y")]
        );
        assert_eq!(resolve("${web.url}/p", c), [env("WEB_URL"), text("/p")]);
        assert_eq!(
            resolve("${audit.url}", c),
            [
                Part::EnvOr(
                    "AUDIT_URL".into(),
                    vec![text("https://web.example.com/api")]
                ),
                text("/v1")
            ]
        );
        // A key the configuration lacks is a variable, with its default when it has one.
        assert_eq!(resolve("${MISSING}", c), [env("MISSING")]);
        assert_eq!(
            resolve("${A:${B:http://b.example.com}}", None),
            [Part::EnvOr(
                "A".into(),
                vec![Part::EnvOr("B".into(), vec![text("http://b.example.com")])]
            )]
        );
        assert_eq!(resolve("${A:}", None), [Part::EnvOr("A".into(), vec![])]);
    }

    #[test]
    fn a_placeholder_that_cannot_be_read_is_an_unknown_part() {
        assert_eq!(
            resolve("http://${open", None),
            [text("http://"), Part::Hole]
        );
        assert_eq!(resolve("${}x", None), [Part::Hole, text("x")]);
        assert_eq!(resolve("${a b}", None), [Part::Hole]);
        assert_eq!(resolve("#{'${a}'}", None), [Part::Hole]);
    }

    #[test]
    fn a_cycle_of_keys_stops_at_the_placeholder_depth() {
        let config = props(&[("a", "${b}"), ("b", "${a}")]);
        assert_eq!(resolve("${a}", Some(&config)), [Part::Hole]);
        // One key short of the depth still resolves.
        let mut chain: Properties = (0..PLACEHOLDER_DEPTH - 1)
            .map(|n| (format!("k{n}"), format!("${{k{}}}", n + 1)))
            .collect();
        chain.insert(format!("k{}", PLACEHOLDER_DEPTH - 1), "end".into());
        assert_eq!(resolve("${k0}", Some(&chain)), [text("end")]);
        chain.insert(format!("k{}", PLACEHOLDER_DEPTH - 1), "${k0}".into());
        assert_eq!(resolve("${k0}", Some(&chain)), [Part::Hole]);
    }

    #[test]
    fn keys_that_double_each_other_stop_at_the_budget() {
        // Each key holds the next twice: the value would be 2^31 copies of the last one.
        let mut config: Properties = (0..31)
            .map(|n| (format!("k{n}"), format!("${{k{0}}}${{k{0}}}", n + 1)))
            .collect();
        config.insert("k31".into(), "xxxxxxxx".into());
        let parts = resolve("${k0}", Some(&config));
        let size: usize = (parts.iter())
            .map(|p| match p {
                Part::Text(t) => t.len(),
                _ => 0,
            })
            .sum();
        assert!(size <= PLACEHOLDER_BUDGET);
        assert_eq!(parts.last(), Some(&Part::Hole));
    }

    #[test]
    fn a_source_root_of_a_build_maps_its_files_to_the_module_of_the_build() {
        use crate::package::{Manifest, Summary};
        let files: &[(&str, &[u8])] = &[(
            "tools/src/main/resources/application.properties",
            b"a=tools\n",
        )];
        let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        let build = Manifest {
            path: "tools/build.gradle".into(),
            summary: Summary {
                roots: strings(&[
                    "src/core/java",
                    "src/it/java",
                    "src/main/java",
                    "src/test/java",
                ]),
                test_roots: strings(&["src/it/java", "src/test/java"]),
                ..Summary::default()
            },
        };
        let configs = Configs::read(files, std::slice::from_ref(&build));
        let tools = props(&[("a", "tools")]);
        assert_eq!(configs.of("tools/src/core/java/a/B.java"), Some(&tools));
        assert_eq!(configs.of("tools/src/main/java/a/B.java"), Some(&tools));
        // A test source set takes none, as `src/test` does, and neither does a file outside
        // every root.
        assert!(configs.of("tools/src/it/java/a/B.java").is_none());
        assert!(configs.of("tools/other/a/B.java").is_none());
        // Without the build, a literal source set is in no module.
        let bare = Configs::read(files, &[]);
        assert!(bare.of("tools/src/core/java/a/B.java").is_none());
    }

    #[test]
    fn resolved_reads_the_text_keeps_an_environment_read_and_blanks_any_other_part() {
        let config = props(&[("a", "http://a.example.com")]);
        let value = Value::Str(vec![text("${a}/x"), env("B"), Part::Ref("p".into())]);
        let Value::Str(parts) = resolved(&value, Some(&config)) else {
            panic!("a string stays one");
        };
        assert_eq!(
            parts,
            [text("http://a.example.com/x"), env("B"), Part::Hole]
        );
        assert!(matches!(
            resolved(&Value::Ref("p".into()), None),
            Value::Unknown
        ));
    }
}
