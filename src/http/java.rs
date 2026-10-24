//! Spring MVC providers (spec #26): a method of a `@RestController` or `@Controller` class
//! carrying a mapping annotation serves the class prefix (`@RequestMapping`) joined to each of
//! its paths. The pack reads an annotation as a call decorating its target (`Call::decorates`,
//! the target a class or a method), so the annotations, `RequestMethod.X` and the constants of
//! the paths are read here, never in the pack: an annotation or an enum counts only when
//! `Code::outside` says an import from Spring binds its name (a class or an enum of the
//! repository with the same name hides it), and a path folds `static final` constants and `+`
//! through `Code::value`. `@RequestMapping` without `method` serves every method (`ANY`).
//! `server.servlet.context-path` is not applied.

use super::{ANY, contract, join, path_literal, provider};
use crate::contract::{Found, Site};
use crate::lang::{Call, Kind, Value};
use crate::resolve::Code;

const WEB: &str = "org.springframework.web.bind.annotation.";
const CONTROLLERS: &[&str] = &[
    "org.springframework.web.bind.annotation.RestController",
    "org.springframework.stereotype.Controller",
];
const MAPPING: &str = "org.springframework.web.bind.annotation.RequestMapping";
/// The method annotations that give one method, by the annotation's name in `WEB`.
const SHORTCUTS: &[(&str, &str)] = &[
    ("GetMapping", "GET"),
    ("PostMapping", "POST"),
    ("PutMapping", "PUT"),
    ("PatchMapping", "PATCH"),
    ("DeleteMapping", "DELETE"),
];
/// The members of Spring's `RequestMethod`.
const REQUEST_METHODS: &[&str] = &[
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE",
];

/// Spring MVC providers: see the module.
pub(super) fn providers<'a>(code: &Code<'a>) -> Vec<Found> {
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        if !path.ends_with(".java") {
            continue;
        }
        // What Spring annotation a call is, by its name outside the repository.
        let spring = |call: &'a Call| -> Option<String> {
            Some(
                code.outside(path, &call.scope, &call.callee, false)?
                    .spelled,
            )
        };
        let annotations: Vec<(usize, &'a Call, String)> = (facts.calls.iter().enumerate())
            .filter(|(_, c)| c.decorates.is_some())
            .filter_map(|(n, c)| Some((n, c, spring(c)?)))
            .collect();
        for (n, call, name) in &annotations {
            // A method's annotation; a class carries the prefix and the controller marker.
            let Some(target) = call
                .decorates
                .as_deref()
                .filter(|t| (facts.defs.iter()).any(|d| d.kind == Kind::Method && d.name == *t))
            else {
                continue;
            };
            let on = |class: &'a str| {
                annotations
                    .iter()
                    .filter(move |(_, c, _)| c.decorates.as_deref() == Some(class))
            };
            // A method of a controller class: its annotation's scope is the class.
            let shortcut = SHORTCUTS.iter().find(|(s, _)| *name == format!("{WEB}{s}"));
            if shortcut.is_none() && name != MAPPING
                || !on(&call.scope).any(|(.., n)| CONTROLLERS.contains(&n.as_str()))
            {
                continue;
            }
            // The index keeps no argument: they come from the full facts of the file.
            let Some(given) = code.call(path, *n) else {
                continue;
            };
            let methods = match shortcut {
                Some((_, method)) => vec![method.to_string()],
                None => match request_methods(code, path, given) {
                    Some(methods) => methods,
                    None => continue,
                },
            };
            let prefixes = match on(&call.scope).find(|(.., n)| n == MAPPING) {
                None => vec![String::new()],
                Some((c, ..)) => match code.call(path, *c) {
                    // A class-level `method` restricts every method of the class: not read.
                    Some(class) if class.keyword("method").is_none() => {
                        match paths(code, path, class) {
                            Some(prefixes) => prefixes,
                            None => continue,
                        }
                    }
                    _ => continue,
                },
            };
            let Some(routes) = paths(code, path, given) else {
                continue;
            };
            for prefix in &prefixes {
                for route in &routes {
                    let full = join(prefix, route);
                    if !balanced(&full) {
                        continue;
                    }
                    for method in &methods {
                        let site = Site::Def {
                            path: path.into(),
                            name: target.into(),
                        };
                        found.push(provider(site, contract(method, &full)));
                    }
                }
            }
        }
    }
    found
}

/// The paths of a mapping annotation: `value` or `path` (or the one unnamed argument), a string
/// or an array of them, constants folded; the root when it gives none. `None` when it gives
/// both, or a value that is no list of strings; a path that cannot be read yields none.
fn paths<'a>(code: &Code<'a>, file: &'a str, call: &'a Call) -> Option<Vec<String>> {
    let given: Vec<&Value> = (call.args.iter())
        .chain(call.keyword("value"))
        .chain(call.keyword("path"))
        .collect();
    let value = match given.as_slice() {
        [] => return Some(vec![String::new()]),
        [one] => code.value(file, &call.scope, one),
        _ => return None,
    };
    match value {
        Value::List(items) if items.is_empty() => Some(vec![String::new()]),
        Value::List(items) => Some(items.into_iter().filter_map(path_literal).collect()),
        one => path_literal(one).map(|p| vec![p]),
    }
}

/// The methods `@RequestMapping(method = ..)` names: every method without one, and those of
/// `RequestMethod.X` that Spring's enum binds (a member read through an import from Spring, so
/// a type of the repository by that name is none).
fn request_methods<'a>(code: &Code<'a>, file: &'a str, call: &'a Call) -> Option<Vec<String>> {
    let member = |value: &'a Value| -> Option<String> {
        let Value::Ref(name) = value else {
            return None;
        };
        let spelled = code.outside(file, &call.scope, name, false)?.spelled;
        let member = spelled.strip_prefix(&format!("{WEB}RequestMethod."))?;
        REQUEST_METHODS
            .contains(&member)
            .then(|| member.to_string())
    };
    let methods: Vec<String> = match call.keyword("method") {
        None => return Some(vec![ANY.into()]),
        Some(Value::List(items)) => items.iter().filter_map(member).collect(),
        Some(one) => member(one).into_iter().collect(),
    };
    let empty = matches!(call.keyword("method"), Some(Value::List(items)) if items.is_empty());
    match (methods.is_empty(), empty) {
        (true, true) => Some(vec![ANY.into()]),
        (true, false) => None,
        _ => Some(methods),
    }
}

/// Whether every `{` of `path` closes within its segment, as `{id:[0-9]{3}}` does: a pattern
/// with a `/` inside braces is not read.
fn balanced(path: &str) -> bool {
    path.split('/').all(|segment| {
        let mut depth = 0usize;
        for c in segment.chars() {
            match c {
                '{' => depth += 1,
                '}' => match depth.checked_sub(1) {
                    Some(d) => depth = d,
                    None => return false,
                },
                _ => {}
            }
        }
        depth == 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::java::Java;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    const IMPORTS: &str = "import org.springframework.web.bind.annotation.GetMapping;\n\
        import org.springframework.web.bind.annotation.RequestMapping;\n\
        import org.springframework.web.bind.annotation.RequestMethod;\n\
        import org.springframework.web.bind.annotation.RestController;\n";

    /// The contracts of the providers one controller file gives.
    fn served(imports: &str, body: &str) -> Vec<String> {
        let src = format!("package a;\n{imports}\n{body}\n");
        let facts = Java.parse("a/C.java", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a/C.java", &facts)]);
        let resolver = Resolver::new(&files, Java.modules(&[], &files).unwrap(), &Java);
        let code = Code::new(&resolver, &files, &Java);
        let mut found: Vec<String> = (providers(&code).into_iter())
            .filter_map(|f| f.contract)
            .collect();
        found.sort();
        found
    }

    fn controller(annotations: &str, methods: &str) -> Vec<String> {
        served(
            IMPORTS,
            &format!("@RestController {annotations} class C {{ {methods} }}"),
        )
    }

    #[test]
    fn a_method_less_request_mapping_serves_every_method_and_an_empty_list_too() {
        let ok = |m: &str| {
            controller(
                "",
                &format!("@RequestMapping(value = \"/x\"{m}) void f() {{}}"),
            )
        };
        assert_eq!(ok(""), ["http::ANY::/x"]);
        assert_eq!(ok(", method = {}"), ["http::ANY::/x"]);
        assert_eq!(ok(", method = RequestMethod.TRACE"), ["http::TRACE::/x"]);
    }

    #[test]
    fn a_method_that_is_not_springs_enum_member_serves_nothing() {
        let with = |imports: &str, local: &str, m: &str| {
            served(
                imports,
                &format!(
                    "@RestController class C {{ {local} @RequestMapping(value = \"/x\", method = {m}) void f() {{}} }}"
                ),
            )
        };
        let spring = IMPORTS;
        assert_eq!(with(spring, "", "RequestMethod.GET"), ["http::GET::/x"]);
        // Nowhere to be read, not a member, or hidden by a type of the file.
        assert!(with(spring, "", "Unknown.GET").is_empty());
        assert!(with(spring, "", "RequestMethod.SPELL").is_empty());
        assert!(with(spring, "enum RequestMethod { GET }", "RequestMethod.GET").is_empty());
        let other = IMPORTS.replace(
            "org.springframework.web.bind.annotation.RequestMethod",
            "org.acme.RequestMethod",
        );
        assert!(with(&other, "", "RequestMethod.GET").is_empty());
    }

    #[test]
    fn an_annotation_of_another_package_or_without_a_controller_serves_nothing() {
        let other = IMPORTS.replace(
            "org.springframework.web.bind.annotation.GetMapping",
            "org.acme.GetMapping",
        );
        assert!(
            served(
                &other,
                "@RestController class C { @GetMapping(\"/x\") void f() {} }"
            )
            .is_empty()
        );
        assert!(served(IMPORTS, "class C { @GetMapping(\"/x\") void f() {} }").is_empty());
        // A nested class's own annotations are not its methods'.
        let nested = "@RestController class C { @RequestMapping(\"/in\") @RestController static class D { @GetMapping(\"/x\") void f() {} } }";
        assert_eq!(served(IMPORTS, nested), ["http::GET::/in/x"]);
    }

    #[test]
    fn what_cannot_be_read_with_certainty_serves_nothing() {
        // `value` and `path` together, a class-level `method`, a pattern with a `/` inside braces.
        let both = "@GetMapping(value = \"/a\", path = \"/b\") void f() {}";
        assert!(controller("", both).is_empty());
        let restricted = "@GetMapping(\"/a\") void f() {}";
        assert!(controller("@RequestMapping(method = RequestMethod.POST)", restricted).is_empty());
        let slash = "@GetMapping(\"/a/{id:x/y}\") void f() {}";
        assert!(controller("", slash).is_empty());
        let regex = "@GetMapping(\"/a/{id:[0-9]{3}}\") void f() {}";
        assert_eq!(controller("", regex), ["http::GET::/a/{}"]);
    }

    #[test]
    fn a_class_prefix_joins_each_path_of_each_array() {
        let found = controller(
            "@RequestMapping({\"/v1\", \"/v2/\"})",
            "@GetMapping({\"a\", \"/b/\"}) void f() {}",
        );
        assert_eq!(
            found,
            [
                "http::GET::/v1/a",
                "http::GET::/v1/b",
                "http::GET::/v2/a",
                "http::GET::/v2/b"
            ]
        );
    }
}
