use crate::lang::ts::dynamic_import;
use crate::lang::{Call, Value};
use crate::resolve::{Code, Follow, Form, Of};

fn made_by<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    name: &'a str,
    export: (&str, &str),
) -> bool {
    let origins = code.origins(path, scope, Of::Name(name), Follow::Constants);
    let [origin] = origins.as_slice() else {
        return false;
    };
    let made = (origin.outside.as_ref())
        .and_then(|o| o.imported.as_ref())
        .map(|(package, name)| (*package, name.as_str()));
    origin.form == Form::Call
        && made == Some(export)
        && origin.routes.iter().all(|r| r.steps.is_empty())
}

pub(super) fn added_plugin<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    call: &'a Call,
) -> Option<String> {
    let receiver = call.callee.strip_suffix(".add")?;
    if receiver.is_empty() || receiver.contains('.') || receiver.starts_with('(') {
        return None;
    }
    let backend = ("@backstage/backend-defaults", "createBackend");
    if !made_by(code, path, &call.scope, receiver, backend) {
        return None;
    }
    let [argument] = code.args(path, index)? else {
        return None;
    };
    dynamic_import(argument).map(str::to_string)
}

pub(super) fn test_features<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    call: &'a Call,
) -> Vec<String> {
    let started = code.imported(path, &call.scope, &call.callee);
    if started.as_ref().map(|(p, n)| (*p, n.as_str()))
        != Some(("@backstage/backend-test-utils", "startTestBackend"))
    {
        return Vec::new();
    }
    let Some([Value::Object(fields)]) = code.args(path, index) else {
        return Vec::new();
    };
    let features = fields.iter().find(|(key, _)| key == "features");
    let Some((_, Value::List(items))) = features else {
        return Vec::new();
    };
    (items.iter())
        .filter_map(|item| dynamic_import(item).map(str::to_string))
        .collect()
}
