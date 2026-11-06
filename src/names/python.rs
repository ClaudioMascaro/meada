//! External constructors and one level of factories followed through Python values.
use crate::lang::Call;
use crate::resolve::Code;
pub(crate) use arguments::{Construction, Factories};
mod arguments;
const CALL: &str = "(call)";

/// The call that `form`, a method called on a call written in place (`X(...).method()`), is
/// called on: the last to end of the calls that start where `form` does, inside it.
fn head<'a>(code: &Code<'a>, path: &'a str, form: &Call) -> Option<&'a Call> {
    let calls = &code.files()[path].calls;
    // Packs emit an enclosing call before its receiver. The next same-start call is
    // therefore its longest proper receiver, without scanning the file per SDK call.
    let at = code.call_index(path, &form.range)? + 1;
    let receiver = calls.get(at)?;
    (receiver.range.start == form.range.start && receiver.range.end < form.range.end)
        .then(|| code.call(path, at))
        .flatten()
}

/// What `callee`, written in `scope` of `path`, names in a package outside the repository,
/// spelled from the package's import: `pika.BlockingConnection` however it is imported. A
/// method of a local (`connection.channel`) is none.
pub(crate) fn imported<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    callee: &'a str,
) -> Option<String> {
    let (package, spelled) = code.imported(path, scope, callee)?;
    Some(format!("{package}.{spelled}"))
}
