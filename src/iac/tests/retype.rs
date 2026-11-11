//! An override file's `type` over a variable whose own block declares a set (#740). Terraform
//! converts the default to the declared set when it decodes the block, then again to the
//! override's type; Meada follows it (OpenTofu 1.13 and later convert it once, to the final type,
//! and name other instances: a ceiling). Each row's expected value is what Terraform v1.17.1's
//! `console` printed for `[for k, v in var.s : "${k}-${v}"]`; a set's names are its elements, which
//! Meada leaves unresolved, and a configuration Terraform rejects gets no exact name.

use super::*;

/// A row: its label, the declaring file, the `for_each` collection, the override files and the
/// expected answer.
type Row<'a> = (&'a str, String, &'a str, Vec<(&'a str, &'a str)>, &'a str);

const UNRESOLVED: &str = "unresolved";

/// The instances of a `for_each` over `source`, named by key and value, as `queue-<key>-<value>`
/// joined by `, `, or `unresolved` when Meada gives no exact name; `main` and the files `more`.
fn answer(main: &str, source: &str, more: &[(&str, &str)]) -> String {
    answer_in("main.tf", main, source, more)
}

/// `answer` for a declaring file named `file`.
fn answer_in(file: &str, main: &str, source: &str, more: &[(&str, &str)]) -> String {
    let main = format!(
        "{main}resource \"a\" \"q\" {{\n  for_each = {{for k, v in {source} : k => v}}\n  name = \"queue-${{each.key}}-${{each.value}}\"\n}}\n"
    );
    let mut list = vec![(file, main.as_str())];
    list.extend_from_slice(more);
    let found: Vec<String> = (names(&list).into_iter())
        .filter(|n| n.starts_with("a.q"))
        .collect();
    let exact: Vec<&str> = (found.iter())
        .filter_map(|n| Some(n.split_once(" = ")?.1.split(" [").next().unwrap()))
        .map(|n| n.trim_matches('"'))
        .collect();
    match found.iter().any(|n| n.contains(" ~ ")) || exact.is_empty() {
        true => UNRESOLVED.to_string(),
        false => exact.join(", "),
    }
}

fn declared(default: &str) -> String {
    format!("variable \"s\" {{\n  type = set(string)\n  default = {default}\n}}\n")
}

fn over(body: &str) -> String {
    format!("variable \"s\" {{\n{body}}}\n")
}

#[test]
fn an_override_retyping_a_declared_set_converts_the_default_as_terraform_does() {
    let dup = declared("[\"b\", \"a\", \"a\"]");
    let pair = declared("[\"b\", \"a\"]");
    let list = over("  type = list(string)\n");
    let own = over("  type = list(string)\n  default = [\"b\", \"a\", \"a\"]\n");
    let tuple = over("  type = tuple([string, string])\n");
    let any = over("  type = any\n");
    let set = over("  type = set(string)\n");
    let only = over("  default = [\"b\", \"a\", \"a\"]\n");
    let rows: [Row; 12] = [
        (
            "no override, read by tolist",
            dup.clone(),
            "tolist(var.s)",
            vec![],
            "queue-0-a, queue-1-b",
        ),
        (
            "list",
            dup.clone(),
            "var.s",
            vec![("override.tf", &list)],
            "queue-0-a, queue-1-b",
        ),
        (
            "list over a pair",
            pair.clone(),
            "var.s",
            vec![("override.tf", &list)],
            "queue-0-a, queue-1-b",
        ),
        (
            "any",
            dup.clone(),
            "var.s",
            vec![("override.tf", &any)],
            UNRESOLVED,
        ),
        (
            "set restated",
            dup.clone(),
            "var.s",
            vec![("override.tf", &set)],
            UNRESOLVED,
        ),
        (
            "tuple, rejected",
            dup.clone(),
            "var.s",
            vec![("override.tf", &tuple)],
            UNRESOLVED,
        ),
        (
            "tuple over a pair, rejected",
            pair,
            "var.s",
            vec![("override.tf", &tuple)],
            UNRESOLVED,
        ),
        (
            "list with its own default",
            dup.clone(),
            "var.s",
            vec![("override.tf", &own)],
            "queue-0-b, queue-1-a, queue-2-a",
        ),
        (
            "a default only",
            declared("[\"x\"]"),
            "var.s",
            vec![("override.tf", &only)],
            UNRESOLVED,
        ),
        (
            "a default, then a type",
            declared("[\"x\"]"),
            "var.s",
            vec![("a_override.tf", &only), ("b_override.tf", &list)],
            "queue-0-a, queue-1-b",
        ),
        (
            "declared list, set override",
            "variable \"s\" {\n  type = list(string)\n  default = [\"b\", \"a\", \"a\"]\n}\n"
                .to_string(),
            "var.s",
            vec![("override.tf", &set)],
            UNRESOLVED,
        ),
        ("no override", dup, "var.s", vec![], UNRESOLVED),
    ];
    for (row, main, source, more, expected) in rows {
        assert_eq!(answer(&main, source, &more), expected, "{row}");
    }
}

/// The same conversions in a child module called without the argument, its override file beside it.
#[test]
fn a_child_module_default_is_converted_as_terraform_does() {
    let child = format!(
        "{}resource \"a\" \"q\" {{\n  for_each = {{for k, v in var.s : k => v}}\n  name = \"queue-${{each.key}}-${{each.value}}\"\n}}\n",
        declared("[\"b\", \"a\", \"a\"]")
    );
    let list = over("  type = list(string)\n");
    let names = names(&[
        ("main.tf", "module \"m\" {\n  source = \"./m\"\n}\n"),
        ("m/main.tf", &child),
        ("m/override.tf", &list),
    ]);
    let want = [
        "module.m",
        "module.m.a.q[\"0\"] = \"queue-0-a\"",
        "module.m.a.q[\"1\"] = \"queue-1-b\"",
    ];
    assert_eq!(names, want);
}

/// A var file's value is converted once, to the final type, whatever the default was.
#[test]
fn a_var_file_value_is_converted_once_to_the_final_type() {
    let vars = ("terraform.tfvars", "s = [\"b\", \"a\", \"a\"]\n");
    let list = over("  type = list(string)\n");
    let got = answer(
        &declared("[\"x\"]"),
        "var.s",
        &[("override.tf", &list), vars],
    );
    assert_eq!(got, "queue-0-b, queue-1-a, queue-2-a");
}

/// No declared type: nothing was converted to a set, so an override's list keeps the default.
#[test]
fn an_untyped_default_retyped_as_a_list_keeps_its_order() {
    let main = "variable \"s\" {\n  default = [\"b\", \"a\", \"a\"]\n}\n";
    let list = over("  type = list(string)\n");
    let got = answer(main, "var.s", &[("override.tf", &list)]);
    assert_eq!(got, "queue-0-b, queue-1-a, queue-2-a");
}

/// A value that enters a variable declared `set(T)` is converted to `T`, then sorted and
/// deduplicated as cty does, from whichever source: Terraform v1.17.1 and OpenTofu v1.13.9 agree
/// on every row.
#[test]
fn a_value_entering_a_declared_set_is_converted_to_its_element_type() {
    let by = |main: &str| answer(main, "tolist(var.s)", &[]);
    let set = |ty: &str, default: &str| {
        format!("variable \"s\" {{\n  type = set({ty})\n  default = {default}\n}}\n")
    };
    assert_eq!(by(&set("string", "[10, 9]")), "queue-0-10, queue-1-9");
    assert_eq!(by(&set("number", "[2, \"10\"]")), "queue-0-2, queue-1-10");
    assert_eq!(
        by(&set("number", "[\"10\", \"9\"]")),
        "queue-0-9, queue-1-10"
    );
    let list = "variable \"s\" {\n  type = list(string)\n  default = [\"10\", \"9\"]\n}\n";
    let number = over("  type = set(number)\n");
    assert_eq!(
        answer(list, "tolist(var.s)", &[("override.tf", &number)]),
        "queue-0-9, queue-1-10"
    );
    // An element Meada cannot convert with certainty leaves the value unresolved.
    assert_eq!(by(&set("number", "[\"x\"]")), UNRESOLVED);
    assert_eq!(by(&set("set(string)", "[[\"a\"]]")), UNRESOLVED);
}

/// The default, a var file's value and a module argument of a declared `set(string)` are one set,
/// read by `tolist`, by `length` as a `count`, by `join` and by `for_each`.
#[test]
fn every_source_of_a_declared_set_is_sorted_and_deduplicated() {
    let value = "[\"b\", \"a\", \"a\"]";
    let readers: [(&str, &str, &[&str]); 4] = [
        (
            "tolist",
            "resource \"a\" \"q\" {\n  for_each = {for k, v in tolist(var.s) : k => v}\n  name = \"queue-${each.key}-${each.value}\"\n}\n",
            &["q[\"0\"] = \"queue-0-a\"", "q[\"1\"] = \"queue-1-b\""],
        ),
        (
            "length",
            "resource \"a\" \"q\" {\n  count = length(var.s)\n  name = \"queue-${count.index}\"\n}\n",
            &["q[0] = \"queue-0\"", "q[1] = \"queue-1\""],
        ),
        (
            "join",
            "resource \"a\" \"q\" {\n  name = \"queue-${join(\"-\", var.s)}\"\n}\n",
            &["q = \"queue-a-b\""],
        ),
        (
            "for_each",
            "resource \"a\" \"q\" {\n  for_each = var.s\n  name = \"queue-${each.key}\"\n}\n",
            &["q[\"a\"] = \"queue-a\"", "q[\"b\"] = \"queue-b\""],
        ),
    ];
    let variable = "variable \"s\" {\n  type = set(string)\n}\n";
    let with_default =
        format!("variable \"s\" {{\n  type = set(string)\n  default = {value}\n}}\n");
    let sources = [
        ("default", "", ""),
        ("terraform.tfvars", variable, "terraform.tfvars"),
        ("x.auto.tfvars", variable, "x.auto.tfvars"),
        ("module argument", variable, "module"),
    ];
    for (source, declaration, file) in sources {
        for (reader, resource, want) in readers {
            let (prefix, main, more): (&str, String, Vec<(String, String)>) = match source {
                "default" => ("", format!("{with_default}{resource}"), vec![]),
                "module argument" => (
                    "module.m.",
                    format!("module \"m\" {{\n  source = \"./m\"\n  s = {value}\n}}\n"),
                    vec![("m/main.tf".into(), format!("{declaration}{resource}"))],
                ),
                _ => (
                    "",
                    format!("{declaration}{resource}"),
                    vec![(file.into(), format!("s = {value}\n"))],
                ),
            };
            let mut list = vec![("main.tf", main.as_str())];
            list.extend(more.iter().map(|(p, t)| (p.as_str(), t.as_str())));
            let got: Vec<String> = (names(&list).into_iter())
                .filter(|n| n.starts_with(&format!("{prefix}a.")))
                .map(|n| n.split(" [").next().unwrap().to_string())
                .collect();
            let want: Vec<String> = want.iter().map(|w| format!("{prefix}a.{w}")).collect();
            assert_eq!(got, want, "{source} read by {reader}");
        }
    }
}

/// An argument reaches a declared set through a `list(string)` variable of the module between: the
/// list keeps its order and the set sorts it.
#[test]
fn an_argument_passed_through_a_list_variable_is_converted_by_the_declared_set() {
    let outer = "module \"m\" {\n  source = \"./m\"\n  l = [\"b\", \"a\", \"a\"]\n}\n";
    let mid = "variable \"l\" {\n  type = list(string)\n}\nmodule \"n\" {\n  source = \"./n\"\n  s = var.l\n}\n";
    let inner = format!(
        "{}resource \"a\" \"q\" {{\n  for_each = {{for k, v in tolist(var.s) : k => v}}\n  name = \"queue-${{each.key}}-${{each.value}}\"\n}}\n",
        "variable \"s\" {\n  type = set(string)\n}\n"
    );
    let got = names(&[
        ("main.tf", outer),
        ("m/main.tf", mid),
        ("m/n/main.tf", &inner),
    ]);
    let got: Vec<&str> = got.iter().map(|n| n.split(" [").next().unwrap()).collect();
    assert_eq!(
        got[got.len() - 2..],
        [
            "module.m.module.n.a.q[\"0\"] = \"queue-0-a\"",
            "module.m.module.n.a.q[\"1\"] = \"queue-1-b\""
        ]
    );
}

/// A root variable declared as a set and retyped to `any` takes a var file's value as written:
/// the value is converted once, to `any`, so it is a tuple and `for` keys it by index. The
/// default is still a set, converted by the declaring block first.
#[test]
fn a_var_file_value_of_a_set_retyped_to_any_is_converted_once() {
    let any = over("  type = any\n");
    let list = over("  type = list(string)\n");
    let json = ("terraform.tfvars.json", "{\"s\": [\"b\", \"a\", \"a\"]}\n");
    let files = [
        ("terraform.tfvars", "s = [\"b\", \"a\", \"a\"]\n"),
        ("x.auto.tfvars", "s = [\"b\", \"a\", \"a\"]\n"),
        json,
        ("x.auto.tfvars.json", json.1),
    ];
    for file in ["main.tf", "main.tofu"] {
        for (vars, content) in files {
            for (label, override_file) in [("any", &any), ("list", &list)] {
                let got = answer_in(
                    file,
                    &declared("[\"x\"]"),
                    "var.s",
                    &[("override.tf", override_file), (vars, content)],
                );
                assert_eq!(
                    got, "queue-0-b, queue-1-a, queue-2-a",
                    "{file} {vars} retyped to {label}"
                );
            }
        }
        // The same variable on its default: a set, which `for` keys by element.
        let on_default = answer_in(
            file,
            &declared("[\"x\"]"),
            "var.s",
            &[("override.tf", &any)],
        );
        assert_eq!(on_default, UNRESOLVED, "{file} default");
    }
}
