//! The questions the JVM packs' contract kinds ask of a name: what a library outside the
//! repository it is, by the spellings `Code::outside` gives it. An on-demand import is a
//! candidate, not a fact, so a kind accepts a name when exactly one of its spellings is in the
//! kind's own table (`one`).

use crate::lang::{Bound, Class, Facts, Kind};
use crate::qualify;
use crate::resolve::{Code, Follow, Form, Of, Outside};

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

/// The value the nearest binding of `name` around `scope` of `path` holds (`Facts::bound`, which
/// only the Kotlin pack fills), when no other binding of the name is in the same scope (a name
/// declared twice, a loop variable, a parameter, a constant, a property).
pub fn local<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &str) -> Option<&'a Bound> {
    let facts = code.files()[path];
    let mut at = Some(scope);
    while let Some(s) = at {
        let mut bound = (code.bound(path).iter()).filter(|b| b.scope == s && b.name == name);
        let held = bound.next();
        let qualified = qualify(s, name);
        let others = bound.next().is_some()
            // A `val` of unknown class has its own entry in `locals`: the pack drops its bound
            // when another binding shares the name. A typed one has a receiver, and no entry.
            || (facts.locals.binary_search(&qualified).is_ok()
                && (held.is_none()
                    || facts.receivers.iter().any(|r| r.scope == s && r.name == name)))
            || (facts.consts.iter()).any(|c| c.name == qualified)
            || (facts.defs.iter()).any(|d| d.name == s && d.params.iter().any(|p| p.name == name))
            || (facts.receivers.iter())
                .any(|r| r.scope == s && r.name == name && r.class == Class::Unknown);
        match (held, others) {
            (Some(found), false) => return Some(found),
            (None, false)
                if !facts
                    .receivers
                    .iter()
                    .any(|r| r.scope == s && r.name == name) =>
            {
                at = (!s.is_empty()).then(|| crate::parent(s));
            }
            _ => return None,
        }
    }
    None
}
