//! Terraform and OpenTofu files as config values (#640, ADR 0025): every attribute of every
//! block of a `.tf`, `.tofu`, `.tfvars` or `*.auto.tfvars` file is a row, its key path running
//! over the block's type and labels and the attribute's name (`resource.aws_sqs_queue.orders.name`),
//! a block repeated under the same type and labels `[n]` as the JSON syntax writes it. `hcl-edit`
//! parses (a span for every node, so a line and the text as written); `hcl-rs` reads the string a
//! heredoc of literals holds. `.tf.json`, `.tofu.json` and `.tfvars.json` go through the JSON
//! reader in the parent module, whose key paths are the same.
//!
//! A literal keeps its type (`string`, `number`, `boolean`; null is left out). What is not a
//! literal is stored as written under a type of its own, until static evaluation resolves it:
//! `expression` (a reference, a call, an operator, a `for`), `template` (a string or heredoc with
//! an interpolation or a directive). A list or an object of literal keys is read element by
//! element, as YAML is; an object with a key that is no literal is one `expression`.
//!
//! A `variable` with `sensitive = true` keeps its `default` redacted, and a `.tfvars` file every
//! value it gives to a sensitive variable (`Sensitive`): key path, type and line, no value.

use std::collections::{BTreeSet, HashMap};

use hcl::eval::{Context, Evaluate};
use hcl_edit::Span;
use hcl_edit::expr::{Expression, ObjectKey};
use hcl_edit::parser;
use hcl_edit::structure::{Block, Body, Structure};
use hcl_edit::template::Element;

use super::{ANY_VARIABLE, Row, STORED_EXPANSION, STORED_SLACK, VALUE_MAX, is_tfvars, key_path};
use crate::document::events::Step;

/// How many brackets (`{`, `[`, `(`) may be open inside each other in one file; deeper, the file
/// is unreadable. The parser and the walk below recurse once per level.
// ponytail: real Terraform nests a dozen levels; 64 keeps the parser's recursion shallow in a
// debug build's test thread.
const NESTING_MAX: usize = 64;

/// The limits `hcl-edit` 0.10 puts on a file, past which the parser fails and the file is
/// unreadable: chained operators (`MAX_OPERATOR_DEPTH`) and nested expressions, templates and
/// blocks (`MAX_NESTING_DEPTH`). Named here for the test that feeds them; the parser enforces them.
#[cfg(test)]
const PARSER_OPERATOR_DEPTH: usize = 128;
#[cfg(test)]
const PARSER_NESTING_DEPTH: usize = 64;

type Fail = (usize, String);

/// Whether the file at `path` is read as HCL: `*.tf`, `*.tofu`, `*.tfvars`.
pub(super) fn is_hcl(path: &str) -> bool {
    [".tf", ".tofu", ".tfvars"]
        .iter()
        .any(|e| path.ends_with(e))
}

/// The rows of the HCL file at `path`: `Err` with the line and the reason when it does not
/// parse, nests past `NESTING_MAX` or stores past the budget. `sensitive` are the variables a
/// `.tfvars` file gives values to that are redacted.
pub(super) fn read(path: &str, src: &[u8], sensitive: &BTreeSet<String>) -> Result<Vec<Row>, Fail> {
    let text = std::str::from_utf8(src).map_err(|e| {
        let line = src[..e.valid_up_to()]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        (line, "not UTF-8".to_string())
    })?;
    let newlines: Vec<usize> = (text.bytes().enumerate())
        .filter(|&(_, b)| b == b'\n')
        .map(|(i, _)| i)
        .collect();
    let line = |offset: usize| newlines.partition_point(|&n| n < offset) + 1;
    if let Some(at) = nested_past(text) {
        return Err((
            line(at),
            format!("nested deeper than {NESTING_MAX} brackets"),
        ));
    }
    let body = parser::parse_body(text).map_err(|e| {
        let reason = e.message().lines().next().unwrap_or_default();
        (e.location().line(), reason.to_string())
    })?;
    let budget = src
        .len()
        .saturating_mul(STORED_EXPANSION)
        .saturating_add(STORED_SLACK);
    let mut walk = Walk {
        text,
        newlines,
        left: budget,
        budget,
        rows: Vec::new(),
    };
    let tfvars = is_tfvars(path);
    let hide = if tfvars {
        Hide::Names(sensitive)
    } else {
        Hide::Nothing
    };
    walk.body(&body, &mut Vec::new(), 0, false, hide)?;
    Ok(walk.rows)
}

/// The byte offset where brackets nest past `NESTING_MAX`, counting every `{`, `[` and `(`
/// even inside a string: a count that is only ever too high.
fn nested_past(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (at, b) in text.bytes().enumerate() {
        match b {
            b'{' | b'[' | b'(' => {
                depth += 1;
                if depth > NESTING_MAX {
                    return Some(at);
                }
            }
            b'}' | b']' | b')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

/// Which attributes of a body are secret.
#[derive(Clone, Copy)]
enum Hide<'a> {
    Nothing,
    /// The attribute of this name: a sensitive variable's `default`.
    Attribute(&'a str),
    /// Every attribute named in the set: the values a `.tfvars` file gives sensitive variables.
    Names(&'a BTreeSet<String>),
}

impl Hide<'_> {
    fn hides(self, name: &str) -> bool {
        match self {
            Hide::Nothing => false,
            Hide::Attribute(a) => a == name,
            Hide::Names(names) => names.contains(name) || names.contains(ANY_VARIABLE),
        }
    }
}

struct Walk<'a> {
    text: &'a str,
    /// The offset of every `\n`, for the line of a span.
    newlines: Vec<usize>,
    budget: usize,
    /// What the rows may still store.
    left: usize,
    rows: Vec<Row>,
}

impl Walk<'_> {
    fn line(&self, offset: usize) -> usize {
        self.newlines.partition_point(|&n| n < offset) + 1
    }

    /// Adds the row at `path`; `value` is `None` for a secret. The text is cut at `VALUE_MAX`.
    fn leaf(
        &mut self,
        path: &[Step],
        value: Option<&str>,
        kind: &'static str,
        offset: usize,
    ) -> Result<(), Fail> {
        let key = key_path(path);
        let line = self.line(offset);
        let (value, cut) = match value {
            Some(v) if v.len() > VALUE_MAX => (
                Some(v[..v.floor_char_boundary(VALUE_MAX)].to_string()),
                true,
            ),
            v => (v.map(str::to_string), false),
        };
        let size = key.len() + value.as_ref().map_or(0, String::len);
        self.left = self
            .left
            .checked_sub(size)
            .ok_or_else(|| (line, format!("values past {} bytes", self.budget)))?;
        self.rows.push(Row {
            key,
            value,
            kind,
            line,
            cut,
        });
        Ok(())
    }

    /// The structures of `body` under `path`: an attribute is a leaf, or a collection read
    /// element by element; a block adds its type and labels, and an index when the same type and
    /// labels repeat in this body. `hidden` is a secret subtree, `hide` the attributes of this
    /// body that are secret.
    fn body(
        &mut self,
        body: &Body,
        path: &mut Vec<Step>,
        depth: usize,
        hidden: bool,
        hide: Hide,
    ) -> Result<(), Fail> {
        let names = |b: &Block| -> Vec<String> {
            let mut names = vec![b.ident.as_str().to_string()];
            names.extend(b.labels.iter().map(|l| l.as_str().to_string()));
            names
        };
        let mut total: HashMap<Vec<String>, usize> = HashMap::new();
        for block in body.blocks() {
            *total.entry(names(block)).or_default() += 1;
        }
        let mut seen: HashMap<Vec<String>, usize> = HashMap::new();
        for structure in body.iter() {
            match structure {
                Structure::Attribute(a) => {
                    let name = a.key.as_str();
                    path.push(Step::Key(name.to_string()));
                    let secret = hidden || hide.hides(name);
                    self.expression(&a.value, path, secret)?;
                    path.pop();
                }
                Structure::Block(b) => {
                    let names = names(b);
                    let before = path.len();
                    path.extend(names.iter().cloned().map(Step::Key));
                    if total[&names] > 1 {
                        let n = seen.entry(names).or_default();
                        path.push(Step::Index(*n));
                        *n += 1;
                    }
                    // A variable's `default` is secret when it says `sensitive = true`.
                    let sensitive = depth == 0
                        && b.ident.as_str() == "variable"
                        && b.labels.len() == 1
                        && (b.body.get_attribute("sensitive"))
                            .is_some_and(|a| matches!(&a.value, Expression::Bool(v) if *v.value()));
                    let inner = if sensitive {
                        Hide::Attribute("default")
                    } else {
                        Hide::Nothing
                    };
                    self.body(&b.body, path, depth + 1, hidden, inner)?;
                    path.truncate(before);
                }
            }
        }
        Ok(())
    }

    fn expression(
        &mut self,
        expr: &Expression,
        path: &mut Vec<Step>,
        hidden: bool,
    ) -> Result<(), Fail> {
        let offset = expr.span().map_or(0, |s| s.start);
        let source = self.text;
        let written = expr.span().and_then(|s| source.get(s));
        match expr {
            Expression::Null(_) => Ok(()),
            Expression::Bool(b) => {
                let text = b.value().to_string();
                self.leaf(path, shown(hidden, Some(&text)), "boolean", offset)
            }
            Expression::Number(n) => {
                let text = n.value().to_string();
                let text = written.unwrap_or(&text).to_string();
                self.leaf(path, shown(hidden, Some(&text)), "number", offset)
            }
            Expression::String(s) => {
                self.leaf(path, shown(hidden, Some(s.value())), "string", offset)
            }
            Expression::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    path.push(Step::Index(i));
                    self.expression(item, path, hidden)?;
                    path.pop();
                }
                Ok(())
            }
            Expression::Object(object) => {
                let keys: Option<Vec<String>> = object
                    .iter()
                    .map(|(k, _)| match k {
                        ObjectKey::Ident(i) => Some(i.as_str().to_string()),
                        ObjectKey::Expression(Expression::String(s)) => Some(s.value().clone()),
                        ObjectKey::Expression(_) => None,
                    })
                    .collect();
                let Some(keys) = keys else {
                    let text = written.unwrap_or_default();
                    return self.leaf(path, shown(hidden, Some(text)), "expression", offset);
                };
                for (key, (_, value)) in keys.into_iter().zip(object.iter()) {
                    path.push(Step::Key(key));
                    self.expression(value.expr(), path, hidden)?;
                    path.pop();
                }
                Ok(())
            }
            Expression::HeredocTemplate(h)
                if h.template.iter().all(|e| matches!(e, Element::Literal(_))) =>
            {
                // A heredoc of literals is the string HCL reads: its indentation stripped.
                match hcl::Expression::from(expr.clone()).evaluate(&Context::new()) {
                    Ok(hcl::Value::String(s)) => {
                        self.leaf(path, shown(hidden, Some(&s)), "string", offset)
                    }
                    _ => self.leaf(path, shown(hidden, written), "template", offset),
                }
            }
            Expression::StringTemplate(_) | Expression::HeredocTemplate(_) => {
                self.leaf(path, shown(hidden, written), "template", offset)
            }
            _ => self.leaf(path, shown(hidden, written), "expression", offset),
        }
    }
}

/// `text`, unless it is secret.
fn shown(hidden: bool, text: Option<&str>) -> Option<&str> {
    text.filter(|_| !hidden)
}

#[cfg(test)]
mod tests {
    use super::super::{Outcome, VALUE_MAX, read, read_with};
    use super::*;

    /// `(key path, value, type, line)` of the rows of `text` read as the file `path`.
    fn rows(path: &str, text: &str) -> Vec<(String, Option<String>, &'static str, usize)> {
        let (outcome, rows) = read(path, text.as_bytes());
        assert!(matches!(outcome, Outcome::Read { .. }), "{outcome:?}");
        (rows.into_iter())
            .map(|r| (r.key, r.value, r.kind, r.line))
            .collect()
    }

    fn row(
        key: &str,
        value: &str,
        kind: &'static str,
        line: usize,
    ) -> (String, Option<String>, &'static str, usize) {
        (key.into(), Some(value.into()), kind, line)
    }

    #[test]
    fn a_key_path_runs_over_block_types_labels_and_attribute_names() {
        let text = concat!(
            "resource \"aws_sqs_queue\" \"orders\" {\n",
            "  name = \"orders-queue\"\n",
            "  fifo_queue = true\n",
            "  delay_seconds = 5\n",
            "  tags = {\n    team = \"payments\"\n    \"cost-center\" = \"42\"\n  }\n",
            "  redrive { max_receive_count = 3 }\n",
            "}\n",
            "resource \"aws_sqs_queue\" \"my-dead\" {\n  name = \"dead\"\n}\n",
        );
        assert_eq!(
            rows("main.tf", text),
            [
                row(
                    "resource.aws_sqs_queue.orders.name",
                    "orders-queue",
                    "string",
                    2
                ),
                row(
                    "resource.aws_sqs_queue.orders.fifo_queue",
                    "true",
                    "boolean",
                    3
                ),
                row(
                    "resource.aws_sqs_queue.orders.delay_seconds",
                    "5",
                    "number",
                    4
                ),
                row(
                    "resource.aws_sqs_queue.orders.tags.team",
                    "payments",
                    "string",
                    6
                ),
                row(
                    "resource.aws_sqs_queue.orders.tags[\"cost-center\"]",
                    "42",
                    "string",
                    7
                ),
                row(
                    "resource.aws_sqs_queue.orders.redrive.max_receive_count",
                    "3",
                    "number",
                    9
                ),
                row(
                    "resource.aws_sqs_queue[\"my-dead\"].name",
                    "dead",
                    "string",
                    12
                ),
            ]
        );
    }

    #[test]
    fn a_block_repeated_under_the_same_type_and_labels_is_indexed_from_zero() {
        let text = concat!(
            "resource \"aws_security_group\" \"web\" {\n",
            "  ingress {\n    from_port = 80\n  }\n",
            "  ingress {\n    from_port = 443\n  }\n",
            "  egress {\n    from_port = 0\n  }\n",
            "}\n",
            "locals { a = 1 }\nlocals { b = 2 }\n",
            "provider \"aws\" { alias = \"eu\" }\nprovider \"aws\" { alias = \"us\" }\n",
            "provider \"google\" { project = \"p\" }\n",
        );
        let keys: Vec<String> = rows("main.tf", text).into_iter().map(|r| r.0).collect();
        assert_eq!(
            keys,
            [
                "resource.aws_security_group.web.ingress[0].from_port",
                "resource.aws_security_group.web.ingress[1].from_port",
                "resource.aws_security_group.web.egress.from_port",
                "locals[0].a",
                "locals[1].b",
                "provider.aws[0].alias",
                "provider.aws[1].alias",
                "provider.google.project",
            ]
        );
    }

    #[test]
    fn a_list_and_an_object_are_read_element_by_element() {
        let text = "locals {\n  zones = [\"a\", \"b\"]\n  nested = [{ x = 1 }, [true]]\n  empty = []\n  none = null\n}\n";
        assert_eq!(
            rows("main.tf", text),
            [
                row("locals.zones[0]", "a", "string", 2),
                row("locals.zones[1]", "b", "string", 2),
                row("locals.nested[0].x", "1", "number", 3),
                row("locals.nested[1][0]", "true", "boolean", 3),
            ]
        );
    }

    #[test]
    fn an_expression_is_stored_as_written_under_a_type_of_its_own() {
        let text = concat!(
            "locals {\n",
            "  ref = var.environment\n",
            "  call = format(\"%s-%s\", var.a, \"b\")\n",
            "  cond = var.on ? \"x\" : \"y\"\n",
            "  sum = 1 + 2\n",
            "  idx = aws_sqs_queue.orders[0].arn\n",
            "  each = [for s in var.list : upper(s)]\n",
            "  paren = (1)\n",
            "  dyn = { (var.k) = 1 }\n",
            "}\n",
        );
        let found = rows("main.tf", text);
        let kinds: Vec<_> = found
            .iter()
            .map(|r| (r.0.as_str(), r.1.as_deref().unwrap(), r.2))
            .collect();
        assert_eq!(
            kinds,
            [
                ("locals.ref", "var.environment", "expression"),
                (
                    "locals.call",
                    "format(\"%s-%s\", var.a, \"b\")",
                    "expression"
                ),
                ("locals.cond", "var.on ? \"x\" : \"y\"", "expression"),
                ("locals.sum", "1 + 2", "expression"),
                ("locals.idx", "aws_sqs_queue.orders[0].arn", "expression"),
                (
                    "locals.each",
                    "[for s in var.list : upper(s)]",
                    "expression"
                ),
                ("locals.paren", "(1)", "expression"),
                ("locals.dyn", "{ (var.k) = 1 }", "expression"),
            ]
        );
    }

    #[test]
    fn a_string_with_an_interpolation_or_a_directive_is_a_template_as_written() {
        let text = concat!(
            "locals {\n",
            "  plain = \"orders\"\n",
            "  escaped = \"a$${b}\"\n",
            "  name = \"${var.env}-orders\"\n",
            "  only = \"${var.env}\"\n",
            "  directive = \"%{ if var.on }yes%{ else }no%{ endif }\"\n",
            "}\n",
        );
        let found = rows("main.tf", text);
        let shown: Vec<_> = found
            .iter()
            .map(|r| (r.1.as_deref().unwrap(), r.2))
            .collect();
        assert_eq!(
            shown,
            [
                ("orders", "string"),
                ("a${b}", "string"),
                ("\"${var.env}-orders\"", "template"),
                ("\"${var.env}\"", "template"),
                ("\"%{ if var.on }yes%{ else }no%{ endif }\"", "template"),
            ]
        );
    }

    #[test]
    fn a_heredoc_of_literals_is_a_string_and_one_with_an_interpolation_a_template() {
        let text = concat!(
            "resource \"aws_sqs_queue_policy\" \"p\" {\n",
            "  policy = <<-EOT\n    {\n      \"Sid\": \"all\"\n    }\n  EOT\n",
            "  raw = <<EOT\nkeep  indent\nEOT\n",
            "  dynamic = <<EOT\nqueue: ${aws_sqs_queue.orders.arn}\nEOT\n",
            "  after = 1\n",
            "}\n",
        );
        let found = rows("main.tf", text);
        assert_eq!(
            found[0],
            row(
                "resource.aws_sqs_queue_policy.p.policy",
                "{\n  \"Sid\": \"all\"\n}\n",
                "string",
                2
            )
        );
        assert_eq!(
            found[1],
            row(
                "resource.aws_sqs_queue_policy.p.raw",
                "keep  indent\n",
                "string",
                7
            )
        );
        assert_eq!(found[2].0, "resource.aws_sqs_queue_policy.p.dynamic");
        assert_eq!(found[2].2, "template");
        assert!(
            found[2]
                .1
                .as_deref()
                .unwrap()
                .starts_with("<<EOT\nqueue: ${aws_sqs_queue.orders.arn}")
        );
        assert_eq!(
            found[3],
            row("resource.aws_sqs_queue_policy.p.after", "1", "number", 13)
        );
    }

    #[test]
    fn a_sensitive_variable_keeps_its_default_redacted_and_the_rest_of_its_block() {
        let text = concat!(
            "variable \"db_password\" {\n  type = string\n  sensitive = true\n  default = \"hunter2\"\n}\n",
            "variable \"limits\" {\n  sensitive = true\n  default = { max = 5, names = [\"a\"] }\n}\n",
            "variable \"region\" {\n  sensitive = false\n  default = \"eu-west-1\"\n}\n",
            "variable \"plain\" {\n  default = \"shown\"\n}\n",
        );
        let (outcome, found) = read("variables.tf", text.as_bytes());
        assert_eq!(
            outcome,
            Outcome::Read {
                values: 9,
                redacted: 3,
                sensitive: vec![]
            }
        );
        let hidden: Vec<_> = (found.iter())
            .filter(|r| r.value.is_none())
            .map(|r| (r.key.as_str(), r.kind, r.line))
            .collect();
        assert_eq!(
            hidden,
            [
                ("variable.db_password.default", "string", 4),
                ("variable.limits.default.max", "number", 8),
                ("variable.limits.default.names[0]", "string", 8),
            ]
        );
        let shown = |key: &str| {
            found
                .iter()
                .find(|r| r.key == key)
                .and_then(|r| r.value.clone())
        };
        assert_eq!(
            shown("variable.db_password.sensitive").as_deref(),
            Some("true")
        );
        assert_eq!(
            shown("variable.region.default").as_deref(),
            Some("eu-west-1")
        );
        assert_eq!(shown("variable.plain.default").as_deref(), Some("shown"));
        assert!(!format!("{found:?}").contains("hunter2"));
    }

    #[test]
    fn a_tfvars_file_redacts_every_value_it_gives_a_sensitive_variable() {
        let text = "region = \"eu-west-1\"\ndb_password = \"hunter2\"\nlimits = { max = 5 }\ntags = [\"x\"]\n";
        let sensitive: BTreeSet<String> = ["db_password", "limits"].map(String::from).into();
        for path in ["terraform.tfvars", "prod.auto.tfvars", "envs/prd.tfvars"] {
            let (outcome, found) = read_with(path, text.as_bytes(), &sensitive);
            let hidden: Vec<_> = found
                .iter()
                .filter(|r| r.value.is_none())
                .map(|r| r.key.as_str())
                .collect();
            assert_eq!(hidden, ["db_password", "limits.max"], "{path}");
            assert!(
                matches!(&outcome, Outcome::Read { redacted: 2, sensitive: s, .. } if s.len() == 2),
                "{outcome:?}"
            );
            assert!(!format!("{found:?}").contains("hunter2"));
        }
        // A `.tf` file's top-level attribute of the same name is not a tfvars value.
        let (_, found) = read_with("main.tf", b"db_password = \"x\"\n", &sensitive);
        assert!(found[0].value.is_some());
    }

    #[test]
    fn a_terraform_json_file_reads_as_json_with_its_strings_typed_as_templates() {
        let text = r#"{
  "variable": {
    "token": {"type": "string", "sensitive": true, "default": "s3cret"},
    "name": {"default": "shown"}
  },
  "resource": {"aws_sqs_queue": {"orders": {"name": "${var.name}-orders", "tags": ["${var.a}", "a$${b}", "%{if x}y%{endif}"]}}}
}"#;
        let (outcome, found) = read("main.tf.json", text.as_bytes());
        assert!(
            matches!(
                outcome,
                Outcome::Read {
                    values: 8,
                    redacted: 1,
                    ..
                }
            ),
            "{outcome:?}"
        );
        let by = |key: &str| found.iter().find(|r| r.key == key).unwrap();
        assert_eq!(by("variable.token.default").value, None);
        assert_eq!(by("variable.name.default").value.as_deref(), Some("shown"));
        assert_eq!(by("resource.aws_sqs_queue.orders.name").kind, "template");
        assert_eq!(
            by("resource.aws_sqs_queue.orders.name").value.as_deref(),
            Some("\"${var.name}-orders\"")
        );
        assert_eq!(
            by("resource.aws_sqs_queue.orders.tags[0]").kind,
            "expression"
        );
        assert_eq!(by("resource.aws_sqs_queue.orders.tags[1]").kind, "string");
        assert_eq!(by("resource.aws_sqs_queue.orders.tags[2]").kind, "template");
        // A tfvars.json file is redacted for the sensitive variables like a tfvars file.
        let sensitive: BTreeSet<String> = ["token".to_string()].into();
        let (_, found) = read_with(
            "prod.tfvars.json",
            br#"{"token": "abc", "region": "eu"}"#,
            &sensitive,
        );
        assert_eq!(
            (found[0].key.as_str(), found[0].value.as_deref()),
            ("token", None)
        );
        assert_eq!(found[1].value.as_deref(), Some("eu"));
        // The same name in an ordinary JSON file is no secret.
        let (_, found) = read_with("prod.json", br#"{"token": "abc"}"#, &sensitive);
        assert_eq!(found[0].value.as_deref(), Some("abc"));
    }

    #[test]
    fn a_tfvars_file_read_against_any_variable_redacts_every_value() {
        let sensitive: BTreeSet<String> = [crate::config_values::ANY_VARIABLE.to_string()].into();
        let (outcome, found) = read_with("a.tfvars", b"a = \"x\"\nb = [1]\n", &sensitive);
        assert!(found.iter().all(|r| r.value.is_none()));
        assert!(
            matches!(outcome, Outcome::Read { redacted: 2, .. }),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_file_that_does_not_parse_is_unreadable_at_its_line() {
        let (outcome, found) = read(
            "broken.tf",
            b"resource \"a\" \"b\" {\n  name = \n}\n{{ nope\n",
        );
        assert!(
            matches!(&outcome, Outcome::Unreadable { line, .. } if *line >= 2),
            "{outcome:?}"
        );
        assert!(found.is_empty());
        let (outcome, _) = read("latin.tf", b"a = \"\xe9\"\n");
        assert!(
            matches!(outcome, Outcome::Unreadable { line: 1, .. }),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_value_past_the_value_ceiling_is_stored_cut() {
        let long = "a".repeat(VALUE_MAX + 10);
        let (_, found) = read("a.tf", format!("x = \"{long}\"\ny = {long:?}\n").as_bytes());
        assert_eq!(
            (found[0].value.as_ref().unwrap().len(), found[0].cut),
            (VALUE_MAX, true)
        );
        let (_, found) = read(
            "a.tf",
            format!("x = foo({})\n", "a, ".repeat(VALUE_MAX)).as_bytes(),
        );
        assert_eq!(
            (
                found[0].kind,
                found[0].value.as_ref().unwrap().len(),
                found[0].cut
            ),
            ("expression", VALUE_MAX, true)
        );
    }

    /// Brackets nest at most `NESTING_MAX` deep: one more is unreadable at the file's line, and
    /// the file at the limit reads.
    #[test]
    fn a_file_nested_past_the_ceiling_is_unreadable() {
        let nest = |n: usize| format!("x = {}1{}\n", "[".repeat(n), "]".repeat(n));
        let (outcome, found) = read("a.tf", nest(NESTING_MAX + 1).as_bytes());
        assert!(
            matches!(&outcome, Outcome::Unreadable { line: 1, reason } if reason.contains("nested")),
            "{outcome:?}"
        );
        assert!(found.is_empty());
        let (outcome, found) = read("a.tf", nest(NESTING_MAX).as_bytes());
        assert!(
            matches!(outcome, Outcome::Read { values: 1, .. }),
            "{outcome:?}"
        );
        assert_eq!(found[0].key.len(), 1 + 3 * NESTING_MAX);
    }

    /// `hcl-edit` has two limits of its own: 128 chained operators and 64 levels of expression,
    /// template and block nesting (not counted as brackets). Past either the file is unreadable,
    /// with the parser's reason; at the limit it reads.
    #[test]
    fn a_file_past_the_parsers_own_limits_is_unreadable() {
        let sum = |n: usize| format!("x = a{}\n", " + a".repeat(n));
        let chain = |n: usize| format!("x = {}b\n", "a ? b : ".repeat(n));
        let (outcome, _) = read("a.tf", sum(PARSER_OPERATOR_DEPTH).as_bytes());
        assert!(
            matches!(outcome, Outcome::Read { values: 1, .. }),
            "{outcome:?}"
        );
        let (outcome, found) = read("a.tf", sum(PARSER_OPERATOR_DEPTH + 1).as_bytes());
        assert!(
            matches!(outcome, Outcome::Unreadable { line: 1, .. }),
            "{outcome:?}"
        );
        assert!(found.is_empty());
        let (outcome, _) = read("a.tf", chain(PARSER_NESTING_DEPTH).as_bytes());
        assert!(
            matches!(outcome, Outcome::Read { values: 1, .. }),
            "{outcome:?}"
        );
        let (outcome, _) = read("a.tf", chain(PARSER_NESTING_DEPTH + 1).as_bytes());
        assert!(
            matches!(outcome, Outcome::Unreadable { line: 1, .. }),
            "{outcome:?}"
        );
    }

    /// Long labels nested deep in a heap of blocks repeat in every key path below them: past the
    /// stored budget the file stops.
    #[test]
    fn a_file_whose_key_paths_store_past_the_budget_is_unreadable() {
        let label = "k".repeat(3000);
        let leaves: String = (0..3000).map(|n| format!("a{n} = 1\n")).collect();
        let text = format!("b \"{label}\" {{\n{leaves}}}\n");
        let (outcome, found) = read("a.tf", text.as_bytes());
        assert!(
            matches!(&outcome, Outcome::Unreadable { reason, .. } if reason.contains("past")),
            "{outcome:?}"
        );
        assert!(found.is_empty());
    }
}
