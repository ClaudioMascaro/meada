//! A variable with a block in a file only OpenTofu reads (`.tofu`, `.tofu.json`), its declaring
//! block or an override file's, and retyped by an override file (#740). Terraform reads none of
//! such a file, so "follow Terraform" has nothing to follow for it, and OpenTofu 1.13 and later
//! convert the default once, to the final type, where 1.12 and earlier convert it as Terraform
//! does: where the two give different elements or another kind of collection the name is
//! unresolved, never one tool's. A variable all of whose blocks Terraform reads keeps following
//! Terraform. One declared in a file Terraform reads also has Terraform's reading, the directory
//! without the `.tofu` files: it is unresolved where that reading gives another value or type,
//! and is the variable's where OpenTofu refuses the directory (#767). Each expected value for a
//! `.tf` row is Terraform v1.17.1's, for a `.tofu` row OpenTofu v1.13.9's (`0-b`, `1-a`, `2-a`
//! for the retyping rows), and where they differ the row says so.

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

/// A declaration Terraform reads, changed by a block in a file only OpenTofu reads (#767):
/// Terraform reads the directory without that file and OpenTofu with it, so where the two give the
/// variable another value or another type no name is exact. Terraform v1.17.1 and OpenTofu v1.13.9
/// name other instances on every row (the issue's three: `default = ["c"]`, `0-a` and `1-b`
/// against `0-c`; `set(number)` over `["10", "9"]`, `0-10` and `1-9` against `0-9` and `1-10`; a
/// list retyped a set, `0-b`, `1-a` and `2-a` against `0-a` and `1-b`) but the last, a list of
/// strings retyped a list of numbers, where both name `0-10`, `1-9` and `2-10`.
#[test]
fn a_declaration_terraform_reads_is_unresolved_where_a_file_only_opentofu_reads_changes_it() {
    let main = format!("{DECLARED}{READ}");
    let read = |declaration: &str| format!("{declaration}{READ}");
    let typed = |ty: &str| format!("variable \"s\" {{\n  type = {ty}\n}}\n");
    let (set, numbers, any) = (typed("set(string)"), typed("list(number)"), typed("any"));
    let set_number = typed("set(number)");
    let own = "variable \"s\" {\n  type = list(string)\n  default = [\"b\", \"a\", \"a\"]\n}\n";
    let c = "variable \"s\" {\n  default = [\"c\"]\n}\n";
    let ten = read("variable \"s\" {\n  type = set(string)\n  default = [\"10\", \"9\"]\n}\n");
    let list = read(own);
    let strings =
        read("variable \"s\" {\n  type = list(string)\n  default = [\"10\", \"9\", \"10\"]\n}\n");
    let x = read("variable \"s\" {\n  type = set(string)\n  default = [\"x\"]\n}\n");
    let child = read(&set);
    let call = "module \"m\" {\n  source = \"./m\"\n  s = [\"b\", \"a\", \"a\"]\n}\n";
    let rows: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "override.tofu",
            vec![("main.tf", &main), ("override.tofu", LIST)],
        ),
        (
            "override.tofu.json",
            vec![("main.tf", &main), ("override.tofu.json", LIST_JSON)],
        ),
        (
            "x_override.tofu",
            vec![("main.tf", &main), ("x_override.tofu", LIST)],
        ),
        (
            "a .tf.json declaration, override.tofu",
            vec![
                ("main.tf.json", DECLARED_JSON),
                ("read.tf", READ),
                ("override.tofu", LIST),
            ],
        ),
        (
            "a .tf.json declaration, override.tofu.json",
            vec![
                ("main.tf.json", DECLARED_JSON),
                ("read.tf", READ),
                ("override.tofu.json", LIST_JSON),
            ],
        ),
        (
            "a child module",
            vec![
                ("main.tf", MODULE),
                ("m/main.tf", &main),
                ("m/override.tofu", LIST),
            ],
        ),
        (
            "an override.tofu giving its own default",
            vec![("main.tf", &main), ("override.tofu", own)],
        ),
        (
            "an override.tofu giving only a default",
            vec![("main.tf", &main), ("override.tofu", c)],
        ),
        (
            "an override.tofu.json giving only a default",
            vec![
                ("main.tf", &main),
                (
                    "override.tofu.json",
                    r#"{"variable": {"s": {"default": ["c"]}}}"#,
                ),
            ],
        ),
        (
            "a child module's default",
            vec![
                ("main.tf", MODULE),
                ("m/main.tf", &main),
                ("m/override.tofu", c),
            ],
        ),
        (
            "a set retyped a set of numbers",
            vec![("main.tf", &ten), ("override.tofu", &set_number)],
        ),
        (
            "a list retyped a set",
            vec![("main.tf", &list), ("override.tofu", &set)],
        ),
        (
            "a .tf override's default, then a .tofu one's",
            vec![
                ("main.tf", &main),
                (
                    "a_override.tf",
                    "variable \"s\" {\n  default = [\"e\", \"d\", \"d\"]\n}\n",
                ),
                ("b_override.tofu", c),
            ],
        ),
        (
            "a .tf override's type, then a .tofu one's",
            vec![
                ("main.tf", &main),
                ("a_override.tf", LIST),
                ("b_override.tofu", &any),
            ],
        ),
        (
            "a var file's value under another type",
            vec![
                ("main.tf", &x),
                ("override.tofu", LIST),
                ("terraform.tfvars", "s = [\"b\", \"a\", \"a\"]\n"),
            ],
        ),
        (
            "a module argument under another type",
            vec![
                ("main.tf", call),
                ("m/main.tf", &child),
                ("m/override.tofu", LIST),
            ],
        ),
        (
            "strings retyped numbers, spelled alike",
            vec![("main.tf", &strings), ("override.tofu", &numbers)],
        ),
    ];
    for (row, files) in rows {
        assert_eq!(answer(&files), UNRESOLVED, "{row}");
    }
}

/// Where Terraform's reading of the directory and OpenTofu's give the variable one value of one
/// type, a declaration Terraform reads stays exact beside a block only OpenTofu reads (#767):
/// Terraform v1.17.1 and OpenTofu v1.13.9 name `0-a` and `1-b` on every row, and `0-b`, `1-a`,
/// `2-a` for a variable with no `type` that the override types `any`, which keeps the value too.
#[test]
fn a_declaration_terraform_reads_is_exact_where_both_tools_read_one_value() {
    let main = format!("{DECLARED}{READ}");
    let x = format!("variable \"s\" {{\n  type = set(string)\n  default = [\"x\"]\n}}\n{READ}");
    let set = "variable \"s\" {\n  type = set(string)\n}\n";
    let restated = "variable \"s\" {\n  default = [\"b\", \"a\", \"a\"]\n}\n";
    let y = "variable \"s\" {\n  default = [\"y\"]\n}\n";
    let call = "module \"m\" {\n  source = \"./m\"\n  s = [\"b\", \"a\", \"a\"]\n}\n";
    let rows: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "an override.tofu restating the set",
            vec![("main.tf", &main), ("override.tofu", set)],
        ),
        (
            "an override.tofu restating the default",
            vec![("main.tf", &main), ("override.tofu", restated)],
        ),
        (
            "a .tofu override's type, then a .tf one's",
            vec![
                ("main.tf", &main),
                ("a_override.tofu", LIST),
                ("b_override.tf", set),
            ],
        ),
        (
            "a child module, the default restated",
            vec![
                ("main.tf", MODULE),
                ("m/main.tf", &main),
                ("m/override.tofu", restated),
            ],
        ),
        (
            "a var file's value under one type",
            vec![
                ("main.tf", &x),
                ("override.tofu", y),
                ("terraform.tfvars", "s = [\"b\", \"a\"]\n"),
            ],
        ),
        (
            "a module argument under one type",
            vec![("main.tf", call), ("m/main.tf", &x), ("m/override.tofu", y)],
        ),
    ];
    for (row, files) in rows {
        assert_eq!(answer(&files), TERRAFORM, "{row}");
    }
    let untyped = format!("variable \"s\" {{\n  default = [\"b\", \"a\", \"a\"]\n}}\n{READ}");
    let any = "variable \"s\" {\n  type = any\n}\n";
    let files = [("main.tf", untyped.as_str()), ("override.tofu", any)];
    assert_eq!(answer(&files), "f-0-b, f-1-a, f-2-a");
}

/// A directory OpenTofu refuses is read as Terraform reads it, the only tool that runs it (#767):
/// a default with a string no number spells, under a list or a set of numbers only a `.tofu`
/// override gives. OpenTofu v1.13.9 refuses each such row ("Invalid default value for variable"),
/// and Terraform v1.17.1 names what each expects. A default Terraform refuses, under a type only
/// OpenTofu reads, names nothing exactly: Terraform refuses, and Meada follows it (#740).
#[test]
fn a_directory_opentofu_refuses_is_read_as_terraform_reads_it() {
    let read = |declaration: &str| format!("{declaration}{READ}");
    let z = read("variable \"s\" {\n  type = list(string)\n  default = [\"z\"]\n}\n");
    let x = read("variable \"s\" {\n  type = set(string)\n  default = [\"x\"]\n}\n");
    let refused = read("variable \"s\" {\n  type = list(number)\n  default = [\"z\"]\n}\n");
    let numbers = "variable \"s\" {\n  type = list(number)\n}\n";
    let set_number = "variable \"s\" {\n  type = set(number)\n}\n";
    let rows: Vec<Row> = vec![
        (
            "a list of numbers, the default",
            vec![("main.tf", &z), ("override.tofu", numbers)],
            "f-0-z",
        ),
        (
            "a list of numbers, a var file's value",
            vec![
                ("main.tf", &z),
                ("override.tofu", numbers),
                ("terraform.tfvars", "s = [\"10\", \"9\", \"10\"]\n"),
            ],
            "f-0-10, f-1-9, f-2-10",
        ),
        (
            "a set of numbers, a var file's value",
            vec![
                ("main.tf", &x),
                ("override.tofu", set_number),
                ("terraform.tfvars", "s = [\"10\", \"9\"]\n"),
            ],
            "f-0-10, f-1-9",
        ),
        (
            "a default Terraform refuses",
            vec![("main.tf", &refused), ("override.tofu", LIST)],
            UNRESOLVED,
        ),
    ];
    for (row, files, want) in rows {
        assert_eq!(answer(&files), want, "{row}");
    }
}

/// A default no set type touches converts to one value by either rule in these rows (a list of a
/// primitive type converts its elements alike once or twice, any other type keeps the value), so a
/// block in a file only OpenTofu reads changes nothing: it names what the same variable names in
/// `.tf` files. Terraform v1.17.1 (the files renamed) and OpenTofu v1.13.9 agree on every row.
#[test]
fn a_default_no_set_type_touches_is_exact_with_a_block_only_opentofu_reads() {
    let variable = |ty: &str, default: &str| {
        let ty = match ty {
            "" => String::new(),
            ty => format!("  type = {ty}\n"),
        };
        format!("variable \"s\" {{\n{ty}  default = {default}\n}}\n")
    };
    let read = |ty: &str, default: &str| format!("{}{READ}", variable(ty, default));
    let numbers = read("list(string)", "[10, 9, 10]");
    let list = read("list(string)", "[\"b\", \"a\", \"a\"]");
    let untyped = read("", "[\"b\", \"a\", \"a\"]");
    let tuple = read("tuple([string, number])", "[\"b\", 1]");
    let objects = format!(
        "{}resource \"a\" \"f\" {{\n  for_each = toset([for k, v in var.s : \"${{k}}-${{v.name}}\"])\n  name = \"f-${{each.value}}\"\n}}\n",
        variable(
            "list(object({ name = string }))",
            "[{ name = \"b\" }, { name = \"a\" }, { name = \"a\" }]"
        )
    );
    let same = variable("", "[10, 9, 10]");
    let rows: Vec<Row> = vec![
        (
            "numbers in a list(string), main.tofu",
            vec![("main.tofu", &numbers)],
            "f-0-10, f-1-9, f-2-10",
        ),
        (
            "numeric strings in a list(number), main.tofu.json",
            vec![
                (
                    "main.tofu.json",
                    r#"{"variable": {"s": {"type": "list(number)", "default": ["10", "9"]}}}"#,
                ),
                ("read.tf", READ),
            ],
            "f-0-10, f-1-9",
        ),
        (
            "a list of objects, main.tofu",
            vec![("main.tofu", &objects)],
            "f-0-b, f-1-a, f-2-a",
        ),
        (
            "an override.tofu giving only a default",
            vec![("main.tofu", &numbers), ("override.tofu", &same)],
            "f-0-10, f-1-9, f-2-10",
        ),
        (
            "an override.tf giving only a default",
            vec![("main.tofu", &numbers), ("override.tf", &same)],
            "f-0-10, f-1-9, f-2-10",
        ),
        (
            "a list in main.tf, the same default in override.tofu",
            vec![
                ("main.tf", &list),
                (
                    "override.tofu",
                    "variable \"s\" {\n  default = [\"b\", \"a\", \"a\"]\n}\n",
                ),
            ],
            "f-0-b, f-1-a, f-2-a",
        ),
        (
            "untyped in main.tf, the same default in override.tofu.json",
            vec![
                ("main.tf", &untyped),
                (
                    "override.tofu.json",
                    r#"{"variable": {"s": {"default": ["b", "a", "a"]}}}"#,
                ),
            ],
            "f-0-b, f-1-a, f-2-a",
        ),
        (
            "a tuple in main.tf, the same default in override.tofu",
            vec![
                ("main.tf", &tuple),
                (
                    "override.tofu",
                    "variable \"s\" {\n  default = [\"b\", 1]\n}\n",
                ),
            ],
            "f-0-b, f-1-1",
        ),
    ];
    for (row, files, want) in rows {
        assert_eq!(answer(&files), want, "{row}");
    }
}

/// A list of a primitive type converts its elements (#770), so an override in a file only OpenTofu
/// reads that retypes it can spell other elements once and twice converted: `["010", "9"]`
/// declared `list(number)` and retyped `list(string)` or `any` names `f-0-10`, `f-1-9` in
/// Terraform v1.17.1 (the override in `override.tf`) and `f-0-010`, `f-1-9` in OpenTofu v1.13.9.
/// `["10", "9"]` retyped `any` names `f-0-10`, `f-1-9` in both, strings once and numbers twice
/// converted, and is OpenTofu's value where only OpenTofu reads the declaration: a read that tells
/// `"10"` from `10` names `f-0-ten` in OpenTofu, `f-0-other` in Terraform. Declared in a `.tf`
/// file, the variable is one Terraform reads without the override, a list of numbers where
/// OpenTofu holds strings, so no name is exact (#767).
#[test]
fn a_list_an_opentofu_override_retypes_is_unresolved_where_the_conversions_differ() {
    let main = |default: &str| {
        format!("variable \"s\" {{\n  type = list(number)\n  default = {default}\n}}\n{READ}")
    };
    let written = main(r#"["010", "9"]"#);
    for ty in ["list(string)", "any"] {
        let retype = format!("variable \"s\" {{\n  type = {ty}\n}}\n");
        for declared in ["main.tf", "main.tofu"] {
            let opentofu = [
                (declared, written.as_str()),
                ("override.tofu", retype.as_str()),
            ];
            assert_eq!(answer(&opentofu), UNRESOLVED, "{ty}, {declared}");
        }
        let terraform = [
            ("main.tf", written.as_str()),
            ("override.tf", retype.as_str()),
        ];
        assert_eq!(answer(&terraform), "f-0-10, f-1-9", "{ty}");
    }
    let canonical = main(r#"["10", "9"]"#);
    let any = "variable \"s\" {\n  type = any\n}\n";
    let opentofu = [("main.tofu", canonical.as_str()), ("override.tofu", any)];
    assert_eq!(answer(&opentofu), "f-0-10, f-1-9");
    let both = [("main.tf", canonical.as_str()), ("override.tofu", any)];
    assert_eq!(answer(&both), UNRESOLVED);
    let typed = r#"variable "s" {
  type = list(number)
  default = ["10", "9"]
}
resource "a" "f" {
  for_each = toset([for k, v in tolist(var.s) : "${k}-${v == "10" ? "ten" : "other"}"])
  name = "f-${each.value}"
}
"#;
    let opentofu = [("main.tofu", typed), ("override.tofu", any)];
    assert_eq!(answer(&opentofu), "f-0-ten, f-1-other");
    let both = [("main.tf", typed), ("override.tofu", any)];
    assert_eq!(answer(&both), UNRESOLVED);
    let terraform = [("main.tf", typed), ("override.tf", any)];
    assert_eq!(answer(&terraform), "f-0-other, f-1-other");
}

/// The last block that gives a default decides it: one an override file Terraform reads gives
/// after a `.tofu` override's is the default of both tools (`0-d`, `1-e` in each).
#[test]
fn a_default_a_later_override_gives_replaces_the_one_an_opentofu_override_gave() {
    let main = format!("{DECLARED}{READ}");
    let first = "variable \"s\" {\n  default = [\"c\"]\n}\n";
    let last = "variable \"s\" {\n  default = [\"e\", \"d\", \"d\"]\n}\n";
    let rows: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "a_override.tofu, then b_override.tf",
            vec![
                ("main.tf", &main),
                ("a_override.tofu", first),
                ("b_override.tf", last),
            ],
        ),
        (
            "a_override.tofu.json, then b_override.tf.json",
            vec![
                ("main.tf", &main),
                (
                    "a_override.tofu.json",
                    r#"{"variable": {"s": {"default": ["c"]}}}"#,
                ),
                (
                    "b_override.tf.json",
                    r#"{"variable": {"s": {"default": ["e", "d", "d"]}}}"#,
                ),
            ],
        ),
    ];
    for (row, files) in rows {
        assert_eq!(answer(&files), "f-0-d, f-1-e", "{row}");
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

/// A ceiling: only a variable's blocks are compared with Terraform's reading (#767). A local
/// overridden in a file only OpenTofu reads keeps OpenTofu's value: `"a"` in `main.tf` and `"b"`
/// in `override.tofu` names `f-b`, OpenTofu v1.13.9's instance, where Terraform v1.17.1 names
/// `f-a`.
#[test]
fn a_local_an_opentofu_override_gives_keeps_opentofu_s_value() {
    let main = "locals {\n  l = \"a\"\n}\nresource \"a\" \"f\" {\n  name = \"f-${local.l}\"\n}\n";
    let files = [
        ("main.tf", main),
        ("override.tofu", "locals {\n  l = \"b\"\n}\n"),
    ];
    assert_eq!(answer(&files), "f-b");
}
