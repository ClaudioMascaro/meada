use std::collections::{BTreeMap, BTreeSet};

use super::normalized;
use crate::contract::{Found, Site};
use crate::http::{
    CallAt, METHODS, Mounts, contract, handler_site, join, path_literal, prefixes, provider,
};
use crate::lang::{Call, Value};
use crate::names::{one, spellings};
use crate::resolve::{Code, Of, Target, passes_nothing};

const VERBS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
];
const ON: &str = "on";
const REGISTER: &str = "register";
const SELF: &str = "self.";
const BOOT: &str = "boot(routes:)";

pub(super) struct Dialect {
    pub roots: &'static [&'static str],
    pub group: &'static str,
    pub method_first: bool,
    pub collections: bool,
    pub middleware: bool,
}

pub(super) const VAPOR: Dialect = Dialect {
    roots: &["Vapor.Application", "Vapor.Application.make"],
    group: "grouped",
    method_first: true,
    collections: true,
    middleware: true,
};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Node<'a> {
    Call(CallAt<'a>, Vec<CallAt<'a>>),
    Boot(&'a str, &'a str),
}

fn method(callee: &str) -> &str {
    callee.rsplit('.').next().unwrap_or(callee)
}

const PATHS: [&str; 3] = ["String", "Substring", "PathComponent"];

fn middleware(arg: &Value) -> bool {
    matches!(arg, Value::Call(inv)
        if inv.callee.starts_with(char::is_uppercase)
            && !inv.callee.split('.').any(|s| PATHS.contains(&s)))
}

// ponytail: real apps make tens of groups; past this (a crafted web of factories) nothing is
// served, rather than routes under part of their prefixes.
const VAPOR_NODES: usize = 4096;

struct Vapor<'c, 'a> {
    code: &'c Code<'a>,
    dialect: &'c Dialect,
    overflow: bool,
    ids: BTreeMap<Node<'a>, usize>,
    nodes: Vec<Node<'a>>,
    pending: Vec<usize>,
    roots: BTreeSet<usize>,
    mounts: Mounts<'a, usize>,
    registered: BTreeMap<(&'a str, &'a str), Vec<Node<'a>>>,
}

impl<'c, 'a> Vapor<'c, 'a> {
    fn framework(&self, path: &'a str, index: usize) -> bool {
        self.code.reaches(path, index).is_none()
    }

    fn intern(&mut self, node: Node<'a>) -> Option<usize> {
        if let Some(&id) = self.ids.get(&node) {
            return Some(id);
        }
        if self.nodes.len() >= VAPOR_NODES {
            self.overflow = true;
            return None;
        }
        let id = self.nodes.len();
        self.ids.insert(node.clone(), id);
        self.nodes.push(node);
        self.pending.push(id);
        Some(id)
    }

    fn origins(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        context: &[CallAt<'a>],
    ) -> Option<Vec<Node<'a>>> {
        let mut out = BTreeSet::new();
        let (found, unbound) =
            (self.code).origins_unbound(path, scope, Of::Value(value), passes_nothing, context);
        for (function, at) in unbound {
            if at == 0 && function.name.ends_with(&format!(".{BOOT}")) {
                out.insert(Node::Boot(function.path, function.name));
            }
        }
        for origin in found {
            let Some(call) = origin.call else {
                continue;
            };
            for route in origin.routes {
                let distinct: BTreeSet<&CallAt<'a>> = route.through.iter().collect();
                if distinct.len() < route.through.len() {
                    return None;
                }
                out.insert(Node::Call(call, route.through));
            }
        }
        Some(out.into_iter().collect())
    }

    fn application(&self, path: &'a str, index: usize) -> bool {
        let call = &self.code.files()[path].calls[index];
        let spelled = spellings(self.code, path, &call.scope, &call.callee);
        one(&spelled, |s| self.dialect.roots.contains(&s)).is_some()
    }

    fn components(&self, path: &'a str, call: &'a Call, args: &[&'a Value]) -> Option<String> {
        if self.dialect.middleware
            && call.callee.ends_with(self.dialect.group)
            && args.iter().all(|&a| middleware(a))
        {
            return Some(String::new());
        }
        let mut out = Vec::new();
        for &arg in args {
            out.push(path_literal(self.code.value(path, &call.scope, arg))?);
        }
        Some(out.join("/"))
    }

    fn read(&mut self, id: usize) {
        let code = self.code;
        let (call_at, context) = match self.nodes[id].clone() {
            Node::Boot(path, name) => {
                let parents = self
                    .registered
                    .get(&(path, name))
                    .cloned()
                    .unwrap_or_default();
                let parents = (parents.into_iter())
                    .filter_map(|node| Some((self.intern(node)?, Some(String::new()))))
                    .collect();
                self.mounts.insert(id, parents);
                return;
            }
            Node::Call(call_at, context) => (call_at, context),
        };
        let (path, index) = call_at;
        let Some(call) = code.call(path, index) else {
            return;
        };
        if self.application(path, index) {
            self.roots.insert(id);
            return;
        }
        let Some(receiver) = call.receiver.as_ref() else {
            return;
        };
        if method(&call.callee) != self.dialect.group || !self.framework(path, index) {
            return;
        }
        let args: Vec<&Value> = call.args.iter().collect();
        let prefix = match call.kwargs.is_empty() {
            true => self.components(path, call, &args),
            false => None,
        };
        let Some(parents) = self.origins(path, &call.scope, receiver, &context) else {
            return;
        };
        let parents: Vec<(usize, Option<String>)> = (parents.into_iter())
            .filter_map(|node| Some((self.intern(node)?, prefix.clone())))
            .collect();
        self.mounts.insert(id, parents);
    }

    fn http_method(value: &Value) -> Option<String> {
        let name = match value {
            Value::Ref(name) => method(name),
            Value::Member(ty, name) if ty.is_empty() => name,
            _ => return None,
        };
        let method = name.to_uppercase();
        METHODS.contains(&method.as_str()).then_some(method)
    }

    fn collection(&self, path: &'a str, call: &'a Call) -> Option<Target<'a>> {
        let arg = call.keyword("collection").or_else(|| call.args.first())?;
        let (scope, arg) = match arg {
            Value::Ref(name) => {
                let bound = self.code.local(path, &call.scope, name)?;
                (bound.within.as_str(), &bound.value)
            }
            other => (call.scope.as_str(), other),
        };
        let Value::Call(constructed) = arg else {
            return None;
        };
        self.code.method(path, scope, &constructed.callee, BOOT)
    }
}

struct Route<'a> {
    path: &'a str,
    call: &'a Call,
    pattern: String,
    method: String,
    handler: Option<&'a Value>,
    nodes: Vec<usize>,
}

pub(super) fn providers(code: &Code, dialect: &Dialect) -> Vec<Found> {
    let files: Vec<_> = (code.files().iter())
        .filter(|(p, _)| p.ends_with(".swift"))
        .map(|(p, f)| (*p, *f))
        .collect();
    let mut vapor = Vapor {
        code,
        dialect,
        overflow: false,
        ids: BTreeMap::new(),
        nodes: Vec::new(),
        pending: Vec::new(),
        roots: BTreeSet::new(),
        mounts: Mounts::new(),
        registered: BTreeMap::new(),
    };
    let mut routes = Vec::new();
    for &(path, facts) in &files {
        for (n, call) in facts.calls.iter().enumerate() {
            let name = method(&call.callee);
            let verb = VERBS.iter().find(|(m, _)| *m == name).map(|(_, v)| *v);
            let known = verb.is_some() || (name == ON || (dialect.collections && name == REGISTER));
            if !known || !vapor.framework(path, n) {
                continue;
            }
            let Some(call) = code.call(path, n) else {
                continue;
            };
            let Some(receiver) = call.receiver.as_ref() else {
                continue;
            };
            let Some(nodes) = vapor.origins(path, &call.scope, receiver, &[]) else {
                continue;
            };
            if name == REGISTER {
                if let Some(collection) = vapor.collection(path, call) {
                    (vapor.registered.entry((collection.path, collection.name)))
                        .or_default()
                        .extend(nodes);
                }
                continue;
            }
            let positional: Vec<&Value> = (call.args.iter())
                .filter(|v| !matches!(v, Value::Function(_)))
                .collect();
            let (method, components) = match (verb, dialect.method_first) {
                (Some(verb), _) => (Some(verb.to_string()), &positional[..]),
                (None, true) => match positional.split_first() {
                    Some((first, rest)) => (Vapor::http_method(first), rest),
                    None => (None, &positional[..]),
                },
                (None, false) => (
                    call.keyword("method").and_then(Vapor::http_method),
                    &positional[..],
                ),
            };
            let trailing = call.args.last().filter(|v| matches!(v, Value::Function(_)));
            let handler = call.keyword("use").or(trailing);
            let pattern = vapor.components(path, call, components);
            let (Some(method), Some(pattern)) = (method, pattern) else {
                continue;
            };
            let nodes = nodes.into_iter().filter_map(|n| vapor.intern(n)).collect();
            routes.push(Route {
                path,
                call,
                pattern,
                method,
                handler,
                nodes,
            });
        }
    }
    while let Some(id) = vapor.pending.pop() {
        vapor.read(id);
    }
    if vapor.overflow {
        return Vec::new();
    }
    let root = |id: usize| vapor.roots.contains(&id).then(String::new);
    let mut found = Vec::new();
    for route in &routes {
        let mut served = BTreeSet::new();
        for &node in &route.nodes {
            served.extend(prefixes(node, &vapor.mounts, &root, &mut Vec::new()));
        }
        for prefix in served {
            let full = normalized(&join(&prefix, &route.pattern));
            let scope = &route.call.scope;
            let site = match route.handler {
                Some(Value::Ref(name)) if name.starts_with(SELF) => {
                    match code.definition(route.path, scope, &name[SELF.len()..]) {
                        Some(t) => Site::Def {
                            path: t.path.into(),
                            name: t.name.into(),
                        },
                        None => Site::File(route.path.into()),
                    }
                }
                handler => handler_site(code, route.path, scope, handler, &route.method, &full),
            };
            found.push(provider(site, contract(&route.method, &full)));
        }
    }
    found
}
