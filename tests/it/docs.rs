//! The repository map keeps up with the code: every limit marked `ponytail:` is named in
//! `docs/agents/ceilings.md` or a part of `docs/agents/ceilings/` (CODING_STANDARDS.md, hostile input),
//! source comments use `ponytail:` rather than `shortcut:`, every
//! entry point kind and process step mark is in the list the map gives of them, every module has
//! its line in `docs/agents/architecture.md`, and every Markdown file under `docs/agents` stays
//! within its size budget (the map and the files split by `examples/split_agents.rs`).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// The name a `const` item on this line declares: `pub const NAME: u8` gives `NAME`, while a
/// `const fn` and a line of comment give none.
fn const_name(line: &str) -> Option<String> {
    let (_, rest) = line
        .split_once("const ")
        .filter(|_| !line.trim_start().starts_with("//"))?;
    rest.starts_with(|c: char| c.is_ascii_uppercase()).then(|| {
        (rest.chars())
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect()
    })
}

/// Every `const` of the text with whether its comment block holds `ponytail:`. The block is the
/// `//` lines right above the const, or above the first of a run of consecutive `const` lines
/// (a const with a comment of its own starts a run of its own): a run shares its marker.
fn consts(text: &str) -> Vec<(String, bool)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = const_name(line) else {
            continue;
        };
        let first = (0..i)
            .rev()
            .take_while(|j| {
                const_name(lines[*j]).is_some() || lines[*j].trim_start().starts_with("#[")
            })
            .last()
            .unwrap_or(i);
        let comments = lines[..first]
            .iter()
            .rev()
            .take_while(|l| l.trim_start().starts_with("//"));
        out.push((name, comments.into_iter().any(|l| l.contains("ponytail:"))));
    }
    out
}

/// The `const` names whose comment block holds `ponytail:` (see `consts`).
fn marked(text: &str) -> Vec<String> {
    (consts(text).into_iter())
        .filter_map(|(name, marked)| marked.then_some(name))
        .collect()
}

/// The sources whose limits the checks guard: the binary's and the core's.
fn limit_sources() -> Vec<(String, String)> {
    let mut files = Vec::new();
    sources(Path::new("src"), &mut files);
    sources(Path::new("meada-core/src"), &mut files);
    files
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
    // The list is the index and the area files of `docs/agents/ceilings/` (split by budget, #644).
    let ceilings: String = (agents_markdown().into_iter())
        .filter(|(path, _)| path.starts_with("docs/agents/ceilings"))
        .map(|(_, text)| text)
        .collect();
    let files = limit_sources();
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
        "limits marked `ponytail:` but not named in the known ceilings: {missing:?}"
    );
}

#[test]
fn a_marker_counts_only_in_the_comment_block_right_above() {
    let text = "// ponytail: a ceiling\n/// Doc.\nconst A: u8 = 1;\n\nconst B: u8 = 2;\n\
                // ponytail: not this one\nfn f() {}\nconst C: u8 = 3;\n";
    assert_eq!(marked(text), vec!["A".to_string()]);
}

/// Names that look like a limit (`MAX_*`, `*_MAX`, `*_LIMIT`, `*_CAP`) and are not a ceiling,
/// each with its reason.
const NOT_CEILINGS: &[(&str, &str)] = &[
    (
        "MAX_AGE_MIN",
        "a minimum, the shortest `[oidc.github] max_age_seconds`",
    ),
    (
        "MAX_OUT",
        "a hostile-input test's allowance for what a reader returns, not a bound of the code",
    ),
];

fn looks_like_a_limit(name: &str) -> bool {
    name.starts_with("MAX_")
        || name.ends_with("_MAX")
        || name.ends_with("_LIMIT")
        || name.ends_with("_CAP")
}

/// What is wrong with the limits of `files`: a const named like a limit without a `ponytail:`
/// marker, and a marked limit name declared twice (the ceilings list limits by name, so one name
/// stands for one limit).
fn limit_problems(files: &[(String, String)], allowed: &[(&str, &str)]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (path, text) in files {
        for (name, marked) in consts(text) {
            // Only limit names must be unique: each language pack keeps its own `VALUE_DEPTH`.
            if marked && looks_like_a_limit(&name) && !seen.insert(name.clone()) {
                problems.push(format!(
                    "{path}: {name} is a second marked const of that name"
                ));
            }
            let excused = allowed.iter().any(|(n, _)| *n == name);
            if !marked && looks_like_a_limit(&name) && !excused {
                problems.push(format!(
                    "{path}: {name} is named like a limit but not marked"
                ));
            }
        }
    }
    problems
}

#[test]
fn every_limit_is_marked_once() {
    let files = limit_sources();
    let problems = limit_problems(&files, NOT_CEILINGS);
    assert!(
        problems.is_empty(),
        "mark a limit `ponytail:` and list it in the known ceilings (`docs/agents/ceilings/`), rename it, share it, or \
         add it to NOT_CEILINGS with its reason: {problems:#?}"
    );
    let unmarked: BTreeSet<String> = (files.iter())
        .flat_map(|(_, text)| consts(text))
        .filter_map(|(name, marked)| (!marked).then_some(name))
        .collect();
    for (name, reason) in NOT_CEILINGS {
        assert!(looks_like_a_limit(name) && !reason.is_empty(), "{name}");
        // An excuse outlives its const unnoticed otherwise.
        assert!(
            unmarked.contains(*name),
            "{name} is excused but no unmarked const has it"
        );
    }
}

#[test]
fn an_unmarked_or_repeated_limit_is_a_problem() {
    let file = |text: &str| vec![("a.rs".to_string(), text.to_string())];
    // A run shares the comment block above its first const.
    let run = "// ponytail: both\nconst A_MAX: u8 = 1;\nconst B_LIMIT: u8 = 2;\n";
    assert!(limit_problems(&file(run), &[]).is_empty());
    let bare = "const FOO_MAX: u8 = 1;\n";
    assert_eq!(limit_problems(&file(bare), &[]).len(), 1);
    assert!(limit_problems(&file(bare), &[("FOO_MAX", "a minimum")]).is_empty());
    let twice =
        "// ponytail: one\nconst A_MAX: u8 = 1;\n\n// ponytail: two\nconst A_MAX: u8 = 2;\n";
    assert_eq!(limit_problems(&file(twice), &[]).len(), 1);
    let two_files = [file(run), file(run)].concat();
    assert_eq!(limit_problems(&two_files, &[]).len(), 2);
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
        "docs/agents/map/overview/identities-and-formats.md",
        "`kind` being ",
        ", each step",
        &kinds,
    );
    assert!(
        missing.is_empty(),
        "entry point kinds missing from the `processes` kinds in the identities map: {missing:?}"
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

// ponytail: the architecture, contracts and resolution/index/shape ceiling indexes route an
// agent to relevant topics within 8 KiB, in bytes. Detail belongs in their topic leaves.
const MAP_BUDGET: u64 = 8 * 1024;

// ponytail: the relocated overview, contract and resolution/index/shape ceiling topics each fit
// within 24 KiB, in bytes; unrelated areas keep their existing budget during this first wave.
const DETAIL_BUDGET: u64 = 24 * 1024;

// ponytail: every other Markdown file under `docs/agents` (an area file, the ceilings list, a part
// the splitter wrote) holds at most 64 KiB, in bytes, until its area is migrated. A
// file past it is split by `examples/split_agents.rs`, never trimmed (#644).
const AREA_BUDGET: u64 = 64 * 1024;

/// Every Markdown file under `docs/agents`, as (path, text), in path order.
fn agents_markdown() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "md") {
                let text = fs::read_to_string(&path).unwrap();
                out.push((path.display().to_string(), text));
            }
        }
    }
    let mut out = Vec::new();
    walk(Path::new("docs/agents"), &mut out);
    out
}

/// The files of `sizes` (path, bytes) past their budget, each as the failure message names it.
fn over_budget(sizes: &[(String, u64)]) -> Vec<String> {
    (sizes.iter())
        .map(|(path, size)| {
            let budget = if matches!(
                path.as_str(),
                "docs/agents/architecture.md"
                    | "docs/agents/map/contracts.md"
                    | "docs/agents/ceilings/resolution-the-index-and-shape-readers.md"
            ) {
                MAP_BUDGET
            } else if [
                "docs/agents/map/overview/",
                "docs/agents/map/contracts/",
                "docs/agents/ceilings/resolution-the-index-and-shape-readers/",
            ]
            .iter()
            .any(|prefix| path.starts_with(prefix))
            {
                DETAIL_BUDGET
            } else {
                AREA_BUDGET
            };
            (path, size, budget)
        })
        .filter(|(_, size, budget)| *size > budget)
        .map(|(path, size, budget)| {
            format!(
                "{path} is {size} bytes, over its budget of {budget} bytes: split it by area with \
                 `scripts/split-agents {path}` in this change (a part still over \
                 its budget needs `## ` headings of its own first), never trim it by dropping entries"
            )
        })
        .collect()
}

#[test]
fn every_agents_markdown_file_stays_within_its_budget() {
    let files = agents_markdown();
    // The scan sees the map, the index of the ceilings and the parts the splitter wrote.
    for seen in [
        "architecture.md",
        "ceilings.md",
        "ceilings/language-packs.md",
    ] {
        assert!(files.iter().any(|(p, _)| p.ends_with(seen)), "{seen}");
    }
    // The scan above covers `src/` only: the budgets of this file are named by hand.
    let ceilings: String = (files.iter())
        .filter(|(path, _)| path.starts_with("docs/agents/ceilings"))
        .map(|(_, text)| text.as_str())
        .collect();
    for name in ["MAP_BUDGET", "DETAIL_BUDGET", "AREA_BUDGET"] {
        assert!(
            ceilings.contains(&format!("`{name}`")),
            "{name} not in the ceilings"
        );
    }
    let sizes: Vec<(String, u64)> = (files.iter())
        .map(|(path, text)| (path.clone(), text.len() as u64))
        .collect();
    let over = over_budget(&sizes);
    assert!(over.is_empty(), "{over:#?}");
}

#[test]
fn a_file_past_its_budget_fails_with_the_path_the_size_the_budget_and_the_splitter() {
    let map = ("docs/agents/architecture.md".to_string(), MAP_BUDGET + 1);
    let area = ("docs/agents/map/tests.md".to_string(), AREA_BUDGET + 7);
    let fits = ("docs/agents/map/core.md".to_string(), AREA_BUDGET);
    let over = over_budget(&[map, area, fits]);
    assert_eq!(over.len(), 2, "{over:#?}");
    assert!(over[0].contains("docs/agents/architecture.md is 8193 bytes"));
    assert!(over[0].contains("budget of 8192 bytes"));
    assert!(over[1].contains("docs/agents/map/tests.md is 65543 bytes"));
    assert!(over[1].contains("scripts/split-agents docs/agents/map/tests.md"));
}

#[test]
fn migrated_indexes_and_topics_enforce_their_budgets_at_the_boundary() {
    for (path, budget) in [
        ("docs/agents/map/contracts.md", MAP_BUDGET),
        (
            "docs/agents/ceilings/resolution-the-index-and-shape-readers.md",
            MAP_BUDGET,
        ),
        (
            "docs/agents/map/overview/identities-and-formats.md",
            DETAIL_BUDGET,
        ),
        (
            "docs/agents/map/contracts/http-extraction-and-linking.md",
            DETAIL_BUDGET,
        ),
        (
            "docs/agents/ceilings/resolution-the-index-and-shape-readers/resolution-and-value-flow.md",
            DETAIL_BUDGET,
        ),
        ("docs/agents/map/queries.md", AREA_BUDGET),
    ] {
        assert!(over_budget(&[(path.to_string(), budget)]).is_empty());
        let over = over_budget(&[(path.to_string(), budget + 1)]);
        assert_eq!(over.len(), 1, "{path}: {over:?}");
        assert!(over[0].contains(&format!("budget of {budget} bytes")));
    }
}

#[test]
fn every_relative_link_of_the_agents_docs_resolves() {
    let mut broken = Vec::new();
    for (path, text) in agents_markdown() {
        let mut rest = text.as_str();
        while let Some(at) = rest.find("](") {
            rest = &rest[at + 2..];
            let target = rest.split(')').next().unwrap();
            let file = target.split('#').next().unwrap();
            if file.is_empty() || file.contains(':') || target.contains(char::is_whitespace) {
                continue;
            }
            let dir = Path::new(&path).parent().unwrap();
            if !dir.join(file).exists() {
                broken.push(format!("{path}: {target}"));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "links that resolve to no file: {broken:#?}"
    );
}

/// The map's bullets that link a file in `map/`, as (lead, linked files): the lead is what they name
/// before the first `: `, the files are the `map/*.md` they link. A bullet with no `: ` has no lead.
fn module_leads(map: &str) -> Vec<(String, Vec<String>)> {
    let mut bullets: Vec<String> = Vec::new();
    for line in map.lines() {
        if line.starts_with("- ") {
            bullets.push(line.to_string());
        } else if line.starts_with("  ") && !bullets.is_empty() {
            let last = bullets.last_mut().unwrap();
            last.push(' ');
            last.push_str(line.trim());
        }
    }
    (bullets.iter())
        // A bullet that names an area file points to it with a link into `map/`.
        .filter(|b| b.contains("](map/"))
        .filter_map(|b| {
            let (lead, _) = b.split_once(": ")?;
            let files = b.split("](map/").skip(1);
            let files = files.map(|r| r.split([')', '#']).next().unwrap().to_string());
            Some((lead.to_string(), files.collect()))
        })
        .collect()
}

#[test]
fn every_ui_module_is_named_in_the_console_frontend_map() {
    // Tests under `ui/tests` are out of scope; stale symbol names stay a review judgement.
    let docs: String = (agents_markdown().into_iter())
        .filter(|(p, _)| p.ends_with("architecture.md") || p.contains("/map/"))
        .map(|(_, text)| text)
        .collect();
    let unnamed: Vec<String> = (fs::read_dir("ui/src").unwrap())
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".ts") || n.ends_with(".tsx"))
        .filter(|n| !docs.contains(&format!("`{n}`")))
        .collect();
    assert!(
        unnamed.is_empty(),
        "ui/src files with no backticked name under docs/agents: {unnamed:?}"
    );
}

#[test]
fn every_module_has_a_line_in_the_map_naming_its_area_file() {
    let map = fs::read_to_string("docs/agents/architecture.md").unwrap();
    // Module lines sit under Crates and Modules, before the Console frontend pointer.
    let modules = &map[..map
        .find("\n## Console frontend")
        .expect("a Console frontend section")];
    let leads = module_leads(modules);
    let mut missing = Vec::new();
    let mut unnamed = Vec::new();
    for dir in ["src", "meada-core/src"] {
        let mut stems = BTreeSet::new();
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() || path.extension().is_some_and(|e| e == "rs") {
                stems.insert(path.file_stem().unwrap().to_string_lossy().to_string());
            }
        }
        for stem in stems {
            let mut named = false;
            for (lead, files) in &leads {
                let forms = [
                    format!("`{stem}.rs`"),
                    format!("`{stem}/`"),
                    format!("`meada-core/src/{stem}.rs`"),
                ];
                // `meada-core` shares file names with the binary's crate: its lines say so.
                if (lead.contains("meada-core") != (dir != "src"))
                    || !forms.iter().any(|f| lead.contains(f.as_str()))
                {
                    continue;
                }
                named = true;
                // The linked file holds the module's detail: it names the module as the line does.
                let written: Vec<String> = (forms.iter())
                    .filter(|f| lead.contains(f.as_str()))
                    .map(|f| f.trim_matches('`').to_string())
                    .collect();
                for file in files {
                    // A split area file keeps its detail in the parts of its directory (#644).
                    let area = format!("docs/agents/map/{file}");
                    let parts = area.trim_end_matches(".md");
                    let named_in = |text: &str| written.iter().any(|w| text.contains(w.as_str()));
                    match fs::read_to_string(&area) {
                        Ok(text) if !named_in(&text) && fs::read_dir(parts).is_ok() => {
                            let mut split = (agents_markdown().into_iter())
                                .filter(|(p, _)| p.starts_with(&format!("{parts}/")));
                            if !split.any(|(_, text)| named_in(&text)) {
                                unnamed.push(format!(
                                    "{dir}/{stem}: map/{file} and its parts never name {written:?}"
                                ));
                            }
                        }
                        Err(_) => unnamed.push(format!(
                            "{dir}/{stem}: links map/{file}, which does not exist"
                        )),
                        Ok(text) if !named_in(&text) => unnamed
                            .push(format!("{dir}/{stem}: map/{file} never names {written:?}")),
                        Ok(_) => {}
                    }
                }
            }
            if !named {
                missing.push(format!("{dir}/{stem}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "modules with no line in the architecture map naming their area file: {missing:?}"
    );
    assert!(
        unnamed.is_empty(),
        "module lines linking an area file that lacks the module: {unnamed:#?}"
    );
}

#[test]
fn a_module_lead_is_what_a_bullet_with_an_area_link_names_before_its_colon() {
    let map = "- `a.rs`: one. [x](map/x.md)\n- `b.rs`: no link\n- `e.rs` no colon [z](map/z.md)\n- `c.rs`, `d/`: two,\n  wrapped. [y](map/y.md)\n";
    assert_eq!(
        module_leads(map),
        vec![
            ("- `a.rs`".to_string(), vec!["x.md".to_string()]),
            ("- `c.rs`, `d/`".to_string(), vec!["y.md".to_string()]),
        ]
    );
}

/// `CHANGELOG.md merge=union` (#576): two branches adding a bullet under the same heading merge
/// clean and keep both.
#[test]
fn changelog_union_merge_keeps_both_bullets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("changelog-union-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::copy(root.join(".gitattributes"), dir.join(".gitattributes")).unwrap();
    let git = |args: &[&str]| {
        crate::common::git(&dir, args);
    };
    let write = |bullet: &str| {
        let text = format!("## [Unreleased]\n\n### Added\n\n- base\n{bullet}");
        fs::write(dir.join("CHANGELOG.md"), text).unwrap();
    };
    git(&["init", "-q", "-b", "main"]);
    write("");
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    git(&["checkout", "-qb", "side"]);
    write("- side\n");
    git(&["commit", "-qam", "side"]);
    git(&["checkout", "-q", "main"]);
    write("- main\n");
    git(&["commit", "-qam", "main"]);
    git(&["merge", "-q", "--no-edit", "side"]);
    let merged = fs::read_to_string(dir.join("CHANGELOG.md")).unwrap();
    fs::remove_dir_all(&dir).unwrap();
    assert!(
        merged.contains("- main") && merged.contains("- side"),
        "{merged}"
    );
    assert!(!merged.contains("<<<<<<<"), "{merged}");
}

/// Problems of a changelog: a `###` heading twice in one `## [...]` section (what a
/// `merge=union` of two branches that each opened it leaves), and an `[Unreleased]` bullet that
/// names no issue (`#n`) on any of its lines, or repeats an earlier bullet of the section.
fn changelog_problems(text: &str) -> Vec<String> {
    let has_issue = |bullet: &str| {
        bullet
            .split('#')
            .skip(1)
            .any(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    };
    let mut problems = Vec::new();
    let (mut section, mut headings, mut bullet) = (String::new(), BTreeSet::new(), String::new());
    let mut seen = BTreeSet::new();
    let end_bullet = |section: &str,
                      bullet: &mut String,
                      seen: &mut BTreeSet<String>,
                      problems: &mut Vec<String>| {
        if section.contains("[Unreleased]") && !bullet.is_empty() {
            if !has_issue(bullet) {
                problems.push(format!(
                    "{section}: the bullet {bullet:?} names no issue (#n)"
                ));
            }
            if !seen.insert(bullet.clone()) {
                problems.push(format!("{section}: the bullet {bullet:?} appears twice"));
            }
        }
        bullet.clear();
    };
    for line in text.lines().map(str::trim_end) {
        if line.is_empty() || line.starts_with("- ") || line.starts_with('#') {
            end_bullet(&section, &mut bullet, &mut seen, &mut problems);
        }
        if line.starts_with("## ") {
            (section, headings, seen) = (line.to_string(), BTreeSet::new(), BTreeSet::new());
        } else if line.starts_with("### ") {
            if !headings.insert(line.to_string()) {
                problems.push(format!("{section}: the heading `{line}` appears twice"));
            }
        } else if line.starts_with("- ") || (line.starts_with(' ') && !bullet.is_empty()) {
            bullet.push_str(line.trim());
            bullet.push(' ');
        }
    }
    end_bullet(&section, &mut bullet, &mut seen, &mut problems);
    problems
}

#[test]
fn the_changelog_has_one_heading_of_each_kind_per_section_and_an_issue_on_every_unreleased_bullet()
{
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
    let problems = changelog_problems(&text);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_repeated_heading_or_an_unreleased_bullet_without_an_issue_is_a_problem() {
    let ok = "## [Unreleased]\n\n### Added\n\n- one\n  more (#1)\n\n### Fixed\n\n- two (#2)\n\n\
              ## [1.0.0]\n\n### Fixed\n\n- old, no issue\n";
    assert_eq!(changelog_problems(ok), Vec::<String>::new());
    let twice = format!("{ok}\n### Fixed\n\n- again\n");
    assert_eq!(
        changelog_problems(&twice),
        ["## [1.0.0]: the heading `### Fixed` appears twice"]
    );
    let bare = "## [Unreleased]\n\n### Fixed\n\n- a (#1)\n- b\n  still none\n### Fixed\n";
    let found = changelog_problems(bare);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].contains("b still none ") && found[0].contains("[Unreleased]"));
    assert!(found[1].contains("### Fixed") && found[1].contains("[Unreleased]"));
    let copied = "## [Unreleased]\n\n### Added\n\n- a\n  (#1)\n\n### Fixed\n\n- a\n  (#1)\n\n## [1.0.0]\n\n- a (#1)\n- a (#1)\n";
    let found = changelog_problems(copied);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("a (#1) ") && found[0].contains("appears twice"));
    let same = "## [Unreleased]\n\n### Added\n\n- a\n  (#1)\n\n- a\n  (#1)\n";
    let found = changelog_problems(same);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("a (#1) ") && found[0].contains("appears twice"));
}

#[test]
fn schema_version_matches_the_migrations() {
    let source = fs::read_to_string("src/postgres.rs").unwrap();
    let list = source
        .split("const MIGRATIONS: &[&str] = &[")
        .nth(1)
        .and_then(|rest| rest.split("\n];").next())
        .unwrap();
    // Each entry is a string literal opening at the list's own indentation.
    let count = list.lines().filter(|l| l.starts_with("    \"")).count();
    assert_eq!(
        count as u32,
        crate::common::SCHEMA_VERSION,
        "SCHEMA_VERSION in tests/it/common/mod.rs must equal the entries of MIGRATIONS"
    );
}

#[test]
fn source_comments_use_ponytail_instead_of_shortcut() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    let mut problems = Vec::new();
    for (path, text) in limit_sources() {
        let tree = parser.parse(&text, None).unwrap();
        let mut pending = vec![tree.root_node()];
        while let Some(node) = pending.pop() {
            if matches!(node.kind(), "line_comment" | "block_comment") {
                for (offset, line) in text[node.byte_range()].lines().enumerate() {
                    if line.contains("shortcut:") {
                        let line = node.start_position().row + offset + 1;
                        problems.push(format!(
                            "{path}:{line}: use `ponytail:` instead of `shortcut:` (CODING_STANDARDS.md)"
                        ));
                    }
                }
            } else {
                pending.extend(node.children(&mut node.walk()));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
