//! The PyPI ecosystem (spec #24): a `pyproject.toml` or a `requirements*.txt` read into the same
//! `Summary` a `package.json` gives, its names normalized as PEP 503 does. Ranges are kept as
//! written and never evaluated: every PyPI package link says `range unchecked`.

use super::{Entry, MAX_BYTES, MAX_NAME, Summary};
use crate::lang::python::pyproject::{Pyproject, Requirement};

/// The dependency fields, as entries and consumer sites name them.
pub const PROJECT: &str = "project.dependencies";
pub const POETRY: &str = "tool.poetry.dependencies";
pub const REQUIREMENTS: &str = "requirements";

/// Whether the file `name` is a pip requirements file: `requirements.txt`,
/// `requirements-dev.txt`, `requirements_test.txt` and the like.
pub fn is_requirements(name: &str) -> bool {
    name.starts_with("requirements") && name.ends_with(".txt")
}

/// `name` normalized as PEP 503 does (lower case, each run of `-`, `_` and `.` one `-`), when it
/// is a valid PEP 508 name (ASCII letters, digits, `-`, `_` and `.`, starting and ending with a
/// letter or digit) of at most `MAX_NAME` bytes; `None` otherwise.
pub fn normalize(name: &str) -> Option<String> {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let valid = name.len() <= MAX_NAME
        && edge(name.chars().next())
        && edge(name.chars().last())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if !valid {
        return None;
    }
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '-' | '_' | '.' if out.ends_with('-') => {}
            '-' | '_' | '.' => out.push('-'),
            c => out.push(c.to_ascii_lowercase()),
        }
    }
    Some(out)
}

/// The module name a distribution's import is spelled with when it follows the convention:
/// its normalized name with `-` turned into `_` (`acme-billing` is `acme_billing`).
pub fn import_name(normalized: &str) -> String {
    normalized.replace('-', "_")
}

/// The normalized name and the version specifier (`None` when there is none) of a PEP 508
/// requirement: extras and environment markers are dropped, and the specifier is kept as
/// written, without the parentheses around it. `None` for a direct reference (`name @ url`) or
/// anything that is no requirement.
pub fn requirement(s: &str) -> Option<(String, Option<String>)> {
    let s = s.split(';').next()?.trim();
    let end =
        (s.find(|c: char| !(c.is_ascii_alphanumeric() || "-_.".contains(c)))).unwrap_or(s.len());
    let name = normalize(&s[..end])?;
    let mut rest = s[end..].trim_start();
    if let Some(extras) = rest.strip_prefix('[') {
        rest = extras.split_once(']')?.1.trim_start();
    }
    let rest = (rest.strip_prefix('(').and_then(|r| r.strip_suffix(')'))).unwrap_or(rest);
    let rest = rest.trim();
    if !(rest.is_empty() || rest.starts_with(['<', '>', '=', '!', '~'])) {
        return None;
    }
    Some((name, (!rest.is_empty()).then(|| rest.to_string())))
}

/// The entries `found` gives under `field`, sorted by name, the first of a name kept.
fn entries(field: &str, found: impl Iterator<Item = (String, Option<String>)>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = found
        .map(|(name, range)| Entry {
            field: field.to_string(),
            name,
            range,
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries.dedup_by(|b, a| a.name == b.name);
    entries
}

/// Reads a `pyproject.toml`. Hostile input, as for a `package.json`: a file over `MAX_BYTES`,
/// not UTF-8 or not TOML reads as empty; a name that is not a valid distribution name makes no
/// provider, and a dependency that is not a valid requirement is left out.
pub fn read_pyproject(src: &[u8]) -> Summary {
    let Some(p) = (src.len() <= MAX_BYTES)
        .then(|| Pyproject::parse(src).ok())
        .flatten()
    else {
        return Summary::default();
    };
    let (mut pep621, mut poetry) = (Vec::new(), Vec::new());
    for r in p.requirements() {
        match r {
            Requirement::Pep508(s) => pep621.extend(requirement(s)),
            Requirement::Poetry(name, version) => poetry.extend(
                normalize(name).map(|n| (n, version.filter(|v| !v.is_empty()).map(String::from))),
            ),
        }
    }
    let mut dependencies = entries(PROJECT, pep621.into_iter());
    dependencies.extend(entries(POETRY, poetry.into_iter()));
    Summary {
        name: p.name().and_then(normalize),
        version: p.version().filter(|v| !v.is_empty()).map(String::from),
        dependencies,
        roots: p.roots(),
        scripts: p.scripts(),
        ..Summary::default()
    }
}

/// Reads a `requirements*.txt` line by line, a line ending in `\` joined to the next as pip does:
/// comments (`#` at the start or after whitespace), blank lines, option lines (`-r`, `-e`,
/// `--index-url`, `--hash`, ...) and URLs or paths are skipped, and a per-requirement option
/// (` --hash=...`) is cut. Nothing the file
/// names elsewhere (`-r other.txt`) is followed. A file over `MAX_BYTES` or not UTF-8 reads as
/// empty.
pub fn read_requirements(src: &[u8]) -> Summary {
    let Some(text) = (src.len() <= MAX_BYTES)
        .then(|| std::str::from_utf8(src).ok())
        .flatten()
    else {
        return Summary::default();
    };
    // A line ending in `\` continues on the next, as pip joins them; a comment line ends it.
    let mut lines = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        match line.strip_suffix('\\') {
            Some(head) if !line.trim_start().starts_with('#') => pending.push_str(head),
            _ => {
                pending.push_str(line);
                lines.push(std::mem::take(&mut pending));
            }
        }
    }
    lines.push(pending);
    let found = lines.iter().filter_map(|line| {
        let comment = line
            .char_indices()
            .find(|&(i, c)| c == '#' && (i == 0 || line[..i].ends_with(char::is_whitespace)));
        let line = comment.map_or(line.as_str(), |(i, _)| &line[..i]).trim();
        if line.is_empty() || line.starts_with('-') || line.contains("://") {
            return None;
        }
        requirement(line.split_once(" -").map_or(line, |(r, _)| r))
    });
    Summary {
        dependencies: entries(REQUIREMENTS, found),
        ..Summary::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps(s: &Summary) -> Vec<(&str, &str, Option<&str>)> {
        (s.dependencies.iter())
            .map(|e| (e.field.as_str(), e.name.as_str(), e.range.as_deref()))
            .collect()
    }

    #[test]
    fn names_normalize_as_pep_503_does() {
        let table = [
            ("Shared_Lib", Some("shared-lib")),
            ("acme.billing", Some("acme-billing")),
            ("a--_.b", Some("a-b")),
            ("A1", Some("a1")),
            ("x", Some("x")),
            ("-a", None),
            ("a-", None),
            ("a b", None),
            ("a::b", None),
            ("ä", None),
            ("", None),
        ];
        for (name, want) in table {
            assert_eq!(normalize(name).as_deref(), want, "{name:?}");
        }
        assert_eq!(
            normalize(&"a".repeat(MAX_NAME)).map(|n| n.len()),
            Some(MAX_NAME)
        );
        assert_eq!(normalize(&"a".repeat(MAX_NAME + 1)), None);
        assert_eq!(import_name("acme-billing"), "acme_billing");
    }

    #[test]
    fn a_requirement_gives_its_name_and_specifier() {
        let some = |n: &str, r: Option<&str>| Some((n.to_string(), r.map(String::from)));
        let table = [
            ("shared-lib", some("shared-lib", None)),
            ("Shared_Lib>=1.0,<2", some("shared-lib", Some(">=1.0,<2"))),
            (
                "shared-lib [fast, x] ~= 1.4",
                some("shared-lib", Some("~= 1.4")),
            ),
            ("shared-lib (>=1.0)", some("shared-lib", Some(">=1.0"))),
            (
                "shared-lib==1.*; python_version >= '3.9'",
                some("shared-lib", Some("==1.*")),
            ),
            (
                "shared-lib ; sys_platform == 'linux'",
                some("shared-lib", None),
            ),
            ("shared-lib!=1.1", some("shared-lib", Some("!=1.1"))),
            // A direct reference, an unclosed extra or text after the name is no requirement.
            ("shared-lib @ https://example.com/s.whl", None),
            ("shared-lib[fast", None),
            ("shared-lib latest", None),
            ("./local", None),
            ("", None),
        ];
        for (s, want) in table {
            assert_eq!(requirement(s), want, "{s:?}");
        }
    }

    #[test]
    fn a_requirements_file_skips_options_comments_and_urls() {
        let src = "\
# the app's dependencies
-r requirements-dev.txt
-e ./local
--index-url https://pypi.example.com/simple
shared-lib[fast]>=1.0 # pinned below 2
Shared.Tools==0.3.0 \\
    --hash=sha256:abc
git+https://example.com/x.git#egg=x
https://example.com/y.whl
y @ https://example.com/y.whl
z#not-a-comment
shared-lib==9
";
        let s = read_requirements(src.as_bytes());
        assert_eq!(
            deps(&s),
            [
                (REQUIREMENTS, "shared-lib", Some(">=1.0")),
                (REQUIREMENTS, "shared-tools", Some("==0.3.0")),
            ]
        );
        assert_eq!((s.name, s.version), (None, None));
    }

    #[test]
    fn a_line_ending_in_a_backslash_continues_on_the_next() {
        let src = "shared-lib \\\n    >=1.0,<2 \\\n    ; python_version >= '3.9'\n\
                   other\\\n-tools==1\n# a comment ends no continuation \\\nlast==2\n";
        let s = read_requirements(src.as_bytes());
        assert_eq!(
            deps(&s),
            [
                (REQUIREMENTS, "last", Some("==2")),
                (REQUIREMENTS, "other-tools", Some("==1")),
                (REQUIREMENTS, "shared-lib", Some(">=1.0,<2")),
            ]
        );
    }

    #[test]
    fn a_pyproject_reads_its_distribution_and_both_dependency_tables() {
        let src = r#"
[project]
name = "Acme_Billing"
version = "1.2.0"
dependencies = ["shared-lib[fast]>=1.0; python_version >= '3.9'", "y @ https://x", "bad name"]
[tool.poetry.dependencies]
python = "^3.11"
Shared_Tools = { path = "../tools" }
other = "^2"
[tool.setuptools.packages.find]
where = ["src"]
[project.scripts]
serve = "acme.cli:main"
"#;
        let s = read_pyproject(src.as_bytes());
        assert_eq!(s.name.as_deref(), Some("acme-billing"));
        assert_eq!(s.version.as_deref(), Some("1.2.0"));
        assert_eq!(
            deps(&s),
            [
                (PROJECT, "shared-lib", Some(">=1.0")),
                (POETRY, "other", Some("^2")),
                (POETRY, "shared-tools", None),
            ]
        );
        assert_eq!(s.roots, ["src"]);
        assert_eq!(s.scripts, [("acme.cli".to_string(), "main".to_string())]);
        let poetry =
            read_pyproject(b"[tool.poetry]\nname = \"shared.tools\"\nversion = \"0.3.0\"\n");
        assert_eq!(poetry.name.as_deref(), Some("shared-tools"));
        let invalid = read_pyproject(b"[project]\nname = \"-x\"\n");
        assert_eq!(invalid.name, None);
    }

    #[test]
    fn a_manifest_too_large_or_unreadable_reads_as_empty() {
        let pad = format!("[project]\nname = \"a\"\n#{}", " ".repeat(MAX_BYTES));
        assert_eq!(read_pyproject(pad.as_bytes()), Summary::default());
        let reqs = format!("a\n#{}", " ".repeat(MAX_BYTES));
        assert_eq!(read_requirements(reqs.as_bytes()), Summary::default());
        for bad in [&b"[project\n"[..], b"\xff", b"name = "] {
            assert_eq!(read_pyproject(bad), Summary::default());
        }
        assert_eq!(read_requirements(b"\xffa"), Summary::default());
        let nested = format!("a = {}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert_eq!(read_pyproject(nested.as_bytes()), Summary::default());
    }
}
