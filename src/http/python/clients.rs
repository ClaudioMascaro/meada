use super::super::rust::frameworks::{self, CLIENT_LIBRARIES};
use super::super::{HTTP_METHODS, Library, based, consumer, fetch};
use crate::contract::Found;
use crate::lang::{Facts, Imported, Part, Value};
use crate::resolve::{Code, Expand, Expanded, Follow, Of, Origin};

const PYTHON_LIBRARIES: &[Library] = &[
    Library {
        modules: &["requests"],
        clients: &["requests.Session"],
    },
    Library {
        modules: &["httpx"],
        clients: &["httpx.Client", "httpx.AsyncClient"],
    },
    Library {
        modules: &[],
        clients: &["aiohttp.ClientSession"],
    },
];

fn libraries() -> impl Iterator<Item = &'static Library> {
    PYTHON_LIBRARIES.iter().chain(CLIENT_LIBRARIES)
}

fn names_library(facts: &Facts) -> bool {
    let roots = libraries()
        .flat_map(|l| l.modules.iter().chain(l.clients))
        .filter_map(|n| n.split('.').next());
    let roots: Vec<&str> = roots.collect();
    let head = |path: &str| roots.contains(&path.split('.').next().unwrap_or_default());
    (facts.imports.iter()).any(|i| match &i.name {
        Imported::Path(target) => head(target),
        _ => roots.contains(&i.source.as_str()),
    }) || facts.externs.iter().any(|e| roots.contains(&e.as_str()))
}

fn sends(member: &str) -> bool {
    HTTP_METHODS.contains(&member) || member == "request"
}

// ponytail: a Python receiver that is the result of a call (`Session().get(...)`,
// `make().get(...)`) is not read, a Rust one is (the pack gives the receiver's value); a
// parameter given a client takes the base of every caller that passes one.
fn client_base<'a>(code: &Code<'a>, origins: Vec<Origin<'a>>) -> Option<(Option<Vec<Part>>, bool)> {
    let mut bases: Vec<Option<Vec<Part>>> = Vec::new();
    let mut joins = true;
    for (at, i) in origins.iter().filter_map(|o| o.call) {
        let Some(ctor) = code.call(at, i) else {
            continue;
        };
        let name = code.outside(at, &ctor.scope, &ctor.callee, false);
        let Some(name) =
            name.filter(|n| libraries().any(|l| l.clients.contains(&n.spelled.as_str())))
        else {
            continue;
        };
        joins &= name.spelled == "aiohttp.ClientSession";
        let base = match ctor.keyword("base_url") {
            None => None,
            Some(url) => match code.value(at, &ctor.scope, url) {
                Value::Undefined => None,
                Value::Str(parts) => Some(parts),
                _ => Some(crate::http::client_base()),
            },
        };
        if !bases.contains(&base) {
            bases.push(base);
        }
    }
    match bases.len() {
        0 => None,
        1 => bases.pop().map(|base| (base, joins)),
        _ => Some((Some(crate::http::client_base()), joins)),
    }
}

pub(in crate::http) fn consumers(code: &Code) -> Vec<Found> {
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        let imports = names_library(facts);
        for (n, call) in facts.calls.iter().enumerate() {
            let function = imports
                .then(|| code.outside(path, &call.scope, &call.callee, false))
                .flatten()
                .and_then(|outside| {
                    let (module, function) = outside.spelled.rsplit_once('.')?;
                    let library = libraries().any(|l| l.modules.contains(&module));
                    (library && sends(function)).then(|| (function.to_string(), None, false))
                });
            let method =
                (call.callee.rsplit_once('.')).filter(|(_, m)| function.is_none() && sends(m));
            let (member, base, joins) = match (function, method) {
                (Some(function), _) => function,
                (None, Some((receiver, member))) => {
                    let origins = match code.receiver(path, n) {
                        Some(value) => code.origins(
                            path,
                            &call.scope,
                            Of::Value(value),
                            Follow::Values {
                                passes: frameworks::passes_on,
                            },
                        ),
                        None if receiver.starts_with('(') => continue,
                        None => code.origins(
                            path,
                            &call.scope,
                            Of::Name(receiver),
                            Follow::Values {
                                passes: frameworks::passes_on,
                            },
                        ),
                    };
                    match client_base(code, origins) {
                        Some((base, joins)) => (member.to_string(), base, joins),
                        None => continue,
                    }
                }
                _ => continue,
            };
            let read = |args: &[Value], kwargs: &[(String, Value)]| {
                let arg = |n: usize, key: &str| {
                    let named = kwargs.iter().find(|(k, _)| k == key).map(|(_, v)| v);
                    args.get(n).or(named)
                };
                let (method, url) = match member.as_str() {
                    "request" => (arg(0, "method").cloned(), arg(1, "url")?),
                    name => (
                        Some(Value::Str(vec![Part::Text(name.to_uppercase())])),
                        arg(0, "url")?,
                    ),
                };
                let url = match base.as_deref() {
                    Some(base) if joins && crate::http::rooted_over_unread(base, url) => {
                        url.clone()
                    }
                    base => based(base, url),
                };
                fetch(&[url, method.unwrap_or(Value::Unknown)])
            };
            for Expanded {
                site: (site, at),
                found: read,
                ..
            } in code.expand(Expand::Call(path, n), &read)
            {
                found.push(consumer(site, at, read, None));
            }
        }
    }
    found
}
