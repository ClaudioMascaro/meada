//! An override file's `type` over a variable whose own block declares a set (#740). Terraform
//! converts the default to the declared set when it decodes the block, then again to the override's
//! type; OpenTofu 1.13 and later convert it once, to the final type. The two tools name different
//! instances, so a default that comes from the declaring block stays unresolved under any override
//! `type`; a default the override gives, or a value a var file gives, is converted once by both.

use super::*;

/// The nodes of a `for_each` over `var.s` named by its key and value, declared by `main` with the
/// override file `over` and the var file `vars` (if any).
fn named(declared: &str, over: &str, vars: Option<&str>) -> Vec<String> {
    let main = format!(
        "variable \"s\" {{\n{declared}}}\nresource \"a\" \"q\" {{\n  for_each = {{for k, v in var.s : k => v}}\n  name = \"queue-${{each.key}}-${{each.value}}\"\n}}\n"
    );
    let over = format!("variable \"s\" {{\n{over}}}\n");
    let mut list = vec![("main.tf", main.as_str()), ("override.tf", over.as_str())];
    list.extend(vars.map(|v| ("terraform.tfvars", v)));
    names(&list)
}

const SET: &str = "  type = set(string)\n  default = [\"b\", \"a\", \"a\"]\n";
const UNRESOLVED: [&str; 1] = ["a.q ~ \"queue-*-*\""];

#[test]
fn an_override_retyping_a_declared_set_leaves_the_default_unresolved() {
    for over in [
        "  type = list(string)\n",
        "  type = any\n",
        "  type = tuple([string, string])\n",
        "  type = set(string)\n",
    ] {
        assert_eq!(named(SET, over, None), UNRESOLVED, "{over}");
    }
    let reordered = "  type = set(string)\n  default = [\"b\", \"a\"]\n";
    assert_eq!(
        named(reordered, "  type = list(string)\n", None),
        UNRESOLVED
    );
}

#[test]
fn a_default_converted_once_keeps_its_exact_names() {
    let exact = |names: Vec<String>| -> Vec<String> {
        names
            .iter()
            .map(|n| n.split(" [").next().unwrap().to_string())
            .collect()
    };
    // A default the override gives: both tools convert it once, to the final list.
    let list = "  type = list(string)\n  default = [\"b\", \"a\", \"a\"]\n";
    let want = [
        "a.q[\"0\"] = \"queue-0-d\"",
        "a.q[\"1\"] = \"queue-1-c\"",
        "a.q[\"2\"] = \"queue-2-c\"",
    ];
    let over = "  type = list(string)\n  default = [\"d\", \"c\", \"c\"]\n";
    assert_eq!(exact(named(list, over, None)), want);
    // No declared type: nothing was converted to a set.
    let untyped = "  default = [\"b\", \"a\", \"a\"]\n";
    let want = [
        "a.q[\"0\"] = \"queue-0-b\"",
        "a.q[\"1\"] = \"queue-1-a\"",
        "a.q[\"2\"] = \"queue-2-a\"",
    ];
    assert_eq!(exact(named(untyped, "  type = list(string)\n", None)), want);
    // A var file's value is converted once, to the final type, whatever the default was.
    let vars = Some("s = [\"b\", \"a\", \"a\"]\n");
    assert_eq!(exact(named(SET, "  type = list(string)\n", vars)), want);
}
