//! Terraform and OpenTofu JSON syntax, adapted into the same blocks and expressions as native
//! HCL. The shared event reader validates and bounds JSON and supplies source locations.

use std::collections::BTreeMap;

use hcl_edit::expr::{Expression, Object, ObjectKey, ObjectValue};
use hcl_edit::template::{Element, Template};
use serde_json::Value;

use super::{
    Block, Kind, Module, Named, ROOT_STORED_MAX, Variable, name_argument, refs_of, subscription,
};
use crate::config_values;
use crate::document::events::{self, Step};
use crate::lang::MAX_BYTES;

/// Work spent constructing expressions from one JSON file, before allocating their ASTs.
// ponytail: HCL's object AST inserts keys linearly; charging the square of a collection's width
// and template separators bounds wide hostile objects as well as ordinary linear input.
const JSON_EXPR_WORK_MAX: usize = 1 << 20;

pub(super) fn is_file(path: &str) -> bool {
    path.ends_with(".tf.json") || path.ends_with(".tofu.json")
}

pub(super) fn read(path: &str, src: &[u8], module: &mut Module) {
    if src.len() > MAX_BYTES {
        return;
    }
    let mut reader = Read {
        module: Module::default(),
        path,
        left: ROOT_STORED_MAX,
        expressions: Expressions {
            work: JSON_EXPR_WORK_MAX,
            bytes: ROOT_STORED_MAX,
        },
    };
    if events::json_spanned(src, |span, value| reader.block(span, value)).is_err() {
        return;
    }
    module.blocks.extend(reader.module.blocks);
    module.variables.extend(reader.module.variables);
    module.locals.extend(reader.module.locals);
    module.outputs.extend(reader.module.outputs);
}

struct Read<'a> {
    module: Module,
    path: &'a str,
    left: usize,
    expressions: Expressions,
}

impl Read<'_> {
    fn store(&mut self, cost: usize) -> bool {
        match self.left.checked_sub(cost) {
            Some(left) => {
                self.left = left;
                true
            }
            None => false,
        }
    }

    /// Bodies are consumed as their collection closes, before a repeated JSON property can
    /// replace an earlier section in the generic value tree. All text stays transient.
    fn block(&mut self, span: events::Collection<'_>, value: &Value) {
        let Some(Step::Key(section)) = span.path.first() else {
            return;
        };
        let labels = match section.as_str() {
            "resource" | "data" => 2,
            "variable" | "output" | "module" => 1,
            "locals" => 0,
            _ => return,
        };
        if body_path(span.path, labels) != Some(span.path) {
            return;
        }
        let Some(body) = value.as_object() else {
            return;
        };
        let names: Vec<&str> = span
            .path
            .iter()
            .filter_map(|step| match step {
                Step::Key(key) => Some(key.as_str()),
                Step::Index(_) => None,
            })
            .skip(1)
            .collect();
        let cost = path_cost(span.path)
            .saturating_add(self.path.len())
            .saturating_add(names.iter().map(|n| n.len()).sum::<usize>())
            .saturating_add(std::mem::size_of::<Block>());
        if !self.store(cost) {
            return;
        }
        let key = config_values::key_path(span.path);
        let (line, end_line) = (span.line, span.end_line);
        let section = section.as_str();
        match section {
            "variable" => {
                self.module.variables.insert(
                    names[0].to_string(),
                    Variable {
                        default: body.get("default").map(|v| self.expressions.read(v, true)),
                        sensitive: body.get("sensitive") == Some(&Value::Bool(true)),
                        path: self.path.into(),
                        line,
                    },
                );
            }
            "locals" => {
                for (name, value) in body.iter().filter(|(name, _)| name.as_str() != "//") {
                    if !self.store(name.len().saturating_add(std::mem::size_of::<Named>())) {
                        continue;
                    }
                    let expr = self.expressions.read(value, false);
                    self.module.locals.insert(name.clone(), named(expr));
                }
            }
            "output" => {
                if let Some(value) = body.get("value") {
                    self.module.outputs.insert(
                        names[0].to_string(),
                        named(self.expressions.read(value, false)),
                    );
                }
            }
            _ => {
                let kind = match section {
                    "resource" => Kind::Resource,
                    "data" => Kind::Data,
                    _ => Kind::Module,
                };
                let ty = if kind == Kind::Module { "" } else { names[0] };
                let all_args = kind == Kind::Module || subscription::reads(ty);
                let named_arg = name_argument(kind, ty);
                let mut attrs = BTreeMap::new();
                let mut refs = Vec::new();
                for (name, value) in body.iter().filter(|(name, _)| name.as_str() != "//") {
                    let expr = if name == "depends_on" {
                        self.expressions.dependencies(value)
                    } else {
                        let literal = name == "provider"
                            || (kind == Kind::Module
                                && matches!(name.as_str(), "source" | "version" | "providers"));
                        self.expressions.read(value, literal)
                    };
                    refs.extend(refs_of(&expr));
                    if all_args
                        || name == named_arg
                        || matches!(name.as_str(), "count" | "for_each")
                    {
                        // Unknown fallbacks still occupy model entries. Charge their names and
                        // containers too, so exhausting AST room cannot retain unlimited ones.
                        let cost = name
                            .len()
                            .saturating_mul(if all_args { 2 } else { 1 })
                            .saturating_add(std::mem::size_of::<Expression>())
                            .saturating_add(if all_args {
                                std::mem::size_of::<Named>()
                            } else {
                                0
                            });
                        if self.store(cost) {
                            attrs.insert(name.clone(), expr);
                        }
                    }
                }
                let args = if all_args {
                    attrs
                        .iter()
                        .map(|(name, expr)| (name.clone(), Named::new(expr)))
                        .collect()
                } else {
                    BTreeMap::new()
                };
                self.module.blocks.push(Block {
                    kind,
                    ty: ty.into(),
                    name: names.last().copied().unwrap_or_default().to_string(),
                    path: self.path.into(),
                    line,
                    end_line,
                    json_path: Some(key),
                    named: (kind != Kind::Module)
                        .then(|| attrs.get(named_arg).cloned())
                        .flatten(),
                    count: attrs.get("count").cloned(),
                    for_each: attrs.get("for_each").cloned(),
                    refs,
                    source: (kind == Kind::Module)
                        .then(|| {
                            body.get("source")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        })
                        .flatten(),
                    args,
                });
            }
        }
    }
}

fn named(expr: Expression) -> Named {
    Named {
        refs: refs_of(&expr),
        expr,
    }
}

/// An upper bound for a rendered path, before allocating its escaped keys.
pub(crate) fn path_cost(path: &[Step]) -> usize {
    path.iter().fold(0usize, |sum, step| {
        sum.saturating_add(match step {
            Step::Key(key) => key.len().saturating_mul(6).saturating_add(4),
            Step::Index(_) => 24,
        })
    })
}

/// The path of a block body at the start of `path`, allowing JSON arrays at each label
/// level and at the body. Its remaining steps, if any, start with an attribute's key.
pub(crate) fn body_path(path: &[Step], labels: usize) -> Option<&[Step]> {
    let mut left = labels + 1; // The section, then its labels.
    let mut end = 0;
    for (index, step) in path.iter().enumerate() {
        match step {
            Step::Key(_) if left > 0 => left -= 1,
            Step::Index(_) if index > 0 && matches!(path[index - 1], Step::Key(_)) => {}
            _ => break,
        }
        end = index + 1;
    }
    (left == 0).then_some(&path[..end])
}

struct Expressions {
    work: usize,
    bytes: usize,
}

impl Expressions {
    fn charge(&mut self, work: usize) -> bool {
        match self.work.checked_sub(work) {
            Some(left) => {
                self.work = left;
                true
            }
            None => false,
        }
    }

    fn read(&mut self, value: &Value, literal: bool) -> Expression {
        // At most three copies survive in the common HCL model (name/count/input plus refs).
        let cost = model_size(value, literal).saturating_mul(3);
        let Some(left) = self.bytes.checked_sub(cost) else {
            return unknown();
        };
        self.bytes = left;
        self.convert(value, literal)
    }

    fn convert(&mut self, value: &Value, literal: bool) -> Expression {
        if !self.charge(1) {
            return unknown();
        }
        match value {
            Value::Null => Expression::null(),
            Value::Bool(b) => (*b).into(),
            Value::Number(n) => n.to_string().parse().unwrap_or_else(|_| unknown()),
            Value::String(s) if literal => s.clone().into(),
            Value::String(s) => self.string(s),
            Value::Array(items) => items
                .iter()
                .map(|v| self.convert(v, literal))
                .collect::<hcl_edit::expr::Array>()
                .into(),
            Value::Object(map) => {
                if !self.charge(map.len().saturating_mul(map.len())) {
                    return unknown();
                }
                let mut object = Object::with_capacity(map.len());
                for (key, value) in map {
                    let key = if literal {
                        key.clone().into()
                    } else {
                        self.string(key)
                    };
                    object.insert(ObjectKey::Expression(key), self.convert(value, literal));
                }
                object.into()
            }
        }
    }

    fn string(&mut self, text: &str) -> Expression {
        let separators = text
            .bytes()
            .filter(|b| matches!(b, b',' | b'=' | b':'))
            .count();
        if !self.charge(
            text.len()
                .saturating_add(separators.saturating_mul(separators)),
        ) {
            return unknown();
        }
        let Ok(template) = text.parse::<Template>() else {
            return unknown();
        };
        if template.len() == 1 {
            match template.iter().next() {
                // JSON's single-interpolation rule preserves collections, booleans and numbers.
                Some(Element::Interpolation(i)) => return i.expr.clone(),
                Some(Element::Literal(s)) => return s.value().clone().into(),
                _ => {}
            }
        }
        Expression::StringTemplate(template.into())
    }

    fn dependencies(&mut self, value: &Value) -> Expression {
        let Some(items) = value.as_array() else {
            return unknown();
        };
        // Traversals are expressions rather than string templates. Even a one-byte invalid
        // element would otherwise allocate one unknown AST node without charging any room.
        let syntax = items.iter().fold(0usize, |sum, value| {
            sum.saturating_add(value.as_str().map_or(0, |text| {
                text.len().saturating_mul(std::mem::size_of::<Expression>())
            }))
        });
        let cost = model_size(value, true)
            .saturating_add(syntax)
            .saturating_mul(3);
        let Some(left) = self.bytes.checked_sub(cost) else {
            return unknown();
        };
        if !self.charge(items.len()) {
            return unknown();
        }
        self.bytes = left;
        items
            .iter()
            .map(|value| {
                let Some(text) = value.as_str() else {
                    return unknown();
                };
                if !self.charge(text.len()) {
                    return unknown();
                }
                match text.parse::<Expression>() {
                    Ok(expr @ Expression::Traversal(_)) => expr,
                    _ => unknown(),
                }
            })
            .collect::<hcl_edit::expr::Array>()
            .into()
    }
}

/// Conservative room for a JSON value's expression nodes and text, before constructing them.
fn model_size(value: &Value, literal: bool) -> usize {
    let nested = match value {
        Value::String(s) => text_size(s, literal),
        Value::Array(items) => items
            .iter()
            .map(|v| model_size(v, literal))
            .fold(0, usize::saturating_add),
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| {
                std::mem::size_of::<(ObjectKey, ObjectValue)>()
                    .saturating_add(text_size(k, literal))
                    .saturating_add(model_size(v, literal))
            })
            .fold(0, usize::saturating_add),
        _ => 0,
    };
    std::mem::size_of::<Expression>().saturating_add(nested)
}

/// A conservative AST estimate without a second template lexer. Plain text stays proportional
/// to its bytes; text after a possible marker charges each byte for an expression and element.
/// Counting an escaped marker or literal braces too is conservative, never an undercharge.
fn text_size(text: &str, literal: bool) -> usize {
    if literal {
        return text.len();
    }
    let start = [text.find("${"), text.find("%{")]
        .into_iter()
        .flatten()
        .min();
    let Some(start) = start else {
        return text.len();
    };
    text.len()
        .saturating_add(text.len().saturating_sub(start).saturating_mul(
            std::mem::size_of::<Expression>().saturating_add(std::mem::size_of::<Element>()),
        ))
}

fn unknown() -> Expression {
    hcl_edit::Ident::new("_meada_json_unknown").into()
}
