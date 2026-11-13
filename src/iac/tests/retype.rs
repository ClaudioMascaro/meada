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

/// A row given as whole files: its label, the files, the address of the resource read and the
/// expected answer.
type FileRow<'a> = (&'a str, Vec<(&'a str, String)>, &'a str, &'a str);

const UNRESOLVED: &str = "unresolved";

/// The instances of a `for_each` over `source`, named by key and value, as `queue-<key>-<value>`
/// joined by `, `, or `unresolved` when Meada gives no exact name; `main` and the files `more`.
fn answer(main: &str, source: &str, more: &[(&str, &str)]) -> String {
    answer_in("main.tf", main, source, more)
}

/// `answer` for a declaring file named `file`.
fn answer_in(file: &str, main: &str, source: &str, more: &[(&str, &str)]) -> String {
    let main = format!("{main}{}", reader(source));
    let mut list = vec![(file, main.as_str())];
    list.extend_from_slice(more);
    exact_names(&list, "a.q")
}

/// The resource `a.q`: an instance per key of `source`, named `queue-<key>-<value>`.
fn reader(source: &str) -> String {
    format!(
        "resource \"a\" \"q\" {{\n  for_each = {{for k, v in {source} : k => v}}\n  name = \"queue-${{each.key}}-${{each.value}}\"\n}}\n"
    )
}

/// The resource `a.q` with `for_each = <source>`, each instance named `fe-<key>`.
fn for_each_over(source: &str) -> String {
    format!("resource \"a\" \"q\" {{\n  for_each = {source}\n  name = \"fe-${{each.key}}\"\n}}\n")
}

/// The exact names of the instances of `address` in the files `list`, joined by `, `, or
/// `unresolved` when Meada gives no exact name.
fn exact_names(list: &[(&str, &str)], address: &str) -> String {
    let found: Vec<String> = (names(list).into_iter())
        .filter(|n| n.starts_with(address))
        .collect();
    let named: Vec<&str> = (found.iter())
        .filter_map(|n| Some(n.split_once(" = ")?.1.split(" [").next().unwrap()))
        .map(|n| n.trim_matches('"'))
        .collect();
    match found.iter().any(|n| n.contains(" ~ ")) || named.is_empty() {
        true => UNRESOLVED.to_string(),
        false => named.join(", "),
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

/// A value that enters a variable declared `list(T)` of a primitive `T` has its elements
/// converted to `T`, in their order (#770): Terraform v1.17.1 and OpenTofu v1.13.9 agree on every
/// row, refuse `x` and `yes`, and name the last row `0-9`, `1--0`.
#[test]
fn a_value_entering_a_declared_list_is_converted_to_its_element_type() {
    let by = |main: &str| answer(main, "var.s", &[]);
    let list = |ty: &str, default: &str| {
        format!("variable \"s\" {{\n  type = list({ty})\n  default = {default}\n}}\n")
    };
    assert_eq!(
        by(&list("number", r#"["010", "9", "9", "1.50", "+5", "1e2"]"#)),
        "queue-0-10, queue-1-9, queue-2-9, queue-3-1.5, queue-4-5, queue-5-100"
    );
    assert_eq!(
        by(&list("string", "[1.50, true]")),
        "queue-0-1.5, queue-1-true"
    );
    assert_eq!(
        by(&list("bool", r#"["true", "false"]"#)),
        "queue-0-true, queue-1-false"
    );
    let declared = "variable \"s\" {\n  type = list(number)\n}\n";
    let given = [("terraform.tfvars", "s = [\"010\"]\n")];
    assert_eq!(answer(declared, "var.s", &given), "queue-0-10");
    // An element Meada cannot convert with certainty leaves the value unresolved: one the type
    // does not take, and `-0`, whose sign Meada's numbers do not keep.
    assert_eq!(by(&list("number", r#"["x"]"#)), UNRESOLVED);
    assert_eq!(by(&list("bool", r#"["yes"]"#)), UNRESOLVED);
    assert_eq!(by(&list("number", r#"["9", "-0"]"#)), UNRESOLVED);
}

/// A ceiling: Meada converts no other declared type, so `"010"` given to a primitive, a tuple, a
/// map, an object or a list of objects keeps its spelling, where Terraform v1.17.1 and OpenTofu
/// v1.13.9 name `10` in each.
#[test]
fn a_value_entering_a_type_meada_does_not_convert_keeps_its_spelling() {
    let kept = |ty: &str, default: &str, read: &str| {
        let main = format!("variable \"s\" {{\n  type = {ty}\n  default = {default}\n}}\n");
        answer(&main, &format!("{{ only = {read} }}"), &[])
    };
    for (ty, default, read) in [
        ("number", r#""010""#, "var.s"),
        ("tuple([number])", r#"["010"]"#, "var.s[0]"),
        ("map(number)", r#"{ a = "010" }"#, r#"var.s["a"]"#),
        ("object({ n = number })", r#"{ n = "010" }"#, "var.s.n"),
        (
            "list(object({ n = number }))",
            r#"[{ n = "010" }]"#,
            "var.s[0].n",
        ),
    ] {
        assert_eq!(kept(ty, default, read), "queue-only-010", "{ty}");
    }
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

/// A default its final type refuses (`set(string)` retyped a tuple, `["x"]` under `list(number)`):
/// Terraform v1.17.1 and OpenTofu v1.13.9 refuse the configuration whatever gives the variable
/// its value, so a var file's value and a module argument name nothing exactly (#769). A `null`
/// set retyped a tuple is refused by Terraform alone (OpenTofu converts it once), and Meada
/// follows Terraform (#740). A ceiling: a default Meada cannot convert with certainty is treated
/// the same, though both tools take `["-0"]` under `list(number)` and name `queue-0-1` and
/// `queue-1-2` for `["1", "2"]`. A default the final type takes keeps the value's names.
#[test]
fn a_default_meada_cannot_convert_to_its_types_leaves_every_source_unresolved() {
    let tuple = over("  type = tuple([string, string, string])\n");
    let list = over("  type = list(string)\n");
    let set = declared("[\"x\"]");
    let null = declared("null");
    let numbers = "variable \"s\" {\n  type = list(number)\n  default = [\"x\"]\n}\n";
    let negative_zero = "variable \"s\" {\n  type = list(number)\n  default = [\"-0\"]\n}\n";
    let three = "[\"b\", \"a\", \"a\"]";
    let rows = [
        (
            "-0 under list(number), a ceiling",
            negative_zero,
            None,
            "[\"1\", \"2\"]",
            UNRESOLVED,
        ),
        (
            "retyped a tuple",
            set.as_str(),
            Some(&tuple),
            three,
            UNRESOLVED,
        ),
        (
            "a null set retyped a tuple",
            null.as_str(),
            Some(&tuple),
            three,
            UNRESOLVED,
        ),
        (
            "declared list(number)",
            numbers,
            None,
            "[\"1\", \"2\"]",
            UNRESOLVED,
        ),
        (
            "retyped a list",
            set.as_str(),
            Some(&list),
            three,
            "queue-0-b, queue-1-a, queue-2-a",
        ),
    ];
    for (row, declaration, retype, value, want) in rows {
        let given = format!("s = {value}\n");
        for vars in ["terraform.tfvars", "x.auto.tfvars"] {
            let mut more = vec![(vars, given.as_str())];
            more.extend(retype.map(|t| ("override.tf", t.as_str())));
            assert_eq!(answer(declaration, "var.s", &more), want, "{row}, {vars}");
        }
        let call = format!("module \"m\" {{\n  source = \"./m\"\n  {given}}}\n");
        let child = format!("{declaration}{}", reader("var.s"));
        let mut files = vec![("main.tf", call.as_str()), ("m/main.tf", child.as_str())];
        files.extend(retype.map(|t| ("m/override.tf", t.as_str())));
        assert_eq!(
            exact_names(&files, "module.m.a.q"),
            want,
            "{row}, module argument"
        );
    }
}

/// `for_each` over a tuple: Terraform v1.17.1 and OpenTofu v1.13.9 refuse it (it takes a map or a
/// set of strings), so no name is exact (#769). The same elements as a set name each instance.
#[test]
fn for_each_over_a_tuple_gives_no_exact_name() {
    let each = for_each_over("var.s");
    let typed = |body: &str| format!("{}{each}", over(body));
    let call = |arg: &str| format!("module \"m\" {{\n  source = \"./m\"\n  s = {arg}\n}}\n");
    let any = over("  type = any\n");
    let vars = ("terraform.tfvars", "s = [\"b\", \"a\", \"a\"]\n");
    let set_x = format!("{}{each}", declared("[\"x\"]"));
    let set_dup = format!("{}{each}", declared("[\"b\", \"a\", \"a\"]"));
    let rows: [FileRow; 12] = [
        (
            "a var file's value, retyped any",
            vec![
                ("main.tf", set_x),
                ("override.tf", any.clone()),
                (vars.0, vars.1.into()),
            ],
            "a.q",
            UNRESOLVED,
        ),
        (
            "a var file's value, untyped",
            vec![("main.tf", typed("")), (vars.0, vars.1.into())],
            "a.q",
            UNRESOLVED,
        ),
        (
            "an untyped default",
            vec![("main.tf", typed("  default = [\"b\", \"a\"]\n"))],
            "a.q",
            UNRESOLVED,
        ),
        (
            "a default typed any",
            vec![(
                "main.tf",
                typed("  type = any\n  default = [\"b\", \"a\"]\n"),
            )],
            "a.q",
            UNRESOLVED,
        ),
        (
            "a declared tuple",
            vec![(
                "main.tf",
                typed("  type = tuple([string, string])\n  default = [\"b\", \"a\"]\n"),
            )],
            "a.q",
            UNRESOLVED,
        ),
        (
            "a module argument typed any",
            vec![
                ("main.tf", call("[\"b\", \"a\"]")),
                ("m/main.tf", typed("  type = any\n")),
            ],
            "module.m.a.q",
            UNRESOLVED,
        ),
        (
            "a module argument written as a for",
            vec![
                ("main.tf", call("[for x in [\"b\", \"a\"] : x]")),
                ("m/main.tf", typed("")),
            ],
            "module.m.a.q",
            UNRESOLVED,
        ),
        (
            "written out",
            vec![("main.tf", for_each_over("[\"b\", \"a\"]"))],
            "a.q",
            UNRESOLVED,
        ),
        (
            "a set retyped any, on its default",
            vec![("main.tf", set_dup), ("override.tf", any)],
            "a.q",
            "fe-a, fe-b",
        ),
        (
            "a var file's value of a set",
            vec![
                ("main.tf", typed("  type = set(string)\n")),
                (vars.0, vars.1.into()),
            ],
            "a.q",
            "fe-a, fe-b",
        ),
        (
            "a module argument made a set",
            vec![
                ("main.tf", call("toset([\"b\", \"a\"])")),
                ("m/main.tf", typed("  type = any\n")),
            ],
            "module.m.a.q",
            "fe-a, fe-b",
        ),
        (
            "a module argument to a set",
            vec![
                ("main.tf", call("[\"b\", \"a\", \"a\"]")),
                ("m/main.tf", typed("  type = set(string)\n")),
            ],
            "module.m.a.q",
            "fe-a, fe-b",
        ),
    ];
    for (row, files, address, want) in rows {
        let list: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
        assert_eq!(exact_names(&list, address), want, "{row}");
    }
}

/// A ceiling: a list Meada does not know as a tuple expands, though Terraform v1.17.1 and OpenTofu
/// v1.13.9 refuse a `for_each` over a list as over a tuple (#769). Meada keeps no value's kind
/// past a variable read whole (#717): a declared `list(string)`, a local holding a tuple and the
/// result of `concat`, `keys` or `tolist` name `fe-a` and `fe-b`.
#[test]
fn a_list_meada_does_not_know_as_a_tuple_still_expands() {
    let list = "variable \"s\" {\n  type = list(string)\n  default = [\"b\", \"a\"]\n}\n";
    let local = "locals {\n  l = [\"b\", \"a\"]\n}\n";
    for (row, main) in [
        (
            "a declared list",
            format!("{list}{}", for_each_over("var.s")),
        ),
        ("a local", format!("{local}{}", for_each_over("local.l"))),
        ("concat", for_each_over("concat([\"b\"], [\"a\"])")),
        ("keys", for_each_over("keys({ b = 1, a = 2 })")),
        ("tolist", for_each_over("tolist([\"b\", \"a\"])")),
    ] {
        assert_eq!(
            exact_names(&[("main.tf", main.as_str())], "a.q"),
            "fe-a, fe-b",
            "{row}"
        );
    }
}

/// Whether a variable holds a set is known per root (#783): roots evaluated one after another in
/// one repository, each declaring its own `var.s`, answer as each would alone. The root `a` is
/// evaluated first, its resource `a.p`; the root `b` second, its resource `a.q`.
#[test]
fn each_root_knows_its_own_sets() {
    let root_a = |declaring: &str| {
        let resource = reader("var.s").replace("resource \"a\" \"q\"", "resource \"a\" \"p\"");
        format!("{declaring}{resource}")
    };
    let root_b = |declaring: &str| format!("{declaring}{}", reader("var.s"));
    let set = declared("[\"b\", \"a\", \"a\"]");
    let list = "variable \"s\" {\n  type = list(string)\n  default = [\"b\", \"a\", \"a\"]\n}\n";
    let retype = over("  type = list(string)\n");
    let after_a_set = [
        ("a/main.tf", root_a(&set)),
        ("b/main.tf", root_b(&set)),
        ("b/override.tf", retype),
    ];
    let after_a_list = [("a/main.tf", root_a(list)), ("b/main.tf", root_b(&set))];
    let answers = [&after_a_set[..], &after_a_list[..]].map(|files| {
        let files: Vec<(&str, &str)> = files.iter().map(|(f, s)| (*f, s.as_str())).collect();
        exact_names(&files, "a.q")
    });
    // A set in the root before leaves the list `b` retypes it to named; a list in the root before
    // names no instance of `b`'s set by index.
    assert_eq!(answers, ["queue-0-a, queue-1-b", UNRESOLVED]);
}
