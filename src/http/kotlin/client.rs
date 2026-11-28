use super::{VERBS, external, implicit};
use crate::contract::Found;
use crate::index::Indexed;
use crate::jvm::names::{LOCAL_DEPTH, kotlin};
use crate::jvm::spring::{self, Configs};
use crate::lang::kotlin::TYPE_ARGUMENT;
use crate::lang::{Bound, Call, Part, Value};
use crate::names::{one, spellings, typed};
use crate::resolve::{Code, Expand, Expanded, Follow, Injected, Of, passes_nothing};

use super::super::{based, consumer, fetches};

const REQUEST: &str = "io.ktor.client.request.";
const CLIENT: &str = "io.ktor.client.HttpClient";
const DEFAULT_REQUEST: &[&str] = &[
    "io.ktor.client.plugins.defaultRequest",
    "io.ktor.client.features.defaultRequest",
];
const REQUEST_FUNCTION: &str = "request";
const METHOD: &str = "method=";

#[derive(PartialEq)]
enum Client<'a> {
    Made(&'a str, usize),
    Typed,
    Unknown,
}

struct Clients<'c, 'a> {
    code: &'c Code<'a>,
}

impl<'c, 'a> Clients<'c, 'a> {
    fn of(&self, path: &'a str, scope: &'a str, receiver: &'a Value) -> Option<Vec<Client<'a>>> {
        self.reach(path, scope, receiver, 0)
    }

    fn reach(
        &self,
        path: &'a str,
        scope: &'a str,
        receiver: &'a Value,
        depth: usize,
    ) -> Option<Vec<Client<'a>>> {
        let found = self.code.origins(
            path,
            scope,
            Of::Value(receiver),
            Follow::Values {
                passes: passes_nothing,
            },
        );
        let (mut out, mut other) = (Vec::new(), false);
        for origin in found {
            let made = origin.outside.map(|o| o.spellings()).unwrap_or_default();
            match origin.call {
                Some((p, i)) if made.iter().any(|s| s == CLIENT) || self.makes(p, i) => {
                    out.push(Client::Made(p, i));
                }
                _ => other = true,
            }
        }
        if out.is_empty() {
            let Value::Ref(name) = receiver else {
                return None;
            };
            let written = typed(self.code, path, scope, name);
            if written.iter().any(|s| s == CLIENT) {
                return Some(vec![Client::Typed]);
            }
            let Bound { within, value, .. } = self.code.local(path, scope, name)?;
            return match depth < LOCAL_DEPTH {
                true => self.reach(path, within, value, depth + 1),
                false => None,
            };
        }
        if other {
            out.push(Client::Unknown);
        }
        Some(out)
    }

    fn makes(&self, path: &'a str, index: usize) -> bool {
        let call = &self.code.files()[path].calls[index];
        let spelled = spellings(self.code, path, &call.scope, &call.callee);
        one(&spelled, |s| s == CLIENT).is_some()
    }

    fn base(&self, clients: &[Client<'a>]) -> Option<Vec<Part>> {
        let mut bases: Vec<Option<Vec<Part>>> = Vec::new();
        for client in clients {
            let base = match client {
                Client::Made(path, index) => self.configured(path, *index),
                Client::Typed | Client::Unknown => Some(crate::http::client_base()),
            };
            if !bases.contains(&base) {
                bases.push(base);
            }
        }
        match bases.len() {
            0 => None,
            1 => bases.pop().flatten(),
            _ => Some(crate::http::client_base()),
        }
    }

    fn configured(&self, path: &'a str, index: usize) -> Option<Vec<Part>> {
        let code = self.code;
        let Some(Value::Function(block)) = code.call(path, index)?.args.last() else {
            return None;
        };
        let calls = |scope: &str, name: &str| -> Vec<(usize, &'a Call)> {
            let found = (code.files()[path].calls.iter().enumerate())
                .filter(|(_, c)| c.scope == scope && implicit(&c.callee) == Some(name));
            found
                .filter_map(|(n, _)| Some((n, code.call(path, n)?)))
                .collect()
        };
        let mut bases: Vec<Vec<Part>> = Vec::new();
        for (_, default) in calls(block, "defaultRequest") {
            let known = spellings(code, path, &default.scope, "defaultRequest");
            if one(&known, |s| DEFAULT_REQUEST.contains(&s)).is_none() {
                continue;
            }
            let Some(Value::Function(lambda)) = default.args.last() else {
                continue;
            };
            let urls = calls(lambda, "url");
            let hole = crate::http::client_base;
            let base = match &urls[..] {
                [] => continue,
                [(_, url)] => match url.args.first() {
                    Some(Value::Function(_)) | None => hole(),
                    Some(value) => match code.value(path, &url.scope, value) {
                        Value::Str(parts) => parts,
                        _ => hole(),
                    },
                },
                _ => hole(),
            };
            bases.push(base);
        }
        match &mut bases[..] {
            [] => None,
            [one] => Some(std::mem::take(one)),
            _ => Some(crate::http::client_base()),
        }
    }
}

fn requested(kwargs: &[(String, Value)], given: bool) -> Value {
    match kwargs.iter().find(|(k, _)| k == METHOD) {
        None if given => Value::Unknown,
        None => text("GET".into()),
        Some((_, Value::Ref(name))) => Value::Ref(name.clone()),
        Some((_, Value::External(spelled))) => external(spelled).map_or(Value::Unknown, text),
        Some(_) => Value::Unknown,
    }
}

fn text(method: String) -> Value {
    Value::Str(vec![Part::Text(method)])
}

pub(in crate::http) fn consumers<'a>(code: &Code<'a>, indexed: &Indexed) -> Vec<Found> {
    let configs = std::cell::OnceCell::new();
    let inject = |code: &Code<'a>, path: &'a str, on: Injected<'a>| {
        let configs = configs.get_or_init(|| Configs::read(indexed.configs, indexed.manifests));
        spring::inject(configs, code, path, on.annotated()?)
    };
    let clients = Clients { code };
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        if !kotlin(path) {
            continue;
        }
        for (n, stored) in facts.calls.iter().enumerate() {
            let Some((_, member)) = stored.callee.rsplit_once('.') else {
                continue;
            };
            if !VERBS.contains(&member) && member != REQUEST_FUNCTION {
                continue;
            }
            let spelled = spellings(code, path, &stored.scope, member);
            if one(&spelled, |s| s.strip_prefix(REQUEST) == Some(member)).is_none() {
                continue;
            }
            let Some(call) = code.call(path, n) else {
                continue;
            };
            let Some(receiver) = code.receiver(path, n) else {
                continue;
            };
            let Some(of) = clients.of(path, &call.scope, receiver) else {
                continue;
            };
            let base = clients.base(&of);
            let read = |args: &[Value], kwargs: &[(String, Value)]| {
                let [Value::Call(request)] = args else {
                    return None;
                };
                let first = (request.args.first()).filter(|v| !matches!(v, Value::Function(_)));
                let named = kwargs.iter().find(|(k, _)| k == "urlString");
                let url = (first.or(named.map(|(_, v)| v)))
                    .cloned()
                    .unwrap_or(Value::Unknown);
                let rest = request.args.iter().skip(usize::from(first.is_some()));
                let given = rest.into_iter().any(|v| !matches!(v, Value::Function(_)))
                    || (kwargs.iter()).any(|(k, v)| {
                        let assigned = k.ends_with('=') && k != TYPE_ARGUMENT;
                        k != "urlString" && !assigned && !matches!(v, Value::Function(_))
                    });
                let method = match member {
                    REQUEST_FUNCTION => requested(kwargs, given),
                    verb => text(verb.to_uppercase()),
                };
                let url = match base.as_deref() {
                    Some(base) if crate::http::rooted_over_unread(base, &url) => url,
                    base => based(base, &url),
                };
                fetches(&[url, method])
            };
            for Expanded {
                site: (site, at),
                found: read,
                ..
            } in code.expand_with(Expand::Chain(path, n), Some(&inject), &read)
            {
                (read.into_iter()).for_each(|read| found.push(consumer(site, at, read, None)));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Lead;
    use crate::lang::java::Java;
    use crate::lang::kotlin::Kotlin;
    use crate::lang::{LanguagePack, Limit};
    use crate::resolve::{EVAL_DEPTH, Resolver};
    use std::collections::BTreeMap;

    const KOTLIN: &str = "src/main/kotlin/a/C.kt";
    const IMPORTS: &str = "import io.ktor.client.HttpClient\nimport io.ktor.client.engine.cio.CIO\n\
        import io.ktor.client.request.get\n";

    fn read(body: &str) -> (usize, std::time::Duration) {
        with_code(body, |code, indexed| {
            let runs = (0..3).map(|_| {
                let at = std::time::Instant::now();
                (consumers(code, indexed).len(), at.elapsed())
            });
            runs.min_by_key(|(_, took)| *took).unwrap()
        })
    }

    fn with_code<R>(body: &str, f: impl Fn(&Code, &Indexed) -> R) -> R {
        let src = format!("package a\n{IMPORTS}{body}");
        let facts = Kotlin.parse(KOTLIN, src.as_bytes()).unwrap();
        let files = BTreeMap::from([(KOTLIN, &facts)]);
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let code = Code::new(&resolver, &files, &Java);
        let indexed = Indexed {
            manifests: &[],
            tsconfigs: &[],
            exported: &[],
            files: &[],
            configs: &[],
            symlinks: &[],
            channel_clients: &[],
        };
        f(&code, &indexed)
    }

    #[test]
    fn a_path_after_a_base_meada_did_not_read_is_an_open_item_that_names_it() {
        let body = "import io.ktor.client.plugins.defaultRequest\n\
            fun base(): String = runtimeBase()\n\
            val client = HttpClient(CIO) { defaultRequest { url(base()) } }\n\
            suspend fun f(typed: HttpClient) {\n    client.get(\"orders\")\n    \
            client.get(\"/health\")\n    typed.get(\"v1/items\")\n    typed.get(\"/ping\")\n    \
            HttpClient(CIO).get(\"${base()}/items\")\n}\n";
        let mut found: Vec<_> = with_code(body, |code, indexed| {
            (consumers(code, indexed).into_iter())
                .map(|f| (f.contract, f.lead))
                .collect()
        });
        found.sort_by_key(|(contract, lead)| (contract.clone(), lead.clone().map(|l| l.path)));
        let lead = |limit: Limit, path: &str| {
            Some(Lead {
                limit,
                path: path.into(),
            })
        };
        assert_eq!(
            found,
            [
                (None, lead(Limit::Returned("base".into()), "/items")),
                (None, lead(Limit::ClientBase, "/orders")),
                (None, lead(Limit::ClientBase, "/v1/items")),
                (Some("http::GET::/health".into()), None),
                (Some("http::GET::/ping".into()), None),
            ]
        );
    }

    #[test]
    fn a_client_is_followed_through_a_bounded_chain_of_locals() {
        let chain = |locals: usize| {
            let held: String = (1..locals)
                .map(|i| format!("    val c{i} = c{}\n", i - 1))
                .collect();
            let last = locals - 1;
            format!(
                "suspend fun f() {{\n    val c0 = HttpClient(CIO)\n{held}    \
                 c{last}.get(\"https://x.example.com/p\")\n}}\n"
            )
        };
        assert_eq!(read(&chain(LOCAL_DEPTH + 1)).0, 1);
        assert_eq!(read(&chain(EVAL_DEPTH + LOCAL_DEPTH)).0, 1);
        assert_eq!(read(&chain(EVAL_DEPTH + LOCAL_DEPTH + 1)).0, 0);
    }

    #[test]
    fn many_client_locals_are_read_in_linear_time() {
        let time = |n: usize| {
            let body: String = (0..n)
                .map(|i| {
                    format!(
                        "suspend fun f{i}() {{ val c{i} = HttpClient(CIO); \
                         c{i}.get(\"https://x.example.com/p{i}\") }}\n"
                    )
                })
                .collect();
            let (found, took) = read(&body);
            assert_eq!(found, n);
            took
        };
        let (small, large) = (time(500), time(2000));
        assert!(large < small * 9, "{small:?} then {large:?}");
    }
}
