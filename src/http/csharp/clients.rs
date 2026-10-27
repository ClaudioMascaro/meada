//! C# HTTP consumers (spec #28, #111): the `HttpClient` methods that send a request. A call
//! counts only when its receiver is an `HttpClient` by its declared type or by the construction
//! it is made on, the type read through the usings (`Code::written` gives the type the source
//! writes, `names::spellings` the names the imports give it), never by text; a client
//! `IHttpClientFactory.CreateClient(name)` made, held by a local or a field, is a named client,
//! which is not read.
//!
//! The call is read whole (`Expand::Chain`), so a wrapper (a method that takes the path, the
//! method or the client as a parameter) turns each of its call sites into a consumer with its
//! arguments substituted, and a `HttpMethod.X` arrives as a `Value::External` spelled by the
//! imports of the file that writes it. The URL folds string literals, interpolated, verbatim and
//! raw strings, `+`, `const` and `static readonly` fields (the pack's `Facts::consts`), locals
//! bound once (`Facts::bound`) and `new Uri(..)`. A name assigned anywhere in its file (`=`,
//! `+=`, `++`, `out`, `ref`) holds no one value, so a reassigned local, request or client is
//! unknown. A client's base is the `BaseAddress` its construction sets in an object
//! initializer (`new HttpClient { BaseAddress = new Uri(base) }`, in a local, a field, a
//! property or a `static readonly` field), or the one `BaseAddress = new Uri(base)` assignment
//! made on the name the call is made on, in the scope that declares the name: a base not read
//! (two assignments, one that may not run, one that sets another address than the initializer,
//! a name that is assigned or hidden) is unknown, so the path alone is the contract. A relative URL joins the base as `Uri` does (RFC 3986).
//!
//! A typed client (#113): the `HttpClient` a dependency injection container passes to a
//! constructor nothing calls (`Injected::Param`) has the one `BaseAddress` the constructor sets on
//! it when no registration sets another, else the one every `AddHttpClient<T>(..)` of its class
//! sets (`registered`), and a `readonly` field the constructor assigns it holds it (the pack's
//! `Const::scope`). Two registrations with different bases, or one Meada cannot read, leave it
//! unknown.
// ponytail: a field set from a method's parameter or assigned twice has an unknown base, and so
// has a client a wrapper takes whose callers build it elsewhere; an instance `readonly` field
// holds only what one constructor assigns it.

use super::super::{based, consumer, fetches};
use super::reads::Reads;
use crate::contract::Found;
use crate::index::Indexed;
use crate::lang::{Bound, Class, Facts, Invocation, Part, Receiver, Step, Ty, Value};
use crate::names::{is_type, one};
use crate::resolve::{Code, Expand, Expanded, Injected, Target, method as member};
use std::cell::OnceCell;
use std::collections::BTreeMap;

const HTTP_CLIENT: &str = "System.Net.Http.HttpClient";
const REQUEST: &str = "System.Net.Http.HttpRequestMessage";
const HTTP_METHOD: &str = "System.Net.Http.HttpMethod.";
/// The methods of an `HttpClient` that send one method.
const CLIENT_METHODS: &[(&str, &str)] = &[
    ("GetAsync", "GET"),
    ("PostAsync", "POST"),
    ("PutAsync", "PUT"),
    ("PatchAsync", "PATCH"),
    ("DeleteAsync", "DELETE"),
    ("GetStringAsync", "GET"),
    ("GetStreamAsync", "GET"),
    ("GetByteArrayAsync", "GET"),
    ("GetFromJsonAsync", "GET"),
    ("PostAsJsonAsync", "POST"),
    ("PutAsJsonAsync", "PUT"),
];
/// The members of `HttpMethod` that name one method.
const MEMBERS: &[(&str, &str)] = &[
    ("Get", "GET"),
    ("Post", "POST"),
    ("Put", "PUT"),
    ("Patch", "PATCH"),
    ("Delete", "DELETE"),
    ("Head", "HEAD"),
    ("Options", "OPTIONS"),
];
/// The member `BaseAddress` of an `HttpClient`.
const BASE: &str = "BaseAddress";
/// The framework's registration of a typed client, `services.AddHttpClient<T>(..)`.
const REGISTER: &str = "AddHttpClient";

/// C# HTTP consumers: see the module.
pub(in crate::http) fn consumers<'a>(code: &Code<'a>, indexed: &Indexed) -> Vec<Found> {
    let reads = Reads::new(indexed.configs);
    // Read the first time a typed client's constructor is met.
    let typed = OnceCell::new();
    let inject = |code: &Code<'a>, _: &'a str, on: Injected<'a>| match on {
        Injected::Param(ctor, n) => {
            let typed = typed.get_or_init(|| registered(code, &reads));
            injected(code, &reads, typed, ctor, n)
        }
        Injected::Annotated(_) => None,
    };
    let mut found = Vec::new();
    for (&path, facts) in code.files().iter().filter(|(p, _)| p.ends_with(".cs")) {
        for (n, stored) in facts.calls.iter().enumerate() {
            let Some(sent) = sent(stored.callee.as_str()) else {
                continue;
            };
            let Some((receiver, _)) = stored.callee.rsplit_once('.') else {
                continue;
            };
            if stored.decorates.is_some() || !is_client(code, path, n, receiver) {
                continue;
            }
            let read = |values: &[Value], _: &[(String, Value)]| {
                let [Value::Call(call)] = values else {
                    return None;
                };
                let at = (path, stored.scope.as_str());
                let (url, method) = sends(code, &reads, at, n, receiver, sent, call);
                fetches(&[url, method])
            };
            for Expanded {
                site: (site, at),
                found: reads,
                ..
            } in code.expand_with(Expand::Chain(path, n), Some(&inject), &read)
            {
                (reads.into_iter()).for_each(|read| found.push(consumer(site, at, read, None)));
            }
        }
    }
    found
}

/// What a call of the method `callee` ends in sends.
#[derive(Clone, Copy)]
enum Sent {
    /// The method of this name sends this HTTP method.
    Verb(&'static str),
    /// `SendAsync(new HttpRequestMessage(method, url))`.
    Request,
}

fn sent(callee: &str) -> Option<Sent> {
    let name = member(callee);
    match CLIENT_METHODS.iter().find(|(m, _)| *m == name) {
        Some((_, verb)) => Some(Sent::Verb(verb)),
        None if name == "SendAsync" => Some(Sent::Request),
        None => None,
    }
}

/// Whether the call at `index` of `path`, made on `receiver`, is made on an `HttpClient`: the
/// receiver is written as one (a field, a property, a parameter, a typed local), or was made by
/// `new HttpClient(..)`; one `CreateClient` made, held by a local or a field, is a named client.
fn is_client<'a>(code: &Code<'a>, path: &'a str, index: usize, receiver: &'a str) -> bool {
    let scope = code.files()[path].calls[index].scope.as_str();
    if named(code, (path, scope), receiver) {
        return false;
    }
    if let Some((at, within, ty)) = code.written(path, scope, receiver) {
        return is_http_client(code, at, within, ty);
    }
    // No type written: the construction in place, or a local made by `new HttpClient()`.
    match code.receiver(path, index) {
        Some(Value::New(ty, _)) => is_http_client(code, path, scope, ty),
        _ => false,
    }
}

/// Whether `ty`, written in `scope` of `path`, is `System.Net.Http.HttpClient`, in full or
/// through the usings.
fn is_http_client<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, ty: &'a str) -> bool {
    is_type(code, path, scope, ty, HTTP_CLIENT)
}

/// Whether the name `receiver` holds the client `CreateClient(name)` made: a local declared
/// with it, or a name assigned it (`_n = factory.CreateClient("x")`), at the scope that
/// declares it.
fn named<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), receiver: &'a str) -> bool {
    let name = receiver.strip_prefix("this.").unwrap_or(receiver);
    let made =
        |b: &&Bound| matches!(&b.value, Value::Call(c) if member(&c.callee) == "CreateClient");
    code.local(path, scope, name).iter().any(made)
        || code
            .written(path, scope, name)
            .is_some_and(|(at, within, _)| {
                at == path
                    && code
                        .bounds(path, within, &format!("{name}="))
                        .iter()
                        .any(made)
            })
}

/// The binding of `name.member` (a member assigned through the name), when the scope that
/// binds it is the one that declares `name`: a parameter that hides a field, or a nested type's
/// own field, keeps its own.
fn written<'a>(
    code: &Code<'a>,
    (path, scope): (&'a str, &'a str),
    name: &'a str,
    member: &str,
) -> Option<&'a Bound> {
    let name = name.strip_prefix("this.").unwrap_or(name);
    let head = name.split('.').next().unwrap_or(name);
    let (at, within, _) = code.written(path, scope, head)?;
    let bound = code.local(path, scope, &format!("{name}.{member}"))?;
    (at == path && bound.scope == within).then_some(bound)
}

/// The URL and the method of `call`, read whole by `Expand::Chain`, made on `receiver` (as the
/// stored call writes it) at `at`.
fn sends<'a>(
    code: &Code<'a>,
    reads: &Reads,
    (path, scope): (&'a str, &'a str),
    index: usize,
    receiver: &'a str,
    sent: Sent,
    call: &Invocation,
) -> (Value, Value) {
    let text = |m: &str| Value::Str(vec![Part::Text(m.to_string())]);
    let (url, method) = match sent {
        Sent::Verb(verb) => (call.args.first(), text(verb)),
        Sent::Request => match call.args.first() {
            Some(Value::New(ty, args)) if is_request(code, (path, scope), ty) => {
                // `request.Method = HttpMethod.X`, set after the construction, wins.
                let set = match code.call(path, index).and_then(|c| c.args.first()) {
                    Some(Value::Ref(name)) => written(code, (path, scope), name, "Method"),
                    _ => None,
                };
                let method = match set {
                    Some(b) => member_set(code, (path, &b.within), &b.value),
                    None => http_method(args.first()),
                };
                (args.get(1), method)
            }
            // A request a wrapper takes: read at each caller.
            Some(Value::Ref(name)) => return (Value::Ref(name.clone()), Value::Ref(name.clone())),
            _ => (None, Value::Unknown),
        },
    };
    let url = reads.value(code, path, self::url(code, (path, scope), url));
    match call.receiver.as_deref() {
        // A client a wrapper takes: its base is read at each caller.
        Some(Value::Ref(name)) => (Value::Ref(name.clone()), method),
        client => {
            // The assignment on the name the call is made on wins over the initializer.
            let assigned = (written(code, (path, scope), receiver, BASE))
                .map(|b| code.value(path, &b.within, &b.value));
            let base = base(code, reads, path, client, assigned.as_ref());
            (based_on(base, url), method)
        }
    }
}

/// The method a folded `HttpMethod.X` gives: the member as text, a parameter as it is (the call
/// is a wrapper's), anything else unknown.
fn http_method(value: Option<&Value>) -> Value {
    let member = |s: &str| {
        let name = s.strip_prefix(HTTP_METHOD)?;
        MEMBERS.iter().find(|(m, _)| *m == name).map(|(_, v)| *v)
    };
    match value {
        Some(Value::External(spellings)) => match one(spellings, |s| member(s).is_some()) {
            Some(s) => Value::Str(vec![Part::Text(member(s).unwrap_or_default().into())]),
            None => Value::Unknown,
        },
        Some(Value::Ref(name)) => Value::Ref(name.clone()),
        _ => Value::Unknown,
    }
}

/// The method a `HttpMethod.X` assigned to a request names (`Method = HttpMethod.X`), written as
/// is in `scope` of `path`: unknown when it is not one of the members or `HttpMethod` is not the
/// framework's.
fn member_set<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), value: &'a Value) -> Value {
    let named = match value {
        Value::Ref(r) => r.rsplit_once('.'),
        _ => None,
    };
    let method = HTTP_METHOD.trim_end_matches('.');
    match named {
        Some((ty, name)) if is_type(code, path, scope, ty, method) => {
            match MEMBERS.iter().find(|(m, _)| *m == name) {
                Some((_, verb)) => Value::Str(vec![Part::Text(verb.to_string())]),
                None => Value::Unknown,
            }
        }
        _ => Value::Unknown,
    }
}

/// The URL a folded argument gives: through `new Uri(..)` of one argument, which the
/// repository does not declare.
fn url<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), value: Option<&Value>) -> Value {
    match value {
        Some(Value::New(ty, args)) if is_uri(code, (path, scope), ty) && args.len() == 1 => {
            args[0].clone()
        }
        Some(value) => value.clone(),
        None => Value::Unknown,
    }
}

/// Whether `ty`, written in `scope`, is `System.Uri`: no type of the repository hides it.
fn is_uri<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), ty: &str) -> bool {
    ty == "System.Uri" || (ty == "Uri" && code.definition(path, scope, "Uri").is_none())
}

/// Whether `ty`, written in `scope`, is `HttpRequestMessage`: in full or through the usings of
/// the call (`is_type`).
// ponytail: the type of a construction read whole is no name of the file, so it is looked up as
// the end of `REQUEST` it writes; an alias using (`using R = ..HttpRequestMessage`) is not seen.
fn is_request<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), ty: &str) -> bool {
    let end = REQUEST.len().checked_sub(ty.len()).map(|at| &REQUEST[at..]);
    end.is_some_and(|end| end == ty && is_type(code, path, scope, end, REQUEST))
}

/// The base address of a client folded as `client` (a construction with an initializer) or
/// assigned (`BaseAddress = new Uri(base)`); a hole when neither is read. Its environment and configuration reads are decided (`Reads`).
fn base<'a>(
    code: &Code<'a>,
    reads: &Reads,
    path: &'a str,
    client: Option<&Value>,
    assigned: Option<&Value>,
) -> Vec<Part> {
    let initial = match client {
        Some(Value::New(_, args)) => match args.last() {
            Some(Value::Object(set)) => set.iter().rev().find(|(k, _)| k == BASE).map(|(_, v)| v),
            _ => None,
        },
        _ => None,
    };
    // An assignment may come after the call, or not run: one that sets another address than
    // the initializer leaves the base unknown.
    let [assigned, initial] = [assigned, initial].map(|v| v.map(|v| address(code, reads, path, v)));
    match (assigned, initial) {
        (Some(a), Some(i)) if a != i => vec![Part::Hole],
        (Some(set), _) | (None, Some(set)) => set,
        (None, None) => vec![Part::Hole],
    }
}

/// The parts of the address a `BaseAddress = new Uri(base)` written in `path` sets, its reads
/// decided; a hole when it is not read.
fn address<'a>(code: &Code<'a>, reads: &Reads, path: &'a str, set: &Value) -> Vec<Part> {
    match set {
        Value::New(_, args) if args.len() == 1 => match reads.value(code, path, args[0].clone()) {
            Value::Str(parts) => parts,
            Value::Ref(name) => vec![Part::Ref(name.clone())],
            _ => vec![Part::Hole],
        },
        _ => vec![Part::Hole],
    }
}

/// The typed clients the repository registers with the framework's `AddHttpClient<T>(..)`
/// (`AddHttpClient<I, T>`: the last type argument), by the class's qualified name: the address
/// each registration's configuring lambda sets on its client parameter
/// (`c => c.BaseAddress = new Uri(base)`), read where it is written. A registration that only
/// names the client adds none; one Meada cannot read (a method group, a lambda that passes its
/// client on, a `ConfigureHttpClient` chained on it) adds an unknown one.
// ponytail: a lambda's address is folded where it is written: a parameter of the method that
// registers the client is unknown.
fn registered<'a>(code: &Code<'a>, reads: &Reads) -> BTreeMap<&'a str, Vec<Vec<Part>>> {
    let mut out: BTreeMap<&'a str, Vec<Vec<Part>>> = BTreeMap::new();
    for (&path, facts) in code.files().iter().filter(|(p, _)| p.ends_with(".cs")) {
        // Built the first time a registration is met: the file's calls by scope, by the call
        // they are made on, and its bindings by scope.
        let index = OnceCell::new();
        for (n, stored) in facts.calls.iter().enumerate() {
            if member(&stored.callee) != REGISTER
                || stored.decorates.is_some()
                || code.reaches(path, n).is_some()
            {
                continue;
            }
            let Some(Step::Argument(tys)) = code.uses(path, n).and_then(|u| u.first()?.first())
            else {
                continue;
            };
            let Some(Ty::Named(ty)) = tys.last() else {
                continue;
            };
            let Some(class) = code.definition(path, &stored.scope, ty) else {
                continue;
            };
            let index = index.get_or_init(|| Index::of(facts));
            let parts = match registration(code, (path, facts), index, n) {
                Some(Some(set)) => address(
                    code,
                    reads,
                    path,
                    &code.value(path, &set.within, &set.value),
                ),
                Some(None) => continue,
                None => vec![Part::Hole],
            };
            let entry = out.entry(class.name).or_default();
            if !entry.contains(&parts) {
                entry.push(parts);
            }
        }
    }
    out
}

/// A file's calls by the scope they are written in and by the call they are made on
/// (`Call::on`), and its bindings by scope (`Facts::receivers`).
struct Index<'a> {
    by_scope: BTreeMap<&'a str, Vec<usize>>,
    on: BTreeMap<usize, Vec<usize>>,
    bindings: BTreeMap<&'a str, Vec<&'a Receiver>>,
}

impl<'a> Index<'a> {
    fn of(facts: &'a Facts) -> Self {
        let (mut by_scope, mut on) = (BTreeMap::new(), BTreeMap::new());
        for (n, call) in facts.calls.iter().enumerate() {
            by_scope
                .entry(call.scope.as_str())
                .or_insert_with(Vec::new)
                .push(n);
            if let Some(made) = call.on {
                on.entry(made).or_insert_with(Vec::new).push(n);
            }
        }
        let mut bindings: BTreeMap<&str, Vec<&Receiver>> = BTreeMap::new();
        for r in &facts.receivers {
            bindings.entry(r.scope.as_str()).or_default().push(r);
        }
        Index {
            by_scope,
            on,
            bindings,
        }
    }
}

/// What the registration at call `n` of `path` sets as its client's `BaseAddress`: `Some(None)`
/// when it configures nothing, `None` when Meada cannot read what it configures.
fn registration<'a>(
    code: &Code<'a>,
    (path, facts): (&'a str, &'a Facts),
    index: &Index<'a>,
    n: usize,
) -> Option<Option<&'a Bound>> {
    // A `ConfigureHttpClient` on the registration's builder configures the client again.
    let mut chain = vec![n];
    while let Some(m) = chain.pop() {
        for &next in index.on.get(&m).into_iter().flatten() {
            if member(&facts.calls[next].callee) == "ConfigureHttpClient" {
                return None;
            }
            chain.push(next);
        }
    }
    let args = &code.call(path, n)?.args;
    let lambda = match args.last() {
        Some(Value::Function(lambda)) => lambda.as_str(),
        // The client's name, or nothing.
        _ if args.iter().all(|a| matches!(a, Value::Str(_))) => return Some(None),
        _ => return None,
    };
    // The lambda's parameters: its bindings of no class and no value (a local holds one).
    let params: Vec<&str> = (index.bindings.get(lambda).into_iter().flatten())
        .filter(|r| r.class == Class::Unknown)
        .filter(|r| {
            (code.local(path, lambda, &r.name)).is_some_and(|b| matches!(b.value, Value::Unknown))
        })
        .map(|r| r.name.as_str())
        .collect();
    // A call in the lambda (or deeper) that takes a parameter may set anything on it.
    let inside = (index.by_scope.range(lambda..))
        .take_while(|(s, _)| s.starts_with(lambda))
        .filter(|(s, _)| s.len() == lambda.len() || s[lambda.len()..].starts_with('.'))
        .flat_map(|(_, calls)| calls);
    for &m in inside {
        let args = code.call(path, m).map_or(&[][..], |c| c.args.as_slice());
        if args
            .iter()
            .any(|a| matches!(a, Value::Ref(r) if params.contains(&r.as_str())))
        {
            return None;
        }
    }
    let set: Vec<&Bound> = (params.iter())
        .flat_map(|p| code.bounds(path, lambda, &format!("{p}.{BASE}")))
        .collect();
    match set.as_slice() {
        [] => Some(None),
        [b] => Some(Some(b)),
        _ => None,
    }
}

/// The client a dependency injection container passes for the parameter `n` of the
/// constructor `ctor`, when it is an `HttpClient`: one with the address the constructor sets on
/// the parameter, or else the one address every registration of its class as a typed client
/// sets (`registered`). `None` when neither gives one.
fn injected<'a>(
    code: &Code<'a>,
    reads: &Reads,
    registered: &BTreeMap<&'a str, Vec<Vec<Part>>>,
    ctor: Target<'a>,
    n: usize,
) -> Option<Value> {
    let class = crate::parent(ctor.name);
    let facts = code.files().get(ctor.path)?;
    if !(facts.constructors.get(class)).is_some_and(|c| c.iter().any(|c| c == ctor.name)) {
        return None;
    }
    let param = code.param(ctor, n)?;
    let (at, within, ty) = code.written(ctor.path, ctor.name, &param.name)?;
    if !is_http_client(code, at, within, ty) {
        return None;
    }
    let own = code.bounds(ctor.path, ctor.name, &format!("{}.{BASE}", param.name));
    let registered = registered.get(class).map_or(&[][..], Vec::as_slice);
    let parts = match (own.as_slice(), registered) {
        // The constructor's own address, which no registration contradicts: one set after an
        // early `return`, or not at all, would leave the registration's.
        ([b], _) => {
            let own = code.value(ctor.path, &b.within, &b.value);
            let own = address(code, reads, ctor.path, &own);
            match registered.iter().all(|r| *r == own) {
                true => own,
                false => return None,
            }
        }
        ([], [one]) => one.clone(),
        _ => return None,
    };
    let address = Value::New("Uri".into(), vec![Value::Str(parts)]);
    let set = Value::Object(vec![(BASE.into(), address)]);
    Some(Value::New(HTTP_CLIENT.into(), vec![set]))
}

/// `url` with `base` in front when `url` is relative to it, as `Uri` joins them (RFC 3986): a
/// path from the root (`/orders`) replaces the base's own path, and a relative one replaces the
/// base's last segment (`https://c/v1` and `orders` give `/orders`, `https://c/v1/` gives
/// `/v1/orders`).
fn based_on(base: Vec<Part>, url: Value) -> Value {
    // A base read from configuration (`Part::EnvOr`) joins on the literal it defaults to.
    let base = match &base[..] {
        [Part::EnvOr(var, default)] => {
            vec![Part::EnvOr(var.clone(), rebase(default.clone(), &url))]
        }
        _ => rebase(base, &url),
    };
    based(Some(&base), &url)
}

/// `base` cut where `url` starts from, as `Uri` joins them (`based_on`).
fn rebase(mut base: Vec<Part>, url: &Value) -> Vec<Part> {
    let first = match url {
        Value::Str(parts) => match parts.first() {
            Some(Part::Text(t)) => Some(t.as_str()),
            _ => None,
        },
        _ => None,
    };
    match (first, path_start(&base)) {
        (Some(t), Some((n, at))) if t.starts_with('/') => cut(&mut base, n, at),
        (Some(t), Some((n, at))) if !t.contains("://") => {
            // After the last `/` of the base's path.
            let last = (n..base.len()).rev().find_map(|m| match &base[m] {
                Part::Text(t) => {
                    let from = if m == n { at } else { 0 };
                    t[from..].rfind('/').map(|i| (m, from + i))
                }
                _ => None,
            });
            if let Some((m, i)) = last {
                cut(&mut base, m, i + 1);
            }
        }
        _ => {}
    }
    base
}

/// `base` kept up to byte `at` of its part `n`.
fn cut(base: &mut Vec<Part>, n: usize, at: usize) {
    if let Part::Text(t) = &mut base[n] {
        t.truncate(at);
    }
    base.truncate(n + 1);
}

/// The part and byte where the path of `base` begins: the first `/` after the host, which is
/// what follows a literal `://`, or the first text after a part that is not literal when the
/// scheme is not (an address read from configuration).
fn path_start(base: &[Part]) -> Option<(usize, usize)> {
    let (mut scheme, mut opaque) = (false, false);
    for (n, part) in base.iter().enumerate() {
        let Part::Text(t) = part else {
            opaque = true;
            continue;
        };
        let from = match (scheme, t.find("://")) {
            (true, _) => 0,
            (false, Some(at)) => at + 3,
            (false, None) if opaque => 0,
            (false, None) => continue,
        };
        scheme = true;
        if let Some(at) = t[from..].find('/') {
            return Some((n, from + at));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::csharp::CSharp;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    /// A consumer as `(contract, via, method)`, the method only for an unresolved one.
    type Sent = (Option<String>, Option<String>, Option<String>);

    const USINGS: &str = "using System;\nusing System.Net.Http;\nusing System.Net.Http.Json;\n\
                          using Microsoft.AspNetCore.Builder;\nusing Microsoft.Extensions.Configuration;\n";

    /// The consumers `code` has, with `configs` as the config files the index read.
    fn found(code: &Code, configs: &[(&str, &[u8])]) -> Vec<Found> {
        let indexed = Indexed {
            manifests: &[],
            tsconfigs: &[],
            exported: &[],
            files: &[],
            configs,
        };
        consumers(code, &indexed)
    }

    /// The consumers of the class body `body` in `A/C.cs`, with other files of the repository.
    fn sent_with(body: &str, others: &[(&str, &str)]) -> Vec<Sent> {
        sent_in(body, others, &[])
    }

    /// `sent_with`, with config files (`(path, bytes)`) beside the sources.
    fn sent_in(body: &str, others: &[(&str, &str)], configs: &[(&str, &[u8])]) -> Vec<Sent> {
        let src = format!("{USINGS}\nnamespace A;\npublic class C {{\n{body}\n}}\n");
        let mut parsed = vec![("A/C.cs", CSharp.parse("A/C.cs", src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, CSharp.parse(path, source.as_bytes()).unwrap()));
        }
        // The projects give each file its unit, as the index does.
        let join = {
            let raw: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
            CSharp.join(configs, &raw)
        };
        for (path, facts) in &mut parsed {
            join(path, facts);
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = CSharp.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &CSharp);
        let code = Code::new(&resolver, &files, &CSharp);
        let mut found: Vec<Sent> = (found(&code, configs).into_iter())
            .map(|f| (f.contract, f.via, f.method))
            .collect();
        found.sort();
        found
    }

    fn sent(body: &str) -> Vec<Sent> {
        sent_with(body, &[])
    }

    /// The contracts of the consumers of `body`.
    fn contracts(body: &str) -> Vec<String> {
        sent(body).into_iter().filter_map(|(c, ..)| c).collect()
    }

    fn to(contract: &str) -> Sent {
        (Some(contract.into()), None, None)
    }

    fn host(contract: &str, host: &str) -> Sent {
        (Some(contract.into()), Some(format!("host:{host}")), None)
    }

    /// An unresolved consumer of `method`.
    fn open(method: &str) -> Sent {
        (None, None, Some(method.into()))
    }

    const CLIENT: &str = "private readonly HttpClient _http;\n";

    #[test]
    fn each_http_client_method_sends_its_method() {
        let body = format!(
            "{CLIENT}async void F() {{\n\
             await _http.GetAsync(\"/a\");\n\
             await _http.PostAsync(\"/b\", null);\n\
             await _http.PutAsync(\"/c\", null);\n\
             await _http.PatchAsync(\"/d\", null);\n\
             await _http.DeleteAsync(\"/e\");\n\
             await _http.GetStringAsync(\"/f\");\n\
             await _http.GetStreamAsync(\"/g\");\n\
             await _http.GetByteArrayAsync(\"/h\");\n\
             await _http.GetFromJsonAsync<int>(\"/i\");\n\
             await _http.PostAsJsonAsync(\"/j\", 1);\n\
             await _http.PutAsJsonAsync(\"/k\", 1);\n\
             await this._http.GetAsync(\"/l\");\n\
             }}"
        );
        assert_eq!(
            contracts(&body),
            [
                "http::DELETE::/e",
                "http::GET::/a",
                "http::GET::/f",
                "http::GET::/g",
                "http::GET::/h",
                "http::GET::/i",
                "http::GET::/l",
                "http::PATCH::/d",
                "http::POST::/b",
                "http::POST::/j",
                "http::PUT::/c",
                "http::PUT::/k",
            ]
        );
    }

    #[test]
    fn send_async_names_its_method_and_url_in_the_request_message() {
        let body = format!(
            "{CLIENT}async void F() {{\n\
             await _http.SendAsync(new HttpRequestMessage(HttpMethod.Post, \"/a\"));\n\
             await _http.SendAsync(new HttpRequestMessage(HttpMethod.Delete, new Uri(\"https://h.example.com/b\")));\n\
             var req = new HttpRequestMessage(HttpMethod.Put, \"/c\");\n\
             await _http.SendAsync(req);\n\
             await _http.SendAsync(Build());\n\
             }}"
        );
        assert_eq!(
            sent(&body),
            [
                (None, None, None),
                host("http::DELETE::/b", "h.example.com"),
                to("http::POST::/a"),
                to("http::PUT::/c"),
            ]
        );
    }

    #[test]
    fn a_base_address_joins_in_front_of_a_relative_url() {
        let body = "static readonly HttpClient Shared = new HttpClient { BaseAddress = new Uri(\"https://s.example.com/api/\") };\n\
             private readonly HttpClient _field = new HttpClient { BaseAddress = new Uri(\"https://f.example.com\") };\n\
             private readonly HttpClient _set;\n\
             C() { _set = new HttpClient(); _set.BaseAddress = new Uri(\"https://c.example.com/v1/\"); }\n\
             async void F() {\n\
             await Shared.GetAsync(\"orders\");\n\
             await Shared.GetAsync(\"/root\");\n\
             await _field.GetAsync(\"/orders\");\n\
             await _set.GetAsync(\"orders/1\");\n\
             var local = new HttpClient { BaseAddress = new Uri(\"https://l.example.com\") };\n\
             await local.GetAsync(\"/l\");\n\
             HttpClient typed = new() { BaseAddress = new Uri(\"https://t.example.com\") };\n\
             await typed.GetAsync(\"/t\");\n\
             var assigned = new HttpClient();\n\
             assigned.BaseAddress = new Uri(\"https://a.example.com\");\n\
             await assigned.GetAsync(\"/a\");\n\
             await new HttpClient().GetAsync(\"https://x.example.com/n\");\n\
             }";
        assert_eq!(
            sent(body),
            [
                host("http::GET::/a", "a.example.com"),
                host("http::GET::/api/orders", "s.example.com"),
                host("http::GET::/l", "l.example.com"),
                host("http::GET::/n", "x.example.com"),
                host("http::GET::/orders", "f.example.com"),
                host("http::GET::/root", "s.example.com"),
                host("http::GET::/t", "t.example.com"),
                host("http::GET::/v1/orders/1", "c.example.com"),
            ]
        );
    }

    #[test]
    fn urls_fold_interpolation_verbatim_raw_strings_plus_and_constants() {
        let other = "namespace A.Config;\npublic static class Hosts { public const string Orders = \"https://o.example.com\"; public static readonly string Api = Orders + \"/api\"; }\n";
        let body = format!(
            "{CLIENT}const string Base = \"https://b.example.com\";\n\
             static readonly string Path = Base + \"/p/{{x}}\";\n\
             async void F(int id) {{\n\
             await _http.GetAsync($\"{{Base}}/orders/{{id}}\");\n\
             await _http.GetAsync(Path);\n\
             await _http.GetAsync(A.Config.Hosts.Api + \"/users\");\n\
             await _http.GetAsync(@\"https://v.example.com/a\\b\");\n\
             await _http.GetAsync($@\"https://v.example.com/{{id}}/\"\"q\"\"\");\n\
             await _http.GetAsync(\"\"\"https://r.example.com/raw\"\"\");\n\
             await _http.GetAsync($\"https://i.example.com/{{{{lit}}}}/{{id:D2}}\");\n\
             await _http.GetAsync($$\"\"\"https://r.example.com/{{id}}/{{y}}\"\"\");\n\
             }}"
        );
        assert_eq!(
            sent_with(&body, &[("A/Config/Hosts.cs", other)]),
            [
                host("http::GET::/a\\b", "v.example.com"),
                host("http::GET::/api/users", "o.example.com"),
                host("http::GET::/orders/{}", "b.example.com"),
                host("http::GET::/p/{}", "b.example.com"),
                host("http::GET::/raw", "r.example.com"),
                host("http::GET::/{}/\"q\"", "v.example.com"),
                host("http::GET::/{}/{}", "i.example.com"),
                host("http::GET::/{}/{}", "r.example.com"),
            ]
        );
    }

    #[test]
    fn a_call_whose_url_cannot_be_evaluated_is_an_unresolved_consumer_of_its_method() {
        let body = format!(
            "{CLIENT}string Build() => \"\";\n\
             async void F() {{\n\
             await _http.GetAsync(Build());\n\
             await _http.PostAsJsonAsync(Build(), 1);\n\
             }}"
        );
        assert_eq!(sent(&body), [open("GET"), open("POST")]);
    }

    #[test]
    fn a_wrapper_makes_each_call_site_a_consumer_through_any_level() {
        let body = format!(
            "{CLIENT}Task<string> Inner(string p) => _http.GetStringAsync($\"https://w.example.com/{{p}}\");\n\
             Task<string> Outer(string id) => Inner(\"orders/\" + id);\n\
             Task<string> Twice(string id) => Outer(id + \"/lines\");\n\
             void Use(string unknown) {{ Outer(\"1\"); Twice(unknown); Twice(\"2\"); }}\n"
        );
        assert_eq!(
            contracts(&body),
            [
                "http::GET::/orders/1",
                "http::GET::/orders/2/lines",
                "http::GET::/orders/{}/lines",
            ]
        );
    }

    #[test]
    fn a_nameof_path_part_is_a_known_segment() {
        let body = format!(
            "{CLIENT}Task<string> Orders() => _http.GetStringAsync(\"https://w.example.com/api/\" + nameof(Orders));\n"
        );
        assert_eq!(contracts(&body), ["http::GET::/api/Orders"]);
    }

    #[test]
    fn a_field_set_in_the_constructor_is_still_the_client_of_a_wrapper() {
        let body = "private readonly HttpClient _http;\n\
            public C(HttpClient http) { this._http = http; }\n\
            private Task<string> Fetch(string path) => _http.GetStringAsync($\"https://w.example.com{path}\");\n\
            public async Task Run(string id) { await Fetch(\"/direct\"); }\n";
        assert_eq!(contracts(body), ["http::GET::/direct"]);
    }

    #[test]
    fn a_wrapper_that_calls_itself_is_read_once() {
        let body = format!(
            "{CLIENT}Task<string> Walk(string p, int n) {{ if (n > 0) Walk(p, n - 1); return _http.GetStringAsync(p); }}\n\
             void Use() {{ Walk(\"https://c.example.com/x\", 2); }}\n"
        );
        assert_eq!(contracts(&body), ["http::GET::/x"]);
    }

    #[test]
    fn a_wrapper_takes_the_method_the_request_and_the_client_from_its_callers() {
        let body = format!(
            "{CLIENT}Task<HttpResponseMessage> Send(HttpMethod m, string u) => _http.SendAsync(new HttpRequestMessage(m, u));\n\
             Task<string> Get(HttpClient c, string p) => c.GetStringAsync(p);\n\
             Task<HttpResponseMessage> Raw(HttpRequestMessage r) => _http.SendAsync(r);\n\
             void Use() {{\n\
             Send(HttpMethod.Patch, \"https://s.example.com/p\");\n\
             Get(new HttpClient {{ BaseAddress = new Uri(\"https://g.example.com/api\") }}, \"/x\");\n\
             Raw(new HttpRequestMessage(HttpMethod.Head, \"https://r.example.com/h\"));\n\
             }}\n"
        );
        assert_eq!(
            sent(&body),
            [
                host("http::GET::/x", "g.example.com"),
                host("http::HEAD::/h", "r.example.com"),
                host("http::PATCH::/p", "s.example.com"),
            ]
        );
    }

    #[test]
    fn only_an_http_client_by_its_type_sends() {
        let other =
            "namespace A.Other;\npublic class Fake { public Task GetAsync(string u) => null; }\n";
        let hidden = "namespace A.Mine;\npublic class HttpClient { public Task GetAsync(string u) => null; }\n\
                      public class User { HttpClient c; void F() { c.GetAsync(\"/mine\"); } }\n";
        let body = format!(
            "{CLIENT}A.Other.Fake _fake;\n\
             IHttpClientFactory _factory;\n\
             async void F() {{\n\
             await _fake.GetAsync(\"/fake\");\n\
             await _factory.CreateClient(\"orders\").GetAsync(\"/chained\");\n\
             HttpClient named = _factory.CreateClient(\"orders\");\n\
             await named.GetAsync(\"/named\");\n\
             var made = _factory.CreateClient();\n\
             await made.GetAsync(\"/made\");\n\
             await _http.GetAsync(\"/kept\");\n\
             }}"
        );
        assert_eq!(
            sent_with(
                &body,
                &[("A/Other/Fake.cs", other), ("A/Mine/User.cs", hidden)]
            ),
            [to("http::GET::/kept")]
        );
    }

    /// A type written in full is the library's unless the repository declares that very name:
    /// the compiler prefers the source's type. Another type under the library's root hides
    /// nothing.
    #[test]
    fn a_type_written_in_full_is_the_library_s_unless_the_repository_declares_it() {
        let body = "System.Net.Http.HttpClient _full;\n\
                    async void F() { await _full.GetAsync(\"https://x.example.com/full\"); }";
        let own = "namespace System.Net.Http;\n\
                   public class HttpClient { public Task GetAsync(string u) => null; }\n";
        let other = "namespace System.Acme;\npublic class Tool { }\n";
        assert_eq!(sent_with(body, &[("S/Http.cs", own)]), []);
        assert_eq!(
            sent_with(body, &[("S/Tool.cs", other)]),
            [host("http::GET::/full", "x.example.com")]
        );
    }

    #[test]
    fn the_type_is_read_through_an_alias_using() {
        let body = "Net.HttpClient _alias;\n\
             void F() { _alias.GetAsync(\"/alias\"); }";
        let src =
            format!("using Net = System.Net.Http;\nnamespace A;\npublic class C {{\n{body}\n}}\n");
        let parsed = [("A/C.cs", CSharp.parse("A/C.cs", src.as_bytes()).unwrap())];
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = CSharp.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &CSharp);
        let code = Code::new(&resolver, &files, &CSharp);
        let mut found: Vec<_> = self::found(&code, &[])
            .into_iter()
            .filter_map(|f| f.contract)
            .collect();
        found.sort();
        assert_eq!(found, ["http::GET::/alias"]);
    }

    /// A client with a base, in C# source.
    fn client(host: &str) -> String {
        format!("new HttpClient {{ BaseAddress = new Uri(\"https://{host}.example.com\") }}")
    }

    #[test]
    fn a_reassigned_request_is_no_constant() {
        let body = format!(
            "{CLIENT}async Task F() {{\n\
             var req = new HttpRequestMessage(HttpMethod.Get, \"https://h.example.com/a\");\n\
             req = new HttpRequestMessage(HttpMethod.Post, \"https://h.example.com/b\");\n\
             await _http.SendAsync(req);\n}}"
        );
        assert_eq!(contracts(&body), Vec::<String>::new());
        assert_eq!(sent(&body).len(), 1);
    }

    #[test]
    fn a_reassigned_client_has_an_unknown_base() {
        let body = format!(
            "async Task F() {{\n\
             var c = {};\n\
             c = {};\n\
             await c.GetAsync(\"/x\");\n}}",
            client("a"),
            client("b")
        );
        assert_eq!(sent(&body), [to("http::GET::/x")]);
    }

    #[test]
    fn a_field_assigned_after_its_initializer_has_an_unknown_base() {
        let field = format!("private readonly HttpClient _c = {};\n", client("default"));
        for assign in [
            "public C(HttpClient c) { _c = c; }",
            "public C(HttpClient c) { this._c = c; }",
            "void Swap(HttpClient c) { _c = c; }",
        ] {
            let body = format!("{field}{assign}\nasync Task F() {{ await _c.GetAsync(\"/x\"); }}");
            assert_eq!(sent(&body), [to("http::GET::/x")], "{assign}");
        }
    }

    #[test]
    fn a_local_assigned_by_out_ref_or_compound_is_no_constant() {
        let body = format!(
            "{CLIENT}static void Fill(out string s) {{ s = \"/b\"; }}\n\
             async Task F() {{\n\
             string u = \"https://h.example.com/a\";\n\
             Fill(out u);\n\
             await _http.GetAsync(u);\n\
             string v = \"https://h.example.com/v\";\n\
             v += \"/w\";\n\
             await _http.GetAsync(v);\n}}"
        );
        assert_eq!(contracts(&body), Vec::<String>::new());
        assert_eq!(sent(&body).len(), 2);
    }

    #[test]
    fn two_base_address_assignments_are_an_unknown_base() {
        let field = format!(
            "private readonly HttpClient _h = {};\n\
             void Prod() {{ _h.BaseAddress = new Uri(\"https://prod.example.com\"); }}\n\
             void Stage() {{ _h.BaseAddress = new Uri(\"https://stage.example.com\"); }}\n\
             async Task F() {{ await _h.GetAsync(\"/x\"); }}",
            client("default")
        );
        assert_eq!(sent(&field), [to("http::GET::/x")]);
        let local = format!(
            "async Task F(bool b) {{\n\
             var c = {};\n\
             if (b) c.BaseAddress = new Uri(\"https://prod.example.com\");\n\
             else c.BaseAddress = new Uri(\"https://stage.example.com\");\n\
             await c.GetAsync(\"/x\");\n}}",
            client("default")
        );
        assert_eq!(sent(&local), [to("http::GET::/x")]);
    }

    #[test]
    fn a_base_assigned_through_a_field_is_not_the_base_of_a_parameter_that_hides_it() {
        let body = "private readonly HttpClient http;\n\
             public C() { this.http.BaseAddress = new Uri(\"https://field.example.com\"); }\n\
             async Task F(HttpClient http) { await http.GetAsync(\"/x\"); }\n\
             void Use() { F(new HttpClient { BaseAddress = new Uri(\"https://arg.example.com\") }); }";
        assert_eq!(sent(body), [host("http::GET::/x", "arg.example.com")]);
        let alone = body.replace("void Use() { F(", "void Use() { var _ = (");
        assert_eq!(sent(&alone), [to("http::GET::/x")]);
    }

    #[test]
    fn a_nested_class_with_its_own_field_keeps_its_own_base() {
        let body = "private readonly HttpClient _http;\n\
             public C() { _http.BaseAddress = new Uri(\"https://outer.example.com\"); }\n\
             async Task F() { await _http.GetAsync(\"/x\"); }\n\
             public class Inner {\n\
             private readonly HttpClient _http;\n\
             async Task G() { await _http.GetAsync(\"/y\"); }\n}";
        assert_eq!(
            sent(body),
            [
                host("http::GET::/x", "outer.example.com"),
                to("http::GET::/y")
            ]
        );
    }

    #[test]
    fn a_relative_url_merges_with_the_base_path_as_rfc_3986_does() {
        let body = "async Task F() {\n\
             var a = new HttpClient { BaseAddress = new Uri(\"https://c.example.com/v1\") };\n\
             var b = new HttpClient { BaseAddress = new Uri(\"https://c.example.com/v1/\") };\n\
             var d = new HttpClient { BaseAddress = new Uri(\"https://c.example.com\") };\n\
             var e = new HttpClient { BaseAddress = new Uri(\"https://c.example.com/v1/x\") };\n\
             await a.GetAsync(\"orders\");\n\
             await b.GetAsync(\"orders\");\n\
             await d.GetAsync(\"orders\");\n\
             await e.GetAsync(\"orders\");\n}";
        let mut paths: Vec<_> = contracts(body);
        paths.sort();
        assert_eq!(
            paths,
            [
                "http::GET::/orders",
                "http::GET::/orders",
                "http::GET::/v1/orders",
                "http::GET::/v1/orders"
            ]
        );
    }

    #[test]
    fn a_base_with_no_literal_scheme_is_cut_at_the_first_path_for_a_rooted_url() {
        let body = "static string Cfg() => \"x\";\n\
             async Task F() {\n\
             var c = new HttpClient { BaseAddress = new Uri(Cfg() + \"/api/\") };\n\
             await c.GetAsync(\"/x\");\n}";
        assert_eq!(contracts(body), ["http::GET::/x"]);
    }

    #[test]
    fn a_named_client_held_in_a_field_is_not_read() {
        let body = "private readonly HttpClient _n;\n\
             public C(IHttpClientFactory f) { _n = f.CreateClient(\"orders\"); }\n\
             async Task F() { await _n.GetAsync(\"/named\"); }\n\
             private HttpClient _m;\n\
             public void Set(IHttpClientFactory f) { this._m = f.CreateClient(); }\n\
             async Task G() { await _m.GetAsync(\"/named2\"); }";
        assert_eq!(sent(body), Vec::<Sent>::new());
    }

    #[test]
    fn a_fully_qualified_http_client_counts() {
        let body = "private readonly System.Net.Http.HttpClient _q;\n\
             async Task F() {\n\
             await _q.GetAsync(\"https://q.example.com/q\");\n\
             await new System.Net.Http.HttpClient().GetAsync(\"https://q.example.com/n\");\n}";
        assert_eq!(
            sent(body),
            [
                host("http::GET::/n", "q.example.com"),
                host("http::GET::/q", "q.example.com")
            ]
        );
    }

    #[test]
    fn a_method_set_after_the_request_is_built_is_read() {
        let body = format!(
            "{CLIENT}async Task F() {{\n\
             var req = new HttpRequestMessage(HttpMethod.Get, \"https://r.example.com/p\");\n\
             req.Method = HttpMethod.Post;\n\
             await _http.SendAsync(req);\n}}"
        );
        assert_eq!(contracts(&body), ["http::POST::/p"]);
    }

    #[test]
    fn a_local_url_is_followed_once_and_a_reassigned_one_is_not() {
        let body = format!(
            "{CLIENT}async Task F(int id) {{\n\
             var a = $\"https://l.example.com/a/{{id}}\";\n\
             string b = \"https://l.example.com/b\";\n\
             const string k = \"https://l.example.com/k\";\n\
             await _http.GetAsync(a);\n\
             await _http.GetAsync(b);\n\
             await _http.GetAsync(k);\n}}\n\
             async Task G() {{\n\
             string u = \"https://l.example.com/u\";\n\
             u += \"/more\";\n\
             await _http.GetAsync(u);\n}}"
        );
        assert_eq!(
            sent(&body),
            [
                open("GET"),
                host("http::GET::/a/{}", "l.example.com"),
                host("http::GET::/b", "l.example.com"),
                host("http::GET::/k", "l.example.com"),
            ]
        );
    }

    #[test]
    fn a_null_conditional_call_is_a_consumer() {
        let body = "async Task F() { await _http?.GetAsync(\"https://n.example.com/n\"); }";
        assert_eq!(contracts(&format!("{CLIENT}{body}")), ["http::GET::/n"]);
    }

    #[test]
    fn a_property_initializer_gives_the_base_of_the_client_it_holds() {
        let body = format!(
            "public HttpClient Client {{ get; }} = {};\n\
             async Task F() {{ await Client.GetAsync(\"/p\"); }}",
            client("prop")
        );
        assert_eq!(sent(&body), [host("http::GET::/p", "prop.example.com")]);
    }

    #[test]
    fn a_get_async_of_the_repository_beside_the_client_call_is_not_called_by_it() {
        // The pack's call, not the kind's: the client call must stay a call of the outside type.
        let body = format!(
            "{CLIENT}async Task F() {{ await _http.GetAsync(\"https://z.example.com/z\"); }}"
        );
        let other = "namespace A;\npublic class Repo { public Task GetAsync(string s) => null; }\n";
        assert_eq!(
            sent_with(&body, &[("A/Repo.cs", other)]),
            [host("http::GET::/z", "z.example.com")]
        );
    }

    /// An unresolved consumer's `via` and a resolved one's, with the twin readings of a
    /// configuration default (`http::fetches`) as the kind gives them.
    fn env(contract: &str, var: &str) -> Sent {
        (Some(contract.into()), Some(format!("env:{var}")), None)
    }

    fn twins(contract: &str, var: &str, host: &str) -> [Sent; 2] {
        [
            (
                Some(contract.into()),
                Some(format!("env:{var}|host:{host}")),
                None,
            ),
            (
                Some(contract.into()),
                Some(format!("host:{host}|default")),
                None,
            ),
        ]
    }

    const GET_ENV: &str = "Environment.GetEnvironmentVariable";

    #[test]
    fn an_environment_read_is_the_base_variable_through_fallbacks_trim_and_forms() {
        let urls = [
            format!("{GET_ENV}(\"X\") + \"/a\""),
            format!("$\"{{{GET_ENV}(\"X\")}}/a\""),
            format!("({GET_ENV}(\"X\") ?? \"http://localhost\") + \"/a\""),
            format!("({GET_ENV}(\"X\") ?? throw new Exception()) + \"/a\""),
            format!("{GET_ENV}(\"X\")!.TrimEnd('/') + \"/a\""),
            format!("({GET_ENV}(\"X\") ?? \"d\").TrimEnd('/').Trim() + \"/a\""),
            format!("System.{GET_ENV}(\"X\") + \"/a\""),
            // A fallback chain with one read, whatever else the fallbacks are.
            format!("({GET_ENV}(\"X\") ?? Other.Get(\"Y\")) + \"/a\""),
            format!("({GET_ENV}(\"X\") ?? \"d\" ?? \"e\") + \"/a\""),
            format!("(({GET_ENV}(\"X\") ?? \"d\") ?? \"e\") + \"/a\""),
        ];
        for url in urls {
            let body = format!("{CLIENT}async Task F() {{ await _http.GetAsync({url}); }}");
            assert_eq!(sent(&body), [env("http::GET::/a", "X")], "{url}");
        }
    }

    #[test]
    fn an_environment_read_is_one_read_and_the_framework_s() {
        let not = [
            // Two reads.
            format!("({GET_ENV}(\"X\") ?? {GET_ENV}(\"Y\")) + \"/a\""),
            // Three operands: the reads count across the whole chain.
            format!("({GET_ENV}(\"X\") ?? \"d\" ?? {GET_ENV}(\"Y\")) + \"/a\""),
            format!("({GET_ENV}(\"X\") ?? {GET_ENV}(\"Y\") ?? \"d\") + \"/a\""),
            format!("(({GET_ENV}(\"X\") ?? \"d\") ?? {GET_ENV}(\"Y\")) + \"/a\""),
            format!("({GET_ENV}(\"X\") ?? {GET_ENV}(\"Y\") ?? {GET_ENV}(\"Z\")) + \"/a\""),
            // Not a literal name, or another call with a literal.
            format!("{GET_ENV}(name) + \"/a\""),
            format!("{GET_ENV}(\"X\", Machine) + \"/a\""),
            "Settings.Get(\"X\") + \"/a\"".to_string(),
            format!("{GET_ENV}(\"X\").ToLower() + \"/a\""),
        ];
        for url in not {
            let body =
                format!("{CLIENT}async Task F(string name) {{ await _http.GetAsync({url}); }}");
            assert_eq!(contracts(&body), ["http::GET::/a"], "{url}");
            assert_eq!(
                sent(&body),
                [(Some("http::GET::/a".into()), None, None)],
                "{url}"
            );
        }
        // A type of the repository named `Environment` hides the framework's.
        let hiding = "namespace A;\npublic static class Environment { \
                      public static string GetEnvironmentVariable(string s) => s; }\n";
        let body = format!(
            "{CLIENT}async Task F() {{ await _http.GetAsync({GET_ENV}(\"X\") + \"/a\"); }}"
        );
        assert_eq!(
            sent_with(&body, &[("A/Environment.cs", hiding)]),
            [(Some("http::GET::/a".into()), None, None)]
        );
    }

    #[test]
    fn a_local_or_a_field_bound_to_an_environment_read_is_the_read() {
        let get = format!("{GET_ENV}(\"X\")");
        for (decl, url) in [
            (format!("var web = {get};"), "web + \"/a\""),
            (format!("var web = {get};"), "$\"{web}/a\""),
            (format!("string web = {get}!;"), "web + \"/a\""),
            (format!("var web = {get} ?? \"d\";"), "web + \"/a\""),
            (format!("var web = ({get}).TrimEnd('/');"), "web + \"/a\""),
            (String::new(), "Web + \"/a\""),
        ] {
            let field = format!("private static readonly string Web = {get};\n");
            let body =
                format!("{field}{CLIENT}async Task F() {{ {decl} await _http.GetAsync({url}); }}");
            assert_eq!(sent(&body), [env("http::GET::/a", "X")], "{decl} {url}");
        }
        // A local bound to a call with a literal that is no read stays unknown.
        let body = format!(
            "{CLIENT}async Task F() {{ var web = Other.Get(\"X\"); await _http.GetAsync(web + \"/a\"); }}"
        );
        assert_eq!(sent(&body), [(Some("http::GET::/a".into()), None, None)]);
    }

    #[test]
    fn a_client_base_from_the_environment_joins_its_relative_urls() {
        let body = format!(
            "public async Task F() {{\n\
             var home = {GET_ENV}(\"HOME_URL\") + \"/v1/\";\n\
             var http = new HttpClient {{ BaseAddress = new Uri(home) }};\n\
             await http.GetAsync(\"orders\");\n\
             await http.GetAsync(\"/root\");\n\
             var plain = new HttpClient {{ BaseAddress = new Uri({GET_ENV}(\"API_URL\")!) }};\n\
             await plain.GetAsync(\"/plain\");\n}}"
        );
        assert_eq!(
            sent(&body),
            [
                env("http::GET::/plain", "API_URL"),
                env("http::GET::/root", "HOME_URL"),
                env("http::GET::/v1/orders", "HOME_URL"),
            ]
        );
    }

    const SETTINGS: &[u8] = br#"{
        // A comment, and a trailing comma below.
        "Api": { "Url": "https://api.example.com/v1/", "Local": "http://localhost:5000", },
        "audit": "https://audit.example.com",
        "Port": 8080, "Empty": ""
    }"#;

    fn configured(body: &str) -> Vec<Sent> {
        let body = format!("private readonly IConfiguration _config;\n{CLIENT}{body}");
        sent_in(&body, &[], &[("appsettings.json", SETTINGS)])
    }

    #[test]
    fn a_configuration_key_with_a_literal_is_its_variable_with_the_literal_as_default() {
        let body = "async Task F() {\n\
             await _http.GetAsync(_config[\"Api:Url\"] + \"orders\");\n\
             await _http.GetAsync(_config[\"AUDIT\"] + \"/audit\");\n\
             await _http.GetAsync(_config[\"Api:Local\"] + \"/local\");\n}";
        let mut expected = Vec::new();
        // `orders` has no slash after the variable: that reading names no path.
        expected.push((
            None,
            Some("env:Api__Url|host:api.example.com".into()),
            Some("GET".into()),
        ));
        expected.extend(
            twins("http::GET::/v1/orders", "Api__Url", "api.example.com")
                .into_iter()
                .skip(1),
        );
        expected.extend(twins("http::GET::/audit", "AUDIT", "audit.example.com"));
        // `localhost:5000` is a host too: both readings stay until a workspace serves it.
        expected.extend(twins("http::GET::/local", "Api__Local", "localhost:5000"));
        expected.sort();
        let mut found = configured(body);
        found.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn a_key_the_file_lacks_or_holds_as_no_string_is_the_variable_alone() {
        let body = "async Task F() {\n\
             await _http.GetAsync(_config[\"Missing:Key\"] + \"/m\");\n\
             await _http.GetAsync(_config[\"Port\"] + \"/p\");\n\
             await _http.GetAsync(_config[\"Empty\"] + \"/e\");\n\
             await _http.GetAsync(_config[\"Api\"] + \"/o\");\n}";
        assert_eq!(
            configured(body),
            [
                env("http::GET::/e", "Empty"),
                env("http::GET::/m", "Missing__Key"),
                env("http::GET::/o", "Api"),
                env("http::GET::/p", "Port"),
            ]
        );
    }

    #[test]
    fn a_configuration_receiver_is_declared_so_or_a_web_application_builder_s() {
        let src = |receiver: &str, decl: &str| {
            let body = format!(
                "{decl}\nasync Task F(HttpClient h) {{ await h.GetAsync({receiver}[\"audit\"] + \"/x\"); }}"
            );
            sent_in(&body, &[], &[("appsettings.json", SETTINGS)])
        };
        let twin = twins("http::GET::/x", "audit", "audit.example.com").to_vec();
        assert_eq!(src("_c", "IConfiguration _c;"), twin);
        assert_eq!(src("this._c", "IConfiguration _c;"), twin);
        assert_eq!(
            src(
                "_c",
                "Microsoft.Extensions.Configuration.IConfiguration _c;"
            ),
            twin
        );
        assert_eq!(src("_c.Configuration", "WebApplicationBuilder _c;"), twin);
        let made = |factory: &str| {
            let body = format!(
                "async Task F(HttpClient h) {{ var builder = {factory}(); \
                 await h.GetAsync(builder.Configuration[\"audit\"] + \"/x\"); }}"
            );
            sent_in(&body, &[], &[("appsettings.json", SETTINGS)])
        };
        assert_eq!(made("WebApplication.CreateSlimBuilder"), twin);
        assert_eq!(made("WebApplication.CreateBuilder"), twin);
        assert_eq!(
            made("Other.CreateBuilder"),
            [(Some("http::GET::/x".into()), None, None)]
        );
        // Another type, an undeclared name, a dictionary, a section and a method result.
        for (receiver, decl) in [
            ("_c", "Dictionary<string, string> _c;"),
            ("_c", "IConfigurationSection _c;"),
            ("_c", "IConfigurationRoot _c;"),
            ("_c", ""),
            ("_c.Settings", "IConfiguration _c;"),
            ("_c.GetSection(\"Api\")", "IConfiguration _c;"),
            ("builder.Environment", "WebApplicationBuilder builder;"),
        ] {
            let found = src(receiver, decl);
            assert_eq!(
                found,
                [(Some("http::GET::/x".into()), None, None)],
                "{receiver} {decl}"
            );
        }
    }

    #[test]
    fn a_configuration_type_of_the_repository_is_no_configuration() {
        let body = "private readonly IConfiguration _config;\n\
                    async Task F(HttpClient h) { await h.GetAsync(_config[\"audit\"] + \"/x\"); }";
        let own =
            "namespace A;\npublic interface IConfiguration { string this[string k] { get; } }\n";
        assert_eq!(
            sent_in(
                body,
                &[("A/IConfiguration.cs", own)],
                &[("appsettings.json", SETTINGS)]
            ),
            [(Some("http::GET::/x".into()), None, None)]
        );
    }

    #[test]
    fn only_the_base_file_of_the_project_is_read_and_get_value_and_sections_are_not() {
        let dev: &[u8] =
            br#"{"Api": {"Url": "https://dev.example.com/"}, "audit": "https://dev.example.com"}"#;
        let body = "async Task F() {\n\
             await _http.GetAsync(_config[\"audit\"] + \"/x\");\n\
             await _http.GetAsync(_config.GetValue<string>(\"audit\") + \"/v\");\n\
             await _http.GetAsync(_config.GetSection(\"Api\")[\"Url\"] + \"/s\");\n}";
        let body = format!("private readonly IConfiguration _config;\n{CLIENT}{body}");
        let found = sent_in(
            &body,
            &[],
            &[
                ("appsettings.Development.json", dev),
                ("appsettings.json", SETTINGS),
            ],
        );
        let mut expected = twins("http::GET::/x", "audit", "audit.example.com").to_vec();
        expected.extend([
            (Some("http::GET::/s".into()), None, None),
            (Some("http::GET::/v".into()), None, None),
        ]);
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn a_client_base_from_configuration_joins_on_the_literal_it_defaults_to() {
        let body = "public async Task F() {\n\
             var rel = new HttpClient { BaseAddress = new Uri(_config[\"Api:Url\"]!) };\n\
             await rel.GetAsync(\"orders\");\n\
             await rel.GetAsync(\"/root\");\n}";
        // The variable's reading keeps the relative URL beside the base (`/orders`); the
        // default's joins on the literal (`Uri`: `/v1/` + `orders`, a root-relative URL replaces the path).
        let mut expected = Vec::new();
        expected.extend(twins("http::GET::/root", "Api__Url", "api.example.com"));
        expected.push((
            Some("http::GET::/orders".into()),
            Some("env:Api__Url|host:api.example.com".into()),
            None,
        ));
        expected.push((
            Some("http::GET::/v1/orders".into()),
            Some("host:api.example.com|default".into()),
            None,
        ));
        expected.sort();
        assert_eq!(configured(body), expected);
    }

    #[test]
    fn a_read_in_a_constructor_reaches_the_base_assigned_there() {
        let body = "private readonly HttpClient _h = new HttpClient();\n\
             public C(IConfiguration config) { _h.BaseAddress = new Uri(config[\"Missing:Url\"]!); }\n\
             public async Task F() { await _h.GetAsync(\"/x\"); }";
        assert_eq!(
            sent_in(body, &[], &[("appsettings.json", SETTINGS)]),
            [env("http::GET::/x", "Missing__Url")]
        );
    }

    const SDK: &[u8] = br#"<Project Sdk="Microsoft.NET.Sdk"></Project>"#;
    const OLD: &[u8] = b"<Project ToolsVersion=\"15.0\" \
                         xmlns=\"http://schemas.microsoft.com/developer/msbuild/2003\"></Project>";

    #[test]
    fn a_file_reads_the_configuration_of_the_project_that_holds_it() {
        let body = format!(
            "private readonly IConfiguration _config;\n{CLIENT}\
             async Task F() {{ await _http.GetAsync(_config[\"audit\"] + \"/x\"); }}"
        );
        let twin = twins("http::GET::/x", "audit", "audit.example.com").to_vec();
        // The file of another project (`B/`) does not configure `A/C.cs` once a project holds it.
        let found = sent_in(
            &body,
            &[],
            &[("A/A.csproj", SDK), ("B/appsettings.json", SETTINGS)],
        );
        assert_eq!(found, [env("http::GET::/x", "audit")]);
        // A project file the pack does not read (old-style) is no project: the SDK one above holds
        // the file, as the pack's `join` says.
        let configs = [
            ("Root.csproj", SDK),
            ("A/Old.csproj", OLD),
            ("appsettings.json", SETTINGS),
        ];
        assert_eq!(sent_in(&body, &[], &configs), twin);
        // A file in no project reads the nearest file above it.
        assert_eq!(sent_in(&body, &[], &[("appsettings.json", SETTINGS)]), twin);
    }

    /// A typed client `C`: its constructor takes the `HttpClient`, a field holds it.
    const TYPED: &str = "private readonly HttpClient _http;\n\
                         public C(HttpClient http) { _http = http; }\n\
                         public async Task F() { await _http.GetAsync(\"/a\"); await this._http.GetAsync(\"b\"); }";

    /// The consumers of `TYPED`, with `registered` written in a method of another file.
    fn typed(registered: &str) -> Vec<Sent> {
        let startup = format!(
            "{USINGS}using Microsoft.Extensions.DependencyInjection;\nnamespace A;\n\
             public static class Startup {{\n\
             public static void Add(IServiceCollection services, IConfiguration config) {{\n{registered}\n}}\n}}\n"
        );
        sent_in(
            TYPED,
            &[("A/Startup.cs", &startup)],
            &[("appsettings.json", SETTINGS)],
        )
    }

    #[test]
    fn a_typed_client_takes_the_base_its_registration_gives() {
        assert_eq!(
            typed(
                "services.AddHttpClient<C>(c => c.BaseAddress = new Uri(\"https://typed.example.com/v1/\"));"
            ),
            [
                host("http::GET::/a", "typed.example.com"),
                host("http::GET::/v1/b", "typed.example.com")
            ]
        );
    }

    #[test]
    fn a_typed_client_takes_a_base_from_configuration_and_the_last_type_argument() {
        let read = "services.AddHttpClient<IApi, C>((sp, c) => { c.BaseAddress = new Uri(config[\"Api:Url\"]!); });";
        assert_eq!(
            typed(&format!("{read}\nservices.AddHttpClient<C>();")),
            [
                twins("http::GET::/a", "Api__Url", "api.example.com").to_vec(),
                // The variable's own value is unknown: a relative URL follows its root.
                vec![twins("http::GET::/b", "Api__Url", "api.example.com")[0].clone()],
                vec![twins("http::GET::/v1/b", "Api__Url", "api.example.com")[1].clone()],
            ]
            .concat()
        );
    }

    #[test]
    fn a_typed_client_registered_with_two_bases_or_none_has_an_unknown_base() {
        let one =
            "services.AddHttpClient<C>(c => c.BaseAddress = new Uri(\"https://one.example.com\"));";
        let two =
            "services.AddHttpClient<C>(c => c.BaseAddress = new Uri(\"https://two.example.com\"));";
        let unknown = [to("http::GET::/a"), to("http::GET::/b")];
        assert_eq!(typed(&format!("{one}\n{two}")), unknown);
        assert_eq!(typed(&format!("{one}\n{one}")).len(), 2);
        assert_eq!(
            typed(&format!("{one}\n{one}"))[0].1.as_deref(),
            Some("host:one.example.com")
        );
        // Registered as another class, and a base that does not fold.
        assert_eq!(
            typed(
                "services.AddHttpClient<Startup>(c => c.BaseAddress = new Uri(\"https://other.example.com\"));"
            ),
            unknown
        );
        assert_eq!(
            typed("services.AddHttpClient<C>(c => c.BaseAddress = new Uri(Url()));"),
            unknown
        );
    }

    #[test]
    fn a_base_the_constructor_sets_on_its_parameter_reaches_the_field_that_holds_it() {
        let body = "private readonly HttpClient _http;\n\
             public C(HttpClient http) { http.BaseAddress = new Uri(\"https://own.example.com/\"); _http = http; }\n\
             public async Task F() { await _http.GetAsync(\"/a\"); }";
        assert_eq!(sent(body), [host("http::GET::/a", "own.example.com")]);
    }

    #[test]
    fn a_client_of_a_method_or_of_a_called_constructor_is_not_injected() {
        let startup = format!(
            "{USINGS}using Microsoft.Extensions.DependencyInjection;\nnamespace A;\n\
             public static class Startup {{\n\
             public static void Add(IServiceCollection services) {{\n\
             services.AddHttpClient<C>(c => c.BaseAddress = new Uri(\"https://typed.example.com\"));\n}}\n\
             public static C Make() => new C(new HttpClient());\n}}\n"
        );
        let body = "private readonly HttpClient _http;\n\
             public C(HttpClient http) { _http = http; }\n\
             public async Task F() { await _http.GetAsync(\"/a\"); }\n\
             public async Task G(HttpClient h) { await h.GetAsync(\"/g\"); }";
        assert_eq!(
            sent_with(body, &[("A/Startup.cs", &startup)]),
            [to("http::GET::/a"), to("http::GET::/g")]
        );
    }

    /// `TYPED` with the constructor `ctor` in place of its own, registered with `registered`.
    fn typed_with(ctor: &str, registered: &str) -> Vec<Sent> {
        let body = TYPED.replace("public C(HttpClient http) { _http = http; }", ctor);
        let startup = format!(
            "{USINGS}using Microsoft.Extensions.DependencyInjection;\nnamespace A;\n\
             public static class Startup {{\n\
             public static void Add(IServiceCollection services) {{\n{registered}\n}}\n}}\n"
        );
        sent_with(&body, &[("A/Startup.cs", &startup)])
    }

    const REGISTERED: &str =
        "services.AddHttpClient<C>(c => c.BaseAddress = new Uri(\"https://typed.example.com/\"));";

    #[test]
    fn a_base_the_constructor_may_not_set_or_that_differs_from_the_registration_is_unknown() {
        let unknown = [to("http::GET::/a"), to("http::GET::/b")];
        for ctor in [
            "public C(HttpClient http) { if (http.BaseAddress == null) http.BaseAddress = new Uri(\"https://own.example.com/\"); _http = http; }",
            "public C(HttpClient http, bool f) { http.BaseAddress = f ? new Uri(\"https://own.example.com/\") : null; _http = http; }",
            "public C(HttpClient http) { http.BaseAddress ??= new Uri(\"https://own.example.com/\"); _http = http; }",
            "public C(HttpClient http, int n) { switch (n) { case 1: http.BaseAddress = new Uri(\"https://own.example.com/\"); break; } _http = http; }",
            "public C(HttpClient http) { http.BaseAddress = new Uri(\"https://own.example.com/\"); _http = http; }",
        ] {
            assert_eq!(typed_with(ctor, REGISTERED), unknown, "{ctor}");
        }
        // The same address as the registration's, or no registration, is the constructor's.
        let own = "public C(HttpClient http) { http.BaseAddress = new Uri(\"https://typed.example.com/\"); _http = http; }";
        assert_eq!(
            typed_with(own, REGISTERED),
            [
                host("http::GET::/a", "typed.example.com"),
                host("http::GET::/b", "typed.example.com")
            ]
        );
    }

    #[test]
    fn a_base_a_local_client_may_not_be_given_or_that_differs_from_its_initializer_is_unknown() {
        let body = "async Task F(bool f) {\n\
             var c = new HttpClient { BaseAddress = new Uri(\"https://a.example.com\") };\n\
             if (f) c.BaseAddress = new Uri(\"https://b.example.com\");\n\
             await c.GetAsync(\"/c\");\n\
             var d = new HttpClient { BaseAddress = new Uri(\"https://a.example.com\") };\n\
             d.BaseAddress = new Uri(\"https://b.example.com\");\n\
             await d.GetAsync(\"/d\");\n\
             var e = new HttpClient();\n\
             while (f) e.BaseAddress = new Uri(\"https://e.example.com\");\n\
             await e.GetAsync(\"/e\");\n}";
        assert_eq!(
            sent(body),
            [
                to("http::GET::/c"),
                to("http::GET::/d"),
                to("http::GET::/e")
            ]
        );
    }

    #[test]
    fn a_typed_client_the_code_builds_with_a_target_typed_new_is_not_injected() {
        let made = format!(
            "{REGISTERED}\nC made = new(new HttpClient {{ BaseAddress = new Uri(\"https://explicit.example.com/\") }});"
        );
        assert_eq!(
            typed_with("public C(HttpClient http) { _http = http; }", &made),
            [to("http::GET::/a"), to("http::GET::/b")]
        );
    }

    #[test]
    fn a_registration_meada_cannot_read_leaves_the_base_unknown() {
        let unknown = [to("http::GET::/a"), to("http::GET::/b")];
        let ctor = "public C(HttpClient http) { _http = http; }";
        let other =
            "services.AddHttpClient<C>(c => c.BaseAddress = new Uri(\"https://two.example.com\"));";
        // `ConfigureHttpClient` on the registration sets it again.
        let chained = "services.AddHttpClient<C>(c => c.BaseAddress = new Uri(\"https://one.example.com\"))\
                       .ConfigureHttpClient(c => c.BaseAddress = new Uri(\"https://two.example.com\"));";
        assert_eq!(typed_with(ctor, chained), unknown);
        // A method group, or a lambda that passes the client on, beside another registration.
        let group = "services.AddHttpClient<C>(Configure);";
        let passed = "services.AddHttpClient<C>(c => Configure(c));";
        for unread in [group, passed] {
            assert_eq!(
                typed_with(ctor, &format!("{unread}\n{other}")),
                unknown,
                "{unread}"
            );
            assert_eq!(
                typed_with(ctor, &format!("{other}\n{unread}")),
                unknown,
                "{unread}"
            );
        }
        // A base set on a local of the lambda is no base of its client.
        let local = "services.AddHttpClient<C>((sp, c) => { var o = new HttpClient(); \
                     o.BaseAddress = new Uri(\"https://other.example.com\"); c.Timeout = TimeSpan.FromSeconds(1); });";
        assert_eq!(typed_with(ctor, local), unknown);
        // A named registration, and one that only names the client, configure nothing else.
        let named = "services.AddHttpClient<C>(\"n\", c => c.BaseAddress = new Uri(\"https://two.example.com\"));\n\
                     services.AddHttpClient<C>();";
        assert_eq!(
            typed_with(ctor, named),
            [
                host("http::GET::/a", "two.example.com"),
                host("http::GET::/b", "two.example.com")
            ]
        );
    }

    #[test]
    fn a_field_the_constructor_assigns_through_a_null_check_holds_the_injected_client() {
        let ctor = "public C(HttpClient http) { _http = http ?? throw new ArgumentNullException(nameof(http)); }";
        assert_eq!(
            typed_with(ctor, REGISTERED),
            [
                host("http::GET::/a", "typed.example.com"),
                host("http::GET::/b", "typed.example.com")
            ]
        );
    }
}
