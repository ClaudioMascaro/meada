use crate::lang::Call;
use crate::resolve::Code;
pub(crate) use arguments::{Construction, Factories};
mod arguments;
const CALL: &str = "(call)";

fn head<'a>(code: &Code<'a>, path: &'a str, form: &Call) -> Option<&'a Call> {
    let calls = &code.files()[path].calls;
    let at = code.call_index(path, &form.range)? + 1;
    let receiver = calls.get(at)?;
    (receiver.range.start == form.range.start && receiver.range.end < form.range.end)
        .then(|| code.call(path, at))
        .flatten()
}

pub(crate) fn imported<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    callee: &'a str,
) -> Option<String> {
    let (package, spelled) = code.imported(path, scope, callee)?;
    Some(format!("{package}.{spelled}"))
}
