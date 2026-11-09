//! Standard mapping mutations, independent of SDK service/operation policy.
use crate::lang::{Part, Value};
use crate::resolve::{Code, MemberEffect, method};

fn key(value: &Value) -> Option<&str> {
    match value {
        Value::Str(parts) => match parts.as_slice() {
            [Part::Text(s)] => Some(s),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn writes<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
) -> Option<Vec<MemberEffect<'a>>> {
    let stored = &code.files()[path].calls[index];
    let python = code.pack().extensions().contains(&"py");
    let ts = code.pack().extensions().contains(&"ts");
    let method = method(&stored.callee);
    let relevant = python
        && matches!(
            method,
            "update" | "setdefault" | "pop" | "popitem" | "clear"
        )
        || ts && stored.callee == "Object.assign";
    if !relevant {
        return Some(Vec::new());
    }
    let call = code.call(path, index)?;
    let mut out = Vec::new();
    let mut add = |object, field| {
        out.push(MemberEffect {
            scope: &call.scope,
            object,
            field,
            start: call.range.end,
        })
    };
    let fields = |value: &'a Value| match value {
        Value::Object(fields) => fields
            .iter()
            .map(|(k, _)| (!matches!(k.as_str(), "..." | "[]")).then_some(k.as_str()))
            .collect(),
        _ => vec![None],
    };
    if ts {
        if code.value_bound(path, &call.scope, "Object")
            || code.imported(path, &call.scope, &call.callee).is_some()
            || code.reaches(path, index).is_some()
        {
            return Some(out);
        }
        if let Some(Value::Ref(object)) = call.args.first() {
            for source in call.args.iter().skip(1) {
                for field in fields(source) {
                    add(object, field);
                }
            }
        }
    } else if let Some((object, _)) = call.callee.rsplit_once('.') {
        match method {
            "update" => {
                for source in &call.args {
                    for field in fields(source) {
                        add(object, field);
                    }
                }
                for (name, _) in &call.kwargs {
                    add(object, (name != "...").then_some(name.as_str()));
                }
            }
            "setdefault" | "pop" => add(object, call.args.first().and_then(key)),
            _ => add(object, None),
        }
    }
    Some(out)
}
