//! IaC nodes (#649, ADR 0025): every Terraform and OpenTofu resource, data block and module call
//! of the `.tf` and `.tofu` files a repository holds, with an address, a type, a file, a span and
//! the root module that roots its evaluation, and the static references between them, stored like
//! calls. A Compose project (#652, `compose`) and a CloudFormation or SAM template (#653,
//! `cloudformation`) give nodes of the same shape. `search` finds a node with the kind `iac`,
//! `context` explains it and `trace` walks its references both ways. Never exported.
//!
//! A directory of Terraform files is a module. A module block whose `source` is a local path
//! (`./` or `../`, staying inside the repository) calls the module of that directory; a directory
//! no other one calls is a root module, labelled by its path (`.` for the repository root). Each
//! root is evaluated alone: its blocks are nodes at their own address (`<type>.<name>`,
//! `data.<type>.<name>`, `module.<name>`), and each call of a local module instantiates that
//! module's blocks once, prefixed by the call's address (`module.orders.aws_sqs_queue.this`). A
//! `count` or `for_each` over values that resolve to literals gives one node per instance,
//! suffixed `[key]`.
//!
//! A reference is a traversal in a block's expressions that names a resource, a data block or a
//! module; one through `var.<name>`, `local.<name>` or a module's output (`module.<name>.<out>`) is
//! followed to what the caller's argument, the local or the output refers to, and keeps the name
//! it went through as its `via`.
//!
//! Static evaluation (#650) resolves each node's physical name, its `name` argument, and the
//! `count` and `for_each` of its block with `hcl-rs`'s evaluator and the pure functions of
//! `functions`, from the repository's own values: variable defaults, the values a
//! `terraform.tfvars` or `*.auto.tfvars` file of the root gives, locals, a local module's inputs
//! and outputs per call, `each` and `count`. Nothing else is evaluated (data sources, resource
//! attributes, registry modules, `terraform.workspace`, `-var`, `TF_VAR_`, a sensitive
//! variable): a part of a name built from it is a wildcard `*`, and the name a pattern. A name
//! that went through a root module's variable records it as `Overridable`, by its default or by
//! the var file that gives it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use hcl::eval::{Context, Evaluate};
use hcl::{Map, Value};
use hcl_edit::Span;
use hcl_edit::expr::{Expression, ObjectKey, Traversal, TraversalOperator};
use hcl_edit::parser;
use hcl_edit::template::{Directive, Element, Template};
use hcl_edit::visit::{self, Visit};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::config_values;
use crate::contract::Hit;
use crate::document::events::Step;
use crate::lang::MAX_BYTES;
use crate::render::{rank, section};
use crate::trace::Direction;
use meada_core::export::Level;
use meada_core::trace::{Chain, Row};

pub(crate) mod cloudformation;
mod compose;
mod functions;
mod pattern;
pub(crate) mod provision;
pub(crate) mod serverless;
pub(crate) mod types;

/// The instances one `count` or `for_each` gives; past it the block is one node with no key.
// ponytail: a fleet of a few hundred instances is real; past a thousand the keys come from data.
const EXPANSION_MAX: usize = 1000;

/// How deep local module calls nest; a call deeper keeps its node and instantiates nothing.
// ponytail: real module trees nest three or four levels; 16 stops a chain built to run on.
const MODULE_DEPTH_MAX: usize = 16;

/// How many names (`var`, `local`, outputs) a reference or a value may go through before it is
/// dropped (a value then unresolved).
// ponytail: a name passed down a few modules goes through a handful; 32 stops a chain of locals.
const EVAL_DEPTH_MAX: usize = 32;

/// The bytes one evaluated value may hold (`size`); past it the value is unresolved.
// ponytail: a name, a `for_each` map or a list of regions holds far less; 64 KiB stops a local
// doubled at every step.
const EVAL_VALUE_MAX: usize = 64 << 10;

/// The bytes one expression may make at most (`cost`, an upper bound) for `hcl-rs` to evaluate
/// it; past it the expression is unresolved, but for its template parts.
// ponytail: a `for` over a map of a few hundred entries costs a few hundred KiB; 1 MiB stops a
// `for` inside a `for` over wide lists.
const EVAL_COST_MAX: usize = 1 << 20;

/// The cost (`cost`) the evaluations of one root module may spend together; past it every value
/// left is unresolved.
// ponytail: 64 MiB is thousands of times what a real root spends; it bounds a module instanced
// thousands of times with costly locals.
const EVAL_WORK_MAX: usize = 64 * MAX_BYTES;

/// The nodes one root module's evaluation gives; past it the rest of the root is left out.
// ponytail: a large root holds a few thousand blocks with its modules; 10,000 is past real ones.
const NODES_MAX: usize = 10_000;

/// The references one node keeps, in address order; past it the rest are left out.
// ponytail: a block names a handful of others; 64 is past a wide `depends_on`.
const REFERENCES_MAX: usize = 64;

/// The bytes of addresses one root's nodes and references may store; past it the rest of the
/// root is left out. Instancing multiplies a module by its calls, so a small file can ask for
/// much more than its size.
// ponytail: four times the per-file ceiling holds a root of a few thousand nodes; past it the
// evaluation is built to explode.
const ROOT_STORED_MAX: usize = 4 * MAX_BYTES;

/// The bytes of addresses the nodes and references of the whole repository may store, across
/// its roots; past it the rest is left out.
// ponytail: sixteen times the per-file ceiling holds a monorepo of environments; many roots
// calling one exploding module would pass it.
const STORED_MAX: usize = 16 * MAX_BYTES;

/// Whether the file at `path` holds IaC nodes: a `.tf` or `.tofu` file (HCL), or a Compose file
/// or its override by name (#652).
pub(crate) fn is_source(path: &str) -> bool {
    path.ends_with(".tf") || path.ends_with(".tofu") || compose::is_file(path)
}

/// A text a node records (a Compose service's image or build context, a Serverless function's
/// handler or event) after its variables were expanded: a default (Compose's `${VAR:-default}`,
/// Serverless's `${opt:x, 'default'}`) is in the text, with `overridable` set, and a variable
/// nothing resolves stays in the text as `${VAR}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Resolved {
    pub text: String,
    pub overridable: bool,
}

/// Whether the file at `path` is a var file Terraform loads by itself into its directory's root
/// module: `terraform.tfvars`, `terraform.tfvars.json`, `*.auto.tfvars` or `*.auto.tfvars.json`.
fn is_var_file(path: &str) -> bool {
    var_file_rank(path).is_some()
}

/// Where Terraform loads the var file at `path` among its directory's: `terraform.tfvars`, then
/// `terraform.tfvars.json`, then each `*.auto.tfvars` and `*.auto.tfvars.json` in name order, a
/// later value winning; `None` for a file it does not load by itself.
fn var_file_rank(path: &str) -> Option<(u8, &str)> {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file {
        "terraform.tfvars" => Some((0, file)),
        "terraform.tfvars.json" => Some((1, file)),
        _ if file.ends_with(".auto.tfvars") || file.ends_with(".auto.tfvars.json") => {
            Some((2, file))
        }
        _ => None,
    }
}

/// Whether the file at `path` is read by the evaluation: a source or a var file.
pub(crate) fn is_input(path: &str) -> bool {
    is_source(path) || is_var_file(path)
}

/// One IaC node as stored.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    pub address: String,
    /// The label of the root module its evaluation starts from: its directory, `.` for the root.
    pub root: String,
    /// The resource or data type, or `module` for a module call.
    pub ty: String,
    /// Its physical name: its `name` argument as static evaluation resolves it, each unresolved
    /// part a `*` when it is a pattern.
    pub name: Option<String>,
    /// Whether `name` is a pattern: some part of it is unresolved.
    pub pattern: bool,
    /// The values a deployer can override that `name` went through, in path and line order.
    pub overridable: Vec<Overridable>,
    pub path: String,
    pub line: usize,
    pub end_line: usize,
    /// The key path of its block, which its config values start with.
    pub block: String,
    /// The image a Compose service runs.
    pub image: Option<Resolved>,
    /// The build context of a Compose service: a path inside the repository from its root.
    pub build: Option<Resolved>,
    /// What a Kubernetes object is to Crossplane (#655): `crossplane claim`, `crossplane
    /// composition` and the like; `None` for any other node.
    pub role: Option<String>,
    /// The handler of a Serverless function (#654).
    pub handler: Option<Resolved>,
    /// The events of a Serverless function, in file order.
    pub events: Vec<Resolved>,
}

/// A root module's variable a name went through: by its default, or by the value a var file
/// gives it (ADR 0025), either of which a deployer can override.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct Overridable {
    pub path: String,
    pub line: usize,
    /// `var.<name>`.
    pub variable: String,
    /// Whether a var file gives the value, rather than the variable's default.
    pub tfvars: bool,
}

/// A reference from the node `source` to the node `target` of the root `root`, through `via`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Reference {
    pub root: String,
    pub source: String,
    pub target: String,
    pub via: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Kind {
    Resource,
    Data,
    Module,
}

/// An instance key: `[0]` of a `count`, `["eu"]` of a `for_each`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Key {
    Int(u64),
    Str(String),
}

impl Key {
    fn shown(&self) -> String {
        match self {
            Key::Int(n) => format!("[{n}]"),
            Key::Str(s) => format!("[{}]", serde_json::to_string(s).unwrap_or_default()),
        }
    }
}

/// What a traversal names: a block (with the instance key written after it and, for a module,
/// the output read), or a variable or local whose value is followed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Ref {
    Block {
        kind: Kind,
        ty: String,
        name: String,
        key: Option<Key>,
        output: Option<String>,
    },
    Var(String),
    Local(String),
}

impl Ref {
    /// The name a reference went through, when it is not the block itself.
    fn via(&self) -> Option<String> {
        match self {
            Ref::Block {
                name,
                key,
                output: Some(out),
                ..
            } => {
                let key = key.as_ref().map_or(String::new(), Key::shown);
                Some(format!("module.{name}{key}.{out}"))
            }
            Ref::Block { .. } => None,
            Ref::Var(name) => Some(format!("var.{name}")),
            Ref::Local(name) => Some(format!("local.{name}")),
        }
    }
}

/// A resource, data or module block as read.
struct Block {
    kind: Kind,
    ty: String,
    name: String,
    path: String,
    line: usize,
    end_line: usize,
    /// Its `name` argument, which static evaluation resolves into its physical name; `None` for a
    /// module call, whose `name` is one of its inputs.
    named: Option<Expression>,
    count: Option<Expression>,
    for_each: Option<Expression>,
    refs: Vec<Ref>,
    /// A module call's `source`, and each of its arguments.
    source: Option<String>,
    args: BTreeMap<String, Named>,
}

/// An expression a name holds (a local, an output, a module call's argument), with the
/// references it makes.
struct Named {
    refs: Vec<Ref>,
    expr: Expression,
}

impl Named {
    fn new(expr: &Expression) -> Named {
        Named {
            refs: refs_of(expr),
            expr: expr.clone(),
        }
    }
}

/// A `variable` block: its default, whether it is sensitive, and where it is declared.
struct Variable {
    default: Option<Expression>,
    sensitive: bool,
    path: String,
    line: usize,
}

/// The value a var file gives a variable, and where.
struct Given {
    value: Value,
    path: String,
    line: usize,
    /// Where its file loads among the directory's var files (`var_file_rank`).
    rank: (u8, String),
}

impl Block {
    fn local(&self) -> String {
        match self.kind {
            Kind::Resource => format!("{}.{}", self.ty, self.name),
            Kind::Data => format!("data.{}.{}", self.ty, self.name),
            Kind::Module => format!("module.{}", self.name),
        }
    }

    /// The key path of the block (ADR 0025), which its config values start with.
    fn key_path(&self) -> String {
        let steps: Vec<&str> = match self.kind {
            Kind::Resource => vec!["resource", &self.ty, &self.name],
            Kind::Data => vec!["data", &self.ty, &self.name],
            Kind::Module => vec!["module", &self.name],
        };
        let steps: Vec<Step> = steps.into_iter().map(|s| Step::Key(s.into())).collect();
        config_values::key_path(&steps)
    }
}

/// The blocks, outputs, locals and variables of one directory, and the values its var files
/// give (read only when it is a root).
#[derive(Default)]
struct Module {
    blocks: Vec<Block>,
    outputs: BTreeMap<String, Named>,
    locals: BTreeMap<String, Named>,
    variables: BTreeMap<String, Variable>,
    given: BTreeMap<String, Given>,
}

/// The directory a local module `source` names from `dir`: `./` or `../` joined and normalized,
/// `None` for any other source or one that climbs out of the repository.
fn local_source(dir: &str, source: &str) -> Option<String> {
    if !(source.starts_with("./") || source.starts_with("../")) {
        return None;
    }
    joined(dir, source)
}

/// `rel` joined to the directory `dir` and normalized (`.` and empty steps dropped, `..` popped);
/// `None` for a path that climbs out of the repository or is absolute. The result has no
/// leading or trailing `/`, and is empty for the root.
pub(crate) fn joined(dir: &str, rel: &str) -> Option<String> {
    if rel.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Collects the references of the expressions it visits.
#[derive(Default)]
struct Refs(Vec<Ref>);

impl Visit for Refs {
    fn visit_traversal(&mut self, node: &Traversal) {
        if let Expression::Variable(root) = &node.expr {
            let ops: Vec<&TraversalOperator> = node.operators.iter().map(|o| o.value()).collect();
            if let Some(r) = reference(root.as_str(), &ops) {
                self.0.push(r);
            }
        }
        visit::visit_traversal(self, node);
    }
}

fn refs_of(expr: &Expression) -> Vec<Ref> {
    let mut refs = Refs::default();
    refs.visit_expr(expr);
    refs.0
}

/// The reference a traversal from the variable `root` through `ops` makes, if any.
fn reference(root: &str, ops: &[&TraversalOperator]) -> Option<Ref> {
    let attr = |at: usize| match ops.get(at) {
        Some(TraversalOperator::GetAttr(i)) => Some(i.as_str().to_string()),
        _ => None,
    };
    // An index or a splat right after the block's name: `Some(key)` for a literal index.
    let keyed = |at: usize| -> (Option<Key>, usize) {
        match ops.get(at) {
            Some(TraversalOperator::Index(Expression::Number(n))) => {
                (n.value().as_u64().map(Key::Int), at + 1)
            }
            Some(TraversalOperator::Index(Expression::String(s))) => {
                (Some(Key::Str(s.value().clone())), at + 1)
            }
            Some(TraversalOperator::LegacyIndex(n)) => (Some(Key::Int(*n.value())), at + 1),
            Some(
                TraversalOperator::Index(_)
                | TraversalOperator::AttrSplat(_)
                | TraversalOperator::FullSplat(_),
            ) => (None, at + 1),
            _ => (None, at),
        }
    };
    let block = |kind, ty: String, name: String, at: usize| {
        let (key, next) = keyed(at);
        let output = (kind == Kind::Module).then(|| attr(next)).flatten();
        Ref::Block {
            kind,
            ty,
            name,
            key,
            output,
        }
    };
    match root {
        "var" => Some(Ref::Var(attr(0)?)),
        "local" => Some(Ref::Local(attr(0)?)),
        "each" | "count" | "path" | "terraform" | "self" => None,
        "module" => Some(block(Kind::Module, String::new(), attr(0)?, 1)),
        "data" => Some(block(Kind::Data, attr(0)?, attr(1)?, 2)),
        ty => Some(block(Kind::Resource, ty.to_string(), attr(0)?, 1)),
    }
}

/// The line of each byte offset of `text`, 1-based.
fn lines(text: &str) -> impl Fn(usize) -> usize {
    let newlines: Vec<usize> = (text.bytes().enumerate())
        .filter(|&(_, b)| b == b'\n')
        .map(|(i, _)| i)
        .collect();
    move |offset| newlines.partition_point(|&n| n < offset) + 1
}

/// The text of `src` as HCL, `None` when it is not UTF-8 or nests past the HCL reader's ceiling
/// (config values count it unreadable).
fn text(src: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(src).ok()?;
    config_values::nested_past(text).is_none().then_some(text)
}

/// Reads the values the var file at `path` gives into `module`, a value of a file Terraform loads
/// later (`var_file_rank`) winning, whatever order the files come in. A value that is no constant
/// (Terraform refuses it) is left out; a JSON var file's strings are literals, as Terraform reads
/// them, each at the line of its key's first occurrence.
fn read_var_file(path: &str, src: &[u8], module: &mut Module) {
    let (Some(rank), Some(text)) = (var_file_rank(path), text(src)) else {
        return;
    };
    let line = lines(text);
    let mut values: Vec<(String, Value, usize)> = Vec::new();
    if path.ends_with(".json") {
        let Ok(map) = serde_json::from_str::<Map<String, Value>>(text) else {
            return;
        };
        for (name, value) in map {
            let key = serde_json::to_string(&name).unwrap_or_default();
            let at = line(text.find(&key).unwrap_or(0));
            values.push((name, value, at));
        }
    } else {
        let Ok(body) = parser::parse_body(text) else {
            return;
        };
        for a in body.attributes() {
            if let Ok(value) = hcl::Expression::from(a.value.clone()).evaluate(&Context::new()) {
                let at = line(a.span().map_or(0, |s| s.start));
                values.push((a.key.as_str().to_string(), value, at));
            }
        }
    }
    let rank = (rank.0, rank.1.to_string());
    for (name, value, line) in values {
        if module.given.get(&name).is_none_or(|g| g.rank <= rank) {
            let path = path.to_string();
            let rank = rank.clone();
            module.given.insert(
                name,
                Given {
                    value,
                    path,
                    line,
                    rank,
                },
            );
        }
    }
}

/// Reads the blocks, outputs, locals and variables of the Terraform file at `path` into `module`;
/// a file that is not UTF-8, nests past the HCL reader's ceiling or does not parse adds nothing
/// (config values count it unreadable).
fn read(path: &str, src: &[u8], module: &mut Module) {
    if is_var_file(path) {
        return read_var_file(path, src, module);
    }
    let Some(text) = text(src) else {
        return;
    };
    let Ok(body) = parser::parse_body(text) else {
        return;
    };
    let line = lines(text);
    for b in body.blocks() {
        let labels: Vec<&str> = b.labels.iter().map(|l| l.as_str()).collect();
        let attr = |name: &str| b.body.get_attribute(name).map(|a| &a.value);
        let (kind, ty, name) = match (b.ident.as_str(), &labels[..]) {
            ("resource", [ty, name]) => (Kind::Resource, *ty, *name),
            ("data", [ty, name]) => (Kind::Data, *ty, *name),
            ("module", [name]) => (Kind::Module, "", *name),
            ("output", [name]) => {
                if let Some(value) = attr("value") {
                    module.outputs.insert(name.to_string(), Named::new(value));
                }
                continue;
            }
            ("locals", []) => {
                for a in b.body.attributes() {
                    module
                        .locals
                        .insert(a.key.as_str().to_string(), Named::new(&a.value));
                }
                continue;
            }
            ("variable", [name]) => {
                let variable = Variable {
                    default: attr("default").cloned(),
                    sensitive: matches!(attr("sensitive"), Some(Expression::Bool(b)) if *b.value()),
                    path: path.to_string(),
                    line: line(b.span().map_or(0, |s| s.start)),
                };
                module.variables.insert(name.to_string(), variable);
                continue;
            }
            _ => continue,
        };
        let span = b.span().unwrap_or(0..0);
        let mut refs = Refs::default();
        refs.visit_body(&b.body);
        let mut args = BTreeMap::new();
        if kind == Kind::Module {
            for a in b.body.attributes() {
                args.insert(a.key.as_str().to_string(), Named::new(&a.value));
            }
        }
        let source = match attr("source") {
            Some(Expression::String(s)) if kind == Kind::Module => Some(s.value().clone()),
            _ => None,
        };
        module.blocks.push(Block {
            kind,
            ty: ty.to_string(),
            name: name.to_string(),
            path: path.to_string(),
            line: line(span.start),
            end_line: line(span.end.saturating_sub(1).max(span.start)),
            // A module call's `name` is one of its inputs, not the name of something it creates.
            // A resource the type table lists (#656) is named by the argument the table names.
            named: (kind != Kind::Module)
                .then(|| attr(name_argument(kind, ty)).cloned())
                .flatten(),
            count: attr("count").cloned(),
            for_each: attr("for_each").cloned(),
            refs: refs.0,
            source,
            args,
        });
    }
}

/// The argument that holds the physical name of a block of `kind` and `ty`: the one the type
/// table names for a resource it lists (`bucket` for `aws_s3_bucket`), else `name`.
fn name_argument(kind: Kind, ty: &str) -> &'static str {
    match kind {
        Kind::Resource => types::entry("terraform", ty).map_or("name", |e| e.name.as_str()),
        _ => "name",
    }
}

/// A block by its kind, type and name, as a reference names it.
type BlockName<'m> = (Kind, &'m str, &'m str);

/// A value as static evaluation finds it: known, a string with unresolved parts (each `*`), or
/// unresolved, with the values a deployer can override that it went through.
#[derive(Debug, Clone, PartialEq)]
struct Evaluated {
    val: Val,
    through: BTreeSet<Overridable>,
}

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Known(Value),
    Pattern(String),
    Unknown,
}

impl Evaluated {
    fn unknown() -> Evaluated {
        Evaluated {
            val: Val::Unknown,
            through: BTreeSet::new(),
        }
    }

    /// The text a template shows for it, and whether some of it is unresolved: a scalar as
    /// written, a pattern's text, `*` for anything else.
    fn text(&self) -> (String, bool) {
        match &self.val {
            Val::Known(Value::String(s)) => (s.clone(), false),
            Val::Known(Value::Number(n)) => (n.to_string(), false),
            Val::Known(Value::Bool(b)) => (b.to_string(), false),
            Val::Pattern(p) => (p.clone(), true),
            _ => ("*".into(), true),
        }
    }
}

/// The bytes `v` holds as JSON, about: what the value ceilings count.
fn size(v: &Value) -> usize {
    match v {
        Value::Null | Value::Bool(_) | Value::Number(_) => 8,
        Value::String(s) => s.len() + 2,
        Value::Array(items) => (items.iter().map(size)).fold(2, usize::saturating_add),
        Value::Object(map) => {
            (map.iter().map(|(k, v)| k.len() + 3 + size(v))).fold(2, usize::saturating_add)
        }
    }
}

/// The value a traversal reaches inside the bound values `roots` through attributes and literal
/// indices only, when it reaches one.
fn bound<'v>(t: &Traversal, roots: &'v Map<String, Value>) -> Option<&'v Value> {
    let Expression::Variable(root) = &t.expr else {
        return None;
    };
    let mut v = roots.get(root.as_str())?;
    for op in t.operators.iter().map(|o| o.value()) {
        v = match (op, v) {
            (TraversalOperator::GetAttr(a), Value::Object(m)) => m.get(a.as_str())?,
            (TraversalOperator::Index(Expression::String(k)), Value::Object(m)) => {
                m.get(k.value().as_str())?
            }
            (TraversalOperator::Index(Expression::Number(n)), Value::Array(l)) => {
                l.get(n.value().as_u64()? as usize)?
            }
            (TraversalOperator::LegacyIndex(n), Value::Array(l)) => l.get(*n.value() as usize)?,
            _ => return None,
        };
    }
    Some(v)
}

/// An upper bound of the bytes evaluating `expr` makes, its variables bound to `roots` and the
/// iterator variables of the `for` around it to `iters` (a name and the most one element holds).
/// A product (a `for`, `formatlist`, `join`, `replace`) multiplies, a function that may grow its
/// input (a `format` width, `jsonencode`'s escapes, `split`'s quotes, a case change) grows it, and
/// anything else adds up.
fn cost(expr: &Expression, roots: &Map<String, Value>, iters: &mut Vec<(String, usize)>) -> usize {
    let add = |a: usize, b: usize| a.saturating_add(b);
    match expr {
        Expression::Null(_) | Expression::Bool(_) | Expression::Number(_) => 8,
        Expression::String(s) => s.len() + 2,
        Expression::Array(items) => (items.iter()).map(|e| cost(e, roots, iters)).fold(2, add),
        Expression::Object(object) => (object.iter())
            .map(|(k, v)| {
                let key = match k {
                    ObjectKey::Ident(i) => i.as_str().len(),
                    ObjectKey::Expression(e) => cost(e, roots, iters),
                };
                add(key, cost(v.expr(), roots, iters))
            })
            .fold(2, add),
        Expression::StringTemplate(t) => template_cost(t, roots, iters),
        Expression::HeredocTemplate(h) => template_cost(&h.template, roots, iters),
        Expression::Parenthesis(p) => cost(p.inner(), roots, iters),
        Expression::Variable(v) => match iters.iter().rev().find(|(n, _)| n == v.as_str()) {
            Some((_, n)) => *n,
            None => roots.get(v.as_str()).map_or(8, size),
        },
        Expression::Traversal(t) => {
            let indices = (t.operators.iter()).filter_map(|o| match o.value() {
                TraversalOperator::Index(e) => Some(cost(e, roots, iters)),
                _ => None,
            });
            let indices = indices.collect::<Vec<_>>().into_iter().fold(0, add);
            let base = match bound(t, roots) {
                Some(v) if iters.is_empty() => size(v),
                _ => cost(&t.expr, roots, iters),
            };
            add(base, indices)
        }
        Expression::Conditional(c) => [&c.cond_expr, &c.true_expr, &c.false_expr]
            .into_iter()
            .map(|e| cost(e, roots, iters))
            .fold(0, add),
        Expression::FuncCall(f) => {
            let args: Vec<usize> = f.args.iter().map(|e| cost(e, roots, iters)).collect();
            let sum = args.iter().copied().fold(8, add);
            match f.name.name.as_str() {
                "formatlist" | "join" | "replace" => sum.saturating_mul(sum),
                "format" => add(sum, args.len().saturating_mul(functions::WIDTH_MAX)),
                "jsonencode" => sum.saturating_mul(6),
                "split" => sum.saturating_mul(4),
                "upper" | "lower" | "title" => sum.saturating_mul(3),
                _ => sum,
            }
        }
        Expression::UnaryOp(u) => add(cost(&u.expr, roots, iters), 8),
        Expression::BinaryOp(b) => add(
            cost(&b.lhs_expr, roots, iters),
            cost(&b.rhs_expr, roots, iters),
        ),
        Expression::ForExpr(f) => {
            let intro = &f.intro;
            let (n, each) = elements(&intro.collection_expr, roots, iters);
            let vars = [intro.key_var.as_ref(), Some(&intro.value_var)];
            let depth = iters.len();
            iters.extend(
                vars.into_iter()
                    .flatten()
                    .map(|v| (v.as_str().to_string(), each)),
            );
            let body = [f.key_expr.as_ref(), Some(&f.value_expr)]
                .into_iter()
                .chain([f.cond.as_ref().map(|c| &c.expr)])
                .flatten()
                .map(|e| cost(e, roots, iters))
                .fold(4, add);
            iters.truncate(depth);
            n.saturating_mul(body)
        }
    }
}

fn template_cost(
    t: &Template,
    roots: &Map<String, Value>,
    iters: &mut Vec<(String, usize)>,
) -> usize {
    let mut total = 0usize;
    for e in t.iter() {
        let part = match e {
            Element::Literal(s) => s.value().len(),
            Element::Interpolation(i) => cost(&i.expr, roots, iters),
            Element::Directive(d) => match d.as_ref() {
                Directive::If(d) => {
                    let branches = template_cost(&d.if_expr.template, roots, iters).saturating_add(
                        (d.else_expr.as_ref())
                            .map_or(0, |e| template_cost(&e.template, roots, iters)),
                    );
                    cost(&d.if_expr.cond_expr, roots, iters).saturating_add(branches)
                }
                Directive::For(d) => {
                    let f = &d.for_expr;
                    let (n, each) = elements(&f.collection_expr, roots, iters);
                    let depth = iters.len();
                    let vars = [f.key_var.as_ref(), Some(&f.value_var)];
                    iters.extend(
                        vars.into_iter()
                            .flatten()
                            .map(|v| (v.as_str().to_string(), each)),
                    );
                    let body = template_cost(&f.template, roots, iters);
                    iters.truncate(depth);
                    n.saturating_mul(body)
                }
            },
        };
        total = total.saturating_add(part);
    }
    total
}

/// How many elements the collection `expr` gives at most, and the bytes one of them (or its key)
/// holds at most: exact for a bound value, else its cost for both.
fn elements(
    expr: &Expression,
    roots: &Map<String, Value>,
    iters: &mut Vec<(String, usize)>,
) -> (usize, usize) {
    let found = match expr {
        Expression::Traversal(t) if iters.is_empty() => bound(t, roots),
        Expression::Variable(v) if iters.is_empty() => roots.get(v.as_str()),
        _ => None,
    };
    let most = |sizes: &mut dyn Iterator<Item = usize>| sizes.max().unwrap_or(0);
    match found {
        Some(Value::Array(items)) => (items.len(), most(&mut items.iter().map(size))),
        Some(Value::Object(map)) => (
            map.len(),
            most(&mut map.iter().map(|(k, v)| (k.len() + 2).max(size(v)))),
        ),
        Some(v) => (1, size(v)),
        None => {
            let c = cost(expr, roots, iters);
            (c, c)
        }
    }
}

/// Collects the variables an expression names: `each` and `count` say whether it reads the
/// instance it is evaluated in.
#[derive(Default)]
struct Variables(BTreeSet<String>);

impl Visit for Variables {
    fn visit_expr(&mut self, node: &Expression) {
        if let Expression::Variable(v) = node {
            self.0.insert(v.as_str().to_string());
        }
        visit::visit_expr(self, node);
    }
}

/// What `each` and `count` hold in one instance of a block or a module call: its key and
/// `each.value`, with what the `count` or `for_each` went through.
#[derive(Debug, Clone)]
struct Each {
    key: Key,
    value: Value,
    through: BTreeSet<Overridable>,
}

/// A name a value is memoized under, in one instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Name {
    Var(String),
    Local(String),
    Output(String),
}

/// One instance of a module during a root's evaluation.
struct Instance<'m> {
    dir: &'m str,
    /// The instance that calls it, with the call's block.
    parent: Option<(usize, &'m Block)>,
    /// The key and `each.value` of the call's instance that made it, when the call has keys.
    call: Option<Each>,
    /// Each block's nodes, by its kind, type and name: the instance key and the address.
    nodes: HashMap<BlockName<'m>, Vec<(Option<Key>, String)>>,
    /// The instances of each local module it calls, by the call's name, with their keys.
    children: HashMap<&'m str, Vec<(Option<Key>, usize)>>,
}

/// The evaluation of a repository's Terraform files, root by root.
struct Eval<'m> {
    modules: &'m BTreeMap<String, Module>,
    instances: Vec<Instance<'m>>,
    /// The targets each reference resolves to in an instance, once computed.
    memo: HashMap<(usize, &'m Ref), Vec<String>>,
    /// The references being resolved: one met again closes a cycle and resolves to nothing.
    open: HashSet<(usize, &'m Ref)>,
    /// `hcl-rs`'s evaluation context, its functions declared once and its variables (`var`,
    /// `local`, `module`, `each`, `count`) declared again for each expression.
    ctx: Context<'static>,
    /// The value of each name of an instance, once evaluated.
    values: HashMap<(usize, Name), Evaluated>,
    /// The names being evaluated: one met again closes a cycle and is unresolved.
    evaluating: HashSet<(usize, Name)>,
    /// Whether module outputs may be read: not while the instances are made, since a module
    /// called later has none yet.
    outputs: bool,
    /// The cost the root's evaluations may still spend.
    work: usize,
    /// The nodes whose physical name waits for every instance: the node, its instance, its block
    /// and its instance's `each`.
    pending: Vec<(usize, usize, &'m Block, Option<Each>)>,
    root: String,
    root_nodes: usize,
    root_left: usize,
    left: usize,
    nodes: Vec<Node>,
    references: Vec<Reference>,
}

impl<'m> Eval<'m> {
    fn new(modules: &'m BTreeMap<String, Module>, left: usize) -> Eval<'m> {
        let mut ctx = Context::new();
        functions::declare(&mut ctx);
        Eval {
            modules,
            instances: Vec::new(),
            memo: HashMap::new(),
            open: HashSet::new(),
            ctx,
            values: HashMap::new(),
            evaluating: HashSet::new(),
            outputs: false,
            work: 0,
            pending: Vec::new(),
            root: String::new(),
            root_nodes: 0,
            root_left: 0,
            left,
            nodes: Vec::new(),
            references: Vec::new(),
        }
    }

    /// Whether `bytes` more fit both budgets, taking them when they do.
    fn charge(&mut self, bytes: usize) -> bool {
        match (
            self.root_left.checked_sub(bytes),
            self.left.checked_sub(bytes),
        ) {
            (Some(root), Some(all)) => {
                (self.root_left, self.left) = (root, all);
                true
            }
            _ => {
                self.root_left = 0;
                false
            }
        }
    }

    /// The nodes and references of the root module at `dir`.
    fn root(&mut self, dir: &'m str) {
        self.instances.clear();
        self.memo.clear();
        self.open.clear();
        self.values.clear();
        self.evaluating.clear();
        self.pending.clear();
        self.outputs = false;
        self.work = EVAL_WORK_MAX;
        self.root = label(dir).to_string();
        self.root_nodes = 0;
        self.root_left = ROOT_STORED_MAX;
        self.instantiate(dir, "", None, None, &mut vec![dir]);
        // Every instance made, module outputs are known: values are evaluated again with them.
        self.values.clear();
        self.evaluating.clear();
        self.outputs = true;
        for (node, instance, block, each) in std::mem::take(&mut self.pending) {
            let Some(named) = &block.named else { continue };
            let found = self.value(instance, each.as_ref(), named, 0);
            // A null or a collection is no name; anything else is its text, `*` when unresolved.
            let (name, pattern) = match &found.val {
                Val::Known(Value::Null | Value::Array(_) | Value::Object(_)) => (None, false),
                _ => {
                    let (text, pattern) = found.text();
                    // An ARN or a URL in a type the table lists holds the name (#656).
                    let entry = (block.kind == Kind::Resource)
                        .then(|| types::entry("terraform", &block.ty))
                        .flatten();
                    let text = match entry {
                        Some(e) if !pattern => e.read(&text).to_string(),
                        _ => text,
                    };
                    (Some(text), pattern)
                }
            };
            if !self.charge(name.as_ref().map_or(0, String::len)) {
                return;
            }
            let node = &mut self.nodes[node];
            (node.name, node.pattern) = (name, pattern);
            node.overridable = found.through.into_iter().collect();
        }
        let modules = self.modules;
        for i in 0..self.instances.len() {
            let instance = &self.instances[i];
            let mut sources: Vec<(&'m Block, String)> = Vec::new();
            for block in &modules[instance.dir].blocks {
                let key = (block.kind, block.ty.as_str(), block.name.as_str());
                for (_, address) in instance.nodes.get(&key).into_iter().flatten() {
                    sources.push((block, address.clone()));
                }
            }
            for (block, source) in sources {
                let mut targets: BTreeSet<(String, Option<String>)> = BTreeSet::new();
                for r in &block.refs {
                    for target in self.resolve(i, r, 0) {
                        if target != source {
                            targets.insert((target, r.via()));
                        }
                    }
                }
                for (target, via) in targets.into_iter().take(REFERENCES_MAX) {
                    let size = source.len() + target.len() + via.as_ref().map_or(0, String::len);
                    if !self.charge(size) {
                        return;
                    }
                    self.references.push(Reference {
                        root: self.root.clone(),
                        source: source.clone(),
                        target,
                        via,
                    });
                }
            }
        }
    }

    /// Adds an instance of the module at `dir` under `prefix` (the call's address and a dot),
    /// made by the call's instance `call`, its nodes and those of the local modules it calls,
    /// `stack` holding the directories of the calls above it; its position.
    fn instantiate(
        &mut self,
        dir: &'m str,
        prefix: &str,
        parent: Option<(usize, &'m Block)>,
        call: Option<Each>,
        stack: &mut Vec<&'m str>,
    ) -> usize {
        let id = self.instances.len();
        self.instances.push(Instance {
            dir,
            parent,
            call,
            nodes: HashMap::new(),
            children: HashMap::new(),
        });
        let modules = self.modules;
        for block in &modules[dir].blocks {
            let keys: Vec<Option<Each>> = match self.keys(id, block) {
                Some(keys) => keys.into_iter().map(Some).collect(),
                None => vec![None],
            };
            for each in keys {
                let key = each.as_ref().map(|e| e.key.clone());
                let shown = key.as_ref().map_or(String::new(), Key::shown);
                let address = format!("{prefix}{}{shown}", block.local());
                if self.root_nodes >= NODES_MAX || !self.charge(address.len()) {
                    return id;
                }
                self.root_nodes += 1;
                self.pending
                    .push((self.nodes.len(), id, block, each.clone()));
                self.nodes.push(Node {
                    address: address.clone(),
                    root: self.root.clone(),
                    ty: match block.kind {
                        Kind::Module => "module".into(),
                        _ => block.ty.clone(),
                    },
                    name: None,
                    pattern: false,
                    overridable: Vec::new(),
                    path: block.path.clone(),
                    line: block.line,
                    end_line: block.end_line,
                    block: block.key_path(),
                    image: None,
                    build: None,
                    role: None,
                    handler: None,
                    events: Vec::new(),
                });
                let local = (block.source.as_deref())
                    .and_then(|s| local_source(dir, s))
                    .and_then(|t| modules.get_key_value(&t))
                    .map(|(t, _)| t.as_str());
                let callee =
                    local.filter(|t| stack.len() <= MODULE_DEPTH_MAX && !stack.contains(t));
                let entry = (block.kind, block.ty.as_str(), block.name.as_str());
                self.instances[id]
                    .nodes
                    .entry(entry)
                    .or_default()
                    .push((key.clone(), address.clone()));
                if let Some(target) = callee {
                    stack.push(target);
                    let prefix = format!("{address}.");
                    let child = self.instantiate(target, &prefix, Some((id, block)), each, stack);
                    stack.pop();
                    let calls = self.instances[id].children.entry(&block.name);
                    calls.or_default().push((key, child));
                }
            }
        }
        id
    }

    /// The instances the `count` or `for_each` of `block` gives in the instance `at`, sorted
    /// by key as Terraform sorts them; `None` for a block with neither, or with one that does
    /// not resolve to a number, an object or a list of strings, or past `EXPANSION_MAX`.
    fn keys(&mut self, at: usize, block: &'m Block) -> Option<Vec<Each>> {
        if let Some(count) = &block.count {
            let found = self.value(at, None, count, 0);
            let Val::Known(Value::Number(n)) = found.val else {
                return None;
            };
            let n = n.as_u64().filter(|&n| n as usize <= EXPANSION_MAX)?;
            let each = |i| Each {
                key: Key::Int(i),
                value: Value::Null,
                through: found.through.clone(),
            };
            return Some((0..n).map(each).collect());
        }
        let found = self.value(at, None, block.for_each.as_ref()?, 0);
        let entries: BTreeMap<String, Value> = match found.val {
            Val::Known(Value::Object(map)) => map.into_iter().collect(),
            Val::Known(Value::Array(items)) => (items.into_iter())
                .map(|v| match v {
                    Value::String(s) => Some((s.clone(), Value::String(s))),
                    _ => None,
                })
                .collect::<Option<_>>()?,
            _ => return None,
        };
        if entries.len() > EXPANSION_MAX {
            return None;
        }
        let each = |(key, value)| Each {
            key: Key::Str(key),
            value,
            through: found.through.clone(),
        };
        Some(entries.into_iter().map(each).collect())
    }

    /// The value of the name `name` in the instance `at`, memoized; `depth` names already gone
    /// through. A name met again inside its own evaluation (a cycle) is unresolved.
    fn name(&mut self, at: usize, name: Name, depth: usize) -> Evaluated {
        let key = (at, name);
        if let Some(found) = self.values.get(&key) {
            return found.clone();
        }
        if depth > EVAL_DEPTH_MAX || !self.evaluating.insert(key.clone()) {
            return Evaluated::unknown();
        }
        let module = &self.modules[self.instances[at].dir];
        let found = match &key.1 {
            Name::Local(n) => match module.locals.get(n) {
                Some(local) => self.value(at, None, &local.expr, depth + 1),
                None => Evaluated::unknown(),
            },
            Name::Output(n) => match module.outputs.get(n) {
                Some(output) => self.value(at, None, &output.expr, depth + 1),
                None => Evaluated::unknown(),
            },
            Name::Var(n) => self.var(at, n, depth),
        };
        self.evaluating.remove(&key);
        self.values.insert(key, found.clone());
        found
    }

    /// The value of `var.<n>` in the instance `at`: in a module called locally, the call's
    /// argument evaluated in the caller (else the variable's default, which the repository
    /// fixes); in a root, the value its var files give or else its default, either of which a
    /// deployer can override. A sensitive or undeclared variable is unresolved.
    fn var(&mut self, at: usize, n: &str, depth: usize) -> Evaluated {
        let module = &self.modules[self.instances[at].dir];
        let Some(variable) = module.variables.get(n).filter(|v| !v.sensitive) else {
            return Evaluated::unknown();
        };
        if let Some((caller, block)) = self.instances[at].parent {
            let call = self.instances[at].call.clone();
            return match (block.args.get(n), &variable.default) {
                (Some(arg), _) => self.value(caller, call.as_ref(), &arg.expr, depth + 1),
                (None, Some(default)) => self.value(at, None, default, depth + 1),
                (None, None) => Evaluated::unknown(),
            };
        }
        let (mut found, path, line, tfvars) = match (module.given.get(n), &variable.default) {
            (Some(given), _) => {
                let found = Evaluated {
                    val: Val::Known(given.value.clone()),
                    through: BTreeSet::new(),
                };
                (found, &given.path, given.line, true)
            }
            (None, Some(default)) => {
                let found = self.value(at, None, default, depth + 1);
                (found, &variable.path, variable.line, false)
            }
            (None, None) => return Evaluated::unknown(),
        };
        found.through.insert(Overridable {
            path: path.clone(),
            line,
            variable: format!("var.{n}"),
            tfvars,
        });
        found
    }

    /// The value of `module.<name>` in the instance `at`, holding the outputs `outs` of each of
    /// its instances that resolve: an object for a call with no keys, a list for a `count`, an
    /// object by key for a `for_each`. Only the instances `picked` (by a literal key) add what
    /// they went through.
    fn module(
        &mut self,
        at: usize,
        name: &str,
        outs: &BTreeSet<String>,
        picked: &[Option<Key>],
        depth: usize,
    ) -> Option<(Value, BTreeSet<Overridable>)> {
        let children = self.instances[at].children.get(name)?.clone();
        let mut through = BTreeSet::new();
        let mut objects = Vec::new();
        for (key, child) in &children {
            let named = |p: &Option<Key>| p.is_none() || !children.iter().any(|(k, _)| k == p);
            let counted = picked.iter().any(|p| named(p) || p == key);
            let mut object = Map::new();
            for out in outs {
                let found = self.name(*child, Name::Output(out.clone()), depth + 1);
                if counted {
                    through.extend(found.through);
                }
                if let Val::Known(v) = found.val {
                    object.insert(out.clone(), v);
                }
            }
            objects.push((key.clone(), Value::Object(object)));
        }
        let value = match &objects[..] {
            [(None, object)] => object.clone(),
            [(Some(Key::Str(_)), _), ..] => Value::Object(
                (objects.into_iter())
                    .filter_map(|(k, v)| match k {
                        Some(Key::Str(k)) => Some((k, v)),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => Value::Array(objects.into_iter().map(|(_, v)| v).collect()),
        };
        Some((value, through))
    }

    /// The value of `expr` in the instance `at`, inside the block instance `each` when it has one;
    /// `depth` names already gone through. `hcl-rs` evaluates it with every name it reads bound
    /// to its value; when that fails (a part is unresolved), a template keeps its literal text
    /// and the parts that resolve, each other part a `*`, and `var.<x>`, `local.<x>` or a module
    /// output alone is that name's value.
    fn value(
        &mut self,
        at: usize,
        each: Option<&Each>,
        expr: &'m Expression,
        depth: usize,
    ) -> Evaluated {
        if depth > EVAL_DEPTH_MAX || self.work == 0 {
            return Evaluated::unknown();
        }
        let mut through = BTreeSet::new();
        let (mut vars, mut locals, mut mods) = (Map::new(), Map::new(), Map::new());
        let mut outputs: BTreeMap<String, (BTreeSet<String>, Vec<Option<Key>>)> = BTreeMap::new();
        for r in refs_of(expr) {
            let (map, name, found) = match r {
                Ref::Var(n) => (&mut vars, n.clone(), self.name(at, Name::Var(n), depth)),
                Ref::Local(n) => (&mut locals, n.clone(), self.name(at, Name::Local(n), depth)),
                Ref::Block {
                    kind: Kind::Module,
                    name,
                    key,
                    output: Some(out),
                    ..
                } if self.outputs => {
                    let (outs, keys) = outputs.entry(name).or_default();
                    outs.insert(out);
                    keys.push(key);
                    continue;
                }
                _ => continue,
            };
            through.extend(found.through);
            if let Val::Known(v) = found.val {
                map.insert(name, v);
            }
        }
        for (name, (outs, picked)) in outputs {
            if let Some((v, t)) = self.module(at, &name, &outs, &picked, depth) {
                through.extend(t);
                mods.insert(name, v);
            }
        }
        let mut variables = Variables::default();
        variables.visit_expr(expr);
        let (mut each_v, mut count_v) = (Map::new(), Map::new());
        if let Some(e) = each.filter(|_| ["each", "count"].iter().any(|v| variables.0.contains(*v)))
        {
            through.extend(e.through.iter().cloned());
            each_v.insert("value".to_string(), e.value.clone());
            match &e.key {
                Key::Int(i) => {
                    count_v.insert("index".to_string(), Value::from(*i));
                    each_v.insert("key".to_string(), Value::from(*i));
                }
                Key::Str(s) => {
                    each_v.insert("key".to_string(), Value::from(s.as_str()));
                }
            }
        }
        let roots: Map<String, Value> = [
            ("var", vars),
            ("local", locals),
            ("module", mods),
            ("each", each_v),
            ("count", count_v),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), Value::Object(v)))
        .collect();
        let cost = cost(expr, &roots, &mut Vec::new()).max(1);
        if cost <= EVAL_COST_MAX && cost <= self.work {
            self.work -= cost;
            for (name, v) in roots {
                self.ctx.declare_var(name, v);
            }
            match hcl::Expression::from(expr.clone()).evaluate(&self.ctx) {
                Ok(v) if size(&v) <= EVAL_VALUE_MAX => {
                    return Evaluated {
                        val: Val::Known(v),
                        through,
                    };
                }
                _ => {}
            }
        } else {
            self.work = self.work.saturating_sub(EVAL_COST_MAX.min(cost));
        }
        self.partial(at, each, expr, depth)
    }

    /// The value of `expr` when it does not evaluate whole, by its form (`value`).
    fn partial(
        &mut self,
        at: usize,
        each: Option<&Each>,
        expr: &'m Expression,
        depth: usize,
    ) -> Evaluated {
        let template = match expr {
            Expression::Parenthesis(p) => return self.value(at, each, p.inner(), depth),
            Expression::StringTemplate(t) => &**t,
            Expression::HeredocTemplate(h) => &h.template,
            Expression::Traversal(t) => {
                let Expression::Variable(root) = &t.expr else {
                    return Evaluated::unknown();
                };
                let ops: Vec<&TraversalOperator> = t.operators.iter().map(|o| o.value()).collect();
                return match (root.as_str(), reference(root.as_str(), &ops)) {
                    ("var", Some(Ref::Var(n))) if ops.len() == 1 => {
                        self.name(at, Name::Var(n), depth)
                    }
                    ("local", Some(Ref::Local(n))) if ops.len() == 1 => {
                        self.name(at, Name::Local(n), depth)
                    }
                    (
                        "module",
                        Some(Ref::Block {
                            name,
                            key,
                            output: Some(out),
                            ..
                        }),
                    ) if self.outputs && ops.len() == 2 + usize::from(key.is_some()) => {
                        let children = self.instances[at].children.get(name.as_str());
                        let found: Vec<usize> = (children.into_iter().flatten())
                            .filter(|(k, _)| k == &key)
                            .map(|(_, c)| *c)
                            .collect();
                        match found[..] {
                            [child] => self.name(child, Name::Output(out), depth + 1),
                            _ => Evaluated::unknown(),
                        }
                    }
                    _ => Evaluated::unknown(),
                };
            }
            _ => return Evaluated::unknown(),
        };
        let (mut text, mut open, mut through) = (String::new(), false, BTreeSet::new());
        for element in template.iter() {
            match element {
                Element::Literal(s) => text.push_str(s.value()),
                Element::Interpolation(i) => {
                    let found = self.value(at, each, &i.expr, depth);
                    let (part, unresolved) = found.text();
                    text.push_str(&part);
                    open |= unresolved;
                    through.extend(found.through);
                }
                Element::Directive(_) => {
                    text.push('*');
                    open = true;
                }
            }
            if text.len() > EVAL_VALUE_MAX {
                return Evaluated::unknown();
            }
        }
        let val = match open {
            true => {
                while text.contains("**") {
                    text = text.replace("**", "*");
                }
                Val::Pattern(text)
            }
            false => Val::Known(Value::String(text)),
        };
        Evaluated { val, through }
    }

    /// The addresses the reference `r` in the instance `at` resolves to, sorted, at most
    /// `REFERENCES_MAX`; `depth` names already gone through.
    fn resolve(&mut self, at: usize, r: &'m Ref, depth: usize) -> Vec<String> {
        if depth > EVAL_DEPTH_MAX || !self.open.insert((at, r)) {
            return Vec::new();
        }
        if let Some(found) = self.memo.get(&(at, r)) {
            self.open.remove(&(at, r));
            return found.clone();
        }
        let modules = self.modules;
        let mut found: BTreeSet<String> = BTreeSet::new();
        // The instances of `list` the key names, or all of them when it names none of them.
        fn pick<T: Clone>(list: &[(Option<Key>, T)], key: &Option<Key>) -> Vec<T> {
            let named: Vec<T> = (list.iter())
                .filter(|(k, _)| key.is_some() && k == key)
                .map(|(_, t)| t.clone())
                .collect();
            match named.is_empty() {
                true => list.iter().map(|(_, t)| t.clone()).collect(),
                false => named,
            }
        }
        match r {
            Ref::Block {
                kind,
                ty,
                name,
                key,
                output,
            } => {
                if let (Kind::Module, Some(out)) = (kind, output) {
                    let children = self.instances[at].children.get(name.as_str());
                    for child in pick(children.map_or(&[][..], Vec::as_slice), key) {
                        let dir = self.instances[child].dir;
                        let output = modules[dir].outputs.get(out);
                        for inner in output.into_iter().flat_map(|o| &o.refs) {
                            found.extend(self.resolve(child, inner, depth + 1));
                        }
                    }
                }
                if found.is_empty() {
                    let entry = (*kind, ty.as_str(), name.as_str());
                    let nodes = self.instances[at].nodes.get(&entry);
                    found.extend(pick(nodes.map_or(&[][..], Vec::as_slice), key));
                }
            }
            Ref::Var(name) => {
                if let Some((caller, block)) = self.instances[at].parent {
                    for inner in block.args.get(name).into_iter().flat_map(|a| &a.refs) {
                        found.extend(self.resolve(caller, inner, depth + 1));
                    }
                }
            }
            Ref::Local(name) => {
                let dir = self.instances[at].dir;
                for inner in modules[dir]
                    .locals
                    .get(name)
                    .into_iter()
                    .flat_map(|l| &l.refs)
                {
                    found.extend(self.resolve(at, inner, depth + 1));
                }
            }
        }
        let found: Vec<String> = found.into_iter().take(REFERENCES_MAX).collect();
        self.open.remove(&(at, r));
        self.memo.insert((at, r), found.clone());
        found
    }
}

/// The label of the root module at `dir`: its path, `.` for the repository's root.
fn label(dir: &str) -> &str {
    if dir.is_empty() { "." } else { dir }
}

/// The Terraform files of one repository read into modules, one per directory.
pub(crate) struct Repository {
    modules: BTreeMap<String, Module>,
    /// The Compose files (#652), by path.
    compose: Vec<(String, Vec<u8>)>,
}

impl Repository {
    /// Reads the Terraform files `files` (path and bytes, `is_input`) of one repository.
    pub(crate) fn read(files: &[(String, Vec<u8>)]) -> Repository {
        let mut modules: BTreeMap<String, Module> = BTreeMap::new();
        let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let mut compose = Vec::new();
        for (path, src) in sorted {
            if compose::is_file(path) {
                compose.push((path.clone(), src.clone()));
                continue;
            }
            let dir = config_values::parent(path).to_string();
            read(path, src, modules.entry(dir).or_default());
        }
        Repository { modules, compose }
    }

    /// The directories each local module call of the module at `dir` names.
    fn calls<'a>(&'a self, dir: &'a str) -> impl Iterator<Item = String> + 'a {
        (self.modules.get(dir).into_iter().flat_map(|m| &m.blocks))
            .filter_map(move |b| b.source.as_deref().and_then(|s| local_source(dir, s)))
    }

    /// The Terraform root modules by directory, each with its evaluation unit: its directory and
    /// those of the local modules it calls, however deep (up to `MODULE_DEPTH_MAX`), whether they
    /// hold files or not. A directory another one calls as a local module is evaluated only
    /// through its calls, so it is no root.
    fn terraform_units(&self) -> BTreeMap<&str, BTreeSet<String>> {
        let called: HashSet<String> = (self.modules.keys())
            .flat_map(|dir| self.calls(dir).filter(move |t| t != dir))
            .collect();
        let roots = self.modules.keys().filter(|d| !called.contains(*d));
        roots
            .map(|root| {
                let mut unit = BTreeSet::from([root.clone()]);
                let mut level = vec![root.clone()];
                for _ in 0..=MODULE_DEPTH_MAX {
                    let next: Vec<String> = (level.iter())
                        .flat_map(|d| self.calls(d).collect::<Vec<_>>())
                        .filter(|d| unit.insert(d.clone()))
                        .collect();
                    level = next;
                }
                (root.as_str(), unit)
            })
            .collect()
    }

    /// Each root, by its label, with its evaluation unit: a Terraform root module with its
    /// called modules, or the directory of a Compose project (#652), which joins the Terraform
    /// root of the same directory.
    pub(crate) fn units(&self) -> Vec<(String, BTreeSet<String>)> {
        let mut units: BTreeMap<String, BTreeSet<String>> = (self.terraform_units().into_iter())
            .map(|(dir, unit)| (dir.to_string(), unit))
            .collect();
        for (path, _) in &self.compose {
            let dir = config_values::parent(path).to_string();
            units.entry(dir.clone()).or_default().insert(dir);
        }
        units
            .into_iter()
            .map(|(dir, unit)| (label(&dir).to_string(), unit))
            .collect()
    }

    /// The IaC nodes and references of the roots whose label `pick` keeps, each evaluated alone,
    /// in root and file order, with `left` bytes to store them all.
    pub(crate) fn evaluate(
        &self,
        pick: impl Fn(&str) -> bool,
        left: usize,
    ) -> (Vec<Node>, Vec<Reference>) {
        let mut eval = Eval::new(&self.modules, left);
        let terraform = self.terraform_units();
        let mut compose_dirs: Vec<&str> = (self.compose.iter())
            .map(|(p, _)| config_values::parent(p))
            .collect();
        compose_dirs.dedup();
        for (root, _) in self.units() {
            if !pick(&root) {
                continue;
            }
            let dir = if root == "." { "" } else { root.as_str() };
            if let Some((dir, _)) = terraform.get_key_value(dir) {
                eval.root(dir);
            }
            if compose_dirs.contains(&dir) {
                let files: Vec<(String, Vec<u8>)> = (self.compose.iter())
                    .filter(|(p, _)| config_values::parent(p) == dir)
                    .cloned()
                    .collect();
                let (nodes, references) = compose::evaluate(&files, &mut eval.left);
                eval.nodes.extend(nodes);
                eval.references.extend(references);
            }
        }
        (eval.nodes, eval.references)
    }
}

/// The IaC nodes and references of the Terraform files `files` (path and bytes) of one
/// repository, each root module evaluated alone, in root and file order.
#[cfg(test)]
pub(crate) fn evaluate(files: &[(String, Vec<u8>)]) -> (Vec<Node>, Vec<Reference>) {
    let (mut nodes, mut references) = Repository::read(files).evaluate(|_| true, STORED_MAX);
    for (path, src) in files.iter().filter(|(p, s)| template_wanted(p, s)) {
        let (n, r) = template_evaluate(path, src);
        nodes.extend(n);
        references.extend(r);
    }
    (nodes, references)
}

/// Evaluates again the root modules of `repository` whose unit holds a directory of `dirs`
/// (where a Terraform file changed, appeared or left) and those not stored yet, drops the rows
/// of the roots that are no more, and keeps the rows of every other root as they are.
pub(crate) fn refresh(
    tx: &Transaction,
    repository: &Repository,
    dirs: &BTreeSet<String>,
) -> Result<()> {
    let mut statement = tx.prepare(&format!(
        "SELECT DISTINCT root FROM iac_nodes WHERE {}",
        terraform_path("path")
    ))?;
    let stored: BTreeSet<String> = statement
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let units = repository.units();
    let again: BTreeSet<&str> = (units.iter())
        .filter(|(root, unit)| !stored.contains(root) || !unit.is_disjoint(dirs))
        .map(|(root, _)| root.as_str())
        .collect();
    for root in &stored {
        if again.contains(root.as_str()) || !units.iter().any(|(r, _)| r == root) {
            forget(tx, Some(root), Owner::Terraform)?;
        }
    }
    let bytes = |sql: &str| -> Result<usize> {
        let n: Option<i64> = tx.query_row(sql, [], |r| r.get(0)).optional()?.flatten();
        Ok(n.unwrap_or(0) as usize)
    };
    let kept = bytes(&format!(
        "SELECT sum(length(CAST(address AS BLOB)) + coalesce(length(CAST(name AS BLOB)), 0)) \
         FROM iac_nodes WHERE {}",
        terraform_path("path")
    ))? + bytes(&format!(
        "SELECT sum(length(CAST(source AS BLOB)) + length(CAST(target AS BLOB)) \
         + coalesce(length(CAST(via AS BLOB)), 0)) FROM iac_references r WHERE EXISTS \
         (SELECT 1 FROM iac_nodes n WHERE n.root = r.root AND n.address = r.source AND {})",
        terraform_path("n.path")
    ))?;
    let (nodes, references) =
        repository.evaluate(|root| again.contains(root), STORED_MAX.saturating_sub(kept));
    insert(tx, &nodes, &references)
}

/// The condition that the file in `column` is one this module reads: a Terraform file or, since
/// #652, a Compose file. The Terraform and Compose nodes and the Kubernetes nodes (#651) share
/// the tables, and a Terraform root and a Kubernetes unit may share a label, so each side touches
/// the rows of its own files only.
fn terraform_path(column: &str) -> String {
    let mut any = vec![
        format!("{column} LIKE '%.tf'"),
        format!("{column} LIKE '%.tofu'"),
    ];
    for name in compose::NAMES {
        any.push(format!("{column} = '{name}' OR {column} LIKE '%/{name}'"));
    }
    format!("({})", any.join(" OR "))
}

/// Who writes a stored node. The Terraform and Compose nodes (#649, #652), the Kubernetes nodes
/// (#651) and the CloudFormation nodes (#653) share the tables, and each is rewritten by its own
/// refresh, so each touches the rows it owns only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    /// A node without `::` in its type, in a Terraform, OpenTofu or Compose file.
    Terraform,
    /// A node whose type has no `::`, in any other file: a Kubernetes kind.
    Kubernetes,
    /// A node whose type has a `::`, regardless of filename: `AWS::SQS::Queue`, `Custom::Name`.
    CloudFormation,
}

impl Owner {
    /// The condition that a row of `iac_nodes` is owned by `self`.
    pub(crate) fn sql(self) -> String {
        let own = terraform_path("path");
        let typed = "type LIKE '%::%'";
        match self {
            Owner::Terraform => format!("({own} AND NOT {typed})"),
            Owner::Kubernetes => format!("(NOT {own} AND NOT {typed})"),
            Owner::CloudFormation => typed.to_string(),
        }
    }
}

/// Deletes the stored nodes and references `owner` writes, of the root `root` or of every root.
pub(crate) fn forget(tx: &Transaction, root: Option<&str>, owner: Owner) -> Result<()> {
    let own = owner.sql();
    let mut args: Vec<&dyn rusqlite::ToSql> = Vec::new();
    let mut scope = String::new();
    if let Some(root) = &root {
        scope = " AND root = ?1".into();
        args.push(root);
    }
    // The references go first: they are found through the nodes that source them.
    tx.execute(
        &format!(
            "DELETE FROM iac_references WHERE (root, source) IN \
             (SELECT root, address FROM iac_nodes WHERE {own}{scope})"
        ),
        args.as_slice(),
    )?;
    tx.execute(
        &format!("DELETE FROM iac_nodes WHERE {own}{scope}"),
        args.as_slice(),
    )?;
    Ok(())
}

/// Whether the file `path` with bytes `src` holds a template unit: a CloudFormation or SAM
/// template, or a Serverless service (#654), whose `resources` section is one.
fn template_wanted(path: &str, src: &[u8]) -> bool {
    cloudformation::wanted(path, src) || serverless::wanted(path, src)
}

/// The nodes and references of a template unit: a Serverless service by its file name, else a
/// template. A service file that is none yields nothing.
pub(crate) fn template_evaluate(path: &str, src: &[u8]) -> (Vec<Node>, Vec<Reference>) {
    match serverless::is_file(path) {
        true => serverless::evaluate(path, src),
        false => cloudformation::evaluate(path, src),
    }
}

/// Whether a refresh that read the file `path` again (`src`) or lost it (`None`) changes the
/// CloudFormation nodes: it says it is a template, or it held nodes.
pub(crate) fn template_changed(tx: &Transaction, path: &str, src: Option<&[u8]>) -> Result<bool> {
    if !cloudformation::is_source(path) {
        return Ok(false);
    }
    if src.is_some_and(|src| template_wanted(path, src)) {
        return Ok(true);
    }
    let held = format!(
        "SELECT 1 FROM iac_nodes WHERE path = ?1 AND {} LIMIT 1",
        Owner::CloudFormation.sql()
    );
    Ok(tx.prepare_cached(&held)?.exists(params![path])?)
}

/// Replaces the nodes and references of the template at `path` with those of `src` (`None` for a
/// file that left): a template is its own evaluation unit, so no other root is touched.
pub(crate) fn refresh_template(tx: &Transaction, path: &str, src: Option<&[u8]>) -> Result<()> {
    forget(tx, Some(path), Owner::CloudFormation)?;
    // ponytail: a file past `MAX_BYTES` is no template, as config values count it unreadable.
    let readable = |s: &&[u8]| s.len() <= MAX_BYTES && template_wanted(path, s);
    if let Some(src) = src.filter(readable) {
        let (nodes, references) = template_evaluate(path, src);
        insert(tx, &nodes, &references)?;
    }
    Ok(())
}

/// Stores `nodes` and `references`.
pub(crate) fn insert(tx: &Transaction, nodes: &[Node], references: &[Reference]) -> Result<()> {
    let mut insert = tx
        .prepare_cached("INSERT INTO iac_nodes VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)")?;
    for n in nodes {
        let (line, end) = (n.line as i64, n.end_line as i64);
        let overridable = serde_json::to_string(&n.overridable)?;
        let (image, build) = (n.image.as_ref(), n.build.as_ref());
        let events = serde_json::to_string(&n.events)?;
        insert.execute(params![
            n.address,
            n.root,
            n.ty,
            n.name,
            n.pattern,
            overridable,
            n.path,
            line,
            end,
            n.block,
            image.map(|i| &i.text),
            image.map(|i| i.overridable),
            build.map(|b| &b.text),
            build.map(|b| b.overridable),
            n.role,
            n.handler.as_ref().map(|h| &h.text),
            n.handler.as_ref().map(|h| h.overridable),
            events,
        ])?;
    }
    let mut insert = tx.prepare_cached("INSERT INTO iac_references VALUES (?1, ?2, ?3, ?4)")?;
    for r in references {
        insert.execute(params![r.root, r.source, r.target, r.via])?;
    }
    Ok(())
}

/// The columns of a node, in `row`'s order.
const COLUMNS: &str = "address, root, type, name, pattern, overridable, path, line, end_line, \
                       block, image, image_overridable, build, build_overridable, role, \
                       handler, handler_overridable, events";

/// The text at column `at` and its overridable flag at the next one, when there is a text.
fn resolved(r: &rusqlite::Row, at: usize) -> rusqlite::Result<Option<Resolved>> {
    let text: Option<String> = r.get(at)?;
    let overridable: Option<bool> = r.get(at + 1)?;
    Ok(text.map(|text| Resolved {
        text,
        overridable: overridable.unwrap_or(false),
    }))
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Node> {
    let overridable: String = r.get(5)?;
    Ok(Node {
        address: r.get(0)?,
        root: r.get(1)?,
        ty: r.get(2)?,
        name: r.get(3)?,
        pattern: r.get(4)?,
        overridable: serde_json::from_str(&overridable).unwrap_or_default(),
        path: r.get(6)?,
        line: r.get::<_, i64>(7)? as usize,
        end_line: r.get::<_, i64>(8)? as usize,
        block: r.get(9)?,
        image: resolved(r, 10)?,
        build: resolved(r, 12)?,
        role: r.get(14)?,
        handler: resolved(r, 15)?,
        events: serde_json::from_str(&r.get::<_, String>(17)?).unwrap_or_default(),
    })
}

/// `<identity> <path>:<line> <address>`.
fn at(identity: &str, n: &Node) -> String {
    format!("{identity} {}:{} {}", n.path, n.line, n.address)
}

/// The IaC nodes of `db` (repository `identity`) whose address, type or physical name matches
/// `needle`, lowercased, as `search` hits: `<identity> <path>:<line> <address> (<type>[, name
/// "<name>"], root <root>)`, `pattern` in place of `name` for a pattern, ordered by rank, then
/// repository, path, line, address and root.
pub(crate) fn hits(identity: &str, db: &Connection, needle: &str) -> Result<Vec<Hit>> {
    // ponytail: scans every node of every repository; an indexed lookup waits for a workspace
    // that makes it slow.
    let mut statement = db.prepare(&format!("SELECT {COLUMNS} FROM iac_nodes"))?;
    let mut hits = Vec::new();
    for n in statement.query_map([], row)? {
        let n = n?;
        let fields = [Some(n.address.as_str()), Some(&n.ty), n.name.as_deref()];
        let Some(r) = (fields.into_iter().flatten())
            .filter_map(|f| rank(&f.to_lowercase(), needle))
            .min()
        else {
            continue;
        };
        let word = if n.pattern { "pattern" } else { "name" };
        let name = (n.name.as_ref()).map_or(String::new(), |s| {
            format!(", {word} {}", serde_json::to_string(s).unwrap_or_default())
        });
        let text = format!("{} ({}{name}, root {})", at(identity, &n), n.ty, n.root);
        let key = format!(
            "{identity} {} {:010} {} {}",
            n.path, n.line, n.address, n.root
        );
        hits.push((r, key, text));
    }
    Ok(hits)
}

/// The nodes `text` names in `db` (repository `identity`): `<address>`, `<path>::<address>` or
/// `<identity>::<path>::<address>`, any of them followed by ` (root <root>)` to pick a root.
fn named(identity: &str, db: &Connection, text: &str) -> Result<Vec<Node>> {
    let (text, root) = match text
        .strip_suffix(')')
        .and_then(|t| t.rsplit_once(" (root "))
    {
        Some((text, root)) => (text, Some(root)),
        None => (text, None),
    };
    // The address is the whole text, or what follows one of its `::`.
    let mut candidates = vec![text];
    candidates.extend(text.match_indices("::").map(|(i, _)| &text[i + 2..]));
    let mut statement = db.prepare(&format!(
        "SELECT {COLUMNS} FROM iac_nodes WHERE address = ?1"
    ))?;
    let mut found = Vec::new();
    for address in candidates {
        for n in statement.query_map(params![address], row)? {
            let n = n?;
            let path = format!("{}::{address}", n.path);
            let fits = text == address || text == path || text == format!("{identity}::{path}");
            if fits && root.is_none_or(|r| r == n.root) {
                found.push(n);
            }
        }
    }
    found.sort_by(|a, b| {
        (&a.path, a.line, &a.address, &a.root).cmp(&(&b.path, b.line, &b.address, &b.root))
    });
    found.dedup();
    Ok(found)
}

/// The nodes of the workspace `text` names (`named`), with the position of their repository;
/// `repos` holds each repository's identity and index.
pub(crate) fn found(repos: &[(&str, &Connection)], text: &str) -> Result<Vec<(usize, Node)>> {
    let mut found = Vec::new();
    for (n, (identity, db)) in repos.iter().enumerate() {
        found.extend(named(identity, db, text)?.into_iter().map(|node| (n, node)));
    }
    Ok(found)
}

/// The answer when `text` fits several nodes: one line each, with what to pass for it.
fn several(repos: &[(&str, &Connection)], text: &str, found: &[(usize, Node)]) -> Vec<String> {
    let short = |n: &Node| format!("{} (root {})", n.address, n.root);
    let mut lines = vec![format!(
        "`{text}` fits {} IaC nodes; pass one of:",
        found.len()
    )];
    lines.extend(found.iter().map(|(r, n)| {
        let identity = repos[*r].0;
        let pass = match found.iter().filter(|(_, o)| short(o) == short(n)).count() {
            1 => short(n),
            _ => format!("{identity}::{}::{}", n.path, short(n)),
        };
        format!("{identity} {}:{} {pass}", n.path, n.line)
    }));
    lines
}

/// The nodes `node` references (`Down`) or that reference it (`Up`), each with the name the
/// reference went through, in path, line and address order.
fn references(
    db: &Connection,
    node: &Node,
    direction: Direction,
) -> Result<Vec<(Node, Option<String>)>> {
    let (from, to) = match direction {
        Direction::Down => ("source", "target"),
        Direction::Up => ("target", "source"),
    };
    let sql = format!(
        "SELECT {}, r.via \
         FROM iac_references r JOIN iac_nodes n ON n.root = r.root AND n.address = r.{to} \
         WHERE r.root = ?1 AND r.{from} = ?2",
        COLUMNS
            .split(", ")
            .map(|c| format!("n.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut statement = db.prepare(&sql)?;
    let mut found: Vec<(Node, Option<String>)> = statement
        .query_map(params![node.root, node.address], |r| {
            Ok((row(r)?, r.get(18)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    found.sort_by(|(a, va), (b, vb)| {
        (&a.path, a.line, &a.address, va).cmp(&(&b.path, b.line, &b.address, vb))
    });
    Ok(found)
}

/// A recorded text as JSON, then ` (overridable)` when a default gave it and ` (unresolved)` when
/// a variable with no default is still in it.
fn resolved_shown(f: &Resolved) -> String {
    let marks = [
        (f.overridable, " (overridable)"),
        (f.text.contains("${"), " (unresolved)"),
    ];
    let marks: String = marks.iter().filter(|m| m.0).map(|m| m.1).collect();
    format!(
        "{}{marks}",
        serde_json::to_string(&f.text).unwrap_or_default()
    )
}

/// `<identity> <path>:<line> <address>`, then ` via <name>` when the reference went through one.
fn reached(identity: &str, n: &Node, via: &Option<String>) -> String {
    let via = via.as_ref().map_or(String::new(), |v| format!(" via {v}"));
    format!("{}{via}", at(identity, n))
}

/// `context` on the IaC node `text` names, or `None` when it names none: its first line
/// `<identity> <path>:<line>-<end> <address>`, then its type, its physical name (marked
/// `(pattern)` when a part is unresolved), its root, its Crossplane role when it has one (#655),
/// the overridable values its name went through, the nodes it references, those that reference
/// it and its config values. A text that fits several nodes lists them.
pub(crate) fn context(repos: &[(&str, &Connection)], text: &str) -> Result<Option<Vec<String>>> {
    let found = found(repos, text)?;
    let [(r, n)] = &found[..] else {
        return Ok((!found.is_empty()).then(|| several(repos, text, &found)));
    };
    let (identity, db) = repos[*r];
    let name = (n.name.as_ref()).map_or("none".into(), |s| {
        let pattern = if n.pattern { " (pattern)" } else { "" };
        format!("{}{pattern}", serde_json::to_string(s).unwrap_or_default())
    });
    let mut lines = vec![
        format!(
            "{identity} {}:{}-{} {}",
            n.path, n.line, n.end_line, n.address
        ),
        format!("type: {}", n.ty),
        format!("name: {name}"),
        format!("root: {}", n.root),
    ];
    lines.extend(n.role.as_ref().map(|r| format!("role: {r}")));
    for (title, field) in [
        ("image", &n.image),
        ("build", &n.build),
        ("handler", &n.handler),
    ] {
        if let Some(f) = field {
            lines.push(format!("{title}: {}", resolved_shown(f)));
        }
    }
    for event in &n.events {
        lines.push(format!("event: {}", resolved_shown(event)));
    }
    let overridable = (n.overridable.iter()).map(|o| {
        let from = if o.tfvars { "tfvars" } else { "default" };
        format!("{identity} {}:{} {} ({from})", o.path, o.line, o.variable)
    });
    lines.extend(section("overridable", overridable.collect()));
    for (title, direction) in [
        ("references", Direction::Down),
        ("referenced by", Direction::Up),
    ] {
        let reached_nodes = references(db, n, direction)?;
        let items = (reached_nodes.iter()).map(|(m, via)| reached(identity, m, via));
        lines.extend(section(title, items.collect()));
    }
    let images = crate::kube::images(db, &n.root, &n.address)?;
    if !images.is_empty() {
        lines.extend(section("images", images));
    }
    let values = config_values::under(identity, db, &n.path, &n.block)?;
    lines.extend(section("values", values));
    Ok(Some(lines))
}

/// `trace` from the IaC node `text` names, or `None` when it names none: the node, then the nodes
/// it references (`Down`) or that reference it (`Up`), depth first, each indented under the one
/// it hangs from, a node met again on its own branch marked ` (cycle)` and one already followed
/// ` (see above)`; then the chain footer. A text that fits several nodes lists them.
pub(crate) fn trace(
    repos: &[(&str, &Connection)],
    text: &str,
    direction: Direction,
    budget: Option<usize>,
    mut provisioned: impl FnMut(usize, &Node, usize) -> Vec<Row>,
) -> Result<Option<String>> {
    let found = found(repos, text)?;
    let [(r, start)] = &found[..] else {
        return Ok((!found.is_empty())
            .then(|| crate::render::budgeted(&several(repos, text, &found), budget)));
    };
    let (identity, db) = repos[*r];
    let mut rows = Vec::new();
    let mut expanded: HashSet<String> = HashSet::new();
    let mut path: Vec<String> = Vec::new();
    // (depth, node, via), popped in walk order.
    let mut stack: Vec<(usize, Node, Option<String>)> = vec![(0, start.clone(), None)];
    while let Some((depth, n, via)) = stack.pop() {
        path.truncate(depth);
        let mark = match () {
            _ if path.contains(&n.address) => " (cycle)",
            _ if expanded.contains(&n.address) => " (see above)",
            _ => "",
        };
        let text = format!(
            "{}{}{mark}",
            "  ".repeat(depth),
            reached(identity, &n, &via)
        );
        let level = (depth > 0).then_some(Level::Exact);
        rows.push(Row::call(text, *r, level));
        if !mark.is_empty() {
            continue;
        }
        if direction == Direction::Down {
            rows.extend(provisioned(*r, &n, depth + 1));
        }
        expanded.insert(n.address.clone());
        path.push(n.address.clone());
        let next = references(db, &n, direction)?;
        stack.extend(next.into_iter().rev().map(|(m, via)| (depth + 1, m, via)));
    }
    let chain = Chain {
        rows,
        missing: Vec::new(),
    };
    Ok(Some(chain.answer(budget)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
        (list.iter())
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect()
    }

    fn addresses(nodes: &[Node]) -> Vec<String> {
        (nodes.iter())
            .map(|n| format!("{} @{}", n.address, n.root))
            .collect()
    }

    fn shown(references: &[Reference]) -> Vec<String> {
        (references.iter())
            .map(|r| {
                let via = r
                    .via
                    .as_ref()
                    .map_or(String::new(), |v| format!(" via {v}"));
                format!("{} -> {}{via}", r.source, r.target)
            })
            .collect()
    }

    #[test]
    fn resources_data_blocks_and_module_calls_are_nodes_at_their_address() {
        let (nodes, _) = evaluate(&files(&[(
            "main.tf",
            concat!(
                "resource \"aws_sqs_queue\" \"orders\" {\n  name = \"orders-queue\"\n}\n",
                "data \"aws_caller_identity\" \"me\" {}\n",
                "module \"remote\" {\n  source = \"terraform-aws-modules/sqs/aws\"\n}\n",
                "variable \"x\" {}\noutput \"y\" { value = 1 }\nlocals { z = 1 }\n",
                "provider \"aws\" {}\n",
            ),
        )]));
        assert_eq!(
            addresses(&nodes),
            [
                "aws_sqs_queue.orders @.",
                "data.aws_caller_identity.me @.",
                "module.remote @."
            ]
        );
        let queue = &nodes[0];
        assert_eq!(
            (
                queue.ty.as_str(),
                queue.name.as_deref(),
                queue.line,
                queue.end_line
            ),
            ("aws_sqs_queue", Some("orders-queue"), 1, 3)
        );
        assert_eq!(queue.block, "resource.aws_sqs_queue.orders");
        assert_eq!(nodes[1].block, "data.aws_caller_identity.me");
        assert_eq!(
            (nodes[2].ty.as_str(), nodes[2].block.as_str()),
            ("module", "module.remote")
        );
    }

    #[test]
    fn a_local_module_is_instantiated_once_per_call_under_its_address() {
        let (nodes, _) = evaluate(&files(&[
            (
                "envs/prod/main.tf",
                "module \"a\" { source = \"../../modules/q\" }\nmodule \"b\" { source = \"../../modules/q\" }\n",
            ),
            (
                "modules/q/main.tf",
                "resource \"aws_sqs_queue\" \"this\" {}\n",
            ),
            (
                "envs/stage/main.tf",
                "module \"a\" { source = \"./../../modules/q/\" }\n",
            ),
        ]));
        assert_eq!(
            addresses(&nodes),
            [
                "module.a @envs/prod",
                "module.a.aws_sqs_queue.this @envs/prod",
                "module.b @envs/prod",
                "module.b.aws_sqs_queue.this @envs/prod",
                "module.a @envs/stage",
                "module.a.aws_sqs_queue.this @envs/stage",
            ]
        );
    }

    #[test]
    fn a_module_source_that_climbs_out_of_the_repository_or_is_remote_calls_nothing() {
        assert_eq!(local_source("a/b", "../../c").as_deref(), Some("c"));
        assert_eq!(local_source("a", "./m/"), Some("a/m".into()));
        assert_eq!(local_source("a", "../../c"), None);
        assert_eq!(local_source("", "../x"), None);
        assert_eq!(local_source("", "git::https://x/y.git"), None);
        assert_eq!(local_source("", "/abs/path"), None);
        let (nodes, _) = evaluate(&files(&[
            ("main.tf", "module \"out\" { source = \"../outside\" }\n"),
            ("outside/main.tf", "resource \"a\" \"b\" {}\n"),
        ]));
        // `outside` is a root of its own: nothing calls it from inside the repository.
        assert_eq!(addresses(&nodes), ["module.out @.", "a.b @outside"]);
    }

    #[test]
    fn count_and_for_each_over_literals_give_one_node_per_key() {
        let (nodes, _) = evaluate(&files(&[
            (
                "main.tf",
                concat!(
                    "resource \"a\" \"counted\" { count = 2 }\n",
                    "resource \"a\" \"set\" { for_each = toset([\"us\", \"eu\", \"us\"]) }\n",
                    "resource \"a\" \"map\" { for_each = { prd = 1, \"stg-1\" = 2 } }\n",
                    "resource \"a\" \"open\" { for_each = var.regions }\n",
                    "resource \"a\" \"zero\" { count = 0 }\n",
                    "module \"m\" {\n  source = \"./m\"\n  count = 2\n}\n",
                ),
            ),
            ("m/main.tf", "resource \"b\" \"c\" {}\n"),
        ]));
        assert_eq!(
            addresses(&nodes),
            [
                "a.counted[0] @.",
                "a.counted[1] @.",
                "a.set[\"eu\"] @.",
                "a.set[\"us\"] @.",
                "a.map[\"prd\"] @.",
                "a.map[\"stg-1\"] @.",
                "a.open @.",
                "module.m[0] @.",
                "module.m[0].b.c @.",
                "module.m[1] @.",
                "module.m[1].b.c @.",
            ]
        );
    }

    #[test]
    fn a_count_past_the_expansion_ceiling_is_one_node_with_no_key() {
        let text = |n: usize| format!("resource \"a\" \"b\" {{ count = {n} }}\n");
        let (nodes, _) = evaluate(&files(&[("main.tf", &text(EXPANSION_MAX))]));
        assert_eq!(nodes.len(), EXPANSION_MAX);
        let (nodes, _) = evaluate(&files(&[("main.tf", &text(EXPANSION_MAX + 1))]));
        assert_eq!(addresses(&nodes), ["a.b @."]);
    }

    #[test]
    fn references_name_resources_data_blocks_modules_and_their_outputs() {
        let (_, references) = evaluate(&files(&[
            (
                "main.tf",
                concat!(
                    "data \"aws_iam_policy_document\" \"p\" {}\n",
                    "resource \"aws_sns_topic\" \"t\" {\n  policy = data.aws_iam_policy_document.p.json\n}\n",
                    "module \"q\" {\n  source = \"./q\"\n  topic = aws_sns_topic.t.arn\n}\n",
                    "resource \"aws_lambda_event_source_mapping\" \"m\" {\n",
                    "  event_source_arn = module.q.arn\n  depends_on = [module.q]\n}\n",
                    "resource \"aws_x\" \"y\" { tags = { a = \"${local.topic}\" } }\n",
                    "locals { topic = aws_sns_topic.t.name }\n",
                ),
            ),
            (
                "q/main.tf",
                concat!(
                    "variable \"topic\" {}\n",
                    "resource \"aws_sqs_queue\" \"this\" {}\n",
                    "resource \"aws_sns_topic_subscription\" \"s\" {\n",
                    "  topic_arn = var.topic\n  endpoint = aws_sqs_queue.this.arn\n}\n",
                    "output \"arn\" { value = aws_sqs_queue.this.arn }\n",
                ),
            ),
        ]));
        assert_eq!(
            shown(&references),
            [
                "aws_sns_topic.t -> data.aws_iam_policy_document.p",
                "module.q -> aws_sns_topic.t",
                "aws_lambda_event_source_mapping.m -> module.q",
                "aws_lambda_event_source_mapping.m -> module.q.aws_sqs_queue.this via module.q.arn",
                "aws_x.y -> aws_sns_topic.t via local.topic",
                "module.q.aws_sns_topic_subscription.s -> aws_sns_topic.t via var.topic",
                "module.q.aws_sns_topic_subscription.s -> module.q.aws_sqs_queue.this",
            ]
        );
    }

    #[test]
    fn a_literal_index_picks_its_instance_and_any_other_names_them_all() {
        let (_, references) = evaluate(&files(&[(
            "main.tf",
            concat!(
                "resource \"a\" \"q\" { for_each = toset([\"eu\", \"us\"]) }\n",
                "resource \"b\" \"one\" { x = a.q[\"eu\"].arn }\n",
                "resource \"b\" \"all\" { x = values(a.q)[*].arn }\n",
                "resource \"b\" \"missing\" { x = a.q[\"ap\"].arn }\n",
                "resource \"b\" \"gone\" { x = a.nothing.arn }\n",
            ),
        )]));
        assert_eq!(
            shown(&references),
            [
                "b.one -> a.q[\"eu\"]",
                "b.all -> a.q[\"eu\"]",
                "b.all -> a.q[\"us\"]",
                "b.missing -> a.q[\"eu\"]",
                "b.missing -> a.q[\"us\"]",
            ]
        );
    }

    /// Locals that name each other twice over resolve in linear time, a cycle of locals resolves
    /// to nothing, and a chain longer than `EVAL_DEPTH_MAX` stops.
    #[test]
    fn a_chain_of_names_past_the_evaluation_depth_stops() {
        // `n` locals, the last named by `b.u`, each naming the one before twice.
        let chain = |n: usize| {
            let mut text = String::from("resource \"a\" \"t\" {}\nlocals {\n  l0 = a.t.id\n");
            for i in 1..n {
                text += &format!("  l{i} = [local.l{}, local.l{}]\n", i - 1, i - 1);
            }
            text += &format!("}}\nresource \"b\" \"u\" {{ x = local.l{} }}\n", n - 1);
            text
        };
        let (_, references) = evaluate(&files(&[("main.tf", &chain(EVAL_DEPTH_MAX))]));
        let last = EVAL_DEPTH_MAX - 1;
        assert_eq!(
            shown(&references),
            [format!("b.u -> a.t via local.l{last}")]
        );
        let (_, references) = evaluate(&files(&[("main.tf", &chain(EVAL_DEPTH_MAX + 1))]));
        assert!(references.is_empty(), "{references:?}");
        let cycle =
            "locals {\n  a = local.b\n  b = local.a\n}\nresource \"b\" \"u\" { x = local.a }\n";
        let (_, references) = evaluate(&files(&[("main.tf", cycle)]));
        assert!(references.is_empty());
    }

    #[test]
    fn modules_nested_past_the_ceiling_keep_their_call_and_instantiate_nothing() {
        let depth = MODULE_DEPTH_MAX + 3;
        let mut list: Vec<(String, String)> = (0..depth)
            .map(|i| {
                (
                    format!("m{i}/main.tf"),
                    format!("module \"n\" {{ source = \"../m{}\" }}\n", i + 1),
                )
            })
            .collect();
        list.push((
            format!("m{depth}/main.tf"),
            "resource \"a\" \"b\" {}\n".into(),
        ));
        let list: Vec<(String, Vec<u8>)> =
            list.into_iter().map(|(p, t)| (p, t.into_bytes())).collect();
        let (nodes, _) = evaluate(&list);
        // m0 is the only root; each level adds one `module.n`.
        assert_eq!(nodes.len(), MODULE_DEPTH_MAX + 1);
        assert!(nodes.iter().all(|n| n.root == "m0" && n.ty == "module"));
        let deepest = nodes.last().unwrap();
        assert_eq!(
            deepest.address.matches("module.n").count(),
            MODULE_DEPTH_MAX + 1
        );
        // A module calling itself is a cycle: its call keeps no instance.
        let (nodes, _) = evaluate(&files(&[(
            "main.tf",
            "module \"me\" { source = \"./\" }\n",
        )]));
        assert_eq!(addresses(&nodes), ["module.me @."]);
    }

    #[test]
    fn a_root_past_the_node_ceiling_leaves_the_rest_out() {
        let text: String = (0..NODES_MAX + 5)
            .map(|i| format!("resource \"a\" \"r{i}\" {{}}\n"))
            .collect();
        let (nodes, _) = evaluate(&files(&[
            ("main.tf", &text),
            ("other/main.tf", "resource \"a\" \"b\" {}\n"),
        ]));
        assert_eq!(nodes.len(), NODES_MAX + 1);
        assert_eq!(nodes.last().unwrap().address, "a.b");
    }

    #[test]
    fn a_node_past_the_reference_ceiling_keeps_the_first_in_address_order() {
        let mut text: String = (0..REFERENCES_MAX + 5)
            .map(|i| format!("resource \"a\" \"r{i:03}\" {{}}\n"))
            .collect();
        let names: Vec<String> = (0..REFERENCES_MAX + 5)
            .map(|i| format!("a.r{i:03}"))
            .collect();
        text += &format!(
            "resource \"b\" \"all\" {{ depends_on = [{}] }}\n",
            names.join(", ")
        );
        let (_, references) = evaluate(&files(&[("main.tf", &text)]));
        assert_eq!(references.len(), REFERENCES_MAX);
        assert_eq!(references.last().unwrap().target, names[REFERENCES_MAX - 1]);
    }

    /// A module called twice by each level instantiates two to the power of its depth: the root's
    /// stored budget stops it, whatever the nesting ceiling allows.
    #[test]
    fn a_root_past_its_stored_budget_leaves_the_rest_out() {
        let mut list: Vec<(String, Vec<u8>)> = (0..MODULE_DEPTH_MAX)
            .map(|i| {
                let label = "k".repeat(2000);
                let calls = format!(
                    "module \"{label}a\" {{ source = \"../m{0}\" }}\nmodule \"{label}b\" {{ source = \"../m{0}\" }}\n",
                    i + 1
                );
                (format!("m{i}/main.tf"), calls.into_bytes())
            })
            .collect();
        list.push((
            format!("m{MODULE_DEPTH_MAX}/main.tf"),
            b"resource \"a\" \"b\" {}\n".to_vec(),
        ));
        let (nodes, _) = evaluate(&list);
        let stored: usize = nodes.iter().map(|n| n.address.len()).sum();
        assert!(stored <= ROOT_STORED_MAX, "{stored}");
        assert!(nodes.len() < NODES_MAX, "{}", nodes.len());
    }

    /// Roots that each call a module exploding under its stored budget still stop at the
    /// repository's: past `STORED_MAX` the later roots are left out.
    #[test]
    fn a_repository_past_its_stored_budget_leaves_the_rest_out() {
        let label = "k".repeat(2000);
        let mut list: Vec<(String, Vec<u8>)> = (0..MODULE_DEPTH_MAX)
            .map(|i| {
                let calls = format!(
                    "module \"{label}a\" {{ source = \"../m{0}\" }}\nmodule \"{label}b\" {{ source = \"../m{0}\" }}\n",
                    i + 1
                );
                (format!("m{i}/main.tf"), calls.into_bytes())
            })
            .collect();
        list.push((
            format!("m{MODULE_DEPTH_MAX}/main.tf"),
            b"resource \"a\" \"b\" {}\n".to_vec(),
        ));
        let roots = STORED_MAX / ROOT_STORED_MAX + 2;
        for r in 0..roots {
            let text = format!("module \"{label}\" {{ source = \"../m0\" }}\n");
            list.push((format!("r{r:02}/main.tf"), text.into_bytes()));
        }
        let (nodes, references) = evaluate(&list);
        let stored: usize = (nodes.iter().map(|n| n.address.len()))
            .chain(references.iter().map(|r| r.source.len() + r.target.len()))
            .sum();
        assert!(stored <= STORED_MAX, "{stored}");
        let of = |root: &str| nodes.iter().filter(|n| n.root == root).count();
        let last = format!("r{:02}", roots - 1);
        assert!(of(&last) * 10 < of("r00"), "{} of {}", of(&last), of("r00"));
    }

    /// A chain of locals as deep as `EVAL_DEPTH_MAX`, each naming the one before twice, read by
    /// many resources: each local resolves once per root, not twice per link. Measured on
    /// 2026-11-04 in a debug build: about 0.1 s, a fortieth of the bound.
    #[test]
    fn a_chain_of_locals_named_twice_resolves_in_linear_time() {
        let mut text = String::from("resource \"a\" \"t\" {}\nlocals {\n  l0 = a.t.id\n");
        for i in 1..EVAL_DEPTH_MAX {
            text += &format!("  l{i} = [local.l{}, local.l{}]\n", i - 1, i - 1);
        }
        text += "}\n";
        for i in 0..2000 {
            text += &format!(
                "resource \"b\" \"u{i}\" {{ x = local.l{} }}\n",
                EVAL_DEPTH_MAX - 1
            );
        }
        let start = std::time::Instant::now();
        let (_, references) = evaluate(&files(&[("main.tf", &text)]));
        let took = start.elapsed();
        assert_eq!(references.len(), 2000);
        assert!(took < std::time::Duration::from_secs(4), "{took:?}");
    }

    /// Directories that only call each other are no root (Terraform refuses such a cycle), so
    /// their blocks are no nodes; a root that calls into the cycle instantiates it once.
    #[test]
    fn directories_calling_only_each_other_are_no_root() {
        let a = "module \"b\" { source = \"../b\" }\nresource \"x\" \"a\" {}\n";
        let b = "module \"a\" { source = \"../a\" }\nresource \"x\" \"b\" {}\n";
        let (nodes, _) = evaluate(&files(&[("a/main.tf", a), ("b/main.tf", b)]));
        assert!(nodes.is_empty(), "{nodes:?}");
        let root = "module \"in\" { source = \"./a\" }\n";
        let (nodes, _) = evaluate(&files(&[
            ("main.tf", root),
            ("a/main.tf", a),
            ("b/main.tf", b),
        ]));
        assert_eq!(
            addresses(&nodes),
            [
                "module.in @.",
                "module.in.module.b @.",
                "module.in.module.b.module.a @.",
                "module.in.module.b.x.b @.",
                "module.in.x.a @.",
            ]
        );
    }

    /// Each node's address and physical name (` = "<name>"`, ` ~ "<pattern>"` for a pattern),
    /// then each overridable value its name went through.
    fn names(list: &[(&str, &str)]) -> Vec<String> {
        let (nodes, _) = evaluate(&files(list));
        (nodes.iter())
            .map(|n| {
                let name = match (&n.name, n.pattern) {
                    (None, _) => String::new(),
                    (Some(s), false) => format!(" = {s:?}"),
                    (Some(s), true) => format!(" ~ {s:?}"),
                };
                let through: String = (n.overridable.iter())
                    .map(|o| {
                        let from = if o.tfvars { " tfvars" } else { "" };
                        format!(" [{} {}:{}{from}]", o.variable, o.path, o.line)
                    })
                    .collect();
                format!("{}{name}{through}", n.address)
            })
            .collect()
    }

    #[test]
    fn the_pure_functions_resolve_names_and_any_other_call_stays_unresolved() {
        let text = concat!(
            "locals {\n  svc = \"orders\"\n  tags = { team = \"shop\" }\n}\n",
            "resource \"a\" \"format\" { name = format(\"%s-%03d-%%-%v\", local.svc, 7, true) }\n",
            "resource \"a\" \"formatlist\" { name = join(\",\", formatlist(\"q-%s\", [\"a\", \"b\"])) }\n",
            "resource \"a\" \"split\" { name = join(\"-\", concat([\"a\"], split(\",\", \"b,c\"))) }\n",
            "resource \"a\" \"case\" { name = \"${upper(local.svc)}${lower(\"X\")}${title(\"my queue\")}\" }\n",
            "resource \"a\" \"trim\" { name = \"${trim(\"--a--\", \"-\")}${trimsuffix(\"b.fifo\", \".fifo\")}${trimspace(\" c \")}${chomp(\"d\\n\")}\" }\n",
            "resource \"a\" \"substr\" { name = substr(trimprefix(\"prd-orders\", \"prd-\"), 0, 3) }\n",
            "resource \"a\" \"replace\" { name = replace(local.svc, \"o\", \"0\") }\n",
            "resource \"a\" \"lookup\" { name = lookup(merge(local.tags, { env = \"prd\" }), \"env\", \"x\") }\n",
            "resource \"a\" \"keys\" { name = coalesce(\"\", element(keys(local.tags), 0), \"z\") }\n",
            "resource \"a\" \"values\" { name = join(\"\", values(tomap(local.tags)), tolist(toset([\"b\", \"a\", \"b\"]))) }\n",
            "resource \"a\" \"length\" { name = \"${length(local.svc)}-${tostring(tonumber(\"2\") + 1)}\" }\n",
            "resource \"a\" \"json\" { name = jsonencode({ a = \"<b>\", n = [1, true] }) }\n",
            "resource \"a\" \"pattern\" { name = replace(local.svc, \"/o+/\", \"0\") }\n",
            "resource \"a\" \"file\" { name = \"q-${file(\"name.txt\")}\" }\n",
            "resource \"a\" \"verb\" { name = format(\"%x\", 10) }\n",
            "resource \"a\" \"wide\" { name = format(\"%257d\", 1) }\n",
        );
        assert_eq!(
            names(&[("main.tf", text)]),
            [
                "a.format = \"orders-007-%-true\"",
                "a.formatlist = \"q-a,q-b\"",
                "a.split = \"a-b-c\"",
                "a.case = \"ORDERSxMy Queue\"",
                "a.trim = \"abcd\"",
                "a.substr = \"ord\"",
                "a.replace = \"0rders\"",
                "a.lookup = \"prd\"",
                "a.keys = \"team\"",
                "a.values = \"shopab\"",
                "a.length = \"6-3\"",
                "a.json = \"{\\\"a\\\":\\\"\\\\u003cb\\\\u003e\\\",\\\"n\\\":[1,true]}\"",
                "a.pattern ~ \"*\"",
                "a.file ~ \"q-*\"",
                "a.verb ~ \"*\"",
                "a.wide ~ \"*\"",
            ]
        );
    }

    /// A template keeps its literal text and every part that resolves; what is never evaluated
    /// (a data source, a resource's attribute, a registry module, `terraform.workspace`, an
    /// undeclared or sensitive variable, a directive over one) is a `*`, and a call over it
    /// leaves the whole value unresolved.
    #[test]
    fn unresolved_parts_of_a_name_become_wildcards_of_a_pattern() {
        let text = concat!(
            "variable \"secret\" {\n  sensitive = true\n  default = \"leak\"\n}\n",
            "locals {\n  prefix = \"${terraform.workspace}-shop\"\n}\n",
            "resource \"a\" \"workspace\" { name = \"${local.prefix}-orders\" }\n",
            "resource \"a\" \"data\" { name = \"alerts-${data.x.y.id}-${data.x.y.n}\" }\n",
            "resource \"a\" \"attribute\" { name = \"${a.workspace.name}-dlq\" }\n",
            "module \"registry\" { source = \"terraform-aws-modules/sqs/aws\" }\n",
            "resource \"a\" \"registry\" { name = module.registry.queue_name }\n",
            "resource \"a\" \"undeclared\" { name = \"q-${var.nothing}\" }\n",
            "resource \"a\" \"secret\" { name = \"q-${var.secret}\" }\n",
            "resource \"a\" \"directive\" { name = \"%{ if var.nothing }a%{ endif }b\" }\n",
            "resource \"a\" \"call\" { name = upper(\"q-${terraform.workspace}\") }\n",
            "resource \"a\" \"heredoc\" {\n  name = <<EOT\nq-${local.prefix}\nEOT\n}\n",
            "resource \"a\" \"local\" { name = local.prefix }\n",
            "resource \"a\" \"none\" {}\n",
        );
        assert_eq!(
            names(&[("main.tf", text)]),
            [
                "a.workspace ~ \"*-shop-orders\"",
                "a.data ~ \"alerts-*-*\"",
                "a.attribute ~ \"*-dlq\"",
                "module.registry",
                "a.registry ~ \"*\"",
                "a.undeclared ~ \"q-*\"",
                "a.secret ~ \"q-*\"",
                "a.directive ~ \"*b\"",
                "a.call ~ \"*\"",
                "a.heredoc ~ \"q-*-shop\\n\"",
                "a.local ~ \"*-shop\"",
                "a.none",
            ]
        );
    }

    /// A root's variable is overridable by its default or by the value its var files give
    /// (`terraform.tfvars`, then each `*.auto.tfvars` in name order, the last winning); a local
    /// module's input carries what its argument went through, and a default its call leaves is
    /// fixed by the repository.
    #[test]
    fn a_name_records_the_overridable_values_it_went_through() {
        let main = concat!(
            "variable \"env\" {\n  default = \"dev\"\n}\n",
            "variable \"team\" {\n  default = \"shop\"\n}\n",
            "variable \"open\" {}\n",
            "locals {\n  full = \"${var.team}-orders\"\n}\n",
            "resource \"a\" \"q\" { name = \"${var.env}-${local.full}\" }\n",
            "resource \"a\" \"fixed\" { name = \"orders\" }\n",
            "resource \"a\" \"open\" { name = \"q-${var.open}\" }\n",
            "module \"m\" {\n  source = \"./m\"\n  name = \"x-${var.env}\"\n}\n",
            "resource \"a\" \"out\" { name = module.m.full }\n",
        );
        let module = concat!(
            "variable \"name\" {}\nvariable \"suffix\" {\n  default = \"q\"\n}\n",
            "variable \"env\" {\n  default = \"inner\"\n}\n",
            "resource \"b\" \"c\" { name = \"${var.name}-${var.suffix}-${var.env}\" }\n",
            "output \"full\" { value = \"${var.name}-out\" }\n",
        );
        let list = [
            ("main.tf", main),
            ("m/main.tf", module),
            (
                "terraform.tfvars",
                "env = \"stg\"\nteam = \"core\"\nopen = data.x\n",
            ),
            ("z.auto.tfvars", "env = \"prd\"\n"),
            ("a.auto.tfvars", "env = \"aaa\"\n"),
            ("m/terraform.tfvars", "suffix = \"ignored\"\n"),
        ];
        assert_eq!(
            names(&list),
            [
                "a.q = \"prd-core-orders\" [var.team terraform.tfvars:2 tfvars] [var.env z.auto.tfvars:1 tfvars]",
                "a.fixed = \"orders\"",
                "a.open ~ \"q-*\"",
                "module.m",
                "module.m.b.c = \"x-prd-q-inner\" [var.env z.auto.tfvars:1 tfvars]",
                "a.out = \"x-prd-out\" [var.env z.auto.tfvars:1 tfvars]",
            ]
        );
        let (nodes, _) = evaluate(&files(&[("main.tf", main), ("m/main.tf", module)]));
        let q = &nodes[0];
        let through: Vec<(&str, usize, bool)> = (q.overridable.iter())
            .map(|o| (o.variable.as_str(), o.line, o.tfvars))
            .collect();
        assert_eq!(q.name.as_deref(), Some("dev-shop-orders"));
        assert_eq!(through, [("var.env", 1, false), ("var.team", 4, false)]);
    }

    /// Terraform loads `terraform.tfvars`, then `terraform.tfvars.json`, then each `*.auto.tfvars`
    /// and `*.auto.tfvars.json` in name order, a later value winning; a JSON string is a literal.
    #[test]
    fn var_files_give_values_in_the_order_terraform_loads_them() {
        let main = concat!(
            "variable \"a\" {}\nvariable \"b\" {}\nvariable \"c\" {}\n",
            "resource \"x\" \"y\" { name = \"${var.a}-${var.b}-${var.c}\" }\n",
        );
        let list = [
            ("main.tf", main),
            (
                "terraform.tfvars",
                "a = \"hcl\"\nb = \"hcl\"\nc = \"hcl\"\n",
            ),
            (
                "terraform.tfvars.json",
                "{\n  \"b\": \"json\",\n  \"c\": \"${x}\"\n}\n",
            ),
            ("z.auto.tfvars.json", "{\"c\": \"auto\"}"),
            ("prod.tfvars", "a = \"ignored\"\n"),
        ];
        assert_eq!(
            names(&list),
            [concat!(
                "x.y = \"hcl-json-auto\" [var.a terraform.tfvars:1 tfvars] ",
                "[var.b terraform.tfvars.json:2 tfvars] [var.c z.auto.tfvars.json:1 tfvars]"
            )]
        );
    }

    /// Names built from a chain of locals as deep as `EVAL_DEPTH_MAX`, each a template naming the
    /// one before twice, for many resources: each local is evaluated once per root. Measured on
    /// 2026-11-04 in a debug build: well under a second, a tenth of the bound.
    #[test]
    fn names_through_a_chain_of_locals_evaluate_in_linear_time() {
        let mut text = String::from("locals {\n  l0 = \"a\"\n");
        for i in 1..EVAL_DEPTH_MAX / 2 {
            text += &format!(
                "  l{i} = \"${{local.l{}}}${{substr(local.l{}, 0, 1)}}\"\n",
                i - 1,
                i - 1
            );
        }
        text += "}\n";
        for i in 0..2000 {
            let last = EVAL_DEPTH_MAX / 2 - 1;
            text += &format!("resource \"b\" \"u{i}\" {{ name = \"${{local.l{last}}}-{i}\" }}\n");
        }
        let start = std::time::Instant::now();
        let (nodes, _) = evaluate(&files(&[("main.tf", &text)]));
        let took = start.elapsed();
        assert_eq!(nodes.len(), 2000);
        assert!(nodes.iter().all(|n| !n.pattern), "{:?}", nodes[0]);
        assert!(took < std::time::Duration::from_secs(8), "{took:?}");
    }

    /// `count` and `for_each` over a collection that resolves to literals give one node per
    /// instance, `each` and `count.index` naming it, and a module called with `for_each` has one
    /// instance per key, its arguments evaluated with its `each`.
    #[test]
    fn count_and_for_each_expand_over_values_that_resolve_to_literals() {
        let main = concat!(
            "variable \"regions\" {\n  default = [\"us\", \"eu\"]\n}\n",
            "locals {\n  queues = { orders = 1, billing = 2 }\n}\n",
            "resource \"a\" \"r\" {\n  for_each = toset(var.regions)\n  name = \"q-${each.key}\"\n}\n",
            "resource \"a\" \"m\" {\n  for_each = local.queues\n  name = \"${each.key}-${each.value}\"\n}\n",
            "resource \"a\" \"c\" {\n  count = length(var.regions) - 1\n  name = \"n-${count.index}\"\n}\n",
            "module \"m\" {\n  source = \"./m\"\n  for_each = toset([\"a\", \"b\"])\n  name = each.key\n}\n",
            "resource \"a\" \"open\" {\n  for_each = data.x.y.ids\n  name = each.key\n}\n",
            "resource \"a\" \"list\" {\n  for_each = [1, 2]\n}\n",
        );
        let list = [
            ("main.tf", main),
            (
                "m/main.tf",
                "variable \"name\" {}\nresource \"b\" \"c\" { name = var.name }\n",
            ),
        ];
        assert_eq!(
            names(&list),
            [
                "a.r[\"eu\"] = \"q-eu\" [var.regions main.tf:1]",
                "a.r[\"us\"] = \"q-us\" [var.regions main.tf:1]",
                "a.m[\"billing\"] = \"billing-2\"",
                "a.m[\"orders\"] = \"orders-1\"",
                "a.c[0] = \"n-0\" [var.regions main.tf:1]",
                "module.m[\"a\"]",
                "module.m[\"a\"].b.c = \"a\"",
                "module.m[\"b\"]",
                "module.m[\"b\"].b.c = \"b\"",
                "a.open ~ \"*\"",
                "a.list",
            ]
        );
    }

    /// A `for_each` over a resolved collection past `EXPANSION_MAX` is one node with no key.
    #[test]
    fn a_for_each_past_the_expansion_ceiling_is_one_node_with_no_key() {
        let text = |n: usize| {
            let items: Vec<String> = (0..n).map(|i| format!("\"k{i}\"")).collect();
            format!(
                "locals {{\n  all = [{}]\n}}\nresource \"a\" \"b\" {{ for_each = toset(local.all) }}\n",
                items.join(", ")
            )
        };
        let (nodes, _) = evaluate(&files(&[("main.tf", &text(EXPANSION_MAX))]));
        assert_eq!(nodes.len(), EXPANSION_MAX);
        let (nodes, _) = evaluate(&files(&[("main.tf", &text(EXPANSION_MAX + 1))]));
        assert_eq!(addresses(&nodes), ["a.b @."]);
    }

    /// A name through a chain of locals deeper than `EVAL_DEPTH_MAX` is unresolved, as is one
    /// through a cycle of locals; within it, it resolves.
    #[test]
    fn a_name_past_the_evaluation_depth_or_around_a_cycle_is_unresolved() {
        let chain = |n: usize| {
            let mut text = String::from("locals {\n  l0 = \"q\"\n");
            for i in 1..n {
                text += &format!("  l{i} = \"${{local.l{}}}\"\n", i - 1);
            }
            text + &format!("}}\nresource \"a\" \"b\" {{ name = local.l{} }}\n", n - 1)
        };
        assert_eq!(
            names(&[("main.tf", &chain(EVAL_DEPTH_MAX / 2))]),
            ["a.b = \"q\""]
        );
        assert_eq!(
            names(&[("main.tf", &chain(EVAL_DEPTH_MAX + 1))]),
            ["a.b ~ \"*\""]
        );
        let cycle = concat!(
            "locals {\n  a = \"${local.b}-x\"\n  b = local.a\n}\n",
            "resource \"a\" \"b\" { name = \"q-${local.a}\" }\n",
        );
        assert_eq!(names(&[("main.tf", cycle)]), ["a.b ~ \"q-*-x\""]);
    }

    /// A name passed down local modules resolves at every level up to `MODULE_DEPTH_MAX`; the
    /// call past it instantiates nothing, so its module's blocks are no nodes and name nothing.
    #[test]
    fn a_name_passed_down_modules_past_the_nesting_ceiling_stops_with_them() {
        let depth = MODULE_DEPTH_MAX + 2;
        let mut list: Vec<(String, String)> = vec![(
            "main.tf".into(),
            "module \"n\" {\n  source = \"./m1\"\n  name = \"q\"\n}\n".into(),
        )];
        for i in 1..=depth {
            let text = format!(
                "variable \"name\" {{}}\nresource \"a\" \"r\" {{ name = \"${{var.name}}-{i}\" }}\n\
                 module \"n\" {{\n  source = \"../m{}\"\n  name = var.name\n}}\n",
                i + 1
            );
            list.push((format!("m{i}/main.tf"), text));
        }
        let list: Vec<(&str, &str)> = list.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let resources: Vec<String> = (names(&list).into_iter())
            .filter(|n| n.contains(".a.r"))
            .collect();
        assert_eq!(resources.len(), MODULE_DEPTH_MAX);
        let deepest = resources.last().unwrap();
        assert!(
            deepest.ends_with(&format!(" = \"q-{MODULE_DEPTH_MAX}\"")),
            "{deepest}"
        );
    }

    /// A value past `EVAL_VALUE_MAX` (a local doubled at every step), an expression whose cost
    /// passes `EVAL_COST_MAX` (a `for` in a `for` in a `for` over a wide list) and the names past
    /// a root's `EVAL_WORK_MAX` are unresolved; below each ceiling the same forms resolve.
    #[test]
    fn values_past_their_size_cost_and_work_ceilings_are_unresolved() {
        let mut doubled = String::from("locals {\n  l0 = \"ab\"\n");
        for i in 1..20 {
            doubled += &format!("  l{i} = \"${{local.l{}}}${{local.l{}}}\"\n", i - 1, i - 1);
        }
        doubled += "}\nresource \"a\" \"small\" { name = length(local.l4) }\n";
        doubled += "resource \"a\" \"big\" { name = \"q-${local.l19}\" }\n";
        assert_eq!(
            names(&[("main.tf", &doubled)]),
            ["a.small = \"32\"", "a.big ~ \"q-*\""]
        );
        let cubed = |n: usize| {
            let items: Vec<String> = (0..n).map(|i| format!("\"k{i}\"")).collect();
            format!(
                "locals {{\n  l = [{}]\n}}\nresource \"a\" \"b\" {{ name = length(flatten([for a in local.l : [for b in local.l : [for c in local.l : 1]]])) }}\n\
                 resource \"a\" \"c\" {{ name = length([for a in local.l : [for b in local.l : [for c in local.l : 1]]]) }}\n",
                items.join(", ")
            )
        };
        assert_eq!(
            names(&[("main.tf", &cubed(3))]),
            ["a.b ~ \"*\"", "a.c = \"3\""]
        );
        assert_eq!(
            names(&[("main.tf", &cubed(200))]),
            ["a.b ~ \"*\"", "a.c ~ \"*\""]
        );
        let items: Vec<String> = (0..2000).map(|i| format!("\"item-{i:04}\"")).collect();
        let mut wide = format!("locals {{\n  big = [{}]\n}}\n", items.join(", "));
        let n = 3500;
        for i in 0..n {
            wide += &format!("resource \"a\" \"r{i:04}\" {{ name = length(local.big) }}\n");
        }
        let found = names(&[("main.tf", &wide)]);
        assert_eq!(found[0], "a.r0000 = \"2000\"");
        assert_eq!(found[n - 1], format!("a.r{:04} ~ \"*\"", n - 1));
    }

    #[test]
    fn a_unit_is_a_root_with_the_directories_of_its_local_module_calls() {
        let repository = Repository::read(&files(&[
            (
                "envs/prod/main.tf",
                "module \"q\" { source = \"../../modules/q\" }\n",
            ),
            ("envs/prod/terraform.tfvars", "x = 1\n"),
            ("modules/q/main.tf", "module \"w\" { source = \"../w\" }\n"),
            ("modules/w/main.tf", "resource \"a\" \"b\" {}\n"),
            ("main.tf", "module \"gone\" { source = \"./gone\" }\n"),
        ]));
        let units: Vec<(String, Vec<String>)> = (repository.units().into_iter())
            .map(|(root, unit)| (root, unit.into_iter().collect()))
            .collect();
        let unit = |root: &str, dirs: &[&str]| {
            (
                root.to_string(),
                dirs.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            units,
            [
                unit(".", &["", "gone"]),
                unit("envs/prod", &["envs/prod", "modules/q", "modules/w"]),
            ]
        );
    }

    #[test]
    fn a_file_that_does_not_parse_adds_no_node() {
        let (nodes, _) = evaluate(&files(&[
            ("broken.tf", "resource \"a\" \"b\" {\n"),
            ("ok.tf", "resource \"a\" \"c\" {}\n"),
        ]));
        assert_eq!(addresses(&nodes), ["a.c @."]);
    }
}
