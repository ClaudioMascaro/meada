use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use hcl::eval::{Context, Evaluate};
use hcl::{Map, Value};
use hcl_edit::Span;
use hcl_edit::expr::{Expression, ObjectKey, Traversal, TraversalOperator};
use hcl_edit::parser;
use hcl_edit::structure::{Body, Structure};
use hcl_edit::template::{Directive, Element, Strip, Template};
use hcl_edit::visit::{self, Visit};

use self::functions::Elem;
use hcl_edit::visit_mut::{self, VisitMut};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::config_values;
use crate::contract::Hit;
use crate::document::events::{self, Step, Syntax};
use crate::lang::MAX_BYTES;
use crate::render::{rank, section};
use crate::trace::Direction;
use meada_core::export::{Level, Link};
use meada_core::trace::{Chain, Row, arrow};

pub(crate) mod actions;
mod attribute;
mod azure;
pub(crate) mod cloudformation;
pub(crate) mod compose;
pub(crate) mod config_uses;
pub(crate) mod environment;
mod functions;
mod gcp;
mod http;
pub(crate) mod json;
pub(crate) mod mapping;
pub(crate) mod orphans;
pub(crate) mod pattern;
pub(crate) mod provision;
pub(crate) mod serverless;
pub(crate) mod subscription;
pub(crate) mod types;
pub(crate) mod workload;

// ponytail: a fleet of a few hundred instances is real; past a thousand the keys come from data.
const EXPANSION_MAX: usize = 1000;

// ponytail: real module trees nest three or four levels; 16 stops a chain built to run on.
const MODULE_DEPTH_MAX: usize = 16;

// ponytail: a name passed down a few modules goes through a handful; 32 stops a chain of locals.
const EVAL_DEPTH_MAX: usize = 32;

// ponytail: a name goes through a handful of variables or parameters; 32 stops one joined from
// thousands of them, whose list every name built on it would copy.
const OVERRIDABLE_MAX: usize = 32;

// ponytail: a name, a `for_each` map or a list of regions holds far less; 64 KiB stops a local
// doubled at every step.
const EVAL_VALUE_MAX: usize = 64 << 10;

// ponytail: a `for` over a map of a few hundred entries costs a few hundred KiB; 1 MiB stops a
// `for` inside a `for` over wide lists, and a template in a `for` before it takes a second.
const EVAL_COST_MAX: usize = 1 << 20;

// ponytail: measured on 2026-11-14 in a debug build (CPU time of 20 resources near the ceiling,
// against one part per element), a part takes about 1 µs (a literal, an `if` directive) to 1.7 µs
// (an interpolation, a call of `meada_exact`), where the other steps take about 0.3 µs a unit of
// cost; 8 covers the slowest form.
const TEMPLATE_PART_COST: usize = 8;

// ponytail: 64 MiB is thousands of times what a real root spends; it bounds a module instanced
// thousands of times with costly locals.
const EVAL_WORK_MAX: usize = 64 * MAX_BYTES;

// ponytail: a large root holds a few thousand blocks with its modules; 10,000 is past real ones.
const NODES_MAX: usize = 10_000;

// ponytail: a block names a handful of others; 64 is past a wide `depends_on`.
const REFERENCES_MAX: usize = 64;

// ponytail: four times the per-file ceiling holds a root of a few thousand nodes; past it the
// evaluation is built to explode.
const ROOT_STORED_MAX: usize = 4 * MAX_BYTES;

// ponytail: sixteen times the per-file ceiling holds a monorepo of environments; many roots
// calling one exploding module would pass it.
const STORED_MAX: usize = 16 * MAX_BYTES;

pub(crate) fn is_source(path: &str) -> bool {
    path.ends_with(".tf")
        || path.ends_with(".tofu")
        || json::is_file(path)
        || compose::is_file(path)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Resolved {
    pub text: String,
    pub overridable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Dockerfile {
    pub text: String,
    pub overridable: bool,
    pub inline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkloadImage {
    pub container: String,
    pub image: Resolved,
    pub through: Vec<Overridable>,
}

impl WorkloadImage {
    pub(crate) fn size(&self) -> usize {
        self.container.len()
            + self.image.text.len()
            + self
                .through
                .iter()
                .map(|o| o.path.len() + o.variable.len() + 16)
                .sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct Projection {
    pub name: Option<String>,
    pub pattern: bool,
    pub through: Vec<Overridable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defining: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<mapping::Code>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<mapping::Binding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Ingress {
    pub host: String,
    pub prefix: String,
    pub service: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Route {
    pub method: Resolved,
    pub path: Resolved,
    pub at: String,
}

impl Route {
    pub(crate) fn size(&self) -> usize {
        self.method.text.len() + self.path.text.len() + self.at.len()
    }
}

impl Projection {
    pub(crate) fn size(&self) -> usize {
        self.name.as_ref().map_or(0, String::len)
            + self.entry.as_ref().map_or(0, String::len)
            + self.defining.iter().map(String::len).sum::<usize>()
            + self.code.as_ref().map_or(0, |c| c.key.len() + 24)
            + self.binding.as_ref().map_or(0, mapping::Binding::size)
            + self
                .through
                .iter()
                .map(|o| o.path.len() + o.variable.len() + 16)
                .sum::<usize>()
    }
}

fn is_var_file(path: &str) -> bool {
    var_file_rank(path).is_some()
}

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

pub(crate) fn is_input(path: &str) -> bool {
    is_source(path) || is_var_file(path)
}

pub(super) fn is_resource_block(block: &str) -> bool {
    block
        .strip_prefix("resource")
        .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('['))
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Node {
    pub address: String,
    pub root: String,
    pub ty: String,
    pub name: Option<String>,
    pub pattern: bool,
    pub overridable: Vec<Overridable>,
    pub path: String,
    pub line: usize,
    pub end_line: usize,
    pub block: String,
    pub image: Option<Resolved>,
    pub build: Option<Resolved>,
    pub build_image: bool,
    pub dockerfile: Option<Dockerfile>,
    pub role: Option<String>,
    pub handler: Option<Resolved>,
    pub events: Vec<Resolved>,
    pub routes: Vec<Route>,
    pub subscription: Option<subscription::Subscription>,
    pub environment: Vec<environment::Entry>,
    pub images: Vec<WorkloadImage>,
    pub provisioned: Option<Projection>,
    pub ingress: Vec<Ingress>,
    pub actions: Option<actions::Facts>,
}

impl Node {
    pub(crate) fn workload_size(&self) -> usize {
        self.routes.iter().map(Route::size).sum::<usize>()
            + self.images.iter().map(WorkloadImage::size).sum::<usize>()
            + self
                .environment
                .iter()
                .map(environment::Entry::size)
                .sum::<usize>()
            + self.build.as_ref().map_or(0, |p| p.text.len())
            + self.dockerfile.as_ref().map_or(0, |d| d.text.len())
            + self.handler.as_ref().map_or(0, |p| p.text.len())
            + self.provisioned.as_ref().map_or(0, Projection::size)
            + self
                .ingress
                .iter()
                .map(|i| i.host.len() + i.prefix.len() + i.service.len())
                .sum::<usize>()
    }
    pub(crate) fn identity(&self, repository: &str) -> String {
        format!("{repository}::{}::{}", self.path, self.address)
    }

    pub(crate) fn sides_first(&self) -> bool {
        actions::owns(self)
    }

    pub(crate) fn span(&self, source: Option<&[u8]>) -> crate::index::Span {
        let mut span = crate::index::Span::default();
        span.path = self.path.clone();
        if let Some(source) = source {
            let lengths = source
                .split_inclusive(|&b| b == b'\n')
                .map(<[u8]>::len)
                .collect::<Vec<_>>();
            span.start = lengths
                .iter()
                .take(self.line.saturating_sub(1))
                .sum::<usize>() as i64;
            span.end = lengths.iter().take(self.end_line).sum::<usize>() as i64;
        }
        span
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct Overridable {
    pub path: String,
    pub line: usize,
    pub variable: String,
    pub tfvars: bool,
}

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

struct Block {
    kind: Kind,
    ty: String,
    name: String,
    path: String,
    line: usize,
    end_line: usize,
    json_path: Option<String>,
    named: Option<Expression>,
    count: Option<Expression>,
    for_each: Option<Expression>,
    refs: BTreeMap<String, Vec<Ref>>,
    source: Option<String>,
    args: BTreeMap<String, Named>,
    nested: Vec<hcl_edit::structure::Block>,
}

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

#[derive(Clone, Copy, PartialEq)]
enum Declared {
    Set(Elem),
    List(Elem),
    Any,
    Other,
}

impl Declared {
    fn text(text: &str) -> Declared {
        let text = text.trim();
        let inner = |prefix: &str| {
            let rest = text.strip_prefix(prefix)?;
            Some(Elem::named(rest.strip_suffix(')')?))
        };
        match (inner("set("), inner("list(")) {
            (Some(elem), _) => Declared::Set(elem),
            (_, Some(elem)) => Declared::List(elem),
            _ if text == "any" => Declared::Any,
            _ => Declared::Other,
        }
    }

    fn expr(expr: &Expression) -> Declared {
        let elem = |f: &hcl_edit::expr::FuncCall| match f.args.iter().next() {
            Some(Expression::Variable(v)) => Elem::named(v.as_str()),
            _ => Elem::Other,
        };
        match expr {
            Expression::FuncCall(f) if f.name.name.as_str() == "set" => Declared::Set(elem(f)),
            Expression::FuncCall(f) if f.name.name.as_str() == "list" => Declared::List(elem(f)),
            Expression::Variable(v) if v.as_str() == "any" => Declared::Any,
            _ => Declared::Other,
        }
    }

    fn converts(self) -> bool {
        match self {
            Declared::Set(_) => true,
            Declared::List(elem) => elem.primitive(),
            Declared::Any | Declared::Other => false,
        }
    }
}

fn converted_through(mut value: Value, steps: &[Declared]) -> Option<Value> {
    let mut set = false;
    for step in steps {
        match *step {
            Declared::Set(elem) => {
                value = functions::to_set(&value, elem).ok()?;
                set = true;
            }
            Declared::List(elem) if set || elem.primitive() => {
                value = functions::to_list(&value, elem).ok()?;
                set = false;
            }
            Declared::List(_) => {}
            Declared::Any => {}
            Declared::Other if set => return None,
            Declared::Other => {}
        }
    }
    Some(value)
}

fn ends_in_set(steps: &[Declared]) -> bool {
    let last = steps.iter().rev().find(|t| **t != Declared::Any);
    matches!(last, Some(Declared::Set(_)))
}

fn declared(found: Evaluated, steps: &[Declared]) -> Evaluated {
    match found.val {
        Val::Known(v) => Evaluated {
            val: converted_through(v, steps).map_or(Val::Unknown, Val::Known),
            through: found.through,
        },
        _ => found,
    }
}

fn converted_once(value: Value, ty: Option<Declared>) -> Option<Value> {
    match ty {
        Some(Declared::Set(elem)) => functions::to_set(&value, elem).ok(),
        Some(Declared::List(elem)) => functions::to_list(&value, elem).ok(),
        _ => Some(value),
    }
}

fn declared_default(found: Evaluated, variable: &Variable) -> Evaluated {
    let converts = (variable.through.iter()).any(|t| t.converts());
    let (true, true, Val::Known(v)) = (variable.opentofu, converts, &found.val) else {
        return declared(found, &variable.through);
    };
    let (twice, once) = (
        converted_through(v.clone(), &variable.through),
        converted_once(v.clone(), variable.ty),
    );
    let same_kind = ends_in_set(&variable.through) == ends_in_set(variable.ty.as_slice());
    let agree = match (variable.through.iter()).any(|t| matches!(t, Declared::Set(_))) {
        true => twice == once,
        false => twice.as_ref().map(spelled) == once.as_ref().map(spelled),
    };
    Evaluated {
        val: (once.filter(|_| agree && same_kind)).map_or(Val::Unknown, Val::Known),
        through: found.through,
    }
}

fn spelled(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(spelled).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), spelled(v))).collect())
        }
        scalar => functions::shown(scalar).map_or_else(|| scalar.clone(), Value::String),
    }
}

#[derive(Clone)]
struct Variable {
    default: Option<Expression>,
    ty: Option<Declared>,
    type_text: Option<String>,
    through: Vec<Declared>,
    opentofu: bool,
    sensitive: bool,
    path: String,
    line: usize,
    terraform: Option<Box<Variable>>,
    terraform_only: bool,
}

impl Variable {
    fn merge(&mut self, mut variable: Variable, overriding: bool) {
        variable.sensitive |= self.sensitive;
        variable.opentofu = self.opentofu | config_values::only_opentofu(&variable.path);
        self.opentofu = variable.opentofu;
        let (own, merged) = (variable.ty, variable.ty.or(self.ty));
        let type_text = (variable.type_text.clone()).or_else(|| self.type_text.clone());
        if overriding && variable.default.is_none() {
            self.sensitive = variable.sensitive;
            self.through.extend(own);
            self.ty = merged;
            self.type_text = type_text;
        } else {
            variable.through = own
                .into_iter()
                .chain(merged.filter(|_| overriding))
                .collect();
            variable.ty = merged;
            variable.type_text = type_text;
            variable.terraform = self.terraform.take();
            *self = variable;
        }
    }

    fn opentofu_refuses(&self) -> bool {
        let number = matches!(
            self.ty,
            Some(Declared::List(Elem::Number) | Declared::Set(Elem::Number))
        );
        let Some(Expression::Array(items)) = self.default.as_ref().filter(|_| number) else {
            return false;
        };
        (items.iter())
            .any(|item| matches!(item, Expression::String(s) if s.value().parse::<f64>().is_err()))
    }
}

struct Given {
    value: Option<Value>,
    path: String,
    line: usize,
    rank: (u8, String),
}

impl Module {
    fn declare(&mut self, name: &str, mut variable: Variable, overriding: bool) {
        let tofu = config_values::only_opentofu(&variable.path);
        if let Some(declared) = self.variables.get_mut(name) {
            let across_tools = !overriding && tofu != config_values::only_opentofu(&declared.path);
            match (across_tools, tofu) {
                (true, false) => {
                    variable.terraform_only = true;
                    variable.sensitive |= declared.sensitive;
                }
                (_, true) if across_tools || declared.terraform_only => {
                    declared.terraform_only = true;
                    declared.sensitive |= variable.sensitive;
                    return;
                }
                _ => {
                    if tofu && !declared.opentofu {
                        declared.terraform = Some(Box::new(declared.clone()));
                    } else if let Some(terraform) = declared.terraform.as_mut().filter(|_| !tofu) {
                        terraform.merge(variable.clone(), overriding);
                    }
                    declared.merge(variable, overriding);
                    return;
                }
            }
        } else if overriding {
            return;
        }
        variable.through = variable.default.iter().flat_map(|_| variable.ty).collect();
        variable.opentofu = tofu;
        self.variables.insert(name.to_string(), variable);
    }

    fn settle(&mut self) {
        for variable in self.variables.values_mut() {
            if variable.opentofu_refuses()
                && let Some(terraform) = variable.terraform.take()
            {
                *variable = *terraform;
            }
        }
    }

    fn add(&mut self, block: Block, overriding: bool) {
        if !overriding {
            self.blocks.push(block);
            return;
        }
        if self.addresses.is_empty() {
            self.addresses = (self.blocks.iter().enumerate())
                .map(|(i, b)| (b.local(), i))
                .collect();
        }
        if let Some(&i) = self.addresses.get(&block.local()) {
            self.blocks[i].merge(block);
        }
    }
}

fn assign(map: &mut BTreeMap<String, Named>, name: &str, value: Named, overriding: bool) {
    if !overriding || map.contains_key(name) {
        map.insert(name.to_string(), value);
    }
}

fn nested_type(b: &hcl_edit::structure::Block) -> &str {
    match (b.ident.as_str(), b.labels.first()) {
        ("dynamic", Some(label)) => label.as_str(),
        (ident, _) => ident,
    }
}

fn keyed_refs(body: &Body) -> BTreeMap<String, Vec<Ref>> {
    let mut keyed: BTreeMap<String, Vec<Ref>> = BTreeMap::new();
    for structure in body.iter() {
        let mut refs = Refs::default();
        let key = match structure {
            Structure::Attribute(a) => {
                refs.visit_expr(&a.value);
                a.key.as_str()
            }
            Structure::Block(b) => {
                refs.visit_block(b);
                nested_type(b)
            }
        };
        keyed.entry(key.to_string()).or_default().extend(refs.0);
    }
    keyed
}

impl Block {
    fn merge(&mut self, o: Block) {
        self.named = o.named.or(self.named.take());
        self.count = o.count.or(self.count.take());
        self.for_each = o.for_each.or(self.for_each.take());
        self.source = o.source.or(self.source.take());
        self.args.extend(o.args);
        self.refs.extend(o.refs);
        let types: HashSet<String> = (o.nested.iter())
            .map(|b| nested_type(b).to_string())
            .collect();
        self.nested.retain(|b| !types.contains(nested_type(b)));
        self.nested.extend(o.nested);
    }

    fn local(&self) -> String {
        match self.kind {
            Kind::Resource => format!("{}.{}", self.ty, self.name),
            Kind::Data => format!("data.{}.{}", self.ty, self.name),
            Kind::Module => format!("module.{}", self.name),
        }
    }

    fn key_path(&self) -> String {
        if let Some(path) = &self.json_path {
            return path.clone();
        }
        let steps: Vec<&str> = match self.kind {
            Kind::Resource => vec!["resource", &self.ty, &self.name],
            Kind::Data => vec!["data", &self.ty, &self.name],
            Kind::Module => vec!["module", &self.name],
        };
        let steps: Vec<Step> = steps.into_iter().map(|s| Step::Key(s.into())).collect();
        config_values::key_path(&steps)
    }
}

#[derive(Default)]
struct Module {
    blocks: Vec<Block>,
    outputs: BTreeMap<String, Named>,
    locals: BTreeMap<String, Named>,
    variables: BTreeMap<String, Variable>,
    given: BTreeMap<String, Given>,
    addresses: HashMap<String, usize>,
    overridden: bool,
    unreadable: bool,
    unread: Option<(u8, String)>,
}

fn local_source(dir: &str, source: &str) -> Option<String> {
    if !(source.starts_with("./") || source.starts_with("../")) {
        return None;
    }
    joined(dir, source)
}

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

fn reference(root: &str, ops: &[&TraversalOperator]) -> Option<Ref> {
    let attr = |at: usize| match ops.get(at) {
        Some(TraversalOperator::GetAttr(i)) => Some(i.as_str().to_string()),
        _ => None,
    };
    let keyed = |at: usize| -> (Option<Key>, usize) {
        match ops.get(at) {
            Some(TraversalOperator::Index(Expression::Number(n))) => {
                (n.value().as_u64().map(Key::Int), at + 1)
            }
            Some(TraversalOperator::Index(Expression::String(s))) => {
                (Some(Key::Str(functions::nfc(s.value().clone()))), at + 1)
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

fn lines(text: &str) -> impl Fn(usize) -> usize {
    let newlines: Vec<usize> = (text.bytes().enumerate())
        .filter(|&(_, b)| b == b'\n')
        .map(|(i, _)| i)
        .collect();
    move |offset| newlines.partition_point(|&n| n < offset) + 1
}

fn evaluated(expr: &Expression, ctx: &Context, sets: &BTreeSet<String>) -> Option<Value> {
    evaluation(expr, ctx, sets).0
}

fn evaluation(expr: &Expression, ctx: &Context, sets: &BTreeSet<String>) -> (Option<Value>, bool) {
    let mut exact = Exact {
        ctx,
        sets,
        left: 0,
        inexact: false,
        unordered: 0,
    };
    let value = quiet(|| {
        let mut expr = expr.clone();
        exact.visit_expr_mut(&mut expr);
        hcl::Expression::from(expr).evaluate(ctx).ok()
    })
    .flatten();
    (value, exact.inexact)
}

struct Exact<'a, 'c> {
    ctx: &'a Context<'c>,
    sets: &'a BTreeSet<String>,
    left: usize,
    inexact: bool,
    unordered: usize,
}

fn is_set(expr: &Expression, named: &mut dyn FnMut(&Ref) -> bool) -> bool {
    match expr {
        Expression::FuncCall(f) => f.name.name.as_str() == "toset",
        Expression::Parenthesis(p) => is_set(p.inner(), named),
        Expression::StringTemplate(t) if t.len() == 1 => match t.iter().next() {
            Some(Element::Interpolation(i)) => is_set(&i.expr, named),
            _ => false,
        },
        Expression::Conditional(c) => is_set(&c.true_expr, named) || is_set(&c.false_expr, named),
        Expression::Traversal(_) => lone_ref(expr).is_some_and(|r| named(&r)),
        _ => false,
    }
}

fn lone_ref(expr: &Expression) -> Option<Ref> {
    let Expression::Traversal(t) = expr else {
        return None;
    };
    let (Expression::Variable(root), [op]) = (&t.expr, &t.operators[..]) else {
        return None;
    };
    reference(root.as_str(), &[op.value()]).filter(|r| matches!(r, Ref::Var(_) | Ref::Local(_)))
}

impl Exact<'_, '_> {
    fn keyed_set(&self, key: bool, collection: &Expression) -> bool {
        key && is_set(collection, &mut |r| {
            r.via().is_some_and(|v| self.sets.contains(&v))
        })
    }
}

impl VisitMut for Exact<'_, '_> {
    fn visit_string_template_mut(&mut self, node: &mut hcl_edit::template::StringTemplate) {
        if node.len() == 1
            && let Some(Element::Interpolation(sole)) = node.iter_mut().next()
        {
            return self.visit_expr_mut(&mut sole.expr);
        }
        visit_mut::visit_string_template_mut(self, node);
    }

    fn visit_interpolation_mut(&mut self, node: &mut hcl_edit::template::Interpolation) {
        self.visit_expr_mut(&mut node.expr);
        let expr = std::mem::replace(&mut node.expr, undefined());
        let name = hcl_edit::expr::FuncName::new(hcl_edit::Ident::new("meada_exact"));
        let args = vec![Expression::from("text"), expr];
        node.expr = Expression::from(hcl_edit::expr::FuncCall::new(name, args.into()));
    }

    fn visit_for_template_expr_mut(&mut self, node: &mut hcl_edit::template::ForTemplateExpr) {
        let unordered = self.unordered;
        let keyed = self.keyed_set(node.key_var.is_some(), &node.collection_expr);
        self.visit_expr_mut(&mut node.collection_expr);
        if self.unordered > unordered || keyed {
            self.inexact = true;
            node.collection_expr = undefined();
        }
        visit_mut::visit_for_template_expr_mut(self, node);
    }

    fn visit_for_intro_mut(&mut self, node: &mut hcl_edit::expr::ForIntro) {
        let unordered = self.unordered;
        let keyed = self.keyed_set(node.key_var.is_some(), &node.collection_expr);
        visit_mut::visit_for_intro_mut(self, node);
        if self.unordered > unordered || keyed {
            self.inexact = true;
            node.collection_expr = undefined();
        }
    }

    fn visit_expr_mut(&mut self, expr: &mut Expression) {
        use hcl_edit::expr::{BinaryOperator as B, UnaryOperator};
        let left = self.left;
        visit_mut::visit_expr_mut(self, expr);
        let (operator, operands) = match expr {
            Expression::FuncCall(f) if f.name.name.as_str() == "meada_exact" => {
                *expr = undefined();
                return;
            }
            Expression::StringTemplate(t) => {
                *expr = template_expr(Template::from(std::mem::take(t)));
                return;
            }
            Expression::HeredocTemplate(h) => {
                *expr = template_expr(std::mem::take(&mut h.template));
                return;
            }
            Expression::String(s) => {
                if !s.is_ascii() {
                    *expr = Expression::from(functions::nfc(s.value().clone()));
                }
                return;
            }
            Expression::Number(n) => {
                if !(n.as_repr()).is_some_and(|written| printed_as_written(n.value(), written)) {
                    self.inexact = true;
                    *expr = undefined();
                }
                return;
            }
            Expression::Object(object) => {
                let mut entries: Vec<_> = std::mem::take(object).into_iter().collect();
                for (key, _) in &mut entries {
                    match key {
                        ObjectKey::Expression(key) => self.visit_expr_mut(key),
                        ObjectKey::Ident(i) if !i.is_ascii() => {
                            let name = functions::nfc(i.as_str().to_string());
                            *key = ObjectKey::Expression(Expression::from(name));
                        }
                        ObjectKey::Ident(_) => {}
                    }
                }
                let keys: Option<Vec<String>> = (entries.iter())
                    .map(|(k, _)| match k {
                        ObjectKey::Ident(i) => Some(i.as_str().to_string()),
                        ObjectKey::Expression(Expression::String(s)) => {
                            Some(s.as_str().to_string())
                        }
                        ObjectKey::Expression(_) => None,
                    })
                    .collect();
                match keys {
                    Some(keys) => {
                        let mut keyed: Vec<_> = keys.into_iter().zip(entries).collect();
                        keyed.sort_by(|a, b| a.0.cmp(&b.0));
                        entries = keyed.into_iter().map(|(_, e)| e).collect();
                    }
                    None => self.unordered += 1,
                }
                *object = entries.into_iter().collect();
                return;
            }
            Expression::ForExpr(f) if f.key_expr.is_some() => {
                self.unordered += 1;
                return;
            }
            Expression::BinaryOp(op)
                if matches!(
                    op.operator.value(),
                    B::Plus | B::Minus | B::Mul | B::Div | B::Mod
                ) =>
            {
                (Some(*op.operator.value()), vec![&op.lhs_expr, &op.rhs_expr])
            }
            Expression::UnaryOp(op) if *op.operator.value() == UnaryOperator::Neg => {
                (None, vec![&op.expr])
            }
            _ => return,
        };
        let values = (self.left == left)
            .then(|| {
                operands
                    .iter()
                    .map(|e| self.integer(e))
                    .collect::<Option<Vec<_>>>()
            })
            .flatten();
        let Some(values) = values else {
            self.left += 1;
            self.inexact = true;
            let operator = operator.map_or("neg", |o| o.as_str());
            let args = [Expression::from(operator)].into_iter();
            let args = args
                .chain(operands.into_iter().cloned())
                .collect::<Vec<_>>();
            let name = hcl_edit::expr::FuncName::new(hcl_edit::Ident::new("meada_exact"));
            *expr = Expression::from(hcl_edit::expr::FuncCall::new(name, args.into()));
            return;
        };
        let number = (values.into_iter().collect::<Option<Vec<i128>>>())
            .and_then(|v| arithmetic(operator, &v));
        *expr = match number {
            Some(n) => Expression::from(n),
            None => {
                self.inexact = true;
                undefined()
            }
        };
    }
}

fn undefined() -> Expression {
    Expression::from(hcl_edit::Ident::new_unchecked("meada inexact"))
}

fn arithmetic(
    operator: Option<hcl_edit::expr::BinaryOperator>,
    values: &[i128],
) -> Option<hcl::Number> {
    use hcl_edit::expr::BinaryOperator as B;
    let n = match (operator, values) {
        (None, &[a]) => a.checked_neg(),
        (Some(B::Plus), &[a, b]) => a.checked_add(b),
        (Some(B::Minus), &[a, b]) => a.checked_sub(b),
        (Some(B::Mul), &[a, b]) => a.checked_mul(b),
        (Some(B::Div), &[a, b]) => (b != 0 && a % b == 0).then(|| a / b),
        (Some(B::Mod), &[a, b]) => a.checked_rem(b),
        _ => None,
    }?;
    let negative_zero = match operator {
        None => true,
        Some(B::Mul | B::Div) => values.iter().any(|&v| v < 0),
        _ => false,
    };
    if n == 0 && negative_zero {
        return None;
    }
    (i64::try_from(n).map(hcl::Number::from))
        .or_else(|_| u64::try_from(n).map(hcl::Number::from))
        .ok()
}

fn template_expr(t: Template) -> Expression {
    let mut elements: Vec<Element> = t.into_iter().collect();
    if let [Element::Interpolation(_)] = elements.as_slice()
        && let Some(Element::Interpolation(sole)) = elements.pop()
    {
        return sole.expr;
    }
    template_call(elements, Strip::None, Strip::None)
}

fn template_call(elements: Vec<Element>, prev: Strip, next: Strip) -> Expression {
    use hcl_edit::expr::{Conditional, ForExpr, ForIntro, FuncCall, FuncName};
    let strips: Vec<Strip> = elements.iter().map(element_strip).collect();
    let last = elements.len().saturating_sub(1);
    let mut args = vec![Expression::from("template")];
    for (i, element) in elements.into_iter().enumerate() {
        let before = if i == 0 { prev } else { strips[i - 1] };
        let after = if i == last { next } else { strips[i + 1] };
        args.push(match element {
            Element::Literal(s) => Expression::from(stripped(s.value(), before, after)),
            Element::Interpolation(interpolation) => interpolation.expr,
            Element::Directive(d) => match *d {
                Directive::If(d) => {
                    let (yes, no, endif) = (d.if_expr, d.else_expr, d.endif_expr.strip);
                    let yes_next = no.as_ref().map_or(endif, |no| no.strip);
                    let yes_text =
                        template_call(yes.template.into_iter().collect(), yes.strip, yes_next);
                    let no_text = match no {
                        Some(no) => {
                            template_call(no.template.into_iter().collect(), no.strip, endif)
                        }
                        None => Expression::from(""),
                    };
                    Expression::from(Conditional::new(yes.cond_expr, yes_text, no_text))
                }
                Directive::For(d) => {
                    let (f, endfor) = (d.for_expr, d.endfor_expr.strip);
                    let body = template_call(f.template.into_iter().collect(), f.strip, endfor);
                    let mut intro = ForIntro::new(f.value_var, f.collection_expr);
                    intro.key_var = f.key_var;
                    Expression::from(ForExpr::new(intro, body))
                }
            },
        });
    }
    let name = FuncName::new(hcl_edit::Ident::new("meada_exact"));
    Expression::from(FuncCall::new(name, args.into()))
}

fn element_strip(e: &Element) -> Strip {
    match e {
        Element::Literal(_) => Strip::None,
        Element::Interpolation(i) => i.strip,
        Element::Directive(d) => {
            let (start, end) = match d.as_ref() {
                Directive::If(d) => (d.if_expr.strip, d.endif_expr.strip),
                Directive::For(d) => (d.for_expr.strip, d.endfor_expr.strip),
            };
            Strip::from((start.strip_start(), end.strip_end()))
        }
    }
}

fn stripped(literal: &str, before: Strip, after: Strip) -> String {
    let space = |c: char| c.is_whitespace() && c != '\r' && c != '\n';
    let mut s = literal;
    if before.strip_end() {
        s = s.trim_start_matches(space);
        s = (s.strip_prefix("\r\n").or_else(|| s.strip_prefix('\n'))).unwrap_or(s);
    }
    if after.strip_start() {
        s = s.trim_end_matches(space);
        s = (s.strip_suffix("\r\n").or_else(|| s.strip_suffix('\n'))).unwrap_or(s);
    }
    s.to_string()
}

fn exact_call(args: hcl::eval::FuncArgs) -> std::result::Result<Value, String> {
    let operator = match args[0].as_str() {
        Some("text") => {
            return match &args[1] {
                Value::Null | Value::Array(_) | Value::Object(_) => Err("no text".into()),
                v => Ok(v.clone()),
            };
        }
        Some("template") => {
            let mut text = String::new();
            for part in args.iter().skip(1) {
                match part {
                    Value::Array(texts) => {
                        for t in texts {
                            text.push_str(t.as_str().ok_or("no text")?);
                        }
                    }
                    v => text.push_str(&functions::shown(v).ok_or("no text")?),
                }
            }
            return Ok(Value::String(functions::nfc(text)));
        }
        Some("neg") => None,
        Some(o) => Some(o.parse().map_err(|_| "no operator")?),
        None => return Err("no operator".into()),
    };
    let integer = |v: &Value| match v {
        Value::Number(n) if !n.is_f64() => {
            n.as_i64().map(i128::from).or(n.as_u64().map(i128::from))
        }
        _ => None,
    };
    let values = (args.iter().skip(1))
        .map(integer)
        .collect::<Option<Vec<_>>>();
    (values.and_then(|v| arithmetic(operator, &v)))
        .map(Value::Number)
        .ok_or_else(|| "arithmetic Meada cannot do exactly".into())
}

impl Exact<'_, '_> {
    fn integer(&self, expr: &Expression) -> Option<Option<i128>> {
        match hcl::Expression::from(expr.clone())
            .evaluate(self.ctx)
            .ok()?
        {
            Value::Number(n) if !n.is_f64() => {
                Some(n.as_i64().map(i128::from).or(n.as_u64().map(i128::from)))
            }
            _ => Some(None),
        }
    }
}

thread_local! {
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn quiet<T>(f: impl FnOnce() -> T) -> Option<T> {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET.with(std::cell::Cell::get) {
                previous(info);
            }
        }));
    });
    let outer = QUIET.with(|q| q.replace(true));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    QUIET.with(|q| q.set(outer));
    out.ok()
}

fn shallow(src: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(src).ok()?;
    config_values::nested_past(text).is_none().then_some(text)
}

fn text(src: &[u8]) -> Option<&str> {
    shallow(src).filter(|text| config_values::wide_past(text).is_none())
}

fn read_var_file(path: &str, src: &[u8], module: &mut Module) {
    let Some(rank) = var_file_rank(path) else {
        return;
    };
    let rank = (rank.0, rank.1.to_string());
    let Some(values) = var_file_values(path, src) else {
        module.unread = module.unread.take().max(Some(rank));
        return;
    };
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

fn var_file_values(path: &str, src: &[u8]) -> Option<Vec<(String, Option<Value>, usize)>> {
    if src.len() > MAX_BYTES {
        return None;
    }
    let json = path.ends_with(".json");
    let text = if json { shallow(src) } else { text(src) }?;
    let mut values = Vec::new();
    if json {
        let map = serde_json::from_str::<Map<String, serde_json::Value>>(text).ok()?;
        let keys = RefCell::new(HashMap::new());
        let key = |path: &[Step], line| {
            if let [Step::Key(name)] = path {
                keys.borrow_mut().insert(name.clone(), line);
            }
        };
        let scalar = |s: events::Scalar<'_>| key(s.path, s.line);
        events::read_located(Syntax::Json, src, scalar, |c, _| key(c.path, c.line)).ok()?;
        let keys = keys.into_inner();
        for (name, value) in map {
            let at = keys.get(&name).copied().unwrap_or(1);
            let value = (!float(&value))
                .then(|| serde_json::from_value::<Value>(value).ok())
                .flatten()
                .map(functions::nfc_value)
                .filter(|v| size(v) <= EVAL_VALUE_MAX);
            values.push((name, value, at));
        }
    } else {
        let body = parser::parse_body(text).ok()?;
        let line = lines(text);
        let mut work = EVAL_WORK_MAX;
        let mut ctx = Context::new();
        functions::declare_exact(&mut ctx);
        for a in body.attributes() {
            let cost = steps(&a.value, &Map::new());
            let spend = cost.saturating_add(widths(&a.value));
            let value = if cost <= EVAL_COST_MAX && spend <= work {
                work -= spend;
                match evaluation(&a.value, &ctx, &BTreeSet::new()) {
                    (Some(v), _) => (size(&v) <= EVAL_VALUE_MAX).then_some(v),
                    (None, true) => None,
                    (None, false) => continue,
                }
            } else {
                None
            };
            let at = line(a.span().map_or(0, |s| s.start));
            values.push((a.key.as_str().to_string(), value, at));
        }
    }
    Some(values)
}

fn read(path: &str, src: &[u8], module: &mut Module) {
    if is_var_file(path) {
        read_var_file(path, src, module);
        return;
    }
    let overriding = config_values::is_override(path);
    module.overridden |= overriding;
    if src.len() > MAX_BYTES {
        module.unreadable = true;
        return;
    }
    if json::is_file(path) {
        return json::read(path, src, module);
    }
    let Some(text) = text(src) else {
        module.unreadable = true;
        return;
    };
    let Ok(body) = parser::parse_body(text) else {
        module.unreadable = true;
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
                    assign(&mut module.outputs, name, Named::new(value), overriding);
                }
                continue;
            }
            ("locals", []) => {
                for a in b.body.attributes() {
                    let value = Named::new(&a.value);
                    assign(&mut module.locals, a.key.as_str(), value, overriding);
                }
                continue;
            }
            ("variable", [name]) => {
                let variable = Variable {
                    default: attr("default").cloned(),
                    ty: attr("type").map(Declared::expr),
                    type_text: attr("type").map(|t| t.to_string()),
                    through: Vec::new(),
                    opentofu: false,
                    sensitive: crate::config_values::hcl::is_sensitive(attr("sensitive")),
                    path: path.to_string(),
                    line: line(b.span().map_or(0, |s| s.start)),
                    terraform: None,
                    terraform_only: false,
                };
                module.declare(name, variable, overriding);
                continue;
            }
            _ => continue,
        };
        let span = b.span().unwrap_or(0..0);
        let mut args = BTreeMap::new();
        let workload = types::workload("terraform", ty).is_some();
        if kind == Kind::Module
            || subscription::reads(ty)
            || http::reads(ty)
            || workload
            || matches!(ty, "archive_file" | "google_sql_database")
        {
            for a in b.body.attributes() {
                args.insert(a.key.as_str().to_string(), Named::new(&a.value));
            }
        }
        let source = match attr("source") {
            Some(Expression::String(s)) if kind == Kind::Module => Some(s.value().clone()),
            _ => None,
        };
        let block = Block {
            kind,
            ty: ty.to_string(),
            name: name.to_string(),
            path: path.to_string(),
            line: line(span.start),
            end_line: line(span.end.saturating_sub(1).max(span.start)),
            json_path: None,
            named: (kind != Kind::Module)
                .then(|| attr(name_argument(kind, ty)).cloned())
                .flatten(),
            count: attr("count").cloned(),
            for_each: attr("for_each").cloned(),
            refs: keyed_refs(&b.body),
            source,
            args,
            nested: if workload {
                b.body.blocks().cloned().collect()
            } else {
                Vec::new()
            },
        };
        module.add(block, overriding);
    }
}

fn name_argument(kind: Kind, ty: &str) -> &'static str {
    match kind {
        Kind::Resource => types::entry("terraform", ty)
            .map(|e| e.name.as_str())
            .or_else(|| types::workload("terraform", ty).and_then(|w| w.name.as_deref()))
            .unwrap_or("name"),
        _ => "name",
    }
}

type BlockName<'m> = (Kind, &'m str, &'m str);

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

fn lexical(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut entries = (map.into_iter())
                .map(|(k, v)| (k, lexical(v)))
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(lexical).collect()),
        v => v,
    }
}

fn printed_as_written(n: &hcl::Number, written: &str) -> bool {
    let shown = n.to_string();
    !shown.contains('e') && decimal(&shown).is_some_and(|d| decimal(written) == Some(d))
}

fn decimal(text: &str) -> Option<(bool, String, i64)> {
    let (negative, text) = match text.strip_prefix('-') {
        Some(t) => (
            true,
            t.rsplit(|c: char| c.is_whitespace() || c == '/').next()?,
        ),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, power) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let significant = digits.trim_start_matches('0');
    let kept = significant.trim_end_matches('0');
    let power = (power.parse::<i64>().ok())
        .and_then(|p| p.checked_sub(i64::try_from(fraction.len()).ok()?))
        .and_then(|p| p.checked_add(i64::try_from(significant.len() - kept.len()).ok()?))?;
    Some(match kept {
        "" => (negative, String::new(), 0),
        _ => (negative, kept.to_string(), power),
    })
}

fn float(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Number(n) => n.is_f64(),
        serde_json::Value::Array(items) => items.iter().any(float),
        serde_json::Value::Object(map) => map.values().any(float),
        _ => false,
    }
}

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

struct Roots<'v> {
    values: &'v Map<String, Value>,
    sizes: BTreeMap<&'v str, usize>,
    read_sizes: RefCell<HashMap<*const Value, usize>>,
    read_elements: RefCell<HashMap<*const Value, (usize, usize)>>,
}

impl Roots<'_> {
    fn size(&self, name: &str) -> Option<usize> {
        self.sizes.get(name).copied()
    }

    fn read_size(&self, v: &Value) -> usize {
        *(self.read_sizes.borrow_mut())
            .entry(std::ptr::from_ref(v))
            .or_insert_with(|| size(v))
    }

    fn read_elements(&self, v: &Value) -> (usize, usize) {
        let key = std::ptr::from_ref(v);
        if let Some(&found) = self.read_elements.borrow().get(&key) {
            return found;
        }
        let most = |sizes: &mut dyn Iterator<Item = usize>| sizes.max().unwrap_or(0);
        let found = match v {
            Value::Array(items) => (items.len(), most(&mut items.iter().map(size))),
            Value::Object(map) => (
                map.len(),
                most(&mut map.iter().map(|(k, v)| (k.len() + 2).max(size(v)))),
            ),
            v => (1, self.read_size(v)),
        };
        self.read_elements.borrow_mut().insert(key, found);
        found
    }
}

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

fn shadowed(name: &str, iters: &[(String, usize)]) -> bool {
    iters.iter().any(|(n, _)| n == name)
}

fn unshadowed<'v>(
    t: &Traversal,
    roots: &Roots<'v>,
    iters: &[(String, usize)],
) -> Option<&'v Value> {
    match &t.expr {
        Expression::Variable(v) if shadowed(v.as_str(), iters) => None,
        _ => bound(t, roots.values),
    }
}

fn copied(t: &Traversal, roots: &Roots<'_>, iters: &[(String, usize)]) -> usize {
    match &t.expr {
        Expression::Variable(v) if !iters.is_empty() => roots.size(v.as_str()).unwrap_or(0),
        _ => 0,
    }
}

fn steps(expr: &Expression, roots: &Map<String, Value>) -> usize {
    let sizes = roots.iter().map(|(k, v)| (k.as_str(), size(v))).collect();
    let roots = Roots {
        values: roots,
        sizes,
        read_sizes: RefCell::default(),
        read_elements: RefCell::default(),
    };
    let mut overhead = 0;
    let made = cost(expr, &roots, &mut Vec::new(), &mut overhead);
    made.saturating_add(overhead).max(1)
}

fn cost(
    expr: &Expression,
    roots: &Roots<'_>,
    iters: &mut Vec<(String, usize)>,
    overhead: &mut usize,
) -> usize {
    let add = |a: usize, b: usize| a.saturating_add(b);
    match expr {
        Expression::Null(_) | Expression::Bool(_) | Expression::Number(_) => 8,
        Expression::String(s) => s.len() + 2,
        Expression::Array(items) => (items.iter())
            .map(|e| cost(e, roots, iters, overhead).max(1))
            .fold(2, add),
        Expression::Object(object) => (object.iter())
            .map(|(k, v)| {
                let key = match k {
                    ObjectKey::Ident(i) => i.as_str().len(),
                    ObjectKey::Expression(e) => cost(e, roots, iters, overhead),
                };
                add(key, cost(v.expr(), roots, iters, overhead))
            })
            .fold(2, add),
        Expression::StringTemplate(t) => template_cost(t, roots, iters, overhead),
        Expression::HeredocTemplate(h) => template_cost(&h.template, roots, iters, overhead),
        Expression::Parenthesis(p) => cost(p.inner(), roots, iters, overhead),
        Expression::Variable(v) => match iters.iter().rev().find(|(n, _)| n == v.as_str()) {
            Some((_, n)) => *n,
            None => roots.size(v.as_str()).unwrap_or(8),
        },
        Expression::Traversal(t) => {
            let indices = (t.operators.iter()).filter_map(|o| match o.value() {
                TraversalOperator::Index(e) => Some(cost(e, roots, iters, overhead)),
                _ => None,
            });
            let indices = indices.collect::<Vec<_>>().into_iter().fold(0, add);
            let base = match unshadowed(t, roots, iters) {
                Some(v) => {
                    *overhead = add(*overhead, copied(t, roots, iters));
                    roots.read_size(v)
                }
                None => cost(&t.expr, roots, iters, overhead),
            };
            add(base, indices)
        }
        Expression::Conditional(c) => [&c.cond_expr, &c.true_expr, &c.false_expr]
            .into_iter()
            .map(|e| cost(e, roots, iters, overhead))
            .fold(0, add),
        Expression::FuncCall(f) => {
            let args: Vec<usize> = f
                .args
                .iter()
                .map(|e| cost(e, roots, iters, overhead))
                .collect();
            let sum = args.iter().copied().fold(8, add);
            let square = sum.saturating_mul(sum);
            match (f.name.name.as_str(), args.as_slice()) {
                ("formatlist" | "join" | "replace", _) if f.args.expand_final() => square,
                ("join", [sep, lists @ ..]) => {
                    let sep = shown_len(f.args.iter().next(), *sep);
                    let lists = lists.iter().copied().fold(0, add);
                    add(sum, lists.saturating_mul(add(sep, 3))).min(square)
                }
                ("replace", [s, _, to]) => {
                    let s = shown_len(f.args.iter().next(), *s);
                    let to = shown_len(f.args.iter().nth(2), *to);
                    add(sum, s.saturating_mul(add(to, 1))).min(square)
                }
                ("formatlist", [_, ..]) => {
                    add(sum, formatlist_cost(f, &args, roots, iters)).min(square)
                }
                ("format", _) => add(sum, args.len().saturating_mul(functions::WIDTH_MAX)),
                ("jsonencode", _) => sum.saturating_mul(6),
                ("split", _) => sum.saturating_mul(4),
                ("upper" | "lower" | "title", _) => sum.saturating_mul(3),
                _ => sum,
            }
        }
        Expression::UnaryOp(u) => add(cost(&u.expr, roots, iters, overhead), 8),
        Expression::BinaryOp(b) => add(
            cost(&b.lhs_expr, roots, iters, overhead),
            cost(&b.rhs_expr, roots, iters, overhead),
        ),
        Expression::ForExpr(f) => {
            let intro = &f.intro;
            let (n, each) = elements(&intro.collection_expr, roots, iters, overhead);
            let vars = [intro.key_var.as_ref(), Some(&intro.value_var)];
            let depth = iters.len();
            iters.extend(
                vars.into_iter()
                    .flatten()
                    .map(|v| (v.as_str().to_string(), each)),
            );
            let before = std::mem::take(overhead);
            let body = [f.key_expr.as_ref(), Some(&f.value_expr)]
                .into_iter()
                .chain([f.cond.as_ref().map(|c| &c.expr)])
                .flatten()
                .map(|e| cost(e, roots, iters, overhead))
                .fold(4, add);
            *overhead = add(before, n.saturating_mul(*overhead));
            iters.truncate(depth);
            n.saturating_mul(body)
        }
    }
}

fn formatlist_cost(
    f: &hcl_edit::expr::FuncCall,
    costs: &[usize],
    roots: &Roots<'_>,
    iters: &[(String, usize)],
) -> usize {
    let (mut rows, mut unknown_rows) = (None, 1);
    let (mut lists, mut repeated) = (0usize, 0usize);
    for (e, &c) in f.args.iter().zip(costs).skip(1) {
        let value = match e {
            Expression::Traversal(t) => unshadowed(t, roots, iters),
            _ => None,
        };
        let len = match (e, value) {
            (Expression::Array(items), _) => Some(items.len()),
            (_, Some(Value::Array(items))) => Some(items.len()),
            _ => None,
        };
        match len {
            Some(n) => {
                rows = Some(rows.map_or(n, |r: usize| r.min(n)));
                lists = lists.saturating_add(c);
            }
            None => {
                repeated = repeated.saturating_add(c);
                let scalar = value.is_some()
                    || matches!(
                        e,
                        Expression::String(_)
                            | Expression::Number(_)
                            | Expression::Bool(_)
                            | Expression::Null(_)
                    );
                if !scalar {
                    unknown_rows = unknown_rows.max(c);
                }
            }
        }
    }
    let row = (costs.len() - 1).saturating_mul(functions::WIDTH_MAX);
    let row = row
        .saturating_add(shown_len(f.args.iter().next(), costs[0]))
        .saturating_add(repeated.saturating_mul(6));
    let rows = rows.unwrap_or(unknown_rows);
    rows.saturating_mul(row)
        .saturating_add(lists.saturating_mul(6))
}

fn shown_len(e: Option<&Expression>, cost: usize) -> usize {
    match e {
        Some(Expression::String(s)) => s.len(),
        _ => cost.saturating_mul(3),
    }
}

fn template_cost(
    t: &Template,
    roots: &Roots<'_>,
    iters: &mut Vec<(String, usize)>,
    overhead: &mut usize,
) -> usize {
    let mut total = 0usize;
    for e in t.iter() {
        if !iters.is_empty() {
            *overhead = overhead.saturating_add(TEMPLATE_PART_COST);
        }
        let part = match e {
            Element::Literal(s) => s.value().len(),
            Element::Interpolation(i) => cost(&i.expr, roots, iters, overhead),
            Element::Directive(d) => match d.as_ref() {
                Directive::If(d) => {
                    let branches = template_cost(&d.if_expr.template, roots, iters, overhead)
                        .saturating_add(
                            (d.else_expr.as_ref())
                                .map_or(0, |e| template_cost(&e.template, roots, iters, overhead)),
                        );
                    cost(&d.if_expr.cond_expr, roots, iters, overhead).saturating_add(branches)
                }
                Directive::For(d) => {
                    let f = &d.for_expr;
                    let (n, each) = elements(&f.collection_expr, roots, iters, overhead);
                    let depth = iters.len();
                    let vars = [f.key_var.as_ref(), Some(&f.value_var)];
                    iters.extend(
                        vars.into_iter()
                            .flatten()
                            .map(|v| (v.as_str().to_string(), each)),
                    );
                    let before = std::mem::take(overhead);
                    let body = template_cost(&f.template, roots, iters, overhead);
                    *overhead = before.saturating_add(n.saturating_mul(*overhead));
                    iters.truncate(depth);
                    n.saturating_mul(body)
                }
            },
        };
        total = total.saturating_add(part);
    }
    total
}

fn elements(
    expr: &Expression,
    roots: &Roots<'_>,
    iters: &mut Vec<(String, usize)>,
    overhead: &mut usize,
) -> (usize, usize) {
    if let Expression::FuncCall(f) = expr
        && matches!(f.name.name.as_str(), "tolist" | "toset" | "tomap")
        && f.args.len() == 1
        && !f.args.expand_final()
        && let Some(arg) = f.args.iter().next()
    {
        return elements(arg, roots, iters, overhead);
    }
    match expr {
        Expression::Parenthesis(p) => return elements(p.inner(), roots, iters, overhead),
        Expression::Conditional(c) => {
            let cond = cost(&c.cond_expr, roots, iters, overhead);
            *overhead = overhead.saturating_add(cond);
            let (a, x) = elements(&c.true_expr, roots, iters, overhead);
            let (b, y) = elements(&c.false_expr, roots, iters, overhead);
            return (a.max(b), x.max(y));
        }
        Expression::Array(items) => {
            let costs = (items.iter()).map(|e| cost(e, roots, iters, overhead));
            let costs: Vec<usize> = costs.collect();
            *overhead = (costs.iter()).fold(*overhead, |a, &c| a.saturating_add(c));
            return (items.len(), costs.into_iter().max().unwrap_or(0));
        }
        Expression::Object(object) => {
            let mut each = 0;
            for (k, v) in object.iter() {
                let key = match k {
                    ObjectKey::Ident(i) => i.as_str().len() + 2,
                    ObjectKey::Expression(e) => cost(e, roots, iters, overhead),
                };
                let value = cost(v.expr(), roots, iters, overhead);
                *overhead = overhead.saturating_add(key).saturating_add(value);
                each = each.max(key).max(value);
            }
            return (object.len(), each);
        }
        _ => {}
    }
    let found = match expr {
        Expression::Traversal(t) => unshadowed(t, roots, iters).inspect(|_| {
            *overhead = overhead.saturating_add(copied(t, roots, iters));
        }),
        Expression::Variable(v) if !shadowed(v.as_str(), iters) => {
            roots.values.get(v.as_str()).inspect(|_| {
                if !iters.is_empty() {
                    let root = roots.size(v.as_str()).unwrap_or(0);
                    *overhead = overhead.saturating_add(root);
                }
            })
        }
        _ => None,
    };
    match found {
        Some(v) => roots.read_elements(v),
        None => {
            let c = cost(expr, roots, iters, overhead);
            (c, c)
        }
    }
}

#[derive(Default)]
struct Variables(BTreeMap<String, usize>);

impl Variables {
    fn of(expr: &Expression) -> Variables {
        let mut variables = Variables::default();
        variables.visit_expr(expr);
        variables
    }

    fn reads(&self, name: &str) -> usize {
        self.0.get(name).copied().unwrap_or(0)
    }

    fn reads_instance(&self) -> bool {
        self.0.contains_key("each") || self.0.contains_key("count")
    }
}

impl Visit for Variables {
    fn visit_expr(&mut self, node: &Expression) {
        if let Expression::Variable(v) = node {
            *self.0.entry(v.as_str().to_string()).or_default() += 1;
        }
        visit::visit_expr(self, node);
    }
}

fn widths(expr: &Expression) -> usize {
    struct Widths(usize);
    impl Visit for Widths {
        fn visit_object(&mut self, node: &hcl_edit::expr::Object) {
            self.0 = (self.0).saturating_add(node.len().saturating_mul(node.len()));
            visit::visit_object(self, node);
        }
    }
    let mut widths = Widths(0);
    widths.visit_expr(expr);
    widths.0
}

#[derive(Debug, Clone)]
struct Each {
    key: Key,
    value: Value,
    through: BTreeSet<Overridable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Name {
    Var(String),
    Local(String),
    Output(String),
}

struct Instance<'m> {
    dir: &'m str,
    parent: Option<(usize, &'m Block)>,
    call: Option<Each>,
    nodes: HashMap<BlockName<'m>, Vec<(Option<Key>, String)>>,
    children: HashMap<&'m str, Vec<(Option<Key>, usize)>>,
}

struct Eval<'m> {
    modules: &'m BTreeMap<String, Module>,
    mappings: &'m mapping::Config,
    instances: Vec<Instance<'m>>,
    memo: HashMap<(usize, &'m Ref), Vec<String>>,
    open: HashSet<(usize, &'m Ref)>,
    ctx: Context<'static>,
    values: HashMap<(usize, Name), Evaluated>,
    sets: HashMap<(usize, Name), bool>,
    evaluating: HashSet<(usize, Name)>,
    outputs: bool,
    work: usize,
    pending: Vec<(usize, usize, &'m Block, Option<Each>)>,
    subscription_inputs: subscription::Inputs,
    node_inputs: HashMap<String, (usize, BTreeSet<Overridable>)>,
    root: String,
    root_nodes: usize,
    root_left: usize,
    left: usize,
    nodes: Vec<Node>,
    references: Vec<Reference>,
}

impl<'m> Eval<'m> {
    fn new(
        modules: &'m BTreeMap<String, Module>,
        mappings: &'m mapping::Config,
        left: usize,
    ) -> Eval<'m> {
        let mut ctx = Context::new();
        functions::declare(&mut ctx);
        Eval {
            modules,
            mappings,
            instances: Vec::new(),
            memo: HashMap::new(),
            open: HashSet::new(),
            ctx,
            values: HashMap::new(),
            sets: HashMap::new(),
            evaluating: HashSet::new(),
            outputs: false,
            work: 0,
            pending: Vec::new(),
            subscription_inputs: subscription::Inputs::default(),
            node_inputs: HashMap::new(),
            root: String::new(),
            root_nodes: 0,
            root_left: 0,
            left,
            nodes: Vec::new(),
            references: Vec::new(),
        }
    }

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

    fn root(&mut self, dir: &'m str) {
        self.node_inputs.clear();
        self.instances.clear();
        self.memo.clear();
        self.open.clear();
        self.values.clear();
        self.sets.clear();
        self.evaluating.clear();
        self.pending.clear();
        self.subscription_inputs = subscription::Inputs::default();
        self.outputs = false;
        self.work = EVAL_WORK_MAX;
        self.root = label(dir).to_string();
        self.root_nodes = 0;
        self.root_left = ROOT_STORED_MAX;
        self.instantiate(dir, "", None, None, &mut vec![dir]);
        self.values.clear();
        self.evaluating.clear();
        self.outputs = true;
        let pending = std::mem::take(&mut self.pending);
        for &(node, _, block, _) in &pending {
            if block.kind == Kind::Resource
                && (block.ty.starts_with("azurerm_") || block.ty.starts_with("google_"))
                && types::entry("terraform", &block.ty).is_some()
            {
                self.nodes[node].provisioned = Some(Projection::default());
            }
        }
        let mut shared: HashMap<(usize, *const Block), Evaluated> = HashMap::new();
        for &(node, instance, block, ref each) in &pending {
            let Some(named) = &block.named else { continue };
            let key = (instance, std::ptr::from_ref(block));
            let found = match shared.get(&key) {
                Some(found) => found.clone(),
                None => {
                    let found = self.value(instance, each.as_ref(), named, 0);
                    if each.is_some()
                        && matches!(found.val, Val::Known(_))
                        && !Variables::of(named).reads_instance()
                    {
                        shared.insert(key, found.clone());
                    }
                    found
                }
            };
            let (name, pattern) = match &found.val {
                Val::Known(Value::Null | Value::Array(_) | Value::Object(_)) => (None, false),
                _ => {
                    let (text, pattern) = found.text();
                    let entry = (block.kind == Kind::Resource)
                        .then(|| types::entry("terraform", &block.ty))
                        .flatten();
                    let name = match entry {
                        Some(e) if !pattern && e.ty.starts_with("google_") => e
                            .project(&text)
                            .map(|name| {
                                name.strip_prefix("subscriptions/")
                                    .unwrap_or(&name)
                                    .to_string()
                            })
                            .or(Some(text)),
                        Some(e) if !pattern => e.project(&text),
                        _ => Some(text),
                    };
                    (name, pattern)
                }
            };
            if !self.charge(name.as_ref().map_or(0, String::len)) {
                return;
            }
            let node = &mut self.nodes[node];
            (node.name, node.pattern) = (name, pattern);
            node.overridable = found.through.into_iter().collect();
        }
        for &(node, instance, block, ref each) in &pending {
            self.mapped_module(node, instance, each.as_ref(), block);
            if self.bound_instance(instance)
                && (each.is_some() || (block.count.is_none() && block.for_each.is_none()))
            {
                let mut through = self.instance_through(instance);
                if let Some(each) = each {
                    through.extend(each.through.iter().cloned());
                }
                self.node_inputs
                    .insert(self.nodes[node].address.clone(), (node, through));
            }
        }
        self.prepare_subscriptions(&pending);
        for &(node, _, block, _) in &pending {
            self.azure_projection(node, block);
            self.gcp_projection(node, block);
        }
        self.azure_subscriptions(&pending);
        for &(node, instance, block, ref each) in &pending {
            self.gcp_database(node, instance, each.as_ref(), block);
        }
        for &(node, instance, block, ref each) in &pending {
            self.gcp_workload(node, instance, each.as_ref(), block);
            self.azure_workload(node, instance, each.as_ref(), block);
            self.workload(node, instance, each.as_ref(), block);
            self.module_workload(node, instance, each.as_ref(), block);
        }
        self.http_routes(&pending);
        for &(node, instance, block, ref each) in &pending {
            let Some(subscription) = self.subscription(node, instance, each.as_ref(), block) else {
                continue;
            };
            let bytes = serde_json::to_vec(&subscription).map_or(usize::MAX, |s| s.len());
            if !self.charge(bytes) {
                return;
            }
            self.nodes[node].subscription = Some(subscription);
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
                for r in block.refs.values().flatten() {
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
                    build_image: false,
                    dockerfile: None,
                    role: None,
                    handler: None,
                    events: Vec::new(),
                    routes: Vec::new(),
                    subscription: None,
                    environment: Vec::new(),
                    images: Vec::new(),
                    provisioned: None,
                    ingress: Vec::new(),
                    actions: None,
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
        let for_each = block.for_each.as_ref()?;
        let found = self.value(at, None, for_each, 0);
        let entries: BTreeMap<String, Value> = match found.val {
            Val::Known(Value::Object(map)) => map.into_iter().collect(),
            Val::Known(Value::Array(_)) if self.is_tuple(at, for_each) => return None,
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

    fn holds_set(&mut self, at: usize, name: Name, depth: usize) -> bool {
        let key = (at, name);
        if let Some(&found) = self.sets.get(&key) {
            return found;
        }
        if depth > EVAL_DEPTH_MAX {
            return false;
        }
        self.sets.insert(key.clone(), false);
        let modules = self.modules;
        let module = &modules[self.instances[at].dir];
        let (scope, expr, kept) = match &key.1 {
            Name::Local(n) => (at, module.locals.get(n).map(|l| &l.expr), false),
            Name::Var(n) => match (module.variables.get(n), self.instances[at].parent) {
                (Some(v), _)
                    if matches!(
                        v.ty,
                        Some(Declared::Set(_) | Declared::List(_) | Declared::Other)
                    ) =>
                {
                    let set = matches!(v.ty, Some(Declared::Set(_)));
                    self.sets.insert(key, set);
                    return set;
                }
                (Some(v), Some((caller, block))) => match block.args.get(n) {
                    Some(arg) => (caller, Some(&arg.expr), false),
                    None => (at, v.default.as_ref(), ends_in_set(&v.through)),
                },
                (Some(_), None) if module.given.contains_key(n) => (at, None, false),
                (Some(v), None) => (at, v.default.as_ref(), ends_in_set(&v.through)),
                (None, _) => (at, None, false),
            },
            Name::Output(_) => (at, None, false),
        };
        let found = kept
            || expr.is_some_and(|expr| {
                is_set(expr, &mut |r| match r {
                    Ref::Var(n) => self.holds_set(scope, Name::Var(n.clone()), depth + 1),
                    Ref::Local(n) => self.holds_set(scope, Name::Local(n.clone()), depth + 1),
                    Ref::Block { .. } => false,
                })
            });
        self.sets.insert(key, found);
        found
    }

    fn is_tuple(&self, at: usize, expr: &Expression) -> bool {
        match expr {
            Expression::Array(_) => true,
            Expression::ForExpr(f) => f.key_expr.is_none(),
            Expression::Parenthesis(p) => self.is_tuple(at, p.inner()),
            _ => matches!(lone_ref(expr), Some(Ref::Var(n)) if self.holds_tuple(at, &n)),
        }
    }

    fn holds_tuple(&self, at: usize, n: &str) -> bool {
        let module = &self.modules[self.instances[at].dir];
        let Some(v) = module.variables.get(n) else {
            return false;
        };
        let converts = |t: &Declared| matches!(t, Declared::Set(_) | Declared::List(_));
        let literal = !v.through.iter().any(converts);
        match (v.ty, self.instances[at].parent) {
            (Some(Declared::Other), _) => true,
            (Some(Declared::Set(_) | Declared::List(_)), _) => false,
            (_, Some((caller, block))) => match block.args.get(n) {
                Some(arg) => self.is_tuple(caller, &arg.expr),
                None => literal,
            },
            (_, None) => module.given.contains_key(n) || literal,
        }
    }

    fn var(&mut self, at: usize, n: &str, depth: usize) -> Evaluated {
        let module = &self.modules[self.instances[at].dir];
        let Some(variable) = module.variables.get(n).filter(|v| !v.sensitive) else {
            return Evaluated::unknown();
        };
        let found = self.reading(at, n, variable, depth);
        let Some(terraform) = variable.terraform.as_deref() else {
            return found;
        };
        let ty = |v: &Variable| -> String {
            (v.type_text.as_deref().unwrap_or("any").split_whitespace()).collect()
        };
        let same = ty(variable) == ty(terraform)
            && ends_in_set(&variable.through) == ends_in_set(&terraform.through)
            && self.reading(at, n, terraform, depth).val == found.val;
        match same {
            true => found,
            false => Evaluated {
                val: Val::Unknown,
                through: found.through,
            },
        }
    }

    fn reading(&mut self, at: usize, n: &str, variable: &'m Variable, depth: usize) -> Evaluated {
        let module = &self.modules[self.instances[at].dir];
        if let Some((caller, block)) = self.instances[at].parent {
            let call = self.instances[at].call.clone();
            return match (block.args.get(n), &variable.default) {
                (Some(arg), _) => {
                    let found = self.value(caller, call.as_ref(), &arg.expr, depth + 1);
                    self.received(at, found, variable, depth)
                }
                (None, Some(default)) => {
                    let found = self.value(at, None, default, depth + 1);
                    declared_default(found, variable)
                }
                (None, None) => Evaluated::unknown(),
            };
        }
        let given = module.given.get(n);
        let unread =
            (module.unread.as_ref()).is_some_and(|unread| given.is_none_or(|g| g.rank < *unread));
        let (mut found, path, line, tfvars) = match (given, &variable.default) {
            _ if unread => (Evaluated::unknown(), &variable.path, variable.line, false),
            (Some(given), _) => {
                let found = Evaluated {
                    val: given.value.clone().map_or(Val::Unknown, Val::Known),
                    through: BTreeSet::new(),
                };
                (
                    self.received(at, found, variable, depth),
                    &given.path,
                    given.line,
                    true,
                )
            }
            (None, Some(default)) => {
                let found = self.value(at, None, default, depth + 1);
                (
                    declared_default(found, variable),
                    &variable.path,
                    variable.line,
                    false,
                )
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

    fn received(
        &mut self,
        at: usize,
        found: Evaluated,
        variable: &'m Variable,
        depth: usize,
    ) -> Evaluated {
        if !variable.opentofu
            && let Some(default) = &variable.default
            && let Val::Known(v) = self.value(at, None, default, depth + 1).val
            && converted_through(v, &variable.through).is_none()
        {
            return Evaluated {
                val: Val::Unknown,
                through: found.through,
            };
        }
        declared(found, variable.ty.as_slice())
    }

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

    fn value(
        &mut self,
        at: usize,
        each: Option<&Each>,
        expr: &'m Expression,
        depth: usize,
    ) -> Evaluated {
        if depth > EVAL_DEPTH_MAX {
            return Evaluated::unknown();
        }
        if let Expression::String(s) = expr
            && s.len() + 2 <= EVAL_VALUE_MAX
        {
            return Evaluated {
                val: Val::Known(Value::String(functions::nfc(s.value().clone()))),
                through: BTreeSet::new(),
            };
        }
        if self.work == 0 {
            return Evaluated::unknown();
        }
        let mut through = BTreeSet::new();
        let (mut vars, mut locals, mut mods) = (Map::new(), Map::new(), Map::new());
        let mut sets = BTreeSet::new();
        let mut outputs: BTreeMap<String, (BTreeSet<String>, Vec<Option<Key>>)> = BTreeMap::new();
        let refs = refs_of(expr);
        let mut seen = HashSet::new();
        for r in refs.iter().cloned() {
            let held = match &r {
                Ref::Var(n) => Some(Name::Var(n.clone())),
                Ref::Local(n) => Some(Name::Local(n.clone())),
                _ => None,
            };
            if let (Some(held), Some(via)) = (held, r.via())
                && self.holds_set(at, held, depth)
            {
                sets.insert(via);
            }
            if matches!(r, Ref::Var(_) | Ref::Local(_)) && !seen.insert(r.clone()) {
                continue;
            }
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
        let variables = Variables::of(expr);
        let (mut each_v, mut count_v) = (Map::new(), Map::new());
        if let Some(e) = each.filter(|_| variables.reads_instance()) {
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
        if through.len() > OVERRIDABLE_MAX {
            return Evaluated::unknown();
        }
        let roots: Map<String, Value> = [
            ("var", vars),
            ("local", locals),
            ("module", mods),
            ("each", each_v),
            ("count", count_v),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), lexical(Value::Object(v))))
        .collect();
        let cost = steps(expr, &roots);
        let spend = cost.saturating_add(widths(expr));
        if cost <= EVAL_COST_MAX && spend <= self.work {
            self.work -= spend;
            for (name, v) in roots {
                self.ctx.declare_var(name, v);
            }
            match evaluated(expr, &self.ctx, &sets) {
                Some(v) if size(&v) <= EVAL_VALUE_MAX => {
                    return Evaluated {
                        val: Val::Known(v),
                        through,
                    };
                }
                _ => {}
            }
        } else {
            let text = (expr.span()).map_or_else(|| expr.to_string().len(), |s| s.len());
            let spent = (roots.iter())
                .map(|(name, v)| {
                    let reads = variables.reads(name).saturating_add(1).saturating_mul(2);
                    size(v).saturating_mul(reads)
                })
                .fold(text, usize::saturating_add);
            self.work = self.work.saturating_sub(spent.min(EVAL_COST_MAX));
        }
        self.partial(at, each, expr, depth)
    }

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
                Val::Pattern(functions::nfc(text))
            }
            false => Val::Known(Value::String(functions::nfc(text))),
        };
        Evaluated { val, through }
    }

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

fn label(dir: &str) -> &str {
    if dir.is_empty() { "." } else { dir }
}

pub(crate) struct Repository {
    modules: BTreeMap<String, Module>,
    mappings: mapping::Config,
    compose: Vec<(String, Vec<u8>)>,
}

impl Repository {
    #[cfg(test)]
    pub(crate) fn read(files: &[(String, Vec<u8>)]) -> Repository {
        let (files, shadowed) = config_values::unshadowed(files.to_vec());
        Self::read_with(&files, &shadowed, &mapping::Config::default())
    }

    pub(crate) fn read_with(
        files: &[(String, Vec<u8>)],
        shadowed: &[String],
        mappings: &mapping::Config,
    ) -> Repository {
        let mut modules: BTreeMap<String, Module> = BTreeMap::new();
        let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
        sorted.sort_by_key(|(path, _)| (config_values::is_override(path), path));
        let mut compose = Vec::new();
        for (path, src) in sorted {
            if compose::is_file(path) {
                compose.push((path.clone(), src.clone()));
                continue;
            }
            let dir = config_values::parent(path).to_string();
            read(path, src, modules.entry(dir).or_default());
        }
        modules.values_mut().for_each(Module::settle);
        for path in shadowed {
            if let Some(module) = modules.get_mut(config_values::parent(path)) {
                module.unreadable = true;
            }
        }
        let unknown = modules
            .values_mut()
            .filter(|m| m.overridden && m.unreadable);
        for variable in unknown.flat_map(|m| m.variables.values_mut()) {
            variable.sensitive = true;
        }
        Repository {
            modules,
            compose,
            mappings: mappings.clone(),
        }
    }

    fn calls<'a>(&'a self, dir: &'a str) -> impl Iterator<Item = String> + 'a {
        (self.modules.get(dir).into_iter().flat_map(|m| &m.blocks))
            .filter_map(move |b| b.source.as_deref().and_then(|s| local_source(dir, s)))
    }

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

    pub(crate) fn evaluate(
        &self,
        pick: impl Fn(&str) -> bool,
        left: usize,
    ) -> (Vec<Node>, Vec<Reference>) {
        let mut eval = Eval::new(&self.modules, &self.mappings, left);
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
        "SELECT sum(length(CAST(address AS BLOB)) + coalesce(length(CAST(name AS BLOB)), 0) \
         + length(CAST(environment AS BLOB)) + length(CAST(images AS BLOB)) \
         + coalesce(length(CAST(provisioned AS BLOB)), 0) + length(CAST(ingress AS BLOB))) \
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

fn terraform_path(column: &str) -> String {
    let mut any = vec![
        format!("{column} LIKE '%.tf'"),
        format!("{column} LIKE '%.tofu'"),
        format!("{column} LIKE '%.tf.json'"),
        format!("{column} LIKE '%.tofu.json'"),
    ];
    for name in compose::NAMES {
        any.push(format!("{column} = '{name}' OR {column} LIKE '%/{name}'"));
    }
    format!("({})", any.join(" OR "))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    Terraform,
    Kubernetes,
    CloudFormation,
    Actions,
}

impl Owner {
    pub(crate) fn sql(self) -> String {
        let own = terraform_path("path");
        let typed = "type LIKE '%::%'";
        let ci = actions::ROWS;
        match self {
            Owner::Terraform => format!("({own} AND NOT {typed} AND NOT {ci})"),
            Owner::Kubernetes => format!("(NOT {own} AND NOT {typed} AND NOT {ci})"),
            Owner::CloudFormation => typed.to_string(),
            Owner::Actions => ci.to_string(),
        }
    }
}

pub(crate) fn forget(tx: &Transaction, root: Option<&str>, owner: Owner) -> Result<()> {
    let own = owner.sql();
    let mut args: Vec<&dyn rusqlite::ToSql> = Vec::new();
    let mut scope = String::new();
    if let Some(root) = &root {
        scope = " AND root = ?1".into();
        args.push(root);
    }
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

pub(crate) fn template_wanted(path: &str, src: &[u8]) -> bool {
    cloudformation::wanted(path, src) || serverless::wanted(path, src)
}

pub(crate) fn template_evaluate(path: &str, src: &[u8]) -> (Vec<Node>, Vec<Reference>) {
    match serverless::is_file(path) {
        true => serverless::evaluate(path, src),
        false => cloudformation::evaluate(path, src),
    }
}

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

pub(crate) fn insert(tx: &Transaction, nodes: &[Node], references: &[Reference]) -> Result<()> {
    let mut insert = tx
        .prepare_cached("INSERT INTO iac_nodes VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)")?;
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
            n.build_image,
            n.subscription
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            serde_json::to_string(&n.environment)?,
            serde_json::to_string(&n.images)?,
            n.provisioned
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            serde_json::to_string(&n.ingress)?,
            serde_json::to_string(&n.routes)?,
            n.dockerfile
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            n.actions.as_ref().map(serde_json::to_string).transpose()?,
        ])?;
    }
    let mut insert = tx.prepare_cached("INSERT INTO iac_references VALUES (?1, ?2, ?3, ?4)")?;
    for r in references {
        insert.execute(params![r.root, r.source, r.target, r.via])?;
    }
    Ok(())
}

const COLUMNS: &str = "address, root, type, name, pattern, overridable, path, line, end_line, \
                       block, image, image_overridable, build, build_overridable, role, \
                       handler, handler_overridable, events, build_image, subscription, environment, images, provisioned, ingress, routes, dockerfile, actions";

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
        build_image: r.get(18)?,
        dockerfile: r
            .get::<_, Option<String>>(25)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        role: r.get(14)?,
        handler: resolved(r, 15)?,
        events: serde_json::from_str(&r.get::<_, String>(17)?).unwrap_or_default(),
        routes: serde_json::from_str(&r.get::<_, String>(24)?).unwrap_or_default(),
        environment: serde_json::from_str(&r.get::<_, String>(20)?).unwrap_or_default(),
        images: serde_json::from_str(&r.get::<_, String>(21)?).unwrap_or_default(),
        provisioned: r
            .get::<_, Option<String>>(22)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        ingress: serde_json::from_str(&r.get::<_, String>(23)?).unwrap_or_default(),
        subscription: r
            .get::<_, Option<String>>(19)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        actions: r
            .get::<_, Option<String>>(26)?
            .and_then(|s| serde_json::from_str(&s).ok()),
    })
}

pub(crate) fn nodes(db: &Connection) -> Result<Vec<Node>> {
    Ok(db
        .prepare(&format!(
            "SELECT {COLUMNS} FROM iac_nodes ORDER BY root, path, address"
        ))?
        .query_map([], row)?
        .collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn all_references(db: &Connection) -> Result<Vec<Reference>> {
    Ok(db.prepare("SELECT root, source, target, via FROM iac_references ORDER BY root, source, target, via")?
        .query_map([], |row| Ok(Reference { root: row.get(0)?, source: row.get(1)?, target: row.get(2)?, via: row.get(3)? }))?
        .collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn at(identity: &str, n: &Node) -> String {
    format!("{identity} {}:{} {}", n.path, n.line, n.address)
}

pub(crate) fn hits(
    identity: &str,
    db: &Connection,
    needle: &str,
    note: impl Fn(&Node) -> &'static str,
) -> Result<Vec<Hit>> {
    // ponytail: scans every node of every repository; an indexed lookup waits for a workspace
    // that makes it slow.
    let mut statement = db.prepare(&format!(
        "SELECT {COLUMNS} FROM iac_nodes WHERE NOT {}",
        actions::ROWS
    ))?;
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
        let text = format!(
            "{} ({}{name}, root {}){}",
            at(identity, &n),
            n.ty,
            n.root,
            note(&n)
        );
        let key = format!(
            "{identity} {} {:010} {} {}",
            n.path, n.line, n.address, n.root
        );
        hits.push((r, key, text));
    }
    Ok(hits)
}

pub(crate) fn named(identity: &str, nodes: &[Node], text: &str) -> Vec<Node> {
    let (text, root) = match text
        .strip_suffix(')')
        .and_then(|t| t.rsplit_once(" (root "))
    {
        Some((text, root)) => (text, Some(root)),
        None => (text, None),
    };
    let mut found = nodes
        .iter()
        .filter(|n| {
            let path = format!("{}::{}", n.path, n.address);
            (text == n.address
                || text == path
                || text == n.identity(identity)
                || actions::names(identity, n, text))
                && root.is_none_or(|r| r == n.root)
        })
        .cloned()
        .collect::<Vec<_>>();
    found.sort_by(|a, b| {
        (&a.path, a.line, &a.address, &a.root).cmp(&(&b.path, b.line, &b.address, &b.root))
    });
    found.dedup();
    found
}

pub(crate) fn found(exports: &[crate::index::Export], text: &str) -> Vec<(usize, Node)> {
    exports
        .iter()
        .enumerate()
        .flat_map(|(n, e)| {
            named(&e.repository, &e.iac, text)
                .into_iter()
                .map(move |node| (n, node))
        })
        .collect()
}

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
            Ok((row(r)?, r.get(COLUMNS.split(", ").count())?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    found.sort_by(|(a, va), (b, vb)| {
        (&a.path, a.line, &a.address, va).cmp(&(&b.path, b.line, &b.address, vb))
    });
    Ok(found)
}

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

fn reached(identity: &str, n: &Node, via: &Option<String>) -> String {
    let via = via.as_ref().map_or(String::new(), |v| format!(" via {v}"));
    format!("{}{via}", at(identity, n))
}

pub(crate) fn context(
    repos: &[(&str, &Connection)],
    exports: &[crate::index::Export],
    text: &str,
) -> Result<Option<Vec<String>>> {
    let found = found(exports, text);
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

pub(crate) fn trace<'l>(
    repos: &[(&str, &Connection)],
    exports: &[crate::index::Export],
    text: &str,
    direction: Direction,
    budget: Option<usize>,
    mut attached: impl FnMut(usize, &Node, usize) -> Vec<Row>,
    mut calls: impl FnMut(usize, &Node) -> Vec<(usize, Node, &'l Link)>,
) -> Result<Option<String>> {
    let found = found(exports, text);
    let [(r, start)] = &found[..] else {
        return Ok((!found.is_empty())
            .then(|| crate::render::budgeted(&several(repos, text, &found), budget)));
    };
    let mut rows = Vec::new();
    let mut expanded: HashSet<(usize, String, String)> = HashSet::new();
    let mut path: Vec<(usize, String, String)> = Vec::new();
    let mut stack = vec![(0, *r, start.clone(), None::<String>, None::<&Link>)];
    while let Some((depth, r, n, via, link)) = stack.pop() {
        path.truncate(depth);
        let (identity, db) = repos[r];
        let key = (r, n.root.clone(), n.address.clone());
        let mark = match () {
            _ if path.contains(&key) => " (cycle)",
            _ if expanded.contains(&key) => " (see above)",
            _ => "",
        };
        let indent = "  ".repeat(depth);
        rows.push(match link {
            Some(link) => {
                let text = format!("{indent}{} {}{mark}", arrow(link), at(identity, &n));
                let mut row = Row::link(text, r, link);
                row.text += &std::mem::take(&mut row.end);
                row
            }
            None => {
                let text = format!("{indent}{}{mark}", reached(identity, &n, &via));
                Row::call(text, r, (depth > 0).then_some(Level::Exact))
            }
        });
        if !mark.is_empty() {
            continue;
        }
        rows.extend(attached(r, &n, depth + 1));
        expanded.insert(key.clone());
        path.push(key);
        let called = calls(r, &n).into_iter();
        let mut next: Vec<_> = called
            .map(|(m, node, link)| (depth + 1, m, node, None, Some(link)))
            .collect();
        let referenced = references(db, &n, direction)?.into_iter();
        next.extend(referenced.map(|(node, via)| (depth + 1, r, node, via, None)));
        stack.extend(next.into_iter().rev());
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

    mod oracle;
    mod retype;
    mod shapes;
    mod tofu;

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

    #[test]
    fn a_literal_index_in_either_spelling_picks_its_instance() {
        let (_, references) = evaluate(&files(&[(
            "main.tf",
            concat!(
                "resource \"a\" \"q\" { for_each = toset([\"e\\u0301\", \"z\"]) }\n",
                "resource \"b\" \"nfd\" { x = a.q[\"e\\u0301\"].arn }\n",
                "resource \"b\" \"nfc\" { x = a.q[\"\\u00e9\"].arn }\n",
            ),
        )]));
        assert_eq!(
            shown(&references),
            ["b.nfd -> a.q[\"\u{e9}\"]", "b.nfc -> a.q[\"\u{e9}\"]"]
        );
    }

    #[test]
    fn a_chain_of_names_past_the_evaluation_depth_stops() {
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
        assert_eq!(nodes.len(), MODULE_DEPTH_MAX + 1);
        assert!(nodes.iter().all(|n| n.root == "m0" && n.ty == "module"));
        let deepest = nodes.last().unwrap();
        assert_eq!(
            deepest.address.matches("module.n").count(),
            MODULE_DEPTH_MAX + 1
        );
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
    fn terraform_json_evaluates_typed_templates_literals_and_local_modules() {
        let main = serde_json::json!({
            "variable": {
                "env": {"default": "dev"},
                "literal": {"default": "${terraform.workspace}"},
                "n": {"default": 2},
                "queues": {"default": {"a": "north", "b": "south"}},
                "secret": {"sensitive": true, "default": "never-repeat-this"}
            },
            "locals": {"prefix": "${upper(var.env)}", "map": "${var.queues}"},
            "resource": {"a": {
                "child": {"name": "${module.child.full}"},
                "counted": {"count": "${var.n}", "name": "${lower(local.prefix)}-${count.index}"},
                "each": {"for_each": "${local.map}", "name": "${each.key}-${each.value}"},
                "escaped": {"name": "$${var.env}-%%{literal}"},
                "literal": {"name": "${var.literal}"},
                "partial": {"name": "orders-${data.a.lookup.id}"},
                "plain": {"name": "var.env"},
                "sensitive": {"name": "q-${var.secret}"}
            }},
            "data": {"a": {"lookup": {}}},
            "module": {"child": {"source": "./child", "name": "${local.prefix}"}}
        })
        .to_string();
        let child = serde_json::json!({
            "variable": {"name": {}},
            "resource": {"b": {"inner": {"name": "${var.name}"}}},
            "output": {"full": {"value": "${lower(var.name)}-child"}}
        })
        .to_string();
        assert_eq!(
            names(&[("main.tf.json", &main), ("child/main.tofu.json", &child)]),
            [
                "data.a.lookup",
                "module.child",
                "module.child.b.inner = \"DEV\" [var.env main.tf.json:1]",
                "a.child = \"dev-child\" [var.env main.tf.json:1]",
                "a.counted[0] = \"dev-0\" [var.env main.tf.json:1] [var.n main.tf.json:1]",
                "a.counted[1] = \"dev-1\" [var.env main.tf.json:1] [var.n main.tf.json:1]",
                "a.each[\"a\"] = \"a-north\" [var.queues main.tf.json:1]",
                "a.each[\"b\"] = \"b-south\" [var.queues main.tf.json:1]",
                "a.escaped = \"${var.env}-%{literal}\"",
                "a.literal = \"${terraform.workspace}\" [var.literal main.tf.json:1]",
                "a.partial ~ \"orders-*\"",
                "a.plain = \"var.env\"",
                "a.sensitive ~ \"q-*\"",
            ]
        );
    }

    #[test]
    fn terraform_json_expression_construction_work_bounds_wide_objects() {
        let wide: serde_json::Map<String, serde_json::Value> = (0..1025)
            .map(|n| (format!("k{n:04}"), serde_json::Value::Bool(true)))
            .collect();
        let source = serde_json::json!({
            "locals": {"wide": wide},
            "resource": {"a": {"limited": {"name": "bounded-${join(\"\", keys(local.wide))}"}}}
        })
        .to_string();
        assert_eq!(
            names(&[("main.tf.json", &source)]),
            ["a.limited ~ \"bounded-*\""]
        );
    }

    #[test]
    fn terraform_json_ast_room_bounds_templates_without_cutting_literal_strings() {
        let literal = "1".repeat(4096);
        let prefix = "p".repeat(4096);
        let source = serde_json::json!({"resource": {"a": {
            "limited": {"name": "${1}".repeat(4096)},
            "literal": {"name": literal},
            "prefixed": {"name": format!("{prefix}${{1}}")}
        }}})
        .to_string();
        assert_eq!(
            names(&[("main.tf.json", &source)]),
            [
                "a.limited ~ \"*\"".to_string(),
                format!("a.literal = \"{literal}\""),
                format!("a.prefixed = \"{prefix}1\""),
            ]
        );
    }

    #[test]
    fn terraform_json_dependency_room_leaves_other_references_resolvable() {
        let source = serde_json::json!({"resource": {
            "a": {"target": {}},
            "b": {
                "limited": {"depends_on": vec!["a.target"; 4096]},
                "ordinary": {"depends_on": ["a.target"]}
            }
        }})
        .to_string();
        let (nodes, references) = evaluate(&files(&[("main.tf.json", &source)]));
        assert_eq!(nodes.len(), 3);
        let actual: Vec<_> = references
            .iter()
            .map(|r| (r.source.as_str(), r.target.as_str()))
            .collect();
        assert_eq!(actual, [("b.ordinary", "a.target")]);
    }

    #[test]
    fn terraform_json_references_keep_the_first_in_address_order() {
        let names: Vec<_> = (0..REFERENCES_MAX + 5)
            .map(|i| format!("a.r{i:03}"))
            .collect();
        let resources: serde_json::Map<String, serde_json::Value> = (0..names.len())
            .map(|i| (format!("r{i:03}"), serde_json::json!({})))
            .collect();
        let source = serde_json::json!({"resource": {
            "a": resources,
            "b": {"all": {"depends_on": names.iter().rev().collect::<Vec<_>>()}}
        }})
        .to_string();
        let (_, references) = evaluate(&files(&[("main.tf.json", &source)]));
        let actual: Vec<_> = references.iter().map(|r| &r.target).collect();
        assert_eq!(actual, names[..REFERENCES_MAX].iter().collect::<Vec<_>>());
    }

    #[test]
    fn the_pure_functions_resolve_names_and_any_other_call_stays_unresolved() {
        let text = concat!(
            "locals {\n  svc = \"orders\"\n  tags = { team = \"shop\" }\n  m = { zeta = \"zz\", alpha = \"aa\" }\n}\n",
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
            "resource \"a\" \"set\" { name = \"${length(toset([1, 1.0, { a = 1, b = 2 }, { b = 2, a = 1 }, \"1\"]))}\" }\n",
            "resource \"a\" \"length\" { name = \"${length(local.svc)}-${tostring(tonumber(\"2\") + 1)}\" }\n",
            "resource \"a\" \"json\" { name = jsonencode({ a = \"<b>\", n = [1, true] }) }\n",
            "resource \"a\" \"pattern\" { name = replace(local.svc, \"/o+/\", \"0\") }\n",
            "resource \"a\" \"file\" { name = \"q-${file(\"name.txt\")}\" }\n",
            "resource \"a\" \"verb\" { name = format(\"%x\", 10) }\n",
            "resource \"a\" \"wide\" { name = format(\"%257d\", 1) }\n",
            "resource \"a\" \"empty\" { name = \"q-${length(split(\",\", \"\"))}\" }\n",
            "resource \"a\" \"forobj\" { name = join(\"-\", [for k, v in local.m : v]) }\n",
            "resource \"a\" \"directive\" { name = \"%{ for k, v in local.m }${v}%{ endfor }-q\" }\n",
            "resource \"a\" \"first\" { name = \"first-${[for k, v in local.m : k][0]}\" }\n",
            "resource \"a\" \"ordered\" { name = jsonencode(local.m) }\n",
            "resource \"a\" \"merged\" { name = jsonencode(merge(local.m, { beta = \"bb\" })) }\n",
            "resource \"a\" \"literal\" { name = join(\"-\", [for k, v in { zeta = 1, \"alpha\" = 2 } : k]) }\n",
            "resource \"a\" \"mergefor\" { name = \"m-${[for k, v in merge({ zeta = 1 }, { alpha = 2 }) : k][0]}\" }\n",
            "resource \"a\" \"comprehension\" { name = jsonencode({ for s in [\"b\", \"a\"] : s => s }) }\n",
            "resource \"a\" \"overcomp\" { name = join(\"-\", [for k, v in { for s in [\"b\", \"a\"] : s => s } : k]) }\n",
            "resource \"a\" \"computed\" { name = join(\"-\", [for k, v in { (local.svc) = 1, alpha = 2 } : k]) }\n",
            "resource \"a\" \"inner\" { name = [for s in [\"x\"] : [for k, v in { zeta = s, alpha = 1 } : k][0]][0] }\n",
            "resource \"a\" \"inject\" { name = \"inj-${meada_exact(\"+\", 1, 2)}\" }\n",
            "resource \"a\" \"shadowcall\" { name = [for meada_inexact in [\"x\"] : meada_exact(\"+\", 1, 2)][0] }\n",
            "resource \"a\" \"shadowfor\" { name = [for meada_inexact in [[\"y\"]] : [for k, v in { (local.svc) = 1 } : v][0]][0] }\n",
            "resource \"a\" \"keyarith\" { name = keys({ (0 - 18446744073709551615) = 1 })[0] }\n",
            "resource \"a\" \"keyinject\" { name = keys({ (meada_exact(\"+\", 1, 2)) = 1 })[0] }\n",
            "resource \"a\" \"keyfor\" { name = [for x in [0] : keys({ (x - 18446744073709551615) = 1 })[0]][0] }\n",
            "resource \"a\" \"keyok\" { name = keys({ (\"k-${2 * 3}\") = 1 })[0] }\n",
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
                "a.set ~ \"*\"",
                "a.length = \"6-3\"",
                "a.json = \"{\\\"a\\\":\\\"\\\\u003cb\\\\u003e\\\",\\\"n\\\":[1,true]}\"",
                "a.pattern ~ \"*\"",
                "a.file ~ \"q-*\"",
                "a.verb ~ \"*\"",
                "a.wide ~ \"*\"",
                "a.empty = \"q-1\"",
                "a.forobj = \"aa-zz\"",
                "a.directive ~ \"*-q\"",
                "a.first = \"first-alpha\"",
                "a.ordered = \"{\\\"alpha\\\":\\\"aa\\\",\\\"zeta\\\":\\\"zz\\\"}\"",
                "a.merged = \"{\\\"alpha\\\":\\\"aa\\\",\\\"beta\\\":\\\"bb\\\",\\\"zeta\\\":\\\"zz\\\"}\"",
                "a.literal = \"alpha-zeta\"",
                "a.mergefor = \"m-alpha\"",
                "a.comprehension = \"{\\\"a\\\":\\\"a\\\",\\\"b\\\":\\\"b\\\"}\"",
                "a.overcomp ~ \"*\"",
                "a.computed ~ \"*\"",
                "a.inner = \"alpha\"",
                "a.inject ~ \"inj-*\"",
                "a.shadowcall ~ \"*\"",
                "a.shadowfor ~ \"*\"",
                "a.keyarith ~ \"*\"",
                "a.keyinject ~ \"*\"",
                "a.keyfor ~ \"*\"",
                "a.keyok = \"k-6\"",
            ]
        );
    }

    #[test]
    fn a_variable_marked_sensitive_by_a_string_is_never_resolved() {
        let tf = concat!(
            "variable \"s\" {\n  sensitive = \"true\"\n  default = \"tok\"\n}\n",
            "resource \"a\" \"s\" { name = \"q-${var.s}\" }\n",
        );
        let json = r#"{"variable": {"s": {"sensitive": "true", "default": "tok"}},
 "resource": {"a": {"s": {"name": "q-${var.s}"}}}}"#;
        assert_eq!(names(&[("main.tf", tf)]), ["a.s ~ \"q-*\""]);
        assert_eq!(names(&[("main.tf.json", json)]), ["a.s ~ \"q-*\""]);
    }

    #[test]
    fn a_variable_an_override_file_marks_sensitive_is_never_resolved() {
        let main = concat!(
            "variable \"s\" {\n  default = \"tok\"\n}\n",
            "resource \"a\" \"s\" { name = \"q-${var.s}\" }\n",
        );
        let marked = "variable \"s\" {\n  sensitive = true\n}\n";
        for file in ["a_override.tf", "override.tf", "z_override.tf.json"] {
            let marked = match file.ends_with(".json") {
                true => r#"{"variable": {"s": {"sensitive": true}}}"#,
                false => marked,
            };
            let list = [("main.tf", main), (file, marked)];
            assert_eq!(names(&list), ["a.s ~ \"q-*\""], "{file}");
        }
        let given = [
            (
                "main.tf",
                "variable \"s\" {\n  sensitive = true\n}\nresource \"a\" \"s\" { name = \"q-${var.s}\" }\n",
            ),
            ("override.tf", "variable \"s\" {\n  default = \"tok\"\n}\n"),
        ];
        assert_eq!(names(&given), ["a.s ~ \"q-*\""]);
        let json = [
            (
                "main.tf.json",
                r#"{"variable": {"s": {"default": "tok"}}, "resource": {"a": {"s": {"name": "q-${var.s}"}}}}"#,
            ),
            ("a_override.tf", marked),
        ];
        assert_eq!(names(&json), ["a.s ~ \"q-*\""]);
    }

    #[test]
    fn override_files_merge_into_the_blocks_of_their_directory() {
        let main = concat!(
            "variable \"env\" {\n  default = \"dev\"\n}\n",
            "variable \"team\" {\n  default = \"shop\"\n}\n",
            "locals {\n  suffix = \"q\"\n}\n",
            "resource \"a\" \"q\" { name = \"${var.env}-${var.team}-${local.suffix}\" }\n",
            "resource \"a\" \"r\" {\n  name = \"keep\"\n  count = 1\n}\n",
            "module \"m\" {\n  source = \"./m\"\n  name = \"inner-${var.env}\"\n}\n",
            "resource \"a\" \"out\" { name = module.m.full }\n",
            "resource \"a\" \"j\" { name = \"hcl\" }\n",
        );
        let module = "variable \"name\" {}\noutput \"full\" { value = \"${var.name}-out\" }\n";
        let list = [
            ("main.tf", main),
            ("m/main.tf", module),
            (
                "a_override.tf",
                concat!(
                    "variable \"env\" {\n  default = \"prd\"\n}\n",
                    "locals {\n  suffix = \"z\"\n}\n",
                    "resource \"a\" \"r\" { count = 2 }\n",
                    "resource \"a\" \"ghost\" { name = \"never\" }\n",
                ),
            ),
            (
                "override.tf",
                "variable \"team\" {\n  description = \"y\"\n}\n",
            ),
            (
                "z_override.tf.json",
                r#"{"resource": {"a": {"j": {"name": "json-${var.team}"}}}}"#,
            ),
            (
                "m/override.tf",
                "output \"full\" { value = \"${var.name}-ovr\" }\n",
            ),
        ];
        assert_eq!(
            names(&list),
            [
                "a.q = \"prd-shop-z\" [var.env a_override.tf:1] [var.team main.tf:4]",
                "a.r[0] = \"keep\"",
                "a.r[1] = \"keep\"",
                "module.m",
                "a.out = \"inner-prd-ovr\" [var.env a_override.tf:1]",
                "a.j = \"json-shop\" [var.team main.tf:4]",
            ]
        );
    }

    #[test]
    fn an_override_block_replaces_the_references_of_what_it_sets() {
        let (_, references) = evaluate(&files(&[
            (
                "main.tf",
                concat!(
                    "resource \"b\" \"y\" {}\nresource \"b\" \"z\" {}\nresource \"b\" \"w\" {}\n",
                    "resource \"a\" \"x\" {\n  target = b.y.id\n  keep = b.w.id\n",
                    "  rule { from = b.y.id }\n  rule { from = b.y.arn }\n}\n",
                ),
            ),
            (
                "x_override.tf",
                "resource \"a\" \"x\" {\n  target = b.z.id\n  rule { from = b.z.id }\n}\n",
            ),
        ]));
        assert_eq!(shown(&references), ["a.x -> b.w", "a.x -> b.z"]);
    }

    #[test]
    fn a_json_dynamic_block_counts_as_the_type_it_generates() {
        let dynamic = |target: &str| {
            format!(
                r#"{{"dynamic": {{"rule": [{{"for_each": [1], "content": {{"from": "${{{target}}}"}}}}]}}}}"#
            )
        };
        let targets = "resource \"b\" \"y\" {}\nresource \"b\" \"z\" {}\nresource \"b\" \"w\" {}\n";
        let base = format!(
            "{targets}resource \"a\" \"x\" {{\n  rule {{ from = b.y.id }}\n  other {{ from = b.w.id }}\n}}\n"
        );
        let over = format!(r#"{{"resource": {{"a": {{"x": {}}}}}}}"#, dynamic("b.z.id"));
        let (_, references) =
            evaluate(&files(&[("main.tf", &base), ("x_override.tf.json", &over)]));
        assert_eq!(shown(&references), ["a.x -> b.w", "a.x -> b.z"]);
        let base = format!(r#"{{"resource": {{"a": {{"x": {}}}}}}}"#, dynamic("b.y.id"));
        let over = "resource \"a\" \"x\" {\n  rule { from = b.z.id }\n}\n";
        let list = [
            ("b.tf", targets),
            ("main.tf.json", &base),
            ("x_override.tf", over),
        ];
        assert_eq!(shown(&evaluate(&files(&list)).1), ["a.x -> b.z"]);
    }

    #[test]
    fn a_json_file_whose_root_is_an_array_of_bodies_is_read() {
        let json = r#"[{"variable": {"env": {"default": "dev"}}},
 {"resource": {"a": {"r": {"name": "q-${var.env}"}}}}]"#;
        assert_eq!(
            names(&[("main.tf.json", json)]),
            ["a.r = \"q-dev\" [var.env main.tf.json:1]"]
        );
        let main = "resource \"a\" \"q\" { name = \"q-base\" }\n";
        let over = r#"[{"resource": {"a": {"q": {"name": "q-over"}}}}]"#;
        let list = [("main.tf", main), ("override.tf.json", over)];
        assert_eq!(names(&list), ["a.q = \"q-over\""]);
        let main = "variable \"s\" {\n  default = \"tok\"\n}\nresource \"a\" \"s\" { name = \"q-${var.s}\" }\n";
        let marked = r#"[{"variable": {"s": {"sensitive": true}}}]"#;
        let list = [("main.tf", main), ("override.tf.json", marked)];
        assert_eq!(names(&list), ["a.s ~ \"q-*\""]);
    }

    #[test]
    fn no_variable_resolves_beside_an_override_file_and_a_file_that_does_not_read() {
        let main = concat!(
            "variable \"s\" {\n  default = \"tok\"\n}\n",
            "resource \"a\" \"s\" { name = \"q-${var.s}\" }\n",
        );
        let big = format!("# {}\n", "x".repeat(MAX_BYTES));
        let cases: [&[(&str, &str)]; 4] = [
            &[("main.tf", main), ("x_override.tf", "variable {{{")],
            &[("main.tf", main), ("a_override.tf.json", "{")],
            &[
                ("main.tf", main),
                ("broken.tf.json", "{"),
                ("override.tf", "locals {}\n"),
            ],
            &[("main.tf", main), ("big_override.tf", big.as_str())],
        ];
        for list in cases {
            assert_eq!(names(list), ["a.s ~ \"q-*\""], "{}", list[1].0);
        }
        let alone = [("main.tf", main), ("broken.tf", "variable {{{")];
        assert_eq!(names(&alone), ["a.s = \"q-tok\" [var.s main.tf:1]"]);
    }

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

    #[test]
    fn a_name_through_more_overridable_values_than_the_ceiling_is_unresolved() {
        let name = |n: usize| {
            let vars: String = (0..n)
                .map(|i| format!("variable \"v{i}\" {{\n  default = \"v\"\n}}\n"))
                .collect();
            let parts: String = (0..n).map(|i| format!("${{var.v{i}}}")).collect();
            let main = format!("{vars}resource \"a\" \"q\" {{ name = \"{parts}\" }}\n");
            let (nodes, _) = evaluate(&files(&[("main.tf", main.as_str())]));
            let q = &nodes[0];
            (q.name.clone(), q.pattern, q.overridable.len())
        };
        let within = (Some("v".repeat(OVERRIDABLE_MAX)), false, OVERRIDABLE_MAX);
        assert_eq!(name(OVERRIDABLE_MAX), within);
        assert_eq!(name(OVERRIDABLE_MAX + 1), (Some("*".into()), true, 0));
    }

    #[test]
    fn arithmetic_the_evaluator_cannot_do_leaves_the_value_unresolved() {
        let list = [
            (
                "main.tf",
                concat!(
                    "variable \"n\" {\n  default = 1\n}\n",
                    "variable \"k\" {\n  default = 9\n}\n",
                    "variable \"f\" {\n  default = 9\n}\n",
                    "variable \"b\" {\n  default = 9\n}\n",
                    "resource \"a\" \"rem\" { name = \"orders-${5 % 0}\" }\n",
                    "resource \"a\" \"sub\" { name = substr(\"abc\", 1, 9223372036854775807) }\n",
                    "resource \"a\" \"var\" { name = \"q-${var.n}\" }\n",
                    "resource \"a\" \"vk\" { name = \"q-${var.k}\" }\n",
                    "resource \"a\" \"vf\" { name = \"q-${var.f}\" }\n",
                    "resource \"a\" \"vb\" { name = \"q-${var.b}\" }\n",
                    "resource \"a\" \"neg\" { name = \"orders-${0 - 18446744073709551615}\" }\n",
                    "resource \"a\" \"wrap\" { name = \"orders-${9223372036854775807 * 3}\" }\n",
                    "resource \"a\" \"div\" { name = \"orders-${5 / 0}\" }\n",
                    "resource \"a\" \"half\" { name = \"orders-${7 / 2}\" }\n",
                    "resource \"a\" \"float\" { name = \"orders-${0.1 + 0.2}\" }\n",
                    "resource \"a\" \"big\" { name = \"orders-${18446744073709551615 - 1}\" }\n",
                    "resource \"a\" \"low\" { name = \"orders-${-9223372036854775807 - 1}\" }\n",
                    "resource \"a\" \"mod\" { name = \"orders-${-7 % 3}\" }\n",
                    "resource \"a\" \"loop\" { name = join(\"-\", [for x in [1, 2] : x + 1]) }\n",
                    "resource \"a\" \"loopneg\" { name = \"orders-${[for x in [0] : x - 18446744073709551615][0]}\" }\n",
                    "resource \"a\" \"loophalf\" { name = \"orders-${[for x in [7] : x / 2][0]}\" }\n",
                    "resource \"a\" \"loopwrap\" { name = \"orders-${[for x in [9223372036854775807] : -x * 3][0]}\" }\n",
                    "resource \"a\" \"loopbig\" { name = \"orders-${[for x in [18446744073709551615] : x - 1][0]}\" }\n",
                    "resource \"a\" \"shadow\" { name = \"orders-${[for meada_inexact in [5] : 7 / 2][0]}\" }\n",
                ),
            ),
            (
                "terraform.tfvars",
                "n = 7 % 0\nk = 7 / 2\nf = 0.1 + 0.2\nb = 0 - 18446744073709551615\n",
            ),
        ];
        assert_eq!(
            names(&list),
            [
                "a.rem ~ \"orders-*\"",
                "a.sub = \"bc\"",
                "a.var ~ \"q-*\" [var.n terraform.tfvars:1 tfvars]",
                "a.vk ~ \"q-*\" [var.k terraform.tfvars:2 tfvars]",
                "a.vf ~ \"q-*\" [var.f terraform.tfvars:3 tfvars]",
                "a.vb ~ \"q-*\" [var.b terraform.tfvars:4 tfvars]",
                "a.neg ~ \"orders-*\"",
                "a.wrap ~ \"orders-*\"",
                "a.div ~ \"orders-*\"",
                "a.half ~ \"orders-*\"",
                "a.float ~ \"orders-*\"",
                "a.big = \"orders-18446744073709551614\"",
                "a.low = \"orders--9223372036854775808\"",
                "a.mod = \"orders--1\"",
                "a.loop = \"2-3\"",
                "a.loopneg ~ \"orders-*\"",
                "a.loophalf ~ \"orders-*\"",
                "a.loopwrap ~ \"orders-*\"",
                "a.loopbig = \"orders-18446744073709551614\"",
                "a.shadow ~ \"orders-*\"",
            ]
        );
    }

    #[test]
    fn a_number_terraform_prints_otherwise_than_a_float_stays_unresolved() {
        let main = [
            ("big", "${18446744073709551616}"),
            ("negbig", "${-18446744073709551615}"),
            ("low", "${-9223372036854775809}"),
            ("exp", "${1e20}"),
            ("tiny", "${0.0000001}"),
            ("long", "${0.10000000000000000001}"),
            ("dot", "${9007199254740993.0}"),
            ("numbig", "${tonumber(\"18446744073709551616\")}"),
            ("numlong", "${tonumber(\"0.10000000000000000001\")}"),
            ("half", "${1.5}"),
            ("tenth", "${0.1}"),
            ("one", "${1.0}"),
            ("sci", "${1.5e2}"),
            ("max", "${18446744073709551615}"),
            ("min", "${-9223372036854775808}"),
            ("comment", "${-/*c*/7}"),
            ("negzero", "${-0}"),
            ("commentzero", "${-/*c*/0}"),
            ("negof", "${-(0)}"),
            ("mulzero", "${0 * -1}"),
            ("divzero", "${0 / -1}"),
            ("zero", "${0 * 1}"),
            ("subzero", "${0 - 0}"),
            ("numhalf", "${tonumber(\"0.5\")}"),
            ("nummax", "${tonumber(\"18446744073709551615\")}"),
            ("vj", "${var.j}"),
            ("vk", "${var.k}"),
            ("vl", "${var.l}"),
            ("vm", "${var.m}"),
            ("vn", "${var.n}"),
            ("lj", "${local.j}"),
        ];
        let mut tf = String::new();
        for v in ["j", "k", "l", "m", "n"] {
            tf += &format!("variable \"{v}\" {{}}\n");
        }
        for (name, value) in main {
            tf += &format!("resource \"a\" \"{name}\" {{ name = \"q-{value}\" }}\n");
        }
        let list = [
            ("main.tf", tf.as_str()),
            ("locals.tf.json", "{\"locals\": {\"j\": 0.5}}"),
            (
                "terraform.tfvars.json",
                "{\"j\": 0.5, \"k\": 18446744073709551616, \"l\": 7, \"m\": -9223372036854775809, \"n\": 1.0}",
            ),
        ];
        assert_eq!(
            names(&list),
            [
                "a.big ~ \"q-*\"",
                "a.negbig ~ \"q-*\"",
                "a.low ~ \"q-*\"",
                "a.exp ~ \"q-*\"",
                "a.tiny ~ \"q-*\"",
                "a.long ~ \"q-*\"",
                "a.dot ~ \"q-*\"",
                "a.numbig ~ \"q-*\"",
                "a.numlong ~ \"q-*\"",
                "a.half = \"q-1.5\"",
                "a.tenth = \"q-0.1\"",
                "a.one = \"q-1\"",
                "a.sci = \"q-150\"",
                "a.max = \"q-18446744073709551615\"",
                "a.min = \"q--9223372036854775808\"",
                "a.comment = \"q--7\"",
                "a.negzero ~ \"q-*\"",
                "a.commentzero ~ \"q-*\"",
                "a.negof ~ \"q-*\"",
                "a.mulzero ~ \"q-*\"",
                "a.divzero ~ \"q-*\"",
                "a.zero = \"q-0\"",
                "a.subzero = \"q-0\"",
                "a.numhalf = \"q-0.5\"",
                "a.nummax = \"q-18446744073709551615\"",
                "a.vj ~ \"q-*\" [var.j terraform.tfvars.json:1 tfvars]",
                "a.vk ~ \"q-*\" [var.k terraform.tfvars.json:1 tfvars]",
                "a.vl = \"q-7\" [var.l terraform.tfvars.json:1 tfvars]",
                "a.vm ~ \"q-*\" [var.m terraform.tfvars.json:1 tfvars]",
                "a.vn ~ \"q-*\" [var.n terraform.tfvars.json:1 tfvars]",
                "a.lj ~ \"q-*\"",
            ]
        );
    }

    #[test]
    fn a_var_file_evaluates_templates_and_for_arithmetic() {
        let main = concat!(
            "variable \"env\" {\n  default = \"dev\"\n}\n",
            "variable \"n\" {}\n",
            "variable \"b\" {}\n",
            "variable \"c\" {\n  default = \"dev\"\n}\n",
            "variable \"d\" {\n  default = \"dev\"\n}\n",
            "variable \"e\" {}\n",
            "variable \"f\" {\n  default = \"dev\"\n}\n",
            "variable \"g\" {}\n",
            "variable \"h\" {}\n",
            "variable \"i\" {\n  default = \"dev\"\n}\n",
            "variable \"j\" {}\n",
            "resource \"a\" \"env\" { name = \"orders-${var.env}\" }\n",
            "resource \"a\" \"n\" { name = var.n }\n",
            "resource \"a\" \"b\" { name = \"q-${var.b}\" }\n",
            "resource \"a\" \"c\" { name = var.c }\n",
            "resource \"a\" \"d\" { name = var.d }\n",
            "resource \"a\" \"e\" { name = var.e }\n",
            "resource \"a\" \"f\" { name = var.f }\n",
            "resource \"a\" \"g\" { name = \"q-${var.g}\" }\n",
            "resource \"a\" \"h\" { name = var.h }\n",
            "resource \"a\" \"i\" { name = \"q-${var.i}\" }\n",
            "resource \"a\" \"j\" { name = var.j }\n",
            "resource \"a\" \"k\" {\n  name = chomp(<<EOT\nq-${{\n  b = 2 # note\n  a = 1\n}[\"a\"]}\nEOT\n  )\n}\n",
        );
        let tfvars = concat!(
            "env = \"prd-${\"eu\"}\"\n",
            "n = \"%{for x in [\"a\", \"b\"]}${x}%{endfor}-${1}\"\n",
            "b = [for x in [1, 2] : x * 2][1]\n",
            "c = \"pre-${null}\"\n",
            "d = upper(\"x\")\n",
            "e = \"%{for k, v in {b = 1, a = 2}}${k}${v}%{endfor}\"\n",
            "f = \"%{for x in [meada_exact(\"+\", 1, 2)]}${x}%{endfor}\"\n",
            "g = \"${{b = 1, a = 2}[\"a\"]}\"\n",
            "h = \"%{for x in [-0]}${x}%{endfor}\"\n",
            "i = \"${{\n  b = 1 # note\n  a = 2\n}[\"a\"]}\"\n",
            "j = \"%{for k, v in {\n  b = 1 // note\n  a = 2\n}}${k}%{endfor}\"\n",
        );
        assert_eq!(
            names(&[("main.tf", main), ("terraform.tfvars", tfvars)]),
            [
                "a.env = \"orders-prd-eu\" [var.env terraform.tfvars:1 tfvars]",
                "a.n = \"ab-1\" [var.n terraform.tfvars:2 tfvars]",
                "a.b = \"q-4\" [var.b terraform.tfvars:3 tfvars]",
                "a.c = \"dev\" [var.c main.tf:6]",
                "a.d = \"dev\" [var.d main.tf:9]",
                "a.e = \"a2b1\" [var.e terraform.tfvars:6 tfvars]",
                "a.f = \"dev\" [var.f main.tf:13]",
                "a.g = \"q-2\" [var.g terraform.tfvars:8 tfvars]",
                "a.h ~ \"*\" [var.h terraform.tfvars:9 tfvars]",
                "a.i = \"q-2\" [var.i terraform.tfvars:10 tfvars]",
                "a.j = \"ab\" [var.j terraform.tfvars:14 tfvars]",
                "a.k = \"q-1\"",
            ]
        );
    }

    #[test]
    fn exact_names_agree_with_the_terraform_oracle() {
        let cases: Vec<_> = (include_str!("../tests/terraform-oracle/cases.tsv").lines())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| match line.split('\t').collect::<Vec<_>>()[..] {
                [must, expression, terraform] => (must == "=", expression, terraform),
                _ => panic!("not a case: {line}"),
            })
            .collect();
        let tf: String = (cases.iter().enumerate())
            .map(|(i, (_, e, _))| format!("resource \"a\" \"r{i}\" {{ name = \"q-${{{e}}}\" }}\n"))
            .collect();
        let found: HashMap<_, _> = (names(&[("main.tf", &tf)]).into_iter())
            .filter_map(|line| Some((line.split_once(' ')?.0.to_string(), line)))
            .collect();
        let wrong: Vec<_> = (cases.iter().enumerate())
            .filter_map(|(i, &(must, expression, terraform))| {
                let line = found.get(&format!("a.r{i}"))?;
                let exact = line.split_once(" = ").map(|(_, name)| name);
                let right = match exact {
                    Some(name) => !terraform.starts_with('!') && name == format!("{terraform:?}"),
                    None => !must,
                };
                (!right).then(|| format!("{expression}: Terraform {terraform}, Meada {line}"))
            })
            .collect();
        assert_eq!(found.len(), cases.len());
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn a_for_over_a_converted_list_of_objects_expands() {
        let main = concat!(
            "variable \"items\" {}\n",
            "resource \"a\" \"q\" {\n",
            "  for_each = {for o in tolist(var.items) : o.name => o}\n",
            "  name = \"q-${each.key}\"\n}\n",
        );
        let items: String = (0..40)
            .map(|i| {
                format!(
                    "  {{ name = \"s{i:02}\", port = {i}, tier = \"backend-tier\", d = null }},\n"
                )
            })
            .collect();
        let tfvars = format!("items = [\n{items}]\n");
        let got = names(&[("main.tf", main), ("terraform.tfvars", &tfvars)]);
        assert_eq!(got.len(), 40, "{got:?}");
        assert!(got.iter().all(|n| n.contains(" = \"q-s")), "{got:?}");
    }

    #[test]
    fn a_for_over_a_literal_collection_costs_as_over_a_bound_one() {
        let list = (0..40)
            .map(|i| format!("{{ name = \"s{i:02}\", size = {i} }}"))
            .collect::<Vec<_>>()
            .join(", ");
        let object = (0..40)
            .map(|i| format!("s{i:02} = {{ size = {i}, tier = \"backend-tier\" }}"))
            .collect::<Vec<_>>()
            .join(", ");
        let numbers = (0..400).map(|i| i.to_string()).collect::<Vec<_>>();
        let numbers = numbers.join(", ");
        let main = format!(
            "resource \"a\" \"l\" {{\n  for_each = {{ for o in [{list}] : o.name => o }}\n  name = \"l-${{each.key}}\"\n}}\n\
             resource \"a\" \"o\" {{\n  for_each = {{ for k, v in {{ {object} }} : k => v }}\n  name = \"o-${{each.key}}\"\n}}\n\
             resource \"a\" \"n\" {{\n  name = \"n-${{length([for a in [{numbers}] : [for b in [{numbers}] : \"abcdefgh\"]])}}\"\n}}\n"
        );
        let got = names(&[("main.tf", &main)]);
        let of = |address: &str| -> Vec<String> {
            (got.iter().filter(|n| n.starts_with(address)).cloned()).collect()
        };
        let named = |r: &str| -> Vec<String> {
            (0..40)
                .map(|i| format!("a.{r}[\"s{i:02}\"] = \"{r}-s{i:02}\""))
                .collect()
        };
        assert_eq!(
            (of("a.l"), of("a.o"), of("a.n")),
            (named("l"), named("o"), vec!["a.n ~ \"n-*\"".to_string()])
        );
    }

    #[test]
    fn a_literal_collection_is_charged_its_elements_once() {
        let numbers = (0..400).map(|i| i.to_string()).collect::<Vec<_>>();
        let numbers = numbers.join(", ");
        let two = format!("[for a in [{numbers}] : [for b in [{numbers}] : \"abcdefgh\"]]");
        let main = format!(
            "resource \"a\" \"u\" {{ name = \"u-${{length([for x in [{two}] : 1])}}\" }}\n\
             resource \"a\" \"w\" {{ name = \"w-${{length([for k, v in {{ x = {two} }} : 1])}}\" }}\n"
        );
        assert_eq!(
            names(&[("main.tf", &main)]),
            ["a.u ~ \"u-*\"", "a.w ~ \"w-*\""]
        );
        let three = format!(
            "length([for x in [[for a in [{numbers}] : [for b in [{numbers}] : [for c in [{numbers}] : 1]]]] : 1])"
        );
        let expr = parser::parse_expr(&three).unwrap();
        assert!(steps(&expr, &Map::new()) > EVAL_COST_MAX);
    }

    #[test]
    fn join_formatlist_and_replace_are_costed_by_their_output() {
        let s = "a-".repeat(1000);
        let n = (0..400)
            .map(|i| format!("\"item-{i:03}\""))
            .collect::<Vec<_>>();
        let n = n.join(", ");
        let main = format!(
            "variable \"l\" {{ default = [\"a\", \"b\", \"c\", \"d\"] }}\n\
             variable \"m\" {{ default = [\"a\", \"b\", \"c\"] }}\n\
             variable \"n\" {{ default = [{n}] }}\n\
             variable \"s\" {{ default = \"{s}\" }}\n\
             resource \"a\" \"j\" {{ name = join(\"-\", [join(\"-\", [join(\",\", [\"a\"])])]) }}\n\
             resource \"a\" \"k\" {{ name = join(\"-\", [join(\",\", var.l), join(\",\", var.l)]) }}\n\
             resource \"a\" \"f\" {{ name = \"q-${{join(\"\", [for a in var.m : join(\"\", [for b in var.m : \"${{a}}${{b}}\"])])}}\" }}\n\
             resource \"a\" \"g\" {{ name = join(\",\", formatlist(\"q-%s\", var.n)) }}\n\
             resource \"a\" \"r\" {{ name = replace(var.s, \"-\", \"_\") }}\n"
        );
        let items: Vec<String> = (0..400).map(|i| format!("q-item-{i:03}")).collect();
        let want = [
            "a.j = \"a\"".to_string(),
            "a.k = \"a,b,c,d-a,b,c,d\" [var.l main.tf:1]".to_string(),
            "a.f = \"q-aaabacbabbbccacbcc\" [var.m main.tf:2]".to_string(),
            format!("a.g = \"{}\" [var.n main.tf:3]", items.join(",")),
            format!("a.r = \"{}\" [var.s main.tf:4]", "a_".repeat(1000)),
        ];
        assert_eq!(names(&[("main.tf", &main)]), want);
    }

    #[test]
    fn a_join_formatlist_or_replace_past_the_cost_ceiling_is_refused() {
        let (wide, a, to) = ("x".repeat(2000), "a".repeat(2000), "y".repeat(1000));
        let list = (0..1000).map(|i| i.to_string()).collect::<Vec<_>>();
        let list = list.join(", ");
        let escaped = vec![format!("\"{}\"", "\\u0001".repeat(200)); 1000].join(", ");
        let n = "${1.7976931348623157e308}";
        let (empty, many) = (vec!["\"\""; 6300].join(", "), vec!["\"\""; 975].join(", "));
        let empty_for = vec!["\"%{for x in []}%{endfor}\""; 2000].join(", ");
        for text in [
            format!("join(\"{wide}\", [{list}])"),
            format!("formatlist(\"%s-%s\", \"{wide}\", [{list}])"),
            format!("replace(\"{a}\", \"a\", \"{to}\")"),
            format!("join([\"{wide}\", [{list}]]...)"),
            format!("formatlist([\"%s-%s\", \"{wide}\", [{list}]]...)"),
            format!("replace([\"{a}\", \"a\", \"{to}\"]...)"),
            format!("formatlist(\"%s-%s\", \"{wide}\", [for i in [{list}] : i])"),
            format!("formatlist(\"%q\", [{escaped}])"),
            format!("replace(\"{}\", \"a\", \"x{n}\")", "a".repeat(95_000)),
            format!("join(\"{}\", [{empty}])", n.repeat(10)),
            format!("formatlist(\"x{}\", [{many}])", n.repeat(100)),
            format!("join(\"{}\", [{empty_for}])", "x".repeat(100_000)),
        ] {
            let expr = parser::parse_expr(&text).unwrap();
            assert!(steps(&expr, &Map::new()) > EVAL_COST_MAX, "{text}");
        }
    }

    #[test]
    fn a_join_formatlist_or_replace_the_square_accepted_stays_named() {
        let items = |n: usize| (0..n).map(|i| format!("\"i-{i:02}\"")).collect::<Vec<_>>();
        let (p, q) = (items(60).join(", "), items(70).join(", "));
        let (s, t) = ("b".repeat(398), "y".repeat(398));
        let (u, w) = ("b".repeat(500), "y".repeat(507));
        let main = format!(
            "variable \"p\" {{ default = [{p}] }}\n\
             variable \"q\" {{ default = [{q}] }}\n\
             variable \"s\" {{ default = \"{s}\" }}\n\
             variable \"t\" {{ default = \"{t}\" }}\n\
             variable \"u\" {{ default = \"{u}\" }}\n\
             variable \"w\" {{ default = \"{w}\" }}\n\
             resource \"a\" \"p\" {{ name = formatlist(\"q-%s\", [for x in var.p : x])[0] }}\n\
             resource \"a\" \"e\" {{ name = element(formatlist(\"q-%s\", tolist(var.q)), 0) }}\n\
             resource \"a\" \"t\" {{ name = replace(var.s, \"a\", var.t) }}\n\
             resource \"a\" \"d\" {{ name = replace(\"${{var.u}}\", \"a\", \"${{var.w}}\") }}\n"
        );
        let want = [
            "a.p = \"q-i-00\" [var.p main.tf:1]".to_string(),
            "a.e = \"q-i-00\" [var.q main.tf:2]".to_string(),
            format!("a.t = \"{s}\" [var.s main.tf:3] [var.t main.tf:4]"),
            format!("a.d = \"{u}\" [var.u main.tf:5] [var.w main.tf:6]"),
        ];
        assert_eq!(names(&[("main.tf", &main)]), want);
    }

    #[test]
    fn a_key_over_a_set_is_unresolved() {
        let main = concat!(
            "resource \"a\" \"q\" {\n",
            "  for_each = {for k, v in toset([\"x\", \"y\"]) : k => v}\n",
            "  name = \"q-${each.key}\"\n}\n",
            "resource \"a\" \"t\" { name = \"t-%{for k, v in toset([\"x\"])}${k}%{endfor}\" }\n",
            "resource \"a\" \"v\" { name = \"v-${[for v in toset([\"y\", \"x\"]) : v][0]}\" }\n",
            "resource \"a\" \"p\" { name = \"p-%{for k, v in (toset([\"x\"]))}${k}%{endfor}\" }\n",
            "resource \"a\" \"c\" {\n",
            "  for_each = {for k, v in (true ? toset([\"x\"]) : []) : k => v}\n",
            "  name = \"c-${each.key}\"\n}\n",
        );
        let got = names(&[("main.tf", main)]);
        let want = [
            "a.q ~ \"q-*\"",
            "a.t ~ \"t-*\"",
            "a.v = \"v-x\"",
            "a.p ~ \"p-*\"",
            "a.c ~ \"c-*\"",
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn a_key_over_a_named_set_is_unresolved() {
        let each = "  for_each = {for k, v in local.s : k => v}\n  name = \"q-${each.key}\"\n}\n";
        let q = format!("resource \"a\" \"q\" {{\n{each}");
        let case = |locals: &str, extra: &str| {
            let main = format!("locals {{\n{locals}}}\n{extra}{q}");
            names(&[("main.tf", &main)])
        };
        assert_eq!(case("  s = toset([\"x\"])\n", ""), ["a.q ~ \"q-*\""]);
        assert_eq!(
            case("  t = toset([\"x\"])\n  s = local.t\n", ""),
            ["a.q ~ \"q-*\""]
        );
        assert_eq!(case("  s = (toset([\"x\"]))\n", ""), ["a.q ~ \"q-*\""]);
        assert_eq!(
            case("  t = toset([\"x\"])\n  s = \"${local.t}\"\n", ""),
            ["a.q ~ \"q-*\""]
        );
        assert_eq!(
            case("  s = true ? toset([\"x\"]) : []\n", ""),
            ["a.q ~ \"q-*\""]
        );
        assert_eq!(
            case(
                "  s = toset([\"x\"])\n",
                "resource \"a\" \"d\" { name = \"d-%{for k, v in local.s}${k}%{endfor}\" }\n"
            ),
            ["a.d ~ \"d-*\"", "a.q ~ \"q-*\""]
        );
        assert_eq!(case("  s = [\"x\"]\n", ""), ["a.q[\"0\"] = \"q-0\""]);
        let value = "resource \"a\" \"v\" { name = \"v-${[for v in local.s : v][0]}\" }\n";
        assert_eq!(
            case("  s = toset([\"x\"])\n", value),
            ["a.v = \"v-x\"", "a.q ~ \"q-*\""]
        );
        let var = |decl: &str, files: &[(&str, &str)]| {
            let main = format!(
                "variable \"s\" {{\n{decl}}}\n{}",
                q.replace("local.s", "var.s")
            );
            let mut all = vec![("main.tf", main.as_str())];
            all.extend_from_slice(files);
            names(&all)
        };
        let typed = "  type = set(string)\n  default = [\"x\"]\n";
        assert_eq!(var(typed, &[]), ["a.q ~ \"q-*\""]);
        assert_eq!(
            var(
                "  type = set(string)\n",
                &[("terraform.tfvars", "s = [\"x\"]\n")]
            ),
            ["a.q ~ \"q-*\""]
        );
        let json = r#"{"variable": {"s": {"type": "set(string)", "default": ["x"]}},
            "resource": {"a": {"q": {"for_each": "${{for k, v in var.s : k => v}}",
            "name": "q-${each.key}"}}}}"#;
        assert_eq!(names(&[("main.tf.json", json)]), ["a.q ~ \"q-*\""]);
        assert_eq!(
            var("  default = [\"x\"]\n", &[]),
            ["a.q[\"0\"] = \"q-0\" [var.s main.tf:1]"]
        );
    }

    #[test]
    fn a_key_over_a_named_set_read_twice_is_unresolved() {
        let case = |s: &str, for_each: &str| {
            let main = format!(
                "locals {{\n  s = {s}\n}}\nresource \"a\" \"q\" {{\n  for_each = {for_each}\n  name = \"q-${{each.key}}\"\n}}\n"
            );
            names(&[("main.tf", &main)])
        };
        let first = "{for k, v in local.s : k => v if length(local.s) > 0}";
        let after = "length(local.s) > 0 ? {for k, v in local.s : k => v} : {}";
        assert_eq!(
            [
                case("toset([\"x\"])", first),
                case("toset([\"x\"])", after),
                case("[\"x\"]", first),
                case("[\"x\"]", after),
            ],
            [
                vec!["a.q ~ \"q-*\"".to_string()],
                vec!["a.q ~ \"q-*\"".to_string()],
                vec!["a.q[\"0\"] = \"q-0\"".to_string()],
                vec!["a.q[\"0\"] = \"q-0\"".to_string()],
            ]
        );
    }

    #[test]
    fn a_for_over_a_conversion_of_an_expanded_argument_keeps_its_cost() {
        let main = concat!(
            "variable \"l\" {}\nvariable \"y\" {}\n",
            "resource \"a\" \"q\" {\n",
            "  for_each = {for a in tolist(var.l...) : a => a if length(jsonencode(var.y)) > 0 && a == \"a1\"}\n",
            "  name = \"q-${each.key}\"\n}\n",
        );
        let l: Vec<String> = (0..200).map(|i| format!("\"a{i}\"")).collect();
        let tfvars = format!("l = [[{}]]\ny = \"{}\"\n", l.join(", "), "y".repeat(60_000));
        let got = names(&[("main.tf", main), ("terraform.tfvars", &tfvars)]);
        assert_eq!(got, ["a.q ~ \"q-*\""]);
    }

    #[test]
    fn a_var_file_value_past_the_evaluation_ceilings_is_given_and_unresolved() {
        let list = (0..300).map(|i| i.to_string()).collect::<Vec<_>>();
        let list = list.join(", ");
        let main = concat!(
            "variable \"x\" {\n  default = []\n}\n",
            "resource \"a\" \"q\" { name = \"q-${length(var.x)}\" }\n",
        );
        let given = |path: &str, text: String| names(&[("main.tf", main), (path, &text)]);
        let tfvars = "terraform.tfvars";
        let long = "a".repeat(EVAL_VALUE_MAX);
        let got = [
            given(tfvars, format!("x = [for a in [{list}] : 1]\n")),
            given(
                tfvars,
                format!("x = [for a in [{list}] : [for b in [{list}] : 1]]\n"),
            ),
            given(tfvars, format!("x = \"{long}\"\n")),
            given("terraform.tfvars.json", format!("{{\"x\": \"{long}\"}}")),
        ];
        let many = (0..270)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let costly = format!("[for a in [{many}] : [for b in [{many}] : z]]");
        let n = 4 * EVAL_WORK_MAX / EVAL_COST_MAX;
        let values: String = (0..n).map(|i| format!("x{i} = {costly}\n")).collect();
        let last = format!("[for a in [{many}] : [for b in [{many}] : \"ok-ok-ok\"]][0][0]");
        let values = format!("first = \"ok\"\n{values}last = {last}\n");
        let main = concat!(
            "variable \"first\" { default = \"\" }\nvariable \"last\" { default = \"\" }\n",
            "resource \"a\" \"first\" { name = \"q-${var.first}\" }\n",
            "resource \"a\" \"last\" { name = \"q-${var.last}\" }\n",
        );
        let spent = names(&[("main.tf", main), (tfvars, &values)]);
        let unresolved = vec!["a.q ~ \"q-*\"".to_string()];
        assert_eq!(
            (got, spent),
            (
                [
                    vec!["a.q = \"q-300\" [var.x terraform.tfvars:1 tfvars]".to_string()],
                    unresolved.clone(),
                    unresolved.clone(),
                    unresolved,
                ],
                vec![
                    "a.first = \"q-ok\" [var.first terraform.tfvars:1 tfvars]".to_string(),
                    format!(
                        "a.last ~ \"q-*\" [var.last terraform.tfvars:{} tfvars]",
                        n + 2
                    ),
                ]
            )
        );
    }

    #[test]
    fn a_json_var_file_with_a_wide_object_gives_its_values() {
        let keys: Vec<String> = (0..5000).map(|i| format!("\"k{i}\": 1")).collect();
        let values = format!("{{\"wide\": {{{}}}, \"x\": \"ok\"}}", keys.join(", "));
        let main =
            "variable \"x\" { default = \"\" }\nresource \"a\" \"q\" { name = \"q-${var.x}\" }\n";
        assert_eq!(
            names(&[("main.tf", main), ("terraform.tfvars.json", &values)]),
            ["a.q = \"q-ok\" [var.x terraform.tfvars.json:1 tfvars]"]
        );
    }

    #[test]
    fn a_json_var_file_finds_the_lines_of_its_keys_in_linear_time() {
        let n = 60_000;
        let keys: String = (0..n).map(|i| format!("  \"k{i}\": \"v\",\n")).collect();
        let values = format!("{{\n{keys}  \"x\": \"ok\"\n}}\n");
        let main =
            "variable \"x\" { default = \"\" }\nresource \"a\" \"q\" { name = \"q-${var.x}\" }\n";
        let start = std::time::Instant::now();
        let got = names(&[("main.tf", main), ("terraform.tfvars.json", &values)]);
        let took = start.elapsed();
        let line = n + 2;
        assert_eq!(
            got,
            [format!(
                "a.q = \"q-ok\" [var.x terraform.tfvars.json:{line} tfvars]"
            )]
        );
        assert!(took < std::time::Duration::from_secs(20), "{took:?}");
    }

    #[test]
    fn a_template_in_a_wide_for_is_bounded_by_its_steps() {
        let body = "${a}".repeat(40);
        let root = |m: usize, n: usize| {
            let items = vec!["\"a\""; m].join(", ");
            let tfvars = format!("items = [{items}]\n");
            let resources: String = (0..n)
                .map(|i| {
                    format!(
                        "resource \"aws_sqs_queue\" \"r{i}\" {{\n  name = \"q-${{length([for a in var.items : \"{body}\"])}}\"\n}}\n"
                    )
                })
                .collect();
            let main = format!("variable \"items\" {{}}\n{resources}");
            let start = std::time::Instant::now();
            let got = names(&[("main.tf", &main), ("terraform.tfvars", &tfvars)]);
            (got, start.elapsed())
        };
        let (hundred, _) = root(100, 1);
        assert_eq!(
            hundred,
            ["aws_sqs_queue.r0 = \"q-100\" [var.items terraform.tfvars:1 tfvars]"]
        );
        for (n, secs) in [(1, 1), (64, 4)] {
            let (got, took) = root(8000, n);
            assert_eq!(got.len(), n);
            assert!(got.iter().all(|s| s.ends_with(" ~ \"q-*\"")), "{got:?}");
            assert!(took < std::time::Duration::from_secs(secs), "{n}: {took:?}");
        }
    }

    #[test]
    fn a_nested_template_for_of_no_part_is_refused() {
        let ones = vec!["1"; 1000].join(", ");
        let nested = format!(
            "\"%{{ for a in [{ones}] }}%{{ for b in [{ones}] }}%{{ endfor }}%{{ endfor }}\""
        );
        let expr = parser::parse_expr(&nested).unwrap();
        assert!(steps(&expr, &Map::new()) > EVAL_COST_MAX);
        let alone = format!("\"%{{ for a in [{ones}] }}%{{ endfor }}\"");
        let expr = parser::parse_expr(&alone).unwrap();
        assert!(steps(&expr, &Map::new()) <= EVAL_COST_MAX);
    }

    #[test]
    fn a_template_is_evaluated_without_its_text() {
        let mut ctx = Context::new();
        functions::declare(&mut ctx);
        let var = Map::from_iter([
            ("x".to_string(), Value::from("ex")),
            ("y".to_string(), Value::from("why")),
            ("n".to_string(), Value::from(7)),
            ("t".to_string(), Value::Bool(true)),
            ("f".to_string(), Value::Bool(false)),
            ("l".to_string(), Value::from_iter(["a", "b", "c"])),
            (
                "m".to_string(),
                Value::Object(Map::from_iter([
                    ("k1".to_string(), Value::from("v1")),
                    ("k2".to_string(), Value::from("v2")),
                ])),
            ),
        ]);
        ctx.declare_var("var", Value::Object(var));
        let templates = [
            r#""q-${var.x}-${var.y}""#,
            r#""a\n$${b} %%{c} \"d\" ${var.x}""#,
            r#""${var.n}""#,
            r#""n=${var.n}""#,
            r#""%{ if var.t }yes%{ else }no%{ endif }""#,
            r#""%{ if var.f }yes%{ endif }-${var.x}""#,
            r#""x  %{~ if var.t ~}  y  %{~ else ~}  n  %{~ endif ~}  z""#,
            r#""a ${~ var.x ~} b""#,
            r#""%{ for k, v in var.m }${k}=${v};%{ endfor }""#,
            "\"%{ for v in var.l ~}\n  ${v}\n%{~ endfor }\"",
            r#""%{ for v in var.l }%{ if v == "b" }B%{ else }${v}%{ endif }%{ endfor }""#,
            "<<-EOT\n    a ${var.x}\n      b\n    EOT",
            "<<EOT\n%{ for v in var.l }${v}\n%{ endfor }\nEOT",
        ];
        for text in templates {
            let expr = parser::parse_expr(text).unwrap();
            let mut exact = Exact {
                ctx: &ctx,
                sets: &BTreeSet::new(),
                left: 0,
                inexact: false,
                unordered: 0,
            };
            let mut called = expr.clone();
            exact.visit_expr_mut(&mut called);
            let called = hcl::Expression::from(called);
            assert!(!format!("{called:?}").contains("TemplateExpr"), "{text}");
            let parsed = hcl::Expression::from(expr.clone()).evaluate(&ctx).ok();
            assert!(parsed.is_some(), "{text}");
            assert_eq!(evaluated(&expr, &ctx, &BTreeSet::new()), parsed, "{text}");
        }
    }

    #[test]
    fn templates_the_text_charge_refused_resolve() {
        let list = |n: usize| {
            (0..n)
                .map(|i| format!("\"e{i}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let long = "a".repeat(17_000);
        let forty = "${x}".repeat(40);
        let main = format!(
            "variable \"two\" {{ default = [{}] }}\nvariable \"wide\" {{ default = [{}] }}\nvariable \"some\" {{ default = [{}] }}\nresource \"a\" \"long\" {{ name = \"q-${{length([for x in var.two : \"{long}${{x}}\"])}}\" }}\nresource \"a\" \"if\" {{ name = \"q-${{length([for x in var.wide : \"%{{ if x == \"e1\" }}y%{{ else }}n%{{ endif }}\"])}}\" }}\nresource \"a\" \"forty\" {{ name = \"q-${{length([for x in var.some : \"{forty}\"])}}\" }}\n",
            list(2),
            list(2000),
            list(198),
        );
        let got = names(&[("main.tf", &main)]);
        let trimmed: Vec<_> = got.iter().map(|s| s.split(" [").next().unwrap()).collect();
        assert_eq!(
            trimmed,
            [
                "a.long = \"q-2\"",
                "a.if = \"q-2000\"",
                "a.forty = \"q-198\""
            ],
            "{got:?}"
        );
    }

    #[test]
    fn a_for_over_a_for_of_templates_still_resolves() {
        let l: Vec<String> = (0..30).map(|i| format!("\"a{i}\"")).collect();
        let main = format!(
            "variable \"l\" {{ default = [{}] }}\n{}{}",
            l.join(", "),
            "resource \"a\" \"s\" { name = \"q-${length([for x in [for y in var.l : \"${y}x\"] : \"${x}y\"])}\" }\n",
            "resource \"a\" \"t\" { name = \"q-${length([for x in [for y in var.l : [\"${y}1\", \"${y}2\"]] : x])}\" }\n",
        );
        assert_eq!(
            names(&[("main.tf", &main)]),
            [
                "a.s = \"q-30\" [var.l main.tf:1]",
                "a.t = \"q-30\" [var.l main.tf:1]",
            ]
        );
    }

    #[test]
    fn a_for_in_a_for_body_counts_the_elements_it_reads() {
        let list = |n: usize| {
            (0..n)
                .map(|i| format!("\"{i}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let root = |n: usize, name: &str| {
            let main = format!(
                "variable \"l\" {{ default = [{}] }}\nresource \"a\" \"p\" {{ name = \"{name}\" }}\n",
                list(n)
            );
            names(&[("main.tf", &main)])
        };
        let two = "q-${length([for a in var.l : [for b in var.l : \"${a}${b}\"]])}";
        let three = "q-${length([for a in var.l : [for b in var.l : [for c in var.l : \"${a}${b}${c}\"]]])}";
        let hidden = "q-${length([for var in [{l = [1]}] : [for b in var.l : \"${b}\"]])}";
        assert_eq!(
            [root(25, two), root(5, three), root(2, hidden)],
            [
                vec!["a.p = \"q-25\" [var.l main.tf:1]".to_string()],
                vec!["a.p = \"q-5\" [var.l main.tf:1]".to_string()],
                vec!["a.p = \"q-1\" [var.l main.tf:1]".to_string()],
            ]
        );
    }

    #[test]
    fn a_traversal_in_a_for_body_counts_the_root_it_copies() {
        let items = |m: usize| vec!["\"a\""; m].join(", ");
        let timed = |list: &[(&str, &str)]| {
            let start = std::time::Instant::now();
            let got = names(list);
            (got, start.elapsed())
        };
        let refused = |got: &[String]| got.iter().all(|s| s.ends_with(" ~ \"q-*\""));
        let tfvars = |m: usize| format!("items = [{}]\n", items(m));
        let resource = |i: usize, reads: &str, from: &str| {
            format!(
                "resource \"a\" \"r{i}\" {{\n  name = \"q-${{length([for a in {from} : {reads}[0]])}}\"\n}}\n"
            )
        };
        let one = format!(
            "variable \"items\" {{}}\n{}",
            resource(0, "var.items", "var.items")
        );
        let big = tfvars(21_000);
        let (got, took) = timed(&[("main.tf", &one), ("terraform.tfvars", &big)]);
        assert!(refused(&got) && got.len() == 1, "{got:?}");
        assert!(took < std::time::Duration::from_secs(1), "one: {took:?}");
        let eight: String = (0..8)
            .map(|i| resource(i, "var.items", "var.items"))
            .collect();
        let eight = format!("variable \"items\" {{}}\n{eight}");
        let (got, took) = timed(&[("main.tf", &eight), ("terraform.tfvars", &tfvars(10_000))]);
        assert!(refused(&got) && got.len() == 8, "{got:?}");
        assert!(took < std::time::Duration::from_secs(2), "eight: {took:?}");
        let local = format!(
            "variable \"items\" {{}}\nlocals {{\n  x = var.items\n}}\n{}",
            resource(0, "local.x", "local.x")
        );
        let each = String::from(
            "variable \"items\" {}\nresource \"a\" \"r0\" {\n  for_each = { k = var.items }\n  name = \"q-${length([for a in each.value : each.value[0]])}\"\n}\n",
        );
        let module = format!(
            "variable \"items\" {{}}\nmodule \"m\" {{\n  source = \"./m\"\n  items = var.items\n}}\n{}",
            resource(0, "module.m.o", "module.m.o")
        );
        let child = "variable \"items\" {}\noutput \"o\" { value = var.items }\n";
        for main in [&local, &each, &module] {
            let (got, took) = timed(&[
                ("main.tf", main),
                ("terraform.tfvars", &big),
                ("m/main.tf", child),
            ]);
            let got: Vec<String> = got.into_iter().filter(|s| s.starts_with("a.")).collect();
            assert!(refused(&got) && got.len() == 1, "{got:?}");
            assert!(took < std::time::Duration::from_secs(1), "{main}: {took:?}");
        }
    }

    #[test]
    fn a_root_read_many_times_in_a_for_body_is_sized_once() {
        let reads = vec!["each.value[0]"; 20_000].join(", ");
        let main = format!(
            "variable \"items\" {{}}\nresource \"a\" \"r\" {{\n  for_each = {{ k = var.items }}\n  name = \"q-${{length([for a in each.value : [{reads}]])}}\"\n}}\n"
        );
        let root = |m: usize| {
            let tfvars = format!("items = [{}]\n", vec!["\"a\""; m].join(", "));
            let start = std::time::Instant::now();
            let got = names(&[("main.tf", &main), ("terraform.tfvars", &tfvars)]);
            assert_eq!(got, ["a.r[\"k\"] ~ \"q-*\""]);
            start.elapsed()
        };
        let (few, wide) = (root(8), root(21_000));
        let bound = 3 * few + std::time::Duration::from_millis(500);
        assert!(wide < bound, "{wide:?} against {few:?}");
    }

    #[test]
    fn a_for_in_a_for_body_reads_its_collection_through_any_root() {
        let l = (0..25)
            .map(|i| format!("\"{i}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let name = |from: &str, inner: &str| {
            format!("q-${{length([for a in {from} : [for b in {inner} : \"${{a}}${{b}}\"]])}}")
        };
        let child = "variable \"l\" {}\noutput \"o\" { value = var.l }\n";
        let head = format!(
            "variable \"l\" {{ default = [{l}] }}\nvariable \"m\" {{ default = {{ k = [{l}] }} }}\nlocals {{\n  x = var.l\n}}\nmodule \"c\" {{\n  source = \"./c\"\n  l = var.l\n}}\n"
        );
        let body = [
            ("local", name("var.l", "local.x")),
            ("module", name("var.l", "module.c.o")),
            ("key", name("var.l", "var.m[\"k\"]")),
            ("cond", name("var.l", "(true ? var.l : [])")),
        ];
        let mut main = head.clone();
        for (n, name) in &body {
            main += &format!("resource \"a\" \"{n}\" {{ name = \"{name}\" }}\n");
        }
        main += &format!(
            "resource \"a\" \"each\" {{\n  for_each = {{ k = var.l }}\n  name = \"{}\"\n}}\n",
            name("each.value", "each.value")
        );
        let got = names(&[("main.tf", &main), ("c/main.tf", child)]);
        let resolved = (got.iter()).filter(|s| s.contains(" = \"q-25\"")).count();
        assert_eq!((got.len(), resolved), (6, 5), "{got:?}");
    }

    #[test]
    fn a_long_template_evaluated_once_resolves() {
        let long = "a".repeat(33_000);
        let main = format!(
            "variable \"x\" {{ default = \"x\" }}\nresource \"a\" \"q\" {{ name = \"q-%{{ if var.x == \"x\" }}${{var.x}}%{{ endif }}{long}\" }}\n"
        );
        assert_eq!(
            names(&[("main.tf", &main)]),
            [format!("a.q = \"q-x{long}\" [var.x main.tf:1]")]
        );
    }

    #[test]
    fn refusals_over_a_wide_variable_spend_the_root_work_they_copy() {
        let items = vec!["\"a\""; 21_000].join(", ");
        let tfvars = format!("items = [{items}]\n");
        let root = |name: &str| {
            let r: String = (0..9999)
                .map(|i| format!("resource \"a\" \"r{i}\" {{ name = \"{name}\" }}\n"))
                .collect();
            let main = format!(
                "variable \"items\" {{}}\n{r}resource \"a\" \"z\" {{ name = \"constant\" }}\n"
            );
            let start = std::time::Instant::now();
            let (nodes, _) = evaluate(&files(&[("main.tf", &main), ("terraform.tfvars", &tfvars)]));
            (nodes, start.elapsed())
        };
        let (_, constants) = root("constant");
        let (nodes, refused) = root("q-${length([for a in var.items : [for b in var.items : a]])}");
        let z = nodes.iter().find(|n| n.address == "a.z");
        assert_eq!(nodes.len(), 10_000);
        assert_eq!(
            z.map(|n| (n.name.as_deref(), n.pattern)),
            Some((Some("constant"), false))
        );
        assert!(
            refused < 10 * constants,
            "{refused:?} against {constants:?}"
        );
    }

    #[test]
    fn a_refused_expression_spends_what_it_went_through() {
        let items = vec!["\"a\""; 8000].join(", ");
        let body = "${a}".repeat(40);
        let refused: String = (0..32)
            .map(|i| {
                format!(
                    "resource \"a\" \"r{i}\" {{ name = \"q-${{length([for a in var.items : \"{body}\"])}}\" }}\n"
                )
            })
            .collect();
        let main = format!(
            "variable \"items\" {{}}\n{refused}resource \"a\" \"z\" {{ name = \"constant\" }}\n"
        );
        let tfvars = format!("items = [{items}]\n");
        let got = names(&[("main.tf", &main), ("terraform.tfvars", &tfvars)]);
        assert_eq!(got.len(), 33);
        assert!(
            got[..32].iter().all(|s| s.ends_with(" ~ \"q-*\"")),
            "{got:?}"
        );
        assert!(got.contains(&"a.z = \"constant\"".to_string()), "{got:?}");
    }

    #[test]
    fn refusals_of_a_long_expression_spend_its_text() {
        let long = "a".repeat(60_000);
        let list = vec!["1"; 20].join(", ");
        let main = format!(
            "variable \"x\" {{ default = \"x\" }}\nresource \"a\" \"r\" {{\n  count = 1000\n  name = \"q-${{length([for a in [{list}] : \"{long}\"])}}\"\n}}\nresource \"a\" \"z\" {{ name = \"q-${{var.x}}\" }}\n"
        );
        let got = names(&[("main.tf", &main)]);
        assert_eq!(got.len(), 1001);
        assert_eq!(got[0], "a.r[0] ~ \"q-*\"");
        assert_eq!(got.last().map(String::as_str), Some("a.z ~ \"*\""));
    }

    #[test]
    fn a_refusal_spends_its_root_reads_up_to_the_cost_ceiling() {
        let big = vec!["\"a\""; 20_000].join(", ");
        let root = |count: usize, name: &str| {
            let main = format!(
                "variable \"x\" {{ default = \"x\" }}\nvariable \"big\" {{ default = [{big}] }}\nresource \"a\" \"r\" {{\n  count = {count}\n  name = \"q-${{{name}}}\"\n}}\nresource \"a\" \"z\" {{ name = \"q-${{var.x}}\" }}\n"
            );
            let got = names(&[("main.tf", &main)]);
            assert_eq!(got.len(), count + 1);
            assert_eq!(got[0], "a.r[0] ~ \"q-*\"");
            got.last().cloned()
        };
        let resolved = Some("a.z = \"q-x\" [var.x main.tf:1]".to_string());
        let reads = vec!["var.big"; 40].join(", ");
        assert_eq!(root(20, &format!("length(concat({reads}))")), resolved);
        let resources = vec!["a.o.id"; 50].join(", ");
        let named = format!("join(\",\", concat(var.big, [{resources}]))");
        assert_eq!(root(40, &named), resolved);
    }

    #[test]
    fn a_known_name_the_instances_share_is_charged_once() {
        let big = vec!["\"a\""; 2_000].join(", ");
        let reads = vec!["var.big"; 2_000].join(", ");
        let spent = |count: usize| {
            let main = format!(
                "variable \"big\" {{}}\nvariable \"e\" {{ default = [] }}\nresource \"a\" \"r\" {{\n  count = {count}\n  name  = \"q-${{length([for a in var.e : [{reads}]])}}\"\n}}\n"
            );
            let tfvars = format!("big = [{big}]\n");
            let repository =
                Repository::read(&files(&[("main.tf", &main), ("terraform.tfvars", &tfvars)]));
            let mut eval = Eval::new(&repository.modules, &repository.mappings, STORED_MAX);
            eval.root("");
            let named: Vec<_> = (eval.nodes.iter())
                .map(|n| (n.name.as_deref(), n.pattern))
                .collect();
            assert_eq!(named, vec![(Some("q-0"), false); count]);
            EVAL_WORK_MAX - eval.work
        };
        let (one, ten) = (spent(1), spent(10));
        assert!(one > 0 && ten < 2 * one, "{ten} against {one}");
    }

    #[test]
    fn the_estimate_sizes_a_bound_value_once_in_linear_time() {
        let var = Map::from_iter([
            (
                "big".to_string(),
                Value::Array(vec![Value::from("a"); 200_000]),
            ),
            ("e".to_string(), Value::Array(Vec::new())),
        ]);
        let roots = Map::from_iter([("var".to_string(), Value::Object(var))]);
        let reads = vec!["var.big"; 40_000].join(", ");
        let expr = parser::parse_expr(&format!("length([for a in var.e : [{reads}]])")).unwrap();
        let start = std::time::Instant::now();
        let estimated = steps(&expr, &roots);
        let took = start.elapsed();
        assert!(estimated < 100, "{estimated}");
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    }

    #[test]
    fn a_refusal_counts_each_root_by_its_own_reads() {
        let big = vec!["\"a\""; 20_000].join(", ");
        let row = vec!["a"; 20].join(", ");
        let index = vec!["count.index"; 50].join(" + ");
        let main = format!(
            "variable \"x\" {{ default = \"x\" }}\nvariable \"big\" {{ default = [{big}] }}\nresource \"a\" \"r\" {{\n  count = 40\n  name  = \"q-${{length([for a in var.big : [{row}]]) + {index}}}\"\n}}\nresource \"a\" \"z\" {{ name = \"q-${{var.x}}\" }}\n"
        );
        let got = names(&[("main.tf", &main)]);
        assert_eq!(got.len(), 41);
        assert!(
            got[..40].iter().all(|s| s.ends_with(" ~ \"q-*\"")),
            "{got:?}"
        );
        assert_eq!(got[40], "a.z = \"q-x\" [var.x main.tf:1]");
    }

    #[test]
    fn a_literal_past_the_value_ceiling_is_unresolved() {
        let long = "a".repeat(EVAL_VALUE_MAX);
        let fits = &long[..EVAL_VALUE_MAX - 2];
        let main = format!(
            "locals {{\n  l = \"{long}\"\n}}\nresource \"a\" \"long\" {{ name = \"{long}\" }}\nresource \"a\" \"read\" {{ name = \"q-${{local.l}}\" }}\nresource \"a\" \"fits\" {{ name = \"{fits}\" }}\n"
        );
        assert_eq!(
            names(&[("main.tf", &main)]),
            [
                "a.long ~ \"*\"".to_string(),
                "a.read ~ \"q-*\"".to_string(),
                format!("a.fits = \"{fits}\""),
            ]
        );
    }

    #[test]
    fn a_wide_object_spends_its_keys_squared_from_the_root_work() {
        let keys: String = (0..1500).map(|i| format!("    k{i} = \"v\"\n")).collect();
        let module = format!(
            "locals {{\n  m = {{\n{keys}  }}\n}}\nresource \"a\" \"q\" {{ name = local.m.k0 }}\n"
        );
        let main = "module \"m\" {\n  source = \"./m\"\n  count = 40\n}\n";
        let (nodes, _) = evaluate(&files(&[("main.tf", main), ("m/main.tf", &module)]));
        let queues: Vec<_> = nodes.iter().filter(|n| n.ty == "a").collect();
        let resolved = (queues.iter())
            .filter(|n| n.name.as_deref() == Some("v"))
            .count();
        assert_eq!(queues.len(), 40);
        assert!(resolved > 0 && resolved < 40, "{resolved}");
    }

    #[test]
    fn trim_and_toset_evaluate_in_linear_time() {
        let mut ctx = Context::new();
        functions::declare(&mut ctx);
        ctx.declare_var("a", Value::from("b".repeat(40_000)));
        ctx.declare_var("c", Value::from(format!("{}b", "a".repeat(20_000))));
        let strings = (0..40_000).map(|i| Value::from(format!("s{i}")));
        ctx.declare_var("strings", Value::Array(strings.collect()));
        let numbers = (0..40_000).map(Value::from);
        ctx.declare_var("numbers", Value::Array(numbers.collect()));
        let mut took = Vec::new();
        for (text, expected) in [
            ("trim(a, c)", Value::from("")),
            ("length(toset(strings))", Value::from(40_000)),
            ("length(toset(numbers))", Value::from(40_000)),
        ] {
            let expr = parser::parse_expr(text).unwrap();
            let start = std::time::Instant::now();
            assert_eq!(
                evaluated(&expr, &ctx, &BTreeSet::new()),
                Some(expected),
                "{text}"
            );
            took.push(start.elapsed());
        }
        let bound = std::time::Duration::from_secs(2);
        assert!(took.iter().all(|t| *t < bound), "{took:?}");
    }

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

    #[test]
    fn a_var_file_that_does_not_read_leaves_the_values_it_may_give_unresolved() {
        let main = concat!(
            "variable \"x\" {\n  default = \"def\"\n}\n",
            "resource \"a\" \"q\" { name = \"q-${var.x}\" }\n",
        );
        let wide: String = (0..4097).map(|i| format!("  k{i} = {i}\n")).collect();
        let unread = [
            ("terraform.tfvars", "x = \"given\"\n}\n".to_string()),
            (
                "terraform.tfvars",
                format!("x = \"given\"\nw = {{\n{wide}}}\n"),
            ),
            (
                "terraform.tfvars",
                format!("x = \"given\"\n#{}\n", "a".repeat(MAX_BYTES)),
            ),
            ("terraform.tfvars.json", "{\"x\": \"given\",}".to_string()),
        ];
        let unresolved = ["a.q ~ \"q-*\" [var.x main.tf:1]"];
        for (file, text) in &unread {
            assert_eq!(
                names(&[("main.tf", main), (file, text)]),
                unresolved,
                "{file}"
            );
        }
        let later = [
            ("main.tf", main),
            ("terraform.tfvars", "x = \"given\"\n}\n"),
            ("z.auto.tfvars", "x = \"auto\"\n"),
        ];
        assert_eq!(
            names(&later),
            ["a.q = \"q-auto\" [var.x z.auto.tfvars:1 tfvars]"]
        );
        let earlier = [
            ("main.tf", main),
            ("terraform.tfvars", "x = \"given\"\n"),
            ("z.auto.tfvars.json", "{"),
        ];
        assert_eq!(names(&earlier), unresolved);
    }

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

    #[test]
    fn a_variable_typed_as_a_list_given_a_set_keeps_its_index_key() {
        let module = |ty: &str| {
            format!(
                "variable \"s\" {{\n{ty}}}\nresource \"a\" \"q\" {{\n  for_each = {{for k, v in var.s : k => v}}\n  name = \"queue-${{each.key}}\"\n}}\n"
            )
        };
        let call = |arg: &str, ty: &str| {
            let main = format!(
                "locals {{ t = toset([\"x\"]) }}\nmodule \"m\" {{\n  source = \"./m\"\n  s = {arg}\n}}\n"
            );
            names(&[("main.tf", &main), ("m/main.tf", &module(ty))])
        };
        let list = ["module.m", "module.m.a.q[\"0\"] = \"queue-0\""];
        assert_eq!(call("toset([\"x\"])", "  type = list(string)\n"), list);
        assert_eq!(call("local.t", "  type = list(any)\n"), list);
        let unresolved = ["module.m", "module.m.a.q ~ \"queue-*\""];
        assert_eq!(call("local.t", "  type = any\n"), unresolved);
        assert_eq!(call("local.t", ""), unresolved);
        assert_eq!(call("local.t", "  type = set(string)\n"), unresolved);
    }

    #[test]
    fn an_override_retyping_a_set_variable_as_a_list_keeps_its_index_key() {
        let main = concat!(
            "variable \"s\" {\n  type = set(string)\n  default = [\"x\"]\n}\n",
            "resource \"a\" \"q\" {\n  for_each = {for k, v in var.s : k => v}\n  name = \"queue-${each.key}\"\n}\n",
        );
        let over = |body: &str| {
            let over = format!("variable \"s\" {{\n{body}}}\n");
            names(&[("main.tf", main), ("override.tf", &over)])
        };
        assert_eq!(
            over("  type = list(string)\n  default = [\"x\"]\n"),
            ["a.q[\"0\"] = \"queue-0\" [var.s override.tf:1]"]
        );
        assert_eq!(
            over("  type = list(string)\n"),
            ["a.q[\"0\"] = \"queue-0\" [var.s main.tf:1]"]
        );
        assert_eq!(over("  description = \"d\"\n"), ["a.q ~ \"queue-*\""]);
    }

    #[test]
    fn a_long_chain_of_locals_named_for_a_set_stops_at_the_depth() {
        let n = 20_000;
        let mut text = String::from("locals {\n  l0 = toset([\"x\"])\n");
        for i in 1..n {
            text += &format!("  l{i} = local.l{}\n", i - 1);
        }
        text += &format!(
            "}}\nresource \"a\" \"q\" {{\n  for_each = {{for k, v in local.l{} : k => v}}\n  name = \"q-${{each.key}}\"\n}}\n",
            n - 1
        );
        assert_eq!(names(&[("main.tf", &text)]), ["a.q ~ \"q-*\""]);
    }

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
