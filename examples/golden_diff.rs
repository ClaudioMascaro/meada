//! Summarises how an export golden changed between two git revisions, per section.
//!
//! ```text
//! cargo run --example golden_diff -- [golden.json...] [--range <range>]
//! ```
//!
//! `--range A...B` (the default is `origin/main...HEAD`) compares the merge-base of `A` and `B`
//! with `B`. `--range A` alone compares `A` with the file on disk (the working tree). With no
//! golden given, every `tests/golden/*.export.json` that differs between the two sides is
//! compared. A file missing on one side is reported as added or removed.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

const CAP: usize = 20;
const KEYED: &[&str] = &[
    "symbols",
    "calls",
    "providers",
    "consumers",
    "packages",
    "links",
];
const OTHER: &[&str] = &[
    "files",
    "bindings",
    "publishes",
    "dependencies",
    "communities",
    "processes",
];

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn field(v: &Value, name: &str) -> String {
    match v.get(name) {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "null".into(),
    }
}

/// The identity of an entry in a section.
fn key(section: &str, v: &Value) -> String {
    let parts: &[&str] = match section {
        "symbols" | "calls" => &["id"],
        "packages" => &["ecosystem", "directory", "name"],
        "links" => &["consumer", "provider", "contract"],
        "providers" | "consumers" => &["contract", "site"],
        _ => return v.to_string(),
    };
    parts
        .iter()
        .map(|p| field(v, p))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The fields whose change is worth naming, per section.
fn watched(section: &str) -> &'static [&'static str] {
    match section {
        "calls" => &["level", "target", "candidates"],
        "links" => &["level"],
        "packages" => &["version"],
        "consumers" => &["via"],
        _ => &[],
    }
}

/// A short form of a value; a target is named `set` and a list is counted, so moves group.
fn short(f: &str, v: Option<&Value>) -> String {
    let s = match v {
        None | Some(Value::Null) => "null".into(),
        Some(Value::String(_)) if f == "target" => "set".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => format!("[{} items]", a.len()),
        Some(o) => o.to_string(),
    };
    if s.chars().count() > 60 {
        format!("{}...", s.chars().take(57).collect::<String>())
    } else {
        s
    }
}

fn list(prefix: &str, indent: &str, items: &BTreeSet<String>, out: &mut String) {
    for k in items.iter().take(CAP) {
        out.push_str(&format!("{indent}{prefix}{k}\n"));
    }
    if items.len() > CAP {
        out.push_str(&format!("{indent}... and {} more\n", items.len() - CAP));
    }
}

fn index<'a>(section: &str, doc: &'a Value) -> BTreeMap<String, &'a Value> {
    doc[section]
        .as_array()
        .map(|a| a.iter().map(|v| (key(section, v), v)).collect())
        .unwrap_or_default()
}

fn section_diff(section: &str, base: &Value, head: &Value) -> String {
    let (b, h) = (index(section, base), index(section, head));
    let added: BTreeSet<String> = h.keys().filter(|k| !b.contains_key(*k)).cloned().collect();
    let removed: BTreeSet<String> = b.keys().filter(|k| !h.contains_key(*k)).cloned().collect();
    // "field: old -> new" -> keys
    let mut moves: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut changed = 0;
    for (k, bv) in &b {
        let Some(hv) = h.get(k) else { continue };
        if bv == hv {
            continue;
        }
        changed += 1;
        for f in watched(section) {
            if bv.get(*f) != hv.get(*f) {
                let (old, new) = (short(f, bv.get(*f)), short(f, hv.get(*f)));
                // Another target, or other candidates of the same count, read alike in short.
                let label = if old == new {
                    format!("{f}: changed")
                } else {
                    format!("{f}: {old} -> {new}")
                };
                moves.entry(label).or_default().insert(k.clone());
            }
        }
    }
    if added.is_empty() && removed.is_empty() && changed == 0 {
        return String::new();
    }
    let mut out = format!(
        "  {section}: +{} -{} ~{changed}\n",
        added.len(),
        removed.len()
    );
    list("+ ", "    ", &added, &mut out);
    list("- ", "    ", &removed, &mut out);
    for (label, keys) in &moves {
        out.push_str(&format!("    {label}: {}\n", keys.len()));
        list("", "      ", keys, &mut out);
    }
    out
}

fn other_diff(section: &str, base: &Value, head: &Value) -> String {
    let set = |d: &Value| -> BTreeSet<String> {
        d[section]
            .as_array()
            .map(|a| a.iter().map(Value::to_string).collect())
            .unwrap_or_default()
    };
    let (b, h) = (set(base), set(head));
    let (added, removed) = (h.difference(&b).count(), b.difference(&h).count());
    if added + removed == 0 {
        String::new()
    } else {
        format!("  {section}: +{added} -{removed}\n")
    }
}

fn diff_docs(base: &Value, head: &Value) -> String {
    let mut out = String::new();
    if base["repository"] != head["repository"] {
        out.push_str("  repository changed\n");
    }
    for s in KEYED {
        out.push_str(&section_diff(s, base, head));
    }
    for s in OTHER {
        out.push_str(&other_diff(s, base, head));
    }
    out
}

/// `None` as `rev` reads the working tree.
fn load(rev: Option<&str>, path: &str) -> Option<Value> {
    let text = match rev {
        Some(rev) => git(&["show", &format!("{rev}:{path}")])?,
        None => std::fs::read_to_string(path).ok()?,
    };
    serde_json::from_str(&text).ok()
}

fn main() {
    let mut range = "origin/main...HEAD".to_string();
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--range" {
            range = args.next().expect("--range needs a value");
        } else {
            files.push(a);
        }
    }
    let (base, head) = match range.split_once("...") {
        Some((a, b)) => (
            git(&["merge-base", a, b])
                .expect("no merge-base")
                .trim()
                .to_string(),
            Some(b.to_string()),
        ),
        None => (range.clone(), None),
    };
    if files.is_empty() {
        let mut diff = vec!["diff", "--name-only", &base];
        diff.extend(head.as_deref());
        diff.extend(["--", "tests/golden/*.export.json"]);
        files = git(&diff)
            .expect("git diff failed")
            .lines()
            .map(String::from)
            .collect();
        if files.is_empty() {
            println!("no export golden changed in {range}");
        }
    }
    for f in &files {
        println!("== {f} ({range})");
        match (load(Some(&base), f), load(head.as_deref(), f)) {
            (Some(b), Some(h)) => {
                let d = diff_docs(&b, &h);
                print!("{}", if d.is_empty() { "  unchanged\n" } else { &d });
            }
            (None, Some(_)) => println!("  added"),
            (Some(_), None) => println!("  removed"),
            (None, None) => println!("  missing on both sides"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn groups_call_level_moves_and_ignores_unchanged_sections() {
        let call = |id: &str, level: Value| json!({"id": id, "target": null, "level": level});
        let base = json!({"calls": [call("a", json!("ambiguous")), call("b", json!("exact")), call("c", json!("exact"))]});
        let head = json!({"calls": [call("a", Value::Null), call("b", json!("exact")), call("d", json!("exact"))]});
        let out = diff_docs(&base, &head);
        assert!(out.contains("calls: +1 -1 ~1"), "{out}");
        assert!(out.contains("level: ambiguous -> null: 1"), "{out}");
        assert!(!out.contains("symbols"), "{out}");
        assert_eq!(diff_docs(&base, &base), "");
        let moved = json!({"calls": [{"id": "b", "target": "x::f", "level": "exact"}]});
        let retargeted = json!({"calls": [{"id": "b", "target": "x::g", "level": "exact"}]});
        let out = diff_docs(&moved, &retargeted);
        assert!(out.contains("target: changed: 1"), "{out}");
    }
}
