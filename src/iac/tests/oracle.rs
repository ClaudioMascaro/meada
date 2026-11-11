//! The module cases of `tests/terraform-oracle/modules/`: directories of Terraform files whose
//! `for_each` keys (`want`) the oracle computes with HashiCorp's libraries and Terraform's rules
//! at the boundaries a name crosses (a variable's declared type, a module argument, an override
//! file). An exact name must be the one Terraform gives, and a case marked `=` must be exact.

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
