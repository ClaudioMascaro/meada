//! Java request builders and imported client factories, without accepting declared types.
use crate::lang::{Invocation, Value};
use crate::resolve::Code;

pub(super) fn constructor(name: &str, steps: &[&str], client: &str) -> bool {
    name.strip_suffix(".create")
        .is_some_and(|n| client_type(n, client))
        && steps.is_empty()
        || name
            .strip_suffix(".builder")
            .is_some_and(|n| client_type(n, client))
            && steps.last() == Some(&"build")
            && steps.iter().all(|s| {
                matches!(
                    *s,
                    "build"
                        | "region"
                        | "credentialsProvider"
                        | "endpointOverride"
                        | "httpClient"
                        | "httpClientBuilder"
                        | "overrideConfiguration"
                        | "defaultsMode"
                        | "dualstackEnabled"
                        | "fipsEnabled"
                )
            })
}

pub(super) fn made<'a>(code: &Code<'a>, path: &'a str, index: usize, client: &str) -> bool {
    let Some(chain) = code.chain(path, index) else {
        return false;
    };
    let Some(&start) = chain.last() else {
        return false;
    };
    let Some(call) = code.call(path, start) else {
        return false;
    };
    let steps: Vec<_> = chain
        .iter()
        .rev()
        .skip(1)
        .map(|&i| crate::resolve::method(&code.files()[path].calls[i].callee))
        .collect();
    crate::names::known(code, path, &call.scope, &call.callee, |s| {
        constructor(s, &steps, client)
    })
    .is_some()
}

pub(super) fn names<'v>(call: &'v Invocation, field: &str) -> Option<Vec<Option<&'v Value>>> {
    let Some(request) = call.args.first() else {
        return Some(vec![None]);
    };
    field_of(request, field)
}

pub(super) fn field_of<'v>(value: &'v Value, field: &str) -> Option<Vec<Option<&'v Value>>> {
    if matches!(value, Value::Ref(_)) {
        return None;
    }
    let Value::Call(top) = value else {
        return Some(vec![None]);
    };
    let key = format!("{}{}", field[..1].to_ascii_lowercase(), &field[1..]);
    let steps = std::iter::successors(Some(top), |s| match s.receiver.as_deref() {
        Some(Value::Call(next)) => Some(next),
        _ => None,
    });
    for step in steps {
        if crate::resolve::method(&step.callee) == key {
            return Some(step.args.iter().map(Some).collect());
        }
    }
    Some(vec![None])
}

fn of(call: &Invocation, classes: &[&str], method: &str) -> bool {
    let Some(Value::External(names)) = call.receiver.as_deref() else {
        return false;
    };
    if crate::resolve::method(&call.callee) != method {
        return false;
    }
    crate::names::one(names, |s| classes.contains(&s)).is_some()
        // A full package spelling whose head is unbound has only on-demand candidates in
        // the folded value. As names::known does, accept its written standard-library name.
        // A local, type or explicit import hiding `java` has no such implicit candidate.
        || classes.iter().any(|class| call.callee == format!("{class}.{method}")
            && names.contains(&format!("java.lang.{class}")))
}

pub(super) fn sequence(call: &Invocation) -> bool {
    of(call, &["java.util.List", "java.util.Set"], "of")
        || of(call, &["java.util.Arrays"], "asList")
}

pub(super) fn map(call: &Invocation) -> bool {
    of(call, &["java.util.Map"], "of")
}

fn client_type(name: &str, client: &str) -> bool {
    name == client
        || name
            .strip_suffix("AsyncClient")
            .is_some_and(|n| Some(n) == client.strip_suffix("Client"))
}
