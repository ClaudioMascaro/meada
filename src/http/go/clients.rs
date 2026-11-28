use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeSet, HashMap};

use super::super::{CallAt, based, consumer, fetch};
use super::split;
use crate::contract::Found;
use crate::lang::go::package_call;
use crate::lang::{Call, Part, Value};
use crate::resolve::{Code, Expand, Expanded, Field, Follow, Form, Of, method as member};

// ponytail: real chains hold a handful of calls; the limit keeps a crafted one cheap.
const CLIENT_DEPTH: usize = 64;

const HTTP: &str = "net/http";

const HTTP_METHODS: &[(&str, &str)] = &[
    ("Get", "GET"),
    ("Head", "HEAD"),
    ("Post", "POST"),
    ("PostForm", "POST"),
];

pub(super) fn is_package_send(package: &str, name: &str) -> bool {
    package == HTTP && HTTP_METHODS.iter().any(|(member, _)| *member == name)
}

const RESTY_METHODS: &[(&str, &str)] = &[
    ("Get", "GET"),
    ("Post", "POST"),
    ("Put", "PUT"),
    ("Patch", "PATCH"),
    ("Delete", "DELETE"),
    ("Head", "HEAD"),
    ("Options", "OPTIONS"),
];

const RESTY: &[&str] = &["github.com/go-resty/resty", "resty.dev"];

const RESTY_CLIENTS: &[&str] = &["New", "NewWithClient", "NewWithLocalAddr"];

fn passes(member: &str) -> bool {
    ["Set", "Enable", "Disable", "Add", "On"]
        .iter()
        .any(|p| member.starts_with(p))
}

fn resty_type(name: &str, ty: &str) -> bool {
    split(name).is_some_and(|(package, name)| RESTY.contains(&package.as_str()) && name == ty)
}

enum Method {
    Is(&'static str),
    Argument(usize),
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Client<'a> {
    Made(CallAt<'a>, Vec<CallAt<'a>>, bool),
    Typed,
    Unknown,
}

struct Set<'a> {
    made: CallAt<'a>,
    through: Vec<CallAt<'a>>,
    base: Vec<Part>,
}

type Held<'a> = Option<Vec<Client<'a>>>;

struct Clients<'c, 'a> {
    code: &'c Code<'a>,
    bases: OnceCell<Vec<Set<'a>>>,
    walked: RefCell<HashMap<(bool, CallAt<'a>), Vec<Client<'a>>>>,
    fields: RefCell<HashMap<(bool, bool, Field<'a>), Held<'a>>>,
}

impl<'c, 'a> Clients<'c, 'a> {
    fn typed(&self, path: &'a str, scope: &'a str, name: &'a str, ty: &str) -> bool {
        let written = self.written_type(path, scope, name);
        written.is_some_and(|w| resty_type(&w, ty))
    }

    fn written_type(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<String> {
        let found = self
            .code
            .origins(path, scope, Of::Name(name), Follow::Declarations);
        let origin = found.into_iter().next()?;
        let typed = origin.form == Form::Type && origin.routes.iter().all(|r| r.steps.is_empty());
        typed.then_some(origin.outside?.spelled)
    }

    fn clients(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        depth: usize,
    ) -> Vec<Client<'a>> {
        self.reaches(path, scope, value, depth, false)
    }

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
            && let Some(held) = self.held(field, depth, request, crate::http::is_test_file(path))
        {
            return held;
        }
        let mut out = self.traced(path, scope, value, depth, request);
        let ty = if request { "Request" } else { "Client" };
        if let Value::Ref(name) = value
            && self.typed(path, scope, name, ty)
            && (out.is_empty() || self.untraced(path, scope, name, depth, request))
        {
            out.push(Client::Typed);
        }
        out
    }

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

    fn held(&self, field: Field<'a>, depth: usize, request: bool, test_read: bool) -> Held<'a> {
        if depth >= CLIENT_DEPTH {
            return Some(vec![Client::Unknown]);
        }
        let key = (request, test_read, field);
        if let Some(known) = self.fields.borrow().get(&key) {
            return known.clone();
        }
        (self.fields.borrow_mut()).insert(key, Some(vec![Client::Unknown]));
        let values = match self.code.field_written(field.2) {
            true => None,
            false => self.code.field_values(field, test_read),
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
        (self.fields.borrow_mut()).insert(key, found.clone());
        found
    }

    fn untraced(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        depth: usize,
        request: bool,
    ) -> bool {
        let Some((function, n)) = self.code.unchanged_param_of(path, scope, name) else {
            return false;
        };
        let args = self.code.param_args(function, n);
        args.iter()
            .any(|&(p, sc, arg)| self.traced(p, sc, arg, depth + 1, request).is_empty())
    }

    fn hands(&self, path: &'a str, index: usize, depth: usize, request: bool) -> Vec<Client<'a>> {
        if let Some(known) = self.walked.borrow().get(&(request, (path, index))) {
            return known.clone();
        }
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
        let default = named.is_some_and(|n| n == format!("{HTTP}.DefaultClient.{member}"));
        let client = self.written_type(path, &call.scope, receiver);
        if default || client.is_some_and(|c| c == format!("{HTTP}.Client")) {
            return Some((Method::Is(table(HTTP_METHODS)?), 0, None));
        }
        let clients = self.requests(path, &call.scope, self.code.receiver(path, index)?, 0);
        let base = self.base(&clients)?;
        match member {
            "Execute" => Some((Method::Argument(0), 1, base)),
            _ => Some((Method::Is(table(RESTY_METHODS)?), 0, base)),
        }
    }
}

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

    fn found(src: &str) -> Vec<Found> {
        let facts = Go.parse("x.go", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("x.go", &facts)]);
        let modules = Go.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Go);
        let code = Code::new(&resolver, &files, &Go);
        consumers(&code)
    }

    fn requests(src: &str) -> Vec<Request> {
        (found(src).into_iter())
            .map(|f| (f.contract, f.via))
            .collect()
    }

    fn unread() -> Vec<Request> {
        vec![(None, None)]
    }

    const HEAD: &str = "package x\n\nimport \"github.com/go-resty/resty/v2\"\n";
    const BASE: &str = "resty.New().SetBaseURL(\"https://deep.example.com\")";

    fn sent(via: Option<&str>) -> Vec<Request> {
        vec![(Some("http::GET::/s".into()), via.map(String::from))]
    }

    #[test]
    fn a_method_result_of_a_typed_client_is_not_a_written_type() {
        let src = |body: &str| {
            let head = "package x\n\nimport \"net/http\"\n\nfunc f(c *http.Client) {";
            format!("{head}\n\t{body}\n}}\n")
        };
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
        let deep = sent(Some("host:deep.example.com"));
        assert_eq!(chained(CLIENT_DEPTH - 3), deep);
        assert_eq!(chained(CLIENT_DEPTH - 2), []);
    }

    #[test]
    fn a_client_held_through_fields_past_the_limit_has_an_unknown_base() {
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
        let control = src.replace("s.repo.Get(ctx, id)", "repo.Get(ctx, id)");
        assert_eq!(requests(&control), []);
    }

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
        let direct = "func A() *C { return &C{base: os.Getenv(\"API_URL\")} }\n";
        assert_eq!(vias(direct), [read.clone(), read.clone()]);
        let both = format!("{new}{getenv}func B() *C {{ return New(os.Getenv(\"API_URL\")) }}\n");
        assert_eq!(vias(&both), [read.clone(), read.clone()]);
        let outer = format!(
            "{new}func Outer(b string) *C {{ return New(b) }}\n\
             func A() *C {{ return Outer(os.Getenv(\"API_URL\")) }}\n"
        );
        assert_eq!(vias(&outer), [read.clone(), read.clone()]);
        let exact = "type F struct{}\nfunc (f *F) Make(b string) *C { return &C{base: b} }\n\
             func A() *C { f := &F{}; return f.Make(os.Getenv(\"API_URL\")) }\n";
        assert_eq!(vias(exact), [read.clone(), read.clone()]);
        let unknown = vec![None, None];
        for rest in [
            format!("{direct}func B() *C {{ return &C{{base: os.Getenv(\"OTHER_URL\")}} }}\n"),
            format!("{new}{getenv}func B() *C {{ return New(os.Getenv(\"OTHER_URL\")) }}\n"),
            "func A() C { return C{os.Getenv(\"API_URL\")} }\n".to_string(),
            format!("{direct}func B() C {{ return C{{os.Getenv(\"API_URL\")}} }}\n"),
            format!("{direct}type B = C\nvar b = B{{base: os.Getenv(\"API_URL\")}}\n"),
            String::new(),
            format!("{direct}func (c *C) Set(b string) {{ c.base = b }}\n"),
            new.to_string(),
            "type F struct{}\nfunc (f *F) Make(b string) *C { return &C{base: b} }\n\
             func A(x interface{ Make(string) *C }) *C { return x.Make(os.Getenv(\"API_URL\")) }\n"
                .to_string(),
            "func A() *C { return &C{base: \"http://localhost:3000\"} }\n".to_string(),
            "func A() *C { return &C{base: os.Getenv(\"API_URL\") + \"/v1\"} }\n".to_string(),
            format!("{new}{getenv}func B() *C {{ return New(\"http://localhost:3000\") }}\n"),
            "func New(b string) *C { New(b); return &C{base: b} }\n".to_string(),
            "type D struct{ base string }\n\
             func A(d *D) *C { return &C{base: d.base} }\n\
             func B(c *C) *D { return &D{base: c.base} }\n"
                .to_string(),
        ] {
            assert_eq!(vias(&rest), unknown, "{rest}");
        }
    }

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
        assert!(took < std::time::Duration::from_secs(15), "{took:?}");
    }

    #[test]
    fn a_request_field_written_through_a_selector_is_no_consumer() {
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
        assert_eq!(held("r"), unread());
    }

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
