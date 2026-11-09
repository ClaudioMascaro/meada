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

/// The local a builder chain starts from (`b` in `b.message(..).build()`).
pub(crate) fn root(value: &Value) -> Option<&str> {
    let Value::Call(call) = value else {
        return None;
    };
    let mut call = call;
    loop {
        match call.receiver.as_deref()? {
            Value::Call(next) => call = next,
            Value::Ref(local) => return Some(local),
            _ => return None,
        }
    }
}

/// Whether the request `value` reads a builder `rebuilt` outside `range`: its own chain
/// (`b.message(..).build()`) or one a step takes (`entries(e.build())`). A named value
/// (`Put p = pb.build()`) is what its declaration built, so only what happened to its builder up
/// to that `.build()` counts: a value built before the builder is set again is a copy.
pub(super) fn changed<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: &'a Value,
    range: &std::ops::Range<usize>,
    setter: &dyn Fn(&str) -> bool,
) -> bool {
    let mut walk = Walk {
        code,
        path,
        scope,
        setter,
        seen: std::collections::HashSet::new(),
    };
    walk.value(value, range, usize::MAX)
}

/// The named values and builders one `changed` question has followed.
struct Walk<'a, 'c> {
    code: &'c Code<'a>,
    path: &'a str,
    scope: &'a str,
    setter: &'c dyn Fn(&str) -> bool,
    seen: std::collections::HashSet<*const crate::lang::Bound>,
}

impl<'a> Walk<'a, '_> {
    /// Whether `value`, made outside `range`, reads a builder set again before `until`.
    fn value(&mut self, value: &'a Value, range: &std::ops::Range<usize>, until: usize) -> bool {
        match value {
            Value::Ref(name) => self.named(name, range, until),
            Value::Call(top) => {
                root(value).is_some_and(|b| {
                    rebuilt_until(self.code, self.path, self.scope, b, range, until, self.setter)
                        // The builder was made from values of its own.
                        || self.named(b, range, until)
                }) || std::iter::successors(Some(top), |c| match c.receiver.as_deref() {
                    Some(Value::Call(next)) => Some(next),
                    _ => None,
                })
                .flat_map(|c| &c.args)
                .any(|a| self.value(a, range, until))
            }
            _ => false,
        }
    }

    /// Whether the local `name` was declared from a builder set again before its own `.build()`.
    /// Each local is judged once; a chain of them past `EVAL_DEPTH` is not followed, so the name
    /// stays unresolved.
    fn named(&mut self, name: &str, range: &std::ops::Range<usize>, until: usize) -> bool {
        let Some(bound) = self.code.local(self.path, self.scope, name) else {
            return false;
        };
        if !self.seen.insert(bound) {
            return false;
        }
        if self.seen.len() > crate::resolve::EVAL_DEPTH {
            return true;
        }
        match &bound.value {
            Value::Call(made) => self.value(&bound.value, &made.range, made.range.start),
            alias @ Value::Ref(_) => self.value(alias, range, until),
            _ => false,
        }
    }
}

/// Whether the builder `local`, read in `scope` of `path`, is set again outside `range` by a
/// step `setter` accepts, or handed to a call there: a Java builder changes in place, so the
/// chain a call reads off it is not what it was given. A step counts anywhere in a chain off the
/// builder (`b.message(..).topicArn(..)`), or off a local holding it or such a chain (a step
/// returns its builder). A local counts in its scope, a `final` field (a constant) anywhere in
/// its file.
pub(crate) fn rebuilt<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    local: &'a str,
    range: &std::ops::Range<usize>,
    setter: &dyn Fn(&str) -> bool,
) -> bool {
    rebuilt_until(code, path, scope, local, range, usize::MAX, setter)
}

/// `rebuilt`, counting only the calls that end by `until`: a local's builder set again after the
/// value was built does not change that value. A `final` field is set from any method, so its
/// order in the file says nothing and `until` does not apply.
fn rebuilt_until<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    local: &'a str,
    range: &std::ops::Range<usize>,
    until: usize,
    setter: &dyn Fn(&str) -> bool,
) -> bool {
    let within = if code.local(path, scope, local).is_some() {
        Some(scope)
    } else if code.constant_value(path, scope, local).is_some() {
        None
    } else {
        return false;
    };
    let named = |name: &str| {
        name == local
            || name.strip_prefix("this.") == Some(local)
            || within
                .and_then(|s| code.local(path, s, name))
                .is_some_and(|b| {
                    matches!(&b.value, Value::Ref(n) if n == local)
                        || unbuilt(&b.value) && root(&b.value) == Some(local)
                })
    };
    // A step that still returns the builder: `build()` makes the request, which cannot change,
    // and `Object`'s methods return a value of their own.
    fn unbuilt(value: &Value) -> bool {
        matches!(value, Value::Call(c) if !matches!(
            crate::resolve::method(&c.callee),
            "build" | "toString" | "hashCode" | "equals" | "getClass"
        ))
    }
    let holds = |value: &Value| match value {
        Value::Ref(name) => named(name),
        value => unbuilt(value) && root(value).is_some_and(named),
    };
    code.files()[path].calls.iter().enumerate().any(|(i, c)| {
        within.is_none_or(|s| c.scope == s && c.range.end <= until)
            && !(range.start <= c.range.start && c.range.end <= range.end)
            && code.call(path, i).is_some_and(|call| {
                let steps = std::iter::successors(
                    Some((call.callee.as_str(), call.receiver.as_ref())),
                    |(_, receiver)| match receiver {
                        Some(Value::Call(next)) => {
                            Some((next.callee.as_str(), next.receiver.as_deref()))
                        }
                        _ => None,
                    },
                );
                let (mut set, mut on) = (false, false);
                for (callee, receiver) in steps {
                    set |= setter(crate::resolve::method(callee));
                    on = match receiver {
                        Some(Value::Call(_)) => continue,
                        Some(value) => holds(value),
                        None => callee.rsplit_once('.').is_some_and(|(head, _)| named(head)),
                    };
                }
                set && on || call.args.iter().any(holds)
            })
    })
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
