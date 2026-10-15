//! The repository map keeps up with the code: every limit marked `ponytail:` is named under
//! "Known ceilings" in `docs/agents/architecture.md` (CODING_STANDARDS.md, hostile input).

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
