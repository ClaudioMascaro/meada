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
        let mut list = Vec::new();
        read(&case, "", &mut list);
        let list: Vec<(&str, &str)> = list.iter().map(|(p, t)| (&p[..], &t[..])).collect();
        let lines: Vec<String> = (names(&list).into_iter())
            .filter(|l| {
                let at = l.split(' ').next().unwrap();
                at == "a.q" || at.starts_with("a.q[") || at.starts_with("module.m.a.q")
            })
            .collect();
        let unresolved = lines.iter().any(|l| l.contains(" ~ "));
        let mut exact: Vec<&str> = (lines.iter())
            .filter_map(|l| Some(l.split_once(" = ")?.1.split(" [").next().unwrap()))
            .collect();
        exact.sort();
        let mut expected: Vec<String> = (keys.split(',').filter(|k| !k.is_empty()))
            .map(|k| format!("{:?}", format!("queue-{k}")))
            .collect();
        expected.sort();
        let right = match (lines.is_empty(), unresolved) {
            (true, _) => false,
            (_, true) => must != "=",
            _ => !keys.starts_with('!') && exact == expected,
        };
        if !right {
            wrong.push(format!(
                "{}: Terraform {keys:?}, Meada {lines:?}",
                case.display()
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
