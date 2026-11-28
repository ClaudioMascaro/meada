use crate::lang::{Facts, Value};
use crate::names::known;
use crate::resolve::Code;

// ponytail: a client held through more locals than this is not followed, so its calls are no
// consumers; real code holds one through one or two, and the limit keeps a crafted chain of
// locals from costing a walk per link.
pub const LOCAL_DEPTH: usize = 8;

pub fn java_files<'a>(code: &Code<'a>) -> impl Iterator<Item = (&'a str, &'a Facts)> + use<'a> {
    (code.files().iter())
        .filter(|(path, _)| path.ends_with(".java"))
        .map(|(&path, &facts)| (path, facts))
}

pub fn jvm_files<'a>(code: &Code<'a>) -> impl Iterator<Item = (&'a str, &'a Facts)> + use<'a> {
    (code.files().iter())
        .filter(|(path, _)| path.ends_with(".java") || kotlin(path))
        .map(|(&path, &facts)| (path, facts))
}

pub fn kotlin(path: &str) -> bool {
    path.ends_with(".kt") || path.ends_with(".kts")
}

pub fn array_of<'a>(
    code: &Code<'a>,
    file: &'a str,
    scope: &'a str,
    v: &'a Value,
) -> Option<&'a [Value]> {
    let Value::Call(call) = v else { return None };
    (call.receiver.is_none() && call.callee == "arrayOf").then_some(())?;
    known(code, file, scope, &call.callee, |s| s == "kotlin.arrayOf")?;
    Some(&call.args)
}
