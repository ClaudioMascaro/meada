use super::{Found, Role, Site};
use crate::lang::{Part, Value, literal};
use crate::resolve::{Code, Expand, Frame};
use std::collections::{BTreeMap, BTreeSet, HashMap};

mod csharp;
mod go;
pub(crate) mod java;
pub(crate) mod mutations;
pub(crate) mod origins;

#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub(crate) enum Identity<T> {
    #[default]
    Unknown,
    Leaf(T),
    Object(Vec<(String, Identity<T>)>),
    List(Vec<Identity<T>>),
}

impl<T> Identity<T> {
    pub(crate) fn read(value: &Value, leaf: &impl Fn(&Value) -> Option<T>) -> Self {
        match value {
            Value::Object(fields) => Self::Object(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::read(value, leaf)))
                    .collect(),
            ),
            Value::List(values) => {
                Self::List(values.iter().map(|value| Self::read(value, leaf)).collect())
            }
            _ => leaf(value).map_or(Self::Unknown, Self::Leaf),
        }
    }
}

pub(crate) fn argument<'a>(
    code: &Code<'a>,
    path: &'a str,
    value: &Value,
) -> Option<crate::names::Argument> {
    use crate::names::Argument;
    match value {
        Value::Str(parts) => match parts.as_slice() {
            [
                Part::Read {
                    scope,
                    callee,
                    key,
                    parameter: false,
                    then,
                },
            ] if then.is_empty() => {
                let aliases = code.files()[path]
                    .imports
                    .iter()
                    .map(|i| (i.local.as_str(), i.local.as_str()))
                    .collect();
                Some(
                    if csharp::environment(code, path, scope, callee, &aliases) {
                        Argument::Environment(key.clone())
                    } else {
                        Argument::Unresolved
                    },
                )
            }
            _ => crate::names::argument(value),
        },
        Value::Call(call) if code.pack().extensions().contains(&"go") => {
            Some(go::environment(call).map_or(Argument::Unresolved, Argument::Environment))
        }
        _ => crate::names::argument(value),
    }
}

pub(crate) struct Client {
    pub java: &'static str,
    pub go: &'static str,
    pub csharp: &'static str,
}

pub(crate) struct Operation {
    pub method: &'static str,
    pub field: &'static str,
    pub role: Role,
    pub hierarchy: bool,
}

pub(crate) struct Service {
    pub client: Client,
    pub declared: &'static [&'static str],
    pub kind: &'static str,
    pub operations: &'static [Operation],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Language {
    Java,
    Go,
    Csharp,
}

impl Language {
    fn of(code: &Code) -> Option<Self> {
        let extensions = code.pack().extensions();
        if extensions.contains(&"java") {
            Some(Self::Java)
        } else if extensions.contains(&"go") {
            Some(Self::Go)
        } else if extensions.contains(&"cs") {
            Some(Self::Csharp)
        } else {
            None
        }
    }

    fn method(self, method: &str, canonical: &str) -> bool {
        match self {
            Self::Java => {
                method.len() == canonical.len()
                    && method.as_bytes().first()
                        == canonical
                            .as_bytes()
                            .first()
                            .map(u8::to_ascii_lowercase)
                            .as_ref()
                    && method.get(1..) == canonical.get(1..)
            }
            Self::Go => method == canonical,
            Self::Csharp => csharp::method(method, canonical),
        }
    }
}

pub(crate) fn extract<'a>(
    code: &Code<'a>,
    prefix: &str,
    services: &[Service],
    indexed: Option<&crate::index::Indexed>,
) -> Vec<Found> {
    let Some(language) = Language::of(code) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let counts = Counts::default();
    let written = std::cell::OnceCell::new();
    let builders = java::Builders::default();
    let configs = std::cell::OnceCell::new();
    let inject = |code: &Code<'a>, path: &'a str, on: crate::resolve::Injected<'a>| {
        let indexed = indexed?;
        let configs = configs
            .get_or_init(|| crate::jvm::spring::Configs::read(indexed.configs, indexed.manifests));
        crate::jvm::spring::inject(configs, code, path, on.annotated()?)
    };
    for (&path, facts) in code.files() {
        if language == Language::Java && !path.ends_with(".java") {
            continue;
        }
        let mut origins = origins::Origins::new(code, language);
        let mut apart = origins::Origins::new(code, language);
        let aliases: BTreeMap<_, _> = if language == Language::Csharp {
            facts
                .imports
                .iter()
                .map(|i| (i.local.as_str(), i.local.as_str()))
                .collect()
        } else {
            BTreeMap::new()
        };
        for (index, stored) in facts.calls.iter().enumerate() {
            let method = crate::resolve::method(&stored.callee);
            let (budget_start, mut cut) = (code.spent_apart(path), false);
            let operations = services.iter().flat_map(|service| {
                (service.operations.iter())
                    .filter(|o| language.method(method, o.method))
                    .map(move |operation| (service, operation))
            });
            for (service, operation) in operations {
                let transaction = transaction(operation.field);
                let origins = if transaction {
                    &mut apart
                } else {
                    &mut origins
                };
                let mut row = || {
                    if !origins.call(path, index, &service.client)
                        && !origins.declared(path, index, service.declared)
                    {
                        return;
                    }
                    let entries = language != Language::Java && reads_entries(operation.field);
                    let held = |params: Params<'_, 'a>| {
                        let field = operation.field;
                        held_changed(code, path, index, field, language, &counts, params)
                    };
                    let judged = match language {
                        Language::Go if entries => java::Judged {
                            changed: held(Params::Unknown),
                            parameter: None,
                            fresh: false,
                        },
                        _ => {
                            let field = operation.field;
                            let judged = changed_request(
                                code, path, index, field, language, &written, &builders,
                            );
                            java::Judged {
                                changed: judged.changed || (entries && held(Params::Unknown)),
                                ..judged
                            }
                        }
                    };
                    let changed = judged.changed;
                    let per_site =
                        language == Language::Go && entries && changed && !held(Params::Held);
                    let read = |args: &[Value], keywords: &crate::resolve::Keywords| {
                        let [Value::Call(call)] = args else {
                            return Some(vec![Name::default()]);
                        };
                        let values = request(call, keywords, operation.field, language)?;
                        let mut names = Vec::new();
                        for value in &values {
                            names.push(match (changed && !per_site, value) {
                                (true, _) => Name::default(),
                                (false, Some(v)) => {
                                    match name(code, path, Some(v), service.kind, &aliases) {
                                        Some(name) => name,
                                        None if per_site => Name::default(),
                                        None => return None,
                                    }
                                }
                                (false, None) if service.kind == "eventbridge" => Name {
                                    literal: Some("default".into()),
                                    env: None,
                                },
                                (false, None) if service.kind == "sns" => continue,
                                (false, None) => Name::default(),
                            });
                        }
                        Some(names)
                    };
                    let injected = (language == Language::Java)
                        .then_some(&inject as &crate::resolve::Inject<'a, '_>);
                    let checked = match language {
                        Language::Go => entries,
                        Language::Java => true,
                        _ => false,
                    };
                    let expanded = code.with_request_parameters(checked, || {
                        code.expand_with(Expand::Chain(path, index), injected, &read)
                    });
                    for expanded in expanded {
                        let (at, index) = expanded.site;
                        let named = |n: &Name| n.literal.is_some() || n.env.is_some();
                        let passed = expanded.found.iter().any(named)
                            && match language {
                                Language::Java => passed_changed(
                                    code,
                                    &expanded.frames,
                                    operation.field,
                                    &builders,
                                    &judged,
                                ),
                                _ => per_site && held(Params::Bound(&expanded.frames)),
                            };
                        for mut name in expanded.found {
                            if passed {
                                name = Name::default();
                            }
                            if operation.hierarchy {
                                name.literal = None;
                            }
                            let contract = match &name.literal {
                                Some(name) => format!("{prefix}::{}::{name}", service.kind),
                                None => format!("{prefix}::{}", service.kind),
                            };
                            found.push(Found {
                                identity: None,
                                role: operation.role,
                                site: if operation.role == Role::Consumer {
                                    Site::Def {
                                        path: at.into(),
                                        name: code.files()[at].calls[index].scope.clone(),
                                    }
                                } else {
                                    Site::Call {
                                        path: at.into(),
                                        index,
                                    }
                                },
                                contract: Some(contract),
                                via: name.env.map(|v| format!("env:{v}")),
                                method: Some(operation.method.into()),
                                fragments: Vec::new(),
                                shape: None,
                                lead: None,
                                cap: None,
                                client: None,
                            });
                        }
                    }
                };
                if !transaction {
                    row();
                    continue;
                }
                if cut {
                    continue;
                }
                let spent = code.spent_apart(path);
                code.set_spent_apart(path, budget_start);
                code.apart(row);
                let end = code.spent_apart(path);
                cut = end > crate::resolve::EVAL_BUDGET;
                code.set_spent_apart(path, spent.saturating_add(end - budget_start));
            }
        }
    }
    found
}

fn setter(selector: &str) -> impl Fn(&str) -> bool + '_ {
    move |method| {
        selector.split(['.', '|']).any(|member| {
            let member = member.trim_end_matches("[]");
            member.get(..1).is_some_and(|head| {
                method.len() == member.len()
                    && method[..1] == head.to_ascii_lowercase()
                    && method[1..] == member[1..]
            })
        })
    }
}

fn transaction(selector: &str) -> bool {
    selector.matches('.').count() == 2
}

fn reads_entries(selector: &str) -> bool {
    selector.contains("[].")
}

fn passed_changed<'a>(
    code: &Code<'a>,
    frames: &[crate::resolve::Frame<'a>],
    selector: &str,
    builders: &java::Builders<'a>,
    request: &java::Judged,
) -> bool {
    let (mut parameter, mut fresh) = (request.parameter, request.fresh);
    for frame in frames {
        let Some(call) = code.call(frame.path, frame.index) else {
            parameter = None;
            continue;
        };
        if parameter.is_some()
            && code
                .def(frame.def.path, frame.def.name)
                .is_none_or(|def| def.params.len() != call.args.len())
        {
            return true;
        }
        let position = parameter.take();
        if let Some(p) = position
            && fresh
            && !call.args.get(p).is_some_and(java::made_here)
        {
            return true;
        }
        if let Some(Value::Ref(list)) = position.and_then(|p| call.args.get(p))
            && (call.args.iter().enumerate())
                .any(|(i, arg)| Some(i) != position && java::hands(arg, list))
        {
            return true;
        }
        for (i, arg) in call.args.iter().enumerate() {
            let read = position == Some(i);
            let judged = java::changed(
                code,
                frame.path,
                &call.scope,
                arg,
                &call.range,
                selector,
                builders,
                read,
            );
            if judged.changed {
                return true;
            }
            if read {
                (parameter, fresh) = (judged.parameter, judged.fresh);
            }
        }
    }
    false
}

fn changed_request<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    selector: &str,
    language: Language,
    written: &std::cell::OnceCell<BTreeSet<&'a str>>,
    builders: &java::Builders<'a>,
) -> java::Judged {
    let judged = |changed| java::Judged {
        changed,
        parameter: None,
        fresh: false,
    };
    let Some(call) = code.call(path, index) else {
        return judged(false);
    };
    let request = match language {
        Language::Go => call.args.get(1),
        Language::Csharp => call.keyword("request").or_else(|| call.args.first()),
        Language::Java => {
            let Some(r) = call.args.first() else {
                return judged(false);
            };
            return java::changed(
                code,
                path,
                &call.scope,
                r,
                &call.range,
                selector,
                builders,
                false,
            );
        }
    };
    let mut values: Vec<&Value> = request.into_iter().collect();
    for member in selector.split('.') {
        let mut next = Vec::new();
        for value in values {
            for field in member.split('|').map(|f| f.trim_end_matches("[]")) {
                if let Value::Ref(_) = value {
                    if opaque(code, field, language, written) {
                        return judged(true);
                    }
                    continue;
                }
                let inner = match language {
                    Language::Go => go::field_of(value, field),
                    _ => csharp::field_of(value, field),
                };
                let Some(Some(inner)) = inner else {
                    continue;
                };
                match member.ends_with("[]").then(|| sequence(inner)).flatten() {
                    Some(items) => next.extend(items),
                    None => next.push(inner),
                }
            }
        }
        values = next;
    }
    judged(false)
}

fn hiding_builtins<'a>(code: &Code<'a>) -> BTreeSet<&'a str> {
    let mut packages = BTreeSet::new();
    for (&path, facts) in code.files() {
        let mut names = (facts.defs.iter().map(|d| d.name.as_str()))
            .chain(facts.consts.iter().map(|c| c.name.as_str()));
        if names.any(|n| n == "len" || n == "cap") {
            packages.insert(directory(path));
        }
    }
    packages
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

type Written<'a> = HashMap<(&'a str, &'a str), u32>;

#[derive(Default)]
struct Counts<'a> {
    files: std::cell::RefCell<HashMap<&'a str, Written<'a>>>,
    hiding: std::cell::OnceCell<BTreeSet<&'a str>>,
}

impl<'a> Counts<'a> {
    fn twice(&self, code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> bool {
        let mut files = self.files.borrow_mut();
        let counts = files.entry(path).or_insert_with(|| {
            let hides =
                (self.hiding.get_or_init(|| hiding_builtins(code))).contains(directory(path));
            (code.mentioned(path).iter())
                .map(|m| {
                    let reads = if hides { m.read } else { 0 };
                    let count = m.count.saturating_add(reads);
                    ((m.scope.as_str(), m.name.as_str()), count)
                })
                .collect()
        });
        counts.get(&(scope, name)) == Some(&2)
    }
}

#[derive(Clone, Copy)]
enum Params<'f, 'a> {
    Unknown,
    Held,
    Bound(&'f [Frame<'a>]),
}

#[derive(Clone, Copy)]
struct Place<'a> {
    file: &'a str,
    scope: &'a str,
}

fn held_changed<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    selector: &str,
    language: Language,
    counts: &Counts<'a>,
    params: Params<'_, 'a>,
) -> bool {
    let Some(call) = code.call(path, index) else {
        return false;
    };
    let go = language == Language::Go;
    let request = if go {
        call.args.get(1)
    } else {
        call.keyword("request").or_else(|| call.args.first())
    };
    let followed = |mut value: &'a Value, mut at: Place<'a>| {
        for _ in 0..crate::resolve::EVAL_DEPTH {
            let Value::Ref(name) = value else {
                return Some((value, at));
            };
            let local = if go {
                (code.constant_value(at.file, at.scope, name)).map(|(file, within, holds)| {
                    (file == at.file).then_some((within, within, holds))
                })
            } else {
                (code.local(at.file, at.scope, name))
                    .map(|b| Some((b.scope.as_str(), b.within.as_str(), &b.value)))
            };
            if let Some(local) = local {
                let (declared, written, holds) = local?;
                if !counts.twice(code, at.file, declared, name) {
                    return None;
                }
                (value, at.scope) = (holds, written);
                continue;
            }
            let (function, n) = code.param_of(at.file, at.scope, name).filter(|_| go)?;
            if !counts.twice(code, function.path, function.name, name) {
                return None;
            }
            match params {
                Params::Unknown => return None,
                Params::Held => return Some((value, at)),
                Params::Bound(frames) => {
                    let frame = frames.iter().find(|f| f.def == function)?;
                    let passes = code.call(frame.path, frame.index)?;
                    let count = passes.args.len();
                    if !code.runs_alone(function)
                        || code.param(function, count).is_some()
                        || code.param(function, count.wrapping_sub(1)).is_none()
                    {
                        return None;
                    }
                    let site = Some((frame.path, frame.index));
                    let &[(file, scope, passed)] = &code.param_values(function, n, site)?[..]
                    else {
                        return None;
                    };
                    if file != frame.path || scope != passes.scope {
                        return None;
                    }
                    (value, at) = (passed, Place { file, scope });
                }
            }
        }
        None
    };
    let start = Place {
        file: path,
        scope: &call.scope,
    };
    let mut values: Vec<(&'a Value, Place<'a>)> = request.map(|r| (r, start)).into_iter().collect();
    let members: Vec<&str> = selector.split('.').collect();
    for (depth, member) in members.iter().enumerate() {
        let mut next = Vec::new();
        for (value, at) in values {
            let Some((value, at)) = followed(value, at) else {
                return true;
            };
            if depth + 1 == members.len() {
                continue;
            }
            for field in member.split('|').map(|f| f.trim_end_matches("[]")) {
                let inner = if go {
                    go::field_of(value, field)
                } else {
                    csharp::field_of(value, field)
                };
                let Some(Some(inner)) = inner else {
                    continue;
                };
                if !member.ends_with("[]") {
                    next.push((inner, at));
                    continue;
                }
                let Some((list, at)) = followed(inner, at) else {
                    return true;
                };
                next.extend((sequence(list).into_iter().flatten()).map(|item| (item, at)));
            }
        }
        values = next;
    }
    false
}

fn opaque<'a>(
    code: &Code<'a>,
    field: &str,
    language: Language,
    written: &std::cell::OnceCell<BTreeSet<&'a str>>,
) -> bool {
    let field = match language {
        Language::Csharp => csharp::field_name(field),
        _ => field,
    };
    code.field_written(field)
        || language == Language::Csharp
            && written
                .get_or_init(|| {
                    code.files()
                        .keys()
                        .flat_map(|&path| code.bindings(path))
                        .filter_map(|bound| {
                            bound
                                .name
                                .rsplit_once('.')
                                .map(|(_, field)| field.trim_end_matches('='))
                        })
                        .collect()
                })
                .contains(field)
}

fn request<'v>(
    call: &'v crate::lang::Invocation,
    keywords: &'v crate::resolve::Keywords,
    path: &str,
    language: Language,
) -> Option<Vec<Option<Value>>> {
    let (head, tail) = path
        .split_once('.')
        .map_or((path, None), |(h, t)| (h, Some(t)));
    let values = match language {
        Language::Java => java::names(call, head.trim_end_matches("[]")),
        Language::Go => go::names(call, head.trim_end_matches("[]")),
        Language::Csharp => csharp::names(call, keywords, head.trim_end_matches("[]")),
    }?;
    let whole = match language {
        Language::Java => call.args.first(),
        Language::Go => call.args.get(1),
        Language::Csharp => (keywords
            .iter()
            .find(|(k, _)| k == "request")
            .map(|(_, v)| v))
        .or_else(|| call.args.first()),
    };
    let (variants, field) = tail.map_or((None, None), |tail| match tail.rsplit_once('.') {
        Some((variants, field)) => (Some(variants), Some(field)),
        None => (None, Some(tail)),
    });
    let member = |value: &'v Value, field: &str| match language {
        Language::Java => Some(java::field_of(value, field)?.first().copied().flatten()),
        Language::Go => go::field_of(value, field),
        Language::Csharp => csharp::field_of(value, field),
    };
    let mut out = Vec::new();
    let mut skipped = false;
    for value in values {
        let Some(value) = value else {
            out.push((!whole.is_some_and(complete)).then_some(Value::Unknown));
            continue;
        };
        if tail == Some("*") {
            out.extend(map_keys(value)?.into_iter().map(Some));
            continue;
        }
        let values = if head.ends_with("[]") {
            sequence(value)?
        } else {
            vec![value]
        };
        for value in values {
            let value = match variants {
                None => value,
                Some(variants) => {
                    let mut set = None;
                    for variant in variants.split('|') {
                        set = set.or(member(value, variant)?);
                    }
                    match set {
                        Some(set) => set,
                        None if complete(value) => {
                            skipped = true;
                            continue;
                        }
                        None => {
                            out.push(Some(Value::Unknown));
                            continue;
                        }
                    }
                }
            };
            let read = match field {
                None => Some(value),
                Some(field) => member(value, field)?,
            };
            out.push(match read {
                Some(read) => Some(read.clone()),
                None => (!complete(value)).then_some(Value::Unknown),
            });
        }
    }
    if out.is_empty() && !skipped {
        out.push(Some(Value::Unknown));
    }
    Some(out)
}

fn complete(value: &Value) -> bool {
    match value {
        Value::Object(_) => true,
        Value::New(_, args) => matches!(args.as_slice(), [Value::Object(_)]),
        Value::Call(call) => std::iter::successors(Some(call), |s| match s.receiver.as_deref() {
            Some(Value::Call(next)) => Some(next),
            _ => None,
        })
        .last()
        .is_some_and(|first| {
            first.callee.rsplit_once('.').is_some_and(|(ty, method)| {
                let ty = ty.rsplit('.').next().unwrap_or(ty);
                method == "builder"
                    && (ty.ends_with("Request")
                        || ty.ends_with("RequestEntry")
                        || matches!(ty, "TransactWriteItem" | "TransactGetItem"))
            })
        }),
        _ => false,
    }
}

fn sequence(value: &Value) -> Option<Vec<&Value>> {
    match value {
        Value::Ref(_) => None,
        Value::List(values) => Some(values.iter().collect()),
        Value::New(_, values) => match values.as_slice() {
            [Value::List(values)] => Some(values.iter().collect()),
            _ => Some(vec![value]),
        },
        Value::Call(call) if java::sequence(call) => Some(call.args.iter().collect()),
        value => Some(vec![value]),
    }
}

fn map_keys(value: &Value) -> Option<Vec<Value>> {
    let value = match value {
        Value::New(_, args) => match args.as_slice() {
            [value] => value,
            _ => return Some(vec![Value::Unknown]),
        },
        other => other,
    };
    match value {
        Value::Ref(_) => None,
        Value::Object(fields) => Some(
            fields
                .iter()
                .map(|(key, value)| match (key.as_str(), value) {
                    ("[]", Value::List(pair)) => pair.first().cloned().unwrap_or(Value::Unknown),
                    ("[]" | "...", _) => Value::Unknown,
                    _ => Value::Str(vec![Part::Text(key.clone())]),
                })
                .collect(),
        ),
        Value::List(entries) => Some(
            entries
                .iter()
                .map(|entry| match entry {
                    Value::List(pair) if pair.len() == 2 => pair[0].clone(),
                    _ => Value::Unknown,
                })
                .collect(),
        ),
        Value::Call(call) if java::map(call) => {
            Some(call.args.iter().step_by(2).cloned().collect())
        }
        _ => Some(vec![Value::Unknown]),
    }
}

#[derive(Default)]
struct Name {
    literal: Option<String>,
    env: Option<String>,
}

fn name<'a>(
    code: &Code<'a>,
    path: &'a str,
    value: Option<&Value>,
    kind: &str,
    aliases: &BTreeMap<&'a str, &'a str>,
) -> Option<Name> {
    match value {
        Some(Value::Ref(_)) => None,
        Some(Value::Str(parts)) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => None,
        Some(Value::Str(parts)) => {
            let env = parts.iter().find_map(|p| match p {
                Part::Env(v) | Part::EnvOr(v, _) => Some(v.clone()),
                Part::Read {
                    scope,
                    callee,
                    key,
                    parameter: false,
                    then,
                } if then.is_empty() && csharp::environment(code, path, scope, callee, aliases) => {
                    Some(key.clone())
                }
                _ => None,
            });
            let literal = if kind == "sqs" {
                let mut tail: Vec<_> = parts
                    .iter()
                    .rev()
                    .map_while(|p| match p {
                        Part::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                tail.reverse();
                let tail = tail.concat();
                if tail.contains('/') {
                    tail.rsplit('/')
                        .next()
                        .filter(|n| !n.is_empty())
                        .map(str::to_string)
                } else {
                    literal(parts).and_then(|s| physical(kind, &s))
                }
            } else {
                literal(parts).and_then(|s| physical(kind, &s))
            };
            Some(Name {
                env: env.filter(|_| literal.is_none()),
                literal,
            })
        }
        Some(Value::Call(call)) if code.pack().extensions().contains(&"go") => Some(Name {
            literal: None,
            env: go::environment(call),
        }),
        _ => Some(Name::default()),
    }
}

fn physical(kind: &str, text: &str) -> Option<String> {
    let ty = match kind {
        "sqs" => "aws_sqs_queue",
        "sns" => "aws_sns_topic",
        "eventbridge" => "aws_cloudwatch_event_bus",
        "kinesis" => "aws_kinesis_stream",
        "s3" => "aws_s3_bucket",
        "dynamodb" => "aws_dynamodb_table",
        "ssm" => "aws_ssm_parameter",
        "secretsmanager" => "aws_secretsmanager_secret",
        _ => return None,
    };
    let read = crate::iac::types::entry("terraform", ty)?.read(text);
    (!read.is_empty() && !read.starts_with("arn:") && !read.contains("://"))
        .then(|| read.to_string())
}
