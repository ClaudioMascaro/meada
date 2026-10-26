//! The questions the contract kinds ask of a name: what a library outside the repository it is,
//! by the spellings `Code::outside` gives it. An on-demand import is a candidate, not a fact,
//! so a kind accepts a name when exactly one of its spellings is in the kind's own table
//! (`one`). The JVM and C# kinds share them.

use crate::lang::{Facts, Kind};
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
    let outside = code.outside(file, scope, name, false)?;
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
