//! Go consumers (spec #25): the calls of `net/http` that name a method and a URL, and the
//! requests of resty. A call is a consumer when the source shows it reaches the library: through
//! the package (`http.Get`, `http.NewRequest`, `http.DefaultClient.Get`), through a name written as
//! a `net/http` `Client` (`c.Get`), or through a request made by `R()` on a client that traces
//! back to resty's constructor or is written as a resty `Client`. The URL is folded like any
//! other value (`lang/go/values.rs`) and a client wrapper is read at each of its call sites
//! (`Code::expand`). A resty client's base URL is the argument of every `SetBaseURL` on it; a
//! client held in a struct field is the one the composite literals that set the field give it.
//!
//! Three readings of `Code`: a function of a package is a callee that `Code::outside` names with
//! the package as its head (`direct`); a name written as a type is the `Code::origins` answer
//! with `Follow::Declarations` of the form `Type`; and the calls a client comes from are
//! `Code::origins` with `Follow::Values`, each route's climbed calls kept apart, so two clients
//! one factory makes stay apart and a base URL set on one does not leak to the other.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeSet, HashMap};

use super::super::{CallAt, based, consumer, fetch};
use super::split;
use crate::contract::Found;
use crate::lang::go::package_call;
use crate::lang::{Call, Part, Value};
use crate::resolve::{Code, Expand, Expanded, Field, Follow, Form, Of, method as member};

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
        let written = self.written_type(path, scope, name);
        written.is_some_and(|w| resty_type(&w, ty))
    }

    /// The type outside the repository that the source writes for `name` (`c *http.Client` is
    /// `net/http.Client`): the `Declarations` origin of the form `Type` with no steps. A method
    /// result of a typed value (`x := c.M()`, form `Type` with the step `M`) and a repository
    /// function's written result type (form `Call`) are no written type.
    fn written_type(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<String> {
        let found = self
            .code
            .origins(path, scope, Of::Name(name), Follow::Declarations);
        let origin = found.into_iter().next()?;
        let typed = origin.form == Form::Type && origin.routes.iter().all(|r| r.steps.is_empty());
        typed.then_some(origin.outside?.spelled)
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
        let found = self.code.origins(
            path,
            scope,
            Of::Value(value),
            Follow::Values {
                passes: crate::resolve::passes_nothing,
            },
        );
        // Each route's climbed calls, once: routes that differ only in the constant they held
        // repeat the same calls, and one client is not read twice.
        let mut climbs = BTreeSet::new();
        for origin in found {
            let Some(call) = origin.call else { continue };
            climbs.extend(origin.routes.into_iter().map(|r| (call, r.through)));
        }
        for ((p, i), through) in climbs {
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
    /// of the repository gives it. `None` when no literal sets it. A field read as a client is
    /// Unknown when a literal sets it to a value that is not traced or that the source does not
    /// tell, when it is written through a selector anywhere (`x.rc = ..`), or when it is met again
    /// while it is followed. A field read as a request (`request`: `s.repo.Get(..)` with no `R()`
    /// before it) holds a request only through a traced one or a value written as resty's
    /// `Request`: untraced there is a repository, a cache or a Kubernetes client, not "maybe
    /// resty", so those cases hold nothing (the cycle and depth guards stay Unknown).
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
            None if request => Some(Vec::new()),
            None => Some(vec![Client::Unknown]),
            Some(values) if values.is_empty() => None,
            Some(values) => {
                let mut out = Vec::new();
                for (path, scope, value) in values {
                    let clients = self.reaches(path, scope, value, depth + 1, request);
                    let untraced =
                        clients.contains(&Client::Typed) || (clients.is_empty() && !request);
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
        let made = package_call(self.code, path, &call.scope, &call.callee)
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
        let hole = crate::http::client_base();
        let mut bases: Vec<Option<Vec<Part>>> = Vec::new();
        for client in clients {
            let given: Vec<&Vec<Part>> = match client {
                Client::Made(made, through, _) => (set.iter())
                    .filter(|s| s.made == *made && through.ends_with(&s.through))
                    .map(|s| &s.base)
                    .collect(),
                // Only its type is known: whatever base it was given, Meada did not read it.
                Client::Typed | Client::Unknown => vec![&hole],
            };
            let base = match given[..] {
                [] => None,
                [one] => Some(one.clone()),
                [first, ref rest @ ..] if rest.iter().all(|b| *b == first) => Some(first.clone()),
                _ => Some(crate::http::client_base()),
            };
            if !bases.contains(&base) {
                bases.push(base);
            }
        }
        match bases.len() {
            0 => None,
            1 => bases.pop(),
            _ => Some(Some(crate::http::client_base())),
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
                    _ => crate::http::client_base(),
                };
                for client in self.clients(path, &call.scope, receiver, 0) {
                    let Client::Made(made, through, field) = client else {
                        continue;
                    };
                    // Set through a field: on which of the instances it holds is not known.
                    let base = match field {
                        true => crate::http::client_base(),
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
        let named = package_call(self.code, path, &call.scope, &call.callee);
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
        let client = self.written_type(path, &call.scope, receiver);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Lead;
    use crate::lang::go::Go;
    use crate::lang::{LanguagePack, Limit};
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    type Request = (Option<String>, Option<String>);

    /// The consumers of the Go file `src`.
    fn found(src: &str) -> Vec<Found> {
        let facts = Go.parse("x.go", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("x.go", &facts)]);
        let modules = Go.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Go);
        let code = Code::new(&resolver, &files, &Go);
        consumers(&code)
    }

    /// The contract and the `via` of each consumer of the Go file `src`.
    fn requests(src: &str) -> Vec<Request> {
        (found(src).into_iter())
            .map(|f| (f.contract, f.via))
            .collect()
    }

    /// A request on a client whose base Meada did not read: unresolved, with no `via` (#801).
    fn unread() -> Vec<Request> {
        vec![(None, None)]
    }

    const HEAD: &str = "package x\n\nimport \"github.com/go-resty/resty/v2\"\n";
    const BASE: &str = "resty.New().SetBaseURL(\"https://deep.example.com\")";

    /// `GET /s` with the base `BASE` sets, or with `via`.
    fn sent(via: Option<&str>) -> Vec<Request> {
        vec![(Some("http::GET::/s".into()), via.map(String::from))]
    }

    #[test]
    fn a_method_result_of_a_typed_client_is_not_a_written_type() {
        let src = |body: &str| {
            let head = "package x\n\nimport \"net/http\"\n\nfunc f(c *http.Client) {";
            format!("{head}\n\t{body}\n}}\n")
        };
        // The typed client itself sends; what one of its methods hands back is not a client.
        assert_eq!(requests(&src("c.Get(\"/s\")")).len(), 1);
        assert_eq!(requests(&src("x := c.Wrapped()\n\tx.Get(\"/s\")")), []);
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
        assert_eq!(held(CLIENT_DEPTH - 3), unread());
    }

    #[test]
    fn a_field_set_by_literals_past_the_budget_has_an_unknown_base() {
        let held = |n: usize| {
            let literals = "{c: client}, ".repeat(n);
            requests(&format!(
                "{HEAD}\nvar client = {BASE}\n\ntype T struct {{ c *resty.Client }}\n\n\
                 var all = []T{{{literals}}}\n\nfunc (t *T) Get() {{ t.c.R().Get(\"/s\") }}\n"
            ))
        };
        assert_eq!(held(3), sent(Some("host:deep.example.com")));
        // Past the file's budget the base is never claimed (the request itself may be absent).
        let past = held(crate::resolve::EVAL_BUDGET);
        assert!(past.iter().all(|(_, via)| via.is_none()), "{past:?}");
    }

    #[test]
    fn a_resty_verb_on_a_field_that_is_no_traced_request_is_no_consumer() {
        let src = "package x\n\nimport (\n\t\"context\"\n\t\"sigs.k8s.io/controller-runtime/pkg/client\"\n)\n\n\
            type Repo interface {\n\tGet(ctx context.Context, id string) (string, error)\n\
            \tDelete(ctx context.Context, id string) error\n\tPatch(ctx context.Context, id, v string) error\n}\n\n\
            type Service struct {\n\trepo Repo\n\tk8s client.Client\n}\n\n\
            func New(repo Repo, k8s client.Client) *Service { return &Service{repo: repo, k8s: k8s} }\n\n\
            func (s *Service) Show(ctx context.Context, id string) (string, error) { return s.repo.Get(ctx, id) }\n\
            func (s *Service) Remove(ctx context.Context, id string) error { return s.repo.Delete(ctx, id) }\n\
            func (s *Service) Edit(ctx context.Context, id string) error { return s.repo.Patch(ctx, id, \"v\") }\n\
            func (s *Service) Pod(ctx context.Context, k client.ObjectKey, o client.Object) error { return s.k8s.Get(ctx, k, o) }\n";
        assert_eq!(requests(src), []);
        // The control: a repository passed as a parameter.
        let control = src.replace("s.repo.Get(ctx, id)", "repo.Get(ctx, id)");
        assert_eq!(requests(&control), []);
    }

    /// A field holds the environment read every composite literal that sets it gives it (a
    /// literal sets the fields it names, https://go.dev/ref/spec#Composite_literals), a
    /// constructor's parameter through every exact caller; an assignment to a selector writes the
    /// field (https://go.dev/ref/spec#Assignment_statements), and the field is then unknown.
    #[test]
    fn a_field_holds_the_environment_read_every_literal_gives_it() {
        let head = "package x\n\nimport (\n\t\"net/http\"\n\t\"os\"\n\t\"strings\"\n)\n\n\
            type C struct{ base string }\n\n\
            func (c *C) send(method, path string) { http.NewRequest(method, c.base+path, nil) }\n\
            func (c *C) Direct() { http.Get(c.base + \"/orders\") }\n\
            func (c *C) Orders() { c.send(\"GET\", \"/orders\") }\n";
        let vias = |rest: &str| -> Vec<Option<String>> {
            let found = requests(&format!("{head}{rest}"));
            found.into_iter().map(|(_, via)| via).collect()
        };
        let read = Some("env:API_URL".to_string());
        let new = "func New(base string) *C { return &C{base: base} }\n";
        let getenv =
            "func A() *C { return New(strings.TrimRight(os.Getenv(\"API_URL\"), \"/\")) }\n";
        // Directly, and through a constructor whose every caller passes the read.
        let direct = "func A() *C { return &C{base: os.Getenv(\"API_URL\")} }\n";
        assert_eq!(vias(direct), [read.clone(), read.clone()]);
        let both = format!("{new}{getenv}func B() *C {{ return New(os.Getenv(\"API_URL\")) }}\n");
        assert_eq!(vias(&both), [read.clone(), read.clone()]);
        // A constructor of a constructor.
        let outer = format!(
            "{new}func Outer(b string) *C {{ return New(b) }}\n\
             func A() *C {{ return Outer(os.Getenv(\"API_URL\")) }}\n"
        );
        assert_eq!(vias(&outer), [read.clone(), read.clone()]);
        // The control of the name match below: an exact call of a constructor method.
        let exact = "type F struct{}\nfunc (f *F) Make(b string) *C { return &C{base: b} }\n\
             func A() *C { f := &F{}; return f.Make(os.Getenv(\"API_URL\")) }\n";
        assert_eq!(vias(exact), [read.clone(), read.clone()]);
        let unknown = vec![None, None];
        for rest in [
            // Two literals, or two callers, give different reads.
            format!("{direct}func B() *C {{ return &C{{base: os.Getenv(\"OTHER_URL\")}} }}\n"),
            format!("{new}{getenv}func B() *C {{ return New(os.Getenv(\"OTHER_URL\")) }}\n"),
            // A literal by position.
            "func A() C { return C{os.Getenv(\"API_URL\")} }\n".to_string(),
            format!("{direct}func B() C {{ return C{{os.Getenv(\"API_URL\")}} }}\n"),
            // A literal of a type the source does not tell is the struct's (an alias).
            format!("{direct}type B = C\nvar b = B{{base: os.Getenv(\"API_URL\")}}\n"),
            // No literal sets the field.
            String::new(),
            // A write through a selector anywhere.
            format!("{direct}func (c *C) Set(b string) {{ c.base = b }}\n"),
            // A constructor with no caller, or one a name match alone reaches.
            new.to_string(),
            "type F struct{}\nfunc (f *F) Make(b string) *C { return &C{base: b} }\n\
             func A(x interface{ Make(string) *C }) *C { return x.Make(os.Getenv(\"API_URL\")) }\n"
                .to_string(),
            // A literal string, or a read with more around it.
            "func A() *C { return &C{base: \"http://localhost:3000\"} }\n".to_string(),
            "func A() *C { return &C{base: os.Getenv(\"API_URL\") + \"/v1\"} }\n".to_string(),
            format!("{new}{getenv}func B() *C {{ return New(\"http://localhost:3000\") }}\n"),
            // A constructor that calls itself.
            "func New(b string) *C { New(b); return &C{base: b} }\n".to_string(),
            // Two fields set from each other.
            "type D struct{ base string }\n\
             func A(d *D) *C { return &C{base: d.base} }\n\
             func B(c *C) *D { return &D{base: c.base} }\n"
                .to_string(),
        ] {
            assert_eq!(vias(&rest), unknown, "{rest}");
        }
    }

    /// `T0.c` holds what `m0` gives it from `T1.c`, and so on to `Tn.c`, which a literal sets to
    /// a read; `reads` methods read `T0.c`.
    fn read_through_fields(n: usize, reads: usize) -> Vec<Request> {
        let mut src = "package x\n\nimport (\n\t\"net/http\"\n\t\"os\"\n)\n".to_string();
        for j in 0..reads {
            src += &format!("\nfunc (t *T0) Get{j}() {{ http.Get(t.c + \"/s\") }}\n");
        }
        for i in 0..=n {
            src += &format!("\ntype T{i} struct {{ c string }}\n");
            src += &match i < n {
                true => format!(
                    "func m{i}(t *T{}) *T{i} {{ return &T{i}{{c: t.c}} }}\n",
                    i + 1
                ),
                false => {
                    format!("func m{i}() *T{i} {{ return &T{i}{{c: os.Getenv(\"API_URL\")}} }}\n")
                }
            };
        }
        requests(&src)
    }

    #[test]
    fn a_field_read_through_fields_past_the_limit_is_unknown() {
        let read = |n| read_through_fields(n, 1).pop().unwrap().1;
        assert_eq!(read(3).as_deref(), Some("env:API_URL"));
        assert_eq!(
            read(crate::resolve::FIELD_DEPTH - 1).as_deref(),
            Some("env:API_URL")
        );
        assert_eq!(read(crate::resolve::FIELD_DEPTH), None);
    }

    /// Each field is folded once: many reads of a field held through a chain of fields stay
    /// within the file's budget.
    #[test]
    fn a_field_many_methods_read_is_folded_once() {
        let found = read_through_fields(crate::resolve::FIELD_DEPTH - 1, 2000);
        assert_eq!(found.len(), 2000);
        assert!(
            found
                .iter()
                .all(|(_, via)| via.as_deref() == Some("env:API_URL"))
        );
    }

    /// A lattice of constructors, each calling the next twice, ends within the file's budget:
    /// every path up the callers is climbed (as `Code::expand` climbs a wrapper's), each climb
    /// charged, so 2^40 paths stop at the budget and leave the field unknown.
    #[test]
    fn a_field_through_a_lattice_of_constructors_ends_in_bounded_time() {
        let lattice = |n: usize| {
            let mut src = "package x\n\nimport (\n\t\"net/http\"\n\t\"os\"\n)\n\n\
                type C struct{ base string }\n\n\
                func (c *C) Get() { http.Get(c.base + \"/s\") }\n\n\
                func N0(b string) *C { return &C{base: b} }\n"
                .to_string();
            for i in 1..=n {
                let p = i - 1;
                src += &format!("func N{i}(b string) *C {{ N{p}(b); return N{p}(b) }}\n");
            }
            src += &format!("func Main() *C {{ return N{n}(os.Getenv(\"API_URL\")) }}\n");
            let at = std::time::Instant::now();
            let found = requests(&src);
            (found, at.elapsed())
        };
        let (found, _) = lattice(3);
        assert_eq!(found, sent(Some("env:API_URL")));
        let (found, took) = lattice(40);
        assert_eq!(found, unread());
        // Measured at about 0.6 s in a local debug build.
        assert!(took < std::time::Duration::from_secs(15), "{took:?}");
    }

    #[test]
    fn a_request_field_written_through_a_selector_is_no_consumer() {
        // The ceiling of #621: the field's declared type is not read.
        let src = format!(
            "{HEAD}\ntype T struct {{ req *resty.Request }}\n\n\
             func (t *T) Set(r *resty.Request) {{ t.req = r }}\n\n\
             func (t *T) Go() {{ t.req.Get(\"/s\") }}\n"
        );
        assert_eq!(requests(&src), []);
    }

    #[test]
    fn a_request_held_in_a_field_keeps_its_consumer() {
        let held = |literal: &str| {
            requests(&format!(
                "{HEAD}\ntype T struct {{ req *resty.Request }}\n\n\
                 func f(r *resty.Request) *T {{ return &T{{req: {literal}}} }}\n\n\
                 func (t *T) Go() {{ t.req.Get(\"/s\") }}\n"
            ))
        };
        assert_eq!(
            held(&format!("{BASE}.R()")),
            sent(Some("host:deep.example.com"))
        );
        // A request that only its type tells is a request with no known base.
        assert_eq!(held("r"), unread());
    }

    /// resty appends a request's path to the client's base, which may hold a path of its own: on
    /// a client whose base Meada did not read (a field nothing sets) the request is an open item
    /// that names the client's base, its candidates the routes that end with the path (#801).
    #[test]
    fn a_request_on_a_client_whose_base_was_not_read_names_the_client_base() {
        let src = format!(
            "{HEAD}\ntype T struct {{ c *resty.Client }}\n\n\
             func (t *T) Go() {{ t.c.R().Get(\"/orders\") }}\n"
        );
        let leads: Vec<_> = (found(&src).into_iter())
            .map(|f| (f.contract, f.lead))
            .collect();
        let lead = Lead {
            limit: Limit::ClientBase,
            path: "/orders".into(),
        };
        assert_eq!(leads, [(None, Some(lead))]);
    }
}
