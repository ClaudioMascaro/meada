//! IaC nodes (#649, ADR 0025): every Terraform and OpenTofu resource, data block and module call
//! of the `.tf` and `.tofu` files a repository holds, with an address, a type, a file, a span and
//! the root module that roots its evaluation, and the static references between them, stored like
//! calls. `search` finds a node with the kind `iac`, `context` explains it and `trace` walks its
//! references both ways. Never exported.
//!
//! A directory of Terraform files is a module. A module block whose `source` is a local path
//! (`./` or `../`, staying inside the repository) calls the module of that directory; a directory
//! no other one calls is a root module, labelled by its path (`.` for the repository root). Each
//! root is evaluated alone: its blocks are nodes at their own address (`<type>.<name>`,
//! `data.<type>.<name>`, `module.<name>`), and each call of a local module instantiates that
//! module's blocks once, prefixed by the call's address (`module.orders.aws_sqs_queue.this`). A
//! `count` or `for_each` over literals gives one node per instance, suffixed `[key]`.
//!
//! A reference is a traversal in a block's expressions that names a resource, a data block or a
//! module; one through `var.<name>`, `local.<name>` or a module's output (`module.<name>.<out>`) is
//! followed to what the caller's argument, the local or the output refers to, and keeps the name
//! it went through as its `via`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use hcl_edit::Span;
use hcl_edit::expr::{Expression, ObjectKey, Traversal, TraversalOperator};
use hcl_edit::parser;
use hcl_edit::structure::Body;
use hcl_edit::visit::{self, Visit};
use rusqlite::{Connection, Transaction, params};

use crate::Result;
use crate::config_values;
use crate::contract::Hit;
use crate::document::events::Step;
use crate::lang::MAX_BYTES;
use crate::render::{rank, section};
use crate::trace::Direction;
use meada_core::export::Level;
use meada_core::trace::{Chain, Row};

/// The instances one `count` or `for_each` gives; past it the block is one node with no key.
// ponytail: a fleet of a few hundred instances is real; past a thousand the keys come from data.
const EXPANSION_MAX: usize = 1000;

/// How deep local module calls nest; a call deeper keeps its node and instantiates nothing.
// ponytail: real module trees nest three or four levels; 16 stops a chain built to run on.
const MODULE_DEPTH_MAX: usize = 16;

/// How many names (`var`, `local`, outputs) a reference may go through before it is dropped.
// ponytail: a name passed down a few modules goes through a handful; 32 stops a chain of locals.
const EVAL_DEPTH_MAX: usize = 32;

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

/// Whether the file at `path` holds IaC nodes: a `.tf` or `.tofu` file (HCL).
pub(crate) fn is_source(path: &str) -> bool {
    path.ends_with(".tf") || path.ends_with(".tofu")
}

/// One IaC node as stored.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    pub address: String,
    /// The label of the root module its evaluation starts from: its directory, `.` for the root.
    pub root: String,
    /// The resource or data type, or `module` for a module call.
    pub ty: String,
    /// Its physical name: the literal of its `name` attribute.
    pub name: Option<String>,
    pub path: String,
    pub line: usize,
    pub end_line: usize,
    /// The key path of its block, which its config values start with.
    pub block: String,
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
    physical: Option<String>,
    /// The instance keys, `None` for a block with no literal `count` or `for_each`.
    keys: Option<Vec<Key>>,
    refs: Vec<Ref>,
    /// A module call's `source`, and the references of each of its arguments.
    source: Option<String>,
    args: BTreeMap<String, Vec<Ref>>,
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

/// The blocks, outputs and locals of one directory.
#[derive(Default)]
struct Module {
    blocks: Vec<Block>,
    outputs: BTreeMap<String, Vec<Ref>>,
    locals: BTreeMap<String, Vec<Ref>>,
}

/// The directory a local module `source` names from `dir`: `./` or `../` joined and normalized,
/// `None` for any other source or one that climbs out of the repository.
fn local_source(dir: &str, source: &str) -> Option<String> {
    if !(source.starts_with("./") || source.starts_with("../")) {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in source.split('/') {
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

/// The instance keys of a literal `count` (`[0]` to `[n - 1]`) or `for_each` (the keys of an
/// object, the strings of `toset([...])`, sorted as Terraform sorts them); `None` for anything
/// else or past `EXPANSION_MAX`.
fn keys(body: &Body) -> Option<Vec<Key>> {
    let keys: Vec<Key> = if let Some(count) = body.get_attribute("count") {
        let Expression::Number(n) = &count.value else {
            return None;
        };
        let n = n.value().as_u64()?;
        if n as usize > EXPANSION_MAX {
            return None;
        }
        (0..n).map(Key::Int).collect()
    } else {
        let each = &body.get_attribute("for_each")?.value;
        let strings: BTreeSet<String> = match each {
            Expression::Object(object) => (object.iter())
                .map(|(k, _)| match k {
                    ObjectKey::Ident(i) => Some(i.as_str().to_string()),
                    ObjectKey::Expression(Expression::String(s)) => Some(s.value().clone()),
                    ObjectKey::Expression(_) => None,
                })
                .collect::<Option<_>>()?,
            Expression::FuncCall(call)
                if call.name.namespace.is_empty() && call.name.name.as_str() == "toset" =>
            {
                let args: Vec<&Expression> = call.args.iter().collect();
                let [Expression::Array(items)] = args[..] else {
                    return None;
                };
                (items.iter())
                    .map(|e| match e {
                        Expression::String(s) => Some(s.value().clone()),
                        _ => None,
                    })
                    .collect::<Option<_>>()?
            }
            _ => return None,
        };
        if strings.len() > EXPANSION_MAX {
            return None;
        }
        strings.into_iter().map(Key::Str).collect()
    };
    Some(keys)
}

/// Reads the blocks, outputs and locals of the Terraform file at `path` into `module`; a file that
/// is not UTF-8, nests past the HCL reader's ceiling or does not parse adds nothing (config values
/// count it unreadable).
fn read(path: &str, src: &[u8], module: &mut Module) {
    let Ok(text) = std::str::from_utf8(src) else {
        return;
    };
    if config_values::nested_past(text).is_some() {
        return;
    }
    let Ok(body) = parser::parse_body(text) else {
        return;
    };
    let newlines: Vec<usize> = (text.bytes().enumerate())
        .filter(|&(_, b)| b == b'\n')
        .map(|(i, _)| i)
        .collect();
    let line = |offset: usize| newlines.partition_point(|&n| n < offset) + 1;
    for b in body.blocks() {
        let labels: Vec<&str> = b.labels.iter().map(|l| l.as_str()).collect();
        let (kind, ty, name) = match (b.ident.as_str(), &labels[..]) {
            ("resource", [ty, name]) => (Kind::Resource, *ty, *name),
            ("data", [ty, name]) => (Kind::Data, *ty, *name),
            ("module", [name]) => (Kind::Module, "", *name),
            ("output", [name]) => {
                if let Some(value) = b.body.get_attribute("value") {
                    module
                        .outputs
                        .insert(name.to_string(), refs_of(&value.value));
                }
                continue;
            }
            ("locals", []) => {
                for a in b.body.attributes() {
                    module
                        .locals
                        .insert(a.key.as_str().to_string(), refs_of(&a.value));
                }
                continue;
            }
            _ => continue,
        };
        let span = b.span().unwrap_or(0..0);
        let mut refs = Refs::default();
        refs.visit_body(&b.body);
        let literal = |attr: &str| match b.body.get_attribute(attr).map(|a| &a.value) {
            Some(Expression::String(s)) => Some(s.value().clone()),
            _ => None,
        };
        let mut args = BTreeMap::new();
        if kind == Kind::Module {
            for a in b.body.attributes() {
                args.insert(a.key.as_str().to_string(), refs_of(&a.value));
            }
        }
        module.blocks.push(Block {
            kind,
            ty: ty.to_string(),
            name: name.to_string(),
            path: path.to_string(),
            line: line(span.start),
            end_line: line(span.end.saturating_sub(1).max(span.start)),
            // A module call's `name` is one of its inputs, not the name of something it creates.
            physical: (kind != Kind::Module).then(|| literal("name")).flatten(),
            keys: keys(&b.body),
            refs: refs.0,
            source: (kind == Kind::Module).then(|| literal("source")).flatten(),
            args,
        });
    }
}

/// A block by its kind, type and name, as a reference names it.
type BlockName<'m> = (Kind, &'m str, &'m str);

/// One instance of a module during a root's evaluation.
struct Instance<'m> {
    dir: &'m str,
    /// The instance that calls it, with the call's block.
    parent: Option<(usize, &'m Block)>,
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
    root: String,
    root_nodes: usize,
    root_left: usize,
    left: usize,
    nodes: Vec<Node>,
    references: Vec<Reference>,
}

impl<'m> Eval<'m> {
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
        self.root = if dir.is_empty() { "." } else { dir }.to_string();
        self.root_nodes = 0;
        self.root_left = ROOT_STORED_MAX;
        self.instantiate(dir, "", None, &mut vec![dir]);
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
    /// its nodes and those of the local modules it calls, `stack` holding the directories of
    /// the calls above it; its position.
    fn instantiate(
        &mut self,
        dir: &'m str,
        prefix: &str,
        parent: Option<(usize, &'m Block)>,
        stack: &mut Vec<&'m str>,
    ) -> usize {
        let id = self.instances.len();
        self.instances.push(Instance {
            dir,
            parent,
            nodes: HashMap::new(),
            children: HashMap::new(),
        });
        let modules = self.modules;
        for block in &modules[dir].blocks {
            let keys: Vec<Option<Key>> = match &block.keys {
                Some(keys) => keys.iter().cloned().map(Some).collect(),
                None => vec![None],
            };
            for key in keys {
                let shown = key.as_ref().map_or(String::new(), Key::shown);
                let address = format!("{prefix}{}{shown}", block.local());
                if self.root_nodes >= NODES_MAX || !self.charge(address.len()) {
                    return id;
                }
                self.root_nodes += 1;
                self.nodes.push(Node {
                    address: address.clone(),
                    root: self.root.clone(),
                    ty: match block.kind {
                        Kind::Module => "module".into(),
                        _ => block.ty.clone(),
                    },
                    name: block.physical.clone(),
                    path: block.path.clone(),
                    line: block.line,
                    end_line: block.end_line,
                    block: block.key_path(),
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
                    let child =
                        self.instantiate(target, &format!("{address}."), Some((id, block)), stack);
                    stack.pop();
                    let calls = self.instances[id].children.entry(&block.name);
                    calls.or_default().push((key, child));
                }
            }
        }
        id
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
                        for inner in modules[dir].outputs.get(out).into_iter().flatten() {
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
                    for inner in block.args.get(name).into_iter().flatten() {
                        found.extend(self.resolve(caller, inner, depth + 1));
                    }
                }
            }
            Ref::Local(name) => {
                let dir = self.instances[at].dir;
                for inner in modules[dir].locals.get(name).into_iter().flatten() {
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

/// The IaC nodes and references of the Terraform files `files` (path and bytes) of one
/// repository, each root module evaluated alone, in root and file order.
pub(crate) fn evaluate(files: &[(String, Vec<u8>)]) -> (Vec<Node>, Vec<Reference>) {
    let mut modules: BTreeMap<String, Module> = BTreeMap::new();
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, src) in sorted {
        read(
            path,
            src,
            modules
                .entry(config_values::parent(path).to_string())
                .or_default(),
        );
    }
    // A directory another one calls as a local module is evaluated only through its calls.
    let called: HashSet<String> = (modules.iter())
        .flat_map(|(dir, m)| {
            (m.blocks.iter())
                .filter_map(|b| b.source.as_deref().and_then(|s| local_source(dir, s)))
                .filter(move |t| t != dir)
        })
        .collect();
    let mut eval = Eval {
        modules: &modules,
        instances: Vec::new(),
        memo: HashMap::new(),
        open: HashSet::new(),
        root: String::new(),
        root_nodes: 0,
        root_left: 0,
        left: STORED_MAX,
        nodes: Vec::new(),
        references: Vec::new(),
    };
    for dir in modules.keys().filter(|d| !called.contains(*d)) {
        eval.root(dir);
    }
    (eval.nodes, eval.references)
}

/// Replaces every IaC node and reference of the repository with `nodes` and `references`.
pub(crate) fn store(tx: &Transaction, nodes: &[Node], references: &[Reference]) -> Result<()> {
    tx.execute_batch("DELETE FROM iac_nodes; DELETE FROM iac_references;")?;
    let mut insert =
        tx.prepare_cached("INSERT INTO iac_nodes VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)")?;
    for n in nodes {
        let (line, end) = (n.line as i64, n.end_line as i64);
        insert.execute(params![
            n.address, n.root, n.ty, n.name, n.path, line, end, n.block
        ])?;
    }
    let mut insert = tx.prepare_cached("INSERT INTO iac_references VALUES (?1, ?2, ?3, ?4)")?;
    for r in references {
        insert.execute(params![r.root, r.source, r.target, r.via])?;
    }
    Ok(())
}

const NODE: &str = "SELECT address, root, type, name, path, line, end_line, block FROM iac_nodes";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Node> {
    Ok(Node {
        address: r.get(0)?,
        root: r.get(1)?,
        ty: r.get(2)?,
        name: r.get(3)?,
        path: r.get(4)?,
        line: r.get::<_, i64>(5)? as usize,
        end_line: r.get::<_, i64>(6)? as usize,
        block: r.get(7)?,
    })
}

/// `<identity> <path>:<line> <address>`.
fn at(identity: &str, n: &Node) -> String {
    format!("{identity} {}:{} {}", n.path, n.line, n.address)
}

/// The IaC nodes of `db` (repository `identity`) whose address, type or physical name matches
/// `needle`, lowercased, as `search` hits: `<identity> <path>:<line> <address> (<type>[, name
/// "<name>"], root <root>)`, ordered by rank, then repository, path, line, address and root.
pub(crate) fn hits(identity: &str, db: &Connection, needle: &str) -> Result<Vec<Hit>> {
    // ponytail: scans every node of every repository; an indexed lookup waits for a workspace
    // that makes it slow.
    let mut statement = db.prepare(NODE)?;
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
        let name = (n.name.as_ref()).map_or(String::new(), |s| {
            format!(", name {}", serde_json::to_string(s).unwrap_or_default())
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
    let mut statement = db.prepare(&format!("{NODE} WHERE address = ?1"))?;
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
fn found(repos: &[(&str, &Connection)], text: &str) -> Result<Vec<(usize, Node)>> {
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
        "SELECT n.address, n.root, n.type, n.name, n.path, n.line, n.end_line, n.block, r.via \
         FROM iac_references r JOIN iac_nodes n ON n.root = r.root AND n.address = r.{to} \
         WHERE r.root = ?1 AND r.{from} = ?2"
    );
    let mut statement = db.prepare(&sql)?;
    let mut found: Vec<(Node, Option<String>)> = statement
        .query_map(params![node.root, node.address], |r| {
            Ok((row(r)?, r.get(8)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    found.sort_by(|(a, va), (b, vb)| {
        (&a.path, a.line, &a.address, va).cmp(&(&b.path, b.line, &b.address, vb))
    });
    Ok(found)
}

/// `<identity> <path>:<line> <address>`, then ` via <name>` when the reference went through one.
fn reached(identity: &str, n: &Node, via: &Option<String>) -> String {
    let via = via.as_ref().map_or(String::new(), |v| format!(" via {v}"));
    format!("{}{via}", at(identity, n))
}

/// `context` on the IaC node `text` names, or `None` when it names none: its first line
/// `<identity> <path>:<line>-<end> <address>`, then its type, its physical name, its root, the
/// nodes it references, those that reference it and its config values. A text that fits several
/// nodes lists them.
pub(crate) fn context(repos: &[(&str, &Connection)], text: &str) -> Result<Option<Vec<String>>> {
    let found = found(repos, text)?;
    let [(r, n)] = &found[..] else {
        return Ok((!found.is_empty()).then(|| several(repos, text, &found)));
    };
    let (identity, db) = repos[*r];
    let name = (n.name.as_ref()).map_or("none".into(), |s| {
        serde_json::to_string(s).unwrap_or_default()
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
    for (title, direction) in [
        ("references", Direction::Down),
        ("referenced by", Direction::Up),
    ] {
        let reached_nodes = references(db, n, direction)?;
        let items = (reached_nodes.iter()).map(|(m, via)| reached(identity, m, via));
        lines.extend(section(title, items.collect()));
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

    #[test]
    fn a_file_that_does_not_parse_adds_no_node() {
        let (nodes, _) = evaluate(&files(&[
            ("broken.tf", "resource \"a\" \"b\" {\n"),
            ("ok.tf", "resource \"a\" \"c\" {}\n"),
        ]));
        assert_eq!(addresses(&nodes), ["a.c @."]);
    }
}
