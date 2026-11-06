//! C# request initializers, constructor calls and framework environment reads.
use crate::lang::{Invocation, Value};
use crate::resolve::Code;

pub(super) fn names<'v>(
    call: &'v Invocation,
    keywords: &'v crate::resolve::Keywords,
    field: &str,
) -> Option<Vec<Option<&'v Value>>> {
    let request = keywords
        .iter()
        .find(|(k, _)| k == "request")
        .map(|(_, v)| v)
        .or_else(|| call.args.first());
    match request {
        Some(Value::New(_, _) | Value::Object(_)) => field_of(request?, field).map(|v| vec![v]),
        value => {
            let field = field_name(field);
            let key = format!("{}{}", field[..1].to_ascii_lowercase(), &field[1..]);
            Some(vec![
                keywords
                    .iter()
                    .find(|(k, _)| k == &key)
                    .map(|(_, v)| v)
                    .or(value),
            ])
        }
    }
}

pub(super) fn field_of<'v>(value: &'v Value, field: &str) -> Option<Option<&'v Value>> {
    let field = field_name(field);
    super::go::field_of(value, field)
}

pub(super) fn method(method: &str, canonical: &str) -> bool {
    let method = method.strip_suffix("Async").unwrap_or(method);
    let canonical = match canonical {
        "HeadObject" => "GetObjectMetadata",
        other => other,
    };
    method == canonical
}

pub(super) fn environment<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &str,
    callee: &str,
    aliases: &std::collections::BTreeMap<&'a str, &'a str>,
) -> bool {
    let Some((head, "GetEnvironmentVariable")) = callee.rsplit_once('.') else {
        return false;
    };
    let facts = code.files()[path];
    let Some(scope) = (scope.is_empty().then_some("")).or_else(|| {
        facts
            .defs
            .iter()
            .find(|d| d.name == scope)
            .map(|d| d.name.as_str())
    }) else {
        return false;
    };
    let head = match head {
        "Environment" => "Environment",
        "System.Environment" => "System.Environment",
        other => match aliases.get(other) {
            Some(head) => head,
            None => return false,
        },
    };
    // Resolve in the original scope, so a local/type hiding an imported alias wins.
    crate::names::known(code, path, scope, head, |s| s == "System.Environment").is_some()
}

pub(super) fn field_name(field: &str) -> &str {
    match field {
        "Bucket" => "BucketName",
        other => other,
    }
}
