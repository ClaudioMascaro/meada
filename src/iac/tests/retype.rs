//! An override file's `type` over a variable whose own block declares a set (#740). Terraform
//! converts the default to the declared set when it decodes the block, then again to the
//! override's type; Meada follows it (OpenTofu 1.13 and later convert it once, to the final type,
//! and name other instances: a ceiling). Each row's expected value is what Terraform v1.17.1's
//! `console` printed for `[for k, v in var.s : "${k}-${v}"]`; a set's names are its elements, which
//! Meada leaves unresolved, and a configuration Terraform rejects gets no exact name.

use super::*;

const UNRESOLVED: &str = "unresolved";

/// The instances of a `for_each` over `source`, named by key and value, as `queue-<key>-<value>`
/// joined by `, `, or `unresolved` when Meada gives no exact name; `main` and the files `more`.
fn answer(main: &str, source: &str, more: &[(&str, &str)]) -> String {
    let main = format!(
        "{main}resource \"a\" \"q\" {{\n  for_each = {{for k, v in {source} : k => v}}\n  name = \"queue-${{each.key}}-${{each.value}}\"\n}}\n"
    );
    let mut list = vec![("main.tf", main.as_str())];
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
    let rows: [(&str, String, &str, Vec<(&str, &str)>, &str); 12] = [
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
