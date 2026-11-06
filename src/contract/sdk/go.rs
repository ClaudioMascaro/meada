//! Go imported constructor functions and input composite literals.
use crate::lang::{Invocation, Value, property};
use crate::resolve::Code;

pub(super) fn constructor(name: &str, steps: &[&str], package: &str) -> bool {
    steps.is_empty()
        && name
            .strip_prefix(package)
            .is_some_and(|s| matches!(s, ".New" | ".NewFromConfig"))
}

pub(super) fn made<'a>(code: &Code<'a>, path: &'a str, index: usize, client: &str) -> bool {
    let Some(call) = code.call(path, index) else {
        return false;
    };
    code.outside(path, &call.scope, &call.callee, call.by_path)
        .is_some_and(|o| constructor(&o.spelled, &[], client) && o.direct)
}

pub(super) fn names<'v>(call: &'v Invocation, field: &str) -> Option<Vec<Option<&'v Value>>> {
    let request = call.args.get(1).unwrap_or(&Value::Undefined);
    field_of(request, field).map(|v| vec![v])
}

pub(super) fn field_of<'v>(value: &'v Value, field: &str) -> Option<Option<&'v Value>> {
    match value {
        Value::Ref(_) => None,
        Value::Object(fields) => property(fields, field).ok().map(|v| v.map(string_pointer)),
        Value::New(_, args) => match args.as_slice() {
            [Value::Object(fields)] => property(fields, field).ok().map(|v| v.map(string_pointer)),
            _ => Some(None),
        },
        _ => Some(None),
    }
}

/// The SDK's string pointer helper preserves its argument. A same-named local does not.
fn string_pointer(value: &Value) -> &Value {
    if let Value::Call(call) = value
        && crate::resolve::method(&call.callee) == "String"
        && matches!(call.receiver.as_deref(), Some(Value::External(names)) if crate::names::one(names, |s| s == "github.com/aws/aws-sdk-go-v2/aws").is_some())
        && let [arg] = call.args.as_slice()
    {
        return arg;
    }
    value
}

/// Import aliases remain verified runtime reads after the pack leaves them as a call.
pub(super) fn environment(call: &Invocation) -> Option<String> {
    if crate::resolve::method(&call.callee) != "Getenv"
        || !matches!(call.receiver.as_deref(), Some(Value::External(names)) if crate::names::one(names, |s| s == "os").is_some())
    {
        return None;
    }
    let [Value::Str(parts)] = call.args.as_slice() else {
        return None;
    };
    crate::lang::literal(parts).filter(|s| !s.is_empty())
}
