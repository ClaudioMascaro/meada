//! The questions the JVM packs' contract kinds ask of a name: what a library outside the
//! repository it is, by the spellings `Code::outside` gives it. An on-demand import is a
//! candidate, not a fact, so a kind accepts a name when exactly one of its spellings is in the
//! kind's own table (`one`).

use crate::lang::{Facts, Kind, Value};
use crate::resolve::{Code, Follow, Form, Of, Outside};

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

/// Whether `target`, what an annotation of `facts` decorates, is a method of the file: a
/// method's annotation, not a class's or a field's.
pub fn is_method(facts: &Facts, target: &str) -> bool {
    (facts.defs.iter()).any(|d| d.kind == Kind::Method && d.name == target)
}

/// The one spelling of a name (`Outside`, `Value::External`) that `is_known` accepts: two of
/// them, or none, is no name the kind knows.
pub fn one(spellings: &[String], is_known: impl Fn(&str) -> bool) -> Option<&str> {
    let mut hits = spellings.iter().filter(|s| is_known(s));
    let found = hits.next()?;
    hits.next().is_none().then_some(found.as_str())
}

/// The name `name`, written in `scope` of `file`, as a name of a library `is_known` accepts:
/// bound by an import (single-type or static) from outside, or the one candidate of the
/// on-demand imports in the table. Two candidates in the table, or none, is no such name.
pub fn known<'a>(
    code: &Code<'a>,
    file: &'a str,
    scope: &'a str,
    name: &'a str,
    is_known: impl Fn(&str) -> bool,
) -> Option<String> {
    let outside = code.outside(file, scope, name, false)?;
    let spellings = match outside.candidates.is_empty() {
        true => vec![outside.spelled],
        false => outside.candidates,
    };
    one(&spellings, is_known).map(str::to_string)
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

/// How the imports of `path` may spell `name`, written in `scope` (`Outside::spellings`).
pub fn spellings<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> Vec<String> {
    (code.outside(path, scope, name, false)).map_or_else(Vec::new, Outside::spellings)
}

/// How the imports may spell the outside type the source writes for `name` (a field, a
/// parameter, a local): none when it writes no such type.
pub fn typed<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> Vec<String> {
    declared(code, path, scope, name).unwrap_or_default()
}

/// `typed`, telling a name the source writes no type for (`None`) from one whose type is of
/// the repository (no spelling).
pub fn declared<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    name: &'a str,
) -> Option<Vec<String>> {
    let found = code.origins(path, scope, Of::Name(name), Follow::Declarations);
    match found.into_iter().next() {
        Some(o) if o.form == Form::Type && o.routes.iter().all(|r| r.steps.is_empty()) => {
            Some(o.outside.map_or_else(Vec::new, Outside::spellings))
        }
        _ => None,
    }
}
