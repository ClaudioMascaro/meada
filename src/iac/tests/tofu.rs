//! A variable with a block in a file only OpenTofu reads (`.tofu`, `.tofu.json`), its declaring
//! block or an override file's, and retyped by an override file (#740). Terraform reads none of
//! such a file, so "follow Terraform" has nothing to follow for it, and OpenTofu 1.13 and later
//! convert the default once, to the final type, where 1.12 and earlier convert it as Terraform
//! does: where the two give different elements or another kind of collection the name is
//! unresolved, never one tool's. A variable all of whose blocks Terraform reads keeps following
//! Terraform. Each expected value for a `.tf` row is Terraform v1.17.1's, for a `.tofu` row
//! OpenTofu v1.13.9's (`0-b`, `1-a`, `2-a` for the retyping rows).

use super::*;

/// A row: its label, its files and the expected answer.
type Row<'a> = (&'a str, Vec<(&'a str, &'a str)>, &'a str);

const UNRESOLVED: &str = "unresolved";

/// The instances of `a.f` (a `for_each` over `tolist(var.s)` keyed `<index>-<element>`) in
/// `files`, as exact names joined by `, `, or `unresolved` when Meada gives no exact name.
fn answer(files: &[(&str, &str)]) -> String {
    let found: Vec<String> = (names(files).into_iter())
        .filter(|n| n.starts_with("a.f") || n.starts_with("module.m.a.f"))
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

const MODULE: &str = "module \"m\" {\n  source = \"./m\"\n}\n";
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
        (
            "a child module",
            vec![
                ("main.tf", MODULE),
                ("m/main.tf", &main),
                ("m/override.tf", LIST),
            ],
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
        // The `.tofu` file is read in place of the `.tf` of the same name: its set, not `[1]`.
        (
            "a .tofu file in place of the .tf of the same name",
            vec![
                ("main.tf", "variable \"s\" {\n  default = [1]\n}\n"),
                ("main.tofu", &main),
            ],
            TERRAFORM,
        ),
    ];
    for (row, files, want) in rows {
        assert_eq!(answer(&files), want, "{row}");
    }
}

/// Two declarations of one variable, in either order of their files.
#[test]
fn a_variable_declared_twice_keeps_every_files_kind_whatever_the_order() {
    let tf = "variable \"s\" {\n  type = set(string)\n}\n";
    let tofu = "variable \"s\" {\n  default = [\"x\"]\n}\n";
    for files in [
        [("a.tf", tf), ("b.tofu", tofu)],
        [("a.tofu", tofu), ("b.tf", tf)],
    ] {
        let bytes: Vec<(String, Vec<u8>)> = (files.iter())
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect();
        let repository = Repository::read(&bytes);
        assert!(repository.modules[""].variables["s"].opentofu, "{files:?}");
    }
}

/// A set that a `.tofu` override retypes is compared even when its declaration is a `.tf` file:
/// Terraform does not read the override, so its answer is the set, and OpenTofu's is the list.
#[test]
fn a_declaration_terraform_reads_retyped_by_a_file_only_opentofu_reads_is_compared() {
    let main = format!("{DECLARED}{READ}");
    let set = "variable \"s\" {\n  type = set(string)\n}\n";
    let own = "variable \"s\" {\n  type = list(string)\n  default = [\"b\", \"a\", \"a\"]\n}\n";
    let rows: Vec<Row> = vec![
        (
            "override.tofu",
            vec![("main.tf", &main), ("override.tofu", LIST)],
            UNRESOLVED,
        ),
        (
            "override.tofu.json",
            vec![("main.tf", &main), ("override.tofu.json", LIST_JSON)],
            UNRESOLVED,
        ),
        (
            "x_override.tofu",
            vec![("main.tf", &main), ("x_override.tofu", LIST)],
            UNRESOLVED,
        ),
        (
            "a .tf.json declaration, override.tofu",
            vec![
                ("main.tf.json", DECLARED_JSON),
                ("read.tf", READ),
                ("override.tofu", LIST),
            ],
            UNRESOLVED,
        ),
        (
            "a .tf.json declaration, override.tofu.json",
            vec![
                ("main.tf.json", DECLARED_JSON),
                ("read.tf", READ),
                ("override.tofu.json", LIST_JSON),
            ],
            UNRESOLVED,
        ),
        (
            "a child module",
            vec![
                ("main.tf", MODULE),
                ("m/main.tf", &main),
                ("m/override.tofu", LIST),
            ],
            UNRESOLVED,
        ),
        // Terraform keeps the declared default, OpenTofu takes the override's.
        (
            "an override.tofu giving its own default",
            vec![("main.tf", &main), ("override.tofu", own)],
            UNRESOLVED,
        ),
        // The two rules give one set.
        (
            "an override.tofu restating the set",
            vec![("main.tf", &main), ("override.tofu", set)],
            TERRAFORM,
        ),
    ];
    for (row, files, want) in rows {
        assert_eq!(answer(&files), want, "{row}");
    }
}

/// The conversions are compared by the elements each tool holds, not by how the default is
/// written: a number written where a string is declared names the same instance.
#[test]
fn a_declaration_only_opentofu_reads_is_exact_where_the_elements_are_the_same_once_converted() {
    let read = |declaration: &str| format!("{declaration}{READ}");
    let numbers = read("variable \"s\" {\n  type = set(string)\n  default = [10, 9]\n}\n");
    let strings = read("variable \"s\" {\n  type = set(string)\n  default = [\"10\", \"9\"]\n}\n");
    let number = "variable \"s\" {\n  type = list(number)\n}\n";
    let unsorted = read("variable \"s\" {\n  type = set(string)\n  default = [9, 10]\n}\n");
    let rows: Vec<Row> = vec![
        (
            "numbers into set(string), then list(string)",
            vec![("main.tofu", &numbers), ("override.tofu", LIST)],
            "f-0-10, f-1-9",
        ),
        (
            "numeric strings into set(string), then list(number)",
            vec![("main.tofu", &strings), ("override.tofu", number)],
            "f-0-10, f-1-9",
        ),
        (
            "the same in a .tofu.json declaration",
            vec![
                (
                    "main.tofu.json",
                    r#"{"variable": {"s": {"type": "set(string)", "default": [10, 9]}}}"#,
                ),
                ("read.tf", READ),
                ("override.tofu.json", LIST_JSON),
            ],
            "f-0-10, f-1-9",
        ),
        // Sorted as strings by the set, written in the other order: the tools differ.
        (
            "unsorted numbers",
            vec![("main.tofu", &unsorted), ("override.tofu", LIST)],
            UNRESOLVED,
        ),
    ];
    for (row, files, want) in rows {
        assert_eq!(answer(&files), want, "{row}");
    }
}
