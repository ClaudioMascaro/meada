//! The repository map keeps up with the code: every limit marked `ponytail:` is named under
//! "Known ceilings" in `docs/agents/architecture.md` (CODING_STANDARDS.md, hostile input), and
//! every entry point kind and process step mark is in the list the map gives of them.

use std::fs;
use std::path::Path;

/// The `const` names whose comment block (the `//` lines right above them) holds `ponytail:`.
fn marked(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut names = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let code = !line.trim_start().starts_with("//");
        let Some((_, rest)) = line.split_once("const ").filter(|_| code) else {
            continue;
        };
        if !rest.starts_with(|c: char| c.is_ascii_uppercase()) {
            continue;
        }
        let comments = lines[..i]
            .iter()
            .rev()
            .take_while(|l| l.trim_start().starts_with("//"));
        if comments.into_iter().any(|l| l.contains("ponytail:")) {
            let name: String = (rest.chars())
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            names.push(name);
        }
    }
    names
}

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

#[test]
fn every_marked_limit_is_a_known_ceiling() {
    let map = fs::read_to_string("docs/agents/architecture.md").unwrap();
    let start = map
        .find("\n## Known ceilings")
        .expect("a Known ceilings section");
    let section = &map[start + 1..];
    let ceilings = match section[1..].find("\n## ") {
        Some(end) => &section[..end + 1],
        None => section,
    };
    let mut files = Vec::new();
    sources(Path::new("src"), &mut files);
    let found: Vec<(&String, String)> = (files.iter())
        .flat_map(|(path, text)| marked(text).into_iter().map(move |n| (path, n)))
        .collect();
    // The scan itself must see the limits it guards.
    assert!(found.iter().any(|(_, name)| name == "MAX_NESTING"));
    let missing: Vec<String> = (found.into_iter())
        .filter(|(_, name)| !ceilings.contains(&format!("`{name}`")))
        .map(|(path, name)| format!("{path}: {name}"))
        .collect();
    assert!(
        missing.is_empty(),
        "limits marked `ponytail:` but not named in Known ceilings: {missing:?}"
    );
}

#[test]
fn a_marker_counts_only_in_the_comment_block_right_above() {
    let text = "// ponytail: a ceiling\n/// Doc.\nconst A: u8 = 1;\n\nconst B: u8 = 2;\n\
                // ponytail: not this one\nfn f() {}\nconst C: u8 = 3;\n";
    assert_eq!(marked(text), vec!["A".to_string()]);
}

/// The names in `names` that `doc` does not hold in backticks between `anchor` and the first
/// `end` after it, where the doc lists them.
fn unlisted(doc: &str, anchor: &str, end: &str, names: &[String]) -> Vec<String> {
    let text = fs::read_to_string(doc).unwrap();
    let start = (text.find(anchor)).unwrap_or_else(|| panic!("{anchor} in {doc}"));
    let list = &text[start..];
    let list = &list[..list.find(end).unwrap_or_else(|| panic!("{end} in {doc}"))];
    (names.iter())
        .filter(|n| !list.contains(&format!("`{n}`")))
        .cloned()
        .collect()
}

#[test]
fn every_entry_point_kind_is_listed_in_the_map() {
    let entry = fs::read_to_string("meada-core/src/export.rs").unwrap();
    // The arms of `EntryKind::name`: `EntryKind::Route => "route",`.
    let kinds: Vec<String> = (entry.lines())
        .filter_map(|l| l.trim().strip_prefix("EntryKind::")?.split_once(" => \""))
        .map(|(_, name)| name.trim_end_matches("\",").to_string())
        .collect();
    assert!(kinds.contains(&"program".to_string()), "{kinds:?}");
    let missing = unlisted(
        "docs/agents/architecture.md",
        "`kind` being ",
        ", each step",
        &kinds,
    );
    assert!(
        missing.is_empty(),
        "entry point kinds missing from the `processes` kinds in the architecture map: {missing:?}"
    );
}

#[test]
fn every_process_mark_is_listed_in_the_queries_map() {
    let trace = fs::read_to_string("meada-core/src/export.rs").unwrap();
    let start = trace.find("\npub enum Mark {").expect("enum Mark");
    // The variants as serde's `kebab-case` writes them: `DepthLimit` is `depth-limit`.
    let marks: Vec<String> = (trace[start..].lines().skip(2))
        .map(str::trim)
        .take_while(|l| *l != "}")
        .filter(|l| !l.starts_with("//"))
        .map(|variant| {
            let mut name = String::new();
            for (i, c) in variant.trim_end_matches(',').chars().enumerate() {
                if c.is_ascii_uppercase() && i > 0 {
                    name.push('-');
                }
                name.push(c.to_ascii_lowercase());
            }
            name
        })
        .collect();
    assert!(marks.contains(&"depth-limit".to_string()), "{marks:?}");
    let missing = unlisted("docs/agents/map/queries.md", "`Mark` (", ")", &marks);
    assert!(
        missing.is_empty(),
        "process marks missing from the `Mark` list of docs/agents/map/queries.md: {missing:?}"
    );
}
