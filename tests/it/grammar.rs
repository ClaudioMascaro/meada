//! The normative output grammar validates every rendered-answer golden (#145).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Deserialize;

const REFERENCE: &str = "docs/reference/output-grammar.md";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum State {
    Search,
    Context,
    Community,
    Processes,
    Source,
    Unavailable,
    Callers,
    CallerRows,
    Callees,
    CalleeRows,
    ConsumerRows,
    Contracts,
    Sides,
    Details,
    Choice,
    Trace,
    TraceRows,
    TraceCut,
    AfterTrace,
    Done,
}

impl State {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Search
                | Self::CalleeRows
                | Self::ConsumerRows
                | Self::Contracts
                | Self::Sides
                | Self::Details
                | Self::Choice
                | Self::AfterTrace
                | Self::Done
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    kind: String,
    states: Vec<State>,
    pattern: String,
    next: Option<State>,
}

struct Grammar(Vec<(Rule, Regex)>);

#[derive(Debug)]
struct Failure {
    at: usize,
    why: String,
}

impl Failure {
    fn line(at: usize, state: State, line: &str) -> Self {
        Self {
            at,
            why: format!("{state:?}: undocumented or misplaced line {line:?}"),
        }
    }
}

fn reference() -> String {
    fs::read_to_string(REFERENCE).expect("the normative output grammar exists")
}

fn rule_source(doc: &str) -> Result<&str, String> {
    doc.split_once("```json meada-output-grammar\n")
        .and_then(|(_, rest)| rest.split_once("\n```"))
        .map(|(rules, _)| rules)
        .ok_or_else(|| "missing executable grammar fence".into())
}

impl Grammar {
    fn parse(doc: &str) -> Result<Self, String> {
        let rules: Vec<Rule> =
            serde_json::from_str(rule_source(doc)?).map_err(|e| e.to_string())?;
        let mut documented = BTreeMap::new();
        for line in doc.lines().filter_map(|l| l.strip_prefix("| `")) {
            let (kind, rest) = line.split_once("` | ").ok_or("malformed line-kind table")?;
            let states = rest.split_once(" | ").ok_or("missing state column")?.0;
            let states: Result<BTreeSet<State>, _> = states
                .split(", ")
                .map(|s| {
                    let name = s.trim_matches('`');
                    serde_json::from_value(serde_json::json!(name))
                })
                .collect();
            if documented
                .insert(kind, states.map_err(|e| e.to_string())?)
                .is_some()
            {
                return Err(format!("duplicate prose kind {kind}"));
            }
        }
        let executable: BTreeMap<_, _> = rules
            .iter()
            .map(|r| {
                (
                    r.kind.as_str(),
                    r.states.iter().copied().collect::<BTreeSet<_>>(),
                )
            })
            .collect();
        if executable.is_empty() || executable.len() != rules.len() || executable != documented {
            return Err("prose/executable kind or state inventory differs".into());
        }
        for (kind, captures) in [
            ("symbol-header", &["first", "last"][..]),
            ("cut", &["shown", "total"][..]),
            ("source", &[][..]),
        ] {
            let rule = rules
                .iter()
                .find(|r| r.kind == kind)
                .ok_or(format!("missing {kind}"))?;
            let pattern = Regex::new(&rule.pattern).map_err(|e| e.to_string())?;
            if captures
                .iter()
                .any(|name| !pattern.capture_names().flatten().any(|n| n == *name))
            {
                return Err(format!("{kind} must capture {captures:?}"));
            }
        }
        rules
            .into_iter()
            .map(|r| {
                if !r.pattern.starts_with('^')
                    || !r.pattern.ends_with('$')
                    || (r.kind != "source"
                        && (r.pattern.contains(".*") || r.pattern.contains(".+")))
                    || (r.states.contains(&State::Source)
                        && (r.kind != "source" || r.states.len() != 1))
                {
                    return Err(format!("{}: unanchored or opaque structural rule", r.kind));
                }
                let regex = Regex::new(&r.pattern).map_err(|e| format!("{}: {e}", r.kind))?;
                Ok((r, regex))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    fn check(&self, file: &str, tool: State, text: &str) -> Result<(), String> {
        let lines: Vec<_> = text.lines().collect();
        if !text.ends_with('\n') || lines.is_empty() {
            return Err(format!(
                "{file}:1: answer must be nonempty and newline terminated"
            ));
        }
        self.walk(&lines, tool, 0, tool, None)
            .map_err(|e| format!("{file}:{}: {}", e.at + 1, e.why))
    }

    fn walk(
        &self,
        lines: &[&str],
        initial: State,
        mut at: usize,
        mut state: State,
        mut span: Option<usize>,
    ) -> Result<(), Failure> {
        while let Some(line) = lines.get(at) {
            if state == State::Source {
                return self.source(lines, initial, at, span);
            }
            if line.is_empty() && state.terminal() {
                state = initial;
                span = None;
                at += 1;
                continue;
            }
            let Some((rule, regex)) = self
                .0
                .iter()
                .find(|(r, re)| r.states.contains(&state) && re.is_match(line))
            else {
                return Err(Failure::line(at, state, line));
            };
            if rule.kind == "symbol-header" {
                let captures = regex.captures(line).unwrap();
                let first: usize = captures["first"]
                    .parse()
                    .map_err(|_| Failure::line(at, state, line))?;
                let last: usize = captures["last"]
                    .parse()
                    .map_err(|_| Failure::line(at, state, line))?;
                span = last.checked_sub(first).and_then(|n| n.checked_add(1));
                if span.is_none() {
                    return Err(Failure::line(at, state, line));
                }
            }
            let trace_cut = rule.kind == "cut" && matches!(state, State::Trace | State::TraceRows);
            state = if trace_cut {
                State::TraceCut
            } else {
                rule.next.unwrap_or(state)
            };
            at += 1;
        }
        if state.terminal() {
            Ok(())
        } else {
            Err(Failure::line(at, state, "<end of answer>"))
        }
    }

    fn source(
        &self,
        lines: &[&str],
        initial: State,
        at: usize,
        span: Option<usize>,
    ) -> Result<(), Failure> {
        let Some(total) = span else {
            return self.walk(lines, initial, at, State::Unavailable, None);
        };
        let (_, opaque) = self.0.iter().find(|(r, _)| r.kind == "source").unwrap();
        let (_, cut) = self.0.iter().find(|(r, _)| r.kind == "cut").unwrap();
        let mut candidates = Vec::new();
        let mut failure = Failure::line(at, State::Source, lines[at]);
        if let Some(end) = at.checked_add(total).filter(|end| *end <= lines.len())
            && lines[at..end].iter().all(|line| opaque.is_match(line))
        {
            candidates.push(end);
        }
        for shown in 0..total.min(lines.len() - at) {
            let Some(captures) = cut.captures(lines[at + shown]) else {
                continue;
            };
            if captures["shown"].parse::<usize>() == Ok(shown)
                && captures["total"].parse::<usize>() == Ok(total)
                && lines[at..at + shown]
                    .iter()
                    .all(|line| opaque.is_match(line))
            {
                candidates.push(at + shown + 1);
            } else {
                failure = Failure {
                    at: at + shown,
                    why: "Source: cut counts disagree with header/prefix".into(),
                };
            }
        }
        candidates.push(at);
        for next in candidates {
            match self.walk(lines, initial, next, State::Callers, None) {
                Ok(()) => return Ok(()),
                Err(error) if error.at >= failure.at => failure = error,
                Err(_) => {}
            }
        }
        Err(failure)
    }
}

fn goldens(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            goldens(&path, out);
        } else if path.extension().is_some_and(|e| e == "txt") {
            out.push(path);
        }
    }
}

fn tool(path: &Path) -> Result<Option<State>, String> {
    let name = path.file_name().unwrap().to_str().unwrap();
    for (marker, state) in [
        (".search-", State::Search),
        (".context-", State::Context),
        (".trace-", State::Trace),
    ] {
        if name.contains(marker) {
            return Ok(Some(state));
        }
    }
    if name == "schema.txt"
        || name.starts_with("cli.")
        || name.starts_with("surface.cli.")
        || [".status", ".workspace-status", ".decide.", ".decider."]
            .iter()
            .any(|s| name.contains(s))
    {
        return Ok(None);
    }
    Err(format!(
        "{}:1: unclassified text golden; name query answers .search-, .context- or .trace-",
        path.display()
    ))
}

#[test]
fn the_prose_and_executable_line_kinds_stay_in_sync() {
    Grammar::parse(&reference()).unwrap();
}

#[test]
fn every_rendered_answer_golden_has_documented_lines() {
    let grammar = Grammar::parse(&reference()).unwrap();
    let mut files = Vec::new();
    goldens(Path::new("tests/golden"), &mut files);
    files.sort();
    let mut counts = BTreeMap::<State, usize>::new();
    for path in files {
        if let Some(state) = tool(&path).unwrap() {
            grammar
                .check(
                    path.to_str().unwrap(),
                    state,
                    &fs::read_to_string(&path).unwrap(),
                )
                .unwrap();
            *counts.entry(state).or_default() += 1;
        }
    }
    for state in [State::Search, State::Context, State::Trace] {
        assert!(
            counts.get(&state).is_some_and(|n| *n > 0),
            "{state:?} has no goldens"
        );
    }
}

#[test]
fn an_unknown_level_cannot_hide_in_a_relation_name() {
    let grammar = Grammar::parse(&reference()).unwrap();
    let text = fs::read_to_string("tests/golden/ts-basic.context-add.txt").unwrap();
    let bad = text.replacen("src/math.ts:8 sum", "src/math.ts:8 sum [invented: bad]", 1);
    let error = grammar
        .check("ts-basic.context-add.txt", State::Context, &bad)
        .unwrap_err();
    assert!(error.contains("ts-basic.context-add.txt:8:"), "{error}");
}

#[test]
fn unknown_context_lines_report_the_golden_and_line() {
    let grammar = Grammar::parse(&reference()).unwrap();
    let text = fs::read_to_string("tests/golden/ts-basic.context-add.txt").unwrap();
    for (bad, line) in [
        (
            text.replacen("callers:\n", "callers:\nundocumented: surprise\n", 1),
            8,
        ),
        (text.replacen("community:", "cluster:", 1), 2),
    ] {
        let error = grammar
            .check("ts-basic.context-add.txt", State::Context, &bad)
            .unwrap_err();
        assert!(
            error.contains(&format!("ts-basic.context-add.txt:{line}:")),
            "{error}"
        );
    }
}

#[test]
fn trace_levels_and_footer_fields_are_structural() {
    let grammar = Grammar::parse(&reference()).unwrap();
    let text = fs::read_to_string("tests/golden/chain.trace-resource-down.txt").unwrap();
    for (bad, line) in [
        (text.replacen("→[declared:", "→[invented:", 1), 2),
        (text.replacen("chain: declared", "chain: invented", 1), 5),
        (text.replacen("86 tokens", "many tokens", 1), 5),
        (
            text.lines()
                .filter(|l| !l.starts_with("chain:"))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
            5,
        ),
    ] {
        let error = grammar
            .check("chain.trace-resource-down.txt", State::Trace, &bad)
            .unwrap_err();
        assert!(
            error.contains(&format!("chain.trace-resource-down.txt:{line}:")),
            "{error}"
        );
    }
}

#[test]
fn source_is_opaque_only_for_the_counted_span() {
    let grammar = Grammar::parse(&reference()).unwrap();
    let text = "local/web src/raw.ts:1-3 f\ncommunity: none\nprocesses: none\n\
                callers:\n\ncommunity: raw_source\ncallers: none\ncallees: none\n";
    grammar
        .check("counted.context-raw.txt", State::Context, text)
        .unwrap();
    let bad = text.replace("callers: none\n", "callers: none\nunknown: tail\n");
    let error = grammar
        .check("counted.context-raw.txt", State::Context, &bad)
        .unwrap_err();
    assert!(error.contains("counted.context-raw.txt:8:"), "{error}");

    let cut = fs::read_to_string("tests/golden/chain.context-processes-span-cut.txt").unwrap();
    let bad = cut.replace("after 4 of 12", "after 5 of 12");
    let error = grammar
        .check("cut.context-span.txt", State::Context, &bad)
        .unwrap_err();
    assert!(error.contains("cut.context-span.txt:8:"), "{error}");
}

#[test]
fn an_unavailable_span_cannot_hide_structural_lines() {
    let grammar = Grammar::parse(&reference()).unwrap();
    let text = "local/web src/raw.ts:1 f\ncommunity: none\nprocesses: none\n\
                (span text unavailable: src/raw.ts changed since indexing)\n\
                callers: none\ncallees: none\n";
    grammar
        .check("unavailable.context-raw.txt", State::Context, text)
        .unwrap();
    let bad = text.replace(
        "(span text unavailable:",
        "callers:\n(span text unavailable:",
    );
    let error = grammar
        .check("unavailable.context-raw.txt", State::Context, &bad)
        .unwrap_err();
    assert!(error.contains("unavailable.context-raw.txt:4:"), "{error}");
}

#[test]
fn changing_only_the_prose_or_executable_inventory_is_rejected() {
    let doc = reference();
    let source = rule_source(&doc).unwrap();
    let mut rules: Vec<serde_json::Value> = serde_json::from_str(source).unwrap();
    rules.retain(|r| r["kind"] != "community");
    let removed = doc.replacen(source, &serde_json::to_string(&rules).unwrap(), 1);
    assert!(
        Grammar::parse(&removed)
            .err()
            .unwrap()
            .contains("inventory differs")
    );
    let prose_only = doc.clone() + "\n| `new-kind` | `search` | Undocumented in the checker. |\n";
    assert!(
        Grammar::parse(&prose_only)
            .err()
            .unwrap()
            .contains("inventory differs")
    );
    let mut rules: Vec<serde_json::Value> = serde_json::from_str(source).unwrap();
    rules.push(
        serde_json::json!({"kind": "new-kind", "states": ["search"], "pattern": "^new-kind$"}),
    );
    let added = doc.replacen(source, &serde_json::to_string(&rules).unwrap(), 1);
    assert!(
        Grammar::parse(&added)
            .err()
            .unwrap()
            .contains("inventory differs")
    );
    let different_state = doc.replacen(
        "| `community` | `community` |",
        "| `community` | `search` |",
        1,
    );
    assert!(
        Grammar::parse(&different_state)
            .err()
            .unwrap()
            .contains("inventory differs")
    );
}

#[test]
fn a_structural_fallback_cannot_accept_arbitrary_lines() {
    let doc = reference();
    let source = rule_source(&doc).unwrap();
    let mut rules: Vec<serde_json::Value> = serde_json::from_str(source).unwrap();
    rules.iter_mut().find(|r| r["kind"] == "relation").unwrap()["pattern"] = "^.*$".into();
    let bad = doc.replacen(source, &serde_json::to_string(&rules).unwrap(), 1);
    assert!(
        Grammar::parse(&bad)
            .err()
            .unwrap()
            .contains("opaque structural rule")
    );
}

#[test]
fn new_text_goldens_cannot_silently_escape_classification() {
    let error = tool(Path::new("tests/golden/future.answer.txt")).unwrap_err();
    assert!(error.contains("future.answer.txt:1:"), "{error}");
}

#[test]
fn all_method_providers_use_any_in_the_contract_identifier() {
    let grammar = Grammar::parse(&reference()).unwrap();
    let text = "http::ANY::/health\n  local/api provider src/api.py:1 health\n";
    grammar
        .check("any.context-route.txt", State::Context, text)
        .unwrap();
    let bad = text.replace("::ANY::", "::*::");
    let error = grammar
        .check("any.context-route.txt", State::Context, &bad)
        .unwrap_err();
    assert!(error.contains("any.context-route.txt:1:"), "{error}");
}
