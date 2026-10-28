//! The questions the contract kinds ask of a name: what a library outside the repository it is,
//! by the spellings `Code::outside` gives it. An on-demand import is a candidate, not a fact,
//! so a kind accepts a name when exactly one of its spellings is in the kind's own table
//! (`one`). The JVM and C# kinds share them.

use crate::lang::{Call, Facts, Kind};
use crate::resolve::{Code, Follow, Form, Of, Outside};

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
    written_or(
        code.outside(file, scope, name, false),
        code,
        file,
        scope,
        name,
        is_known,
    )
}

/// `known` for the callee of an annotation (`Call::decorates`): a base from outside the
/// repository hides no import from it (#354).
pub fn annotation<'a>(
    code: &Code<'a>,
    file: &'a str,
    call: &'a Call,
    is_known: impl Fn(&str) -> bool,
) -> Option<String> {
    let outside = code.outside_decorator(file, &call.scope, &call.callee);
    written_or(outside, code, file, &call.scope, &call.callee, is_known)
}

/// The spelling the imports give `outside`, else (no import binds the head) the name as written
/// in full. The implicit `java.lang.*` makes a candidate of a dotted name, so an `outside`
/// with only that is still unbound.
fn written_or<'a>(
    outside: Option<Outside>,
    code: &Code<'a>,
    file: &'a str,
    scope: &'a str,
    name: &'a str,
    is_known: impl Fn(&str) -> bool,
) -> Option<String> {
    let unbound = outside.as_ref().is_none_or(|o| o.imported.is_none());
    let imported = outside.and_then(|o| accepted(o, &is_known));
    imported.or_else(|| written_in_full(code, file, scope, name, is_known).filter(|_| unbound))
}

/// A dotted `name` whose head nothing binds (no import, no declaration, no definition of the
/// repository) and that no definition names in full is its own spelling (`@retrofit2.http.GET`).
fn written_in_full<'a>(
    code: &Code<'a>,
    file: &'a str,
    scope: &'a str,
    name: &'a str,
    is_known: impl Fn(&str) -> bool,
) -> Option<String> {
    let (head, _) = name.split_once('.')?;
    (code.definition(file, scope, head).is_none() && in_full(code, file, scope, name, is_known))
        .then(|| name.to_string())
}

fn accepted(outside: Outside, is_known: impl Fn(&str) -> bool) -> Option<String> {
    let spellings = match outside.candidates.is_empty() {
        true => vec![outside.spelled],
        false => outside.candidates,
    };
    one(&spellings, is_known).map(str::to_string)
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

/// Whether `name`, written in full in `scope` of `path` (`System.Net.Http.HttpClient`), is a
/// library's name `is_known` accepts: no definition of the repository binds it there, since the
/// compiler prefers a type the source declares under a library's name.
pub fn in_full<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    name: &'a str,
    is_known: impl Fn(&str) -> bool,
) -> bool {
    is_known(name) && code.definition(path, scope, name).is_none()
}

/// Whether the type `ty`, written in `scope` of `path`, is the library type `full`: written in
/// full (`in_full`), or the one spelling the imports give it.
pub fn is_type<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    ty: &'a str,
    full: &str,
) -> bool {
    in_full(code, path, scope, ty, |n| n == full)
        || one(&spellings(code, path, scope, ty), |s| s == full).is_some()
}
