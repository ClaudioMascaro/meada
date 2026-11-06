//! The questions the contract kinds ask of a name: what a library outside the repository it is,
//! by the spellings `Code::outside` gives it. An on-demand import is a candidate, not a fact,
//! so a kind accepts a name when exactly one of its spellings is in the kind's own table
//! (`one`). The JVM and C# kinds share them.

pub(crate) mod python;
pub(crate) mod ts;

use crate::lang::{Call, Facts, Kind};
use crate::resolve::{Code, Follow, Form, Of, Outside};

/// The readable part of a contract name argument, after value expansion.
pub(crate) enum Argument {
    Literal(String),
    Environment(String),
    Unresolved,
}

/// A parameter asks expansion to climb its exact callers; runtime names stay unresolved.
pub(crate) fn argument(value: &crate::lang::Value) -> Option<Argument> {
    use crate::lang::{Part, Value, literal};
    Some(match value {
        Value::Ref(_) => return None,
        Value::Str(parts) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => return None,
        Value::Str(parts) => match &parts[..] {
            [Part::Env(name) | Part::EnvOr(name, _)] => Argument::Environment(name.clone()),
            _ => literal(parts).map_or(Argument::Unresolved, Argument::Literal),
        },
        _ => Argument::Unresolved,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::java::Java;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    const GET: &str = "org.springframework.web.bind.annotation.GetMapping";

    /// What `annotation` makes of the annotation `callee`, written in `a/C.java` under
    /// `imports`, with the other files of the repository (`(path, source)`): the framework's
    /// `GetMapping` is the one name the table knows.
    fn written(imports: &str, callee: &str, others: &[(&str, &str)]) -> Option<String> {
        let src = format!("package a;\n{imports}\nclass C {{ @{callee}(\"/x\") void f() {{}} }}\n");
        let mut parsed = vec![("a/C.java", Java.parse("a/C.java", src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, Java.parse(path, source.as_bytes()).unwrap()));
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let code = Code::new(&resolver, &files, &Java);
        let facts = files["a/C.java"];
        let call = (facts.calls.iter())
            .find(|c| c.decorates.is_some())
            .unwrap();
        annotation(&code, "a/C.java", call, |s| s == GET)
    }

    /// The three guards of `written_in_full` (#360): a dotted name is its own spelling only when
    /// no import binds its head, no definition of the repository is its head, and none names it
    /// in full. The repository wins each time.
    #[test]
    fn a_name_written_in_full_is_the_library_s_only_when_the_repository_binds_none_of_it() {
        assert_eq!(written("", GET, &[]).as_deref(), Some(GET));
        // An import binds the head: `org` is the imported class, not the root package.
        assert_eq!(written("import com.acme.org;", GET, &[]), None);
        // A class of the repository is the head.
        let head = [("a/org.java", "package a; class org {}")];
        assert_eq!(written("", GET, &head), None);
        // A class of the repository is spelled exactly like the library's.
        let own = format!(
            "package {}; @interface GetMapping {{}}",
            GET.rsplit_once('.').unwrap().0
        );
        let same = [(
            "org/springframework/web/bind/annotation/GetMapping.java",
            &*own,
        )];
        assert_eq!(written("", GET, &same), None);
        // Another class of that package hides nothing.
        let other = own.replace("GetMapping", "PostMapping");
        let beside = [(
            "org/springframework/web/bind/annotation/PostMapping.java",
            &*other,
        )];
        assert_eq!(written("", GET, &beside).as_deref(), Some(GET));
    }
}
