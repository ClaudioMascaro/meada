//! A declared type with no set at its top level leaves the value as an untyped variable keeps it,
//! from every source (#720): only a top-level `set(...)` converts. A list of objects, maps or
//! lists named by `for_each` gives its keys, as Terraform does.

use super::*;

/// The exact names of the instances in `list`, each without its `[var ...]` suffix.
fn exact(list: &[(&str, &str)]) -> Vec<String> {
    (names(list).into_iter())
        .map(|n| n.split(" [").next().unwrap().to_string())
        .collect()
}

/// A list of two collections keyed by their first element: its type, its HCL value, its JSON
/// value and the key expression of one element `q`.
type Case<'a> = (&'a str, &'a str, serde_json::Value, &'a str);

fn cases<'a>() -> [(&'a str, Case<'a>); 4] {
    let objects = serde_json::json!([{"n": "b"}, {"n": "a"}]);
    let lists = serde_json::json!([["b"], ["a"]]);
    [
        (
            "list(object)",
            (
                "list(object({ n = string }))",
                r#"[{ n = "b" }, { n = "a" }]"#,
                objects.clone(),
                "q.n",
            ),
        ),
        (
            "list(map)",
            (
                "list(map(string))",
                r#"[{ n = "b" }, { n = "a" }]"#,
                objects.clone(),
                "q.n",
            ),
        ),
        (
            "list(list)",
            ("list(list(string))", r#"[["b"], ["a"]]"#, lists, "q[0]"),
        ),
        (
            "tuple of objects",
            (
                "tuple([object({ n = string }), object({ n = string })])",
                r#"[{ n = "b" }, { n = "a" }]"#,
                objects,
                "q.n",
            ),
        ),
    ]
}

fn resource(key: &str) -> String {
    format!(
        "resource \"a\" \"obj\" {{\n  for_each = {{ for q in var.queues : {key} => q }}\n  name = \"obj-${{each.key}}\"\n}}\n"
    )
}

/// A source: its label, its files and the prefix of its instances' addresses.
type Source<'a> = (&'a str, Vec<(&'a str, &'a str)>, String);

const WANT: [&str; 2] = ["a.obj[\"a\"] = \"obj-a\"", "a.obj[\"b\"] = \"obj-b\""];

#[test]
fn a_declared_list_of_collections_gives_its_keys_from_every_source() {
    for (label, (ty, value, json, key)) in cases() {
        let read = resource(key);
        let declared = format!("variable \"queues\" {{\n  type = {ty}\n}}\n{read}");
        let with_default =
            format!("variable \"queues\" {{\n  type = {ty}\n  default = {value}\n}}\n{read}");
        let tfvars = format!("queues = {value}\n");
        let json_vars = serde_json::json!({ "queues": json }).to_string();
        let argument = format!("module \"m\" {{\n  source = \"./m\"\n  queues = {value}\n}}\n");
        let earlier = format!("variable \"queues\" {{\n  type = {ty}\n  default = []\n}}\n{read}");
        let override_default = format!("variable \"queues\" {{\n  default = {value}\n}}\n");
        let in_json = serde_json::json!({
            "variable": {"queues": {"type": ty, "default": json}},
            "resource": {"a": {"obj": {
                "for_each": format!("${{{{ for q in var.queues : {key} => q }}}}"),
                "name": "obj-${each.key}"
            }}}
        })
        .to_string();
        let rows: Vec<Source> = vec![
            ("default", vec![("main.tf", &with_default)], String::new()),
            (
                "terraform.tfvars",
                vec![("main.tf", &declared), ("terraform.tfvars", &tfvars)],
                String::new(),
            ),
            (
                "auto.tfvars",
                vec![("main.tf", &declared), ("x.auto.tfvars", &tfvars)],
                String::new(),
            ),
            (
                "json var file",
                vec![
                    ("main.tf", &declared),
                    ("terraform.tfvars.json", &json_vars),
                ],
                String::new(),
            ),
            (
                "module argument",
                vec![("main.tf", &argument), ("m/main.tf", &declared)],
                "module.m.".into(),
            ),
            (
                "override default",
                vec![("main.tf", &earlier), ("override.tf", &override_default)],
                String::new(),
            ),
            (
                "main.tf.json",
                vec![("main.tf.json", &in_json)],
                String::new(),
            ),
        ];
        for (source, files, prefix) in rows {
            let got: Vec<String> = (exact(&files).into_iter())
                .filter(|n| n.starts_with(&format!("{prefix}a.obj")))
                .collect();
            let want: Vec<String> = WANT.iter().map(|w| format!("{prefix}{w}")).collect();
            assert_eq!(got, want, "{label} from {source}");
        }
    }
}

/// Every declared type with no set at its top level (a list, a tuple, a map, an object, each of a
/// scalar, an object, a map or a list) names what an untyped variable of the same value names.
#[test]
fn a_type_without_a_top_level_set_names_what_an_untyped_variable_names() {
    let shapes: [(&str, &str, &str); 16] = [
        ("list(string)", "list(string)", r#"["b", "a"]"#),
        (
            "list(object)",
            "list(object({ n = string }))",
            r#"[{ n = "b" }, { n = "a" }]"#,
        ),
        (
            "list(map)",
            "list(map(string))",
            r#"[{ n = "b" }, { n = "a" }]"#,
        ),
        ("list(list)", "list(list(string))", r#"[["b"], ["a"]]"#),
        (
            "tuple of scalars",
            "tuple([string, string])",
            r#"["b", "a"]"#,
        ),
        (
            "tuple of objects",
            "tuple([object({ n = string }), object({ n = string })])",
            r#"[{ n = "b" }, { n = "a" }]"#,
        ),
        (
            "tuple of maps",
            "tuple([map(string), map(string)])",
            r#"[{ n = "b" }, { n = "a" }]"#,
        ),
        (
            "tuple of lists",
            "tuple([list(string), list(string)])",
            r#"[["b"], ["a"]]"#,
        ),
        ("map(string)", "map(string)", r#"{ x = "b", y = "a" }"#),
        (
            "map(object)",
            "map(object({ n = string }))",
            r#"{ x = { n = "b" }, y = { n = "a" } }"#,
        ),
        (
            "map(map)",
            "map(map(string))",
            r#"{ x = { n = "b" }, y = { n = "a" } }"#,
        ),
        (
            "map(list)",
            "map(list(string))",
            r#"{ x = ["b"], y = ["a"] }"#,
        ),
        (
            "object of scalars",
            "object({ x = string, y = string })",
            r#"{ x = "b", y = "a" }"#,
        ),
        (
            "object of objects",
            "object({ x = object({ n = string }), y = object({ n = string }) })",
            r#"{ x = { n = "b" }, y = { n = "a" } }"#,
        ),
        (
            "object of maps",
            "object({ x = map(string), y = map(string) })",
            r#"{ x = { n = "b" }, y = { n = "a" } }"#,
        ),
        (
            "object of lists",
            "object({ x = list(string), y = list(string) })",
            r#"{ x = ["b"], y = ["a"] }"#,
        ),
    ];
    let read = "resource \"a\" \"q\" {\n  for_each = { for k, v in var.x : k => v }\n  name = \"q-${each.key}\"\n}\n";
    for (label, ty, value) in shapes {
        let typed = format!("variable \"x\" {{\n  type = {ty}\n  default = {value}\n}}\n{read}");
        let untyped = format!("variable \"x\" {{\n  default = {value}\n}}\n{read}");
        let got = names(&[("main.tf", &typed)]);
        assert_eq!(got, names(&[("main.tf", &untyped)]), "{label}");
        assert!(
            got.iter().any(|n| n.contains(" = \"q-")),
            "{label}: {got:?}"
        );
        // The same value given by a var file.
        let declared = format!("variable \"x\" {{\n  type = {ty}\n}}\n{read}");
        let bare = format!("variable \"x\" {{\n}}\n{read}");
        let vars = format!("x = {value}\n");
        let got = names(&[("main.tf", &declared), ("terraform.tfvars", &vars)]);
        assert!(
            got.iter().any(|n| n.contains(" = \"q-")),
            "{label} by a var file: {got:?}"
        );
        assert_eq!(
            got,
            names(&[("main.tf", &bare), ("terraform.tfvars", &vars)]),
            "{label} by a var file"
        );
    }
}
