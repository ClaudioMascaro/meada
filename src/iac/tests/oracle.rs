//! The module cases of `tests/terraform-oracle/modules/`: directories of Terraform files whose
//! `for_each` keys (`want`) the oracle computes with HashiCorp's libraries and Terraform's rules
//! at the boundaries a name crosses (a variable's declared type, a module argument, an override
//! file). An exact name must be the one Terraform gives, and a case marked `=` must be exact.
//!
//! The recorded cases of `tests/terraform-oracle/recorded.jsonl` hold the tools' own answers: the
//! names `console` gives each resource from its `count`, `for_each` and `name`, in Terraform
//! 1.17.1 for a directory of `.tf` files and in OpenTofu 1.13.9 for one with a `.tofu` file, run
//! on 2026-11-10 by the black-box verifiers of #754. A case or a resource the tool refuses is left
//! out, also where a plan would then refuse the whole directory. Each line is a case: `files`, and
//! per reader (a resource of `aws_sqs_queue`, or `""` for all of them) those names in `want` and a
//! `mark`. An OpenTofu case whose variable is declared in a `.tf` file and changed by a `.tofu`
//! block is `~` where Terraform, reading the directory without that block, gives the variable
//! another value or type (#767).

use super::*;
use std::path::Path;

fn read(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = format!("{prefix}{}", path.file_name().unwrap().to_str().unwrap());
        if path.is_dir() {
            read(&path, &format!("{name}/"), out);
        } else if name != "want" {
            out.push((name, std::fs::read_to_string(&path).unwrap()));
        }
    }
}

#[test]
fn module_cases_agree_with_the_terraform_oracle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/terraform-oracle/modules");
    let mut cases: Vec<_> = (std::fs::read_dir(&root).unwrap())
        .map(|e| e.unwrap().path())
        .collect();
    cases.sort();
    assert!(!cases.is_empty());
    let mut wrong = Vec::new();
    for case in cases {
        let want = std::fs::read_to_string(case.join("want")).unwrap();
        let (must, keys) = want.trim_end_matches('\n').split_once('\t').unwrap();
        let keys = if keys == "-" { "" } else { keys };
        let mut list = Vec::new();
        read(&case, "", &mut list);
        let list: Vec<(&str, &str)> = list.iter().map(|(p, t)| (&p[..], &t[..])).collect();
        let lines: Vec<String> = (names(&list).into_iter())
            .filter(|l| {
                let at = l.split(' ').next().unwrap();
                at == "a.q" || at.starts_with("a.q[") || at.starts_with("module.m.a.q")
            })
            .collect();
        let right = judge(must, keys, &lines);
        if !right {
            wrong.push(format!(
                "{}: Terraform {keys:?}, Meada {lines:?}",
                case.display()
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// Whether Meada's lines for `a.q` agree with Terraform's `keys` (`!` and a reason: it refuses; no
/// key: no instance). Every exact line must be a name Terraform gives; a line that is a pattern
/// leaves the others partial, and `must` `=` allows none.
fn judge(must: &str, keys: &str, lines: &[String]) -> bool {
    assert!(matches!(must, "=" | "~"), "not a marker: {must:?}");
    let mut expected: Vec<String> = (keys
        .split(',')
        .filter(|k| !k.is_empty() && !keys.starts_with('!')))
    .map(|k| format!("{:?}", format!("queue-{k}")))
    .collect();
    expected.sort();
    let mut exact: Vec<&str> = (lines.iter())
        .filter_map(|l| Some(l.split_once(" = ")?.1.split(" [").next().unwrap()))
        .collect();
    exact.sort();
    let pattern = lines.iter().any(|l| l.contains(" ~ "));
    let given = exact.iter().all(|name| expected.iter().any(|e| e == name));
    given
        && match pattern {
            true => must != "=",
            false => exact == expected,
        }
}

#[test]
fn a_case_is_judged_on_every_exact_line_it_gives() {
    let lines = |l: &[&str]| l.iter().map(|l| l.to_string()).collect::<Vec<_>>();
    // An exact line Terraform does not give fails beside a pattern line, and in a refused case.
    let wrong = lines(&[
        "a.q[\"0\"] = \"queue-0\" [var.s main.tf:1]",
        "a.q ~ \"queue-*\"",
    ]);
    assert!(!judge("~", "x", &wrong));
    assert!(!judge(
        "~",
        "!for_each is null",
        &lines(&["a.q[\"0\"] = \"queue-0\""])
    ));
    // The right exact line passes beside a pattern line unless the case must be exact.
    let right = lines(&["a.q[\"x\"] = \"queue-x\"", "a.q ~ \"queue-*\""]);
    assert!(judge("~", "x", &right) && !judge("=", "x", &right));
    // No instance is a case: no line, or a pattern line, and an exact line fails it.
    assert!(judge("=", "", &[]) && judge("~", "", &lines(&["a.q ~ \"queue-*\""])));
    assert!(!judge("=", "", &lines(&["a.q[\"0\"] = \"queue-0\""])));
    // An instance missing from an all-exact answer fails.
    assert!(!judge("=", "x,y", &lines(&["a.q[\"x\"] = \"queue-x\""])));
    assert!(!judge("=", "x", &[]));
}

#[test]
#[should_panic(expected = "not a marker")]
fn an_unknown_marker_fails() {
    judge("?", "x", &[]);
}

#[test]
fn recorded_cases_agree_with_the_tools() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/terraform-oracle/recorded.jsonl");
    let text = std::fs::read_to_string(path).unwrap();
    let mut wrong = Vec::new();
    for line in text.lines() {
        let case: serde_json::Value = serde_json::from_str(line).unwrap();
        let list: Vec<(&str, &str)> = (case["files"].as_object().unwrap().iter())
            .map(|(p, t)| (&p[..], t.as_str().unwrap()))
            .collect();
        let (nodes, _) = evaluate(&files(&list));
        for (reader, want) in case["want"].as_object().unwrap() {
            let want: Vec<&str> = (want.as_array().unwrap().iter())
                .map(|n| n.as_str().unwrap())
                .collect();
            let tag = format!("aws_sqs_queue.{reader}");
            let (exact, partial): (Vec<&Node>, Vec<&Node>) = (nodes.iter())
                .filter(|n| n.ty == "aws_sqs_queue")
                .filter(|n| {
                    let rest = n.address.split_once(&tag).map(|(_, rest)| rest);
                    reader.is_empty() || rest.is_some_and(|r| r.is_empty() || r.starts_with('['))
                })
                .partition(|n| n.name.is_some() && !n.pattern);
            let exact: Vec<&str> = exact.iter().filter_map(|n| n.name.as_deref()).collect();
            let mark = case["mark"][reader].as_str().unwrap();
            if !agrees(mark, &want, &exact, !partial.is_empty()) {
                wrong.push(format!(
                    "{} {reader:?} {mark}: {} {want:?}, Meada {exact:?} and {} more",
                    case["case"],
                    case["tool"],
                    partial.len()
                ));
            }
        }
    }
    assert!(
        text.lines().count() > 1900 && wrong.is_empty(),
        "{wrong:#?}"
    );
}

/// Whether Meada's `exact` names, and `partial` for a node it names by a pattern or not at all,
/// agree with the names the tool creates. Under `~` each exact name is one of the tool's, no more
/// times than the tool creates it (an instance Meada stops giving at all still passes: only `=`
/// holds every instance); under `=` Meada also names every instance exactly, as it did when the
/// case was recorded; `!` is a difference recorded with its issue or ceiling in `why`, and it must
/// still differ, so that the fix moves the case to `=`.
fn agrees(mark: &str, want: &[&str], exact: &[&str], partial: bool) -> bool {
    let mut left = want.to_vec();
    let within = (exact.iter()).all(|name| {
        (left.iter().position(|w| w == name))
            .map(|at| left.swap_remove(at))
            .is_some()
    });
    match mark {
        "=" => within && left.is_empty() && !partial,
        "~" => within,
        "!" => !within,
        _ => panic!("not a marker: {mark:?}"),
    }
}

#[test]
fn a_recorded_case_is_judged_on_every_exact_name() {
    // An exact name the tool does not create fails, also beside a pattern and once too many.
    assert!(!agrees("~", &["0-a", "1-b"], &["0-b"], true));
    assert!(!agrees("~", &["0-a"], &["0-a", "0-a"], false));
    assert!(agrees("~", &["0-a", "1-b"], &["0-a"], true) && agrees("~", &["0-a"], &[], true));
    // A case recorded exact fails once an instance is a pattern or is missing.
    assert!(agrees("=", &["0-a", "0-a"], &["0-a", "0-a"], false));
    assert!(!agrees("=", &["0-a", "1-b"], &["0-a"], true));
    assert!(!agrees("=", &["0-a", "1-b"], &["0-a"], false));
    // A recorded difference fails once Meada agrees.
    assert!(agrees("!", &["0-10"], &["0-010"], false) && !agrees("!", &["0-10"], &["0-10"], false));
}
