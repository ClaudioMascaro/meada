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
            let field = match field {
                "Bucket" => "BucketName",
                other => other,
            };
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
    let field = match field {
        "Bucket" => "BucketName",
        other => other,
    };
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

pub(super) fn environment<'a>(code: &Code<'a>, path: &'a str, scope: &str, callee: &str) -> bool {
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
    // Use the source's scope, so a repository Environment type or local hides System's.
    head == "Environment"
        && crate::names::one(
            &crate::names::spellings(code, path, scope, "Environment"),
            |s| s == "System.Environment",
        )
        .is_some()
        || head == "System.Environment"
            && code.definition(path, scope, "System.Environment").is_none()
}
