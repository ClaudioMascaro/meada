//! What the JVM packs' contract kinds read besides the name questions of `names` (the crate's):
//! the Java and Kotlin files of a repository, and Kotlin's `arrayOf`.

use crate::lang::{Facts, Value};
use crate::names::known;
use crate::resolve::Code;

/// How many locals a JVM client reader follows a client through (`val a = b`, `val b =
/// Client()`, `c.use { it.get(..) }` on a local `c`, `Code::local`).
// ponytail: a client held through more locals than this is not followed, so its calls are no
// consumers; real code holds one through one or two, and the limit keeps a crafted chain of
// locals from costing a walk per link.
pub const LOCAL_DEPTH: usize = 8;

/// The Java files of `code`, with their facts: what the Java readers of the kinds read.
pub fn java_files<'a>(code: &Code<'a>) -> impl Iterator<Item = (&'a str, &'a Facts)> + use<'a> {
    (code.files().iter())
        .filter(|(path, _)| path.ends_with(".java"))
        .map(|(&path, &facts)| (path, facts))
}

/// The Java and Kotlin files of `code`, with their facts: what the readers of annotations that
/// both languages write the same way read (Spring MVC, the channel clients).
pub fn jvm_files<'a>(code: &Code<'a>) -> impl Iterator<Item = (&'a str, &'a Facts)> + use<'a> {
    (code.files().iter())
        .filter(|(path, _)| path.ends_with(".java") || kotlin(path))
        .map(|(&path, &facts)| (path, facts))
}

/// Whether `path` is a Kotlin file, which writes a `new` as a plain call.
pub fn kotlin(path: &str) -> bool {
    path.ends_with(".kt") || path.ends_with(".kts")
}

/// The elements of Kotlin's `arrayOf(..)` written as an annotation's array, when the name is
/// the standard library's (a function of the repository by that name is none).
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
