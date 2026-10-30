//! The output grammar of `docs/reference/output-grammar.md` against every rendered-answer golden:
//! the document's tables are the grammar, so the check reads its line kinds from them. Every line
//! of a `search`, `context` or `trace` golden matches a line kind of its tool, every line kind is
//! shown by a golden (or named in `UNSHOWN`), and the document's levels, `search` kinds and entry
//! kinds are the code's.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;

use crate::common::fixture_repo;
use meada::engine::{Engine, ItemKind};
use meada_core::export::{EntryKind, Level};

const GRAMMAR: &str = "docs/reference/output-grammar.md";

/// The tools whose answers the grammar covers, as golden file names spell them.
const TOOLS: [&str; 3] = ["search", "context", "trace"];

/// The terminals the matcher implements (`Grammar::ends`).
const TERMINALS: [&str; 4] = ["word", "number", "text", "indent"];

/// Line kinds no golden shows, each with the test that pins it instead.
const UNSHOWN: &[(&str, &str)] = &[
    // Only a file changing between the refresh and the read gives it: the unit test
    // `engine::tests::a_span_changed_since_indexing_keeps_the_start_alone_in_the_first_line`.
    ("context", "unavailable"),
];

/// Forms of a field that no golden can show, left out when the goldens are checked: a fixture's
/// files are all readable, so a place is always `<path>:<line>` there.
const UNSHOWN_FORMS: &[(&str, &str)] = &[("place", "<path>")];

/// One piece of a pattern: text that stands for itself, or a field, optional with `?`.
#[derive(Debug, Clone)]
enum Item {
    Text(String),
    Field { name: String, optional: bool },
}

/// A line kind: the tool it belongs to (`None`: every tool), its name and its pattern.
struct Kind {
    tool: Option<String>,
    name: String,
    pattern: Vec<Item>,
}

/// The document's grammar: its line kinds, its fields with their forms, its terminals, and the
/// words of its `search` kind table.
struct Grammar {
    kinds: Vec<Kind>,
    /// Each field's forms, as written and as items.
    fields: HashMap<String, Vec<(String, Vec<Item>)>>,
    /// Field names in the order the document defines their forms, each once.
    order: Vec<String>,
    terminals: Vec<String>,
    search_kinds: Vec<String>,
    /// The line kinds each section of a symbol's `context` holds, by the section's name (`None`:
    /// before the first section).
    sections: HashMap<Option<String>, Vec<String>>,
}

/// What the check found: the lines no kind matches, as `<golden>:<line number>: <line>`, and
/// the line kinds some line matched, as `(tool, name)` (`every` for a kind of every tool).
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    shown: HashSet<(String, String)>,
}

/// The content of the code span that is all of `cell`, as CommonMark reads it: one space is
/// stripped from each end when both ends have one.
fn code(cell: &str) -> Option<String> {
    let cell = cell.trim();
    let ticks = cell.chars().take_while(|&c| c == '`').count();
    if ticks == 0 || cell.len() < 2 * ticks {
        return None;
    }
    let fence = &cell[..ticks];
    let inner = cell[ticks..].strip_suffix(fence)?;
    // A run of as many backticks inside would close the span early: the cell is no one span.
    let runs = inner.split(|c| c != '`').filter(|run| !run.is_empty());
    if inner.ends_with('`') || runs.into_iter().any(|run| run.len() == ticks) {
        return None;
    }
    let stripped = inner.strip_prefix(' ').and_then(|i| i.strip_suffix(' '));
    Some(match stripped {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => inner.to_string(),
    })
}

/// `pattern` as items: `<name>` and `<name?>` are fields when `name` is lowercase letters,
/// digits and hyphens starting with a letter; any other `<` is text.
fn items(pattern: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut text = String::new();
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        let field = rest.strip_prefix('<').and_then(|r| {
            let end = r.find('>')?;
            let (name, optional) = match r[..end].strip_suffix('?') {
                Some(name) => (name, true),
                None => (&r[..end], false),
            };
            let named = name.starts_with(|c: char| c.is_ascii_lowercase())
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            named.then(|| (name.to_string(), optional, end + 2))
        });
        match field {
            Some((name, optional, len)) => {
                if !text.is_empty() {
                    items.push(Item::Text(std::mem::take(&mut text)));
                }
                items.push(Item::Field { name, optional });
                rest = &rest[len..];
            }
            None => {
                text.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    if !text.is_empty() {
        items.push(Item::Text(text));
    }
    items
}

/// `name` of the field `<name>`.
fn unbracketed(field: &str) -> String {
    field
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

/// The cells of a table row, split at every pipe: no pattern of the document holds one.
fn cells(row: &str) -> Vec<String> {
    let inner = row.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

impl Grammar {
    /// The grammar the tables of `doc` give. A table's header says what it holds; a line kind
    /// belongs to the tool its `## ` section names (`## Every tool`: all of them).
    fn parse(doc: &str) -> Self {
        let mut grammar = Grammar {
            kinds: Vec::new(),
            fields: HashMap::new(),
            order: Vec::new(),
            terminals: Vec::new(),
            search_kinds: Vec::new(),
            sections: HashMap::new(),
        };
        let mut section = String::new();
        let lines: Vec<&str> = doc.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            if let Some(title) = line.strip_prefix("## ") {
                section = title.trim().to_string();
            }
            let header = lines.get(i + 1).is_some_and(|l| l.starts_with("| ---"));
            if !(line.starts_with('|') && header) {
                i += 1;
                continue;
            }
            let head = cells(line);
            let head: Vec<&str> = head.iter().map(String::as_str).collect();
            i += 2;
            while let Some(row) = lines.get(i).filter(|l| l.starts_with('|')) {
                let row = cells(row);
                let form = |n: usize| code(&row[n]).unwrap_or_else(|| panic!("{row:?}: no code"));
                match head[..] {
                    ["Terminal", "Matches"] => grammar.terminals.push(unbracketed(&form(0))),
                    ["Field", "Form", "Meaning"] => {
                        let name = unbracketed(&form(0));
                        let name = name.as_str();
                        if !grammar.fields.contains_key(name) {
                            grammar.order.push(name.to_string());
                        }
                        let forms = grammar.fields.entry(name.to_string()).or_default();
                        forms.push((form(1), items(&form(1))));
                    }
                    ["Kind", "Line", "Meaning"] => {
                        let tool = match section.trim_matches('`') {
                            "Every tool" => None,
                            tool if TOOLS.contains(&tool) => Some(tool.to_string()),
                            other => panic!("line kinds under `## {other}`, no tool"),
                        };
                        grammar.kinds.push(Kind {
                            tool,
                            name: row[0].clone(),
                            pattern: items(&form(1)),
                        });
                    }
                    ["`kind`", "Lists"] => grammar.search_kinds.push(form(0)),
                    ["Section", "Line kinds"] => {
                        let section = code(&row[0]).map(|s| s.trim_end_matches(':').to_string());
                        let kinds = row[1].split(", ").map(str::to_string).collect();
                        grammar.sections.insert(section, kinds);
                    }
                    _ => {}
                }
                i += 1;
            }
        }
        grammar
    }

    /// The positions of `line` that matching `items` from any of `starts` can end at.
    fn ends(&self, items: &[Item], line: &str, starts: Vec<bool>) -> Vec<bool> {
        items.iter().fold(starts, |at, item| match item {
            Item::Text(text) => {
                let mut next = vec![false; at.len()];
                for p in (0..at.len()).filter(|&p| at[p]) {
                    if line[p..].starts_with(text.as_str()) {
                        next[p + text.len()] = true;
                    }
                }
                next
            }
            Item::Field { name, optional } => {
                let mut next = self.field(name, line, &at);
                if *optional {
                    next.iter_mut().zip(&at).for_each(|(n, a)| *n |= a);
                }
                next
            }
        })
    }

    /// The positions matching the field `name` from any of `at` ends at.
    fn field(&self, name: &str, line: &str, at: &[bool]) -> Vec<bool> {
        let mut next = vec![false; at.len()];
        // A run of characters each `fits`, at least one: every end after a start within it.
        let run = |fits: &dyn Fn(char) -> bool, next: &mut Vec<bool>| {
            let mut open = false;
            for (p, c) in line.char_indices() {
                open = (open || at[p]) && fits(c);
                if open {
                    next[p + c.len_utf8()] = true;
                }
            }
        };
        match name {
            "word" => run(&|c| c != ' ', &mut next),
            "number" => run(&|c| c.is_ascii_digit(), &mut next),
            "text" => run(&|_| true, &mut next),
            "indent" => {
                for p in (0..at.len()).filter(|&p| at[p]) {
                    let mut q = p;
                    next[q] = true;
                    while line[q..].starts_with("  ") {
                        q += 2;
                        next[q] = true;
                    }
                }
            }
            _ => {
                let forms = (self.fields.get(name)).unwrap_or_else(|| panic!("no field <{name}>"));
                for (_, form) in forms {
                    let ends = self.ends(form, line, at.to_vec());
                    next.iter_mut().zip(ends).for_each(|(n, e)| *n |= e);
                }
            }
        }
        next
    }

    /// Whether `kind` matches all of `line`.
    fn matches(&self, kind: &Kind, line: &str) -> bool {
        let mut start = vec![false; line.len() + 1];
        start[0] = true;
        self.ends(&kind.pattern, line, start)[line.len()]
    }

    /// The kinds of `tool` (its own and every tool's) that match `line`.
    fn kinds_of<'g>(&'g self, tool: &str, line: &str) -> Vec<&'g Kind> {
        (self.kinds.iter())
            .filter(|k| k.tool.as_deref().is_none_or(|t| t == tool))
            .filter(|k| self.matches(k, line))
            .collect()
    }

    /// The grammar without the field forms `forms` names, `(field, form as written)`, each of
    /// which it has.
    fn without(mut self, forms: &[(&str, &str)]) -> Self {
        for &(name, written) in forms {
            let field = (self.fields.get_mut(name)).unwrap_or_else(|| panic!("no field <{name}>"));
            let before = field.len();
            field.retain(|(form, _)| form != written);
            assert_eq!(field.len() + 1, before, "<{name}> has no form `{written}`");
        }
        self
    }

    /// The kinds each line of the sections of a symbol's `context` matches, from `from` to the
    /// end of the answer: a section line opens its section, and every other line matches the
    /// kinds the section lists (or a kind of every tool). No kind: the line fails.
    fn sections(&self, lines: &[&str], from: usize) -> Vec<Vec<&Kind>> {
        let opens = [
            self.kind("context", "section"),
            self.kind("context", "empty section"),
        ];
        let mut section = None;
        let mut out = Vec::new();
        for line in lines[from..].iter().take_while(|l| !l.is_empty()) {
            if opens.iter().any(|k| self.matches(k, line)) {
                section = line.split(':').next().map(str::to_string);
                out.push(
                    opens
                        .into_iter()
                        .filter(|k| self.matches(k, line))
                        .collect(),
                );
                continue;
            }
            let listed = (self.sections.get(&section))
                .unwrap_or_else(|| panic!("no section `{section:?}` in {GRAMMAR}"));
            let kinds = (self.kinds_of("context", line).into_iter())
                .filter(|k| k.tool.is_none() || listed.contains(&k.name))
                .collect();
            out.push(kinds);
        }
        out
    }

    fn kind(&self, tool: &str, name: &str) -> &Kind {
        (self.kinds.iter())
            .find(|k| k.tool.as_deref() == Some(tool) && k.name == name)
            .unwrap_or_else(|| panic!("no {tool} line kind `{name}` in {GRAMMAR}"))
    }

    /// The fields every pattern and form names, terminals included.
    fn named(&self) -> BTreeSet<String> {
        let patterns = (self.kinds.iter().map(|k| &k.pattern))
            .chain(self.fields.values().flatten().map(|(_, form)| form));
        (patterns.flatten())
            .filter_map(|item| match item {
                Item::Field { name, .. } => Some(name.clone()),
                Item::Text(_) => None,
            })
            .collect()
    }

    /// The text forms of the field `name`, in the document's order.
    fn words(&self, name: &str) -> Vec<String> {
        (self.fields[name].iter())
            .map(|(_, form)| match &form[..] {
                [Item::Text(word)] => word.clone(),
                other => panic!("<{name}> has a form that is no word: {other:?}"),
            })
            .collect()
    }
}

/// The rendered-answer goldens, `(file name, tool)`: those whose name has a part after the
/// fixture that is a tool, alone or with a case (`chain.trace-down.txt`, `jvm-packages.search.txt`).
fn rendered() -> Vec<(String, &'static str)> {
    let mut goldens: Vec<(String, &str)> = (fs::read_dir("tests/golden").unwrap())
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|name| {
            let parts: Vec<&str> = name.strip_suffix(".txt")?.split('.').skip(1).collect();
            let tool = TOOLS.into_iter().find(|tool| {
                (parts.iter()).any(|p| p == tool || p.starts_with(&format!("{tool}-")))
            })?;
            Some((name, tool))
        })
        .collect();
    goldens.sort();
    goldens
}

/// Checks each golden `(name, tool, text)` against `grammar`. A blank line outside a symbol's
/// span text separates two answers a test wrote into one golden. A symbol's answer in `context`
/// is the line before a community line, that line and the processes line, the span text, then
/// the sections: the span text is verbatim and matches no kind (its length is the head's range,
/// shorter when its cut note follows it or when it is left out), and a line under a section
/// matches a kind the document lists for that section.
fn check(grammar: &Grammar, goldens: &[(String, &str, String)]) -> Report {
    let mut report = Report::default();
    for (name, tool, text) in goldens {
        let lines: Vec<&str> = text.lines().collect();
        let found = |n: usize, kinds: Vec<&Kind>, report: &mut Report| {
            if kinds.is_empty() {
                report
                    .failures
                    .push(format!("{name}:{}: {}", n + 1, lines[n]));
            }
            for kind in kinds {
                report
                    .shown
                    .insert((tool_of(kind).into(), kind.name.clone()));
            }
        };
        let community = grammar.kind("context", "community");
        let mut n = 0;
        while n < lines.len() {
            let symbol = *tool == "context"
                && n + 2 < lines.len()
                && grammar.matches(community, lines[n + 1]);
            if !symbol {
                if !lines[n].is_empty() {
                    found(n, grammar.kinds_of(tool, lines[n]), &mut report);
                }
                n += 1;
                continue;
            }
            for (k, line) in lines.iter().enumerate().skip(n).take(3) {
                found(k, grammar.kinds_of(tool, line), &mut report);
            }
            let start = n + 3;
            let head = grammar.matches(grammar.kind("context", "head"), lines[n]);
            let length = if head { range(lines[n]) } else { 0 };
            let cut = (0..length).find(|&k| {
                (lines.get(start + k))
                    .is_some_and(|l| l.starts_with(&format!("… cut after {k} of {length} lines ")))
            });
            let whole = (start + length <= lines.len()).then_some(length);
            let candidates: Vec<usize> = cut.into_iter().chain(whole).chain([0]).collect();
            let fits = |c: &usize| {
                grammar
                    .sections(&lines, start + c)
                    .iter()
                    .all(|k| !k.is_empty())
            };
            let span = *candidates
                .iter()
                .find(|c| fits(c))
                .unwrap_or(&candidates[0]);
            let tail = grammar.sections(&lines, start + span);
            for (k, kinds) in tail.iter().enumerate() {
                found(start + span + k, kinds.clone(), &mut report);
            }
            n = start + span + tail.len();
        }
    }
    report
}

/// The number of lines `<path>:<line>-<last>` names in a `context` head.
fn range(head: &str) -> usize {
    let place = head.split(' ').nth(1).unwrap_or_default();
    let (_, range) = place.rsplit_once(':').unwrap_or_default();
    let (first, last) = range.split_once('-').unwrap_or_default();
    let (first, last): (usize, usize) = (first.parse().unwrap(), last.parse().unwrap());
    (last + 1).saturating_sub(first)
}

/// The tool `kind` belongs to, `every` for a kind of every tool.
fn tool_of(kind: &Kind) -> &str {
    kind.tool.as_deref().unwrap_or("every")
}

fn goldens() -> Vec<(String, &'static str, String)> {
    (rendered().into_iter())
        .map(|(name, tool)| {
            let text = fs::read_to_string(format!("tests/golden/{name}")).unwrap();
            (name, tool, text)
        })
        .collect()
}

/// The document's grammar, without the forms no golden can show.
fn grammar() -> Grammar {
    Grammar::parse(&fs::read_to_string(GRAMMAR).unwrap()).without(UNSHOWN_FORMS)
}

#[test]
fn every_line_of_every_rendered_answer_golden_is_of_a_documented_kind() {
    let goldens = goldens();
    // The scan itself must see the goldens of every tool: each has well over 30.
    for tool in TOOLS {
        assert!(
            goldens.iter().filter(|g| g.1 == tool).count() > 30,
            "{tool}"
        );
    }

    let report = check(&grammar(), &goldens);

    assert!(
        report.failures.is_empty(),
        "lines of no line kind in {GRAMMAR}:\n{}",
        report.failures.join("\n")
    );
}

#[test]
fn every_documented_line_kind_is_shown_by_a_golden() {
    let grammar = grammar();

    let report = check(&grammar, &goldens());

    let unshown: Vec<(&str, &str)> = (grammar.kinds.iter())
        .map(|k| (tool_of(k), k.name.as_str()))
        .filter(|(tool, name)| !report.shown.contains(&(tool.to_string(), name.to_string())))
        .filter(|kind| !UNSHOWN.contains(kind))
        .collect();
    assert!(
        unshown.is_empty(),
        "line kinds no golden shows: {unshown:?}"
    );
    for (tool, name) in UNSHOWN {
        grammar.kind(tool, name);
        let shown = report.shown.contains(&(tool.to_string(), name.to_string()));
        assert!(
            !shown,
            "{tool} `{name}` is shown by a golden: take it out of UNSHOWN"
        );
    }
}

#[test]
fn every_field_is_defined_and_used_and_every_terminal_is_the_matchers() {
    let grammar = grammar();

    assert_eq!(grammar.terminals, TERMINALS);
    let named = grammar.named();
    let undefined: Vec<&String> = (named.iter())
        .filter(|n| !grammar.fields.contains_key(*n) && !TERMINALS.contains(&n.as_str()))
        .collect();
    assert!(undefined.is_empty(), "fields with no form: {undefined:?}");
    let unused: Vec<&String> = (grammar.order.iter())
        .filter(|n| !named.contains(*n))
        .collect();
    assert!(unused.is_empty(), "fields no pattern names: {unused:?}");
}

#[test]
fn every_section_of_context_lists_its_line_kinds() {
    let grammar = grammar();

    let listed: BTreeSet<&String> = grammar.sections.keys().flatten().collect();
    let words = grammar.words("section");
    assert_eq!(
        listed,
        words.iter().collect(),
        "the sections of the table and of <section>"
    );
    assert!(
        grammar.sections.contains_key(&None),
        "no row for the lines before `callers:`"
    );
    for name in grammar.sections.values().flatten() {
        grammar.kind("context", name);
    }
}

#[test]
fn the_documented_levels_are_the_code_s_strongest_first() {
    // A level's place, strongest first: a level added to the code stops this compiling.
    let strength = |level: Level| match level {
        Level::Exact => 0,
        Level::Declared => 1,
        Level::Inferred => 2,
        Level::Decided => 3,
        Level::Ambiguous => 4,
    };

    let words = grammar().words("level");

    let levels: Vec<Level> = (words.iter())
        .map(|w| Level::parse(w).unwrap_or_else(|| panic!("`{w}` is no level")))
        .collect();
    assert_eq!(
        levels.iter().map(|&l| strength(l)).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
    assert!(
        levels.is_sorted(),
        "the code orders levels otherwise: {words:?}"
    );
}

#[test]
fn the_documented_entry_kinds_are_the_code_s() {
    let words = grammar().words("entry-kind");

    assert_eq!(words, EntryKind::ALL.map(|k| k.name().to_string()));
}

#[test]
fn the_documented_open_item_kinds_are_the_code_s() {
    // An open item kind added to the code stops this compiling until it is listed.
    let listed = |kind: ItemKind| match kind {
        ItemKind::Ambiguous | ItemKind::Unresolved => kind.name(),
    };

    let words = grammar().words("item-kind");

    let kinds = [ItemKind::Ambiguous, ItemKind::Unresolved];
    assert_eq!(words, kinds.map(listed));
}

#[test]
fn the_documented_search_kinds_are_the_ones_search_takes() {
    let repo = fixture_repo("grammar_search_kinds", "ts-basic", None);
    let engine = Engine::open(&repo).unwrap();
    // The refusal of an unknown kind lists the valid ones: `` `kind` takes `a`, `b` or `c`, ... ``.
    let refusal = engine
        .search("x", Some("nothing"), None)
        .unwrap_err()
        .to_string();
    let valid = refusal.split(", not ").next().unwrap();
    let taken: Vec<String> = (valid.split('`').skip(3).step_by(2))
        .map(str::to_string)
        .collect();

    assert_eq!(grammar().search_kinds, taken, "{refusal}");
}

/// A grammar of one kind per tool, for the tests of the check itself.
const SMALL: &str = "\
| Terminal | Matches |
| --- | --- |
| `<word>` | |
| `<number>` | |
| `<text>` | |
| `<indent>` | |

| Field | Form | Meaning |
| --- | --- | --- |
| `<place>` | `<word>:<number>` | |
| `<mark>` | ` [<word>]` | |

## `search`

| Kind | Line | Meaning |
| --- | --- | --- |
| hit | `<word> <place> <text><mark?>` | |
| none | `` no matches for `<text?>` `` | |

## `context`

| Kind | Line | Meaning |
| --- | --- | --- |
| head | `<word> <word>:<number>-<number> <text>` | |
| community | `community: <word>` | |
| processes | `processes: none` | |
| section | `<word>:` | |
| empty section | `<word>: none` | |
| caller | `<word> <place> <text>` | |

| Section | Line kinds |
| --- | --- |
| (before `callers:`) | |
| `callers:` | caller |

## `trace`

| Kind | Line | Meaning |
| --- | --- | --- |
| row | `<indent><word> <place> <text>` | |

## Every tool

| Kind | Line | Meaning |
| --- | --- | --- |
| cut | `… cut after <number> of <number> lines (budget <number> tokens); raise budget for more` | |
";

fn one(name: &str, tool: &'static str, text: &str) -> Vec<(String, &'static str, String)> {
    vec![(name.to_string(), tool, text.to_string())]
}

#[test]
fn a_line_of_an_undocumented_kind_fails_naming_the_golden_and_the_line() {
    let grammar = Grammar::parse(SMALL);
    let text = "r a.ts:1 add [inferred]\nno matches for ``\nr a.ts:2 sub\nfrobbed: r\n";

    let report = check(&grammar, &one("x.search-a.txt", "search", text));

    assert_eq!(report.failures, ["x.search-a.txt:4: frobbed: r"]);
}

#[test]
fn a_kind_documented_for_another_tool_does_not_match() {
    let grammar = Grammar::parse(SMALL);

    let report = check(
        &grammar,
        &one("x.trace-a.txt", "trace", "no matches for `a`\n"),
    );

    assert_eq!(report.failures, ["x.trace-a.txt:1: no matches for `a`"]);
}

#[test]
fn a_kind_left_out_of_the_document_fails_the_goldens_that_show_it() {
    // The real grammar without its `trace` footer: every chain golden ends in an unknown line.
    let doc = fs::read_to_string(GRAMMAR).unwrap();
    let doc: String = (doc.lines())
        .filter(|l| !l.starts_with("| footer |"))
        .map(|l| format!("{l}\n"))
        .collect();
    let grammar = Grammar::parse(&doc).without(UNSHOWN_FORMS);

    let report = check(&grammar, &goldens());

    assert!(
        report
            .failures
            .iter()
            .any(|f| f.starts_with("chain.trace-down.txt:"))
    );
    assert!(report.failures.iter().all(|f| f.contains(" chain: ")));
}

#[test]
fn span_text_is_verbatim_for_its_range_and_shorter_when_cut() {
    let grammar = Grammar::parse(SMALL);
    let whole = "r a.ts:3-4 f\ncommunity: none\nprocesses: none\n\nanything: at all\ncallers:\n";
    let cut = "r a.ts:3-5 f\ncommunity: none\nprocesses: none\nfn f() {\n\
               … cut after 1 of 3 lines (budget 9 tokens); raise budget for more\ncallers:\n";
    let left_out = "r a.ts:3-9 f\ncommunity: none\nprocesses: none\ncallers:\n";
    let past = "r a.ts:3-3 f\ncommunity: none\nprocesses: none\nfn f() {}\nanything: at all\n";

    for (text, failures) in [
        (whole, &[][..]),
        (cut, &[]),
        (left_out, &[]),
        (past, &["x.context-a.txt:5: anything: at all"]),
    ] {
        let report = check(&grammar, &one("x.context-a.txt", "context", text));

        assert_eq!(report.failures, failures, "{text}");
    }
}

#[test]
fn a_documented_kind_shown_by_no_golden_is_reported() {
    let grammar = Grammar::parse(SMALL);

    let report = check(&grammar, &one("x.search-a.txt", "search", "r a.ts:1 add\n"));

    assert!(report.shown.contains(&("search".into(), "hit".into())));
    assert!(!report.shown.contains(&("search".into(), "none".into())));
}

#[test]
fn a_pattern_reads_fields_optional_fields_and_code_spans() {
    assert_eq!(
        code("`` no `<query>` ``").as_deref(),
        Some("no `<query>`"),
        "a double fence strips one space"
    );
    assert_eq!(code("` [<level>]`").as_deref(), Some(" [<level>]"));
    assert_eq!(code("`a` or `b`"), None, "two spans are no one span");
    let pattern = items("<a> <b?>-<Not> <x y>");
    assert!(matches!(&pattern[..], [
        Item::Field { optional: false, .. },
        Item::Text(_),
        Item::Field { optional: true, .. },
        Item::Text(rest),
    ] if rest == "-<Not> <x y>"));
}
