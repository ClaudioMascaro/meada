mod clients;
mod declarative;

use super::{ANY, METHODS, balanced, contract, join, path_literal, provider};
use crate::contract::{Found, Site};
use crate::index::Indexed;
use crate::jvm::names::{LOCAL_DEPTH, array_of, java_files, jvm_files};
use crate::lang::{Bound, Call, Value};
use crate::names::{annotation as known_annotation, is_method, known, one, typed};
use crate::resolve::{Code, Expand, Follow, Inject, Of, passes_nothing};

const WEB: &str = "org.springframework.web.bind.annotation.";
const CONTROLLERS: &[&str] = &[
    "org.springframework.web.bind.annotation.RestController",
    "org.springframework.stereotype.Controller",
];
const MAPPING: &str = "org.springframework.web.bind.annotation.RequestMapping";
const SHORTCUTS: &[(&str, &str)] = &[
    ("GetMapping", "GET"),
    ("PostMapping", "POST"),
    ("PutMapping", "PUT"),
    ("PatchMapping", "PATCH"),
    ("DeleteMapping", "DELETE"),
];
const REQUEST_METHODS: &[&str] = &[
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE",
];
const REQUEST_METHOD: &str = "org.springframework.web.bind.annotation.RequestMethod.";
const MAPPING_PATHS: &[&str] = &["value", "path"];

pub(super) fn consumers<'a>(code: &Code<'a>, indexed: &Indexed) -> Vec<Found> {
    let mut found = clients::consumers(code, indexed);
    found.extend(declarative::consumers(code, indexed));
    found
}

pub(super) fn providers<'a>(code: &Code<'a>) -> Vec<Found> {
    let mut found = Vec::new();
    for (path, facts) in jvm_files(code) {
        let spring =
            |call: &'a Call| -> Option<String> { known_annotation(code, path, call, annotation) };
        let annotations: Vec<(usize, &'a Call, String)> = (facts.calls.iter().enumerate())
            .filter(|(_, c)| c.decorates.is_some())
            .filter_map(|(n, c)| Some((n, c, spring(c)?)))
            .collect();
        for (n, call, name) in &annotations {
            let Some(target) = call.decorates.as_deref().filter(|t| is_method(facts, t)) else {
                continue;
            };
            let on = |class: &'a str| {
                annotations
                    .iter()
                    .filter(move |(_, c, _)| c.decorates.as_deref() == Some(class))
            };
            let shortcut = shortcut(name);
            if shortcut.is_none() && name != MAPPING
                || !on(&call.scope).any(|(.., n)| CONTROLLERS.contains(&n.as_str()))
            {
                continue;
            }
            let Some(given) = code.call(path, *n) else {
                continue;
            };
            let methods = match shortcut {
                Some(method) => vec![method.to_string()],
                None => match request_methods(code, path, given) {
                    Some(methods) => methods,
                    None => continue,
                },
            };
            let prefixes = match on(&call.scope).find(|(.., n)| n == MAPPING) {
                None => vec![String::new()],
                Some((c, ..)) => match code.call(path, *c) {
                    Some(class) if class.keyword("method").is_none() => {
                        match paths(code, path, class, MAPPING_PATHS) {
                            Some(prefixes) => prefixes,
                            None => continue,
                        }
                    }
                    _ => continue,
                },
            };
            let Some(routes) = paths(code, path, given, MAPPING_PATHS) else {
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
    jax_rs(code, &mut found);
    found
}

const JAX_RS: &[&str] = &["javax.ws.rs.", "jakarta.ws.rs."];

fn jax_rs_annotation(spelled: &str) -> bool {
    (JAX_RS.iter())
        .filter_map(|p| spelled.strip_prefix(p))
        .any(|n| n == "Path" || METHODS.contains(&n))
}

fn jax_rs<'a>(code: &Code<'a>, found: &mut Vec<Found>) {
    for (path, facts) in java_files(code) {
        let annotations: Vec<(usize, &'a Call, String)> = (facts.calls.iter().enumerate())
            .filter(|(_, c)| c.decorates.is_some())
            .filter_map(|(n, c)| {
                let name = known_annotation(code, path, c, jax_rs_annotation)?;
                Some((n, c, name))
            })
            .collect();
        let path_of = |target: &str| {
            (annotations.iter()).find(|(_, c, n)| {
                c.decorates.as_deref() == Some(target)
                    && (JAX_RS.iter()).any(|p| n.strip_prefix(p) == Some("Path"))
            })
        };
        for (_, call, name) in &annotations {
            let verb = (JAX_RS.iter())
                .filter_map(|p| name.strip_prefix(p))
                .find(|n| METHODS.contains(n));
            let (Some(verb), Some(target)) = (verb, call.decorates.as_deref()) else {
                continue;
            };
            if !is_method(facts, target) {
                continue;
            }
            let read = |entry: Option<&(usize, &'a Call, String)>| {
                let given = code.call(path, entry?.0)?;
                paths(code, path, given, MAPPING_PATHS)
            };
            if facts.interfaces.contains(&call.scope) {
                continue;
            }
            let Some(prefixes) = read(path_of(&call.scope)) else {
                continue;
            };
            let routes = match path_of(target) {
                None => Some(vec![String::new()]),
                some => read(some),
            };
            let Some(routes) = routes else { continue };
            for (prefix, route) in prefixes
                .iter()
                .flat_map(|p| routes.iter().map(move |r| (p, r)))
            {
                let full = join(prefix, route);
                if balanced(&full) {
                    let site = Site::Def {
                        path: path.into(),
                        name: target.into(),
                    };
                    found.push(provider(site, contract(verb, &full)));
                }
            }
        }
    }
}

fn annotation(spelled: &str) -> bool {
    CONTROLLERS.contains(&spelled) || mapping(spelled)
}

fn mapping(spelled: &str) -> bool {
    spelled == MAPPING || shortcut(spelled).is_some()
}

fn shortcut(spelled: &str) -> Option<&'static str> {
    let name = spelled.strip_prefix(WEB)?;
    (SHORTCUTS.iter())
        .find(|(s, _)| *s == name)
        .map(|(_, method)| *method)
}

fn member_of<'s>(spelled: &'s str, of: &str) -> Option<&'s str> {
    let member = spelled.strip_prefix(of)?;
    REQUEST_METHODS.contains(&member).then_some(member)
}

fn made<'a>(
    code: &Code<'a>,
    (path, scope): (&'a str, &'a str),
    client: &'a Value,
    ty: &str,
    makes: impl Fn(&Code<'a>, &'a str, usize) -> bool,
) -> bool {
    reach(code, (path, scope), client, ty, &makes, 0)
}

fn reach<'a>(
    code: &Code<'a>,
    (path, scope): (&'a str, &'a str),
    client: &'a Value,
    ty: &str,
    makes: &impl Fn(&Code<'a>, &'a str, usize) -> bool,
    depth: usize,
) -> bool {
    match client {
        Value::Ref(name) => {
            if one(&typed(code, path, scope, name), |t| t == ty).is_some() {
                return true;
            }
            let follow = Follow::Values {
                passes: passes_nothing,
            };
            let origins = code.origins(path, scope, Of::Name(name), follow);
            if (origins.iter().filter_map(|o| o.call)).any(|(at, i)| makes(code, at, i)) {
                return true;
            }
            match code.local(path, scope, name) {
                Some(Bound { within, value, .. }) if depth < LOCAL_DEPTH => {
                    reach(code, (path, within), value, ty, makes, depth + 1)
                }
                _ => false,
            }
        }
        Value::Call(inner) => code
            .call_index(path, &inner.range)
            .is_some_and(|i| makes(code, path, i)),
        _ => false,
    }
}

fn folded_local<'a>(
    code: &Code<'a>,
    (path, scope): (&'a str, &'a str),
    client: &'a Value,
    inject: Option<&Inject<'a, '_>>,
) -> Option<Value> {
    let (mut scope, mut client) = (scope, client);
    for _ in 0..LOCAL_DEPTH {
        let Value::Ref(name) = client else { break };
        let Bound { within, value, .. } = code.local(path, scope, name)?;
        (scope, client) = (within, value);
    }
    let Value::Call(made) = client else {
        return None;
    };
    let at = code.call_index(path, &made.range)?;
    let read = |values: &[Value], _: &[(String, Value)]| match values {
        [call @ Value::Call(_)] => Some(call.clone()),
        _ => None,
    };
    let mut folded = code.expand_with(Expand::Chain(path, at), inject, &read);
    match folded.len() {
        1 => folded.pop().map(|f| f.found),
        _ => None,
    }
}

fn paths<'a>(code: &Code<'a>, file: &'a str, call: &'a Call, keys: &[&str]) -> Option<Vec<String>> {
    let given: Vec<&Value> = (call.args.iter())
        .chain(keys.iter().filter_map(|k| call.keyword(k)))
        .collect();
    let fold = |v: &'a Value| code.value(file, &call.scope, v);
    let items: Vec<Value> = match given.as_slice() {
        [] => return Some(vec![String::new()]),
        [one] => match array_of(code, file, &call.scope, one) {
            Some(elements) => elements.iter().map(fold).collect(),
            None => match fold(one) {
                Value::List(items) => items,
                one => return path_literal(one).map(|p| vec![p]),
            },
        },
        several if call.args.len() == several.len() => several.iter().map(|v| fold(v)).collect(),
        _ => return None,
    };
    match items.is_empty() {
        true => Some(vec![String::new()]),
        false => Some(items.into_iter().filter_map(path_literal).collect()),
    }
}

fn request_methods<'a>(code: &Code<'a>, file: &'a str, call: &'a Call) -> Option<Vec<String>> {
    let member = |value: &'a Value| -> Option<String> {
        let Value::Ref(name) = value else {
            return None;
        };
        let member = |s: &str| member_of(s, REQUEST_METHOD).is_some();
        let spelled = known(code, file, &call.scope, name, member)?;
        member_of(&spelled, REQUEST_METHOD).map(str::to_string)
    };
    let listed: Option<&[Value]> = match call.keyword("method") {
        Some(Value::List(items)) => Some(items),
        Some(one) => array_of(code, file, &call.scope, one),
        None => None,
    };
    let methods: Vec<String> = match (call.keyword("method"), listed) {
        (None, _) => return Some(vec![ANY.into()]),
        (_, Some(items)) => items.iter().filter_map(member).collect(),
        (Some(one), None) => member(one).into_iter().collect(),
    };
    let empty = listed.is_some_and(<[Value]>::is_empty);
    match (methods.is_empty(), empty) {
        (true, true) => Some(vec![ANY.into()]),
        (true, false) => None,
        _ => Some(methods),
    }
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

    fn served(imports: &str, body: &str) -> Vec<String> {
        served_with(imports, body, &[])
    }

    fn served_with(imports: &str, body: &str, others: &[(&str, &str)]) -> Vec<String> {
        let src = format!("package a;\n{imports}\n{body}\n");
        let mut parsed = vec![("a/C.java", Java.parse("a/C.java", src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, Java.parse(path, source.as_bytes()).unwrap()));
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
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
        let nested = "@RestController class C { @RequestMapping(\"/in\") @RestController static class D { @GetMapping(\"/x\") void f() {} } }";
        assert_eq!(served(IMPORTS, nested), ["http::GET::/in/x"]);
    }

    #[test]
    fn an_annotation_written_in_full_gives_way_to_the_repository() {
        const SPRING: &str = "org.springframework.web.bind.annotation";
        let body = format!(
            "@{SPRING}.RestController class C {{ @{SPRING}.GetMapping(\"/x\") void f() {{}} }}"
        );
        assert_eq!(served("", &body), ["http::GET::/x"]);
        assert!(served("import com.acme.org;", &body).is_empty());
        let head = [("a/org.java", "package a; class org {}")];
        assert!(served_with("", &body, &head).is_empty());
        let own = |name: &str| {
            format!(
                "package {SPRING}; public @interface {name} {{ String[] value() default {{}}; }}"
            )
        };
        let (get, rest) = (own("GetMapping"), own("RestController"));
        let get = [("org/GetMapping.java", &*get)];
        assert!(served_with("", &body, &get).is_empty());
        let rest = [("org/RestController.java", &*rest)];
        assert!(served_with("", &body, &rest).is_empty());
    }

    #[test]
    fn a_controller_with_an_outside_base_serves_its_methods() {
        let imports = format!("{IMPORTS}import com.corp.shared.BaseController;\n");
        let based = "@RestController @RequestMapping(\"/orders\") class C extends BaseController { \
            @GetMapping(\"/{id}\") void get() {} }";
        assert_eq!(served(&imports, based), ["http::GET::/orders/{}"]);
        let own = "@RestController @RequestMapping(\"/orders\") class C { @GetMapping(\"/{id}\") void get() {} }";
        assert_eq!(served(&imports, own), ["http::GET::/orders/{}"]);
    }

    #[test]
    fn what_cannot_be_read_with_certainty_serves_nothing() {
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

    const WILD: &str = "import org.springframework.web.bind.annotation.*;\n";
    const ROUTE: &str = "@RestController @RequestMapping(\"/p\") class C { \
        @GetMapping(\"/x\") void f() {} \
        @RequestMapping(value = \"/y\", method = RequestMethod.POST) void g() {} }";

    #[test]
    fn a_wildcard_import_from_spring_binds_its_annotations_and_enum() {
        assert_eq!(served(WILD, ROUTE), ["http::GET::/p/x", "http::POST::/p/y"]);
        let more = format!("import java.util.*;\nimport org.acme.*;\n{WILD}");
        assert_eq!(
            served(&more, ROUTE),
            ["http::GET::/p/x", "http::POST::/p/y"]
        );
    }

    #[test]
    fn a_wildcard_import_binds_nothing_another_binding_or_candidate_may_take() {
        const POST: [&str; 1] = ["http::POST::/p/y"];
        let same = [("a/GetMapping.java", "package a; class GetMapping {}")];
        assert_eq!(served_with(WILD, ROUTE, &same), POST);
        let annotation = [(
            "a/GetMapping.java",
            "package a; @interface GetMapping { String[] value(); }",
        )];
        assert_eq!(served_with(WILD, ROUTE, &annotation), POST);
        let single = format!("{WILD}import org.acme.GetMapping;\n");
        assert_eq!(served(&single, ROUTE), POST);
        let other = [("b/GetMapping.java", "package b; public class GetMapping {}")];
        let repository = format!("{WILD}import b.*;\n");
        assert_eq!(served_with(&repository, ROUTE, &other), POST);
        let unrelated = [("b/Other.java", "package b; public class Other {}")];
        assert_eq!(served_with(&repository, ROUTE, &unrelated).len(), 2);
        let ambiguous = format!("{WILD}import static org.acme.Util.*;\nimport b.*;\n");
        assert_eq!(served_with(&ambiguous, ROUTE, &other), POST);
    }

    #[test]
    fn a_static_wildcard_import_of_the_enum_binds_its_members() {
        let imports = format!(
            "{IMPORTS}import static org.springframework.web.bind.annotation.RequestMethod.*;\n"
        );
        let found = controller(
            "",
            "@RequestMapping(value = \"/x\", method = GET) void f() {}",
        );
        assert!(found.is_empty(), "without the import nothing binds GET");
        let found = served(
            &imports,
            "@RestController class C { @RequestMapping(value = \"/x\", method = {GET, POST}) void f() {} }",
        );
        assert_eq!(found, ["http::GET::/x", "http::POST::/x"]);
        let other = format!("{IMPORTS}import static org.acme.Methods.*;\n");
        let body =
            "@RestController class C { @RequestMapping(value = \"/x\", method = GET) void f() {} }";
        assert!(served(&other, body).is_empty());
    }

    #[test]
    fn a_list_with_an_unreadable_element_serves_the_readable_ones() {
        let found = controller(
            "",
            "@RequestMapping(value = {\"/a\", Api.NOT_A_CONSTANT}, method = {RequestMethod.GET, Missing.X}) void f() {}",
        );
        assert_eq!(found, ["http::GET::/a"]);
    }

    const KOTLIN_IMPORTS: &str = "import org.springframework.web.bind.annotation.GetMapping\n\
        import org.springframework.web.bind.annotation.RequestMapping\n\
        import org.springframework.web.bind.annotation.RequestMethod\n\
        import org.springframework.web.bind.annotation.RestController\n";

    fn served_kotlin(imports: &str, body: &str, others: &[(&str, &str)]) -> Vec<String> {
        use crate::lang::kotlin::Kotlin;
        let src = format!("package a\n{imports}\n{body}\n");
        let mut parsed = vec![("a/C.kt", Kotlin.parse("a/C.kt", src.as_bytes()).unwrap())];
        for (path, source) in others {
            let facts = match path.ends_with(".kt") {
                true => Kotlin.parse(path, source.as_bytes()),
                false => Java.parse(path, source.as_bytes()),
            };
            parsed.push((path, facts.unwrap()));
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let code = Code::new(&resolver, &files, &Java);
        let mut found: Vec<String> = (providers(&code).into_iter())
            .filter_map(|f| f.contract)
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_kotlin_controller_reads_arrays_methods_and_every_method() {
        let body = "@RestController @RequestMapping([\"/a\", \"/b\"]) class C {\n\
            @GetMapping(\"/x/{id}\") fun f() {}\n\
            @RequestMapping(path = [\"/y\"], method = [RequestMethod.GET, RequestMethod.HEAD]) fun g() {}\n\
            @RequestMapping(\"/z\") fun h() {}\n}";
        assert_eq!(
            served_kotlin(KOTLIN_IMPORTS, body, &[]),
            [
                "http::ANY::/a/z",
                "http::ANY::/b/z",
                "http::GET::/a/x/{}",
                "http::GET::/a/y",
                "http::GET::/b/x/{}",
                "http::GET::/b/y",
                "http::HEAD::/a/y",
                "http::HEAD::/b/y",
            ]
        );
    }

    #[test]
    fn kotlin_paths_fold_constants_of_the_class_its_companion_and_other_files() {
        let others = [
            (
                "a/Api.kt",
                "package a\nconst val BASE = \"/api\"\nobject Api { const val ITEM = \"/{id}\" }",
            ),
            (
                "a/Paths.java",
                "package a; class Paths { static final String JAVA = \"/java\"; }",
            ),
        ];
        let body = "@RestController @RequestMapping(BASE + \"/v1\") class C {\n\
            companion object {\n const val FLAG = \"/flag\"\n }\n\
            @GetMapping(Api.ITEM + FLAG) fun f() {}\n\
            @GetMapping(path = [Paths.JAVA, \"/\" + \"lit\"]) fun g() {}\n}";
        assert_eq!(
            served_kotlin(KOTLIN_IMPORTS, body, &others),
            [
                "http::GET::/api/v1/java",
                "http::GET::/api/v1/lit",
                "http::GET::/api/v1/{}/flag",
            ]
        );
    }

    #[test]
    fn a_kotlin_star_import_from_spring_binds_its_annotations_and_enum() {
        let imports = "import org.springframework.web.bind.annotation.*\n";
        let body = "@RestController class C {\n\
            @GetMapping(\"/x\") fun f() {}\n\
            @RequestMapping(\"/y\", method = [RequestMethod.PUT]) fun g() {}\n}";
        assert_eq!(
            served_kotlin(imports, body, &[]),
            ["http::GET::/x", "http::PUT::/y"]
        );
        let own = [("a/GetMapping.kt", "package a\nannotation class GetMapping")];
        assert_eq!(served_kotlin(imports, body, &own), ["http::PUT::/y"]);
    }

    #[test]
    fn kotlin_several_paths_and_array_of_fill_the_arrays() {
        let body = "@RestController @RequestMapping(\"/p\", \"/q\") class C {\n\
            @GetMapping(\"/a\", \"/b\") fun f() {}\n\
            @PostMapping(value = arrayOf(\"/c\")) fun g() {}\n\
            @RequestMapping(\"/d\", method = arrayOf(RequestMethod.PUT)) fun h() {}\n\
            @RequestMapping(\"/e\", method = arrayOf()) fun i() {}\n}";
        let imports =
            format!("{KOTLIN_IMPORTS}import org.springframework.web.bind.annotation.PostMapping\n");
        let found = served_kotlin(&imports, body, &[]);
        let wanted = |m: &str, route: &str| ["/p", "/q"].map(|p| format!("http::{m}::{p}{route}"));
        let mut expected: Vec<String> = [
            ("GET", "/a"),
            ("GET", "/b"),
            ("POST", "/c"),
            ("PUT", "/d"),
            ("ANY", "/e"),
        ]
        .iter()
        .flat_map(|(m, r)| wanted(m, r))
        .collect();
        expected.sort();
        assert_eq!(found, expected);
        let mixed = "@RestController class C { @GetMapping(\"/a\", path = [\"/b\"]) fun f() {} }";
        assert!(served_kotlin(KOTLIN_IMPORTS, mixed, &[]).is_empty());
        let own = [(
            "a/Own.kt",
            "package a\nfun arrayOf(vararg s: String): Array<String> = emptyArray()",
        )];
        let hidden = "@RestController class C { @GetMapping(arrayOf(\"/a\")) fun f() {} }";
        assert!(served_kotlin(KOTLIN_IMPORTS, hidden, &own).is_empty());
        let plain = "@RestController class C { @GetMapping(arrayOf(\"/a\")) fun f() {} }";
        assert_eq!(served_kotlin(KOTLIN_IMPORTS, plain, &[]), ["http::GET::/a"]);
    }

    #[test]
    fn a_kotlin_name_of_the_repository_hides_springs() {
        let own = [(
            "a/GetMapping.kt",
            "package a\nannotation class GetMapping(val value: String)",
        )];
        let imports = KOTLIN_IMPORTS.replace(
            "import org.springframework.web.bind.annotation.GetMapping\n",
            "",
        );
        let body = "@RestController class C { @GetMapping(\"/x\") fun f() {} }";
        assert!(served_kotlin(&imports, body, &own).is_empty());
    }

    const JAXRS: &str = "import javax.ws.rs.GET;\nimport javax.ws.rs.Path;\n";

    fn resource(imports: &str, body: &str) -> Vec<String> {
        served(imports, body)
    }

    #[test]
    fn a_jax_rs_resource_joins_the_class_path_and_the_method_path() {
        let body = "@Path(\"/items\") class C { @GET @Path(\"{id:[0-9]{3}}\") void f() {} @GET void g() {} }";
        assert_eq!(
            resource(JAXRS, body),
            ["http::GET::/items", "http::GET::/items/{}"]
        );
        let jakarta = JAXRS.replace("javax", "jakarta");
        assert_eq!(resource(&jakarta, body).len(), 2);
    }

    #[test]
    fn each_jax_rs_method_annotation_gives_its_method() {
        let verbs = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
        let imports: String = (verbs.iter())
            .map(|v| format!("import jakarta.ws.rs.{v};\n"))
            .collect::<String>()
            + "import jakarta.ws.rs.Path;\n";
        let methods: String = (verbs.iter())
            .map(|v| format!("@{v} @Path(\"/{v}\") void m{v}() {{}} "))
            .collect();
        let found = resource(&imports, &format!("@Path(\"/r\") class C {{ {methods} }}"));
        let mut want: Vec<String> = (verbs.iter())
            .map(|v| format!("http::{v}::/r/{v}"))
            .collect();
        want.sort();
        assert_eq!(found, want);
    }

    #[test]
    fn jax_rs_paths_fold_constants() {
        let body = "@Path(Api.BASE + \"/x\") class C { @GET @Path(Api.ID) void f() {} }";
        let api = [(
            "a/Api.java",
            "package a; class Api { static final String BASE = \"/b\"; static final String ID = \"{id}\"; }",
        )];
        assert_eq!(served_with(JAXRS, body, &api), ["http::GET::/b/x/{}"]);
    }

    #[test]
    fn jax_rs_names_count_only_through_an_import_and_a_resource_class() {
        let body = "@Path(\"/x\") class C { @GET void f() {} }";
        let other = JAXRS.replace("javax.ws.rs.GET", "org.acme.GET");
        assert!(resource(&other, body).is_empty());
        let shadow = [("a/GET.java", "package a; class GET {}")];
        assert_eq!(served_with(JAXRS, body, &shadow), ["http::GET::/x"]);
        let wild = "import javax.ws.rs.*;\n";
        assert!(served_with(wild, body, &shadow).is_empty());
        let nested =
            "import javax.ws.rs.*;\n@Path(\"/x\") class C { class GET {} @GET void f() {} }";
        assert!(resource("", nested).is_empty());
        let single = format!("{JAXRS}import org.acme.GET;\n");
        assert!(resource(&single, body).is_empty());
        assert!(resource(JAXRS, "class C { @GET void f() {} }").is_empty());
        assert!(resource("", body).is_empty());
        assert!(resource(JAXRS, "@Path(\"/x\") class C { @Path(\"/y\") void f() {} }").is_empty());
    }

    #[test]
    fn a_jax_rs_interface_is_a_client_and_serves_nothing() {
        let client = "@Path(\"/users\") interface I { @GET void f(); }";
        assert!(resource(JAXRS, client).is_empty());
        let nested = "@Path(\"/x\") class C { @GET void g() {} @Path(\"/in\") interface I { @GET void f(); } }";
        assert_eq!(resource(JAXRS, nested), ["http::GET::/x"]);
    }

    #[test]
    fn a_jax_rs_wildcard_binds_through_one_package_only() {
        let body = "@Path(\"/x\") class C { @GET void f() {} }";
        assert_eq!(resource("import javax.ws.rs.*;\n", body), ["http::GET::/x"]);
        assert_eq!(
            resource("import jakarta.ws.rs.*;\n", body),
            ["http::GET::/x"]
        );
        let both = "import javax.ws.rs.*;\nimport jakarta.ws.rs.*;\n";
        assert!(resource(both, body).is_empty());
    }

    #[test]
    fn application_path_is_not_applied() {
        let imports = format!(
            "{JAXRS}import javax.ws.rs.ApplicationPath;\nimport javax.ws.rs.core.Application;\n"
        );
        let body = "@ApplicationPath(\"/api\") class App extends Application {} \
            @Path(\"/x\") class C { @GET void f() {} }";
        assert_eq!(resource(&imports, body), ["http::GET::/x"]);
        let nested =
            "class App { @Path(\"/app\") static class P { @GET @Path(\"/y\") void f() {} } }";
        assert_eq!(resource(&imports, nested), ["http::GET::/app/y"]);
    }
}
