//! Go consumers (spec #25): the calls of `net/http` that name a method and a URL, and the
//! requests of resty. A call is a consumer when the source shows it reaches the library: through
//! the package (`http.Get`, `http.NewRequest`, `http.DefaultClient.Get`), through a name written as
//! a `net/http` `Client` (`c.Get`), or through a request made by `R()` on a client that traces
//! back to resty's constructor or is written as a resty `Client`. The URL is folded like any
//! other value (`lang/go/values.rs`) and a client wrapper is read at each of its call sites
//! (`Code::expand`). A resty client's base URL is the argument of every `SetBaseURL` on it; a
//! client held in a struct field is the one the composite literals that set the field give it.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use super::super::{CallAt, based, consumer, fetch};
use super::split;
use crate::contract::Found;
use crate::lang::{Call, Part, Value};
use crate::resolve::{Code, Field, method as member};

/// How many calls a client or a request is followed back through (`New().SetX().R().SetY()`),
/// and how many names it is followed through.
// ponytail: real chains hold a handful of calls; the limit keeps a crafted one cheap.
const CLIENT_DEPTH: usize = 64;

const HTTP: &str = "net/http";

/// The methods of a `net/http` `Client` that name a URL, with the HTTP method each sends.
const HTTP_METHODS: &[(&str, &str)] = &[
    ("Get", "GET"),
    ("Head", "HEAD"),
    ("Post", "POST"),
    ("PostForm", "POST"),
];

/// The methods of a resty request that send it, with the HTTP method each sends.
const RESTY_METHODS: &[(&str, &str)] = &[
    ("Get", "GET"),
    ("Post", "POST"),
    ("Put", "PUT"),
    ("Patch", "PATCH"),
    ("Delete", "DELETE"),
    ("Head", "HEAD"),
    ("Options", "OPTIONS"),
];

/// The packages of resty, without a major version.
const RESTY: &[&str] = &["github.com/go-resty/resty", "resty.dev"];

/// The functions of resty that make a client.
const RESTY_CLIENTS: &[&str] = &["New", "NewWithClient", "NewWithLocalAddr"];

/// Whether the `member` of a resty client or request hands back the same client or request:
/// its setters (`SetBaseURL`, `SetHeader`) and switches.
fn passes(member: &str) -> bool {
    ["Set", "Enable", "Disable", "Add", "On"]
        .iter()
        .any(|p| member.starts_with(p))
}

/// Whether `name` (`github.com/go-resty/resty/v2.Client`) is the type `ty` of resty.
fn resty_type(name: &str, ty: &str) -> bool {
    split(name).is_some_and(|(package, name)| RESTY.contains(&package.as_str()) && name == ty)
}

/// How a call sends: the HTTP method it sends, or the argument that gives it, and the argument
/// that gives the URL.
enum Method {
    Is(&'static str),
    Argument(usize),
}

/// A resty client, as far as the source tells: the call that makes it with the calls of the
/// functions it came through (two clients one factory makes differ by the call that made each)
/// and whether it was reached through a struct field (which instance a base set there reaches is
/// not known); only its type; or one whose base is not known.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Client<'a> {
    Made(CallAt<'a>, Vec<CallAt<'a>>, bool),
    Typed,
    Unknown,
}

/// A base URL set on the clients a call makes: those that came through the calls `through` or
/// through more, as a base set inside a factory reaches what the factory makes.
struct Set<'a> {
    made: CallAt<'a>,
    through: Vec<CallAt<'a>>,
    base: Vec<Part>,
}

/// The clients a struct field holds, `None` when no literal sets it.
type Held<'a> = Option<Vec<Client<'a>>>;

struct Clients<'c, 'a> {
    code: &'c Code<'a>,
    /// The base URLs set on resty clients, read on first use.
    bases: OnceCell<Vec<Set<'a>>>,
    /// The clients each call hands back (`true`: the client a request is made on), so a long
    /// chain several calls start from is walked once.
    walked: RefCell<HashMap<(bool, CallAt<'a>), Vec<Client<'a>>>>,
    /// The clients each struct field holds, `None` when no literal sets it, so a field many
    /// requests read is followed once.
    fields: RefCell<HashMap<(bool, Field<'a>), Held<'a>>>,
}

impl<'c, 'a> Clients<'c, 'a> {
    /// Whether the name `name` is written as the type `ty` of resty.
    fn typed(&self, path: &'a str, scope: &'a str, name: &'a str, ty: &str) -> bool {
        let written = self.code.outside_type(path, scope, name);
        written.is_some_and(|w| resty_type(&w, ty))
    }

    /// The resty clients `value`, written in `scope` of `path`, may be: those the calls it comes
    /// from make, else the one its type names.
    fn clients(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        depth: usize,
    ) -> Vec<Client<'a>> {
        self.reaches(path, scope, value, depth, false)
    }

    /// The resty clients the requests `value` may be are made on.
    fn requests(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        depth: usize,
    ) -> Vec<Client<'a>> {
        self.reaches(path, scope, value, depth, true)
    }

    fn reaches(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        depth: usize,
        request: bool,
    ) -> Vec<Client<'a>> {
        if let Value::Ref(name) = value
            && let Some(field) = self.code.field(path, scope, name)
            && let Some(held) = self.held(field, depth, request)
        {
            return held;
        }
        let mut out = self.traced(path, scope, value, depth, request);
        let ty = if request { "Request" } else { "Client" };
        if let Value::Ref(name) = value
            && self.typed(path, scope, name, ty)
            && (out.is_empty() || self.untraced(path, scope, name, depth, request))
        {
            // Only its type is known, or a caller passes one whose origin is not.
            out.push(Client::Typed);
        }
        out
    }

    /// The clients the calls `value` comes from make or hand back.
    fn traced(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        depth: usize,
        request: bool,
    ) -> Vec<Client<'a>> {
        if depth >= CLIENT_DEPTH {
            return Vec::new();
        }
        let mut out: Vec<Client<'a>> = Vec::new();
        for ((p, i), through) in self.code.value_origins_through(path, scope, value) {
            for client in self.hands(p, i, depth + 1, request) {
                let client = match client {
                    Client::Made(made, inner, field) => {
                        let through = through.iter().chain(&inner).copied().collect();
                        Client::Made(made, through, field)
                    }
                    other => other,
                };
                if !out.contains(&client) {
                    out.push(client);
                }
            }
        }
        out
    }

    /// The clients the struct field `field` holds: those of the values every composite literal
    /// of the repository gives it. Unknown when a literal sets it to a value that is not traced
    /// or that the source does not tell, when it is written through a selector anywhere
    /// (`x.rc = ..`), or when it is met again while it is followed. `None` when no literal sets
    /// it.
    fn held(&self, field: Field<'a>, depth: usize, request: bool) -> Held<'a> {
        if depth >= CLIENT_DEPTH {
            return Some(vec![Client::Unknown]);
        }
        if let Some(known) = self.fields.borrow().get(&(request, field)) {
            return known.clone();
        }
        (self.fields.borrow_mut()).insert((request, field), Some(vec![Client::Unknown]));
        let values = match self.code.field_written(field.2) {
            true => None,
            false => self.code.field_values(field),
        };
        let found = match values {
            None => Some(vec![Client::Unknown]),
            Some(values) if values.is_empty() => None,
            Some(values) => {
                let mut out = Vec::new();
                for (path, scope, value) in values {
                    let clients = self.reaches(path, scope, value, depth + 1, request);
                    let untraced = clients.is_empty() || clients.contains(&Client::Typed);
                    let clients = clients.into_iter().map(|client| match client {
                        Client::Made(made, through, _) => Client::Made(made, through, true),
                        Client::Typed => Client::Unknown,
                        Client::Unknown => Client::Unknown,
                    });
                    for client in clients.chain(untraced.then_some(Client::Unknown)) {
                        if !out.contains(&client) {
                            out.push(client);
                        }
                    }
                }
                Some(out)
            }
        };
        (self.fields.borrow_mut()).insert((request, field), found.clone());
        found
    }

    /// Whether `name`, a parameter, is passed a client whose origin is not traced by a caller.
    fn untraced(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        depth: usize,
        request: bool,
    ) -> bool {
        let Some((function, n)) = self.code.param_of(path, scope, name) else {
            return false;
        };
        let args = self.code.param_args(function, n);
        args.iter()
            .any(|&(p, sc, arg)| self.traced(p, sc, arg, depth + 1, request).is_empty())
    }

    /// The clients the call at `index` of `path` hands back (`request` false), or makes its
    /// request on (`request` true): the call that makes a client, a setter that hands back what
    /// it is called on, `R()` that makes a request on a client.
    fn hands(&self, path: &'a str, index: usize, depth: usize, request: bool) -> Vec<Client<'a>> {
        if let Some(known) = self.walked.borrow().get(&(request, (path, index))) {
            return known.clone();
        }
        // A call reached again through its own chain adds nothing.
        self.walked
            .borrow_mut()
            .insert((request, (path, index)), Vec::new());
        let found = self.hand(path, index, depth, request);
        self.walked
            .borrow_mut()
            .insert((request, (path, index)), found.clone());
        found
    }

    fn hand(&self, path: &'a str, index: usize, depth: usize, request: bool) -> Vec<Client<'a>> {
        let Some(call) = self.code.call(path, index) else {
            return Vec::new();
        };
        let member = member(&call.callee);
        let made = self
            .code
            .outside_call(path, &call.scope, &call.callee)
            .as_deref()
            .and_then(split)
            .is_some_and(|(package, name)| {
                RESTY.contains(&package.as_str()) && RESTY_CLIENTS.contains(&name)
            });
        if made {
            return if request {
                Vec::new()
            } else {
                vec![Client::Made((path, index), Vec::new(), false)]
            };
        }
        let Some(receiver) = self.code.receiver(path, index) else {
            return Vec::new();
        };
        match (request, member) {
            (true, "R") => self.clients(path, &call.scope, receiver, depth),
            (true, _) if passes(member) => self.requests(path, &call.scope, receiver, depth),
            (false, _) if passes(member) => self.clients(path, &call.scope, receiver, depth),
            _ => Vec::new(),
        }
    }

    /// The base URL of `clients`: `Some(None)` when none is set, an unknown base when they
    /// differ. `None` when there is no client.
    fn base(&self, clients: &[Client<'a>]) -> Option<Option<Vec<Part>>> {
        let set = self.bases.get_or_init(|| self.set());
        let hole = vec![Part::Hole];
        let mut bases: Vec<Option<Vec<Part>>> = Vec::new();
        for client in clients {
            let given: Vec<&Vec<Part>> = match client {
                Client::Made(made, through, _) => (set.iter())
                    .filter(|s| s.made == *made && through.ends_with(&s.through))
                    .map(|s| &s.base)
                    .collect(),
                Client::Typed => Vec::new(),
                Client::Unknown => vec![&hole],
            };
            let base = match given[..] {
                [] => None,
                [one] => Some(one.clone()),
                [first, ref rest @ ..] if rest.iter().all(|b| *b == first) => Some(first.clone()),
                _ => Some(vec![Part::Hole]),
            };
            if !bases.contains(&base) {
                bases.push(base);
            }
        }
        match bases.len() {
            0 => None,
            1 => bases.pop(),
            _ => Some(Some(vec![Part::Hole])),
        }
    }

    /// The base URL each `SetBaseURL` gives, with the clients it may be called on.
    fn set(&self) -> Vec<Set<'a>> {
        let mut set: Vec<Set<'a>> = Vec::new();
        for (&path, facts) in self.code.files() {
            for (n, stored) in facts.calls.iter().enumerate() {
                if !stored.callee.ends_with("SetBaseURL") {
                    continue;
                }
                let call = self.code.call(path, n);
                let (Some(call), Some(receiver)) = (call, self.code.receiver(path, n)) else {
                    continue;
                };
                let [url] = &call.args[..] else {
                    continue;
                };
                let base = match self.code.value(path, &call.scope, url) {
                    Value::Str(parts) => parts,
                    _ => vec![Part::Hole],
                };
                for client in self.clients(path, &call.scope, receiver, 0) {
                    let Client::Made(made, through, field) = client else {
                        continue;
                    };
                    // Set through a field: on which of the instances it holds is not known.
                    let base = match field {
                        true => vec![Part::Hole],
                        false => base.clone(),
                    };
                    let known = set
                        .iter()
                        .any(|s| (s.made, &s.through, &s.base) == (made, &through, &base));
                    if !known {
                        set.push(Set {
                            made,
                            through,
                            base,
                        });
                    }
                }
            }
        }
        set
    }

    /// How the call at `index` of `path` sends, with the base URL of the client it is made on: a
    /// function of `net/http`, a `Client` of it, or a resty request.
    fn send(
        &self,
        path: &'a str,
        index: usize,
        call: &'a Call,
    ) -> Option<(Method, usize, Option<Vec<Part>>)> {
        let (receiver, member) = call.callee.rsplit_once('.')?;
        let table = |table: &[(&str, &'static str)]| {
            table.iter().find(|(m, _)| *m == member).map(|(_, v)| *v)
        };
        let named = self.code.outside_call(path, &call.scope, &call.callee);
        let split = named.as_deref().and_then(split);
        match split
            .as_ref()
            .map(|(package, name)| (package.as_str(), *name))
        {
            // A function of the package.
            Some((HTTP, "NewRequest")) => return Some((Method::Argument(0), 1, None)),
            Some((HTTP, "NewRequestWithContext")) => {
                return Some((Method::Argument(1), 2, None));
            }
            Some((HTTP, function)) => {
                debug_assert_eq!(function, member);
                return Some((Method::Is(table(HTTP_METHODS)?), 0, None));
            }
            _ => {}
        }
        // The default client, or a name written as `http.Client`.
        let default = named.is_some_and(|n| n == format!("{HTTP}.DefaultClient.{member}"));
        let client = self.code.outside_type(path, &call.scope, receiver);
        if default || client.is_some_and(|c| c == format!("{HTTP}.Client")) {
            return Some((Method::Is(table(HTTP_METHODS)?), 0, None));
        }
        // A resty request.
        let clients = self.requests(path, &call.scope, self.code.receiver(path, index)?, 0);
        let base = self.base(&clients)?;
        match member {
            "Execute" => Some((Method::Argument(0), 1, base)),
            _ => Some((Method::Is(table(RESTY_METHODS)?), 0, base)),
        }
    }
}

/// `url` with its environment reads made holes when it holds more than one: which variable is
/// the base is unknown, so none is.
fn one_read(url: Value) -> Value {
    let Value::Str(parts) = url else {
        return url;
    };
    let reads = parts.iter().filter(|p| matches!(p, Part::Env(_))).count();
    let hole = |p: Part| {
        if matches!(p, Part::Env(_)) {
            Part::Hole
        } else {
            p
        }
    };
    match reads {
        0 | 1 => Value::Str(parts),
        _ => Value::Str(parts.into_iter().map(hole).collect()),
    }
}

/// Go consumers: see the module.
pub(in crate::http) fn consumers<'a>(code: &Code<'a>) -> Vec<Found> {
    if !code.pack().extensions().contains(&"go") {
        return Vec::new();
    }
    let clients = Clients {
        code,
        bases: OnceCell::new(),
        walked: RefCell::new(HashMap::new()),
        fields: RefCell::new(HashMap::new()),
    };
    let sends: Vec<&str> = [HTTP_METHODS, RESTY_METHODS]
        .iter()
        .flat_map(|t| t.iter().map(|(m, _)| *m))
        .chain(["NewRequest", "NewRequestWithContext", "Execute"])
        .collect();
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        for (n, stored) in facts.calls.iter().enumerate() {
            let member = member(&stored.callee);
            if !sends.contains(&member) {
                continue;
            }
            // The values of the call: `files` holds none, only the facts the index keeps.
            let Some(call) = code.call(path, n) else {
                continue;
            };
            let Some((method, url, base)) = clients.send(path, n, call) else {
                continue;
            };
            let read = |args: &[Value], _: &[(String, Value)]| {
                let method = match &method {
                    Method::Is(m) => Value::Str(vec![Part::Text(m.to_string())]),
                    Method::Argument(at) => args.get(*at)?.clone(),
                };
                let url = one_read(based(base.as_deref(), args.get(url)?));
                fetch(&[url, method])
            };
            for (site, at, read) in code.expand(path, n, &read) {
                found.push(consumer(site, at, read, None));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::go::Go;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    type Request = (Option<String>, Option<String>);

    /// The contract and the `via` of each consumer of the Go file `src`.
    fn requests(src: &str) -> Vec<Request> {
        let facts = Go.parse("x.go", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("x.go", &facts)]);
        let modules = Go.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Go);
        let code = Code::new(&resolver, &files, &Go);
        let found = consumers(&code).into_iter();
        found.map(|f| (f.contract, f.via)).collect()
    }

    const HEAD: &str = "package x\n\nimport \"github.com/go-resty/resty/v2\"\n";
    const BASE: &str = "resty.New().SetBaseURL(\"https://deep.example.com\")";

    /// `GET /s` with the base `BASE` sets, or with `via`.
    fn sent(via: Option<&str>) -> Vec<Request> {
        vec![(Some("http::GET::/s".into()), via.map(String::from))]
    }

    #[test]
    fn a_setter_chain_past_the_limit_is_no_consumer() {
        let chained = |n: usize| {
            let setters = ".SetTimeout(1)".repeat(n);
            requests(&format!(
                "{HEAD}\nfunc f() {{\n\t{BASE}{setters}.R().Get(\"/s\")\n}}\n"
            ))
        };
        // `New`, `SetBaseURL` and `R()` count too.
        let deep = sent(Some("host:deep.example.com"));
        assert_eq!(chained(CLIENT_DEPTH - 3), deep);
        assert_eq!(chained(CLIENT_DEPTH - 2), []);
    }

    #[test]
    fn a_client_held_through_fields_past_the_limit_has_an_unknown_base() {
        // `T0.c` holds what `m0` gives it from `T1.c`, and so on to `Tn.c`, which a literal sets.
        let held = |n: usize| {
            let mut src = format!("{HEAD}\nfunc (t *T0) Get() {{ t.c.R().Get(\"/s\") }}\n");
            for i in 0..=n {
                src += &format!("\ntype T{i} struct {{ c *resty.Client }}\n");
                src += &match i < n {
                    true => format!(
                        "func m{i}(t *T{}) *T{i} {{ return &T{i}{{c: t.c}} }}\n",
                        i + 1
                    ),
                    false => format!("func m{i}() *T{i} {{ return &T{i}{{c: {BASE}}} }}\n"),
                };
            }
            requests(&src)
        };
        let deep = sent(Some("host:deep.example.com"));
        assert_eq!(held(3), deep);
        // `R()`, the fields, `SetBaseURL` and `New` count.
        assert_eq!(held(CLIENT_DEPTH - 4), deep);
        assert_eq!(held(CLIENT_DEPTH - 3), sent(None));
    }
}
