use super::super::{
    ANY, CallAt, Mounts, PARAM, handler_site, join, normalize_path, path_literal, prefixes,
    provider,
};
use super::frameworks::{self, Made};
use crate::contract::Found;
use crate::lang::{Call, Value, literal};
use crate::resolve::{Code, Follow, Of, Origin, method, scope_of};
use std::collections::{BTreeSet, HashMap, HashSet};

const AXUM_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "trace",
];

// ponytail: a route serves a handful of methods; a crafted chain is not read to its end.
const METHOD_CHAIN: usize = 64;

fn axum_routers_of<'a>(origins: Vec<Origin<'a>>) -> Vec<CallAt<'a>> {
    let router = |o: &Origin| {
        let outside = o.outside.as_ref();
        outside.is_some_and(|o| frameworks::made(&o.spelled) == Some(Made::Router))
    };
    (origins.into_iter())
        .filter(router)
        .filter_map(|o| o.call)
        .collect()
}

fn method_router<'a>(
    code: &Code<'a>,
    path: &'a str,
    value: &'a Value,
) -> Option<Vec<(&'a Call, String)>> {
    let Value::Call(invocation) = value else {
        return None;
    };
    let chain = code.chain(path, code.call_index(path, &invocation.range)?)?;
    if chain.len() > METHOD_CHAIN {
        return None;
    }
    let (&start, steps) = chain.split_last()?;
    let mut out = Vec::new();
    let start = code.call(path, start)?;
    let outside = code.outside(path, &start.scope, &start.callee, start.by_path);
    let external = outside
        .map(|o| o.spelled)
        .filter(|_| start.receiver.is_none())?;
    if frameworks::made(&external) != Some(Made::MethodRouter) {
        return None;
    }
    match external.strip_prefix("axum.routing.")? {
        "any" => out.push((start, ANY.to_string())),
        "MethodRouter.new" => {}
        name => out.push((start, name.to_uppercase())),
    }
    for &at in steps.iter().rev() {
        let call = code.call(path, at)?;
        match call.callee.rsplit('.').next()? {
            name if AXUM_METHODS.contains(&name) => out.push((call, name.to_uppercase())),
            name if frameworks::passes(Made::MethodRouter).contains(&name) => {}
            _ => return None,
        }
    }
    Some(out)
}

fn axum_routers<'a>(
    code: &Code<'a>,
    path: &'a str,
    call: &'a Call,
    value: &'a Value,
) -> Vec<CallAt<'a>> {
    axum_routers_of(code.origins(
        path,
        &call.scope,
        Of::Value(value),
        Follow::Values {
            passes: frameworks::passes_on,
        },
    ))
}

#[derive(Default)]
struct Unread<'a> {
    seen: HashSet<*const Value>,
    router_chain: HashMap<(&'a str, usize), bool>,
}

impl<'a> Unread<'a> {
    fn may_be_router(&mut self, code: &Code<'a>, path: &'a str, index: usize) -> bool {
        if let Some(&known) = self.router_chain.get(&(path, index)) {
            return known;
        }
        let Some(chain) = code.chain(path, index) else {
            self.router_chain.insert((path, index), false);
            return false;
        };
        let passes = frameworks::passes(Made::Router);
        let mut all = true;
        for (n, &at) in chain.iter().enumerate().rev() {
            if n + 1 < chain.len() {
                let callee = code.call(path, at).map_or("", |c| c.callee.as_str());
                all &= passes.contains(&method(callee));
            }
            self.router_chain.insert((path, at), all);
        }
        all
    }
}

fn is_plain(ty: &str) -> bool {
    const PLAIN: &[&str] = &[
        "str", "String", "Path", "PathBuf", "OsStr", "OsString", "bool", "char", "usize", "isize",
        "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "i128", "f32", "f64",
    ];
    PLAIN.contains(&ty.rsplit("::").next().unwrap_or(ty))
}

fn axum_unread<'a>(
    code: &Code<'a>,
    unread: &mut Unread<'a>,
    path: &'a str,
    scope: &'a str,
    value: &'a Value,
) -> BTreeSet<CallAt<'a>> {
    let mut found = BTreeSet::new();
    let mut work = vec![(path, scope, value)];
    while let Some((path, scope, value)) = work.pop() {
        if !unread.seen.insert(std::ptr::from_ref(value)) {
            continue;
        }
        let index = match value {
            Value::Call(invocation) => code.call_index(path, &invocation.range),
            _ => None,
        };
        let plain = match value {
            Value::Ref(name) => code
                .written(path, scope, name)
                .is_some_and(|(_, _, ty)| is_plain(ty)),
            _ => false,
        };
        if !plain && index.is_none_or(|at| unread.may_be_router(code, path, at)) {
            let routers = axum_routers_of(code.origins(
                path,
                scope,
                Of::Value(value),
                Follow::Values {
                    passes: frameworks::passes_on,
                },
            ));
            if !routers.is_empty() {
                found.extend(routers);
                continue;
            }
        }
        match value {
            Value::Call(_) => {
                let Some(index) = index else {
                    continue;
                };
                let Some(call) = code.call(path, index) else {
                    continue;
                };
                let given = call.args.iter().chain(&call.receiver);
                work.extend(given.map(|v| (path, call.scope.as_str(), v)));
                work.extend(code.returned(path, index));
            }
            Value::Ref(name) => {
                let held = code.constant(path, scope, name);
                work.extend(held.map(|((at, name), v)| (at, scope_of(name), v)));
            }
            Value::List(items) => work.extend(items.iter().map(|v| (path, scope, v))),
            _ => {}
        }
    }
    found
}

const OPEN: char = '\u{1}';
const CLOSE: char = '\u{2}';

fn axum_path(path: &str) -> String {
    let segments = path.split('/');
    let segments = segments.map(|s| if s.starts_with('*') { PARAM } else { s });
    let path = segments.collect::<Vec<_>>().join("/");
    path.replace("{{", &OPEN.to_string())
        .replace("}}", &CLOSE.to_string())
}

fn axum_literal(path: &str) -> String {
    path.replace(OPEN, "{").replace(CLOSE, "}")
}

// ponytail: a router held by `let mut` or reassigned, `route_service`, and
// `on(MethodFilter, h)` are not followed; the routes on them are not providers.
pub(super) fn providers(code: &Code) -> Vec<Found> {
    if !code.pack().extensions().contains(&"rs") {
        return Vec::new();
    }
    let mut mounts = Mounts::new();
    let mut routes = Vec::new();
    for (&path, facts) in code.files() {
        let mut touched = false;
        let mut walked = Unread::default();
        for (n, call) in facts.calls.iter().enumerate() {
            let member = call.callee.rsplit('.').next().unwrap_or_default();
            if !matches!(
                member,
                "route" | "nest" | "nest_service" | "merge" | "route_service" | "fallback_service"
            ) {
                continue;
            }
            touched = true;
            let Some(call) = code.call(path, n) else {
                continue;
            };
            let Some(receiver) = &call.receiver else {
                continue;
            };
            let parents = axum_routers(code, path, call, receiver);
            match (member, call.args.as_slice()) {
                ("route", [route, handlers]) => {
                    let Value::Str(parts) = code.value(path, &call.scope, route) else {
                        continue;
                    };
                    let (Some(route), Some(handlers)) =
                        (literal(&parts), method_router(code, path, handlers))
                    else {
                        continue;
                    };
                    for at in parents {
                        routes.push((path, at, axum_path(&route), handlers.clone()));
                    }
                }
                ("nest" | "nest_service" | "merge", args) => {
                    let (prefix, child) = match (member, args) {
                        ("nest" | "nest_service", [prefix, child]) => {
                            let prefix = code.value(path, &call.scope, prefix);
                            (path_literal(prefix).map(|p| axum_path(&p)), child)
                        }
                        ("merge", [child]) => (Some(String::new()), child),
                        _ => continue,
                    };
                    let children = axum_routers(code, path, call, child);
                    for child in &children {
                        let mounted = mounts.entry(*child).or_default();
                        mounted.extend(parents.iter().map(|&at| (at, prefix.clone())));
                    }
                    if children.is_empty() {
                        for unread in axum_unread(code, &mut walked, path, &call.scope, child) {
                            mounts.entry(unread).or_default();
                        }
                    }
                }
                ("route_service" | "fallback_service", args) => {
                    for unread in args
                        .iter()
                        .flat_map(|a| axum_unread(code, &mut walked, path, &call.scope, a))
                    {
                        mounts.entry(unread).or_default();
                    }
                }
                _ => {}
            }
        }
        if touched {
            for (scope, value) in code.rebound(path) {
                for unread in axum_unread(code, &mut walked, path, scope, value) {
                    mounts.entry(unread).or_default();
                }
            }
        }
    }
    let mut found = Vec::new();
    let root = |_: CallAt| Some(String::new());
    for (path, at, route, handlers) in routes {
        for prefix in prefixes(at, &mounts, &root, &mut Vec::new()) {
            let full = axum_literal(&normalize_path(&join(&prefix, &route)));
            for (call, method) in &handlers {
                let site = handler_site(code, path, &call.scope, call.args.first(), method, &full);
                found.push(provider(site, format!("http::{method}::{full}")));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(lets: usize, links: usize) -> String {
        let chain = ".a()".repeat(links);
        let rebound: String = (0..lets)
            .map(|i| format!("    let x{i} = base.b();\n    let x{i} = 0;\n"))
            .collect();
        format!(
            "use axum::{{Router, routing::get}};\nasync fn h() {{}}\nfn f() -> u8 {{ 0 }}\n\
             pub fn g() {{\n    let base = f(){chain};\n{rebound}    Router::new().route(\"/a\", get(h));\n}}\n"
        )
    }

    #[test]
    fn many_lets_sharing_one_deep_value_are_read_in_linear_time() {
        use crate::lang::LanguagePack;
        use crate::lang::rust::Rust;
        use crate::resolve::Resolver;
        let src = shared(40, 500);
        let facts = Rust.parse("src/lib.rs", src.as_bytes()).unwrap();
        let files = std::collections::BTreeMap::from([("src/lib.rs", &facts)]);
        let resolver = Resolver::new(&files, Rust.modules(&[], &files).unwrap(), &Rust);
        let code = Code::new(&resolver, &files, &Rust);
        let start = std::time::Instant::now();
        let found = providers(&code);
        let took = start.elapsed();
        assert_eq!(found.len(), 1);
        assert!(took < std::time::Duration::from_secs(10), "{took:?}");
    }

    #[test]
    fn an_escaped_brace_is_literal_and_a_wildcard_is_a_parameter() {
        let read = |p: &str| axum_literal(&normalize_path(&axum_path(p)));
        assert_eq!(read("/{{raw}}"), "/{raw}");
        assert_eq!(read("/a/{{x}}/{id}/*rest"), "/a/{x}/{}/{}");
        assert_eq!(read("/{*rest}"), "/{}");
    }
}
