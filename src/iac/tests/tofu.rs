//! A variable declared in a file only OpenTofu reads (`.tofu`, `.tofu.json`) and retyped by an
//! override file (#740). Terraform reads none of it, so "follow Terraform" has nothing to follow,
//! and OpenTofu 1.13 and later convert the default once, to the final type, where 1.12 and
//! earlier convert it as Terraform does: where the two give different values the name is
//! unresolved, never one tool's. A declaration Terraform reads keeps following Terraform. Each
//! expected value for a `.tf` row is Terraform v1.17.1's, for a `.tofu` row OpenTofu v1.13.9's
//! (`0-b`, `1-a`, `2-a` for the retyping rows).

use super::*;

/// A row: its label, its files and the expected answer.
type Row<'a> = (&'a str, Vec<(&'a str, &'a str)>, &'a str);

const UNRESOLVED: &str = "unresolved";

/// The instances of `a.f` (a `for_each` over `tolist(var.s)` keyed `<index>-<element>`) in
/// `files`, as exact names joined by `, `, or `unresolved` when Meada gives no exact name.
fn answer(files: &[(&str, &str)]) -> String {
    let found: Vec<String> = (names(files).into_iter())
        .filter(|n| n.starts_with("a.f"))
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

const READ: &str = "resource \"a\" \"f\" {\n  for_each = toset([for k, v in tolist(var.s) : \"${k}-${v}\"])\n  name = \"f-${each.value}\"\n}\n";
const DECLARED: &str =
    "variable \"s\" {\n  type = set(string)\n  default = [\"b\", \"a\", \"a\"]\n}\n";
const LIST: &str = "variable \"s\" {\n  type = list(string)\n}\n";
const LIST_JSON: &str = r#"{"variable": {"s": {"type": "list(string)"}}}"#;
const DECLARED_JSON: &str =
    r#"{"variable": {"s": {"type": "set(string)", "default": ["b", "a", "a"]}}}"#;

/// Terraform: the default is a set first (`0-a`, `1-b`), then a list in that order.
const TERRAFORM: &str = "f-0-a, f-1-b";

#[test]
fn a_declaration_terraform_reads_follows_terraform_under_every_override_it_reads() {
    let main = format!("{DECLARED}{READ}");
    let json = [("main.tf.json", DECLARED_JSON), ("read.tf", READ)];
    let rows: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "override.tf",
            vec![("main.tf", &main), ("override.tf", LIST)],
        ),
        (
            "a_override.tf",
            vec![("main.tf", &main), ("a_override.tf", LIST)],
        ),
        (
            "override.tf.json",
            vec![("main.tf", &main), ("override.tf.json", LIST_JSON)],
        ),
        (
            "tf.json declaration",
            vec![json[0], json[1], ("override.tf", LIST)],
        ),
        // Terraform ignores a `.tofu` override, and the default alone gives the same names.
        (
            "override.tofu",
            vec![("main.tf", &main), ("override.tofu", LIST)],
        ),
        (
            "override.tofu.json",
            vec![("main.tf", &main), ("override.tofu.json", LIST_JSON)],
        ),
    ];
    for (row, files) in rows {
        assert_eq!(answer(&files), TERRAFORM, "{row}");
    }
}

#[test]
fn a_declaration_only_opentofu_reads_is_unresolved_where_the_two_conversions_differ() {
    let main = format!("{DECLARED}{READ}");
    let rows: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "override.tofu",
            vec![("main.tofu", &main), ("override.tofu", LIST)],
        ),
        (
            "x_override.tf",
            vec![("main.tofu", &main), ("x_override.tf", LIST)],
        ),
        (
            "override.tofu.json",
            vec![("main.tofu", &main), ("override.tofu.json", LIST_JSON)],
        ),
        (
            "override.tf.json",
            vec![("main.tofu", &main), ("override.tf.json", LIST_JSON)],
        ),
        (
            "x_override.tofu",
            vec![("main.tofu", &main), ("x_override.tofu", LIST)],
        ),
        (
            "tofu.json declaration, override.tofu",
            vec![
                ("main.tofu.json", DECLARED_JSON),
                ("read.tofu", READ),
                ("override.tofu", LIST),
            ],
        ),
        (
            "tofu.json declaration, override.tf",
            vec![
                ("main.tofu.json", DECLARED_JSON),
                ("read.tf", READ),
                ("override.tf", LIST),
            ],
        ),
        (
            "a .tofu file in place of the .tf of the same name",
            vec![
                ("main.tf", "variable \"s\" {\n  default = [1]\n}\n"),
                ("main.tofu", &main),
                ("override.tf", LIST),
            ],
        ),
        (
            "override.tf.json over a tofu.json declaration",
            vec![
                ("main.tofu.json", DECLARED_JSON),
                ("read.tf", READ),
                ("override.tf.json", LIST_JSON),
            ],
        ),
        (
            "set to any",
            vec![
                ("main.tofu", &main),
                ("override.tf", "variable \"s\" {\n  type = any\n}\n"),
            ],
        ),
    ];
    for (row, files) in rows {
        assert_eq!(answer(&files), UNRESOLVED, "{row}");
    }
}

/// Where the two rules give one value, a `.tofu` declaration stays exact.
#[test]
fn a_declaration_only_opentofu_reads_is_exact_where_the_two_conversions_agree() {
    let main = format!("{DECLARED}{READ}");
    let set = "variable \"s\" {\n  type = set(string)\n}\n";
    let own = "variable \"s\" {\n  type = list(string)\n  default = [\"b\", \"a\", \"a\"]\n}\n";
    let vars = "s = [\"b\", \"a\", \"a\"]\n";
    let rows: Vec<Row> = vec![
        ("no override", vec![("main.tofu", &main)], TERRAFORM),
        (
            "an override restating the set",
            vec![("main.tofu", &main), ("override.tofu", set)],
            TERRAFORM,
        ),
        (
            "an override with its own default of the final type",
            vec![("main.tofu", &main), ("override.tofu", own)],
            "f-0-b, f-1-a, f-2-a",
        ),
        (
            "a var file's value, converted once",
            vec![
                ("main.tofu", &main),
                ("override.tf", LIST),
                ("terraform.tfvars", vars),
            ],
            "f-0-b, f-1-a, f-2-a",
        ),
        (
            "a .tofu.json declaration, no override",
            vec![("main.tofu.json", DECLARED_JSON), ("read.tf", READ)],
            TERRAFORM,
        ),
    ];
    for (row, files, want) in rows {
        assert_eq!(answer(&files), want, "{row}");
    }
}
