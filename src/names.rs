pub(crate) mod python;
pub(crate) mod ts;

use crate::lang::{Call, Facts, Kind};
use crate::resolve::{Code, Follow, Form, Of, Outside};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Argument {
    Literal(String),
    Environment(String),
    Unresolved,
}

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

pub fn is_method(facts: &Facts, target: &str) -> bool {
    (facts.defs.iter()).any(|d| d.kind == Kind::Method && d.name == target)
}

pub fn one(spellings: &[String], is_known: impl Fn(&str) -> bool) -> Option<&str> {
    let mut hits = spellings.iter().filter(|s| is_known(s));
    let found = hits.next()?;
    hits.next().is_none().then_some(found.as_str())
}

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

pub fn annotation<'a>(
    code: &Code<'a>,
    file: &'a str,
    call: &'a Call,
    is_known: impl Fn(&str) -> bool,
) -> Option<String> {
    let outside = code.outside_decorator(file, &call.scope, &call.callee);
    written_or(outside, code, file, &call.scope, &call.callee, is_known)
}

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

pub fn spellings<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> Vec<String> {
    (code.outside(path, scope, name, false)).map_or_else(Vec::new, Outside::spellings)
}

pub fn typed<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> Vec<String> {
    declared(code, path, scope, name).unwrap_or_default()
}

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

pub fn in_full<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    name: &'a str,
    is_known: impl Fn(&str) -> bool,
) -> bool {
    is_known(name) && code.definition(path, scope, name).is_none()
}

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

    #[test]
    fn a_name_written_in_full_is_the_library_s_only_when_the_repository_binds_none_of_it() {
        assert_eq!(written("", GET, &[]).as_deref(), Some(GET));
        assert_eq!(written("import com.acme.org;", GET, &[]), None);
        let head = [("a/org.java", "package a; class org {}")];
        assert_eq!(written("", GET, &head), None);
        let own = format!(
            "package {}; @interface GetMapping {{}}",
            GET.rsplit_once('.').unwrap().0
        );
        let same = [(
            "org/springframework/web/bind/annotation/GetMapping.java",
            &*own,
        )];
        assert_eq!(written("", GET, &same), None);
        let other = own.replace("GetMapping", "PostMapping");
        let beside = [(
            "org/springframework/web/bind/annotation/PostMapping.java",
            &*other,
        )];
        assert_eq!(written("", GET, &beside).as_deref(), Some(GET));
    }
}
